//! Decoding boundary captures: views by pin, macrocell and design signal; changes; VCD.

use crate::dbg::design::Signal;
use crate::dbg::part::{CellKind, Part};
use anyhow::{anyhow, bail, Result};
use std::fmt::Write as _;
use std::io::Write;

/// One macrocell's three cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McState {
    pub mc: u16,
    pub pin: Option<u16>,
    /// The pad (or pinless macrocell input).
    pub pad: bool,
    /// The macrocell's output enable.
    pub oe: bool,
    /// The macrocell's output-mux value (register Q when the output taps the register).
    pub out: bool,
}

pub fn macrocells(part: &Part, bits: &[bool]) -> Vec<McState> {
    (1..=part.macrocells())
        .filter_map(|mc| {
            let get = |k| part.mc_cell(mc, k).map(|c| bits[c.bit]);
            Some(McState {
                mc,
                pin: part.mc_cell(mc, CellKind::Out).and_then(|c| c.pin),
                pad: get(CellKind::In)?,
                oe: get(CellKind::Oe)?,
                out: get(CellKind::Out)?,
            })
        })
        .collect()
}

/// The dedicated inputs (GCLK1, GCLK2, GCLR, OE1) and their values.
pub fn dedicated(part: &Part, bits: &[bool]) -> Vec<(String, Option<u16>, bool)> {
    part.cells.iter().filter_map(|c| c.dedicated.map(|d| (d.name().to_string(), c.pin, bits[c.bit]))).collect()
}

/// Human-readable snapshot: dedicated inputs, design signals, optionally every macrocell.
pub fn render(part: &Part, bits: &[bool], signals: &[Signal], all_macrocells: bool) -> String {
    let mut o = String::new();
    let ded: Vec<String> = dedicated(part, bits).iter().map(|(n, p, v)| format!("{n}(pin {})={}", p.map_or("-".into(), |p| p.to_string()), u8::from(*v))).collect();
    writeln!(o, "dedicated: {}", ded.join(" ")).unwrap();
    if !signals.is_empty() {
        let w = signals.iter().map(|s| s.name.len()).max().unwrap_or(0);
        for s in signals {
            writeln!(o, "  {:w$}  {:>2} {}", s.name, s.how.tag(), u8::from(s.value(bits))).unwrap();
        }
    }
    if all_macrocells || signals.is_empty() {
        writeln!(o, "  MC    pin  pad oe out").unwrap();
        for m in macrocells(part, bits) {
            writeln!(o, "  {:<5} {:>3}   {}  {}  {}", format!("MC{}", m.mc), m.pin.map_or("-".into(), |p| p.to_string()), u8::from(m.pad), u8::from(m.oe), u8::from(m.out)).unwrap();
        }
    }
    o
}

/// What a trace records: design signals, else every macrocell cell and the dedicated inputs.
pub fn trace_columns(part: &Part, signals: &[Signal]) -> Vec<(String, usize, bool)> {
    if !signals.is_empty() {
        return signals.iter().map(|s| (s.name.clone(), s.bit, s.invert)).collect();
    }
    part.cells.iter().map(|c| (c.name(), c.bit, false)).collect()
}

/// Columns whose value differs between two captures: `(name, old, new)`.
pub fn changes(columns: &[(String, usize, bool)], prev: &[bool], cur: &[bool]) -> Vec<(String, bool, bool)> {
    columns
        .iter()
        .filter(|(_, b, _)| prev[*b] != cur[*b])
        .map(|(n, b, inv)| (n.clone(), prev[*b] != *inv, cur[*b] != *inv))
        .collect()
}

/// A VCD file of repeated captures (timestamps in microseconds since the first capture).
pub struct VcdCapture<W: Write> {
    w: vcd::Writer<W>,
    columns: Vec<(String, usize, bool)>,
    ids: Vec<vcd::IdCode>,
    prev: Option<Vec<bool>>,
}

