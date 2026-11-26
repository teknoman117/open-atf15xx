//! Placement, global-net assignment, product-term allocation, UIM routing and fuse assembly.

use crate::jed::Fuses;
use crate::logic::{Signal, Sop};
use crate::netlist::{Cell, Design, Storage, XorSplit};
use crate::tt2::Pla;
use atf15xx_arch::{ConfigBit, Dedicated, Device, Family, McBit, Source};
use std::collections::{BTreeMap, BTreeSet};

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

#[derive(Debug)]
pub struct FitError {
    /// What the failure is about, set where it is raised (spec F9.11).
    pub kind: FitErrorKind,
    /// The message for the user.
    pub msg: String,
}

impl std::fmt::Display for FitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}
impl std::error::Error for FitError {}

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
    if pl.fold_host || pl.hosted.is_some() || !pl.helper_pts.is_empty() || pl.pts.get(&1).map_or(false, |p| p.iter().any(|(s, _)| matches!(s, Signal::Fold(_)))) {
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
            return err(FitErrorKind::Capacity, format!(
                "no free macrocell: {} cells need at least {need} macrocells (at most {} pairs can share one), the device has {nmc}",
                design.cells.len(),
                regs.min(hosts)
            ));
        }
    }
    let mut last = None;
    let mut avoid: BTreeSet<(String, u16)> = BTreeSet::new();
    let mut repairs = 0;
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
                eprintln!("fit attempt (limit {limit}, extra {extra}): {:.2} s: {}", ta.elapsed().as_secs_f64(), attempt.as_ref().err().map_or("ok".into(), |e| e.msg.chars().take(80).collect::<String>()));
            }
            match attempt {
                Ok(r) => return Ok(r),
                Err(e) if e.kind() == FitErrorKind::Routing && e.msg.contains("[movable: ") && repairs < 96 => {
                    // move one floating node out of the congested block and try again
                    let blk = e.msg.split("block ").nth(1).and_then(|x| x.chars().next()).unwrap();
                    let b = (blk as u8 - b'A') as u16;
                    let list = e.msg.split("[movable: ").nth(1).unwrap().trim_end_matches(']');
                    // never push a cell out of every block but one (it would have nowhere left
                    // when that block fills up)
                    let nblk = (dev.family.macrocells() / 16) as usize;
                    let avoided = |n: &str| avoid.iter().filter(|(m, _)| m == n).count();
                    if let Some(n) = list
                        .split(',')
                        .find(|n| !n.is_empty() && !avoid.contains(&(n.to_string(), b)) && avoided(n) + 3 <= nblk)
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
    let partition = placer != Placer::Greedy;
    let f = dev.family;
    let nmc = f.macrocells();
    let mut uses = vec![McUse::default(); nmc + 1];
    let mut placement: Vec<(String, Place)> = vec![];
    let mut place_of: BTreeMap<String, Place> = BTreeMap::new();
    let jtag_pins: Vec<u16> = if opts.jtag {
        vec![dev.jtag.tdi, dev.jtag.tms, dev.jtag.tck, dev.jtag.tdo]
    } else {
        vec![]
    };
    let mut reserved_pins: BTreeSet<u16> = jtag_pins.iter().copied().collect();
    if opts.pd1 {
        reserved_pins.insert(dev.pd1);
    }
    if opts.pd2 {
        reserved_pins.insert(dev.pd2);
    }
    let keep = opts.preassign != Preassign::Ignore;

    // ---- global-net candidates: which input signals want a dedicated pin
    let mut want_clk: Vec<String> = vec![];
    let mut want_clr: Vec<String> = vec![];
    let mut want_oe: Vec<String> = vec![];
    for c in &design.cells {
        if let Some((Signal::Pad(n), _)) = c.clk.as_ref().and_then(|s| s.single_literal()) {
            if design.inputs.contains_key(n) && !want_clk.contains(n) {
                want_clk.push(n.clone());
            }
        }
        if let Some(ar) = &c.ar {
            for p in &ar.0 {
                if let [(Signal::Pad(n), _)] = p.as_slice() {
                    if design.inputs.contains_key(n) && !want_clr.contains(n) {
                        want_clr.push(n.clone());
                    }
                }
            }
        }
        if let Some((Signal::Pad(n), _)) = c.oe.as_ref().and_then(|s| s.single_literal()) {
            if design.inputs.contains_key(n) && !want_oe.contains(n) {
                want_oe.push(n.clone());
            }
        }
    }

    // ---- place inputs
    let mut free_dedicated: Vec<(u16, Dedicated)> = vec![
        (dev.gclk1, Dedicated::Gclk1),
        (dev.oe1, Dedicated::Oe1),
        (dev.gclr, Dedicated::Gclr),
        (dev.gclk2_oe2, Dedicated::Gclk2Oe2),
    ];
    let mut taken_pins: BTreeSet<u16> = BTreeSet::new();
    // pinned inputs first
    for name in &design.input_order {
        let Some(pin) = design.inputs[name].filter(|_| keep) else { continue };
        let pl = if let Some(func) = dev.dedicated(pin) {
            free_dedicated.retain(|x| x.0 != pin);
            Place::Dedicated { pin, func }
        } else if let Some(mc) = dev.pin_mc(pin).or_else(|| dev.input_only_mc(pin)) {
            if reserved_pins.contains(&pin) {
                return err(FitErrorKind::Other, format!("{name}: pin {pin} is reserved (JTAG/power-down)"));
            }
            uses[mc as usize].pad_used = true;
            Place::Pin { pin, mc }
        } else {
            return err(FitErrorKind::Other, format!("{name}: pin {pin} is not a user pin of {}", dev.name));
        };
        taken_pins.insert(pin);
        place_of.insert(name.clone(), pl);
    }
    // pinned cells
    for c in &design.cells {
        if c.is_output {
            if let Some(pin) = c.pin.filter(|_| keep) {
                let Some(mc) = dev.pin_mc(pin) else {
                    return err(FitErrorKind::Other, format!("{}: pin {pin} cannot be an output on {}", c.name, dev.name));
                };
                if reserved_pins.contains(&pin) {
                    return err(FitErrorKind::Other, format!("{}: pin {pin} is reserved (JTAG/power-down)", c.name));
                }
                taken_pins.insert(pin);
                place_of.insert(c.name.clone(), Place::Pin { pin, mc });
            }
        }
    }
    // node constraints, unless the macrocell already drives a pinned output (the vendor then
    // ignores the constraint)
    for c in &design.cells {
        if let Some(n) = c.node.filter(|_| keep && !c.is_output) {
            if (601..=600 + nmc as u16).contains(&n) {
                let mc = n - 600;
                if !place_of.values().any(|p| matches!(p, Place::Pin { mc: m, .. } | Place::Node { mc: m } if *m == mc)) {
                    place_of.insert(c.name.clone(), Place::Node { mc });
                }
            }
        }
    }
    // product-term demand per cell (estimate), and cascade helper reservations below pinned cells
    let demand: Vec<usize> = design.cells.iter().map(|c| pt_demand(c, design.xor.get(&c.name))).collect();
    let mut reserved_mcs: BTreeSet<u16> = BTreeSet::new();
    for (i, c) in design.cells.iter().enumerate() {
        if demand[i] > 5 {
            if let Some(&Place::Pin { mc, .. } | &Place::Node { mc }) = place_of.get(&c.name) {
                for k in 1..=((demand[i] - 1) / 5) as u16 {
                    if mc > k && (mc - k - 1) / 16 == (mc - 1) / 16 {
                        reserved_mcs.insert(mc - k);
                    }
                }
            }
        }
    }
    // macrocells holding a PT-free input register (filled by pack_input_registers)
    let mut share: BTreeSet<u16> = BTreeSet::new();
    // floating inputs wanting a global net
    let mut grab = |name: &str, kinds: &[Dedicated], place_of: &mut BTreeMap<String, Place>, taken: &mut BTreeSet<u16>| {
        if place_of.contains_key(name) {
            return;
        }
        if let Some(i) = free_dedicated.iter().position(|(_, d)| kinds.contains(d)) {
            let (pin, func) = free_dedicated.remove(i);
            taken.insert(pin);
            place_of.insert(name.to_string(), Place::Dedicated { pin, func });
        }
    };
    for n in &want_clk {
        grab(n, &[Dedicated::Gclk1, Dedicated::Gclk2Oe2], &mut place_of, &mut taken_pins);
    }
    for n in &want_clr {
        grab(n, &[Dedicated::Gclr], &mut place_of, &mut taken_pins);
    }
    for n in &want_oe {
        grab(n, &[Dedicated::Oe1, Dedicated::Gclk2Oe2], &mut place_of, &mut taken_pins);
    }
    // JTAG / power-down pins occupy their macrocells' pads (the vendor lists them as placed inputs)
    let mut pseudo_inputs: Vec<u16> = jtag_pins.clone();
    if opts.pd1 {
        pseudo_inputs.push(dev.pd1);
    }
    if opts.pd2 {
        pseudo_inputs.push(dev.pd2);
    }
    for &p in &pseudo_inputs {
        if let Some(mc) = dev.pin_mc(p) {
            uses[mc as usize].pad_used = true;
        }
    }
    let mc_taken = |uses: &Vec<McUse>, place_of: &BTreeMap<String, Place>, mc: u16| -> bool {
        uses[mc as usize].pad_used
            || place_of.values().any(|p| matches!(p, Place::Pin { mc: m, .. } | Place::Node { mc: m } if *m == mc))
    };
    // floating outputs: lowest block first, highest macrocell of the block first (vendor order)
    for c in &design.cells {
        if c.is_output && !place_of.contains_key(&c.name) {
            let slot = dev
                .io_pins()
                .map(|(p, m)| ((m - 1) / 16, std::cmp::Reverse(m), p))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|(_, std::cmp::Reverse(m), p)| (m, p))
                .find(|&(m, p)| {
                    !reserved_pins.contains(&p)
                        && !taken_pins.contains(&p)
                        && !mc_taken(&uses, &place_of, m)
                        && !reserved_mcs.contains(&m)
                        && helpers_free(m, demand[design.cell_index(&c.name).unwrap()], &place_of, &reserved_mcs, &share)
                });
            let Some((mc, pin)) = slot else { return err(FitErrorKind::Capacity, format!("no free output pin for {}", c.name)) };
            taken_pins.insert(pin);
            reserve_helpers(mc, demand[design.cell_index(&c.name).unwrap()], &mut reserved_mcs);
            place_of.insert(c.name.clone(), Place::Pin { pin, mc });
        }
    }
    // .PIN feedback marks output pads used
    for c in &design.cells {
        for s in c.inputs() {
            if let Signal::Pad(n) = &s {
                if let Some(Place::Pin { mc, .. }) = design.cell(n).and(place_of.get(n)) {
                    uses[*mc as usize].pad_used = true;
                }
            }
        }
    }
    // logic doubling: pair buried registers with combinatorial outputs when macrocells run short
    let mut host_of: BTreeMap<usize, usize> = BTreeMap::new(); // register cell -> host output cell
    {
        let helpers: usize = demand.iter().map(|&d| if d > 5 { (d - 1) / 5 } else { 0 }).sum();
        let mut need = (design.cells.len() + helpers + extra_pairs).saturating_sub(nmc);
        if opts.logic_doubling && need > 0 {
            need = usize::MAX; // dense design with doubling allowed: pair every eligible register
        }
        if need > 0 && (opts.logic_doubling || extra_pairs > 0) {
            let mut hosts_used: BTreeSet<usize> = BTreeSet::new();
            // registers with the most inputs first: they are the hardest to place elsewhere
            let mut order: Vec<usize> = (0..design.cells.len()).collect();
            order.sort_by_key(|&i| std::cmp::Reverse(design.cells[i].inputs().len()));
            for ri in order {
                let r = &design.cells[ri];
                if host_of.len() >= need {
                    break;
                }
                if !doubling_register_ok(r) || place_of.contains_key(&r.name) && r.node.is_some() {
                    continue;
                }
                // a hosted register is clocked by a global network: its clock must be a clock pin
                let global_clk = match r.clk.as_ref().and_then(|k| k.single_literal()) {
                    Some((Signal::Pad(n), _)) => match place_of.get(n) {
                        Some(Place::Dedicated { pin, .. }) | Some(Place::Pin { pin, .. }) => {
                            [dev.gclk1, dev.gclk2_oe2, dev.gclk3].contains(pin)
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if !global_clk {
                    continue;
                }
                let extra = 1
                    + usize::from(r.ar.as_ref().map_or(false, |a| a.0.iter().any(|p| p.len() != 1)))
                    + usize::from(r.ce.is_some() || r.storage == Storage::Tff);
                let host = design.cells.iter().enumerate().find(|(yi, y)| {
                    !hosts_used.contains(yi)
                        && y.is_output
                        && y.storage == Storage::Comb
                        && demand[*yi] + extra <= 5
                        && (y.oe.is_none() || !design.cells.iter().any(|o| o.inputs().contains(&Signal::Fb(y.name.clone(), false))))
                        && place_of.contains_key(&y.name)
                        && fanin_ok(&design, &place_of, (mc_of(&place_of, &y.name) - 1) / 16, r, fanin_limit)
                        && !avoid.contains(&(r.name.clone(), (mc_of(&place_of, &y.name) - 1) / 16))
                });
                if let Some((yi, _)) = host {
                    hosts_used.insert(yi);
                    host_of.insert(ri, yi);
                    let mc = mc_of(&place_of, &design.cells[yi].name);
                    place_of.insert(r.name.clone(), Place::Node { mc });
                }
            }
            // a hosted T flip-flop becomes D = !Q with CE = T (the vendor's conversion), so its D
            // fits the single PT2 of a doubled macrocell
            for &ri in host_of.keys() {
                let r = &mut design.cells[ri];
                if r.storage == Storage::Tff {
                    let q = Signal::Fb(r.name.clone(), true);
                    r.ce = Some(std::mem::replace(&mut r.data, Sop(vec![vec![(q, false)]])));
                    r.storage = Storage::Dff;
                }
            }
        }
        if std::env::var_os("ATF_FIT_DEBUG").is_some() {
            for (i, c) in design.cells.iter().enumerate() {
                if demand[i] > 5 {
                    eprintln!("fit: {} needs {} PTs ({} products)", c.name, demand[i], c.data.0.len());
                }
            }
            eprintln!(
                "fit: {} cells, {} cascade helpers, {} MCs, need {} doubled, got {} (fan-in limit {fanin_limit}, extra {extra_pairs})",
                design.cells.len(), helpers, nmc, need, host_of.len()
            );
        }
        for (&ri, &yi) in &host_of {
            let mc = mc_of(&place_of, &design.cells[yi].name);
            place_of.insert(design.cells[ri].name.clone(), Place::Node { mc });
        }
    }

    // block assignment for the floating buried cells (spec F9.4), only when the greedy placement
    // alone has failed
    let assigned: BTreeMap<String, u16> = if partition {
        partition_blocks(&design, &place_of, &reserved_mcs, &demand, (nmc / 16) as u16, avoid, fanin_limit)
    } else {
        BTreeMap::new()
    };
    let in_assigned = |n: &str, m: u16| assigned.get(n).map_or(true, |&b| b == (m - 1) / 16);
    // input registers: a D flip-flop or latch whose D is an input pin goes into that pin's
    // macrocell with D from the pad, before the buried-cell phases hand that macrocell out
    let mut fast_in: BTreeSet<String> = BTreeSet::new();
    if opts.pack_input_registers {
        pack_input_registers(&design, &mut place_of, &reserved_mcs, fanin_limit, avoid, &mut fast_in, &mut share);
    }
    // floating buried cells, phase 1: lowest block first, highest macrocell whose pin is not
    // usable (unbonded, or reserved for JTAG / power-down)
    let pin_unusable = |m: u16| dev.mc_pin(m).map_or(true, |p| reserved_pins.contains(&p));
    let cell_mcs_now = |place_of: &BTreeMap<String, Place>| -> BTreeSet<u16> {
        design
            .cells
            .iter()
            .filter_map(|x| match place_of.get(&x.name) {
                Some(Place::Pin { mc, .. }) | Some(Place::Node { mc }) => Some(*mc),
                _ => None,
            })
            .collect()
    };
    for c in &design.cells {
        if c.is_output || place_of.contains_key(&c.name) {
            continue;
        }
        let cell_mcs = cell_mcs_now(&place_of);
        let pick = (1..=nmc as u16)
            .map(|m| ((m - 1) / 16, std::cmp::Reverse(m)))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|(_, std::cmp::Reverse(m))| m)
            .find(|&m| {
                pin_unusable(m)
                    && !cell_mcs.contains(&m)
                    && !reserved_mcs.contains(&m)
                    && helpers_free(m, demand[design.cell_index(&c.name).unwrap()], &place_of, &reserved_mcs, &share)
                    && fanin_ok(&design, &place_of, (m - 1) / 16, c, fanin_limit)
                    && !avoid.contains(&(c.name.clone(), (m - 1) / 16))
                    && in_assigned(&c.name, m)
            });
        if let Some(mc) = pick {
            reserve_helpers(mc, demand[design.cell_index(&c.name).unwrap()], &mut reserved_mcs);
            place_of.insert(c.name.clone(), Place::Node { mc });
        }
    }
    // floating inputs: visit cells block by block (outputs before nodes), place each unplaced input
    // at the lowest free bonded macrocell, in order of first use (vendor order)
    let mut visit: Vec<(u16, bool, usize)> = design
        .cells
        .iter()
        .enumerate()
        .map(|(i, c)| (place_of.get(&c.name).map_or(99, |_| (mc_of(&place_of, &c.name) - 1) / 16), !c.is_output, i))
        .collect();
    visit.sort();
    let mut order: Vec<String> = vec![];
    for (_, _, i) in visit {
        let c = &design.cells[i];
        let mut sigs: Vec<&Signal> = vec![];
        for s in [Some(&c.data), c.clk.as_ref(), c.ce.as_ref(), c.ar.as_ref(), c.ap.as_ref(), c.oe.as_ref()].into_iter().flatten() {
            for p in &s.0 {
                for (sig, _) in p {
                    sigs.push(sig);
                }
            }
        }
        for sig in sigs {
            if let Signal::Pad(n) = sig {
                if design.inputs.contains_key(n) && !order.contains(n) {
                    order.push(n.clone());
                }
            }
        }
    }
    for name in order.iter().chain(design.input_order.iter()) {
        if place_of.contains_key(name) {
            continue;
        }
        let cell_mcs = cell_mcs_now(&place_of);
        let slot = dev
            .io_pins()
            .map(|(p, m)| (m, p))
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .find(|&(m, p)| {
                !reserved_pins.contains(&p) && !taken_pins.contains(&p) && !uses[m as usize].pad_used && !cell_mcs.contains(&m)
            });
        let Some((mc, pin)) = slot else {
            // all I/O pins taken: an unused dedicated input pin also reaches every UIM
            let ded = [dev.gclk1, dev.oe1, dev.gclr, dev.gclk2_oe2]
                .into_iter()
                .find(|p| !taken_pins.contains(p) && !reserved_pins.contains(p) && !place_of.values().any(|x| matches!(x, Place::Dedicated { pin, .. } if pin == p)));
            match ded.and_then(|p| dev.dedicated(p).map(|f| (p, f))) {
                Some((p, func)) => {
                    taken_pins.insert(p);
                    place_of.insert(name.clone(), Place::Dedicated { pin: p, func });
                    continue;
                }
                None => return err(FitErrorKind::InputPins, format!("no free input pin for {name}")),
            }
        };
        taken_pins.insert(pin);
        uses[mc as usize].pad_used = true;
        place_of.insert(name.clone(), Place::Pin { pin, mc });
    }
    // input registers whose pin was placed only now
    if opts.pack_input_registers {
        pack_input_registers(&design, &mut place_of, &reserved_mcs, fanin_limit, avoid, &mut fast_in, &mut share);
    }
    // floating buried cells, phase 2: macrocells whose pad is an input, then any free one
    for c in &design.cells {
        if c.is_output || place_of.contains_key(&c.name) {
            continue;
        }
        let cell_mcs = cell_mcs_now(&place_of);
        let order: Vec<u16> = (1..=nmc as u16)
            .map(|m| ((m - 1) / 16, std::cmp::Reverse(m)))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|(_, std::cmp::Reverse(m))| m)
            .collect();
        let d = demand[design.cell_index(&c.name).unwrap()];
        let ok = |m: u16| {
            !avoid.contains(&(c.name.clone(), (m - 1) / 16))
                && !cell_mcs.contains(&m)
                && !reserved_mcs.contains(&m)
                && helpers_free(m, d, &place_of, &reserved_mcs, &share)
                && fanin_ok(&design, &place_of, (m - 1) / 16, c, fanin_limit)
        };
        let pick = order
            .iter()
            .copied()
            .find(|&m| uses[m as usize].pad_used && ok(m) && in_assigned(&c.name, m))
            .or_else(|| order.iter().copied().find(|&m| ok(m) && in_assigned(&c.name, m)))
            .or_else(|| order.iter().copied().find(|&m| uses[m as usize].pad_used && ok(m)))
            .or_else(|| order.iter().copied().find(|&m| ok(m)))
            .or_else(|| {
                // over budget everywhere: the free macrocell whose block grows least
                let mut best: Option<(usize, u16)> = None;
                // (the avoid list from UIM repairs is a preference: a full device ignores it)
                for &m in &order {
                    if cell_mcs.contains(&m) || reserved_mcs.contains(&m) || !helpers_free(m, d, &place_of, &reserved_mcs, &share) {
                        continue;
                    }
                    let mut lim = 0;
                    while !fanin_ok(&design, &place_of, (m - 1) / 16, c, lim) {
                        lim += 1;
                    }
                    if best.map_or(true, |(l, _)| lim < l) {
                        best = Some((lim, m));
                    }
                }
                best.map(|x| x.1)
            });
        if pick.is_none() && std::env::var_os("ATF_FIT_DEBUG").is_some() {
            for &m in &order {
                eprintln!(
                    "node {}: mc {m}: cell {} reserved {} helpers_free {} avoid {} pad_used {}",
                    c.name,
                    cell_mcs.contains(&m),
                    reserved_mcs.contains(&m),
                    helpers_free(m, d, &place_of, &reserved_mcs, &share),
                    avoid.contains(&(c.name.clone(), (m - 1) / 16)),
                    uses[m as usize].pad_used
                );
            }
        }
        let Some(mc) = pick else { return err(FitErrorKind::Capacity, format!("no free macrocell for node {}", c.name)) };
        reserve_helpers(mc, d, &mut reserved_mcs);
        place_of.insert(c.name.clone(), Place::Node { mc });
    }
    // UIM-aware positions (failure path only): which muxes can select a buried cell's feedback
    // depends on its macrocell, so move/swap buried cells within their blocks until every block's
    // sources can be matched onto its 40 muxes
    if placer == Placer::Partition {
        let movable: Vec<String> = design
            .cells
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                matches!(place_of.get(&c.name), Some(Place::Node { .. }))
                    && !host_of.contains_key(i)
                    && !fast_in.contains(&c.name)
                    && demand[*i] <= 5
                    && c.node.map_or(true, |n| n == 0)
            })
            .map(|(_, c)| c.name.clone())
            .collect();
        optimize_positions(&design, &mut place_of, &movable, &reserved_mcs, f);
    }
    for (i, c) in design.cells.iter().enumerate() {
        let mc = match place_of[&c.name] {
            Place::Pin { mc, .. } | Place::Node { mc } => mc,
            Place::Dedicated { .. } => unreachable!(),
        };
        if host_of.contains_key(&i) {
            uses[mc as usize].hosted = Some(i);
            continue;
        }
        if let Some(j) = uses[mc as usize].cell {
            return err(FitErrorKind::Other, format!("{} and {} both placed on MC{mc}", design.cells[j].name, c.name));
        }
        uses[mc as usize].cell = Some(i);
    }

    // ---- global nets
    let net_of_pin = |pin: u16| -> Option<u8> {
        if pin == dev.gclk1 {
            Some(1)
        } else if pin == dev.gclk2_oe2 {
            Some(2)
        } else if pin == dev.gclk3 {
            Some(3)
        } else {
            None
        }
    };
    let pin_of_name = |n: &str| -> Option<u16> {
        match place_of.get(n)? {
            Place::Pin { pin, .. } | Place::Dedicated { pin, .. } => Some(*pin),
            Place::Node { .. } => None,
        }
    };
    // GCLK networks: `gclk_net[n-1] = (clock pin k, positive)` -- network n takes clock pin k
    // (1 = GCLK1, 2 = GCLK2/OE2, 3 = GCLK3 I/O) through its source mux and inverts it unless
    // positive. The invert bit is per network: when cells clock on both edges of one clock pin,
    // the polarity used by more cells (cells with a clock enable count first, as they cannot use
    // a PT clock) gets the pin's own network; with `share_clock_networks` the other polarity gets
    // a free network (one whose own pin clocks nothing), otherwise those cells use a PT clock.
    let mut gclk_net: [Option<(u8, bool)>; 3] = [None; 3];
    {
        let mut votes: BTreeMap<u8, (usize, usize)> = BTreeMap::new();
        for c in &design.cells {
            if let Some((Signal::Pad(n), pos)) = c.clk.as_ref().and_then(|f| f.single_literal()) {
                if let Some(net) = pin_of_name(&n).and_then(net_of_pin) {
                    let w = if c.ce.is_some() { 1000 } else { 1 };
                    let v = votes.entry(net).or_default();
                    if pos { v.0 += w } else { v.1 += w }
                }
            }
        }
        for (&k, &(p, n)) in &votes {
            gclk_net[k as usize - 1] = Some((k, p >= n));
        }
        if opts.share_clock_networks {
            for (&k, &(p, n)) in &votes {
                if p > 0 && n > 0 {
                    if let Some(free) = (0..3).find(|&i| gclk_net[i].is_none()) {
                        gclk_net[free] = Some((k, p < n));
                    }
                }
            }
        }
    }
    let net_for = |k: u8, pos: bool| -> Option<u8> { (0..3u8).find(|&i| gclk_net[i as usize] == Some((k, pos))).map(|i| i + 1) };
    let mut gclr_inv = None::<bool>;
    let mut goe: BTreeMap<u8, (Source, bool)> = BTreeMap::new();

    // ---- PT plans
    let source_of = |s: &Signal| -> Result<Source, FitError> {
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
    };
    FOLDS.with(|f| {
        let mut f = f.borrow_mut();
        f.clear();
        // foldback signals introduced at netlist level (Design::nodes_to_foldbacks)
        f.extend(design.folds.iter().map(|(k, v)| (k.clone(), v.clone())));
    });
    let mut plans: Vec<PtPlan> = vec![];
    for c in &design.cells {
        let mut pl = PtPlan::default();
        // data polarity
        let target_given = !c.active_low;
        let as_is: Vec<Vec<(Signal, bool)>> = if target_given { c.data.0.clone() } else { c.data.complement().0 };
        let flipped: Vec<Vec<(Signal, bool)>> =
            if target_given { if c.data.0.len() > 1 { c.data.complement().0 } else { vec![vec![]; 99] } } else { c.data.0.clone() };
        // a foldback line exists in one polarity only: a cover with a negated foldback literal
        // cannot be built (complementing a function of foldbacks can create one)
        let neg_fold = |v: &Vec<Vec<(Signal, bool)>>| v.iter().flatten().any(|(s, p)| !p && matches!(s, Signal::Fold(_)));
        let (as_is, flipped) = if neg_fold(&as_is) && !neg_fold(&flipped) && flipped.len() < 99 {
            (as_is, flipped)
        } else if neg_fold(&flipped) {
            (as_is, vec![vec![]; 99]) // never choose the flipped cover
        } else {
            (as_is, flipped)
        };
        if neg_fold(&as_is) && (flipped.len() >= 99 || neg_fold(&flipped)) {
            return err(FitErrorKind::Netlist, format!("{}: needs an inverted foldback line", c.name));
        }
        if as_is.is_empty() || Sop(as_is.clone()).is_one() {
            pl.constant = Some(!as_is.is_empty());
        } else if (flipped.len() < as_is.len() || neg_fold(&as_is)) && !Sop(flipped.clone()).is_one() && !flipped.is_empty() {
            pl.sum = flipped.clone();
            pl.invert = true;
        } else {
            pl.sum = as_is.clone();
        }
        if fast_in.contains(&c.name) {
            // D comes from the pad through the D/Q fast path: no data product at all
            pl.fast_in = true;
            pl.sum.clear();
            pl.invert = false;
            pl.constant = None;
        } else if let Some(x) = design.xor.get(&c.name).filter(|_| pl.constant.is_none() && !host_of.values().any(|&y| y == plans.len())) {
            // XOR mode: sum = g, PT2 = p, x = g ⊕ p; the Invert fuse is the XOR's B enable, so
            // the polarity comes from the cover (`!f = !g ⊕ p`)
            let g = if target_given { &x.g } else { &x.gbar };
            if 1 + g.0.len() <= pl.sum.len() && !neg_fold(&g.0) {
                pl.sum = g.0.clone();
                pl.invert = true;
                pl.xor = true;
                pl.pts.insert(2, x.p.clone());
            }
        }
        // clock
        if let Some(clk) = &c.clk {
            match clk.single_literal() {
                Some((Signal::Pad(n), pos)) if pin_of_name(n).and_then(net_of_pin).and_then(|k| net_for(k, pos)).is_some() => {
                    let net = pin_of_name(n).and_then(net_of_pin).and_then(|k| net_for(k, pos)).unwrap();
                    pl.clk = ClkUse::Global(net);
                }
                _ => {
                    pl.clk = ClkUse::Pt;
                    pl.pts.insert(4, one_product(clk, &c.name, "clock")?);
                }
            }
        }
        if let Some(ce) = &c.ce {
            if pl.clk == ClkUse::Pt {
                return err(FitErrorKind::Other, format!("{}: clock enable needs a global clock", c.name));
            }
            pl.ce_pt = true;
            pl.pts.insert(4, one_product(ce, &c.name, "clock enable")?);
        }
        if let Some(ar) = &c.ar {
            let mut rest = vec![];
            for p in &ar.0 {
                match p.as_slice() {
                    // the GCLR network has one polarity for the whole device: the first user fixes
                    // it, the other polarity falls back to a PT reset (as the vendor does)
                    [(Signal::Pad(n), pos)]
                        if pin_of_name(n) == Some(dev.gclr) && !pl.ar_global && gclr_inv.map_or(true, |inv| inv == !pos) =>
                    {
                        pl.ar_global = true;
                        gclr_inv = Some(!pos);
                    }
                    _ => rest.push(p.clone()),
                }
            }
            match rest.len() {
                0 => {}
                1 => {
                    pl.ar_pt = true;
                    pl.pts.insert(3, rest.pop().unwrap());
                }
                // several products: one foldback (complement is one product) or two levels
                _ => {
                    pl.ar_pt = true;
                    pl.pts.insert(3, one_product(&Sop(rest), &c.name, "asynchronous reset")?);
                }
            }
        }
        if let Some(ap) = &c.ap {
            pl.ap_pt = true;
            pl.pts.insert(5, one_product(ap, &c.name, "asynchronous preset")?);
        }
        pl.oe = if !c.is_output {
            OeUse::Off
        } else if let Some(oe) = &c.oe {
            match oe.single_literal() {
                Some((s, pos)) => {
                    let src = source_of(s)?;
                    let n = (1..=6u8)
                        .find(|&n| goe.get(&n).map_or(false, |x| *x == (src, !pos)))
                        .or_else(|| (1..=6u8).find(|&n| !goe.contains_key(&n) && f.goe_choices(n as usize).contains(&src)));
                    match n {
                        Some(n) => {
                            goe.insert(n, (src, !pos));
                            OeUse::Global(n)
                        }
                        None if !pl.ap_pt => {
                            pl.pts.insert(5, one_product(oe, &c.name, "output enable")?);
                            OeUse::Pt
                        }
                        None => return err(FitErrorKind::Other, format!("{}: no global OE reaches its enable signal", c.name)),
                    }
                }
                None if oe.is_one() => OeUse::Always,
                None if pl.ap_pt => return err(FitErrorKind::Other, format!("{}: PT5 needed for both preset and OE", c.name)),
                None => {
                    pl.pts.insert(5, one_product(oe, &c.name, "output enable")?);
                    OeUse::Pt
                }
            }
        } else {
            OeUse::Always
        };
        // sum terms into the free PTs; if they do not fit, factor a common literal out of products
        // whose remainders are single literals and use a foldback expander for the remainder
        // (`L&a # L&b # L&!c` = `L & !(!a & !b & c)`), as the vendor does
        let free: Vec<usize> = (1..=5).filter(|k| !pl.pts.contains_key(k)).collect();
        if opts.foldback && pl.sum.len() > free.len() && pl.constant.is_none() {
            let factor = |mut s: Vec<Vec<(Signal, bool)>>| {
                while s.len() > free.len() {
                    match factor_with_foldback(&s) {
                        Some(x) => s = x,
                        None => break,
                    }
                }
                s
            };
            // factor the chosen polarity; if it still does not fit, the other polarity may
            // factor better (`L&a&(b+c+!d) + ...` has a larger cover but a common cube)
            let mine = factor(pl.sum.clone());
            let other_cover = if pl.invert { as_is.clone() } else { flipped.clone() };
            if !pl.xor && mine.len() > free.len() && !other_cover.is_empty() && other_cover.len() < 99 && !Sop(other_cover.clone()).is_one() {
                let other = factor(other_cover);
                if other.len() < mine.len() {
                    pl.sum = other;
                    pl.invert = !pl.invert;
                } else {
                    pl.sum = mine;
                }
            } else {
                pl.sum = mine;
            }
        }
        let n = pl.sum.len();
        for (i, p) in pl.sum.clone().into_iter().enumerate() {
            if i < free.len() {
                pl.pts.insert(free[i], p);
                pl.in_sum.insert(free[i]);
            } else {
                let k = i - free.len() + 1;
                if k > 5 || !opts.cascade {
                    return err(FitErrorKind::Capacity, format!("{}: {} product terms do not fit", c.name, n));
                }
                pl.helper_pts.insert(k, p);
            }
        }
        plans.push(pl);
    }
    // logic doubling: move the hosted register's D (PT2), reset (PT3) and CE (PT4) into the host
    for (&ri, &yi) in &host_of {
        let rp = plans[ri].clone();
        if rp.invert || rp.sum.len() != 1 || rp.constant.is_some() || !matches!(rp.clk, ClkUse::Global(_)) {
            return err(FitErrorKind::Other, format!("{}: cannot share a macrocell with {}", design.cells[ri].name, design.cells[yi].name));
        }
        let yp = &mut plans[yi];
        let mut reserved: BTreeSet<usize> = [2].into_iter().collect();
        if rp.ar_pt {
            reserved.insert(3);
        }
        if rp.ce_pt {
            reserved.insert(4);
        }
        let specials: BTreeMap<usize, Vec<(Signal, bool)>> =
            yp.pts.iter().filter(|(k, _)| !yp.in_sum.contains(k)).map(|(k, v)| (*k, v.clone())).collect();
        if specials.keys().any(|k| reserved.contains(k)) || !yp.helper_pts.is_empty() {
            return err(FitErrorKind::Other, format!("{}: cannot share a macrocell with {}", design.cells[ri].name, design.cells[yi].name));
        }
        let free: Vec<usize> = (1..=5).filter(|k| !specials.contains_key(k) && !reserved.contains(k)).collect();
        if yp.sum.len() > free.len() {
            return err(FitErrorKind::Other, format!("{}: cannot share a macrocell with {}", design.cells[ri].name, design.cells[yi].name));
        }
        yp.pts = specials;
        yp.in_sum.clear();
        for (k, p) in free.iter().zip(yp.sum.clone()) {
            yp.pts.insert(*k, p);
            yp.in_sum.insert(*k);
        }
        yp.pts.insert(2, rp.sum[0].clone());
        if rp.ar_pt {
            yp.pts.insert(3, rp.pts[&3].clone());
        }
        if rp.ce_pt {
            yp.pts.insert(4, rp.pts[&4].clone());
        }
        yp.hosted = Some(rp.clone().into());
    }

    // cascade helpers
    for (i, c) in design.cells.iter().enumerate() {
        if plans[i].helper_pts.is_empty() {
            continue;
        }
        let mc = mc_of(&place_of, &c.name);
        // a PT-free input register (D from the pad) leaves its macrocell's PTs and sum free
        let shareable = |j: usize| plans[j].fast_in && plans[j].pts.is_empty() && !plans[j].fold_host;
        if (mc - 1) % 16 == 0
            || uses[mc as usize - 1].cell.map_or(false, |j| !shareable(j))
            || uses[mc as usize - 1].helper_for.is_some()
        {
            return err(FitErrorKind::Cascade, format!("{}: needs cascade from MC{} which is not free", c.name, mc - 1));
        }
        uses[mc as usize - 1].helper_for = Some(mc);
    }

    // ---- foldback expanders: one free macrocell per (product, block), highest first
    let fold_products: BTreeMap<String, Vec<(Signal, bool)>> = FOLDS.with(|f| f.borrow().clone());
    let mut fold_mc: BTreeMap<(String, usize), u16> = BTreeMap::new();
    for (i, c) in design.cells.iter().enumerate() {
        let b = (mc_of(&place_of, &c.name) as usize - 1) / 16;
        let mut folds_needed: Vec<String> = plans[i]
            .pts
            .values()
            .chain(plans[i].helper_pts.values())
            .flatten()
            .filter_map(|(s, _)| if let Signal::Fold(k) = s { Some(k.clone()) } else { None })
            .collect();
        // a foldback product may itself read foldbacks (NAND of NANDs = OR of products, see
        // one_product): host those in the same block too, inner ones after their readers
        let mut q = 0;
        while q < folds_needed.len() {
            let inner: Vec<String> = fold_products
                .get(&folds_needed[q])
                .map(|p| p.iter().filter_map(|(s, _)| if let Signal::Fold(k) = s { Some(k.clone()) } else { None }).collect())
                .unwrap_or_default();
            for k in inner {
                if !folds_needed.contains(&k) {
                    folds_needed.push(k);
                }
            }
            q += 1;
        }
        for k in &folds_needed {
            {
                {
                    if fold_mc.contains_key(&(k.clone(), b)) {
                        continue;
                    }
                    let taken = |m: usize, fold_mc: &BTreeMap<(String, usize), u16>| fold_mc.values().any(|&x| x as usize == m);
                    // with logic doubling (dense designs) keep free macrocells free: host on logic first
                    let mut m = if opts.logic_doubling {
                        None
                    } else {
                        (b * 16 + 1..=b * 16 + 16)
                            .rev()
                            .find(|&m| uses[m].cell.is_none() && uses[m].helper_for.is_none() && !taken(m, &fold_mc))
                    };
                    if m.is_none() {
                        // host on a macrocell with logic whose plan leaves a PT free: move its PT1 there
                        // (also the reading cell itself: its PT1 becomes the foldback line and its sum
                        // reads the fold through another PT, as the vendor does)
                        m = (b * 16 + 1..=b * 16 + 16).rev().find(|&m| {
                            uses[m].helper_for.is_none()
                                && uses[m].cell.map_or(false, |j| !taken(m, &fold_mc) && fold_movable(&plans[j]).is_some())
                        });
                        if let Some(mm) = m {
                            let j = uses[mm].cell.unwrap();
                            let k2 = fold_movable(&plans[j]).unwrap();
                            let pl = &mut plans[j];
                            if let Some(p1) = pl.pts.remove(&1) {
                                pl.pts.insert(k2, p1);
                                pl.in_sum.remove(&1);
                                pl.in_sum.insert(k2);
                            }
                            pl.fold_host = true;
                        }
                    }
                    if m.is_none() {
                        m = (b * 16 + 1..=b * 16 + 16)
                            .rev()
                            .find(|&m| uses[m].cell.is_none() && uses[m].helper_for.is_none() && !taken(m, &fold_mc));
                    }
                    if m.is_none() && std::env::var_os("ATF_FIT_DEBUG").is_some() {
                        let here: Vec<&String> = fold_mc.iter().filter(|((_, bb), _)| *bb == b).map(|((k, _), _)| k).collect();
                        eprintln!("fold hosting: block {b}: {} foldbacks hosted, need {k}; macrocells:", here.len());
                        for h in &here {
                            eprintln!("  hosted: {h}");
                        }
                        for m in b * 16 + 1..=b * 16 + 16 {
                            let why = match uses[m].cell {
                                None if uses[m].helper_for.is_some() => "cascade helper".to_string(),
                                None => "free".into(),
                                Some(j) => {
                                    let pl = &plans[j];
                                    format!("{} pts {:?} sum {:?} fold_host {} hosted {} helpers {}", design.cells[j].name, pl.pts.keys().collect::<Vec<_>>(), pl.in_sum, pl.fold_host, pl.hosted.is_some(), pl.helper_pts.len())
                                }
                            };
                            eprintln!("  MC{m}: {}{}", if taken(m, &fold_mc) { "[hosts a fold] " } else { "" }, why);
                        }
                    }
                    let Some(m) = m else { return err(FitErrorKind::Capacity, format!("{}: no free macrocell for a foldback expander", c.name)) };
                    fold_mc.insert((k.clone(), b), m as u16);
                }
            }
        }
    }
    let fold_at: BTreeMap<u16, Vec<(Signal, bool)>> =
        fold_mc.iter().map(|((k, _), &m)| (m, fold_products[k].clone())).collect();

    // ---- UIM routing
    let mut routing: BTreeMap<usize, Vec<(usize, String, String)>> = BTreeMap::new();
    let mut col_of: BTreeMap<(usize, Source), usize> = BTreeMap::new();
    let mut sel_of: BTreeMap<(usize, usize), Source> = BTreeMap::new();
    for b in 0..f.blocks() {
        let mut need: Vec<(Source, String)> = vec![];
        for m in (b * 16 + 1)..=(b * 16 + 16) {
            let u = &uses[m];
            let plan = u.cell.map(|i| &plans[i]).or_else(|| {
                u.helper_for.and_then(|t| uses[t as usize].cell).map(|i| &plans[i])
            });
            let mut prods: Vec<&Vec<(Signal, bool)>> = vec![];
            if let Some(p) = fold_at.get(&(m as u16)) {
                prods.push(p);
            }
            if let Some(plan) = plan {
                if u.cell.is_some() {
                    prods.extend(plan.pts.values());
                } else {
                    prods.extend(plan.helper_pts.values());
                }
            }
            for p in prods {
                for (s, _) in p {
                    if matches!(s, Signal::Fold(_)) {
                        continue;
                    }
                    let src = source_of(s)?;
                    if !need.iter().any(|x| x.0 == src) {
                        need.push((src, s.to_string()));
                    }
                }
            }
        }
        // a macrocell holding a cell *and* the cascade helper of its neighbour also carries the
        // helper's PTs (the writer puts them there): their inputs need routing too. Added last,
        // so a block whose other PTs already read them keeps its order (and its fuses).
        for m in (b * 16 + 1)..=(b * 16 + 16) {
            let u = &uses[m];
            let (Some(_), Some(t)) = (u.cell, u.helper_for) else { continue };
            let Some(i) = uses[t as usize].cell else { continue };
            for p in plans[i].helper_pts.values() {
                for (s, _) in p {
                    if matches!(s, Signal::Fold(_)) {
                        continue;
                    }
                    let src = source_of(s)?;
                    if !need.iter().any(|x| x.0 == src) {
                        need.push((src, s.to_string()));
                    }
                }
            }
        }
        if std::env::var_os("ATF_FIT_DEBUG").is_some() {
            let mut who: Vec<String> = vec![];
            for m in (b * 16 + 1)..=(b * 16 + 16) {
                if let Some(i) = uses[m].cell {
                    let c = &design.cells[i];
                    who.push(format!("{}{}({})", c.name, if c.is_output { "*" } else { "" }, c.inputs().len()));
                }
                if let Some(i) = uses[m].hosted {
                    who.push(format!("+{}({})", design.cells[i].name, design.cells[i].inputs().len()));
                }
            }
            eprintln!("block {}: {} inputs: {}", atf15xx_arch::block_letter(b), need.len(), who.join(" "));
        }
        // an always-enabled output's pad and feedback carry the same value: either may be routed
        let alt_of = |src: Source| -> Option<Source> {
            if let Source::Pad(mc) = src {
                let u = &uses[mc as usize];
                if u.hosted.is_none() {
                    if let Some(i) = u.cell {
                        let c = &design.cells[i];
                        if c.is_output && c.oe.is_none() && !(c.registered() && c.storage == Storage::Latch) {
                            return Some(Source::Fb(mc));
                        }
                    }
                }
            }
            None
        };
        let alts: Vec<Vec<Source>> = need.iter().map(|x| std::iter::once(x.0).chain(alt_of(x.0)).collect()).collect();
        if std::env::var_os("ATF_FIT_DEBUG").is_some() && route_block_alt(f, &alts).is_none() {
            for (a, (_, name)) in alts.iter().zip(need.iter()) {
                let n: usize = a.iter().map(|s| (0..40).filter(|&m| f.uim_choices(m).contains(s)).count()).sum();
                eprintln!("uim block {b}: {name:?} {a:?}: {n} muxes");
            }
        }
        let assign = route_block_alt(f, &alts).ok_or_else(|| {
            // floating buried cells of this block, most inputs first: candidates to move elsewhere
            let mut movable: Vec<(usize, String)> = ((b * 16 + 1)..=(b * 16 + 16))
                .flat_map(|m| [uses[m].cell, uses[m].hosted])
                .flatten()
                .map(|i| &design.cells[i])
                .filter(|c| !c.is_output && (c.node.is_none() || c.node == Some(0)))
                .map(|c| (c.inputs().len(), c.name.clone()))
                .collect();
            movable.sort_by(|a, b| b.cmp(a));
            // then floating buried cells elsewhere that feed this block: another macrocell puts
            // their feedback on other UIM multiplexers
            let mut feeders: Vec<(usize, String)> = need
                .iter()
                .filter_map(|(src, _)| match src {
                    Source::Fb(m) => uses[*m as usize].cell.map(|i| &design.cells[i]),
                    _ => None,
                })
                .filter(|c| !c.is_output && (c.node.is_none() || c.node == Some(0)))
                .filter(|c| !movable.iter().any(|(_, n)| n == &c.name))
                .map(|c| (c.inputs().len(), c.name.clone()))
                .collect();
            feeders.sort();
            movable.extend(feeders);
            FitError::new(FitErrorKind::Routing, format!(
                "UIM routing failed for block {} ({} distinct inputs) [movable: {}]",
                atf15xx_arch::block_letter(b),
                need.len(),
                movable.iter().map(|x| x.1.clone()).collect::<Vec<_>>().join(",")
            ))
        })?;
        let mut r = vec![];
        for ((src, name), (mux, chosen)) in need.iter().zip(assign) {
            col_of.insert((b, *src), mux);
            sel_of.insert((b, mux), chosen);
            r.push((mux, name.clone(), chosen.vendor_name()));
        }
        r.sort();
        routing.insert(b, r);
    }

    FOLD_MC.with(|x| *x.borrow_mut() = fold_mc.clone());
    // ---- assemble
    let mut fz = Fuses::new(f);
    fz.security = opts.security;
    for a in f.mc_filler_fields() {
        fz.fill(a, 16, true);
    }
    for b in 0..f.blocks() {
        for m in 0..40 {
            fz.fill(f.uim_addr(b, m), f.uim_width(), true);
        }
        for (mux, src) in sel_of.iter().filter(|((bb, _), _)| *bb == b).map(|((_, m), s)| (*m, *s)) {
            let i = f.uim_choices(mux).iter().position(|&x| x == src).unwrap();
            fz.set(f.uim_addr(b, mux) + i, false);
        }
    }
    for n in 1..=6u8 {
        fz.fill(f.goe_addr(n as usize), f.uim_width(), true);
        if let Some((src, _)) = goe.get(&n) {
            let i = f.goe_choices(n as usize).iter().position(|x| x == src).unwrap();
            fz.set(f.goe_addr(n as usize) + i, false);
        }
    }
    // global configuration
    let cfg = f.config_base();
    fz.fill(cfg, 32, true);
    fz.set(f.config_addr(ConfigBit::Arming), false);
    fz.set(f.config_addr(ConfigBit::Termination), !opts.pin_keep);
    fz.set(f.config_addr(ConfigBit::Pd1), !opts.pd1);
    fz.set(f.config_addr(ConfigBit::Pd2), !opts.pd2);
    for n in 1..=6u8 {
        fz.set(f.config_addr(ConfigBit::GoeInvert(n)), !goe.get(&n).map_or(false, |x| x.1));
    }
    fz.set(f.config_addr(ConfigBit::GclrInvert), !gclr_inv.unwrap_or(false));
    // clock networks: source mux and invert (unused networks: the vendor default, own pin, not
    // inverted)
    for n in 1..=3u8 {
        let (k, pos) = gclk_net[n as usize - 1].unwrap_or((n, true));
        let (b0, b1) = atf15xx_arch::decode::gclk_select_for(k);
        fz.set(f.config_addr(ConfigBit::GclkInvert(n)), !pos);
        fz.set(f.config_addr(ConfigBit::GclkMux(n, 0)), b0);
        fz.set(f.config_addr(ConfigBit::GclkMux(n, 1)), b1);
    }
    for n in 1..=3u8 {
        fz.set(f.config_addr(ConfigBit::Itd(n)), !opts.itd[n as usize - 1]);
    }
    fz.set(f.config_addr(ConfigBit::TmsPullup), !opts.tms_pullup);
    fz.set(f.config_addr(ConfigBit::TdiPullup), !opts.tdi_pullup);
    fz.set(f.config_addr(ConfigBit::PowerReset), !opts.power_reset);
    fz.fill(f.special_base(), 4, true);
    fz.set(f.config_addr(ConfigBit::Jtag), opts.jtag);
    fz.fill(f.ues_base(), 16, true);

    // macrocells
    for m in 1..=nmc {
        let u = &uses[m];
        let mut bits: BTreeMap<McBit, bool> = BTreeMap::new();
        let mut set = |b: McBit, v: bool| {
            bits.insert(b, v);
        };
        let mc_name = u.cell.map(|i| design.cells[i].name.clone()).unwrap_or_default();
        let low_power = opts.mc_power.contains(&mc_name) || opts.mc_power == Select::All;
        let fast = opts.output_fast == Select::All || u.cell.map_or(false, |i| opts.output_fast.contains(&design.cells[i].name));
        if let Some(i) = u.cell {
            let c = &design.cells[i];
            let pl = &plans[i];
            encode_logic(c, pl, u.pad_used, dev.mc_pin(m as u16).is_some(), &mut set);
            if opts.observable_registers && c.registered() && !c.is_output && pl.hosted.is_none() && !pl.fast_in {
                set(McBit::OMux, false);
            }
            if let Some(t) = u.helper_for {
                // input register + cascade helper in one macrocell: the register takes D from the
                // pad (D/Q fast path), the PTs all go to the sum, the sum to the cascade
                set(McBit::CasMux, true);
                set(McBit::PtPowerOff, false);
                for k in 1..=5 {
                    set(pt_mux_bit(k), false);
                }
                for (&k, p) in &plans[uses[t as usize].cell.unwrap()].helper_pts {
                    write_pt(&mut fz, f, m, k, p, b_of(m), &col_of, &source_of)?;
                }
            }
            set(McBit::Slew, !fast);
            set(McBit::OpenDrain, opts.open_collector.contains(&c.name));
            for (&k, p) in &pl.pts {
                write_pt(&mut fz, f, m, k, p, b_of(m), &col_of, &source_of)?;
            }
            if let Some(p) = fold_at.get(&(m as u16)) {
                set(McBit::Pt1Mux, true);
                write_pt(&mut fz, f, m, 1, p, b_of(m), &col_of, &source_of)?;
            }
            if let (Some(h), Some(ri)) = (&pl.hosted, u.hosted) {
                // vendor encoding of a doubled macrocell (observed in spi / shift-register)
                let r = &design.cells[ri];
                set(McBit::DMux, false);
                set(McBit::Pt2Mux, false);
                set(McBit::FbMux, false);
                set(McBit::XorB, false);
                set(McBit::Storage, r.storage != Storage::Latch);
                let (g0, g1) = match h.clk {
                    ClkUse::Global(1) => (true, true),
                    ClkUse::Global(3) => (false, true),
                    _ => (false, false),
                };
                set(McBit::Gclk0, g0);
                set(McBit::Gclk1, g1);
                set(McBit::GclrMux, !h.ar_global);
                set(McBit::Pt4Func, false);
                set(McBit::Pt4Mux, h.ce_pt);
                set(McBit::Pt3Mux, h.ar_pt || !pl.in_sum.contains(&3));
            }
        } else if let Some(t) = u.helper_for {
            // cascade helper: all PTs into its sum, sum to cascade output
            let pl = &plans[uses[t as usize].cell.unwrap()];
            encode_unused(u.pad_used, &mut set);
            set(McBit::CasMux, true);
            set(McBit::PtPowerOff, false);
            for k in 1..=5 {
                set(pt_mux_bit(k), false);
            }
            for (&k, p) in &pl.helper_pts {
                write_pt(&mut fz, f, m, k, p, b_of(m), &col_of, &source_of)?;
            }
        } else if let Some(p) = fold_at.get(&(m as u16)) {
            encode_unused(u.pad_used, &mut set);
            set(McBit::Pt1Mux, true);
            set(McBit::PtPowerOff, false);
            write_pt(&mut fz, f, m, 1, p, b_of(m), &col_of, &source_of)?;
        } else {
            encode_unused(u.pad_used, &mut set);
        }
        set(McBit::LowPower, low_power);
        bits.insert(McBit::Slew, !fast);
        for (b, v) in bits {
            fz.set(f.mc_bit_addr(m, b), v);
        }
    }

    for (name, pl) in &place_of {
        placement.push((name.clone(), pl.clone()));
    }
    Ok(FitResult { device: dev, fuses: fz, placement, routing, design })
}

