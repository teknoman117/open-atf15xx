//! Fuse-array → programming-row maps (§5).

use crate::isp::device::Family;

/// AS-type block: rows r0..=r1, cols c0..=c1, index = base + rs·(r−r0) + cs·(c−c0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsBlock {
    pub r0: usize,
    pub r1: usize,
    pub c0: usize,
    pub c1: usize,
    pub base: i64,
    pub rs: i64,
    pub cs: i64,
}

const fn asb(r0: usize, r1: usize, c0: usize, c1: usize, base: i64, rs: i64, cs: i64) -> AsBlock {
    AsBlock {
        r0,
        r1,
        c0,
        c1,
        base,
        rs,
        cs,
    }
}

/// §5.2.1–§5.2.3 (the "15439 + 80·r − c" forms are written with r0/c0 = 0).
pub const AS_1502: &[AsBlock] = &[
    asb(0, 11, 0, 79, 15439, 80, -1),
    asb(12, 107, 0, 79, 7584, 1, -96),
    asb(128, 223, 0, 79, 15264, 1, -96),
    asb(224, 228, 0, 79, 16715, 1, -5),
    asb(224, 228, 80, 85, 16745, 1, -5),
];
pub const AS_1504: &[AsBlock] = &[
    asb(0, 11, 6, 165, 30879, 160, -1),
    asb(12, 107, 6, 165, 15264, 1, -96),
    asb(128, 223, 6, 165, 30624, 1, -96),
    asb(224, 232, 0, 165, 34125, 1, -9),
];
pub const AS_1508: &[AsBlock] = &[
    asb(0, 11, 6, 325, 61759, 320, -1),
    asb(12, 107, 6, 325, 30624, 1, -96),
    asb(128, 223, 6, 325, 61344, 1, -96),
    asb(224, 250, 0, 325, 74055, 1, -27),
];

/// BE-type segment (§5.3). `g == 0` means G = ∞. `explicit` overrides the formula.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeSeg {
    pub r0: usize,
    pub r1: usize,
    pub base: i64,
    pub g: usize,
    pub rstride: i64,
    pub t: [i64; 8],
    pub explicit: Option<[i64; 8]>,
}

const LIN: [i64; 8] = [7, 6, 5, 4, 3, 2, 1, 0];
const G96: [i64; 8] = [672, 576, 480, 384, 288, 192, 96, 0];
const fn seg(r0: usize, r1: usize, base: i64, g: usize, rstride: i64, t: [i64; 8]) -> BeSeg {
    BeSeg {
        r0,
        r1,
        base,
        g,
        rstride,
        t,
        explicit: None,
    }
}
const fn cells(r: usize, c: [i64; 8]) -> BeSeg {
    BeSeg {
        r0: r,
        r1: r,
        base: 0,
        g: 0,
        rstride: 0,
        t: [0; 8],
        explicit: Some(c),
    }
}

