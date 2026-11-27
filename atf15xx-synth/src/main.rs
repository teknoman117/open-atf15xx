//! `atf15xx-synth`: Verilog → .jed/.svf for ATF1502AS/1504AS/1508AS with open tools only.

use atf15xx_synth::constraints::Constraints;
use atf15xx_synth::flow::{self, FlowOptions, InitPolicy, MapMode};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Open Verilog → ATF15xx CPLD flow (Yosys + atf15xx fitter + atf15xx-tool SVF).
#[derive(Parser)]
#[command(name = "atf15xx-synth", version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Verilog → .jed, .svf, .fit, .pin, .pins.json (runs Yosys, the fitter and atmisp)
    Build {
        /// Verilog source files
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[command(flatten)]
        common: Common,
    },
    /// Yosys JSON (from `yosys ... script synth_atf15xx.ys; write_json`) → .jed etc.
    FitJson {
        json: PathBuf,
        #[command(flatten)]
        common: Common,
    },
    /// Print the bundled Yosys synthesis script
    Script {
        /// Initial-value policy the script is for (invert: Yosys inverts init-1 registers)
        #[arg(long, value_enum, default_value = "error")]
        init: InitPolicy,
        /// Logic mapping the script is for
        #[arg(long, value_enum, default_value = "none")]
        map: MapMode,
    },
}

#[derive(Args)]
struct Common {
    /// Part, e.g. ATF1504AS-PLCC44, ATF1508ASV-TQFP100, ATF1502AS-10JC44, P1504C44
    #[arg(short, long)]
    device: Option<String>,
    /// Top module (default: Yosys auto-top / the JSON's top attribute)
    #[arg(long)]
    top: Option<String>,
    /// Constraints file (set_io / fast / open_collector / low_power / option / device)
    #[arg(short = 'c', long)]
    constraints: Option<PathBuf>,
    /// Pin constraint NAME=PIN (repeatable; NAME as in Verilog, e.g. q[3]=8)
    #[arg(long = "pin", value_name = "NAME=PIN")]
    pins: Vec<String>,
    /// Fitter option KEY=VALUE (repeatable), e.g. jtag=off, logic_doubling=on
    #[arg(long = "option", value_name = "KEY=VALUE")]
    options: Vec<String>,
    /// Output directory
    #[arg(short, long, default_value = "out")]
    output: PathBuf,
    /// Product terms per cell (its macrocell and cascade helpers, at most 30) when inlining or splitting logic
    #[arg(long, default_value_t = 10)]
    max_products: usize,
    /// Seconds after which no further fitting strategy is started
    #[arg(long, default_value_t = 120)]
    fit_time: u64,
    /// Required register-to-register frequency in MHz: strategies are tried until one meets it;
    /// exit status 2 if none does (the best fit is still written)
    #[arg(long)]
    fmax: Option<f64>,
    /// Optimisation goal once the timing targets are met: area (fewest macrocells, default) or
    /// speed (highest fmax); overrides the constraints file's `optimize`
    #[arg(long, value_enum)]
    optimize: Option<atf15xx_synth::constraints::Goal>,
    /// Speed grade for timing (-7, -10, -15, -20, -25)
    #[arg(long, default_value = "-10", allow_hyphen_values = true)]
    grade: String,
    /// Do not convert counters to toggle flip-flops
    #[arg(long)]
    no_toggles: bool,
    /// Also write a .tt2 (for the vendor fitter / our fit150x binaries, as an oracle)
    #[arg(long)]
    emit_tt2: bool,
    /// Do not write an SVF
    #[arg(long)]
    no_svf: bool,
    /// SVF revision C instead of D
    #[arg(long)]
    svf_rev_c: bool,
    /// Text for the JEDEC `Created:` and SVF `Created on:` lines
    #[arg(long)]
    timestamp: Option<String>,
    /// Yosys executable
    #[arg(long, default_value = "yosys")]
    yosys: String,
    /// Registers with a Verilog initial value of 1: error (default), ignore (power up at 0), or
    /// invert (store them inverted so that power-up 0 gives 1)
    #[arg(long, value_enum, default_value = "error")]
    init: InitPolicy,
    /// Logic mapping: auto (gates and words, the better fit kept; default), none (Yosys gates),
    /// words (Yosys word-level cells lowered by the flow, spec F9.9), abc-gates, abc-sop (Yosys <= 0.68)
    #[arg(long, value_enum, default_value = "auto")]
    map: MapMode,
}

