//! Device selection: `ATF1504AS-PLCC44`, `ATF1508ASV-TQFP100`, `ATF1502AS-10JC44` (Microchip
//! ordering codes), fitter names (`P1504C44`) or CUPL names (`f1504plcc44`).

use crate::Device;

#[derive(Clone, Debug)]
pub struct Part {
    /// Fitter device (pin map, family).
    pub device: &'static Device,
    /// Chip name for ATMISP / atf15xx-tool (selects the IDCODE): `ATF1504AS`, `ATF1504ASV`, …
    pub chip: String,
}

const VARIANTS: [&str; 4] = ["ASVL", "ASV", "ASL", "AS"];

/// Parse a part string.
pub fn parse(s: &str) -> Result<Part, String> {
    if let Some(d) = Device::by_name(s) {
        return Ok(Part { device: d, chip: format!("ATF{}AS", d.family.number()) });
    }
    let up = s.to_ascii_uppercase().replace('_', "-");
    let rest = up.strip_prefix("ATF").ok_or_else(|| bad(s))?;
    let fam = rest.get(..4).ok_or_else(|| bad(s))?;
    if !["1502", "1504", "1508"].contains(&fam) {
        return Err(format!("{s}: only ATF1502/1504/1508 AS-series parts are supported"));
    }
    let rest = &rest[4..];
    let variant = VARIANTS.iter().find(|v| rest.starts_with(**v)).ok_or_else(|| {
        if rest.starts_with("BE") || rest.starts_with("RE") {
            format!("{s}: BE/RE parts are not supported by the fitter (AS series only)")
        } else {
            bad(s)
        }
    })?;
    let chip = format!("ATF{fam}{variant}");
    let tail = rest[variant.len()..].trim_start_matches('-');
    // package: explicit name, or ordering-code letter + pin count (J = PLCC, A = TQFP, Q = PQFP)
    let package = if let Some(p) = ["PLCC", "TQFP", "PQFP"].iter().find(|p| tail.contains(**p)) {
        let i = tail.find(p).unwrap();
        let digits: String = tail[i + p.len()..].chars().take_while(|c| c.is_ascii_digit()).collect();
        format!("{p}{digits}")
    } else {
        let code = tail.trim_start_matches(|c: char| c.is_ascii_digit());
        let mut ch = code.chars();
        let kind = match ch.next() {
            Some('J') => "PLCC",
            Some('A') => "TQFP",
            Some('Q') => "PQFP",
            _ => return Err(bad(s)),
        };
        let digits: String = code.chars().skip(1).filter(|c| c.is_ascii_digit()).collect();
        format!("{kind}{digits}")
    };
    let d = Device::all()
        .find(|d| d.family.number() == fam && d.package.eq_ignore_ascii_case(&package))
        .ok_or_else(|| format!("{s}: no {package} package for the ATF{fam}"))?;
    Ok(Part { device: d, chip })
}

fn bad(s: &str) -> String {
    format!("{s}: expected a part like ATF1504AS-PLCC44 (or a fitter/CUPL device name)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts() {
        let p = parse("ATF1504AS-PLCC44").unwrap();
        assert_eq!(p.device.package, "PLCC44");
        assert_eq!(p.chip, "ATF1504AS");
        let p = parse("ATF1508ASV-TQFP100").unwrap();
        assert_eq!(p.chip, "ATF1508ASV");
        let p = parse("ATF1502AS-10JC44").unwrap();
        assert_eq!(p.device.package, "PLCC44");
        assert!(parse("ATF1504BE-TQFP44").is_err());
        assert!(parse("f1504plcc44").is_ok());
    }
}
