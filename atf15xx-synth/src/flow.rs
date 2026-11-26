//! The flow driver: Verilog → Yosys → netlist → fitter → `.jed` / `.svf` / reports.

use crate::constraints::{Constraints, Goal, TimingTargets};
use crate::part::Part;
use crate::{opt, tt2out, yosys};
use atf15xx_fit::fit::{fit_design, FitResult, Options, Place};
use atf15xx_fit::{jed, report};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const SYNTH_SCRIPT: &str = include_str!("../data/synth_atf15xx.ys");

/// What to do with Verilog initial values of 1 (`reg q = 1;`, `initial q = 1;`). The flow
/// assumes every macrocell register powers up at 0 (no fuse selects a power-up value); whether
/// that holds on silicon is open (docs/worklog/04-simulator.md, 05-verilog-flow.md), so the
/// default refuses designs that need a 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum InitPolicy {
    /// Fail if any register has an initial value of 1 (default).
    Error,
    /// Accept, warn, and implement power-up 0 anyway.
    Ignore,
    /// Let Yosys (`dfflegalize`, init 0 only) store such registers inverted, so that power-up 0
    /// gives the initial value 1. Correct only if the registers really power up at 0.
    Invert,
}

/// How Yosys maps the logic before atf15xx-synth builds the covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum MapMode {
    /// Fine-grained gates straight from `synth -noabc` (default; works with any Yosys).
    None,
    /// ABC re-synthesis to simple gates (`abc -g ...`).
    AbcGates,
    /// ABC two-level covers (`abc -sop -I 40 -P 80`): Yosys 0.68 and older only.
    AbcSop,
    /// Word level (spec F9.9): Yosys stops at its coarse cells, `wordmap` lowers them.
    Words,
    /// Both `none` (gates) and `words`, fitted, the better fit kept (spec F9.9; default).
    Auto,
}

/// Is fit `b` better than fit `a` (spec F8.7, F9.9)? Meeting the timing targets first; then
/// with the area goal fewer macrocells, then fmax; with the speed goal fmax, then macrocells.
pub fn better(a: &Outputs, b: &Outputs) -> bool {
    let mcs = |o: &Outputs| macrocells_used(&o.result) as f64;
    let f = |o: &Outputs| o.fmax_mhz.unwrap_or(f64::INFINITY);
    if let (Some(ta), Some(tb)) = (a.timing_met, b.timing_met) {
        if ta != tb {
            return tb;
        }
    }
    let key = |o: &Outputs| if o.goal == Goal::Speed { (-f(o), mcs(o)) } else { (mcs(o), -f(o)) };
    key(b) < key(a)
}

