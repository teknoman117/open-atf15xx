//! The fitting-strategy search (spec F7.4, F8.7, F9.9, F9.11): the netlist passes of
//! [`crate::opt`] and the fitter's own collapse in several combinations, each fitted with
//! [`fit_design`], the best fit kept by [`fit_key`] against the timing targets. Front ends
//! (`atf15xx-synth`) hand it a [`Design`] and get the chosen [`FitResult`] back.

use crate::fit::{fit_design, FitResult, Options, Place};
use crate::netlist::Design;
use crate::opt;
use atf15xx_arch::Device;
use std::path::PathBuf;

/// Timing targets (spec F8.7): what the flow must meet before it optimises area.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimingTargets {
    /// Register-to-register frequency for every clock, MHz.
    pub fmax: Option<f64>,
    /// Per clock port (Verilog name): MHz.
    pub clocks: Vec<(String, f64)>,
    /// Maximum pin-to-pin (tPD), clock-to-output (tCO) and input setup (tSU) delays, ns.
    pub tpd: Option<f64>,
    pub tco: Option<f64>,
    pub tsu: Option<f64>,
    pub goal: Goal,
}

impl TimingTargets {
    pub fn any(&self) -> bool {
        self.fmax.is_some() || !self.clocks.is_empty() || self.tpd.is_some() || self.tco.is_some() || self.tsu.is_some()
    }
}

/// What the flow optimises once the timing targets are met (or when there are none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Goal {
    /// Fewest macrocells (default).
    #[default]
    Area,
    /// Highest fmax, then fewest macrocells: more strategies are tried (wider sums, fewer levels).
    Speed,
}

/// What the search is asked for, besides the fitter options.
#[derive(Clone, Debug)]
pub struct SearchOptions {
    /// Product terms per cell (macrocell + one cascade neighbour) for inlining and splitting.
    pub max_products: usize,
    /// `max_products` was given explicitly (fewer strategies are tried).
    pub max_products_given: bool,
    /// Seconds after which no further fitting strategy is started.
    pub fit_time: u64,
    /// Toggle (T flip-flop) recovery.
    pub toggles: bool,
    /// Also render the netlist as `.tt2` (the first strategy's, or the chosen one's).
    pub emit_tt2: bool,
    /// Directory for the pre-fit `.tt2` and the vendor command line (with `emit_tt2`), written
    /// before fitting so that a failed fit can be inspected.
    pub debug_dir: Option<PathBuf>,
    /// Speed grade for timing (`-7` … `-25`).
    pub grade: String,
    pub targets: TimingTargets,
    /// Per-clock targets resolved to netlist names: (Verilog name, netlist name, MHz).
    pub clocks: Vec<(String, String, f64)>,
}

/// The chosen fit.
pub struct Found {
    pub result: FitResult,
    /// The `.tt2` of the chosen strategy's netlist (with `emit_tt2`).
    pub tt2: Option<String>,
    /// The chosen strategy and its netlist-pass log, for the summary.
    pub strategy: String,
    pub warnings: Vec<String>,
}

/// Bounds that no strategy can beat, checked before the (possibly long) netlist passes: one
/// macrocell flip-flop per register, and enough pins. `label` names the part in the messages.
pub fn check_bounds(d: &Design, dev: &Device, opts: &Options, label: &str) -> Result<(), String> {
    // every register or latch needs a macrocell's flip-flop (logic doubling only pairs a buried
    // register with combinatorial logic): more than the device has can never fit, whatever the
    // strategies do, so say so before the (possibly long) mapping passes
    let regs = d.cells.iter().filter(|c| c.storage != crate::netlist::Storage::Comb).count();
    let mcs = dev.family.macrocells();
    if regs > mcs {
        return Err(format!("fit failed: {regs} registers need at least {regs} macrocells (one flip-flop each), the {label} has {mcs}"));
    }
    // likewise pins: outputs need I/O pins, inputs any pin
    let (outs, ins) = (d.cells.iter().filter(|c| c.is_output).count(), d.inputs.len());
    let dedicated = [dev.gclk1, dev.gclk2_oe2, dev.oe1, dev.gclr].iter().collect::<std::collections::BTreeSet<_>>().len();
    // inputs may also use the input-only and the dedicated (GCLK/OE/GCLR) pins
    // with JTAG enabled its four pins are reserved
    let jtag_io = if opts.jtag { dev.io_pins().filter(|(p, _)| dev.is_jtag_pin(*p)).count() } else { 0 };
    let (io, input_only) = (dev.io_pins().count() - jtag_io, dev.input_only_pins().count() + dedicated);
    if outs > io || outs + ins > io + input_only {
        return Err(format!(
            "fit failed: {outs} outputs and {ins} inputs need {} pins ({outs} of them I/O pins), the {label} has {} ({io} I/O)",
            outs + ins,
            io + input_only
        ));
    }
    Ok(())
}