/// Estimated product terms a cell needs in its macrocell (sum + special functions).
fn pt_demand(c: &Cell, xor: Option<&XorSplit>) -> usize {
    let target = if c.active_low { c.data.complement() } else { c.data.clone() };
    let mut n = target.0.len();
    if let Some(x) = xor {
        n = n.min(1 + if c.active_low { x.gbar.0.len() } else { x.g.0.len() });
    }
    if n > 1 {
        let alt = if c.active_low { c.data.0.len() } else { c.data.complement().0.len() };
        n = n.min(alt.max(1));
    }
    if n > 3 {
        // foldback factoring (see factor_with_foldback); the FOLDS registry is reset before planning
        let mut s = target.0.clone();
        while s.len() > 3 {
            match factor_with_foldback(&s) {
                Some(x) => s = x,
                None => break,
            }
        }
        n = n.min(s.len());
    }
    let single_pad = |s: &Option<Sop>| s.as_ref().map_or(false, |x| matches!(x.single_literal(), Some((Signal::Pad(_), _))));
    if c.clk.is_some() && !single_pad(&c.clk) {
        n += 1;
    }
    if c.ce.is_some() {
        n += 1;
    }
    if c.ar.is_some() && !single_pad(&c.ar) {
        n += 1;
    }
    if c.ap.is_some() {
        n += 1;
    }
    if c.oe.as_ref().map_or(false, |x| x.single_literal().is_none() && !x.is_one()) {
        n += 1;
    }
    n
}

