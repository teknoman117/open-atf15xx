//! UIM-aware positions within the blocks (spec F4.6b): move and swap buried cells until every
//! block's sources can be matched onto its multiplexers.

use super::*;

/// Local search over the macrocell positions of `movable` buried cells (within their blocks:
/// moves to free macrocells and swaps), minimising the unmatched UIM sources summed over blocks
/// (spec F4.6a). Block membership, and so each block's set of signals, stays the same, so a move
/// only re-matches the blocks that read a moved cell. Stops at zero, after 500 moves without
/// improvement, or when the start is hopeless (more than 4 unmatched per block on average).
pub(super) fn optimize_positions(
    design: &Design,
    place_of: &mut BTreeMap<String, Place>,
    movable: &[String],
    reserved: &BTreeSet<u16>,
    f: Family,
) {
    use rand::{Rng, SeedableRng};
    if movable.is_empty() {
        return;
    }
    let nb = (f.macrocells() / 16) as usize;
    let mc_of = |p: &BTreeMap<String, Place>, n: &str| match p.get(n) {
        Some(Place::Pin { mc, .. }) | Some(Place::Node { mc }) => Some(*mc),
        _ => None,
    };
    let blk = |mc: u16| ((mc - 1) / 16) as usize;
    // signals read by each block, and the blocks reading each movable cell
    let mut sig: Vec<BTreeSet<Signal>> = vec![BTreeSet::new(); nb];
    for c in &design.cells {
        if let Some(mc) = mc_of(place_of, &c.name) {
            sig[blk(mc)].extend(uim_inputs(design, c));
        }
    }
    let sig: Vec<Vec<Signal>> = sig.into_iter().map(|s| s.into_iter().collect()).collect();
    let readers: BTreeMap<&str, Vec<usize>> = movable
        .iter()
        .map(|n| {
            let r = (0..nb).filter(|&b| sig[b].iter().any(|s| matches!(s, Signal::Fb(m, _) if m == n))).collect();
            (n.as_str(), r)
        })
        .collect();
    let block_cost = |p: &BTreeMap<String, Place>, b: usize| -> usize {
        let need: Vec<Vec<Source>> = sig[b].iter().map(|s| est_sources(design, p, s)).filter(|v| !v.is_empty()).collect();
        uim_unmatched(f, &need)
    };
    let mut costs: Vec<usize> = (0..nb).map(|b| block_cost(place_of, b)).collect();
    let mut cur: usize = costs.iter().sum();
    let start = cur;
    if cur == 0 || cur > 4 * nb {
        return;
    }
    let original = place_of.clone();
    let mut at: BTreeMap<u16, String> = design.cells.iter().filter_map(|c| mc_of(place_of, &c.name).map(|m| (m, c.name.clone()))).collect();
    let is_movable: BTreeSet<&str> = movable.iter().map(|s| s.as_str()).collect();
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(1);
    let mut stale = 0;
    while cur > 0 && stale < 500 {
        stale += 1;
        let a = &movable[rng.random_range(0..movable.len())];
        let ma = mc_of(place_of, a).unwrap();
        let target = (blk(ma) * 16 + 1 + rng.random_range(0..16)) as u16;
        if target == ma || reserved.contains(&target) {
            continue;
        }
        let other = at.get(&target).cloned();
        if other.as_deref().map_or(false, |o| !is_movable.contains(o)) {
            continue;
        }
        let apply = |p: &mut BTreeMap<String, Place>, x: u16, y: u16| {
            p.insert(a.clone(), Place::Node { mc: x });
            if let Some(o) = &other {
                p.insert(o.clone(), Place::Node { mc: y });
            }
        };
        apply(place_of, target, ma);
        let mut touched: BTreeSet<usize> = readers[a.as_str()].iter().copied().collect();
        if let Some(o) = &other {
            touched.extend(readers[o.as_str()].iter().copied());
        }
        let new: Vec<(usize, usize)> = touched.iter().map(|&b| (b, block_cost(place_of, b))).collect();
        let next = cur - new.iter().map(|&(b, _)| costs[b]).sum::<usize>() + new.iter().map(|&(_, c)| c).sum::<usize>();
        if next <= cur {
            if next < cur {
                stale = 0;
            }
            cur = next;
            for (b, c) in new {
                costs[b] = c;
            }
            at.insert(target, a.clone());
            match &other {
                Some(o) => {
                    at.insert(ma, o.clone());
                }
                None => {
                    at.remove(&ma);
                }
            }
        } else {
            apply(place_of, ma, target);
        }
    }
    if std::env::var_os("ATF_FIT_DEBUG").is_some() {
        eprintln!("positions: {start} -> {cur} unmatched UIM sources{}", if cur > 0 { " (kept the original)" } else { "" });
    }
    if cur > 0 {
        *place_of = original;
    }
}
