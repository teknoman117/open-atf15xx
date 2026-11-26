# 06 — Time-bounded comparison with Verilator, state injection and guarantee levels

Driver: `tools/sim/vcompare.py` (standalone) or `equiv.py --sim-engine verilator` (as an engine).
Tool: Verilator **5.054** built from source in `docker/sim/Dockerfile` (tag `v5.054`, commit
`bbb60a403932a9f695cb6ff001ceb73de6ad9234`, checked at build time).

## T1 Why

A design whose interesting state is deep (a 28- or 64-bit counter) cannot be reached by
simulating from power-up, and plain bounded model checking or PDR may not converge either
(the open flow's `blink.v` reached only BOUNDED 40 in 300 s). Two things fix that:

* **Correspondence-based proofs** (formal): if each source register has a macrocell register
  that holds the same value, the equivalence is 1-inductive over that correspondence, so wide
  counters prove in milliseconds. ABC's `dprove` computes the correspondence itself (`scorr`:
  candidate equivalences from simulation, then proved or refined by induction) and proves the
  miter. It is now the first formal engine in `equiv.py` (03-equivalence.md §E3).
* **State injection** (simulation): with an explicit correspondence, the testbench periodically
  writes the same random state into corresponding registers of both models and keeps comparing.
  Every injected state is a start state for the comparison, so carries out of 60-bit counters are
  exercised within seconds.

Everything runs under hard time limits (T5).

## T2 Testbench

One Verilator model `vtop` contains `gold` (the source, split pin interface) and `gate` (the
JED, `atf15xx-sim verilog`); both see the same `pN_in`. Tri-state and bidirectional pins are
split *before* Verilator: the source by `vsplit.py` (Yosys `tribuf`, then `$tribuf` →
`pN_oe = E`, `pN_out = E & D`, readers see `E ? D : pN_in`), the JED by the emitter's explicit
OE ports. So no `z` reaches Verilator. After every step the generated C++ compares, per pin,
OE and the value while enabled. A pin the source never drives must never be driven by the JED.

Stimulus, all deterministic from `--seed`:

1. the reset sequence (`--reset pN=V:K`, `--reset-clock N`);
2. then alternating phases: `--random-len` steps of random single-input changes (each change is
   kept only if `--assume` holds; the expression is compiled into the C++), and `--burst-len`
   steps that toggle only the clock pins (the GCLK pins the source uses, plus `--clock N`);
3. every `--inject-every` steps, if the correspondence is complete, a state injection. The
   injected states mix uniform random, mostly-ones and mostly-zeros patterns, because a
   counter carries only on long runs of ones.

On the first mismatch the run continues for `--window` steps and writes:
* `mismatch_pins.vcd`: every input and both sides' `oe`/`out`, from `window` steps before to
  `window` steps after the mismatch;
* `mismatch_full.vcd`: every signal of both models (Verilator trace), from the mismatch on.

## T3 Register correspondence

Sources, in order:

1. `<jed stem>.map.json` (`atf15xx-map/1`, docs/architecture/fitter/05-vendor-outputs.md §F5.4,
   written by our fitters): each registered cell's `source` net (`counter[5]`), macrocell and
   `q_is_value` (polarity).
2. The fitter report's placement lines (`NAME is placed at … (MC m)`), vendor or ours, against
   the gold model's names: `.tt2` node names, or Verilog bits (`name[i]` → `namei`).
3. Signatures: a calibration run from power-up (no injection) records every register. Name-matched
   pairs must have equal or complementary traces, which fixes their polarity and rejects wrong
   names. An unmatched source bit pairs with a unique JED register whose trace is equal or
   complementary and that toggles.

Gold register names are taken through Yosys' renaming (`assign c[31:16] = _22_;`), so a
register that `proc` split into chunks still matches `c[17]`. Only JED registers whose Q is read
(feedback or pin) count. Injection is enabled only when the correspondence is **complete**: every
source register bit and every read JED register is in a class. The report says how it was built,
e.g. `68 classes (68 by name)`.

A mismatch after an injection is reported as `FAIL … (from injected state)`. A correct fit
computes the same next state as the source from every pair of corresponding states, unless it
relies on states the source can never reach (sequential optimisation, which neither fitter does).
Such a FAIL means "different on a corresponding state". Check whether that state is reachable
before blaming the fitter for a design with unreachable states (one-hot FSMs, for example).