fn options(c: &Common) -> Result<FlowOptions, String> {
    let mut cons = match &c.constraints {
        Some(p) => Constraints::parse(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?)
            .map_err(|e| format!("{}: {e}", p.display()))?,
        None => Constraints::default(),
    };
    for p in &c.pins {
        let (n, v) = p.split_once('=').ok_or("--pin expects NAME=PIN")?;
        cons.pins.insert(n.to_string(), v.parse().map_err(|_| format!("--pin {p}: bad pin"))?);
    }
    let fit_options = c
        .options
        .iter()
        .map(|o| {
            let (k, v) = o.split_once('=').unwrap_or((o, "on"));
            (k.to_string(), v.split(',').map(String::from).collect())
        })
        .collect();
    let chain = atf15xx_fit::capacity::MAX_CHAIN_PTS;
    if !(2..=chain).contains(&c.max_products) {
        return Err(format!("--max-products {}: must be 2 to {chain} (the longest cascade chain)", c.max_products));
    }
    Ok(FlowOptions {
        part: c.device.clone(),
        top: c.top.clone(),
        constraints: cons,
        fit_options,
        max_products: c.max_products,
        max_products_given: c.max_products != 10,
        fit_time: c.fit_time,
        toggles: !c.no_toggles,
        emit_tt2: c.emit_tt2,
        svf: !c.no_svf,
        svf_rev_c: c.svf_rev_c,
        created: c.timestamp.clone(),
        yosys: c.yosys.clone(),
        init: c.init,
        debug_dir: Some(c.output.clone()),
        fmax: c.fmax,
        grade: c.grade.clone(),
        goal: c.optimize,
    })
}

fn run(cli: Cli) -> Result<(), String> {
    let cmdline = std::env::args().collect::<Vec<_>>().join(" ");
    let (jsons, common) = match cli.cmd {
        Cmd::Script { init, map } => {
            print!("{}", flow::synth_script(init, map));
            return Ok(());
        }
        Cmd::Build { files, common } => {
            let j = flow::synth_all(&files, common.top.as_deref(), &common.output, &common.yosys, common.init, common.map)?;
            (j, common)
        }
        Cmd::FitJson { json, common } => (vec![(MapMode::None, json)], common),
    };
    let o = options(&common)?;
    // each mapping fitted; the better fit kept (--map auto)
    let mut best: Option<(MapMode, flow::Outputs)> = None;
    let mut errs = vec![];
    let mut notes = vec![];
    // the mappings are fitted in parallel, then compared in order (deterministic)
    use rayon::prelude::*;
    let fits: Vec<(MapMode, Result<flow::Outputs, String>)> = jsons
        .par_iter()
        .map(|(mode, json)| {
            let r = std::fs::read_to_string(json)
                .map_err(|e| format!("{}: {e}", json.display()))
                .and_then(|text| flow::fit_json(&text, &o, &cmdline));
            (*mode, r)
        })
        .collect();
    for (mode, fit) in fits {
        match fit {
            Ok(r) => {
                notes.push(format!(
                    "{mode:?}: {} macrocells{}",
                    atf15xx_fit::search::macrocells_used(&r.result),
                    r.fmax_mhz.map_or(String::new(), |f| format!(", fmax {f:.1} MHz"))
                ));
                if best.as_ref().map_or(true, |(_, b)| flow::better(b, &r)) {
                    best = Some((mode, r));
                }
            }
            Err(e) => {
                notes.push(format!("{mode:?}: {e}"));
                errs.push(e);
            }
        }
    }
    let Some((mode, mut out)) = best else { return Err(errs.join("; ")) };
    if jsons.len() > 1 {
        out.warnings.push(format!("mapping {mode:?} kept ({})", notes.join("; ")));
    }
    for w in &out.warnings {
        eprintln!("warning: {w}");
    }
    let files = flow::write_outputs(&out, &common.output)?;
    eprintln!("{}", out.summary);
    for f in files {
        eprintln!("wrote {}", f.display());
    }
    if out.timing_met == Some(false) {
        std::process::exit(2);
    }
    Ok(())
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
