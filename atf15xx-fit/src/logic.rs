//! Sum-of-products logic over named signals, with a small two-level minimiser
//! (unate-recursive complement, cube merging, single-cube containment, irredundant cover).

use std::collections::BTreeMap;

/// A literal reference: what the product-term array can see through the UIM.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Signal {
    /// Pad (input buffer) of a pin: an input pin, or `X.PIN` of an output.
    Pad(String),
    /// Macrocell feedback of cell `X`; `true` = register Q (`X.Q`), `false` = combinatorial.
    Fb(String, bool),
    /// Foldback expander output (a NAND product term shared inside one block), by key.
    Fold(String),
}

impl Signal {
    pub fn name(&self) -> &str {
        match self {
            Signal::Pad(n) | Signal::Fb(n, _) | Signal::Fold(n) => n,
        }
    }
}

impl std::fmt::Display for Signal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Signal::Pad(n) => write!(f, "{n}"),
            Signal::Fb(n, true) => write!(f, "{n}.Q"),
            Signal::Fb(n, false) => write!(f, "{n}"),
            Signal::Fold(n) => write!(f, "FOLD({n})"),
        }
    }
}

/// A product term: conjunction of literals (sorted, no duplicates). Empty = constant 1.
pub type Product = Vec<(Signal, bool)>;

/// A sum of products. Empty = constant 0.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Sop(pub Vec<Product>);

/// CUPL-style text: `a & !b # c` (`0` / `1` for the constants).
impl std::fmt::Display for Sop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            return f.write_str("0");
        }
        for (i, p) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" # ")?;
            }
            if p.is_empty() {
                f.write_str("1")?;
            }
            for (j, (s, pos)) in p.iter().enumerate() {
                write!(f, "{}{}{}", if j > 0 { " & " } else { "" }, if *pos { "" } else { "!" }, s)?;
            }
        }
        Ok(())
    }
}

impl Sop {
    pub fn zero() -> Sop {
        Sop(vec![])
    }
    pub fn one() -> Sop {
        Sop(vec![vec![]])
    }
    pub fn is_zero(&self) -> bool {
        self.0.is_empty()
    }
    pub fn is_one(&self) -> bool {
        self.0.iter().any(|p| p.is_empty())
    }
    /// Single positive literal?
    pub fn single_literal(&self) -> Option<(&Signal, bool)> {
        match self.0.as_slice() {
            [p] if p.len() == 1 => Some((&p[0].0, p[0].1)),
            _ => None,
        }
    }
    pub fn signals(&self) -> Vec<Signal> {
        let mut v: Vec<Signal> = self.0.iter().flatten().map(|(s, _)| s.clone()).collect();
        v.sort();
        v.dedup();
        v
    }
    pub fn push(&mut self, mut p: Product) {
        p.sort();
        p.dedup();
        // contradictory product (x & !x) is constant 0
        if p.windows(2).any(|w| w[0].0 == w[1].0) {
            return;
        }
        self.0.push(p);
    }

    /// Substitute signal `s` (both polarities) by `f` / `!f` (`fnot`).
    pub fn substitute(&self, s: &Signal, f: &Sop, fnot: &Sop) -> Sop {
        let mut out = Sop::zero();
        for p in &self.0 {
            let mut acc = Sop::one();
            let mut rest: Product = vec![];
            for (sig, pos) in p {
                if sig == s {
                    acc = acc.and(if *pos { f } else { fnot });
                } else {
                    rest.push((sig.clone(), *pos));
                }
            }
            for mut q in acc.0 {
                q.extend(rest.iter().cloned());
                out.push(q);
            }
        }
        out
    }

    pub fn and(&self, o: &Sop) -> Sop {
        let mut out = Sop::zero();
        for a in &self.0 {
            for b in &o.0 {
                let mut p = a.clone();
                p.extend(b.iter().cloned());
                out.push(p);
            }
        }
        out
    }

    pub fn or(&self, o: &Sop) -> Sop {
        let mut out = self.clone();
        for p in &o.0 {
            out.push(p.clone());
        }
        out
    }

    /// Minimised cover of this function (memoised per thread, spec F9.11).
    pub fn minimize(&self) -> Sop {
        memo(&MINIMIZE, self, || {
            let (vars, cover) = self.encode();
            match cover {
                None => self.clone(),
                Some(c) => decode(&vars, &irredundant(&merge(c))),
            }
        })
    }

