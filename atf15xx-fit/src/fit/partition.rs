//! Block assignment of the floating buried cells (spec F4.6a, F9.4): a greedy seed improved by
//! moves and swaps under the 40-input UIM limit.

use super::*;

/// Assign the floating buried cells (not outputs, not yet placed) to blocks so that every block's
/// distinct UIM inputs stay within 40 and the worst block is as small as possible (spec F9.4).
/// Capacity per block = its macrocells not taken by placed cells or reserved helpers; a cell
/// takes 1 + its cascade helpers. Cost, compared lexicographically: inputs over 40 summed over
/// blocks, the largest block, the sum over blocks. Greedy seed (most inputs first, block whose
/// input set grows least), then deterministic local search with moves and swaps.
pub(super) fn partition_blocks(
    design: &Design,
    place_of: &BTreeMap<String, Place>,
    reserved: &BTreeSet<u16>,
    demand: &[usize],
    nb: u16,
    avoid: &BTreeSet<(String, u16)>,
    // the attempt's fan-in budget (<= 40): leaves headroom for the UIM matching, which can fail
    // below 40 because each mux selects only some sources
    limit: usize,
) -> BTreeMap<String, u16> {
    let nb = nb as usize;
    // signals interned once (the local search below moves cells thousands of times; hashing
    // signal names on every move dominated the fitter's profile, 2026-11-25)
    let mut ids: BTreeMap<Signal, u32> = BTreeMap::new();
    let cell_ids: Vec<Vec<u32>> = design
        .cells
        .iter()
        .map(|c| {
            uim_inputs(design, c)
                .into_iter()
                .map(|s| {
                    let n = ids.len() as u32;
                    *ids.entry(s).or_insert(n)
                })
                .collect()
        })
        .collect();
    let nsig = ids.len();
    // counts.n[b][signal] = number of cells in block b reading it; counts.distinct[b] = signals > 0
    #[derive(Clone)]
    struct Counts {
        n: Vec<Vec<u32>>,
        distinct: Vec<usize>,
    }
    impl Counts {
        fn add(&mut self, b: usize, s: &[u32]) {
            for &x in s {
                let c = &mut self.n[b][x as usize];
                if *c == 0 {
                    self.distinct[b] += 1;
                }
                *c += 1;
            }
        }
        fn remove(&mut self, b: usize, s: &[u32]) {
            for &x in s {
                let c = &mut self.n[b][x as usize];
                *c -= 1;
                if *c == 0 {
                    self.distinct[b] -= 1;
                }
            }
        }
    }
    let mut counts = Counts { n: vec![vec![0; nsig]; nb], distinct: vec![0; nb] };
    let mut cap: Vec<i64> = vec![16; nb];
    for m in reserved {
        cap[(*m as usize - 1) / 16] -= 1;
    }
    let mut floating: Vec<usize> = vec![];
    for (i, c) in design.cells.iter().enumerate() {
        match place_of.get(&c.name) {
            Some(Place::Pin { mc, .. }) | Some(Place::Node { mc }) => {
                let b = (*mc as usize - 1) / 16;
                counts.add(b, &cell_ids[i]);
                cap[b] -= 1;
            }
            None if !c.is_output => floating.push(i),
            _ => {}
        }
    }
    // macrocells that are placed (by pin) but carry no cell are still free for buried logic
    let size = |i: usize| 1 + helpers_for(demand[i]) as i64;
    // cell index -> block (NONE: not placed by the partitioner)
    const NONE: usize = usize::MAX;
    let mut blk: Vec<usize> = vec![NONE; design.cells.len()];
    let allowed = |i: usize, b: usize| !avoid.contains(&(design.cells[i].name.clone(), b as u16));
    let mut order = floating.clone();
    order.sort_by_key(|&i| std::cmp::Reverse(cell_ids[i].len()));
    for &i in &order {
        let s = &cell_ids[i];
        let mut best: Option<((usize, usize, usize), usize)> = None;
        for b in 0..nb {
            if cap[b] < size(i) || !allowed(i, b) {
                continue;
            }
            let grow = s.iter().filter(|&&x| counts.n[b][x as usize] == 0).count();
            let after = counts.distinct[b] + grow;
            let key = (after.saturating_sub(limit), grow, after);
            if best.as_ref().map_or(true, |(k, _)| key < *k) {
                best = Some((key, b));
            }
        }
        let Some((_, b)) = best else { continue };
        counts.add(b, s);
        cap[b] -= size(i);
        blk[i] = b;
    }
    let cost = |counts: &Counts| -> (usize, usize, usize) {
        let d = &counts.distinct;
        (d.iter().map(|x| x.saturating_sub(limit)).sum(), *d.iter().max().unwrap_or(&0), d.iter().sum())
    };
    let placed: Vec<usize> = order.iter().copied().filter(|&i| blk[i] != NONE).collect();
    if placed.len() > 1 && nb > 1 {
        // several independently seeded local searches from the greedy seed; keep the best (one
        // run alone was found to depend on its random sequence)
        let start = (counts.clone(), cap.clone(), blk.clone());
        let mut best: Option<((usize, usize, usize), Counts, Vec<usize>)> = None;
        for restart in 0..8u64 {
            let (mut counts, mut cap, mut blk) = start.clone();
            let mut cur = cost(&counts);
            use rand::{Rng, SeedableRng};
            let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(restart);
            let mut rnd = |n: usize| rng.random_range(0..n);
            for _ in 0..20000 {
                let i = placed[rnd(placed.len())];
                let from = blk[i];
                let to = rnd(nb);
                if to == from || !allowed(i, to) {
                    continue;
                }
                let si = &cell_ids[i];
                if cap[to] >= size(i) {
                    counts.remove(from, si);
                    counts.add(to, si);
                    let c = cost(&counts);
                    if c <= cur {
                        cur = c;
                        cap[from] += size(i);
                        cap[to] -= size(i);
                        blk[i] = to;
                    } else {
                        counts.remove(to, si);
                        counts.add(from, si);
                    }
                } else {
                    // swap with a cell of the target block
                    let cands: Vec<usize> = placed.iter().copied().filter(|&j| blk[j] == to).collect();
                    if cands.is_empty() {
                        continue;
                    }
                    let j = cands[rnd(cands.len())];
                    if !allowed(j, from) || cap[from] + size(i) - size(j) < 0 || cap[to] + size(j) - size(i) < 0 {
                        continue;
                    }
                    let sj = &cell_ids[j];
                    counts.remove(from, si);
                    counts.remove(to, sj);
                    counts.add(to, si);
                    counts.add(from, sj);
                    let c = cost(&counts);
                    if c <= cur {
                        cur = c;
                        cap[from] += size(i) - size(j);
                        cap[to] += size(j) - size(i);
                        blk[i] = to;
                        blk[j] = from;
                    } else {
                        counts.remove(to, si);
                        counts.remove(from, sj);
                        counts.add(from, si);
                        counts.add(to, sj);
                    }
                }
            }
            if best.as_ref().map_or(true, |(c, _, _)| cur < *c) {
                best = Some((cur, counts, blk));
            }
        }
        let (_, c, b) = best.unwrap();
        counts = c;
        blk = b;
        if std::env::var_os("ATF_FIT_DEBUG").is_some() {
            eprintln!("partition: {} floating cells, per-block inputs {:?}", placed.len(), counts.distinct);
        }
    }
    blk.into_iter().enumerate().filter(|(_, b)| *b != NONE).map(|(i, b)| (design.cells[i].name.clone(), b as u16)).collect()
}

