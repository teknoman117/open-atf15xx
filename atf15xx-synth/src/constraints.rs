//! Constraints: a small line-based file (`.acf`, "ATF15xx constraints") and Verilog port
//! attributes. Format (spec §F7.5):
//!
//! ```text
//! # comment
//! device  ATF1504AS-PLCC44          # part (overridden by --device)
//! set_io  clk   43                  # pin of a port bit: name as in Verilog (q[3]) or sanitised (q3)
//! set_io  q[3:0] 4 5 6 8            # a range takes pins LSB first
//! fast    q[0]                      # fast slew rate on an output (vendor: output_fast)
//! open_collector  irq_n             # open-collector output
//! low_power  q[3:1]                 # macrocell low-power mode (vendor: mc_power)
//! bus_keeper off                    # pin keeper on all I/O pins (one global fuse; default on)
//! pullup  tms on                    # JTAG TMS / TDI pull-ups (default off)
//! input_transition_detect gclk1 off # ITD on the GCLK1/2/3 pins (default on)
//! power_down pd1 on                 # PD1 / PD2 pins as power-down inputs (default off)
//! power_reset on                    # power-on reset hysteresis option (vendor power_reset)
//! security on                       # set the security fuse (JEDEC G1)
//! jtag off                          # JTAG pins as I/O -- reprogramming then needs 12 V on OE1!
//! option  logic_doubling on         # any fitter option (-strategy KEY VALUE)
//! fmax    66.6                      # timing (spec F8.7): register-to-register target, MHz
//! clock   clk 66.6                  # ... for the registers clocked by one clock port
//! tpd     15                        # max pin-to-pin delay, ns (also tco, tsu)
//! optimize speed                    # area (default: fewest macrocells that meet the targets)
//!                                   # or speed (highest fmax, area second)
//! ```
//!
//! The bus keeper is a single device-wide fuse (config C+1; the vendor treats `pin_keep = A B`
//! like `on`), so there is no per-port keeper. With the keeper off, every unused input-capable pin
//! floats; tie unused dedicated inputs (GCLK/OE/GCLR) on the board (silicon: floating ones caused
//! intermittent errors, spec F2.9).
//!
//! Verilog attributes on ports give the same per-port settings:
//! `(* LOC = "43" *) input clk;`, `(* LOC = "4 5 6 8" *) output [3:0] q;`, `(* FAST *)`,
//! `(* OPEN_COLLECTOR *)`, `(* LOW_POWER *)`. File entries override attributes.

use crate::yosys::PortBit;
use atf15xx_fit::fit::{Options, Select};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub struct Constraints {
    pub device: Option<String>,
    /// port bit (sanitised or Verilog name) -> pin
    pub pins: BTreeMap<String, u16>,
    pub fast: BTreeSet<String>,
    pub open_collector: BTreeSet<String>,
    pub low_power: BTreeSet<String>,
    /// fitter options in order (device-level directives are stored here too, as fitter options)
    pub options: Vec<(String, Vec<String>)>,
    /// Timing targets and the optimisation goal (spec F8.7).
    pub timing: TimingTargets,
}

/// The timing targets and the goal live with the strategy search that uses them.
pub use atf15xx_fit::search::{Goal, TimingTargets};

/// `66.6`, `66.6MHz`, `15ns` → number.
fn number(w: Option<&&str>, unit: &str, e: &dyn Fn(&str) -> String) -> Result<f64, String> {
    let s = w.ok_or_else(|| e("missing value"))?.to_ascii_lowercase();
    s.trim_end_matches(unit)
        .parse::<f64>()
        .map_err(|_| e("bad number"))
        .and_then(|v| if v > 0.0 { Ok(v) } else { Err(e("must be positive")) })
}

fn onoff(w: Option<&&str>, e: &dyn Fn(&str) -> String) -> Result<String, String> {
    match w.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("on") | Some("1") | Some("yes") | Some("true") => Ok("on".into()),
        Some("off") | Some("0") | Some("no") | Some("false") => Ok("off".into()),
        _ => Err(e("expected on or off")),
    }
}

