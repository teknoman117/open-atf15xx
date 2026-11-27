//! Fuse arrays and JEDEC files: a region-structured writer (`write`) and the shared reader
//! (`parse`, on the `jedec` crate) for any JESD3 fuse file, including the vendor fitters' and ATMISP's.

use atf15xx_arch::{block_letter, Family};
use std::fmt::Write as _;

/// A fuse array; `true` = 1 (unprogrammed / not connected).
#[derive(Clone, PartialEq, Eq)]
pub struct Fuses {
    pub family: Family,
    pub bits: Vec<bool>,
    /// JEDEC `G1` (security fuse) requested.
    pub security: bool,
}

impl std::fmt::Debug for Fuses {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fuses({}, {} bits)", self.family, self.bits.len())
    }
}

impl Fuses {
    /// All-zero array (the JEDEC `F0` default the vendor uses).
    pub fn new(family: Family) -> Fuses {
        Fuses { family, bits: vec![false; family.fuse_count()], security: false }
    }
    pub fn get(&self, a: usize) -> bool {
        self.bits[a]
    }
    pub fn set(&mut self, a: usize, v: bool) {
        self.bits[a] = v;
    }
    pub fn fill(&mut self, a: usize, n: usize, v: bool) {
        self.bits[a..a + n].iter_mut().for_each(|b| *b = v);
    }
    pub fn range_str(&self, a: usize, n: usize) -> String {
        self.bits[a..a + n].iter().map(|&b| if b { '1' } else { '0' }).collect()
    }
    /// JEDEC fuse checksum (sum of 8-fuse bytes, fuse 0 = LSB).
    pub fn checksum(&self) -> u16 {
        let mut sum: u32 = 0;
        for chunk in self.bits.chunks(8) {
            let mut byte = 0u32;
            for (i, &b) in chunk.iter().enumerate() {
                if b {
                    byte |= 1 << i;
                }
            }
            sum += byte;
        }
        (sum & 0xffff) as u16
    }
}

/// Read a JEDEC file (JESD3-C; the shared reader [`atf15xx_sim::parse_jed`], on the `jedec`
/// crate): the vendor fitters', ATMISP's and ours.
pub fn parse(text: &str) -> Result<Fuses, String> {
    let f = atf15xx_sim::parse_jed(text).map_err(|e| format!("{e:#}"))?;
    Ok(Fuses { family: f.family, bits: f.bits, security: f.security })
}

/// Write the fuse array as a JEDEC file (JESD3-C), one `L` field per fuse-map item, in address
/// order, each region introduced by an `N` (note) field. Every fuse is written explicitly; the
/// layout keeps every field under 200 bytes (ATMISP's reader limit, spec §3.3). `created` is a
/// free-form date for the header.
pub fn write(fz: &Fuses, created: &str, pin_count: u16) -> String {
    let f = fz.family;
    let mut items: Vec<(usize, usize, String)> = vec![];
    for mc in 1..=f.macrocells() {
        for pt in 1..=5 {
            items.push((f.pt_addr(mc, pt), 96, format!("MC{mc} PT{pt}")));
        }
    }
    for (a, b, _) in f.mc_fields() {
        items.push((a, 32, format!("block {} macrocell configuration", block_letter(b))));
    }
    for a in f.mc_filler_fields() {
        items.push((a, 16, "macrocell configuration (unused)".into()));
    }
    for b in 0..f.blocks() {
        for m in 0..40 {
            items.push((f.uim_addr(b, m), f.uim_width(), format!("block {} UIM mux {m}", block_letter(b))));
        }
    }
    for n in 1..=6 {
        items.push((f.goe_addr(n), f.uim_width(), format!("global OE {n}")));
    }
    items.push((f.config_base(), 32, "device configuration".into()));
    items.push((f.special_base(), 4, "JTAG special bits".into()));
    items.push((f.ues_base(), 16, "user electronic signature".into()));
    let (r, n) = f.reserved();
    items.push((r, n, "reserved".into()));
    items.sort();
    debug_assert_eq!(items.iter().map(|i| i.1).sum::<usize>(), f.fuse_count());

    let crlf = "\r\n";
    let mut o = String::new();
    write!(o, "\x02JEDEC fuse map for {}, written by open-atf150x {}{crlf}", f.chip_name(), env!("CARGO_PKG_VERSION")).unwrap();
    write!(o, "Created: {created}*{crlf}").unwrap();
    write!(o, "QF{}* QP{pin_count}* F0* G{}*{crlf}", f.fuse_count(), u8::from(fz.security)).unwrap();
    for (a, len, label) in items {
        write!(o, "L{a:05} {}* N {label}*{crlf}", fz.range_str(a, len)).unwrap();
    }
    write!(o, "C{:04X}*{crlf}\x030000{crlf}", fz.checksum()).unwrap();
    o
}

/// The date of a JED: `Created on:` (vendor files) or `Created:` (ours), if present.
pub fn created_on(text: &str) -> Option<&str> {
    let (i, end) = match text.find("Created on: ") {
        Some(i) => (i + 12, &['\r', '\n'][..]),
        None => (text.find("Created: ")? + 9, &['*', '\r', '\n'][..]),
    };
    let e = text[i..].find(end)? + i;
    Some(&text[i..e])
}

/// `QP` pin count of a JED.
pub fn pin_count(text: &str) -> Option<u16> {
    let i = text.find("QP")? + 2;
    text[i..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}
