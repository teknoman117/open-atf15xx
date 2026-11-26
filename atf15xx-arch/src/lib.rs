//! Architecture and fuse-map database for the ATF1502AS, ATF1504AS and ATF1508AS CPLDs.
//!
//! Everything here was derived from the output of the vendor fitters (`fit1502.exe`,
//! `fit1504.exe`, `fit1508.exe`, run under wine) on generated designs: prjbureau's fuzzers (the
//! fuse map), our own differential experiments (devices, timing: `tools/fuzzers/`) and Atmel's
//! BSDL files as shipped with prjbureau (boundary scan). The tables are generated at build time
//! by `build/main.rs` from the prjbureau submodule and `data/`; nothing is parsed at run time.
//! See `docs/architecture/fitter/` for the derivations and evidence.
//!
//! Conventions: macrocells are numbered `1..=n` globally (`MC1` = block A, local 0); a block is
//! `0..blocks` (`A` = 0); fuse addresses are JEDEC `L` addresses. A fuse value of `0` means
//! "programmed/connected" in the product-term array.

pub mod bsdl;
pub mod decode;
mod layout;
pub mod part;
pub mod timing;

use layout::{FORWARD_PAIRS, MC_BIT_LAYOUT};
use std::fmt;

/// Device family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    F1502,
    F1504,
    F1508,
}

impl Family {
    pub const ALL: [Family; 3] = [Family::F1502, Family::F1504, Family::F1508];