/// A fit against the timing targets (spec F8.7): all met?, its fmax (MHz, infinite without
/// register paths), and the violations as text. `clocks`: (Verilog name, netlist name, MHz).
pub fn judge(r: &FitResult, t: &TimingTargets, grade: &str, clocks: &[(String, String, f64)]) -> Result<(bool, f64, Vec<String>), String> {
    let rep = timing_of(r, grade)?;
    let fmax = rep.fmax_mhz.unwrap_or(f64::INFINITY);
    let mut v = vec![];
    if let Some(want) = t.fmax {
        if fmax < want {
            v.push(format!("fmax {fmax:.1} MHz < {want:.1} MHz"));
        }
    }
    for (vname, nname, want) in clocks {
        let pin = r.placement.iter().find(|(n, _)| n == nname).and_then(|(_, p)| match p {
            Place::Pin { pin, .. } | Place::Dedicated { pin, .. } => Some(*pin),
            _ => None,
        });
        match pin.map(|p| rep.period_for_clock(p)) {
            None => v.push(format!("clock {vname}: not on a pin")),
            Some(None) => {}
            Some(Some(per)) => {
                let f = 1000.0 / per;
                if f < *want {
                    v.push(format!("clock {vname}: {f:.1} MHz < {want:.1} MHz"));
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
            }
        }
    }
    Ok((v.is_empty(), fmax, v))
}

/// The word-level synthesis script (spec F9.9).
pub const SYNTH_SCRIPT_WORDS: &str = include_str!("../data/synth_atf15xx_words.ys");

/// The synthesis script for an initial-value policy and a logic mapping.
pub fn synth_script(init: InitPolicy, map: MapMode) -> String {
    let base = if map == MapMode::Words { SYNTH_SCRIPT_WORDS } else { SYNTH_SCRIPT };
    let s = match init {
        InitPolicy::Invert => base.replace(" 01 ", " 0 ").replace(" 01\n", " 0\n"),
        _ => base.to_string(),
    };
    if map == MapMode::Words {
        let sel: Vec<String> = crate::wordmap::UNSUPPORTED.iter().map(|t| format!("t:{t}")).collect();
        return s.replace("#@TECHMAP@", &format!("techmap {}", sel.join(" ")));
    }
    let m = match map {
        MapMode::None => "",
        MapMode::AbcGates => "abc -g AND,NAND,OR,NOR,XOR,XNOR,ANDNOT,ORNOT,MUX",
        MapMode::AbcSop => "abc -sop -I 40 -P 80",
        MapMode::Words | MapMode::Auto => "",
    };
    s.replace("#@MAP@", m)
}

#[derive(Clone, Debug)]
pub struct FlowOptions {
    pub part: Option<String>,
    pub top: Option<String>,
    pub constraints: Constraints,
    /// Extra fitter options (`key value...`).
    pub fit_options: Vec<(String, Vec<String>)>,
    /// Product terms per cell (macrocell + one cascade neighbour) for inlining and splitting.
    pub max_products: usize,
    /// `max_products` was given explicitly (fewer strategies are tried).
    pub max_products_given: bool,
    /// Seconds after which no further fitting strategy is started.
    pub fit_time: u64,
    pub toggles: bool,
    pub emit_tt2: bool,
    pub svf: bool,
    pub svf_rev_c: bool,
    pub created: Option<String>,
    pub yosys: String,
    pub init: InitPolicy,
    /// Directory for the pre-fit `.tt2` (with `emit_tt2`), also on fit failure.
    pub debug_dir: Option<PathBuf>,
    /// Required register-to-register frequency (MHz): strategies are tried until one meets it
    /// (spec F8.5); the best one is kept otherwise, with a warning.
    pub fmax: Option<f64>,
    /// Speed grade for the timing report (`-7` … `-25`).
    pub grade: String,
    /// Optimisation goal (overrides the constraints file's `optimize`).
    pub goal: Option<Goal>,
}

impl Default for FlowOptions {
    fn default() -> Self {
        FlowOptions {
            part: None,
            top: None,
            constraints: Constraints::default(),
            fit_options: vec![],
            max_products: 10,
            max_products_given: false,
            fit_time: 120,
            toggles: true,
            emit_tt2: false,
            svf: true,
            svf_rev_c: false,
            created: None,
            yosys: "yosys".into(),
            init: InitPolicy::Error,
            debug_dir: None,
            fmax: None,
            grade: "-10".into(),
            goal: None,
        }
    }
}

/// Run Yosys: Verilog files → JSON netlist (`<out>/<top>.json`, log in `<out>/yosys.log`).
/// Run Yosys for each mapping `map` stands for (`auto`: gates and words), writing
/// `netlist.json` (gates, abc) and/or `netlist-words.json`.
pub fn synth_all(files: &[PathBuf], top: Option<&str>, out: &Path, yosys: &str, init: InitPolicy, map: MapMode) -> Result<Vec<(MapMode, PathBuf)>, String> {
    let modes = if map == MapMode::Auto { vec![MapMode::None, MapMode::Words] } else { vec![map] };
    // the Yosys runs are independent processes: run them side by side (spec F9.11)
    std::thread::scope(|s| {
        let hs: Vec<_> = modes.iter().map(|&m| s.spawn(move || synth(files, top, out, yosys, init, m).map(|p| (m, p)))).collect();
        hs.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("yosys thread panicked".into()))).collect()
    })
}

