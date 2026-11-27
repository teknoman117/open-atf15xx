//! Placement (spec F4.5, F4.6): pins, pinned cells, node constraints, cascade helpers, logic
//! doubling, input registers and the floating cells, with the fan-in and PT-demand estimates.

use super::*;

/// Placement (spec F4.4–F4.6): inputs and dedicated pins, pinned cells, outputs, logic doubling
/// hosts, fast input registers, buried nodes, and (partitioning placer) their positions. The
/// phases run in the vendor's order; each is a method of [`PlaceState`].
pub(super) fn place_cells(
    dev: &'static Device,
    design: &mut Design,
    opts: &Options,
    fanin_limit: usize,
    extra_pairs: usize,
    avoid: &BTreeSet<(String, u16)>,
    placer: Placer,
) -> Result<Placed, FitError> {
    let mut st = PlaceState::new(dev, design, opts, fanin_limit, avoid);
    let (want_clk, want_clr, want_oe) = st.global_net_candidates(design);
    st.place_pinned_inputs(design)?;
    st.place_pinned_outputs(design)?;
    st.apply_node_constraints(design);
    st.estimate_demand(design)?;
    for n in &want_clk {
        st.grab_dedicated(n, &[Dedicated::Gclk1, Dedicated::Gclk2Oe2]);
    }
    for n in &want_clr {
        st.grab_dedicated(n, &[Dedicated::Gclr]);
    }
    for n in &want_oe {
        st.grab_dedicated(n, &[Dedicated::Oe1, Dedicated::Gclk2Oe2]);
    }
    st.mark_pseudo_inputs();
    st.place_floating_outputs(design)?;
    st.mark_pin_feedback(design);
    st.pair_for_doubling(design, extra_pairs);
    // block assignment for the floating buried cells (spec F9.4), only when the greedy placement
    // alone has failed
    if placer != Placer::Greedy {
        st.assigned = partition_blocks(design, &st.place_of, &st.reserved_mcs, &st.demand, (st.nmc / 16) as u16, avoid, fanin_limit);
    }
    // input registers: a D flip-flop or latch whose D is an input pin goes into that pin's
    // macrocell with D from the pad, before the buried-cell phases hand that macrocell out
    st.pack_input_registers(design);
    st.place_buried_on_unusable_pins(design);
    st.place_floating_inputs(design)?;
    // input registers whose pin was placed only now
    st.pack_input_registers(design);
    st.place_buried_cells(design)?;
    if placer == Placer::Partition {
        st.optimize_positions(design);
    }
    st.finish(design)
}

/// The state the placement phases share: what is placed where, and what is reserved.
struct PlaceState<'a> {
    dev: &'static Device,
    opts: &'a Options,
    fanin_limit: usize,
    avoid: &'a BTreeSet<(String, u16)>,
    nmc: usize,
    keep: bool,
    uses: Vec<McUse>,
    place_of: BTreeMap<String, Place>,
    jtag_pins: Vec<u16>,
    /// JTAG and power-down pins: never user pins.
    reserved_pins: BTreeSet<u16>,
    free_dedicated: Vec<(u16, Dedicated)>,
    taken_pins: BTreeSet<u16>,
    /// Product-term demand per cell (estimate, crate::capacity::pt_demand).
    demand: Vec<usize>,
    /// Macrocells held for cascade helpers.
    reserved_mcs: BTreeSet<u16>,
    /// Macrocells holding a PT-free input register (filled by pack_input_registers).
    share: BTreeSet<u16>,
    /// Logic doubling: register cell -> host output cell.
    host_of: BTreeMap<usize, usize>,
    /// Block assignment of the floating buried cells (partitioning placer only).
    assigned: BTreeMap<String, u16>,
    fast_in: BTreeSet<String>,
}