    pub fn from_number(s: &str) -> Option<Family> {
        match s {
            "1502" => Some(Family::F1502),
            "1504" => Some(Family::F1504),
            "1508" => Some(Family::F1508),
            _ => None,
        }
    }
    pub fn number(self) -> &'static str {
        match self {
            Family::F1502 => "1502",
            Family::F1504 => "1504",
            Family::F1508 => "1508",
        }
    }
    /// Chip name as written in the JEDEC header, e.g. `ATF1504AS`.
    pub fn chip_name(self) -> String {
        format!("ATF{}AS", self.number())
    }
    pub fn blocks(self) -> usize {
        match self {
            Family::F1502 => 2,
            Family::F1504 => 4,
            Family::F1508 => 8,
        }
    }
    pub fn macrocells(self) -> usize {
        16 * self.blocks()
    }
    /// Total number of fuses (`QF`).
    pub fn fuse_count(self) -> usize {
        self.data().qf
    }
    /// Width of one UIM / global-OE selector field (one-cold).
    pub fn uim_width(self) -> usize {
        self.data().uim_width
    }

    fn data(self) -> &'static FamilyData {
        &FAMILY_DATA[self as usize]
    }

    // ---------------------------------------------------------------- product-term array

    /// Physical position of logical block `b` in the PT array (and macrocell-config region).
    pub fn block_position(self, b: usize) -> usize {
        match self {
            Family::F1502 => [0, 1][b],
            Family::F1504 => [0, 1, 3, 2][b],
            Family::F1508 => [0, 1, 2, 3, 7, 6, 5, 4][b],
        }
    }
    /// Physical slot (0..16) of a block-local macrocell index in the PT array.
    pub fn pt_slot(self, local: usize) -> usize {
        match self {
            Family::F1502 => local,
            _ => 15 - local,
        }
    }
    /// Start address of product term `pt` (1..=5) of macrocell `mc`. Each row is 96 fuses.
    pub fn pt_addr(self, mc: usize, pt: usize) -> usize {
        assert!((1..=5).contains(&pt) && (1..=self.macrocells()).contains(&mc));
        let (b, l) = ((mc - 1) / 16, (mc - 1) % 16);
        let s = self.pt_slot(l);
        let within = if s % 2 == 0 { 5 - pt } else { pt - 1 };
        (self.block_position(b) * 80 + s * 5 + within) * 96
    }
    /// End of the PT array (= start of the macrocell-config region).
    pub fn pt_end(self) -> usize {
        self.blocks() * 80 * 96
    }
    /// Column (0..96) within a PT row for UIM mux `mux` (0..40), true or complemented literal.
    pub fn pt_input_column(self, mux: usize, complement: bool) -> usize {
        assert!(mux < 40);
        let swap = matches!(self, Family::F1502) && mux % 2 == 1;
        16 + 2 * mux + usize::from(complement != swap)
    }
    /// Column (0..16) within a PT row for the foldback (expander) output of block-local MC `local`.
    pub fn pt_foldback_column(self, local: usize) -> usize {
        local
    }

    // ---------------------------------------------------------------- macrocell configuration

    /// Address of a macrocell configuration bit.
    pub fn mc_bit_addr(self, mc: usize, bit: McBit) -> usize {
        let (pair, k) = bit.location();
        let (b, l) = ((mc - 1) / 16, (mc - 1) % 16);
        let f = self.data().mcfg_fields.iter().find(|f| f.block == b && (f.pair[0], f.pair[1]) == pair).expect("mcfg field");
        let forward = FORWARD_PAIRS.contains(&pair) != matches!(self, Family::F1502);
        let s = if forward { l } else { 15 - l };
        f.addr + 2 * s + (usize::from(k) ^ (s & 1))
    }
    /// The labelled 32-bit macrocell configuration fields, in address order: `(addr, block, [Sa, Sb])`.
    pub fn mc_fields(self) -> Vec<(usize, usize, [u8; 2])> {
        self.data().mcfg_fields.iter().map(|f| (f.addr, f.block, f.pair)).collect()
    }
    /// The unlabelled 16-bit fields interleaved with the macrocell fields (always written as ones).
    pub fn mc_filler_fields(self) -> Vec<usize> {
        (0..self.blocks() * 6).map(|i| self.pt_end() + 80 * i).collect()
    }
    pub fn uim_base(self) -> usize {
        self.pt_end() + self.blocks() * 480
    }

    // ---------------------------------------------------------------- interconnect

    /// Address of the selector of UIM mux `mux` feeding block `block`.
    pub fn uim_addr(self, block: usize, mux: usize) -> usize {
        assert!(block < self.blocks() && mux < 40);
        self.data().uim_fields[block * 40 + mux]
    }
    /// The inputs selectable by UIM mux `mux` (same table for every block). Selecting entry
    /// `i` means writing `0` at position `i` of the field (all ones = unused).
    pub fn uim_choices(self, mux: usize) -> &'static [Source] {
        self.data().uim_table[mux]
    }
    /// Address of global OE network `n` (1..=6) selector.
    pub fn goe_addr(self, n: usize) -> usize {
        assert!((1..=6).contains(&n));
        let idx = match self {
            Family::F1502 => 6 - n,
            _ => n - 1,
        };
        self.uim_base() + 40 * self.blocks() * self.uim_width() + idx * self.uim_width()
    }
    /// Inputs selectable by global OE network `n` (1..=6).
    pub fn goe_choices(self, n: usize) -> &'static [Source] {
        self.data().goe_table[n - 1]
    }

    // ---------------------------------------------------------------- global configuration

    pub fn goe_base(self) -> usize {
        self.goe_addr(if matches!(self, Family::F1502) { 6 } else { 1 })
    }
    /// The 32-bit "device configuration bits" field.
    pub fn config_base(self) -> usize {
        self.goe_base() + 6 * self.uim_width()
    }
    /// Address of a global configuration bit.
    pub fn config_addr(self, bit: ConfigBit) -> usize {
        self.config_base() + bit.offset()
    }
    /// The 4 "Special Purpose Bits (JTAG)".
    pub fn special_base(self) -> usize {
        self.config_base() + 32
    }
    /// The 16 user electronic signature bits.
    pub fn ues_base(self) -> usize {
        self.config_base() + 36
    }
    /// The trailing reserved bits (written as zeros): `(addr, len)`.
    pub fn reserved(self) -> (usize, usize) {
        let a = self.config_base() + 52;
        (a, self.fuse_count() - a)
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ATF{}AS", self.number())
    }
}

pub fn block_letter(b: usize) -> char {
    (b'A' + b as u8) as char
}

