//! Yosys JSON (after `data/synth_atf15xx.ys`) → [`atf15xx_fit::netlist::Design`].
//!
//! Accepted cells (exactly what the synthesis script leaves; anything else is an error):
//!
//! | Yosys cell | Meaning | Netlist |
//! |---|---|---|
//! | `$sop` | sum of products | `Sop` (inlined into readers, or a buried node) |
//! | `$_NOT_`, `$_BUF_` | inverter / buffer | literal polarity |
//! | `$_DFF_P_`, `$_DFFE_PP_` | D flip-flop (+ clock enable) | `Storage::Dff`, `clk`, `ce` |
//! | `$_DFF_PP0_`/`PP1_`, `$_DFFE_PP0P_`/`PP1P_` | + async reset / set | `ar` / `ap` |
//! | `$_DFFSR_PPP_`, `$_DFFSRE_PPPP_` | + async set and reset | `ap` and `ar` |
//! | `$_DLATCH_P_`, `$_DLATCH_PP0_`/`PP1_`, `$_DLATCHSR_PPP_` | transparent-high latch (± async R/S) | `Storage::Latch` |
//! | `$_TBUF_` | tri-state driver of an output/inout port | `oe` |
//!
//! Mapping rules (spec: docs/architecture/fitter/07-yosys-flow.md §F7.4):
//! * every bit of an output/inout port becomes an output cell named after the port bit;
//! * a storage element whose Q drives a port (directly or through a `$_TBUF_`) lives in that
//!   port's macrocell; other storage elements become buried register cells;
//! * reading an input port is `Pad`; reading an inout port is `Pad` (pin feedback); reading a
//!   storage Q is `Fb(cell, true)`; reading the internal value of a comb output is the macrocell
//!   feedback `Fb(cell, false)`, never the pad, so tri-stated outputs read their own logic
//!   correctly;
//! * every other `$sop` output becomes a buried combinatorial node, later inlined by
//!   [`crate::opt::collapse_limited`] where that keeps the product-term count reasonable.

pub use crate::ynet::Bit;
use crate::ynet::{self, Const, Netlist};
use atf15xx_fit::logic::{Signal, Sop};
use atf15xx_fit::netlist::{Cell, Design, Storage};
use std::collections::{BTreeMap, BTreeSet};

/// Parameter as an unsigned integer (a missing one is an error).
fn param_u(v: Option<&Const>) -> Result<usize, String> {
    let b = v.ok_or("bad parameter")?.bits()?;
    Ok(b.iter().enumerate().filter(|(_, &x)| x).map(|(i, _)| 1usize << i).sum())
}

/// Make a Verilog name usable as a fitter / `.tt2` signal name: `q[3]` → `q3`, escapes and
/// punctuation → `_` (the same rule as `tools/sim/yosys2tt2.py`).
pub fn sanitize(n: &str) -> String {
    let n = n.trim_start_matches('\\');
    let mut out = String::new();
    let mut chars = n.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' {
            let mut num = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    num.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            if chars.peek() == Some(&']') && !num.is_empty() {
                chars.next();
                out.push_str(&num);
                continue;
            }
            out.push('_');
            out.push_str(&num);
        } else if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if !out.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        out.insert(0, 'N');
    }
    out
}

/// A port bit of the top module.
#[derive(Clone, Debug)]
pub struct PortBit {
    /// Verilog name, e.g. `q[3]`.
    pub verilog: String,
    /// Netlist / fitter name, e.g. `q3`.
    pub name: String,
    pub dir: String,
    pub bit: Bit,
    /// Pin from a `LOC`/`loc`/`PIN`/`pin` attribute on the port (per bit: `LOC = "12 13 14"`).
    pub loc: Option<u16>,
    /// Other port attributes (lower-cased key -> value).
    pub attrs: BTreeMap<String, String>,
}