/// Assign the floating buried cells (not outputs, not yet placed) to blocks so that every block's
/// distinct UIM inputs stay within 40 and the worst block is as small as possible (spec F9.4).
/// Capacity per block = its macrocells not taken by placed cells or reserved helpers; a cell
/// takes 1 + its cascade helpers. Cost, compared lexicographically: inputs over 40 summed over
/// blocks, the largest block, the sum over blocks. Greedy seed (most inputs first, block whose
/// input set grows least), then deterministic local search with moves and swaps.
fn partition_blocks(
    design: &Design,
    place_of: &BTreeMap<String, Place>,
    reserved: &BTreeSet<u16>,
    demand: &[usize],
    nb: u16,
    avoid: &BTreeSet<(String, u16)>,
    // the attempt's fan-in budget (<= 40): leaves headroom for the UIM matching, which can fail
    // below 40 because each mux selects only some sources
    limit: usize,
) -> BTreeMap<String, u16> {
    let nb = nb as usize;
    // signals interned once (the local search below moves cells thousands of times; hashing
    // signal names on every move dominated the fitter's profile, 2026-11-25)
    let mut ids: BTreeMap<Signal, u32> = BTreeMap::new();
    let cell_ids: Vec<Vec<u32>> = design
        .cells
        .iter()
        .map(|c| {
            uim_inputs(design, c)
                .into_iter()
                .map(|s| {
                    let n = ids.len() as u32;
                    *ids.entry(s).or_insert(n)
                })
                .collect()
        })
        .collect();
    let nsig = ids.len();
    // counts.n[b][signal] = number of cells in block b reading it; counts.distinct[b] = signals > 0
    #[derive(Clone)]
    struct Counts {
        n: Vec<Vec<u32>>,
        distinct: Vec<usize>,
    }
    impl Counts {
        fn add(&mut self, b: usize, s: &[u32]) {
            for &x in s {
                let c = &mut self.n[b][x as usize];
                if *c == 0 {
                    self.distinct[b] += 1;
                }
                *c += 1;
            }
        }
        fn remove(&mut self, b: usize, s: &[u32]) {
            for &x in s {
                let c = &mut self.n[b][x as usize];
                *c -= 1;
                if *c == 0 {
                    self.distinct[b] -= 1;
                }
            }
        }
    }
    let mut counts = Counts { n: vec![vec![0; nsig]; nb], distinct: vec![0; nb] };
    let mut cap: Vec<i64> = vec![16; nb];
    for m in reserved {
        cap[(*m as usize - 1) / 16] -= 1;
    }
    let mut floating: Vec<usize> = vec![];
    for (i, c) in design.cells.iter().enumerate() {
        match place_of.get(&c.name) {
            Some(Place::Pin { mc, .. }) | Some(Place::Node { mc }) => {
                let b = (*mc as usize - 1) / 16;
                counts.add(b, &cell_ids[i]);
                cap[b] -= 1;
            }
            None if !c.is_output => floating.push(i),
            _ => {}
        }
    }
    // macrocells that are placed (by pin) but carry no cell are still free for buried logic
    let size = |i: usize| 1 + if demand[i] > 5 { ((demand[i] - 1) / 5) as i64 } else { 0 };
    // cell index -> block (NONE: not placed by the partitioner)
    const NONE: usize = usize::MAX;
    let mut blk: Vec<usize> = vec![NONE; design.cells.len()];
    let allowed = |i: usize, b: usize| !avoid.contains(&(design.cells[i].name.clone(), b as u16));
    let mut order = floating.clone();
    order.sort_by_key(|&i| std::cmp::Reverse(cell_ids[i].len()));
    for &i in &order {
        let s = &cell_ids[i];
        let mut best: Option<((usize, usize, usize), usize)> = None;
        for b in 0..nb {
            if cap[b] < size(i) || !allowed(i, b) {
                continue;
            }
            let grow = s.iter().filter(|&&x| counts.n[b][x as usize] == 0).count();
            let after = counts.distinct[b] + grow;
            let key = (after.saturating_sub(limit), grow, after);
            if best.as_ref().map_or(true, |(k, _)| key < *k) {
                best = Some((key, b));
            }
        }
        let Some((_, b)) = best else { continue };
        counts.add(b, s);
        cap[b] -= size(i);
        blk[i] = b;
    }
    let cost = |counts: &Counts| -> (usize, usize, usize) {
        let d = &counts.distinct;
        (d.iter().map(|x| x.saturating_sub(limit)).sum(), *d.iter().max().unwrap_or(&0), d.iter().sum())
    };
    let placed: Vec<usize> = order.iter().copied().filter(|&i| blk[i] != NONE).collect();
    if placed.len() > 1 && nb > 1 {
        // several independently seeded local searches from the greedy seed; keep the best (one
        // run alone was found to depend on its random sequence)
        let start = (counts.clone(), cap.clone(), blk.clone());
        let mut best: Option<((usize, usize, usize), Counts, Vec<usize>)> = None;
        for restart in 0..8u64 {
            let (mut counts, mut cap, mut blk) = start.clone();
            let mut cur = cost(&counts);
            use rand::{Rng, SeedableRng};
            let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(restart);
            let mut rnd = |n: usize| rng.random_range(0..n);
            for _ in 0..20000 {
                let i = placed[rnd(placed.len())];
                let from = blk[i];
                let to = rnd(nb);
                if to == from || !allowed(i, to) {
                    continue;
                }
                let si = &cell_ids[i];
                if cap[to] >= size(i) {
                    counts.remove(from, si);
                    counts.add(to, si);
                    let c = cost(&counts);
                    if c <= cur {
                        cur = c;
                        cap[from] += size(i);
                        cap[to] -= size(i);
                        blk[i] = to;
                    } else {
                        counts.remove(to, si);
                        counts.add(from, si);
                    }
                } else {
                    // swap with a cell of the target block
                    let cands: Vec<usize> = placed.iter().copied().filter(|&j| blk[j] == to).collect();
                    if cands.is_empty() {
                        continue;
                    }
                    let j = cands[rnd(cands.len())];
                    if !allowed(j, from) || cap[from] + size(i) - size(j) < 0 || cap[to] + size(j) - size(i) < 0 {
                        continue;
                    }
                    let sj = &cell_ids[j];
                    counts.remove(from, si);
                    counts.remove(to, sj);
                    counts.add(to, si);
                    counts.add(from, sj);
                    let c = cost(&counts);
                    if c <= cur {
                        cur = c;
                        cap[from] += size(i) - size(j);
                        cap[to] += size(j) - size(i);
                        blk[i] = to;
                        blk[j] = from;
                    } else {
                        counts.remove(to, si);
                        counts.remove(from, sj);
                        counts.add(from, si);
                        counts.add(to, sj);
                    }
                }
            }
            if best.as_ref().map_or(true, |(c, _, _)| cur < *c) {
                best = Some((cur, counts, blk));
            }
        }
        let (_, c, b) = best.unwrap();
        counts = c;
        blk = b;
        if std::env::var_os("ATF_FIT_DEBUG").is_some() {
            eprintln!("partition: {} floating cells, per-block inputs {:?}", placed.len(), counts.distinct);
        }
    }
    blk.into_iter().enumerate().filter(|(_, b)| *b != NONE).map(|(i, b)| (design.cells[i].name.clone(), b as u16)).collect()
}

