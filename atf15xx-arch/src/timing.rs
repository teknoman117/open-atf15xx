//! Speed-grade timing parameters (ns), per family and grade: the datasheet values truncated as
//! the fitters do, replaced by the fitters' own where their SDF output shows them (spec F8.3;
//! generated at build time from `data/timing.json`, tools/fuzzers/fitter-timing).

use crate::Family;

include!(concat!(env!("OUT_DIR"), "/timing.rs"));

/// Index of a speed grade (`-10` or `10`) in [`GRADES`].
pub fn grade_index(grade: &str) -> Option<usize> {
    let want = format!("-{}", grade.trim_start_matches('-'));
    GRADES.iter().position(|g| *g == want)
}

/// Every parameter of a family: `(name, values by grade)`.
pub fn params(family: Family) -> &'static [(&'static str, [f64; 5])] {
    PARAMS[family as usize]
}

/// One parameter at one grade.
pub fn value(family: Family, name: &str, grade: usize) -> Option<f64> {
    params(family)
        .iter()
        .find(|p| p.0 == name)
        .map(|p| p.1[grade])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups() {
        assert_eq!(grade_index("10"), Some(1));
        assert_eq!(grade_index("-25"), Some(4));
        assert_eq!(value(Family::F1508, "tLAD", 0), Some(3.0));
        assert!(params(Family::F1502).len() > 40);
    }
}