impl<'a> PlaceState<'a> {
    fn new(dev: &'static Device, design: &Design, opts: &'a Options, fanin_limit: usize, avoid: &'a BTreeSet<(String, u16)>) -> Self {
        let nmc = dev.family.macrocells();
        let jtag_pins: Vec<u16> = if opts.jtag { vec![dev.jtag.tdi, dev.jtag.tms, dev.jtag.tck, dev.jtag.tdo] } else { vec![] };
        let mut reserved_pins: BTreeSet<u16> = jtag_pins.iter().copied().collect();
        if opts.pd1 {
            reserved_pins.insert(dev.pd1);
        }
        if opts.pd2 {
            reserved_pins.insert(dev.pd2);
        }
        PlaceState {
            dev,
            opts,
            fanin_limit,
            avoid,
            nmc,
            keep: opts.preassign != Preassign::Ignore,
            uses: vec![McUse::default(); nmc + 1],
            place_of: BTreeMap::new(),
            jtag_pins,
            reserved_pins,
            free_dedicated: vec![
                (dev.gclk1, Dedicated::Gclk1),
                (dev.oe1, Dedicated::Oe1),
                (dev.gclr, Dedicated::Gclr),
                (dev.gclk2_oe2, Dedicated::Gclk2Oe2),
            ],
            taken_pins: BTreeSet::new(),
            demand: vec![0; design.cells.len()],
            reserved_mcs: BTreeSet::new(),
            share: BTreeSet::new(),
            host_of: BTreeMap::new(),
            assigned: BTreeMap::new(),
            fast_in: BTreeSet::new(),
        }
    }

    /// Input signals that want a dedicated pin: single-literal clocks, resets and output enables.
    fn global_net_candidates(&self, design: &Design) -> (Vec<String>, Vec<String>, Vec<String>) {
        let mut want_clk: Vec<String> = vec![];
        let mut want_clr: Vec<String> = vec![];
        let mut want_oe: Vec<String> = vec![];
        for c in &design.cells {
            if let Some((Signal::Pad(n), _)) = c.clk.as_ref().and_then(|s| s.single_literal()) {
                if design.inputs.contains_key(n) && !want_clk.contains(n) {
                    want_clk.push(n.clone());
                }
            }
            if let Some(ar) = &c.ar {
                for p in &ar.0 {
                    if let [(Signal::Pad(n), _)] = p.as_slice() {
                        if design.inputs.contains_key(n) && !want_clr.contains(n) {
                            want_clr.push(n.clone());
                        }
                    }
                }
            }
            if let Some((Signal::Pad(n), _)) = c.oe.as_ref().and_then(|s| s.single_literal()) {
                if design.inputs.contains_key(n) && !want_oe.contains(n) {
                    want_oe.push(n.clone());
                }
            }
        }
        (want_clk, want_clr, want_oe)
    }

    fn place_pinned_inputs(&mut self, design: &Design) -> Result<(), FitError> {
        let dev = self.dev;
        for name in &design.input_order {
            let Some(pin) = design.inputs[name].filter(|_| self.keep) else { continue };
            let pl = if let Some(func) = dev.dedicated(pin) {
                self.free_dedicated.retain(|x| x.0 != pin);
                Place::Dedicated { pin, func }
            } else if let Some(mc) = dev.pin_mc(pin).or_else(|| dev.input_only_mc(pin)) {
                if self.reserved_pins.contains(&pin) {
                    return err(FitErrorKind::Other, format!("{name}: pin {pin} is reserved (JTAG/power-down)"));
                }
                self.uses[mc as usize].pad_used = true;
                Place::Pin { pin, mc }
            } else {
                return err(FitErrorKind::Other, format!("{name}: pin {pin} is not a user pin of {}", dev.name));
            };
            self.taken_pins.insert(pin);
            self.place_of.insert(name.clone(), pl);
        }
        Ok(())
    }

    fn place_pinned_outputs(&mut self, design: &Design) -> Result<(), FitError> {
        for c in &design.cells {
            if c.is_output {
                if let Some(pin) = c.pin.filter(|_| self.keep) {
                    let Some(mc) = self.dev.pin_mc(pin) else {
                        return err(FitErrorKind::Other, format!("{}: pin {pin} cannot be an output on {}", c.name, self.dev.name));
                    };
                    if self.reserved_pins.contains(&pin) {
                        return err(FitErrorKind::Other, format!("{}: pin {pin} is reserved (JTAG/power-down)", c.name));
                    }
                    self.taken_pins.insert(pin);
                    self.place_of.insert(c.name.clone(), Place::Pin { pin, mc });
                }
            }
        }
        Ok(())
    }

    /// Node constraints, unless the macrocell already drives a pinned output (the vendor then
    /// ignores the constraint).
    fn apply_node_constraints(&mut self, design: &Design) {
        for c in &design.cells {
            if let Some(n) = c.node.filter(|_| self.keep && !c.is_output) {
                if (601..=600 + self.nmc as u16).contains(&n) {
                    let mc = n - 600;
                    if !self.place_of.values().any(|p| matches!(p, Place::Pin { mc: m, .. } | Place::Node { mc: m } if *m == mc)) {
                        self.place_of.insert(c.name.clone(), Place::Node { mc });
                    }
                }
            }
        }
    }

