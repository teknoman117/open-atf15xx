# 05 — Harness guide

## Stable entry point: check a .jed against Verilog

```sh
tools/sim/run.sh python3 tools/sim/equiv.py DESIGN.jed --device P1504C44 \
    --verilog top.v [more.v ...] --top TOP \
    (--fit DESIGN.fit | --pin PORT=N ... | --pin-file pins.txt) \
    [--json result.json] [--work DIR] [--assume EXPR] [--init zero|free --reset pN=V:K] [--power-up 0|1|fuse]
```

This interface is kept stable for other workstreams (the open Verilog → JED → SVF flow uses it).

* Inputs: the JED, the device (fitter name, e.g. `P1504C44`, `P1508C84`), the Verilog sources and
  top module, and a pin map. The pin map can be the fitter report's
  `NAME is placed at pin N` / `NAME assigned to pin N` lines, `--pin PORT=N` options, or a file of
  `PORT=N` / `PORT N` lines (`#` comments). A vector bit is `name[i]` or `namei`. Every output port bit
  must have a pin; unused inputs may be left out.
* Exit status: **0** PASS, **1** FAIL (a difference was found), **2** ERROR (a tool failed, or bad usage).
* Last stdout line: `RESULT: PASS|FAIL|ERROR`. With `--json FILE`:
  `{"result", "jed", "device", "pins_compared", "source_drives", "options",
  "formal": {"status": PROVEN|BOUNDED|FAIL|ERROR|N/A, "message", "vcd", "counterexample",
  "power_up_explains", "power_up_diagnosis"}, "sim": {"status": PASS|FAIL|ERROR, "message", "vcd"}}`.
  Paths are relative to the repository root.
* Default semantics: source registers without an initial value start at 0, the JED's registers at
  the modelled power-up (`--power-up`, default 0). Both engines run (`--no-formal` / `--no-sim`
  skip one). A formal FAIL is followed by a **power-up diagnosis**, which tells you whether *any*
  initial state of either side could explain the difference (see "Power-up" below).
* **Time bounds:** `--budget S` (total wall clock, default 600 s) bounds everything.
  `--sim-engine verilator` (recommended for counters and other deep state) adds the Verilator
  engine: correspondence-based state injection, cycle budget `--cycles`, Verilator compile budget
  `--compile-budget`. The last two lines are `GUARANTEE: PROVEN … | BOUNDED n [+ SIMULATED …] |
  SIMULATED n cycles + k injections | FAIL | NONE` and `RESULT:` (06-verilator.md §T4).
* Standalone Verilator comparison with the same source and pin options:
  `tools/sim/run.sh python3 tools/sim/vcompare.py X.jed --device D --verilog X.v --top T --fit X.fit [--budget 60]`
  (exit 0 no difference, 1 FAIL, 2 error/timeout).
* For a Verilog → `.tt2` → fitter flow on our own fitter in one command:
  `tools/sim/verilog_flow.py SRC.v --top T --device D [--pin clk=43]` (synthesis via
  `tools/sim/synth_atf15xx.ys` + `yosys2tt2.py`, then `fit150x`, then the check above).

## Power-up

The device powers its registers up to a fixed state that no document gives (§F2.8 rule 15, [I]).
The checker separates the questions:

| option | what it sets |
|---|---|
| `--power-up 0` (default) / `1` | every JED register starts at 0 / 1 |
| `--power-up fuse` | each JED register starts at its `Invert` fuse (prjbureau calls this fuse `reset`: "power-on reset value", GND = 0 / VCC = 1) [P] |
| `--init zero` (default) | source registers without an `initial` value start at 0 (`.tt2`: all registers 0) |
| `--init free` + `--reset pN=V:K` | both sides start anywhere; equality is required after a K-step reset sequence on pin N; this is the right check for a design that has a reset and does not rely on its power-up state |

After a formal FAIL, `equiv.py` replays the counterexample's inputs, extended to 60 steps, with
*both* sides' registers free, and asks SAT whether any pair of initial states makes every pin
equal. The verdict is printed (`power-up:`):

* "the logic differs (not a power-up question)": no change of power-up semantics or initial
  values can fix it;
* "the difference may be only the power-up state": try `--power-up fuse`, give the source an
  `initial` value, or use `--init free --reset …`.

Everything runs in the `open-atf150x-sim` Docker image through `tools/sim/run.sh`, which builds
the images on first use and mounts the repository at `/work` (only the repository is visible
inside; copy external designs into it, e.g. under `.scratch/`). The Rust binary is built in
release mode on demand (`CARGO_TARGET_DIR=.scratch/target-docker`); set `SIM_NO_BUILD=1` to
skip the rebuild check.