/// UIM sources a signal can be routed from, given the current placement (before PT planning).
pub(super) fn est_sources(design: &Design, place_of: &BTreeMap<String, Place>, s: &Signal) -> Vec<Source> {
    match s {
        Signal::Pad(n) => match place_of.get(n) {
            Some(Place::Pin { mc, .. }) => vec![Source::Pad(*mc)],
            Some(Place::Dedicated { func, .. }) => vec![match func {
                Dedicated::Gclk1 => Source::Gclk1,
                Dedicated::Gclr => Source::Gclr,
                Dedicated::Oe1 => Source::Oe1,
                Dedicated::Gclk2Oe2 => Source::Oe2,
            }],
            _ => vec![],
        },
        Signal::Fb(n, _) => match place_of.get(n) {
            Some(Place::Pin { mc, .. }) if design.cell(n).map_or(false, |c| c.oe.is_none()) => vec![Source::Pad(*mc), Source::Fb(*mc)],
            Some(Place::Pin { mc, .. }) | Some(Place::Node { mc }) => vec![Source::Fb(*mc)],
            _ => vec![],
        },
        Signal::Fold(_) => vec![],
    }
}

/// Number of sources of one block that cannot be matched onto its 40 UIM muxes (maximum
/// bipartite matching, as in [`route_block_alt`], counting the failures instead of stopping).
pub(super) fn uim_unmatched(f: Family, need: &[Vec<Source>]) -> usize {
    let cands: Vec<Vec<usize>> = need
        .iter()
        .map(|alts| {
            let mut v: Vec<usize> = alts.iter().flat_map(|s| (0..40).filter(move |&m| f.uim_choices(m).contains(s))).collect();
            v.sort();
            v.dedup();
            v
        })
        .collect();
    let mut owner = [None::<usize>; 40];
    fn augment(i: usize, cands: &[Vec<usize>], owner: &mut [Option<usize>; 40], seen: &mut [bool; 40]) -> bool {
        for &m in &cands[i] {
            if !seen[m] {
                seen[m] = true;
                if owner[m].map_or(true, |j| augment(j, cands, owner, seen)) {
                    owner[m] = Some(i);
                    return true;
                }
            }
        }
        false
    }
    (0..need.len()).filter(|&i| !augment(i, &cands, &mut owner, &mut [false; 40])).count()
}
