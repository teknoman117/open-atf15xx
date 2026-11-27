//! Netlist optimisations between the Yosys front end and the fitter (spec §F7.4).

use crate::capacity::mcs_for;
use crate::logic::{Signal, Sop};
use crate::netlist::{Cell, Design, Storage};

fn n_products(s: &Sop) -> usize {
    s.0.len()
}

/// Most distinct inputs a cell may get from inlining or analysis rewriting. A macrocell's PTs see
/// only its block's 40 UIM inputs (`crate::fit::UIM_INPUTS_PER_BLOCK`); staying well below
/// leaves room for the other cells of the block, and keeps carry chains as buried nodes.
pub const MAX_SUPPORT: usize = 32;

/// Products the fitter has to place for a data cover: it picks the cheaper polarity itself.
fn cost(s: &Sop) -> usize {
    let n = s.0.len();
    if n <= 1 {
        n
    } else if n > 4 && s.support() > 12 {
        // complementing a wide cover is exponential (a 16:1 mux): assume the given polarity
        n
    } else {
        n.min(s.complement().0.len())
    }
}

/// Product terms available for a cell's data: `max_products` (the cell's macrocell and its
/// cascade helpers, at most [`crate::capacity::MAX_CHAIN_PTS`]) minus the PTs its special functions may take (a clock that is a single literal usually goes
/// to a GCLK, so it is not counted).
pub fn data_limit(c: &Cell, max_products: usize) -> usize {
    let mut used = [&c.ce, &c.ar, &c.ap, &c.oe].iter().filter(|x| x.as_ref().map_or(false, |f| !f.is_one())).count();
    if c.clk.as_ref().map_or(false, |f| f.single_literal().is_none()) {
        used += 1;
    }
    max_products.saturating_sub(used).max(2)
}

/// Split data covers that exceed [`data_limit`] into buried OR nodes (`<cell>_s<k>`, at most
/// `limit` products each) so that every cell fits its macrocell and cascade helpers.
/// Returns the number of nodes created.
/// Intermediate cubes [`split_large`] spends on finding out whether the complement is cheaper.
const SPLIT_COMPLEMENT_CUBES: usize = 1 << 14;

pub fn split_large(d: &mut Design, max_products: usize) -> usize {
    let mut created = 0;
    let mut i = 0;
    while i < d.cells.len() {
        let limit = data_limit(&d.cells[i], max_products);
        let c = &d.cells[i];
        if cost(&c.data) > limit {
            // work in the cheaper polarity (a complement too large to compute is not cheaper)
            let (mut data, flip) = match c.data.complement_if_smaller(c.data.0.len(), SPLIT_COMPLEMENT_CUBES) {
                Some(comp) => (comp, true),
                _ => (c.data.minimize(), false),
            };
            let base = c.name.clone();
            let mut nodes = vec![];
            let node_limit = data_limit(&Cell { ce: None, ar: None, ap: None, oe: None, clk: None, ..c.clone() }, max_products);
            while data.0.len() > limit {
                let take = node_limit.min(data.0.len() - limit + 1);
                let at = data.0.len() - take;
                let chunk: Vec<_> = data.0.split_off(at);
                let n = (0..)
                    .map(|k| format!("{base}_s{k}"))
                    .find(|n| d.cell_index(n).is_none() && !nodes.iter().any(|x: &Cell| &x.name == n))
                    .unwrap();
                let mut node = Cell {
                    name: n.clone(),
                    is_output: false,
                    pin: None,
                    node: Some(0),
                    storage: Storage::Comb,
                    data: Sop(chunk),
                    active_low: false,
                    clk: None,
                    ce: None,
                    ar: None,
                    ap: None,
                    oe: None,
                };
                node.data = node.data.minimize();
                data.0.push(vec![(Signal::Fb(n, false), true)]);
                nodes.push(node);
            }
            let c = &mut d.cells[i];
            c.data = data;
            if flip {
                c.active_low = !c.active_low;
            }
            created += nodes.len();
            d.cells.extend(nodes);
        }
        i += 1;
    }
    created
}

