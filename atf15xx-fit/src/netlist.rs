//! Technology-level netlist IR: the interface between front ends (`.tt2` today, Yosys BLIF/JSON
//! later) and the placer/router/assembler.
//!
//! A design is a set of *input pins* and *cells*. Each cell is one macrocell's worth of logic:
//! a sum-of-products data function, an optional storage element with its controls, and an
//! optional output pin with output enable.

use crate::logic::{Signal, Sop};
use crate::tt2::Pla;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storage {
    Comb,
    Dff,
    Tff,
    Latch,
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub name: String,
    /// Drives a package pin (declared in `#$ PINS`); `pin` is its constraint if any.
    pub is_output: bool,
    pub pin: Option<u16>,
    /// Buried-node constraint (`600+mc` feedback node, `300+mc` foldback node).
    pub node: Option<u16>,
    pub storage: Storage,
    /// Data function (combinatorial value, D, T or latch data), before output inversion.
    pub data: Sop,
    /// Output / register sense is inverted (`Y-`, `Y.D-` in the `.tt2`).
    pub active_low: bool,
    /// Clock (or latch enable).
    pub clk: Option<Sop>,
    pub ce: Option<Sop>,
    pub ar: Option<Sop>,
    pub ap: Option<Sop>,
    pub oe: Option<Sop>,
}

impl Cell {
    fn new(name: &str) -> Cell {
        Cell {
            name: name.to_string(),
            is_output: false,
            pin: None,
            node: None,
            storage: Storage::Comb,
            data: Sop::zero(),
            active_low: false,
            clk: None,
            ce: None,
            ar: None,
            ap: None,
            oe: None,
        }
    }
    pub fn registered(&self) -> bool {
        self.storage != Storage::Comb
    }
    /// All signals this cell's product terms read.
    pub fn inputs(&self) -> Vec<Signal> {
        let mut v = self.data.signals();
        for s in [&self.clk, &self.ce, &self.ar, &self.ap, &self.oe].into_iter().flatten() {
            v.extend(s.signals());
        }
        v.sort();
        v.dedup();
        v
    }
}

#[derive(Clone, Debug, Default)]
pub struct Design {
    pub name: String,
    /// Input-only signals: name -> pin constraint.
    pub inputs: BTreeMap<String, Option<u16>>,
    /// Declaration order of input pins (placement order of floating ones).
    pub input_order: Vec<String>,
    pub cells: Vec<Cell>,
    /// Foldback signals introduced at netlist level ([`Design::nodes_to_foldbacks`]): key ->
    /// the product whose NAND it is (literals may be other foldbacks). The fitter hosts each in
    /// every block that reads it.
    pub folds: BTreeMap<String, Vec<(Signal, bool)>>,
    /// XOR decompositions found by [`Design::find_xor_splits`]: cell -> split. The cell's `data`
    /// stays the full function; the fitter implements it as `g ⊕ p` (PT2 to the XOR).
    pub xor: BTreeMap<String, XorSplit>,
}

/// A cell's data function as `g ⊕ p`, with `p` one product (the macrocell's PT2 in XOR mode,
/// spec F2.8 row 6: `Pt2Mux` = `Invert` = 1 gives `x = sum ⊕ pt2`).
#[derive(Clone, Debug)]
pub struct XorSplit {
    pub p: Vec<(Signal, bool)>,
    /// Covers of `g` (for the function) and of `!g` (for an active-low cell: `!f = !g ⊕ p`).
    pub g: Sop,
    pub gbar: Sop,
}

