//! Conversion C1 (chain → SVF) and the run algorithm (§7), with the validation of §9.

use crate::isp::bits::{bits, msb16, ones, prepend, zeros, Bits, UNDEF};
use crate::isp::chain::{Chain, Device};
use crate::isp::device::{
    self, be_kind, is_as, is_atf150, is_be_or_re, is_unknown, op, BeKind, Family, Instr, VClass,
};
use crate::isp::diag::{Diagnostics, Error};
use crate::isp::fusemap::{DeviceImage, FuseMap};
use crate::isp::jedec::{self, J1};
use crate::isp::jedout;
use crate::isp::svf::{Svf, SvfOptions};

#[derive(Debug, Clone)]
pub struct GenOptions {
    pub svf: SvfOptions,
    pub allow_mixed_voltage: bool,
    /// Date text for the SVF `Created on:` line and the Load JEDEC `Created:` line (§6.3, §8.2).
    pub ctime: String,
    pub load_output: bool,
}

#[derive(Debug)]
pub struct Output {
    pub svf: Vec<u8>,
    /// (device index 1-based, resolved JEDEC path as written in the chain, file bytes)
    pub jedec_outputs: Vec<(usize, String, Vec<u8>)>,
    pub diag: Diagnostics,
}

pub const V1: &str = "Cannot program ATF15xxBE with other device types!";
pub const V2: &str = "Mixed voltage programming is not suggested! Do you want to continue?";

struct Dev<'a> {
    d: &'a Device,
    image: Option<DeviceImage>,
    security: u8,
}

/// Run conversion C1. `read_jed(index, path)` returns the bytes of a device's JEDEC file.
pub fn generate(
    chain: &Chain,
    mut read_jed: impl FnMut(usize, &str) -> Result<Vec<u8>, Error>,
    opts: &GenOptions,
    mut diag: Diagnostics,
) -> Result<Output, Error> {
    // ---- §9.2 N1 / N2 ----
    for (i, d) in chain.devices.iter().enumerate() {
        let passive = matches!(d.instr, Instr::Bypass | Instr::NoneOrInvalid);
        if d.name.contains("1502SE") || (!device::is_accepted_name(&d.name) && !passive) {
            return Err(Error::Input(format!(
                "Unsupported device name '{}' for device # {}",
                d.name,
                i + 1
            )));
        }
        if d.name == "UNKNOWN" && !matches!(d.instr, Instr::Bypass | Instr::NoneOrInvalid) {
            return Err(Error::Input(format!(
                "Bypass must be selected for all UNKNOWN devices (device # {})",
                i + 1
            )));
        }
    }
    // ---- §9.1 family / voltage mix ----
    let mut s = 0;
    for (cls, w) in [(VClass::V5, 5), (VClass::V3, 6), (VClass::BE, 7)] {
        if chain
            .devices
            .iter()
            .any(|d| device::voltage_class(&d.name) == Some(cls))
        {
            s += w;
        }
    }
    if s >= 12 {
        return Err(Error::Input(V1.into()));
    }
    if s >= 8 {
        if !opts.allow_mixed_voltage {
            return Err(Error::Input(format!(
                "{V2}\n(use --allow-mixed-voltage to continue)"
            )));
        }
        diag.warn("Mixed voltage programming is not suggested!");
    }
    // ---- device images (§3, §5) ----
    let mut devs = Vec::new();
    for (i, d) in chain.devices.iter().enumerate() {
        let x = i + 1;
        let mut image = None;
        let mut security = b'0';
        if let Some(fam) = device::expected_family(&d.name) {
            if d.instr.reads_jedec() {
                let bytes = read_jed(x, &d.jed_path)?;
                let j = jedec::parse(&bytes)?;
                if j.family() != Some(fam) {
                    return Err(Error::Input(J1.into()));
                }
                if let Some(jf) = fam.jtag_fuse() {
                    if j.fuses[jf] != b'1'
                        && matches!(d.instr, Instr::ProgramVerify | Instr::ProgramVerifySecure)
                    {
                        diag.warn(format!(
                            "JEDEC file for device # {x} has the JTAG port disabled; the device cannot be reprogrammed via JTAG afterwards"
                        ));
                    }
                }
                security = j.security;
                image = Some(DeviceImage::from_fuses(&FuseMap::new(fam), &j.fuses));
            } else {
                image = Some(DeviceImage::blank(fam));
            }
        }
        if d.name == "ATF1502ASVL" {
            diag.warn("ATF1502ASVL has no IDCODE in ATMISP's table; the IDCODE scan expects a single '1' bit");
        }
        devs.push(Dev { d, image, security });
    }
    let mut g = Gen {
        devs: &devs,
        s: Svf::new(opts.svf.clone()),
        diag,
    };
    g.run(&opts.ctime);
    let Gen { s, mut diag, .. } = g;
    // ---- §8 Load outputs ----
    let mut jedec_outputs = Vec::new();
    if opts.load_output {
        for (i, dv) in devs.iter().enumerate() {
            if dv.d.instr == Instr::Load {
                if let (Some(fam), Some(img)) =
                    (device::expected_family(&dv.d.name), dv.image.as_ref())
                {
                    jedec_outputs.push((
                        i + 1,
                        dv.d.jed_path.clone(),
                        jedout::write(fam, img, &opts.ctime),
                    ));
                }
            }
        }
    }
    diag.transcript.shrink_to_fit();
    Ok(Output {
        svf: s.into_bytes(),
        jedec_outputs,
        diag,
    })
}