    /// Product-term demand per cell (estimate), and cascade helper reservations below the cells
    /// placed so far.
    fn estimate_demand(&mut self, design: &Design) -> Result<(), FitError> {
        self.demand = design.cells.iter().map(|c| pt_demand(c, design.xor.get(&c.name))).collect();
        // a cell borrows cascade helpers only within its block, at most a chain of MAX_CHAIN_MCS
        // (crate::capacity): placement uses the same estimate, so a larger demand can never be
        // placed (spec F4.3)
        if let Some((i, &n)) = self.demand.iter().enumerate().max_by_key(|(_, &n)| n).filter(|(_, &n)| n > MAX_CHAIN_PTS) {
            return err(
                FitErrorKind::Capacity,
                format!(
                    "{}: needs {n} product terms, more than a cascade chain in one block holds ({MAX_CHAIN_PTS})",
                    design.cells[i].name
                ),
            );
        }
        for (i, c) in design.cells.iter().enumerate() {
            if let Some(&Place::Pin { mc, .. } | &Place::Node { mc }) = self.place_of.get(&c.name) {
                for k in 1..=helpers_for(self.demand[i]) as u16 {
                    if mc > k && (mc - k - 1) / 16 == (mc - 1) / 16 {
                        self.reserved_mcs.insert(mc - k);
                    }
                }
            }
        }
        Ok(())
    }

    /// A floating input wanting a global net takes a free dedicated pin of one of `kinds`.
    fn grab_dedicated(&mut self, name: &str, kinds: &[Dedicated]) {
        if self.place_of.contains_key(name) {
            return;
        }
        if let Some(i) = self.free_dedicated.iter().position(|(_, d)| kinds.contains(d)) {
            let (pin, func) = self.free_dedicated.remove(i);
            self.taken_pins.insert(pin);
            self.place_of.insert(name.to_string(), Place::Dedicated { pin, func });
        }
    }

    /// JTAG / power-down pins occupy their macrocells' pads (the vendor lists them as placed
    /// inputs).
    fn mark_pseudo_inputs(&mut self) {
        let dev = self.dev;
        let mut pseudo_inputs: Vec<u16> = self.jtag_pins.clone();
        if self.opts.pd1 {
            pseudo_inputs.push(dev.pd1);
        }
        if self.opts.pd2 {
            pseudo_inputs.push(dev.pd2);
        }
        for &p in &pseudo_inputs {
            if let Some(mc) = dev.pin_mc(p) {
                self.uses[mc as usize].pad_used = true;
            }
        }
    }

    fn mc_taken(&self, mc: u16) -> bool {
        self.uses[mc as usize].pad_used
            || self.place_of.values().any(|p| matches!(p, Place::Pin { mc: m, .. } | Place::Node { mc: m } if *m == mc))
    }

    /// Floating outputs: lowest block first, highest macrocell of the block first (vendor order).
    fn place_floating_outputs(&mut self, design: &Design) -> Result<(), FitError> {
        for c in &design.cells {
            if c.is_output && !self.place_of.contains_key(&c.name) {
                let d = self.demand[design.cell_index(&c.name).unwrap()];
                let slot = self
                    .dev
                    .io_pins()
                    .map(|(p, m)| ((m - 1) / 16, std::cmp::Reverse(m), p))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .map(|(_, std::cmp::Reverse(m), p)| (m, p))
                    .find(|&(m, p)| {
                        !self.reserved_pins.contains(&p)
                            && !self.taken_pins.contains(&p)
                            && !self.mc_taken(m)
                            && !self.reserved_mcs.contains(&m)
                            && helpers_free(m, d, &self.place_of, &self.reserved_mcs, &self.share)
                    });
                let Some((mc, pin)) = slot else { return err(FitErrorKind::Capacity, format!("no free output pin for {}", c.name)) };
                self.taken_pins.insert(pin);
                reserve_helpers(mc, d, &mut self.reserved_mcs);
                self.place_of.insert(c.name.clone(), Place::Pin { pin, mc });
            }
        }
        Ok(())
    }