impl Design {
    /// Find XOR decompositions (spec F4.6b). With `pattern`: the vendor's `-XOR_Synthesis`, a
    /// function that is the XOR/XNOR of two literals (`A&!B # !A&B` -> PT1 = A, PT2 = B). With `general`: for
    /// any cell of 3 or more products (support up to 12), the product `p` (a literal, or a
    /// product of the cover of the function or its complement) that minimises `1 + |g|`,
    /// `g = f ⊕ p`, kept when that beats the best plain cover. Constant, fast-input and
    /// foldback-reading cells are left alone.
    pub fn find_xor_splits(&mut self, pattern: bool, general: bool) {
        let mut out = BTreeMap::new();
        for c in &self.cells {
            if c.data.is_zero() || c.data.is_one() || c.data.signals().iter().any(|s| matches!(s, Signal::Fold(_))) {
                continue;
            }
            if let Some(x) = two_literal_xor(&c.data).filter(|_| pattern) {
                if c.storage == Storage::Comb && !c.active_low {
                    out.insert(c.name.clone(), x);
                    continue;
                }
            }
            if !general || c.data.support() > 12 {
                continue;
            }
            let f = &c.data;
            let fbar = f.complement();
            let target_len = |x: &XorSplit| if c.active_low { x.gbar.0.len() } else { x.g.0.len() };
            let plain = f.0.len().min(fbar.0.len().max(1));
            if plain < 3 {
                continue;
            }
            let mut cands: Vec<Vec<(Signal, bool)>> = vec![];
            for s in f.signals() {
                cands.push(vec![(s.clone(), true)]);
            }
            for p in f.0.iter().chain(fbar.0.iter()).take(24) {
                if p.len() > 1 && !cands.contains(p) {
                    cands.push(p.clone());
                }
            }
            let mut best: Option<(usize, XorSplit)> = None;
            for p in cands {
                let ps = Sop(vec![p.clone()]);
                let pbar = ps.complement();
                let g = f.and(&pbar).or(&fbar.and(&ps)).minimize();
                if g.is_zero() || g.is_one() {
                    continue;
                }
                let gbar = g.complement();
                let x = XorSplit { p, g, gbar };
                let cost = 1 + target_len(&x);
                if cost < plain && best.as_ref().map_or(true, |(b, _)| cost < *b) {
                    best = Some((cost, x));
                }
            }
            if let Some((_, x)) = best {
                out.insert(c.name.clone(), x);
            }
        }
        self.xor = out;
    }

    pub fn cell(&self, name: &str) -> Option<&Cell> {
        self.cells.iter().find(|c| c.name == name)
    }
    pub fn cell_index(&self, name: &str) -> Option<usize> {
        self.cells.iter().position(|c| c.name == name)
    }

