//! Static timing analysis of a fitted ATF15xx fuse map (spec `docs/architecture/fitter/08-timing.md`
//! §F8.5): register-to-register period and fmax, pin-to-pin delays (tPD), clock-to-output (tCO)
//! and input setup (tSU), from the decoded configuration and the vendor speed-grade tables
//! (`atf15xx_arch::timing`, whole ns as the vendor truncates them).
//!
//! Path model (F8.2): a signal enters its block's PTs through the UIM (tUIM); a sum adds tLAD,
//! each cascade stage tPEXP, a foldback line tSEXP; a combinatorial feedback adds tCOMB before
//! the next UIM pass; a register launches at tRD after its clock and captures with tSU; global
//! clocks arrive at tIN + tGLOB, PT clocks at their PT's arrival + tIC; outputs add tOD1/tOD2
//! (fast slew at 5 V / 3.3 V) or tOD3 (slow slew). `LowPower` macrocells add tRPA to tLAD,
//! tLAC, tIC and tSEXP. Each reported value is at least the datasheet's pin-to-pin value of its
//! class (tCNT, tPD1, tCOP/tACOP, tSU_global; F8.2.1).
//!
//! Approximations (documented in F8.5): all global clock networks are taken to arrive together
//! (no skew); a register clocked by a PT is timed against its own clock arrival; async
//! reset/preset paths and latches are not timed; a pin whose macrocell drives it is read as an
//! external input.

use crate::model::{Clock, Lit, Model, Oe, Sig};
use atf15xx_arch::decode::{DSource, PtUse, Storage};
use std::collections::BTreeMap;

/// Speed-grade parameters of one family (ns).
#[derive(Clone, Debug)]
pub struct Params {
    pub grade: String,
    values: BTreeMap<&'static str, f64>,
}

impl Params {
    /// Parameters for `grade` (e.g. `-10` or `10`) of the model's family (`atf15xx_arch::timing`).
    pub fn for_model(model: &Model, grade: &str) -> Result<Params, String> {
        use atf15xx_arch::timing;
        let gi = timing::grade_index(grade).ok_or_else(|| format!("unknown speed grade {grade} (known: {})", timing::GRADES.join(" ")))?;
        let values = timing::params(model.family).iter().map(|(k, v)| (*k, v[gi])).collect();
        Ok(Params { grade: timing::GRADES[gi].to_string(), values })
    }

    pub fn get(&self, name: &str) -> f64 {
        self.values.get(name).copied().unwrap_or(0.0)
    }
}

/// Supply voltage of the output banks (selects tOD1 or tOD2 for fast-slew outputs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vccio {
    V5,
    V3_3,
}

/// One timed path: its delay, the UIM passes on it, and where it starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arrival {
    pub ns: f64,
    pub passes: u32,
    /// Launching macrocell (register) or pin, for the report.
    pub from: From,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum From {
    Reg(u16),
    Pin(u16),
    Pad(u16),
}

impl Arrival {
    fn later(a: Option<Arrival>, b: Option<Arrival>) -> Option<Arrival> {
        match (a, b) {
            (Some(x), Some(y)) => Some(if y.ns > x.ns || (y.ns == x.ns && y.passes > x.passes) { y } else { x }),
            (x, None) => x,
            (None, y) => y,
        }
    }
    fn plus(self, ns: f64, passes: u32) -> Arrival {
        Arrival { ns: self.ns + ns, passes: self.passes + passes, from: self.from }
    }
}

/// A register's worst path (data or clock enable) in one analysis.
#[derive(Clone, Debug)]
pub struct RegPath {
    pub mc: u16,
    /// Pin of the clock that captures this path (global clock networks), for per-clock targets.
    pub clock_pin: Option<u16>,
    /// Required clock period (register to register) or setup time (pin to register), ns.
    pub ns: f64,
    pub passes: u32,
    pub from: From,
    pub what: &'static str,
}

#[derive(Clone, Debug)]
pub struct PinPath {
    pub pin: u16,
    pub mc: u16,
    pub ns: f64,
    pub passes: u32,
    pub from: Option<From>,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub grade: String,
    pub vccio: Vccio,
    /// Register-to-register paths, worst first.
    pub reg_to_reg: Vec<RegPath>,
    /// Minimum clock period (ns) and fmax (MHz), if there is a register-to-register path.
    pub period: Option<f64>,
    pub fmax_mhz: Option<f64>,
    pub tpd: Vec<PinPath>,
    pub tco: Vec<PinPath>,
    pub tsu: Vec<RegPath>,
    /// Combinatorial loops met (macrocells), not timed through.
    pub loops: Vec<u16>,
    /// The register-to-register floor applied (tCNT of the grade).
    pub tcnt: f64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Paths launched by registers (time 0 = the clock edge at the registers).
    Reg,
    /// Paths launched by pins (time 0 = the input pin).
    Pin,
}