    /// `.PIN` feedback marks output pads used.
    fn mark_pin_feedback(&mut self, design: &Design) {
        for c in &design.cells {
            for s in c.inputs() {
                if let Signal::Pad(n) = &s {
                    if let Some(Place::Pin { mc, .. }) = design.cell(n).and(self.place_of.get(n)) {
                        self.uses[*mc as usize].pad_used = true;
                    }
                }
            }
        }
    }

    /// Logic doubling: pair buried registers with combinatorial outputs when macrocells run short.
    fn pair_for_doubling(&mut self, design: &mut Design, extra_pairs: usize) {
        let dev = self.dev;
        let nmc = self.nmc;
        let helpers: usize = self.demand.iter().map(|&d| helpers_for(d)).sum();
        let mut need = (design.cells.len() + helpers + extra_pairs).saturating_sub(nmc);
        if self.opts.logic_doubling && need > 0 {
            need = usize::MAX; // dense design with doubling allowed: pair every eligible register
        }
        if need > 0 && (self.opts.logic_doubling || extra_pairs > 0) {
            let mut hosts_used: BTreeSet<usize> = BTreeSet::new();
            // registers with the most inputs first: they are the hardest to place elsewhere
            let mut order: Vec<usize> = (0..design.cells.len()).collect();
            order.sort_by_key(|&i| std::cmp::Reverse(design.cells[i].inputs().len()));
            for ri in order {
                let r = &design.cells[ri];
                if self.host_of.len() >= need {
                    break;
                }
                if !doubling_register_ok(r) || self.place_of.contains_key(&r.name) && r.node.is_some() {
                    continue;
                }
                // a hosted register is clocked by a global network: its clock must be a clock pin
                let global_clk = match r.clk.as_ref().and_then(|k| k.single_literal()) {
                    Some((Signal::Pad(n), _)) => match self.place_of.get(n) {
                        Some(Place::Dedicated { pin, .. }) | Some(Place::Pin { pin, .. }) => {
                            [dev.gclk1, dev.gclk2_oe2, dev.gclk3].contains(pin)
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if !global_clk {
                    continue;
                }
                let extra = 1
                    + usize::from(r.ar.as_ref().map_or(false, |a| a.0.iter().any(|p| p.len() != 1)))
                    + usize::from(r.ce.is_some() || r.storage == Storage::Tff);
                let (place_of, demand) = (&self.place_of, &self.demand);
                let host = design.cells.iter().enumerate().find(|(yi, y)| {
                    !hosts_used.contains(yi)
                        && y.is_output
                        && y.storage == Storage::Comb
                        && demand[*yi] + extra <= PTS_PER_MC
                        && (y.oe.is_none() || !design.cells.iter().any(|o| o.inputs().contains(&Signal::Fb(y.name.clone(), false))))
                        && place_of.contains_key(&y.name)
                        && fanin_ok(design, place_of, (mc_of(place_of, &y.name) - 1) / 16, r, self.fanin_limit)
                        && !self.avoid.contains(&(r.name.clone(), (mc_of(place_of, &y.name) - 1) / 16))
                });
                if let Some((yi, _)) = host {
                    hosts_used.insert(yi);
                    self.host_of.insert(ri, yi);
                    let mc = mc_of(&self.place_of, &design.cells[yi].name);
                    self.place_of.insert(r.name.clone(), Place::Node { mc });
                }
            }
            // a hosted T flip-flop becomes D = !Q with CE = T (the vendor's conversion), so its D
            // fits the single PT2 of a doubled macrocell
            for &ri in self.host_of.keys() {
                let r = &mut design.cells[ri];
                if r.storage == Storage::Tff {
                    let q = Signal::Fb(r.name.clone(), true);
                    r.ce = Some(std::mem::replace(&mut r.data, Sop(vec![vec![(q, false)]])));
                    r.storage = Storage::Dff;
                }
            }
            // the converted cells' inputs changed: the UIM input lists are rebuilt from here
            uim_memo_clear();
        }
        if std::env::var_os("ATF_FIT_DEBUG").is_some() {
            for (i, c) in design.cells.iter().enumerate() {
                if self.demand[i] > PTS_PER_MC {
                    eprintln!("fit: {} needs {} PTs ({} products)", c.name, self.demand[i], c.data.0.len());
                }
            }
            eprintln!(
                "fit: {} cells, {} cascade helpers, {} MCs, need {} doubled, got {} (fan-in limit {}, extra {extra_pairs})",
                design.cells.len(),
                helpers,
                nmc,
                need,
                self.host_of.len(),
                self.fanin_limit
            );
        }
        for (&ri, &yi) in self.host_of.iter() {
            let mc = mc_of(&self.place_of, &design.cells[yi].name);
            self.place_of.insert(design.cells[ri].name.clone(), Place::Node { mc });
        }
    }

    fn pack_input_registers(&mut self, design: &Design) {
        if self.opts.pack_input_registers {
            pack_input_registers(
                design,
                &mut self.place_of,
                &self.reserved_mcs,
                self.fanin_limit,
                self.avoid,
                &mut self.fast_in,
                &mut self.share,
            );
        }
    }

    /// The partitioner's block for a cell (any block without one).
    fn in_assigned(&self, n: &str, m: u16) -> bool {
        self.assigned.get(n).map_or(true, |&b| b == (m - 1) / 16)
    }

    /// The macrocells holding a cell now.
    fn cell_mcs(&self, design: &Design) -> BTreeSet<u16> {
        design
            .cells
            .iter()
            .filter_map(|x| match self.place_of.get(&x.name) {
                Some(Place::Pin { mc, .. }) | Some(Place::Node { mc }) => Some(*mc),
                _ => None,
            })
            .collect()
    }

    /// Every macrocell, lowest block first, highest macrocell of the block first.
    fn mcs_in_vendor_order(&self) -> Vec<u16> {
        (1..=self.nmc as u16)
            .map(|m| ((m - 1) / 16, std::cmp::Reverse(m)))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|(_, std::cmp::Reverse(m))| m)
            .collect()
    }

    /// Floating buried cells, phase 1: lowest block first, highest macrocell whose pin is not
    /// usable (unbonded, or reserved for JTAG / power-down).
    fn place_buried_on_unusable_pins(&mut self, design: &Design) {
        let dev = self.dev;
        let order = self.mcs_in_vendor_order();
        for c in &design.cells {
            if c.is_output || self.place_of.contains_key(&c.name) {
                continue;
            }
            let cell_mcs = self.cell_mcs(design);
            let d = self.demand[design.cell_index(&c.name).unwrap()];
            let pick = order.iter().copied().find(|&m| {
                dev.mc_pin(m).map_or(true, |p| self.reserved_pins.contains(&p))
                    && !cell_mcs.contains(&m)
                    && !self.reserved_mcs.contains(&m)
                    && helpers_free(m, d, &self.place_of, &self.reserved_mcs, &self.share)
                    && fanin_ok(design, &self.place_of, (m - 1) / 16, c, self.fanin_limit)
                    && !self.avoid.contains(&(c.name.clone(), (m - 1) / 16))
                    && self.in_assigned(&c.name, m)
            });
            if let Some(mc) = pick {
                reserve_helpers(mc, d, &mut self.reserved_mcs);
                self.place_of.insert(c.name.clone(), Place::Node { mc });
            }
        }
    }

    /// Floating inputs: visit cells block by block (outputs before nodes), place each unplaced
    /// input at the lowest free bonded macrocell, in order of first use (vendor order).
    fn place_floating_inputs(&mut self, design: &Design) -> Result<(), FitError> {
        let dev = self.dev;
        let mut visit: Vec<(u16, bool, usize)> = design
            .cells
            .iter()
            .enumerate()
            .map(|(i, c)| (self.place_of.get(&c.name).map_or(99, |_| (mc_of(&self.place_of, &c.name) - 1) / 16), !c.is_output, i))
            .collect();
        visit.sort();
        let mut order: Vec<String> = vec![];
        for (_, _, i) in visit {
            let c = &design.cells[i];
            let mut sigs: Vec<&Signal> = vec![];
            for s in [Some(&c.data), c.clk.as_ref(), c.ce.as_ref(), c.ar.as_ref(), c.ap.as_ref(), c.oe.as_ref()].into_iter().flatten() {
                for p in &s.0 {
                    for (sig, _) in p {
                        sigs.push(sig);
                    }
                }
            }
            for sig in sigs {
                if let Signal::Pad(n) = sig {
                    if design.inputs.contains_key(n) && !order.contains(n) {
                        order.push(n.clone());
                    }
                }
            }
        }
        for name in order.iter().chain(design.input_order.iter()) {
            if self.place_of.contains_key(name) {
                continue;
            }
            let cell_mcs = self.cell_mcs(design);
            let slot = dev.io_pins().map(|(p, m)| (m, p)).collect::<BTreeMap<_, _>>().into_iter().find(|&(m, p)| {
                !self.reserved_pins.contains(&p)
                    && !self.taken_pins.contains(&p)
                    && !self.uses[m as usize].pad_used
                    && !cell_mcs.contains(&m)
            });
            let Some((mc, pin)) = slot else {
                // all I/O pins taken: an unused dedicated input pin also reaches every UIM
                let ded = [dev.gclk1, dev.oe1, dev.gclr, dev.gclk2_oe2].into_iter().find(|p| {
                    !self.taken_pins.contains(p)
                        && !self.reserved_pins.contains(p)
                        && !self.place_of.values().any(|x| matches!(x, Place::Dedicated { pin, .. } if pin == p))
                });
                match ded.and_then(|p| dev.dedicated(p).map(|f| (p, f))) {
                    Some((p, func)) => {
                        self.taken_pins.insert(p);
                        self.place_of.insert(name.clone(), Place::Dedicated { pin: p, func });
                        continue;
                    }
                    None => return err(FitErrorKind::InputPins, format!("no free input pin for {name}")),
                }
            };
            self.taken_pins.insert(pin);
            self.uses[mc as usize].pad_used = true;
            self.place_of.insert(name.clone(), Place::Pin { pin, mc });
        }
        Ok(())
    }

    /// Floating buried cells, phase 2: macrocells whose pad is an input, then any free one.
    fn place_buried_cells(&mut self, design: &Design) -> Result<(), FitError> {
        let order = self.mcs_in_vendor_order();
        for c in &design.cells {
            if c.is_output || self.place_of.contains_key(&c.name) {
                continue;
            }
            let cell_mcs = self.cell_mcs(design);
            let d = self.demand[design.cell_index(&c.name).unwrap()];
            let ok = |m: u16| {
                !self.avoid.contains(&(c.name.clone(), (m - 1) / 16))
                    && !cell_mcs.contains(&m)
                    && !self.reserved_mcs.contains(&m)
                    && helpers_free(m, d, &self.place_of, &self.reserved_mcs, &self.share)
                    && fanin_ok(design, &self.place_of, (m - 1) / 16, c, self.fanin_limit)
            };
            let pad_used = |m: u16| self.uses[m as usize].pad_used;
            let pick = order
                .iter()
                .copied()
                .find(|&m| pad_used(m) && ok(m) && self.in_assigned(&c.name, m))
                .or_else(|| order.iter().copied().find(|&m| ok(m) && self.in_assigned(&c.name, m)))
                .or_else(|| order.iter().copied().find(|&m| pad_used(m) && ok(m)))
                .or_else(|| order.iter().copied().find(|&m| ok(m)))
                .or_else(|| {
                    // over budget everywhere: the free macrocell whose block grows least
                    let mut best: Option<(usize, u16)> = None;
                    // (the avoid list from UIM repairs is a preference: a full device ignores it)
                    for &m in &order {
                        if cell_mcs.contains(&m)
                            || self.reserved_mcs.contains(&m)
                            || !helpers_free(m, d, &self.place_of, &self.reserved_mcs, &self.share)
                        {
                            continue;
                        }
                        let lim = fanin_count(design, &self.place_of, (m - 1) / 16, c);
                        if best.map_or(true, |(l, _)| lim < l) {
                            best = Some((lim, m));
                        }
                    }
                    best.map(|x| x.1)
                });
            if pick.is_none() && std::env::var_os("ATF_FIT_DEBUG").is_some() {
                for &m in &order {
                    eprintln!(
                        "node {}: mc {m}: cell {} reserved {} helpers_free {} avoid {} pad_used {}",
                        c.name,
                        cell_mcs.contains(&m),
                        self.reserved_mcs.contains(&m),
                        helpers_free(m, d, &self.place_of, &self.reserved_mcs, &self.share),
                        self.avoid.contains(&(c.name.clone(), (m - 1) / 16)),
                        pad_used(m)
                    );
                }
            }
            let Some(mc) = pick else { return err(FitErrorKind::Capacity, format!("no free macrocell for node {}", c.name)) };
            reserve_helpers(mc, d, &mut self.reserved_mcs);
            self.place_of.insert(c.name.clone(), Place::Node { mc });
        }
        Ok(())
    }

    /// UIM-aware positions (failure path only): which muxes can select a buried cell's feedback
    /// depends on its macrocell, so move/swap buried cells within their blocks until every
    /// block's sources can be matched onto its 40 muxes.
    fn optimize_positions(&mut self, design: &Design) {
        let movable: Vec<String> = design
            .cells
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                matches!(self.place_of.get(&c.name), Some(Place::Node { .. }))
                    && !self.host_of.contains_key(i)
                    && !self.fast_in.contains(&c.name)
                    && self.demand[*i] <= PTS_PER_MC
                    && c.node.map_or(true, |n| n == 0)
            })
            .map(|(_, c)| c.name.clone())
            .collect();
        optimize_positions(design, &mut self.place_of, &movable, &self.reserved_mcs, self.dev.family);
    }

