//! Word-level lowering (spec F9.9): Yosys stops at its coarse cells (`$eq`, `$lt`, `$add`,
//! `$reduce_xor`, `$pmux`, `$shiftx`, …) and this module lowers them into `$sop` cells with the
//! macrocell in mind, instead of Yosys's generic gate techmap:
//!
//! * XOR reductions become a tree of 4-input XORs: 8 products each, which XOR synthesis
//!   (F4.6b) turns into one macrocell (XOR3 in the sum, the fourth input on PT2);
//! * `==` / `<` share one `a_i ⊕ b_i` node per bit pair (common subexpressions across cells), and
//!   compare with constants are single products;
//! * `x + 1` carries are products of the lower bits (no ripple chain: toggle recovery and carry
//!   chains, F9.8, take it from there); general adders ripple with majority carries;
//! * variable shifts / `$shiftx` (`in[s]`) and `$pmux` become one product per case.
//!
//! The result is the fine-cell form the importer already reads (`$sop`, flip-flops, `$_TBUF_`),
//! so names, storage, tri-states and every later pass are unchanged. Cells this module does not
//! lower are left for Yosys's techmap ([`UNSUPPORTED`]); the Yosys plugin decides that per
//! design, the CLI's script lists them.

use crate::ynet::{self, Bit, Const, Netlist};
use std::collections::BTreeMap;

/// A bit: a net or a constant (`x`/`z` are taken as 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Lb {
    Net(u64),
    C(bool),
}

type Lit = (Lb, bool);
type Prod = Vec<Lit>;

/// Coarse cell types lowered here.
pub const LOWERED: &[&str] = &[
    "$not",
    "$pos",
    "$buf",
    "$and",
    "$or",
    "$xor",
    "$xnor",
    "$reduce_and",
    "$reduce_or",
    "$reduce_xor",
    "$reduce_xnor",
    "$reduce_bool",
    "$logic_not",
    "$logic_and",
    "$logic_or",
    "$eq",
    "$ne",
    "$eqx",
    "$nex",
    "$lt",
    "$le",
    "$gt",
    "$ge",
    "$mux",
    "$pmux",
    "$shiftx",
    "$add",
    "$sub",
    "$neg",
];

/// Coarse cell types left to Yosys's techmap (`techmap` + `simplemap`) before the netlist
/// reaches Rust. Shifts by a constant are wiring after `opt_expr`/`wreduce`; variable ones are
/// rare in CPLD designs.
pub const UNSUPPORTED: &[&str] = &[
    "$mul",
    "$div",
    "$mod",
    "$divfloor",
    "$modfloor",
    "$pow",
    "$shl",
    "$shr",
    "$sshl",
    "$sshr",
    "$shift",
    "$alu",
    "$macc",
    "$macc_v2",
    "$lcu",
    "$fa",
    "$bmux",
    "$demux",
    "$bweqx",
    "$bwmux",
    "$tribuf",
    "$lut",
    "$slice",
    "$concat",
    "$bweq",
];

struct Ctx {
    next: u64,
    n: usize,
    alias: BTreeMap<u64, Lb>,
    out: BTreeMap<String, ynet::Cell>,
    xors: BTreeMap<(Lb, Lb), Lb>,
    /// chunk equality nodes of wide compares, shared by `$eq` and `$lt` of the same operands
    chunk_eq: BTreeMap<Prod, Lb>,
    form: ChunkForm,
}

impl Ctx {
    fn res(&self, b: Lb) -> Lb {
        let mut b = b;
        while let Lb::Net(n) = b {
            match self.alias.get(&n) {
                Some(&x) if x != b => b = x,
                _ => break,
            }
        }
        b
    }

