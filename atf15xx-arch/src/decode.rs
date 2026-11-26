//! Decoding direction: JEDEC fuse array → architectural netlist.
//!
//! [`decode`] turns a fuse array (index = JEDEC `L` address, `true` = fuse value 1) into a
//! [`Decoded`] description of every macrocell, product term, UIM selection and global setting.
//! The functional meaning of each field is specified in
//! `docs/architecture/fitter/02-fuse-map.md` §F2.9 ("Functional semantics"); in short, for each
//! macrocell `m` in block `b`, with `pt(k)` the AND of its literals (`false` when the PT is off):
//!
//! ```text
//! sum   = OR of pt(k) for k with PtUse::Sum  (+ the sum of MC m-1 if that MC has cascade_out)
//! xb    = pt(2) if PT2 is PtUse::Xor else 1
//! x     = sum ^ xb ^ !invert                      -- no XOR PT ⇒ x = sum ^ invert
//! d     = x ^ Q (toggle) | x | pt(2) (FastD) | pad (fast input register)
//! clk   = pt(4) (Pt4Mux∧Pt4Func) | 1 (¬Pt4Mux∧Pt4Func) | GCLKn (¬Pt4Func; then CE = pt(4) if Pt4Mux)
//! Q     = D flip-flop on the rising clock edge (if CE) / transparent latch while clock is 1;
//!         asynchronous reset (PT3 | GCLR) has priority over preset (PT5)
//! pin   = x (Output::Comb) or Q (Output::Reg), driven when the OE is true (open drain: only 0)
//! fb    = x (Feedback::Comb) or Q (Feedback::Reg)  -- what UIM entry `Xnfb` sees
//! pad   = the pin's value (driven or external)     -- what UIM entry `Xnp` sees
//! fold  = !pt(1) when PT1 is PtUse::Foldback        -- foldback column `l` of block b
//! ```
//!
//! A macrocell with `cascade_out` hands its sum to MC+1; its own XOR "A" input is then the
//! constant 1 (this is how the vendor encodes constant-1 outputs: no PTs, `cascade_out`).

use crate::{ConfigBit, Family, McBit, Source};

/// One literal of a product term.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Literal {
    /// UIM mux `mux` of the macrocell's block; `positive` = true literal.
    Uim { mux: u8, positive: bool },
    /// Foldback line of block-local macrocell `local` (the complement of that MC's PT1).
    Foldback { local: u8 },
}

