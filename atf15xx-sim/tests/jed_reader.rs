//! The JEDEC reader (`parse_jed_bytes`, on the `jedec` crate since 2026-11-27) against the reader it
//! replaced, kept here as the reference: every committed JED (the fitter's conformance vectors
//! and ATMISP's test vectors, our own layout) must read to the same fuses, except the vectors
//! that are broken on purpose, which the new reader must reject for the stated reason.
use std::path::{Path, PathBuf};

/// The reader before the `jedec` crate (lenient: no STX/ETX, checksums or file checksum needed).
fn reference_parse(text: &str) -> Option<Vec<bool>> {
    let body = text.split('\x02').nth(1).unwrap_or(text);
    let body = body.split('\x03').next().unwrap_or(body);
    let mut qf = None;
    let mut default = false;
    let mut ls: Vec<(usize, Vec<bool>)> = vec![];
    for f in body.split('*').skip(1).map(str::trim) {
        if let Some(n) = f.strip_prefix("QF") {
            qf = Some(n.trim().parse::<usize>().ok()?);
        } else if f == "F0" || f == "F1" {
            default = f == "F1";
        } else if let Some(rest) = f.strip_prefix('L') {
            let (a, bits) = rest.split_once(|c: char| c.is_whitespace()).unwrap_or((rest, ""));
            ls.push((a.parse().ok()?, bits.chars().filter(|c| *c == '0' || *c == '1').map(|c| c == '1').collect()));
        }
    }
    let qf = qf?;
    atf15xx_arch::Family::ALL.into_iter().find(|f| f.fuse_count() == qf)?;
    let mut bits = vec![default; qf];
    for (a, v) in ls {
        if a + v.len() > qf {
            return None;
        }
        bits[a..a + v.len()].copy_from_slice(&v);
    }
    Some(bits)
}

fn jeds(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            jeds(&p, out);
        } else if p.extension().map_or(false, |x| x == "jed") {
            out.push(p);
        }
    }
}

#[test]
fn reads_every_committed_jed_like_the_reference() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = vec![];
    jeds(&root.join("atf15xx-fit/tests/vectors"), &mut files);
    jeds(&root.join("atf15xx-tool/tests/vectors"), &mut files);
    files.sort();
    assert!(files.len() > 120, "{} JEDs", files.len());
    // broken on purpose (ATMISP input tests): what the new reader must say about them
    // (the reference was lenient about checksums and field syntax, ATMISP is too: its quirks are
    // atf15xx-tool's own reader's, spec §3.3; a file headed for a chip with a wrong `C` is refused)
    let expected_rejects: &[(&str, &str)] = &[
        ("jedec-input/b2_jed_badcsum", "invalid fuse checksum"),
        ("jedec-input/b2_jed_partial_f1", "invalid fuse checksum"), // C0000 for a non-zero sum
        ("jedec-input/jed_long_lfield", "invalid fuse checksum"),   // C0000 for a non-zero sum
        ("jedec-input/jed_cr_in_addr", "invalid character"),        // CR inside an L address
        ("jedec-input/jed_digit2_and_X", "invalid character"),      // 'X' fuse digits
        ("jedec-input/jed_digits_and_X", "invalid character"),
        ("jedec-input/b2_jed_noqf", "missing QF field"),
        ("jedec-input/jed_lowercase_note", "missing QF field"),
        // ATF15xxBE/RE: other fuse counts (not this project's parts)
        ("BE/a.jed", "is not an ATF1502AS/1504AS/1508AS fuse count"),
        ("RE/a.jed", "is not an ATF1502AS/1504AS/1508AS fuse count"),
    ];
    let (mut same, mut bad) = (0, vec![]);
    for p in &files {
        let raw = std::fs::read(p).unwrap();
        let text: String = raw.iter().map(|&b| b as char).collect();
        let rel = p.strip_prefix(&root).unwrap().to_string_lossy().to_string();
        let new = atf15xx_sim::parse_jed_bytes(&raw);
        let want_reject = expected_rejects.iter().find(|(d, _)| rel.contains(d));
        match (reference_parse(&text), &new, want_reject) {
            (_, Err(e), Some((_, why))) if format!("{e:#}").contains(why) => same += 1,
            (Some(old), Ok(f), None) if old == f.bits => same += 1,
            (old, new, want) => bad.push(format!(
                "{rel}: reference {}, new {}, expected {:?}",
                old.map_or("rejects".into(), |b| format!("{} fuses", b.len())),
                match new {
                    Ok(f) => format!("{} fuses", f.bits.len()),
                    Err(e) => format!("error {e:#}"),
                },
                want.map(|w| w.1)
            )),
        }
    }
    assert!(bad.is_empty(), "{same} as expected, {} not:\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn checks_what_the_reference_did_not() {
    // a wrong fuse checksum, and a wrong file checksum after ETX
    let good = "\x02*QF34192*F0*L0 11*C0003*\x030000";
    assert!(atf15xx_sim::parse_jed(good).is_ok());
    assert!(atf15xx_sim::parse_jed(&good.replace("C0003", "C0004")).is_err());
    assert!(atf15xx_sim::parse_jed(&good.replace("\x030000", "\x031234")).is_err());
    // only a default (no `L` field): every fuse at it (a jedec 0.1.0 bug worked around)
    let blank = atf15xx_sim::parse_jed("\x02*QF16808*F1*G0*\x030000").unwrap();
    assert!(blank.bits.len() == 16808 && blank.bits.iter().all(|&b| b));
    // no STX / ETX / file checksum: not a JEDEC file
    assert!(atf15xx_sim::parse_jed("*QF34192*F0*").is_err());
    assert!(atf15xx_sim::parse_jed("\x02*QF34192*F0*\x03").is_err());
    // the security fuse
    assert!(atf15xx_sim::parse_jed("\x02*QF34192*F0*G1*\x030000").unwrap().security);
}
