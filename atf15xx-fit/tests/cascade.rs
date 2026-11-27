//! Cascade chains (spec F4.3, `atf15xx-fit/src/capacity.rs`): sums wider than one macrocell, fitted
//! and checked in the simulator. The designs are those of `tools/fuzzers/fitter-cascade` (whose
//! vendor results show the chain layout): Y = OR over i < N of (S == i) & D_i, with a 6-bit
//! select S, so the sum needs exactly N product terms.
use atf15xx_arch::{Device, Family};
use atf15xx_fit::capacity::{helpers_for, MAX_CHAIN_PTS};
use atf15xx_fit::{tt2::Pla, Options, Place};
use atf15xx_sim::model::{Model, Options as SimOptions};
use atf15xx_sim::sim::Sim;
use std::collections::BTreeMap;

const SEL: usize = 6;

fn tt2(n: usize, ypin: Option<u16>) -> String {
    let sel: Vec<String> = (0..SEL).map(|k| format!("S{k}")).collect();
    let data: Vec<String> = (0..n).map(|i| format!("D{i}")).collect();
    let ins: Vec<String> = sel.iter().chain(&data).cloned().collect();
    let pins: Vec<String> =
        ins.iter().map(|x| format!("{x}+")).chain([format!("Y+{}", ypin.map_or(String::new(), |p| format!(":{p}")))]).collect();
    let mut t = format!(
        "#$ DEVICE  f1504tqfp100\n#$ PINS  {} {} \n.i {}\n.o 1\n.type f\n.ilb  {} \n.ob   Y \n.phase 1\n.p {n}\n",
        pins.len(),
        pins.join(" "),
        ins.len(),
        ins.join(" ")
    );
    for i in 0..n {
        let s: String = (0..SEL).map(|k| if (i >> k) & 1 == 1 { '1' } else { '0' }).collect();
        let d: String = (0..n).map(|j| if j == i { '1' } else { '-' }).collect();
        t += &format!("{s}{d} 1\n");
    }
    t + ".e\n"
}

fn pin_of(r: &atf15xx_fit::FitResult, name: &str) -> (u16, u16) {
    match r.placement.iter().find(|(n, _)| n == name).map(|(_, p)| p) {
        Some(Place::Pin { pin, mc }) => (*pin, *mc),
        p => panic!("{name}: {p:?}"),
    }
}

fn check(n: usize, ypin: Option<u16>) {
    let dev = Device::by_name("P1504T100").unwrap();
    let pla = Pla::parse(&tt2(n, ypin)).unwrap();
    let opts = Options { device: Some(dev.name.to_string()), ..Options::default() };
    let r = atf15xx_fit::fit(Family::F1504, &pla, &opts).unwrap_or_else(|e| panic!("{n} terms: {e}"));
    let (yp, ymc) = pin_of(&r, "Y");
    let m = Model::new(dev, &r.fuses.bits, SimOptions::default());
    // the chain: Y's macrocell and the helpers directly below it, all with product terms
    let h = helpers_for(n) as u16;
    for k in 0..=h {
        assert!(m.mc(ymc - k).has_pts(), "{n} terms: MC{} (Y at MC{ymc}) has no product terms", ymc - k);
    }
    let below = ymc - h - 1;
    assert!(below == 0 || (below - 1) / 16 != (ymc - 1) / 16 || !m.mc(below).has_pts(), "{n} terms: chain longer than {h} helpers");
    // the function: for every select value, D_i alone high and D_i alone low
    let sp: Vec<u16> = (0..SEL).map(|k| pin_of(&r, &format!("S{k}")).0).collect();
    let dp: Vec<u16> = (0..n).map(|i| pin_of(&r, &format!("D{i}")).0).collect();
    let mut sim = Sim::new(&m);
    for s in 0..(1usize << SEL) {
        for alone_high in [true, false] {
            let mut ext = BTreeMap::new();
            for (k, &p) in sp.iter().enumerate() {
                ext.insert(p, (s >> k) & 1 == 1);
            }
            for (i, &p) in dp.iter().enumerate() {
                ext.insert(p, (i == s) == alone_high);
            }
            sim.step(&ext);
            let want = s < n && alone_high;
            assert_eq!(sim.pin_value(yp), want, "{n} terms: S = {s}, D{s} alone {}", if alone_high { "high" } else { "low" });
        }
    }
}

#[test]
fn chains_compute_the_sum() {
    for n in [6, 10, 11, 12, 15, 16, 20, 25] {
        check(n, None);
        check(n, Some(46)); // MC38, mid-block
    }
}

#[test]
fn sums_beyond_the_longest_chain_are_refused() {
    let dev = Device::by_name("P1504T100").unwrap();
    let pla = Pla::parse(&tt2(MAX_CHAIN_PTS + 1, None)).unwrap();
    let opts = Options { device: Some(dev.name.to_string()), ..Options::default() };
    let e = atf15xx_fit::fit(Family::F1504, &pla, &opts).err().expect("no fit");
    assert!(e.is_capacity(), "{e}");
}

/// The chain's layout (2026-11-27, as the vendor builds it, `tools/fuzzers/fitter-cascade`): the
/// products in the order 1, 2, ..., n-1, 0 fill the helpers from the deepest up, five each, and
/// the cell's own PTs take the rest, in PT order.
#[test]
fn chains_are_laid_out_as_the_vendor_does() {
    let dev = Device::by_name("P1504T100").unwrap();
    for (n, want) in [
        (6, vec![(37, vec![1, 2, 3, 4, 5]), (38, vec![0])]),
        (8, vec![(37, vec![1, 2, 3, 4, 5]), (38, vec![6, 7, 0])]),
        (12, vec![(36, vec![1, 2, 3, 4, 5]), (37, vec![6, 7, 8, 9, 10]), (38, vec![11, 0])]),
    ] {
        let pla = Pla::parse(&tt2(n, Some(46))).unwrap();
        let opts = Options { device: Some(dev.name.to_string()), ..Options::default() };
        let r = atf15xx_fit::fit(Family::F1504, &pla, &opts).unwrap();
        let m = Model::new(dev, &r.fuses.bits, SimOptions::default());
        let data_mc: Vec<u16> = (0..n).map(|i| pin_of(&r, &format!("D{i}")).1).collect();
        for (mc, products) in want {
            // which D input each sum PT of `mc` reads, in PT order
            let got: Vec<usize> = m
                .mc(mc)
                .sum_pts()
                .map(|p| {
                    p.iter()
                        .find_map(|l| match l {
                            atf15xx_sim::model::Lit::Sig(atf15xx_sim::model::Sig::Pad(pm), true) => data_mc.iter().position(|d| d == pm),
                            _ => None,
                        })
                        .unwrap()
                })
                .collect();
            assert_eq!(got, products, "{n} terms, MC{mc}");
        }
    }
}
