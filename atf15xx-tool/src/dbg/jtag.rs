//! JTAG access: a small transport trait, an OpenOCD (Tcl RPC) implementation, and the
//! boundary-scan operations on top of it.
//!
//! Bit order everywhere: index 0 is the bit nearest TDO, i.e. the first shifted out (BSDL cell
//! numbering; OpenOCD's `drscan` value has it in its least significant bit).

use crate::dbg::part::Part;
use anyhow::{anyhow, bail, Context, Result};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A JTAG chain with one device of interest.
pub trait Jtag {
    /// Load an instruction.
    fn ir(&mut self, opcode: u64) -> Result<()>;
    /// Shift `bits` through the selected data register; returns the bits shifted out.
    fn dr(&mut self, bits: &[bool]) -> Result<Vec<bool>>;
}

/// How to reach OpenOCD.
#[derive(Clone, Debug)]
pub struct OpenOcdConfig {
    /// Connect to a running OpenOCD's Tcl server (`host:port`) instead of starting one.
    pub connect: Option<String>,
    /// Interface configuration for a spawned OpenOCD (the bench: an FT232H).
    pub interface: String,
    /// Adapter speed in kHz.
    pub speed: u32,
    /// Tcl port of a spawned OpenOCD.
    pub port: u16,
    /// TAP name (`cpld.tap` when spawned by us).
    pub tap: String,
    /// The OpenOCD executable.
    pub program: String,
    /// More configuration files for a spawned OpenOCD (`-f`, after the interface), e.g. a board file.
    pub files: Vec<String>,
    /// More commands for a spawned OpenOCD (`-c`, before `init`), e.g. `adapter serial X`.
    pub commands: Vec<String>,
}

impl Default for OpenOcdConfig {
    fn default() -> Self {
        OpenOcdConfig {
            connect: None,
            interface: "interface/ftdi/um232h.cfg".into(),
            speed: 400,
            port: 6666,
            tap: "cpld.tap".into(),
            program: "openocd".into(),
            files: vec![],
            commands: vec![],
        }
    }
}

/// OpenOCD through its Tcl RPC server (commands terminated by `\x1a`).
pub struct OpenOcd {
    stream: TcpStream,
    child: Option<Child>,
    tap: String,
    /// The log file of an OpenOCD we started (removed when it exits).
    log: Option<std::path::PathBuf>,
}