impl<W: Write> VcdCapture<W> {
    pub fn new(out: W, part: &Part, columns: Vec<(String, usize, bool)>) -> Result<Self> {
        let mut w = vcd::Writer::new(out);
        w.comment(&format!("atf15xx-tool boundary-scan SAMPLE captures of {} ({})", part.name, part.device))?;
        w.timescale(1, vcd::TimescaleUnit::US)?;
        w.add_module(&vcd_ident(&part.name))?;
        let mut ids = vec![];
        for (name, _, _) in &columns {
            ids.push(w.add_wire(1, &vcd_ident(name))?);
        }
        w.upscope()?;
        w.enddefinitions()?;
        Ok(VcdCapture { w, columns, ids, prev: None })
    }

    pub fn record(&mut self, t_us: u64, bits: &[bool]) -> Result<()> {
        let changed: Vec<usize> = (0..self.columns.len())
            .filter(|&i| self.prev.as_ref().is_none_or(|p| p[self.columns[i].1] != bits[self.columns[i].1]))
            .collect();
        if !changed.is_empty() {
            self.w.timestamp(t_us)?;
            for i in changed {
                let (_, bit, inv) = self.columns[i];
                self.w.change_scalar(self.ids[i], if bits[bit] != inv { vcd::Value::V1 } else { vcd::Value::V0 })?;
            }
        }
        self.prev = Some(bits.to_vec());
        Ok(())
    }

    /// Close the trace at `t_us` and flush it.
    pub fn finish(mut self, t_us: u64) -> Result<()> {
        self.w.timestamp(t_us)?;
        self.w.flush()?;
        Ok(())
    }
}

/// Parse `PIN=V` / `MCn=V` / `SIGNAL=V` assignments for EXTEST into `(mc, value)`.
pub fn parse_drives(part: &Part, signals: &[Signal], args: &[String]) -> Result<Vec<(u16, bool)>> {
    let mut out = vec![];
    for a in args {
        let (k, v) = a.split_once('=').ok_or_else(|| anyhow!("{a}: expected NAME=0|1"))?;
        let v = match v {
            "0" => false,
            "1" => true,
            _ => bail!("{a}: value must be 0 or 1"),
        };
        let mc = if let Some(n) = k.strip_prefix("MC").or_else(|| k.strip_prefix("mc")).and_then(|n| n.parse::<u16>().ok()) {
            n
        } else if let Some(p) = k.strip_prefix("pin").and_then(|n| n.parse::<u16>().ok()).or_else(|| k.parse::<u16>().ok()) {
            part.cells
                .iter()
                .find(|c| c.kind == CellKind::Out && c.pin == Some(p))
                .and_then(|c| c.mc)
                .ok_or_else(|| anyhow!("pin {p} is no I/O pin of {}", part.name))?
        } else if let Some(s) = signals.iter().find(|s| s.name == k) {
            part.cells.iter().find(|c| c.bit == s.bit).and_then(|c| c.mc).ok_or_else(|| anyhow!("{k}: no macrocell"))?
        } else {
            bail!("{k}: not MCn, a pin number or a design signal")
        };
        match part.mc_cell(mc, CellKind::Out).and_then(|c| c.pin) {
            None => bail!("{k}: MC{mc} has no pin on {}", part.name),
            Some(p) if part.jtag_pins.contains(&p) => bail!("{k}: pin {p} is a JTAG pin"),
            _ => {}
        }
        out.push((mc, v));
    }
    Ok(out)
}

/// The EXTEST vector: every cell as captured in `base` (so pins keep their current state and
/// direction), except that each driven macrocell's pin is enabled and set.
pub fn drive_vector(part: &Part, base: &[bool], drives: &[(u16, bool)]) -> Vec<bool> {
    let mut bits = base.to_vec();
    for &(mc, v) in drives {
        if let Some(c) = part.mc_cell(mc, CellKind::Oe) {
            bits[c.bit] = true;
        }
        if let Some(c) = part.mc_cell(mc, CellKind::Out) {
            bits[c.bit] = v;
        }
    }
    bits
}

/// VCD identifiers may not contain spaces; signal names from Verilog may contain `[`, `]`, `.`.
fn vcd_ident(s: &str) -> String {
    s.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect()
}