/// Split products of cells that read more than [`MAX_SUPPORT`] signals into shared prefix
/// nodes (R4, spec F9.5): the literals common to the most wide products first, so a wide
/// counter's toggle functions become a carry *chain* (`C = en·q0…q30`, `T63 = C·q31…q62`)
/// instead of Yosys's lookahead tree. Returns the number of nodes made.
pub fn factor_wide(d: &mut Design) -> usize {
    use std::collections::BTreeMap;
    let mut made = 0;
    for _ in 0..64 {
        let wide: Vec<usize> = (0..d.cells.len()).filter(|&i| d.cells[i].data.support() > MAX_SUPPORT).collect();
        if wide.is_empty() {
            break;
        }
        let mut freq: BTreeMap<(Signal, bool), usize> = BTreeMap::new();
        for &i in &wide {
            for p in &d.cells[i].data.0 {
                for l in p {
                    *freq.entry(l.clone()).or_default() += 1;
                }
            }
        }
        // the widest cell's longest product, its most shared literals first
        let &w = wide.iter().max_by_key(|&&i| (d.cells[i].data.support(), std::cmp::Reverse(i))).unwrap();
        let Some(p) = d.cells[w].data.0.iter().max_by_key(|p| p.len()).cloned() else { break };
        let mut lits = p.clone();
        lits.sort_by(|a, b| freq[b].cmp(&freq[a]).then_with(|| a.cmp(b)));
        let k = (MAX_SUPPORT - 1).min(lits.len().saturating_sub(1));
        if k < 2 {
            break;
        }
        let mut head: Vec<(Signal, bool)> = lits[..k].to_vec();
        head.sort();
        let name = (0..).map(|n| format!("carry{n}")).find(|n| d.cell_index(n).is_none()).unwrap();
        let lit = (Signal::Fb(name.clone(), false), true);
        for c in d.cells.iter_mut() {
            let mut changed = false;
            let mut prods = c.data.0.clone();
            for q in prods.iter_mut() {
                if head.iter().all(|l| q.contains(l)) {
                    q.retain(|l| !head.contains(l));
                    q.push(lit.clone());
                    changed = true;
                }
            }
            if changed {
                let mut s = Sop::zero();
                for q in prods {
                    s.push(q);
                }
                c.data = s;
            }
        }
        d.cells.push(Cell {
            name,
            is_output: false,
            pin: None,
            node: Some(0),
            storage: Storage::Comb,
            data: Sop(vec![head]),
            active_low: false,
            clk: None,
            ce: None,
            ar: None,
            ap: None,
            oe: None,
        });
        made += 1;
    }
    made
}

fn readers_of(d: &Design, name: &str) -> Vec<usize> {
    d.cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.name != name && c.inputs().iter().any(|s| matches!(s, Signal::Fb(n, false) if n == name)))
        .map(|(i, _)| i)
        .collect()
}

/// Inline buried combinatorial nodes into their readers when every reader's data function stays
/// within [`data_limit`] products after minimisation and the macrocell count does not grow
/// (`ties`: also when a saved node only becomes a cascade neighbour of its reader) (and special functions stay single
/// products). Nodes that cannot be inlined are kept (marked with node number 0 so that the fitter
/// does not collapse them either). Unlike `Design::collapse_comb_nodes` (the vendor's
/// unconditional collapse) this cannot blow up XOR-heavy logic exponentially.
pub fn collapse_limited(d: &mut Design, max_products: usize, ties: bool) {
    let mut rejected: std::collections::BTreeSet<String> = Default::default();
    loop {
        let mut progress = false;
        let mut cands: Vec<String> = d
            .cells
            .iter()
            .filter(|c| !c.is_output && c.storage == Storage::Comb && c.node.is_none() && c.oe.is_none())
            .map(|c| c.name.clone())
            .collect();
        if cands.is_empty() {
            break;
        }
        // smallest nodes first: they are the cheapest to inline
        cands.sort_by_key(|n| d.cell(n).map_or(0, |c| c.data.0.len()));
        for name in cands {
            let Some(i) = d.cell_index(&name) else { continue };
            let node = d.cells[i].clone();
            let s = Signal::Fb(name.clone(), false);
            let (f, fnot) = if node.active_low {
                (node.data.complement(), node.data.minimize())
            } else {
                (node.data.minimize(), node.data.complement())
            };
            let readers = readers_of(d, &name);
            // tentative substitution; macrocell balance: the node's own macrocell is saved, each
            // reader may need more (a cascade neighbour) when its cover outgrows one macrocell
            let mut ok = true;
            let mut extra_mcs: isize = -1;
            let mut extra_pts: isize = -(cost(&node.data) as isize);
            let mut new = vec![];
            for &r in &readers {
                let c = &d.cells[r];
                let sub = |x: &Sop| x.substitute(&s, &f, &fnot).minimize();
                // support first: a wide function must not be complemented or costed
                let mut ins: Vec<Signal> = c.inputs().into_iter().filter(|x| *x != s).chain(node.inputs()).collect();
                ins.sort();
                ins.dedup();
                if ins.len() > MAX_SUPPORT {
                    ok = false;
                    break;
                }
                let data = sub(&c.data);
                if cost(&data) > data_limit(c, max_products) {
                    ok = false;
                    break;
                }
                let mut slots = vec![];
                for slot in [&c.clk, &c.ce, &c.ar, &c.ap, &c.oe] {
                    let v = slot.as_ref().map(|x| sub(x));
                    if v.as_ref().map_or(false, |x| n_products(x) > 1 && n_products(&x.complement()) > 1) {
                        ok = false;
                    }
                    slots.push(v);
                }
                if !ok {
                    break;
                }
                extra_mcs += mcs_for(c, &data) as isize - mcs_for(c, &c.data) as isize;
                extra_pts += cost(&data) as isize - cost(&c.data) as isize;
                new.push((r, data, slots));
            }
            // a node with several readers is shared logic: duplicating it must not cost more
            // product terms than it saves (e.g. `!load` expanding into every bit of a register)
            let shared_ok = readers.len() <= 1 || extra_pts <= 0;
            if ok && shared_ok && (extra_mcs < 0 || ties && extra_mcs == 0) {
                for (r, data, slots) in new {
                    let c = &mut d.cells[r];
                    c.data = data;
                    let mut it = slots.into_iter();
                    for slot in [&mut c.clk, &mut c.ce, &mut c.ar, &mut c.ap, &mut c.oe] {
                        *slot = it.next().unwrap();
                    }
                }
                d.cells.remove(i);
                rejected.remove(&name);
                progress = true;
            } else {
                rejected.insert(name);
            }
        }
        if !progress {
            break;
        }
    }
    // what stays is a buried node; the fitter must not collapse it either
    for c in d.cells.iter_mut() {
        if rejected.contains(&c.name) && c.node.is_none() {
            c.node = Some(0);
        }
    }
}

