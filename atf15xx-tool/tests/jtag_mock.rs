//! The boundary-scan operations and the debugger's views through a simulated TAP.

use atf15xx_tool::dbg::design::{self, DesignMap};
use atf15xx_tool::dbg::part::{CellKind, Part};
use atf15xx_tool::dbg::{snapshot, Jtag, Scanner};

/// A TAP with one ATF15xx: IDCODE, BYPASS, SAMPLE (captures `state`) and EXTEST (drives `pins`).
struct MockTap {
    ir: u64,
    idcode: u32,
    state: Vec<bool>,
    preload: Vec<bool>,
    driven: Option<Vec<bool>>,
}

impl Jtag for MockTap {
    fn ir(&mut self, opcode: u64) -> anyhow::Result<()> {
        self.ir = opcode;
        Ok(())
    }
    fn dr(&mut self, bits: &[bool]) -> anyhow::Result<Vec<bool>> {
        // a shift register of the selected length: what comes out first is the captured value,
        // then the bits shifted in
        let captured: Vec<bool> = match self.ir {
            0x059 => (0..32).map(|i| self.idcode >> i & 1 == 1).collect(),
            0x3ff => vec![false],
            0x055 | 0x000 => self.state.clone(),
            other => panic!("unexpected IR 0x{other:x}"),
        };
        let n = captured.len();
        let mut chain = captured.clone();
        chain.extend_from_slice(bits);
        let out = chain[..bits.len()].to_vec();
        let last = &chain[bits.len()..bits.len() + n];
        match self.ir {
            0x055 => self.preload = last.to_vec(),
            0x000 => self.driven = Some(last.to_vec()),
            _ => {}
        }
        Ok(out)
    }
}

fn part() -> Option<Part> {
    Some(Part::load("ATF1508AS-PLCC84").unwrap())
}

#[test]
fn idcode_length_and_sample() {
    let Some(part) = part() else { return };
    let mut state = vec![false; part.length];
    // MC16 (pin 4 on the PLCC84) drives 1, MC3 (buried in our map below) holds Q = 1
    for (mc, k) in [(16, CellKind::Oe), (16, CellKind::Out), (3, CellKind::Out)] {
        state[part.mc_cell(mc, k).unwrap().bit] = true;
    }
    let mut tap = MockTap { ir: 0, idcode: 0x0150_803f, state: state.clone(), preload: vec![], driven: None };
    let mut s = Scanner::new(&mut tap, &part);
    assert_eq!(s.check_idcode().unwrap(), 0x0150_803f);
    assert_eq!(s.measure_length().unwrap(), 352);
    let bits = s.sample().unwrap();
    assert_eq!(bits, state);
    let mcs = snapshot::macrocells(&part, &bits);
    let m16 = mcs.iter().find(|m| m.mc == 16).unwrap();
    assert!(m16.oe && m16.out);
    assert_eq!(m16.pin, Some(4));

    let map: DesignMap = serde_json::from_str(
        r#"{"cells": [
              {"name": "led", "mc": 16, "pin": 4, "registered": true, "q_is_value": true, "output_tap": "reg"},
              {"name": "cnt0", "mc": 3, "registered": true, "q_is_value": false, "output_tap": "reg"},
              {"name": "nxt", "mc": 5, "registered": true, "output_tap": "comb"},
              {"name": "y", "mc": 7, "registered": false}],
            "inputs": [{"name": "btn", "pin": 81}]}"#,
    )
    .unwrap();
    let sig = design::signals(&part, &map);
    let v = |n: &str| sig.iter().find(|s| s.name == n).unwrap().value(&bits);
    assert!(v("led"));
    assert!(!v("cnt0"), "q_is_value = false inverts");
    assert_eq!(sig.iter().find(|s| s.name == "nxt").unwrap().how, design::How::D);
    assert_eq!(sig.iter().find(|s| s.name == "btn").unwrap().how, design::How::Pad);
    let text = snapshot::render(&part, &bits, &sig, false);
    assert!(text.contains("led") && text.contains("dedicated: "), "{text}");
}

#[test]
fn watch_changes_and_vcd() {
    let Some(part) = part() else { return };
    let map: DesignMap = serde_json::from_str(r#"{"cells": [{"name": "q", "mc": 16, "registered": true, "output_tap": "reg"}]}"#).unwrap();
    let sig = design::signals(&part, &map);
    let cols = snapshot::trace_columns(&part, &sig);
    let a = vec![false; part.length];
    let mut b = a.clone();
    b[part.mc_cell(16, CellKind::Out).unwrap().bit] = true;
    assert_eq!(snapshot::changes(&cols, &a, &b), vec![("q".to_string(), false, true)]);
    let path = std::env::temp_dir().join(format!("atf15xx-tool-test-{}.vcd", std::process::id()));
    {
        let f = std::fs::File::create(&path).unwrap();
        let mut v = snapshot::VcdCapture::new(f, &part, cols).unwrap();
        v.record(0, &a).unwrap();
        v.record(10, &a).unwrap();
        v.record(20, &b).unwrap();
        v.finish(21).unwrap();
    }
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(text.contains("$var wire 1") && text.contains(" q $end"), "{text}");
    assert!(text.contains("#0") && text.contains("#20") && !text.contains("#10"), "only changes are recorded:\n{text}");
}

#[test]
fn extest_drives_only_the_requested_pins() {
    let Some(part) = part() else { return };
    let mut state = vec![false; part.length];
    // MC14 is an output driving 1 (oe = 1, out = 1); MC16 an input (oe = 0)
    state[part.mc_cell(14, CellKind::Oe).unwrap().bit] = true;
    state[part.mc_cell(14, CellKind::Out).unwrap().bit] = true;
    let mut tap = MockTap { ir: 0, idcode: 0x0150_803f, state: state.clone(), preload: vec![], driven: None };
    let drives = snapshot::parse_drives(&part, &[], &["MC16=1".to_string(), "pin81=0".to_string()]).unwrap();
    assert_eq!(drives, vec![(16, true), (128, false)]);
    let mut s = Scanner::new(&mut tap, &part);
    let base = s.sample().unwrap();
    let bits = snapshot::drive_vector(&part, &base, &drives);
    s.extest(&bits).unwrap();
    s.release().unwrap();
    drop(s);
    let driven = tap.driven.clone().unwrap();
    let get = |mc, k| driven[part.mc_cell(mc, k).unwrap().bit];
    assert!(get(16, CellKind::Oe) && get(16, CellKind::Out));
    assert!(get(128, CellKind::Oe) && !get(128, CellKind::Out));
    assert!(get(14, CellKind::Oe) && get(14, CellKind::Out), "other pins keep their captured state");
    assert_eq!(tap.ir, 0x3ff, "released with BYPASS");
    assert!(snapshot::parse_drives(&part, &[], &["MC2=1".to_string()]).is_err(), "MC2 has no pin on the PLCC84");
}