    /// Minimised cover of the complement. Functions of more than 64 signals (which no macrocell
    /// can implement: a block sees 40 UIM inputs) are complemented symbolically by Shannon
    /// expansion, without minimisation; that is correct but can be large, so callers that only
    /// need a cost should use [`Sop::try_complement`].
    pub fn complement(&self) -> Sop {
        memo(&COMPLEMENT, self, || {
            let (vars, cover) = self.encode();
            match cover {
                None => complement_wide(self, usize::MAX).expect("unbounded"),
                Some(c) => decode(&vars, &irredundant(&merge(complement(&c, vars.len())))),
            }
        })
    }

    /// The complement, or None when computing it would pass `max_cubes` intermediate cubes: for
    /// callers that only ask whether the complement is *cheaper* (a function whose complement
    /// needs that many cubes is not). Within [`PLAIN_COMPLEMENT_LIMIT`] the result is exactly
    /// [`Sop::complement`]'s; functions of more than 64 signals return None.
    pub fn complement_within(&self, max_cubes: usize) -> Option<Sop> {
        // memoised per thread like the others: the fitter asks again in every attempt
        thread_local! {
            static WITHIN: std::cell::RefCell<rustc_hash::FxHashMap<(Sop, usize), Option<Sop>>> = Default::default();
        }
        let key = (self.clone(), max_cubes);
        if let Some(r) = WITHIN.with(|m| m.borrow().get(&key).cloned()) {
            return r;
        }
        let r = (|| {
            let (vars, cover) = self.encode();
            let c = cover?;
            let mut budget = PLAIN_COMPLEMENT_LIMIT;
            if complement_plain(&c, vars.len(), &mut budget).is_some() {
                return Some(self.complement());
            }
            let raw = complement_folded_within(&c, vars.len(), max_cubes)?;
            Some(decode(&vars, &irredundant(&merge(raw))))
        })();
        WITHIN.with(|m| {
            let mut m = m.borrow_mut();
            if m.len() >= MEMO_MAX {
                m.clear();
            }
            m.insert(key, r.clone());
        });
        r
    }

    /// The complement if it has at most `max_products` products (None otherwise). Wide
    /// functions (more than 64 signals) are complemented symbolically within the same bound.
    pub fn try_complement(&self, max_products: usize) -> Option<Sop> {
        if self.support() > 64 {
            return complement_wide(self, max_products);
        }
        let c = self.complement();
        (c.0.len() <= max_products).then_some(c)
    }

    /// Number of variables in the support.
    pub fn support(&self) -> usize {
        self.signals().len()
    }

    fn encode(&self) -> (Vec<Signal>, Option<Vec<Cube>>) {
        let vars = self.signals();
        if vars.len() > 64 {
            return (vars, None);
        }
        let idx: BTreeMap<&Signal, usize> = vars.iter().enumerate().map(|(i, s)| (s, i)).collect();
        let cover = self
            .0
            .iter()
            .map(|p| {
                let mut c = Cube { care: 0, val: 0 };
                for (s, pos) in p {
                    let b = 1u64 << idx[s];
                    c.care |= b;
                    if *pos {
                        c.val |= b;
                    }
                }
                c
            })
            .collect();
        (vars, Some(cover))
    }
}

/// Complement by Shannon expansion on the most frequent signal (any number of signals), with
/// single-cube containment; None when the result would exceed `max` products.
fn complement_wide(f: &Sop, max: usize) -> Option<Sop> {
    if f.is_zero() {
        return Some(Sop::one());
    }
    if f.is_one() {
        return Some(Sop::zero());
    }
    let mut count: BTreeMap<&Signal, usize> = BTreeMap::new();
    for p in &f.0 {
        for (s, _) in p {
            *count.entry(s).or_default() += 1;
        }
    }
    let v = count.iter().max_by_key(|(_, n)| **n).map(|(s, _)| (*s).clone()).unwrap();
    let (one, zero) = (Sop::one(), Sop::zero());
    let f1 = f.substitute(&v, &one, &zero);
    let f0 = f.substitute(&v, &zero, &one);
    let c1 = complement_wide(&absorb(f1), max)?;
    let c0 = complement_wide(&absorb(f0), max)?;
    let mut out = Sop(vec![vec![(v.clone(), true)]]).and(&c1).or(&Sop(vec![vec![(v, false)]]).and(&c0));
    out = absorb(out);
    (out.0.len() <= max).then_some(out)
}

