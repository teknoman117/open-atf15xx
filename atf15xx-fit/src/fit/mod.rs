//! The fitter (spec F4): placement, global-net assignment, product-term allocation, UIM routing
//! and fuse assembly. This module holds the options, results and errors, the attempt driver
//! ([`fit_design`], [`fit_attempt`]) and the state passed between the passes; each pass has its
//! own module, in pipeline order:
//!
//! | pass | module | state out |
//! |---|---|---|
//! | placement | [`place`] (with [`partition`], [`positions`]) | `Placed` |
//! | global networks | [`nets`] | `Nets` |
//! | product-term plans | [`pts`] | `Vec<PtPlan>` |
//! | foldback expanders | [`fold`] | `Folds` |
//! | UIM routing | [`uim`] | `Routing` |
//! | fuse assembly | [`encode`] | [`FitResult`] |
//!
//! [`bound`] exports a fit's block-assignment problem for the exact bound (F4.6c). The capacity
//! numbers all passes share are in [`crate::capacity`].

use crate::jed::Fuses;
use crate::logic::{Signal, Sop};
use crate::netlist::{Cell, Design, Storage};
use crate::tt2::Pla;
use atf15xx_arch::{ConfigBit, Dedicated, Device, Family, McBit, Source};
use std::collections::{BTreeMap, BTreeSet};

mod bound;
mod encode;
mod fold;
mod nets;
mod partition;
mod place;
mod positions;
mod pts;
mod uim;
pub use self::bound::{block_problem, BlockItem, BlockProblem};
pub(crate) use self::pts::factor_with_foldback;
pub use self::uim::{route_block, route_block_alt};
use self::{encode::*, fold::*, nets::*, partition::*, place::*, positions::*, pts::*, uim::*};

/// A per-signal option: off, on for everything, or on for the listed names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Select {
    #[default]
    Off,
    All,
    Only(Vec<String>),
}