/// Expand `q[3:0]` into `q[3] q[2] q[1] q[0]`; `q[0]` and `clk` stay as they are.
fn expand(name: &str) -> Vec<String> {
    if let Some((base, rest)) = name.split_once('[') {
        if let Some(range) = rest.strip_suffix(']') {
            if let Some((a, b)) = range.split_once(':') {
                if let (Ok(a), Ok(b)) = (a.trim().parse::<i64>(), b.trim().parse::<i64>()) {
                    let (lo, hi) = (a.min(b), a.max(b));
                    let mut v: Vec<String> = (lo..=hi).map(|i| format!("{base}[{i}]")).collect();
                    if a > b {
                        v.reverse();
                    }
                    return v;
                }
            }
        }
    }
    vec![name.to_string()]
}

impl Constraints {
    pub fn parse(text: &str) -> Result<Constraints, String> {
        let mut c = Constraints::default();
        for (ln, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let w: Vec<&str> = line.split_whitespace().collect();
            let e = |m: &str| format!("line {}: {m}: {raw}", ln + 1);
            match w[0].to_ascii_lowercase().as_str() {
                "device" | "part" => c.device = Some(w.get(1).ok_or_else(|| e("missing part"))?.to_string()),
                "set_io" | "pin" | "loc" => {
                    let names = expand(w.get(1).ok_or_else(|| e("missing port"))?);
                    let pins: Vec<u16> = w[2..].iter().map(|p| p.parse().map_err(|_| e("bad pin number"))).collect::<Result<_, _>>()?;
                    // `q[3:0] 4 5 6 8` lists pins LSB first
                    let mut names = names;
                    if names.len() > 1 && names[0] > names[names.len() - 1] {
                        names.reverse();
                    }
                    if names.len() != pins.len() {
                        return Err(e("number of pins does not match the number of bits"));
                    }
                    for (n, p) in names.into_iter().zip(pins) {
                        c.pins.insert(n, p);
                    }
                }
                k @ ("fast" | "open_collector" | "low_power") => {
                    if w.len() < 2 {
                        return Err(e("missing port"));
                    }
                    let set = match k {
                        "fast" => &mut c.fast,
                        "open_collector" => &mut c.open_collector,
                        _ => &mut c.low_power,
                    };
                    for n in &w[1..] {
                        set.extend(expand(n));
                    }
                }
                "bus_keeper" | "keeper" | "pin_keep" => {
                    if w.len() > 2
                        || (w.len() == 2
                            && !matches!(w[1].to_ascii_lowercase().as_str(), "on" | "off" | "1" | "0" | "yes" | "no" | "true" | "false"))
                    {
                        return Err(e(
                            "the bus keeper is one device-wide fuse: use `bus_keeper on` or `bus_keeper off` (no per-port keeper exists)",
                        ));
                    }
                    c.options.push(("pin_keep".into(), vec![onoff(w.get(1), &e)?]));
                }
                "pullup" => {
                    let which = w.get(1).map(|s| s.to_ascii_lowercase()).ok_or_else(|| e("pullup tms|tdi on|off"))?;
                    let k = match which.as_str() {
                        "tms" => "tms_pullup",
                        "tdi" => "tdi_pullup",
                        _ => return Err(e("pullup tms|tdi on|off")),
                    };
                    c.options.push((k.into(), vec![onoff(w.get(2), &e)?]));
                }
                "input_transition_detect" | "itd" => {
                    let which = w
                        .get(1)
                        .map(|s| s.to_ascii_lowercase())
                        .ok_or_else(|| e("input_transition_detect gclk1|gclk2|gclk3|all on|off"))?;
                    let n = match which.as_str() {
                        "gclk1" => "1",
                        "gclk2" => "2",
                        "gclk3" => "3",
                        "all" => "all",
                        _ => return Err(e("input_transition_detect gclk1|gclk2|gclk3|all on|off")),
                    };
                    c.options.push(("itd_net".into(), vec![n.into(), onoff(w.get(2), &e)?]));
                }
                "power_down" => {
                    let which = w.get(1).map(|s| s.to_ascii_lowercase()).ok_or_else(|| e("power_down pd1|pd2 on|off"))?;
                    if which != "pd1" && which != "pd2" {
                        return Err(e("power_down pd1|pd2 on|off"));
                    }
                    c.options.push((which, vec![onoff(w.get(2), &e)?]));
                }
                k @ ("power_reset" | "security" | "jtag") => {
                    c.options.push((k.into(), vec![onoff(w.get(1), &e)?]));
                }
                "fmax" => c.timing.fmax = Some(number(w.get(1), "mhz", &e)?),
                "clock" => {
                    let n = w.get(1).ok_or_else(|| e("missing clock port"))?;
                    c.timing.clocks.push((n.to_string(), number(w.get(2), "mhz", &e)?));
                }
                "tpd" => c.timing.tpd = Some(number(w.get(1), "ns", &e)?),
                "tco" => c.timing.tco = Some(number(w.get(1), "ns", &e)?),
                "tsu" => c.timing.tsu = Some(number(w.get(1), "ns", &e)?),
                "optimize" | "optimise" => {
                    c.timing.goal = match w.get(1).map(|s| s.to_ascii_lowercase()).as_deref() {
                        Some("area") => Goal::Area,
                        Some("speed") | Some("timing") => Goal::Speed,
                        _ => return Err(e("expected area or speed")),
                    }
                }
                "option" | "set_option" => {
                    let k = w.get(1).ok_or_else(|| e("missing option"))?;
                    c.options.push((k.to_string(), w[2..].iter().map(|s| s.to_string()).collect()));
                }
                other => return Err(e(&format!("unknown directive {other}"))),
            }
        }
        Ok(c)
    }