/// The 24 per-macrocell configuration bits. Names follow the function each bit was observed to
/// control; where one fuse serves two functions both are named (see the docs for the full
/// routing tables). Values are the raw fuse values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum McBit {
    /// Output-enable selector bit 0 (with `OeMux1`, `OeMux2`: 0 = disabled, 7 = PT5/always, else GOE n).
    OeMux0,
    OeMux1,
    OeMux2,
    /// PT5 special function: 1 = asynchronous preset, 0 = output enable.
    Pt5Func,
    /// Output source: 1 = combinatorial (XOR output), 0 = register Q. Shared with fast-input select.
    OMux,
    /// 0 = global clear (GCLR) resets the register, 1 = not connected.
    GclrMux,
    /// 1 = D/T flip-flop, 0 = latch.
    Storage,
    /// Register D input: 1 = XOR (logic), 0 = fast path (pin / PT2).
    DMux,
    /// Output slew: 1 = slow, 0 = fast.
    Slew,
    /// PT4 special function: 1 = clock, 0 = clock enable.
    Pt4Func,
    /// PT1 routing: 1 = foldback expander, 0 = sum term.
    Pt1Mux,
    /// 1 = sum term drives the cascade output to the next macrocell (also XOR-A select).
    CasMux,
    /// Global clock selector bits (`Gclk1 Gclk0`: 11 = GCLK1, 00 = GCLK2, 10 = GCLK3, written as k1,k0).
    Gclk0,
    Gclk1,
    /// 1 = reduced-power macrocell.
    LowPower,
    /// 1 = product terms of this macrocell powered down.
    PtPowerOff,
    /// PT2 routing: 1 = XOR input, 0 = sum term.
    Pt2Mux,
    /// XOR B input: 1 = register Q (toggle flip-flop), 0 = constant / PT2.
    XorB,
    /// PT5 routing: 1 = special function (preset / OE), 0 = sum term.
    Pt5Mux,
    /// 1 = XOR output inverted (sum term complemented).
    Invert,
    /// PT3 routing: 1 = asynchronous reset, 0 = sum term.
    Pt3Mux,
    /// PT4 routing: 1 = clock / clock enable, 0 = sum term.
    Pt4Mux,
    /// 1 = open-drain output, 0 = push-pull.
    OpenDrain,
    /// Feedback to the UIM: 1 = combinatorial, 0 = register Q.
    FbMux,
}

impl McBit {
    pub const ALL: [McBit; 24] = [
        McBit::OeMux0,
        McBit::OeMux1,
        McBit::Pt5Func,
        McBit::OeMux2,
        McBit::OMux,
        McBit::GclrMux,
        McBit::Storage,
        McBit::DMux,
        McBit::Slew,
        McBit::Pt4Func,
        McBit::Pt1Mux,
        McBit::CasMux,
        McBit::Gclk0,
        McBit::Gclk1,
        McBit::LowPower,
        McBit::PtPowerOff,
        McBit::Pt2Mux,
        McBit::XorB,
        McBit::Pt5Mux,
        McBit::Invert,
        McBit::Pt3Mux,
        McBit::Pt4Mux,
        McBit::OpenDrain,
        McBit::FbMux,
    ];

    /// `(S-label pair (lower, higher), k)`: the bit is the `k`-th of the two bits of its macrocell
    /// column, after undoing the per-column parity swap.
    pub fn location(self) -> ((u8, u8), bool) {
        let i = McBit::ALL.iter().position(|&b| b == self).unwrap();
        (MC_BIT_LAYOUT[i].1, MC_BIT_LAYOUT[i].2)
    }
    /// The bit's name, e.g. `OeMux0`.
    pub fn name(self) -> &'static str {
        MC_BIT_LAYOUT[McBit::ALL.iter().position(|&b| b == self).unwrap()].0
    }
}

/// Bits of the 32-bit device configuration field (plus the special-purpose bits after it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigBit {
    /// Offset 0; vendor writes 0.
    Arming,
    /// 0 = bus keeper on all pins (`pin_keep`, default), 1 = high-Z.
    Termination,
    /// 0 = PD1 pin is the power-down input.
    Pd1,
    Pd2,
    /// Global OE `n` inversion; 1 = not inverted (active high).
    GoeInvert(u8),
    /// Global clear inversion; 1 = active high.
    GclrInvert,
    /// Global clock network `n` (1..=3) inversion (vendor writes 0).
    GclkInvert(u8),
    /// Global clock network `n` (1..=3) source select, bit `i` (0/1).
    GclkMux(u8, u8),
    /// Input-transition-detection (standby wake-up) for clock pin `n` (1..=3); 0 = on.
    Itd(u8),
    /// 0 = TMS pull-up on.
    TmsPullup,
    /// 0 = TDI pull-up on.
    TdiPullup,
    /// 0 = power-on reset hysteresis option (`power_reset on`).
    PowerReset,
    /// Offsets 26..31: never observed other than 1.
    Unknown(u8),
    /// Special bit 0: never observed other than 1 (security is the JEDEC `G` field).
    Special0,
    /// Special bit 1: 1 = JTAG pins reserved for JTAG.
    Jtag,
    /// Special bits 2, 3: never observed other than 1.
    Special(u8),
}