struct Ctx<'a> {
    m: &'a Model,
    p: &'a Params,
    mode: Mode,
    memo_x: BTreeMap<u16, Option<Arrival>>,
    busy: std::collections::BTreeSet<u16>,
    loops: std::collections::BTreeSet<u16>,
}

impl Ctx<'_> {
    fn mc(&self, mc: u16) -> &crate::model::Mc {
        &self.m.mcs[mc as usize - 1]
    }
    fn rpa(&self, mc: u16) -> f64 {
        if self.m.decoded.macrocells[mc as usize - 1].low_power {
            self.p.get("tRPA")
        } else {
            0.0
        }
    }
    /// Clock arrival of a register (from the clock pins), ns.
    fn clock_arrival(&mut self, mc: u16) -> f64 {
        match self.mc(mc).clock {
            Clock::Global(_) | Clock::High => self.p.get("tIN") + self.p.get("tGLOB"),
            Clock::Pt4 => {
                let saved = std::mem::replace(&mut self.mode, Mode::Pin);
                let memo = std::mem::take(&mut self.memo_x);
                let a = self.pt(mc, 3).map_or(0.0, |a| a.ns);
                self.memo_x = memo;
                self.mode = saved;
                a + self.p.get("tIC") + self.rpa(mc)
            }
        }
    }
    /// Arrival of a signal at the UIM input (before tUIM).
    fn sig(&mut self, s: Sig) -> Option<Arrival> {
        match s {
            Sig::Const(_) => None,
            Sig::Pin(pin) => (self.mode == Mode::Pin).then(|| Arrival { ns: self.p.get("tIN"), passes: 0, from: From::Pin(pin) }),
            Sig::Pad(mc) => (self.mode == Mode::Pin).then(|| Arrival { ns: self.p.get("tIO"), passes: 0, from: From::Pad(mc) }),
            Sig::Fb(mc) => {
                let c = self.mc(mc);
                if c.fb_comb {
                    self.x(mc).map(|a| a.plus(self.p.get("tCOMB"), 0))
                } else if self.mode == Mode::Reg && c.active {
                    Some(Arrival { ns: self.p.get("tRD"), passes: 0, from: From::Reg(mc) })
                } else {
                    None
                }
            }
        }
    }
    /// Arrival at the output of PT `k` (0-based) of `mc` (before tLAD).
    fn pt(&mut self, mc: u16, k: usize) -> Option<Arrival> {
        let lits = self.mc(mc).pts[k].clone()?;
        let mut best = None;
        for l in lits {
            let a = match l {
                Lit::Sig(s, _) => self.sig(s).map(|a| a.plus(self.p.get("tUIM"), 1)),
                Lit::Fold(host) => self.fold(host),
            };
            best = Arrival::later(best, a);
        }
        best
    }
    /// Arrival of the foldback line hosted by `host` (= ¬PT1 of that macrocell).
    fn fold(&mut self, host: u16) -> Option<Arrival> {
        if self.mc(host).pt_use[0] != PtUse::Foldback {
            return None;
        }
        self.pt(host, 0).map(|a| a.plus(self.p.get("tSEXP") + self.rpa(host), 0))
    }
    /// Arrival of the macrocell's sum (OR of its sum PTs and the cascade from MC−1).
    fn sum(&mut self, mc: u16) -> Option<Arrival> {
        let lad = self.p.get("tLAD") + self.rpa(mc);
        let mut best = None;
        for k in 0..5 {
            if self.mc(mc).pt_use[k] == PtUse::Sum {
                best = Arrival::later(best, self.pt(mc, k).map(|a| a.plus(lad, 0)));
            }
        }
        if self.mc(mc).cascade_in {
            best = Arrival::later(best, self.sum(mc - 1).map(|a| a.plus(self.p.get("tPEXP"), 0)));
        }
        best
    }
    /// Arrival of the XOR output `x`.
    fn x(&mut self, mc: u16) -> Option<Arrival> {
        if let Some(a) = self.memo_x.get(&mc) {
            return *a;
        }
        if !self.busy.insert(mc) {
            self.loops.insert(mc);
            return None;
        }
        let c = self.mc(mc).clone();
        let lad = self.p.get("tLAD") + self.rpa(mc);
        let pt2 = if c.xor_pt2 { self.pt(mc, 1).map(|a| a.plus(lad, 0)) } else { None };
        let a = if c.const_one {
            None
        } else if c.a_is_one {
            pt2
        } else {
            Arrival::later(self.sum(mc), pt2)
        };
        self.busy.remove(&mc);
        self.memo_x.insert(mc, a);
        a
    }
    /// Arrival of the register's D input and its setup time.
    fn d(&mut self, mc: u16) -> (Option<Arrival>, f64) {
        let c = self.mc(mc).clone();
        match c.d_source {
            DSource::Logic => {
                let mut a = self.x(mc);
                if c.storage == Storage::TFlipFlop && self.mode == Mode::Reg {
                    // D = x ^ Q: the register's own Q enters only the XOR (no UIM pass)
                    a = Arrival::later(a, Some(Arrival { ns: self.p.get("tRD"), passes: 0, from: From::Reg(mc) }));
                }
                (a, self.p.get("tSU"))
            }
            DSource::Pt2 => (self.pt(mc, 1).map(|a| a.plus(self.p.get("tLAD") + self.rpa(mc), 0)), self.p.get("tSU")),
            DSource::Pad => {
                ((self.mode == Mode::Pin).then(|| Arrival { ns: self.p.get("tFIN"), passes: 0, from: From::Pad(mc) }), self.p.get("tFSU"))
            }
        }
    }
    /// Clock-enable arrival (PT4 as CE), with its setup.
    fn ce(&mut self, mc: u16) -> Option<Arrival> {
        if !self.mc(mc).ce_pt4 {
            return None;
        }
        self.pt(mc, 3).map(|a| a.plus(self.p.get("tLAC") + self.rpa(mc), 0))
    }
}