/// Remove products that contain another product (single-cube containment).
fn absorb(f: Sop) -> Sop {
    let mut ps: Vec<Product> = f.0;
    ps.sort_by_key(|p| p.len());
    ps.dedup();
    let mut keep: Vec<Product> = vec![];
    for p in ps {
        if !keep.iter().any(|q| q.iter().all(|l| p.contains(l))) {
            keep.push(p);
        }
    }
    Sop(keep)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Cube {
    care: u64,
    val: u64,
}

impl Cube {
    fn contains(&self, o: &Cube) -> bool {
        // every point of o is in self
        self.care & !o.care == 0 && (self.val ^ o.val) & self.care == 0
    }
    fn lits(&self) -> u32 {
        self.care.count_ones()
    }
}

fn decode(vars: &[Signal], c: &[Cube]) -> Sop {
    let mut s = Sop::zero();
    for cube in c {
        let mut p = vec![];
        for (i, v) in vars.iter().enumerate() {
            if cube.care >> i & 1 == 1 {
                p.push((v.clone(), cube.val >> i & 1 == 1));
            }
        }
        s.push(p);
    }
    s
}

fn tautology(c: &[Cube], _n: usize) -> bool {
    if c.iter().any(|x| x.care == 0) {
        return true;
    }
    if c.is_empty() {
        return false;
    }
    // variables in both polarities (binate), from whole-word masks: a cover that is unate in
    // every variable and has no universal cube is not a tautology
    let (mut pos, mut neg) = (0u64, 0u64);
    for x in c {
        pos |= x.care & x.val;
        neg |= x.care & !x.val;
    }
    let binate = pos & neg;
    if binate == 0 {
        return false;
    }
    // the most binate variable (largest min(p, q), then p + q; the lowest index on ties)
    let mut best: Option<(usize, (u32, u32))> = None;
    let mut m = binate;
    while m != 0 {
        let i = m.trailing_zeros() as usize;
        m &= m - 1;
        let b = 1u64 << i;
        let (mut p, mut q) = (0u32, 0u32);
        for x in c {
            if x.care & b != 0 {
                if x.val & b != 0 {
                    p += 1
                } else {
                    q += 1
                }
            }
        }
        let score = (p.min(q), p + q);
        if best.map_or(true, |(_, s)| score > s) {
            best = Some((i, score));
        }
    }
    let (i, _) = best.expect("binate variable");
    tautology(&cofactor(c, i, true), 64) && tautology(&cofactor(c, i, false), 64)
}

fn cofactor(c: &[Cube], i: usize, v: bool) -> Vec<Cube> {
    let b = 1u64 << i;
    c.iter()
        .filter(|x| x.care & b == 0 || ((x.val & b != 0) == v))
        .map(|x| Cube { care: x.care & !b, val: x.val & !b })
        .collect()
}

/// The complement as a cube list (merged and made irredundant by the caller). The plain Shannon
/// expansion ([`complement_plain`]) is tried first, as it always was, so every result it gives
/// within [`PLAIN_COMPLEMENT_LIMIT`] cubes is unchanged; beyond that (XOR-rich functions:
/// comparators, parity, where its output grows exponentially and its minimisation took
/// minutes) [`complement_folded`] merges the two halves at every split instead.
fn complement(c: &[Cube], n: usize) -> Vec<Cube> {
    let mut budget = PLAIN_COMPLEMENT_LIMIT;
    match complement_plain(c, n, &mut budget) {
        Some(v) => v,
        None => complement_folded(c, n),
    }
}

/// Raw cubes the plain complement may produce before [`complement_folded`] takes over.
const PLAIN_COMPLEMENT_LIMIT: usize = 4096;

/// The splitting variable: the one most cubes depend on (the first such).
fn split_var(c: &[Cube], n: usize) -> usize {
    let mut best = (0, 0usize);
    for i in 0..n {
        let b = 1u64 << i;
        let k = c.iter().filter(|x| x.care & b != 0).count();
        if k > best.1 {
            best = (i, k);
        }
    }
    best.0
}

/// Shannon complement with the halves recombined: a cube in both halves loses the split
/// variable, and so does a cube of one half contained in a cube of the other
/// (`x·p + !x·q` with `p ⊆ q` covers `p`). The result is a cover of the complement, much smaller
/// than the plain expansion's on XOR-rich functions.
fn complement_folded(c: &[Cube], n: usize) -> Vec<Cube> {
    complement_folded_within(c, n, usize::MAX).expect("unbounded")
}

/// [`complement_folded`], `None` as soon as one level's result exceeds `max_cubes` cubes.
fn complement_folded_within(c: &[Cube], n: usize, max_cubes: usize) -> Option<Vec<Cube>> {
    if c.is_empty() {
        return Some(vec![Cube { care: 0, val: 0 }]);
    }
    if c.iter().any(|x| x.care == 0) {
        return Some(vec![]);
    }
    if c.len() == 1 {
        let x = c[0];
        return Some((0..n).filter(|i| x.care >> i & 1 == 1).map(|i| Cube { care: 1 << i, val: !x.val & (1 << i) }).collect());
    }
    let i = split_var(c, n);
    let b = 1u64 << i;
    let r1 = complement_folded_within(&cofactor(c, i, true), n, max_cubes)?;
    let r0 = complement_folded_within(&cofactor(c, i, false), n, max_cubes)?;
    if r1.len() + r0.len() > max_cubes {
        return None;
    }
    let in0: std::collections::BTreeSet<Cube> = r0.iter().copied().collect();
    let in1: std::collections::BTreeSet<Cube> = r1.iter().copied().collect();
    let mut out: Vec<Cube> = vec![];
    for p in &r1 {
        if in0.contains(p) || r0.iter().any(|q| q.contains(p)) {
            out.push(*p);
        } else {
            out.push(Cube { care: p.care | b, val: p.val | b });
        }
    }
    for q in &r0 {
        if in1.contains(q) {
            continue; // already emitted without the literal
        }
        if r1.iter().any(|p| p.contains(q)) {
            out.push(*q);
        } else {
            out.push(Cube { care: q.care | b, val: q.val & !b });
        }
    }
    out.sort();
    out.dedup();
    let snap = out.clone();
    out.retain(|x| !snap.iter().any(|y| y != x && y.contains(x)));
    Some(out)
}

/// The plain Shannon complement; `None` once it has produced more than `budget` cubes.
fn complement_plain(c: &[Cube], n: usize, budget: &mut usize) -> Option<Vec<Cube>> {
    if c.is_empty() {
        return take(budget, vec![Cube { care: 0, val: 0 }]);
    }
    if c.iter().any(|x| x.care == 0) {
        return Some(vec![]);
    }
    if c.len() == 1 {
        let x = c[0];
        return take(
            budget,
            (0..n).filter(|i| x.care >> i & 1 == 1).map(|i| Cube { care: 1 << i, val: !x.val & (1 << i) }).collect(),
        );
    }
    let mut best = (0, 0usize);
    for i in 0..n {
        let b = 1u64 << i;
        let k = c.iter().filter(|x| x.care & b != 0).count();
        if k > best.1 {
            best = (i, k);
        }
    }
    let i = best.0;
    let b = 1u64 << i;
    let mut out = vec![];
    for (v, set) in [(true, b), (false, 0)] {
        for mut x in complement_plain(&cofactor(c, i, v), n, budget)? {
            x.care |= b;
            x.val = (x.val & !b) | set;
            out.push(x);
        }
    }
    Some(out)
}

/// Charge `v`'s cubes to the budget (`None` when it runs out).
fn take(budget: &mut usize, v: Vec<Cube>) -> Option<Vec<Cube>> {
    *budget = budget.checked_sub(v.len())?;
    Some(v)
}

/// Repeatedly merge distance-1 cubes and drop contained cubes; expand when nothing else changes.
///
/// The result is exactly that of the plain algorithm ([`merge_general`], also the reference in
/// the tests): each
/// pass merges the *first* mergeable pair of the sorted cover (smallest cube, then its smallest
/// partner), then drops every cube contained in another, and only a pass that changes nothing
/// expands literals. The plain version searched all pairs and all containments after every merge
/// (about n³ for n cubes: minutes on a 12-bit comparator's covers); here the first pair is found
/// by looking up each cube's distance-1 neighbours in the sorted set, and once the cover is
/// containment-free only the new cube is checked against the others.
fn merge(c: Vec<Cube>) -> Vec<Cube> {
    // the lookups need cubes without stray bits outside their care set
    if c.iter().any(|x| x.val & !x.care != 0) {
        return merge_general(c);
    }
    let mut set: std::collections::BTreeSet<Cube> = c.into_iter().collect();
    // is the cover containment-free?
    let mut clean = false;
    let drop_contained = |set: &mut std::collections::BTreeSet<Cube>| -> bool {
        let snap: Vec<Cube> = set.iter().copied().collect();
        let before = set.len();
        set.retain(|x| !snap.iter().any(|y| y != x && y.contains(x)));
        set.len() != before
    };
    // a cube's smallest partner above it: same care set, one more literal positive
    let partner_above = |set: &std::collections::BTreeSet<Cube>, x: &Cube| -> Option<Cube> {
        let mut best: Option<u64> = None;
        let mut bits = x.care & !x.val;
        while bits != 0 {
            let b = bits & bits.wrapping_neg();
            bits &= bits - 1;
            let v = x.val | b;
            if best.map_or(true, |w| v < w) && set.contains(&Cube { care: x.care, val: v }) {
                best = Some(v);
            }
        }
        best.map(|v| Cube { care: x.care, val: v })
    };
    // the search state between merges: no cube below `frontier` has a partner above it, except
    // possibly those in `dirty` (partners of cubes created since, and created cubes below it)
    let mut frontier: Option<Cube> = None;
    let mut dirty: std::collections::BTreeSet<Cube> = std::collections::BTreeSet::new();
    loop {
        let mut changed = false;
        // the first cube (sorted order) with a partner of the same care set differing in one
        // literal, and its smallest such partner (a partner is larger: a smaller one would have
        // been found first)
        let mut from_dirty = None;
        for y in dirty.clone() {
            dirty.remove(&y);
            if !set.contains(&y) {
                continue;
            }
            if let Some(p) = partner_above(&set, &y) {
                from_dirty = Some((y, p));
                dirty.insert(y);
                break;
            }
        }
        let scan_start = frontier.unwrap_or(Cube { care: 0, val: 0 });
        let limit = from_dirty.map(|(y, _)| y);
        let mut from_scan = None;
        for x in set.range(scan_start..) {
            if limit.is_some_and(|l| *x >= l) {
                break;
            }
            if let Some(p) = partner_above(&set, x) {
                from_scan = Some((*x, p));
                break;
            }
            frontier = Some(*x);
        }
        let pair = match (from_dirty, from_scan) {
            (Some(a), Some(b)) => Some(if b.0 < a.0 { b } else { a }),
            (a, b) => a.or(b),
        };
        if let Some((a, b)) = pair {
            if frontier.map_or(true, |f| a > f) {
                frontier = Some(a);
            }
            dirty.remove(&a);
            let d = (a.val ^ b.val) & a.care;
            let m = Cube { care: a.care & !d, val: a.val & !d };
            set.remove(&a);
            set.remove(&b);
            set.insert(m);
            // the new cube and its partners below the frontier may now pair
            let below = |x: &Cube, f: &Option<Cube>| f.is_some_and(|f| *x < f);
            if below(&m, &frontier) {
                dirty.insert(m);
            }
            let mut bits = m.care;
            while bits != 0 {
                let bit = bits & bits.wrapping_neg();
                bits &= bits - 1;
                let n = Cube { care: m.care, val: m.val ^ bit };
                if below(&n, &frontier) && set.contains(&n) {
                    dirty.insert(n);
                }
            }
            changed = true;
            if clean {
                // only the new cube can contain others or be contained
                let victims: Vec<Cube> = set.iter().filter(|x| **x != m && m.contains(x)).copied().collect();
                let covered = set.iter().any(|y| *y != m && y.contains(&m));
                for v in victims {
                    set.remove(&v);
                }
                if covered {
                    set.remove(&m);
                }
            } else {
                drop_contained(&mut set);
                clean = true;
            }
        } else if !clean {
            changed = drop_contained(&mut set);
            clean = true;
        }
        if !changed {
            // expand: drop literals while the cube stays inside the function
            let mut c: Vec<Cube> = set.iter().copied().collect();
            let f = c.clone();
            for k in 0..c.len() {
                for i in 0..64 {
                    let b = 1u64 << i;
                    if c[k].care & b == 0 {
                        continue;
                    }
                    let e = Cube { care: c[k].care & !b, val: c[k].val & !b };
                    if cube_in_cover(&e, &f) {
                        c[k] = e;
                        changed = true;
                    }
                }
            }
            if !changed {
                return c;
            }
            set = c.into_iter().collect();
            clean = false;
            frontier = None;
            dirty.clear();
        }
    }
}

/// The plain algorithm: [`merge`] for covers with bits outside a cube's care set (which this
/// module does not produce), and the reference [`merge`] is tested against.
fn merge_general(mut c: Vec<Cube>) -> Vec<Cube> {
    loop {
        c.sort();
        c.dedup();
        let mut changed = false;
        'outer: for i in 0..c.len() {
            for j in i + 1..c.len() {
                let (a, b) = (c[i], c[j]);
                if a.care == b.care {
                    let d = (a.val ^ b.val) & a.care;
                    if d.count_ones() == 1 {
                        let m = Cube { care: a.care & !d, val: a.val & !d };
                        c[i] = m;
                        c.swap_remove(j);
                        changed = true;
                        break 'outer;
                    }
                }
            }
        }
        // containment
        let before = c.len();
        let snapshot = c.clone();
        c.retain(|x| !snapshot.iter().any(|y| y != x && y.contains(x)));
        if c.len() != before {
            changed = true;
        }
        // expand: drop literals while the cube stays inside the function
        if !changed {
            let f = c.clone();
            for k in 0..c.len() {
                for i in 0..64 {
                    let b = 1u64 << i;
                    if c[k].care & b == 0 {
                        continue;
                    }
                    let e = Cube { care: c[k].care & !b, val: c[k].val & !b };
                    if cube_in_cover(&e, &f) {
                        c[k] = e;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            return c;
        }
    }
}

fn cube_in_cover(x: &Cube, f: &[Cube]) -> bool {
    // f restricted to x is a tautology
    let mut cof: Vec<Cube> = vec![];
    for y in f {
        if (y.val ^ x.val) & y.care & x.care != 0 {
            continue;
        }
        cof.push(Cube { care: y.care & !x.care, val: y.val & !x.care });
    }
    tautology(&cof, 64)
}

/// Greedy irredundant cover: drop cubes covered by the rest (largest first).
fn irredundant(c: &[Cube]) -> Vec<Cube> {
    let mut v: Vec<Cube> = c.to_vec();
    v.sort_by_key(|x| std::cmp::Reverse(x.lits()));
    let mut i = 0;
    while i < v.len() {
        let rest: Vec<Cube> = v.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, x)| *x).collect();
        if cube_in_cover(&v[i], &rest) {
            v.remove(i);
        } else {
            i += 1;
        }
    }
    v.sort_by_key(|x| (x.lits(), x.care, x.val));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(n: &str) -> Signal {
        Signal::Pad(n.into())
    }

    /// The fast `merge` gives exactly the plain algorithm's cover (same cubes, same order) on
    /// random covers and their complements (the two inputs `Sop::minimize` and
    /// `Sop::complement` give it).
    /// The folded complement is exactly the complement: on every point of the space, a point is
    /// in it iff it is not in the cover (truth tables, random covers).
    #[test]
    fn folded_complement_is_exact() {
        use rand::{Rng, SeedableRng};
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(11);
        let hit = |c: &[Cube], m: u64| c.iter().any(|x| (m ^ x.val) & x.care == 0);
        for _ in 0..300 {
            let n = rng.random_range(1..=10usize);
            let cover: Vec<Cube> = (0..rng.random_range(0..=20usize))
                .map(|_| {
                    let care: u64 = rng.random::<u64>() & ((1u64 << n) - 1);
                    Cube { care, val: rng.random::<u64>() & care }
                })
                .collect();
            let f = complement_folded(&cover, n);
            for m in 0..(1u64 << n) {
                assert_ne!(hit(&cover, m), hit(&f, m), "point {m:b} of {cover:?} -> {f:?}");
            }
        }
        // XOR-rich: a == b over 8 bit pairs; the folded complement stays small
        let n = 16;
        let eq: Vec<Cube> = (0..256u64)
            .map(|v| {
                let mut val = 0;
                for i in 0..8 {
                    if v >> i & 1 == 1 {
                        val |= 3 << (2 * i);
                    }
                }
                Cube { care: (1 << n) - 1, val }
            })
            .collect();
        assert!(complement_folded(&eq, n).len() <= 64);
    }

    #[test]
    fn merge_equals_the_plain_algorithm() {
        use rand::{Rng, SeedableRng};
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(7);
        for round in 0..460 {
            // mostly small covers; the last 60 rounds wide ones (complements of thousands of cubes)
            let (n, cubes) = if round < 400 {
                (rng.random_range(2..=9usize), rng.random_range(1..=24usize))
            } else {
                (rng.random_range(10..=12usize), rng.random_range(8..=40usize))
            };
            let cover: Vec<Cube> = (0..cubes)
                .map(|_| {
                    let care: u64 = rng.random::<u64>() & ((1u64 << n) - 1);
                    Cube { care, val: rng.random::<u64>() & care }
                })
                .collect();
            for c in [cover.clone(), complement(&cover, n)] {
                assert_eq!(merge(c.clone()), merge_general(c.clone()), "round {round}: {c:?}");
            }
        }
    }
    #[test]
    fn complement_of_or_is_one_product() {
        let mut f = Sop::zero();
        f.push(vec![(s("A"), true)]);
        f.push(vec![(s("B"), true)]);
        let c = f.complement();
        assert_eq!(c.0, vec![vec![(s("A"), false), (s("B"), false)]]);
    }
    #[test]
    fn complement_of_sum3_has_two_terms() {
        let mut f = Sop::zero();
        f.push(vec![(s("A"), true), (s("B"), true)]);
        f.push(vec![(s("B"), false), (s("C"), true)]);
        f.push(vec![(s("A"), false), (s("C"), false)]);
        assert_eq!(f.complement().0.len(), 2);
    }
}

#[cfg(test)]
mod wide_tests {
    use super::*;

    #[test]
    fn complement_of_a_70_input_product_does_not_panic() {
        let p: Product = (0..70).map(|i| (Signal::Pad(format!("x{i}")), true)).collect();
        let f = Sop(vec![p]);
        let c = f.complement();
        assert_eq!(c.0.len(), 70);
        assert!(f.try_complement(10).is_none());
        // f & !f = 0 and the complement is exact on a few points: all ones -> f = 1, !f = 0
        assert!(f.and(&c).0.iter().all(|q| q.windows(2).any(|w| w[0].0 == w[1].0)) || f.and(&c).is_zero());
    }
}

// Memoised covers (spec F9.11): `minimize` and `complement` are pure, and the flow's passes and
// the fitter's planner ask for the same covers again and again (every strategy and fit attempt
// starts from the same netlist). One cache per thread (no locks for the parallel strategies),
// cleared when it grows past `MEMO_MAX` entries.
type Memo = std::cell::RefCell<(rustc_hash::FxHashMap<Sop, Sop>, u64, u64)>;
thread_local! {
    static MINIMIZE: Memo = std::cell::RefCell::new(Default::default());
    static COMPLEMENT: Memo = std::cell::RefCell::new(Default::default());
}
const MEMO_MAX: usize = 1 << 16;

fn memo(cache: &'static std::thread::LocalKey<Memo>, key: &Sop, f: impl FnOnce() -> Sop) -> Sop {
    if let Some(v) = cache.with(|c| {
        let mut c = c.borrow_mut();
        let hit = c.0.get(key).cloned();
        if hit.is_some() {
            c.1 += 1;
        } else {
            c.2 += 1;
        }
        hit
    }) {
        return v;
    }
    let v = f();
    cache.with(|c| {
        let mut c = c.borrow_mut();
        if c.0.len() >= MEMO_MAX {
            c.0.clear();
        }
        c.0.insert(key.clone(), v.clone());
    });
    v
}

/// (hits, misses) of this thread's `minimize` and `complement` caches.
pub fn memo_stats() -> ((u64, u64), (u64, u64)) {
    let s = |m: &'static std::thread::LocalKey<Memo>| m.with(|c| (c.borrow().1, c.borrow().2));
    (s(&MINIMIZE), s(&COMPLEMENT))
}

