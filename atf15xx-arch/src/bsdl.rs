//! The boundary register of each part (ATF1502AS/1504AS/1508AS in every package), generated at
//! build time from Atmel's BSDL files as shipped with prjbureau and checked against the device
//! tables (see `build/bsdl.rs`). Cell 0 is the one nearest TDO, i.e. shifted out first.

/// What a boundary cell observes or controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CellKind {
    /// A pad (or the pinless macrocell's pad input): observe only (BC_4).
    In,
    /// A macrocell's output-enable value (SAMPLE) / the pad's enable (EXTEST).
    Oe,
    /// A macrocell's output-mux value (SAMPLE) / the pad's data (EXTEST).
    Out,
}

/// A dedicated input pin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DedicatedPin {
    Gclk1,
    Gclk2,
    Gclr,
    Oe1,
}

impl DedicatedPin {
    pub fn name(self) -> &'static str {
        match self {
            DedicatedPin::Gclk1 => "GCLK1",
            DedicatedPin::Gclk2 => "GCLK2",
            DedicatedPin::Gclr => "GCLR",
            DedicatedPin::Oe1 => "OE1",
        }
    }
}

/// One boundary cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    /// Index in the boundary register (0 = nearest TDO).
    pub bit: usize,
    pub kind: CellKind,
    /// The macrocell, for macrocell cells.
    pub mc: Option<u16>,
    /// The dedicated input, for dedicated input cells.
    pub dedicated: Option<DedicatedPin>,
    /// The package pin, if any.
    pub pin: Option<u16>,
}

impl Cell {
    /// `MC12.in`, `MC12.oe`, `MC12.out`, `GCLK1`, ...
    pub fn name(&self) -> String {
        match (self.mc, self.dedicated) {
            (Some(mc), _) => format!(
                "MC{mc}.{}",
                match self.kind {
                    CellKind::In => "in",
                    CellKind::Oe => "oe",
                    CellKind::Out => "out",
                }
            ),
            (None, Some(d)) => d.name().to_string(),
            (None, None) => format!("cell{}", self.bit),
        }
    }
}

/// The JTAG view of a part: instructions, IDCODE and boundary register.
#[derive(Debug)]
pub struct Part {
    /// e.g. `ATF1508AS-PLCC84`
    pub name: &'static str,
    /// The device, e.g. `P1508C84`.
    pub device: &'static str,
    /// The BSDL file it was generated from, e.g. `1508AS_J84.BSD`.
    pub bsdl_file: &'static str,
    pub ir_len: usize,
    pub opcodes: &'static [(&'static str, u64)],
    /// IDCODE values the part may report (the BSDL's `X` bits expanded).
    pub idcodes: &'static [u32],
    pub length: usize,
    pub cells: &'static [Cell],
    /// Disagreements between the BSDL and the device table that do not change the cell map.
    pub notes: &'static [&'static str],
    /// TDI, TMS, TCK, TDO: their macrocells' cells exist, but the pads belong to the TAP.
    pub jtag_pins: [u16; 4],
    /// The BSDL's maximum TCK (TAP_SCAN_CLOCK), in kHz.
    pub max_tck_khz: Option<u32>,
}

include!(concat!(env!("OUT_DIR"), "/bsdl.rs"));

/// The part `ATF1508AS-PLCC84` (case-insensitive), or why there is none.
pub fn part(name: &str) -> Result<&'static Part, String> {
    if let Some(p) = PARTS.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
        return Ok(p);
    }
    if let Some((_, e)) = PART_ERRORS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
    {
        return Err(e.to_string());
    }
    Err(format!(
        "no part {name} (known: {})",
        PARTS.iter().map(|p| p.name).collect::<Vec<_>>().join(" ")
    ))
}

/// Every part with a boundary-register map.
pub fn parts() -> &'static [Part] {
    PARTS
}

/// The BSDL files whose IDCODE matches `id` (e.g. `1508AS_J84.BSD`): identifies the die (the
/// IDCODE names the device and voltage, not the package).
pub fn identify(id: u32) -> Vec<&'static str> {
    BSDL_IDCODES
        .iter()
        .filter(|(_, ids)| ids.contains(&id))
        .map(|(f, _)| *f)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plcc84() {
        let p = part("atf1508as-plcc84").unwrap();
        assert_eq!(p.device, "P1508C84");
        assert_eq!(p.ir_len, 10);
        assert_eq!(p.length, p.cells.len());
        assert!(identify(p.idcodes[0])
            .iter()
            .any(|f| f.eq_ignore_ascii_case(p.bsdl_file)));
    }
}
