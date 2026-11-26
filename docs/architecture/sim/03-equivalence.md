# 03 — Equivalence method

Driver: `tools/sim/equiv.py JED --device D (--tt2 X.tt2 | --verilog a.v … --top T | --gold-jed Y.jed) [--fit X.fit]`.

## E1 What "equivalent" means

A fitted JED (*gate*) is equivalent to its source (*gold*) if, for every sequence of external
pin levels starting from power-up, after every step and on every pin:

* the two drive the pin at the same time (`pN_oe` equal), and
* when they drive it, they drive the same value (`pN_out` equal; masked, 02-verilog.md §V1);
* a pin the source does not drive (an input, or a pin it does not use) must never be driven by
  the JED: driving it would fight whatever is connected (`--ignore-pin N` to exempt a pin).

Internal state (which macrocell holds what, inverted registers, foldbacks, cascades) is not
compared, so any correct fit is accepted.

## E2 Gold models (all with the split pin interface)

| source | converter | semantics |
|---|---|---|
| `.tt2` (CUPL netlist) | `tools/sim/tt2v.py` | CUPL meaning of `X`, `X.D/.REG/.T/.L`, `.C/.LE/.CE/.AR/.AP/.OE`, `.Q/.PIN` feedback, `-` and `.phase` complements; empty OE cover = always on; open-collector outputs from the `.fit` command line; registers power up 0 |
| Verilog | `tools/sim/vsplit.py` (Yosys `proc; flatten; tribuf -logic; opt; memory`) | the Verilog semantics of the source. Each top-level port bit is bound to a pin. An input drives `pN_in`. An output gives `pN_oe = 1`. A tri-state (`$tribuf` E, D) gives `pN_oe = E`, `pN_out = E & D`, and readers of the port inside the design see the pad `E ? D : pN_in` |
| another JED | `atf15xx-sim verilog` | the model of that JED (e.g. vendor vs. our fitter, when both use the same pins) |

Pins: `NAME is placed at pin N` / `NAME assigned to pin N` lines of the fitter's `.fit` report
(the fitted placement), else the `.tt2` `#$ PINS` line, plus `--pin PORT=N` for Verilog. A
vector bit `name[i]` is called `namei`, as `tools/sim/yosys2tt2.py` and the vendor
fitter name it.

Register initial values in gold: `--init zero` (default) gives every register 0 at power-up,
the device's behaviour; Yosys `setundef -zero -init` fills registers that have no `initial`
value. `--init free` leaves gold *and* gate registers unconstrained (formal: free initial
state; simulation: random initial values in gate, X in Verilog gold), for designs that rely on
a reset sequence: combine it with `--reset pN=V:K` (pin N held at V for the first K steps,
outputs compared from step K on) and, for a synchronous reset, `--reset-clock N` (pin N toggles
during those steps). In simulation, source outputs that stay X (registers the reset does not
clear) are skipped and counted. The JED's power-up is a model option (`--power-up 0|1|fuse`,
01-model.md §M7).

## E3 Formal check

1. `miter.v` instantiates gold and gate on shared `pN_in` and computes
   `bad = ok & a & (step ≥ R) & (any pin differs)`, where `a` is the `--assume`
   expression and the reset-sequence constraints of this step, `ok` remembers that `a` held in all
   earlier steps, and R = the reset length or `--skip` (registers on `$global_clock`).
2. Yosys: `read_verilog -formal; proc; flatten; clk2fflogic; opt; techmap; dffunmap; aigmap;
   write_aiger -zinit`. `clk2fflogic` turns every clocked and level-sensitive storage element
   (any clock net, including product-term clocks; async reset/preset; latches) into logic over a
   single global step clock, with edges detected from the previous step's value. So one formal
   step is one simulator step (01-model.md §M5): any set of inputs may change, flip-flops load
   the D from before the step.
3. ABC `dprove` first: signal correspondence (`scorr`: equivalences between the two designs'
   registers and signals, guessed by simulation and proved by induction) plus induction on the
   miter. **PROVEN** when it closes the proof. This settles wide counters in milliseconds
   (28-bit `blink.v`: 0.06 s; 64-bit counter: under 1 s). Then ABC `pdr` (property-directed
   reachability, unbounded): **PROVEN** = `bad` is unreachable from power-up for every input
   sequence. All engines share the `--budget` (06-verilator.md §T5). A reachable `bad` is **FAIL**, then Yosys `sat -seq N`
   replays it to `cex.vcd`, and `cex.txt` / the console summary lists the inputs per step and
   the pins that differ. If PDR times out, `bmc3` gives **BOUNDED** (no difference in the first
   N steps). After a FAIL, the **power-up diagnosis** replays the counterexample's inputs
   (extended to 60 steps) with both sides' registers free and asks SAT for *any* pair of initial
   states with equal pins on every step. No model means the difference is in the logic. A model
   means it may be only the initial state. This query is sound for the "logic differs"
   verdict, because one input sequence with no matching initial state suffices.
4. Combinational loops (AIGER needs an acyclic netlist): if the miter has a structural loop,
   Yosys `sat -tempinduct` is used instead (loop = constraint set). Induction gives PROVEN, a
   base-case failure gives FAIL with `cex.vcd`, and an inconclusive run gives BOUNDED to
   `--depth` steps. A loop is
   functionally harmless only under some input constraint (e.g. an address latch is transparent
   to a bus that the same chip drives only while RD is active: ALE ∧ RD would oscillate);
   `--assume` states that constraint. Without it the solver can resolve a bistable loop
   differently in gold and gate and report a spurious FAIL (see `system-controller`, worklog 04).

## E4 Simulation check

Gold and gate in one iverilog testbench, the same `pN_in`, random single-change stimulus
(`--steps`, `--seed`; stimulus filtered to satisfy `--assume` and `--reset`), all compared pins
written each step (0/1/Z/X) and compared in Python. Power-up is handled as in 02-verilog.md §V4.
The first difference is reported with its step, pin, the stimulus CSV and `sim.vcd`.
Simulation is incomplete by nature: the mutation test found differences that only formal sees.
It is a fast first check and gives a waveform that is easy to read.

## E5 Verdict

`RESULT: PASS` when no engine reports FAIL (formal PROVEN or BOUNDED, simulation PASS);
`FAIL` (exit 1) when either finds a difference; `ERROR` (exit 2) when a tool failed. Always read
the formal line: BOUNDED is weaker than PROVEN.

## E6 Self-checks of the method

* `cosim.py`: emitter vs. Rust simulator (same model, two executions).
* `mutate.py`: single-fuse mutants of a passing JED must FAIL unless formal proves them benign;
  a mutant that formal proves equal but simulation finds different would be a flow bug.
* the corrupted-JED, OE-feedback and counter cases in worklog 04.
