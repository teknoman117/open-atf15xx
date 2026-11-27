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
    /// The security fuse (`G1`) is set.
    pub security: bool,
}

/// Read a JEDEC file (JESD3-C) with the `jedec` crate: STX, `*`-terminated fields, ETX and the
/// file checksum; `QF` gives the fuse count (and so the family), `F` the default, `L` fields set
/// fuses, and a `C` fuse checksum and a non-zero file checksum must be right. ATMISP's own reader
/// quirks are `atf15xx-tool`'s business (`isp::jedec`).
pub fn parse_jed_bytes(raw: &[u8]) -> Result<Fuses> {
    let quirks = jedec::Quirks::new();
    let read = |b: &[u8]| {
        jedec::JEDECFile::from_bytes(b, &quirks).map(|j| (j.f.iter().map(|b| *b).collect::<Vec<bool>>(), j.secure_fuse == Some(true)))
    };
    let (bits, security) = match read(raw) {
        Ok(x) => x,
        // jedec 0.1.0 allocates the fuse array at the first `L` field, so a file that is only
        // `QF` + `F` (every fuse at the default) fails with MissingF: read a copy with a no-op
        // `L0` (fuse 0 at the default) and the file checksum waived (candidate upstream fix)
        Err(jedec::JedParserError::MissingF) => match default_only_copy(raw) {
            Some(copy) => read(&copy).map_err(|e| anyhow!("JEDEC: {e}"))?,
            None => return Err(anyhow!("JEDEC: {}", jedec::JedParserError::MissingF)),
        },
        Err(e) => return Err(anyhow!("JEDEC: {e}")),
    };
    let qf = bits.len();
    let family = atf15xx_arch::Family::ALL
        .into_iter()
        .find(|f| f.fuse_count() == qf)
        .ok_or_else(|| anyhow!("QF{qf} is not an ATF1502AS/1504AS/1508AS fuse count"))?;
    Ok(Fuses { family, bits, security })
}

/// For a JEDEC file with an `F` default and no `L` field: a copy with `L0 <default>` after the `F`
/// field and the file checksum set to `0000` (see [`parse_jed_bytes`]).
fn default_only_copy(raw: &[u8]) -> Option<Vec<u8>> {
    let stx = raw.iter().position(|&b| b == 0x02)?;
    let etx = stx + raw[stx..].iter().position(|&b| b == 0x03)?;
    let body = &raw[stx + 1..etx];
    let fields: Vec<&[u8]> = body.split(|&b| b == b'*').collect();
    let is_field = |f: &[u8], c: u8| f.iter().find(|b| !b.is_ascii_whitespace()) == Some(&c);
    if fields.iter().skip(1).any(|f| is_field(f, b'L')) {
        return None;
    }
    let fi = fields.iter().skip(1).position(|f| is_field(f, b'F'))? + 1;
    let default = *fields[fi].iter().rev().find(|b| !b.is_ascii_whitespace())?;
    let mut out = raw[..=stx].to_vec();
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            out.push(b'*');
        }
        out.extend_from_slice(f);
        if i == fi {
            out.extend_from_slice(b"*L0 ");
            out.push(default);
        }
    }
    out.extend_from_slice(b"\x030000");
    Some(out)
}

/// [`parse_jed_bytes`] of a file read as text (characters are bytes, as [`read_jed`] reads them).
pub fn parse_jed(text: &str) -> Result<Fuses> {
    let raw: Vec<u8> = text.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect();
    parse_jed_bytes(&raw)
}

/// Read a JEDEC file and return its fuse array.
pub fn read_jed(path: &std::path::Path) -> Result<Fuses> {
    let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    parse_jed_bytes(&raw).with_context(|| path.display().to_string())
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
        return Err(anyhow!("{} is a {} fuse map but device {} is a {}", jed.display(), fz.family, dev.name, dev.family));
    }
    Ok(model::Model::new(dev, &fz.bits, opts))
}
