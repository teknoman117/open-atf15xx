//! Self-contained checks of the model, simulator and emitter on the fitter test vectors
//! (atf15xx-fit/tests/vectors). The cross-checks against iverilog and the
//! formal flow are in tools/sim/ (see docs/architecture/sim/).

use atf15xx_sim::model::{Model, Options};
use atf15xx_sim::sim::Sim;
use atf15xx_sim::verilog::{emit, EmitOptions};
use rand::{Rng, SeedableRng};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn vectors() -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../atf15xx-fit/tests/vectors");
    let mut v = vec![];
    for fam in ["1502", "1504", "1508"] {
        let Ok(rd) = std::fs::read_dir(root.join(fam)) else { continue };
        let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for p in files {
            if p.extension().is_some_and(|e| e == "jed") {
                let args = std::fs::read_to_string(p.with_extension("args")).unwrap_or_default();
                let mut it = args.split_whitespace();
                let dev = it.by_ref().skip_while(|a| *a != "-device").nth(1).unwrap_or("").to_string();
                if !dev.is_empty() {
                    v.push((p, dev));
                }
            }
        }
    }
    v
}

/// Run `steps` random steps (GCLK1 toggling) and return the pin drives after every step.
fn trace(m: &Model, steps: usize, seed: u64) -> Vec<Vec<(u16, bool, bool)>> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut s = Sim::new(m);
    let pins: Vec<u16> = m.pins.iter().map(|p| p.0).collect();
    let mut out = vec![];
    for i in 0..steps {
        let mut e = BTreeMap::new();
        for &p in &pins {
            e.insert(p, rng.random_bool(0.5));
        }
        e.insert(m.device.gclk1, i % 2 == 1);
        e.insert(m.device.gclr, false);
        s.step(&e);
        out.push(s.outputs().iter().map(|(&p, o)| (p, o.oe, o.oe && o.value)).collect());
    }
    out
}

#[test]
fn every_vector_loads_simulates_and_emits() {
    let vs = vectors();
    assert!(vs.len() >= 90, "expected the fitter test vectors, found {}", vs.len());
    for (jed, dev) in &vs {
        let m = atf15xx_sim::load(jed, dev, Options::default()).unwrap_or_else(|e| panic!("{}: {e:#}", jed.display()));
        let t = trace(&m, 50, 1);
        assert_eq!(t.len(), 50);
        let v = emit(&m, &EmitOptions { module: "dut".into(), pins_wrapper: true });
        assert!(v.contains("module dut (") && v.contains("module dut_pins ("), "{}", jed.display());
        assert_eq!(v.matches("endmodule").count(), 2);
    }
}

#[test]
fn simulation_is_deterministic() {
    let (jed, dev) = &vectors()[0];
    let m = atf15xx_sim::load(jed, dev, Options::default()).unwrap();
    assert_eq!(trace(&m, 200, 7), trace(&m, 200, 7));
}

#[test]
fn flipping_used_fuses_changes_behaviour() {
    // Mutation check: for each of a few vectors, flip fuses until the equations change and make
    // sure some such flip also changes the simulated pin behaviour.
    let mut checked = 0;
    for (jed, dev) in vectors().iter().step_by(10) {
        let fz = atf15xx_sim::read_jed(jed).unwrap();
        let d = atf15xx_sim::device(dev).unwrap();
        let base = Model::new(d, &fz.bits, Options::default());
        let base_eq = atf15xx_sim::verilog::equations(&base);
        let base_t = trace(&base, 300, 3);
        let mut rng = rand::rngs::StdRng::seed_from_u64(11);
        let mut caught = false;
        for _ in 0..5000 {
            let mut bits = fz.bits.clone();
            let i = rng.random_range(0..bits.len());
            bits[i] = !bits[i];
            let m = Model::new(d, &bits, Options::default());
            if atf15xx_sim::verilog::equations(&m) == base_eq {
                continue;
            }
            if trace(&m, 300, 3) != base_t {
                caught = true;
                break;
            }
        }
        assert!(caught, "{}: no visible mutation found", jed.display());
        checked += 1;
    }
    assert!(checked >= 5);
}

#[test]
fn parse_jed_rejects_bad_input() {
    assert!(atf15xx_sim::parse_jed("\x02*QF123*F0*\x03").is_err());
    assert!(atf15xx_sim::parse_jed("\x02*F0*\x03").is_err());
    let ok = atf15xx_sim::parse_jed("\x02*QF34192*F0*L0 11*\x030000").unwrap();
    assert_eq!(ok.bits.len(), 34192);
    assert!(ok.bits[0] && ok.bits[1] && !ok.bits[2]);
}

#[test]
fn power_up_from_fuse_follows_invert() {
    let mut seen_one = false;
    for (jed, dev) in vectors() {
        let opts = Options { power_up_from_fuse: true, ..Options::default() };
        let m = atf15xx_sim::load(&jed, &dev, opts).unwrap();
        for (mc, c) in m.mcs.iter().zip(&m.decoded.macrocells) {
            assert_eq!(mc.init, c.invert_fuse);
            seen_one |= mc.init;
        }
        let s = Sim::new(&m);
        for (i, mc) in m.mcs.iter().enumerate() {
            if !mc.active {
                assert_eq!(s.q[i], mc.init, "{}: idle MC{} must hold its power-up value", jed.display(), mc.mc);
            }
        }
    }
    assert!(seen_one, "no vector has an Invert fuse set");
}
