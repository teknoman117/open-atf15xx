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

fn complement(c: &[Cube], n: usize) -> Vec<Cube> {
    if c.is_empty() {
        return vec![Cube { care: 0, val: 0 }];
    }
    if c.iter().any(|x| x.care == 0) {
        return vec![];
    }
    if c.len() == 1 {
        let x = c[0];
        return (0..n)
            .filter(|i| x.care >> i & 1 == 1)
            .map(|i| Cube { care: 1 << i, val: !x.val & (1 << i) })
            .collect();
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
        for mut x in complement(&cofactor(c, i, v), n) {
            x.care |= b;
            x.val = (x.val & !b) | set;
            out.push(x);
        }
    }
    out
}

/// Repeatedly merge distance-1 cubes and drop contained cubes.
fn merge(mut c: Vec<Cube>) -> Vec<Cube> {
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