pub fn synth(files: &[PathBuf], top: Option<&str>, out: &Path, yosys: &str, init: InitPolicy, map: MapMode) -> Result<PathBuf, String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    // Yosys runs from the sources' common directory and reads them by relative path: its
    // internal names embed the path as given, and their sort order against path-free names
    // decides bit numbering and kept wire names, hence the fit (spec F9.7). Results then depend
    // on the file names only, not on how the command line spelled them.
    let abs = |p: &Path| std::fs::canonicalize(p).map_err(|e| format!("{}: {e}", p.display()));
    let out = abs(out)?;
    let out = out.as_path();
    let files_abs: Vec<PathBuf> = files.iter().map(|f| abs(f)).collect::<Result<_, _>>()?;
    let base: PathBuf = {
        let mut b = files_abs.first().and_then(|f| f.parent()).map(Path::to_path_buf).unwrap_or_default();
        while !files_abs.iter().all(|f| f.starts_with(&b)) {
            if !b.pop() {
                break;
            }
        }
        b
    };
    let rel: Vec<PathBuf> = files_abs.iter().map(|f| f.strip_prefix(&base).map(Path::to_path_buf).unwrap_or_else(|_| f.clone())).collect();
    let files = &rel[..];
    let words = map == MapMode::Words;
    let script = out.join(if words { "synth_atf15xx_words.ys" } else { "synth_atf15xx.ys" });
    std::fs::write(&script, synth_script(init, map)).map_err(|e| e.to_string())?;
    let json = out.join(if words { "netlist-words.json" } else { "netlist.json" });
    let reads: Vec<String> = files.iter().map(|f| format!("read_verilog -sv {};", shell_word(f))).collect();
    let hier = match top {
        Some(t) => format!("hierarchy -check -top {t};"),
        None => "hierarchy -check -auto-top;".into(),
    };
    let cmd = format!(
        "{} {hier} script {}; stat; write_json -compat-int {}",
        reads.join(" "),
        shell_word(&script),
        shell_word(&json)
    );
    let st = Command::new(yosys)
        .current_dir(&base)
        .args(["-q", "-l"])
        .arg(out.join(if words { "yosys-words.log" } else { "yosys.log" }))
        .arg("-p")
        .arg(&cmd)
        .status()
        .map_err(|e| format!("cannot run {yosys}: {e} (run inside docker/yosys/run.sh, or pass --yosys)"))?;
    if !st.success() {
        return Err(format!("yosys failed (see {})", out.join("yosys.log").display()));
    }
    Ok(json)
}

fn shell_word(p: &Path) -> String {
    let s = p.display().to_string();
    if s.contains(|c: char| c.is_whitespace() || c == ';') {
        format!("\"{s}\"")
    } else {
        s
    }
}

