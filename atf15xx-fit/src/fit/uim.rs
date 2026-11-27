//! UIM routing (spec F4.6): matching every block's sources onto its 40 multiplexers.

use super::*;

/// UIM routing (spec F4.7): every block's inputs matched onto its 40 multiplexers.
pub(super) fn route_uim(
    dev: &'static Device,
    design: &Design,
    placed: &Placed,
    plans: &[PtPlan],
    folds: &Folds,
) -> Result<Routing, FitError> {
    let f = dev.family;
    let Placed { uses, place_of, .. } = placed;
    let Folds { fold_at, .. } = folds;
    let source_of = |s: &Signal| signal_source(design, place_of, s);
    // ---- UIM routing
    let mut routing: BTreeMap<usize, Vec<(usize, String, String)>> = BTreeMap::new();
    let mut col_of: BTreeMap<(usize, Source), usize> = BTreeMap::new();
    let mut sel_of: BTreeMap<(usize, usize), Source> = BTreeMap::new();
    for b in 0..f.blocks() {
        let mut need: Vec<(Source, String)> = vec![];
        for m in (b * 16 + 1)..=(b * 16 + 16) {
            let u = &uses[m];
            let plan = u.cell.map(|i| &plans[i]).or_else(|| u.helper_for.and_then(|t| uses[t as usize].cell).map(|i| &plans[i]));
            let mut prods: Vec<&Vec<(Signal, bool)>> = vec![];
            if let Some(p) = fold_at.get(&(m as u16)) {
                prods.push(p);
            }
            if let Some(plan) = plan {
                if u.cell.is_some() {
                    prods.extend(plan.pts.values());
                } else if let Some(t) = u.helper_for {
                    prods.extend(helper_pts_at(plan, t, m as u16).map(|(_, p)| p));
                }
            }
            for p in prods {
                for (s, _) in p {
                    if matches!(s, Signal::Fold(_)) {
                        continue;
                    }
                    let src = source_of(s)?;
                    if !need.iter().any(|x| x.0 == src) {
                        need.push((src, s.to_string()));
                    }
                }
            }
        }
        // a macrocell holding a cell *and* the cascade helper of its neighbour also carries the
        // helper's PTs (the writer puts them there): their inputs need routing too. Added last,
        // so a block whose other PTs already read them keeps its order (and its fuses).
        for m in (b * 16 + 1)..=(b * 16 + 16) {
            let u = &uses[m];
            let (Some(_), Some(t)) = (u.cell, u.helper_for) else { continue };
            let Some(i) = uses[t as usize].cell else { continue };
            for (_, p) in helper_pts_at(&plans[i], t, m as u16) {
                for (s, _) in p {
                    if matches!(s, Signal::Fold(_)) {
                        continue;
                    }
                    let src = source_of(s)?;
                    if !need.iter().any(|x| x.0 == src) {
                        need.push((src, s.to_string()));
                    }
                }
            }
        }
        if std::env::var_os("ATF_FIT_DEBUG").is_some() {
            let mut who: Vec<String> = vec![];
            for m in (b * 16 + 1)..=(b * 16 + 16) {
                if let Some(i) = uses[m].cell {
                    let c = &design.cells[i];
                    who.push(format!("{}{}({})", c.name, if c.is_output { "*" } else { "" }, c.inputs().len()));
                }
                if let Some(i) = uses[m].hosted {
                    who.push(format!("+{}({})", design.cells[i].name, design.cells[i].inputs().len()));
                }
            }
            eprintln!("block {}: {} inputs: {}", atf15xx_arch::block_letter(b), need.len(), who.join(" "));
        }
        // an always-enabled output's pad and feedback carry the same value: either may be routed
        let alt_of = |src: Source| -> Option<Source> {
            if let Source::Pad(mc) = src {
                let u = &uses[mc as usize];
                if u.hosted.is_none() {
                    if let Some(i) = u.cell {
                        let c = &design.cells[i];
                        if c.is_output && c.oe.is_none() && !(c.registered() && c.storage == Storage::Latch) {
                            return Some(Source::Fb(mc));
                        }
                    }
                }
            }
            None
        };
        let alts: Vec<Vec<Source>> = need.iter().map(|x| std::iter::once(x.0).chain(alt_of(x.0)).collect()).collect();
        if std::env::var_os("ATF_FIT_DEBUG").is_some() && route_block_alt(f, &alts).is_none() {
            for (a, (_, name)) in alts.iter().zip(need.iter()) {
                let n: usize = a.iter().map(|s| (0..40).filter(|&m| f.uim_choices(m).contains(s)).count()).sum();
                eprintln!("uim block {b}: {name:?} {a:?}: {n} muxes");
            }
        }
        let assign = route_block_alt(f, &alts).ok_or_else(|| {
            // floating buried cells of this block, most inputs first: candidates to move elsewhere
            let mut movable: Vec<(usize, String)> = ((b * 16 + 1)..=(b * 16 + 16))
                .flat_map(|m| [uses[m].cell, uses[m].hosted])
                .flatten()
                .map(|i| &design.cells[i])
                .filter(|c| !c.is_output && (c.node.is_none() || c.node == Some(0)))
                .map(|c| (c.inputs().len(), c.name.clone()))
                .collect();
            movable.sort_by(|a, b| b.cmp(a));
            // then floating buried cells elsewhere that feed this block: another macrocell puts
            // their feedback on other UIM multiplexers
            let mut feeders: Vec<(usize, String)> = need
                .iter()
                .filter_map(|(src, _)| match src {
                    Source::Fb(m) => uses[*m as usize].cell.map(|i| &design.cells[i]),
                    _ => None,
                })
                .filter(|c| !c.is_output && (c.node.is_none() || c.node == Some(0)))
                .filter(|c| !movable.iter().any(|(_, n)| n == &c.name))
                .map(|c| (c.inputs().len(), c.name.clone()))
                .collect();
            feeders.sort();
            movable.extend(feeders);
            FitError::new(
                FitErrorKind::Routing,
                format!(
                    "UIM routing failed for block {} ({} distinct inputs) [movable: {}]",
                    atf15xx_arch::block_letter(b),
                    need.len(),
                    movable.iter().map(|x| x.1.clone()).collect::<Vec<_>>().join(",")
                ),
            )
        })?;
        let mut r = vec![];
        for ((src, name), (mux, chosen)) in need.iter().zip(assign) {
            col_of.insert((b, *src), mux);
            sel_of.insert((b, mux), chosen);
            r.push((mux, name.clone(), chosen.vendor_name()));
        }
        r.sort();
        routing.insert(b, r);
    }

    Ok(Routing { routing, col_of, sel_of })
}

