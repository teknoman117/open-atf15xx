//! The fuse map of each family, from prjbureau's `database.json` (docs/plan/prjbureau-import.md).
//!
//! From prjbureau's per-macrocell option fuses, UIM switches and GOE muxes:
//!   qf, blocks, uim_width   from `ranges`, `blocks`, the switch fuse count
//!   mcfg_fields             the address of each 32-fuse macrocell field (block, field pair), from any
//!                           option fuse in it, checked against every other one with the slot/parity
//!                           rule of `Family::mc_bit_addr`
//!   uim_fields, uim_table   each block's mux k (PT input column 16 + 2k) and the input selected by
//!                           writing 0 at each position
//!   goe_table               the same for the six global OE muxes
//! A field the database does not determine is taken from the reference family (the ATF1504AS) when
//! one rule holds there for every block; a table entry it does not determine is `Source::None`.
//! Any other inconsistency fails the build.

use crate::layout::{FORWARD_PAIRS, MC_BIT_LAYOUT};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const BLOCKS: &str = "ABCDEFGH";

pub struct Fam {
    pub number: String,
    pub qf: usize,
    pub blocks: usize,
    pub uim_width: usize,
    /// (block, pair) -> address
    pub mcfg: BTreeMap<(usize, (u8, u8)), usize>,
    /// (block, mux) -> address
    pub uim_fields: BTreeMap<(usize, usize), usize>,
    pub uim_table: Vec<Vec<Option<String>>>,
    pub goe_table: Vec<Vec<Option<String>>>,
    pub inferred: Vec<String>,
}

fn option_fuses(bit: &str, fuses: &[u64]) -> Vec<u64> {
    // oe_mux bits 0 and 1 share a field (their order alternates with the macrocell), bit 2 is in
    // another; both gclk_mux bits share one
    match bit {
        "OeMux0" | "OeMux1" | "Gclk0" | "Gclk1" => fuses[..2].to_vec(),
        "OeMux2" => fuses[2..3].to_vec(),
        _ => fuses[..1].to_vec(),
    }
}

fn source_name(name: &str) -> String {
    let blk = |n: usize| BLOCKS.as_bytes()[(n - 1) / 16] as char;
    if let Some(n) = name
        .strip_prefix("MC")
        .and_then(|r| r.strip_suffix("_FB"))
        .and_then(|n| n.parse().ok())
    {
        return format!("{}{}fb", blk(n), n);
    }
    if let Some(n) = name
        .strip_prefix('M')
        .and_then(|r| r.strip_suffix("_PAD"))
        .and_then(|n| n.parse::<usize>().ok())
    {
        return format!("{}{}p", blk(n), n);
    }
    match name {
        "C1_PAD" => "GCLK".into(),
        "C2_PAD" => "OE2".into(),
        "E1_PAD" => "OE1".into(),
        "R_PAD" => "GCLR".into(),
        "GND0" => "GND".into(),
        _ => panic!("prjbureau: unknown source {name}"),
    }
}

