//! `atf15xx-sim` command-line tool (see atf15xx-sim/README.md).

use anyhow::{bail, Context, Result};
use atf15xx_sim::info;
use atf15xx_sim::model::{Model, Options, PinKind};
use atf15xx_sim::sim::Sim;
use atf15xx_sim::verilog::{self, EmitOptions};
use clap::{Args, Parser, Subcommand};
use rand::{Rng, SeedableRng};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

/// Functional model, simulator and Verilog emitter for ATF1502AS/1504AS/1508AS JEDEC files.
#[derive(Parser)]
#[command(name = "atf15xx-sim", version, about, long_about = None,
    after_help = "Model semantics, options and the equivalence flow: docs/architecture/sim/ (harness guide: 05-harness.md).")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args, Clone)]
struct ModelArgs {
    /// Device: fitter name (P1504C44) or CUPL name (f1504plcc44)
    #[arg(short, long)]
    device: String,
    /// When async reset and preset are both asserted, preset wins (default: reset wins)
    #[arg(long)]
    preset_wins: bool,
    /// Register power-up value: 0, 1, or `fuse` = each register's `Invert` fuse (prjbureau's
    /// `reset` fuse: the power-on value of the storage element)
    #[arg(long, default_value = "0", value_parser = ["0", "1", "fuse"])]
    power_up: String,
    /// Apply a cascade from MC-1 even if MC-1 has no product terms
    #[arg(long)]
    cascade_without_pts: bool,
    /// Accepted for compatibility: a cascade lender's XOR sees A = 1 is now the default
    /// (silicon-verified)
    #[arg(long, hide = true)]
    lender_a_is_one: bool,
    /// A cascade lender with PTs uses its own sum on its XOR (the old, silicon-refuted reading)
    #[arg(long)]
    lender_uses_own_sum: bool,
    /// Also clock the registers of macrocells that have no PTs and no cascade
    #[arg(long)]
    clock_idle_mcs: bool,
    /// OE code 7 uses PT5 even when PT5 is steered into the sum (decode.rs reading; default:
    /// the output is then disabled)
    #[arg(long)]
    oe_pt5_ungated: bool,
}