/// Pin driving the global clock network of a register (None for PT clocks).
fn clock_pin_of(m: &Model, mc: u16) -> Option<u16> {
    match m.mcs[mc as usize - 1].clock {
        Clock::Global(n) => match m.gclk.get(n as usize - 1).map(|g| g.0) {
            Some(Sig::Pin(p)) => Some(p),
            Some(Sig::Pad(src)) => m.mcs[src as usize - 1].pin,
            _ => None,
        },
        _ => None,
    }
}

fn tod(m: &Model, p: &Params, mc: u16, vccio: Vccio) -> f64 {
    if m.decoded.macrocells[mc as usize - 1].slow_slew {
        p.get("tOD3")
    } else if vccio == Vccio::V3_3 {
        p.get("tOD2")
    } else {
        p.get("tOD1")
    }
}

/// Analyse `model` at speed grade `p`.
pub fn analyse(model: &Model, p: &Params, vccio: Vccio) -> Report {
    let mut loops = std::collections::BTreeSet::new();
    let regs: Vec<u16> = model.mcs.iter().filter(|c| c.active && c.storage != Storage::Latch).map(|c| c.mc).collect();
    let registered = |mc: u16| model.mcs[mc as usize - 1].active && model.mcs[mc as usize - 1].storage != Storage::Latch;
    // register to register
    let mut ctx = Ctx { m: model, p, mode: Mode::Reg, memo_x: BTreeMap::new(), busy: Default::default(), loops: Default::default() };
    let mut reg_to_reg = vec![];
    for &mc in &regs {
        let skew = ctx.clock_arrival(mc) - (p.get("tIN") + p.get("tGLOB"));
        let (d, su) = ctx.d(mc);
        for (a, what) in [(d, "data"), (ctx.ce(mc), "clock enable")] {
            if let Some(a) = a {
                reg_to_reg.push(RegPath {
                    mc,
                    clock_pin: clock_pin_of(model, mc),
                    ns: a.ns + su - skew,
                    passes: a.passes,
                    from: a.from,
                    what,
                });
            }
        }
    }
    loops.extend(ctx.loops.iter().copied());
    reg_to_reg.sort_by(|a, b| b.ns.total_cmp(&a.ns));
    let period = reg_to_reg.first().map(|r| r.ns.max(p.get("tCNT")));
    // pin paths
    let mut ctx = Ctx { m: model, p, mode: Mode::Pin, memo_x: BTreeMap::new(), busy: Default::default(), loops: Default::default() };
    let mut tpd = vec![];
    let mut tco = vec![];
    let mut tsu = vec![];
    for c in &model.mcs {
        let Some(pin) = c.pin else { continue };
        if c.oe == Oe::Never {
            continue;
        }
        let od = tod(model, p, c.mc, vccio);
        if c.out_comb || !registered(c.mc) {
            if let Some(a) = ctx.x(c.mc) {
                let ns = (a.ns + p.get("tCOMB") + od).max(p.get("tPD1"));
                tpd.push(PinPath { pin, mc: c.mc, ns, passes: a.passes, from: Some(a.from) });
            }
        } else {
            let pt_clock = c.clock == Clock::Pt4;
            let ns = (ctx.clock_arrival(c.mc) + p.get("tRD") + od).max(p.get(if pt_clock { "tACOP" } else { "tCOP" }));
            tco.push(PinPath { pin, mc: c.mc, ns, passes: 0, from: None });
        }
    }
    for &mc in &regs {
        let clk = ctx.clock_arrival(mc);
        let (d, su) = ctx.d(mc);
        let fast = model.mcs[mc as usize - 1].d_source == DSource::Pad;
        for (a, what) in [(d, "data"), (ctx.ce(mc), "clock enable")] {
            if let Some(a) = a {
                let floor = if fast { p.get("tFSU_global") } else { p.get("tSU_global") };
                tsu.push(RegPath {
                    mc,
                    clock_pin: clock_pin_of(model, mc),
                    ns: (a.ns + su - clk).max(floor),
                    passes: a.passes,
                    from: a.from,
                    what,
                });
            }
        }
    }
    loops.extend(ctx.loops.iter().copied());
    tpd.sort_by(|a, b| b.ns.total_cmp(&a.ns));
    tco.sort_by(|a, b| b.ns.total_cmp(&a.ns));
    tsu.sort_by(|a, b| b.ns.total_cmp(&a.ns));
    Report {
        grade: p.grade.clone(),
        vccio,
        fmax_mhz: period.map(|t| 1000.0 / t),
        period,
        reg_to_reg,
        tpd,
        tco,
        tsu,
        loops: loops.into_iter().collect(),
        tcnt: p.get("tCNT"),
    }
}