struct Gen<'a> {
    devs: &'a [Dev<'a>],
    s: Svf,
    diag: Diagnostics,
}

fn ir(opcode: u64) -> Bits {
    bits(opcode, 10)
}

impl<'a> Gen<'a> {
    fn n(&self) -> usize {
        self.devs.len()
    }
    fn dev(&self, x: usize) -> &Dev<'a> {
        &self.devs[x - 1]
    }
    fn img(&self, x: usize) -> &DeviceImage {
        self.dev(x).image.as_ref().expect("device image")
    }
    fn name(&self, x: usize) -> &str {
        &self.dev(x).d.name
    }

    // ---- §6.6 chain composition ----
    fn cir(&self, x: usize, opcode: u64) -> Bits {
        let mut s = Bits::new();
        for i in 1..=self.n() {
            let c = if i == x {
                ir(opcode)
            } else if is_unknown(self.name(i)) {
                ones(self.dev(i).d.ir_width as usize)
            } else {
                ones(10)
            };
            s = prepend(&c, &s);
        }
        s
    }
    fn cdr(&self, x: usize, data: &[u8]) -> Bits {
        let mut s = Bits::new();
        for i in 1..=self.n() {
            s = if i == x {
                prepend(data, &s)
            } else {
                prepend(b"0", &s)
            };
        }
        s
    }
    fn cadr(&self, x: usize, prefix: &[u8], r: u64) -> Bits {
        let a = prepend(&bits(r, 8), prefix);
        let mut s = Bits::new();
        for i in 1..=self.n() {
            s = if i == x {
                prepend(&a, &s)
            } else {
                prepend(b"1", &s)
            };
        }
        s
    }
    fn sir(&mut self, x: usize, opcode: u64) {
        let b = self.cir(x, opcode);
        self.s.sir(&b);
    }
    fn sdr(&mut self, x: usize, data: &[u8]) {
        let b = self.cdr(x, data);
        self.s.sdr(&b);
    }
    fn check(&mut self, x: usize, b: &[u8]) {
        if self.dev(x).d.instr.verify_with_mask() {
            self.s.sdr_tdo_mask(b, b, &ones(b.len()));
        } else {
            self.s.sdr_tdo(b, b);
        }
    }

    // ---- §7.2 chain-wide ----
    fn init(&mut self) {
        self.s.delay(0, 20, 50);
        self.s.reset();
        self.s.idle();
        self.s.delay(0, 20, 50);
    }
    fn isc(&mut self, enable: bool) {
        let (mut irs, mut drs) = (Bits::new(), Bits::new());
        for i in 1..=self.n() {
            let n = self.name(i);
            let (a, b) = if is_be_or_re(n) {
                if enable {
                    (ir(op::ISC_EN_BE), bits(op::ISC_KEY, 10))
                } else {
                    (ir(op::ISC_DIS_BE), zeros(10))
                }
            } else if is_unknown(n) {
                (ones(self.dev(i).d.ir_width as usize), zeros(1))
            } else if enable {
                (ir(op::ISC_AS), bits(op::ISC_KEY, 10))
            } else {
                (ir(op::ISC_AS), zeros(10))
            };
            irs = prepend(&a, &irs);
            drs = prepend(&b, &drs);
        }
        self.s.sir(&irs);
        self.s.sdr(&drs);
        self.s.idle();
        if !enable {
            self.s.delay(0, 0, 10);
        }
    }
    fn idcode_scan(&mut self) {
        let (mut irs, mut id) = (Bits::new(), Bits::new());
        for i in 1..=self.n() {
            let n = self.name(i).to_string();
            let a = if is_atf150(&n) {
                ir(op::IDCODE)
            } else {
                ones(self.dev(i).d.ir_width as usize)
            };
            let e = match device::idcode(&n) {
                Some(v) => bits(v as u64, 32),
                None if self.s.opt.strict && is_unknown(&n) => b"0".to_vec(), // D4 corrected
                None => b"1".to_vec(),
            };
            irs = prepend(&a, &irs);
            id = prepend(&e, &id);
        }
        let o = ones(id.len());
        self.s.sir(&irs);
        self.s.sdr_tdo_mask(&o, &id, &o);
        self.s.idle();
    }
    fn final_(&mut self) {
        self.s.reset();
        self.s.delay(0, 20, 50);
    }
    fn reinit(&mut self) {
        self.isc(false);
        self.final_();
        self.init();
        self.isc(true);
    }

    // ---- §7.3 AS-type ----
    fn as_ranges(&self, x: usize) -> Vec<(usize, usize)> {
        let last = match device::expected_family(self.name(x)) {
            Some(Family::F1502AS) => Some(0xE4),
            Some(Family::F1504AS) => Some(0xE8),
            Some(Family::F1508AS) => Some(0xFA),
            _ => None,
        };
        let mut v = vec![(0x0C, 0x6B), (0x80, 0xDF), (0x00, 0x0B)];
        if let Some(l) = last {
            v.push((0xE0, l));
        }
        v
    }
    fn erase(&mut self, x: usize) {
        if is_as(self.name(x)) {
            self.sir(x, op::ERASE_AS);
            self.sir(x, op::PROGRAM);
            self.s.idle();
            self.s.delay(0, 0, 210);
            self.sir(x, op::PROGRAM_END);
            self.s.idle();
            self.s.reset();
            self.s.delay(0, 0, 50);
            self.s.idle();
        } else {
            self.sir(x, op::ISC_EN_BE);
            self.sdr(x, &bits(op::ISC_KEY, 10));
            self.s.idle();
            self.sir(x, op::UES);
            self.s.idle();
            self.s.delay(0, 0, 1800);
            self.sir(x, op::ISC_DIS_BE);
            self.sdr(x, &zeros(10));
            self.s.idle();
        }
    }
    fn prog_reg(&mut self, x: usize, data: &[u8], opcode: u64, prefix: &[u8]) {
        self.sir(x, op::ADDR);
        let a = self.cadr(x, prefix, 0);
        self.s.sdr(&a);
        self.sir(x, opcode);
        self.sdr(x, data);
        self.sir(x, op::PROGRAM);
        self.s.idle();
        self.s.delay(0, 0, 30);
        self.sir(x, op::PROGRAM_END);
        self.s.idle();
    }
    fn ver_reg(&mut self, x: usize, data: &[u8], opcode: u64, prefix: &[u8]) {
        self.sir(x, op::ADDR);
        let a = self.cadr(x, prefix, 0);
        self.s.sdr(&a);
        self.sir(x, op::READ);
        self.s.idle();
        self.s.delay(0, 0, 20);
        self.sir(x, opcode);
        let b = self.cdr(x, data);
        self.check(x, &b);
        self.s.idle();
    }
    fn as_program(&mut self, x: usize) {
        for (r0, r1) in self.as_ranges(x) {
            for r in r0..=r1 {
                self.sir(x, op::ADDR);
                let a = self.cadr(x, b"000", r as u64);
                self.s.sdr(&a);
                self.sir(x, op::DATA);
                let row = self.img(x).rows[r].clone();
                self.sdr(x, &row);
                self.sir(x, op::PROGRAM);
                self.s.idle();
                self.s.delay(0, 0, 30);
                self.sir(x, op::PROGRAM_END);
            }
            self.s.idle();
        }
        let im = self.img(x).clone();
        self.prog_reg(x, &im.special, op::SPECIAL, b"010");
        self.prog_reg(x, &im.ues, op::UES, b"110");
        self.prog_reg(x, &im.config, op::CONFIG, b"100");
    }
    fn as_verify(&mut self, x: usize) {
        for (r0, r1) in self.as_ranges(x) {
            for r in r0..=r1 {
                self.sir(x, op::ADDR);
                let a = self.cadr(x, b"000", r as u64);
                self.s.sdr(&a);
                self.sir(x, op::READ);
                self.s.idle();
                self.s.delay(0, 0, 20);
                self.sir(x, op::DATA);
                let b = self.cdr(x, &self.img(x).rows[r]);
                self.check(x, &b);
            }
            self.s.idle();
        }
        let im = self.img(x).clone();
        self.ver_reg(x, &im.special, op::SPECIAL, b"010");
        self.ver_reg(x, &im.ues, op::UES, b"110");
        self.ver_reg(x, &im.config, op::CONFIG, b"100");
    }

    // ---- §7.4/§7.5 BE-type ----
    fn row8(&self, x: usize, r: usize) -> Bits {
        let mut v = self.img(x).rows[r].clone();
        v.reverse();
        v
    }
    fn page(&self, x: usize, p: usize, off: usize) -> Bits {
        let mut v = Bits::with_capacity(256);
        for k in 0..32 {
            v.extend(self.row8(x, off + 32 * p + k));
        }
        v
    }
    fn hs(&mut self, x: usize, sel: u64) {
        self.sir(x, sel);
        self.s.idle();
        self.s.delay(100, 0, 0);
    }
    fn be_ranges(&self, x: usize) -> Vec<(usize, usize, usize, usize, bool)> {
        // (pages last, rows first, rows last, bank offset, bank2)
        match be_kind(self.name(x)) {
            BeKind::K1502 => vec![(0x40, 0x81E, 0x829, 0, false)],
            BeKind::K1504 => vec![(0x83, 0x1080, 0x109B, 0, false)],
            BeKind::K1508 => vec![
                (0x95, 0x12C0, 0x12CB, 0, false),
                (0x88, 0x1120, 0x113D, 4812, true),
            ],
        }
    }
    fn be_program(&mut self, x: usize) {
        for (plast, r0, r1, off, bank2) in self.be_ranges(x) {
            let sel = if bank2 { op::BANK2 } else { op::DATA };
            for p in 0..=plast {
                self.hs(x, sel);
                self.sir(x, op::SPECIAL);
                self.sdr(x, b"00000110");
                self.s.idle();
                self.s.clocks(8);
                self.s.clocks(10);
                if bank2 {
                    self.s.delay(100, 0, 0);
                }
                self.hs(x, op::CONFIG);
                self.hs(x, sel);
                self.sir(x, op::SPECIAL);
                self.sdr(x, b"00000010");
                self.s.idle();
                self.s.clocks(8);
                self.s.clocks(5);
                self.sir(x, op::ADDR);
                self.sdr(x, &msb16((32 * p) as u32));
                self.s.idle();
                self.s.clocks(16);
                self.sir(x, op::PAGE);
                let pg = self.page(x, p, off);
                self.sdr(x, &pg);
                self.s.idle();
                self.s.clocks(256);
                self.sir(x, op::PROGRAM_BE);
                self.s.idle();
                self.s.delay(100, 0, 10);
                self.hs(x, op::CONFIG);
            }
            for r in r0..=r1 {
                self.hs(x, sel);
                self.sir(x, op::SPECIAL);
                self.sdr(x, b"00000110");
                self.s.idle();
                if bank2 {
                    self.s.clocks(8);
                    self.s.clocks(10);
                    self.s.delay(100, 0, 0);
                } else {
                    self.s.clocks(8);
                    self.s.delay(100, 0, 0);
                    self.s.clocks(10);
                }
                self.hs(x, op::CONFIG);
                self.hs(x, sel);
                self.sir(x, op::SPECIAL);
                self.sdr(x, b"00000010");
                self.s.idle();
                self.s.clocks(8);
                if bank2 {
                    self.s.clocks(5);
                }
                self.sir(x, op::ADDR);
                self.sdr(x, &msb16(r as u32));
                self.s.idle();
                self.s.clocks(16);
                self.sir(x, op::BYTE);
                let rw = self.row8(x, off + r);
                self.sdr(x, &rw);
                self.s.idle();
                self.s.clocks(8);
                self.sir(x, op::PROGRAM_BE);
                self.s.idle();
                self.s.delay(100, 0, 10);
                self.hs(x, op::CONFIG);
            }
        }
    }
    fn be_verify_one(&mut self, x: usize, sel: u64, addr: u32, opcode: u64, data: &[u8]) {
        self.hs(x, sel);
        self.sir(x, op::SPECIAL);
        self.sdr(x, b"00000011");
        self.s.idle();
        self.s.clocks(8);
        self.sir(x, op::ADDR);
        self.sdr(x, &msb16(addr));
        self.s.idle();
        self.s.clocks(16);
        self.sir(x, opcode);
        let b = self.cdr(x, data);
        self.check(x, &b);
        self.s.idle();
        self.s.delay(100, 0, 0);
        self.hs(x, op::CONFIG);
    }
    fn be_verify(&mut self, x: usize) {
        for (plast, r0, r1, off, bank2) in self.be_ranges(x) {
            let sel = if bank2 { op::BANK2 } else { op::DATA };
            for p in 0..=plast {
                let pg = self.page(x, p, off);
                self.be_verify_one(x, sel, (32 * p) as u32, op::PAGE, &pg);
            }
            for r in r0..=r1 {
                let rw = self.row8(x, off + r);
                self.be_verify_one(x, sel, r as u32, op::BYTE, &rw);
            }
        }
    }
    fn be_read(&mut self, x: usize, r: u32, bank2: bool) {
        self.hs(x, if bank2 { op::BANK2 } else { op::DATA });
        self.sir(x, op::SPECIAL);
        self.sdr(x, b"00000011");
        self.s.idle();
        self.s.clocks(8);
        self.sir(x, op::ADDR);
        self.sdr(x, &msb16(r));
        self.s.idle();
        self.s.clocks(16);
        self.sir(x, op::BYTE);
        let data: Bits = if self.s.opt.strict {
            zeros(8) // D2/D3 corrected
        } else if bank2 {
            let mut v = zeros(8);
            v.push(UNDEF); // D3
            v
        } else {
            zeros(9) // D2
        };
        let b = self.cdr(x, &data);
        self.s.sdr_tdo(&b, &b);
        self.s.idle();
        self.s.delay(100, 0, 0);
        self.hs(x, op::CONFIG);
    }

    // ---- dispatch (§7.3/§7.5/§7.6) ----
    fn program(&mut self, x: usize) {
        if is_as(self.name(x)) {
            self.as_program(x)
        } else {
            self.be_program(x)
        }
    }
    fn verify(&mut self, x: usize) {
        if is_as(self.name(x)) {
            self.as_verify(x)
        } else {
            self.be_verify(x)
        }
    }
    fn secure(&mut self, x: usize) {
        let n = self.name(x).to_string();
        if is_as(&n) {
            self.prog_reg(x, b"1110", op::SPECIAL, b"010");
        } else if is_be_or_re(&n) {
            if n.contains("1502BE") {
                self.be_read(x, 0x827, false);
            }
            if n.contains("1504BE") {
                self.be_read(x, 0x1099, false);
            }
            if n.contains("1508BE") || n.contains("1508RE") {
                self.be_read(x, 0x113B, true);
            }
        }
    }
    fn verify_secure(&mut self, x: usize) {
        let n = self.name(x).to_string();
        if is_as(&n) {
            self.sir(x, op::ADDR);
            let a = self.cadr(x, b"100", 0);
            self.s.sdr(&a);
            self.sir(x, op::READ);
            self.s.idle();
            self.s.delay(0, 0, 20);
            self.sir(x, op::CONFIG);
            let w = if self.s.opt.strict { 32 } else { 33 }; // D2
            self.sdr(x, &zeros(w));
            self.s.idle();
        } else if is_be_or_re(&n) {
            self.be_read(x, 0, false);
        }
    }
    fn read_ues(&mut self, x: usize) {
        self.sir(x, op::READ_UES);
        self.s.idle();
        self.s.delay(0, 0, 20);
        let b = self.cdr(x, &zeros(16));
        self.check(x, &b);
        self.s.idle();
    }

    // ---- §7.1 ----
    fn run(&mut self, ctime: &str) {
        self.s.header(ctime);
        self.diag
            .log("...  executing JTAG instructions...please wait...");
        self.init();
        self.isc(true);
        self.idcode_scan();
        for x in 1..=self.n() {
            if matches!(
                self.dev(x).d.instr,
                Instr::Erase | Instr::ProgramVerify | Instr::ProgramVerifySecure
            ) {
                self.erase(x);
                self.diag.log(format!("log: erasing device #{x}."));
                self.reinit();
            }
        }
        for x in 1..=self.n() {
            let instr = self.dev(x).d.instr;
            let be = is_be_or_re(self.name(x)) && !is_as(self.name(x));
            let w7 = |i: &str| {
                format!("log: warning: SVF generation procedure cannot be used with {i} procedure together.  This is the software limitation.")
            };
            match instr {
                Instr::Load => {
                    self.reinit();
                    self.verify_secure(x);
                    if be {
                        self.diag.log(format!(
                            "warning: Device #{x} is secured, output jedec is blank."
                        ));
                        self.diag.run_failed = true;
                    }
                    self.diag.log(format!("log: reading device #{x}."));
                    self.verify(x);
                }
                Instr::ProgramVerify => {
                    self.diag.log(format!("log: programming device #{x}."));
                    self.program(x);
                    self.verify(x);
                    self.diag
                        .log(format!("log: verifying device #{x}: ... OK ..."));
                    if self.dev(x).security == b'1' {
                        self.secure(x);
                    }
                }
                Instr::ProgramVerifySecure => {
                    self.diag.log(format!("log: programming device #{x}."));
                    self.program(x);
                    self.verify(x);
                    self.diag
                        .log(format!("log: verifying device #{x}: ... OK ..."));
                    self.diag.log(format!("log: securing device #{x}."));
                    self.secure(x);
                    self.diag
                        .log(format!("log: verifying secure for device #{x}:"));
                    self.reinit();
                    self.verify_secure(x);
                    self.diag.log(w7("Program/Verify/Secure"));
                    self.diag.warn(
                        w7("Program/Verify/Secure")
                            .trim_start_matches("log: ")
                            .to_string(),
                    );
                }
                Instr::Secure => {
                    self.diag.log(format!("log: securing device #{x}."));
                    self.secure(x);
                    self.diag
                        .log(format!("log: verifying if device #{x} secured:"));
                    self.reinit();
                    self.verify_secure(x);
                    self.diag.log(w7("Secure"));
                    self.diag
                        .warn(w7("Secure").trim_start_matches("log: ").to_string());
                }
                Instr::Verify => {
                    self.verify(x);
                    self.diag
                        .log(format!("log: verifying device #{x}: ... OK ..."));
                }
                Instr::VerifySecure => {
                    self.diag
                        .log(format!("log: verifying secure for device #{x}:"));
                    self.reinit();
                    self.verify_secure(x);
                    self.diag.log(w7("Verify Secure"));
                    self.diag
                        .warn(w7("Verify Secure").trim_start_matches("log: ").to_string());
                }
                Instr::BlankCheck => {
                    self.verify(x);
                    self.reinit();
                    self.verify_secure(x);
                    if be {
                        self.diag.log(format!(
                            "log: blank check device #{x}: ... failed blank check since device is secured ..."
                        ));
                        self.diag.run_failed = true;
                    } else {
                        self.diag.log(format!(
                            "log: blank check device #{x}: ... OK ...device is blank."
                        ));
                    }
                }
                Instr::Erase => {
                    self.verify(x);
                    self.diag.log(format!(
                        "log: blank check device #{x}: ... OK ...device is blank."
                    ));
                }
                Instr::ReadUes => {
                    self.diag.log(format!(
                        "log: read User Electronic Signature from device #{x}:"
                    ));
                    self.read_ues(x);
                    self.diag.log("     UES = \"");
                }
                Instr::Bypass | Instr::NoneOrInvalid => {
                    self.diag.log(format!("log: bypass device #{x}."));
                }
            }
        }
        self.isc(false);
        self.final_();
        self.diag.log(if self.diag.run_failed {
            "log: JTAG instructions failed."
        } else {
            "log: JTAG instructions finished successfully."
        });
    }
}