/// UIM sources a signal can be routed from, given the current placement (before PT planning).
fn est_sources(design: &Design, place_of: &BTreeMap<String, Place>, s: &Signal) -> Vec<Source> {
    match s {
        Signal::Pad(n) => match place_of.get(n) {
            Some(Place::Pin { mc, .. }) => vec![Source::Pad(*mc)],
            Some(Place::Dedicated { func, .. }) => vec![match func {
                Dedicated::Gclk1 => Source::Gclk1,
                Dedicated::Gclr => Source::Gclr,
                Dedicated::Oe1 => Source::Oe1,
                Dedicated::Gclk2Oe2 => Source::Oe2,
            }],
            _ => vec![],
        },
        Signal::Fb(n, _) => match place_of.get(n) {
            Some(Place::Pin { mc, .. }) if design.cell(n).map_or(false, |c| c.oe.is_none()) => vec![Source::Pad(*mc), Source::Fb(*mc)],
            Some(Place::Pin { mc, .. }) | Some(Place::Node { mc }) => vec![Source::Fb(*mc)],
            _ => vec![],
        },
        Signal::Fold(_) => vec![],
    }
}

/// Number of sources of one block that cannot be matched onto its 40 UIM muxes (maximum
/// bipartite matching, as in [`route_block_alt`], counting the failures instead of stopping).
fn uim_unmatched(f: Family, need: &[Vec<Source>]) -> usize {
    let cands: Vec<Vec<usize>> = need
        .iter()
        .map(|alts| {
            let mut v: Vec<usize> = alts.iter().flat_map(|s| (0..40).filter(move |&m| f.uim_choices(m).contains(s))).collect();
            v.sort();
            v.dedup();
            v
        })
        .collect();
    let mut owner = [None::<usize>; 40];
    fn augment(i: usize, cands: &[Vec<usize>], owner: &mut [Option<usize>; 40], seen: &mut [bool; 40]) -> bool {
        for &m in &cands[i] {
            if !seen[m] {
                seen[m] = true;
                if owner[m].map_or(true, |j| augment(j, cands, owner, seen)) {
                    owner[m] = Some(i);
                    return true;
                }
            }
        }
        false
    }
    (0..need.len()).filter(|&i| !augment(i, &cands, &mut owner, &mut [false; 40])).count()
}

