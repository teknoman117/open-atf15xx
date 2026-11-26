//! Architecture and fuse-map queries for scripts (plain text, one fact per line), so tools in
//! other languages get the tables from the Rust crates instead of a copy:
//! `atf15xx-sim device`, `fuses`, `mc`, `stats`.

use anyhow::{anyhow, bail, Result};
use atf15xx_arch::decode::{Decoded, Literal};
use atf15xx_arch::{block_letter, Device, Family, McBit};
use std::fmt::Write;

/// `device P1508C84`: the device record.
///
/// ```text
/// device P1508C84 family 1508 package PLCC84 tt2 f1508plcc84
/// io PIN MC                (every I/O pin)
/// input PIN MC             (input-only pins that reach a macrocell pad)
/// gclk1 PIN / oe1 PIN / gclr PIN / gclk2_oe2 PIN / gclk3 PIN / pd1 PIN / pd2 PIN
/// jtag TDI TMS TCK TDO
/// ```
pub fn device_lines(d: &Device) -> String {
    let mut o = format!("device {} family {} package {} tt2 {}\n", d.name, d.family.number(), d.package, d.tt2_device);
    for (p, m) in d.io_pins() {
        writeln!(o, "io {p} {m}").unwrap();
    }
    for (p, m) in d.input_only_pins() {
        writeln!(o, "input {p} {m}").unwrap();
    }
    for (k, v) in [("gclk1", d.gclk1), ("oe1", d.oe1), ("gclr", d.gclr), ("gclk2_oe2", d.gclk2_oe2), ("gclk3", d.gclk3), ("pd1", d.pd1), ("pd2", d.pd2)] {
        writeln!(o, "{k} {v}").unwrap();
    }
    writeln!(o, "jtag {} {} {} {}", d.jtag.tdi, d.jtag.tms, d.jtag.tck, d.jtag.tdo).unwrap();
    o
}

/// `fuses 1508`: every fuse field of a family and the selectable inputs.
///
/// ```text
/// qf N / blocks N / uim_width N
/// pt ADDR MC PT            (96 fuses)
/// mcbit ADDR MC NAME       (1 fuse)
/// uim ADDR BLOCK MUX       (uim_width fuses)
/// goe ADDR N               (uim_width fuses, network N = 1..6)
/// config ADDR / special ADDR / ues ADDR / reserved ADDR LEN
/// uim-input MUX POS NAME   (selecting position POS of mux MUX reads NAME; XXXX = never selected)
/// goe-input N POS NAME
/// ```
pub fn fuse_lines(f: Family) -> String {
    let mut o = format!("qf {}\nblocks {}\nuim_width {}\n", f.fuse_count(), f.blocks(), f.uim_width());
    for mc in 1..=f.macrocells() {
        for pt in 1..=5 {
            writeln!(o, "pt {} {mc} {pt}", f.pt_addr(mc, pt)).unwrap();
        }
    }
    for mc in 1..=f.macrocells() {
        for b in McBit::ALL {
            writeln!(o, "mcbit {} {mc} {}", f.mc_bit_addr(mc, b), b.name()).unwrap();
        }
    }
    for b in 0..f.blocks() {
        for m in 0..40 {
            writeln!(o, "uim {} {} {m}", f.uim_addr(b, m), block_letter(b)).unwrap();
        }
    }
    for n in 1..=6 {
        writeln!(o, "goe {} {n}", f.goe_addr(n)).unwrap();
    }
    let (ra, rn) = f.reserved();
    writeln!(o, "config {}\nspecial {}\nues {}\nreserved {ra} {rn}", f.config_base(), f.special_base(), f.ues_base()).unwrap();
    for m in 0..40 {
        for (i, s) in f.uim_choices(m).iter().enumerate() {
            writeln!(o, "uim-input {m} {i} {}", s.vendor_name()).unwrap();
        }
    }
    for n in 1..=6 {
        for (i, s) in f.goe_choices(n).iter().enumerate() {
            writeln!(o, "goe-input {n} {i} {}", s.vendor_name()).unwrap();
        }
    }
    o
}

/// `mc X.jed -d D --mc 16`: a macrocell's configuration bits and product terms.
pub fn mc_lines(dec: &Decoded, dev: &Device, mc: u16) -> Result<String> {
    let c = dec.macrocells.get(mc as usize - 1).ok_or_else(|| anyhow!("no MC{mc} in {}", dev.name))?;
    let b = (mc as usize - 1) / 16;
    let pin = dev.mc_pin(mc).map_or("-".to_string(), |p| p.to_string());
    let mut o = format!("MC{mc} (block {}, pin {pin}):\n ", block_letter(b));
    for &bit in McBit::ALL.iter() {
        write!(o, " {}={}", bit.name(), u8::from(c.bit(bit))).unwrap();
    }
    o.push('\n');
    for (k, pt) in c.pts.iter().enumerate() {
        match pt {
            None => writeln!(o, "  PT{}: unused", k + 1).unwrap(),
            Some(lits) => {
                let txt: Vec<String> = lits
                    .iter()
                    .map(|l| match *l {
                        Literal::Uim { mux, positive } => {
                            let name = dec.uim[b][mux as usize].map_or(format!("uim{mux}?"), |s| s.vendor_name());
                            format!("{}{name}", if positive { "" } else { "!" })
                        }
                        Literal::Foldback { local } => format!("fold{local}"),
                    })
                    .collect();
                writeln!(o, "  PT{}: {}", k + 1, if txt.is_empty() { "1".into() } else { txt.join(" & ") }).unwrap();
            }
        }
    }
    Ok(o)
}

