//! Yosys plugin core (spec F9.7): the `synth_atf15xx` pass is registered in C++ (`src/rtlil.cc`)
//! and implemented here, across a [`cxx`] bridge instead of a JSON text interchange:
//!
//! * Rust **drives Yosys**: the synthesis script (single-sourced in `atf15xx-synth`) runs through
//!   `Pass::call` from Rust;
//! * Rust **reads RTLIL in-process**: modules, ports, cells and wires arrive as shared structs
//!   with the JSON backend's bit numbering and value encoding, so the flow's importer is reused
//!   unchanged (`-check-json` proves the two netlists identical);
//! * Rust **writes back**: every placed wire gets `atf15xx_mc` / `atf15xx_pin` attributes, visible to
//!   `select`, `stat`, `write_verilog` and later passes.
//!
//! Built with feature `yosys` by `tools/yosys-plugin/build.sh` inside the Yosys image (it needs the
//! Yosys headers).

use std::pin::Pin;

use crate::constraints::Constraints;
use crate::flow::{self, FlowOptions, InitPolicy, MapMode};
use serde_json::{json, Map, Value};

#[cxx::bridge(namespace = "atf15xx")]
mod ffi {
    /// A parameter or attribute value, encoded as `write_json -compat-int` writes it.
    struct Kv {
        key: String,
        /// 0 string, 1 signed int, 2 unsigned int, 3 bit string (MSB first)
        kind: u8,
        text: String,
        int_val: i64,
    }
    /// Bits: ids from 2 (one per SigMap-canonical bit); constants -1 "0", -2 "1", -3 "x", -4 "z".
    struct Conn {
        port: String,
        dir: String,
        bits: Vec<i64>,
    }
    struct CellRec {
        name: String,
        public_name: bool,
        ty: String,
        params: Vec<Kv>,
        attrs: Vec<Kv>,
        known: bool,
        conns: Vec<Conn>,
    }
    struct PortRec {
        name: String,
        dir: String,
        offset: i32,
        upto: bool,
        signed_: bool,
        bits: Vec<i64>,
    }
    struct WireRec {
        name: String,
        public_name: bool,
        bits: Vec<i64>,
        offset: i32,
        upto: bool,
        signed_: bool,
        attrs: Vec<Kv>,
    }
    struct ModuleRec {
        name: String,
        attrs: Vec<Kv>,
        ports: Vec<PortRec>,
        cells: Vec<CellRec>,
        wires: Vec<WireRec>,
    }
    /// A string attribute to set on a wire.
    struct Annot {
        wire: String,
        key: String,
        value: String,
    }
    struct FitArgs {
        top: String,
        part: String,
        constraints: String,
        out_dir: String,
        grade: String,
        /// 0 = no timing target
        fmax: f64,
        check_json: bool,
        annotate: bool,
        toggles: bool,
        /// Fitter options `KEY=VALUE[,VALUE]` (as the CLI's `--option`).
        options: Vec<String>,
        /// `auto` (both, the better fit kept), `gates` or `words` (spec F9.9).
        map: String,
    }

    unsafe extern "C++" {
        include!("atf15xx-synth/src/plugin/rtlil.h");
        type Design;
        fn read_design(design: Pin<&mut Design>) -> Vec<ModuleRec>;
        fn json_text(design: Pin<&mut Design>) -> String;
        fn run_pass(design: Pin<&mut Design>, command: &str);
        fn annotate(design: Pin<&mut Design>, module: &str, annots: &Vec<Annot>) -> usize;
        fn log_message(text: &str);
    }

    extern "Rust" {
        fn synth_and_fit(design: Pin<&mut Design>, args: &FitArgs) -> Result<String>;
    }
}

use ffi::{Design, FitArgs, Kv, ModuleRec};

fn kv_map(kvs: &[Kv]) -> Value {
    let mut m = Map::new();
    for k in kvs {
        let v = match k.kind {
            1 | 2 => json!(k.int_val),
            _ => Value::String(k.text.clone()),
        };
        m.insert(k.key.clone(), v);
    }
    Value::Object(m)
}

fn bits(b: &[i64]) -> Value {
    Value::Array(
        b.iter()
            .map(|&x| match x {
                -1 => json!("0"),
                -2 => json!("1"),
                -3 => json!("x"),
                -4 => json!("z"),
                n => json!(n),
            })
            .collect(),
    )
}

