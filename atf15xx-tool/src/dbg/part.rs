//! The boundary register of a part. The cell map, opcodes and IDCODEs are generated at build time
//! by `atf15xx-arch` from Atmel's BSDL files (shipped with prjbureau) and checked there against
//! the device tables (`atf15xx_arch::bsdl`); nothing is parsed at run time.
//!
//! Cell numbering follows the BSDL: cell 0 is the one nearest TDO, i.e. shifted out first.

use anyhow::{anyhow, Result};
use std::collections::BTreeMap;

pub use atf15xx_arch::bsdl::{Cell, CellKind, DedicatedPin};

/// The JTAG view of a part: instructions, IDCODE and boundary register.
#[derive(Clone, Debug)]
pub struct Part {
    /// e.g. `ATF1508AS-PLCC84`
    pub name: String,
    /// The device, e.g. `P1508C84`.
    pub device: String,
    /// The BSDL file the map was generated from.
    pub bsdl_file: String,
    pub ir_len: usize,
    pub opcodes: BTreeMap<String, u64>,
    /// IDCODE values the part may report (the BSDL's `X` bits expanded).
    pub idcodes: Vec<u32>,
    pub length: usize,
    pub cells: Vec<Cell>,
    /// Disagreements between the BSDL and the device table that do not change the cell map.
    pub notes: Vec<String>,
    /// TDI, TMS, TCK, TDO: their macrocells' cells exist, but the pads belong to the TAP.
    pub jtag_pins: Vec<u16>,
    /// The BSDL's maximum TCK (TAP_SCAN_CLOCK), in kHz.
    pub max_tck_khz: Option<u32>,
}

impl Part {
    /// The part `ATF1508AS-PLCC84`.
    pub fn load(name: &str) -> Result<Part> {
        let p = atf15xx_arch::bsdl::part(name).map_err(|e| anyhow!(e))?;
        Ok(Part {
            name: p.name.to_string(),
            device: p.device.to_string(),
            bsdl_file: p.bsdl_file.to_string(),
            ir_len: p.ir_len,
            opcodes: p.opcodes.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            idcodes: p.idcodes.to_vec(),
            length: p.length,
            cells: p.cells.to_vec(),
            notes: p.notes.iter().map(|n| n.to_string()).collect(),
            jtag_pins: p.jtag_pins.to_vec(),
            max_tck_khz: p.max_tck_khz,
        })
    }

    pub fn opcode(&self, name: &str) -> Result<u64> {
        self.opcodes
            .get(name)
            .copied()
            .ok_or_else(|| anyhow!("{}: no {name} instruction", self.name))
    }

    pub fn cell(&self, name: &str) -> Option<&Cell> {
        self.cells
            .iter()
            .find(|c| c.name().eq_ignore_ascii_case(name))
    }

    pub fn mc_cell(&self, mc: u16, kind: CellKind) -> Option<&Cell> {
        self.cells
            .iter()
            .find(|c| c.mc == Some(mc) && c.kind == kind)
    }

    pub fn macrocells(&self) -> u16 {
        self.cells.iter().filter_map(|c| c.mc).max().unwrap_or(0)
    }
}

/// The BSDL files whose IDCODE matches `id` (e.g. `1508AS_J84.BSD`): identifies a die (the
/// IDCODE names the device and voltage, not the package).
pub fn identify(id: u32) -> Vec<String> {
    atf15xx_arch::bsdl::identify(id)
        .into_iter()
        .map(String::from)
        .collect()
}
