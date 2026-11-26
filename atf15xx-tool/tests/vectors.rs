//! Conformance test (§11.3): every vector in tests/vectors must reproduce ATMISP 7.3's output: the
//! SVF byte for byte, the Load JEDEC files (§8) fuse for fuse.

use flate2::read::GzDecoder;
use atf15xx_tool::isp::chain;
use atf15xx_tool::isp::diag::{Diagnostics, Error};
use atf15xx_tool::isp::generate::{generate, GenOptions};
use atf15xx_tool::isp::jedec;
use atf15xx_tool::isp::svf::{Rev, SvfOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

fn gunzip(p: &Path) -> Vec<u8> {
    let mut v = Vec::new();
    GzDecoder::new(std::fs::File::open(p).unwrap())
        .read_to_end(&mut v)
        .unwrap();
    v
}

/// Remove the line containing `marker` (the timestamp line) and return the rest.
fn without_line(data: &[u8], marker: &str) -> (Vec<u8>, Option<String>) {
    let text = String::from_utf8_lossy(data).into_owned();
    let mut found = None;
    let mut out = String::new();
    for l in text.split_inclusive('\n') {
        if found.is_none() && l.contains(marker) {
            found = Some(l.split_once(marker).unwrap().1.trim_end().to_string());
            continue;
        }
        out.push_str(l);
    }
    (out.into_bytes(), found)
}

/// A Load JEDEC output (§8) matches its expected file when both have the same fuse array and
/// security fuse and ours states the correct fuse checksum (the layout is ours, §8.2).
fn load_output_matches(got: &[u8], want: &[u8]) -> Result<(), String> {
    let g = jedec::parse(got).map_err(|e| format!("unreadable: {e}"))?;
    let w = jedec::parse(want).map_err(|e| format!("expected file unreadable: {e}"))?;
    if g.qf != w.qf || g.fuses != w.fuses {
        return Err("fuses differ".into());
    }
    if g.security != w.security {
        return Err("security fuse differs".into());
    }
    if g.stated_checksum != Some(g.checksum()) || w.stated_checksum != Some(w.checksum()) {
        return Err(format!("checksum: stated {:X?}, computed {:X}", g.stated_checksum, g.checksum()));
    }
    Ok(())
}

fn vector_dirs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors");
    let mut v = Vec::new();
    for g in std::fs::read_dir(&root).unwrap() {
        let g = g.unwrap().path();
        if g.is_dir() {
            for c in std::fs::read_dir(&g).unwrap() {
                v.push(c.unwrap().path());
            }
        }
    }
    v.sort();
    v
}

fn local(dir: &Path, win_path: &str) -> PathBuf {
    dir.join(win_path.rsplit('\\').next().unwrap_or(win_path))
}

fn run_case(dir: &Path) -> Result<(), String> {
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("case.json")).unwrap()).unwrap();
    let expect_ok = meta["expect"] == "ok";
    let mut tck = meta.get("tck").and_then(|v| v.as_i64()).unwrap_or(1);
    let rev = if meta.get("rev").and_then(|v| v.as_str()) == Some("C") {
        Rev::C
    } else {
        Rev::D
    };
    let state_reset = meta
        .get("state_reset")
        .and_then(|v| v.as_i64())
        .unwrap_or(1)
        != 0;
    if !(1..=1000).contains(&tck) {
        if meta.get("compat_tck").and_then(|v| v.as_bool()) == Some(true) {
            tck = 1;
        } else {
            return if expect_ok {
                Err("tck out of range".into())
            } else {
                Ok(())
            };
        }
    }
    let chn = std::fs::read(dir.join("chain.chn")).unwrap();
    let mut diag = Diagnostics::default();
    let parsed = chain::parse(&chn, &mut diag, |_, p| local(dir, p).is_file());
    let expected = dir.join("expected.svf.gz");
    let want = if expected.exists() {
        Some(gunzip(&expected))
    } else {
        None
    };
    let (want_body, ctime) = match &want {
        Some(w) => {
            let (b, t) = without_line(w, "//  Created on: ");
            (b, t.unwrap_or_default())
        }
        None => (Vec::new(), String::new()),
    };
    let result = parsed.and_then(|c| {
        let opts = GenOptions {
            svf: SvfOptions {
                rev,
                tck: tck as u32,
                state_reset,
                strict: false,
            },
            allow_mixed_voltage: true, // ATMISP's dialog was answered "Yes" by the oracle driver
            ctime: ctime.clone(),
            load_output: true,
        };
        generate(
            &c,
            |x, p| std::fs::read(local(dir, p)).map_err(|_| Error::Input(chain::e3(x))),
            &opts,
            diag,
        )
    });
    match (expect_ok, result) {
        (false, Err(_)) => Ok(()),
        (false, Ok(_)) => Err("expected rejection, but generation succeeded".into()),
        (true, Err(e)) => Err(format!("unexpected error: {e}")),
        (true, Ok(out)) => {
            let (got_body, _) = without_line(&out.svf, "//  Created on: ");
            if got_body != want_body {
                let g = String::from_utf8_lossy(&got_body);
                let w = String::from_utf8_lossy(&want_body);
                let line = g
                    .lines()
                    .zip(w.lines())
                    .position(|(a, b)| a != b)
                    .unwrap_or(g.lines().count().min(w.lines().count()));
                return Err(format!(
                    "SVF differs at body line {} (got {:?}, want {:?})",
                    line + 1,
                    g.lines().nth(line),
                    w.lines().nth(line)
                ));
            }
            for (_, p, bytes) in &out.jedec_outputs {
                let name = p.rsplit('\\').next().unwrap();
                let exp = dir.join(format!("expected-{name}.gz"));
                load_output_matches(bytes, &gunzip(&exp)).map_err(|e| format!("Load JEDEC output {name}: {e}"))?;
            }
            Ok(())
        }
    }
}

#[test]
fn all_vectors_match_atmisp() {
    let dirs = vector_dirs();
    assert!(dirs.len() > 100, "test vectors missing");
    let mut failures = Vec::new();
    for d in &dirs {
        if let Err(e) = run_case(d) {
            failures.push(format!(
                "{}: {e}",
                d.strip_prefix(env!("CARGO_MANIFEST_DIR"))
                    .unwrap()
                    .display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} vectors failed:\n{}",
        failures.len(),
        dirs.len(),
        failures.join("\n")
    );
}
