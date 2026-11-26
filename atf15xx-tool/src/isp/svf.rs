//! SVF writer primitives (§6).

use crate::isp::bits::hex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rev {
    C,
    D,
}

#[derive(Debug, Clone)]
pub struct SvfOptions {
    pub rev: Rev,
    /// TCK period in µs, 1..=1000 (§6.5)
    pub tck: u32,
    pub state_reset: bool,
    /// Correct the defects marked † in §9.4.
    pub strict: bool,
}

impl Default for SvfOptions {
    fn default() -> Self {
        SvfOptions {
            rev: Rev::D,
            tck: 1,
            state_reset: true,
            strict: false,
        }
    }
}

/// Accumulates SVF text. Lines are terminated with CR LF (§6.1).
pub struct Svf {
    pub opt: SvfOptions,
    out: String,
}

impl Svf {
    pub fn new(opt: SvfOptions) -> Svf {
        Svf {
            opt,
            out: String::with_capacity(1 << 20),
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.out.into_bytes()
    }

    pub fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push_str("\r\n");
    }

    /// §6.3
    pub fn header(&mut self, ctime: &str) {
        self.line("//  SVF file written by ATMISP version 7.3");
        self.line(&format!("//  Created on: {ctime}"));
        self.line(&format!(
            "//  SVF Rev. {}  ",
            if self.opt.rev == Rev::C { 'C' } else { 'D' }
        ));
        for l in [
            "TRST ABSENT;",
            "ENDIR IDLE;",
            "ENDDR IDLE;",
            "HDR 0;",
            "HIR 0;",
            "TDR 0;",
            "TIR 0;",
        ] {
            self.line(l);
        }
    }

    pub fn sir(&mut self, b: &[u8]) {
        self.line(&format!("SIR {} TDI ({});", b.len(), hex(b)));
    }

    pub fn sdr(&mut self, b: &[u8]) {
        self.line(&format!("SDR {} TDI ({});", b.len(), hex(b)));
    }

    pub fn sdr_tdo(&mut self, b: &[u8], t: &[u8]) {
        self.line(&format!("SDR {} TDI ({})", b.len(), hex(b)));
        self.line(&format!("\tTDO ({});", hex(t)));
    }

    pub fn sdr_tdo_mask(&mut self, b: &[u8], t: &[u8], m: &[u8]) {
        self.line(&format!("SDR {} TDI ({})", b.len(), hex(b)));
        self.line(&format!("\tTDO ({})", hex(t)));
        self.line(&format!("\tMASK ({});", hex(m)));
    }

    pub fn idle(&mut self) {
        self.line("STATE IDLE;");
    }

    pub fn reset(&mut self) {
        if self.opt.state_reset {
            self.line("STATE RESET;");
        }
    }

    /// `DELAY(ns, us, ms)` (§6.4)
    pub fn delay(&mut self, ns: u32, us: u32, ms: u32) {
        let t: i64 = (ns / 1000) as i64 + us as i64 + 1 + 1000 * ms as i64;
        match self.opt.rev {
            Rev::D => self.line(&format!("RUNTEST {t}E-6 SEC;")),
            Rev::C => {
                let n = if self.opt.strict {
                    // D5 corrected: ⌈t / TCK⌉
                    (t + self.opt.tck as i64 - 1) / self.opt.tck as i64
                } else {
                    t / (self.opt.tck as i64 * 1000)
                };
                self.line(&format!("RUNTEST {n} TCK;"));
            }
        }
    }

    /// `CLOCKS(n)`
    pub fn clocks(&mut self, n: u32) {
        self.line(&format!("RUNTEST {n} TCK;"));
    }
}