    /// The macrocell uses from the placement.
    fn finish(mut self, design: &Design) -> Result<Placed, FitError> {
        for (i, c) in design.cells.iter().enumerate() {
            let mc = match self.place_of[&c.name] {
                Place::Pin { mc, .. } | Place::Node { mc } => mc,
                Place::Dedicated { .. } => unreachable!(),
            };
            if self.host_of.contains_key(&i) {
                self.uses[mc as usize].hosted = Some(i);
                continue;
            }
            if let Some(j) = self.uses[mc as usize].cell {
                return err(FitErrorKind::Other, format!("{} and {} both placed on MC{mc}", design.cells[j].name, c.name));
            }
            self.uses[mc as usize].cell = Some(i);
        }
        Ok(Placed { uses: self.uses, place_of: self.place_of, host_of: self.host_of, fast_in: self.fast_in })
    }
}

/// Place every eligible register into the macrocell of the input pin that is its D (see
/// `Options::pack_input_registers`); records them in `fast_in`.
pub(super) fn pack_input_registers(
    design: &Design,
    place_of: &mut BTreeMap<String, Place>,
    reserved: &BTreeSet<u16>,
    fanin_limit: usize,
    avoid: &BTreeSet<(String, u16)>,
    fast_in: &mut BTreeSet<String>,
    share: &mut BTreeSet<u16>,
) {
    for c in &design.cells {
        if c.is_output || place_of.contains_key(&c.name) || !matches!(c.storage, Storage::Dff | Storage::Latch) || c.active_low {
            continue;
        }
        let Some((Signal::Pad(p), true)) = c.data.single_literal() else { continue };
        let Some(&Place::Pin { mc, .. }) = place_of.get(p) else { continue };
        let taken = design
            .cells
            .iter()
            .any(|o| matches!(place_of.get(&o.name), Some(Place::Pin { mc: m, .. } | Place::Node { mc: m }) if *m == mc));
        // a macrocell reserved as a cascade helper still takes a PT-free input register
        if !design.inputs.contains_key(p) || taken || (reserved.contains(&mc) && !pt_free_input_register(c)) {
            continue;
        }
        if !fanin_ok(design, place_of, (mc - 1) / 16, c, fanin_limit) || avoid.contains(&(c.name.clone(), (mc - 1) / 16)) {
            continue;
        }
        place_of.insert(c.name.clone(), Place::Node { mc });
        fast_in.insert(c.name.clone());
        if pt_free_input_register(c) {
            share.insert(mc);
        }
    }
}