/// The ranking of fits, larger is better, shared by the strategy search, the piece forms and the
/// mappings (spec F8.7): meeting the timing targets; then missing them by less (`shortfall`, 0
/// when met or without targets); then by the goal fewer macrocells and higher fmax, or the
/// reverse order.
pub fn fit_key(goal: Goal, met: bool, shortfall: f64, macrocells: usize, fmax: f64) -> (bool, f64, f64, f64) {
    if goal == Goal::Speed {
        (met, -shortfall, fmax, -(macrocells as f64))
    } else {
        (met, -shortfall, -(macrocells as f64), fmax)
    }
}

/// A fit against the timing targets (spec F8.7): all met?, its fmax (MHz, infinite without
/// register paths), the violations as text, and the worst relative shortfall over all targets
/// (0 when all are met; `(want - got) / want` for a frequency, `(got - want) / want` for a
/// delay; 1 for a clock target that cannot be measured), which ranks fits that all miss.
/// `clocks`: (Verilog name, netlist name, MHz).
pub fn judge(
    r: &FitResult,
    t: &TimingTargets,
    grade: &str,
    clocks: &[(String, String, f64)],
) -> Result<(bool, f64, Vec<String>, f64), String> {
    let rep = timing_of(r, grade)?;
    let fmax = rep.fmax_mhz.unwrap_or(f64::INFINITY);
    let mut v = vec![];
    let mut short = 0f64;
    if let Some(want) = t.fmax {
        if fmax < want {
            v.push(format!("fmax {fmax:.1} MHz < {want:.1} MHz"));
            short = short.max((want - fmax) / want);
        }
    }
    for (vname, nname, want) in clocks {
        let pin = r.placement.iter().find(|(n, _)| n == nname).and_then(|(_, p)| match p {
            Place::Pin { pin, .. } | Place::Dedicated { pin, .. } => Some(*pin),
            _ => None,
        });
        match pin.map(|p| rep.period_for_clock(p)) {
            None => {
                v.push(format!("clock {vname}: not on a pin"));
                short = short.max(1.0);
            }
            Some(None) => {}
            Some(Some(per)) => {
                let f = 1000.0 / per;
                if f < *want {
                    v.push(format!("clock {vname}: {f:.1} MHz < {want:.1} MHz"));
                    short = short.max((want - f) / want);
                }
            }
        }
    }
    for (what, worst, want) in [
        ("tPD", rep.tpd.first().map(|p| p.ns), t.tpd),
        ("tCO", rep.tco.first().map(|p| p.ns), t.tco),
        ("tSU", rep.tsu.first().map(|p| p.ns), t.tsu),
    ] {
        if let (Some(w), Some(want)) = (worst, want) {
            if w > want {
                v.push(format!("{what} {w:.1} ns > {want:.1} ns"));
                short = short.max((w - want) / want);
            }
        }
    }
    Ok((v.is_empty(), fmax, v, short))
}