    /// Emit `f` (a sum of products) as a `$sop` cell; constants and single positive literals
    /// need no cell.
    fn emit(&mut self, f: Vec<Prod>) -> Lb {
        let mut prods: Vec<Prod> = vec![];
        'p: for p in f {
            let mut q: Prod = vec![];
            for (b, pos) in p {
                match self.res(b) {
                    Lb::C(v) => {
                        if v != pos {
                            continue 'p; // a false literal: the product is 0
                        }
                    }
                    n => {
                        if q.iter().any(|&(m, mp)| m == n && mp != pos) {
                            continue 'p; // x & !x
                        }
                        if !q.contains(&(n, pos)) {
                            q.push((n, pos));
                        }
                    }
                }
            }
            if q.is_empty() {
                return Lb::C(true);
            }
            prods.push(q);
        }
        if prods.is_empty() {
            return Lb::C(false);
        }
        if prods.len() == 1 && prods[0].len() == 1 && prods[0][0].1 {
            return prods[0][0].0;
        }
        let mut ins: Vec<Lb> = vec![];
        for p in &prods {
            for &(b, _) in p {
                if !ins.contains(&b) {
                    ins.push(b);
                }
            }
        }
        let w = ins.len();
        let mut table = vec![false; 2 * w * prods.len()];
        for (k, p) in prods.iter().enumerate() {
            for &(b, pos) in p {
                let j = ins.iter().position(|&x| x == b).unwrap();
                // (hi, lo) = (1, 0): positive literal, (0, 1): negative (importer sop_terms)
                table[2 * w * k + 2 * j + usize::from(pos)] = true;
            }
        }
        let y = self.next;
        self.next += 1;
        let name = format!("$atf15xx$wordmap${}", self.n);
        self.n += 1;
        let a: Vec<Bit> = ins.iter().map(|b| bit_value(*b)).collect();
        let tstr: String = table.iter().rev().map(|&x| if x { '1' } else { '0' }).collect();
        self.out.insert(
            name,
            ynet::Cell {
                ty: "$sop".into(),
                parameters: BTreeMap::from([
                    ("WIDTH".to_string(), Const::Int(w as i64)),
                    ("DEPTH".to_string(), Const::Int(prods.len() as i64)),
                    ("TABLE".to_string(), Const::Str(tstr)),
                ]),
                attributes: BTreeMap::new(),
                port_directions: Some(BTreeMap::from([("A".to_string(), "input".to_string()), ("Y".to_string(), "output".to_string())])),
                connections: BTreeMap::from([("A".to_string(), a), ("Y".to_string(), vec![Bit::Net(y)])]),
            },
        );
        Lb::Net(y)
    }

    fn not(&mut self, a: Lb) -> Lb {
        self.emit(vec![vec![(a, false)]])
    }
    fn and(&mut self, xs: &[Lb]) -> Lb {
        self.emit(vec![xs.iter().map(|&x| (x, true)).collect()])
    }
    fn or(&mut self, xs: &[Lb]) -> Lb {
        self.emit(xs.iter().map(|&x| vec![(x, true)]).collect())
    }
    fn mux(&mut self, s: Lb, a: Lb, b: Lb) -> Lb {
        self.emit(vec![vec![(s, false), (a, true)], vec![(s, true), (b, true)]])
    }
    /// XOR of up to 4 bits as one cover (2^(k-1) products).
    fn xor_small(&mut self, xs: &[Lb]) -> Lb {
        let xs: Vec<Lb> = xs.iter().map(|&b| self.res(b)).collect();
        let mut inv = false;
        let mut v: Vec<Lb> = vec![];
        for b in xs {
            match b {
                Lb::C(c) => inv ^= c,
                n => {
                    if let Some(i) = v.iter().position(|&m| m == n) {
                        v.remove(i); // a ^ a = 0
                    } else {
                        v.push(n);
                    }
                }
            }
        }
        if v.is_empty() {
            return Lb::C(inv);
        }
        if v.len() == 1 {
            return if inv { self.not(v[0]) } else { v[0] };
        }
        if v.len() == 2 && !inv {
            let key = (v[0].min(v[1]), v[0].max(v[1]));
            if let Some(&x) = self.xors.get(&key) {
                return x;
            }
            let x = self.emit(vec![vec![(v[0], true), (v[1], false)], vec![(v[0], false), (v[1], true)]]);
            self.xors.insert(key, x);
            return x;
        }
        let k = v.len();
        let mut prods = vec![];
        for m in 0u32..(1 << k) {
            if (m.count_ones() % 2 == 1) != inv {
                prods.push((0..k).map(|i| (v[i], m >> i & 1 == 1)).collect());
            }
        }
        self.emit(prods)
    }
    /// XOR of any number of bits: a tree of 4-input XORs.
    fn xor(&mut self, xs: &[Lb]) -> Lb {
        let mut level: Vec<Lb> = xs.to_vec();
        if level.is_empty() {
            return Lb::C(false);
        }
        while level.len() > 4 {
            level = level.chunks(4).map(|c| self.xor_small(c)).collect();
        }
        self.xor_small(&level)
    }
}