/// Result of reading the JSON: the netlist plus port bookkeeping.
#[derive(Debug)]
pub struct Imported {
    pub top: String,
    pub design: Design,
    pub ports: Vec<PortBit>,
    pub warnings: Vec<String>,
    /// Count of Yosys cells by type (for the report).
    pub cell_stats: BTreeMap<String, usize>,
    /// Netlist cell/input name -> the Yosys net (register or wire) or port bit it was made from,
    /// e.g. `counter5` -> `counter[5]` (for source-to-macrocell maps, `<top>.map.json`).
    pub origins: BTreeMap<String, String>,
    /// Explicit Verilog initial values of storage cells (cell name -> value); registers without
    /// one are absent. Power-up semantics are decided by the flow (`flow::InitPolicy`).
    pub inits: BTreeMap<String, bool>,
}

/// Bits of an `init` attribute (LSB first): int, or MSB-first string of 0/1/x.
fn init_bits(v: &Const, n: usize) -> Vec<Option<bool>> {
    if let Const::Int(i) = v {
        let i = u64::try_from(*i).unwrap_or(0);
        return (0..n).map(|k| Some(k < 64 && (i >> k) & 1 == 1)).collect();
    }
    let s = match v {
        Const::Str(s) => s.as_str(),
        _ => "",
    };
    let mut out: Vec<Option<bool>> = s
        .chars()
        .rev()
        .map(|c| match c {
            '0' => Some(false),
            '1' => Some(true),
            _ => None,
        })
        .collect();
    out.resize(n, None);
    out
}

struct Ctx<'a> {
    cells: &'a BTreeMap<String, ynet::Cell>,
    /// bit -> (cell name, output port)
    driver: BTreeMap<Bit, (String, String)>,
    names: BTreeMap<u64, (String, bool)>,
    in_ports: BTreeMap<Bit, String>,
    inout_ports: BTreeMap<Bit, String>,
    /// TBUF-driven port bits: port bit -> (A, E)
    tbuf: BTreeMap<Bit, (Bit, Bit)>,
    /// Bits owned by a netlist cell: bit -> (cell name, registered?)
    owner: BTreeMap<Bit, (String, bool)>,
    /// Buried comb nodes created on demand: bit -> name
    nodes: BTreeMap<Bit, String>,
    used_names: BTreeSet<String>,
    /// netlist name -> Yosys net / port-bit name it was made from
    origins: BTreeMap<String, String>,
}

fn conn(c: &ynet::Cell, p: &str) -> Result<Bit, String> {
    c.port(p).first().cloned().ok_or_else(|| format!("cell has no connection {p}"))
}

/// `$sop` → list of products, each `(input index, positive)`.
fn sop_terms(c: &ynet::Cell) -> Result<Vec<Vec<(usize, bool)>>, String> {
    let p = &c.parameters;
    let w = param_u(p.get("WIDTH"))?;
    let d = param_u(p.get("DEPTH"))?;
    let t = p.get("TABLE").ok_or("bad parameter")?.bits()?;
    let get = |i: usize| t.get(i).copied().unwrap_or(false);
    let mut terms = vec![];
    'term: for k in 0..d {
        let mut lits = vec![];
        for j in 0..w {
            let lo = get(2 * w * k + 2 * j);
            let hi = get(2 * w * k + 2 * j + 1);
            match (hi, lo) {
                (false, false) => {}
                (false, true) => lits.push((j, false)),
                (true, false) => lits.push((j, true)),
                (true, true) => continue 'term, // x & !x: never true
            }
        }
        terms.push(lits);
    }
    Ok(terms)
}

impl<'a> Ctx<'a> {
    fn net_name(&self, b: &Bit) -> String {
        match b {
            Bit::Net(n) => self.names.get(n).map(|x| x.0.clone()).unwrap_or_else(|| format!("n{n}")),
            Bit::Const(c) => format!("const{c}"),
        }
    }

    fn fresh(&mut self, base: &str) -> String {
        let orig = base.trim_start_matches('\\').to_string();
        let base = sanitize(base);
        let mut n = base.clone();
        let mut k = 1;
        while self.used_names.contains(&n) {
            n = format!("{base}_{k}");
            k += 1;
        }
        self.used_names.insert(n.clone());
        self.origins.insert(n.clone(), orig);
        n
    }