/// Bipartite matching of sources onto the 40 UIM muxes of one block.
/// Like [`route_block`], but each requirement may be met by any of its alternative sources (the
/// first is preferred). Returns `(mux, source used)` per requirement.
pub fn route_block_alt(f: Family, need: &[Vec<Source>]) -> Option<Vec<(usize, Source)>> {
    let cands: Vec<Vec<(usize, Source)>> = need
        .iter()
        .map(|alts| alts.iter().flat_map(|s| (0..40).filter(|&m| f.uim_choices(m).contains(s)).map(move |m| (m, *s))).collect())
        .collect();
    let mut owner: [Option<(usize, Source)>; 40] = [None; 40];
    fn augment(i: usize, cands: &[Vec<(usize, Source)>], owner: &mut [Option<(usize, Source)>; 40], seen: &mut [bool; 40]) -> bool {
        for &(m, s) in &cands[i] {
            if seen[m] {
                continue;
            }
            seen[m] = true;
            if owner[m].map_or(true, |(j, _)| augment(j, cands, owner, seen)) {
                owner[m] = Some((i, s));
                return true;
            }
        }
        false
    }
    for i in 0..need.len() {
        let mut seen = [false; 40];
        if !augment(i, &cands, &mut owner, &mut seen) {
            return None;
        }
    }
    let mut out = vec![(0, Source::Gnd); need.len()];
    for (m, o) in owner.iter().enumerate() {
        if let Some((i, s)) = o {
            out[*i] = (m, *s);
        }
    }
    Some(out)
}

pub fn route_block(f: Family, need: &[Source]) -> Option<Vec<usize>> {
    let cands: Vec<Vec<usize>> = need.iter().map(|s| (0..40).filter(|&m| f.uim_choices(m).contains(s)).collect()).collect();
    let mut owner: [Option<usize>; 40] = [None; 40];
    fn augment(i: usize, cands: &[Vec<usize>], owner: &mut [Option<usize>; 40], seen: &mut [bool; 40]) -> bool {
        for &m in &cands[i] {
            if seen[m] {
                continue;
            }
            seen[m] = true;
            if owner[m].map_or(true, |j| augment(j, cands, owner, seen)) {
                owner[m] = Some(i);
                return true;
            }
        }
        false
    }
    for i in 0..need.len() {
        let mut seen = [false; 40];
        if !augment(i, &cands, &mut owner, &mut seen) {
            return None;
        }
    }
    let mut out = vec![0; need.len()];
    for (m, o) in owner.iter().enumerate() {
        if let Some(i) = o {
            out[*i] = m;
        }
    }
    Some(out)
}
