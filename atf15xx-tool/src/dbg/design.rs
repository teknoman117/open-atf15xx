//! Naming a boundary snapshot with a design: the flow's / fitter's `<top>.map.json`.

use crate::dbg::part::{CellKind, Part};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Debug, Deserialize)]
pub struct MapCell {
    pub name: String,
    #[serde(default)]
    pub source: Option<String>,
    pub mc: u16,
    #[serde(default)]
    pub pin: Option<u16>,
    #[serde(default)]
    pub registered: bool,
    #[serde(default = "yes")]
    pub q_is_value: bool,
    /// `reg` or `comb`: what the macrocell's output mux (and so its boundary `out` cell) taps.
    #[serde(default)]
    pub output_tap: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
pub struct MapInput {
    pub name: String,
    #[serde(default)]
    pub source: Option<String>,
    pub pin: u16,
}

/// The parts of `map.json` the debugger uses.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DesignMap {
    #[serde(default)]
    pub cells: Vec<MapCell>,
    #[serde(default)]
    pub inputs: Vec<MapInput>,
}

impl DesignMap {
    pub fn load(path: &Path) -> Result<DesignMap> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

/// How a named value was observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// The register's Q (registered output, or a buried register fitted observable).
    Q,
    /// A register fitted with the output mux on its logic: the cell shows the next-state D.
    D,
    /// A combinatorial cell's value.
    Comb,
    /// An input pin's pad.
    Pad,
}

impl How {
    pub fn tag(self) -> &'static str {
        match self {
            How::Q => "Q",
            How::D => "D",
            How::Comb => "=",
            How::Pad => "in",
        }
    }
}

/// A design signal and the boundary cell that shows it.
#[derive(Clone, Debug)]
pub struct Signal {
    pub name: String,
    pub bit: usize,
    pub how: How,
    /// Complement the cell to get the value (a register storing its complement).
    pub invert: bool,
}

impl Signal {
    pub fn value(&self, bits: &[bool]) -> bool {
        bits[self.bit] != self.invert
    }
}

/// The design's signals that the boundary register shows, in map order (cells, then inputs).
pub fn signals(part: &Part, map: &DesignMap) -> Vec<Signal> {
    let mut out = vec![];
    for c in &map.cells {
        let Some(cell) = part.mc_cell(c.mc, CellKind::Out) else { continue };
        let name = c.source.clone().unwrap_or_else(|| c.name.clone());
        let (how, invert) = if c.registered && c.output_tap.as_deref() != Some("comb") {
            (How::Q, !c.q_is_value)
        } else if c.registered {
            (How::D, false)
        } else {
            (How::Comb, false)
        };
        out.push(Signal { name, bit: cell.bit, how, invert });
    }
    for i in &map.inputs {
        if let Some(cell) = part.cells.iter().find(|c| c.kind == CellKind::In && c.pin == Some(i.pin)) {
            out.push(Signal { name: i.source.clone().unwrap_or_else(|| i.name.clone()), bit: cell.bit, how: How::Pad, invert: false });
        }
    }
    out
}