    /// Resolve a bit to a literal (signal, polarity), or a constant.
    fn literal(&mut self, b: &Bit, pos: bool, depth: usize) -> Result<Result<(Signal, bool), bool>, String> {
        if depth > 1000 {
            return Err("combinational loop through inverters/aliases".into());
        }
        if let Bit::Const(c) = b {
            return match c {
                '0' => Ok(Err(!pos)),
                '1' => Ok(Err(pos)),
                _ => Ok(Err(false)), // x/z read as 0
            };
        }
        if let Some(n) = self.inout_ports.get(b) {
            return Ok(Ok((Signal::Pad(n.clone()), pos)));
        }
        if let Some(n) = self.in_ports.get(b) {
            return Ok(Ok((Signal::Pad(n.clone()), pos)));
        }
        if let Some((n, reg)) = self.owner.get(b) {
            return Ok(Ok((Signal::Fb(n.clone(), *reg), pos)));
        }
        let (cn, _port) = self.driver.get(b).cloned().ok_or_else(|| format!("undriven net {}", self.net_name(b)))?;
        let c = &self.cells[&cn];
        match c.ty.as_str() {
            "$_NOT_" => {
                let a = conn(c, "A")?;
                self.literal(&a, !pos, depth + 1)
            }
            "$_BUF_" => {
                let a = conn(c, "A")?;
                self.literal(&a, pos, depth + 1)
            }
            "$sop" => {
                let terms = sop_terms(c)?;
                let ins = c.port("A").to_vec();
                if terms.len() == 1 && terms[0].len() == 1 {
                    let (j, p) = terms[0][0];
                    return self.literal(&ins[j], p == pos, depth + 1);
                }
                if terms.is_empty() {
                    return Ok(Err(!pos));
                }
                if terms.iter().any(|t| t.is_empty()) {
                    return Ok(Err(pos));
                }
                let name = match self.nodes.get(b) {
                    Some(n) => n.clone(),
                    None => {
                        let n = self.fresh(&self.net_name(b));
                        self.nodes.insert(b.clone(), n.clone());
                        n
                    }
                };
                Ok(Ok((Signal::Fb(name, false), pos)))
            }
            t if is_gate(t) => {
                let name = match self.nodes.get(b) {
                    Some(n) => n.clone(),
                    None => {
                        let n = self.fresh(&self.net_name(b));
                        self.nodes.insert(b.clone(), n.clone());
                        n
                    }
                };
                Ok(Ok((Signal::Fb(name, false), pos)))
            }
            t => Err(format!("unsupported cell {t} ({cn}) driving {}", self.net_name(b))),
        }
    }

    /// The SOP of a fine-grained gate (`synth -noabc` / `simplemap` output) over its inputs' literals.
    fn gate_sop(&mut self, c: &ynet::Cell) -> Result<Sop, String> {
        let t = c.ty.as_str();
        let mut inp = |p: &str| -> Result<Sop, String> {
            let b = conn(c, p)?;
            Ok(match self.literal(&b, true, 0)? {
                Ok(l) => Sop(vec![vec![l]]),
                Err(true) => Sop::one(),
                Err(false) => Sop::zero(),
            })
        };
        let xor = |a: &Sop, b: &Sop| a.and(&b.complement()).or(&a.complement().and(b));
        let f = match t {
            "$_AND_" => inp("A")?.and(&inp("B")?),
            "$_OR_" => inp("A")?.or(&inp("B")?),
            "$_XOR_" => xor(&inp("A")?, &inp("B")?),
            "$_XNOR_" => xor(&inp("A")?, &inp("B")?).complement(),
            "$_NAND_" => inp("A")?.and(&inp("B")?).complement(),
            "$_NOR_" => inp("A")?.or(&inp("B")?).complement(),
            "$_ANDNOT_" => inp("A")?.and(&inp("B")?.complement()),
            "$_ORNOT_" => inp("A")?.or(&inp("B")?.complement()),
            "$_MUX_" | "$_NMUX_" => {
                let (a, b, sel) = (inp("A")?, inp("B")?, inp("S")?);
                let m = sel.complement().and(&a).or(&sel.and(&b));
                if t == "$_NMUX_" {
                    m.complement()
                } else {
                    m
                }
            }
            "$_AOI3_" => inp("A")?.and(&inp("B")?).or(&inp("C")?).complement(),
            "$_OAI3_" => inp("A")?.or(&inp("B")?).and(&inp("C")?).complement(),
            "$_AOI4_" => inp("A")?.and(&inp("B")?).or(&inp("C")?.and(&inp("D")?)).complement(),
            "$_OAI4_" => inp("A")?.or(&inp("B")?).and(&inp("C")?.or(&inp("D")?)).complement(),
            _ => return Err(format!("not a gate: {t}")),
        };
        Ok(f.minimize())
    }