/// Macrocells a cell needs for a data cover: one, plus a cascade neighbour when the cover does
/// not fit the PTs its special functions leave free.
/// `f` with buried combinatorial nodes substituted recursively, or None if the cover grows beyond
/// `budget` products (the analysis view used by toggle recovery: Yosys/ABC leave the register's
/// own Q hidden inside multi-level logic).
/// Exact evaluation of covers through the buried combinatorial nodes with BDDs
/// (`biodivine-lib-bdd`): linear in size for carry and compare chains, where SOP flattening
/// grows exponentially (an up/down counter's carry-lookahead network). Leaves are the
/// signals that are not substitutable nodes (the same nodes [`flatten`] substitutes).
struct BddNet<'a> {
    d: &'a Design,
    vars: biodivine_lib_bdd::BddVariableSet,
    var_of: std::collections::BTreeMap<Signal, biodivine_lib_bdd::BddVariable>,
    sig_of: Vec<Signal>,
    memo: std::collections::BTreeMap<String, biodivine_lib_bdd::Bdd>,
}

fn substitutable<'a>(d: &'a Design, s: &Signal) -> Option<&'a Cell> {
    match s {
        Signal::Fb(n, false) => d.cell(n).filter(|c| !c.is_output && c.storage == Storage::Comb && c.oe.is_none()),
        _ => None,
    }
}

