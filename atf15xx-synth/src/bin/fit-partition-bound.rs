//! Exact lower bound for the fitter's block assignment (spec F4.6c).
//!
//!   cargo run --release -p atf15xx-synth --features ilp --bin fit-partition-bound -- \
//!       partition --netlist netlist.json -d ATF1504AS-PLCC44 [-c design.acf]
//!   fit-partition-bound partition --tt2 design.tt2 -d P1504C44
//!
//! Fits the design (the open flow, or the vendor-compatible fitter for a `.tt2`), takes the
//! block-assignment problem of that fit (`atf15xx_fit::fit::block_problem`: items with their UIM
//! inputs, macrocells incl. cascade helpers, pinned blocks, per-block capacity) and solves it
//! exactly with HiGHS: minimise the largest number of distinct UIM inputs of any block. The model
//! is a relaxation of the fitter's real problem (it ignores UIM mux matching, cascade adjacency
//! and PT limits), so an optimum above 40 proves that no fit with these cells exists, and the
//! optimum is a lower bound to compare our partitioner's result with.

use atf15xx_fit::fit::{block_problem, BlockProblem};
use clap::Parser;
use good_lp::{constraint, variable, Expression, ProblemVariables, Solution, SolverModel, Variable};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fit-partition-bound", version, about = "Exact lower bound (ILP, HiGHS) for the ATF15xx fitter's block assignment")]
enum Cli {
    /// Block assignment: our partitioner's fan-in against the ILP optimum
    Partition {
        #[arg(long)]
        netlist: Option<PathBuf>,
        #[arg(long)]
        tt2: Option<PathBuf>,
        /// Part (flow: ATF1504AS-PLCC44) or device (tt2: P1504C44)
        #[arg(short, long)]
        device: String,
        #[arg(short, long)]
        constraints: Option<PathBuf>,
        /// Solver time limit, seconds
        #[arg(long, default_value_t = 120.0)]
        time: f64,
    },
}

fn fit(netlist: &Option<PathBuf>, tt2: &Option<PathBuf>, device: &str, acf: &Option<PathBuf>) -> Result<atf15xx_fit::fit::FitResult, String> {
    if let Some(n) = netlist {
        let text = std::fs::read_to_string(n).map_err(|e| e.to_string())?;
        let mut o = atf15xx_synth::flow::FlowOptions { part: Some(device.to_string()), ..Default::default() };
        if let Some(c) = acf {
            o.constraints = atf15xx_synth::constraints::Constraints::parse(&std::fs::read_to_string(c).map_err(|e| e.to_string())?)?;
        }
        return Ok(atf15xx_synth::flow::fit_json(&text, &o, "fit-partition-bound")?.result);
    }
    let t = tt2.as_ref().ok_or("--netlist or --tt2")?;
    let text = String::from_utf8_lossy(&std::fs::read(t).map_err(|e| e.to_string())?).to_string();
    let pla = atf15xx_fit::tt2::Pla::parse(&text).map_err(|e| e.to_string())?;
    let dev = atf15xx_arch::Device::by_name(device).ok_or("unknown device")?;
    let mut o = atf15xx_fit::fit::Options { device: Some(device.to_string()), ..Default::default() };
    for (k, v) in pla.atmel_properties() {
        let vals: Vec<String> = v.split_whitespace().map(String::from).collect();
        atf15xx_fit::cli::apply(&mut o, &k, &vals);
    }
    // the vendor's command line, from its .fit next to the .tt2 (e.g. -JTAG off -preassign keep)
    let fitfile = t.with_extension("fit");
    if let Ok(f) = std::fs::read(&fitfile) {
        let f = String::from_utf8_lossy(&f);
        if let Some(line) = f.lines().find(|l| l.trim_start().starts_with("fit15")) {
            let words: Vec<String> = line.split_whitespace().skip(2).map(String::from).collect();
            let mut i = 0;
            while i < words.len() {
                let Some(k) = words[i].strip_prefix('-') else { i += 1; continue };
                let mut key = k.to_string();
                i += 1;
                if key.eq_ignore_ascii_case("strategy") && i < words.len() {
                    key = words[i].clone();
                    i += 1;
                }
                let mut vals = vec![];
                while i < words.len() && !words[i].starts_with('-') {
                    vals.push(words[i].clone());
                    i += 1;
                }
                if !key.eq_ignore_ascii_case("device") {
                    atf15xx_fit::cli::apply(&mut o, &key, &vals);
                }
            }
        }
    }
    atf15xx_fit::fit::fit(dev.family, &pla, &o).map_err(|e| e.to_string())
}