/// Local search over the macrocell positions of `movable` buried cells (within their blocks:
/// moves to free macrocells and swaps), minimising the unmatched UIM sources summed over blocks
/// (spec F4.6a). Block membership, and so each block's set of signals, stays the same, so a move
/// only re-matches the blocks that read a moved cell. Stops at zero, after 500 moves without
/// improvement, or when the start is hopeless (more than 4 unmatched per block on average).
fn optimize_positions(design: &Design, place_of: &mut BTreeMap<String, Place>, movable: &[String], reserved: &BTreeSet<u16>, f: Family) {
    use rand::{Rng, SeedableRng};
    if movable.is_empty() {
        return;
    }
    let nb = (f.macrocells() / 16) as usize;
    let mc_of = |p: &BTreeMap<String, Place>, n: &str| match p.get(n) {
        Some(Place::Pin { mc, .. }) | Some(Place::Node { mc }) => Some(*mc),
        _ => None,
    };
    let blk = |mc: u16| ((mc - 1) / 16) as usize;
    // signals read by each block, and the blocks reading each movable cell
    let mut sig: Vec<BTreeSet<Signal>> = vec![BTreeSet::new(); nb];
    for c in &design.cells {
        if let Some(mc) = mc_of(place_of, &c.name) {
            sig[blk(mc)].extend(uim_inputs(design, c));
        }
    }
    let sig: Vec<Vec<Signal>> = sig.into_iter().map(|s| s.into_iter().collect()).collect();
    let readers: BTreeMap<&str, Vec<usize>> = movable
        .iter()
        .map(|n| {
            let r = (0..nb).filter(|&b| sig[b].iter().any(|s| matches!(s, Signal::Fb(m, _) if m == n))).collect();
            (n.as_str(), r)
        })
        .collect();
    let block_cost = |p: &BTreeMap<String, Place>, b: usize| -> usize {
        let need: Vec<Vec<Source>> = sig[b].iter().map(|s| est_sources(design, p, s)).filter(|v| !v.is_empty()).collect();
        uim_unmatched(f, &need)
    };
    let mut costs: Vec<usize> = (0..nb).map(|b| block_cost(place_of, b)).collect();
    let mut cur: usize = costs.iter().sum();
    let start = cur;
    if cur == 0 || cur > 4 * nb {
        return;
    }
    let original = place_of.clone();
    let mut at: BTreeMap<u16, String> =
        design.cells.iter().filter_map(|c| mc_of(place_of, &c.name).map(|m| (m, c.name.clone()))).collect();
    let is_movable: BTreeSet<&str> = movable.iter().map(|s| s.as_str()).collect();
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(1);
    let mut stale = 0;
    while cur > 0 && stale < 500 {
        stale += 1;
        let a = &movable[rng.random_range(0..movable.len())];
        let ma = mc_of(place_of, a).unwrap();
        let target = (blk(ma) * 16 + 1 + rng.random_range(0..16)) as u16;
        if target == ma || reserved.contains(&target) {
            continue;
        }
        let other = at.get(&target).cloned();
        if other.as_deref().map_or(false, |o| !is_movable.contains(o)) {
            continue;
        }
        let apply = |p: &mut BTreeMap<String, Place>, x: u16, y: u16| {
            p.insert(a.clone(), Place::Node { mc: x });
            if let Some(o) = &other {
                p.insert(o.clone(), Place::Node { mc: y });
            }
        };
        apply(place_of, target, ma);
        let mut touched: BTreeSet<usize> = readers[a.as_str()].iter().copied().collect();
        if let Some(o) = &other {
            touched.extend(readers[o.as_str()].iter().copied());
        }
        let new: Vec<(usize, usize)> = touched.iter().map(|&b| (b, block_cost(place_of, b))).collect();
        let next = cur - new.iter().map(|&(b, _)| costs[b]).sum::<usize>() + new.iter().map(|&(_, c)| c).sum::<usize>();
        if next <= cur {
            if next < cur {
                stale = 0;
            }
            cur = next;
            for (b, c) in new {
                costs[b] = c;
            }
            at.insert(target, a.clone());
            match &other {
                Some(o) => {
                    at.insert(ma, o.clone());
                }
                None => {
                    at.remove(&ma);
                }
            }
        } else {
            apply(place_of, ma, target);
        }
    }
    if std::env::var_os("ATF_FIT_DEBUG").is_some() {
        eprintln!("positions: {start} -> {cur} unmatched UIM sources{}", if cur > 0 { " (kept the original)" } else { "" });
    }
    if cur > 0 {
        *place_of = original;
    }
}

