//! Functional model, cycle simulator and Verilog emitter for ATF1502AS/1504AS/1508AS fuse maps.
//!
//! The model is built from [`atf15xx_arch::decode::decode`] and follows the functional semantics
//! of `docs/architecture/fitter/02-fuse-map.md` §F2.8; see `docs/architecture/sim/` for the exact
//! rules, which are inferred, and the [`model::Options`] that make those choices configurable.

pub mod info;
pub mod model;
pub mod sim;
pub mod timing;
pub mod verilog;

use anyhow::{anyhow, Context, Result};
use atf15xx_arch::Device;

/// A JEDEC fuse array.
pub struct Fuses {
    pub family: atf15xx_arch::Family,
    /// `true` = fuse value 1.
    pub bits: Vec<bool>,
}

/// Parse a JEDEC file: `*`-terminated fields between STX and ETX; `QF` gives the fuse count
/// (and so the family), `F0`/`F1` the default, `L` fields set fuses. Notes and other fields are
/// ignored.
pub fn parse_jed(text: &str) -> Result<Fuses> {
    let body = text.split('\x02').nth(1).unwrap_or(text);
    let body = body.split('\x03').next().unwrap_or(body);
    let mut qf = None;
    let mut default = false;
    let mut ls: Vec<(usize, Vec<bool>)> = vec![];
    for f in body.split('*').skip(1).map(str::trim) {
        if let Some(n) = f.strip_prefix("QF") {
            qf = Some(n.trim().parse::<usize>().context("bad QF field")?);
        } else if f == "F0" || f == "F1" {
            default = f == "F1";
        } else if let Some(rest) = f.strip_prefix('L') {
            let (a, bits) = rest.split_once(|c: char| c.is_whitespace()).unwrap_or((rest, ""));
            let a: usize = a.parse().with_context(|| format!("bad L address '{a}'"))?;
            ls.push((a, bits.chars().filter(|c| *c == '0' || *c == '1').map(|c| c == '1').collect()));
        }
    }
    let qf = qf.ok_or_else(|| anyhow!("no QF field"))?;
    let family = atf15xx_arch::Family::ALL
        .into_iter()
        .find(|f| f.fuse_count() == qf)
        .ok_or_else(|| anyhow!("QF{qf} is not an ATF1502AS/1504AS/1508AS fuse count"))?;
    let mut bits = vec![default; qf];
    for (a, v) in ls {
        if a + v.len() > qf {
            return Err(anyhow!("L{a} writes past QF{qf}"));
        }
        bits[a..a + v.len()].copy_from_slice(&v);
    }
    Ok(Fuses { family, bits })
}

/// Read a JEDEC file and return its fuse array.
pub fn read_jed(path: &std::path::Path) -> Result<Fuses> {
    let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let text: String = raw.iter().map(|&b| b as char).collect();
    parse_jed(&text).with_context(|| path.display().to_string())
}

/// Look up a device by fitter name (`P1504C44`) or CUPL name (`f1504plcc44`).
pub fn device(name: &str) -> Result<&'static Device> {
    Device::by_name(name).ok_or_else(|| {
        let all: Vec<&str> = Device::all().map(|d| d.name).collect();
        anyhow!("unknown device '{name}' (known: {})", all.join(" "))
    })
}

/// Build the model of a JEDEC file for a device.
pub fn load(jed: &std::path::Path, device_name: &str, opts: model::Options) -> Result<model::Model> {
    let dev = device(device_name)?;
    let fz = read_jed(jed)?;
    if fz.family != dev.family {
        return Err(anyhow!(
            "{} is a {} fuse map but device {} is a {}",
            jed.display(),
            fz.family,
            dev.name,
            dev.family
        ));
    }
    Ok(model::Model::new(dev, &fz.bits, opts))
}
