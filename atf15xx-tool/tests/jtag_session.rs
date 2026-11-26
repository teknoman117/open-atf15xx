//! Scripted EXTEST sessions through a simulated TAP whose pads follow the pins it drives.

use atf15xx_tool::dbg::design::{self, DesignMap};
use atf15xx_tool::dbg::part::{CellKind, Part};
use atf15xx_tool::dbg::session::{Options, Session};
use atf15xx_tool::dbg::Jtag;

/// One ATF15xx: SAMPLE/EXTEST capture `state`, except that the input cell of a pin whose output
/// is enabled by the last EXTEST update shows the driven value (the pad follows the driver).
struct MockTap {
    ir: u64,
    state: Vec<bool>,
    /// (oe bit, out bit, in bit) per pin
    pads: Vec<(usize, usize, usize)>,
    driven: Option<Vec<bool>>,
    extest_updates: usize,
}

impl Jtag for MockTap {
    fn ir(&mut self, opcode: u64) -> anyhow::Result<()> {
        self.ir = opcode;
        Ok(())
    }
    fn dr(&mut self, bits: &[bool]) -> anyhow::Result<Vec<bool>> {
        let mut captured = match self.ir {
            0x3ff => vec![false],
            0x055 | 0x000 => self.state.clone(),
            other => panic!("unexpected IR 0x{other:x}"),
        };
        if self.ir == 0x000 {
            if let Some(d) = &self.driven {
                for &(oe, out, inp) in &self.pads {
                    if d[oe] {
                        captured[inp] = d[out];
                    }
                }
            }
        }
        let n = captured.len();
        let mut chain = captured;
        chain.extend_from_slice(bits);
        let out = chain[..bits.len()].to_vec();
        if self.ir == 0x000 {
            self.driven = Some(chain[bits.len()..bits.len() + n].to_vec());
            self.extest_updates += 1;
        }
        Ok(out)
    }
}

fn setup() -> Option<(Part, Vec<u16>)> {
    let part = Part::load("ATF1508AS-PLCC84").unwrap();
    // five I/O pins that are not JTAG pins
    let pins: Vec<u16> = part
        .cells
        .iter()
        .filter(|c| c.kind == CellKind::In && c.mc.is_some())
        .filter_map(|c| c.pin)
        .filter(|p| !part.jtag_pins.contains(p))
        .take(6)
        .collect();
    Some((part, pins))
}

fn tap(part: &Part) -> MockTap {
    let pads = (1..=part.macrocells())
        .filter_map(|mc| {
            let (oe, out, inp) = (part.mc_cell(mc, CellKind::Oe)?, part.mc_cell(mc, CellKind::Out)?, part.mc_cell(mc, CellKind::In)?);
            Some((oe.bit, out.bit, inp.bit))
        })
        .collect();
    MockTap { ir: 0, state: vec![false; part.length], pads, driven: None, extest_updates: 0 }
}

#[test]
fn bus_cycle_drives_inputs_and_reads_back() {
    let Some((part, pins)) = setup() else { return };
    // an 8080-style write port: d[0..3] and wr_n are inputs; led (pins[5]) an output the design drives
    let map: DesignMap = serde_json::from_str(&format!(
        r#"{{"cells": [{{"name": "led", "mc": {led_mc}, "pin": {led}, "registered": false}}],
            "inputs": [{{"name": "d[0]", "pin": {}}}, {{"name": "d[1]", "pin": {}}}, {{"name": "d[2]", "pin": {}}},
                       {{"name": "d[3]", "pin": {}}}, {{"name": "wr_n", "pin": {}}}]}}"#,
        pins[0], pins[1], pins[2], pins[3], pins[4],
        led = pins[5],
        led_mc = part.cells.iter().find(|c| c.kind == CellKind::In && c.pin == Some(pins[5])).unwrap().mc.unwrap(),
    ))
    .unwrap();
    let signals = design::signals(&part, &map);
    let mut t = tap(&part);
    // the design drives led = 1
    let led_mc = part.cells.iter().find(|c| c.kind == CellKind::In && c.pin == Some(pins[5])).unwrap().mc.unwrap();
    t.state[part.mc_cell(led_mc, CellKind::Oe).unwrap().bit] = true;
    t.state[part.mc_cell(led_mc, CellKind::Out).unwrap().bit] = true;

    // without --allow-inputs, the design's input pins are refused
    {
        let mut s = Session::new(&mut t, &part, &signals, Options::default()).unwrap();
        assert!(s.exec("drive wr_n=1").is_err());
    }
    let mut s = Session::new(&mut t, &part, &signals, Options { allow_inputs: true, ..Options::default() }).unwrap();
    for (line, want) in [
        ("# a write cycle", None),
        ("drive d=0x5 wr_n=1", Some("ok drive d=0x5 wr_n=1")),
        ("drive wr_n=0", Some("ok drive wr_n=0")),
        ("print d wr_n", Some("d=0x5 wr_n=0")),
        ("expect d=5 wr_n=0", Some("ok expect d=5 wr_n=0")),
        ("drive wr_n=1", Some("ok drive wr_n=1")),
        ("expect d=0xa", Some("MISMATCH d=0x5 (expected 0xa)")),
        ("float d", Some("ok float d")),
        ("expect d=0", Some("ok expect d=0")),
    ] {
        assert_eq!(s.exec(line).unwrap().as_deref(), want, "{line}");
    }
    assert_eq!(s.failures, 1);
    assert!(s.exec("drive d=0x15").is_err(), "0x15 does not fit d[0..3]");
    assert!(s.exec("drive wr_n=2").is_err());
    assert!(s.exec("frobnicate").is_err());
    s.finish().unwrap();
    let driven = t.driven.clone().unwrap();
    let get = |mc, k| driven[part.mc_cell(mc, k).unwrap().bit];
    assert!(get(led_mc, CellKind::Oe) && get(led_mc, CellKind::Out), "the design's own output follows it");
    assert_eq!(t.ir, 0x3ff, "released with BYPASS at the end");
}
