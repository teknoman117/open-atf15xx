//! Product-term plans (spec F4.3): sums, special PTs, foldback factoring, cascade chains, logic
//! doubling.

use super::*;

/// Product-term plans (spec F4.3): every cell's sum, polarity, special PTs, global-net uses and
/// cascade helpers, and the table of every foldback product they use.
pub(super) fn plan_pts(
    dev: &'static Device,
    design: &Design,
    opts: &Options,
    placed: &mut Placed,
    nets: &mut Nets,
) -> Result<(Vec<PtPlan>, FoldTable), FitError> {
    let Placed { uses, place_of, host_of, fast_in, .. } = placed;
    let Nets { gclk_net, gclr_inv, goe } = nets;
    let mut p = Planner {
        dev,
        design,
        opts,
        place_of,
        gclk_net,
        gclr_inv,
        goe,
        // the foldback products, by name: those introduced at netlist level
        // (Design::nodes_to_foldbacks), then every one the planning below creates
        folds: design.folds.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    };
    let mut plans: Vec<PtPlan> = vec![];
    for (i, c) in design.cells.iter().enumerate() {
        let fast = fast_in.contains(&c.name);
        let hosting = host_of.values().any(|&y| y == i);
        plans.push(p.plan_cell(c, fast, hosting)?);
    }
    double_logic(design, host_of, &mut plans)?;
    reserve_cascade_helpers(design, place_of, uses, &plans)?;
    Ok((plans, p.folds))
}

/// What planning a cell reads and the global state it assigns: the GCLR polarity, the GOE
/// networks and the foldback table.
struct Planner<'a> {
    dev: &'static Device,
    design: &'a Design,
    opts: &'a Options,
    place_of: &'a BTreeMap<String, Place>,
    gclk_net: &'a [Option<(u8, bool)>; 3],
    gclr_inv: &'a mut Option<bool>,
    goe: &'a mut BTreeMap<u8, (Source, bool)>,
    folds: FoldTable,
}

/// A cover as product lists.
type Cover = Vec<Vec<(Signal, bool)>>;

