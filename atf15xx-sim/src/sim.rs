//! Cycle-based simulator of a [`Model`] (docs/architecture/sim/01-model.md §Simulation).
//!
//! One [`Sim::step`] applies a new set of external pin values: the combinational logic settles
//! (fixpoint iteration, so pin feedback, foldback and cascades work in any order), then every
//! register whose clock went 0→1 since the previous step (or latch whose enable is 1) loads its D,
//! asynchronous reset/preset are applied as levels, and the logic settles again.

use crate::model::{Clock, Lit, Mc, Model, Oe, PinKind, Sig};
use atf15xx_arch::decode::{DSource, PtUse, Storage};
use std::collections::BTreeMap;

/// What the device does on one pin after a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinOut {
    /// The device drives the pin.
    pub oe: bool,
    /// The driven value (meaningful only when `oe`).
    pub value: bool,
}

pub struct Sim<'m> {
    pub model: &'m Model,
    pub q: Vec<bool>,
    x: Vec<bool>,
    fb: Vec<bool>,
    pad: Vec<bool>,
    fold: Vec<bool>,
    drive: Vec<Option<bool>>,
    prev_clk: Vec<bool>,
    ext: BTreeMap<u16, bool>,
    /// Steps where the combinational settle did not converge (oscillation).
    pub unstable_steps: u64,
    /// The last step did not settle (combinational loop, e.g. a transparent latch fed back
    /// through its own output pin): the state after it is undefined.
    pub last_unstable: bool,
    /// While true, latches hold (phase 1 of a step: flip-flops sample their pre-edge D).
    freeze_latches: bool,
}

fn mc_idx(mc: u16) -> usize {
    mc as usize - 1
}

