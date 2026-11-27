//! The product-term capacity of macrocells and cascade chains (spec F4.3), the one model that
//! the netlist passes ([`crate::opt`]), placement and the product-term planner share.
//!
//! A macrocell has [`PTS_PER_MC`] product terms. A wider sum borrows the terms of the macrocells
//! below it in its block: helper MC−1 cascades its sum into MC, MC−2 into MC−1, and so on, so a
//! chain of `h` helpers gives up to `5 * (h + 1)` terms minus the cell's special PTs
//! (*oracle-verified*, `tools/fuzzers/fitter-cascade`: the vendor fitter builds such chains
//! for 6–30 terms, always downwards, never longer than [`MAX_CHAIN_MCS`] macrocells).
//!
//! [`pt_demand`] estimates the product terms a cell needs (the cheaper polarity, an XOR split,
//! foldback factoring, its special PTs); placement reserves helpers by it and the netlist
//! passes count macrocells by it ([`mcs_for`]).

use crate::logic::{Signal, Sop};
use crate::netlist::{Cell, XorSplit};

/// Product terms of one macrocell.
pub const PTS_PER_MC: usize = 5;

/// The longest cascade chain, in macrocells (the cell's own and its helpers). The vendor fitter
/// chains up to 6 (30 terms) and builds wider sums from nodes instead (`fitter-cascade`, 31–40
/// terms); longer chains are not oracle-tested, so the fitter does not build them either.
pub const MAX_CHAIN_MCS: usize = 6;

/// Product terms one cell can reach through a cascade chain.
pub const MAX_CHAIN_PTS: usize = MAX_CHAIN_MCS * PTS_PER_MC;

/// Cascade helpers a cell needing `pts` product terms takes, before its special PTs are counted
/// (the estimate placement reserves helpers by).
pub fn helpers_for(pts: usize) -> usize {
    pts.saturating_sub(1) / PTS_PER_MC
}

/// Macrocells cell `c` takes with cover `data`: its own and its cascade helpers.
pub fn mcs_for(c: &Cell, data: &Sop) -> usize {
    1 + helpers_for(pt_demand_with(c, data, None))
}

/// Which helper holds sum term `k` (1-based, counted after the cell's own free PTs) and in which
/// of its PT slots: helper `h` is MC−h.
pub fn helper_slot(k: usize) -> (usize, usize) {
    ((k - 1) / PTS_PER_MC + 1, (k - 1) % PTS_PER_MC + 1)
}

/// Intermediate cubes spent on a complement that only decides between the two polarities.
pub const COMPLEMENT_CHOICE_CUBES: usize = 1 << 14;

/// Estimated product terms a cell needs in its macrocell (sum + special functions).
pub fn pt_demand(c: &Cell, xor: Option<&XorSplit>) -> usize {
    pt_demand_with(c, &c.data, xor)
}

/// [`pt_demand`] of cell `c` with `data` in place of its own cover (the netlist passes cost a
/// cover before they substitute it).
pub fn pt_demand_with(c: &Cell, data: &Sop, xor: Option<&XorSplit>) -> usize {
    let target = if c.active_low { data.complement() } else { data.clone() };
    let mut n = target.0.len();
    if let Some(x) = xor {
        n = n.min(1 + if c.active_low { x.gbar.0.len() } else { x.g.0.len() });
    }
    if n > 1 {
        // the other polarity as an alternative cost; one too large to compute is no alternative
        let alt =
            if c.active_low { data.0.len() } else { data.complement_within(COMPLEMENT_CHOICE_CUBES).map_or(usize::MAX, |x| x.0.len()) };
        n = n.min(alt.max(1));
    }
    if n > 3 {
        n = n.min(factored_len(&target));
    }
    let single_pad = |s: &Option<Sop>| s.as_ref().map_or(false, |x| matches!(x.single_literal(), Some((Signal::Pad(_), _))));
    if c.clk.is_some() && !single_pad(&c.clk) {
        n += 1;
    }
    if c.ce.is_some() {
        n += 1;
    }
    if c.ar.is_some() && !single_pad(&c.ar) {
        n += 1;
    }
    if c.ap.is_some() {
        n += 1;
    }
    if c.oe.as_ref().map_or(false, |x| x.single_literal().is_none() && !x.is_one()) {
        n += 1;
    }
    n
}

/// Products left after foldback factoring of `target` down to 3 (see [`factor_with_foldback`]),
/// memoised per thread: every fit attempt of every strategy asks again for the same covers. The
/// factoring also registers its foldbacks in FOLDS, which planning resets before it factors for
/// real, so a cached answer loses nothing.
pub fn factored_len(target: &Sop) -> usize {
    thread_local! {
        static FACTORED: std::cell::RefCell<rustc_hash::FxHashMap<Sop, usize>> = Default::default();
    }
    if let Some(n) = FACTORED.with(|m| m.borrow().get(target).copied()) {
        return n;
    }
    let mut s = target.0.clone();
    while s.len() > 3 {
        match crate::fit::factor_with_foldback(&s) {
            Some(x) => s = x,
            None => break,
        }
    }
    FACTORED.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 1 << 14 {
            m.clear();
        }
        m.insert(target.clone(), s.len());
    });
    s.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_arithmetic() {
        assert_eq!(helpers_for(5), 0);
        assert_eq!(helpers_for(6), 1);
        assert_eq!(helpers_for(10), 1);
        assert_eq!(helpers_for(11), 2);
        assert_eq!(helpers_for(MAX_CHAIN_PTS), MAX_CHAIN_MCS - 1);
        assert_eq!(helper_slot(1), (1, 1));
        assert_eq!(helper_slot(5), (1, 5));
        assert_eq!(helper_slot(6), (2, 1));
        assert_eq!(helper_slot(12), (3, 2));
    }
}
