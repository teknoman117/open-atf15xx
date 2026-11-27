//! The JTAG guard: does a programming image keep the JTAG port? An image that turns the JTAG
//! pins into I/O can then only be reprogrammed with 12 V on OE1 (CLAUDE.md, bench rules).
//!
//! * JED: the JTAG-enable fuse (special-purpose bit 1; 1 = JTAG kept, the erased state), spec
//!   F2.6 (`isp::device::Family::jtag_fuse`).
//! * SVF: every 4-bit write to the special register (`SIR 10 TDI (292)`, then `SDR 4 TDI (x)`)
//!   must have bit 2 set (bit 2 of that SDR is the JED's JTAG-enable fuse, *oracle-verified* on
//!   the ATF1508AS by flipping only that fuse and diffing ATMISP's SVF; the same layout is assumed
//!   for the ATF1502AS/1504AS). The SVF is read with the `svf` crate, command by command.
//!
//! The port of `tools/silicon/jtag_guard.py` (2026-11-27).

use crate::isp::jedec;

/// The special register's instruction (10-bit IR).
const SPECIAL_IR: u32 = 0x292;

/// A pattern's TDI as a number (LSB-first bytes, as the `svf` crate packs them).
fn tdi_value(p: &svf::Pattern) -> Option<u64> {
    let bytes = p.tdi.as_ref()?;
    Some(bytes.iter().take(8).enumerate().fold(0u64, |v, (i, &b)| v | (u64::from(b) << (8 * i))))
}

/// Every 4-bit value an SVF writes to the special register, in order.
pub fn svf_special_writes(text: &str) -> Result<Vec<u8>, String> {
    let cmds = svf::parse_complete(text).map_err(|e| format!("not a valid SVF: {e:?}"))?;
    let mut writes = vec![];
    for (i, c) in cmds.iter().enumerate() {
        let svf::Command::SIR(p) = c else { continue };
        if p.length != 10 || tdi_value(p) != Some(u64::from(SPECIAL_IR)) {
            continue;
        }
        // the data scan follows within a few commands (a RUNTEST may come between)
        let sdr = cmds[i + 1..].iter().take(3).find_map(|c| match c {
            svf::Command::SDR(d) if d.length == 4 => tdi_value(d),
            _ => None,
        });
        if let Some(v) = sdr {
            writes.push((v & 0xf) as u8);
        }
    }
    Ok(writes)
}

/// Whether an SVF keeps JTAG: it writes the special register at least once, always with bit 2
/// set. `Err` when it cannot tell (no such write, or not an SVF).
pub fn svf_keeps_jtag(text: &str) -> Result<bool, String> {
    let writes = svf_special_writes(text)?;
    if writes.is_empty() {
        return Err("no special-register write found (cannot tell)".into());
    }
    Ok(writes.iter().all(|w| (w >> 2) & 1 == 1))
}

/// Whether a JEDEC file keeps JTAG (its JTAG-enable fuse is 1). `Err` for a file that is not an
/// ATF15xx fuse map.
pub fn jed_keeps_jtag(data: &[u8]) -> Result<bool, String> {
    let j = jedec::parse(data).map_err(|e| e.to_string())?;
    let fam = j.family().ok_or("not an ATF15xx JEDEC file")?;
    Ok(fam.jtag_fuse().is_none_or(|f| j.fuses[f] == b'1'))
}