fn fuses_of(v: &Value) -> Vec<u64> {
    v.as_array()
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

/// The inputs of a one-hot-zero mux by position.
fn mux_row(mux: &Value, problems: &mut Vec<String>, what: &str) -> (usize, Vec<Option<String>>) {
    let fuses = fuses_of(&mux["fuses"]);
    let base = *fuses.iter().min().unwrap();
    let width = fuses.len();
    let mut row = vec![None; width];
    for (name, value) in mux["values"].as_object().unwrap() {
        let value = value.as_u64().unwrap();
        if name == "GND1" {
            assert_eq!(value, (1u64 << width) - 1, "{what}: GND1");
            continue;
        }
        let zeros: Vec<usize> = (0..width).filter(|n| value >> n & 1 == 0).collect();
        if zeros.len() != 1 {
            problems.push(format!("{what}: {name} = {value:b} is not one-hot-zero"));
            continue;
        }
        row[(fuses[zeros[0]] - base) as usize] = Some(source_name(name));
    }
    (base as usize, row)
}

pub fn family(dev: &Value, fam: &str) -> (Fam, Vec<String>) {
    let mut problems = vec![];
    let nb = dev["blocks"].as_object().unwrap().len();
    let widths: BTreeSet<usize> = dev["switches"]
        .as_object()
        .unwrap()
        .values()
        .map(|s| fuses_of(&s["mux"]["fuses"]).len())
        .collect();
    assert_eq!(widths.len(), 1, "{fam}: UIM widths {widths:?}");
    let width = *widths.iter().next().unwrap();
    let qf = dev["ranges"]["reserved"][1].as_u64().unwrap() as usize;
    let forward = |pair: (u8, u8)| FORWARD_PAIRS.contains(&pair) != (fam == "1502");

    // macrocell fields: (block, pair) -> base address candidates
    let mut bases: BTreeMap<(usize, (u8, u8)), BTreeMap<u64, usize>> = BTreeMap::new();
    for mc in 1..=16 * nb {
        let m = &dev["macrocells"][format!("MC{mc}")];
        let (b, l) = ((mc - 1) / 16, (mc - 1) % 16);
        let mut by_pair: BTreeMap<(u8, u8), BTreeSet<u64>> = BTreeMap::new();
        for &(bit, pair, _, opt) in MC_BIT_LAYOUT.iter() {
            let fuses = fuses_of(&m[opt]["fuses"]);
            if fuses.is_empty() {
                continue;
            }
            by_pair
                .entry(pair)
                .or_default()
                .extend(option_fuses(bit, &fuses));
        }
        for (pair, fuses) in by_pair {
            let s = if forward(pair) { l } else { 15 - l } as u64;
            for f in fuses {
                let c = f - 2 * s - ((f - 2 * s) % 2);
                *bases.entry((b, pair)).or_default().entry(c).or_default() += 1;
            }
        }
    }
    let mut mcfg = BTreeMap::new();
    for (key, cands) in &bases {
        if cands.len() != 1 {
            problems.push(format!(
                "block {} field {:?}: inconsistent bases {cands:?}",
                BLOCKS.as_bytes()[key.0] as char,
                key.1
            ));
            continue;
        }
        mcfg.insert(*key, *cands.keys().next().unwrap() as usize);
    }
    // exact bit check: every option fuse must sit where mc_bit_addr puts it
    for mc in 1..=16 * nb {
        let m = &dev["macrocells"][format!("MC{mc}")];
        let (b, l) = ((mc - 1) / 16, (mc - 1) % 16);
        for &(bit, pair, k, opt) in MC_BIT_LAYOUT.iter() {
            let fuses = fuses_of(&m[opt]["fuses"]);
            let Some(&base) = mcfg.get(&(b, pair)) else {
                continue;
            };
            if fuses.is_empty() {
                continue;
            }
            let s = if forward(pair) { l } else { 15 - l };
            let a = (base + 2 * s + (usize::from(k) ^ (s & 1))) as u64;
            if !option_fuses(bit, &fuses).contains(&a) {
                problems.push(format!(
                    "MC{mc} {bit}: expected fuse {a}, database has {fuses:?}"
                ));
            }
        }
    }

    // UIM: block switches by PT input column
    let mut uim_fields = BTreeMap::new();
    let mut rows: BTreeMap<usize, Vec<(String, Vec<Option<String>>)>> = BTreeMap::new();
    for (bn, blk) in dev["blocks"].as_object().unwrap() {
        let b = BLOCKS.find(bn.as_str()).unwrap();
        for (point, col) in blk["pterm_points"].as_object().unwrap() {
            let Some(sw) = point.strip_suffix("_P").filter(|s| s.starts_with("UIM")) else {
                continue;
            };
            let k = (col.as_u64().unwrap() as usize - 16) / 2;
            let (base, row) = mux_row(
                &dev["switches"][sw]["mux"],
                &mut problems,
                &format!("{bn} mux {k}"),
            );
            uim_fields.insert((b, k), base);
            rows.entry(k).or_default().push((bn.clone(), row));
        }
    }
    let mut uim_table = vec![];
    for k in 0..40 {
        let mut merged: Vec<Option<String>> = vec![None; width];
        for (bn, row) in rows.get(&k).into_iter().flatten() {
            for (i, s) in row.iter().enumerate() {
                let Some(s) = s else { continue };
                if merged[i].as_ref().is_some_and(|m| m != s) {
                    problems.push(format!(
                        "mux {k} position {i}: {:?} vs {s} (block {bn})",
                        merged[i]
                    ));
                }
                merged[i] = Some(s.clone());
            }
        }
        uim_table.push(merged);
    }

    // GOE: by address within the GOE region
    let globals = dev["globals"].as_object().unwrap();
    let goe_start = globals
        .iter()
        .filter(|(n, _)| n.starts_with("GOE"))
        .map(|(_, g)| *fuses_of(&g["mux"]["fuses"]).iter().min().unwrap())
        .min()
        .unwrap() as usize;
    let mut goe_table = vec![vec![]; 6];
    for (n, g) in globals.iter().filter(|(n, _)| n.starts_with("GOE")) {
        let (base, row) = mux_row(&g["mux"], &mut problems, n);
        let idx = (base - goe_start) / width;
        let ours = if fam == "1502" { 6 - idx } else { idx + 1 };
        goe_table[ours - 1] = row;
    }
    (
        Fam {
            number: fam.into(),
            qf,
            blocks: nb,
            uim_width: width,
            mcfg,
            uim_fields,
            uim_table,
            goe_table,
            inferred: vec![],
        },
        problems,
    )
}

/// Fill missing macrocell fields from a rule that holds in the reference family: a missing field
/// sits at the same distance from another field of the same block as in every block of the
/// reference. The inferred address must be a free 32-fuse slot.
pub fn infer_fields(out: &mut Fam, reference: &Fam, problems: &mut Vec<String>) {
    let pairs: BTreeSet<(u8, u8)> = reference.mcfg.keys().map(|k| k.1).collect();
    let mut taken: BTreeSet<usize> = out.mcfg.values().flat_map(|&a| a..a + 32).collect();
    for b in 0..out.blocks {
        for &p in &pairs {
            if out.mcfg.contains_key(&(b, p)) {
                continue;
            }
            let mut done = false;
            for &q in &pairs {
                let Some(&qa) = out.mcfg.get(&(b, q)) else {
                    continue;
                };
                let deltas: BTreeSet<i64> = (0..reference.blocks)
                    .map(|rb| reference.mcfg[&(rb, p)] as i64 - reference.mcfg[&(rb, q)] as i64)
                    .collect();
                if deltas.len() != 1 {
                    continue;
                }
                let addr = (qa as i64 + *deltas.iter().next().unwrap()) as usize;
                if (addr..addr + 32).any(|a| taken.contains(&a)) {
                    problems.push(format!(
                        "block {} field {p:?}: inferred {addr} overlaps a field",
                        BLOCKS.as_bytes()[b] as char
                    ));
                    done = true;
                    break;
                }
                out.mcfg.insert((b, p), addr);
                taken.extend(addr..addr + 32);
                out.inferred.push(format!(
                    "block {} field {p:?} = field {q:?} + ({})",
                    BLOCKS.as_bytes()[b] as char,
                    addr as i64 - qa as i64
                ));
                done = true;
                break;
            }
            if !done {
                problems.push(format!(
                    "block {} field {p:?}: not determined",
                    BLOCKS.as_bytes()[b] as char
                ));
            }
        }
    }
}

fn source_expr(s: &Option<String>) -> String {
    match s.as_deref() {
        None => "Source::None".into(),
        Some("GND") => "Source::Gnd".into(),
        Some("GCLK") => "Source::Gclk1".into(),
        Some("GCLR") => "Source::Gclr".into(),
        Some("OE1") => "Source::Oe1".into(),
        Some("OE2") => "Source::Oe2".into(),
        Some(x) if x.ends_with("fb") => format!("Source::Fb({})", &x[1..x.len() - 2]),
        Some(x) => format!("Source::Pad({})", &x[1..x.len() - 1]),
    }
}

/// Rust source for `FAMILY_DATA` (index = `Family as usize`).
pub fn emit(fams: &[Fam]) -> String {
    let mut o = String::from("pub(crate) static FAMILY_DATA: [FamilyData; 3] = [\n");
    for f in fams {
        let mut fields: Vec<_> = f.mcfg.iter().map(|(&(b, p), &a)| (a, b, p)).collect();
        fields.sort();
        let fields: Vec<String> = fields
            .iter()
            .map(|(a, b, p)| {
                format!(
                    "McField {{ addr: {a}, block: {b}, pair: [{}, {}] }}",
                    p.0, p.1
                )
            })
            .collect();
        let uim: Vec<String> = (0..f.blocks)
            .flat_map(|b| (0..40).map(move |k| (b, k)))
            .map(|key| f.uim_fields[&key].to_string())
            .collect();
        let table = |t: &Vec<Vec<Option<String>>>| -> String {
            t.iter()
                .map(|r| {
                    format!(
                        "&[{}]",
                        r.iter().map(source_expr).collect::<Vec<_>>().join(", ")
                    )
                })
                .collect::<Vec<_>>()
                .join(",\n        ")
        };
        o += &format!(
            "    // ATF{n}AS{inf}\n    FamilyData {{\n        qf: {qf},\n        uim_width: {w},\n        mcfg_fields: &[{fields}],\n        uim_fields: &[{uim}],\n        uim_table: &[\n        {ut}],\n        goe_table: &[\n        {gt}],\n    }},\n",
            n = f.number,
            inf = if f.inferred.is_empty() { String::new() } else { format!("; inferred from the ATF1504AS: {}", f.inferred.join("; ")) },
            qf = f.qf,
            w = f.uim_width,
            fields = fields.join(", "),
            uim = uim.join(", "),
            ut = table(&f.uim_table),
            gt = table(&f.goe_table),
        );
    }
    o + "];\n"
}