```sh
docker build -t open-atf150x-tools docker/ && docker build -t open-atf150x-sim docker/sim/   # once (run.sh does it too)
tools/sim/run.sh cargo build --release -p atf15xx-sim
```

## Check a fitted JED against its source

```sh
# CUPL design: .tt2 is the source, the .fit gives the pin placement
tools/sim/run.sh python3 tools/sim/equiv.py X.jed --device P1504C44 --tt2 X.tt2 --fit X.fit

# Verilog design (e.g. through tools/sim/synth_atf15xx.ys + yosys2tt2.py + a fitter)
tools/sim/run.sh python3 tools/sim/equiv.py X.jed --device P1504C44 --verilog X.v --top top --fit X.fit
#   (or --pin clk=43 --pin 'q[0]=12' ... instead of --fit)

# two JEDs for the same pins (vendor vs ours)
tools/sim/run.sh python3 tools/sim/equiv.py ours.jed --device P1504C44 --gold-jed vendor.jed
```

Output:

```
equiv X.jed (P1504C44): 32 pins compared; source drives 12, JED drives 32; work .scratch/sim/equiv/X
  formal: PROVEN  PDR proved the miter output is never 1
  sim:    PASS  4000 steps, 32 pins compared
RESULT: PASS
```

On a difference:

```
  formal: FAIL  counterexample of 1 steps  VCD .scratch/sim/equiv/counter/cex.vcd
    step 0: inputs p1=0 p4=1 p5=0 p8=1 p9=1 p11=1 p12=1 ...
        DIFF pin 6 source 0 jed 1
  sim:    FAIL  step 585 pin 6: source=0 jed=1 (stimulus .scratch/sim/equiv/counter/sim_stim.csv)  VCD .scratch/sim/equiv/counter/sim.vcd
RESULT: FAIL
```

`cex.vcd` has every public net of the miter (`g.*` = source, `t.*` = JED, `pN_in`); one VCD time
step = one simulator step. `cex.txt` is the per-step summary. `sim.vcd` is the iverilog run
(`g`, `u` instances). To look inside the JED at a pin, `atf15xx-sim equations X.jed -d D` lists
every macrocell's product terms in the emitter's names (`pad_m`, `fb_m`, `fold_m`, `pN_in`).

Useful options: `--assume EXPR` (Python syntax over `pN`: `'not (p41 and p44)'`,
`'p5 == 1'`), `--init free --reset p1=1:2`, `--ignore-pin N`, `--no-formal` / `--no-sim`,
`--timeout S` (PDR/BMC), `--depth N` (bounded depth), `--steps N --seed S` (simulation),
`--preset-wins --power-up 1` (model options). Exit status 0 PASS, 1 FAIL, 2 ERROR.

## Check a CUPL design with both fitters

```sh
tools/sim/run.sh python3 tools/sim/check_design.py DIR/NAME [equiv options]
tools/sim/run_designs.sh [LABEL ...]       # the user's real designs (copies them into .scratch/sim/real/)
```

`check_design.py` checks `DIR/NAME.jed` (vendor) and re-fits `NAME.tt2` with our `fit150x` using
the command line recorded in the vendor `.fit`, then checks that JED as well.

## Simulate a JED

```sh
tools/sim/run.sh .scratch/target-docker/release/atf15xx-sim run X.jed -d P1504C44 --cycles 200 --vcd x.vcd
tools/sim/run.sh .scratch/target-docker/release/atf15xx-sim run X.jed -d P1504C44 --stim stim.csv --print
tools/sim/run.sh .scratch/target-docker/release/atf15xx-sim compare a.jed b.jed -d P1504C44 --cycles 5000 --vcd diff.vcd
tools/sim/run.sh .scratch/target-docker/release/atf15xx-sim verilog X.jed -d P1504C44 -o x.v --pins-wrapper
tools/sim/run.sh .scratch/target-docker/release/atf15xx-sim equations X.jed -d P1504C44
```

Stimulus CSV: header = pin numbers, one row of 0/1 per step; pins not listed stay 0.
`--print` writes one row per step: every pin as `0`/`1` (driven), `Z` (not driven) or `-` (input
only), then `unstable` (1 = the step did not settle), and with `--print-q` the register bits.

## Self-checks (run after changing the model or emitter)

```sh
tools/sim/tests/run_tests.sh [60]        # time-bounded suite: 64-bit counter, blink.v, broken variants, vendor fits, tri-state, loops
tools/sim/run.sh python3 tools/sim/cosim.py --engine verilator --corpus   # Verilator vs the Rust simulator
tools/sim/run.sh python3 tools/sim/cosim.py --corpus                        # emitted Verilog vs simulator
tools/sim/run.sh python3 tools/sim/mutate.py X.jed --device D --tt2 X.tt2 --fit X.fit -n 30
docker/run.sh cargo test -p atf15xx-sim
```

 05 — Harness guide

