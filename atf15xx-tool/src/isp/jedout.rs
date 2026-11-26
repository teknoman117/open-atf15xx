//! JEDEC writer used by `Load` (§8): the fuse array of the device image, in our own
//! region-structured layout (§8.2).

use crate::isp::device::Family;
use crate::isp::fusemap::{DeviceImage, FuseMap};
use crate::isp::jedec::checksum;
use std::fmt::Write as _;

/// Fuses per `L` field in the logic-array region (one product term on AS devices); every field
/// stays well under ATMISP's 200-byte limit (§3.2).
const ARRAY_LINE: usize = 96;

/// §8.1: rows/registers → fuse array.
pub fn fuses_from_image(f: Family, img: &DeviceImage) -> Vec<u8> {
    let map = FuseMap::new(f);
    let qf = f.qf();
    let mut fz = vec![b'X'; qf];
    let mut mapped = vec![false; qf];
    for (r, row) in map.rows.iter().enumerate() {
        for (c, &i) in row.iter().enumerate() {
            if i >= 0 {
                fz[i as usize] = img.rows[r][c];
                mapped[i as usize] = true;
            }
        }
    }
    for (idx, data) in [(&map.special, &img.special), (&map.ues, &img.ues), (&map.config, &img.config)] {
        for (k, &i) in idx.iter().enumerate() {
            if i >= 0 {
                fz[i as usize] = data[k];
                mapped[i as usize] = true;
            }
        }
    }
    let (_, _, _, reserved, _) = f.layout();
    for i in 0..qf {
        if mapped[i] {
            continue;
        }
        if i >= reserved {
            if f.is_as() || f == Family::F1502BE {
                fz[i] = b'0';
            }
            // 1504BE / 1508BE reserved fuses stay 'X'
        } else if !f.is_as() {
            fz[i] = b'1';
        }
    }
    for c in fz.iter_mut() {
        if *c == b'X' {
            *c = b'0';
        }
    }
    fz
}

/// Device names a family's fuse map is valid for, as written in the header (§8.2).
fn chip_names(f: Family) -> &'static str {
    match f {
        Family::F1502AS => "ATF1502AS/ASL/ASV/ASVL",
        Family::F1504AS => "ATF1504AS/ASL/ASV/ASVL",
        Family::F1508AS => "ATF1508AS/ASL/ASV/ASVL",
        Family::F1502BE => "ATF1502BE",
        Family::F1504BE => "ATF1504BE",
        Family::F1508BE => "ATF1508BE/ATF1508RE",
    }
}

/// §8.2/§8.3: write a fuse array (`'0'`/`'1'` bytes, `f.qf()` of them) as a JEDEC file.
pub fn render(f: Family, fuses: &[u8], ctime: &str) -> Vec<u8> {
    assert_eq!(fuses.len(), f.qf(), "fuse array size of {f:?}");
    let (config, special, ues, reserved, _) = f.layout();
    let bits = |a: usize, b: usize| std::str::from_utf8(&fuses[a..b]).expect("fuses are ASCII");
    let mut o = String::with_capacity(f.qf() + f.qf() / ARRAY_LINE * 12 + 400);
    let nl = "\r\n";
    write!(o, "\x02JEDEC fuse map for {}, written by open-atf150x atmisp {} (Load){nl}", chip_names(f), env!("CARGO_PKG_VERSION")).unwrap();
    write!(o, "Created: {ctime}*{nl}").unwrap();
    // ATMISP writes no G field for the 1502BE (§8.2); its security fuse is never set by Load.
    let g = if f == Family::F1502BE { "" } else { " G0*" };
    write!(o, "QF{}* F0*{g}{nl}", f.qf()).unwrap();
    write!(o, "N logic array, fuses 0-{}*{nl}", config - 1).unwrap();
    for a in (0..config).step_by(ARRAY_LINE) {
        write!(o, "L{a:05} {}*{nl}", bits(a, (a + ARRAY_LINE).min(config))).unwrap();
    }
    for (a, b, label) in [
        (config, special, "configuration bits"),
        (special, ues, "special purpose bits"),
        (ues, reserved, "user electronic signature"),
        (reserved, f.qf(), "reserved"),
    ] {
        write!(o, "L{a:05} {}* N {label}*{nl}", bits(a, b)).unwrap();
    }
    write!(o, "C{:04X}*{nl}\x030000{nl}", checksum(fuses)).unwrap();
    o.into_bytes()
}

pub fn write(f: Family, img: &DeviceImage, ctime: &str) -> Vec<u8> {
    render(f, &fuses_from_image(f, img), ctime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_round_trips_through_the_reader() {
        for f in Family::ALL {
            // a pseudo-random fuse pattern, so misplaced fuses show
            let z: Vec<u8> = (0..f.qf()).map(|i| if (i * 7 + i / 13) % 3 == 0 { b'1' } else { b'0' }).collect();
            let out = render(f, &z, "Thu Nov 19 14:09:53 2026");
            let j = crate::isp::jedec::parse(&out).unwrap();
            assert_eq!(j.fuses, z, "{f:?}");
            assert_eq!(j.family(), Some(f));
            assert_eq!(j.security, b'0');
            assert_eq!(j.stated_checksum, Some(checksum(&z)));
            let text = String::from_utf8(out).unwrap();
            assert!(text.contains(&format!("C{:04X}*", checksum(&z))), "{f:?}");
            assert_eq!(text.contains("G0*"), f != Family::F1502BE, "{f:?}");
            assert!(text.starts_with('\x02') && text.ends_with("\x030000\r\n"));
            // every field (from its identifier to the '*') is shorter than 200 bytes (§3.2)
            assert!(text.split('*').all(|fld| fld.trim_start().len() < 200), "{f:?}");
        }
    }

    #[test]
    fn blank_load_image() {
        let f = Family::F1504AS;
        let z = fuses_from_image(f, &DeviceImage::blank(f));
        assert!(z[..34186].iter().all(|&c| c == b'1'));
        assert!(z[34186..].iter().all(|&c| c == b'0'));
        assert_eq!(checksum(&z), 0xA052);
    }
}
