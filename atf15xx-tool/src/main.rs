//! `atf15xx-tool`: one command line for ATF15xx programming files (the ATMISP-compatible `svf`,
//! `chain`, `jed`; spec §10), programming (`program`, `erase`) and in-circuit debugging over JTAG
//! (`part`, `idcode`, `probe`, `sample`, `watch`, `capture`, `drive`, `seq`; spec D1).
mod cli_isp;
mod cli_jtag;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "atf15xx-tool", version, about = "Programming files (ATMISP-compatible SVF) and in-circuit debugging (JTAG boundary scan) for ATF15xx CPLDs")]
struct Cli {
    #[command(flatten)]
    jtag: cli_jtag::JtagArgs,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(flatten)]
    Isp(cli_isp::Cmd),
    #[command(flatten)]
    Jtag(cli_jtag::Cmd),
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Isp(cmd) => {
            if let Err(e) = cli_isp::run(cmd) {
                eprintln!("error: {}", e.to_string().replace('\n', " "));
                std::process::exit(e.exit_code());
            }
        }
        Cmd::Jtag(cmd) => {
            if let Err(e) = cli_jtag::run(&cli.jtag, cmd) {
                eprintln!("Error: {e:#}");
                std::process::exit(1);
            }
        }
    }
}
