# 04 — JED simulator and Verilog equivalence checking (`atf15xx-sim`)

## Status

**2026-11-24**: time-bounded Verilator engine done (`tools/sim/vcompare.py`, also
`equiv.py --sim-engine verilator`; docs/architecture/sim/06-verilator.md). Verilator 5.054 is
built from source in `docker/sim/`. Every stage runs under a hard budget (`--budget`, default
60 s for `vcompare.py` and 600 s for `equiv.py`). There is a register correspondence
(`.map.json`, `.fit` names, signatures) with state injection. ABC `dprove` (signal
correspondence + induction) is the first formal engine. The 64-bit counter and the user's
`blink.v` are PROVEN in about 1 s. Their deliberately broken variants are caught within 30 s
(state injection). A pass/fail guarantee line is printed on every run. Earlier state of this log:
deliverables of the first task complete, power-up investigation done (below).

## Scope and ownership

Covers: decoding an ATF1502AS/1504AS/1508AS `.jed` into a functional model, a cycle-based
simulator, a JED → synthesizable Verilog emitter, and a flow that checks a fitted `.jed` against its
source (Verilog via Yosys, or a `.tt2`) formally and by simulation.

Owns: `atf15xx-sim/`, `tools/sim/`, `docs/architecture/sim/`, `docker/sim/`,
`.scratch/sim/`, this log, and the `sim/` section of `tools/README.md`.
Uses read-only: `atf15xx_arch::decode` and the device database, the fuse semantics in
`docs/architecture/fitter/02-fuse-map.md` §F2.8, `tools/sim/jedsim.py` as the reference
behaviour, `fit150x` binaries, and `tools/prjbureau/` for the Verilog → `.tt2` prototype.
Must not edit: `atf15xx-arch`, `atf15xx-fit`, `fit150x`,
`docs/architecture/fitter/`, `tools/sim/jedsim.py`, `external/prjbureau`, existing Docker files.
Problems found in those go to *Open issues* and `.scratch/sim/NOTES.md`.

## Results

All commands run from the repository root; `tools/sim/run.sh` = the `open-atf150x-sim` container.

### Self-checks of the model and emitter

| check | result | command |
|---|---|---|
| Rust simulator vs `jedsim.py`, 95 corpus JEDs (1502/1504/1508), burst stimulus, 1000 steps | 95/95 MATCH | `tools/sim/run.sh python3 tools/sim/xcheck_jedsim.py --corpus --steps 1000` |
| same, single-change stimulus | 95/95 MATCH | `... --corpus --single --steps 1000` |
| same, user vendor JEDs, 5 seeds × 3000 single-change steps | 6/6 MATCH up to the first combinational-loop or invalid-jedsim step (§M6) | `... --single --seed S --steps 3000 --device D .scratch/sim/user/*.jed` |
| emitted Verilog (iverilog) vs Rust simulator, 95 corpus JEDs × 600 steps | 95/95 PASS | `tools/sim/run.sh python3 tools/sim/cosim.py --corpus --steps 600` |
| same, 6 user vendor JEDs × 3000 steps | 6/6 PASS | `tools/sim/run.sh python3 tools/sim/cosim.py --steps 3000 --device D X.jed` |
| crate tests | 5/5; `cargo test --workspace` green | `docker/run.sh cargo test --workspace` |
| corrupted JED (one PT fuse of `spi` flipped) | caught: formal FAIL in 1 step + VCD, simulation FAIL at step 43; `atf15xx-sim compare` FAIL (473 mismatches) | `equiv.py .scratch/sim/t/spi_bad.jed --device P1504C44 --tt2 spi.tt2 --fit spi.fit` |
| mutation test, our `counter` JED vs Verilog, 25 model-visible single-fuse mutants | 10 caught (7 by both, 3 by formal only), 15 proven benign (spot-checked: x∧¬x PTs, always-1 foldback literal, unused GOE), 0 inconsistent | `tools/sim/run.sh python3 tools/sim/mutate.py .scratch/sim/ours/counter.jed --device P1504C44 -n 25 --verilog .scratch/prjbureau-explore/flow/counter.v --top top --fit .scratch/sim/ours/counter.fit` |