impl Planner<'_> {
    /// The global clock network a single-literal clock `n` (polarity `pos`) can use.
    fn clock_net(&self, n: &str, pos: bool) -> Option<u8> {
        let k = placed_pin(self.place_of, n).and_then(|pin| gclk_net_of_pin(self.dev, pin))?;
        (0..3u8).find(|&i| self.gclk_net[i as usize] == Some((k, pos))).map(|i| i + 1)
    }

    fn plan_cell(&mut self, c: &Cell, fast_in: bool, hosting: bool) -> Result<PtPlan, FitError> {
        let mut pl = PtPlan::default();
        // the polarity choice depends on the cell alone: memoised per thread (every attempt of
        // every strategy plans the same cells again)
        let pol = polarity_memo(c, || {
            let mut p = PtPlan::default();
            self.choose_polarity(c, &mut p).map(|(as_is, flipped)| Polarity {
                as_is,
                flipped,
                sum: p.sum,
                invert: p.invert,
                constant: p.constant,
            })
        })?;
        pl.sum = pol.sum.clone();
        pl.invert = pol.invert;
        pl.constant = pol.constant;
        let (as_is, flipped) = (&pol.as_is, &pol.flipped);
        if fast_in {
            // D comes from the pad through the D/Q fast path: no data product at all
            pl.fast_in = true;
            pl.sum.clear();
            pl.invert = false;
            pl.constant = None;
        } else if let Some(x) = self.design.xor.get(&c.name).filter(|_| pl.constant.is_none() && !hosting) {
            // XOR mode: sum = g, PT2 = p, x = g ⊕ p; the Invert fuse is the XOR's B enable, so
            // the polarity comes from the cover (`!f = !g ⊕ p`)
            let g = if !c.active_low { &x.g } else { &x.gbar };
            if 1 + g.0.len() <= pl.sum.len() && !neg_fold(&g.0) {
                pl.sum = g.0.clone();
                pl.invert = true;
                pl.xor = true;
                pl.pts.insert(2, x.p.clone());
            }
        }
        self.plan_clock(c, &mut pl)?;
        self.plan_reset(c, &mut pl)?;
        if let Some(ap) = &c.ap {
            pl.ap_pt = true;
            pl.pts.insert(5, one_product(ap, &c.name, "asynchronous preset", &mut self.folds)?);
        }
        pl.oe = self.plan_oe(c, &mut pl)?;
        self.plan_sum(c, &mut pl, as_is, flipped)?;
        Ok(pl)
    }

    /// The data cover in both polarities, and the cheaper one (or a constant) into `pl`.
    fn choose_polarity(&self, c: &Cell, pl: &mut PtPlan) -> Result<(Cover, Cover), FitError> {
        let target_given = !c.active_low;
        let as_is: Cover = if target_given { c.data.0.clone() } else { c.data.complement().0 };
        let flipped: Cover =
            // (a complement too large to compute is never the cheaper polarity: the 99-product
            // placeholder, as for a single product, is never chosen)
            if target_given {
                match (c.data.0.len() > 1).then(|| c.data.complement_within(COMPLEMENT_CHOICE_CUBES)).flatten() {
                    Some(comp) => comp.0,
                    None => vec![vec![]; 99],
                }
            } else {
                c.data.0.clone()
            };
        // a foldback line exists in one polarity only: a cover with a negated foldback literal
        // cannot be built (complementing a function of foldbacks can create one)
        let (as_is, flipped) = if neg_fold(&as_is) && !neg_fold(&flipped) && flipped.len() < 99 {
            (as_is, flipped)
        } else if neg_fold(&flipped) {
            (as_is, vec![vec![]; 99]) // never choose the flipped cover
        } else {
            (as_is, flipped)
        };
        if neg_fold(&as_is) && (flipped.len() >= 99 || neg_fold(&flipped)) {
            return err(FitErrorKind::Netlist, format!("{}: needs an inverted foldback line", c.name));
        }
        if as_is.is_empty() || Sop(as_is.clone()).is_one() {
            pl.constant = Some(!as_is.is_empty());
        } else if (flipped.len() < as_is.len() || neg_fold(&as_is)) && !Sop(flipped.clone()).is_one() && !flipped.is_empty() {
            pl.sum = flipped.clone();
            pl.invert = true;
        } else {
            pl.sum = as_is.clone();
        }
        Ok((as_is, flipped))
    }

    /// Clock (a global network, else PT4) and clock enable (PT4).
    fn plan_clock(&mut self, c: &Cell, pl: &mut PtPlan) -> Result<(), FitError> {
        if let Some(clk) = &c.clk {
            match clk.single_literal().and_then(|(s, pos)| if let Signal::Pad(n) = s { self.clock_net(&n, pos) } else { None }) {
                Some(net) => pl.clk = ClkUse::Global(net),
                None => {
                    pl.clk = ClkUse::Pt;
                    pl.pts.insert(4, one_product(clk, &c.name, "clock", &mut self.folds)?);
                }
            }
        }
        if let Some(ce) = &c.ce {
            if pl.clk == ClkUse::Pt {
                return err(FitErrorKind::Other, format!("{}: clock enable needs a global clock", c.name));
            }
            pl.ce_pt = true;
            pl.pts.insert(4, one_product(ce, &c.name, "clock enable", &mut self.folds)?);
        }
        Ok(())
    }

    /// Asynchronous reset: the GCLR network where its polarity allows, else PT3.
    fn plan_reset(&mut self, c: &Cell, pl: &mut PtPlan) -> Result<(), FitError> {
        let Some(ar) = &c.ar else { return Ok(()) };
        let gclr = self.dev.gclr;
        let mut rest = vec![];
        for p in &ar.0 {
            match p.as_slice() {
                // the GCLR network has one polarity for the whole device: the first user fixes
                // it, the other polarity falls back to a PT reset (as the vendor does)
                [(Signal::Pad(n), pos)]
                    if placed_pin(self.place_of, n) == Some(gclr) && !pl.ar_global && self.gclr_inv.map_or(true, |inv| inv == !pos) =>
                {
                    pl.ar_global = true;
                    *self.gclr_inv = Some(!pos);
                }
                _ => rest.push(p.clone()),
            }
        }
        match rest.len() {
            0 => {}
            1 => {
                pl.ar_pt = true;
                pl.pts.insert(3, rest.pop().unwrap());
            }
            // several products: one foldback (complement is one product) or two levels
            _ => {
                pl.ar_pt = true;
                pl.pts.insert(3, one_product(&Sop(rest), &c.name, "asynchronous reset", &mut self.folds)?);
            }
        }
        Ok(())
    }

    /// Output enable: a GOE network, else PT5 (shared with the preset).
    fn plan_oe(&mut self, c: &Cell, pl: &mut PtPlan) -> Result<OeUse, FitError> {
        let f = self.dev.family;
        if !c.is_output {
            return Ok(OeUse::Off);
        }
        let Some(oe) = &c.oe else { return Ok(OeUse::Always) };
        Ok(match oe.single_literal() {
            Some((s, pos)) => {
                let src = signal_source(self.design, self.place_of, s)?;
                let goe = &mut *self.goe;
                let n = (1..=6u8)
                    .find(|&n| goe.get(&n).map_or(false, |x| *x == (src, !pos)))
                    .or_else(|| (1..=6u8).find(|&n| !goe.contains_key(&n) && f.goe_choices(n as usize).contains(&src)));
                match n {
                    Some(n) => {
                        goe.insert(n, (src, !pos));
                        OeUse::Global(n)
                    }
                    None if !pl.ap_pt => {
                        pl.pts.insert(5, one_product(oe, &c.name, "output enable", &mut self.folds)?);
                        OeUse::Pt
                    }
                    None => return err(FitErrorKind::Other, format!("{}: no global OE reaches its enable signal", c.name)),
                }
            }
            None if oe.is_one() => OeUse::Always,
            None if pl.ap_pt => return err(FitErrorKind::Other, format!("{}: PT5 needed for both preset and OE", c.name)),
            None => {
                pl.pts.insert(5, one_product(oe, &c.name, "output enable", &mut self.folds)?);
                OeUse::Pt
            }
        })
    }

    /// The sum into the free PTs: foldback factoring when it does not fit, then the cascade
    /// chain for the rest (crate::capacity).
    fn plan_sum(&mut self, c: &Cell, pl: &mut PtPlan, as_is: &Cover, flipped: &Cover) -> Result<(), FitError> {
        // sum terms into the free PTs; if they do not fit, factor a common literal out of products
        // whose remainders are single literals and use a foldback expander for the remainder
        // (`L&a # L&b # L&!c` = `L & !(!a & !b & c)`), as the vendor does
        let free: Vec<usize> = (1..=5).filter(|k| !pl.pts.contains_key(k)).collect();
        if self.opts.foldback && pl.sum.len() > free.len() && pl.constant.is_none() {
            // a function of the cell and its free PTs only: memoised per thread with the
            // foldbacks it creates, which are replayed into this attempt's table
            let key = (c.data.clone(), c.active_low, pl.xor, pl.invert, free.len());
            if let Some(hit) = FACTORED.with(|m| m.borrow().get(&key).cloned()) {
                let (sum, invert, created) = &*hit;
                pl.sum = sum.clone();
                pl.invert = *invert;
                for (k, f) in created {
                    self.folds.insert(k.clone(), f.clone());
                }
            } else {
                let mut created: Vec<(String, Vec<(Signal, bool)>)> = vec![];
                self.factor_sum(pl, as_is, flipped, &free, &mut created);
                for (k, f) in &created {
                    self.folds.insert(k.clone(), f.clone());
                }
                FACTORED.with(|m| {
                    let mut m = m.borrow_mut();
                    if m.len() > 1 << 14 {
                        m.clear();
                    }
                    m.insert(key, std::rc::Rc::new((pl.sum.clone(), pl.invert, created)));
                });
            }
        }
        let n = pl.sum.len();
        if n <= free.len() {
            for (i, p) in pl.sum.clone().into_iter().enumerate() {
                pl.pts.insert(free[i], p);
                pl.in_sum.insert(free[i]);
            }
            return Ok(());
        }
        // a cascade chain, laid out as the vendor does (`tools/fuzzers/fitter-cascade`): the
        // products in the order 1, 2, ..., n-1, 0 fill h helpers completely, the deepest (MC-h)
        // first, and the cell's own PTs take the rest
        let h = (n - free.len()).div_ceil(PTS_PER_MC);
        if h > MAX_CHAIN_MCS - 1 || !self.opts.cascade {
            return err(FitErrorKind::Capacity, format!("{}: {} product terms do not fit", c.name, n));
        }
        let mut seq = pl.sum.clone();
        seq.rotate_left(1);
        let in_helpers = (h * PTS_PER_MC).min(n);
        for (q, p) in seq.into_iter().enumerate() {
            if q < in_helpers {
                // helper (counted from the cell, MC-1) and slot of the q-th product
                let helper = h - q / PTS_PER_MC;
                pl.helper_pts.insert((helper - 1) * PTS_PER_MC + q % PTS_PER_MC + 1, p);
            } else {
                let i = q - in_helpers;
                pl.pts.insert(free[i], p);
                pl.in_sum.insert(free[i]);
            }
        }
        Ok(())
    }

    /// Foldback factoring of `pl.sum` down to the free PTs (and of the other polarity if that
    /// factors better); the foldbacks it creates go to `created`.
    fn factor_sum(
        &self,
        pl: &mut PtPlan,
        as_is: &Cover,
        flipped: &Cover,
        free: &[usize],
        created: &mut Vec<(String, Vec<(Signal, bool)>)>,
    ) {
        let mut factor = |mut s: Cover| {
            while s.len() > free.len() {
                match factor_with_foldback(&s) {
                    Some((x, key, fold)) => {
                        created.push((key, fold));
                        s = x
                    }
                    None => break,
                }
            }
            s
        };
        // factor the chosen polarity; if it still does not fit, the other polarity may
        // factor better (`L&a&(b+c+!d) + ...` has a larger cover but a common cube)
        let mine = factor(pl.sum.clone());
        let other_cover = if pl.invert { as_is.clone() } else { flipped.clone() };
        if !pl.xor && mine.len() > free.len() && !other_cover.is_empty() && other_cover.len() < 99 && !Sop(other_cover.clone()).is_one() {
            let other = factor(other_cover);
            if other.len() < mine.len() {
                pl.sum = other;
                pl.invert = !pl.invert;
            } else {
                pl.sum = mine;
            }
        } else {
            pl.sum = mine;
        }
    }
}

