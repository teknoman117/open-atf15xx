//! The ATMISP-compatible commands of `atf15xx-tool` (§10): `svf`, `chain`, `jed`.

use atf15xx_tool::isp::chain::{self, Chain, Device};
use atf15xx_tool::isp::device::{self, Instr};
use atf15xx_tool::isp::diag::{Diagnostics, Error};
use atf15xx_tool::isp::generate::{generate, GenOptions};
use atf15xx_tool::isp::jedec;
use atf15xx_tool::isp::svf::{Rev, SvfOptions};
use atf15xx_tool::isp::timefmt;
use clap::{Args, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum Cmd {
    /// Convert a chain (and its JEDEC files) to SVF; writes JEDEC files for Load devices.
    Svf(SvfArgs),
    /// Inspect or create chain (.chn) files.
    #[command(subcommand)]
    Chain(ChainCmd),
    /// Inspect JEDEC files.
    #[command(subcommand)]
    Jed(JedCmd),
}

#[derive(Subcommand)]
pub enum ChainCmd {
    /// Parse a chain file and print its devices and diagnostics.
    Show {
        chain: PathBuf,
        #[command(flatten)]
        paths: PathArgs,
    },
    /// Write a chain file from --device options.
    Create {
        #[command(flatten)]
        devices: DeviceArgs,
        #[arg(short, long)]
        output: PathBuf,
        /// Calibration constant written to the header (1..1000)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=1000))]
        calibration: u32,
    },
}

#[derive(Subcommand)]
pub enum JedCmd {
    /// Print QF, family, checksums and the JTAG-enable fuse of a JEDEC file.
    Info { file: PathBuf },
}

#[derive(Args)]
pub struct DeviceArgs {
    /// Device: NAME:INSTRUCTION[:JEDPATH][:w=WIDTH][:noid]. INSTRUCTION is 0-9 or bypass, pv,
    /// erase, blank-check, verify, load, secure, pvs, verify-secure, read-ues.
    #[arg(short, long = "device", value_name = "SPEC")]
    device: Vec<String>,
}

#[derive(Args)]
pub struct PathArgs {
    /// Map Windows drive X: to DIR when resolving JEDEC paths (repeatable), e.g. C=~/.wine32/drive_c
    #[arg(long = "drive", value_name = "X=DIR", value_parser = parse_drive)]
    drive: Vec<(char, PathBuf)>,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum RevArg {
    C,
    D,
}

#[derive(Args)]
pub struct SvfArgs {
    /// Chain file (omit to build the chain from --device options)
    chain: Option<PathBuf>,
    #[command(flatten)]
    devices: DeviceArgs,
    /// SVF file to write
    #[arg(short, long)]
    output: PathBuf,
    /// SVF revision
    #[arg(long, value_enum, ignore_case = true, default_value = "d")]
    rev: RevArg,
    /// TCK period in µs (1..1000)
    #[arg(long, default_value_t = 1, allow_negative_numbers = true)]
    tck: i64,
    /// Replace an out-of-range --tck by 1 like ATMISP, instead of failing
    #[arg(long)]
    compat_tck: bool,
    /// Omit STATE RESET; lines
    #[arg(long)]
    no_state_reset: bool,
    /// Continue when 5 V and 3.3 V devices are mixed (ATMISP asks the user)
    #[arg(long)]
    allow_mixed_voltage: bool,
    /// Correct ATMISP's SVF defects (spec §9.4)
    #[arg(long)]
    strict: bool,
    /// Date text for the SVF and Load JEDEC headers (default: SOURCE_DATE_EPOCH in UTC, else local time)
    #[arg(long)]
    timestamp: Option<String>,
    /// Do not write JEDEC files for Load devices
    #[arg(long)]
    no_load_output: bool,
    #[command(flatten)]
    paths: PathArgs,
    /// Suppress the transcript
    #[arg(short, long)]
    quiet: bool,
}

fn parse_drive(s: &str) -> Result<(char, PathBuf), String> {
    let (d, dir) = s.split_once('=').ok_or("expected X=DIR")?;
    let c = d.trim_end_matches(':').chars().next().filter(|c| c.is_ascii_alphabetic()).ok_or("expected X=DIR")?;
    Ok((c.to_ascii_uppercase(), PathBuf::from(dir)))
}

/// Run one ATMISP-compatible command; errors carry the exit codes of §10.
pub fn run(cmd: Cmd) -> Result<(), Error> {
    match cmd {
        Cmd::Svf(a) => cmd_svf(a),
        Cmd::Chain(ChainCmd::Show { chain, paths }) => cmd_chain_show(&chain, &paths),
        Cmd::Chain(ChainCmd::Create { devices, output, calibration }) => cmd_chain_create(&devices, &output, calibration),
        Cmd::Jed(JedCmd::Info { file }) => cmd_jed_info(&file),
    }
}

fn usage(m: impl Into<String>) -> Error {
    Error::Usage(m.into())
}

/// §10.2
fn parse_device_spec(spec: &str) -> Result<Device, Error> {
    let raw: Vec<&str> = spec.split(':').collect();
    // re-join a Windows drive letter that was split off a path ("C" + "\x\y.jed")
    let mut parts: Vec<String> = Vec::new();
    let mut k = 0;
    while k < raw.len() {
        let p = raw[k];
        if k >= 2
            && p.len() == 1
            && p.chars().all(|c| c.is_ascii_alphabetic())
            && raw.get(k + 1).is_some_and(|n| n.starts_with('\\') || n.starts_with('/'))
        {
            parts.push(format!("{}:{}", p, raw[k + 1]));
            k += 2;
        } else {
            parts.push(p.to_string());
            k += 1;
        }
    }
    if parts.len() < 2 {
        return Err(usage(format!("invalid --device '{spec}' (expected NAME:INSTRUCTION[:JED])")));
    }
    let instr = Instr::parse_cli(&parts[1]).ok_or_else(|| usage(format!("unknown instruction '{}'", parts[1])))?;
    let mut d = Device { name: parts[0].clone(), ir_width: 10, idcode: true, instr, jed_path: String::new() };
    for p in &parts[2..] {
        if let Some(w) = p.strip_prefix("w=") {
            d.ir_width = w.parse().map_err(|_| usage(format!("invalid width '{w}'")))?;
        } else if p == "noid" {
            d.idcode = false;
        } else if d.jed_path.is_empty() {
            d.jed_path = p.clone();
        } else {
            return Err(usage(format!("unexpected '{p}' in --device '{spec}'")));
        }
    }
    if d.ir_width == 0 || d.ir_width > 99 {
        return Err(usage("IR width must be 1..99"));
    }
    Ok(d)
}

/// §10.3 path resolution.
fn resolve(path: &str, base: Option<&Path>, drives: &[(char, PathBuf)], must_exist: bool) -> Option<PathBuf> {
    if path.is_empty() {
        return None;
    }
    let windows_style = path.contains('\\') || (path.len() >= 2 && path.as_bytes()[1] == b':');
    // A Windows path is only meaningful verbatim on Windows; elsewhere it must go through --drive
    // (otherwise "C:\\x\\a.jed" would be taken as an odd relative file name).
    let mut cands: Vec<PathBuf> = if windows_style && !cfg!(windows) { vec![] } else { vec![PathBuf::from(path)] };
    let unix = path.replace('\\', "/");
    let b = unix.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        let letter = (b[0] as char).to_ascii_uppercase();
        for (c, dir) in drives {
            if *c == letter {
                cands.push(dir.join(unix[2..].trim_start_matches('/')));
            }
        }
    } else {
        cands.push(PathBuf::from(&unix));
        if let Some(base) = base {
            if !Path::new(&unix).is_absolute() {
                cands.push(base.join(&unix));
            }
        }
    }
    if must_exist {
        cands.into_iter().find(|p| p.is_file())
    } else {
        cands.into_iter().find(|p| p.parent().is_none_or(|d| d.as_os_str().is_empty() || d.is_dir()))
    }
}

fn print_diag(diag: &Diagnostics, quiet: bool) {
    for w in &diag.warnings {
        eprintln!("warning: {}", w.trim_start_matches("warning: ").replace('\n', " "));
    }
    if !quiet {
        for l in &diag.transcript {
            eprintln!("{l}");
        }
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), Error> {
    use std::io::Write;
    if path.as_os_str() == "-" {
        return std::io::stdout().write_all(data).map_err(|e| Error::Io(format!("cannot write stdout: {e}")));
    }
    // devices, FIFOs etc. cannot be replaced by rename: write them directly
    if path.exists() && !path.is_file() {
        return std::fs::write(path, data).map_err(|e| Error::Io(format!("cannot write {}: {e}", path.display())));
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, data).map_err(|e| Error::Io(format!("cannot write {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::Io(format!("cannot write {}: {e}", path.display()))
    })
}

fn timestamp(explicit: &Option<String>) -> String {
    if let Some(t) = explicit {
        return t.clone();
    }
    if let Some(t) = std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|s| s.trim().parse::<i64>().ok()).and_then(timefmt::from_epoch_utc) {
        return t;
    }
    timefmt::now_local()
}

fn devices_from_specs(specs: &[String], drives: &[(char, PathBuf)], check_jed: bool) -> Result<Vec<Device>, Error> {
    let devices = specs.iter().map(|s| parse_device_spec(s)).collect::<Result<Vec<_>, _>>()?;
    if devices.is_empty() || devices.len() > 50 {
        return Err(Error::Input(chain::E2.into()));
    }
    for (i, d) in devices.iter().enumerate() {
        if check_jed && d.instr.reads_jedec() && resolve(&d.jed_path, None, drives, true).is_none() {
            return Err(Error::Input(chain::e3(i + 1)));
        }
    }
    Ok(devices)
}

fn load_chain(a: &SvfArgs, diag: &mut Diagnostics) -> Result<(Chain, Option<PathBuf>), Error> {
    match (&a.chain, a.devices.device.is_empty()) {
        (Some(path), true) => {
            let data = std::fs::read(path).map_err(|e| Error::Input(format!("Cannot open Chain File {}: {e}", path.display())))?;
            let base = path.parent().map(Path::to_path_buf);
            let c = chain::parse(&data, diag, |_, jp| resolve(jp, base.as_deref(), &a.paths.drive, true).is_some())?;
            Ok((c, base))
        }
        (None, false) => Ok((Chain { calibration: 1, devices: devices_from_specs(&a.devices.device, &a.paths.drive, true)? }, None)),
        (Some(_), false) => Err(usage("give either a chain file or --device options, not both")),
        (None, true) => Err(usage("no chain file or --device given")),
    }
}

fn cmd_svf(a: SvfArgs) -> Result<(), Error> {
    let mut tck = a.tck;
    if !(1..=1000).contains(&tck) {
        if a.compat_tck {
            eprintln!("warning: Invalid Tck period. Default Tck value is set to 1. Valid Tck range: 1 - 1000");
            tck = 1;
        } else {
            return Err(Error::Input("Invalid Tck period. Valid Tck range: 1 - 1000".into()));
        }
    }
    let mut diag = Diagnostics::default();
    let (chain, base) = load_chain(&a, &mut diag)?;
    let opts = GenOptions {
        svf: SvfOptions {
            rev: match a.rev {
                RevArg::C => Rev::C,
                RevArg::D => Rev::D,
            },
            tck: tck as u32,
            state_reset: !a.no_state_reset,
            strict: a.strict,
        },
        allow_mixed_voltage: a.allow_mixed_voltage,
        ctime: timestamp(&a.timestamp),
        load_output: !a.no_load_output,
    };
    let drives = &a.paths.drive;
    let output = generate(
        &chain,
        |x, p| {
            let rp = resolve(p, base.as_deref(), drives, true).ok_or_else(|| Error::Input(chain::e3(x)))?;
            std::fs::read(&rp).map_err(|_| Error::Input(chain::e3(x)))
        },
        &opts,
        diag,
    )?;
    // resolve Load outputs before writing anything (§10.4: no partial output on errors)
    let mut jed_targets = Vec::new();
    for (x, p, bytes) in &output.jedec_outputs {
        let rp = resolve(p, base.as_deref(), drives, false)
            .ok_or_else(|| Error::Input(format!("cannot resolve JEDEC output path for device # {x}: '{p}'")))?;
        jed_targets.push((rp, bytes));
    }
    write_atomic(&a.output, &output.svf)?;
    for (rp, bytes) in jed_targets {
        write_atomic(&rp, bytes)?;
    }
    print_diag(&output.diag, a.quiet);
    Ok(())
}

fn cmd_chain_show(path: &Path, paths: &PathArgs) -> Result<(), Error> {
    let data = std::fs::read(path).map_err(|e| Error::Input(format!("Cannot open Chain File {}: {e}", path.display())))?;
    let base = path.parent().map(Path::to_path_buf);
    let mut diag = Diagnostics::default();
    let c = chain::parse(&data, &mut diag, |_, jp| resolve(jp, base.as_deref(), &paths.drive, true).is_some());
    for w in &diag.warnings {
        eprintln!("warning: {}", w.replace('\n', " "));
    }
    let c = c?;
    println!("calibration constant: {}", c.calibration);
    println!("{:>3}  {:<12} {:>5}  {:<6} {:<22} JEDEC", "#", "device", "width", "idcode", "instruction");
    for (i, d) in c.devices.iter().enumerate() {
        println!(
            "{:>3}  {:<12} {:>5}  {:<6} {:<22} {}",
            i + 1,
            d.name,
            d.ir_width,
            if d.idcode { "yes" } else { "no" },
            if d.instr == Instr::NoneOrInvalid { "(invalid)" } else { d.instr.name() },
            d.jed_path
        );
    }
    Ok(())
}

fn cmd_chain_create(devices: &DeviceArgs, output: &Path, calibration: u32) -> Result<(), Error> {
    let c = Chain { calibration, devices: devices_from_specs(&devices.device, &[], false)? };
    write_atomic(output, &chain::write(&c))
}

fn cmd_jed_info(path: &Path) -> Result<(), Error> {
    let data = std::fs::read(path).map_err(|e| Error::Input(format!("cannot read {}: {e}", path.display())))?;
    let j = jedec::parse(&data)?;
    println!("QF:                {}", j.qf.map_or("(none)".into(), |q| q.to_string()));
    println!("family:            {}", j.family().map_or("(unknown)", |f| f.label()));
    println!("default (F):       {}", j.default as char);
    println!("security (G):      {}", j.security as char);
    println!(
        "checksum:          stated {}  computed {:04X}",
        j.stated_checksum.map_or("(none)".into(), |c| format!("{c:04X}")),
        j.checksum()
    );
    if let Some(f) = j.family() {
        match f.jtag_fuse() {
            Some(jf) => println!("JTAG enable fuse:  F[{jf}] = {}", j.fuses[jf] as char),
            None => println!("JTAG enable fuse:  (none; always enabled)"),
        }
        let names: Vec<&str> = device::SUPPORTED.iter().copied().filter(|n| device::expected_family(n) == Some(f)).collect();
        println!("valid for:         {}", names.join(" "));
    }
    Ok(())
}

/// The SVF that programs and verifies one device with `jed` (the `program` command), and whether
/// the image keeps the JTAG port enabled. `device` is the ISP device name (`ATF1508AS`).
pub(crate) fn program_svf(device: &str, jed: &Path) -> Result<(Vec<u8>, bool), Error> {
    let data = std::fs::read(jed).map_err(|e| Error::Input(format!("cannot read {}: {e}", jed.display())))?;
    let j = jedec::parse(&data)?;
    let fam = j.family().ok_or_else(|| Error::Input(format!("{}: not an ATF15xx JEDEC file", jed.display())))?;
    let jtag_on = fam.jtag_fuse().is_none_or(|f| j.fuses[f] == b'1');
    let svf = one_device_svf(device, "pv", Some((jed, data)))?;
    Ok((svf, jtag_on))
}

/// The SVF that erases one device (the `erase` command).
pub(crate) fn erase_svf(device: &str) -> Result<Vec<u8>, Error> {
    one_device_svf(device, "erase", None)
}

/// SVF for a one-device chain: `instr` as in `--device NAME:INSTR`, with ATMISP's defects corrected
/// (`--strict`), Rev D timing, 1 µs TCK.
fn one_device_svf(device: &str, instr: &str, jed: Option<(&Path, Vec<u8>)>) -> Result<Vec<u8>, Error> {
    if !device::is_accepted_name(device) || !device::is_atf150(device) {
        return Err(usage(format!("cannot program '{device}'")));
    }
    let mut d = parse_device_spec(&format!("{device}:{instr}"))?;
    let data = match jed {
        Some((path, data)) => {
            d.jed_path = path.display().to_string();
            data
        }
        None => vec![],
    };
    let chain = Chain { calibration: 1, devices: vec![d] };
    let opts = GenOptions {
        svf: SvfOptions { rev: Rev::D, tck: 1, state_reset: true, strict: true },
        allow_mixed_voltage: false,
        ctime: timestamp(&None),
        load_output: false,
    };
    let out = generate(&chain, |_, _| Ok(data.clone()), &opts, Diagnostics::default())?;
    for w in &out.diag.warnings {
        eprintln!("warning: {}", w.replace('\n', " "));
    }
    Ok(out.svf)
}