fn bit_value(b: Lb) -> Bit {
    match b {
        Lb::Net(n) => Bit::Net(n),
        Lb::C(true) => Bit::Const('1'),
        Lb::C(false) => Bit::Const('0'),
    }
}

fn to_lb(v: &Bit) -> Lb {
    match v {
        Bit::Net(n) => Lb::Net(*n),
        Bit::Const(c) => Lb::C(*c == '1'),
    }
}

fn param(c: &ynet::Cell, k: &str) -> usize {
    match c.parameters.get(k) {
        Some(Const::Int(n)) => u64::try_from(*n).unwrap_or(0) as usize,
        Some(Const::Str(s)) => s.chars().rev().enumerate().filter(|(_, c)| *c == '1').map(|(i, _)| 1usize << i).sum(),
        _ => 0,
    }
}

/// Port `p` of cell `c`, extended to `w` bits (sign-extended if `signed`).
fn port(ctx: &Ctx, c: &ynet::Cell, p: &str, w: usize, signed: bool) -> Vec<Lb> {
    let mut v: Vec<Lb> = c.port(p).iter().map(|b| ctx.res(to_lb(b))).collect();
    let fill = if signed { v.last().copied().unwrap_or(Lb::C(false)) } else { Lb::C(false) };
    v.truncate(w);
    while v.len() < w {
        v.push(fill);
    }
    v
}

/// Compares wider than this are built from [`CHUNK`]-bit pieces (spec F9.12): one flat cover reads
/// 3 signals per bit, so a 16-bit compare needs 48, more than a block's 40 UIM inputs.
const WIDE_COMPARE: usize = 8;
/// Bits per piece of a wide compare.
const CHUNK: usize = 4;

/// How a wide compare is built from pieces (spec F9.12). Neither form wins everywhere (measured on
/// registered compares of 8–38 bits): XOR nodes cost a macrocell per bit but keep every piece one
/// product wide; flat pieces need no XOR nodes (fewer macrocells, the only form that fits a 20-bit
/// compare on the ATF1504AS) but 7–8 products each. The flow fits both and keeps the better.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkForm {
    /// an XOR node per bit pair, pieces as products of them; else flat pieces over the operand bits
    pub xor_nodes: bool,
    /// bits per piece
    pub bits: usize,
}

/// XOR nodes per bit, 4-bit pieces (the default).
pub const CHUNK_XOR4: ChunkForm = ChunkForm { xor_nodes: true, bits: CHUNK };
/// Flat 3-bit pieces.
pub const CHUNK_FLAT3: ChunkForm = ChunkForm { xor_nodes: false, bits: 3 };