/// A cell's two covers and the polarity planning chooses (`Planner::choose_polarity`).
struct Polarity {
    as_is: Cover,
    flipped: Cover,
    sum: Cover,
    invert: bool,
    constant: Option<bool>,
}

/// One factoring result: the sum, its polarity, and the foldbacks it created.
type Factored = std::rc::Rc<(Cover, bool, Vec<(String, Vec<(Signal, bool)>)>)>;

thread_local! {
    static POLARITY: std::cell::RefCell<rustc_hash::FxHashMap<(Sop, bool), std::rc::Rc<Polarity>>> = Default::default();
    static FACTORED: std::cell::RefCell<rustc_hash::FxHashMap<(Sop, bool, bool, bool, usize), Factored>> = Default::default();
}

/// [`Planner::choose_polarity`] memoised per thread by the cell's cover and polarity (failures,
/// which end the attempt, are not kept).
fn polarity_memo(c: &Cell, f: impl FnOnce() -> Result<Polarity, FitError>) -> Result<std::rc::Rc<Polarity>, FitError> {
    let key = (c.data.clone(), c.active_low);
    if let Some(p) = POLARITY.with(|m| m.borrow().get(&key).cloned()) {
        return Ok(p);
    }
    let p = std::rc::Rc::new(f()?);
    POLARITY.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 1 << 14 {
            m.clear();
        }
        m.insert(key, p.clone());
    });
    Ok(p)
}