### Equivalence: fitted JED vs source

PROVEN = ABC PDR, unbounded; BOUNDED n = no difference in n steps (sat, for designs with a
combinational loop through their own pins); sim = 4000 random single-change steps. Power-up 0
unless stated.

| design (device) | source | vendor JED | our fitter's JED | constraint |
|---|---|---|---|---|
| `spi` (P1504C44) | `.tt2` | PROVEN, sim PASS | PROVEN, sim PASS | — |
| `rpi-pico-8088` (P1504C44) | `.tt2` | PROVEN, sim PASS (with OE-PT5 gating; FAIL on pin 11 without, see Decisions) | PROVEN, sim PASS | — |
| `input` (P1504C44) | `.tt2` | BOUNDED 20, sim PASS | BOUNDED 20, sim PASS | `not (p41 and p44)` (ALE ∧ RD would oscillate) |
| `motioncontroller` (P1504C44) | `.tt2` | BOUNDED 20, sim PASS | fit fails (`no free macrocell for node TIMEBASE_INT`) | `not (p41 and not p44)` |
| `system-controller` (P1508C84) | `.tt2` | BOUNDED 12, sim PASS; FAIL without the constraint (bistable bus loop, Journal) | BOUNDED 12, sim PASS | `not ((p84 == 0 or p80 == 0) and p2 == 0)` (RD/CODE ∧ WR) |
| `shift-register` (P1508C84, CUPL-compiled in `.scratch/user2`) | `.tt2` | **FAIL**: drives open-collector outputs high (pins 4, 10, 11, …), the vendor's truncated `-open_collector` list (known, worklog 02) | PROVEN, sim PASS | — |
| `blink` (P1504C44, CUPL) | `.tt2` | BOUNDED 40, sim PASS | PROVEN (sat induction), sim PASS | — |
| `blink` (P1508C84, CUPL) | `.tt2` | BOUNDED 40, sim PASS | PROVEN (sat induction), sim PASS | — |
| prototype `counter` (P1502C44 / P1504C44 / P1508C84) | Verilog | **FAIL** (pin-feedback bug); PROVEN with `oe` = 1 (pin 44 / 5 / 84) | PROVEN, sim PASS (all three) | — |
| prototype `mix` (P1504C44) | Verilog | **FAIL** (same, via `bus1`); PROVEN with `dir` = 1 | PROVEN, sim PASS | — |
| user `blink.v` (xilinx dir; P1508C84 via `yosys2tt2`) | Verilog | **FAIL**: logic differs, not a power-up question (Journal) | fit fails (`counter10: 11 product terms do not fit`) | — |
| user `spi_controller_mode0.v` (P1504C44) | Verilog | **FAIL**: pin-feedback bug on `spi_out` (Journal); also FAIL vs its own `.tt2` (jedsim agrees) | fit fails (`no free macrocell for node shift_in0`) | after-reset check: `--init free --reset p9=0:6 --reset-clock 43` |
| intermediate `.tt2` of `blink.v` (yosys2tt2) | Verilog | — | — | BOUNDED 30 (synthesis + `yosys2tt2.py` look correct) |