/// Place every eligible register into the macrocell of the input pin that is its D (see
/// `Options::pack_input_registers`); records them in `fast_in`.
fn pack_input_registers(
    design: &Design,
    place_of: &mut BTreeMap<String, Place>,
    reserved: &BTreeSet<u16>,
    fanin_limit: usize,
    avoid: &BTreeSet<(String, u16)>,
    fast_in: &mut BTreeSet<String>,
    share: &mut BTreeSet<u16>,
) {
    for c in &design.cells {
        if c.is_output || place_of.contains_key(&c.name) || !matches!(c.storage, Storage::Dff | Storage::Latch) || c.active_low {
            continue;
        }
        let Some((Signal::Pad(p), true)) = c.data.single_literal() else { continue };
        let Some(&Place::Pin { mc, .. }) = place_of.get(p) else { continue };
        let taken = design.cells.iter().any(|o| matches!(place_of.get(&o.name), Some(Place::Pin { mc: m, .. } | Place::Node { mc: m }) if *m == mc));
        // a macrocell reserved as a cascade helper still takes a PT-free input register
        if !design.inputs.contains_key(p) || taken || (reserved.contains(&mc) && !pt_free_input_register(c)) {
            continue;
        }
        if !fanin_ok(design, place_of, (mc - 1) / 16, c, fanin_limit) || avoid.contains(&(c.name.clone(), (mc - 1) / 16)) {
            continue;
        }
        place_of.insert(c.name.clone(), Place::Node { mc });
        fast_in.insert(c.name.clone());
        if pt_free_input_register(c) {
            share.insert(mc);
        }
    }
}