    /// Build the netlist from a `.tt2` PLA.
    pub fn from_pla(p: &Pla) -> Result<Design, String> {
        let mut d = Design { name: p.title.clone(), ..Default::default() };
        let pin_of: BTreeMap<&str, Option<u16>> = p.pins.iter().map(|(n, _, pin)| (n.as_str(), *pin)).collect();
        let node_of: BTreeMap<&str, Option<u16>> = p.nodes.iter().map(|(n, _, num)| (n.as_str(), *num)).collect();
        // outputs: base name + suffix
        let mut cells: BTreeMap<String, Cell> = BTreeMap::new();
        let mut order: Vec<String> = vec![];
        let split = |o: &str| -> (String, String, bool) {
            let (o, low) = match o.strip_suffix('-') {
                Some(s) => (s, true),
                None => (o, false),
            };
            match o.rsplit_once('.') {
                Some((b, s)) => (b.to_string(), s.to_ascii_uppercase(), low),
                None => (o.to_string(), String::new(), low),
            }
        };
        for o in &p.outputs {
            let (b, _, _) = split(o);
            if !cells.contains_key(&b) {
                let mut c = Cell::new(&b);
                if let Some(pin) = pin_of.get(b.as_str()) {
                    c.is_output = true;
                    c.pin = *pin;
                } else if let Some(n) = node_of.get(b.as_str()) {
                    c.node = *n;
                } else {
                    return Err(format!("output {b} is neither a pin nor a node"));
                }
                order.push(b.clone());
                cells.insert(b.clone(), c);
            }
        }
        // input literals
        let sig_of = |label: &str| -> Signal {
            if let Some(b) = label.strip_suffix(".Q") {
                Signal::Fb(b.to_string(), true)
            } else if let Some(b) = label.strip_suffix(".PIN") {
                Signal::Pad(b.to_string())
            } else if cells.contains_key(label) {
                Signal::Fb(label.to_string(), false)
            } else {
                Signal::Pad(label.to_string())
            }
        };
        let ins: Vec<Signal> = p.inputs.iter().map(|l| sig_of(l)).collect();
        let mut funcs: Vec<Sop> = vec![Sop::zero(); p.outputs.len()];
        for (cube, outs) in &p.cubes {
            let prod: Vec<(Signal, bool)> =
                cube.iter().enumerate().filter_map(|(i, v)| v.map(|v| (ins[i].clone(), v))).collect();
            for &o in outs {
                funcs[o].push(prod.clone());
            }
        }
        for (i, o) in p.outputs.iter().enumerate() {
            let (b, suf, low) = split(o);
            let c = cells.get_mut(&b).unwrap();
            let f = funcs[i].clone();
            match suf.as_str() {
                "" => {
                    c.data = f;
                    c.active_low = low;
                }
                "D" | "REG" => {
                    c.storage = Storage::Dff;
                    c.data = f;
                    c.active_low = low;
                }
                "T" => {
                    c.storage = Storage::Tff;
                    c.data = f;
                    c.active_low = low;
                }
                "L" => {
                    c.storage = Storage::Latch;
                    c.data = f;
                    c.active_low = low;
                }
                "C" | "CK" | "LE" => c.clk = Some(f),
                "CE" => c.ce = Some(f),
                "AR" => c.ar = Some(f),
                "AP" => c.ap = Some(f),
                // the vendor treats an OE with an empty cover (CUPL `Y.OE = 'b'0`) as "always enabled"
                "OE" => c.oe = if f.is_zero() { None } else { Some(f) },
                other => return Err(format!("unsupported output suffix .{other} on {b}")),
            }
        }
        d.cells = order.into_iter().map(|n| cells.remove(&n).unwrap()).collect();
        // `X.PIN` of an always-enabled combinatorial output whose logic is a single literal is
        // replaced by that logic (the vendor fitter does this)
        let subst: Vec<(String, Sop)> = d
            .cells
            .iter()
            .filter(|c| c.is_output && c.storage == Storage::Comb && c.oe.is_none() && !c.active_low && c.data.0.len() == 1 && c.data.0[0].len() == 1)
            .map(|c| (c.name.clone(), c.data.clone()))
            .collect();
        for (n, f) in subst {
            let s = Signal::Pad(n.clone());
            let fnot = f.complement();
            for c in d.cells.iter_mut().filter(|c| c.name != n) {
                if c.inputs().contains(&s) {
                    c.data = c.data.substitute(&s, &f, &fnot);
                }
            }
        }
        // input pins: declared pins that are not cells and are read by some product term
        let mut used: Vec<String> = vec![];
        for c in &d.cells {
            for sig in c.inputs() {
                if let Signal::Pad(n) = sig {
                    if d.cell_index(&n).is_none() && !used.contains(&n) {
                        used.push(n);
                    }
                }
            }
        }
        for (n, _, pin) in &p.pins {
            if d.cell_index(n).is_none() && used.contains(n) {
                d.inputs.insert(n.clone(), *pin);
                d.input_order.push(n.clone());
            }
        }
        for n in used {
            if !d.inputs.contains_key(&n) {
                return Err(format!("signal {n} is read but neither a pin nor a node"));
            }
        }
        Ok(d)
    }