Power-up evidence (`--power-up fuse` = registers start at their `Invert` fuse, prjbureau's `reset`):

| JED vs source | power-up 0 | power-up fuse |
|---|---|---|
| `spi` vendor / ours | PROVEN / PROVEN | **FAIL** (29 steps) / PROVEN |
| `rpi-pico-8088` vendor / ours | PROVEN / PROVEN | **FAIL** / **FAIL** |
| `blink` 1504, 1508 vendor (CUPL) | BOUNDED 40 | BOUNDED 40 |
| `shift-register` ours, prototype `counter` (vendor with `oe`=1, ours), `mix` ours | PROVEN | PROVEN |

Commands:
* `tools/sim/run_designs.sh` (user designs, both fitters; log `.scratch/sim/run_designs.log`);
* prototypes: `tools/sim/run.sh sh .scratch/sim/proto.sh` (log `.scratch/sim/proto.log`), e.g.
  `check_design.py .scratch/prjbureau-explore/flow/ATF1502AS/counter --verilog .scratch/prjbureau-explore/flow/counter.v --top top`;
* user Verilog: `verilog_flow.py .scratch/sim/verilog/X.v --top X --device D --pin clk=N` (ours),
  vendor fit with `docker/prjbureau/run.sh tools/fuzzers/common/fit.sh fit150x X.tt2 -device D -preassign keep`
  in `.scratch/sim/vflow/X/vendor/`, then
  `equiv.py X.jed --device D --verilog X.v --top X --fit X.fit`;
* power-up: `tools/sim/run.sh sh .scratch/sim/pu_real.sh` (log `.scratch/sim/pu_real.log`).

### Time-bounded engines (2026-11-24, `tools/sim/tests/run_tests.sh 60`)

Budget 60 s per check (wall time includes about 1 s of container start). Deep-state designs on
the ATF1508AS; source = Verilog.

| check | engine | result | wall |
|---|---|---|---|
| `counter64` (64-bit counter, open flow, 69 MCs) | equiv `--sim-engine verilator` | PROVEN (dprove) + simulation | 13 s |
| `counter64` broken (MC13 PT1 drops `c[60]`: wrong only near 2^62) | equiv | FAIL (Verilator, from injected state, pin 6, 290 injections) | 31 s |
| `counter64` | vcompare | SIMULATED 1.7·10⁷ cycles + 16 959 injections (time budget) | 60 s |
| `counter64` broken | vcompare | FAIL at step 289 131 | 13 s |
| user `blink.v` (28-bit, open flow) | equiv | PROVEN (dprove) | 12 s |
| `blink.v` broken (MC63 PT1 drops `counter[26]`) | equiv | FAIL (injection) | 30 s |
| `blink.v` vendor fit | vcompare | FAIL at step 0 (power-up; the logic differs too, Journal) | 9 s |
| `spi` vendor (`.tt2`, `.fit` names + 1 signature match) | vcompare | SIMULATED 1.9·10⁷ cycles + 19 006 injections | 60 s |
| corrupted `spi` | equiv | FAIL | 13 s |
| `mix` (tri-state bus, ours) | equiv | PROVEN | 12 s |
| `mix` vendor (pin-feedback bug) | equiv | FAIL | 11 s |
| `input` (combinational loop, `--assume`) | equiv | BOUNDED 12 + SIMULATED 1.3·10⁷ cycles + 13 281 injections | 60 s |

Before: the open flow's `blink.v` reached only BOUNDED 40 in 300 s with PDR. Default-engine
regression (iverilog, no options): `spi` and `rpi-pico-8088` vendor PROVEN in 1.3 s each, the
prototype vendor `counter` FAIL (pin-feedback bug), our `counter` PROVEN in 0.8 s.
Verilator vs the Rust simulator (`cosim.py --engine verilator`): the 95 corpus JEDs and the user
JEDs PASS with two-phase latch evaluation (before it, `input` and `motioncontroller` differed:
latch→flip-flop race, see Decisions).

## Evidence levels

The model inherits its evidence levels from §F2.8 of the fuse-map spec
(`docs/architecture/sim/01-model.md` §M3 lists them per rule). Every rule whose level there is
*inferred*, or where the reading here departs from `decode.rs`, is a configurable option.
Results above are *oracle-backed* to the degree that the model is: the model agrees with jedsim
(validated against vendor `.tt2` files), and the vendor JEDs of 5 real designs prove or bound
equivalent to their `.tt2`.

## Decisions

- **2026-11-24: Formal engine order: ABC `dprove`, then `pdr`, then `bmc3`.** `dprove` computes a
  signal correspondence between the two designs (`scorr`) and proves the miter by induction over
  it. It is the correspondence-based induction the task asked for, with the correspondence found
  automatically, so it also works for vendor fits without names. 28-bit `blink.v`: 0.06 s, where
  PDR had timed out at 300 s. When dprove cannot close the proof, the time goes to the Verilator
  engine and then to PDR/BMC (`equiv.py --sim-engine verilator` stages them in this order).
- **2026-11-24: State injection uses an explicit correspondence**: `.map.json` (primary, our
  fitters), else `.fit` names, else simulation signatures, checked by a calibration run. It is
  applied only when complete. Injected states mix uniform, mostly-ones and mostly-zeros values.
  With uniform states the broken 64-bit counter was not caught in 60 s (its trigger needs a run of
  14 ones); with the mix it is caught after 290 injections. A FAIL from an injected state is
  reported as such (it could involve an unreachable state).
- **2026-11-24: Verilator evaluates each step in two phases** (inputs with latches held, then
  latches open), via a `latch_hold` term added to generated latches (JED model, `.tt2` gold). This
  is the Rust model's step order. Without it Verilator lets a flip-flop sample a latch that opens
  on the same clock edge (`input`: `INPUT_LATCH.LE = CLK`, `INPUT_SYNC.D = INPUT_LATCH` on `CLK`),
  unlike hardware, iverilog (nonblocking latch) and clk2fflogic. Latches of a Verilog source
  (`vsplit.py`) keep Verilator's ordering, which is the source's own Verilog race.
- **2026-11-24: Verilator from source, pinned**: `v5.054` (tag object 76d93f46, commit
  `bbb60a403932a9f695cb6ff001ceb73de6ad9234`, checked in the Dockerfile), the latest release on
  2026-11-24, replacing Ubuntu's 5.020.
- **2026-11-24: Power-up is an explicit, configurable part of every check.**
  * The JED side is set by `--power-up 0|1|fuse` (model). Default 0. `fuse` = each register
    starts at its `Invert` fuse, which prjbureau documents as the shared `reset` ("power-on reset
    value") fuse.
  * The source side is set by `--init zero|free` (zero: registers without an initial value
    start at 0), with `--reset pN=V:K` / `--reset-clock N` for after-reset checks.
  * Every formal FAIL gets a power-up diagnosis: SAT over the counterexample's inputs with both
    initial states free. "No initial state makes the pins equal" means the difference is not a
    power-up question.
  * Evidence for the default 0 [I]: the vendor JEDs of `spi` and `rpi-pico-8088` are PROVEN
    equal to their `.tt2` under 0 and FAIL under `fuse` (CUPL semantics are 0). Since the vendor
    may ignore power-up, this does not exclude `fuse` in silicon. The datasheet and prjbureau hint
    at `fuse` [P]. A silicon power-on test is needed (Open issues).
- **2026-11-24: OE code 7 with PT5 steered into the sum enables nothing** (option
  `oe_pt5_needs_mux`, default on; `--oe-pt5-ungated` restores decode.rs). The spec (§F2.3.2)
  defines code 7 + `Pt5Func` = 0 only for `Pt5Mux` = 1. With the decode reading, the vendor
  `rpi-pico-8088` JED drives pin 11 (MC3, a cascade lender whose PT5 is in the sum) when
  `fb_4 ∧ pad_17 ∧ fb_7`, though the design never uses pin 11 (formal FAIL, 4-step cex,
  `.scratch/sim/equiv/rpi-pico-8088/`). With gating, it is PROVEN. This matches rule 11 (PT3/PT5
  in the sum give 0 to AR/AP) and the vendor writing code 7 on buried cells. [I]
- **2026-11-24: A step settles with latches held, then flip-flops load, then latches open.**
  This is the latch clock-to-output delay: a flip-flop fed by a latch on the same clock gets the
  pre-edge value, and the emitter uses non-blocking latch assignments for the same reason.
  Asynchronous reset/preset and open latches are levels inside the settle (fixpoint), not
  one-shot updates. Earlier variants matched jedsim on the corpus but differed on
  `rpi-pico-8088`, `input` and `motioncontroller`. Investigation showed that the remaining
  differences are jedsim artefacts (Journal), not model questions.
- **2026-11-24: Formal engine = Yosys `clk2fflogic` + AIGER + ABC `pdr`** (unbounded), BMC
  (`bmc3`) when PDR times out, Yosys `sat -tempinduct` for netlists with combinational loops. This
  replaces the plan of `miter -equiv` + `sat -tempinduct` for everything: plain induction cannot
  prove sequential equivalence of differently structured netlists, and PDR can (spi: 7.7 s). The
  miter is our own Verilog (`equiv.py`), because gold and gate have different port sets.
  SymbiYosys is not packaged in Ubuntu 24.04 and is not needed.
- **2026-11-24: Every source becomes Verilog with the split pin interface**: `.tt2` → `tt2v.py`,
  Verilog → `vsplit.py` (Yosys `tribuf -logic` + JSON rewrite), JED → emitter. One engine then
  serves CUPL, Verilog and JED-vs-JED checks, and needs no Python `.tt2` simulator
  (jedsim's `Golden` refuses XOR designs).
- **2026-11-24: A pin the source does not drive must not be driven by the JED** (compared as
  "oe = 0"); `--ignore-pin` exempts a pin. Bus contention is a real failure.
- **2026-11-24: Comparisons between independent models use single-change stimulus**; inputs
  changing in the same step are unordered (as in hardware), so burst stimulus can legitimately
  differ in zero-delay simulators.
- **2026-11-24: The crate has its own JEDEC reader** (`parse_jed`) and derives "clock tied high"
  from the raw bits, so that it does not depend on `atf15xx-fit` or on decode API churn.
- **2026-11-23: Model semantics follow `tools/sim/jedsim.py` where it differs from the prose
  in `decode.rs`** (cascade needs a PT in MC−1; a lender with PTs uses its own sum); both are
  options (`cascade_requires_pts`, `lender_a_is_one`).
- **2026-11-23: Uncertain semantics are options**: reset-vs-preset priority (reset wins [I]),
  power-up value (0 [I]), idle MCs hold their register (jedsim).

## Journal

- **2026-11-25**: `jedsim.py` (the Python model) and `xcheck_jedsim.py` retired; the device record the
  scripts need comes from `atf15xx-sim device` (`simlib.device`). Remaining self-checks: the emitter
  against iverilog (`cosim.py`), mutation tests, the formal flow and vendor-JED identity.
- **2026-11-24** — Verilator engine and budgets (user request: "bound the time consumed").
  * Built Verilator 5.054 from source.
  * Wrote `vcompare.py`: generated C++ testbench, VPI state injection, VCD windows, and
    stage/cycle/total budgets.
  * Added to `equiv.py` (interface unchanged, new options only): `--budget`, `--sim-engine`,
    `dprove` first, a `GUARANTEE:` line, and `sat_induct` now honours the budget.
  * Wrote `tools/sim/tests/` (counter64.v, make_broken.py, run_tests.sh).
  * The open flow's fitter panics on a function with more than 64 inputs
    (`atf15xx-fit/src/logic.rs:133` "complement: too many variables"), reported to main. A
    flat 64-bit top bit cannot be routed anyway: 63 inputs > 40 UIM lines per block.
    `counter64.v` therefore uses registered carry-lookahead flags (it is still a cycle-exact
    64-bit counter).
- **2026-11-24** — **Power-up investigation (coordinator request), user `blink.v`.**
  The source drives the LEDs 1 at power-up and the vendor JED 0; the source (`leds = ~counter[27:24]`,
  no reset, no initial value) leaves power-up undefined. Findings, in order:
  1. The four LED macrocells are registered pin outputs with `Invert` = 1, and they are the only
     `Invert` = 1 registers in the JED. Under `--power-up fuse` the step-0 difference disappears,
     but a difference remains after the first clock.
  2. With *both* sides' registers free and the clock forced to toggle, no initial state makes
     the vendor JED equal to `blink.v` over 60 steps (SAT: no model). The same holds against the
     JED's own `.tt2`. Sanity: the same query finds a model for our PROVEN `counter` pair. So the
     difference is in the logic, not in power-up or initial values. jedsim's tt2 check agrees
     independently (`DIFFERENT from tt2`, 10 500 pin mismatches).
  3. The `.tt2` is BOUNDED-30 equal to `blink.v`, so the vendor fit is where it goes wrong. The
     fit came from pass 2 (`CASCADE_LOGIC` retry after "## Warning : Placement fail"). Flipping the
     `Invert` fuse of the four LED macrocells (fuses 61489/61574/61578/61582) turns the difference
     into an initial-state-only one (the existence query finds a model). The discrepancy is the
     polarity of those four `Invert` bits.
  4. Is the model's `Invert` polarity wrong for registers? No: vendor JEDs with `Invert` = 1
     registers in the same configuration are PROVEN/BOUNDED equal to their `.tt2`
     (`rpi-pico-8088`: 8; `motioncontroller`: 8 registered pin outputs with `out=q`). The cascade
     options (`--lender-a-is-one`, `--cascade-without-pts`) and `--oe-pt5-ungated` do not change the
     verdict.
  Conclusion: a vendor fitting discrepancy in a pass-2 cascade fit [formal under the model;
  jedsim agrees], not the power-up semantics, the initial values, or the method.
- **2026-11-24** — User `spi_controller_mode0.v` (vendor fit of our `yosys2tt2` `.tt2`): FAIL after
  reset as well (`--init free --reset p9=0:6 --reset-clock 43`). The diagnosis says the logic
  differs. Cause: `spi_out` (MC16, pin 4, OE = `fold_13`) reads its own register through `pad_16`.
  In the counterexample the OE is off at t=15 with the external pin at 1, so `pad_16` = 1 while
  `q_16` = 0, and the register loads 1. This is the pin-feedback bug again, on real user Verilog.
- **2026-11-24** — All 6 user designs with vendor output and the 3 CUPL-compiled ones checked with
  both fitters (table). Our fitter is PROVEN/BOUNDED wherever it fits; it fails to fit
  `motioncontroller` and the Verilog `blink.v` / `spi_controller_mode0.v` (capacity: wide sums,
  macrocell count), which is for the fitter workstream.
- **2026-11-24** — Prototype Verilog designs (`.scratch/prjbureau-explore/flow*`: `counter` on
  1502/1504/1508, `mix` on 1504) checked against the Verilog source. The vendor JEDs FAIL in 1 step
  on all three families. The cause is the **suspected vendor bug, now confirmed by formal**: the
  counter registers (`q0..3`, outputs with `.OE = oe`) are read back through their pads
  (`pad_1/3/4/5` in `atf15xx-sim equations`) instead of `fb_*`, so with `oe` = 0 the counter
  sees the external pin levels. Counterexample (1504): `oe` = 0, `en` = 1, pins 12/11/9/8 = 1 →
  JED `carry` (pin 6) = 1, source 0. `mix` has the same defect through `bus1` (`y = r & shared`
  reads `r` from pin 4, driven only while `dir`). With the OE held on, all four vendor JEDs are
  PROVEN. Our fitter's JEDs are PROVEN with no constraint.
- **2026-11-24** — 1508 `system-controller` vendor JED vs `.tt2`, investigated independently.
  With the model's tied-high clock (`Pt4Func` ∧ ¬`Pt4Mux`, the vendor's `-global_fold` latch),
  jedsim (`golden`, 2000 cycles) now reports "matches tt2", which agrees with the fitter
  workstream's fix. Formal finds one counterexample (8 steps): pins 17 (`CPU_AD4`) and 49
  (`BUS_D4`) read each other's pins (`CPU_AD = !Sel & BUS_D.IO`, `BUS_D = CYCLE_CPU & CPU_AD.IO`).
  With both outputs enabled (`CPU_RD|CPU_CODE` and `CPU_WR` at once) this is a bistable loop,
  and the solver resolves it to 0 in the source and 1 in the JED. Both are valid states, so
  this is not a fitting error, and an 8088/8051 bus never asserts RD and WR together. With
  `--assume 'not ((p84 == 0 or p80 == 0) and p2 == 0)'`: BOUNDED 12, sim PASS. Conclusion: no
  remaining vendor 1508 mismatch.
- **2026-11-24** — `input` and `motioncontroller`: the address latch (`[A].L = [D].IO`,
  `LE = ALE`) is transparent to the D bus that the chip itself drives while RD is active, so
  ALE ∧ RD oscillates (the simulator flags it; iverilog would spin). Formal uses the sat path
  because of the structural loop. With the ALE/RD constraint, `input` is BOUNDED 20, sim PASS.
- **2026-11-24** — Built `tools/sim/`: `tt2v.py`, `vsplit.py`, `equiv.py` (miter, PDR, BMC,
  sat fallback, cex replay to VCD + text summary, iverilog co-simulation), `check_design.py`,
  `run_designs.sh`, `cosim.py`, `mutate.py`, `simlib.py`. First proof: `spi` vendor JED ≡ `.tt2`.
- **2026-11-24** — jedsim cross-check differences resolved. In all remaining cases jedsim's own
  state is not a fixpoint, and `xcheck_jedsim.py` now detects and reports this:
  - (a) its 30-round settle does not converge (`system-controller`: driven pads ≠ drive);
  - (b) it samples latches once per step, before async resets and register updates
    (`rpi-pico-8088`: GCLR clears MC4/7/10 → MC20 rises → PT-enabled latch MC16 must load;
    `input`: a GCLK1 edge changes pin 5 that a GCLK3 latch is transparent to).
  The Rust model settles to a true fixpoint in both cases.
- **2026-11-23** — decode API changed twice while the simulator was being written
  (`ClockSource::AlwaysHigh` added, then replaced by `clock_tied_high`); the crate now reads the
  raw bits (NOTES.md).
- **2026-11-23** — Crate built: model (`model.rs`), simulator (`sim.rs`), emitter (`verilog.rs`),
  CLI `verilog | equations | run | compare`. First xcheck: 95/95 corpus JEDs match jedsim.
- **2026-11-23** — Surveyed the decode API, the §F2.8 table and jedsim.py. Built
  `docker/sim/Dockerfile` (`FROM open-atf150x-tools` + iverilog 12.0, Verilator 5.020, z3,
  boolector; Yosys 0.33 + yosys-abc come from the tools image): `docker build -t open-atf150x-sim docker/sim/`.

## Open issues

- **Register power-up value** (0 vs `Invert`/`reset` fuse): inferred; needs a silicon power-on test
  (a JED with `Invert` = 1 registers on pins, read right after power-on). Until then, a design
  that relies on power-up state should be checked under both `--power-up 0` and `--power-up fuse`.
  For our fitter, a power-up-robust choice is `Invert` = 0 on registers without reset or initial value.
- **Vendor `blink.v` fit (pass-2 cascade)**: four LED registers with the wrong `Invert` polarity
  under the model. Reproduce with
  `equiv.py .scratch/sim/vflow/blink/vendor/blink.jed --device P1508C84 --tt2 .scratch/sim/vflow/blink/vendor/blink.tt2 --fit .scratch/sim/vflow/blink/vendor/blink.fit`.
- **Vendor fitter: register feedback of an OE'd output is routed via the pad** (Journal
  2026-11-24). Our fitter must keep using `fb`. Byte-compatibility with the vendor here would copy
  the bug; the equivalence check is the arbiter.
- **decode.rs: `OutputEnable::ProductTerm` is returned for code 7 even when PT5 is in the sum**;
  the simulator gates it (Decisions). Proposal for the fitter workstream: decide in §F2.3.2 /
  §F2.8 rule 13 and in decode. Their oracle runs never compare pins the design does not use,
  so the question is open there too.
- Designs with combinational loops through their own pins get only BOUNDED results (sat
  induction does not converge). A loop-cutting transformation that is sound under the given
  `--assume` would allow PDR.
- `vsplit.py`: Verilog designs with `inout` ports not driven by a tri-state, or multiple drivers,
  are unsupported (04-limitations.md).

## Artifacts

- `tools/sim/vcompare.py`, `tools/sim/tests/` (counter64.v, make_broken.py, run_tests.sh),
  `docs/architecture/sim/06-verilator.md`, `atf15xx-sim/examples/mcbit_addr.rs`.

- `atf15xx-sim/` (README), `tools/sim/` (`tools/README.md` §`sim/`),
  `docs/architecture/sim/` (01 model, 02 emitter, 03 equivalence, 04 limitations, 05 harness).
- `docker/sim/Dockerfile`, image `open-atf150x-sim`.
- `.scratch/sim/` — scratch only: `equiv/`, `designs/` (work directories with `cex.vcd`,
  `sim.vcd`), `user/` and `real/` (copies of the user's designs; not to be committed),
  `run_designs.log`, `proto.log`, `NOTES.md`.