## T4 Guarantee levels (`equiv.py` `GUARANTEE:` line, `--json` `guarantee`)

| level | meaning |
|---|---|
| `PROVEN (signal correspondence + induction (ABC dprove))` | unbounded; equal for every input sequence from power-up |
| `PROVEN (PDR …)` | unbounded (IC3/PDR) |
| `BOUNDED n` | no difference in the first n steps from power-up (bmc3); may be followed by `+ SIMULATED …` |
| `SIMULATED n cycles + k injections` | Verilator found no difference in n steps, including k injected states |
| `SIMULATED n steps (iverilog)` | the iverilog engine found no difference |
| `FAIL` | a difference: formal counterexample (`cex.vcd`, `cex.txt`), or simulation mismatch (VCD window), from power-up or from an injected state |
| `NONE` | no engine produced a result within the budget |

## T5 Budgets

| option | default | what it bounds |
|---|---|---|
| `--budget` | 60 s (`vcompare.py`), 600 s (`equiv.py`) | total wall clock; every stage gets at most what is left |
| `--compile-budget` | 40 s | the Verilator compile (subprocess timeout) |
| `--cycles` | 5·10⁷ | simulation steps |
| `--timeout` | 300 s | each formal engine (dprove / PDR / bmc3), also capped by the budget |

`equiv.py --sim-engine verilator` stages the work:

1. `dprove` (≤ 25 % of the budget);
2. if it did not decide, Verilator with injection (≤ 60 % of what is left);
3. then PDR and bmc3 with the rest, unless the simulation already failed.

A proof caps the following simulation at 20 % of what is left. Every subprocess has a timeout,
and the C++ checks its own deadline every 1024 steps. When a budget runs out, the result states
what was achieved (cycles, injections, bounded depth) instead of hanging.

## T6 Verilator limitations for this use

* **2-state.** Verilator has no X or Z: `--x-assign 0` and `--x-initial 0` (zero power-up) or
  `--x-initial unique` with `+verilator+rand+reset+2` (`--init free`: random, seeded). So an
  undefined source register is a *random defined* value, never X. A source output that depends
  on an unreset register is compared like any other output (iverilog and formal treat it as
  undefined/free instead).
* **Tri-state.** Verilator resolves `z` only in limited cases (top-level `inout` becomes
  `__en`/`__out`; internal multi-driver resolution is restricted). This is why both models are
  split at the pin boundary before Verilator sees them (T2). A source whose tri-states `vsplit.py`
  cannot split (04-limitations.md) cannot use this engine.
* **Initial values.** `reg x = v;` and `initial` blocks are honoured at time 0. The emitter's
  power-up values (`--power-up`) and the source's `initial` values therefore apply, and there is
  no time-0 X→1 clock edge (unlike Icarus, which needs the reset trick of 02-verilog.md §V4).
* **Combinational loops.** Allowed (`UNOPTFLAT` is only a performance warning). A loop that
  oscillates ends the simulation with "didn't converge" (`--converge-limit`), which the harness
  reports as an error at that step. Use `--assume` to exclude the inputs that cause it.
* **Latches.** Verilator evaluates an `always @*` latch before edge-triggered blocks sample, so a
  flip-flop clocked by the edge that opens a latch sees the latch's *new* value. Hardware, the Rust
  model, iverilog (nonblocking latch) and clk2fflogic all give the *old* value. The harness
  therefore evaluates every step twice: first with `latch_hold` = 1, then with it at 0. A
  `!latch_hold` term is added to every generated latch enable (JED model, `.tt2` gold), so
  flip-flops sample first and latches open second. Asynchronous reset/preset still act in both
  phases. With this, the Verilator run of the emitted Verilog matches the Rust simulator on all 95
  corpus JEDs and the user JEDs (`cosim.py --engine verilator`); without it `input` and
  `motioncontroller` differed. Latches in a *Verilog source* (`vsplit.py`) are left alone: their
  ordering is the source's own Verilog race.
* **Event ordering.** Verilator evaluates the model to a fixpoint after each input change, with
  no delays. As with iverilog, inputs that change together are unordered, so the stimulus changes
  one input per step.
* **Speed.** About 3–4·10⁵ steps per second for the 1508 designs with VPI access compiled in
  (`--public-flat-rw`). Compile time is 2–10 s.
