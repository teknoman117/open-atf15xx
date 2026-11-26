//! Functional model of a configured ATF15xxAS device (docs/architecture/sim/01-model.md).

use atf15xx_arch::decode::{self, ClockSource, DSource, Literal, OutputEnable, PtUse, Storage, Tap};
use atf15xx_arch::{Device, Family, McBit, Source};

/// Configurable semantics that are not oracle-verified (§F2.8 rules marked [I]/[P]).
#[derive(Clone, Debug)]
pub struct Options {
    /// Async reset beats async preset when both are asserted (§F2.8 rule 11 [I]).
    pub reset_wins: bool,
    /// Register value at power-up (§F2.8 rule 15 [I]) — used unless `power_up_from_fuse`.
    pub power_up: bool,
    /// Each register powers up to the value of its `Invert` fuse, the fuse prjbureau calls
    /// `reset` ("power-on reset value of the storage element", GND = 0 / VCC = 1). [P]
    pub power_up_from_fuse: bool,
    /// A cascade from MC−1 is only applied if MC−1 has a PT (the former Python model's reading) — otherwise
    /// `CasMux` alone suffices (decode.rs prose).
    pub cascade_requires_pts: bool,
    /// A lender MC (`CasMux` = 1) with PTs sees A = 1 on its own XOR: its sum goes to the cascade
    /// only (decode.rs prose; **silicon-verified** on the ATF1508AS 2026-11-24, spec F2.9, and the
    /// reading under which the vendor's SELFPROGRAMBOARD fit matches its .tt2). Default true;
    /// false = it uses its own sum (the old reading, wrong on silicon).
    pub lender_a_is_one: bool,
    /// MCs with no PTs and no `CasMux` keep their register at the power-up value.
    pub idle_mc_holds: bool,
    /// OE code 7 with `Pt5Func` = 0 enables the output with PT5 only if PT5 is steered to its
    /// special function (`Pt5Mux` = 1); with PT5 in the sum the OE input sees 0, like PT3/PT5 as
    /// reset/preset (§F2.8 rule 11). [I], §F2.3.2 (settled in worklog 05: the spec, decode's
    /// `oe_effective()` and this default agree);
    /// `false` = decode.rs behaviour (OE = PT5 whatever its steering).
    pub oe_pt5_needs_mux: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { reset_wins: true, power_up: false, power_up_from_fuse: false, cascade_requires_pts: true, lender_a_is_one: true, idle_mc_holds: true, oe_pt5_needs_mux: true }
    }
}

/// A resolved signal: what a UIM line, GOE network or clock net carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sig {
    Const(bool),
    /// Pad (pin value) of macrocell `mc`.
    Pad(u16),
    /// Feedback of macrocell `mc`.
    Fb(u16),
    /// A dedicated input pin, by pin number.
    Pin(u16),
}

/// A product-term literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lit {
    /// A signal, true (`positive`) or complemented.
    Sig(Sig, bool),
    /// Foldback line hosted by macrocell `mc` (= ¬PT1 of that MC, or 1 if it isn't a foldback).
    Fold(u16),
}

/// A product term: `None` = off (constant 0); `Some(vec![])` = constant 1.
pub type Pt = Option<Vec<Lit>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clock {
    /// Global clock network 1..=3.
    Global(u8),
    /// PT4.
    Pt4,
    /// Tied high (`Pt4Func` = 1 without `Pt4Mux`): a latch is always transparent, a flip-flop
    /// never clocks (decode `ClockSource::AlwaysHigh`).
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Oe {
    Never,
    Always,
    Pt5,
    /// Global OE network 1..=6.
    Global(u8),
}

#[derive(Clone, Debug)]
pub struct Mc {
    /// Register value at power-up (Options::power_up or the `Invert` fuse).
    pub init: bool,
    pub mc: u16,
    pub block: usize,
    pub local: usize,
    /// Bonded pin, if any (and usable: JTAG pins are excluded while JTAG is enabled).
    pub pin: Option<u16>,
    pub pts: [Pt; 5],
    pub pt_use: [PtUse; 5],
    /// The register is modelled (has PTs or `CasMux`, unless `idle_mc_holds` is off).
    pub active: bool,
    /// OR the sum of MC−1 into this sum.
    pub cascade_in: bool,
    pub cascade_out: bool,
    /// No PTs and `CasMux`: the XOR output is constant 1.
    pub const_one: bool,
    /// XOR A input forced to 1 (lender, with `lender_a_is_one`).
    pub a_is_one: bool,
    pub invert: bool,
    pub xor_pt2: bool,
    pub storage: Storage,
    pub d_source: DSource,
    pub clock: Clock,
    pub ce_pt4: bool,
    pub ar_pt3: bool,
    pub ar_gclr: bool,
    pub ap_pt5: bool,
    pub out_comb: bool,
    pub fb_comb: bool,
    pub oe: Oe,
    pub open_drain: bool,
}

impl Mc {
    /// The PTs that feed the sum.
    pub fn sum_pts(&self) -> impl Iterator<Item = &Vec<Lit>> + '_ {
        (0..5).filter(|&k| self.pt_use[k] == PtUse::Sum).filter_map(move |k| self.pts[k].as_ref())
    }
    pub fn has_pts(&self) -> bool {
        self.pts.iter().any(|p| p.is_some())
    }
}