    /// Merge a buried register into the output pin that merely copies it (`Y = N.Q`), as the
    /// vendor fitter does.
    pub fn absorb_output_registers(&mut self) {
        loop {
            let mut found = None;
            for (yi, y) in self.cells.iter().enumerate() {
                if !y.is_output || y.registered() || y.active_low || y.oe.is_some() {
                    continue;
                }
                if let [p] = y.data.0.as_slice() {
                    if let [(Signal::Fb(n, true), true)] = p.as_slice() {
                        let Some(ni) = self.cell_index(n) else { continue };
                        let nc = &self.cells[ni];
                        let readers = self.cells.iter().filter(|c| c.inputs().iter().any(|s| s.name() == n)).count();
                        if !nc.is_output && nc.registered() && readers == 1 {
                            found = Some((yi, ni));
                            break;
                        }
                    }
                }
            }
            let Some((yi, ni)) = found else { return };
            let n = self.cells[ni].clone();
            let y = &mut self.cells[yi];
            let old = Signal::Fb(n.name.clone(), true);
            let new = Signal::Fb(y.name.clone(), true);
            y.storage = n.storage;
            y.data = n.data.clone();
            y.active_low = n.active_low;
            y.clk = n.clk.clone();
            y.ce = n.ce.clone();
            y.ar = n.ar.clone();
            y.ap = n.ap.clone();
            let yname = y.name.clone();
            self.cells.remove(ni);
            // self-references of the absorbed register now read the output's register
            for c in self.cells.iter_mut().filter(|c| c.name == yname) {
                let one = Sop(vec![vec![(new.clone(), true)]]);
                let zero = Sop(vec![vec![(new.clone(), false)]]);
                c.data = c.data.substitute(&old, &one, &zero);
            }
        }
    }

    /// Special functions (clock, CE, reset, preset, OE) get one product term (or a foldback
    /// expander). Move any that need more into a new buried combinatorial node and read its
    /// feedback instead.
    /// Replace shared combinatorial nodes of 2..=`max_products` products that feed special
    /// functions (clock, CE, AR, AP, OE) by foldback signals, as the vendor does (its `XXL_`
    /// nodes): `N = p1 + ... + pk = !(fold(p1) & ... & fold(pk))` is one outer foldback reading k
    /// inner ones, and `!N` is the product `fold(p1) & ... & fold(pk)`. A reader then keeps its
    /// remaining literals in one product (`CE = N & c0` -> one PT) instead of expanding N, and
    /// the node needs no macrocell and no UIM input (the fitter replicates the foldbacks in every
    /// reading block). Returns the number of nodes converted.
    pub fn nodes_to_foldbacks(&mut self, max_products: usize) -> usize {
        self.nodes_to_foldbacks_with(max_products, false)
    }