## Stable entry point: check a .jed against Verilog

```sh
tools/sim/run.sh python3 tools/sim/equiv.py DESIGN.jed --device P1504C44 \
    --verilog top.v [more.v ...] --top TOP \
    (--fit DESIGN.fit | --pin PORT=N ... | --pin-file pins.txt) \
    [--json result.json] [--work DIR] [--assume EXPR] [--init zero|free --reset pN=V:K] [--power-up 0|1|fuse]
```

This interface is kept stable for other workstreams (the open Verilog → JED → SVF flow uses it).

* Inputs: the JED, the device (fitter name, e.g. `P1504C44`, `P1508C84`), the Verilog sources and
  top module, and a pin map. The pin map can be the fitter report's
  `NAME is placed at pin N` / `NAME assigned to pin N` lines, `--pin PORT=N` options, or a file of
  `PORT=N` / `PORT N` lines (`#` comments). A vector bit is `name[i]` or `namei`. Every output port bit
  must have a pin; unused inputs may be left out.
* Exit status: **0** PASS, **1** FAIL (a difference was found), **2** ERROR (a tool failed, or bad usage).
* Last stdout line: `RESULT: PASS|FAIL|ERROR`. With `--json FILE`:
  `{"result", "jed", "device", "pins_compared", "source_drives", "options",
  "formal": {"status": PROVEN|BOUNDED|FAIL|ERROR|N/A, "message", "vcd", "counterexample",
  "power_up_explains", "power_up_diagnosis"}, "sim": {"status": PASS|FAIL|ERROR, "message", "vcd"}}`.
  Paths are relative to the repository root.
* Default semantics: source registers without an initial value start at 0, the JED's registers at
  the modelled power-up (`--power-up`, default 0). Both engines run (`--no-formal` / `--no-sim`
  skip one). A formal FAIL is followed by a **power-up diagnosis**, which tells you whether *any*
  initial state of either side could explain the difference (see "Power-up" below).
* **Time bounds:** `--budget S` (total wall clock, default 600 s) bounds everything.
  `--sim-engine verilator` (recommended for counters and other deep state) adds the Verilator
  engine: correspondence-based state injection, cycle budget `--cycles`, Verilator compile budget
  `--compile-budget`. The last two lines are `GUARANTEE: PROVEN … | BOUNDED n [+ SIMULATED …] |
  SIMULATED n cycles + k injections | FAIL | NONE` and `RESULT:` (06-verilator.md §T4).
* Standalone Verilator comparison with the same source and pin options:
  `tools/sim/run.sh python3 tools/sim/vcompare.py X.jed --device D --verilog X.v --top T --fit X.fit [--budget 60]`
  (exit 0 no difference, 1 FAIL, 2 error/timeout).
* For a Verilog → `.tt2` → fitter flow on our own fitter in one command:
  `tools/sim/verilog_flow.py SRC.v --top T --device D [--pin clk=43]` (synthesis via
  `tools/sim/synth_atf15xx.ys` + `yosys2tt2.py`, then `fit150x`, then the check above).

## Power-up

The device powers its registers up to a fixed state that no document gives (§F2.8 rule 15, [I]).
The checker separates the questions:

| option | what it sets |
|---|---|
| `--power-up 0` (default) / `1` | every JED register starts at 0 / 1 |
| `--power-up fuse` | each JED register starts at its `Invert` fuse (prjbureau calls this fuse `reset`: "power-on reset value", GND = 0 / VCC = 1) [P] |
| `--init zero` (default) | source registers without an `initial` value start at 0 (`.tt2`: all registers 0) |
| `--init free` + `--reset pN=V:K` | both sides start anywhere; equality is required after a K-step reset sequence on pin N; this is the right check for a design that has a reset and does not rely on its power-up state |

After a formal FAIL, `equiv.py` replays the counterexample's inputs, extended to 60 steps, with
*both* sides' registers free, and asks SAT whether any pair of initial states makes every pin
equal. The verdict is printed (`power-up:`):

* "the logic differs (not a power-up question)": no change of power-up semantics or initial
  values can fix it;
* "the difference may be only the power-up state": try `--power-up fuse`, give the source an
  `initial` value, or use `--init free --reset …`.

Everything runs in the `open-atf150x-sim` Docker image through `tools/sim/run.sh`, which builds
the images on first use and mounts the repository at `/work` (only the repository is visible
inside; copy external designs into it, e.g. under `.scratch/`). The Rust binary is built in
release mode on demand (`CARGO_TARGET_DIR=.scratch/target-docker`); set `SIM_NO_BUILD=1` to
skip the rebuild check.