    /// Apply port attributes and this file to the imported ports and the fitter options.
    /// Returns the per-port pin assignment (sanitised name -> pin) and warnings.
    pub fn apply(&self, ports: &mut [PortBit], opts: &mut Options) -> Result<Vec<String>, String> {
        let mut warn = vec![];
        let resolve = |n: &str, ports: &[PortBit]| -> Option<usize> { ports.iter().position(|p| p.verilog == n || p.name == n) };
        for (n, &pin) in &self.pins {
            let i = resolve(n, ports).ok_or_else(|| format!("constraint for unknown port {n}"))?;
            ports[i].loc = Some(pin);
        }
        let mut sel = |set: &BTreeSet<String>, attr: &str, ports: &[PortBit]| -> Result<Vec<String>, String> {
            let mut v: Vec<String> = vec![];
            for p in ports {
                if p.attrs.contains_key(attr) && p.dir != "input" {
                    v.push(p.name.clone());
                }
            }
            for n in set {
                let i = resolve(n, ports).ok_or_else(|| format!("{attr} for unknown port {n}"))?;
                if ports[i].dir == "input" {
                    warn.push(format!("{attr} on input {n} ignored"));
                    continue;
                }
                v.push(ports[i].name.clone());
            }
            v.sort();
            v.dedup();
            Ok(v)
        };
        let fast = sel(&self.fast, "fast", ports)?;
        let oc = sel(&self.open_collector, "open_collector", ports)?;
        let lp = sel(&self.low_power, "low_power", ports)?;
        for (n, list) in [("output_fast", fast), ("open_collector", oc), ("mc_power", lp)] {
            if !list.is_empty() {
                let s = Select::Only(list);
                match n {
                    "output_fast" => opts.output_fast = s,
                    "open_collector" => opts.open_collector = s,
                    _ => opts.mc_power = s,
                }
            }
        }
        for (k, v) in &self.options {
            if !atf15xx_fit::cli::apply(opts, k, v) {
                return Err(format!("unknown fitter option {k}"));
            }
        }
        Ok(warn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_directives() {
        let c = Constraints::parse(
            "bus_keeper off\npullup tms on\ninput_transition_detect gclk2 off\npower_down pd1 on\npower_reset on\njtag on\n",
        )
        .unwrap();
        let mut o = Options::default();
        c.apply(&mut [], &mut o).unwrap();
        assert!(!o.pin_keep && o.tms_pullup && !o.itd[1] && o.itd[0] && o.pd1 && o.power_reset && o.jtag);
        assert!(Constraints::parse("bus_keeper q[0]\n").unwrap_err().contains("device-wide"));
    }

    #[test]
    fn parse_file() {
        let c =
            Constraints::parse("device ATF1504AS-PLCC44\nset_io clk 43\nset_io q[3:0] 4 5 6 8 # lsb first\nfast q[0]\noption jtag on\n")
                .unwrap();
        assert_eq!(c.pins["clk"], 43);
        assert_eq!(c.pins["q[0]"], 4);
        assert_eq!(c.pins["q[3]"], 8);
        assert!(c.fast.contains("q[0]"));
        assert_eq!(c.options.len(), 1);
    }
}