pub const BE_1502: &[BeSeg] = &[
    seg(0, 53, 15360, 0, 8, LIN),
    seg(54, 107, 15840, 0, 8, LIN),
    seg(108, 1067, 0, 10, 768, G96),
    seg(1068, 1117, 16320, 10, 40, [35, 30, 25, 20, 15, 10, 5, 0]),
    seg(1118, 2077, 7680, 10, 768, G96),
    cells(
        2078,
        [16745, 16740, 16735, 16730, 16725, 16720, 16720, 16724],
    ),
    seg(2079, 2082, 16721, 0, 1, [25, 20, 15, 10, 5, 0, 0, 0]),
    seg(2083, 2089, 16750, 0, 8, LIN),
];
pub const BE_1504: &[BeSeg] = &[
    seg(0, 53, 30720, 0, 8, LIN),
    seg(54, 107, 31200, 0, 8, LIN),
    seg(108, 161, 31680, 0, 8, LIN),
    seg(162, 215, 32160, 0, 8, LIN),
    seg(216, 1175, 0, 10, 768, G96),
    seg(1176, 1265, 32640, 10, 72, [63, 54, 45, 36, 27, 18, 9, 0]),
    seg(1266, 2225, 15360, 10, 768, G96),
    seg(2226, 3185, 7680, 10, 768, G96),
    seg(3186, 3275, 33360, 10, 72, [63, 54, 45, 36, 27, 18, 9, 0]),
    seg(3276, 4235, 23040, 10, 768, G96),
    cells(
        4236,
        [34125, 34116, 34107, 34098, 34089, 34080, 34080, 34088],
    ),
    seg(4237, 4244, 34081, 0, 1, [45, 36, 27, 18, 9, 0, 0, 0]),
    seg(4245, 4251, 34134, 0, 8, LIN),
];
const T216A: [i64; 8] = [189, 2268, 2349, 108, 81, 2160, 2241, 0];
const T216B: [i64; 8] = [2268, 135, 108, 2295, 2160, 27, 0, 2187];
pub const BE_1508: &[BeSeg] = &[
    seg(0, 53, 61440, 0, 8, LIN),
    seg(54, 107, 61920, 0, 8, LIN),
    seg(108, 161, 62400, 0, 8, LIN),
    seg(162, 215, 62880, 0, 8, LIN),
    seg(216, 269, 63360, 0, 8, LIN),
    seg(270, 323, 63840, 0, 8, LIN),
    seg(324, 377, 64320, 0, 8, LIN),
    seg(378, 431, 64800, 0, 8, LIN),
    seg(432, 1391, 7680, 10, 768, G96),
    seg(1392, 1661, 65280, 10, 216, T216A),
    seg(1662, 2621, 0, 10, 768, G96),
    seg(2622, 3581, 30720, 10, 768, G96),
    seg(3582, 3851, 65307, 10, 216, T216B),
    seg(3852, 4811, 38400, 10, 768, G96),
    seg(4812, 5771, 15360, 10, 768, G96),
    seg(5772, 6041, 69600, 10, 216, T216A),
    seg(6042, 7001, 23040, 10, 768, G96),
    seg(7002, 7961, 53760, 10, 768, G96),
    seg(7962, 8231, 69627, 10, 216, T216B),
    seg(8232, 9191, 46080, 10, 768, G96),
    cells(
        9192,
        [74055, 74028, 74001, 73974, 73947, 73920, 73920, 73946],
    ),
    seg(9193, 9218, 73921, 0, 1, [135, 108, 81, 54, 27, 0, 0, 0]),
    seg(9219, 9225, 74082, 0, 8, LIN),
];

/// The fully expanded map of a family: `rows[r][c]` = fuse index or −1 (constant `'1'`).
#[derive(Debug, Clone)]
pub struct FuseMap {
    pub family: Family,
    pub rows: Vec<Vec<i64>>,
    /// AS-type registers (§5.2.4); empty for BE-type.
    pub special: Vec<i64>,
    pub ues: Vec<i64>,
    pub config: Vec<i64>,
}

