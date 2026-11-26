//! Command line compatible with the vendor fitters:
//! `fit1504 design.tt2 -device P1504C44 -preassign keep -strategy JTAG off -strategy output_fast on ...`
//! (`-strategy KEY VALUE` and `-KEY VALUE` are accepted; values up to the next `-option`).
//! Writes `<design>.jed`, `<design>.fit` and `<design>.pin` next to the input.

use crate::fit::{fit, Options, Preassign, Select};
use crate::{jed, report, tt2};
use atf15xx_arch::Family;

fn on(v: &[String]) -> Option<bool> {
    match v.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("on") | Some("auto") => Some(true),
        Some("off") => Some(false),
        _ => None,
    }
}

fn select(v: &[String]) -> Select {
    match on(v) {
        Some(true) => Select::All,
        Some(false) => Select::Off,
        None => Select::Only(
            v.iter().flat_map(|s| s.split(',')).filter(|s| !s.is_empty() && *s != "=").map(String::from).collect(),
        ),
    }
}

/// Apply one `key value...` option. Returns false if the key is unknown.
pub fn apply(o: &mut Options, key: &str, v: &[String]) -> bool {
    let v: Vec<String> = v.iter().filter(|s| s.as_str() != "=").cloned().collect();
    let b = |d: bool| on(&v).unwrap_or(d);
    match key.to_ascii_lowercase().as_str() {
        "device" | "dev" => o.device = v.first().cloned(),
        "preassign" => {
            o.preassign = match v.first().map(|s| s.to_ascii_lowercase()).as_deref() {
                Some("keep") => Preassign::Keep,
                Some("ignore") => Preassign::Ignore,
                _ => Preassign::Try,
            }
        }
        "jtag" => o.jtag = b(true),
        "pin_keep" => o.pin_keep = b(true),
        "output_fast" => o.output_fast = select(&v),
        "open_collector" => o.open_collector = select(&v),
        "mc_power" => o.mc_power = select(&v),
        "security" => o.security = b(false),
        "pd1" => o.pd1 = b(false),
        "pd2" => o.pd2 = b(false),
        "power_reset" => o.power_reset = b(false),
        "tdi_pullup" => o.tdi_pullup = b(false),
        "tms_pullup" => o.tms_pullup = b(false),
        "cascade_logic" => o.cascade = b(true),
        "logic_doubling" => o.logic_doubling = b(false),
        "foldback_logic" => o.foldback = b(true),
        // ours: `itd_net N|all on|off` sets input transition detection of one GCLK pin
        "itd_net" => {
            let on = v.get(1).map_or(true, |s| !s.eq_ignore_ascii_case("off"));
            match v.first().map(|s| s.as_str()) {
                Some("1") => o.itd[0] = on,
                Some("2") => o.itd[1] = on,
                Some("3") => o.itd[2] = on,
                _ => o.itd = [on; 3],
            }
        }
        // ours (not a vendor option): see Options::share_clock_networks
        "clock_networks" => o.share_clock_networks = b(false),
        "special_foldback" => o.special_foldback = b(false),
        "input_registers" => o.pack_input_registers = b(false),
        "observable_registers" => o.observable_registers = b(false),
        "xor_split" => o.xor_split = b(false),
        "gck_itd" => {
            o.itd = match v.first().map(|s| s.to_ascii_lowercase()).as_deref() {
                Some("off") => [false; 3],
                Some("gck0") => [true, false, false],
                Some("gck1") => [false, true, false],
                Some("gck2") => [false, false, true],
                _ => [true; 3],
            }
        }
        // accepted and ignored (no effect on the bitstream in the cases studied)
        "xor_synthesis" => o.xor_synthesis = b(true),
        "global_fold" | "optimize" | "soft_buffer" | "latch_synthesis"
        | "push_gate" | "verilog_sim" | "vhdl_sim" | "out_edif" | "tpd" | "fast_inlatch" | "debug" | "global_oe"
        | "oe_node" | "twoclock" | "ifmt" | "lib" | "pinfile" | "c" => {}
        _ => return false,
    }
    true
}

pub fn main(family: Family) -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(family, &args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("fit{}: {e}", family.number());
            1
        }
    }
}

pub fn run(family: Family, args: &[String]) -> Result<(), String> {
    let mut input = None;
    let mut opts = Options::default();
    let mut cli: Vec<(String, Vec<String>)> = vec![];
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(k) = a.strip_prefix('-') {
            let mut key = k.to_string();
            i += 1;
            if key.eq_ignore_ascii_case("strategy") && i < args.len() {
                key = args[i].clone();
                i += 1;
            }
            let mut vals = vec![];
            while i < args.len() && !(args[i].starts_with('-') && args[i].len() > 1) {
                vals.push(args[i].clone());
                i += 1;
            }
            match key.as_str() {
                "created" => opts.created = Some(vals.join(" ")),
                _ => cli.push((key, vals)),
            }
        } else {
            input = Some(a.clone());
            i += 1;
        }
    }
    let input = input.ok_or("usage: fit150x <design.tt2> [-device D] [-preassign keep|try|ignore] [-strategy KEY VALUE]...")?;
    let text = std::fs::read(&input).map_err(|e| format!("{input}: {e}"))?;
    let text = String::from_utf8_lossy(&text);
    let pla = tt2::Pla::parse(&text).map_err(|e| format!("{input}: {e}"))?;
    for (k, v) in pla.atmel_properties() {
        let vals: Vec<String> = v.split_whitespace().map(String::from).collect();
        apply(&mut opts, &k, &vals);
    }
    // the vendor honours the first occurrence of a repeated option
    let mut seen: Vec<String> = vec![];
    for (k, v) in &cli {
        let lk = k.to_ascii_lowercase();
        if seen.contains(&lk) {
            continue;
        }
        seen.push(lk);
        if !apply(&mut opts, k, v) {
            return Err(format!("unknown option -{k}"));
        }
    }
    let r = fit(family, &pla, &opts).map_err(|e| e.to_string())?;
    let stem = input.strip_suffix(".tt2").or_else(|| input.strip_suffix(".TT2")).unwrap_or(&input).to_string();
    write_outputs(&r, &stem, opts.created.as_deref(), &format!("fit{} {}", family.number(), args.join(" ")))?;
    println!("Design fits successfully");
    Ok(())
}

/// Write `<stem>.jed`, `<stem>.fit`, `<stem>.pin` and `<stem>.map.json` for a fit result; `cmd` is
/// the command line recorded in the `.fit` report, `created` the JEDEC date (default: now).
pub fn write_outputs(r: &crate::fit::FitResult, stem: &str, created: Option<&str>, cmd: &str) -> Result<(), String> {
    let created = created.map(String::from).unwrap_or_else(|| chrono::Local::now().format("%a %b %d %H:%M:%S %Y").to_string());
    let w = |ext: &str, text: String| std::fs::write(format!("{stem}.{ext}"), text).map_err(|e| format!("{stem}.{ext}: {e}"));
    w("jed", jed::write(&r.fuses, &created, r.device.pin_count()))?;
    w("fit", report::fit_report(r, cmd))?;
    w("pin", report::pin_report(r))?;
    // machine-readable placement (not written by the vendor fitter)
    w("map.json", serde_json::to_string_pretty(&report::map_json(r, None)).unwrap())
}
