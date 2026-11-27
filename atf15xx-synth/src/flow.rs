//! The flow driver: Verilog → Yosys → netlist → fitter → `.jed` / `.svf` / reports.

use crate::constraints::{Constraints, Goal};
use crate::part::Part;
use crate::yosys;
use atf15xx_fit::fit::{FitResult, Options, Place};
use atf15xx_fit::search::{fit_key, judge, macrocells_used, timing_of, SearchOptions};
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

/// Is fit `b` better than fit `a` (spec F8.7, F9.9)? The strategy search's ranking
/// ([`fit_key`]) between the flow's alternatives (piece forms, mappings): meeting the timing
/// targets first; if both miss them, the one that misses by less (see [`judge`]); then with
/// the area goal fewer macrocells, then fmax; with the speed goal fmax, then macrocells.
pub fn better(a: &Outputs, b: &Outputs) -> bool {
    let key = |o: &Outputs| {
        fit_key(
            o.goal,
            o.timing_met.unwrap_or(true),
            o.timing_shortfall.unwrap_or(0.0),
            macrocells_used(&o.result),
            o.fmax_mhz.unwrap_or(f64::INFINITY),
        )
    };
    key(b) > key(a)
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
pub fn synth_all(
    files: &[PathBuf],
    top: Option<&str>,
    out: &Path,
    yosys: &str,
    init: InitPolicy,
    map: MapMode,
) -> Result<Vec<(MapMode, PathBuf)>, String> {
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
    let cmd = format!("{} {hier} script {}; stat; write_json -compat-int {}", reads.join(" "), shell_word(&script), shell_word(&json));
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
    /// The worst relative shortfall against the targets (0 when met; None without targets).
    pub timing_shortfall: Option<f64>,
    /// The optimisation goal the fit was chosen for.
    pub goal: Goal,
}

/// Netlist JSON → fitted design and outputs.
pub fn fit_json(json_text: &str, o: &FlowOptions, cmdline: &str) -> Result<Outputs, String> {
    fit_netlist(&crate::ynet::Netlist::from_json(json_text)?, o, cmdline)
}

/// [`fit_json`] from a typed netlist (a JSON file, or RTLIL through the Yosys plugin).
pub fn fit_netlist(netlist: &crate::ynet::Netlist, o: &FlowOptions, cmdline: &str) -> Result<Outputs, String> {
    use crate::wordmap;
    if !wordmap::has_coarse(netlist) {
        return fit_lowered(netlist, o, cmdline);
    }
    // a word-level netlist (--map words, the plugin): lower its coarse cells first (F9.9); wide
    // compares in both piece forms, the better fit kept (F9.12; the first on a tie)
    let forms: &[wordmap::ChunkForm] =
        if wordmap::has_wide_compare(netlist) { &[wordmap::CHUNK_XOR4, wordmap::CHUNK_FLAT3] } else { &[wordmap::CHUNK_XOR4] };
    use rayon::prelude::*;
    let fits: Vec<Result<Outputs, String>> = forms
        .par_iter()
        .map(|&form| {
            let mut n = netlist.clone();
            wordmap::lower(&mut n, form)?;
            fit_lowered(&n, o, cmdline)
        })
        .collect();
    let mut best: Option<(usize, Outputs)> = None;
    let mut first_err = None;
    for (k, fit) in fits.into_iter().enumerate() {
        match fit {
            Ok(r) => {
                if best.as_ref().map_or(true, |(_, b)| better(b, &r)) {
                    best = Some((k, r));
                }
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    match best {
        Some((k, mut r)) => {
            if forms.len() > 1 {
                r.warnings.push(format!("wide compares built as {:?} (of {} forms fitted)", forms[k], forms.len()));
            }
            Ok(r)
        }
        None => Err(first_err.unwrap_or_else(|| "no fit".into())),
    }
}

/// [`fit_netlist`] after the word-level lowering: import, strategies, outputs.
fn fit_lowered(netlist: &crate::ynet::Netlist, o: &FlowOptions, cmdline: &str) -> Result<Outputs, String> {
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
    atf15xx_fit::search::check_bounds(&imp.design, part.device, &opts, &part_name)?;
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
    let so = SearchOptions {
        max_products: o.max_products,
        max_products_given: o.max_products_given,
        fit_time: o.fit_time,
        toggles: o.toggles,
        emit_tt2: o.emit_tt2,
        debug_dir: o.debug_dir.clone(),
        grade: o.grade.clone(),
        targets,
        clocks: clock_targets,
    };
    let found = atf15xx_fit::search::search(part.device, d, &opts, &so)?;
    let (targets, clock_targets) = (so.targets, so.clocks);
    warnings.extend(found.warnings);
    let (result, tt2, strategy) = (found.result, found.tt2, found.strategy);
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
    let (timing_met, timing_shortfall) = if targets.any() {
        let (ok, _, v, short) = judge(&result, &targets, &o.grade, &clock_targets)?;
        if !ok {
            warnings.push(format!("timing targets missed (speed grade {}): {}; see {}.timing.rpt", o.grade, v.join(", "), imp.top));
        }
        (Some(ok), Some(short))
    } else {
        (None, None)
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
        pins.insert(p.verilog.clone(), serde_json::json!({ "name": p.name, "dir": p.dir, "pin": pin }));
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
        timing_shortfall,
        goal: targets.goal,
    })
}

/// `.jed` → SVF (Program/Verify) with atf15xx-tool's ISP module, as ATMISP 7.3 would write it.
pub fn make_svf(chip: &str, jed_bytes: &[u8], rev_c: bool, created: &str) -> Result<Vec<u8>, String> {
    use atf15xx_tool::isp::chain::{Chain, Device};
    use atf15xx_tool::isp::device::Instr;
    use atf15xx_tool::isp::generate::{generate, GenOptions};
    use atf15xx_tool::isp::svf::{Rev, SvfOptions};
    let chain = Chain {
        calibration: 1,
        devices: vec![Device {
            name: chip.to_string(),
            ir_width: 10,
            idcode: true,
            instr: Instr::ProgramVerify,
            jed_path: "design.jed".into(),
        }],
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