    /// [`Design::nodes_to_foldbacks`], optionally limited to one foldback level
    /// (`one_level`, spec F8.7): a node `N = p1 + … + pk` becomes inner foldbacks `fi = !pi`,
    /// and `!N = f1·…·fk` is one level, but `N` itself is the foldback of those foldbacks, two
    /// levels (2·tSEXP). With `one_level`, nodes that some reader uses positively, or whose
    /// products already read foldbacks, stay macrocell nodes (one UIM pass, tUIM + tLAD + tCOMB).
    pub fn nodes_to_foldbacks_with(&mut self, max_products: usize, one_level: bool) -> usize {
        let key_of = |p: &[(Signal, bool)]| p.iter().map(|(s, v)| format!("{}{}", if *v { "" } else { "!" }, s)).collect::<Vec<_>>().join("&");
        let mut n = 0;
        loop {
            let pick = self.cells.iter().position(|c| {
                if c.is_output || c.storage != Storage::Comb || c.oe.is_some() || c.active_low {
                    return false;
                }
                let m = c.data.minimize();
                if m.0.len() < 2 || m.0.len() > max_products || m.0.iter().any(|p| p.iter().any(|(s, v)| !v && matches!(s, Signal::Fold(_)))) {
                    return false;
                }
                let s = Signal::Fb(c.name.clone(), false);
                if one_level {
                    if m.0.iter().any(|p| p.iter().any(|(x, _)| matches!(x, Signal::Fold(_)))) {
                        return false;
                    }
                    let pos = (s.clone(), true);
                    let used_pos = self.cells.iter().any(|o| {
                        [Some(&o.data), o.clk.as_ref(), o.ce.as_ref(), o.ar.as_ref(), o.ap.as_ref(), o.oe.as_ref()]
                            .into_iter()
                            .flatten()
                            .any(|f| f.0.iter().any(|p| p.contains(&pos)))
                    }) || self.folds.values().any(|p| p.contains(&pos));
                    if used_pos {
                        return false;
                    }
                }
                self.cells.iter().any(|o| {
                    [&o.clk, &o.ce, &o.ar, &o.ap, &o.oe].into_iter().flatten().any(|f| f.signals().contains(&s))
                })
            });
            let Some(i) = pick else { return n };
            let c = self.cells.remove(i);
            let m = c.data.minimize();
            // !p for each product: a foldback, or for a single literal simply its complement
            // (both polarities of every signal come from the UIM)
            let inner: Vec<(Signal, bool)> = m
                .0
                .iter()
                .map(|p| match p.as_slice() {
                    [(s, v)] if !matches!(s, Signal::Fold(_)) => (s.clone(), !v),
                    _ => {
                        let k = key_of(p);
                        self.folds.insert(k.clone(), p.clone());
                        (Signal::Fold(k), true)
                    }
                })
                .collect();
            let outer = format!("nand({})", inner.iter().map(|(s, _)| s.to_string()).collect::<Vec<_>>().join(","));
            self.folds.insert(outer.clone(), inner.clone());
            let s = Signal::Fb(c.name.clone(), false);
            // earlier foldback products may read this node: N -> the outer foldback, !N -> the
            // inner foldbacks (a product stays a product)
            for (k, p) in self.folds.iter_mut() {
                if k == &outer {
                    continue;
                }
                if p.iter().any(|(x, _)| x == &s) {
                    let mut q: Vec<(Signal, bool)> = vec![];
                    for (x, v) in p.iter() {
                        if x != &s {
                            q.push((x.clone(), *v));
                        } else if *v {
                            q.push((Signal::Fold(outer.clone()), true));
                        } else {
                            q.extend(inner.iter().cloned());
                        }
                    }
                    q.sort();
                    q.dedup();
                    *p = q;
                }
            }
            let pos = Sop(vec![vec![(Signal::Fold(outer), true)]]);
            let neg = Sop(vec![inner]);
            for o in self.cells.iter_mut() {
                for f in [Some(&mut o.data), o.clk.as_mut(), o.ce.as_mut(), o.ar.as_mut(), o.ap.as_mut(), o.oe.as_mut()].into_iter().flatten() {
                    if f.signals().contains(&s) {
                        *f = f.substitute(&s, &pos, &neg).minimize();
                    }
                }
            }
            n += 1;
        }
    }

    pub fn legalize_special_functions(&mut self) {
        self.legalize_special_functions_with(false)
    }

