//! Minimal fitter reports (`.fit`, `.pin`). Not byte-compatible with the vendor's.
use crate::fit::{FitResult, Place};
use std::fmt::Write as _;

pub fn fit_report(r: &FitResult, cmdline: &str) -> String {
    let mut o = String::new();
    let crlf = "\r\n";
    write!(o, "open-atf150x {} fitter{crlf}{crlf}{cmdline}{crlf}{crlf}", r.device.family).unwrap();
    for (name, p) in &r.placement {
        match p {
            Place::Pin { pin, mc } => write!(o, "{name} is placed at pin {pin} (MC {mc}){crlf}").unwrap(),
            Place::Node { mc } => write!(o, "{name} is placed at feedback node {} (MC {mc}){crlf}", 600 + mc).unwrap(),
            Place::Dedicated { pin, .. } => write!(o, "{name} assigned to pin  {pin}{crlf}").unwrap(),
        }
    }
    write!(o, "{crlf}Universal-Interconnect-Multiplexer assignments{crlf}").unwrap();
    for (b, rows) in &r.routing {
        write!(o, "Multiplexer assignment for block {}{crlf}", atf15xx_arch::block_letter(*b)).unwrap();
        for (mux, name, src) in rows {
            write!(o, "{name}\t\t({src})  : MUX {mux}{crlf}").unwrap();
        }
        o.push_str(crlf);
    }
    o
}

pub fn pin_report(r: &FitResult) -> String {
    let mut o = String::new();
    for (name, p) in &r.placement {
        match p {
            Place::Pin { pin, mc } => writeln!(o, "Pin {pin} = {name}; /* MC {mc} */").unwrap(),
            Place::Dedicated { pin, .. } => writeln!(o, "Pin {pin} = {name};").unwrap(),
            Place::Node { mc } => writeln!(o, "PINNODE {} = {name}; /* MC {mc} Feedback */", 600 + mc).unwrap(),
        }
    }
    o
}

/// Machine-readable placement map (`<name>.map.json`, format `atf15xx-map/1`, spec F5.4): every
/// netlist cell with its macrocell and the fuse-level facts a state-injecting checker needs, and
/// every input with its pin. `origins` maps netlist names to source names (Verilog nets) where
/// known; otherwise `source` is the netlist name.
pub fn map_json(r: &FitResult, origins: Option<&std::collections::BTreeMap<String, String>>) -> serde_json::Value {
    use atf15xx_arch::decode::{decode, Storage as FStorage, Tap};
    use serde_json::json;
    let dec = decode(r.device.family, &r.fuses.bits);
    let place: std::collections::BTreeMap<&str, &Place> = r.placement.iter().map(|(n, p)| (n.as_str(), p)).collect();
    let src = |n: &str| origins.and_then(|o| o.get(n)).cloned().unwrap_or_else(|| n.to_string());
    let mut per_mc: std::collections::BTreeMap<u16, usize> = Default::default();
    for p in place.values() {
        if let Place::Pin { mc, .. } | Place::Node { mc } = p {
            *per_mc.entry(*mc).or_default() += 1;
        }
    }
    let mut cells = vec![];
    for c in &r.design.cells {
        let (mc, pin) = match place.get(c.name.as_str()) {
            Some(Place::Pin { pin, mc }) => (*mc, Some(*pin)),
            Some(Place::Node { mc }) => (*mc, None),
            _ => continue,
        };
        let m = &dec.macrocells[mc as usize - 1];
        let storage = match c.storage {
            crate::netlist::Storage::Comb => "comb",
            crate::netlist::Storage::Dff => "dff",
            crate::netlist::Storage::Tff => "tff",
            crate::netlist::Storage::Latch => "latch",
        };
        cells.push(json!({
            "name": c.name,
            "source": src(&c.name),
            "kind": if c.is_output { "output" } else { "buried" },
            "mc": mc,
            "block": atf15xx_arch::block_letter((mc as usize - 1) / 16).to_string(),
            "node": 600 + mc as u32,
            "pin": pin,
            // two cells in one macrocell: logic doubling (a buried register hosted in a
            // combinatorial output's macrocell; the register is the cell with "registered")
            "shared_mc": per_mc.get(&mc).copied().unwrap_or(0) > 1,
            "storage": storage,
            "registered": c.registered(),
            // the register (or latch) of this macrocell holds the cell's value itself: Q = value
            // (the fitter never stores a complement; `invert_fuse` only flips the sum before the
            // XOR/D input)
            "q_is_value": c.registered(),
            "invert_fuse": m.invert_fuse,
            "fuse_storage": match m.storage { FStorage::DFlipFlop => "dff", FStorage::TFlipFlop => "tff", FStorage::Latch => "latch" },
            "output_tap": match m.output { Tap::Comb => "comb", Tap::Reg => "reg" },
            "feedback_tap": match m.feedback { Tap::Comb => "comb", Tap::Reg => "reg" },
        }));
    }
    let mut inputs = vec![];
    for (n, p) in &r.placement {
        if r.design.cell_index(n).is_some() {
            continue;
        }
        let (pin, mc, dedicated) = match p {
            Place::Pin { pin, mc } => (*pin, Some(*mc), None),
            Place::Dedicated { pin, func } => (*pin, None, Some(format!("{func:?}"))),
            Place::Node { .. } => continue,
        };
        inputs.push(json!({ "name": n, "source": src(n), "pin": pin, "mc": mc, "dedicated": dedicated }));
    }
    json!({
        "format": "atf15xx-map/1",
        "device": r.device.name,
        "design": r.design.name,
        "cells": cells,
        "inputs": inputs,
    })
}
