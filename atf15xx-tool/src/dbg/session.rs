//! A scripted EXTEST session: one command per line, each answered with one line, while the device
//! stays in EXTEST (spec D1.7). For stimulating a design in circuit: driving its input pins
//! (an 8080/Z80-style bus, say) step by step and reading what its logic does.
//!
//! ```text
//! drive SIG=V ...    drive pins (persistent); SIG is a design signal, MCn or a pin number;
//!                    a bus SIG=0x5A drives SIG[0], SIG[1], ... (bit 0 = least significant)
//! float SIG ...      stop driving (the pin goes high-impedance)
//! scan               one EXTEST scan, nothing changed
//! print SIG ...      scan, print the values (buses in hex)
//! expect SIG=V ...   scan, compare; a mismatch is reported and counted
//! wait MS            sleep
//! # ...              comment
//! ```
//!
//! While EXTEST is active, the boundary register, not the design, drives every pin. Unless
//! `hold_outputs` is set, each scan's vector takes the output and output-enable cells from the
//! previous capture, which on these parts holds what the macrocell logic would drive: the
//! design's own outputs follow it, one scan late. Pins that are driven, and their output
//! enables, come from the session. Input cells are observe-only (`BC_4`), so the logic sees
//! the levels at the pads, including the driven ones.
use crate::dbg::design::Signal;
use crate::dbg::jtag::{Jtag, Scanner};
use crate::dbg::part::{CellKind, Part};
use crate::dbg::snapshot;
use anyhow::{anyhow, bail, Result};
use std::collections::BTreeMap;
use std::time::Duration;

/// Options of a session.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Allow driving pins that are inputs of the running design (their output enable is 0), which
    /// something on the board may be driving.
    pub allow_inputs: bool,
    /// Keep the non-driven pins as they were at the start, instead of following the design.
    pub hold_outputs: bool,
}

pub struct Session<'a, J: Jtag> {
    scan: Scanner<'a, J>,
    part: &'a Part,
    signals: &'a [Signal],
    options: Options,
    /// The capture before the session started: decides which pins the design drives.
    initial: Vec<bool>,
    /// The latest capture (EXTEST captures what the logic would drive, and the pads).
    last: Vec<bool>,
    drives: BTreeMap<u16, bool>,
    /// Macrocells released by `float`: high-impedance until driven again.
    floating: BTreeMap<u16, ()>,
    pub failures: usize,
}

impl<'a, J: Jtag> Session<'a, J> {
    /// Start a session: one SAMPLE to see the design's pin directions. EXTEST begins with the
    /// first scan.
    pub fn new(jtag: &'a mut J, part: &'a Part, signals: &'a [Signal], options: Options) -> Result<Self> {
        let mut scan = Scanner::new(jtag, part);
        let initial = scan.sample()?;
        Ok(Session {
            scan,
            part,
            signals,
            options,
            last: initial.clone(),
            initial,
            drives: BTreeMap::new(),
            floating: BTreeMap::new(),
            failures: 0,
        })
    }

    /// The latest capture.
    pub fn bits(&self) -> &[bool] {
        &self.last
    }

