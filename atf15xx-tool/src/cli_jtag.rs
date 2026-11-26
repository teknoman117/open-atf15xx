//! The in-circuit debugging commands of `atf15xx-tool` (spec D1): JTAG boundary scan through OpenOCD.
use anyhow::{bail, Result};
use atf15xx_tool::dbg::design::{self, DesignMap};
use atf15xx_tool::dbg::part::{self, Part};
use atf15xx_tool::dbg::snapshot;
use atf15xx_tool::dbg::{OpenOcd, OpenOcdConfig, Scanner};
use clap::{Args, Subcommand};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How to reach the device (the boundary-scan commands; ignored by the others).
#[derive(Args)]
pub struct JtagArgs {
    /// Part, as `ATF1508AS-PLCC84`.
    #[arg(long, global = true, help_heading = "JTAG", default_value = "ATF1508AS-PLCC84")]
    part: String,
    /// Connect to a running OpenOCD's Tcl server (host:port) instead of starting OpenOCD.
    #[arg(long, global = true, help_heading = "JTAG")]
    connect: Option<String>,
    /// OpenOCD interface configuration when starting OpenOCD (the JTAG adapter), e.g.
    /// `interface/jlink.cfg`, `interface/cmsis-dap.cfg`; an empty value leaves it to --openocd-file.
    #[arg(long, global = true, help_heading = "JTAG", default_value = "interface/ftdi/um232h.cfg")]
    interface: String,
    /// More OpenOCD configuration files (`-f`, after --interface; repeatable).
    #[arg(long = "openocd-file", global = true, help_heading = "JTAG", value_name = "FILE")]
    openocd_files: Vec<String>,
    /// More OpenOCD commands (`-c`, before init; repeatable), e.g. `adapter serial FT1234` to pick
    /// one of several adapters, or `ftdi layout_init 0x0008 0x000b`.
    #[arg(long = "openocd-cmd", global = true, help_heading = "JTAG", value_name = "CMD", allow_hyphen_values = true)]
    openocd_cmds: Vec<String>,
    /// The OpenOCD executable.
    #[arg(long, global = true, help_heading = "JTAG", default_value = "openocd")]
    openocd: String,
    /// JTAG clock in kHz when starting OpenOCD.
    #[arg(long, global = true, help_heading = "JTAG", default_value_t = 400)]
    speed: u32,
    /// Tcl port when starting OpenOCD.
    #[arg(long, global = true, help_heading = "JTAG", default_value_t = 6666)]
    port: u16,
    /// TAP name (with --connect).
    #[arg(long, global = true, help_heading = "JTAG", default_value = "cpld.tap")]
    tap: String,
}

