//! Device database (§4) and instructions (§1.3).

/// Device families (§1.5, §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    F1502AS,
    F1504AS,
    F1508AS,
    F1502BE,
    F1504BE,
    F1508BE,
}

impl Family {
    pub const ALL: [Family; 6] = [
        Family::F1502AS,
        Family::F1504AS,
        Family::F1508AS,
        Family::F1502BE,
        Family::F1504BE,
        Family::F1508BE,
    ];

    pub fn qf(self) -> usize {
        match self {
            Family::F1502AS => 16808,
            Family::F1504AS => 34192,
            Family::F1508AS => 74136,
            Family::F1502BE => 16814,
            Family::F1504BE => 34198,
            Family::F1508BE => 74146,
        }
    }

    pub fn from_qf(qf: usize) -> Option<Family> {
        Family::ALL.iter().copied().find(|f| f.qf() == qf)
    }

    pub fn label(self) -> &'static str {
        match self {
            Family::F1502AS => "1502AS",
            Family::F1504AS => "1504AS",
            Family::F1508AS => "1508AS",
            Family::F1502BE => "1502BE",
            Family::F1504BE => "1504BE",
            Family::F1508BE => "1508BE",
        }
    }

    pub fn is_as(self) -> bool {
        matches!(self, Family::F1502AS | Family::F1504AS | Family::F1508AS)
    }

    /// (rows, row length) — §4.4
    pub fn geometry(self) -> (usize, usize) {
        match self {
            Family::F1502AS => (229, 86),
            Family::F1504AS => (233, 166),
            Family::F1508AS => (251, 326),
            Family::F1502BE => (2090, 8),
            Family::F1504BE => (4252, 8),
            Family::F1508BE => (9226, 8),
        }
    }

    /// First fuse of (config, special, UES, reserved) and the special-bit count — §4.4
    pub fn layout(self) -> (usize, usize, usize, usize, usize) {
        match self {
            Family::F1502AS => (16750, 16782, 16786, 16802, 4),
            Family::F1504AS => (34134, 34166, 34170, 34186, 4),
            Family::F1508AS => (74082, 74114, 74118, 74134, 4),
            Family::F1502BE => (16750, 16782, 16790, 16806, 8),
            Family::F1504BE => (34134, 34166, 34174, 34190, 8),
            Family::F1508BE => (74082, 74114, 74122, 74138, 8),
        }
    }

    /// JTAG-enable fuse (§4.4): the second configuration fuse (configuration start + 33) in
    /// every family; the fitters' JTAG option sets it for the AS families. Only used for our
    /// warning: ATMISP does not check it in SVF mode. `None` would mean "always enabled".
    pub fn jtag_fuse(self) -> Option<usize> {
        Some(self.layout().0 + 33)
    }
}

/// Instructions (§1.3). `NoneOrInvalid` is ATMISP's empty instruction name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instr {
    Bypass,
    ProgramVerify,
    Erase,
    BlankCheck,
    Verify,
    Load,
    Secure,
    ProgramVerifySecure,
    VerifySecure,
    ReadUes,
    NoneOrInvalid,
}

impl Instr {
    pub fn from_code(c: &str) -> Instr {
        match c {
            "0" => Instr::Bypass,
            "1" => Instr::ProgramVerify,
            "2" => Instr::Erase,
            "3" => Instr::BlankCheck,
            "4" => Instr::Verify,
            "5" => Instr::Load,
            "6" => Instr::Secure,
            "7" => Instr::ProgramVerifySecure,
            "8" => Instr::VerifySecure,
            "9" => Instr::ReadUes,
            _ => Instr::NoneOrInvalid,
        }
    }

    pub fn code(self) -> Option<u8> {
        Some(match self {
            Instr::Bypass => 0,
            Instr::ProgramVerify => 1,
            Instr::Erase => 2,
            Instr::BlankCheck => 3,
            Instr::Verify => 4,
            Instr::Load => 5,
            Instr::Secure => 6,
            Instr::ProgramVerifySecure => 7,
            Instr::VerifySecure => 8,
            Instr::ReadUes => 9,
            Instr::NoneOrInvalid => return None,
        })
    }