/// The modules as the Yosys JSON schema (`{"modules": …}`), without a text round trip.
pub fn netlist_value(mods: &[ModuleRec]) -> Value {
    let mut out = Map::new();
    for m in mods {
        let mut ports = Map::new();
        for p in &m.ports {
            let mut o = Map::new();
            o.insert("direction".into(), json!(p.dir));
            if p.offset != 0 {
                o.insert("offset".into(), json!(p.offset));
            }
            if p.upto {
                o.insert("upto".into(), json!(1));
            }
            if p.signed_ {
                o.insert("signed".into(), json!(1));
            }
            o.insert("bits".into(), bits(&p.bits));
            ports.insert(p.name.clone(), Value::Object(o));
        }
        let mut cells = Map::new();
        for c in &m.cells {
            let mut o = Map::new();
            o.insert("hide_name".into(), json!(if c.public_name { 0 } else { 1 }));
            o.insert("type".into(), json!(c.ty));
            o.insert("parameters".into(), kv_map(&c.params));
            o.insert("attributes".into(), kv_map(&c.attrs));
            if c.known {
                o.insert("port_directions".into(), Value::Object(c.conns.iter().map(|k| (k.port.clone(), json!(k.dir))).collect()));
            }
            o.insert("connections".into(), Value::Object(c.conns.iter().map(|k| (k.port.clone(), bits(&k.bits))).collect()));
            cells.insert(c.name.clone(), Value::Object(o));
        }
        let mut nets = Map::new();
        for w in &m.wires {
            let mut o = Map::new();
            o.insert("hide_name".into(), json!(if w.public_name { 0 } else { 1 }));
            o.insert("bits".into(), bits(&w.bits));
            if w.offset != 0 {
                o.insert("offset".into(), json!(w.offset));
            }
            if w.upto {
                o.insert("upto".into(), json!(1));
            }
            if w.signed_ {
                o.insert("signed".into(), json!(1));
            }
            o.insert("attributes".into(), kv_map(&w.attrs));
            nets.insert(w.name.clone(), Value::Object(o));
        }
        out.insert(
            m.name.clone(),
            json!({ "attributes": kv_map(&m.attrs), "ports": ports, "cells": cells, "netnames": nets }),
        );
    }
    json!({ "modules": out })
}

/// Placement annotations from the flow's `map.json`: per placed cell, its Yosys source net gets
/// `atf15xx_mc` (and `atf15xx_pin`); a bit of a vector net gets `atf15xx_mc_<bit>`.
fn annotations(map_json: &str) -> Vec<ffi::Annot> {
    let v: Value = serde_json::from_str(map_json).unwrap_or(Value::Null);
    let mut out = vec![];
    for c in v["cells"].as_array().into_iter().flatten() {
        let (Some(src), Some(mc)) = (c["source"].as_str(), c["mc"].as_u64()) else { continue };
        let (wire, suffix) = match src.split_once('[') {
            Some((w, rest)) => (w.to_string(), format!("_{}", rest.trim_end_matches(']'))),
            None => (src.to_string(), String::new()),
        };
        out.push(ffi::Annot { wire: wire.clone(), key: format!("atf15xx_mc{suffix}"), value: format!("MC{mc}") });
        if let Some(pin) = c["pin"].as_u64() {
            out.push(ffi::Annot { wire, key: format!("atf15xx_pin{suffix}"), value: pin.to_string() });
        }
    }
    out
}