/// An external pin of the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinKind {
    /// I/O pin of macrocell `mc`.
    Io(u16),
    /// Dedicated input (GCLK1, OE1, GCLR, GCLK2/OE2).
    Dedicated,
    /// Input-only pin reaching the pad of macrocell `mc`.
    InputOnly(u16),
}

#[derive(Clone, Debug)]
pub struct Model {
    pub device: &'static Device,
    pub family: Family,
    pub opts: Options,
    pub decoded: decode::Decoded,
    pub mcs: Vec<Mc>,
    /// Pins in ascending order.
    pub pins: Vec<(u16, PinKind)>,
    /// Clock networks 1..=3: source and inversion.
    pub gclk: [(Sig, bool); 3],
    /// Global clear network.
    pub gclr: (Sig, bool),
    /// Global OE networks 1..=6.
    pub goe: [(Sig, bool); 6],
}

fn sig_of(dev: &Device, s: Option<Source>) -> Sig {
    match s {
        None | Some(Source::Gnd) | Some(Source::None) => Sig::Const(false),
        Some(Source::Pad(m)) => Sig::Pad(m),
        Some(Source::Fb(m)) => Sig::Fb(m),
        Some(Source::Gclk1) => Sig::Pin(dev.gclk1),
        Some(Source::Gclr) => Sig::Pin(dev.gclr),
        Some(Source::Oe1) => Sig::Pin(dev.oe1),
        Some(Source::Oe2) => Sig::Pin(dev.gclk2_oe2),
    }
}