/// A cover with a negated foldback literal: foldback lines exist in one polarity only.
fn neg_fold(v: &Cover) -> bool {
    v.iter().flatten().any(|(s, p)| !p && matches!(s, Signal::Fold(_)))
}

/// Logic doubling: move each hosted register's D (PT2), reset (PT3) and CE (PT4) into its host.
fn double_logic(design: &Design, host_of: &BTreeMap<usize, usize>, plans: &mut [PtPlan]) -> Result<(), FitError> {
    for (&ri, &yi) in host_of.iter() {
        let rp = plans[ri].clone();
        let refuse =
            || err(FitErrorKind::Other, format!("{}: cannot share a macrocell with {}", design.cells[ri].name, design.cells[yi].name));
        if rp.invert || rp.sum.len() != 1 || rp.constant.is_some() || !matches!(rp.clk, ClkUse::Global(_)) {
            return refuse();
        }
        let yp = &mut plans[yi];
        let mut reserved: BTreeSet<usize> = [2].into_iter().collect();
        if rp.ar_pt {
            reserved.insert(3);
        }
        if rp.ce_pt {
            reserved.insert(4);
        }
        let specials: BTreeMap<usize, Vec<(Signal, bool)>> =
            yp.pts.iter().filter(|(k, _)| !yp.in_sum.contains(k)).map(|(k, v)| (*k, v.clone())).collect();
        if specials.keys().any(|k| reserved.contains(k)) || !yp.helper_pts.is_empty() {
            return refuse();
        }
        let free: Vec<usize> = (1..=5).filter(|k| !specials.contains_key(k) && !reserved.contains(k)).collect();
        if yp.sum.len() > free.len() {
            return refuse();
        }
        yp.pts = specials;
        yp.in_sum.clear();
        for (k, p) in free.iter().zip(yp.sum.clone()) {
            yp.pts.insert(*k, p);
            yp.in_sum.insert(*k);
        }
        yp.pts.insert(2, rp.sum[0].clone());
        if rp.ar_pt {
            yp.pts.insert(3, rp.pts[&3].clone());
        }
        if rp.ce_pt {
            yp.pts.insert(4, rp.pts[&4].clone());
        }
        yp.hosted = Some(rp.clone().into());
    }
    Ok(())
}

