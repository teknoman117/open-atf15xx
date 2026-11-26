//! LSB-first bit strings (README *Notation*) and the hex rendering of §6.2.
//!
//! A bit string is a byte vector of ASCII characters, normally `b'0'`/`b'1'`. Index 0 is the
//! least-significant bit. Other characters are allowed because ATMISP copies JEDEC fuse
//! characters verbatim (§3.4); the special value [`UNDEF`] models an uninitialised byte (§9.4 D3).

/// Marker for an uninitialised byte leaked into a scan (defect D3).
pub const UNDEF: u8 = 0xFF;

pub type Bits = Vec<u8>;

/// `bits(v, n)`: the n-character LSB-first string of `v`.
pub fn bits(v: u64, n: usize) -> Bits {
    (0..n).map(|i| if (v >> i) & 1 == 1 { b'1' } else { b'0' }).collect()
}

/// 16-bit value written MSB first (§6.7, `BEADDR`).
pub fn msb16(v: u32) -> Bits {
    (0..16).map(|i| if (v >> (15 - i)) & 1 == 1 { b'1' } else { b'0' }).collect()
}

pub fn ones(n: usize) -> Bits {
    vec![b'1'; n]
}

pub fn zeros(n: usize) -> Bits {
    vec![b'0'; n]
}

/// §6.2: groups of 4 from index 0, value = Σ (c−48)·2^k, digits reversed.
/// A group whose value is outside 0..=15 (or that contains [`UNDEF`]) renders as a backtick.
pub fn hex(b: &[u8]) -> String {
    const DIG: &[u8; 16] = b"0123456789abcdef";
    let mut out: Vec<u8> = Vec::with_capacity(b.len() / 4 + 1);
    for chunk in b.chunks(4) {
        let mut v: i64 = 0;
        let mut undef = false;
        for (k, &ch) in chunk.iter().enumerate() {
            if ch == UNDEF {
                undef = true;
            }
            v += (ch as i64 - 48) << k;
        }
        if undef || !(0..=15).contains(&v) {
            out.push(b'`');
        } else {
            out.push(DIG[v as usize]);
        }
    }
    out.reverse();
    String::from_utf8(out).expect("ascii")
}

/// `a ++ b` helper: returns `front` followed by `back`.
pub fn prepend(front: &[u8], back: &[u8]) -> Bits {
    let mut v = Vec::with_capacity(front.len() + back.len());
    v.extend_from_slice(front);
    v.extend_from_slice(back);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcode_encoding() {
        assert_eq!(bits(0x290, 10), b"0000100101".to_vec());
        assert_eq!(hex(&bits(0x2A1, 10)), "2a1");
        assert_eq!(hex(&bits(0x0C, 11)), "00c");
        assert_eq!(hex(&ones(166)), format!("3{}", "f".repeat(41)));
    }

    #[test]
    fn be_address() {
        assert_eq!(hex(&msb16(0x20)), "0400");
        assert_eq!(hex(&msb16(0x827)), "e410");
    }

    #[test]
    fn undefined_nibble() {
        let mut b = zeros(8);
        b.push(UNDEF);
        assert_eq!(hex(&b), "`00");
    }
}