```sh
docker build -t open-atf150x-tools docker/ && docker build -t open-atf150x-sim docker/sim/   # once (run.sh does it too)
tools/sim/run.sh cargo build --release -p atf15xx-sim
```

## Check a fitted JED against its source

```sh
# CUPL design: .tt2 is the source, the .fit gives the pin placement
tools/sim/run.sh python3 tools/sim/equiv.py X.jed --device P1504C44 --tt2 X.tt2 --fit X.fit

# Verilog design (e.g. through tools/sim/synth_atf15xx.ys + yosys2tt2.py + a fitter)
tools/sim/run.sh python3 tools/sim/equiv.py X.jed --device P1504C44 --verilog X.v --top top --fit X.fit
#   (or --pin clk=43 --pin 'q[0]=12' ... instead of --fit)

# two JEDs for the same pins (vendor vs ours)
tools/sim/run.sh python3 tools/sim/equiv.py ours.jed --device P1504C44 --gold-jed vendor.jed
```

Output:

```
equiv X.jed (P1504C44): 32 pins compared; source drives 12, JED drives 32; work .scratch/sim/equiv/X
  formal: PROVEN  PDR proved the miter output is never 1
  sim:    PASS  4000 steps, 32 pins compared
RESULT: PASS
```

On a difference:

```
  formal: FAIL  counterexample of 1 steps  VCD .scratch/sim/equiv/counter/cex.vcd
    step 0: inputs p1=0 p4=1 p5=0 p8=1 p9=1 p11=1 p12=1 ...
        DIFF pin 6 source 0 jed 1
  sim:    FAIL  step 585 pin 6: source=0 jed=1 (stimulus .scratch/sim/equiv/counter/sim_stim.csv)  VCD .scratch/sim/equiv/counter/sim.vcd
RESULT: FAIL
```

`cex.vcd` has every public net of the miter (`g.*` = source, `t.*` = JED, `pN_in`); one VCD time
step = one simulator step. `cex.txt` is the per-step summary. `sim.vcd` is the iverilog run
(`g`, `u` instances). To look inside the JED at a pin, `atf15xx-sim equations X.jed -d D` lists
every macrocell's product terms in the emitter's names (`pad_m`, `fb_m`, `fold_m`, `pN_in`).

Useful options: `--assume EXPR` (Python syntax over `pN`: `'not (p41 and p44)'`,
`'p5 == 1'`), `--init free --reset p1=1:2`, `--ignore-pin N`, `--no-formal` / `--no-sim`,
`--timeout S` (PDR/BMC), `--depth N` (bounded depth), `--steps N --seed S` (simulation),
`--preset-wins --power-up 1` (model options). Exit status 0 PASS, 1 FAIL, 2 ERROR.

## Check a CUPL design with both fitters

```sh
tools/sim/run.sh python3 tools/sim/check_design.py DIR/NAME [equiv options]
tools/sim/run_designs.sh [LABEL ...]       # the user's real designs (copies them into .scratch/sim/real/)
```

`check_design.py` checks `DIR/NAME.jed` (vendor) and re-fits `NAME.tt2` with our `fit150x` using
the command line recorded in the vendor `.fit`, then checks that JED as well.

## Simulate a JED

```sh
tools/sim/run.sh .scratch/target-docker/release/atf15xx-sim run X.jed -d P1504C44 --cycles 200 --vcd x.vcd
tools/sim/run.sh .scratch/target-docker/release/atf15xx-sim run X.jed -d P1504C44 --stim stim.csv --print
tools/sim/run.sh .scratch/target-docker/release/atf15xx-sim compare a.jed b.jed -d P1504C44 --cycles 5000 --vcd diff.vcd
tools/sim/run.sh .scratch/target-docker/release/atf15xx-sim verilog X.jed -d P1504C44 -o x.v --pins-wrapper
tools/sim/run.sh .scratch/target-docker/release/atf15xx-sim equations X.jed -d P1504C44
```

Stimulus CSV: header = pin numbers, one row of 0/1 per step; pins not listed stay 0.
`--print` writes one row per step: every pin as `0`/`1` (driven), `Z` (not driven) or `-` (input
only), then `unstable` (1 = the step did not settle), and with `--print-q` the register bits.

## Self-checks (run after changing the model or emitter)

```sh
tools/sim/tests/run_tests.sh [60]        # time-bounded suite: 64-bit counter, blink.v, broken variants, vendor fits, tri-state, loops
tools/sim/run.sh python3 tools/sim/cosim.py --engine verilator --corpus   # Verilator vs the Rust simulator
tools/sim/run.sh python3 tools/sim/cosim.py --corpus                        # emitted Verilog vs simulator
tools/sim/run.sh python3 tools/sim/mutate.py X.jed --device D --tt2 X.tt2 --fit X.fit -n 30
docker/run.sh cargo test -p atf15xx-sim
```