/// `a == b` over a chunk as a flat cover of its bits: Π (a_i·b_i + !a_i·!b_i), expanded.
fn eq_flat(a: &[Lb], b: &[Lb]) -> Vec<Prod> {
    let mut prods: Vec<Prod> = vec![vec![]];
    for i in 0..a.len() {
        prods = prods
            .into_iter()
            .flat_map(|p| {
                let mut q1 = p.clone();
                q1.extend([(a[i], true), (b[i], true)]);
                let mut q0 = p;
                q0.extend([(a[i], false), (b[i], false)]);
                [q1, q0]
            })
            .collect();
    }
    prods
}

/// `a < b` over a chunk as a flat cover of its bits: Σ_i !a_i·b_i·Π_{j>i} (a_j ≡ b_j), expanded.
fn less_flat(a: &[Lb], b: &[Lb]) -> Vec<Prod> {
    let n = a.len();
    let mut out = vec![];
    for i in 0..n {
        for mut q in eq_flat(&a[i + 1..], &b[i + 1..]) {
            q.extend([(a[i], false), (b[i], true)]);
            out.push(q);
        }
    }
    out
}

/// The equality literals of `a == b`, bit by bit (an XOR node per bit pair, a literal against a
/// constant).
fn eq_lits(ctx: &mut Ctx, a: &[Lb], b: &[Lb]) -> Vec<Prod> {
    (0..a.len())
        .map(|j| match (ctx.res(a[j]), ctx.res(b[j])) {
            (x, Lb::C(c)) => vec![(x, c)],
            (Lb::C(c), y) => vec![(y, c)],
            (x, y) => {
                let d = ctx.xor_small(&[x, y]);
                vec![(d, false)]
            }
        })
        .collect()
}

/// The equality node of one chunk (a single product), shared between the compares that need it.
fn chunk_eq(ctx: &mut Ctx, lits: &[Prod]) -> Lb {
    let p: Prod = lits.iter().flatten().copied().collect();
    if let Some(&x) = ctx.chunk_eq.get(&p) {
        return x;
    }
    let x = ctx.emit(vec![p.clone()]);
    ctx.chunk_eq.insert(p, x);
    x
}

/// `a == b` for a wide compare: the AND of its chunks' equality nodes.
fn equal_chunked(ctx: &mut Ctx, a: &[Lb], b: &[Lb]) -> Lb {
    let (xor, k) = (ctx.form.xor_nodes, ctx.form.bits);
    if !xor {
        let a: Vec<Lb> = a.iter().map(|&x| ctx.res(x)).collect();
        let b: Vec<Lb> = b.iter().map(|&x| ctx.res(x)).collect();
        let es: Vec<Lb> = (0..a.len())
            .step_by(k)
            .map(|s| {
                let e = (s + k).min(a.len());
                flat_eq_node(ctx, &a[s..e], &b[s..e])
            })
            .collect();
        return ctx.and(&es);
    }
    let lits = eq_lits(ctx, a, b);
    let es: Vec<Lb> = lits.chunks(k).map(|c| chunk_eq(ctx, c)).collect();
    ctx.and(&es)
}

/// A flat chunk equality node, shared between the compares of the same operand bits.
fn flat_eq_node(ctx: &mut Ctx, a: &[Lb], b: &[Lb]) -> Lb {
    let key: Prod = a.iter().zip(b).flat_map(|(&x, &y)| [(x, true), (y, true)]).collect();
    if let Some(&x) = ctx.chunk_eq.get(&key) {
        return x;
    }
    let x = ctx.emit(eq_flat(a, b));
    ctx.chunk_eq.insert(key, x);
    x
}