/// Every cell with helper PTs takes the chain MC−1, MC−2, … below it (crate::capacity), all in
/// its block and free (or a PT-free input register).
fn reserve_cascade_helpers(
    design: &Design,
    place_of: &BTreeMap<String, Place>,
    uses: &mut [McUse],
    plans: &[PtPlan],
) -> Result<(), FitError> {
    for (i, c) in design.cells.iter().enumerate() {
        if plans[i].helper_pts.is_empty() {
            continue;
        }
        let mc = mc_of(place_of, &c.name);
        // a PT-free input register (D from the pad) leaves its macrocell's PTs and sum free
        let shareable = |j: usize| plans[j].fast_in && plans[j].pts.is_empty() && !plans[j].fold_host;
        let n_helpers = helper_slot(*plans[i].helper_pts.keys().max().expect("helper PTs")).0 as u16;
        for h in 1..=n_helpers {
            let m = mc.wrapping_sub(h);
            if mc <= h
                || (m - 1) / 16 != (mc - 1) / 16
                || uses[m as usize].cell.map_or(false, |j| !shareable(j))
                || uses[m as usize].helper_for.is_some()
            {
                return err(FitErrorKind::Cascade, format!("{}: needs cascade from MC{} which is not free", c.name, m));
            }
            uses[m as usize].helper_for = Some(mc);
        }
    }
    Ok(())
}

/// One factoring step: find the literal shared by the most products whose remainders are single
/// literals; replace those products by `literal & FOLD(product of the negated remainders)`.
/// Returns the new sum, and the foldback it uses (name, product) for the caller's [`FoldTable`].
pub(crate) fn factor_with_foldback(sum: &[Vec<(Signal, bool)>]) -> Option<(Vec<Vec<(Signal, bool)>>, String, Vec<(Signal, bool)>)> {
    // a pure function of the cover, asked again for the same covers in every attempt of every
    // strategy: memoised per thread (like the logic module's complement and minimisation)
    type Step = Option<(Vec<Vec<(Signal, bool)>>, String, Vec<(Signal, bool)>)>;
    thread_local! {
        static MEMO: std::cell::RefCell<rustc_hash::FxHashMap<Vec<Vec<(Signal, bool)>>, Step>> = Default::default();
    }
    if let Some(r) = MEMO.with(|m| m.borrow().get(sum).cloned()) {
        return r;
    }
    let r = factor_with_foldback_step(sum);
    MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 1 << 14 {
            m.clear();
        }
        m.insert(sum.to_vec(), r.clone());
    });
    r
}

