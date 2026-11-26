//! The devices (package pin maps, JTAG / power-down / dedicated pins), from our own
//! `data/devices.json` (black-box fitter experiments, tools/fuzzers/fitter-pins).

use serde_json::Value;

pub struct Dev {
    pub name: String,
    pub family: String,
    pub package: String,
    pub tt2_device: String,
    /// (pin, macrocell), by pin
    pub pins: Vec<(u16, u16)>,
    pub input_only: Vec<(u16, u16)>,
    pub jtag: [u16; 4],
    pub pd1: u16,
    pub pd2: u16,
    pub gclk1: u16,
    pub oe1: u16,
    pub gclr: u16,
    pub gclk2_oe2: u16,
    pub gclk3: u16,
}

impl Dev {
    pub fn pin_mc(&self, pin: u16) -> Option<u16> {
        self.pins.iter().find(|p| p.0 == pin).map(|p| p.1)
    }
    pub fn mc_pin(&self, mc: u16) -> Option<u16> {
        self.pins.iter().find(|p| p.1 == mc).map(|p| p.0)
    }
    pub fn input_only_mc(&self, pin: u16) -> Option<u16> {
        self.input_only.iter().find(|p| p.0 == pin).map(|p| p.1)
    }
    pub fn macrocells(&self) -> u16 {
        match self.family.as_str() {
            "1502" => 32,
            "1504" => 64,
            _ => 128,
        }
    }
}

fn pinmap(v: &Value) -> Vec<(u16, u16)> {
    let mut out: Vec<(u16, u16)> = v.as_object().unwrap().iter().map(|(p, m)| (p.parse().unwrap(), m.as_u64().unwrap() as u16)).collect();
    out.sort();
    out
}

pub fn load(text: &str) -> Vec<Dev> {
    let v: Value = serde_json::from_str(text).expect("devices.json");
    let n = |d: &Value, k: &str| d[k].as_u64().unwrap_or_else(|| panic!("devices.json: {k}")) as u16;
    v.as_object()
        .unwrap()
        .iter()
        .map(|(name, d)| Dev {
            name: name.clone(),
            family: name[1..5].to_string(),
            package: d["package"].as_str().unwrap().into(),
            tt2_device: d["tt2_device"].as_str().unwrap().into(),
            pins: pinmap(&d["pins"]),
            input_only: pinmap(&d["input_only"]),
            jtag: [n(&d["jtag"], "tdi"), n(&d["jtag"], "tms"), n(&d["jtag"], "tck"), n(&d["jtag"], "tdo")],
            pd1: n(d, "pd1"),
            pd2: n(d, "pd2"),
            gclk1: n(d, "gclk1"),
            oe1: n(d, "oe1"),
            gclr: n(d, "gclr"),
            gclk2_oe2: n(d, "gclk2_oe2"),
            gclk3: n(d, "gclk3"),
        })
        .collect()
}

pub fn emit(devs: &[Dev]) -> String {
    let pairs = |v: &[(u16, u16)]| v.iter().map(|(a, b)| format!("({a}, {b})")).collect::<Vec<_>>().join(", ");
    let mut o = format!("pub(crate) static DEVICES: [Device; {}] = [\n", devs.len());
    for d in devs {
        o += &format!(
            "    Device {{ name: {:?}, family: Family::F{}, package: {:?}, tt2_device: {:?}, pins: &[{}], input_only: &[{}], \
             jtag: Jtag {{ tdi: {}, tms: {}, tck: {}, tdo: {} }}, pd1: {}, pd2: {}, gclk1: {}, oe1: {}, gclr: {}, gclk2_oe2: {}, gclk3: {} }},\n",
            d.name, d.family, d.package, d.tt2_device, pairs(&d.pins), pairs(&d.input_only),
            d.jtag[0], d.jtag[1], d.jtag[2], d.jtag[3], d.pd1, d.pd2, d.gclk1, d.oe1, d.gclr, d.gclk2_oe2, d.gclk3
        );
    }
    o + "];\n"
}