/// Everything a run produces.
pub struct Outputs {
    pub top: String,
    pub part: Part,
    pub result: FitResult,
    pub jed: String,
    pub svf: Option<Vec<u8>>,
    pub tt2: Option<String>,
    pub fit_report: String,
    pub pin_report: String,
    /// JSON: Verilog port bit -> pin, plus fitter names (for equivalence checkers).
    pub pins_json: String,
    /// `<top>.map.json`: netlist/source name -> macrocell, Invert fuse etc. (`atf15xx-map/1`).
    pub map_json: String,
    pub warnings: Vec<String>,
    pub summary: String,
    /// Static timing report (`<top>.timing.rpt`, spec F8.5).
    pub timing: String,
    /// fmax (MHz) of the fit, and whether it meets the timing targets (None: none given).
    pub fmax_mhz: Option<f64>,
    pub timing_met: Option<bool>,
    /// The optimisation goal the fit was chosen for.
    pub goal: Goal,
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

/// Netlist JSON → fitted design and outputs.
pub fn fit_json(json_text: &str, o: &FlowOptions, cmdline: &str) -> Result<Outputs, String> {
    fit_netlist(&crate::ynet::Netlist::from_json(json_text)?, o, cmdline)
}

/// [`fit_json`] from a typed netlist (a JSON file, or RTLIL through the Yosys plugin).
pub fn fit_netlist(netlist: &crate::ynet::Netlist, o: &FlowOptions, cmdline: &str) -> Result<Outputs, String> {
    // a word-level netlist (--map words, the plugin): lower its coarse cells first (F9.9)
    let lowered;
    let netlist = if crate::wordmap::has_coarse(netlist) {
        let mut n = netlist.clone();
        crate::wordmap::lower(&mut n)?;
        lowered = n;
        &lowered
    } else {
        netlist
    };
    let mut imp = yosys::import_netlist(netlist, o.top.as_deref())?;
    let mut warnings = std::mem::take(&mut imp.warnings);
    let ones: Vec<&String> = imp.inits.iter().filter(|(_, v)| **v).map(|(n, _)| n).collect();
    if !ones.is_empty() {
        let list = ones.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
        match o.init {
            InitPolicy::Error => {
                return Err(format!(
                    "initial value 1 on {list}: registers power up at 0 in this flow; remove the initial value, \
                     reset the register explicitly, or pass --init ignore"
                ))
            }
            InitPolicy::Ignore => warnings.push(format!("initial value 1 ignored (power-up 0) on {list}")),
            InitPolicy::Invert => {
                return Err(format!("initial value 1 left on {list}: run Yosys with `atf15xx-synth script --init invert`"))
            }
        }
    }
    let part_name = o
        .part
        .clone()
        .or_else(|| o.constraints.device.clone())
        .ok_or("no device given (--device ATF1504AS-PLCC44, or `device` in the constraints file)")?;
    let part = crate::part::parse(&part_name)?;
    // The open flow uses hardware features the vendor fitter leaves unused (spec F2.6, F4):
    // a free clock network for the other polarity of a clock pin instead of a PT clock.
    let mut opts = Options {
        device: Some(part.device.name.to_string()),
        share_clock_networks: true,
        pack_input_registers: true,
        observable_registers: true,
        xor_synthesis: true,
        xor_split: true,
        ..Options::default()
    };
    warnings.extend(o.constraints.apply(&mut imp.ports, &mut opts)?);
    for (k, v) in &o.fit_options {
        if !atf15xx_fit::cli::apply(&mut opts, k, v) {
            return Err(format!("unknown fitter option {k}"));
        }
    }
    opts.created = o.created.clone();
    // pins from attributes / constraints
    let mut d = std::mem::take(&mut imp.design);
    for p in &imp.ports {
        if let Some(v) = d.inputs.get_mut(&p.name) {
            *v = p.loc;
        }
        if let Some(i) = d.cell_index(&p.name) {
            d.cells[i].pin = p.loc;
        }
        if let Some(pin) = p.loc {
            if part.device.pin_mc(pin).is_none() && part.device.dedicated(pin).is_none() {
                return Err(format!("{}: pin {pin} is not an I/O or input pin of {}", p.verilog, part.device.name));
            }
        }
    }
    // optimisation (§F7.4) and fitting: a few strategies, the fit with the fewest macrocells wins
    // (max products, inline ties, clock-enable recovery, spread output pins); the first that fits wins
    // timing targets (spec F8.7): the constraints file's, with --fmax / --optimize on top
    let mut targets = o.constraints.timing.clone();
    if let Some(f) = o.fmax {
        targets.fmax = Some(f);
    }
    if let Some(g) = o.goal {
        targets.goal = g;
    }
    let wants_timing = targets.any() || targets.goal == Goal::Speed;
    let clock_targets: Vec<(String, String, f64)> = targets
        .clocks
        .iter()
        .map(|(v, f)| {
            imp.ports
                .iter()
                .find(|p| p.verilog == *v || p.name == *v)
                .map(|p| (v.clone(), p.name.clone(), *f))
                .ok_or_else(|| format!("timing constraint for unknown clock port {v}"))
        })
        .collect::<Result<_, _>>()?;
    let mp = o.max_products;
    let base = Strategy { mp, ties: true, enables: true, spread: false, seed: 0, vcollapse: false, toggles: o.toggles, special_foldback: opts.special_foldback, fold_one_level: false };
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
        .flat_map(|k| [Strategy { seed: k, ..s(mp, true, true, true) }, Strategy { seed: k, ..s(mp, true, true, false) }, Strategy { seed: k, ..s(5, true, true, true) }])
        .collect();
    // dense designs: when there are more cells than macrocells, also try the fitter's vendor-style
    // collapse of combinatorial nodes into their readers (what makes the .tt2 path fit such
    // designs: the wider sums are then factored with foldbacks, De Morgan, spec F4.6a)
    let n_mc = part.device.family.macrocells();
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
            let st = Strategy { special_foldback: sf || opts.special_foldback, toggles: tg && o.toggles, ..vc(true, false) };
            if wants_timing && sf {
                vec![Strategy { fold_one_level: true, ..st }, st]
            } else {
                vec![st]
            }
        })
        .collect();
    // with a timing target: also wider sums (fewer logic levels, longer cascades: tPEXP per
    // stage instead of tUIM + tLAD + tCOMB per level), tried only if the others miss it
    let wider: Vec<Strategy> = if wants_timing && !o.max_products_given {
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
        .filter(|st| !o.max_products_given || st.mp == mp)
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
    let mut best_fmax = f64::NEG_INFINITY;
    let mut best_key: Option<(bool, f64, f64)> = None;
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
        fit: Result<FitResult, atf15xx_fit::fit::FitError>,
    }
    let base = &d;
    type Prefix = std::sync::Arc<std::sync::OnceLock<(atf15xx_fit::netlist::Design, Vec<String>)>>;
    let prefix_cache: std::sync::Mutex<std::collections::HashMap<(usize, bool, bool, bool), Prefix>> = Default::default();
    let run = |st: Strategy, si: usize| -> Result<Outcome, String> {
        let t_pass = std::time::Instant::now();
        let Strategy { mp, ties, enables, spread, seed, vcollapse, toggles, special_foldback, fold_one_level } = st;
            // the netlist passes up to the split depend only on these knobs: computed once per
            // key and shared by the strategies (and threads) that need them (spec F9.11)
            let key = (mp, toggles, enables, ties);
            let slot = prefix_cache.lock().expect("prefix cache").entry(key).or_insert_with(|| std::sync::Arc::new(std::sync::OnceLock::new())).clone();
            let (pd, plog) = slot.get_or_init(|| {
                let mut d = base.clone();
                let mut log: Vec<String> = vec![];
                    if toggles {
                        log.push(format!("{} T flip-flops", opt::recover_toggles(&mut d, mp)));
                        opt::remove_dead(&mut d);
                        let chains = opt::factor_wide(&mut d);
                        if chains > 0 {
                            log.push(format!("{chains} carry-chain nodes"));
                        }
                    }
                    if enables {
                        log.push(format!("{} clock enables", opt::recover_enables(&mut d, mp)));
                        opt::remove_dead(&mut d);
                    }
                    if let (Some(n), Some(c)) = (std::env::var("ATF15XX_FLOW_TRACE").ok(), None::<()>.or(Some(()))) {
                        let _ = c;
                        if let Some(cell) = d.cell(&n) {
                            eprintln!("trace {n} before collapse: data {:?} ce {:?}", cell.data, cell.ce);
                        }
                    }
                    opt::collapse_limited(&mut d, mp, ties);
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
                (d, log)
            });
            let mut d = pd.clone();
            let mut log = plog.clone();
            let t = if o.emit_tt2 { Some(tt2out::write(&d, &part.device.tt2_device)?) } else { None };
            if vcollapse {
                // shared nodes feeding control functions become foldback signals first (the vendor's
                // XXL_ nodes): they stay shared instead of being expanded into every reader.
                // 1. flatten the gate trees into those nodes (collapse everything else into them)
                use atf15xx_fit::{logic::Signal as Sg, netlist::Storage as St};
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
                    if !c.is_output && c.storage == atf15xx_fit::netlist::Storage::Comb && c.node == Some(0) {
                        c.node = None;
                    }
                }
                d.collapse_comb_nodes();
                log.push("vendor-style node collapse".into());
            }
            d.absorb_output_registers();
            d.legalize_special_functions_with(special_foldback);
            if seed > 0 {
                shuffle(&mut d.cells, seed);
            }
            if spread {
                spread_pins(&mut d, part.device, &opts);
            }
            if std::env::var_os("ATF15XX_FLOW_DEBUG").is_some() {
                eprintln!("strategy {si}: max_products {mp}, ties {ties}, enables {enables}, spread {spread} ({st:?})");
                for c in &d.cells {
                    eprintln!(
                        "cell {:24} {:?} out={} products={} (complement {}) inputs={} ce={} ar={} ap={} oe={}",
                        c.name, c.storage, c.is_output, c.data.0.len(), c.data.complement().0.len(), c.inputs().len(),
                        c.ce.is_some(), c.ar.is_some(), c.ap.is_some(), c.oe.is_some()
                    );
                }
            }
            let (n_cells, n_buried) = (d.cells.len(), d.cells.iter().filter(|c| !c.is_output).count());
            let n_pins_in = d.inputs.len();
            let mut sopts = opts.clone();
            sopts.special_foldback = special_foldback;
            // a strategy may not run past the budget (the first one gets at least the whole budget)
            sopts.deadline = Some(t0 + std::time::Duration::from_secs(o.fit_time));
        let name = d.name.clone();
        let passes_s = t_pass.elapsed().as_secs_f64();
        let t_fit = std::time::Instant::now();
        let fit = fit_design(part.device, d, &sopts);
        if std::env::var_os("ATF15XX_FLOW_TIMING").is_some() {
            let ((mh, mm), (ch, cm)) = atf15xx_fit::logic::memo_stats();
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
        if si > 0 && t0.elapsed().as_secs() >= o.fit_time {
            warnings.push(format!("fit time budget of {} s used up after {si} strategies (--fit-time)", o.fit_time));
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
            if let (Some(t), Some(dir)) = (&t, &o.debug_dir) {
                // written before fitting, so that a failed fit can be inspected (and given to the
                // vendor fitter: <top>.vendor-cmd is its command line)
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join(format!("{name}.tt2")), t);
                let _ = std::fs::write(dir.join(format!("{name}.vendor-cmd")), vendor_cmd(&name, part.device, &opts));
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
                    let f = timing_of(&r, &o.grade)?.fmax_mhz.unwrap_or(f64::INFINITY);
                    if best.as_ref().map_or(true, |b| used < b.0 || (used == b.0 && f > best_area_fmax)) {
                        best_area_fmax = f;
                        best = Some((used, r, t, desc));
                    }
                } else {
                    // timing first (F8.7): a fit that meets the targets beats one that does not,
                    // even with more macrocells (e.g. one logic level wider instead of two)
                    let (ok, f, _) = judge(&r, &targets, &o.grade, &clock_targets)?;
                    let key = |ok: bool, f: f64, used: usize| -> (bool, f64, f64) {
                        if targets.goal == Goal::Speed { (ok, f, -(used as f64)) } else { (ok, -(used as f64), f) }
                    };
                    let k = key(ok, f, used);
                    if best_key.map_or(true, |b: (bool, f64, f64)| k > b) {
                        best_key = Some(k);
                        met = ok;
                        best_fmax = f;
                        best = Some((used, r, t, desc));
                    }
                }
            }
            Err(e) => {
                if std::env::var_os("ATF15XX_FLOW_DEBUG").is_some() {
                    eprintln!("strategy {si}: {e} ({n_cells} cells)");
                }
                // the fitter says why: out of macrocells, pins or PTs -> cell-reducing strategies next
                let msg = e.to_string();
                if !capacity_queued && ["no free macrocell", "no free output pin", "do not fit"].iter().any(|k| msg.contains(k)) {
                    capacity_queued = true;
                    for c in capacity.iter().rev() {
                        queue.push_front(*c);
                    }
                    capacity_pending = capacity.iter().copied().filter(|c| !tried.contains(c)).collect();
                }
                first_err.get_or_insert(format!(
                    "fit failed: {e} ({n_cells} cells, {n_buried} of them buried, {n_pins_in} inputs; {} macrocells)",
                    part.device.family.macrocells()
                ));
            }
        }
    }
    let Some((_, result, best_tt2, strategy)) = best else { return Err(first_err.unwrap()) };
    if best_tt2.is_some() {
        tt2 = best_tt2;
    }
    warnings.push(format!("fitted with {strategy}"));
    let tr = timing_of(&result, &o.grade)?;
    let names: std::collections::BTreeMap<u16, String> = result
        .placement
        .iter()
        .filter_map(|(n, p)| match p {
            Place::Pin { mc, .. } | Place::Node { mc } => Some((*mc, n.clone())),
            _ => None,
        })
        .collect();
    let timing = tr.text(&|mc| names.get(&mc).map_or(format!("MC{mc}"), |n| format!("{n} (MC{mc})")), 20);
    let _ = best_fmax;
    let timing_met = if targets.any() {
        let (ok, _, v) = judge(&result, &targets, &o.grade, &clock_targets)?;
        if !ok {
            warnings.push(format!("timing targets missed (speed grade {}): {}; see {}.timing.rpt", o.grade, v.join(", "), imp.top));
        }
        Some(ok)
    } else {
        None
    };
    let created = o.created.clone().unwrap_or_else(|| chrono::Local::now().format("%a %b %d %H:%M:%S %Y").to_string());
    let jed_text = jed::write(&result.fuses, &created, part.device.pin_count());
    let fit_report = report::fit_report(&result, cmdline);
    let pin_report = report::pin_report(&result);
    let mut map = report::map_json(&result, Some(&imp.origins));
    // with --init invert, Yosys may store registers inverted (the source value is then !Q)
    map["init_policy"] = serde_json::json!(format!("{:?}", o.init).to_lowercase());
    let map_json = serde_json::to_string_pretty(&map).unwrap();
    // pin map for checkers
    let mut pins = serde_json::Map::new();
    for p in &imp.ports {
        let place = result.placement.iter().find(|(n, _)| n == &p.name).map(|(_, pl)| pl.clone());
        let pin = match place {
            Some(Place::Pin { pin, .. }) | Some(Place::Dedicated { pin, .. }) => Some(pin),
            _ => None,
        };
        pins.insert(
            p.verilog.clone(),
            serde_json::json!({ "name": p.name, "dir": p.dir, "pin": pin }),
        );
    }
    // pad hazards (spec F2.9): floating dedicated inputs with the keeper off; JTAG disabled
    {
        let used: Vec<u16> = result
            .placement
            .iter()
            .filter_map(|(_, pl)| match pl {
                Place::Pin { pin, .. } | Place::Dedicated { pin, .. } => Some(*pin),
                _ => None,
            })
            .collect();
        let dev = part.device;
        if !opts.pin_keep {
            let free: Vec<String> = [(dev.gclk1, "GCLK1"), (dev.gclr, "GCLR"), (dev.oe1, "OE1"), (dev.gclk2_oe2, "GCLK2/OE2")]
                .iter()
                .filter(|(p, _)| !used.contains(p))
                .map(|(p, n)| format!("{p} ({n})"))
                .collect();
            if !free.is_empty() {
                warnings.push(format!(
                    "bus keeper off: unused dedicated input pin(s) {} will float; tie them high or low on the board (floating inputs caused errors on silicon, spec F2.9)",
                    free.join(", ")
                ));
            }
        }
        if !opts.jtag {
            warnings.push("JTAG is disabled (jtag off): the JTAG pins become I/O and the device can then only be reprogrammed with 12 V on OE1; tools/silicon/program.sh refuses such images".to_string());
        }
    }
    let pins_json = serde_json::to_string_pretty(&serde_json::json!({
        "top": imp.top, "device": part.device.name, "chip": part.chip, "ports": pins
    }))
    .unwrap();
    let svf = if o.svf { Some(make_svf(&part.chip, jed_text.as_bytes(), o.svf_rev_c, &created)?) } else { None };
    let used = macrocells_used(&result);
    let total = part.device.family.macrocells();
    let summary = format!(
        "{}: {} on {} — {} of {} macrocells used, {}, {}; Yosys cells: {}",
        imp.top,
        part.chip,
        part.device.package,
        used,
        total,
        tr.fmax_mhz.map_or("no register-to-register path".to_string(), |f| format!("fmax {f:.1} MHz at {}", o.grade)),
        strategy,
        imp.cell_stats.iter().map(|(k, v)| format!("{k}×{v}")).collect::<Vec<_>>().join(" ")
    );
    Ok(Outputs {
        top: imp.top,
        part,
        result,
        jed: jed_text,
        svf,
        tt2,
        fit_report,
        pin_report,
        pins_json,
        map_json,
        warnings,
        summary,
        timing,
        fmax_mhz: tr.fmax_mhz,
        timing_met,
        goal: targets.goal,
    })
}