/// One fitting strategy of the flow (spec F7.4, F9.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Strategy {
    /// Product terms per cell (macrocell + one cascade neighbour) for inlining and splitting.
    mp: usize,
    /// Inline nodes when the macrocell balance is a tie.
    ties: bool,
    /// Clock-enable recovery.
    enables: bool,
    /// Spread output pins over the blocks.
    spread: bool,
    /// Shuffle the cell order (0 = netlist order).
    seed: u64,
    /// The fitter's vendor-style node collapse (dense designs).
    vcollapse: bool,
    /// Toggle (T flip-flop) recovery.
    toggles: bool,
    /// Shared nodes feeding control functions as foldback signals.
    special_foldback: bool,
    /// Foldback signals limited to one foldback level (spec F8.7): faster, may cost macrocells.
    fold_one_level: bool,
}

/// Static timing of a fit (spec F8.5) at `grade`, 5 V outputs.
pub fn timing_of(r: &FitResult, grade: &str) -> Result<atf15xx_sim::timing::Report, String> {
    let m = atf15xx_sim::model::Model::new(r.device, &r.fuses.bits, atf15xx_sim::model::Options::default());
    let p = atf15xx_sim::timing::Params::for_model(&m, grade)?;
    Ok(atf15xx_sim::timing::analyse(&m, &p, atf15xx_sim::timing::Vccio::V5))
}

