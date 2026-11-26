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
use crate::ynet::{self, Bit, Const, Netlist};
use serde_json::Value;
use std::collections::BTreeMap;

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

fn consts(kvs: &[Kv]) -> BTreeMap<String, Const> {
    kvs.iter()
        .map(|k| {
            let v = match k.kind {
                1 | 2 => Const::Int(k.int_val),
                _ => Const::Str(k.text.clone()),
            };
            (k.key.clone(), v)
        })
        .collect()
}

fn bits(b: &[i64]) -> Vec<Bit> {
    b.iter()
        .map(|&x| match x {
            -1 => Bit::Const('0'),
            -2 => Bit::Const('1'),
            -3 => Bit::Const('x'),
            -4 => Bit::Const('z'),
            n => Bit::Net(n as u64),
        })
        .collect()
}

/// The modules as a typed netlist (spec F9.7 item 1), filled straight from RTLIL: the same
/// structs the JSON reader fills, so the importer cannot tell the two producers apart.
pub fn netlist(mods: &[ModuleRec]) -> Netlist {
    let mut modules = BTreeMap::new();
    for m in mods {
        let ports = m
            .ports
            .iter()
            .map(|p| (p.name.clone(), ynet::Port { direction: p.dir.clone(), bits: bits(&p.bits) }))
            .collect();
        let cells = m
            .cells
            .iter()
            .map(|c| {
                let cell = ynet::Cell {
                    ty: c.ty.clone(),
                    parameters: consts(&c.params),
                    attributes: consts(&c.attrs),
                    port_directions: c.known.then(|| c.conns.iter().map(|k| (k.port.clone(), k.dir.clone())).collect()),
                    connections: c.conns.iter().map(|k| (k.port.clone(), bits(&k.bits))).collect(),
                };
                (c.name.clone(), cell)
            })
            .collect();
        let netnames = m
            .wires
            .iter()
            .map(|w| {
                let n = ynet::NetName {
                    hide_name: u64::from(!w.public_name),
                    bits: bits(&w.bits),
                    offset: i64::from(w.offset),
                    upto: i64::from(w.upto),
                    attributes: consts(&w.attrs),
                };
                (w.name.clone(), n)
            })
            .collect();
        modules.insert(m.name.clone(), ynet::Module { attributes: consts(&m.attrs), ports: Some(ports), cells, netnames });
    }
    Netlist { modules }
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
        let mut netlists: Vec<Netlist> = vec![];
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
            let netlist = netlist(&mods);
            if a.check_json {
                let j = Netlist::from_json(&ffi::json_text(design.as_mut())).map_err(|e| format!("JSON backend: {e}"))?;
                if j.modules != netlist.modules {
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
        let fits: Vec<Result<flow::Outputs, String>> = netlists.par_iter().map(|n| flow::fit_netlist(n, &o, "yosys synth_atf15xx")).collect();
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