    /// As [`Self::legalize_special_functions`]; with `fold_ok`, functions of up to 15 products
    /// stay in place for the fitter's two-level foldback (NAND of NANDs, `fit::one_product`).
    pub fn legalize_special_functions_with(&mut self, fold_ok: bool) {
        let mut extra: Vec<Cell> = vec![];
        // identical functions share one node (e.g. the clock enable of every bit of a shift
        // register): key = the minimised cover
        let mut shared: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
        for c in self.cells.iter_mut() {
            let name = c.name.clone();
            for (tag, slot) in [("C", &mut c.clk), ("CE", &mut c.ce), ("AR", &mut c.ar), ("AP", &mut c.ap), ("OE", &mut c.oe)] {
                let Some(f) = slot.as_ref() else { continue };
                if f.0.len() <= 1 || f.is_one() {
                    continue;
                }
                let m = f.minimize();
                // AR may additionally use one single-literal product (global clear)
                let lits = m.0.iter().filter(|p| p.len() == 1).count();
                let comp = m.complement();
                let neg_fold = comp.0.iter().flatten().any(|(s, p)| !p && matches!(s, Signal::Fold(_)));
                let fits = m.0.len() == 1
                    || (comp.0.len() == 1 && !neg_fold)
                    || (tag == "AR" && m.0.len() == 2 && lits >= 1)
                    || (fold_ok && m.0.len() <= 15);
                if fits {
                    *slot = Some(m);
                    continue;
                }
                let key = format!("{m:?}");
                let n = match shared.get(&key) {
                    Some(n) => n.clone(),
                    None => {
                        let n = format!("{name}_{tag}$");
                        let mut node = Cell::new(&n);
                        node.data = m;
                        extra.push(node);
                        shared.insert(key, n.clone());
                        n
                    }
                };
                *slot = Some(Sop(vec![vec![(Signal::Fb(n, false), true)]]));
            }
        }
        // these helper nodes must not be collapsed again: give them a marker node number 0
        for mut n in extra {
            n.node = Some(0);
            self.cells.push(n);
        }
    }

    /// Move all but the first few products of a cell's sum into a new buried node `<cell>_SUM$n`
    /// and OR in its feedback (used when the cascade neighbour is not free). Returns None if the
    /// cell cannot be split.
    pub fn split_sum(&mut self, name: &str) -> Option<()> {
        let i = self.cell_index(name)?;
        let c = &self.cells[i];
        let specials = [&c.clk, &c.ce, &c.ar, &c.ap, &c.oe].iter().filter(|x| x.is_some()).count();
        let keep = 5usize.saturating_sub(specials + 1).max(1);
        let target = if c.active_low { c.data.complement() } else { c.data.clone() };
        if target.0.len() <= keep {
            return None;
        }
        let n = (0..).map(|k| format!("{name}_SUM${k}")).find(|n| self.cell_index(n).is_none()).unwrap();
        let mut node = Cell::new(&n);
        node.data = Sop(target.0[keep..].to_vec());
        node.node = Some(0);
        let mut data = Sop(target.0[..keep].to_vec());
        data.push(vec![(Signal::Fb(n.clone(), false), true)]);
        let c = &mut self.cells[i];
        c.data = data;
        c.active_low = false;
        self.cells.push(node);
        Some(())
    }

