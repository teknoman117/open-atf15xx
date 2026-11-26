//! JEDEC reader (§3).

use crate::isp::device::Family;
use crate::isp::diag::Error;

pub const J1: &str = "JEDEC file is not valid for this device.\nPlease select a valid JEDEC file!";

#[derive(Debug, Clone)]
pub struct Jedec {
    pub qf: Option<usize>,
    /// Fuse array after finalisation (§3.4); empty when there was no `QF` field.
    pub fuses: Vec<u8>,
    pub default: u8,
    pub security: u8,
    /// Checksum stated in the `C` field, if any.
    pub stated_checksum: Option<u32>,
}

impl Jedec {
    pub fn family(&self) -> Option<Family> {
        self.qf.and_then(Family::from_qf)
    }
    pub fn checksum(&self) -> u32 {
        checksum(&self.fuses)
    }
}

/// §3.6
pub fn checksum(f: &[u8]) -> u32 {
    let mut s: u32 = 0;
    for chunk in f.chunks(8) {
        let mut b = 0u32;
        for (k, &ch) in chunk.iter().enumerate() {
            if ch == b'1' {
                b |= 1 << k;
            }
        }
        s = s.wrapping_add(b);
    }
    s & 0xFFFF
}

fn numeric(field: &[u8]) -> u64 {
    let mut v: u64 = 0;
    for &c in field {
        if c.is_ascii_digit() {
            v = v.wrapping_mul(10).wrapping_add((c - b'0') as u64);
        }
    }
    v
}

fn contains(h: &[u8], n: &[u8]) -> bool {
    h.windows(n.len()).any(|w| w == n)
}

/// Parse a JEDEC file per §3.2–§3.4. Errors are J2-class conditions.
pub fn parse(data: &[u8]) -> Result<Jedec, Error> {
    let mut j = Jedec {
        qf: None,
        fuses: Vec::new(),
        default: b'0',
        security: b'0',
        stated_checksum: None,
    };
    let mut fz: Option<Vec<u8>> = None;
    let mut fields: Vec<&[u8]> = data.split(|&b| b == b'*').collect();
    // the remainder after the last '*' is also a field (it is read by getline at EOF)
    if fields.len() > 1 && fields.last().is_some_and(|f| f.is_empty()) {
        fields.pop();
    }
    for raw in fields {
        process_field(raw, &mut j, &mut fz)?;
    }
    if let Some(mut arr) = fz {
        for c in arr.iter_mut() {
            if *c == b'X' {
                *c = j.default;
            }
        }
        j.fuses = arr;
    }
    Ok(j)
}

fn process_field(raw: &[u8], j: &mut Jedec, fz: &mut Option<Vec<u8>>) -> Result<(), Error> {
    let known = |q: usize| Family::from_qf(q).is_some();
    {
        if contains(raw, b"NOTE") || contains(raw, b"Note") {
            return Ok(());
        }
        let start = raw
            .iter()
            .position(|&c| c.is_ascii_alphanumeric() || c == 2 || c == 3);
        let Some(start) = start else { return Ok(()) };
        // §3.2 step 3: ATMISP copies 200 bytes into an unterminated buffer; fields of 200 bytes or
        // more (from the identifier) are undefined behaviour and are rejected (J2).
        if raw.len() - start >= 200 {
            return Err(Error::Input(J1.into()));
        }
        let f = &raw[start..];
        let c0 = f[0].to_ascii_uppercase();
        let c1 = f.get(1).map(|c| c.to_ascii_uppercase());
        match (c0, c1) {
            (b'Q', Some(b'F')) => {
                let q = numeric(&f[2..]) as usize;
                j.qf = Some(q);
                if q > 10_000_000 {
                    return Err(Error::Input(J1.into()));
                }
                *fz = Some(vec![b'X'; q]);
            }
            (b'Q', Some(b'P')) | (b'Q', Some(b'V')) => {}
            (b'F', _) => {
                let v = numeric(&f[1..]);
                if v > 1 {
                    return Err(Error::Input(J1.into()));
                }
                j.default = b'0' + v as u8;
            }
            (b'G', _) => j.security = (numeric(&f[1..]) as u8).wrapping_add(b'0'),
            (b'C', _) => j.stated_checksum = Some(parse_hex(&f[1..])),
            (b'L', _) => {
                // §3.3 L field: address digits before the first SP/TAB/LF, then every digit.
                let mut addr: usize = 0;
                let mut in_addr = true;
                let mut data_digits: Vec<u8> = Vec::new();
                for &ch in f {
                    if ch == b' ' || ch == b'\t' || ch == b'\n' {
                        in_addr = false;
                    }
                    if ch.is_ascii_digit() {
                        if in_addr {
                            addr = addr.wrapping_mul(10).wrapping_add((ch - b'0') as usize);
                        } else if ch == b'0' || ch == b'1' {
                            data_digits.push(ch);
                        } else {
                            // ATMISP stores digits 2..9 verbatim; they make its hex output
                            // undefined (§3.3), so they are rejected (J2).
                            return Err(Error::Input(J1.into()));
                        }
                    }
                }
                if let (Some(q), Some(arr)) = (j.qf, fz.as_mut()) {
                    if known(q) {
                        if addr + data_digits.len() > q {
                            return Err(Error::Input(J1.into()));
                        }
                        arr[addr..addr + data_digits.len()].copy_from_slice(&data_digits);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_hex(f: &[u8]) -> u32 {
    let mut v: u32 = 0;
    for &c in f {
        if let Some(d) = (c as char).to_digit(16) {
            v = v.wrapping_mul(16).wrapping_add(d);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_fields() {
        let t = b"\x02design*\r\nQF16808* QP44* F1* G1*\r\nL0 0011*\r\nL10\r\n0 1 0*\r\nC1234*\r\n\x030000";
        let j = parse(t).unwrap();
        assert_eq!(j.qf, Some(16808));
        assert_eq!(&j.fuses[0..6], b"001111");
        assert_eq!(&j.fuses[10..13], b"010");
        assert_eq!(j.security, b'1');
        assert_eq!(j.stated_checksum, Some(0x1234));
    }

    #[test]
    fn note_fields_skipped_and_case_insensitive() {
        let t = b"qf16808*f1*L5 00 NOTE x*l7 0*";
        let j = parse(t).unwrap();
        assert_eq!(j.fuses[5], b'1');
        assert_eq!(j.fuses[7], b'0');
    }

    #[test]
    fn long_fields_rejected() {
        let mut t = b"QF16808*F1*L0 ".to_vec();
        t.extend(std::iter::repeat_n(b'0', 196));
        assert!(parse(&[t.as_slice(), b"*"].concat()).is_ok()); // 199 bytes
        t.push(b'0');
        assert!(parse(&[t.as_slice(), b"*"].concat()).is_err()); // 200 bytes
    }
}
