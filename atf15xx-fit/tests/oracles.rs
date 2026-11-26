//! Our home-grown logic checked against established libraries (oracles):
//!
//! * `logic.rs` (`Sop::minimize`, `Sop::complement`) against Berkeley **Espresso**
//!   (`espresso-logic`): exact correctness on exhaustive truth tables, and cover sizes;
//! * the UIM matcher (`route_block_alt`, kept because it reproduces the vendor's mux order)
//!   against **petgraph**'s maximum matching on the real UIM tables: it must route exactly when a
//!   complete matching exists.

use atf15xx_arch::{Family, Source};
use atf15xx_fit::fit::route_block_alt;
use atf15xx_fit::logic::{Signal, Sop};
use espresso_logic::{Cover, CoverType, Cube, CubeType, Minimizable};
use rand::{Rng, SeedableRng};
use std::collections::BTreeMap;

fn eval(s: &Sop, vars: &[Signal], m: u32) -> bool {
    s.0.iter().any(|p| {
        p.iter().all(|(v, pos)| {
            let i = vars.iter().position(|x| x == v).unwrap();
            ((m >> i) & 1 == 1) == *pos
        })
    })
}

fn espresso_cubes(cubes: &[Vec<Option<bool>>]) -> usize {
    let mut c = Cover::anonymous(CoverType::F);
    for k in cubes {
        c.push(Cube::anonymous(k, &[true], CubeType::F));
    }
    c.minimize().expect("espresso").num_cubes()
}

#[test]
fn logic_against_espresso() {
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(7);
    let mut minimize_excess: BTreeMap<i64, usize> = BTreeMap::new();
    let mut complement_excess: BTreeMap<i64, usize> = BTreeMap::new();
    for &n in &[4usize, 6, 8, 10] {
        let vars: Vec<Signal> = (0..n).map(|i| Signal::Pad(format!("v{i}"))).collect();
        for _ in 0..250 {
            let k = rng.random_range(1..=2 * n);
            let cubes: Vec<Vec<Option<bool>>> = (0..k)
                .map(|_| (0..n).map(|_| match rng.random_range(0..3) { 0 => Some(false), 1 => Some(true), _ => None }).collect())
                .collect();
            let f = Sop(cubes.iter().map(|c| c.iter().enumerate().filter_map(|(i, x)| x.map(|b| (vars[i].clone(), b))).collect()).collect());
            let ours = f.minimize();
            let comp = f.complement();
            for m in 0..(1u32 << n) {
                let want = eval(&f, &vars, m);
                assert_eq!(eval(&ours, &vars, m), want, "minimize changed the function: {f}");
                assert_eq!(eval(&comp, &vars, m), !want, "complement is wrong: {f}");
            }
            let e = espresso_cubes(&cubes);
            *minimize_excess.entry(ours.0.len() as i64 - e as i64).or_default() += 1;
            let off: Vec<Vec<Option<bool>>> =
                (0..(1u32 << n)).filter(|&m| !eval(&f, &vars, m)).map(|m| (0..n).map(|i| Some((m >> i) & 1 == 1)).collect()).collect();
            if !off.is_empty() {
                *complement_excess.entry(comp.0.len() as i64 - espresso_cubes(&off) as i64).or_default() += 1;
            }
        }
    }
    eprintln!("minimize: ours - espresso cubes -> functions: {minimize_excess:?}");
    eprintln!("complement: ours - espresso cubes -> functions: {complement_excess:?}");
    // quality bound for minimize (measured 2026-11-25: 991/1000 equal, 9 one cube more)
    assert!(minimize_excess.keys().all(|&d| d <= 1), "minimize more than one cube worse than Espresso");
}

#[test]
fn uim_matcher_against_petgraph() {
    use petgraph::graph::UnGraph;
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(11);
    for f in Family::ALL {
        let mut sources: Vec<Source> = (0..40).flat_map(|m| f.uim_choices(m).iter().copied()).filter(|s| !matches!(s, Source::Gnd | Source::None)).collect();
        sources.sort();
        sources.dedup();
        let (mut routed, mut tried) = (0, 0);
        for _ in 0..400 {
            let n = rng.random_range(20..=44usize).min(sources.len());
            let mut need: Vec<Source> = vec![];
            while need.len() < n {
                let s = sources[rng.random_range(0..sources.len())];
                if !need.contains(&s) {
                    need.push(s);
                }
            }
            // bipartite graph: need i (node i) -- mux m (node n + m)
            let mut g = UnGraph::<(), ()>::new_undirected();
            let nodes: Vec<_> = (0..n + 40).map(|_| g.add_node(())).collect();
            for (i, s) in need.iter().enumerate() {
                for m in 0..40 {
                    if f.uim_choices(m).contains(s) {
                        g.add_edge(nodes[i], nodes[n + m], ());
                    }
                }
            }
            let complete = petgraph::algo::maximum_matching(&g).len() == n;
            let ours = route_block_alt(f, &need.iter().map(|s| vec![*s]).collect::<Vec<_>>());
            assert_eq!(ours.is_some(), complete, "{f:?}: our matcher and petgraph disagree on {need:?}");
            if let Some(r) = ours {
                // a valid routing: distinct muxes, each selecting its source
                let mut used = std::collections::BTreeSet::new();
                for (i, (m, s)) in r.iter().enumerate() {
                    assert_eq!(*s, need[i]);
                    assert!(f.uim_choices(*m).contains(s) && used.insert(*m));
                }
                routed += 1;
            }
            tried += 1;
        }
        eprintln!("{f:?}: {routed}/{tried} random source sets routable; matcher == petgraph on all");
    }
}