/// `a < b` for a wide compare (spec F9.12): per chunk c the flat less-than `l_c` and the equality
/// `e_c`, then `a < b = Σ_c l_c·Π_{d>c} e_d`. Every cell reads at most ~3·CHUNK signals, and the
/// combining cell two per chunk.
fn less_chunked(ctx: &mut Ctx, a: &[Lb], b: &[Lb]) -> Lb {
    let (xor, k) = (ctx.form.xor_nodes, ctx.form.bits);
    let n = a.len();
    let starts: Vec<usize> = (0..n).step_by(k).collect();
    let mut ls = vec![];
    let mut es = vec![];
    if !xor {
        let a: Vec<Lb> = a.iter().map(|&x| ctx.res(x)).collect();
        let b: Vec<Lb> = b.iter().map(|&x| ctx.res(x)).collect();
        for &s in &starts {
            let e = (s + k).min(n);
            ls.push(ctx.emit(less_flat(&a[s..e], &b[s..e])));
            es.push(flat_eq_node(ctx, &a[s..e], &b[s..e]));
        }
    }
    let lits = if xor { eq_lits(ctx, a, b) } else { vec![] };
    for &s in starts.iter().filter(|_| xor) {
        let e = (s + k).min(n);
        let mut prods = vec![];
        for i in s..e {
            let mut p: Prod = vec![(a[i], false), (b[i], true)];
            for l in &lits[i + 1..e] {
                p.extend(l.iter().copied());
            }
            prods.push(p);
        }
        ls.push(ctx.emit(prods));
        es.push(chunk_eq(ctx, &lits[s..e]));
    }
    let mut prods = vec![];
    for c in 0..ls.len() {
        let mut p: Prod = vec![(ls[c], true)];
        for &e in &es[c + 1..] {
            p.push((e, true));
        }
        prods.push(p);
    }
    ctx.emit(prods)
}

/// `a < b` (unsigned, LSB first): Σ_i !a_i·b_i·Π_{j>i} (a_j == b_j).
fn less(ctx: &mut Ctx, a: &[Lb], b: &[Lb]) -> Lb {
    if a.len() > WIDE_COMPARE {
        return less_chunked(ctx, a, b);
    }
    let n = a.len();
    let eqlits: Vec<Prod> = (0..n)
        .map(|j| match (ctx.res(a[j]), ctx.res(b[j])) {
            (x, Lb::C(c)) => vec![(x, c)],
            (Lb::C(c), y) => vec![(y, c)],
            (x, y) => {
                let d = ctx.xor_small(&[x, y]);
                vec![(d, false)]
            }
        })
        .collect();
    let mut prods = vec![];
    for i in 0..n {
        let mut p: Prod = vec![(a[i], false), (b[i], true)];
        for e in &eqlits[i + 1..] {
            p.extend(e.iter().copied());
        }
        prods.push(p);
    }
    ctx.emit(prods)
}

/// `a + b + cin` (LSB first, width of `a`).
fn add(ctx: &mut Ctx, a: &[Lb], b: &[Lb], cin: Lb) -> Vec<Lb> {
    let n = a.len();
    let mut out = vec![];
    let consts: Option<Vec<bool>> = b.iter().map(|&x| if let Lb::C(c) = ctx.res(x) { Some(c) } else { None }).collect();
    // x + 1: carry into bit i = a_0 & … & a_{i-1} (one product)
    if let (Some(k), Lb::C(false)) = (&consts, ctx.res(cin)) {
        if k.first() == Some(&true) && k[1..].iter().all(|&x| !x) {
            for i in 0..n {
                let c = ctx.and(&a[..i]);
                let s = ctx.xor_small(&[a[i], c]);
                out.push(s);
            }
            return out;
        }
    }
    let mut c = cin;
    for i in 0..n {
        let s = ctx.xor_small(&[a[i], b[i], c]);
        out.push(s);
        if i + 1 < n {
            c = ctx.emit(vec![vec![(a[i], true), (b[i], true)], vec![(a[i], true), (c, true)], vec![(b[i], true), (c, true)]]);
        }
    }
    out
}