fn factor_with_foldback_step(sum: &[Vec<(Signal, bool)>]) -> Option<(Vec<Vec<(Signal, bool)>>, String, Vec<(Signal, bool)>)> {
    // De Morgan factoring with the block's foldback NAND terms: products that share a common
    // cube C and differ in one further literal each, `C&r1 # C&r2 # ... # C&rk`, are
    // `C & (r1 + ... + rk)` = `C & !(!r1 & ... & !rk)`: one foldback PT (the NAND) replaces k
    // products, however wide the OR. The vendor does this for |C| = 1 (`L&a # L&b # L&!c`); here C
    // is any common cube (including the empty cube: a plain OR of literals). The grouping that
    // removes the most products wins; ties prefer the vendor's |C| = 1 form.
    let no_fold = |p: &Vec<(Signal, bool)>| p.iter().all(|(s, _)| !matches!(s, Signal::Fold(_)));
    // candidate cubes: each product minus one literal
    let mut cands: Vec<Vec<(Signal, bool)>> = vec![];
    for p in sum.iter().filter(|p| no_fold(p)) {
        for i in 0..p.len() {
            let mut c = p.clone();
            c.remove(i);
            c.sort();
            cands.push(c);
        }
    }
    cands.sort();
    cands.dedup();
    let mut best: Option<(Vec<(Signal, bool)>, Vec<usize>, (usize, bool))> = None;
    for c in cands {
        let members: Vec<usize> = sum
            .iter()
            .enumerate()
            .filter(|(_, p)| no_fold(p) && p.len() == c.len() + 1 && c.iter().all(|l| p.contains(l)))
            .map(|(i, _)| i)
            .collect();
        if members.len() < 2 {
            continue;
        }
        // k products become one: saving k - 1; prefer the vendor's single-literal common cube
        let score = (members.len() - 1, c.len() == 1);
        if best.as_ref().map_or(true, |b| score > b.2) {
            best = Some((c, members, score));
        }
    }
    let (c, members, _) = best?;
    let mut fold: Vec<(Signal, bool)> =
        members.iter().map(|&i| sum[i].iter().find(|x| !c.contains(x)).cloned().map(|(s, p)| (s, !p)).unwrap()).collect();
    fold.sort();
    fold.dedup();
    if fold.windows(2).any(|w| w[0].0 == w[1].0) {
        return None; // x & !x: the remainder sum is a tautology; leave it to the minimiser
    }
    let key = fold.iter().map(|(s, p)| format!("{}{}", if *p { "" } else { "!" }, s)).collect::<Vec<_>>().join("&");
    let mut out: Vec<Vec<(Signal, bool)>> = sum.iter().enumerate().filter(|(i, _)| !members.contains(i)).map(|(_, p)| p.clone()).collect();
    let mut np = c;
    np.push((Signal::Fold(key.clone()), true));
    np.sort();
    out.push(np);
    Some((out, key, fold))
}

/// A special-function product term. If the function needs several products but its complement
/// is one product `P`, use the foldback expander `!P` (what the vendor fitter does); the
/// foldbacks it creates go into `folds`.
pub(super) fn one_product(s: &Sop, cell: &str, what: &str, folds: &mut FoldTable) -> Result<Vec<(Signal, bool)>, FitError> {
    let m = if s.0.len() > 1 { s.minimize() } else { s.clone() };
    match m.0.len() {
        1 => Ok(m.0[0].clone()),
        0 => Ok(vec![]),
        _ => {
            let c = m.complement();
            let c = if c.0.iter().flatten().any(|(s, p)| !p && matches!(s, Signal::Fold(_))) { Sop(vec![vec![]; 2]) } else { c };
            let key_of =
                |p: &[(Signal, bool)]| p.iter().map(|(s, p)| format!("{}{}", if *p { "" } else { "!" }, s)).collect::<Vec<_>>().join("&");
            if c.0.len() == 1 {
                let key = key_of(&c.0[0]);
                folds.insert(key.clone(), c.0[0].clone());
                Ok(vec![(Signal::Fold(key), true)])
            } else if m.0.len() <= 15 && m.0.iter().all(|p| p.iter().all(|(s, v)| *v || !matches!(s, Signal::Fold(_)))) {
                // De Morgan with two foldback levels: p1 + ... + pk = !(!p1 & ... & !pk), where
                // each !pi is the foldback of pi and the outer NAND is a foldback reading them:
                // k + 1 foldback PTs of the block (at most 16) instead of a buried node.
                let inner: Vec<(Signal, bool)> =
                    m.0.iter()
                        .map(|p| match p.as_slice() {
                            // !l for a single literal comes straight from the UIM
                            [(s, v)] if !matches!(s, Signal::Fold(_)) => (s.clone(), !v),
                            _ => {
                                let k = key_of(p);
                                folds.insert(k.clone(), p.clone());
                                (Signal::Fold(k), true)
                            }
                        })
                        .collect();
                let outer = format!("nand({})", inner.iter().map(|(s, _)| s.to_string()).collect::<Vec<_>>().join(","));
                folds.insert(outer.clone(), inner);
                Ok(vec![(Signal::Fold(outer), true)])
            } else {
                err(FitErrorKind::Other, format!("{cell}: {what} needs more than one product term ({} products: {})", m.0.len(), m))
            }
        }
    }
}
