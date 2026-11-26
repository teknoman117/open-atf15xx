//! `atf15xx-fit`: fit a CUPL `.tt2` netlist for any ATF1502AS/1504AS/1508AS device; the family
//! comes from the device. (`fit1502`, `fit1504`, `fit1508` keep the vendor fitters' command line.)
//!
//!   atf15xx-fit design.tt2 -d ATF1504AS-PLCC44 [-o out/design] [--preassign keep]
//!       [--no-jtag] [--security] [--output-fast all|PINS] [--set KEY=VALUE]...

use atf15xx_arch::part;
use atf15xx_fit::cli::{apply, write_outputs};
use atf15xx_fit::fit::{fit, Options};
use atf15xx_fit::tt2;
use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, Copy, ValueEnum)]
enum Preassign {
    /// Use the netlist's pin and node numbers where they fit, move the rest
    Try,
    /// Use them or fail
    Keep,
    /// Place everything freely
    Ignore,
}

/// Fit a CUPL `.tt2` netlist for an ATF1502AS, ATF1504AS or ATF1508AS (the family comes from the
/// device). Writes <OUTPUT>.jed, .fit, .pin and .map.json.
#[derive(Parser)]
#[command(
    name = "atf15xx-fit",
    version,
    after_help = "Every vendor strategy is also available as --set KEY=VALUE (e.g. --set cascade_logic=off, \
--set gck_itd=gck0, --set mc_power=on); the defaults are the vendor fitters'. fit1502/fit1504/fit1508 take the vendor's own command line."
)]
struct Cli {
    /// The netlist (.tt2)
    design: PathBuf,
    /// Device: ATF1504AS-PLCC44 (Microchip ordering codes such as ATF1504AS-10JU44 work too),
    /// fitter name P1504C44 or CUPL name f1504plcc44
    #[arg(short, long)]
    device: String,
    /// Output path without extension (default: the design's path without .tt2)
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// What to do with the pin and node numbers in the netlist
    #[arg(long, value_enum, default_value = "try")]
    preassign: Preassign,
    /// Disable the JTAG port (the pins become I/O; the device can no longer be reprogrammed over JTAG)
    #[arg(long)]
    no_jtag: bool,
    /// Set the security fuse (read-back disabled)
    #[arg(long)]
    security: bool,
    /// Fast slew rate: `all`, or a comma-separated list of output names
    #[arg(long, value_name = "all|NAMES")]
    output_fast: Option<String>,
    /// Open-collector outputs: `all`, or a comma-separated list of output names
    #[arg(long, value_name = "all|NAMES")]
    open_collector: Option<String>,
    /// Buried registers drive the output mux, so boundary-scan SAMPLE reads them
    #[arg(long)]
    observable_registers: bool,
    /// Any vendor strategy or option of ours, as KEY=VALUE (repeatable; later ones win)
    #[arg(long = "set", value_name = "KEY=VALUE")]
    sets: Vec<String>,
    /// JEDEC date text (default: now)
    #[arg(long)]
    created: Option<String>,
    /// No success message
    #[arg(short, long)]
    quiet: bool,
}

fn run(cli: Cli) -> Result<(), String> {
    let p = part::parse(&cli.device)?;
    let dev = p.device;
    let input = cli.design.display().to_string();
    let text = std::fs::read(&cli.design).map_err(|e| format!("{input}: {e}"))?;
    let pla = tt2::Pla::parse(&String::from_utf8_lossy(&text)).map_err(|e| format!("{input}: {e}"))?;
    let mut opts = Options::default();
    // the netlist's own properties first (as the vendor does), then the command line
    for (k, v) in pla.atmel_properties() {
        apply(&mut opts, &k, &v.split_whitespace().map(String::from).collect::<Vec<_>>());
    }
    let set = |opts: &mut Options, k: &str, v: &str| {
        if apply(opts, k, &v.split(|c| c == ' ').filter(|s| !s.is_empty()).map(String::from).collect::<Vec<_>>()) {
            Ok(())
        } else {
            Err(format!("unknown option {k} (see the vendor strategies in atf15xx-fit/README.md)"))
        }
    };
    set(
        &mut opts,
        "preassign",
        match cli.preassign {
            Preassign::Try => "try",
            Preassign::Keep => "keep",
            Preassign::Ignore => "ignore",
        },
    )?;
    if cli.no_jtag {
        set(&mut opts, "jtag", "off")?;
    }
    if cli.security {
        set(&mut opts, "security", "on")?;
    }
    for (k, v) in [("output_fast", &cli.output_fast), ("open_collector", &cli.open_collector)] {
        if let Some(v) = v {
            set(&mut opts, k, if v.eq_ignore_ascii_case("all") { "on" } else { v })?;
        }
    }
    if cli.observable_registers {
        set(&mut opts, "observable_registers", "on")?;
    }
    for s in &cli.sets {
        let (k, v) = s.split_once('=').ok_or_else(|| format!("--set {s}: KEY=VALUE expected"))?;
        set(&mut opts, k.trim(), v.trim())?;
    }
    opts.device = Some(dev.name.to_string());
    opts.created = cli.created.clone();
    let r = fit(dev.family, &pla, &opts).map_err(|e| e.to_string())?;
    let stem = match &cli.output {
        Some(o) => o.display().to_string(),
        None => input.strip_suffix(".tt2").or_else(|| input.strip_suffix(".TT2")).unwrap_or(&input).to_string(),
    };
    if let Some(parent) = std::path::Path::new(&stem).parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let cmd = std::env::args().collect::<Vec<_>>().join(" ");
    write_outputs(&r, &stem, opts.created.as_deref(), &cmd)?;
    if !cli.quiet {
        println!("{}: fits {} ({}); wrote {stem}.jed, .fit, .pin, .map.json", input, dev.name, p.chip);
    }
    Ok(())
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("atf15xx-fit: {e}");
        std::process::exit(1);
    }
}