impl FuseMap {
    pub fn new(family: Family) -> FuseMap {
        let (nrows, rowlen) = family.geometry();
        let mut rows = vec![vec![-1i64; rowlen]; nrows];
        let (mut special, mut ues, mut config) = (vec![], vec![], vec![]);
        match family {
            Family::F1502AS | Family::F1504AS | Family::F1508AS => {
                let tab = match family {
                    Family::F1502AS => AS_1502,
                    Family::F1504AS => AS_1504,
                    _ => AS_1508,
                };
                for b in tab {
                    for (r, row) in rows.iter_mut().enumerate().take(b.r1 + 1).skip(b.r0) {
                        for (c, cell) in row.iter_mut().enumerate().take(b.c1 + 1).skip(b.c0) {
                            *cell = b.base + b.rs * (r - b.r0) as i64 + b.cs * (c - b.c0) as i64;
                        }
                    }
                }
                let (k, s, u, _, _) = family.layout();
                special = (0..4).map(|i| (s + 3 - i) as i64).collect();
                ues = (0..16).map(|i| (u + 15 - i) as i64).collect();
                config = (0..32).map(|i| (k + 31 - i) as i64).collect();
            }
            _ => {
                let tab = match family {
                    Family::F1502BE => BE_1502,
                    Family::F1504BE => BE_1504,
                    _ => BE_1508,
                };
                for s in tab {
                    for (r, row) in rows.iter_mut().enumerate().take(s.r1 + 1).skip(s.r0) {
                        if let Some(c) = s.explicit {
                            row.copy_from_slice(&c);
                            continue;
                        }
                        let i = r - s.r0;
                        let (g, k) = if s.g == 0 { (0, i) } else { (i / s.g, i % s.g) };
                        for (c, cell) in row.iter_mut().enumerate() {
                            *cell = s.base + g as i64 + s.rstride * k as i64 + s.t[c];
                        }
                    }
                }
            }
        }
        FuseMap {
            family,
            rows,
            special,
            ues,
            config,
        }
    }
}

/// Programming data of one device (§5): rows and AS registers as character strings.
#[derive(Debug, Clone)]
pub struct DeviceImage {
    pub rows: Vec<Vec<u8>>,
    pub special: Vec<u8>,
    pub ues: Vec<u8>,
    pub config: Vec<u8>,
}

impl DeviceImage {
    /// Default (erased) image: every cell `'1'` (§3.5). Geometry from the device family.
    pub fn blank(family: Family) -> DeviceImage {
        let (nrows, rowlen) = family.geometry();
        let regs = family.is_as();
        DeviceImage {
            rows: vec![vec![b'1'; rowlen]; nrows],
            special: if regs { vec![b'1'; 4] } else { vec![] },
            ues: if regs { vec![b'1'; 16] } else { vec![] },
            config: if regs { vec![b'1'; 32] } else { vec![] },
        }
    }