pub(super) fn helpers_free(
    mc: u16,
    demand: usize,
    place_of: &BTreeMap<String, Place>,
    reserved: &BTreeSet<u16>,
    share: &BTreeSet<u16>,
) -> bool {
    let k = helpers_for(demand) as u16;
    (1..=k).all(|j| {
        mc > j
            && (mc - j - 1) / 16 == (mc - 1) / 16
            && !reserved.contains(&(mc - j))
            && (share.contains(&(mc - j))
                || !place_of.values().any(|p| matches!(p, Place::Pin { mc: m, .. } | Place::Node { mc: m } if *m == mc - j)))
    })
}

/// An input register that will need no product term: D from the pad (fast path), clocked by a
/// clock pin, no clock enable or preset, reset at most from a pin (GCLR). Its macrocell's PTs,
/// sum and cascade output stay free for a cascade helper or a foldback.
pub(super) fn pt_free_input_register(c: &Cell) -> bool {
    let pin_literal = |s: &Option<Sop>| s.as_ref().map_or(true, |s| matches!(s.single_literal(), Some((Signal::Pad(_), _))));
    c.clk.is_some() && pin_literal(&c.clk) && pin_literal(&c.ar) && c.ce.is_none() && c.ap.is_none()
}

pub(super) fn reserve_helpers(mc: u16, demand: usize, reserved: &mut BTreeSet<u16>) {
    for j in 1..=helpers_for(demand) as u16 {
        reserved.insert(mc - j);
    }
}