fn synth_and_fit(mut design: Pin<&mut Design>, a: &FitArgs) -> Result<String, String> {
    let run = std::panic::AssertUnwindSafe(|| -> Result<String, String> {
        // Rust drives Yosys: hierarchy, then for each mapping the flow's synthesis script line by
        // line, from the same saved RTLIL (`design -save/-load`), each fitted in-process
        let hier = if a.top.is_empty() { "hierarchy -check -auto-top".to_string() } else { format!("hierarchy -check -top {}", a.top) };
        ffi::run_pass(design.as_mut(), &hier);
        let modes: Vec<MapMode> = match a.map.as_str() {
            "" | "auto" => vec![MapMode::None, MapMode::Words],
            "gates" | "none" => vec![MapMode::None],
            "words" => vec![MapMode::Words],
            m => return Err(format!("-map {m}: expected auto, gates or words")),
        };
        if modes.len() > 1 {
            ffi::run_pass(design.as_mut(), "design -save atf15xx_rtl");
        }
        let mut o = FlowOptions {
            part: (!a.part.is_empty()).then(|| a.part.clone()),
            grade: a.grade.clone(),
            fmax: (a.fmax > 0.0).then_some(a.fmax),
            toggles: a.toggles,
            fit_options: a
                .options
                .iter()
                .map(|o| {
                    let (k, v) = o.split_once('=').unwrap_or((o, "on"));
                    (k.to_string(), v.split(',').map(String::from).collect())
                })
                .collect(),
            ..FlowOptions::default()
        };
        if !a.constraints.is_empty() {
            o.constraints = Constraints::parse(&a.constraints)?;
        }
        let mut notes = vec![];
        // 1. Yosys (not thread-safe): each mapping's script from the saved RTLIL, its netlist read
        let mut netlists: Vec<Value> = vec![];
        for (k, mode) in modes.iter().enumerate() {
            if k > 0 {
                ffi::run_pass(design.as_mut(), "design -load atf15xx_rtl");
            }
            for line in flow::synth_script(InitPolicy::Error, *mode).lines() {
                let cmd = line.split('#').next().unwrap_or("").trim();
                if !cmd.is_empty() {
                    ffi::run_pass(design.as_mut(), cmd);
                }
            }
            // the netlist straight from RTLIL (word-level cells included for `words`)
            let mods = ffi::read_design(design.as_mut());
            let netlist = netlist_value(&mods);
            if a.check_json {
                let j: Value = serde_json::from_str(&ffi::json_text(design.as_mut())).map_err(|e| format!("JSON backend: {e}"))?;
                if j["modules"] != netlist["modules"] {
                    return Err(format!("-check-json ({mode:?}): the RTLIL bridge and the JSON backend disagree"));
                }
                notes.push(format!("-check-json ({mode:?}): RTLIL bridge netlist == JSON backend netlist"));
            }
            if modes.len() > 1 {
                ffi::run_pass(design.as_mut(), &format!("design -save atf15xx_map{k}"));
            }
            netlists.push(netlist);
        }
        // 2. the fits, in parallel (pure Rust), compared in order
        use rayon::prelude::*;
        let fits: Vec<Result<flow::Outputs, String>> = netlists.par_iter().map(|n| flow::fit_value(n, &o, "yosys synth_atf15xx")).collect();
        let mut best: Option<(usize, flow::Outputs)> = None;
        let mut errs = vec![];
        for (k, (mode, fit)) in modes.iter().zip(fits).enumerate() {
            match fit {
                Ok(r) => {
                    notes.push(format!(
                        "{mode:?}: {} macrocells{}",
                        flow::macrocells_used(&r.result),
                        r.fmax_mhz.map_or(String::new(), |f| format!(", fmax {f:.1} MHz"))
                    ));
                    if best.as_ref().map_or(true, |(_, b)| flow::better(b, &r)) {
                        best = Some((k, r));
                    }
                }
                Err(e) => {
                    notes.push(format!("{mode:?}: {e}"));
                    errs.push(e);
                }
            }
        }
        let Some((k, r)) = best else { return Err(errs.join("; ")) };
        if modes.len() > 1 {
            // the design the kept fit came from, for the annotations
            ffi::run_pass(design.as_mut(), &format!("design -load atf15xx_map{k}"));
            notes.push(format!("mapping {:?} kept", modes[k]));
        }
        flow::write_outputs(&r, std::path::Path::new(&a.out_dir))?;
        if a.annotate {
            let ann = annotations(&r.map_json);
            let n = ffi::annotate(design.as_mut(), &r.top, &ann);
            notes.push(format!("annotated {n} wire attribute(s) (atf15xx_mc, atf15xx_pin)"));
        }
        let mut s = r.summary.clone();
        for w in r.warnings.iter().map(|w| format!("warning: {w}")).chain(notes) {
            s.push('\n');
            s.push_str(&w);
        }
        if r.timing_met == Some(false) {
            return Err(format!("{s}\ntiming target missed"));
        }
        Ok(s)
    });
    std::panic::catch_unwind(run).unwrap_or_else(|_| Err("internal error (panic) in the ATF15xx flow".into()))
}