impl ConfigBit {
    pub fn offset(self) -> usize {
        use ConfigBit::*;
        match self {
            Arming => 0,
            Termination => 1,
            Pd1 => 2,
            Pd2 => 3,
            GoeInvert(n) => 3 + n as usize,
            GclrInvert => 10,
            GclkInvert(n) => 11 + 3 * (n as usize - 1),
            GclkMux(n, i) => 12 + 3 * (n as usize - 1) + i as usize,
            Itd(n) => 19 + n as usize,
            TmsPullup => 23,
            TdiPullup => 24,
            PowerReset => 25,
            Unknown(i) => i as usize,
            Special0 => 32,
            Jtag => 33,
            Special(i) => 32 + i as usize,
        }
    }
}

/// An input selectable by a UIM or global-OE mux.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    /// The pad (pin input buffer) of macrocell `mc`.
    Pad(u16),
    /// The feedback of macrocell `mc`.
    Fb(u16),
    Gnd,
    /// Dedicated input pins (only some mux rows can reach them).
    Gclk1,
    Gclr,
    Oe1,
    Oe2,
    /// Unconnected entry (`XXXX` in the vendor table).
    None,
}

impl Source {
    pub fn parse(s: &str) -> Option<Source> {
        Some(match s {
            "GND" => Source::Gnd,
            "GCLK" => Source::Gclk1,
            "GCLR" => Source::Gclr,
            "OE1" => Source::Oe1,
            "OE2" => Source::Oe2,
            "XXXX" => Source::None,
            _ => {
                let body = s.get(1..)?;
                if let Some(n) = body.strip_suffix("fb") {
                    Source::Fb(n.parse().ok()?)
                } else if let Some(n) = body.strip_suffix('p') {
                    Source::Pad(n.parse().ok()?)
                } else {
                    return None;
                }
            }
        })
    }
    /// Vendor reference name, e.g. `C44fb`, `A3p`.
    pub fn vendor_name(self) -> String {
        let blk = |mc: u16| block_letter((mc as usize - 1) / 16);
        match self {
            Source::Pad(m) => format!("{}{}p", blk(m), m),
            Source::Fb(m) => format!("{}{}fb", blk(m), m),
            Source::Gnd => "GND".into(),
            Source::Gclk1 => "GCLK".into(),
            Source::Gclr => "GCLR".into(),
            Source::Oe1 => "OE1".into(),
            Source::Oe2 => "OE2".into(),
            Source::None => "XXXX".into(),
        }
    }
}

