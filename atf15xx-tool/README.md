# atf15xx-tool

One tool for ATF1502AS/1504AS/1508AS CPLDs: programming files and in-circuit debugging.

## Programming files (ATMISP-compatible)

A command-line reference implementation of the conversions of Microchip ATMISP 7.3 (module
`atf15xx_tool::isp`, formerly the `atmisp` crate): chain files and JEDEC to SVF, bit for bit as
ATMISP writes it (spec §1–§11, CLI §10; 145 oracle vectors in `tests/vectors`). These commands
never touch hardware.

```
atf15xx-tool svf <CHAIN.chn | -d NAME:INSTR[:JED] ...> -o OUT.svf [--rev C|D] [--tck us] [--strict] ...
atf15xx-tool chain show CHAIN.chn | chain create -d SPEC ... -o OUT.chn
atf15xx-tool jed info FILE.jed
```

e.g. `atf15xx-tool svf -d ATF1508AS:pv:design.jed -o design.svf` (program and verify).

## In-circuit debugging (JTAG boundary scan)

Module `atf15xx_tool::dbg` (feature `jtag`, on by default; formerly the `atf15xx-dbg` crate).
In-circuit debugging over JTAG boundary scan, through OpenOCD.
The boundary register of each part comes from the Atmel BSDL files that ship with the prjbureau
submodule (`external/prjbureau/bsdl`, Atmel's copyright: read at run time, not copied), checked
against this repository's architecture database. Specification:
[docs/architecture/debug/01-boundary-scan.md](../docs/architecture/debug/01-boundary-scan.md).

```
atf15xx-tool [--part ATF1508AS-PLCC84] [--speed kHz] [--connect host:port] <command>

  part [--cells]          the part's JTAG description and cell map (no hardware)
  idcode                  read and check the IDCODE, measure the boundary register
  probe                   auto-probe the JTAG chain, identify devices by IDCODE
  sample [--map M] [--all] [--json]
                          one SAMPLE snapshot: dedicated inputs, design signals (by Verilog
                          name with the fitter's <top>.map.json), every macrocell's pad/oe/out
  watch [--map M] [--interval ms] [--count N]
                          repeated SAMPLE, printing every change with a timestamp
  capture -o f.vcd [--map M] [--count N] [--interval ms]
                          repeated SAMPLE into a VCD file (GTKWave, Surfer, ...)
  drive --yes-drive-pins [--allow-inputs] [--hold ms] MC16=1 4=0 led=1
                          EXTEST: drive pins, read the pads back, give the pins back
  seq --yes-drive-pins [--allow-inputs] [--map M] [SCRIPT|-]
                          scripted EXTEST session (drive / float / scan / print / expect / wait,
                          one answer per line): stimulate a design's inputs, e.g. a bus, step by
                          step while its logic runs (spec D1.7)
```

By default OpenOCD is started with the bench's adapter (`interface/ftdi/um232h.cfg`, 400 kHz, one
TAP with the part's IR length and IDCODEs); `--connect host:6666` uses a running OpenOCD's Tcl server
instead (with `--tap` naming its TAP). The part tables (from Atmel's BSDL files in prjbureau) are
generated into `atf15xx-arch` at build time; nothing is read at run time.

What you see: every pad, and for every macrocell (with or without a pin) its output enable and its
output-mux value, i.e. a register's Q when the output taps it, while the device runs. Registers
cannot be written (the parts have no INTEST); `drive` only drives pins, and disconnects the logic
from all pins while it does.

Library: `Part` (cell map), `Jtag` (transport trait) with `OpenOcd`, `Scanner` (IDCODE, SAMPLE,
EXTEST, BYPASS), `design::signals` (naming), `snapshot` (views, changes, VCD), `session`
(scripted EXTEST sessions).

Tests: `cargo test -p atf15xx-tool` (all 24 BSDL files, the bench map, a simulated TAP). Bench checks:
`python3 tools/silicon/bench/exp_vcnt40.py` and `exp_dbg_seq_vcnt40.py` (with `designs/vcnt40.v`
programmed).