    /// The function of a bit as an SOP over netlist signals.
    fn sop(&mut self, b: &Bit) -> Result<Sop, String> {
        if let Some((cn, _)) = self.driver.get(b).cloned() {
            let c = &self.cells[&cn];
            if is_gate(&c.ty) && !self.owner.contains_key(b) {
                return self.gate_sop(c);
            }
            if c.ty == "$sop" && !self.owner.contains_key(b) {
                let terms = sop_terms(c)?;
                let ins = c.port("A").to_vec();
                let mut out = Sop::zero();
                for t in terms {
                    let mut prod = vec![];
                    let mut zero = false;
                    for (j, p) in t {
                        match self.literal(&ins[j], p, 0)? {
                            Ok(l) => prod.push(l),
                            Err(true) => {}
                            Err(false) => {
                                zero = true;
                                break;
                            }
                        }
                    }
                    if !zero {
                        out.push(prod);
                    }
                }
                return Ok(out);
            }
        }
        Ok(match self.literal(b, true, 0)? {
            Ok(l) => Sop(vec![vec![l]]),
            Err(true) => Sop::one(),
            Err(false) => Sop::zero(),
        })
    }

    /// Like [`Ctx::sop`] but for a bit owned by a cell: the cell's own function, not its feedback.
    /// The function of bit `b` as computed by cell `cell` itself: if `cell` owns `b` (it is the
    /// macrocell that computes it), the owner entry is bypassed; if another cell owns it (e.g. a
    /// register that drives two ports), the result is that cell's feedback (a copy).
    fn sop_of_owned(&mut self, b: &Bit, cell: &str) -> Result<Sop, String> {
        if self.owner.get(b).map_or(false, |(n, _)| n != cell) {
            return self.sop(b);
        }
        let saved = self.owner.remove(b);
        let r = self.sop(b);
        if let Some(s) = saved {
            self.owner.insert(b.clone(), s);
        }
        r
    }
}

/// Fine-grained combinational gates the importer accepts besides `$sop`.
fn is_gate(t: &str) -> bool {
    matches!(
        t,
        "$_AND_"
            | "$_OR_"
            | "$_XOR_"
            | "$_XNOR_"
            | "$_NAND_"
            | "$_NOR_"
            | "$_ANDNOT_"
            | "$_ORNOT_"
            | "$_MUX_"
            | "$_NMUX_"
            | "$_AOI3_"
            | "$_OAI3_"
            | "$_AOI4_"
            | "$_OAI4_"
    )
}

