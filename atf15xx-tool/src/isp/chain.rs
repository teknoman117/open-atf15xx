//! Chain file reader and writer (§2).

use crate::isp::device::{Instr, NAME_LINES, SUPPORTED};
use crate::isp::diag::{Diagnostics, Error};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub ir_width: u32,
    pub idcode: bool,
    pub instr: Instr,
    pub jed_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub calibration: u32,
    pub devices: Vec<Device>,
}

pub const E1: &str = "This is not a valid Chain File.";
pub const E2: &str =
    "Invalid Chain file.\nThe number of devices in Chain file must be between 1 and 50.";
pub fn e3(d: usize) -> String {
    format!(
        "Invalid Chain file.\nJEDEC file specified in Chain file for device # {d} does not exist."
    )
}
pub const W1: &str =
    "Port type specified in Chain file is not supported.\nSetting port type to USB.";
pub const W2: &str =
    "Calibration constant in Chain file is invalid.\nSetting Calibration constant to 1.";
pub const W3: &str = "Cable type not specified in Chain file.\nSetting Cable type to ATDH1150USB.";
pub const W4: &str =
    "Cable type specified in Chain file is not supported.\nSetting Cable type to ATDH1150USB.";
pub fn w5(d: usize) -> String {
    format!("Invalid JTAG instruction width in Chain file.\nSetting JTAG instruction width to 10 for device # {d}")
}

/// C `atoi`: optional leading whitespace and sign, then digits.
pub fn c_atoi(s: &str) -> i64 {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (neg, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut v: i64 = 0;
    for c in rest.bytes() {
        if !c.is_ascii_digit() {
            break;
        }
        v = v.saturating_mul(10).saturating_add((c - b'0') as i64);
    }
    if neg {
        -v
    } else {
        v
    }
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn getc(&mut self) -> Option<u8> {
        let c = self.d.get(self.p).copied();
        if c.is_some() {
            self.p += 1;
        }
        c
    }
    fn readline(&mut self) -> String {
        let start = self.p;
        while self.p < self.d.len() && self.d[self.p] != b'\n' {
            self.p += 1;
        }
        let mut line = &self.d[start..self.p];
        if self.p < self.d.len() {
            self.p += 1;
        }
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        String::from_utf8_lossy(line).into_owned()
    }
}

/// Parse a chain file (§2.2). `jed_exists(d, path)` decides E3 for instructions 1/4/7.
pub fn parse(
    data: &[u8],
    diag: &mut Diagnostics,
    mut jed_exists: impl FnMut(usize, &str) -> bool,
) -> Result<Chain, Error> {
    // CR LF → LF (text mode)
    let mut norm = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\r' && data.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        norm.push(data[i]);
        i += 1;
    }
    let mut r = Reader { d: &norm, p: 0 };
    let e1 = || Error::Input(E1.into());
    let bad = |c: Option<u8>| match c {
        None => true,
        Some(c) => c == b' ' || c == b'\n' || !c.is_ascii_digit(),
    };

    let mut c = r.getc();
    if bad(c) {
        return Err(e1());
    }
    let mut buf = String::new();
    while c != Some(b' ') {
        if buf.len() > 1 {
            return Err(Error::Input(E2.into()));
        }
        match c {
            Some(ch) if ch.is_ascii_digit() => buf.push(ch as char),
            _ => return Err(e1()),
        }
        c = r.getc();
    }
    let n = c_atoi(&buf);
    if n <= 0 || n >= 51 {
        return Err(Error::Input(E2.into()));
    }
    c = r.getc();
    if bad(c) {
        return Err(e1());
    }
    match c {
        Some(b'4') => {}
        Some(b'1') | Some(b'2') | Some(b'3') => diag.warn(W1),
        _ => return Err(e1()),
    }
    if r.getc() != Some(b' ') {
        return Err(e1());
    }
    c = r.getc();
    if bad(c) {
        return Err(e1());
    }
    let (mut kbuf, mut count, mut k) = (String::new(), 0, 0i64);
    while let Some(ch) = c {
        if ch == b' ' || ch == b'\n' {
            break;
        }
        if count < 4 {
            kbuf.push(ch as char);
            k = c_atoi(&kbuf);
        } else {
            k = 1001;
        }
        count += 1;
        c = r.getc();
    }
    if k <= 0 || k >= 1001 {
        diag.warn(W2);
        k = 1;
    }
    c = r.getc();
    if c == Some(b'\n') {
        diag.warn(W3);
    } else {
        if c != Some(b'0') {
            diag.warn(W4);
        }
        while let Some(ch) = c {
            if ch == b'\n' {
                break;
            }
            c = r.getc();
        }
        let _ = r.readline(); // line 2: read and discarded (defect D9)
    }

    let is_name = |s: &str| NAME_LINES.contains(&s);
    let mut devices = Vec::new();
    let mut look = String::new();
    for d in 1..=n as usize {
        let name = if is_name(&look) {
            look.clone()
        } else {
            r.readline()
        };
        let wl = r.readline();
        let il = r.readline();
        look = r.readline();
        let jed_path = if is_name(&look) {
            String::new()
        } else {
            look.clone()
        };
        let idcode = !wl.contains('*');
        let mut w = c_atoi(&wl.replace('*', ""));
        if SUPPORTED.contains(&name.as_str()) && w != 10 {
            diag.warn(w5(d));
            w = 10;
        }
        if w <= 0 || w >= 100 {
            diag.warn(w5(d));
            w = 10;
        }
        let instr = Instr::from_code(&il);
        if (il == "1" || il == "4" || il == "7") && !jed_exists(d, &jed_path) {
            return Err(Error::Input(e3(d)));
        }
        devices.push(Device {
            name,
            ir_width: w as u32,
            idcode,
            instr,
            jed_path,
        });
    }
    Ok(Chain {
        calibration: k as u32,
        devices,
    })
}