impl OpenOcd {
    /// Start OpenOCD for `part` (one TAP, the part's IR length and IDCODEs), or connect to one.
    pub fn open(cfg: &OpenOcdConfig, part: Option<&Part>) -> Result<OpenOcd> {
        if let Some(addr) = &cfg.connect {
            let stream = TcpStream::connect(addr).with_context(|| format!("connecting to OpenOCD at {addr}"))?;
            stream.set_nodelay(true)?;
            return Ok(OpenOcd { stream, child: None, tap: cfg.tap.clone(), log: None });
        }
        let mut args: Vec<String> = vec![];
        if !cfg.interface.is_empty() {
            args.extend(["-f".into(), cfg.interface.clone()]);
        }
        for f in &cfg.files {
            args.extend(["-f".into(), f.clone()]);
        }
        for c in &cfg.commands {
            args.extend(["-c".into(), c.clone()]);
        }
        args.extend(["-c".into(), format!("adapter speed {}", cfg.speed), "-c".into(), "transport select jtag".into()]);
        let tap = match part {
            Some(p) => {
                let ids: String = p.idcodes.iter().map(|id| format!(" -expected-id 0x{id:08x}")).collect();
                args.extend(["-c".into(), format!("jtag newtap cpld tap -irlen {}{ids}", p.ir_len)]);
                "cpld.tap".to_string()
            }
            // no TAP declared: OpenOCD auto-probes the chain (`auto0.tap`, ...)
            None => "auto0.tap".to_string(),
        };
        args.extend([
            "-c".into(),
            format!("tcl_port {}", cfg.port),
            "-c".into(),
            "gdb_port disabled".into(),
            "-c".into(),
            "telnet_port disabled".into(),
            "-c".into(),
            "init".into(),
        ]);
        // OpenOCD's log goes to a file: a pipe nobody reads fills up and stalls OpenOCD
        let log_path = std::env::temp_dir().join(format!("atf15xx-tool-openocd-{}.log", std::process::id()));
        let log = std::fs::File::create(&log_path)?;
        let mut child = Command::new(&cfg.program)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .with_context(|| format!("starting {}", cfg.program))?;
        let start = Instant::now();
        let stream = loop {
            if let Ok(s) = TcpStream::connect(("127.0.0.1", cfg.port)) {
                break s;
            }
            if let Some(status) = child.try_wait()? {
                let err = std::fs::read_to_string(&log_path).unwrap_or_default();
                let _ = std::fs::remove_file(&log_path);
                bail!("OpenOCD exited ({status}):\n{err}");
            }
            if start.elapsed() > Duration::from_secs(10) {
                let _ = child.kill();
                bail!("OpenOCD's Tcl server did not come up on port {}", cfg.port);
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        stream.set_nodelay(true)?;
        Ok(OpenOcd { stream, child: Some(child), tap, log: Some(log_path) })
    }

    /// Run one Tcl command; returns its result text.
    pub fn cmd(&mut self, c: &str) -> Result<String> {
        // one write per command: a separate terminator write would wait for the delayed ACK of
        // the first (Nagle), about 40 ms per command
        let mut msg = Vec::with_capacity(c.len() + 1);
        msg.extend_from_slice(c.as_bytes());
        msg.push(0x1a);
        self.stream.write_all(&msg)?;
        let mut buf = vec![];
        let mut chunk = [0u8; 65536];
        while buf.last() != Some(&0x1a) {
            let n = self.stream.read(&mut chunk)?;
            if n == 0 {
                bail!("OpenOCD closed the connection");
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        buf.pop();
        Ok(String::from_utf8_lossy(&buf).trim().to_string())
    }

    pub fn tap(&self) -> &str {
        &self.tap
    }

    /// TAPs OpenOCD knows (after auto-probing when none was declared), with their IDCODEs.
    pub fn chain(&mut self) -> Result<Vec<(String, u32)>> {
        let names = self.cmd("jtag names")?;
        let mut out = vec![];
        for name in names.split_whitespace() {
            let id = self.cmd(&format!("jtag cget {name} -idcode"))?;
            let id = parse_int(&id).with_context(|| format!("IDCODE of {name}: {id:?}"))?;
            out.push((name.to_string(), id as u32));
        }
        Ok(out)
    }
}

impl Jtag for OpenOcd {
    fn ir(&mut self, opcode: u64) -> Result<()> {
        let tap = self.tap.clone();
        let r = self.cmd(&format!("irscan {tap} 0x{opcode:x}"))?;
        if r.to_ascii_lowercase().contains("error") || r.contains("invalid") {
            bail!("irscan: {r}");
        }
        Ok(())
    }

    fn dr(&mut self, bits: &[bool]) -> Result<Vec<bool>> {
        let tap = self.tap.clone();
        let r = self.cmd(&format!("drscan {tap} {} 0x{}", bits.len(), to_hex(bits)))?;
        from_hex(&r, bits.len()).with_context(|| format!("drscan returned {r:?}"))
    }
}

impl Drop for OpenOcd {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = self.cmd("shutdown");
            let start = Instant::now();
            let mut exited = false;
            while start.elapsed() < Duration::from_secs(3) {
                if let Ok(Some(_)) = child.try_wait() {
                    exited = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if !exited {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        if let Some(log) = self.log.take() {
            let _ = std::fs::remove_file(log);
        }
    }
}

fn parse_int(s: &str) -> Result<u64> {
    let s = s.trim();
    Ok(match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16)?,
        None => s.parse()?,
    })
}

/// Bits (index 0 = least significant) as hex, most significant digit first.
pub fn to_hex(bits: &[bool]) -> String {
    let n = bits.len().div_ceil(4).max(1);
    (0..n)
        .rev()
        .map(|d| {
            let v = (0..4).fold(0u32, |v, k| v | (u32::from(*bits.get(4 * d + k).unwrap_or(&false)) << k));
            std::char::from_digit(v, 16).unwrap()
        })
        .collect()
}

/// The inverse of [`to_hex`] (an optional `0x` prefix is accepted).
pub fn from_hex(s: &str, len: usize) -> Result<Vec<bool>> {
    let s = s.trim();
    let s = s.strip_prefix("0x").unwrap_or(s);
    let digits: Vec<u32> = s.chars().map(|c| c.to_digit(16).ok_or_else(|| anyhow!("not hex: {c:?}"))).collect::<Result<_>>()?;
    let mut bits = vec![false; len];
    for (i, &d) in digits.iter().rev().enumerate() {
        for k in 0..4 {
            if let Some(b) = bits.get_mut(4 * i + k) {
                *b = d >> k & 1 == 1;
            }
        }
    }
    Ok(bits)
}

/// Boundary-scan operations of a part on a transport.
pub struct Scanner<'a, J: Jtag> {
    pub jtag: &'a mut J,
    pub part: &'a Part,
    extest: bool,
    /// The instruction known to be loaded (scanning DR does not change it), so that repeated
    /// captures need one scan each instead of two.
    loaded: Option<u64>,
}

impl<'a, J: Jtag> Scanner<'a, J> {
    pub fn new(jtag: &'a mut J, part: &'a Part) -> Self {
        Scanner { jtag, part, extest: false, loaded: None }
    }

    fn load(&mut self, name: &str) -> Result<()> {
        let op = self.part.opcode(name)?;
        if self.loaded != Some(op) {
            self.jtag.ir(op)?;
            self.loaded = Some(op);
        }
        Ok(())
    }

    pub fn idcode(&mut self) -> Result<u32> {
        self.load("IDCODE")?;
        let bits = self.jtag.dr(&[false; 32])?;
        Ok(bits.iter().enumerate().fold(0u32, |v, (i, &b)| v | (u32::from(b) << i)))
    }

    /// Check the IDCODE against the part.
    pub fn check_idcode(&mut self) -> Result<u32> {
        let id = self.idcode()?;
        if !self.part.idcodes.contains(&id) {
            bail!(
                "IDCODE 0x{id:08x} is not {} (expected {})",
                self.part.name,
                self.part.idcodes.iter().map(|i| format!("0x{i:08x}")).collect::<Vec<_>>().join(" or ")
            );
        }
        Ok(id)
    }

    /// Measure the boundary register: after SAMPLE a run of ones shifted in reappears after
    /// `length` bits (two paddings guard against captured ones next to the marker).
    pub fn measure_length(&mut self) -> Result<usize> {
        let mut found = vec![];
        for pad in [40usize, 43] {
            self.load("SAMPLE")?;
            let n = pad + 3 * self.part.length + 64;
            let input: Vec<bool> = (0..n).map(|i| i >= pad).collect();
            let out = self.jtag.dr(&input)?;
            let mut k = out.len();
            while k > 0 && out[k - 1] {
                k -= 1;
            }
            found.push(k - pad);
        }
        if found.iter().max().unwrap() - found.iter().min().unwrap() > 1 {
            bail!("inconsistent boundary length {found:?}");
        }
        Ok(*found.iter().max().unwrap())
    }

    /// SAMPLE: capture every boundary cell; the device keeps running.
    /// (Loading SAMPLE also leaves EXTEST: the pins are the logic's again.)
    pub fn sample(&mut self) -> Result<Vec<bool>> {
        self.load("SAMPLE")?;
        self.extest = false;
        self.jtag.dr(&vec![false; self.part.length])
    }

    /// Preload `bits` (SAMPLE/PRELOAD), then EXTEST: the pins take the values of the output and
    /// enable cells; returns what the cells captured (the input cells show the pins).
    /// The macrocell logic is disconnected from every pin until [`release`](Self::release).
    pub fn extest(&mut self, bits: &[bool]) -> Result<Vec<bool>> {
        assert_eq!(bits.len(), self.part.length);
        if !self.extest {
            self.load("SAMPLE")?;
            self.jtag.dr(bits)?;
        }
        self.load("EXTEST")?;
        self.extest = true;
        self.jtag.dr(bits)
    }

    /// Leave EXTEST: BYPASS gives the pins back to the device's logic.
    pub fn release(&mut self) -> Result<()> {
        self.load("BYPASS")?;
        self.extest = false;
        Ok(())
    }
}

impl<J: Jtag> Drop for Scanner<'_, J> {
    fn drop(&mut self) {
        if self.extest {
            let _ = self.release();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        for n in [1usize, 4, 5, 31, 32, 352] {
            let bits: Vec<bool> = (0..n).map(|i| (i * 7 + i / 3) % 5 < 2).collect();
            assert_eq!(from_hex(&to_hex(&bits), n).unwrap(), bits, "{n}");
        }
        assert_eq!(to_hex(&[true, false, false, false, true]), "11");
        assert!(from_hex("0x0150803f", 32).unwrap()[0]);
    }
}