impl<'a> BddNet<'a> {
    /// A net over the leaves reachable from `roots` (at most `max_vars` of them).
    fn new(d: &'a Design, roots: &[&Sop], max_vars: usize) -> Option<BddNet<'a>> {
        let mut leaves: std::collections::BTreeSet<Signal> = Default::default();
        let mut seen: std::collections::BTreeSet<String> = Default::default();
        let mut stack: Vec<Signal> = roots.iter().flat_map(|r| r.signals()).collect();
        while let Some(s) = stack.pop() {
            match substitutable(d, &s) {
                Some(c) => {
                    if seen.insert(c.name.clone()) {
                        stack.extend(c.data.signals());
                    }
                }
                None => {
                    leaves.insert(s);
                }
            }
            if leaves.len() > max_vars {
                return None;
            }
        }
        let sig_of: Vec<Signal> = leaves.into_iter().collect();
        let vars = biodivine_lib_bdd::BddVariableSet::new_anonymous(sig_of.len() as u16);
        let var_of = vars.variables().into_iter().zip(sig_of.iter().cloned()).map(|(v, s)| (s, v)).collect();
        Some(BddNet { d, vars, var_of, sig_of, memo: Default::default() })
    }

    fn lit(&mut self, s: &Signal, pos: bool) -> biodivine_lib_bdd::Bdd {
        let b = match substitutable(self.d, s) {
            Some(c) => {
                let name = c.name.clone();
                if let Some(b) = self.memo.get(&name) {
                    b.clone()
                } else {
                    let (data, low) = (c.data.clone(), c.active_low);
                    let mut b = self.sop(&data);
                    if low {
                        b = b.not();
                    }
                    self.memo.insert(name, b.clone());
                    b
                }
            }
            None => self.vars.mk_var(self.var_of[s]),
        };
        if pos {
            b
        } else {
            b.not()
        }
    }

    fn sop(&mut self, f: &Sop) -> biodivine_lib_bdd::Bdd {
        let mut acc = self.vars.mk_false();
        for p in &f.0 {
            let mut t = self.vars.mk_true();
            for (s, pos) in p {
                t = t.and(&self.lit(s, *pos));
            }
            acc = acc.or(&t);
        }
        acc
    }

    fn var(&self, s: &Signal) -> Option<biodivine_lib_bdd::Bdd> {
        self.var_of.get(s).map(|v| self.vars.mk_var(*v))
    }

    /// A small cover of `b` (greedy-optimised DNF, then made irredundant); None beyond `max`.
    fn cover(&self, b: &biodivine_lib_bdd::Bdd, max: usize) -> Option<Sop> {
        let dnf = b
            ._to_optimized_dnf(true, &|dnf: &[biodivine_lib_bdd::BddPartialValuation]| if dnf.len() > max { Err(()) } else { Ok(()) })
            .ok()?;
        let mut s = Sop::zero();
        for clause in dnf {
            s.push(clause.to_values().into_iter().map(|(v, val)| (self.sig_of[v.to_index()].clone(), val)).collect());
        }
        Some(s.minimize())
    }
}

/// Toggle function of a D flip-flop by BDDs (see [`BddNet`]), for next-state functions too
/// large to flatten: `T = D' ⊕ Q` with `D' = e·D + !e·Q`. None if it does not depend on Q or
/// has more than `max` products.
fn bdd_toggle(d: &Design, c: &Cell, max: usize) -> Option<Sop> {
    let q = Signal::Fb(c.name.clone(), true);
    let one = Sop::one();
    let roots: Vec<&Sop> = [Some(&c.data), c.ce.as_ref(), Some(&one)].into_iter().flatten().collect();
    let mut net = BddNet::new(d, &roots, 96)?;
    let qb = net.var(&q)?;
    let mut dn = net.sop(&c.data);
    if c.active_low {
        dn = dn.not();
    }
    if let Some(e) = &c.ce {
        let eb = net.sop(e);
        dn = eb.and(&dn).or(&eb.not().and(&qb));
    }
    let t = dn.xor(&qb);
    if t.is_false() {
        return None;
    }
    net.cover(&t, max)
}

/// Whether `f`, with the combinatorial nodes substituted, depends on more than `max` signals:
/// the leaves (an upper bound) first; then, only if there are more than `max`, the exact support
/// from a BDD whose variables follow a depth-first visit of the logic (interleaving the bits of
/// comparators and carry chains, where a sorted order is exponential), given up (false) beyond
/// [`SUPPORT_BDD_NODES`] nodes.
fn support_exceeds(d: &Design, f: &Sop, max: usize) -> bool {
    let mut order: Vec<Signal> = vec![];
    let mut seen_leaf: std::collections::BTreeSet<Signal> = Default::default();
    let mut seen_node: std::collections::BTreeSet<String> = Default::default();
    // depth-first, products and literals in order
    fn visit(
        d: &Design,
        f: &Sop,
        order: &mut Vec<Signal>,
        leaves: &mut std::collections::BTreeSet<Signal>,
        nodes: &mut std::collections::BTreeSet<String>,
    ) {
        for p in &f.0 {
            for (s, _) in p {
                match substitutable(d, s) {
                    Some(c) => {
                        if nodes.insert(c.name.clone()) {
                            visit(d, &c.data, order, leaves, nodes);
                        }
                    }
                    None => {
                        if leaves.insert(s.clone()) {
                            order.push(s.clone());
                        }
                    }
                }
            }
        }
    }
    visit(d, f, &mut order, &mut seen_leaf, &mut seen_node);
    if order.len() <= max || order.len() > 512 {
        return false;
    }
    let vars = biodivine_lib_bdd::BddVariableSet::new_anonymous(order.len() as u16);
    let var_of: std::collections::BTreeMap<Signal, biodivine_lib_bdd::BddVariable> = order.iter().cloned().zip(vars.variables()).collect();
    let mut memo: std::collections::BTreeMap<String, biodivine_lib_bdd::Bdd> = Default::default();
    fn build(
        d: &Design,
        f: &Sop,
        vars: &biodivine_lib_bdd::BddVariableSet,
        var_of: &std::collections::BTreeMap<Signal, biodivine_lib_bdd::BddVariable>,
        memo: &mut std::collections::BTreeMap<String, biodivine_lib_bdd::Bdd>,
    ) -> Option<biodivine_lib_bdd::Bdd> {
        let mut acc = vars.mk_false();
        for p in &f.0 {
            let mut t = vars.mk_true();
            for (s, pos) in p {
                let b = match substitutable(d, s) {
                    Some(c) => match memo.get(&c.name) {
                        Some(b) => b.clone(),
                        None => {
                            let mut b = build(d, &c.data, vars, var_of, memo)?;
                            if c.active_low {
                                b = b.not();
                            }
                            memo.insert(c.name.clone(), b.clone());
                            b
                        }
                    },
                    None => vars.mk_var(var_of[s]),
                };
                t = t.and(&if *pos { b } else { b.not() });
                if t.size() > SUPPORT_BDD_NODES {
                    return None;
                }
            }
            acc = acc.or(&t);
            if acc.size() > SUPPORT_BDD_NODES {
                return None;
            }
        }
        Some(acc)
    }
    build(d, f, &vars, &var_of, &mut memo).is_some_and(|b| b.support_set().len() > max)
}

/// The size at which [`support_exceeds`] gives up on its BDD.
const SUPPORT_BDD_NODES: usize = 50_000;

fn flatten(d: &Design, f: &Sop, budget: usize) -> Option<Sop> {
    // a cover can never have less support than the function it is: when the fully substituted
    // function depends on more than MAX_SUPPORT signals, the substitution below is bound to fail
    // (at the latest after its last node), so say so at once (see support_exceeds)
    if f.signals().iter().any(|s| substitutable(d, s).is_some()) && support_exceeds(d, f, MAX_SUPPORT) {
        return None;
    }
    let mut f = f.clone();
    for _ in 0..64 {
        let node = f.signals().into_iter().find_map(|s| match &s {
            Signal::Fb(n, false) => {
                d.cell(n).filter(|c| !c.is_output && c.storage == Storage::Comb && c.oe.is_none()).map(|c| (s.clone(), c))
            }
            _ => None,
        });
        let Some((s, c)) = node else { return Some(f) };
        let (fv, fnot) = if c.active_low { (c.data.complement(), c.data.minimize()) } else { (c.data.minimize(), c.data.complement()) };
        f = f.substitute(&s, &fv, &fnot).minimize();
        if f.0.len() > budget || f.support() > MAX_SUPPORT {
            return None;
        }
    }
    None
}

/// Recover toggle flip-flops: a D flip-flop whose next state is `Q ⊕ t` with a smaller cover for
/// `t` than for `D` (+ one product for a clock enable) becomes `Storage::Tff` with data `t` (the
/// macrocell XOR implements the toggle; `t` may still read Q). The next-state function is analysed flattened (buried
/// nodes substituted, up to `budget` products) with the clock enable folded in
/// (`D' = e·D + !e·Q`), so `if (en) q <= q + 1` becomes `T = en & q0 & ... ` without a CE term.
/// Counters shrink from O(n) products per bit (or a tree of nodes) to one. Returns the number of
/// conversions; nodes that become unused are left for [`remove_dead`].
pub fn recover_toggles(d: &mut Design, max_products: usize) -> usize {
    // The flattened D of a wide counter bit with a synchronous wrap is large (it contains the
    // complement of the wrap compare) even though its toggle function is one or two products,
    // so the analysis budget is much larger than any cell could ever use.
    // (silicon, 2026-11-24: with 64 the vblink counter kept a 4-level carry network and failed at
    // 66.6 MHz; with 1024 every bit but one became a 1-2 product T flip-flop.)
    let budget = 1024;
    let mut n = 0;
    for i in 0..d.cells.len() {
        let c = &d.cells[i];
        if c.storage != Storage::Dff {
            continue;
        }
        let dbg = std::env::var_os("ATF15XX_FLOW_DEBUG").is_some();
        let q = Signal::Fb(c.name.clone(), true);
        let qp = Sop(vec![vec![(q.clone(), true)]]);
        let qn = Sop(vec![vec![(q.clone(), false)]]);
        let data = if c.active_low { c.data.complement() } else { c.data.clone() };
        let Some(dflat) = flatten(d, &data, budget) else {
            // too large as a cover (a carry-lookahead network): compute the toggle function
            // exactly with BDDs instead (R4, spec F9.5)
            let limit = data_limit(&Cell { ce: None, ..c.clone() }, max_products);
            match bdd_toggle(d, c, limit) {
                // a toggle function wider than a block's UIM inputs (a wide counter) is accepted:
                // factor_wide splits its products into a shared carry chain
                Some(t) if cost(&t) <= limit => {
                    if dbg {
                        eprintln!("toggles: {}: D does not flatten within {budget} products; BDD: T {} products", c.name, cost(&t));
                    }
                    let c = &mut d.cells[i];
                    c.storage = Storage::Tff;
                    c.data = t;
                    c.active_low = false;
                    c.ce = None;
                    n += 1;
                }
                _ => {
                    if dbg {
                        eprintln!("toggles: {}: D does not flatten within {budget} products (BDD: no small T)", c.name);
                    }
                }
            }
            continue;
        };
        let (deff, old_cost) = match &c.ce {
            Some(e) => {
                let Some(e) = flatten(d, e, budget) else {
                    if dbg {
                        eprintln!("toggles: {}: CE does not flatten", c.name);
                    }
                    continue;
                };
                (e.and(&dflat).or(&e.complement().and(&qp)).minimize(), cost(&c.data) + 1)
            }
            None => (dflat.clone(), cost(&c.data)),
        };
        if !deff.signals().contains(&q) {
            if dbg {
                eprintln!("toggles: {}: next state does not depend on Q: {:?}", c.name, deff.signals());
            }
            continue;
        }
        let d1 = deff.substitute(&q, &Sop::one(), &Sop::zero()).minimize();
        let d0 = deff.substitute(&q, &Sop::zero(), &Sop::one()).minimize();
        // t = q ? !d1 : d0
        let t = qp.and(&d1.complement()).or(&qn.and(&d0)).minimize();
        // T may read Q itself (the T sum has the register feedback like any PT), e.g. for a
        // synchronous load: T = load·(d ⊕ Q) + !load·carry
        if dbg {
            eprintln!("toggles: {}: D' {} products, T {} products", c.name, cost(&deff), cost(&t));
        }
        // compare with the flattened D (with the CE folded in); nodes of the given form cost
        // macrocells too, so the flattened cover is the fair comparison
        let _ = old_cost;
        let limit = data_limit(&Cell { ce: None, ..c.clone() }, max_products);
        if cost(&t) <= limit && cost(&t) < cost(&deff) {
            let c = &mut d.cells[i];
            c.storage = Storage::Tff;
            c.data = t;
            c.active_low = false;
            c.ce = None;
            n += 1;
        }
    }
    n
}

/// Substitute the buried combinatorial nodes on the paths from `target` into `f` (nodes whose
/// transitive support contains `target`); other nodes stay literals. None if it grows beyond
/// `budget` products.
fn flatten_through(d: &Design, f: &Sop, target: &Signal, budget: usize) -> Option<Sop> {
    use std::collections::BTreeMap;
    let mut memo: BTreeMap<String, bool> = BTreeMap::new();
    fn reaches(d: &Design, n: &str, target: &Signal, memo: &mut BTreeMap<String, bool>, depth: usize) -> bool {
        if let Some(&r) = memo.get(n) {
            return r;
        }
        memo.insert(n.to_string(), false);
        let Some(c) = d.cell(n) else { return false };
        let mut r = false;
        if depth < 200 {
            for s in c.data.signals() {
                if &s == target {
                    r = true;
                    break;
                }
                if let Signal::Fb(m, false) = &s {
                    if is_node(d, m) && reaches(d, m, target, memo, depth + 1) {
                        r = true;
                        break;
                    }
                }
            }
        }
        memo.insert(n.to_string(), r);
        r
    }
    let mut f = f.clone();
    for _ in 0..256 {
        let next = f.signals().into_iter().find_map(|s| match &s {
            Signal::Fb(n, false) if is_node(d, n) && reaches(d, n, target, &mut memo, 0) => Some((s.clone(), d.cell(n).unwrap())),
            _ => None,
        });
        let Some((s, c)) = next else { return Some(f) };
        let (fv, fnot) = if c.active_low { (c.data.complement(), c.data.minimize()) } else { (c.data.minimize(), c.data.complement()) };
        f = f.substitute(&s, &fv, &fnot).minimize();
        if f.0.len() > budget || f.support() > MAX_SUPPORT {
            return None;
        }
    }
    None
}

fn is_node(d: &Design, n: &str) -> bool {
    d.cell(n).map_or(false, |c| !c.is_output && c.storage == Storage::Comb && c.oe.is_none())
}

/// Minimise `f` using `dc` as don't-care set: expand each product by dropping literals while it
/// stays inside `f + dc`, then drop products covered by others (the on-set `f·!dc` stays
/// covered: products are only ever enlarged within `f + dc`).
fn minimize_dc(f: &Sop, dc: &Sop) -> Sop {
    let off = f.or(dc).complement();
    let inside = |p: &Vec<(Signal, bool)>| Sop(vec![p.clone()]).and(&off).minimize().is_zero();
    let mut prods: Vec<Vec<(Signal, bool)>> = f.minimize().0;
    for p in prods.iter_mut() {
        let mut k = 0;
        while k < p.len() {
            let mut q = p.clone();
            q.remove(k);
            if inside(&q) {
                *p = q;
            } else {
                k += 1;
            }
        }
    }
    let on = f.and(&dc.complement()).minimize();
    // greedy irredundant cover of the on-set
    let mut keep: Vec<Vec<(Signal, bool)>> = prods.clone();
    let mut i = 0;
    while i < keep.len() {
        let mut rest = keep.clone();
        rest.remove(i);
        if on.and(&Sop(rest.clone()).complement()).minimize().is_zero() {
            keep = rest;
        } else {
            i += 1;
        }
    }
    Sop(keep).minimize()
}

/// Recover clock enables. Yosys' `dfflegalize` turns enables and synchronous resets into
/// multiplexers in front of a plain D flip-flop, which makes every bit of a shift register or
/// loadable register pay for the hold path. With `D0`/`D1` the cofactors of the next state on Q
/// (through the nodes between Q and D), Q is held where `!D0·D1`; if Q never toggles (`D0 → D1`),
/// `CE = D0 + !D1` and `D = D0` with the hold region as don't-care. A multi-product CE becomes a
/// buried node shared by every register with the same enable. A register is a candidate when
/// its D cover shrinks; a multi-product enable is used only when at least two registers share it. Returns the number of converted registers.
pub fn recover_enables(d: &mut Design, max_products: usize) -> usize {
    use std::collections::BTreeMap;
    let mut cand: Vec<(usize, Sop, Sop, usize)> = vec![]; // (cell, ce, new d, saved MCs)
    for (i, c) in d.cells.iter().enumerate() {
        if c.storage != Storage::Dff || c.ce.is_some() || c.active_low {
            continue;
        }
        let dbg = std::env::var_os("ATF15XX_FLOW_DEBUG").is_some();
        let q = Signal::Fb(c.name.clone(), true);
        let Some(df) = flatten_through(d, &c.data, &q, 64) else {
            if dbg {
                eprintln!("enables: {}: does not flatten", c.name)
            }
            continue;
        };
        if !df.signals().contains(&q) {
            if dbg {
                eprintln!("enables: {}: no Q in {:?}", c.name, df.signals())
            }
            continue;
        }
        let d1 = df.substitute(&q, &Sop::one(), &Sop::zero()).minimize();
        let d0 = df.substitute(&q, &Sop::zero(), &Sop::one()).minimize();
        if !d0.and(&d1.complement()).minimize().is_zero() {
            continue; // toggles somewhere: not an enable
        }
        let hold = d0.complement().and(&d1).minimize();
        if hold.is_zero() {
            continue;
        }
        let ce = hold.complement().minimize();
        let nd = minimize_dc(&d0, &hold);
        let mut probe = c.clone();
        probe.ce = Some(Sop(vec![vec![(Signal::Fb("ce".into(), false), true)]]));
        let old = mcs_for(c, &df);
        let new = mcs_for(&probe, &nd);
        if dbg {
            eprintln!("enables: {}: D {} -> {} products, CE {} products, MCs {old} -> {new}", c.name, cost(&df), cost(&nd), ce.0.len())
        }
        if cost(&nd) > data_limit(&probe, max_products) {
            continue;
        }
        // the given form still has its multiplexer nodes (inlined later), so compare covers
        if cost(&nd) >= cost(&df) && new >= old {
            continue;
        }
        cand.push((i, ce, nd, 1));
    }
    // group by enable function
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (k, (_, ce, _, _)) in cand.iter().enumerate() {
        groups.entry(format!("{:?}", ce.0)).or_default().push(k);
    }
    let mut n = 0;
    let mut new_nodes = vec![];
    for (_, ks) in groups {
        let ce = cand[ks[0]].1.clone();
        let single = ce.0.len() == 1;
        let saved: usize = ks.iter().map(|&k| cand[k].3).sum();
        if !(single && saved > 0 || !single && ks.len() > 1) {
            continue;
        }
        let ce_sop = if single {
            ce
        } else {
            let base = format!("{}_ce", d.cells[cand[ks[0]].0].name);
            let name = (0..).map(|k| format!("{base}{k}")).find(|x| d.cell_index(x).is_none()).unwrap();
            new_nodes.push(Cell {
                name: name.clone(),
                is_output: false,
                pin: None,
                node: Some(0),
                storage: Storage::Comb,
                data: ce,
                active_low: false,
                clk: None,
                ce: None,
                ar: None,
                ap: None,
                oe: None,
            });
            Sop(vec![vec![(Signal::Fb(name, false), true)]])
        };
        for &k in &ks {
            let (i, _, nd, _) = &cand[k];
            let c = &mut d.cells[*i];
            c.data = nd.clone();
            c.ce = Some(ce_sop.clone());
            n += 1;
        }
    }
    d.cells.extend(new_nodes);
    n
}

/// Recover asynchronous reset/set of latches. Yosys maps `if (r) q = 0; else if (le) q = d;` to a
/// plain latch with `LE = le + r`, `D = !r·d`, which has a hazard when `r` falls with `le` low
/// (`LE` closes while `D` rises from the same edge). If `LE` has a single-literal product `s` and
/// the data with `s` true is a constant, the latch becomes `LE|s=0`, `D|s=0` with `s` as AR
/// (constant 0) or AP (constant 1): the same function, without the race. Returns the count.
pub fn recover_latch_resets(d: &mut Design) -> usize {
    let mut n = 0;
    for c in d.cells.iter_mut() {
        if c.storage != Storage::Latch || c.ar.is_some() || c.ap.is_some() || c.active_low {
            continue;
        }
        let Some(le) = c.clk.clone() else { continue };
        let lits: Vec<(Signal, bool)> = le.0.iter().filter(|p| p.len() == 1).map(|p| p[0].clone()).collect();
        for (sig, pos) in lits {
            let (one, zero) = if pos { (Sop::one(), Sop::zero()) } else { (Sop::zero(), Sop::one()) };
            let data_on = c.data.substitute(&sig, &one, &zero).minimize();
            let (is0, is1) = (data_on.is_zero(), data_on.is_one());
            if !is0 && !is1 {
                continue;
            }
            let le_off = le.substitute(&sig, &zero, &one).minimize();
            if le_off.is_zero() {
                continue;
            }
            let lit = Sop(vec![vec![(sig.clone(), pos)]]);
            c.data = c.data.substitute(&sig, &zero, &one).minimize();
            c.clk = Some(le_off);
            if is0 {
                c.ar = Some(lit);
            } else {
                c.ap = Some(lit);
            }
            n += 1;
            break;
        }
    }
    n
}

/// Remove buried combinatorial nodes that nothing reads (after toggle recovery or inlining).
pub fn remove_dead(d: &mut Design) -> usize {
    let mut removed = 0;
    loop {
        let dead = d.cells.iter().position(|c| !c.is_output && c.storage == Storage::Comb && readers_of(d, &c.name).is_empty());
        match dead {
            Some(i) => {
                d.cells.remove(i);
                removed += 1;
            }
            None => return removed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(n: &str, reg: bool, pos: bool) -> (Signal, bool) {
        (Signal::Fb(n.into(), reg), pos)
    }

    #[test]
    fn toggle_recovery_on_counter_bit() {
        // q2' = q2 ^ (q0 & q1)
        let data = Sop(vec![
            vec![lit("q2", true, true), lit("q0", true, false)],
            vec![lit("q2", true, true), lit("q1", true, false)],
            vec![lit("q2", true, false), lit("q0", true, true), lit("q1", true, true)],
        ]);
        let mut d = Design::default();
        d.cells.push(Cell {
            name: "q2".into(),
            is_output: true,
            pin: None,
            node: None,
            storage: Storage::Dff,
            data,
            active_low: false,
            clk: Some(Sop(vec![vec![(Signal::Pad("clk".into()), true)]])),
            ce: None,
            ar: None,
            ap: None,
            oe: None,
        });
        assert_eq!(recover_toggles(&mut d, 10), 1);
        assert_eq!(d.cells[0].storage, Storage::Tff);
        assert_eq!(d.cells[0].data.0.len(), 1);
        // with a clock enable: if (en) q <= ~q  ->  T = en
        let mut c = d.cells[0].clone();
        c.storage = Storage::Dff;
        c.data = Sop(vec![vec![lit("q2", true, false)]]);
        c.ce = Some(Sop(vec![vec![(Signal::Pad("en".into()), true)]]));
        d.cells[0] = c;
        assert_eq!(recover_toggles(&mut d, 10), 1);
        assert_eq!(d.cells[0].data, Sop(vec![vec![(Signal::Pad("en".into()), true)]]));
        assert!(d.cells[0].ce.is_none());
    }

    fn cell(name: &str, data: Sop) -> Cell {
        Cell {
            name: name.into(),
            is_output: false,
            pin: None,
            node: None,
            storage: Storage::Comb,
            data,
            active_low: false,
            clk: None,
            ce: None,
            ar: None,
            ap: None,
            oe: None,
        }
    }

    #[test]
    fn collapse_keeps_nodes_beyond_the_support_bound() {
        // n = a0 & ... & a29 (30 inputs), y = n & b0 & ... & b9: inlining would need 40 inputs
        let n: Vec<(Signal, bool)> = (0..30).map(|i| (Signal::Pad(format!("a{i}")), true)).collect();
        let mut y: Vec<(Signal, bool)> = (0..10).map(|i| (Signal::Pad(format!("b{i}")), true)).collect();
        y.push((Signal::Fb("n".into(), false), true));
        let mut d = Design::default();
        d.cells.push(cell("n", Sop(vec![n])));
        let mut out = cell("y", Sop(vec![y]));
        out.is_output = true;
        d.cells.push(out);
        collapse_limited(&mut d, 10, true);
        assert!(d.cell("n").is_some(), "the 30-input node must stay a buried node");
        assert!(d.cell("y").unwrap().inputs().len() <= MAX_SUPPORT);
    }
}