fn storage_kind(t: &str) -> Option<(Storage, bool, bool, bool)> {
    // (storage, has_ce, has_r, has_s)
    Some(match t {
        "$_DFF_P_" => (Storage::Dff, false, false, false),
        "$_DFFE_PP_" => (Storage::Dff, true, false, false),
        "$_DFF_PP0_" => (Storage::Dff, false, true, false),
        "$_DFF_PP1_" => (Storage::Dff, false, false, true),
        "$_DFFE_PP0P_" => (Storage::Dff, true, true, false),
        "$_DFFE_PP1P_" => (Storage::Dff, true, false, true),
        "$_DFFSR_PPP_" => (Storage::Dff, false, true, true),
        "$_DFFSRE_PPPP_" => (Storage::Dff, true, true, true),
        "$_DLATCH_P_" => (Storage::Latch, false, false, false),
        "$_DLATCH_PP0_" => (Storage::Latch, false, true, false),
        "$_DLATCH_PP1_" => (Storage::Latch, false, false, true),
        "$_DLATCHSR_PPP_" => (Storage::Latch, false, true, true),
        _ => return None,
    })
}

/// Read a Yosys JSON netlist.
pub fn import(json: &str, top: Option<&str>) -> Result<Imported, String> {
    import_netlist(&Netlist::from_json(json)?, top)
}