/// Lower the coarse cells of a module (Yosys JSON schema, in place). Returns the number of
/// coarse cells lowered; `Err` names a coarse cell type that is neither lowered nor a fine cell.
pub fn lower_module(m: &mut ynet::Module, form: ChunkForm) -> Result<usize, String> {
    let cells = m.cells.clone();
    let mut next = 2;
    let mut scan = |v: &[Bit]| {
        for b in v {
            if let Bit::Net(n) = b {
                next = next.max(n + 1);
            }
        }
    };
    for c in cells.values() {
        c.connections.values().for_each(|v| scan(v));
    }
    for n in m.netnames.values() {
        scan(&n.bits);
    }
    for p in m.ports.iter().flat_map(|x| x.values()) {
        scan(&p.bits);
    }
    // driver of every coarse output bit, for a topological order (inputs first)
    let lowered = |t: &str| LOWERED.contains(&t);
    let mut driver: BTreeMap<u64, String> = BTreeMap::new();
    for (name, c) in &cells {
        let t = c.ty.as_str();
        if lowered(t) {
            for b in c.port("Y") {
                if let Bit::Net(n) = b {
                    driver.insert(*n, name.clone());
                }
            }
        } else if t.starts_with('$') && !t.starts_with("$_") && t != "$sop" && t != "$scopeinfo" {
            return Err(format!("coarse cell {name} of type {t} is not lowered (run Yosys's techmap on it)"));
        }
    }
    let mut order: Vec<String> = vec![];
    let mut state: BTreeMap<String, u8> = BTreeMap::new();
    fn visit(
        n: &str,
        cells: &BTreeMap<String, ynet::Cell>,
        driver: &BTreeMap<u64, String>,
        state: &mut BTreeMap<String, u8>,
        order: &mut Vec<String>,
    ) {
        match state.get(n) {
            Some(_) => return,
            None => {
                state.insert(n.to_string(), 1);
            }
        }
        let c = &cells[n];
        for (p, v) in &c.connections {
            if p == "Y" {
                continue;
            }
            for b in v {
                if let Some(d) = match b {
                    Bit::Net(x) => driver.get(x),
                    Bit::Const(_) => None,
                } {
                    visit(d, cells, driver, state, order);
                }
            }
        }
        state.insert(n.to_string(), 2);
        order.push(n.to_string());
    }
    for (name, c) in &cells {
        if lowered(&c.ty) {
            visit(name, &cells, &driver, &mut state, &mut order);
        }
    }
    let mut ctx = Ctx { next, n: 0, alias: BTreeMap::new(), out: BTreeMap::new(), xors: BTreeMap::new(), chunk_eq: BTreeMap::new(), form };
    for name in &order {
        let c = &cells[name];
        let t = c.ty.as_str();
        let yw = param(c, "Y_WIDTH").max(c.port("Y").len());
        let (asg, bsg) = (param(c, "A_SIGNED") == 1, param(c, "B_SIGNED") == 1);
        let both = asg && bsg;
        let aw = param(c, "A_WIDTH");
        let bw = param(c, "B_WIDTH");
        let y: Vec<Lb> = match t {
            "$not" => port(&ctx, c, "A", yw, asg).into_iter().map(|a| ctx.not(a)).collect(),
            "$pos" | "$buf" => port(&ctx, c, "A", yw, asg),
            "$and" | "$or" | "$xor" | "$xnor" => {
                let (a, b) = (port(&ctx, c, "A", yw, both), port(&ctx, c, "B", yw, both));
                a.iter()
                    .zip(&b)
                    .map(|(&x, &z)| match t {
                        "$and" => ctx.and(&[x, z]),
                        "$or" => ctx.or(&[x, z]),
                        "$xor" => ctx.xor_small(&[x, z]),
                        _ => ctx.xor_small(&[x, z, Lb::C(true)]),
                    })
                    .collect()
            }
            "$reduce_and" | "$reduce_or" | "$reduce_bool" | "$reduce_xor" | "$reduce_xnor" | "$logic_not" => {
                let a = port(&ctx, c, "A", aw, false);
                let r = match t {
                    "$reduce_and" => ctx.and(&a),
                    "$reduce_or" | "$reduce_bool" => ctx.or(&a),
                    "$reduce_xor" => ctx.xor(&a),
                    "$reduce_xnor" => {
                        let x = ctx.xor(&a);
                        ctx.not(x)
                    }
                    _ => ctx.emit(vec![a.iter().map(|&x| (x, false)).collect()]),
                };
                widen(r, yw)
            }
            "$logic_and" | "$logic_or" => {
                let (a, b) = (port(&ctx, c, "A", aw, false), port(&ctx, c, "B", bw, false));
                let (ra, rb) = (ctx.or(&a), ctx.or(&b));
                let r = if t == "$logic_and" { ctx.and(&[ra, rb]) } else { ctx.or(&[ra, rb]) };
                widen(r, yw)
            }
            "$eq" | "$ne" | "$eqx" | "$nex" => {
                let w = aw.max(bw);
                let (a, b) = (port(&ctx, c, "A", w, both), port(&ctx, c, "B", w, both));
                let eq = if w > WIDE_COMPARE {
                    equal_chunked(&mut ctx, &a, &b)
                } else {
                    let mut p: Prod = vec![];
                    for i in 0..w {
                        match (ctx.res(a[i]), ctx.res(b[i])) {
                            (x, Lb::C(k)) => p.push((x, k)),
                            (Lb::C(k), z) => p.push((z, k)),
                            (x, z) => {
                                let d = ctx.xor_small(&[x, z]);
                                p.push((d, false));
                            }
                        }
                    }
                    ctx.emit(vec![p])
                };
                let r = if t == "$eq" || t == "$eqx" { eq } else { ctx.not(eq) };
                widen(r, yw)
            }
            "$lt" | "$le" | "$gt" | "$ge" => {
                let w = aw.max(bw);
                let (mut a, mut b) = (port(&ctx, c, "A", w, both), port(&ctx, c, "B", w, both));
                if both && w > 0 {
                    // signed: compare with the sign bits swapped
                    std::mem::swap(&mut a[w - 1], &mut b[w - 1]);
                }
                let r = match t {
                    "$lt" => less(&mut ctx, &a, &b),
                    "$gt" => less(&mut ctx, &b, &a),
                    "$le" => {
                        let g = less(&mut ctx, &b, &a);
                        ctx.not(g)
                    }
                    _ => {
                        let l = less(&mut ctx, &a, &b);
                        ctx.not(l)
                    }
                };
                widen(r, yw)
            }
            "$mux" => {
                let w = param(c, "WIDTH");
                let (a, b, s) = (port(&ctx, c, "A", w, false), port(&ctx, c, "B", w, false), port(&ctx, c, "S", 1, false)[0]);
                (0..w).map(|i| ctx.mux(s, a[i], b[i])).collect()
            }
            "$pmux" => {
                let (w, sw) = (param(c, "WIDTH"), param(c, "S_WIDTH"));
                let a = port(&ctx, c, "A", w, false);
                let b = port(&ctx, c, "B", w * sw, false);
                let s = port(&ctx, c, "S", sw, false);
                (0..w)
                    .map(|i| {
                        let mut prods: Vec<Prod> = (0..sw).map(|k| vec![(s[k], true), (b[k * w + i], true)]).collect();
                        let mut none: Prod = s.iter().map(|&x| (x, false)).collect();
                        none.push((a[i], true));
                        prods.push(none);
                        ctx.emit(prods)
                    })
                    .collect()
            }
            "$shiftx" => {
                if bsg || bw > 16 {
                    return Err(format!("{name}: $shiftx with a signed or wider than 16-bit shift is not lowered"));
                }
                let a = port(&ctx, c, "A", aw, false);
                let b = port(&ctx, c, "B", bw, false);
                (0..yw)
                    .map(|j| {
                        let mut prods: Vec<Prod> = vec![];
                        for k in 0..(1usize << bw).min(aw) {
                            if j + k < aw {
                                let mut p: Prod = (0..bw).map(|i| (b[i], k >> i & 1 == 1)).collect();
                                p.push((a[j + k], true));
                                prods.push(p);
                            }
                        }
                        ctx.emit(prods)
                    })
                    .collect()
            }
            "$add" | "$sub" | "$neg" => {
                let (a, b, cin) = match t {
                    "$add" => (port(&ctx, c, "A", yw, both), port(&ctx, c, "B", yw, both), Lb::C(false)),
                    "$sub" => {
                        let b: Vec<Lb> = port(&ctx, c, "B", yw, both).into_iter().map(|x| ctx.not(x)).collect();
                        (port(&ctx, c, "A", yw, both), b, Lb::C(true))
                    }
                    _ => {
                        let a: Vec<Lb> = port(&ctx, c, "A", yw, asg).into_iter().map(|x| ctx.not(x)).collect();
                        (vec![Lb::C(false); yw], a, Lb::C(true))
                    }
                };
                add(&mut ctx, &a, &b, cin)
            }
            _ => unreachable!(),
        };
        for (yb, v) in c.port("Y").iter().zip(y) {
            if let Bit::Net(n) = *yb {
                let v = ctx.res(v);
                if v != Lb::Net(n) {
                    ctx.alias.insert(n, v);
                }
            }
        }
    }
    // a lowered cell whose output is another net: drive the original bit with a buffer if it is a
    // port or a kept name (simplest: rewrite every reference through the aliases)
    let rewrite = |v: &mut Vec<Bit>, ctx: &Ctx| {
        for b in v.iter_mut() {
            if let Bit::Net(n) = *b {
                let r = ctx.res(Lb::Net(n));
                if r != Lb::Net(n) {
                    *b = bit_value(r);
                }
            }
        }
    };
    let count = order.len();
    let mut new_cells = BTreeMap::new();
    for (name, mut c) in cells {
        if lowered(&c.ty) {
            continue;
        }
        for v in c.connections.values_mut() {
            rewrite(v, &ctx);
        }
        new_cells.insert(name, c);
    }
    let gen = std::mem::take(&mut ctx.out);
    for (name, mut c) in gen {
        for v in c.connections.values_mut() {
            rewrite(v, &ctx);
        }
        new_cells.insert(name, c);
    }
    m.cells = new_cells;
    for p in m.ports.iter_mut().flat_map(|x| x.values_mut()) {
        rewrite(&mut p.bits, &ctx);
    }
    for n in m.netnames.values_mut() {
        rewrite(&mut n.bits, &ctx);
    }
    Ok(count)
}

