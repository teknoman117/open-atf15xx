//! `.tt2` writer for a netlist, so the same design can be given to the vendor fitter (oracle) or
//! to our `fit150x` binaries. The format is the inverse of `Design::from_pla` (spec F3).

use atf15xx_fit::logic::{Signal, Sop};
use atf15xx_fit::netlist::{Design, Storage};
use std::collections::BTreeSet;

fn col(s: &Signal, d: &Design) -> String {
    match s {
        Signal::Pad(n) if d.cell_index(n).is_some() => format!("{n}.PIN"),
        Signal::Pad(n) => n.clone(),
        Signal::Fb(n, true) => format!("{n}.Q"),
        Signal::Fb(n, false) => n.clone(),
        Signal::Fold(n) => format!("FOLD_{n}"),
    }
}

/// Render `d` as a CUPL-style `.tt2` for the CUPL device name `tt2_device` (e.g. `f1504plcc44`).
pub fn write(d: &Design, tt2_device: &str) -> Result<String, String> {
    // CUPL has no name for the internal (macrocell) feedback of a combinatorial *output*: the
    // output's name in `.ilb` is rejected ("can not be in .ilb and .olb"). Such readers get a
    // buried copy `<name>_fb` of the output's function.
    let d = &with_output_copies(d);
    let mut outs: Vec<(String, &Sop)> = vec![];
    let one = Sop::one();
    for c in &d.cells {
        if c.name.contains('$') {
            return Err(format!("internal node {} cannot be written to .tt2", c.name));
        }
        let low = if c.active_low { "-" } else { "" };
        match c.storage {
            Storage::Comb => outs.push((format!("{}{low}", c.name), &c.data)),
            Storage::Dff => outs.push((format!("{}.D{low}", c.name), &c.data)),
            Storage::Tff => outs.push((format!("{}.T{low}", c.name), &c.data)),
            Storage::Latch => outs.push((format!("{}.L{low}", c.name), &c.data)),
        }
        let ck = if c.storage == Storage::Latch { "LE" } else { "C" };
        for (suf, slot) in [(ck, &c.clk), ("CE", &c.ce), ("AR", &c.ar), ("AP", &c.ap), ("OE", &c.oe)] {
            if let Some(f) = slot {
                outs.push((format!("{}.{suf}", c.name), f));
            }
        }
        if c.is_output && c.oe.is_none() && c.storage == Storage::Comb && c.data.is_zero() {
            // keep constant-0 outputs declared
        }
        let _ = &one;
    }
    let mut cols: BTreeSet<String> = BTreeSet::new();
    for (_, f) in &outs {
        for s in f.signals() {
            cols.insert(col(&s, d));
        }
    }
    let cols: Vec<String> = cols.into_iter().collect();
    let idx = |n: &str| cols.iter().position(|c| c == n).unwrap();
    let mut rows = vec![];
    for (oi, (_, f)) in outs.iter().enumerate() {
        for p in &f.0 {
            let mut r = vec!['-'; cols.len()];
            for (s, pos) in p {
                r[idx(&col(s, d))] = if *pos { '1' } else { '0' };
            }
            let o: String = (0..outs.len()).map(|k| if k == oi { '1' } else { '0' }).collect();
            rows.push(format!("{} {o}", r.into_iter().collect::<String>()));
        }
    }
    let pins: Vec<String> = d
        .inputs
        .iter()
        .map(|(n, p)| (n.clone(), *p))
        .chain(d.cells.iter().filter(|c| c.is_output).map(|c| (c.name.clone(), c.pin)))
        .map(|(n, p)| match p {
            Some(p) => format!("{n}+:{p}"),
            None => format!("{n}+"),
        })
        .collect();
    let nodes: Vec<String> = d.cells.iter().filter(|c| !c.is_output).map(|c| format!("{}+", c.name)).collect();
    let mut s = String::new();
    let t = &d.name;
    s += &format!("#$ TOOL CUPL\n#$ TITLE {t}\n#$ MODULE {t}\n#$ JEDECFILE {t}\n#$ DEVICE {tt2_device}\n");
    s += &format!("#$ PINS {} {}\n", pins.len(), pins.join(" "));
    s += &format!("#$ NODES {} {}\n", nodes.len(), nodes.join(" "));
    s += &format!(".i {}\n.o {}\n.type f\n", cols.len(), outs.len());
    s += &format!(".ilb {}\n.ob {}\n", cols.join(" "), outs.iter().map(|o| o.0.clone()).collect::<Vec<_>>().join(" "));
    s += &format!(".phase {}\n.p {}\n", "1".repeat(outs.len()), rows.len());
    for r in rows {
        s += &r;
        s.push('\n');
    }
    s += ".e\n";
    Ok(s.replace('\n', "\r\n"))
}

fn with_output_copies(d: &Design) -> Design {
    let mut d = d.clone();
    let read: BTreeSet<String> = d
        .cells
        .iter()
        .flat_map(|c| c.inputs())
        .filter_map(|s| match s {
            Signal::Fb(n, false) => Some(n),
            _ => None,
        })
        .collect();
    let mut extra = vec![];
    for c in &d.cells {
        if c.is_output && c.storage == Storage::Comb && read.contains(&c.name) {
            let mut n = c.clone();
            n.name = format!("{}_fb", c.name);
            n.is_output = false;
            n.pin = None;
            n.node = Some(0);
            n.oe = None;
            extra.push(n);
        }
    }
    let names: Vec<String> = extra.iter().map(|e| e.name.clone()).collect();
    d.cells.extend(extra);
    for e in &names {
        let orig = e.trim_end_matches("_fb").to_string();
        let s = Signal::Fb(orig, false);
        let lit = Sop(vec![vec![(Signal::Fb(e.clone(), false), true)]]);
        let nlit = Sop(vec![vec![(Signal::Fb(e.clone(), false), false)]]);
        for c in d.cells.iter_mut() {
            c.data = c.data.substitute(&s, &lit, &nlit);
            for slot in [&mut c.clk, &mut c.ce, &mut c.ar, &mut c.ap, &mut c.oe] {
                if let Some(x) = slot.as_mut() {
                    *x = x.substitute(&s, &lit, &nlit);
                }
            }
        }
    }
    d
}
