//! Foldback expanders (spec F4.3, F2.6): the macrocell that hosts each foldback product.

use super::*;

/// Foldback expanders (spec F4.6a): one macrocell per (foldback, block), hosted on free
/// macrocells or on logic with a free PT.
pub(super) fn host_foldbacks(
    design: &Design,
    opts: &Options,
    placed: &mut Placed,
    plans: &mut [PtPlan],
    fold_products: &FoldTable,
) -> Result<Folds, FitError> {
    let Placed { uses, place_of, .. } = placed;
    // ---- foldback expanders: one free macrocell per (product, block), highest first
    let mut fold_mc: BTreeMap<(String, usize), u16> = BTreeMap::new();
    for (i, c) in design.cells.iter().enumerate() {
        let b = (mc_of(place_of, &c.name) as usize - 1) / 16;
        let mut folds_needed: Vec<String> = plans[i]
            .pts
            .values()
            .chain(plans[i].helper_pts.values())
            .flatten()
            .filter_map(|(s, _)| if let Signal::Fold(k) = s { Some(k.clone()) } else { None })
            .collect();
        // a foldback product may itself read foldbacks (NAND of NANDs = OR of products, see
        // one_product): host those in the same block too, inner ones after their readers
        let mut q = 0;
        while q < folds_needed.len() {
            let inner: Vec<String> = fold_products
                .get(&folds_needed[q])
                .map(|p| p.iter().filter_map(|(s, _)| if let Signal::Fold(k) = s { Some(k.clone()) } else { None }).collect())
                .unwrap_or_default();
            for k in inner {
                if !folds_needed.contains(&k) {
                    folds_needed.push(k);
                }
            }
            q += 1;
        }
        for k in &folds_needed {
            {
                {
                    if fold_mc.contains_key(&(k.clone(), b)) {
                        continue;
                    }
                    let taken = |m: usize, fold_mc: &BTreeMap<(String, usize), u16>| fold_mc.values().any(|&x| x as usize == m);
                    // with logic doubling (dense designs) keep free macrocells free: host on logic first
                    let mut m = if opts.logic_doubling {
                        None
                    } else {
                        (b * 16 + 1..=b * 16 + 16)
                            .rev()
                            .find(|&m| uses[m].cell.is_none() && uses[m].helper_for.is_none() && !taken(m, &fold_mc))
                    };
                    if m.is_none() {
                        // host on a macrocell with logic whose plan leaves a PT free: move its PT1 there
                        // (also the reading cell itself: its PT1 becomes the foldback line and its sum
                        // reads the fold through another PT, as the vendor does)
                        m = (b * 16 + 1..=b * 16 + 16).rev().find(|&m| {
                            uses[m].helper_for.is_none()
                                && uses[m].cell.map_or(false, |j| !taken(m, &fold_mc) && fold_movable(&plans[j]).is_some())
                        });
                        if let Some(mm) = m {
                            let j = uses[mm].cell.unwrap();
                            let k2 = fold_movable(&plans[j]).unwrap();
                            let pl = &mut plans[j];
                            if let Some(p1) = pl.pts.remove(&1) {
                                pl.pts.insert(k2, p1);
                                pl.in_sum.remove(&1);
                                pl.in_sum.insert(k2);
                            }
                            pl.fold_host = true;
                        }
                    }
                    if m.is_none() {
                        m = (b * 16 + 1..=b * 16 + 16)
                            .rev()
                            .find(|&m| uses[m].cell.is_none() && uses[m].helper_for.is_none() && !taken(m, &fold_mc));
                    }
                    if m.is_none() && std::env::var_os("ATF_FIT_DEBUG").is_some() {
                        let here: Vec<&String> = fold_mc.iter().filter(|((_, bb), _)| *bb == b).map(|((k, _), _)| k).collect();
                        eprintln!("fold hosting: block {b}: {} foldbacks hosted, need {k}; macrocells:", here.len());
                        for h in &here {
                            eprintln!("  hosted: {h}");
                        }
                        for m in b * 16 + 1..=b * 16 + 16 {
                            let why = match uses[m].cell {
                                None if uses[m].helper_for.is_some() => "cascade helper".to_string(),
                                None => "free".into(),
                                Some(j) => {
                                    let pl = &plans[j];
                                    format!(
                                        "{} pts {:?} sum {:?} fold_host {} hosted {} helpers {}",
                                        design.cells[j].name,
                                        pl.pts.keys().collect::<Vec<_>>(),
                                        pl.in_sum,
                                        pl.fold_host,
                                        pl.hosted.is_some(),
                                        pl.helper_pts.len()
                                    )
                                }
                            };
                            eprintln!("  MC{m}: {}{}", if taken(m, &fold_mc) { "[hosts a fold] " } else { "" }, why);
                        }
                    }
                    let Some(m) = m else {
                        return err(FitErrorKind::Capacity, format!("{}: no free macrocell for a foldback expander", c.name));
                    };
                    fold_mc.insert((k.clone(), b), m as u16);
                }
            }
        }
    }
    let fold_at: BTreeMap<u16, Vec<(Signal, bool)>> = fold_mc.iter().map(|((k, _), &m)| (m, fold_products[k].clone())).collect();

    Ok(Folds { fold_mc, fold_at })
}