    /// Collapse purely combinatorial buried nodes into their readers (what the vendor fitter does
    /// by default), unless they carry a node-number constraint.
    pub fn collapse_comb_nodes(&mut self) {
        // A node is kept (node number 0) when collapsing it would give a reader more distinct
        // inputs than a block's UIM delivers (40): that function could never be placed, and
        // complementing it explodes. Within that bound this is the vendor's full collapse.
        const MAX_SUPPORT: usize = 40;
        loop {
            // a node read by a foldback product can only be inlined there if each polarity the
            // products use is a single product (a product cannot absorb a sum)
            let fold_ok = |c: &Cell| {
                let refs: Vec<bool> = self
                    .folds
                    .values()
                    .flatten()
                    .filter_map(|(s, v)| match s {
                        Signal::Fb(m, false) if *m == c.name => Some(*v),
                        _ => None,
                    })
                    .collect();
                // (no foldback reads it: nothing to check, and its covers may be costly)
                if refs.is_empty() {
                    return true;
                }
                let (f, fnot) = if c.active_low { (c.data.complement(), c.data.minimize()) } else { (c.data.minimize(), c.data.complement()) };
                refs.iter().all(|&v| if v { f.0.len() == 1 } else { fnot.0.len() == 1 })
            };
            let Some(i) = self.cells.iter().position(|c| {
                !c.is_output && c.storage == Storage::Comb && c.node.is_none() && c.oe.is_none() && fold_ok(c)
            }) else {
                return;
            };
            let s = Signal::Fb(self.cells[i].name.clone(), false);
            let node_in: Vec<Signal> = self.cells[i].inputs();
            let too_wide = self.cells.iter().enumerate().any(|(j, o)| {
                if j == i {
                    return false;
                }
                let ins = o.inputs();
                if !ins.contains(&s) {
                    return false;
                }
                let mut u: Vec<&Signal> = ins.iter().filter(|x| **x != s).chain(node_in.iter()).collect();
                u.sort();
                u.dedup();
                u.len() > MAX_SUPPORT
            });
            if too_wide || node_in.len() > MAX_SUPPORT {
                self.cells[i].node = Some(0);
                continue;
            }
            let (f, fnot) = {
                let c = &self.cells[i];
                if c.active_low { (c.data.complement(), c.data.minimize()) } else { (c.data.minimize(), c.data.complement()) }
            };
            // nor when the readers' covers would explode (an XOR tree: 2^(n-1) products): no
            // macrocell with its cascades holds more than ~80 products, so such a collapse can
            // never be placed, and computing it hangs (16-input parity, 2026-11-25)
            const MAX_PRODUCTS: usize = 256;
            let explodes = self.cells.iter().enumerate().any(|(j, o)| {
                j != i
                    && [Some(&o.data), o.clk.as_ref(), o.ce.as_ref(), o.ar.as_ref(), o.ap.as_ref(), o.oe.as_ref()].into_iter().flatten().any(|x| {
                        let est: usize = x
                            .0
                            .iter()
                            .map(|p| match p.iter().find(|(y, _)| y == &s) {
                                Some((_, true)) => f.0.len(),
                                Some((_, false)) => fnot.0.len(),
                                None => 1,
                            })
                            .sum();
                        est > MAX_PRODUCTS
                    })
            });
            if explodes {
                self.cells[i].node = Some(0);
                continue;
            }
            self.cells.remove(i);
            // inline into foldback products (single products, checked above)
            for p in self.folds.values_mut() {
                if p.iter().any(|(x, _)| x == &s) {
                    let mut q: Vec<(Signal, bool)> = vec![];
                    for (x, v) in p.iter() {
                        if x != &s {
                            q.push((x.clone(), *v));
                        } else {
                            q.extend((if *v { &f } else { &fnot }).0[0].iter().cloned());
                        }
                    }
                    q.sort();
                    q.dedup();
                    *p = q;
                }
            }
            for other in self.cells.iter_mut() {
                let sub = |x: &Sop| x.substitute(&s, &f, &fnot);
                other.data = sub(&other.data);
                for slot in [&mut other.clk, &mut other.ce, &mut other.ar, &mut other.ap, &mut other.oe] {
                    if let Some(x) = slot.as_mut() {
                        *x = sub(x);
                    }
                }
            }
        }
    }
}

/// `f` = XOR/XNOR of two literals (two products of the same two signals, both literals
/// complemented between them): `f = A ⊕ B ⊕ a ⊕ b ⊕ 1` for the first product `(A=a)(B=b)`,
/// split as `g = A`, `p = B` (positive unless the literals' polarities are equal).
fn two_literal_xor(f: &Sop) -> Option<XorSplit> {
    let [p, q] = f.0.as_slice() else { return None };
    let ([(a, pa), (b, pb)], [(a2, qa), (b2, qb)]) = (p.as_slice(), q.as_slice()) else { return None };
    if a != a2 || b != b2 || pa == qa || pb == qb {
        return None;
    }
    let g = Sop(vec![vec![(a.clone(), true)]]);
    let gbar = Sop(vec![vec![(a.clone(), false)]]);
    Some(XorSplit { p: vec![(b.clone(), pa != pb)], g, gbar })
}
