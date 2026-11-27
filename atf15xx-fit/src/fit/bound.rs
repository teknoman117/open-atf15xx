//! The block-assignment problem of a fit, for the exact bound (spec F4.6c, `fit-partition-bound`).

use super::*;

/// The block-assignment problem of a fit, for exact solvers (spec F4.6c; the ILP bound
/// `fit-partition-bound` in `atf15xx-synth`, feature `ilp`): each item (a cell, or the cells sharing one macrocell) with the UIM
/// signals it reads (as the fitter counts them, [`uim_inputs`]), the macrocells it occupies
/// (itself plus its cascade helpers) and, if it drives a pin, the block it is fixed to; per
/// block the macrocells available to items (16 minus foldback-only hosts), and the block each
/// item has in this fit.
#[derive(Clone, Debug)]
pub struct BlockProblem {
    pub blocks: usize,
    pub capacity: Vec<usize>,
    pub items: Vec<BlockItem>,
}

#[derive(Clone, Debug)]
pub struct BlockItem {
    pub cells: Vec<String>,
    pub inputs: Vec<String>,
    pub mcs: usize,
    pub fixed: Option<usize>,
    pub placed: usize,
}

impl BlockProblem {
    /// Distinct UIM inputs of each block for an assignment (item -> block).
    pub fn fan_in(&self, assign: &[usize]) -> Vec<usize> {
        let mut sets: Vec<BTreeSet<&str>> = vec![BTreeSet::new(); self.blocks];
        for (it, &b) in self.items.iter().zip(assign) {
            sets[b].extend(it.inputs.iter().map(|s| s.as_str()));
        }
        sets.iter().map(|s| s.len()).collect()
    }
}

pub fn block_problem(r: &FitResult) -> BlockProblem {
    let f = r.device.family;
    let nmc = f.macrocells();
    let dec = atf15xx_arch::decode::decode(f, &r.fuses.bits);
    let mut at: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    let mut pinned: BTreeSet<u16> = BTreeSet::new();
    for (name, p) in &r.placement {
        let (mc, pin) = match p {
            Place::Pin { mc, .. } => (*mc, true),
            Place::Node { mc } => (*mc, false),
            Place::Dedicated { .. } => continue,
        };
        if let Some(i) = r.design.cell_index(name) {
            at.entry(mc).or_default().push(i);
            if pin {
                pinned.insert(mc);
            }
        }
    }
    let has_cell = |m: u16| at.contains_key(&m);
    // cascade helpers: MCs below a cell that pass their sum up (cascade_out) and hold no cell
    let helpers = |mc: u16| -> usize {
        let mut k = 0;
        let mut m = mc;
        while m > 1 && (m - 2) / 16 == (mc - 1) / 16 && !has_cell(m - 1) && dec.macrocells[(m - 2) as usize].cascade_out {
            k += 1;
            m -= 1;
        }
        k
    };
    let helper_mcs: BTreeSet<u16> = at.keys().flat_map(|&mc| (mc - helpers(mc) as u16..mc).collect::<Vec<_>>()).collect();
    let blocks = nmc / 16;
    let mut capacity = vec![16usize; blocks];
    for m in 1..=nmc as u16 {
        let c = &dec.macrocells[(m - 1) as usize];
        let used = c.pt_use.iter().any(|u| !matches!(u, atf15xx_arch::decode::PtUse::Off));
        if used && !has_cell(m) && !helper_mcs.contains(&m) {
            capacity[((m - 1) / 16) as usize] -= 1; // a foldback-only host
        }
    }
    let items = at
        .iter()
        .map(|(&mc, cells)| {
            let mut ins: BTreeSet<String> = BTreeSet::new();
            for &i in cells {
                for s in uim_inputs(&r.design, &r.design.cells[i]) {
                    ins.insert(format!("{s:?}"));
                }
            }
            let b = ((mc - 1) / 16) as usize;
            BlockItem {
                cells: cells.iter().map(|&i| r.design.cells[i].name.clone()).collect(),
                inputs: ins.into_iter().collect(),
                mcs: 1 + helpers(mc),
                fixed: pinned.contains(&mc).then_some(b),
                placed: b,
            }
        })
        .collect();
    BlockProblem { blocks, capacity, items }
}