/// Can the cascade helpers of a cell with `demand` PTs placed at `mc` go below it? `share`: MCs
/// whose only occupant is a PT-free input register (D from the pad): their whole PT array and
/// sum are free, so they can lend their sum (spec F4.6a, packing P2+).
fn helpers_free(mc: u16, demand: usize, place_of: &BTreeMap<String, Place>, reserved: &BTreeSet<u16>, share: &BTreeSet<u16>) -> bool {
    if demand <= 5 {
        return true;
    }
    let k = ((demand - 1) / 5) as u16;
    (1..=k).all(|j| {
        mc > j
            && (mc - j - 1) / 16 == (mc - 1) / 16
            && !reserved.contains(&(mc - j))
            && (share.contains(&(mc - j))
                || !place_of.values().any(|p| matches!(p, Place::Pin { mc: m, .. } | Place::Node { mc: m } if *m == mc - j)))
    })
}

/// An input register that will need no product term: D from the pad (fast path), clocked by a
/// clock pin, no clock enable or preset, reset at most from a pin (GCLR). Its macrocell's PTs,
/// sum and cascade output stay free for a cascade helper or a foldback.
fn pt_free_input_register(c: &Cell) -> bool {
    let pin_literal = |s: &Option<Sop>| s.as_ref().map_or(true, |s| matches!(s.single_literal(), Some((Signal::Pad(_), _))));
    c.clk.is_some() && pin_literal(&c.clk) && pin_literal(&c.ar) && c.ce.is_none() && c.ap.is_none()
}

fn reserve_helpers(mc: u16, demand: usize, reserved: &mut BTreeSet<u16>) {
    if demand > 5 {
        for j in 1..=((demand - 1) / 5) as u16 {
            reserved.insert(mc - j);
        }
    }
}

/// Would placing `c` in block `b` keep the block's distinct inputs within `limit`?
fn fanin_ok(design: &Design, place_of: &BTreeMap<String, Place>, b: u16, c: &Cell, limit: usize) -> bool {
    // the union of interned input ids in a reusable bitset (spec F9.11: this check runs for
    // every candidate macrocell, and rebuilding string sets dominated the fitter's profile)
    let mut ids: Vec<std::rc::Rc<[u32]>> = vec![uim_ids(design, c)];
    for o in &design.cells {
        if let Some(Place::Pin { mc, .. } | Place::Node { mc }) = place_of.get(&o.name) {
            if (mc - 1) / 16 == b {
                ids.push(uim_ids(design, o));
            }
        }
    }
    UIM_MEMO.with(|m| {
        let mut m = m.borrow_mut();
        let n = m.intern.len();
        let bits = &mut m.bits;
        bits.clear();
        bits.resize(n / 64 + 1, 0);
        let mut count = 0;
        for l in &ids {
            for &x in l.iter() {
                let (w, k) = (x as usize / 64, x % 64);
                if bits[w] >> k & 1 == 0 {
                    bits[w] |= 1 << k;
                    count += 1;
                }
            }
        }
        count <= limit
    })
}

// Memoised UIM inputs as interned ids (spec F9.11), cleared at every fit attempt (the netlist's
// foldbacks can change between designs). Keyed by the cell's name and a hash of its functions,
// so a cell rewritten during an attempt (a hosted T flip-flop becoming D + CE) gets a new entry.
#[derive(Default)]
struct UimMemo {
    lists: rustc_hash::FxHashMap<(String, u64), std::rc::Rc<[u32]>>,
    intern: rustc_hash::FxHashMap<Signal, u32>,
    bits: Vec<u64>,
}
thread_local! {
    static UIM_MEMO: std::cell::RefCell<UimMemo> = std::cell::RefCell::new(UimMemo::default());
}

fn uim_memo_clear() {
    UIM_MEMO.with(|m| {
        let mut m = m.borrow_mut();
        m.lists.clear();
        m.intern.clear();
    });
}

fn uim_ids(design: &Design, c: &Cell) -> std::rc::Rc<[u32]> {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    (&c.data, &c.clk, &c.ce, &c.ar, &c.ap, &c.oe).hash(&mut h);
    let key = (c.name.clone(), h.finish());
    if let Some(l) = UIM_MEMO.with(|m| m.borrow().lists.get(&key).cloned()) {
        return l;
    }
    let sigs = uim_inputs(design, c);
    UIM_MEMO.with(|m| {
        let mut m = m.borrow_mut();
        let ids: Vec<u32> = sigs
            .into_iter()
            .map(|s| {
                let n = m.intern.len() as u32;
                *m.intern.entry(s).or_insert(n)
            })
            .collect();
        let l: std::rc::Rc<[u32]> = ids.into();
        m.lists.insert(key, l.clone());
        l
    })
}