/// What a product term feeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtUse {
    /// Powered off or all-zero: constant 0, contributes nothing.
    Off,
    Sum,
    /// PT1 → foldback line of the block.
    Foldback,
    /// PT2 → XOR B input.
    Xor,
    /// PT2 → register D directly (`DMux` = 0, `OMux` = 1); not part of the sum.
    FastD,
    /// PT3 → asynchronous reset.
    Reset,
    /// PT4 → clock.
    Clock,
    /// PT4 → clock enable.
    ClockEnable,
    /// PT5 → asynchronous preset.
    Preset,
    /// PT5 → output enable.
    OutputEnable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storage {
    DFlipFlop,
    /// D flip-flop whose D is `x ^ Q`.
    TFlipFlop,
    Latch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockSource {
    /// Global clock network 1..=3.
    Global(u8),
    /// PT4.
    ProductTerm,
    /// Tied high (`Pt4Func` = 1 with PT4 in the sum): a latch is always transparent, a flip-flop
    /// never clocks. The vendor uses this for `-strategy global_fold` nodes.
    AlwaysHigh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DSource {
    /// The XOR output `x`.
    Logic,
    /// PT2 (fast path).
    Pt2,
    /// The macrocell's own pad (fast input register).
    Pad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputEnable {
    /// Output buffer off (buried macrocell or input pin).
    Never,
    Always,
    ProductTerm,
    /// Global OE network 1..=6.
    Global(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tap {
    /// Combinatorial (XOR output `x`).
    Comb,
    /// Register / latch output `Q`.
    Reg,
}

#[derive(Clone, Debug)]
pub struct MacrocellConfig {
    pub mc: u16,
    /// Raw configuration bits.
    pub bits: Vec<(McBit, bool)>,
    /// PT1..PT5: `None` = off (all-zero row or `PtPowerOff`), else the literals (empty = constant 1).
    pub pts: [Option<Vec<Literal>>; 5],
    pub pt_use: [PtUse; 5],
    /// This macrocell's sum also drives the cascade input of MC+1.
    pub cascade_out: bool,
    /// The raw `Invert` fuse: `x = sum ^ xb ^ !invert_fuse` (no XOR PT: `x = sum ^ invert_fuse`).
    pub invert_fuse: bool,
    pub storage: Storage,
    pub d_source: DSource,
    pub clock: ClockSource,
    /// Global clear resets this register.
    pub global_reset: bool,
    pub output: Tap,
    pub feedback: Tap,
    /// Raw OE-mux reading (code 7 ∧ ¬`Pt5Func` = `ProductTerm` whatever `Pt5Mux` is); see
    /// [`MacrocellConfig::oe_effective`] for the PT5 steering.
    pub oe: OutputEnable,
    pub open_drain: bool,
    pub slow_slew: bool,
    pub low_power: bool,
}

impl MacrocellConfig {
    pub fn bit(&self, b: McBit) -> bool {
        self.bits.iter().find(|x| x.0 == b).map(|x| x.1).unwrap()
    }
    /// Without an XOR product term: is the XOR output the complement of the sum?
    pub fn output_inverted(&self) -> bool {
        self.invert_fuse
    }
    /// The output enable as the hardware sees it (§F2.8 rule 13, [I]): OE code 7 with
    /// `Pt5Func` = 0 selects PT5's special-function line, which carries PT5 only when `Pt5Mux`
    /// steers PT5 there; with PT5 in the sum the line is 0 and the output is never enabled
    /// (like the preset, rule 11). [`MacrocellConfig::oe`] keeps the raw OE-mux reading.
    pub fn oe_effective(&self) -> OutputEnable {
        match self.oe {
            OutputEnable::ProductTerm if !self.bit(McBit::Pt5Mux) => OutputEnable::Never,
            o => o,
        }
    }
    /// Does any PT of this macrocell carry logic?
    pub fn uses_pts(&self) -> bool {
        self.pts.iter().any(|p| p.is_some())
    }
}

#[derive(Clone, Debug)]
pub struct Decoded {
    pub family: Family,
    /// Index `mc - 1`.
    pub macrocells: Vec<MacrocellConfig>,
    /// `uim[block][mux]`: selected input, `None` = unused.
    pub uim: Vec<Vec<Option<Source>>>,
    /// Global OE networks 1..=6 (index n-1): selected input and inversion.
    pub goe: [(Option<Source>, bool); 6],
    pub gclr_inverted: bool,
    pub gclk_inverted: [bool; 3],
    /// Raw GCLK source-select bits (bit0, bit1) per network; the vendor writes (1,1),(1,0),(0,0) =
    /// GCLK1 pin, GCLK2 pin, GCLK3 I/O pin.
    pub gclk_select: [(bool, bool); 3],
    /// Source pin of each GCLK network (1 = GCLK1 pin, 2 = GCLK2/OE2 pin, 3 = GCLK3 I/O pin), from
    /// `gclk_select`: any network can take any of the three clock pins (silicon-verified on the
    /// ATF1508AS, F2.6). `None` for the unobserved code (0,1).
    pub gclk_source: [Option<u8>; 3],
    pub jtag: bool,
    pub bus_keeper: bool,
    pub pd: [bool; 2],
    pub itd: [bool; 3],
    pub tms_pullup: bool,
    pub tdi_pullup: bool,
    pub power_reset: bool,
    /// UES bits as stored (LSB = first fuse).
    pub ues: u16,
}

fn code_to_goe(code: u8) -> Option<u8> {
    match code {
        4 => Some(1),
        2 => Some(2),
        6 => Some(3),
        1 => Some(4),
        5 => Some(5),
        3 => Some(6),
        _ => None,
    }
}

/// Decode a fuse array (`fuses.len()` must be the family's fuse count).
pub fn decode(f: Family, fuses: &[bool]) -> Decoded {
    assert_eq!(fuses.len(), f.fuse_count(), "fuse count");
    let get = |a: usize| fuses[a];
    let mut macrocells = vec![];
    for mc in 1..=f.macrocells() {
        let bits: Vec<(McBit, bool)> = McBit::ALL.iter().map(|&b| (b, get(f.mc_bit_addr(mc, b)))).collect();
        let bit = |b: McBit| bits.iter().find(|x| x.0 == b).unwrap().1;
        let mut pts: [Option<Vec<Literal>>; 5] = Default::default();
        for k in 1..=5 {
            let a = f.pt_addr(mc, k);
            let row = &fuses[a..a + 96];
            if bit(McBit::PtPowerOff) || !row.iter().any(|&x| x) {
                continue;
            }
            let mut lits = vec![];
            for (j, &v) in row.iter().enumerate() {
                if v {
                    continue;
                }
                if j < 16 {
                    lits.push(Literal::Foldback { local: j as u8 });
                } else {
                    let mux = (j - 16) / 2;
                    let comp_col = f.pt_input_column(mux, true);
                    lits.push(Literal::Uim { mux: mux as u8, positive: j != comp_col });
                }
            }
            pts[k - 1] = Some(lits);
        }
        let fast_d = !bit(McBit::DMux) && bit(McBit::OMux);
        let mut pt_use = [PtUse::Sum; 5];
        pt_use[0] = if bit(McBit::Pt1Mux) { PtUse::Foldback } else { PtUse::Sum };
        pt_use[1] = if bit(McBit::Pt2Mux) {
            PtUse::Xor
        } else if fast_d {
            PtUse::FastD
        } else {
            PtUse::Sum
        };
        pt_use[2] = if bit(McBit::Pt3Mux) { PtUse::Reset } else { PtUse::Sum };
        pt_use[3] = match (bit(McBit::Pt4Mux), bit(McBit::Pt4Func)) {
            (false, _) => PtUse::Sum,
            (true, true) => PtUse::Clock,
            (true, false) => PtUse::ClockEnable,
        };
        let code = bit(McBit::OeMux0) as u8 | (bit(McBit::OeMux1) as u8) << 1 | (bit(McBit::OeMux2) as u8) << 2;
        pt_use[4] = match (bit(McBit::Pt5Mux), bit(McBit::Pt5Func)) {
            (false, _) => PtUse::Sum,
            (true, true) => PtUse::Preset,
            (true, false) => PtUse::OutputEnable,
        };
        for k in 0..5 {
            if pts[k].is_none() {
                pt_use[k] = PtUse::Off;
            }
        }
        let oe = match code {
            0 => OutputEnable::Never,
            7 if bit(McBit::Pt5Func) => OutputEnable::Always,
            7 => OutputEnable::ProductTerm,
            c => OutputEnable::Global(code_to_goe(c).unwrap()),
        };
        let clock = if bit(McBit::Pt4Mux) && bit(McBit::Pt4Func) {
            ClockSource::ProductTerm
        } else if bit(McBit::Pt4Func) {
            ClockSource::AlwaysHigh
        } else {
            ClockSource::Global(match (bit(McBit::Gclk1), bit(McBit::Gclk0)) {
                (true, true) => 1,
                (true, false) => 3,
                _ => 2,
            })
        };
        macrocells.push(MacrocellConfig {
            mc: mc as u16,
            pts,
            pt_use,
            cascade_out: bit(McBit::CasMux),
            invert_fuse: bit(McBit::Invert),
            storage: if !bit(McBit::Storage) {
                Storage::Latch
            } else if bit(McBit::XorB) {
                Storage::TFlipFlop
            } else {
                Storage::DFlipFlop
            },
            d_source: if bit(McBit::DMux) {
                DSource::Logic
            } else if bit(McBit::OMux) {
                DSource::Pt2
            } else {
                DSource::Pad
            },
            clock,
            global_reset: !bit(McBit::GclrMux),
            output: if bit(McBit::OMux) { Tap::Comb } else { Tap::Reg },
            feedback: if bit(McBit::FbMux) { Tap::Comb } else { Tap::Reg },
            oe,
            open_drain: bit(McBit::OpenDrain),
            slow_slew: bit(McBit::Slew),
            low_power: bit(McBit::LowPower),
            bits,
        });
    }
    let w = f.uim_width();
    let sel = |a: usize, choices: &[Source]| -> Option<Source> {
        (0..w).find(|&i| !fuses[a + i]).map(|i| choices[i])
    };
    let uim = (0..f.blocks()).map(|b| (0..40).map(|m| sel(f.uim_addr(b, m), f.uim_choices(m))).collect()).collect();
    let mut goe = [(None, false); 6];
    for n in 1..=6 {
        goe[n - 1] = (sel(f.goe_addr(n), f.goe_choices(n)), !get(f.config_addr(ConfigBit::GoeInvert(n as u8))));
    }
    let cb = |b: ConfigBit| get(f.config_addr(b));
    let mut ues = 0u16;
    for i in 0..16 {
        if get(f.ues_base() + i) {
            ues |= 1 << i;
        }
    }
    Decoded {
        family: f,
        macrocells,
        uim,
        goe,
        gclr_inverted: !cb(ConfigBit::GclrInvert),
        gclk_inverted: [cb(ConfigBit::GclkInvert(1)), cb(ConfigBit::GclkInvert(2)), cb(ConfigBit::GclkInvert(3))],
        gclk_select: [1u8, 2, 3].map(|n| (cb(ConfigBit::GclkMux(n, 0)), cb(ConfigBit::GclkMux(n, 1)))),
        gclk_source: [1u8, 2, 3].map(|n| gclk_source_of((cb(ConfigBit::GclkMux(n, 0)), cb(ConfigBit::GclkMux(n, 1))))),
        jtag: cb(ConfigBit::Jtag),
        bus_keeper: !cb(ConfigBit::Termination),
        pd: [!cb(ConfigBit::Pd1), !cb(ConfigBit::Pd2)],
        itd: [!cb(ConfigBit::Itd(1)), !cb(ConfigBit::Itd(2)), !cb(ConfigBit::Itd(3))],
        tms_pullup: !cb(ConfigBit::TmsPullup),
        tdi_pullup: !cb(ConfigBit::TdiPullup),
        power_reset: !cb(ConfigBit::PowerReset),
        ues,
    }
}

/// GCLK network source select (bit0, bit1) -> clock pin: (1,1) GCLK1 pin, (1,0) GCLK2/OE2 pin,
/// (0,0) GCLK3 I/O pin; (0,1) never observed.
pub fn gclk_source_of(sel: (bool, bool)) -> Option<u8> {
    match sel {
        (true, true) => Some(1),
        (true, false) => Some(2),
        (false, false) => Some(3),
        (false, true) => None,
    }
}

/// Select bits (bit0, bit1) for a GCLK network sourced from clock pin `pin` (1..=3).
pub fn gclk_select_for(pin: u8) -> (bool, bool) {
    match pin {
        1 => (true, true),
        2 => (true, false),
        _ => (false, false),
    }
}