/// Fit `d` with the strategies of spec F7.4/F9.9 and keep the best (spec F8.7).
pub fn search(dev: &'static Device, d: Design, opts: &Options, so: &SearchOptions) -> Result<Found, String> {
    let targets = &so.targets;
    let wants_timing = targets.any() || targets.goal == Goal::Speed;
    let mut warnings: Vec<String> = vec![];
    let mp = so.max_products;
    let base = Strategy {
        mp,
        ties: true,
        enables: true,
        spread: false,
        seed: 0,
        vcollapse: false,
        toggles: so.toggles,
        special_foldback: opts.special_foldback,
        fold_one_level: false,
    };
    let s = |mp: usize, ties: bool, enables: bool, spread: bool| Strategy { mp, ties, enables, spread, ..base };
    let plain: Vec<Strategy> = vec![
        s(mp, true, true, false),
        s(mp, true, false, false),
        s(mp, true, true, true),
        s(mp, false, true, true),
        s(mp, false, false, false),
        // no cascades at all: wide covers become buried OR nodes, which can go anywhere
        s(5, true, true, true),
        s(5, false, true, true),
    ];
    // then the same with the cells in shuffled orders (the fitter places in netlist order)
    let seeds: u64 = std::env::var("ATF15XX_FLOW_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    let shuffled: Vec<Strategy> = (1..=seeds)
        .flat_map(|k| {
            [
                Strategy { seed: k, ..s(mp, true, true, true) },
                Strategy { seed: k, ..s(mp, true, true, false) },
                Strategy { seed: k, ..s(5, true, true, true) },
            ]
        })
        .collect();
    // dense designs: when there are more cells than macrocells, also try the fitter's vendor-style
    // collapse of combinatorial nodes into their readers (what makes the .tt2 path fit such
    // designs: the wider sums are then factored with foldbacks, De Morgan, spec F4.6a)
    let n_mc = dev.family.macrocells();
    let dense = d.cells.len() > n_mc;
    let vc = |enables: bool, spread: bool| Strategy { vcollapse: true, ..s(mp, true, enables, spread) };
    let dense_first: Vec<Strategy> = if dense { vec![vc(true, false), vc(false, false), vc(true, true)] } else { vec![] };
    // capacity strategies (F9.9): put first as soon as a fit fails for lack of macrocells, pins
    // or product terms: shared nodes as foldback signals, and D instead of T flip-flops (a T
    // conversion can cost a node), the combination the dense SELFPROGRAMBOARD needs
    // with a timing target, each foldback variant also comes limited to one foldback level
    // (no foldback signals and no toggles: macrocell nodes for the shared control functions and
    // D flip-flops, which logic doubling can pair, as WinCUPL's netlists do: fast, if it fits)
    let capacity: Vec<Strategy> = [(true, true), (true, false), (false, true), (false, false)]
        .iter()
        .flat_map(|&(sf, tg)| {
            let st = Strategy { special_foldback: sf || opts.special_foldback, toggles: tg && so.toggles, ..vc(true, false) };
            if wants_timing && sf {
                vec![Strategy { fold_one_level: true, ..st }, st]
            } else {
                vec![st]
            }
        })
        .collect();
    // with a timing target: also wider sums (fewer logic levels, longer cascades: tPEXP per
    // stage instead of tUIM + tLAD + tCOMB per level), tried only if the others miss it
    let wider: Vec<Strategy> = if wants_timing && !so.max_products_given {
        [15, 20, 25].iter().flat_map(|&m| [s(m, true, true, false), s(m, true, true, true)]).collect()
    } else {
        vec![]
    };
    let mut queue: std::collections::VecDeque<Strategy> = dense_first
        .into_iter()
        .chain(plain)
        // the speed goal without a target scans the strategies for the best fmax: no shuffles
        .chain(if targets.goal == Goal::Speed && !targets.any() { vec![] } else { shuffled })
        // an explicit --max-products is used as given
        .filter(|st| !so.max_products_given || st.mp == mp)
        .chain(wider)
        .collect();
    let mut capacity_queued = false;
    // once queued, every capacity strategy is fitted and the best kept (fewest macrocells, then
    // fmax: on dense designs they differ mostly in speed, F8.7)
    let mut capacity_pending: Vec<Strategy> = vec![];
    let mut best_area_fmax = f64::NEG_INFINITY;
    // after the first fit, one alternative is tried and the smaller fit kept: the vendor-style
    // collapse if it has not run yet (it can merge nodes the netlist kept, F9.9)
    let mut improve: Option<Strategy> = if dense { None } else { Some(vc(true, false)) };
    let mut running_alt: Option<Strategy> = None;
    let mut tried: Vec<Strategy> = vec![];
    let mut best: Option<(usize, FitResult, Option<String>, String)> = None;
    let mut first_err = None;
    let mut tt2 = None;
    let t0 = std::time::Instant::now();
    // with --fmax: keep trying strategies until one meets it (best fmax kept meanwhile)
    let mut met = false;
    let mut best_key: Option<(bool, f64, f64, f64)> = None;
    // one strategy's netlist passes and fit, as a pure function of the strategy (spec F9.11):
    // the driver below keeps the sequential decisions and fits the next strategies of the queue
    // speculatively in parallel (rayon), caching their outcomes
    struct Outcome {
        log: Vec<String>,
        t: Option<String>,
        name: String,
        n_cells: usize,
        n_buried: usize,
        n_pins_in: usize,
        fit: Result<FitResult, crate::fit::FitError>,
    }
    let base = &d;
    type Prefix = std::sync::Arc<std::sync::OnceLock<(crate::netlist::Design, Vec<String>)>>;
    let prefix_cache: std::sync::Mutex<std::collections::HashMap<(usize, bool, bool, bool), Prefix>> = Default::default();
    let run = |st: Strategy, si: usize| -> Result<Outcome, String> {
        let t_pass = std::time::Instant::now();
        let Strategy { mp, ties, enables, spread, seed, vcollapse, toggles, special_foldback, fold_one_level } = st;
        // the netlist passes up to the split depend only on these knobs: computed once per
        // key and shared by the strategies (and threads) that need them (spec F9.11)
        let key = (mp, toggles, enables, ties);
        let slot = prefix_cache
            .lock()
            .expect("prefix cache")
            .entry(key)
            .or_insert_with(|| std::sync::Arc::new(std::sync::OnceLock::new()))
            .clone();
        let (pd, plog) = slot.get_or_init(|| {
            let mut d = base.clone();
            let mut log: Vec<String> = vec![];
            // with ATF15XX_FLOW_DEBUG or ATF15XX_FLOW_TIMING, the time of each netlist pass
            let dbg = std::env::var_os("ATF15XX_FLOW_DEBUG").is_some() || std::env::var_os("ATF15XX_FLOW_TIMING").is_some();
            let mut tp = std::time::Instant::now();
            let mut lap = |what: &str| {
                if dbg {
                    eprintln!("pass {what}: {:.2} s", tp.elapsed().as_secs_f64());
                }
                tp = std::time::Instant::now();
            };
            if toggles {
                log.push(format!("{} T flip-flops", opt::recover_toggles(&mut d, mp)));
                opt::remove_dead(&mut d);
                let chains = opt::factor_wide(&mut d);
                if chains > 0 {
                    log.push(format!("{chains} carry-chain nodes"));
                }
                lap("toggles");
            }
            if enables {
                log.push(format!("{} clock enables", opt::recover_enables(&mut d, mp)));
                opt::remove_dead(&mut d);
                lap("enables");
            }
            if let (Some(n), Some(c)) = (std::env::var("ATF15XX_FLOW_TRACE").ok(), None::<()>.or(Some(()))) {
                let _ = c;
                if let Some(cell) = d.cell(&n) {
                    eprintln!("trace {n} before collapse: data {:?} ce {:?}", cell.data, cell.ce);
                }
            }
            opt::collapse_limited(&mut d, mp, ties);
            lap("collapse");
            if let Ok(n) = std::env::var("ATF15XX_FLOW_TRACE") {
                if let Some(cell) = d.cell(&n) {
                    eprintln!("trace {n} after collapse: data {:?} ce {:?}", cell.data, cell.ce);
                }
            }
            opt::recover_latch_resets(&mut d);
            opt::remove_dead(&mut d);
            let splits = opt::split_large(&mut d, mp);
            if splits > 0 {
                log.push(format!("{splits} OR nodes split off"));
            }
            lap("split");
            (d, log)
        });
        let mut d = pd.clone();
        let mut log = plog.clone();
        let t = if so.emit_tt2 { Some(crate::tt2out::write(&d, &dev.tt2_device)?) } else { None };
        let tv = std::time::Instant::now();
        let timing = std::env::var_os("ATF15XX_FLOW_TIMING").is_some();
        if vcollapse {
            // shared nodes feeding control functions become foldback signals first (the vendor's
            // XXL_ nodes): they stay shared instead of being expanded into every reader.
            // 1. flatten the gate trees into those nodes (collapse everything else into them)
            use crate::{logic::Signal as Sg, netlist::Storage as St};
            let feeds_control: std::collections::BTreeSet<String> = d
                .cells
                .iter()
                .flat_map(|o| [&o.clk, &o.ce, &o.ar, &o.ap, &o.oe].into_iter().flatten().flat_map(|f| f.signals()))
                .filter_map(|s| if let Sg::Fb(n, false) = s { Some(n) } else { None })
                .collect();
            let mut held = vec![];
            for c in d.cells.iter_mut() {
                if !c.is_output && c.storage == St::Comb {
                    if feeds_control.contains(&c.name) && c.node.is_none() {
                        c.node = Some(0);
                        held.push(c.name.clone());
                    } else if !feeds_control.contains(&c.name) && c.node == Some(0) {
                        c.node = None;
                    }
                }
            }
            d.collapse_comb_nodes();
            for c in d.cells.iter_mut() {
                if held.contains(&c.name) || (feeds_control.contains(&c.name) && c.node == Some(0)) {
                    c.node = None;
                }
            }
            // 2. convert them
            // (only with foldback signals: otherwise they stay macrocell nodes, one UIM pass)
            let nf = if special_foldback { d.nodes_to_foldbacks_with(4, fold_one_level) } else { 0 };
            if nf > 0 {
                log.push(format!("{nf} nodes as foldback signals"));
            }
            // the flow's own collapse pinned the nodes it rejected (node marker 0); let the
            // fitter's vendor-style collapse (bounded by the 40 UIM inputs) decide instead
            for c in d.cells.iter_mut() {
                if !c.is_output && c.storage == crate::netlist::Storage::Comb && c.node == Some(0) {
                    c.node = None;
                }
            }
            d.collapse_comb_nodes();
            if timing {
                eprintln!("strategy {si}: vendor-style collapse: {:.2} s", tv.elapsed().as_secs_f64());
            }
            log.push("vendor-style node collapse".into());
        }
        let tq = std::time::Instant::now();
        d.absorb_output_registers();
        let ta = tq.elapsed().as_secs_f64();
        d.legalize_special_functions_with(special_foldback);
        if timing {
            eprintln!("pass absorb: {ta:.2} s, legalize: {:.2} s", tq.elapsed().as_secs_f64() - ta);
        }
        if seed > 0 {
            shuffle(&mut d.cells, seed);
        }
        if spread {
            spread_pins(&mut d, dev, opts);
        }
        if std::env::var_os("ATF15XX_FLOW_DEBUG").is_some() {
            eprintln!("strategy {si}: max_products {mp}, ties {ties}, enables {enables}, spread {spread} ({st:?})");
            for c in &d.cells {
                eprintln!(
                    "cell {:24} {:?} out={} products={} (complement {}) inputs={} ce={} ar={} ap={} oe={}",
                    c.name,
                    c.storage,
                    c.is_output,
                    c.data.0.len(),
                    c.data.complement().0.len(),
                    c.inputs().len(),
                    c.ce.is_some(),
                    c.ar.is_some(),
                    c.ap.is_some(),
                    c.oe.is_some()
                );
            }
        }
        let (n_cells, n_buried) = (d.cells.len(), d.cells.iter().filter(|c| !c.is_output).count());
        let n_pins_in = d.inputs.len();
        let mut sopts = opts.clone();
        sopts.special_foldback = special_foldback;
        // a strategy may not run past the budget (the first one gets at least the whole budget)
        sopts.deadline = Some(t0 + std::time::Duration::from_secs(so.fit_time));
        let name = d.name.clone();
        let passes_s = t_pass.elapsed().as_secs_f64();
        let t_fit = std::time::Instant::now();
        let fit = fit_design(dev, d, &sopts);
        if std::env::var_os("ATF15XX_FLOW_TIMING").is_some() {
            let ((mh, mm), (ch, cm)) = crate::logic::memo_stats();
            eprintln!(
                "timing: strategy {si}: passes {passes_s:.2} s, fit {:.2} s ({}); memo minimize {mh}/{} complement {ch}/{} (hits/calls, this thread)",
                t_fit.elapsed().as_secs_f64(),
                if fit.is_ok() { "ok" } else { "failed" },
                mh + mm,
                ch + cm
            );
        }
        Ok(Outcome { log, t, name, n_cells, n_buried, n_pins_in, fit })
    };
    let threads = rayon::current_num_threads().max(1);
    let mut cache: std::collections::HashMap<Strategy, Result<Outcome, String>> = std::collections::HashMap::new();
    while let Some(st) = queue.pop_front() {
        if tried.contains(&st) {
            continue;
        }
        let si = tried.len();
        tried.push(st);
        let was_pending = capacity_pending.contains(&st);
        capacity_pending.retain(|c| *c != st);
        let Strategy { ties, enables, spread, seed, toggles, special_foldback, fold_one_level, .. } = st;
        if best.is_some() && (!wants_timing || (met && targets.goal == Goal::Area)) && running_alt != Some(st) && !was_pending {
            match improve.take() {
                Some(alt) if !wants_timing && !tried.contains(&alt) => {
                    // one more fit with the alternative, then stop
                    queue.clear();
                    queue.push_front(alt);
                    running_alt = Some(alt);
                    tried.pop();
                    continue;
                }
                _ => break,
            }
        }
        if si > 0 && t0.elapsed().as_secs() >= so.fit_time {
            warnings.push(format!("fit time budget of {} s used up after {si} strategies (--fit-time)", so.fit_time));
            break;
        }
        // the outcome: cached, or fitted now together with the next untried strategies
        if !cache.contains_key(&st) {
            use rayon::prelude::*;
            // only strategies known to be needed run speculatively: the pending capacity
            // strategies of a dense design, all of which are fitted anyway (a wider speculation
            // fitted strategies the sequential search never reaches, and the batch waits for the
            // slowest: measured slower)
            let mut batch = vec![st];
            for s in queue.iter() {
                if batch.len() >= threads {
                    break;
                }
                if capacity_pending.contains(s) && !tried.contains(s) && !cache.contains_key(s) && !batch.contains(s) {
                    batch.push(*s);
                }
            }
            let outs: Vec<(Strategy, Result<Outcome, String>)> = batch.par_iter().enumerate().map(|(k, s)| (*s, run(*s, si + k))).collect();
            cache.extend(outs);
        }
        let Outcome { log, t, name, n_cells, n_buried, n_pins_in, fit } = cache.remove(&st).expect("outcome")?;
        if si == 0 {
            tt2 = t.clone();
            if let (Some(t), Some(dir)) = (&t, &so.debug_dir) {
                // written before fitting, so that a failed fit can be inspected (and given to the
                // vendor fitter: <top>.vendor-cmd is its command line)
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join(format!("{name}.tt2")), t);
                let _ = std::fs::write(dir.join(format!("{name}.vendor-cmd")), vendor_cmd(&name, dev, opts));
            }
        }
        match fit {
            Ok(r) => {
                let used = macrocells_used(&r);
                let desc = format!(
                    "strategy {si} (ties {ties}, enables {enables}, spread {spread}, seed {seed}{}{}{}): {}",
                    if toggles { "" } else { ", no toggles" },
                    if special_foldback { ", foldback signals" } else { "" },
                    if fold_one_level { " (one level)" } else { "" },
                    log.join(", ")
                );
                if !wants_timing {
                    let f = timing_of(&r, &so.grade)?.fmax_mhz.unwrap_or(f64::INFINITY);
                    if best.as_ref().map_or(true, |b| used < b.0 || (used == b.0 && f > best_area_fmax)) {
                        best_area_fmax = f;
                        best = Some((used, r, t, desc));
                    }
                } else {
                    // timing first (F8.7): a fit that meets the targets beats one that does not,
                    // even with more macrocells (e.g. one logic level wider instead of two); of
                    // fits that all miss, the one that misses by less (its shortfall is 0 when met)
                    let (ok, f, _, short) = judge(&r, &targets, &so.grade, &so.clocks)?;
                    let k = fit_key(targets.goal, ok, short, used, f);
                    if best_key.map_or(true, |b: (bool, f64, f64, f64)| k > b) {
                        best_key = Some(k);
                        met = ok;
                        best = Some((used, r, t, desc));
                    }
                }
            }
            Err(e) => {
                if std::env::var_os("ATF15XX_FLOW_DEBUG").is_some() {
                    eprintln!("strategy {si}: {e} ({n_cells} cells)");
                }
                // the fitter says why: out of macrocells, pins or PTs -> cell-reducing strategies next
                if !capacity_queued && e.kind() == crate::fit::FitErrorKind::Capacity {
                    capacity_queued = true;
                    for c in capacity.iter().rev() {
                        queue.push_front(*c);
                    }
                    capacity_pending = capacity.iter().copied().filter(|c| !tried.contains(c)).collect();
                }
                first_err.get_or_insert(format!(
                    "fit failed: {e} ({n_cells} cells, {n_buried} of them buried, {n_pins_in} inputs; {} macrocells)",
                    dev.family.macrocells()
                ));
            }
        }
    }
    let Some((_, result, best_tt2, strategy)) = best else { return Err(first_err.unwrap()) };
    if best_tt2.is_some() {
        tt2 = best_tt2;
    }
    Ok(Found { result, tt2, strategy, warnings })
}