/// Signals a cell needs on its block's UIM: its inputs, with netlist-level foldbacks replaced by
/// the signals of their products (a foldback is hosted in the reading block, its product reads
/// the UIM there).
fn uim_inputs(design: &Design, c: &Cell) -> Vec<Signal> {
    let mut out: BTreeSet<Signal> = BTreeSet::new();
    let mut stack: Vec<Signal> = c.inputs();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    while let Some(s) = stack.pop() {
        match &s {
            Signal::Fold(k) => {
                if seen.insert(k.clone()) {
                    if let Some(p) = design.folds.get(k) {
                        stack.extend(p.iter().map(|(x, _)| x.clone()));
                    }
                }
            }
            _ => {
                out.insert(s);
            }
        }
    }
    out.into_iter().collect()
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

thread_local! {
    static FOLD_MC: std::cell::RefCell<BTreeMap<(String, usize), u16>> = Default::default();
    static FOLDS: std::cell::RefCell<BTreeMap<String, Vec<(Signal, bool)>>> = Default::default();
}

/// One factoring step: find the literal shared by the most products whose remainders are single
/// literals; replace those products by `literal & FOLD(product of the negated remainders)`.
fn factor_with_foldback(sum: &[Vec<(Signal, bool)>]) -> Option<Vec<Vec<(Signal, bool)>>> {
    // De Morgan factoring with the block's foldback NAND terms: products that share a common
    // cube C and differ in one further literal each, `C&r1 # C&r2 # ... # C&rk`, are
    // `C & (r1 + ... + rk)` = `C & !(!r1 & ... & !rk)`: one foldback PT (the NAND) replaces k
    // products, however wide the OR. The vendor does this for |C| = 1 (`L&a # L&b # L&!c`); here C
    // is any common cube (including the empty cube: a plain OR of literals). The grouping that
    // removes the most products wins; ties prefer the vendor's |C| = 1 form.
    let no_fold = |p: &Vec<(Signal, bool)>| p.iter().all(|(s, _)| !matches!(s, Signal::Fold(_)));
    // candidate cubes: each product minus one literal
    let mut cands: Vec<Vec<(Signal, bool)>> = vec![];
    for p in sum.iter().filter(|p| no_fold(p)) {
        for i in 0..p.len() {
            let mut c = p.clone();
            c.remove(i);
            c.sort();
            cands.push(c);
        }
    }
    cands.sort();
    cands.dedup();
    let mut best: Option<(Vec<(Signal, bool)>, Vec<usize>, (usize, bool))> = None;
    for c in cands {
        let members: Vec<usize> = sum
            .iter()
            .enumerate()
            .filter(|(_, p)| no_fold(p) && p.len() == c.len() + 1 && c.iter().all(|l| p.contains(l)))
            .map(|(i, _)| i)
            .collect();
        if members.len() < 2 {
            continue;
        }
        // k products become one: saving k - 1; prefer the vendor's single-literal common cube
        let score = (members.len() - 1, c.len() == 1);
        if best.as_ref().map_or(true, |b| score > b.2) {
            best = Some((c, members, score));
        }
    }
    let (c, members, _) = best?;
    let mut fold: Vec<(Signal, bool)> = members
        .iter()
        .map(|&i| sum[i].iter().find(|x| !c.contains(x)).cloned().map(|(s, p)| (s, !p)).unwrap())
        .collect();
    fold.sort();
    fold.dedup();
    if fold.windows(2).any(|w| w[0].0 == w[1].0) {
        return None; // x & !x: the remainder sum is a tautology; leave it to the minimiser
    }
    let key = fold.iter().map(|(s, p)| format!("{}{}", if *p { "" } else { "!" }, s)).collect::<Vec<_>>().join("&");
    FOLDS.with(|f| f.borrow_mut().insert(key.clone(), fold));
    let mut out: Vec<Vec<(Signal, bool)>> =
        sum.iter().enumerate().filter(|(i, _)| !members.contains(i)).map(|(_, p)| p.clone()).collect();
    let mut np = c;
    np.push((Signal::Fold(key), true));
    np.sort();
    out.push(np);
    Some(out)
}

/// A special-function product term. If the function needs several products but its complement
/// is one product `P`, use the foldback expander `!P` (what the vendor fitter does).
fn one_product(s: &Sop, cell: &str, what: &str) -> Result<Vec<(Signal, bool)>, FitError> {
    let m = if s.0.len() > 1 { s.minimize() } else { s.clone() };
    match m.0.len() {
        1 => Ok(m.0[0].clone()),
        0 => Ok(vec![]),
        _ => {
            let c = m.complement();
            let c = if c.0.iter().flatten().any(|(s, p)| !p && matches!(s, Signal::Fold(_))) { Sop(vec![vec![]; 2]) } else { c };
            let key_of = |p: &[(Signal, bool)]| p.iter().map(|(s, p)| format!("{}{}", if *p { "" } else { "!" }, s)).collect::<Vec<_>>().join("&");
            if c.0.len() == 1 {
                let key = key_of(&c.0[0]);
                FOLDS.with(|f| f.borrow_mut().insert(key.clone(), c.0[0].clone()));
                Ok(vec![(Signal::Fold(key), true)])
            } else if m.0.len() <= 15 && m.0.iter().all(|p| p.iter().all(|(s, v)| *v || !matches!(s, Signal::Fold(_)))) {
                // De Morgan with two foldback levels: p1 + ... + pk = !(!p1 & ... & !pk), where
                // each !pi is the foldback of pi and the outer NAND is a foldback reading them:
                // k + 1 foldback PTs of the block (at most 16) instead of a buried node.
                let inner: Vec<(Signal, bool)> = m
                    .0
                    .iter()
                    .map(|p| match p.as_slice() {
                        // !l for a single literal comes straight from the UIM
                        [(s, v)] if !matches!(s, Signal::Fold(_)) => (s.clone(), !v),
                        _ => {
                            let k = key_of(p);
                            FOLDS.with(|f| f.borrow_mut().insert(k.clone(), p.clone()));
                            (Signal::Fold(k), true)
                        }
                    })
                    .collect();
                let outer = format!("nand({})", inner.iter().map(|(s, _)| s.to_string()).collect::<Vec<_>>().join(","));
                FOLDS.with(|f| f.borrow_mut().insert(outer.clone(), inner));
                Ok(vec![(Signal::Fold(outer), true)])
            } else {
                err(FitErrorKind::Other, format!("{cell}: {what} needs more than one product term ({} products: {})", m.0.len(), m))
            }
        }
    }
}

fn pt_mux_bit(k: usize) -> McBit {
    match k {
        1 => McBit::Pt1Mux,
        2 => McBit::Pt2Mux,
        3 => McBit::Pt3Mux,
        4 => McBit::Pt4Mux,
        _ => McBit::Pt5Mux,
    }
}

#[allow(clippy::too_many_arguments)]
fn write_pt(
    fz: &mut Fuses,
    f: Family,
    mc: usize,
    k: usize,
    p: &[(Signal, bool)],
    b: usize,
    col_of: &BTreeMap<(usize, Source), usize>,
    source_of: &dyn Fn(&Signal) -> Result<Source, FitError>,
) -> Result<(), FitError> {
    let a = f.pt_addr(mc, k);
    fz.fill(a, 96, true);
    for (s, pos) in p {
        if let Signal::Fold(k) = s {
            if !pos {
                return err(FitErrorKind::Other, format!("internal: inverted foldback literal {k} in a product term"));
            }
            let Some(m) = FOLD_MC.with(|x| x.borrow().get(&(k.clone(), b)).copied()) else {
                return err(FitErrorKind::Other, format!("internal: MC{mc} reads foldback {k}, which has no expander in block {}", atf15xx_arch::block_letter(b)));
            };
            fz.set(a + f.pt_foldback_column((m as usize - 1) % 16), false);
            continue;
        }
        let src = source_of(s)?;
        let Some(&mux) = col_of.get(&(b, src)) else {
            return err(FitErrorKind::Other, format!("internal: PT{k} of MC{mc} reads {s}, which is not routed into block {}", atf15xx_arch::block_letter(b)));
        };
        fz.set(a + f.pt_input_column(mux, !pos), false);
    }
    Ok(())
}

/// Unused macrocell, or one whose pad is only read as an input.
fn encode_unused(pad_used: bool, set: &mut dyn FnMut(McBit, bool)) {
    use McBit::*;
    // observed vendor patterns (see docs/architecture/fitter/04-fuse-map.md)
    let unused = [
        (OeMux0, false), (OeMux1, false), (Pt5Func, true), (OeMux2, false), (OMux, true), (GclrMux, true),
        (Storage, false), (DMux, true), (Slew, true), (Pt4Func, true), (Pt1Mux, false), (CasMux, false),
        (Gclk0, false), (Gclk1, false), (PtPowerOff, true), (Pt2Mux, false), (XorB, false), (Pt5Mux, false),
        (Invert, false), (Pt3Mux, false), (Pt4Mux, false), (OpenDrain, false), (FbMux, true),
    ];
    for (b, v) in unused {
        set(b, v);
    }
    if pad_used {
        for (b, v) in [(Pt5Func, false), (Storage, true), (CasMux, true), (Pt5Mux, true), (Pt3Mux, true), (Pt4Mux, true)] {
            set(b, v);
        }
    }
}

fn encode_logic(c: &Cell, pl: &PtPlan, pad_used: bool, _bonded: bool, set: &mut dyn FnMut(McBit, bool)) {
    use McBit::*;
    let reg = c.registered();
    // output enable
    let oe_code: u8 = match pl.oe {
        OeUse::Always | OeUse::Pt => 7,
        OeUse::Global(n) => [4, 2, 6, 1, 5, 3][n as usize - 1],
        OeUse::Off => {
            // the vendor's buried-cell encoding is code 7 with `Pt5Func` = 0, i.e. "OE = PT5's
            // special line"; with PT5 steered into the sum that line is (inferred) 0, but
            // decode/simulator readings differ (F2.8 rule 13), so use the unambiguous code 0 then
            if pad_used || pl.in_sum.contains(&5) {
                0
            } else {
                7
            }
        }
    };
    set(OeMux0, oe_code & 1 != 0);
    set(OeMux1, oe_code & 2 != 0);
    set(OeMux2, oe_code & 4 != 0);
    let pt5_func = match pl.oe {
        OeUse::Always => true,
        OeUse::Pt | OeUse::Global(_) | OeUse::Off => pl.ap_pt,
    };
    set(Pt5Func, pt5_func);
    set(OMux, !reg || !c.is_output);
    set(GclrMux, !pl.ar_global);
    set(McBit::Storage, c.storage != crate::netlist::Storage::Latch);
    set(DMux, true);
    let pt4_special = pl.clk == ClkUse::Pt || pl.ce_pt || (!reg && !pl.in_sum.contains(&4));
    set(Pt4Func, if pl.ce_pt { false } else { pl.clk == ClkUse::Pt || !reg });
    set(Pt4Mux, pt4_special);
    set(Pt1Mux, false);
    set(CasMux, pl.constant == Some(true));
    let (g0, g1) = match pl.clk {
        ClkUse::Global(1) => (true, true),
        ClkUse::Global(3) => (false, true),
        _ => (false, false),
    };
    set(Gclk0, g0);
    set(Gclk1, g1);
    // power the PTs down only if none is used: a constant output can still have a PT clock,
    // reset or output enable (e.g. an open-drain `assign irq_n = irq ? 1'b0 : 1'bz`)
    set(PtPowerOff, pl.constant.is_some() && pl.pts.is_empty() && pl.helper_pts.is_empty());
    set(Pt2Mux, pl.xor);
    set(XorB, c.storage == crate::netlist::Storage::Tff);
    set(Pt5Mux, !pl.in_sum.contains(&5));
    set(Invert, pl.invert);
    set(Pt3Mux, !pl.in_sum.contains(&3));
    set(FbMux, !reg);
    if pl.fast_in {
        // D/Q group: d_mux fast + dfast/o_mux pad: FF.D = pad, IO.A = Q (the pin is an input,
        // its OE stays off), feedback = Q
        set(DMux, false);
        set(OMux, false);
    }
}

/// Bipartite matching of sources onto the 40 UIM muxes of one block.
/// Like [`route_block`], but each requirement may be met by any of its alternative sources (the
/// first is preferred). Returns `(mux, source used)` per requirement.
pub fn route_block_alt(f: Family, need: &[Vec<Source>]) -> Option<Vec<(usize, Source)>> {
    let cands: Vec<Vec<(usize, Source)>> = need
        .iter()
        .map(|alts| alts.iter().flat_map(|s| (0..40).filter(|&m| f.uim_choices(m).contains(s)).map(move |m| (m, *s))).collect())
        .collect();
    let mut owner: [Option<(usize, Source)>; 40] = [None; 40];
    fn augment(i: usize, cands: &[Vec<(usize, Source)>], owner: &mut [Option<(usize, Source)>; 40], seen: &mut [bool; 40]) -> bool {
        for &(m, s) in &cands[i] {
            if seen[m] {
                continue;
            }
            seen[m] = true;
            if owner[m].map_or(true, |(j, _)| augment(j, cands, owner, seen)) {
                owner[m] = Some((i, s));
                return true;
            }
        }
        false
    }
    for i in 0..need.len() {
        let mut seen = [false; 40];
        if !augment(i, &cands, &mut owner, &mut seen) {
            return None;
        }
    }
    let mut out = vec![(0, Source::Gnd); need.len()];
    for (m, o) in owner.iter().enumerate() {
        if let Some((i, s)) = o {
            out[*i] = (m, *s);
        }
    }
    Some(out)
}

pub fn route_block(f: Family, need: &[Source]) -> Option<Vec<usize>> {
    let cands: Vec<Vec<usize>> =
        need.iter().map(|s| (0..40).filter(|&m| f.uim_choices(m).contains(s)).collect()).collect();
    let mut owner: [Option<usize>; 40] = [None; 40];
    fn augment(i: usize, cands: &[Vec<usize>], owner: &mut [Option<usize>; 40], seen: &mut [bool; 40]) -> bool {
        for &m in &cands[i] {
            if seen[m] {
                continue;
            }
            seen[m] = true;
            if owner[m].map_or(true, |j| augment(j, cands, owner, seen)) {
                owner[m] = Some(i);
                return true;
            }
        }
        false
    }
    for i in 0..need.len() {
        let mut seen = [false; 40];
        if !augment(i, &cands, &mut owner, &mut seen) {
            return None;
        }
    }
    let mut out = vec![0; need.len()];
    for (m, o) in owner.iter().enumerate() {
        if let Some(i) = o {
            out[*i] = m;
        }
    }
    Some(out)
}

/// The block-assignment problem of a fit, for exact solvers (spec F4.6c; the ILP bound
/// `fit-partition-bound` in `atf15xx-synth`, feature `ilp`): each item (a cell, or the cells sharing one macrocell) with the UIM
/// signals it reads (as the fitter counts them, [`uim_inputs`]), the macrocells it occupies
/// (itself plus its cascade helpers) and, if it drives a pin, the block it is fixed to; per
/// block the macrocells available to items (16 minus foldback-only hosts), and the block each
/// item has in this fit.
#[derive(Clone, Debug)]
pub struct BlockProblem {
    pub blocks: usize,
    pub capacity: Vec<usize>,
    pub items: Vec<BlockItem>,
}

#[derive(Clone, Debug)]
pub struct BlockItem {
    pub cells: Vec<String>,
    pub inputs: Vec<String>,
    pub mcs: usize,
    pub fixed: Option<usize>,
    pub placed: usize,
}

impl BlockProblem {
    /// Distinct UIM inputs of each block for an assignment (item -> block).
    pub fn fan_in(&self, assign: &[usize]) -> Vec<usize> {
        let mut sets: Vec<BTreeSet<&str>> = vec![BTreeSet::new(); self.blocks];
        for (it, &b) in self.items.iter().zip(assign) {
            sets[b].extend(it.inputs.iter().map(|s| s.as_str()));
        }
        sets.iter().map(|s| s.len()).collect()
    }
}

pub fn block_problem(r: &FitResult) -> BlockProblem {
    let f = r.device.family;
    let nmc = f.macrocells();
    let dec = atf15xx_arch::decode::decode(f, &r.fuses.bits);
    let mut at: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    let mut pinned: BTreeSet<u16> = BTreeSet::new();
    for (name, p) in &r.placement {
        let (mc, pin) = match p {
            Place::Pin { mc, .. } => (*mc, true),
            Place::Node { mc } => (*mc, false),
            Place::Dedicated { .. } => continue,
        };
        if let Some(i) = r.design.cell_index(name) {
            at.entry(mc).or_default().push(i);
            if pin {
                pinned.insert(mc);
            }
        }
    }
    let has_cell = |m: u16| at.contains_key(&m);
    // cascade helpers: MCs below a cell that pass their sum up (cascade_out) and hold no cell
    let helpers = |mc: u16| -> usize {
        let mut k = 0;
        let mut m = mc;
        while m > 1 && (m - 2) / 16 == (mc - 1) / 16 && !has_cell(m - 1) && dec.macrocells[(m - 2) as usize].cascade_out {
            k += 1;
            m -= 1;
        }
        k
    };
    let helper_mcs: BTreeSet<u16> = at.keys().flat_map(|&mc| (mc - helpers(mc) as u16..mc).collect::<Vec<_>>()).collect();
    let blocks = nmc / 16;
    let mut capacity = vec![16usize; blocks];
    for m in 1..=nmc as u16 {
        let c = &dec.macrocells[(m - 1) as usize];
        let used = c.pt_use.iter().any(|u| !matches!(u, atf15xx_arch::decode::PtUse::Off));
        if used && !has_cell(m) && !helper_mcs.contains(&m) {
            capacity[((m - 1) / 16) as usize] -= 1; // a foldback-only host
        }
    }
    let items = at
        .iter()
        .map(|(&mc, cells)| {
            let mut ins: BTreeSet<String> = BTreeSet::new();
            for &i in cells {
                for s in uim_inputs(&r.design, &r.design.cells[i]) {
                    ins.insert(format!("{s:?}"));
                }
            }
            let b = ((mc - 1) / 16) as usize;
            BlockItem {
                cells: cells.iter().map(|&i| r.design.cells[i].name.clone()).collect(),
                inputs: ins.into_iter().collect(),
                mcs: 1 + helpers(mc),
                fixed: pinned.contains(&mc).then_some(b),
                placed: b,
            }
        })
        .collect();
    BlockProblem { blocks, capacity, items }
}