    /// Build the image from a fuse array (§5.1).
    pub fn from_fuses(map: &FuseMap, fuses: &[u8]) -> DeviceImage {
        let pick = |i: i64| if i < 0 { b'1' } else { fuses[i as usize] };
        DeviceImage {
            rows: map
                .rows
                .iter()
                .map(|row| row.iter().map(|&i| pick(i)).collect())
                .collect(),
            special: map.special.iter().map(|&i| pick(i)).collect(),
            ues: map.ues.iter().map(|&i| pick(i)).collect(),
            config: map.config.iter().map(|&i| pick(i)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn as_maps_are_bijective_onto_non_reserved_fuses() {
        for fam in [Family::F1502AS, Family::F1504AS, Family::F1508AS] {
            let m = FuseMap::new(fam);
            let mut seen = HashSet::new();
            for row in &m.rows {
                for &i in row {
                    if i >= 0 {
                        assert!(seen.insert(i), "{fam:?} dup {i}");
                    }
                }
            }
            for &i in m.special.iter().chain(&m.ues).chain(&m.config) {
                assert!(seen.insert(i));
            }
            let (_, _, _, reserved, _) = fam.layout();
            assert_eq!(seen.len(), reserved, "{fam:?}");
        }
    }

    /// The constant tables must equal the normative tables of `docs/architecture/05-fuse-maps.md`.
    #[test]
    fn tables_match_specification() {
        let doc = include_str!("../../../docs/architecture/05-fuse-maps.md");
        let mut sections = doc.split("\n### ").skip(1);
        let expect_as = [
            ("1502AS", AS_1502),
            ("1504AS", AS_1504),
            ("1508AS", AS_1508),
        ];
        let expect_be = [
            ("1502BE", BE_1502),
            ("1504BE", BE_1504),
            ("1508BE", BE_1508),
        ];
        let mut found = 0;
        for sec in sections.by_ref() {
            let title = sec.lines().next().unwrap_or("");
            let lines: Vec<&str> = sec
                .lines()
                .filter(|l| {
                    l.starts_with("| ") && !l.starts_with("| Rows") && !l.starts_with("|---")
                })
                .collect();
            if let Some((_, tab)) = expect_as
                .iter()
                .find(|(n, _)| title.contains(&format!(" {n} ")))
            {
                assert_eq!(lines.len(), tab.len(), "{title}");
                for (l, b) in lines.iter().zip(tab.iter()) {
                    let cells: Vec<&str> = l.trim_matches('|').split('|').map(str::trim).collect();
                    assert_eq!(cells[0], format!("{}–{}", b.r0, b.r1));
                    assert_eq!(cells[1], format!("{}–{}", b.c0, b.c1));
                    // evaluate the documented expression at the block corners
                    for (r, c) in [(b.r0, b.c0), (b.r1, b.c1), (b.r0, b.c1), (b.r1, b.c0)] {
                        let want = b.base + b.rs * (r - b.r0) as i64 + b.cs * (c - b.c0) as i64;
                        assert_eq!(
                            eval_expr(cells[2].trim_matches('`'), r as i64, c as i64),
                            want,
                            "{l}"
                        );
                    }
                }
                found += 1;
            } else if let Some((_, tab)) = expect_be
                .iter()
                .find(|(n, _)| title.contains(&format!(" {n} ")))
            {
                assert_eq!(lines.len(), tab.len(), "{title}");
                for (l, s) in lines.iter().zip(tab.iter()) {
                    let cells: Vec<&str> = l.trim_matches('|').split('|').map(str::trim).collect();
                    if let Some(ex) = s.explicit {
                        assert_eq!(cells[0], s.r0.to_string());
                        let want = ex
                            .iter()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(" ");
                        assert!(cells[4].contains(&want), "{l}");
                    } else {
                        assert_eq!(cells[0], format!("{}–{}", s.r0, s.r1));
                        assert_eq!(cells[1], s.base.to_string());
                        assert_eq!(
                            cells[2],
                            if s.g == 0 {
                                "∞".to_string()
                            } else {
                                s.g.to_string()
                            }
                        );
                        assert_eq!(cells[3], s.rstride.to_string());
                        let t =
                            s.t.iter()
                                .map(|v| v.to_string())
                                .collect::<Vec<_>>()
                                .join(" ");
                        assert_eq!(cells[4].trim_matches('`'), t);
                    }
                }
                found += 1;
            }
        }
        assert_eq!(found, 6);
    }

    /// Tiny evaluator for the documented index expressions, e.g. `7584 + (r−12) − 96·c`.
    fn eval_expr(e: &str, r: i64, c: i64) -> i64 {
        let e = e.replace('−', "-").replace('·', "*").replace(' ', "");
        let mut total = 0i64;
        let mut sign = 1i64;
        let mut term = String::new();
        let flush = |t: &str, sign: i64, total: &mut i64| {
            if t.is_empty() {
                return;
            }
            let mut v = 1i64;
            for f in t.split('*') {
                let f = f.trim_matches(|ch| ch == '(' || ch == ')');
                v *= if let Some((a, b)) = f.split_once('-') {
                    let a = if a == "r" { r } else { c };
                    a - b.parse::<i64>().unwrap()
                } else if f == "r" {
                    r
                } else if f == "c" {
                    c
                } else {
                    f.parse::<i64>().unwrap()
                };
            }
            *total += sign * v;
        };
        let mut depth = 0;
        for ch in e.chars() {
            match ch {
                '(' => {
                    depth += 1;
                    term.push(ch)
                }
                ')' => {
                    depth -= 1;
                    term.push(ch)
                }
                '+' | '-' if depth == 0 => {
                    flush(&term, sign, &mut total);
                    term.clear();
                    sign = if ch == '+' { 1 } else { -1 };
                }
                _ => term.push(ch),
            }
        }
        flush(&term, sign, &mut total);
        total
    }
}