/// The vendor fitter command line equivalent to `opts` for `<top>.tt2` (oracle runs).
pub fn vendor_cmd(top: &str, dev: &atf15xx_arch::Device, opts: &Options) -> String {
    use crate::fit::Select;
    let onoff = |b: bool| if b { "on" } else { "off" };
    let sel = |s: &Select| match s {
        Select::All => Some("on".to_string()),
        Select::Off => None,
        Select::Only(v) => Some(v.join(",")),
    };
    let mut a = vec![
        format!("fit{}", dev.family.number()),
        format!("{top}.tt2"),
        format!("-device {}", dev.name),
        "-preassign keep".into(),
        format!("-JTAG {}", onoff(opts.jtag)),
        format!("-pin_keep {}", onoff(opts.pin_keep)),
        format!("-cascade_logic {}", onoff(opts.cascade)),
        format!("-foldback_logic {}", onoff(opts.foldback)),
        format!("-logic_doubling {}", onoff(opts.logic_doubling)),
    ];
    for (k, s) in [("output_fast", &opts.output_fast), ("open_collector", &opts.open_collector), ("mc_power", &opts.mc_power)] {
        if let Some(v) = sel(s) {
            a.push(format!("-{k} {v}"));
        }
    }
    a.join(" ") + "\n"
}