/// Minimise the largest block fan-in exactly. Returns (optimal?, max fan-in, assignment).
fn solve(p: &BlockProblem, time: f64) -> Result<(bool, usize, Vec<usize>), String> {
    let mut vars = ProblemVariables::new();
    let nb = p.blocks;
    let x: Vec<Vec<Option<Variable>>> = p
        .items
        .iter()
        .map(|it| (0..nb).map(|b| if it.fixed.map_or(true, |f| f == b) { Some(vars.add(variable().binary())) } else { None }).collect())
        .collect();
    let mut signals: Vec<&str> = p.items.iter().flat_map(|i| i.inputs.iter().map(|s| s.as_str())).collect();
    signals.sort();
    signals.dedup();
    let y: Vec<Vec<Variable>> = signals.iter().map(|_| (0..nb).map(|_| vars.add(variable().min(0.0).max(1.0))).collect()).collect();
    let big_f = vars.add(variable().min(0.0));
    let sum_y: Expression = y.iter().flatten().map(|&v| Expression::from(v)).sum();
    let mut model = vars.minimise(1000.0 * big_f + sum_y).using(good_lp::highs).set_time_limit(time);
    for xi in &x {
        model = model.with(constraint!(xi.iter().flatten().map(|&v| Expression::from(v)).sum::<Expression>() == 1.0));
    }
    for b in 0..nb {
        let used: Expression = p.items.iter().zip(&x).filter_map(|(it, xi)| xi[b].map(|v| it.mcs as f64 * v)).sum();
        model = model.with(constraint!(used <= p.capacity[b] as f64));
        let fan: Expression = y.iter().map(|ys| Expression::from(ys[b])).sum();
        model = model.with(constraint!(fan <= big_f));
    }
    for (it, xi) in p.items.iter().zip(&x) {
        for s in &it.inputs {
            let k = signals.binary_search(&s.as_str()).unwrap();
            for b in 0..nb {
                if let Some(v) = xi[b] {
                    model = model.with(constraint!(y[k][b] >= v));
                }
            }
        }
    }
    let sol = model.solve().map_err(|e| format!("solver: {e}"))?;
    let assign: Vec<usize> = x.iter().map(|xi| (0..nb).find(|&b| xi[b].map_or(false, |v| sol.value(v) > 0.5)).unwrap_or(0)).collect();
    let fan = p.fan_in(&assign);
    let status_optimal = matches!(sol.status(), good_lp::SolutionStatus::Optimal);
    Ok((status_optimal, *fan.iter().max().unwrap_or(&0), assign))
}

fn main() {
    let Cli::Partition { netlist, tt2, device, constraints, time } = Cli::parse();
    let r = match fit(&netlist, &tt2, &device, &constraints) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("fit failed: {e}");
            std::process::exit(1);
        }
    };
    let p = block_problem(&r);
    let ours: Vec<usize> = p.items.iter().map(|i| i.placed).collect();
    let fan_ours = p.fan_in(&ours);
    let t = std::time::Instant::now();
    match solve(&p, time) {
        Ok((optimal, best, assign)) => {
            let fan_opt = p.fan_in(&assign);
            println!(
                "items {} ({} pinned), blocks {}, capacity {:?} | ours: max {} {:?} | ILP {}: max {} {:?} ({:.1} s){}",
                p.items.len(),
                p.items.iter().filter(|i| i.fixed.is_some()).count(),
                p.blocks,
                p.capacity,
                fan_ours.iter().max().unwrap_or(&0),
                fan_ours,
                if optimal { "optimal" } else { "best found (time limit)" },
                best,
                fan_opt,
                t.elapsed().as_secs_f64(),
                if best > 40 && optimal { " -> no assignment within 40 inputs exists" } else { "" }
            );
        }
        Err(e) => println!("ILP: {e}"),
    }
}