impl<'m> Sim<'m> {
    pub fn new(model: &'m Model) -> Sim<'m> {
        let n = model.mcs.len();
        let mut s = Sim {
            model,
            q: model.mcs.iter().map(|m| m.init).collect(),
            x: vec![false; n],
            fb: vec![false; n],
            pad: vec![false; n],
            fold: vec![true; n],
            drive: vec![None; n],
            prev_clk: vec![false; n],
            ext: BTreeMap::new(),
            unstable_steps: 0,
            freeze_latches: false,
            last_unstable: false,
        };
        s.settle();
        s.prev_clk = (0..n).map(|i| s.clock_of(&model.mcs[i])).collect();
        s
    }

    fn sig(&self, s: Sig) -> bool {
        match s {
            Sig::Const(v) => v,
            Sig::Pad(m) => self.pad[mc_idx(m)],
            Sig::Fb(m) => self.fb[mc_idx(m)],
            Sig::Pin(p) => self.ext.get(&p).copied().unwrap_or(false),
        }
    }

    fn pt(&self, mc: &Mc, k: usize) -> bool {
        match &mc.pts[k] {
            None => false,
            Some(lits) => lits.iter().all(|l| match *l {
                Lit::Sig(s, pos) => self.sig(s) == pos,
                Lit::Fold(host) => self.fold[mc_idx(host)],
            }),
        }
    }

    fn sum(&self, mc: &Mc, sums: &[bool]) -> bool {
        let own = (0..5).any(|k| mc.pt_use[k] == PtUse::Sum && self.pt(mc, k));
        own || (mc.cascade_in && sums[mc_idx(mc.mc) - 1])
    }

    fn clock_of(&self, mc: &Mc) -> bool {
        match mc.clock {
            Clock::Pt4 => self.pt(mc, 3),
            Clock::High => true,
            Clock::Global(n) => {
                let (s, inv) = self.model.gclk[n as usize - 1];
                self.sig(s) ^ inv
            }
        }
    }

    fn oe_of(&self, mc: &Mc) -> bool {
        match mc.oe {
            Oe::Never => false,
            Oe::Always => true,
            Oe::Pt5 => self.pt(mc, 4),
            Oe::Global(n) => {
                let (s, inv) = self.model.goe[n as usize - 1];
                self.sig(s) ^ inv
            }
        }
    }

    /// Settle the combinational logic for the current external values and register state.
    fn settle(&mut self) -> bool {
        let m = self.model;
        let n = m.mcs.len();
        for _ in 0..200 {
            // pads from current drive state
            let mut pad = vec![false; n];
            for &(p, k) in &m.pins {
                let e = self.ext.get(&p).copied().unwrap_or(false);
                match k {
                    PinKind::Io(mc) | PinKind::InputOnly(mc) => {
                        let i = mc_idx(mc);
                        pad[i] = match (k, self.drive[i]) {
                            (PinKind::Io(_), Some(v)) => v,
                            _ => e,
                        };
                    }
                    PinKind::Dedicated => {}
                }
            }
            self.pad = pad;
            let fold: Vec<bool> = m.mcs.iter().map(|mc| if mc.pt_use[0] == PtUse::Foldback { !self.pt(mc, 0) } else { true }).collect();
            let mut sums = vec![false; n];
            for i in 0..n {
                sums[i] = self.sum(&m.mcs[i], &sums);
            }
            let mut x = vec![false; n];
            for (i, mc) in m.mcs.iter().enumerate() {
                // XOR (F2.8 row 6, silicon 2026-11-24): P = PT2 if Pt2Mux else 1; CasMux swaps
                // the inputs: A = CasMux ? P : sum, B = Invert & (CasMux ? 1 : P)
                let p = if mc.xor_pt2 { self.pt(mc, 1) } else { true };
                x[i] = if mc.const_one {
                    true
                } else if mc.a_is_one {
                    p ^ mc.invert
                } else {
                    sums[i] ^ (mc.invert && p)
                };
            }
            // level-sensitive storage is part of the combinational fixpoint: asserted async
            // reset/preset force Q, an open latch passes D through
            let mut q = self.q.clone();
            for (i, mc) in m.mcs.iter().enumerate() {
                if !mc.active {
                    continue;
                }
                if let Some(v) = self.async_level(mc) {
                    q[i] = v;
                } else if mc.storage == Storage::Latch && !self.freeze_latches && self.clock_of(mc) {
                    q[i] = self.d_of(mc, i, &x);
                }
            }
            let q_changed = q != self.q;
            self.q = q;
            let fb: Vec<bool> = (0..n).map(|i| if m.mcs[i].fb_comb { x[i] } else { self.q[i] }).collect();
            let mut drive = vec![None; n];
            for (i, mc) in m.mcs.iter().enumerate() {
                if mc.pin.is_none() {
                    continue;
                }
                let o = if mc.out_comb { x[i] } else { self.q[i] };
                let en = self.oe_of(mc);
                drive[i] = if mc.open_drain {
                    if en && !o {
                        Some(false)
                    } else {
                        None
                    }
                } else if en {
                    Some(o)
                } else {
                    None
                };
            }
            let stable = !q_changed && x == self.x && fb == self.fb && fold == self.fold && drive == self.drive;
            self.x = x;
            self.fb = fb;
            self.fold = fold;
            self.drive = drive;
            if stable {
                return true;
            }
        }
        false
    }

    /// Asserted asynchronous reset/preset value of a register, if any.
    fn async_level(&self, mc: &Mc) -> Option<bool> {
        let m = self.model;
        let ar = (mc.ar_pt3 && self.pt(mc, 2)) || (mc.ar_gclr && (self.sig(m.gclr.0) ^ m.gclr.1));
        let ap = mc.ap_pt5 && self.pt(mc, 4);
        match (ar, ap) {
            (true, true) => Some(!m.opts.reset_wins),
            (true, false) => Some(false),
            (false, true) => Some(true),
            _ => None,
        }
    }

    fn d_of(&self, mc: &Mc, i: usize, x: &[bool]) -> bool {
        match mc.d_source {
            DSource::Logic => x[i] ^ (mc.storage == Storage::TFlipFlop && self.q[i]),
            DSource::Pt2 => self.pt(mc, 1),
            DSource::Pad => self.pad[i],
        }
    }

    /// Apply new external pin values (pins not listed keep their previous value).
    ///
    /// Order (docs/architecture/sim/01-model.md §Simulation):
    /// 1. settle with the new inputs while latches hold their value (async controls still act);
    /// 2. every edge-triggered register whose clock rose since the previous step loads the D it
    ///    saw in that state (if its clock enable is 1 and no async control is asserted) — so a
    ///    flip-flop fed by a latch on the same clock gets the latch's value from before the edge,
    ///    as in hardware (clock-to-output delay of the latch);
    /// 3. settle again with latches transparent while enabled.
    pub fn step(&mut self, ext: &BTreeMap<u16, bool>) {
        for (&p, &v) in ext {
            self.ext.insert(p, v);
        }
        self.freeze_latches = true;
        let mut ok = self.settle();
        self.freeze_latches = false;
        let m = self.model;
        let clks: Vec<bool> = m.mcs.iter().map(|mc| self.clock_of(mc)).collect();
        let mut nq = self.q.clone();
        for (i, mc) in m.mcs.iter().enumerate() {
            if !mc.active || mc.storage == Storage::Latch {
                continue;
            }
            let ce = if mc.ce_pt4 { self.pt(mc, 3) } else { true };
            if clks[i] && !self.prev_clk[i] && ce && self.async_level(mc).is_none() {
                nq[i] = self.d_of(mc, i, &self.x);
            }
        }
        self.prev_clk = clks;
        self.q = nq;
        ok &= self.settle();
        self.last_unstable = !ok;
        if !ok {
            self.unstable_steps += 1;
        }
    }

    /// The device's drive on every pin that has a macrocell.
    pub fn outputs(&self) -> BTreeMap<u16, PinOut> {
        let mut o = BTreeMap::new();
        for &(p, k) in &self.model.pins {
            if let PinKind::Io(mc) = k {
                let d = self.drive[mc_idx(mc)];
                o.insert(p, PinOut { oe: d.is_some(), value: d.unwrap_or(false) });
            }
        }
        o
    }

    /// The value seen on a pin (driven value, else the external value).
    pub fn pin_value(&self, pin: u16) -> bool {
        for &(p, k) in &self.model.pins {
            if p == pin {
                if let PinKind::Io(mc) = k {
                    if let Some(v) = self.drive[mc_idx(mc)] {
                        return v;
                    }
                }
            }
        }
        self.ext.get(&pin).copied().unwrap_or(false)
    }
}