#[derive(Args, Clone)]
pub struct DesignArgs {
    /// The design's `<top>.map.json` (written by the fitter / flow), to name signals.
    #[arg(long)]
    map: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Program a JEDEC file into the device over JTAG (erase, program, verify) through OpenOCD.
    /// Warns when the image disables the JTAG port (its pins become I/O; reprogramming then
    /// needs 12 V on OE1).
    Program {
        /// The JEDEC file (from atf15xx-synth, atf15xx-fit or any fitter).
        jed: PathBuf,
        /// Also write the SVF that is played.
        #[arg(long)]
        svf: Option<PathBuf>,
    },
    /// Erase the device over JTAG through OpenOCD (every fuse to its erased state: no logic, JTAG
    /// port enabled).
    Erase {
        /// Also write the SVF that is played.
        #[arg(long)]
        svf: Option<PathBuf>,
    },
    /// Show the part's JTAG description from its BSDL (no hardware needed).
    Part {
        /// List every boundary cell.
        #[arg(long)]
        cells: bool,
    },
    /// Read the IDCODE and check it against the part.
    Idcode,
    /// Auto-probe the JTAG chain and identify each device by IDCODE.
    Probe,
    /// One SAMPLE snapshot (the device keeps running).
    Sample {
        #[command(flatten)]
        design: DesignArgs,
        /// Also print every macrocell (pad, oe, out).
        #[arg(long)]
        all: bool,
        /// JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Repeated SAMPLE, printing what changes.
    Watch {
        #[command(flatten)]
        design: DesignArgs,
        /// Milliseconds between captures (0 = as fast as the adapter allows).
        #[arg(long, default_value_t = 100)]
        interval: u64,
        /// Stop after this many captures (default: run until interrupted).
        #[arg(long)]
        count: Option<u64>,
    },
    /// Repeated SAMPLE into a VCD file.
    Capture {
        #[command(flatten)]
        design: DesignArgs,
        /// Output VCD file.
        #[arg(long, short)]
        out: PathBuf,
        /// Milliseconds between captures (0 = as fast as possible).
        #[arg(long, default_value_t = 0)]
        interval: u64,
        /// Number of captures.
        #[arg(long, default_value_t = 1000)]
        count: u64,
    },
    /// EXTEST: drive pins (MCn=V, pin number=V or design signal=V). The design is disconnected
    /// from ALL pins while this runs; every other pin keeps the state it had at the capture.
    Drive {
        #[command(flatten)]
        design: DesignArgs,
        /// Required: confirms that nothing else drives the pins being set.
        #[arg(long)]
        yes_drive_pins: bool,
        /// Also drive pins the design currently uses as inputs (their output enable is 0), which
        /// something else may be driving. Without it, only pins the design drives are overridden.
        #[arg(long)]
        allow_inputs: bool,
        /// Keep driving for this many milliseconds before giving the pins back.
        #[arg(long, default_value_t = 0)]
        hold: u64,
        /// Assignments, e.g. `MC16=1 12=0 LED=1`.
        #[arg(required = true)]
        assign: Vec<String>,
    },
    /// A scripted EXTEST session: one command per line from FILE (or `-` for stdin), each
    /// answered with one line; the device stays in EXTEST until the end. Commands: `drive
    /// SIG=V ...` (persistent; buses as `d=0x5A`), `float SIG ...`, `scan`, `print SIG ...`,
    /// `expect SIG=V ...`, `wait MS`. Exits with status 1 if an `expect` failed.
    Seq {
        #[command(flatten)]
        design: DesignArgs,
        /// Required: confirms that nothing else drives the pins being set.
        #[arg(long)]
        yes_drive_pins: bool,
        /// Also drive pins the design uses as inputs (something else must not drive them, e.g. a
        /// CPU holding its bus released).
        #[arg(long)]
        allow_inputs: bool,
        /// Keep the pins that are not driven as they were at the start, instead of following the
        /// design's own outputs (one scan late).
        #[arg(long)]
        hold_outputs: bool,
        /// Script (`-` = stdin, for a harness that sends one command at a time).
        #[arg(default_value = "-")]
        script: PathBuf,
    },
}

/// Run one boundary-scan command.
pub fn run(cli: &JtagArgs, cmd: Cmd) -> Result<()> {
    let cfg = OpenOcdConfig {
        connect: cli.connect.clone(),
        interface: cli.interface.clone(),
        speed: cli.speed,
        port: cli.port,
        tap: cli.tap.clone(),
        program: cli.openocd.clone(),
        files: cli.openocd_files.clone(),
        commands: cli.openocd_cmds.clone(),
    };
    if let Cmd::Probe = cmd {
        let mut ocd = OpenOcd::open(&cfg, None)?;
        for (tap, id) in ocd.chain()? {
            let parts = part::identify(id);
            println!("{tap}: IDCODE 0x{id:08x}  {}", if parts.is_empty() { "(no BSDL matches)".to_string() } else { parts.join(", ") });
        }
        return Ok(());
    }
    let part = Part::load(&cli.part)?;
    if let Cmd::Part { cells } = cmd {
        println!("{} ({}): BSDL {}", part.name, part.device, part.bsdl_file);
        println!("  IR length {}, boundary length {}, {} macrocells", part.ir_len, part.length, part.macrocells());
        println!("  IDCODE {}", part.idcodes.iter().map(|i| format!("0x{i:08x}")).collect::<Vec<_>>().join(" or "));
        if let Some(max) = part.max_tck_khz {
            println!("  maximum TCK {max} kHz; JTAG pins {:?}", part.jtag_pins);
        }
        println!("  instructions {}", part.opcodes.iter().map(|(k, v)| format!("{k}={v:0w$b}", w = part.ir_len)).collect::<Vec<_>>().join(" "));
        let no_pad = (1..=part.macrocells()).filter(|&m| part.mc_cell(m, part::CellKind::In).is_none()).count();
        println!(
            "  cells agree with the device table: {} on pins, {} dedicated inputs; {no_pad} macrocells without a pad input cell",
            part.cells.iter().filter(|c| c.pin.is_some()).count(),
            part.cells.iter().filter(|c| c.dedicated.is_some()).count()
        );
        for n in &part.notes {
            println!("  note: {n}");
        }
        if cells {
            for c in &part.cells {
                println!("  {:>3} {:<10} {}", c.bit, c.name(), c.pin.map_or("-".to_string(), |p| format!("pin {p}")));
            }
        }
        return Ok(());
    }
    if let Some(max) = part.max_tck_khz {
        if cli.speed > max && cli.connect.is_none() {
            bail!("--speed {} kHz is above the part's maximum TCK of {max} kHz (BSDL TAP_SCAN_CLOCK)", cli.speed);
        }
    }
    if let Cmd::Program { jed, svf } = cmd {
        return program(&cfg, &part, &jed, svf.as_deref());
    }
    if let Cmd::Erase { svf } = cmd {
        let device = part.name.split('-').next().unwrap_or(&part.name);
        let bytes = crate::cli_isp::erase_svf(device).map_err(|e| anyhow::anyhow!(e.to_string().replace('\n', " ")))?;
        let t0 = Instant::now();
        play_svf(&cfg, &part, &bytes, svf.as_deref(), "erasing")?;
        println!("erased {} in {:.1} s", part.name, t0.elapsed().as_secs_f64());
        return Ok(());
    }
    let mut ocd = OpenOcd::open(&cfg, Some(&part))?;
    let mut scan = Scanner::new(&mut ocd, &part);
    let id = scan.check_idcode()?;
    let load = |d: &DesignArgs| -> Result<Vec<design::Signal>> {
        Ok(match &d.map {
            Some(m) => design::signals(&part, &DesignMap::load(m)?),
            None => vec![],
        })
    };
    match cmd {
        Cmd::Idcode => {
            let len = scan.measure_length()?;
            println!("IDCODE 0x{id:08x} ({}), boundary register {len} cells", part.name);
            if len != part.length {
                bail!("boundary register is {len} cells, the BSDL says {}", part.length);
            }
        }
        Cmd::Sample { design, all, json } => {
            check_length(&mut scan)?;
            let signals = load(&design)?;
            let bits = scan.sample()?;
            if json {
                let v = serde_json::json!({
                    "part": part.name, "idcode": format!("0x{id:08x}"),
                    "dedicated": snapshot::dedicated(&part, &bits).iter().map(|(n, p, v)| serde_json::json!({"name": n, "pin": p, "value": v})).collect::<Vec<_>>(),
                    "signals": signals.iter().map(|s| serde_json::json!({"name": s.name, "how": s.how.tag(), "value": s.value(&bits)})).collect::<Vec<_>>(),
                    "macrocells": snapshot::macrocells(&part, &bits).iter().map(|m| serde_json::json!({"mc": m.mc, "pin": m.pin, "pad": m.pad, "oe": m.oe, "out": m.out})).collect::<Vec<_>>(),
                    "bits": atf15xx_tool::dbg::jtag::to_hex(&bits),
                });
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                print!("{}", snapshot::render(&part, &bits, &signals, all));
            }
        }
        Cmd::Watch { design, interval, count } => {
            check_length(&mut scan)?;
            let signals = load(&design)?;
            let cols = snapshot::trace_columns(&part, &signals);
            let start = Instant::now();
            let mut prev = scan.sample()?;
            print!("{}", snapshot::render(&part, &prev, &signals, false));
            let mut n = 1u64;
            while count.is_none_or(|c| n < c) {
                std::thread::sleep(Duration::from_millis(interval));
                let cur = scan.sample()?;
                for (name, old, new) in snapshot::changes(&cols, &prev, &cur) {
                    println!("{:>10.3} s  {name}: {} -> {}", start.elapsed().as_secs_f64(), u8::from(old), u8::from(new));
                }
                prev = cur;
                n += 1;
            }
        }
        Cmd::Capture { design, out, interval, count } => {
            check_length(&mut scan)?;
            let signals = load(&design)?;
            let cols = snapshot::trace_columns(&part, &signals);
            let file = std::io::BufWriter::new(std::fs::File::create(&out)?);
            let mut vcd = snapshot::VcdCapture::new(file, &part, cols)?;
            let start = Instant::now();
            let mut last = 0;
            for _ in 0..count {
                let bits = scan.sample()?;
                last = start.elapsed().as_micros() as u64;
                vcd.record(last, &bits)?;
                if interval > 0 {
                    std::thread::sleep(Duration::from_millis(interval));
                }
            }
            vcd.finish(last + 1)?;
            let secs = start.elapsed().as_secs_f64();
            println!("{count} captures in {secs:.2} s ({:.0} per second) -> {}", count as f64 / secs, out.display());
        }
        Cmd::Drive { design, yes_drive_pins, allow_inputs, hold, assign } => {
            if !yes_drive_pins {
                bail!("EXTEST drives the device's pins and disconnects its logic from all of them; \
                       make sure nothing else drives the pins you set, then pass --yes-drive-pins");
            }
            check_length(&mut scan)?;
            let signals = load(&design)?;
            let drives = snapshot::parse_drives(&part, &signals, &assign)?;
            let base = scan.sample()?;
            if !allow_inputs {
                for (mc, _) in &drives {
                    let oe = part.mc_cell(*mc, part::CellKind::Oe).map(|c| base[c.bit]).unwrap_or(false);
                    if !oe {
                        bail!("MC{mc}'s pin is an input of the current design (output enable 0): something else may \
                               be driving it; pass --allow-inputs if you know it is free");
                    }
                }
            }
            let bits = snapshot::drive_vector(&part, &base, &drives);
            let cap = scan.extest(&bits)?;
            if hold > 0 {
                std::thread::sleep(Duration::from_millis(hold));
            }
            let after = scan.extest(&bits)?;
            scan.release()?;
            for (mc, v) in &drives {
                let pin = part.mc_cell(*mc, part::CellKind::Out).and_then(|c| c.pin);
                let pad = part.mc_cell(*mc, part::CellKind::In).map(|c| after[c.bit]);
                println!("MC{mc} (pin {}): drove {}, pad reads {}", pin.unwrap_or(0), u8::from(*v), pad.map_or("-".into(), |p| u8::from(p).to_string()));
            }
            let _ = cap;
        }
        Cmd::Seq { design, yes_drive_pins, allow_inputs, hold_outputs, script } => {
            if !yes_drive_pins {
                bail!("a session drives the device's pins and disconnects its logic from all of them; \
                       make sure nothing else drives the pins you set, then pass --yes-drive-pins");
            }
            check_length(&mut scan)?;
            drop(scan);
            let signals = load(&design)?;
            let mut session = atf15xx_tool::dbg::session::Session::new(
                &mut ocd,
                &part,
                &signals,
                atf15xx_tool::dbg::session::Options { allow_inputs, hold_outputs },
            )?;
            let input: Box<dyn std::io::BufRead> = if script.as_os_str() == "-" {
                Box::new(std::io::BufReader::new(std::io::stdin()))
            } else {
                Box::new(std::io::BufReader::new(std::fs::File::open(&script)?))
            };
            use std::io::Write;
            for (n, line) in std::io::BufRead::lines(input).enumerate() {
                let line = line?;
                match session.exec(&line) {
                    Ok(Some(out)) => println!("{out}"),
                    Ok(None) => {}
                    Err(e) => println!("ERROR line {}: {e:#}", n + 1),
                }
                std::io::stdout().flush()?;
            }
            let failures = session.failures;
            session.finish()?;
            if failures > 0 {
                eprintln!("{failures} expectation(s) failed");
                std::process::exit(1);
            }
            return Ok(());
        }
        Cmd::Part { .. } | Cmd::Probe | Cmd::Program { .. } | Cmd::Erase { .. } => unreachable!(),
    }
    Ok(())
}

fn check_length<J: atf15xx_tool::dbg::Jtag>(scan: &mut Scanner<J>) -> Result<()> {
    let len = scan.measure_length()?;
    if len != scan.part.length {
        bail!("boundary register is {len} cells, {} has {}: wrong part?", scan.part.name, scan.part.length);
    }
    Ok(())
}

/// `program`: SVF from the JED (program + verify), the JTAG check, the IDCODE check, then
/// OpenOCD's `svf` player; its TDO checks are the verify.
fn program(cfg: &OpenOcdConfig, part: &Part, jed: &std::path::Path, keep: Option<&std::path::Path>) -> Result<()> {
    let device = part.name.split('-').next().unwrap_or(&part.name);
    let (svf, jtag_on) = crate::cli_isp::program_svf(device, jed).map_err(|e| anyhow::anyhow!(e.to_string().replace('\n', " ")))?;
    if !jtag_on {
        eprintln!(
            "warning: {} disables the JTAG port: its pins become I/O, and after this the device can only be \
             reprogrammed with 12 V on OE1 (an erase by the high-voltage path)",
            jed.display()
        );
    }
    let t0 = Instant::now();
    play_svf(cfg, part, &svf, keep, &format!("programming {} ({} JTAG port)", jed.display(), if jtag_on { "keeps the" } else { "DISABLES the" }))?;
    println!("programmed and verified {} in {:.1} s", jed.display(), t0.elapsed().as_secs_f64());
    Ok(())
}

/// Check the IDCODE, then play `svf` with OpenOCD's `svf` command (its TDO checks fail the call).
fn play_svf(cfg: &OpenOcdConfig, part: &Part, svf: &[u8], keep: Option<&std::path::Path>, what: &str) -> Result<()> {
    let path = match keep {
        Some(p) => p.to_path_buf(),
        None => std::env::temp_dir().join(format!("atf15xx-tool-{}.svf", std::process::id())),
    };
    std::fs::write(&path, svf)?;
    let mut ocd = OpenOcd::open(cfg, Some(part))?;
    let id = Scanner::new(&mut ocd, part).check_idcode()?;
    eprintln!("{}: IDCODE 0x{id:08x}; {what} ...", part.name);
    let p = path.display().to_string().replace('\\', "/");
    let code = ocd.cmd(&format!("catch {{svf -quiet {{{p}}}}} atf15xx_msg"))?;
    let msg = ocd.cmd("set atf15xx_msg")?;
    if keep.is_none() {
        let _ = std::fs::remove_file(&path);
    }
    if code.trim() != "0" {
        bail!("SVF failed: {msg}");
    }
    Ok(())
}