/// Would placing `c` in block `b` keep the block's distinct inputs within `limit`?
pub(super) fn fanin_ok(design: &Design, place_of: &BTreeMap<String, Place>, b: u16, c: &Cell, limit: usize) -> bool {
    fanin_count(design, place_of, b, c) <= limit
}

/// Distinct UIM inputs block `b` would need with `c` added to the cells placed there.
pub(super) fn fanin_count(design: &Design, place_of: &BTreeMap<String, Place>, b: u16, c: &Cell) -> usize {
    // the union of interned input ids in a reusable bitset (spec F9.11: this check runs for
    // every candidate macrocell, and rebuilding string sets dominated the fitter's profile)
    let mut ids: Vec<std::rc::Rc<[u32]>> = vec![uim_ids(design, c)];
    for o in &design.cells {
        if let Some(Place::Pin { mc, .. } | Place::Node { mc }) = place_of.get(&o.name) {
            if (mc - 1) / 16 == b {
                ids.push(uim_ids(design, o));
            }
        }
    }
    UIM_MEMO.with(|m| {
        let mut m = m.borrow_mut();
        let n = m.intern.len();
        let bits = &mut m.bits;
        bits.clear();
        bits.resize(n / 64 + 1, 0);
        let mut count = 0;
        for l in &ids {
            for &x in l.iter() {
                let (w, k) = (x as usize / 64, x % 64);
                if bits[w] >> k & 1 == 0 {
                    bits[w] |= 1 << k;
                    count += 1;
                }
            }
        }
        count
    })
}