    /// ATMISP's spelling.
    pub fn name(self) -> &'static str {
        match self {
            Instr::Bypass => "Bypass",
            Instr::ProgramVerify => "Program/Verify",
            Instr::Erase => "Erase",
            Instr::BlankCheck => "Blank Check",
            Instr::Verify => "Verify",
            Instr::Load => "Load",
            Instr::Secure => "Secure",
            Instr::ProgramVerifySecure => "Program/Verify/Secure",
            Instr::VerifySecure => "Verify Secure",
            Instr::ReadUes => "Read UES",
            Instr::NoneOrInvalid => "",
        }
    }

    /// Instructions whose JEDEC file is read (§3.1).
    pub fn reads_jedec(self) -> bool {
        matches!(
            self,
            Instr::ProgramVerify | Instr::Verify | Instr::ProgramVerifySecure
        )
    }

    /// Instructions whose verify scans carry a MASK (§7.3 CHECK).
    pub fn verify_with_mask(self) -> bool {
        self.reads_jedec()
    }

    /// Parse a CLI instruction (digit, ATMISP name or alias; §10.2).
    pub fn parse_cli(s: &str) -> Option<Instr> {
        if s.len() == 1 && s.as_bytes()[0].is_ascii_digit() {
            return Some(Instr::from_code(s));
        }
        let k: String = s
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        Some(match k.as_str() {
            "bypass" => Instr::Bypass,
            "programverify" | "pv" | "program" => Instr::ProgramVerify,
            "erase" => Instr::Erase,
            "blankcheck" | "blank" => Instr::BlankCheck,
            "verify" => Instr::Verify,
            "load" | "read" => Instr::Load,
            "secure" => Instr::Secure,
            "programverifysecure" | "pvs" => Instr::ProgramVerifySecure,
            "verifysecure" => Instr::VerifySecure,
            "readues" | "ues" => Instr::ReadUes,
            _ => return None,
        })
    }
}

/// Names that `readline` treats as device-name lines (§2.2.2).
pub const NAME_LINES: [&str; 19] = [
    "ATF1508AS",
    "ATF1508ASV",
    "ATF1504AS",
    "ATF1504ASV",
    "ATF1502AS",
    "ATF1502ASV",
    "ATF1508ASL",
    "ATF1508ASVL",
    "ATF1504ASL",
    "ATF1504ASVL",
    "ATF1502ASL",
    "ATF1502ASVL",
    "ATF1502SE",
    "ATF1502SEL",
    "ATF1502BE",
    "ATF1504BE",
    "ATF1508BE",
    "ATF1508RE",
    "UNKNOWN",
];

/// Names whose IR width is forced to 10 by the chain reader (§2.2.2 step 5).
pub const SUPPORTED: [&str; 15] = [
    "ATF1502AS",
    "ATF1502ASL",
    "ATF1502ASV",
    "ATF1502BE",
    "ATF1504AS",
    "ATF1504ASL",
    "ATF1504ASV",
    "ATF1504ASVL",
    "ATF1504BE",
    "ATF1508AS",
    "ATF1508ASL",
    "ATF1508ASV",
    "ATF1508ASVL",
    "ATF1508BE",
    "ATF1508RE",
];

/// Names accepted by `atf15xx-tool` (§9.2 N1): the table of §4.1 minus the SE parts.
pub fn is_accepted_name(n: &str) -> bool {
    n == "UNKNOWN" || n == "ATF1502ASVL" || SUPPORTED.contains(&n)
}

