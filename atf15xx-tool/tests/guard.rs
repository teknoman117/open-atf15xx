//! The JTAG guard (`atf15xx_tool::guard`) on committed vectors: the fitter's conformance JEDs
//! (vendor fuses; three fitted with `-strategy JTAG off`) and ATMISP's program/verify SVFs, whose
//! special-register writes must say what the JED they were made from says.
use atf15xx_tool::guard::{jed_keeps_jtag, svf_keeps_jtag, svf_special_writes};
use std::io::Read;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The JTAG-enable fuse read independently: the `jedec` crate, and the fuse address of
/// `atf15xx-arch` (generated from prjbureau's database: config bit `Jtag`).
fn reference_keeps_jtag(raw: &[u8]) -> Option<bool> {
    let j = jedec::JEDECFile::from_bytes(raw, &jedec::Quirks::new()).ok()?;
    let fam = atf15xx_arch::Family::ALL.into_iter().find(|f| f.fuse_count() == j.f.len())?;
    Some(j.f[fam.config_addr(atf15xx_arch::ConfigBit::Jtag)])
}

#[test]
fn jed_fuse_matches_an_independent_reading() {
    let (mut on, mut off) = (0, 0);
    let mut files = vec![];
    for fam in ["1502", "1504", "1508"] {
        for e in std::fs::read_dir(root().join("atf15xx-fit/tests/vectors").join(fam)).unwrap() {
            files.push(e.unwrap().path());
        }
    }
    for group in ["program-verify", "svf-options", "jedec-input"] {
        for e in std::fs::read_dir(root().join("atf15xx-tool/tests/vectors").join(group)).unwrap() {
            files.push(e.unwrap().path().join("a.jed"));
        }
    }
    for p in files.iter().filter(|p| p.extension().map_or(false, |x| x == "jed") && p.exists()) {
        let raw = std::fs::read(p).unwrap();
        // AS parts the independent reader accepts (BE/RE parts, broken inputs: not compared)
        let Some(want) = reference_keeps_jtag(&raw) else { continue };
        assert_eq!(jed_keeps_jtag(&raw), Ok(want), "{}", p.display());
        if want {
            on += 1
        } else {
            off += 1
        }
    }
    assert!(on > 90 && off >= 1, "{on} keep JTAG, {off} disable it");
}

#[test]
fn atmisp_svfs_agree_with_their_jeds() {
    let (mut on, mut off) = (0, 0);
    for group in ["program-verify", "svf-options", "jedec-input"] {
        for e in std::fs::read_dir(root().join("atf15xx-tool/tests/vectors").join(group)).unwrap() {
            let d = e.unwrap().path();
            let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join("case.json")).unwrap()).unwrap();
            let dev = &meta["devices"][0];
            // program/verify of an AS part from a JED that ATMISP accepted
            if meta["expect"] != "ok"
                || dev["instr"] != 1
                || !dev["name"].as_str().unwrap_or("").ends_with("AS")
                || !d.join("a.jed").exists()
            {
                continue;
            }
            let Ok(jed) = jed_keeps_jtag(&std::fs::read(d.join("a.jed")).unwrap()) else { continue };
            let mut svf = String::new();
            flate2::read::GzDecoder::new(std::fs::File::open(d.join("expected.svf.gz")).unwrap()).read_to_string(&mut svf).unwrap();
            assert!(!svf_special_writes(&svf).unwrap().is_empty(), "{}", d.display());
            assert_eq!(svf_keeps_jtag(&svf), Ok(jed), "{}", d.display());
            if jed {
                on += 1
            } else {
                off += 1
            }
        }
    }
    assert!(on >= 5 && off >= 1, "{on} keep JTAG, {off} disable it");
}

#[test]
fn cannot_tell_is_a_refusal() {
    assert!(svf_keeps_jtag("SIR 10 TDI (059);\n").is_err()); // no special-register write
    assert!(svf_keeps_jtag("not an svf").is_err());
    assert_eq!(svf_keeps_jtag("SIR 10 TDI (292);\nSDR 4 TDI (f);\n"), Ok(true));
    assert_eq!(svf_keeps_jtag("SIR 10 TDI (292);\nSDR 4 TDI (b);\n"), Ok(false));
    // bit 2 clear
}