/// Deterministic shuffle for placement restarts (seeded ChaCha8: reproducible across versions).
fn shuffle<T>(v: &mut [T], seed: u64) {
    use rand::{seq::SliceRandom, SeedableRng};
    v.shuffle(&mut rand_chacha::ChaCha8Rng::seed_from_u64(seed));
}

/// Pre-assign pins to unconstrained outputs, spreading them over the blocks by fan-in (largest
/// first, into the block with the least fan-in so far), so that no block's UIM gets all the wide
/// outputs (the fitter otherwise fills block A first).
fn spread_pins(d: &mut Design, dev: &atf15xx_arch::Device, opts: &Options) {
    use std::collections::BTreeSet;
    let mut taken: BTreeSet<u16> = d.inputs.values().flatten().copied().collect();
    taken.extend(d.cells.iter().filter_map(|c| c.pin));
    if opts.jtag {
        taken.extend([dev.jtag.tdi, dev.jtag.tms, dev.jtag.tck, dev.jtag.tdo]);
    }
    if opts.pd1 {
        taken.insert(dev.pd1);
    }
    if opts.pd2 {
        taken.insert(dev.pd2);
    }
    let nblk = (dev.family.macrocells() / 16) as u16;
    let mut load = vec![0usize; nblk as usize];
    let mut order: Vec<usize> = (0..d.cells.len()).filter(|&i| d.cells[i].is_output && d.cells[i].pin.is_none()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(d.cells[i].inputs().len()));
    for i in order {
        let mut blocks: Vec<u16> = (0..nblk).collect();
        blocks.sort_by_key(|&b| load[b as usize]);
        for b in blocks {
            let pin = dev.io_pins().filter(|&(p, m)| (m - 1) / 16 == b && !taken.contains(&p)).map(|(p, _)| p).min();
            if let Some(p) = pin {
                taken.insert(p);
                d.cells[i].pin = Some(p);
                load[b as usize] += d.cells[i].inputs().len().max(1);
                break;
            }
        }
    }
}