impl Report {
    /// Minimum period (ns) of the register-to-register paths captured by the clock on `pin`.
    pub fn period_for_clock(&self, pin: u16) -> Option<f64> {
        self.reg_to_reg.iter().filter(|r| r.clock_pin == Some(pin)).map(|r| r.ns.max(self.tcnt)).reduce(f64::max)
    }

    /// Human-readable report; `name` names a macrocell (e.g. from the flow's map.json).
    pub fn text(&self, name: &dyn Fn(u16) -> String, limit: usize) -> String {
        use std::fmt::Write;
        let mut o = String::new();
        let from = |f: From| match f {
            From::Reg(m) => format!("{} (reg)", name(m)),
            From::Pin(p) => format!("pin {p}"),
            From::Pad(m) => format!("pad of MC{m}"),
        };
        writeln!(
            o,
            "timing: speed grade {}, VCCIO {}, vendor tables (whole ns), floors per F8.2.1",
            self.grade,
            if self.vccio == Vccio::V5 { "5 V" } else { "3.3 V" }
        )
        .unwrap();
        match (self.period, self.fmax_mhz) {
            (Some(t), Some(f)) => writeln!(o, "fmax: {f:.1} MHz (period {t:.1} ns)").unwrap(),
            _ => writeln!(o, "fmax: no register-to-register path").unwrap(),
        }
        writeln!(o, "\nregister to register (worst first):").unwrap();
        for r in self.reg_to_reg.iter().take(limit) {
            writeln!(o, "  {:7.1} ns  {:>2} UIM pass(es)  {} -> {} ({})", r.ns, r.passes, from(r.from), name(r.mc), r.what).unwrap();
        }
        writeln!(o, "\npin to pin (tPD):").unwrap();
        for r in self.tpd.iter().take(limit) {
            writeln!(
                o,
                "  {:7.1} ns  {:>2} UIM pass(es)  {} -> pin {} ({})",
                r.ns,
                r.passes,
                r.from.map_or("-".into(), from),
                r.pin,
                name(r.mc)
            )
            .unwrap();
        }
        writeln!(o, "\nclock to output (tCO):").unwrap();
        for r in self.tco.iter().take(limit) {
            writeln!(o, "  {:7.1} ns  pin {} ({})", r.ns, r.pin, name(r.mc)).unwrap();
        }
        writeln!(o, "\ninput setup (tSU, to the register's clock):").unwrap();
        for r in self.tsu.iter().take(limit) {
            writeln!(o, "  {:7.1} ns  {:>2} UIM pass(es)  {} -> {} ({})", r.ns, r.passes, from(r.from), name(r.mc), r.what).unwrap();
        }
        if !self.loops.is_empty() {
            writeln!(o, "\ncombinatorial loops (not timed through): MC {:?}", self.loops).unwrap();
        }
        o
    }
}

/// Macrocell -> cell name from a `map.json` (flow or fitter); empty without one.
pub fn names_from_map(path: Option<&std::path::Path>) -> Result<BTreeMap<u16, String>, anyhow::Error> {
    let mut out = BTreeMap::new();
    if let Some(p) = path {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p)?)?;
        for c in v["cells"].as_array().into_iter().flatten() {
            if let (Some(mc), Some(n)) = (c["mc"].as_u64(), c["name"].as_str()) {
                out.insert(mc as u16, n.to_string());
            }
        }
    }
    Ok(out)
}