impl Model {
    pub fn new(device: &'static Device, fuses: &[bool], opts: Options) -> Model {
        let family = device.family;
        let d = decode::decode(family, fuses);
        let jtag_pins: Vec<u16> = if d.jtag {
            vec![device.jtag.tdi, device.jtag.tms, device.jtag.tck, device.jtag.tdo]
        } else {
            vec![]
        };
        let mut pins: Vec<(u16, PinKind)> = vec![];
        for (p, m) in device.io_pins() {
            if !jtag_pins.contains(&p) {
                pins.push((p, PinKind::Io(m)));
            }
        }
        for p in [device.gclk1, device.oe1, device.gclr, device.gclk2_oe2] {
            if !pins.iter().any(|x| x.0 == p) {
                pins.push((p, PinKind::Dedicated));
            }
        }
        for p in 1..=device.pin_count() {
            if let Some(m) = device.input_only_mc(p) {
                if !pins.iter().any(|x| x.0 == p) {
                    pins.push((p, PinKind::InputOnly(m)));
                }
            }
        }
        pins.sort_by_key(|x| x.0);
        let pin_of_mc = |mc: u16| {
            pins.iter().find_map(|&(p, k)| if k == PinKind::Io(mc) { Some(p) } else { None })
        };
        let n = family.macrocells();
        let mut mcs: Vec<Mc> = Vec::with_capacity(n);
        for c in &d.macrocells {
            let mcn = c.mc;
            let block = (mcn as usize - 1) / 16;
            let local = (mcn as usize - 1) % 16;
            let mut pts: [Pt; 5] = Default::default();
            for k in 0..5 {
                pts[k] = c.pts[k].as_ref().map(|lits| {
                    lits.iter()
                        .map(|l| match *l {
                            Literal::Uim { mux, positive } => {
                                Lit::Sig(sig_of(device, d.uim[block][mux as usize]), positive)
                            }
                            Literal::Foldback { local } => Lit::Fold((block * 16 + local as usize + 1) as u16),
                        })
                        .collect()
                });
            }
            let has = pts.iter().any(|p| p.is_some());
            mcs.push(Mc {
                init: if opts.power_up_from_fuse { c.invert_fuse } else { opts.power_up },
                mc: mcn,
                block,
                local,
                pin: pin_of_mc(mcn),
                pt_use: c.pt_use,
                // a fast input register (D from the pad) needs no PTs to be in use
                active: has || c.cascade_out || c.d_source == DSource::Pad || !opts.idle_mc_holds,
                cascade_in: false,
                cascade_out: c.cascade_out,
                const_one: c.cascade_out && !has,
                a_is_one: c.cascade_out && has && opts.lender_a_is_one,
                invert: c.invert_fuse,
                xor_pt2: c.pt_use[1] == PtUse::Xor,
                storage: c.storage,
                d_source: c.d_source,
                // tied-high clock/enable is derived from the raw bits (Pt4Func = 1, Pt4Mux = 0) so
                // that this crate does not depend on how decode.rs chooses to expose it
                clock: if c.bit(McBit::Pt4Func) && !c.bit(McBit::Pt4Mux) {
                    Clock::High
                } else {
                    match c.clock {
                        ClockSource::ProductTerm => Clock::Pt4,
                        ClockSource::Global(g) => Clock::Global(g),
                        #[allow(unreachable_patterns)]
                        _ => Clock::High,
                    }
                },
                ce_pt4: c.pt_use[3] == PtUse::ClockEnable,
                ar_pt3: c.pt_use[2] == PtUse::Reset,
                ar_gclr: c.global_reset,
                ap_pt5: c.pt_use[4] == PtUse::Preset,
                out_comb: c.output == Tap::Comb,
                fb_comb: c.feedback == Tap::Comb,
                oe: match c.oe {
                    OutputEnable::Never => Oe::Never,
                    OutputEnable::Always => Oe::Always,
                    OutputEnable::ProductTerm if opts.oe_pt5_needs_mux && !c.bit(McBit::Pt5Mux) => Oe::Never,
                    OutputEnable::ProductTerm => Oe::Pt5,
                    OutputEnable::Global(g) => Oe::Global(g),
                },
                open_drain: c.open_drain,
                pts,
            });
        }
        for i in 1..mcs.len() {
            if mcs[i].local > 0 {
                let prev = &mcs[i - 1];
                mcs[i].cascade_in = prev.cascade_out && (prev.has_pts() || !opts.cascade_requires_pts);
            }
        }
        let gclk3 = match device.pin_mc(device.gclk3) {
            Some(m) => Sig::Pad(m),
            None => Sig::Pin(device.gclk3),
        };
        // each network takes one of the three clock pins (source mux, F2.6) and has its own invert
        let clock_pin = |k: u8| match k {
            1 => Sig::Pin(device.gclk1),
            2 => Sig::Pin(device.gclk2_oe2),
            _ => gclk3.clone(),
        };
        let gclk = [0usize, 1, 2].map(|i| (clock_pin(d.gclk_source[i].unwrap_or(i as u8 + 1)), d.gclk_inverted[i]));
        let gclr = (Sig::Pin(device.gclr), d.gclr_inverted);
        let goe = d.goe.map(|(s, inv)| (sig_of(device, s), inv));
        Model { device, family, opts, decoded: d, mcs, pins, gclk, gclr, goe }
    }

    pub fn mc(&self, mc: u16) -> &Mc {
        &self.mcs[mc as usize - 1]
    }

    /// Does the model ever drive this pin (its OE is not `Never`)?
    pub fn pin_can_drive(&self, pin: u16) -> bool {
        self.pins.iter().any(|&(p, k)| p == pin && matches!(k, PinKind::Io(m) if self.mc(m).oe != Oe::Never))
    }

    /// Macrocells whose logic is used (have PTs, cascade or a non-`Never` OE).
    pub fn used_mcs(&self) -> impl Iterator<Item = &Mc> + '_ {
        self.mcs.iter().filter(|m| m.has_pts() || m.cascade_out || m.oe != Oe::Never)
    }
}
