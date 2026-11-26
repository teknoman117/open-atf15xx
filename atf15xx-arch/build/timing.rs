//! The speed-grade tables, from our own `data/timing.json` (datasheets + the vendor fitters' SDF
//! output, tools/fuzzers/fitter-timing).

use serde_json::Value;

pub fn emit(text: &str) -> String {
    let v: Value = serde_json::from_str(text).expect("timing.json");
    let grades: Vec<String> = v["grades"].as_array().unwrap().iter().map(|g| format!("{:?}", g.as_str().unwrap())).collect();
    assert_eq!(grades.len(), 5, "timing.json: five grades expected");
    let mut o = format!("/// Speed grades, in the order of every value array.\npub const GRADES: [&str; 5] = [{}];\n", grades.join(", "));
    o += "pub(crate) static PARAMS: [&[(&str, [f64; 5])]; 3] = [\n";
    for fam in ["1502", "1504", "1508"] {
        o += &format!("    // ATF{fam}AS\n    &[\n");
        for (name, p) in v["families"][fam]["params"].as_object().unwrap() {
            let vals: Vec<String> = p["max"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| format!("{:?}", x.as_f64().unwrap_or_else(|| panic!("timing.json: {fam} {name} has a missing value"))))
                .collect();
            o += &format!("        ({name:?}, [{}]),\n", vals.join(", "));
        }
        o += "    ],\n";
    }
    o + "];\n"
}
