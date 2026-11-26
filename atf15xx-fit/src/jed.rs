//! Fuse arrays and JEDEC files: a region-structured writer (`write`) and a reader (`parse`) that
//! accepts any JESD3 fuse file, including the vendor fitters' and ATMISP's.

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

/// Parse any JEDEC file (fields are `*`-terminated; `L` fields override the `F` default).
pub fn parse(text: &str) -> Result<Fuses, String> {
    let body = text.split('\x02').nth(1).unwrap_or(text);
    let body = body.split('\x03').next().unwrap_or(body);
    let mut fields = body.split('*').map(str::trim);
    fields.next(); // design specification (header text)
    let mut qf = None;
    let mut default = false;
    let mut security = false;
    let mut ls: Vec<(usize, String)> = vec![];
    let mut device = None;
    for f in fields {
        if let Some(n) = f.strip_prefix("QF") {
            qf = Some(n.trim().parse::<usize>().map_err(|e| e.to_string())?);
        } else if f == "F0" || f == "F1" {
            default = f == "F1";
        } else if f == "G1" {
            security = true;
        } else if let Some(rest) = f.strip_prefix('L') {
            let mut it = rest.splitn(2, |c: char| c.is_whitespace());
            let a: usize = it.next().unwrap_or("").parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
            let bits: String = it.next().unwrap_or("").chars().filter(|c| *c == '0' || *c == '1').collect();
            ls.push((a, bits));
        }
    }
    for tag in ["JEDEC file for: ATF", "JEDEC fuse map for ATF"] {
        if let Some(i) = text.find(tag) {
            device = text.get(i + tag.len()..i + tag.len() + 4).and_then(Family::from_number);
        }
    }
    let qf = qf.ok_or("no QF field")?;
    let family = device
        .filter(|f| f.fuse_count() == qf)
        .or_else(|| Family::ALL.into_iter().find(|f| f.fuse_count() == qf))
        .ok_or("unknown device")?;
    let mut fz = Fuses { family, bits: vec![default; qf], security };
    for (a, bits) in ls {
        for (i, c) in bits.chars().enumerate() {
            fz.bits[a + i] = c == '1';
        }
    }
    Ok(fz)
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