fn widen(r: Lb, yw: usize) -> Vec<Lb> {
    let mut v = vec![r];
    v.resize(yw.max(1), Lb::C(false));
    v.truncate(yw.max(1));
    v
}

/// Does the netlist contain coarse cells this module lowers?
pub fn has_coarse(netlist: &Netlist) -> bool {
    netlist.modules.values().any(|m| m.cells.values().any(|c| LOWERED.contains(&c.ty.as_str())))
}

/// Does the netlist compare words wider than [`WIDE_COMPARE`] bits (built from pieces)?
pub fn has_wide_compare(netlist: &Netlist) -> bool {
    let wide = |c: &ynet::Cell| param(c, "A_WIDTH").max(param(c, "B_WIDTH")) > WIDE_COMPARE;
    netlist.modules.values().any(|m| {
        m.cells.values().any(|c| matches!(c.ty.as_str(), "$eq" | "$ne" | "$eqx" | "$nex" | "$lt" | "$le" | "$gt" | "$ge") && wide(c))
    })
}

/// Lower every module of a netlist, wide compares in the given form.
pub fn lower(netlist: &mut Netlist, form: ChunkForm) -> Result<usize, String> {
    let mut n = 0;
    for m in netlist.modules.values_mut() {
        n += lower_module(m, form)?;
    }
    Ok(n)
}