    /// Execute one command line; returns the line to print (`None` for comments / blank lines).
    pub fn exec(&mut self, line: &str) -> Result<Option<String>> {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut words = line.split_whitespace();
        let Some(cmd) = words.next() else { return Ok(None) };
        let args: Vec<&str> = words.collect();
        match cmd {
            "drive" => {
                let mut set = vec![];
                for a in &args {
                    set.extend(self.assignments(a)?);
                }
                for &(mc, v) in &set {
                    self.check_drivable(mc)?;
                    self.drives.insert(mc, v);
                    self.floating.remove(&mc);
                }
                self.step()?;
                Ok(Some(format!("ok drive {}", args.join(" "))))
            }
            "float" => {
                for a in &args {
                    for mc in self.macrocells(a)? {
                        self.drives.remove(&mc);
                        self.floating.insert(mc, ());
                    }
                }
                self.step()?;
                Ok(Some(format!("ok float {}", args.join(" "))))
            }
            "scan" => {
                // `scan N`: N back-to-back scans (the design's outputs keep following, one scan late)
                let n: u32 = args.first().map_or(Ok(1), |s| s.parse()).map_err(|_| anyhow!("scan [N]"))?;
                for _ in 0..n {
                    self.step()?;
                }
                Ok(Some(if n == 1 { "ok scan".into() } else { format!("ok scan {n}") }))
            }
            "print" => {
                self.step()?;
                let mut out = vec![];
                for a in &args {
                    out.push(format!("{a}={}", self.read(a)?.1));
                }
                Ok(Some(out.join(" ")))
            }
            "expect" => {
                self.step()?;
                let mut bad = vec![];
                for a in &args {
                    let (name, want) = a.split_once('=').ok_or_else(|| anyhow!("{a}: expected NAME=VALUE"))?;
                    let (value, shown) = self.read(name)?;
                    if value != parse_value(want)? {
                        bad.push(format!("{name}={shown} (expected {want})"));
                    }
                }
                if bad.is_empty() {
                    Ok(Some(format!("ok expect {}", args.join(" "))))
                } else {
                    self.failures += 1;
                    Ok(Some(format!("MISMATCH {}", bad.join(" "))))
                }
            }
            "release" => {
                // pins back to the design (BYPASS); the next drive/float/scan/print/expect takes
                // them again, preloaded with the design's own values, so the handover is glitch-free
                self.scan.release()?;
                Ok(Some("ok release".into()))
            }
            "sample" => {
                // non-intrusive SAMPLE: the design keeps its pins (also leaves EXTEST)
                self.last = self.scan.sample()?;
                let mut out = vec![];
                for a in &args {
                    out.push(format!("{a}={}", self.read(a)?.1));
                }
                Ok(Some(out.join(" ")))
            }
            "wait" => {
                let ms: u64 = args.first().ok_or_else(|| anyhow!("wait MS"))?.parse()?;
                std::thread::sleep(Duration::from_millis(ms));
                Ok(Some(format!("ok wait {ms}")))
            }
            other => bail!("unknown command {other:?} (drive, float, scan, print, expect, sample, release, wait)"),
        }
    }

    /// End the session: BYPASS gives the pins back to the design.
    pub fn finish(mut self) -> Result<()> {
        self.scan.release()
    }

    fn step(&mut self) -> Result<()> {
        let base = if self.options.hold_outputs { self.initial.clone() } else { self.last.clone() };
        // (the captured output enables already release the pins the design does not drive)
        let mut bits = base;
        for &mc in self.floating.keys() {
            if let Some(c) = self.part.mc_cell(mc, CellKind::Oe) {
                bits[c.bit] = false;
            }
        }
        let drives: Vec<(u16, bool)> = self.drives.iter().map(|(&m, &v)| (m, v)).collect();
        let bits = snapshot::drive_vector(self.part, &bits, &drives);
        self.last = self.scan.extest(&bits)?;
        Ok(())
    }

    fn check_drivable(&self, mc: u16) -> Result<()> {
        if self.options.allow_inputs {
            return Ok(());
        }
        let oe = self.part.mc_cell(mc, CellKind::Oe).map(|c| self.initial[c.bit]).unwrap_or(false);
        if !oe {
            bail!(
                "MC{mc}'s pin is an input of the design (output enable 0): something else may be driving it; \
                   use --allow-inputs once it is free (e.g. the CPU holds the bus released)"
            );
        }
        Ok(())
    }

