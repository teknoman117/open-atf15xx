# open-atf150x

An open toolchain for the **Microchip (Atmel) ATF1502AS, ATF1504AS and ATF1508AS** CPLDs, from
Verilog to a programmed chip, with no vendor software in the path:

```
design.v ──Yosys──► netlist ──atf15xx-synth──► design.jed ──atf15xx-tool──► design.svf ──OpenOCD──► chip
             (or the Yosys plugin: synth_atf15xx)    fitter: placement, PTs, UIM, timing
```

The vendor tools serve only as black-box oracles: our fitter reproduces their fuse maps fuse for
fuse on 92.8–99.7 % of thousands of synthetic test designs (and on all 95 regression vectors), our SVF writer reproduces the vendor programming software's
output byte for byte, and the architecture facts are checked on a real ATF1508AS. JEDEC files are
written in our own layout.

* **Start here:** [`docs/HANDOFF.md`](docs/HANDOFF.md) (state, whole flow, environment, next steps)
* The story of the project so far: [`docs/STORY.md`](docs/STORY.md)
* Specification: [`docs/architecture/`](docs/architecture/README.md) (fitter: `docs/architecture/fitter/`)
* Work logs (what was done, how it is known, what's open): [`docs/worklog/`](docs/worklog/README.md)
* Tools (oracles, silicon bench, equivalence harness): [`tools/`](tools/README.md)

## Crates

| crate | what |
|---|---|
| [`atf15xx-arch`](atf15xx-arch) | architecture and fuse-map database (oracle-derived), JED decoding |
| [`atf15xx-fit`](atf15xx-fit) | fitter library: `.tt2` front end, netlist IR, placement, PT allocation, UIM routing, foldbacks, XOR synthesis, JED reader and writer (own layout); binaries `atf15xx-fit` (family from the device) and `fit1502`/`fit1504`/`fit1508` (the vendor fitters' command line) |
| [`atf15xx-sim`](atf15xx-sim) | functional model, simulator, Verilog emitter and static timing analyser for any ATF15xx JED |
| [`atf15xx-synth`](atf15xx-synth) | Verilog → JED/SVF: Yosys JSON importer, constraints, strategies, `--fmax`; with feature `yosys`, the Yosys plugin `synth_atf15xx` (Rust behind a `cxx` bridge to RTLIL, built in the Yosys image, [`tools/yosys-plugin`](tools/yosys-plugin/README.md)) |
| [`atf15xx-tool`](atf15xx-tool) | JED and chain files → SVF, JEDEC output, and the in-circuit boundary-scan debugger |

Examples (Verilog, constraints, bench tooling): [`examples/`](examples/README.md): an SPI master
(ATF1504AS and a bench ATF1508AS variant driven over JTAG, with the Uno as SPI slave), the
SELFPROGRAMBOARD CPLD and the breakout-atf1508as board's 64-bit shift register.

## Build and test

```
git submodule update --init        # external/prjbureau: atf15xx-arch generates its tables from it
cargo build --release              # all workspace crates
cargo test --release --workspace   # unit tests, fitter conformance vectors, SVF vectors
docker/yosys/run.sh tools/yosys-plugin/build.sh   # the Yosys plugin (needs the pinned Yosys image)
```

External toolchains (Yosys, iverilog, Verilator, prjbureau) run in Docker (`docker/`).
The architecture tables (fuse map, boundary-scan parts) are generated at build time from the
prjbureau submodule and `atf15xx-arch/data/`; nothing is parsed at run time
(docs/plan/prjbureau-import.md).

## Usage

```
# Verilog to JED and SVF (Yosys in the docker/yosys image); --map auto (default) fits both the
# Yosys-gate and the word-level mapping and keeps the better; timing targets go in design.acf
# (fmax / clock clk 66.6 / tpd / tco / tsu / optimize area|speed, spec F8.7) or --fmax/--optimize
atf15xx-synth build design.v -d ATF1508AS-PLCC84 -c design.acf -o out [--fmax 66.6] [--optimize speed]

# the same inside Yosys
yosys -m tools/yosys-plugin/atf15xx.so -p 'read_verilog -sv design.v; synth_atf15xx -top design -part ATF1508AS-PLCC84 -o out'

# fit a CUPL .tt2 (the family comes from the device; fit1502/1504/1508 take the vendor's options)
atf15xx-fit design.tt2 -d ATF1508AS-PLCC84 --preassign keep -o out/design

# timing of any JED (ours or the vendor's)
atf15xx-sim timing out/design.jed -d P1508C84 --grade -10

# program the hardware: erase, program and verify over JTAG (OpenOCD with an FT232H by default;
# --interface for another adapter, --speed in kHz). Warns if the image disables the JTAG port.
atf15xx-tool --part ATF1508AS-PLCC84 --speed 4000 program out/design.jed
atf15xx-tool --part ATF1508AS-PLCC84 erase           # erase and blank-check

# or write the SVF for any SVF player
atf15xx-tool svf -d ATF1508AS:pv:out/design.jed -o design.svf
```

### Programming the hardware

`atf15xx-tool program` and `erase` need OpenOCD (0.12 or later) on the `PATH` and a JTAG adapter
wired to the chip. Like every JTAG command of the tool they start OpenOCD themselves (with
`--interface`, `--speed`, `--port`) and stop it when done; the default `--interface
interface/ftdi/um232h.cfg` is an FT232H (AD0 = TCK, AD1 = TDI, AD2 = TDO, AD3 = TMS). `program`
writes the program-and-verify SVF from the JED, checks the chip's IDCODE against `--part`, and plays
the SVF through OpenOCD; the SVF's read-back is the verify, so a mismatch fails the command. Other
adapters: `--interface interface/jlink.cfg` (or any OpenOCD interface file), `--openocd-file FILE`
for more configuration files, `--openocd-cmd CMD` for more OpenOCD commands (e.g. `--openocd-cmd
"adapter serial FT1234"` to pick one of several adapters; both repeatable), `--openocd PATH` for the
executable. `--connect host:port` uses an OpenOCD that is already running instead; `--svf FILE`
keeps the SVF. `erase` clears every fuse (no logic, JTAG port enabled) and blank-checks the chip. An
image built with JTAG off (`jtag off` in the constraints) is programmed after a warning: its JTAG
pins become I/O, and the chip can then only be reprogrammed after a 12 V erase on OE1.

### Chain files and JEDEC

```
# convert a chain file; map the Windows drive used in the chain's JEDEC paths
atf15xx-tool svf design.chn --drive C=$HOME/.wine32/drive_c -o design.svf
atf15xx-tool svf -d UNKNOWN:bypass:w=8 -d ATF1508AS:erase --rev C --tck 10 -o erase.svf
atf15xx-tool chain create -d ATF1502AS:pv:C:\\work\\a.jed -o a.chn
atf15xx-tool chain show a.chn
atf15xx-tool jed info counter.jed
```

`--strict` corrects the known defects of the vendor's SVF output, which are otherwise reproduced
for byte compatibility (spec §9.4).

## License

ISC (`LICENSE`), the licence of Yosys and nextpnr, for everything in this repository, including the
example designs ported from the author's MPL-2.0 CUPL sources (relicensed by the author). prjbureau
(submodule) is 0BSD.