/// Macrocells holding a cell (pins used only as inputs are not counted).
pub fn macrocells_used(r: &FitResult) -> usize {
    let cells: std::collections::BTreeSet<&str> = r.design.cells.iter().map(|c| c.name.as_str()).collect();
    let mut mcs: std::collections::BTreeSet<u16> = Default::default();
    for (n, p) in &r.placement {
        if let (true, Place::Pin { mc, .. } | Place::Node { mc }) = (cells.contains(n.as_str()), p) {
            mcs.insert(*mc);
        }
    }
    mcs.len()
}

#[cfg(test)]
mod ranking_tests {
    use super::*;

    /// A missed target ranks fits by how close they come (2026-11-26): the 10-bit registered
    /// compare's candidates on the ATF1502AS, asked for 55 MHz (all miss).
    #[test]
    fn missed_targets_rank_by_shortfall() {
        let short = |f: f64| (55.0 - f) / 55.0;
        let small_slow = fit_key(Goal::Area, false, short(41.7), 26, 41.7);
        let big_fast = fit_key(Goal::Area, false, short(52.6), 31, 52.6);
        assert!(big_fast > small_slow);
        // met beats missed, whatever the size
        assert!(fit_key(Goal::Area, true, 0.0, 31, 52.6) > fit_key(Goal::Area, false, 0.01, 10, 54.0));
        // without a target (or with all met): the goal decides, as before
        assert!(fit_key(Goal::Area, true, 0.0, 27, 41.7) > fit_key(Goal::Area, true, 0.0, 31, 52.6));
        assert!(fit_key(Goal::Speed, true, 0.0, 31, 52.6) > fit_key(Goal::Speed, true, 0.0, 27, 41.7));
    }
}