impl Select {
    pub fn contains(&self, name: &str) -> bool {
        match self {
            Select::Off => false,
            Select::All => true,
            Select::Only(v) => v.iter().any(|x| x.eq_ignore_ascii_case(name)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preassign {
    #[default]
    Try,
    Keep,
    Ignore,
}

/// Fitter options; defaults are the vendor fitter's defaults.
#[derive(Clone, Debug)]
pub struct Options {
    pub device: Option<String>,
    pub preassign: Preassign,
    pub jtag: bool,
    pub pin_keep: bool,
    pub output_fast: Select,
    pub open_collector: Select,
    pub mc_power: Select,
    pub security: bool,
    pub pd1: bool,
    pub pd2: bool,
    pub power_reset: bool,
    pub tdi_pullup: bool,
    pub tms_pullup: bool,
    /// Input transition detection on GCLK1..3 pins.
    pub itd: [bool; 3],
    pub cascade: bool,
    /// Allow a combinatorial pin output and a buried register in one macrocell (`logic_doubling`).
    pub logic_doubling: bool,
    pub foldback: bool,
    /// Drive a free GCLK network from an already-used clock pin with the opposite polarity instead
    /// of giving the minority-polarity cells a PT clock (each network has its own source mux and
    /// invert bit, silicon-verified on the ATF1508AS). Off by default: the vendor never does this,
    /// so the vendor-compatible fit150x binaries keep byte identity.
    pub share_clock_networks: bool,
    /// Implement multi-product special functions (clock, CE, AR, AP, OE) with two foldback levels
    /// (OR of products = NAND of the products' NANDs) instead of a buried node: saves the node's
    /// macrocell and UIM input, costs k + 1 foldback PTs in the block and 2·tSEXP.
    pub special_foldback: bool,
    /// Pack a register whose D is an input pin into that pin's own macrocell, D taken straight
    /// from the pad (D/Q group: d_mux fast, dfast pad; spec F2.3): no PT and no UIM input for D,
    /// fast setup (tFSU), and the macrocell's PTs stay free for its cascade/foldback.
    pub pack_input_registers: bool,
    /// Buried registers drive their macrocell's output mux from the register (`OMux` = 0; the OE
    /// is off, so the pin is unaffected): the boundary-scan output cell then captures Q, and
    /// JTAG SAMPLE reads every register's state (`atf15xx-tool sample`). Off by default
    /// (the vendor writes `OMux` = 1 there).
    pub observable_registers: bool,
    /// The vendor's `-XOR_Synthesis on`: a combinatorial function that is the XOR/XNOR of two
    /// literals uses the macrocell XOR (PT1 = A, PT2 = B). Off by default, as in the vendor.
    pub xor_synthesis: bool,
    /// Ours: general XOR synthesis (spec F4.6b): a function becomes `g ⊕ p` with `p` in PT2
    /// when that needs fewer PTs. Off by default; the flow enables it.
    pub xor_split: bool,
    /// Text for the JEDEC `Created:` line (default: current local time in ctime format).
    pub created: Option<String>,
    /// Give up (with the last error) once this instant has passed (no limit by default).
    pub deadline: Option<std::time::Instant>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            device: None,
            preassign: Preassign::Try,
            jtag: true,
            pin_keep: true,
            output_fast: Select::Off,
            open_collector: Select::Off,
            mc_power: Select::Off,
            security: false,
            pd1: false,
            pd2: false,
            power_reset: false,
            tdi_pullup: false,
            tms_pullup: false,
            itd: [true; 3],
            cascade: true,
            logic_doubling: false,
            foldback: true,
            xor_synthesis: false,
            xor_split: false,
            observable_registers: false,
            pack_input_registers: false,
            special_foldback: false,
            share_clock_networks: false,
            created: None,
            deadline: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{msg}")]
pub struct FitError {
    /// What the failure is about, set where it is raised (spec F9.11).
    pub kind: FitErrorKind,
    /// The message for the user.
    pub msg: String,
}

/// What a fit failure is about (spec F9.11): lets callers prune their search instead of
/// matching message text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitErrorKind {
    /// Out of macrocells, output pins, foldback hosts or product terms: only fewer cells
    /// (packing, logic doubling, other netlist transforms) can help, not another placement.
    Capacity,
    /// Out of input pins (more pins than the package has): no placement or packing helps.
    InputPins,
    /// A block's UIM inputs could not be matched onto its 40 multiplexers.
    Routing,
    /// A wide cell's cascade neighbour is taken.
    Cascade,
    /// A cell reads more signals than a block can deliver, or another netlist limit.
    Netlist,
    Other,
}

impl FitError {
    pub fn new(kind: FitErrorKind, msg: impl Into<String>) -> FitError {
        FitError { kind, msg: msg.into() }
    }
    pub fn kind(&self) -> FitErrorKind {
        self.kind
    }
    /// Capacity of any kind (macrocells, pins, product terms): another placement of the same
    /// cells cannot help.
    pub fn is_capacity(&self) -> bool {
        matches!(self.kind, FitErrorKind::Capacity | FitErrorKind::InputPins)
    }
}

fn err<T>(kind: FitErrorKind, m: impl Into<String>) -> Result<T, FitError> {
    Err(FitError::new(kind, m))
}

/// Where a signal ended up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// I/O pin `pin` on macrocell `mc`.
    Pin { pin: u16, mc: u16 },
    /// Dedicated input pin.
    Dedicated { pin: u16, func: Dedicated },
    /// Buried macrocell (feedback node `600 + mc`).
    Node { mc: u16 },
}

#[derive(Clone, Debug)]
pub struct FitResult {
    pub device: &'static Device,
    pub fuses: Fuses,
    pub placement: Vec<(String, Place)>,
    /// Per block: `(mux, source name, vendor source reference)`.
    pub routing: BTreeMap<usize, Vec<(usize, String, String)>>,
    pub design: Design,
}

/// How each macrocell is used after placement.
#[derive(Clone, Debug, Default)]
struct McUse {
    cell: Option<usize>,
    /// Buried register sharing this macrocell with its combinatorial output (logic doubling).
    hosted: Option<usize>,
    /// The pad is read (input pin, JTAG pin, or `.PIN` of an output).
    pad_used: bool,
    /// Lends its product terms to the next macrocell (cascade).
    helper_for: Option<u16>,
}

/// Mapped product-term plan of one cell.
#[derive(Clone, Debug, Default)]
struct PtPlan {
    sum: Vec<Vec<(Signal, bool)>>,
    invert: bool,
    /// PT index (1..=5) -> product, for this MC; cascaded products go to the helper.
    pts: BTreeMap<usize, Vec<(Signal, bool)>>,
    helper_pts: BTreeMap<usize, Vec<(Signal, bool)>>,
    in_sum: BTreeSet<usize>,
    clk: ClkUse,
    ce_pt: bool,
    ar_pt: bool,
    ar_global: bool,
    ap_pt: bool,
    oe: OeUse,
    constant: Option<bool>,
    /// PT1 of this macrocell hosts a foldback expander for its block.
    fold_host: bool,
    /// Buried register sharing the macrocell (logic doubling).
    hosted: Option<Hosted>,
    /// Input register: D straight from this macrocell's own pad (no D product).
    fast_in: bool,
    /// XOR mode: PT2 (this product) goes to the XOR, `x = sum ⊕ pt2` (`Pt2Mux` = `Invert` = 1).
    xor: bool,
}

#[derive(Clone, Debug)]
struct Hosted {
    clk: ClkUse,
    ar_pt: bool,
    ar_global: bool,
    ce_pt: bool,
}

impl From<PtPlan> for Hosted {
    fn from(p: PtPlan) -> Hosted {
        Hosted { clk: p.clk, ar_pt: p.ar_pt, ar_global: p.ar_global, ce_pt: p.ce_pt }
    }
}

/// Can this cell be the buried register of a doubled macrocell (D from PT2)?
fn doubling_register_ok(c: &Cell) -> bool {
    let single = |s: &Option<Sop>| s.as_ref().map_or(true, |x| x.0.len() <= 1);
    let t_ok = c.storage == Storage::Tff && c.ce.is_none();
    !c.is_output
        && (matches!(c.storage, Storage::Dff | Storage::Latch) || t_ok)
        && c.oe.is_none()
        && c.ap.is_none()
        && c.data.0.len() == 1
        && !c.active_low
        && !c.data.is_one()
        && c.clk.as_ref().map_or(false, |k| matches!(k.single_literal(), Some((Signal::Pad(_), true))))
        && c.ar.as_ref().map_or(true, |a| a.0.len() <= 2) // global clear + one PT
        && single(&c.ce)
}

/// A PT slot (2..=5) to which a cell's PT1 product can move, freeing PT1 for a foldback expander.
fn fold_movable(pl: &PtPlan) -> Option<usize> {
    if pl.fold_host
        || pl.hosted.is_some()
        || !pl.helper_pts.is_empty()
        || pl.pts.get(&1).map_or(false, |p| p.iter().any(|(s, _)| matches!(s, Signal::Fold(_))))
    {
        return None;
    }
    if pl.pts.contains_key(&1) && !pl.in_sum.contains(&1) {
        return None;
    }
    (2..=5).find(|k| !pl.pts.contains_key(k))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ClkUse {
    #[default]
    None,
    Global(u8),
    Pt,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum OeUse {
    /// Buried macrocell.
    #[default]
    Off,
    Always,
    Pt,
    Global(u8),
}

pub fn fit(family: Family, pla: &Pla, opts: &Options) -> Result<FitResult, FitError> {
    let dev_name = opts.device.clone().unwrap_or_else(|| pla.device.clone());
    let dev = Device::by_name(&dev_name).ok_or_else(|| FitError::new(FitErrorKind::Other, format!("unknown device {dev_name}")))?;
    if dev.family != family {
        return err(FitErrorKind::Other, format!("device {} is not an ATF{}AS", dev.name, family.number()));
    }
    let mut design = Design::from_pla(pla).map_err(|m| FitError::new(FitErrorKind::Other, m))?;
    design.collapse_comb_nodes();
    design.absorb_output_registers();
    design.legalize_special_functions_with(opts.special_foldback);
    fit_design(dev, design, opts)
}

/// Distinct UIM inputs a block can route to its product terms (all families).
pub const UIM_INPUTS_PER_BLOCK: usize = 40;

pub use crate::capacity::MAX_CHAIN_PTS;
use crate::capacity::{factored_len, helper_slot, helpers_for, pt_demand, COMPLEMENT_CHOICE_CUBES, MAX_CHAIN_MCS, PTS_PER_MC};

/// Cells whose product terms would need more signals than a block's UIM delivers can never fit:
/// report them instead of trying (and instead of complementing huge functions).
pub fn check_support(design: &Design) -> Result<(), FitError> {
    for c in &design.cells {
        let n = c.inputs().iter().filter(|s| !matches!(s, Signal::Fold(_))).count();
        if n > UIM_INPUTS_PER_BLOCK {
            return Err(FitError::new(FitErrorKind::Netlist, format!(
                "{}: its functions depend on {n} distinct signals, but a macrocell's product terms see at most the {UIM_INPUTS_PER_BLOCK} UIM inputs of its block (split the logic into buried nodes)",
                c.name
            )));
        }
    }
    Ok(())
}

/// Fit a netlist. Placement is retried with tighter per-block fan-in budgets when routing fails.
pub fn fit_design(dev: &'static Device, design: Design, opts: &Options) -> Result<FitResult, FitError> {
    check_support(&design)?;
    let mut design = design;
    design.find_xor_splits(opts.xor_synthesis || opts.xor_split, opts.xor_split);
    // fail fast (spec F9.11): inside the fitter only logic doubling makes cells share a
    // macrocell, and it needs an eligible register and a combinatorial output to host it, so
    // cells - min(eligible registers, hosts) macrocells are needed at least
    {
        let nmc = dev.family.macrocells();
        let regs = design.cells.iter().filter(|c| doubling_register_ok(c)).count();
        let hosts = design.cells.iter().filter(|c| c.is_output && c.storage == Storage::Comb).count();
        let need = design.cells.len().saturating_sub(regs.min(hosts));
        if need > nmc {
            return err(
                FitErrorKind::Capacity,
                format!(
                    "no free macrocell: {} cells need at least {need} macrocells (at most {} pairs can share one), the device has {nmc}",
                    design.cells.len(),
                    regs.min(hosts)
                ),
            );
        }
    }
    let mut last = None;
    let mut avoid: BTreeSet<(String, u16)> = BTreeSet::new();
    let mut repairs = 0;
    // repair progress (ATF_FIT_REPAIR_LOG=FILE records it per fit): the fewest distinct inputs a
    // failing block needed so far, and the longest run of repairs without improving on it
    let (mut best_need, mut stale, mut max_stale) = (usize::MAX, 0usize, 0usize);
    let t_fit = std::time::Instant::now();
    let log_repairs = |ok: bool, repairs: usize, max_stale: usize| {
        if let Some(path) = std::env::var_os("ATF_FIT_REPAIR_LOG") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(
                    f,
                    "{} repairs={repairs} max_stale={max_stale} s={:.2}",
                    if ok { "ok" } else { "fail" },
                    t_fit.elapsed().as_secs_f64()
                );
            }
        }
    };
    for _split_round in 0..200 {
        let mut cascade_victim = None;
        let mut retry = false;
        let mut skip_limits_for: Option<usize> = None;
        for (limit, extra) in [(40usize, 0usize), (36, 0), (32, 0), (40, 4), (36, 8), (32, 12), (28, 16), (24, 24), (20, 32)] {
            if skip_limits_for == Some(extra) {
                continue;
            }
            if let (Some(t), Some(_)) = (opts.deadline, &last) {
                if std::time::Instant::now() >= t {
                    return Err(last.unwrap());
                }
            }
            let ta = std::time::Instant::now();
            // first the vendor-like greedy placement (keeps byte identity where it works); if
            // that runs out of macrocells or UIM inputs, the same budget with the partitioner
            // then the partitioner (with the UIM position search), only if the greedy placement
            // failed: keeps the vendor's placements byte-identical
            let mut attempt = fit_attempt(dev, design.clone(), opts, limit, extra, &avoid, Placer::Greedy);
            if matches!(&attempt, Err(e) if e.kind() == FitErrorKind::Routing || e.is_capacity()) {
                if let Ok(r) = fit_attempt(dev, design.clone(), opts, limit, extra, &avoid, Placer::Partition) {
                    attempt = Ok(r);
                }
            }
            if std::env::var_os("ATF_FIT_DEBUG").is_some() {
                eprintln!(
                    "fit attempt (limit {limit}, extra {extra}): {:.2} s: {}",
                    ta.elapsed().as_secs_f64(),
                    attempt.as_ref().err().map_or("ok".into(), |e| e.msg.chars().take(80).collect::<String>())
                );
            }
            match attempt {
                Ok(r) => {
                    log_repairs(true, repairs, max_stale);
                    return Ok(r);
                }
                Err(e) if e.kind() == FitErrorKind::Routing && e.msg.contains("[movable: ") && repairs < 96 => {
                    let need = e
                        .msg
                        .split(" (")
                        .nth(1)
                        .and_then(|x| x.split(' ').next())
                        .and_then(|x| x.parse::<usize>().ok())
                        .unwrap_or(usize::MAX);
                    if need < best_need {
                        best_need = need;
                        stale = 0;
                    } else {
                        stale += 1;
                        max_stale = max_stale.max(stale);
                    }
                    // move one floating node out of the congested block and try again
                    let blk = e.msg.split("block ").nth(1).and_then(|x| x.chars().next()).unwrap();
                    let b = (blk as u8 - b'A') as u16;
                    let list = e.msg.split("[movable: ").nth(1).unwrap().trim_end_matches(']');
                    // never push a cell out of every block but one (it would have nowhere left
                    // when that block fills up)
                    let nblk = (dev.family.macrocells() / 16) as usize;
                    let avoided = |n: &str| avoid.iter().filter(|(m, _)| m == n).count();
                    if let Some(n) =
                        list.split(',').find(|n| !n.is_empty() && !avoid.contains(&(n.to_string(), b)) && avoided(n) + 3 <= nblk)
                    {
                        avoid.insert((n.to_string(), b));
                        repairs += 1;
                        last = Some(e);
                        retry = true;
                        break;
                    }
                    last = Some(e);
                }
                Err(e) if e.kind() == FitErrorKind::Cascade => {
                    cascade_victim = e.msg.split(':').next().map(String::from);
                    last = Some(e);
                    break;
                }
                Err(e) if e.is_capacity() => {
                    // a tighter fan-in limit cannot create macrocells: skip to more doubling
                    skip_limits_for = Some(extra);
                    last = Some(e);
                }
                Err(e) if e.kind() == FitErrorKind::Routing => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        if retry {
            continue;
        }
        // a cell whose cascade neighbour is taken: move part of its sum into a buried helper node
        match cascade_victim.and_then(|n| design.split_sum(&n)) {
            Some(()) => continue,
            None => break,
        }
    }
    log_repairs(false, repairs, max_stale);
    Err(last.unwrap())
}

/// Placement strategy of one fit attempt (spec F4.6a).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placer {
    /// The vendor-like greedy placement (keeps byte identity).
    Greedy,
    /// Block assignment by the partitioner, then greedy within the blocks, then the UIM-aware
    /// position search (kept only if it makes every block routable).
    Partition,
}

fn fit_attempt(
    dev: &'static Device,
    mut design: Design,
    opts: &Options,
    fanin_limit: usize,
    extra_pairs: usize,
    avoid: &BTreeSet<(String, u16)>,
    placer: Placer,
) -> Result<FitResult, FitError> {
    uim_memo_clear();
    // the passes of one attempt (spec F4): placement, global networks, product-term plans,
    // foldback expanders, UIM routing, fuse assembly
    let mut placed = place_cells(dev, &mut design, opts, fanin_limit, extra_pairs, avoid, placer)?;
    let mut nets = global_nets(dev, &design, opts, &placed);
    let (mut plans, fold_products) = plan_pts(dev, &design, opts, &mut placed, &mut nets)?;
    let folds = host_foldbacks(&design, opts, &mut placed, &mut plans, &fold_products)?;
    let routing = route_uim(dev, &design, &placed, &plans, &folds)?;
    assemble(dev, design, opts, placed, &nets, &plans, &folds, routing)
}

/// Foldback products by name (`Signal::Fold`): the netlist's own and those product-term
/// planning creates; foldback hosting places them.
type FoldTable = BTreeMap<String, Vec<(Signal, bool)>>;

/// What placement decided: every cell's and input's place, and the macrocell uses.
struct Placed {
    uses: Vec<McUse>,
    place_of: BTreeMap<String, Place>,
    host_of: BTreeMap<usize, usize>,
    fast_in: BTreeSet<String>,
}

/// The global networks: GCLK sources and polarities, and (filled while planning) the GCLR
/// polarity and the GOE assignments.
struct Nets {
    gclk_net: [Option<(u8, bool)>; 3],
    gclr_inv: Option<bool>,
    goe: BTreeMap<u8, (Source, bool)>,
}

/// Foldback expanders: the macrocell of every (foldback, block), and the product each hosts.
struct Folds {
    fold_mc: BTreeMap<(String, usize), u16>,
    fold_at: BTreeMap<u16, Vec<(Signal, bool)>>,
}

/// UIM routing: per block the muxes used, and the column / selection of every source.
struct Routing {
    routing: BTreeMap<usize, Vec<(usize, String, String)>>,
    col_of: BTreeMap<(usize, Source), usize>,
    sel_of: BTreeMap<(usize, usize), Source>,
}

/// Where a signal enters the UIM: its pad, a dedicated input, or a macrocell feedback.
fn signal_source(design: &Design, place_of: &BTreeMap<String, Place>, s: &Signal) -> Result<Source, FitError> {
    match s {
        Signal::Pad(n) => match place_of.get(n) {
            Some(Place::Pin { mc, .. }) => Ok(Source::Pad(*mc)),
            Some(Place::Dedicated { func, .. }) => Ok(match func {
                Dedicated::Gclk1 => Source::Gclk1,
                Dedicated::Gclr => Source::Gclr,
                Dedicated::Oe1 => Source::Oe1,
                Dedicated::Gclk2Oe2 => Source::Oe2,
            }),
            _ => err(FitErrorKind::Other, format!("signal {n} has no pad")),
        },
        Signal::Fold(k) => err(FitErrorKind::Other, format!("foldback {k} is not a UIM source")),
        Signal::Fb(n, _) => match place_of.get(n) {
            // an always-enabled output's feedback equals its pin: the vendor routes the pad
            Some(Place::Pin { mc, .. }) if design.cell(n).map_or(false, |c| c.oe.is_none()) => Ok(Source::Pad(*mc)),
            Some(Place::Pin { mc, .. }) | Some(Place::Node { mc }) => Ok(Source::Fb(*mc)),
            _ => err(FitErrorKind::Other, format!("signal {n} has no feedback")),
        },
    }
}

/// The GCLK network (1..=3) a dedicated clock pin drives directly, if it is one.
fn gclk_net_of_pin(dev: &Device, pin: u16) -> Option<u8> {
    if pin == dev.gclk1 {
        Some(1)
    } else if pin == dev.gclk2_oe2 {
        Some(2)
    } else if pin == dev.gclk3 {
        Some(3)
    } else {
        None
    }
}

/// The pin a placed input or output sits on.
fn placed_pin(place_of: &BTreeMap<String, Place>, n: &str) -> Option<u16> {
    match place_of.get(n)? {
        Place::Pin { pin, .. } | Place::Dedicated { pin, .. } => Some(*pin),
        Place::Node { .. } => None,
    }
}

/// Can the cascade helpers of a cell with `demand` PTs placed at `mc` go below it? `share`: MCs
/// whose only occupant is a PT-free input register (D from the pad): their whole PT array and
/// sum are free, so they can lend their sum (spec F4.6a, packing P2+).
/// The product terms of `plan` (the cell in macrocell `target`) that helper macrocell `m` holds,
/// with their PT slot in `m` (crate::capacity::helper_slot).
fn helper_pts_at(plan: &PtPlan, target: u16, m: u16) -> impl Iterator<Item = (usize, &Vec<(Signal, bool)>)> {
    let h = (target - m) as usize;
    plan.helper_pts.iter().filter(move |(&k, _)| helper_slot(k).0 == h).map(|(&k, p)| (helper_slot(k).1, p))
}

fn b_of(mc: usize) -> usize {
    (mc - 1) / 16
}

fn mc_of(place_of: &BTreeMap<String, Place>, n: &str) -> u16 {
    match place_of[n] {
        Place::Pin { mc, .. } | Place::Node { mc } => mc,
        Place::Dedicated { .. } => 0,
    }
}
