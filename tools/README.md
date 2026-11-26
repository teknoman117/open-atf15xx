# Tools

None of this is needed to build or use the crates. It exists so that the specification in
`docs/architecture/` and the test data can be reproduced and extended, and so that you can check
`atf15xx-tool` against the real ATMISP with **your own JEDEC files**.

| directory | what |
|---|---|
| [`fuzzers/`](fuzzers/README.md) | **every analysis that runs the vendor programs** (ATMISP, the WinCUPL fitters, prjbureau's fuzzers), one workflow per derived fact, each with a `README.md` and a `CLAUDE.md`; the shared wrappers in `fuzzers/common/` |
| `sim/` | equivalence checking (`equiv.py`, `vcompare.py`, around `atf15xx-sim`), the Verilog → `.tt2` route |
| `flow/` | corpus runs and studies of the open Verilog flow |
| `silicon/` | bench programming and experiments on the real chip |
| `yosys-plugin/` | the in-Yosys pass probe |

## Test harness overview

| Layer | What it checks | Needs ATMISP? | How to run |
|---|---|---|---|
| Unit tests (`src/*.rs`) | bit/hex encoding, chain-file parser/writer, JEDEC parser, that the fuse-map constants equal the tables in `docs/architecture/05-fuse-maps.md`, the Load JEDEC writer (round trip through the reader, field lengths), `ctime` format | no | `cargo test` |
| Conformance vectors (`tests/vectors.rs`) | 145 recorded ATMISP runs (synthetic JEDEC files): SVF reproduced byte for byte, Load JEDEC fuse for fuse | no | `cargo test --test vectors` |
| Fuse-map fuzzing (`fuzzers/atmisp-fusemap`) | every fuse's place in ATMISP's SVF, all seven device names, against `atf15xx-tool` | to regenerate | `python3 tools/fuzzers/atmisp-fusemap/check.py OUT` |
| **Live oracle check** (`fuzzers/atmisp-svf/verify_jeds.py`) | any JEDEC files you supply, run through the real ATMISP 7.3 *and* `atf15xx-tool`, compared byte for byte (SVF) / fuse for fuse (Load JEDEC) | **yes** (wine) | see below |

## Verifying your own ("real") JEDEC files

### One-time setup

1. A 32-bit wine prefix with ATMISP 7.3 installed in `C:\ATMISP7` (the default installer path).
   The scripts use `WINEPREFIX` if set, otherwise `~/.wine32`:
   ```
   WINEPREFIX=~/.wine32 WINEARCH=win32 wine ATMISP7_setup.exe
   ```
   Start it once by hand (`WINEPREFIX=~/.wine32 wine 'C:\ATMISP7\ATMISP.exe'`) to check it runs.
2. The MinGW cross compiler for the GUI driver: `i686-w64-mingw32-gcc` (Gentoo:
   `crossdev -t i686-w64-mingw32`; Debian/Ubuntu: `apt install gcc-mingw-w64-i686`; Fedora:
   `dnf install mingw32-gcc`).
3. Python 3 (standard library only) and a Rust toolchain (the script builds
   `target/release/atf15xx-tool` with cargo if it is missing).

The first run of `verify_jeds.py` compiles `drive.c` and installs it as `C:\probe\drive.exe`;
use `--build-driver` to force a rebuild after editing `drive.c`.

### Running it

```
python3 tools/fuzzers/atmisp-svf/verify_jeds.py path/to/*.jed
python3 tools/fuzzers/atmisp-svf/verify_jeds.py --instr pv,verify,load --rev C --rev D design.jed
python3 tools/fuzzers/atmisp-svf/verify_jeds.py --device ATF1504ASV --instr pv design.jed
```

For each file and each requested instruction/revision it builds a one-device chain (device name from
the file's `QF` — ATF1502AS/1504AS/1508AS/1502BE/1504BE/1508BE — unless `--device` is given), lets
ATMISP generate the SVF through its GUI, runs `atf15xx-tool svf` on the same chain file with
ATMISP's timestamp, and compares the SVF byte for byte (for `load`, also the JEDEC file each
program writes). Each ATMISP run takes 10–15 s and briefly shows a wine "atmisp" virtual-desktop
window.

Options: `--instr` (`pv`, `verify`, `pvs`, `erase`, `blank-check`, `load`, `secure`,
`verify-secure`, `read-ues`, `bypass`; default `pv`), `--device`, `--rev C|D` (repeatable),
`--tck N`, `--no-state-reset`, `--work DIR` (default `./verify-work`), `--force` (re-run ATMISP
even if a reference result is cached in `--work`), `--cli PATH`.

### Reading the report

```
designs/counter.jed  pv  D  MATCH     100840 bytes
designs/huge.jed     pv  D  DIVERGE   atmisp rejects this input on purpose (spec §9.2) …
```

| Verdict | Meaning |
|---|---|
| `MATCH` | byte-identical SVF (and, for `load`, a JEDEC file with identical fuses, security fuse and checksum) |
| `OK-REJECT` | ATMISP wrote no SVF (crash or refusal) and `atf15xx-tool` also refused |
| `DIVERGE` | `atf15xx-tool` refuses an input that ATMISP accepts but handles with undefined or meaningless output — QF/device mismatch, JEDEC fields ≥ 200 bytes, fuse digits other than 0/1 (spec §3, §9.2). Inspect the case; it is not necessarily a bug |
| `MISMATCH` | a real difference: the first differing line is printed; both SVFs are in `--work/<case>/` (`out.svf` from ATMISP, `cli.svf` from atmisp, plus `driver.log` with ATMISP's dialogs/transcript) |
| `SKIP` | the device could not be derived from the file's `QF`; pass `--device` |

The exit status is 0 only if every case is `MATCH` or `OK-REJECT`.

A `MISMATCH` case can be turned into a permanent regression vector with
`python3 tools/fuzzers/atmisp-svf/assemble.py <work-dir> <tmp>` (it empties `<tmp>` first) and copying
the case directory into `atf15xx-tool/tests/vectors/<group>/` — but only if you are allowed to redistribute the JEDEC file (the committed
vectors deliberately contain synthetic data only).

## Architecture database

`atf15xx-arch` generates its tables at build time (prjbureau's `database.json` and BSDL files,
`atf15xx-arch/data/devices.json`, `data/timing.json`; docs/plan/prjbureau-import.md). Scripts query
them through `atf15xx-sim device D`, `atf15xx-sim fuses F`, `atf15xx-sim mc X.jed -d D --mc N`
(`--set MC16.CasMux=1 -o OUT.jed` edits bits) and `atf15xx-sim stats X.jed -d D`.

The fitter command lines: `fit1502|fit1504|fit1508 design.tt2 -device P1504C44 [-preassign keep|try|ignore]
[-strategy KEY VALUE]... [-created TEXT]` (see `atf15xx-fit/README.md`).

## `sim/` — JED simulator and equivalence checking (see `docs/worklog/04-simulator.md`)

Checks that a fitted `.jed` implements its source design (`.tt2`, Verilog, or another JED),
formally (Yosys + ABC PDR) and by simulation (Icarus Verilog). Method and model:
`docs/architecture/sim/`; step-by-step guide: `docs/architecture/sim/05-harness.md`. Everything
runs in the `open-atf150x-sim` image (`docker/sim/Dockerfile`: tools image + iverilog, Verilator
5.054 built from source, z3, boolector) via `tools/sim/run.sh CMD...`, which builds the images on first use. Every tool
takes `--help`.

| tool | purpose | usage |
|---|---|---|
| `atf15xx-sim` (crate `atf15xx-sim`) | fuse map → model: simulate, compare two JEDs, emit Verilog, list equations | `atf15xx-sim run X.jed -d P1504C44 [--stim S.csv --print] [--vcd X.vcd]` · `compare A.jed B.jed -d D` · `verilog X.jed -d D -o x.v` · `equations X.jed -d D` |
| `sim/equiv.py` | **the check**: JED vs source, formal + simulation, PASS/FAIL with counterexample VCD | `equiv.py X.jed --device D --tt2 X.tt2 --fit X.fit` · `--verilog X.v --top T (--fit X.fit \| --pin P=N \| --pin-file F)` · `--gold-jed Y.jed` · `[--json R.json] [--assume EXPR] [--init free --reset pN=V:K] [--power-up 0\|1\|fuse]`; stable entry point, see `docs/architecture/sim/05-harness.md` |
| `sim/vcompare.py` | time-bounded Verilator comparison: one testbench with source and JED, random + clock-burst + reset stimulus, register correspondence (`.map.json` / `.fit` / signatures) and state injection, VCD window at the first mismatch; also `equiv.py --sim-engine verilator` | `vcompare.py X.jed --device D --verilog X.v --top T --fit X.fit [--budget 60] [--cycles N] [--inject-every N]` |
| `sim/tests/run_tests.sh` | time-bounded test suite (64-bit counter, user `blink.v`, broken variants, vendor fits, tri-state, loops) | `tools/sim/tests/run_tests.sh [BUDGET]` |
| `sim/tests/make_broken.py` | make a deep-bug JED: drop one literal from one product term of a macrocell | `make_broken.py IN.jed --device D --mc N -o OUT.jed` |
| `sim/verilog_flow.py` | Verilog → `.tt2` (`synth_atf15xx.ys` + `yosys2tt2.py`) → our `fit150x` → `equiv.py` against the Verilog | `verilog_flow.py SRC.v --top T --device D [--pin clk=43] [equiv options]` |
| `sim/check_design.py` | vendor JED and our `fit150x` re-fit of a CUPL design, both through `equiv.py` | `check_design.py DIR/NAME [--verilog X.v --top T] [equiv options]` |
| `sim/run_designs.sh` | the user's real designs (copies them from `~/Projects/pld/cpld` / `.scratch/user2` into `.scratch/sim/real/`) | `tools/sim/run_designs.sh [LABEL...]` |
| `sim/tt2v.py` | `.tt2` → gold Verilog with the split pin interface | `tt2v.py X.tt2 --fit X.fit -o gold.v` |
| `sim/vsplit.py` | Verilog source → gold Verilog with the split pin interface (tri-states split) | `vsplit.py --top T --fit X.fit -o gold.v X.v` |
| `sim/cosim.py` | self-check: emitted Verilog (iverilog) vs `atf15xx-sim run` | `cosim.py --corpus` · `cosim.py --device D X.jed` |
| `sim/mutate.py` | self-check: single-fuse mutants of a passing JED must be caught | `mutate.py X.jed --device D --tt2 X.tt2 --fit X.fit -n 30` |
| `sim/simlib.py` | library: binary paths, stimulus generation, device lookup | `from simlib import gen_stim, sim_run` |

Outputs go to `.scratch/sim/<tool>/...` (`equiv.py`: `.scratch/sim/equiv/<jed name>/` with
`gold.v`, `gate.v`, `miter.v`, `cex.vcd`, `cex.txt`, `sim.vcd`, logs).

## `flow/` — open Verilog → `.jed`/`.svf` flow (see `docs/worklog/05-verilog-flow.md`)

The flow itself is the `atf15xx-synth` crate (`atf15xx-synth/README.md`; spec
`docs/architecture/fitter/07-yosys-flow.md` §F7.4–F7.5). Image with pinned Yosys v0.71:
`docker build -t open-atf150x-yosys docker/yosys/`, run with `docker/yosys/run.sh`.

| tool | what it does | run |
|---|---|---|
| `flow/corpus.py` | every `atf15xx-synth/tests/designs/*.v` on 1502/1504/1508 (or `NAME.parts`), plus `--extra FILE.v=PART,...`: `atf15xx-synth build`, then `sim/equiv.py` (formal + simulation) against the source Verilog; table in `<out>/results.md` | pinned Yosys: `docker/yosys/run.sh python3 tools/flow/corpus.py --stage build --out D`, then `tools/sim/run.sh python3 tools/flow/corpus.py --stage check --out D`; or `tools/sim/run.sh python3 tools/flow/corpus.py` (both, Yosys 0.33) |
| `flow/experiment.py` | parameter study (Yosys `--map`, `--max-products`, toggle recovery): fit / macrocells per design | `tools/sim/run.sh python3 tools/flow/experiment.py` |

## Silicon (`tools/silicon/`, see its README)

| tool | purpose | usage |
|---|---|---|
| `silicon/jtag_guard.py` | refuse images that would disable JTAG (JED fuse + SVF special-register write) | `jtag_guard.py --jed X.jed --svf X.svf` |
| `silicon/program.sh` | guard, then program with OpenOCD + FT232H | `tools/silicon/program.sh X.svf X.jed` |
| `atf15xx-tool` (Rust) | in-circuit debugger: IDCODE, chain, SAMPLE by Verilog name, watch, VCD capture, guarded EXTEST (spec D1) | `cargo run -p atf15xx-tool -- sample --map out/top.map.json` |
| `silicon/bench/exp_vcnt40.py` | bench check: programs `designs/vcnt40.v`, counts across the 40-bit wrap, reads every register with `atf15xx-tool sample` | `python3 tools/silicon/bench/exp_vcnt40.py` |
| `flow/placement_headroom.py` | per-block UIM inputs of a fitted JED and how far a local search over buried cells reduces them (F9.2) | `python3 tools/flow/placement_headroom.py X.jed P1508C84 8` |
| `yosys-plugin/` | feasibility probe for an in-Yosys ATF15xx pass (F9.3 C) | see its README |