// Memoised UIM inputs as interned ids (spec F9.11), per cell name, cleared at every fit attempt
// (the netlist's foldbacks can change between designs) and after a hosted T flip-flop becomes
// D + CE during placement (the only change to cells within an attempt).
#[derive(Default)]
pub(super) struct UimMemo {
    /// per cell name; valid while the cells do not change (cleared at every attempt and after
    /// placement's one change to cells, the hosted T flip-flops' conversion)
    lists: rustc_hash::FxHashMap<String, std::rc::Rc<[u32]>>,
    intern: rustc_hash::FxHashMap<Signal, u32>,
    bits: Vec<u64>,
}
thread_local! {
    pub(super) static UIM_MEMO: std::cell::RefCell<UimMemo> = std::cell::RefCell::new(UimMemo::default());
}

pub(super) fn uim_memo_clear() {
    UIM_MEMO.with(|m| {
        let mut m = m.borrow_mut();
        m.lists.clear();
        m.intern.clear();
    });
}

pub(super) fn uim_ids(design: &Design, c: &Cell) -> std::rc::Rc<[u32]> {
    if let Some(l) = UIM_MEMO.with(|m| m.borrow().lists.get(c.name.as_str()).cloned()) {
        return l;
    }
    let sigs = uim_inputs(design, c);
    UIM_MEMO.with(|m| {
        let mut m = m.borrow_mut();
        let ids: Vec<u32> = sigs
            .into_iter()
            .map(|s| {
                let n = m.intern.len() as u32;
                *m.intern.entry(s).or_insert(n)
            })
            .collect();
        let l: std::rc::Rc<[u32]> = ids.into();
        m.lists.insert(c.name.clone(), l.clone());
        l
    })
}

/// Signals a cell needs on its block's UIM: its inputs, with netlist-level foldbacks replaced by
/// the signals of their products (a foldback is hosted in the reading block, its product reads
/// the UIM there).
pub(super) fn uim_inputs(design: &Design, c: &Cell) -> Vec<Signal> {
    let mut out: BTreeSet<Signal> = BTreeSet::new();
    let mut stack: Vec<Signal> = c.inputs();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    while let Some(s) = stack.pop() {
        match &s {
            Signal::Fold(k) => {
                if seen.insert(k.clone()) {
                    if let Some(p) = design.folds.get(k) {
                        stack.extend(p.iter().map(|(x, _)| x.clone()));
                    }
                }
            }
            _ => {
                out.insert(s);
            }
        }
    }
    out.into_iter().collect()
}