/// A device = family + package, with its pin map.
#[derive(Debug)]
pub struct Device {
    /// Fitter name, e.g. `P1504C44`
    pub name: &'static str,
    pub family: Family,
    /// e.g. `PLCC44`
    pub package: &'static str,
    /// CUPL device name, e.g. `f1504plcc44`
    pub tt2_device: &'static str,
    /// (I/O pin, macrocell), by pin
    pins: &'static [(u16, u16)],
    /// (input-only pin that reaches a macrocell pad, macrocell), by pin
    input_only: &'static [(u16, u16)],
    pub jtag: Jtag,
    pub pd1: u16,
    pub pd2: u16,
    pub gclk1: u16,
    pub oe1: u16,
    pub gclr: u16,
    pub gclk2_oe2: u16,
    pub gclk3: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct Jtag {
    pub tdi: u16,
    pub tms: u16,
    pub tck: u16,
    pub tdo: u16,
}

impl Device {
    /// Look up a device by fitter name (`P1504C44`) or CUPL name (`f1504plcc44`), case-insensitive.
    pub fn by_name(name: &str) -> Option<&'static Device> {
        DEVICES.iter().find(|d| d.name.eq_ignore_ascii_case(name) || d.tt2_device.eq_ignore_ascii_case(name))
    }
    pub fn all() -> impl Iterator<Item = &'static Device> {
        DEVICES.iter()
    }
    /// Pin number count of the package (`QP`).
    pub fn pin_count(&self) -> u16 {
        self.package.trim_start_matches(|c: char| c.is_ascii_alphabetic()).parse().unwrap()
    }
    /// Macrocell of an I/O pin.
    pub fn pin_mc(&self, pin: u16) -> Option<u16> {
        self.pins.binary_search_by_key(&pin, |p| p.0).ok().map(|i| self.pins[i].1)
    }
    /// I/O pin of a macrocell, if bonded out.
    pub fn mc_pin(&self, mc: u16) -> Option<u16> {
        self.pins.iter().find(|p| p.1 == mc).map(|p| p.0)
    }
    pub fn io_pins(&self) -> impl Iterator<Item = (u16, u16)> + '_ {
        self.pins.iter().copied()
    }
    /// `(input-only pin, macrocell)` for every input-only pin that reaches a macrocell pad.
    pub fn input_only_pins(&self) -> impl Iterator<Item = (u16, u16)> + '_ {
        self.input_only.iter().copied()
    }
    pub fn input_only_mc(&self, pin: u16) -> Option<u16> {
        self.input_only.iter().find(|p| p.0 == pin).map(|p| p.1)
    }
    /// Dedicated (input-only) pin function, if `pin` is one.
    pub fn dedicated(&self, pin: u16) -> Option<Dedicated> {
        if pin == self.gclk1 {
            Some(Dedicated::Gclk1)
        } else if pin == self.oe1 {
            Some(Dedicated::Oe1)
        } else if pin == self.gclr {
            Some(Dedicated::Gclr)
        } else if pin == self.gclk2_oe2 {
            Some(Dedicated::Gclk2Oe2)
        } else {
            None
        }
    }
    pub fn is_jtag_pin(&self, pin: u16) -> bool {
        [self.jtag.tdi, self.jtag.tms, self.jtag.tck, self.jtag.tdo].contains(&pin)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dedicated {
    Gclk1,
    Oe1,
    Gclr,
    Gclk2Oe2,
}

// ---------------------------------------------------------------- generated tables

/// One family's fuse-map tables (generated from prjbureau's database by `build/main.rs`).
struct FamilyData {
    qf: usize,
    uim_width: usize,
    /// The labelled 32-fuse macrocell fields, in address order.
    mcfg_fields: &'static [McField],
    /// UIM selector address by `block * 40 + mux`.
    uim_fields: &'static [usize],
    /// Inputs of UIM mux k by position (`Source::None`: not determined, never selected).
    uim_table: &'static [&'static [Source]],
    goe_table: &'static [&'static [Source]],
}

struct McField {
    addr: usize,
    block: usize,
    pair: [u8; 2],
}

include!(concat!(env!("OUT_DIR"), "/fuse_map.rs"));
include!(concat!(env!("OUT_DIR"), "/devices.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn layout_is_a_partition() {
        for f in Family::ALL {
            let mut seen = HashSet::new();
            let mut add = |a: usize, n: usize| {
                for i in a..a + n {
                    assert!(seen.insert(i), "{f} fuse {i} covered twice");
                }
            };
            for mc in 1..=f.macrocells() {
                for pt in 1..=5 {
                    add(f.pt_addr(mc, pt), 96);
                }
            }
            for (a, _, _) in f.mc_fields() {
                add(a, 32);
            }
            for a in f.mc_filler_fields() {
                add(a, 16);
            }
            for b in 0..f.blocks() {
                for m in 0..40 {
                    add(f.uim_addr(b, m), f.uim_width());
                }
            }
            for n in 1..=6 {
                add(f.goe_addr(n), f.uim_width());
            }
            add(f.config_base(), 32);
            add(f.special_base(), 4);
            add(f.ues_base(), 16);
            let (a, n) = f.reserved();
            add(a, n);
            assert_eq!(seen.len(), f.fuse_count(), "{f}");
        }
    }

    #[test]
    fn mc_bits_are_distinct() {
        for f in Family::ALL {
            let mut seen = HashSet::new();
            for mc in 1..=f.macrocells() {
                for b in McBit::ALL {
                    assert!(seen.insert(f.mc_bit_addr(mc, b)));
                }
            }
            assert_eq!(seen.len(), 24 * f.macrocells());
        }
    }

    #[test]
    fn devices_load() {
        let d = Device::by_name("P1504C44").unwrap();
        assert_eq!(d.family, Family::F1504);
        assert_eq!(d.pin_mc(12), Some(1));
        assert_eq!(Device::by_name("f1508plcc84").unwrap().name, "P1508C84");
        assert_eq!(Device::all().count(), 13);
        assert_eq!(Family::F1504.uim_choices(0).len(), 9);
    }
}