/// [`import`] from a typed netlist (from a JSON file, or built from RTLIL by the Yosys plugin,
/// `atf15xx_synth::plugin`).
pub fn import_netlist(n: &Netlist, top: Option<&str>) -> Result<Imported, String> {
    let mods = &n.modules;
    let top = match top {
        Some(t) => t.to_string(),
        None => mods
            .iter()
            .find(|(_, m)| m.attributes.get("top").and_then(Const::as_u64) == Some(1))
            .map(|(n, _)| n.clone())
            .or_else(|| mods.keys().next().cloned())
            .ok_or("no top module")?,
    };
    let m = mods.get(&top).ok_or_else(|| format!("module {top} not found"))?;
    let cells = &m.cells;
    let mut warnings = vec![];
    let mut cell_stats = BTreeMap::new();
    for c in cells.values() {
        *cell_stats.entry(c.ty.clone()).or_insert(0) += 1;
    }

    let mut ctx = Ctx {
        cells,
        driver: BTreeMap::new(),
        names: BTreeMap::new(),
        in_ports: BTreeMap::new(),
        inout_ports: BTreeMap::new(),
        tbuf: BTreeMap::new(),
        owner: BTreeMap::new(),
        nodes: BTreeMap::new(),
        used_names: BTreeSet::new(),
        origins: BTreeMap::new(),
    };
    let mut init_of: BTreeMap<u64, bool> = BTreeMap::new();
    {
        for (name, n) in &m.netnames {
            let hidden = n.hide_name == 1;
            let bs = &n.bits;
            if let Some(iv) = n.attributes.get("init") {
                for (b, v) in bs.iter().zip(init_bits(iv, bs.len())) {
                    if let (Bit::Net(id), Some(v)) = (b, v) {
                        init_of.insert(*id, v);
                    }
                }
            }
            for (i, b) in bs.iter().enumerate() {
                if let Bit::Net(id) = b {
                    let nm = if bs.len() == 1 { name.clone() } else { format!("{name}[{i}]") };
                    let better = match ctx.names.get(id) {
                        None => true,
                        Some((_, h)) => *h && !hidden,
                    };
                    if better {
                        ctx.names.insert(*id, (nm, hidden));
                    }
                }
            }
        }
    }
    for (cn, c) in cells {
        let dirs = c.port_directions.as_ref().ok_or("cell without port_directions")?;
        for (p, d) in dirs {
            if d == "output" {
                for b in c.port(p).iter().cloned() {
                    if matches!(b, Bit::Net(_)) {
                        ctx.driver.insert(b, (cn.clone(), p.clone()));
                    }
                }
            }
        }
        let t = c.ty.as_str();
        if t == "$_TBUF_" {
            ctx.tbuf.insert(conn(c, "Y")?, (conn(c, "A")?, conn(c, "E")?));
        } else if !(t == "$sop" || is_gate(t) || t == "$_NOT_" || t == "$_BUF_" || storage_kind(t).is_some()) {
            return Err(format!("unsupported Yosys cell {t} ({cn}); run the ATF15xx synthesis script (atf15xx-synth synth) first"));
        }
    }

    // ports
    let mut ports = vec![];
    let pmap = m.ports.as_ref().ok_or("no ports")?;
    for (pn, p) in pmap {
        let dir = p.direction.clone();
        let bs = &p.bits;
        let mut attrs = BTreeMap::new();
        if let Some(a) = m.netnames.get(pn).map(|n| &n.attributes) {
            for (k, v) in a {
                let val = match v {
                    Const::Str(s) => {
                        // Yosys writes string attributes as text and bit-vector ones as 0/1 strings
                        if !s.is_empty() && s.chars().all(|c| c == '0' || c == '1') && s.len() >= 8 {
                            param_u(Some(v)).map(|x| x.to_string()).unwrap_or_else(|_| s.clone())
                        } else {
                            s.clone()
                        }
                    }
                    Const::Int(n) => n.to_string(),
                    Const::Other(_) => continue,
                };
                attrs.insert(k.to_ascii_lowercase(), val);
            }
        }
        let locs: Vec<u16> = ["loc", "pin", "chip_pin"]
            .iter()
            .find_map(|k| attrs.get(*k))
            .map(|s| s.split(|c: char| c == ',' || c.is_whitespace()).filter_map(|x| x.parse().ok()).collect())
            .unwrap_or_default();
        if !locs.is_empty() && locs.len() != bs.len() {
            warnings.push(format!("port {pn}: {} LOC values for {} bits (LSB first); extra/missing ignored", locs.len(), bs.len()));
        }
        // Verilog bit index: Yosys lists bits LSB first; the netname's `offset`/`upto` give the
        // declared range (`input [15:10] a` -> offset 10), so list bit k is a[offset + k]
        let net = m.netnames.get(pn);
        let offset = net.map_or(0, |n| n.offset);
        let upto = net.is_some_and(|n| n.upto != 0);
        for (i, b) in bs.iter().enumerate() {
            let idx = offset + if upto { (bs.len() - 1 - i) as i64 } else { i as i64 };
            let verilog = if bs.len() == 1 { pn.clone() } else { format!("{pn}[{idx}]") };
            let name = ctx.fresh(&verilog);
            ports.push(PortBit { verilog, name, dir: dir.clone(), bit: b.clone(), loc: locs.get(i).copied(), attrs: attrs.clone() });
        }
    }
    for pb in &ports {
        match pb.dir.as_str() {
            "input" => {
                ctx.in_ports.insert(pb.bit.clone(), pb.name.clone());
            }
            "inout" => {
                ctx.inout_ports.insert(pb.bit.clone(), pb.name.clone());
            }
            _ => {}
        }
    }

    // storage homes
    let out_ports: Vec<&PortBit> = ports.iter().filter(|p| p.dir != "input").collect();
    let mut storage: Vec<(String, &ynet::Cell, Storage, bool, bool, bool, Option<&PortBit>)> = vec![];
    let mut inits = BTreeMap::new();
    let mut homed: BTreeSet<String> = BTreeSet::new();
    for c in cells.values() {
        let Some((st, ce, r, s)) = storage_kind(&c.ty) else { continue };
        let q = conn(c, "Q")?;
        let home = out_ports
            .iter()
            .copied()
            .find(|p| !homed.contains(&p.name) && (p.bit == q || ctx.tbuf.get(&p.bit).map_or(false, |(a, _)| *a == q)));
        let name = match home {
            Some(p) => {
                homed.insert(p.name.clone());
                p.name.clone()
            }
            None => ctx.fresh(&ctx.net_name(&q)),
        };
        if let Bit::Net(id) = &q {
            if let Some(v) = init_of.get(id) {
                inits.insert(name.clone(), *v);
            }
        }
        ctx.owner.insert(q, (name.clone(), true));
        storage.push((name, c, st, ce, r, s, home));
    }
    // comb outputs own their driving bit (internal readers use the macrocell feedback)
    for p in &out_ports {
        if homed.contains(&p.name) {
            continue;
        }
        let a = ctx.tbuf.get(&p.bit).map(|x| x.0.clone()).unwrap_or_else(|| p.bit.clone());
        if !ctx.owner.contains_key(&a) {
            if let Some((cn, _)) = ctx.driver.get(&a) {
                if cells[cn].ty == "$sop" || is_gate(&cells[cn].ty) {
                    ctx.owner.insert(a, (p.name.clone(), false));
                }
            }
        }
    }

    let mut design = Design { name: top.clone(), ..Default::default() };
    let new_cell = |name: &str| Cell {
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
    };
    for (name, c, st, ce, r, s, home) in &storage {
        let mut cell = new_cell(name);
        cell.storage = *st;
        cell.data = ctx.sop(&conn(c, "D")?)?;
        let clk_port = if *st == Storage::Latch { "E" } else { "C" };
        cell.clk = Some(ctx.sop(&conn(c, clk_port)?)?);
        if *ce {
            cell.ce = Some(ctx.sop(&conn(c, "E")?)?);
        }
        let both = c.ty.contains("SR");
        if *r {
            cell.ar = Some(ctx.sop(&conn(c, "R")?)?);
        }
        if *s {
            cell.ap = Some(ctx.sop(&conn(c, if both { "S" } else { "R" })?)?);
        }
        if let Some(p) = home {
            cell.is_output = true;
            cell.pin = p.loc;
            if let Some((_, e)) = ctx.tbuf.get(&p.bit).cloned() {
                cell.oe = Some(ctx.sop(&e)?);
            }
        }
        design.cells.push(cell);
    }
    for p in &out_ports {
        if homed.contains(&p.name) {
            continue;
        }
        let mut cell = new_cell(&p.name);
        cell.is_output = true;
        cell.pin = p.loc;
        match ctx.tbuf.get(&p.bit).cloned() {
            Some((a, e)) => {
                cell.data = ctx.sop_of_owned(&a, &p.name)?;
                cell.oe = Some(ctx.sop(&e)?);
            }
            None => {
                if !ctx.driver.contains_key(&p.bit) && matches!(p.bit, Bit::Net(_)) && p.dir == "inout" {
                    // inout used only as an input
                    continue;
                }
                cell.data = ctx.sop_of_owned(&p.bit, &p.name)?;
            }
        }
        design.cells.push(cell);
    }
    // buried comb nodes (the set grows while their functions are built)
    let mut done: BTreeSet<Bit> = BTreeSet::new();
    loop {
        let todo: Vec<(Bit, String)> = ctx.nodes.iter().filter(|(b, _)| !done.contains(*b)).map(|(b, n)| (b.clone(), n.clone())).collect();
        if todo.is_empty() {
            break;
        }
        for (b, n) in todo {
            done.insert(b.clone());
            let mut cell = new_cell(&n);
            cell.data = ctx.sop(&b)?;
            design.cells.push(cell);
        }
    }
    // input pins actually read
    let mut read: BTreeSet<String> = BTreeSet::new();
    for c in &design.cells {
        for s in c.inputs() {
            if let Signal::Pad(n) = s {
                read.insert(n);
            }
        }
    }
    for p in &ports {
        let is_cell = design.cell_index(&p.name).is_some();
        if (p.dir == "input" || (p.dir == "inout" && !is_cell)) && read.contains(&p.name) {
            design.inputs.insert(p.name.clone(), p.loc);
            design.input_order.push(p.name.clone());
        } else if p.dir == "input" && !read.contains(&p.name) {
            warnings.push(format!("input {} is unused", p.verilog));
        }
    }
    let mut origins = ctx.origins.clone();
    for p in &ports {
        origins.insert(p.name.clone(), p.verilog.clone());
    }
    Ok(Imported { top, design, ports, warnings, cell_stats, inits, origins })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(sanitize("q[3]"), "q3");
        assert_eq!(sanitize("\\a.b"), "a_b");
        assert_eq!(sanitize("3x"), "N3x");
    }
}