/// Resource counts of a fitted fuse map, measured the same way whoever fitted it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Macrocells with logic (a product term in the sum, a cascade, or the fast input path).
    pub macrocells: usize,
    /// Macrocells used only to lend a foldback term.
    pub foldback_only: usize,
    pub product_terms: usize,
    pub foldbacks: usize,
    pub cascades: usize,
}

pub fn stats(dec: &Decoded) -> Stats {
    let mut s = Stats::default();
    for c in &dec.macrocells {
        let p = &c.pts;
        let fold = c.bit(McBit::Pt1Mux) && p[0].is_some();
        let logic = p[1..].iter().any(Option::is_some)
            || (p[0].is_some() && !fold)
            || c.bit(McBit::CasMux)
            || (!c.bit(McBit::DMux) && !c.bit(McBit::OMux));
        s.macrocells += usize::from(logic);
        s.foldback_only += usize::from(fold && !logic);
        s.product_terms += p.iter().filter(|x| x.is_some()).count();
        s.foldbacks += usize::from(fold);
        s.cascades += usize::from(c.bit(McBit::CasMux) && p.iter().any(Option::is_some));
    }
    s
}

/// `MC16.CasMux=1` -> (fuse address, value).
pub fn parse_setting(f: Family, s: &str) -> Result<(usize, bool)> {
    let bad = || anyhow!("bad setting '{s}' (want MC<n>.<Bit>=0|1)");
    let (lhs, v) = s.split_once('=').ok_or_else(bad)?;
    let (mc, bit) = lhs.strip_prefix("MC").and_then(|r| r.split_once('.')).ok_or_else(bad)?;
    let mc: usize = mc.parse().map_err(|_| bad())?;
    if !(1..=f.macrocells()).contains(&mc) {
        bail!("no MC{mc} in the {f}");
    }
    let b = McBit::ALL
        .into_iter()
        .find(|b| b.name() == bit)
        .ok_or_else(|| anyhow!("unknown bit {bit} (known: {})", McBit::ALL.map(|b| b.name()).join(" ")))?;
    let v = match v {
        "0" => false,
        "1" => true,
        _ => return Err(bad()),
    };
    Ok((f.mc_bit_addr(mc, b), v))
}

/// Set fuses of a JEDEC file in place: only digits inside existing `L` fields change, so the layout
/// stays the same; the `C` checksum is recomputed. Fails if a fuse is in no `L` field.
pub fn patch_jed(text: &str, sets: &[(usize, bool)]) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut done = vec![false; sets.len()];
    let mut rest = text;
    // fields are `*`-terminated; patch the L fields
    while let Some(i) = rest.find('*') {
        let (field, tail) = rest.split_at(i + 1);
        let body = &field[..field.len() - 1];
        let t = body.trim_start();
        if let Some(r) = t.strip_prefix('L') {
            let lead = body.len() - t.len();
            let digits: String = r.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(a) = digits.parse::<usize>() {
                let (head, bits) = body.split_at(lead + 1 + digits.len());
                let mut k = a;
                let mut nb = String::with_capacity(bits.len());
                for ch in bits.chars() {
                    if ch == '0' || ch == '1' {
                        let mut c = ch;
                        for (j, &(fa, v)) in sets.iter().enumerate() {
                            if fa == k {
                                c = if v { '1' } else { '0' };
                                done[j] = true;
                            }
                        }
                        nb.push(c);
                        k += 1;
                    } else {
                        nb.push(ch);
                    }
                }
                out.push_str(head);
                out.push_str(&nb);
                out.push('*');
                rest = tail;
                continue;
            }
        }
        out.push_str(field);
        rest = tail;
    }
    out.push_str(rest);
    if let Some(j) = done.iter().position(|d| !d) {
        bail!("fuse {} is in no L field of the file", sets[j].0);
    }
    let fz = crate::parse_jed(&out)?;
    let mut sum = 0u32;
    for chunk in fz.bits.chunks(8) {
        sum += chunk.iter().enumerate().map(|(k, &b)| u32::from(b) << k).sum::<u32>();
    }
    let csum = format!("C{:04X}*", sum & 0xffff);
    let Some(ci) = find_c_field(&out) else { return Ok(out) };
    let end = out[ci..].find('*').map(|e| ci + e + 1).unwrap();
    out.replace_range(ci..end, &csum);
    Ok(out)
}

/// Start of the `C` (checksum) field: a `C` that begins a field (after `*` and whitespace).
fn find_c_field(text: &str) -> Option<usize> {
    let b = text.as_bytes();
    let mut i = 0;
    while let Some(j) = text[i..].find('*') {
        let mut k = i + j + 1;
        while k < b.len() && (b[k] as char).is_whitespace() {
            k += 1;
        }
        if k < b.len() && b[k] == b'C' && b.get(k + 1).is_some_and(|c| c.is_ascii_hexdigit()) {
            return Some(k);
        }
        i = i + j + 1;
    }
    None
}