impl ModelArgs {
    fn options(&self) -> Options {
        Options {
            reset_wins: !self.preset_wins,
            power_up: self.power_up == "1",
            power_up_from_fuse: self.power_up == "fuse",
            cascade_requires_pts: !self.cascade_without_pts,
            lender_a_is_one: !self.lender_uses_own_sum,
            idle_mc_holds: !self.clock_idle_mcs,
            oe_pt5_needs_mux: !self.oe_pt5_ungated,
        }
    }
    fn load(&self, jed: &std::path::Path) -> Result<Model> {
        atf15xx_sim::load(jed, &self.device, self.options())
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Emit the fuse map as synthesizable Verilog (split pin interface pN_in/pN_oe/pN_out)
    Verilog {
        /// JEDEC fuse map
        jed: PathBuf,
        #[command(flatten)]
        m: ModelArgs,
        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Module name
        #[arg(long, default_value = "jed_model")]
        module: String,
        /// Also emit <module>_pins, a wrapper with real inout/input pins
        #[arg(long)]
        pins_wrapper: bool,
    },
    /// Static timing analysis: fmax, tPD, tCO, tSU (spec F8.5)
    Timing {
        /// JEDEC fuse map
        jed: PathBuf,
        #[command(flatten)]
        m: ModelArgs,
        /// Speed grade (-7, -10, -15, -20, -25)
        #[arg(long, default_value = "-10", allow_hyphen_values = true)]
        grade: String,
        /// Output supply: 5 (tOD1) or 3.3 (tOD2) for fast-slew outputs
        #[arg(long, default_value = "5", value_parser = ["5", "3.3"])]
        vccio: String,
        /// The flow's or fitter's `<design>.map.json`, to name macrocells
        #[arg(long)]
        map: Option<PathBuf>,
        /// Paths listed per section
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Fail (exit 2) if fmax is below this (MHz)
        #[arg(long)]
        fmax: Option<f64>,
    },
    /// Print the decoded logic (product terms, macrocell configuration, global networks)
    Equations {
        /// JEDEC fuse map
        jed: PathBuf,
        #[command(flatten)]
        m: ModelArgs,
    },
    /// Print a device's pins: I/O pin -> macrocell, input-only pins, dedicated and JTAG pins
    Device {
        /// Device: fitter name (P1504C44) or CUPL name (f1504plcc44)
        device: String,
    },
    /// Print every fuse field of a family (PT rows, macrocell bits, UIM / GOE selectors, config)
    /// and the inputs each UIM / GOE selector position reads
    Fuses {
        /// Family (1502, 1504, 1508) or a device of it
        family: String,
    },
    /// Show a macrocell's configuration bits and product terms, or set bits (writes a new JED)
    Mc {
        /// JEDEC fuse map
        jed: PathBuf,
        #[arg(short, long)]
        device: String,
        /// Macrocell to show (repeatable)
        #[arg(long = "mc")]
        mcs: Vec<u16>,
        /// Set a bit, e.g. MC16.CasMux=1 (repeatable; needs -o)
        #[arg(long)]
        set: Vec<String>,
        /// Output JED for --set (only digits of existing L fields change; checksum recomputed)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Resource counts: macrocells with logic, foldback-only, product terms, foldbacks, cascades
    Stats {
        /// JEDEC fuse map
        jed: PathBuf,
        #[arg(short, long)]
        device: String,
    },
    /// Simulate two fuse maps with the same random stimulus and compare every pin
    Compare {
        /// First JEDEC fuse map
        a: PathBuf,
        /// Second JEDEC fuse map (same device and pins)
        b: PathBuf,
        #[command(flatten)]
        m: ModelArgs,
        /// Number of random steps
        #[arg(long, default_value_t = 2000)]
        cycles: u64,
        /// Random seed
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Write a VCD of both devices' pins
        #[arg(long)]
        vcd: Option<PathBuf>,
    },
    /// Simulate one fuse map with a stimulus file or random stimulus and print/trace the pins
    Run {
        /// JEDEC fuse map
        jed: PathBuf,
        #[command(flatten)]
        m: ModelArgs,
        /// Stimulus CSV: a header of pin numbers, then one row of 0/1 per step (unlisted pins stay 0)
        #[arg(long)]
        stim: Option<PathBuf>,
        /// Random stimulus for this many steps (if no --stim)
        #[arg(long, default_value_t = 100)]
        cycles: u64,
        /// Random seed
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Write a VCD trace of the simulation
        #[arg(long)]
        vcd: Option<PathBuf>,
        /// Print a CSV of every pin after each step (Z = not driven, - = no output), then a column
        /// `unstable` (1 = the step did not settle: combinational loop, state afterwards undefined)
        #[arg(long)]
        print: bool,
        /// With --print: append a column `q` with every macrocell's register bit (MC1 first)
        #[arg(long)]
        print_q: bool,
    },
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

/// Random stimulus: GCLK1 toggles every step,
/// GCLK2 every other step half the time, GCLR pulses 2 % of the time, all other inputs random.
struct Stim {
    rng: rand::rngs::StdRng,
    ded: [u16; 4],
}

impl Stim {
    fn new(m: &Model, seed: u64) -> Stim {
        let d = m.device;
        Stim { rng: rand::rngs::StdRng::seed_from_u64(seed), ded: [d.gclk1, d.gclk2_oe2, d.gclr, d.oe1] }
    }
    fn next(&mut self, m: &Model, cyc: u64, prev: &BTreeMap<u16, bool>) -> BTreeMap<u16, bool> {
        let mut e = BTreeMap::new();
        for &(p, _) in &m.pins {
            e.insert(p, self.rng.random_bool(0.5));
        }
        e.insert(self.ded[0], cyc % 2 == 1);
        let g2 = if self.rng.random_bool(0.5) { (cyc / 2) % 2 == 1 } else { prev.get(&self.ded[1]).copied().unwrap_or(false) };
        e.insert(self.ded[1], g2);
        e.insert(self.ded[2], self.rng.random_bool(0.02));
        e
    }
}

struct Vcd {
    w: std::io::BufWriter<std::fs::File>,
    ids: BTreeMap<String, String>,
}

impl Vcd {
    fn new(path: &std::path::Path, scopes: &[(&str, Vec<String>)]) -> Result<Vcd> {
        let f = std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
        let mut w = std::io::BufWriter::new(f);
        writeln!(w, "$timescale 1ns $end")?;
        let mut ids = BTreeMap::new();
        let mut n = 0usize;
        for (scope, sigs) in scopes {
            writeln!(w, "$scope module {scope} $end")?;
            for s in sigs {
                let mut id = String::new();
                let mut k = n;
                loop {
                    id.push((33 + (k % 94)) as u8 as char);
                    k /= 94;
                    if k == 0 {
                        break;
                    }
                }
                n += 1;
                writeln!(w, "$var wire 1 {id} {s} $end")?;
                ids.insert(format!("{scope}.{s}"), id);
            }
            writeln!(w, "$upscope $end")?;
        }
        writeln!(w, "$enddefinitions $end")?;
        Ok(Vcd { w, ids })
    }
    fn time(&mut self, t: u64) -> Result<()> {
        writeln!(self.w, "#{t}")?;
        Ok(())
    }
    fn val(&mut self, key: &str, v: Option<bool>) -> Result<()> {
        let c = match v {
            Some(true) => '1',
            Some(false) => '0',
            None => 'z',
        };
        writeln!(self.w, "{c}{}", self.ids[key])?;
        Ok(())
    }
}

fn pin_names(m: &Model) -> Vec<String> {
    m.pins.iter().map(|(p, _)| format!("p{p}")).collect()
}

fn trace(vcd: &mut Vcd, scope: &str, s: &Sim) -> Result<()> {
    let outs = s.outputs();
    for &(p, k) in &s.model.pins {
        let v = match k {
            PinKind::Io(_) => {
                let o = outs[&p];
                if o.oe {
                    Some(o.value)
                } else {
                    Some(s.pin_value(p))
                }
            }
            _ => Some(s.pin_value(p)),
        };
        vcd.val(&format!("{scope}.p{p}"), v)?;
    }
    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Verilog { jed, m, output, module, pins_wrapper } => {
            let model = m.load(&jed)?;
            let text = verilog::emit(&model, &EmitOptions { module, pins_wrapper });
            match output {
                Some(p) => std::fs::write(&p, text).with_context(|| format!("writing {}", p.display()))?,
                None => print!("{text}"),
            }
        }
        Cmd::Timing { jed, m, grade, vccio, map, limit, fmax } => {
            let model = m.load(&jed)?;
            let p = atf15xx_sim::timing::Params::for_model(&model, &grade).map_err(anyhow::Error::msg)?;
            let v = if vccio == "3.3" { atf15xx_sim::timing::Vccio::V3_3 } else { atf15xx_sim::timing::Vccio::V5 };
            let r = atf15xx_sim::timing::analyse(&model, &p, v);
            let names = atf15xx_sim::timing::names_from_map(map.as_deref())?;
            print!("{}", r.text(&|mc| names.get(&mc).map_or(format!("MC{mc}"), |n| format!("{n} (MC{mc})")), limit));
            if let (Some(want), Some(got)) = (fmax, r.fmax_mhz) {
                if got < want {
                    eprintln!("timing: fmax {got:.1} MHz is below the required {want:.1} MHz");
                    std::process::exit(2);
                }
            }
        }
        Cmd::Equations { jed, m } => print!("{}", verilog::equations(&m.load(&jed)?)),
        Cmd::Device { device } => print!("{}", info::device_lines(atf15xx_sim::device(&device)?)),
        Cmd::Fuses { family } => {
            let f = atf15xx_arch::Family::from_number(&family)
                .or_else(|| atf15xx_arch::Device::by_name(&family).map(|d| d.family))
                .ok_or_else(|| anyhow::anyhow!("unknown family or device '{family}'"))?;
            print!("{}", info::fuse_lines(f));
        }
        Cmd::Mc { jed, device, mcs, set, output } => {
            let dev = atf15xx_sim::device(&device)?;
            let fz = atf15xx_sim::read_jed(&jed)?;
            if fz.family != dev.family {
                bail!("{} is a {} fuse map but device {} is a {}", jed.display(), fz.family, dev.name, dev.family);
            }
            if !set.is_empty() {
                let out = output.ok_or_else(|| anyhow::anyhow!("--set needs -o OUT.jed"))?;
                let sets = set.iter().map(|s| info::parse_setting(dev.family, s)).collect::<Result<Vec<_>>>()?;
                let raw = std::fs::read(&jed).with_context(|| format!("reading {}", jed.display()))?;
                let text: String = raw.iter().map(|&b| b as char).collect();
                let patched = info::patch_jed(&text, &sets)?;
                std::fs::write(&out, patched.chars().map(|c| c as u8).collect::<Vec<u8>>())
                    .with_context(|| format!("writing {}", out.display()))?;
                eprintln!("wrote {} ({} fuses set; run tools/silicon/jtag_guard.py before programming)", out.display(), sets.len());
            } else {
                let dec = atf15xx_arch::decode::decode(dev.family, &fz.bits);
                for mc in mcs {
                    print!("{}", info::mc_lines(&dec, dev, mc)?);
                }
            }
        }
        Cmd::Stats { jed, device } => {
            let dev = atf15xx_sim::device(&device)?;
            let fz = atf15xx_sim::read_jed(&jed)?;
            let s = info::stats(&atf15xx_arch::decode::decode(dev.family, &fz.bits));
            println!(
                "macrocells {}\nfoldback_only {}\nproduct_terms {}\nfoldbacks {}\ncascades {}",
                s.macrocells, s.foldback_only, s.product_terms, s.foldbacks, s.cascades
            );
        }
        Cmd::Compare { a, b, m, cycles, seed, vcd } => {
            let ma = m.load(&a)?;
            let mb = m.load(&b)?;
            let mut sa = Sim::new(&ma);
            let mut sb = Sim::new(&mb);
            let mut stim = Stim::new(&ma, seed);
            let names = pin_names(&ma);
            let mut tr = match &vcd {
                Some(p) => Some(Vcd::new(p, &[("a", names.clone()), ("b", names)])?),
                None => None,
            };
            let mut prev = BTreeMap::new();
            let mut mism = 0u64;
            let mut first = None;
            for cyc in 0..cycles {
                let e = stim.next(&ma, cyc, &prev);
                sa.step(&e);
                sb.step(&e);
                prev = e;
                let (oa, ob) = (sa.outputs(), sb.outputs());
                for (p, x) in &oa {
                    let y = ob[p];
                    if x.oe != y.oe || (x.oe && x.value != y.value) {
                        mism += 1;
                        if first.is_none() {
                            first = Some((cyc, *p, *x, y));
                        }
                    }
                }
                if let Some(t) = tr.as_mut() {
                    t.time(cyc)?;
                    trace(t, "a", &sa)?;
                    trace(t, "b", &sb)?;
                }
            }
            if let Some(t) = tr.as_mut() {
                t.w.flush()?;
            }
            if mism == 0 {
                println!("PASS: equivalent over {cycles} random steps (seed {seed})");
            } else {
                let (c, p, x, y) = first.unwrap();
                println!(
                    "FAIL: {mism} pin mismatches; first at step {c}, pin {p}: A oe={} v={}  B oe={} v={}",
                    x.oe as u8, x.value as u8, y.oe as u8, y.value as u8
                );
                std::process::exit(2);
            }
        }
        Cmd::Run { jed, m, stim, cycles, seed, vcd, print, print_q } => {
            let model = m.load(&jed)?;
            let mut s = Sim::new(&model);
            let names = pin_names(&model);
            let mut tr = match &vcd {
                Some(p) => Some(Vcd::new(p, &[("dut", names)])?),
                None => None,
            };
            let steps: Vec<BTreeMap<u16, bool>> = match stim {
                Some(path) => {
                    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
                    let mut lines = text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#'));
                    let hdr: Vec<u16> = lines
                        .next()
                        .context("empty stimulus file")?
                        .split(',')
                        .map(|x| x.trim().trim_start_matches('p').parse::<u16>())
                        .collect::<Result<_, _>>()
                        .context("stimulus header must list pin numbers")?;
                    let mut v = vec![];
                    for (ln, l) in lines.enumerate() {
                        let vals: Vec<&str> = l.split(',').map(str::trim).collect();
                        if vals.len() != hdr.len() {
                            bail!("stimulus row {} has {} values, header has {}", ln + 2, vals.len(), hdr.len());
                        }
                        v.push(hdr.iter().zip(vals).map(|(&p, x)| (p, x == "1")).collect());
                    }
                    v
                }
                None => {
                    let mut st = Stim::new(&model, seed);
                    let mut prev = BTreeMap::new();
                    (0..cycles)
                        .map(|c| {
                            let e = st.next(&model, c, &prev);
                            prev = e.clone();
                            e
                        })
                        .collect()
                }
            };
            if print {
                println!(
                    "step,{},unstable{}",
                    model.pins.iter().map(|(p, _)| format!("p{p}")).collect::<Vec<_>>().join(","),
                    if print_q { ",q" } else { "" }
                );
            }
            for (i, e) in steps.iter().enumerate() {
                s.step(e);
                if let Some(t) = tr.as_mut() {
                    t.time(i as u64)?;
                    trace(t, "dut", &s)?;
                }
                if print {
                    let outs = s.outputs();
                    let row: Vec<String> = model
                        .pins
                        .iter()
                        .map(|&(p, _)| match outs.get(&p) {
                            Some(o) if o.oe => (o.value as u8).to_string(),
                            Some(_) => "Z".into(),
                            None => "-".into(),
                        })
                        .collect();
                    let q: String = if print_q { format!(",{}", s.q.iter().map(|&b| if b { '1' } else { '0' }).collect::<String>()) } else { String::new() };
                    println!("{i},{},{}{q}", row.join(","), s.last_unstable as u8);
                }
            }
            if let Some(t) = tr.as_mut() {
                t.w.flush()?;
            }
            if s.unstable_steps > 0 {
                eprintln!("warning: combinational logic did not settle in {} step(s)", s.unstable_steps);
            }
        }
    }
    Ok(())
}