/// The vendor fitter command line equivalent to `opts` for `<top>.tt2` (oracle runs).
pub fn vendor_cmd(top: &str, dev: &atf15xx_arch::Device, opts: &Options) -> String {
    use atf15xx_fit::fit::Select;
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
fn spread_pins(d: &mut atf15xx_fit::netlist::Design, dev: &atf15xx_arch::Device, opts: &Options) {
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

/// `.jed` → SVF (Program/Verify) with atf15xx-tool's ISP module, as ATMISP 7.3 would write it.
pub fn make_svf(chip: &str, jed_bytes: &[u8], rev_c: bool, created: &str) -> Result<Vec<u8>, String> {
    use atf15xx_tool::isp::chain::{Chain, Device};
    use atf15xx_tool::isp::device::Instr;
    use atf15xx_tool::isp::generate::{generate, GenOptions};
    use atf15xx_tool::isp::svf::{Rev, SvfOptions};
    let chain = Chain {
        calibration: 1,
        devices: vec![Device { name: chip.to_string(), ir_width: 10, idcode: true, instr: Instr::ProgramVerify, jed_path: "design.jed".into() }],
    };
    let opts = GenOptions {
        svf: SvfOptions { rev: if rev_c { Rev::C } else { Rev::D }, tck: 1, state_reset: true, strict: false },
        allow_mixed_voltage: false,
        ctime: created.to_string(),
        load_output: false,
    };
    let bytes = jed_bytes.to_vec();
    let out = generate(&chain, |_, _| Ok(bytes.clone()), &opts, Default::default()).map_err(|e| format!("svf: {e}"))?;
    Ok(out.svf)
}

/// Write the outputs into `dir` as `<top>.jed`, `.svf`, `.fit`, `.pin`, `.pins.json` (and `.tt2`).
pub fn write_outputs(o: &Outputs, dir: &Path) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut files = vec![];
    let mut put = |ext: &str, data: &[u8]| -> Result<(), String> {
        let p = dir.join(format!("{}.{ext}", o.top));
        std::fs::write(&p, data).map_err(|e| format!("{}: {e}", p.display()))?;
        files.push(p);
        Ok(())
    };
    put("jed", o.jed.as_bytes())?;
    if let Some(s) = &o.svf {
        put("svf", s)?;
    }
    put("fit", o.fit_report.as_bytes())?;
    put("pin", o.pin_report.as_bytes())?;
    put("pins.json", o.pins_json.as_bytes())?;
    put("map.json", o.map_json.as_bytes())?;
    put("timing.rpt", o.timing.as_bytes())?;
    if let Some(t) = &o.tt2 {
        put("tt2", t.as_bytes())?;
    }
    Ok(files)
}