/// Chain writer (§2.3): CR LF line endings.
pub fn write(chain: &Chain) -> Vec<u8> {
    let mut s = String::new();
    s.push_str(&format!(
        "{} 4 {} 0 \r\n\r\n",
        chain.devices.len(),
        chain.calibration
    ));
    for d in &chain.devices {
        s.push_str(&d.name);
        s.push_str("\r\n");
        s.push_str(&d.ir_width.to_string());
        s.push_str(if d.idcode { "\r\n" } else { " *\r\n" });
        if let Some(code) = d.instr.code() {
            s.push_str(&code.to_string());
        }
        s.push_str("\r\n");
        s.push_str(&d.jed_path);
        s.push_str("\r\n");
    }
    s.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let t =
            b"2 4 1 0 \r\n\r\nUNKNOWN\r\n8 *\r\n0\r\n\r\nATF1504AS\r\n10\r\n1\r\nC:\\x\\a.jed\r\n";
        let mut d = Diagnostics::default();
        let c = parse(t, &mut d, |_, _| true).unwrap();
        assert!(d.warnings.is_empty());
        assert_eq!(c.devices[0].ir_width, 8);
        assert!(!c.devices[0].idcode);
        assert_eq!(c.devices[1].instr, Instr::ProgramVerify);
        assert_eq!(c.devices[1].jed_path, "C:\\x\\a.jed");
        assert_eq!(write(&c), t.to_vec());
    }

    #[test]
    fn line_two_is_discarded() {
        let t = b"1 4 1 0 \nATF1504AS\n10\n2\n";
        let mut d = Diagnostics::default();
        let c = parse(t, &mut d, |_, _| true).unwrap();
        assert_eq!(c.devices[0].name, "10");
        assert_eq!(c.devices[0].ir_width, 2);
        assert_eq!(c.devices[0].instr, Instr::NoneOrInvalid);
    }

    #[test]
    fn header_errors_and_warnings() {
        let mut d = Diagnostics::default();
        assert!(parse(b"0 4 1 0 \n\n", &mut d, |_, _| true).is_err());
        let c = parse(b"1 1 5000\n\nATF1502AS\n8\n2\n\n", &mut d, |_, _| true).unwrap();
        assert_eq!(c.calibration, 1);
        assert_eq!(c.devices[0].ir_width, 10);
        assert_eq!(
            d.warnings,
            vec![W1.to_string(), W2.to_string(), W3.to_string(), w5(1)]
        );
    }
}