    /// The bits of a bus `NAME`: the design signals `NAME[i]`, index ascending.
    fn bus(&self, name: &str) -> Vec<(usize, &Signal)> {
        let mut bits: Vec<(usize, &Signal)> = self
            .signals
            .iter()
            .filter_map(|s| {
                let rest = s.name.strip_prefix(name)?.strip_prefix('[')?.strip_suffix(']')?;
                Some((rest.parse().ok()?, s))
            })
            .collect();
        if bits.is_empty() {
            // the flow names the bits of an inout bus `D0`, `D1`, ...: accept a plain digit suffix
            bits = self
                .signals
                .iter()
                .filter_map(|s| {
                    let rest = s.name.strip_prefix(name)?;
                    (!rest.is_empty() && rest.bytes().all(|c| c.is_ascii_digit())).then(|| (rest.parse().ok(), s))
                })
                .filter_map(|(i, s)| Some((i?, s)))
                .collect();
        }
        bits.sort_by_key(|b| b.0);
        bits
    }

    /// The macrocells an assignment target names: a signal, MCn, a pin, or a bus.
    fn macrocells(&self, name: &str) -> Result<Vec<u16>> {
        let bus = self.bus(name);
        if !bus.is_empty() && !self.signals.iter().any(|s| s.name == name) {
            return bus.iter().map(|(_, s)| self.mc_of(s)).collect();
        }
        Ok(snapshot::parse_drives(self.part, self.signals, &[format!("{name}=0")])?.into_iter().map(|(m, _)| m).collect())
    }

    fn mc_of(&self, s: &Signal) -> Result<u16> {
        self.part.cells.iter().find(|c| c.bit == s.bit).and_then(|c| c.mc).ok_or_else(|| anyhow!("{}: no macrocell", s.name))
    }

    /// `NAME=V` as (macrocell, value) pairs; buses take a number (bit 0 = `NAME[0]`).
    fn assignments(&self, a: &str) -> Result<Vec<(u16, bool)>> {
        let (name, v) = a.split_once('=').ok_or_else(|| anyhow!("{a}: expected NAME=VALUE"))?;
        let bus = self.bus(name);
        if !bus.is_empty() && !self.signals.iter().any(|s| s.name == name) {
            let value = parse_value(v)?;
            if bus.last().map_or(0, |b| b.0) < 63 && value >> (bus.last().unwrap().0 + 1) != 0 {
                bail!("{a}: {value:#x} does not fit {name}[0..{}]", bus.last().unwrap().0);
            }
            return bus.iter().map(|&(i, s)| Ok((self.mc_of(s)?, value >> i & 1 == 1))).collect();
        }
        let value = parse_value(v)?;
        if value > 1 {
            bail!("{a}: a single pin takes 0 or 1");
        }
        snapshot::parse_drives(self.part, self.signals, &[format!("{name}={value}")])
    }

    /// The value of a signal, MCn (its pad), pin or bus in the latest capture, and how to show it.
    fn read(&self, name: &str) -> Result<(u64, String)> {
        let bits = &self.last;
        if let Some(s) = self.signals.iter().find(|s| s.name == name) {
            let v = u64::from(s.value(bits));
            return Ok((v, v.to_string()));
        }
        let bus = self.bus(name);
        if !bus.is_empty() {
            let v = bus.iter().fold(0u64, |acc, &(i, s)| acc | (u64::from(s.value(bits)) << i));
            let digits = (bus.last().unwrap().0 + 4) / 4;
            return Ok((v, format!("0x{v:0digits$x}")));
        }
        // MCn or a pin number: its pad
        let mc = snapshot::parse_drives(self.part, self.signals, &[format!("{name}=0")])?[0].0;
        let cell = self.part.mc_cell(mc, CellKind::In).ok_or_else(|| anyhow!("{name}: no input cell"))?;
        let v = u64::from(bits[cell.bit]);
        Ok((v, v.to_string()))
    }
}

fn parse_value(v: &str) -> Result<u64> {
    let v = v.replace('_', "");
    Ok(if let Some(h) = v.strip_prefix("0x") {
        u64::from_str_radix(h, 16)?
    } else if let Some(b) = v.strip_prefix("0b") {
        u64::from_str_radix(b, 2)?
    } else {
        v.parse()?
    })
}