/// IDCODE table (§4.1), exact name match.
pub fn idcode(n: &str) -> Option<u32> {
    Some(match n {
        "ATF1502AS" | "ATF1502ASL" => 0x0150203F,
        "ATF1502ASV" => 0x0151203F,
        "ATF1502SE" | "ATF1502SEL" => 0x0152203F,
        "ATF1502BE" => 0x0156203F,
        "ATF1504AS" | "ATF1504ASL" => 0x0150403F,
        "ATF1504ASV" | "ATF1504ASVL" => 0x0151403F,
        "ATF1504BE" => 0x0154403F,
        "ATF1508AS" | "ATF1508ASL" => 0x0150803F,
        "ATF1508ASV" | "ATF1508ASVL" => 0x0151803F,
        "ATF1508BE" => 0x0154803F,
        "ATF1508RE" => 0x0155803F,
        _ => return None,
    })
}

/// Voltage classes for the mixed-voltage check (§9.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VClass {
    V5,
    V3,
    BE,
}

pub fn voltage_class(n: &str) -> Option<VClass> {
    match n {
        "ATF1502AS" | "ATF1502ASL" | "ATF1502SE" | "ATF1502SEL" | "ATF1504AS" | "ATF1504ASL"
        | "ATF1508AS" | "ATF1508ASL" => Some(VClass::V5),
        "ATF1502ASV" | "ATF1502ASVL" | "ATF1504ASV" | "ATF1504ASVL" | "ATF1508ASV"
        | "ATF1508ASVL" => Some(VClass::V3),
        "ATF1502BE" | "ATF1504BE" | "ATF1508BE" | "ATF1508RE" => Some(VClass::BE),
        _ => None,
    }
}

// ---- substring predicates (§4.2) ----
pub fn is_as(n: &str) -> bool {
    n.contains("AS")
}
pub fn is_be_or_re(n: &str) -> bool {
    n.contains("BE") || n.contains("RE")
}
pub fn is_unknown(n: &str) -> bool {
    n.contains("UNKNOWN")
}
pub fn is_atf150(n: &str) -> bool {
    n.contains("ATF150")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeKind {
    K1502,
    K1504,
    K1508,
}

pub fn be_kind(n: &str) -> BeKind {
    if n.contains("1502BE") {
        BeKind::K1502
    } else if n.contains("1504BE") {
        BeKind::K1504
    } else {
        BeKind::K1508
    }
}

/// Expected QF from the device name (§4.3).
pub fn expected_family(n: &str) -> Option<Family> {
    if n.contains("1502AS") || n.contains("1502SE") {
        Some(Family::F1502AS)
    } else if n.contains("1502BE") {
        Some(Family::F1502BE)
    } else if n.contains("1504AS") {
        Some(Family::F1504AS)
    } else if n.contains("1504BE") {
        Some(Family::F1504BE)
    } else if n.contains("1508AS") {
        Some(Family::F1508AS)
    } else if n.contains("1508BE") || n.contains("1508RE") {
        Some(Family::F1508BE)
    } else {
        None
    }
}

/// Opcodes (§4.5).
pub mod op {
    pub const IDCODE: u64 = 0x059;
    pub const ISC_AS: u64 = 0x280;
    pub const ISC_EN_BE: u64 = 0x2A4;
    pub const ISC_DIS_BE: u64 = 0x2A5;
    pub const ISC_KEY: u64 = 0x1B9;
    pub const ADDR: u64 = 0x2A1;
    pub const DATA: u64 = 0x290;
    pub const BANK2: u64 = 0x294;
    pub const CONFIG: u64 = 0x291;
    pub const SPECIAL: u64 = 0x292;
    pub const UES: u64 = 0x293;
    pub const PROGRAM: u64 = 0x29E;
    pub const PROGRAM_END: u64 = 0x2BF;
    pub const ERASE_AS: u64 = 0x2B3;
    pub const READ: u64 = 0x28C;
    pub const PAGE: u64 = 0x2A2;
    pub const BYTE: u64 = 0x2A3;
    pub const PROGRAM_BE: u64 = 0x2A6;
    pub const READ_UES: u64 = 0x270;
}
