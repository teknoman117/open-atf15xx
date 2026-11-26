# 01 — Functional model and cycle simulator

Source: `atf15xx-sim/src/model.rs` (model), `src/sim.rs` (simulator).
Evidence tags as in `docs/architecture/fitter/02-fuse-map.md`: **[O]** oracle-verified,
**[L]** vendor labels, **[P]** probable, **[I]** inferred.

## M1 Inputs

* A JEDEC file: `QF` fuse count (selects the family: 16808 / 34192 / 74136), `F0`/`F1` default,
  `L` fields. Parsed by `atf15xx_sim::parse_jed` (no dependency on the fitter crate). The
  checksum is not verified.
* A device (`P1504C44`, `f1504plcc44`, … — `atf15xx_arch::Device`): pin ↔ macrocell map,
  dedicated pins (GCLK1, GCLR, OE1, GCLK2/OE2), GCLK3 pin, JTAG pins, input-only pins.
  The JED family must match the device family.

## M2 Structure

`atf15xx_arch::decode::decode` gives, per macrocell (MC 1…n, 16 per block), the five product
terms as literal lists over the block's 40 UIM lines and 16 foldback lines, their use
(`PtUse`), and the macrocell configuration; plus the UIM selections, GCLK/GCLR inversion, the
global OE selections and the JTAG enable. The model resolves every UIM line to a signal:

| `Source` | model signal |
|---|---|
| `Pad(m)` | `pad_m` — the level on MC m's pin (driven value if the device drives it, else external) |
| `Fb(m)` | `fb_m` — MC m's feedback (register Q, or the combinational `x`, by `FbMux`) |
| `Gclk1` / `Gclr` / `Oe1` / `Oe2` | the external level of that dedicated pin |
| `Gnd` / unused | 0 |

Pins: every I/O pin of the package (minus the four JTAG pins when the JTAG port is enabled),
the four dedicated inputs, and input-only pins (1508 TQFP160).

## M3 Semantics per macrocell

Per step, with all signals settled:

| # | rule | evidence |
|---|---|---|
| 1 | `pt_k` = AND of its literals; a PT with no literals (all fuses blown) = 1; an unprogrammed PT (`None`) = 0 | [O] |
| 2 | foldback line of host MC h = ¬`pt1(h)` if `Pt1Mux` (PT1 → foldback), else 1 | [O] |
| 3 | `sum` = OR of the PTs whose use is `Sum`, OR the cascade from MC−1 (same block) if MC−1 has `CasMux` and at least one PT | [O] (PT condition: option) |
| 4 | `x = sum ⊕ B ⊕ ¬Invert`, B = `pt2` if PT2 is in XOR mode (`Pt2Mux`), else 1; a lender (`CasMux`) with no PTs has `x` = 1 | [O] |
| 5 | register D = `x` (`DMux` = 1; T flip-flop: `x ⊕ Q`), `pt2` (fast path, `DMux` = 0, `OMux` = 1), or the MC's pad (fast input register) | [O] |
| 6 | clock: GCLK1/2/3 (`GCLKn = pin ⊕ invert`; GCLK3 is the pad of the GCLK3 pin's MC), or `pt4` (`Pt4Mux` ∧ `Pt4Func`), or tied high (`Pt4Func` ∧ ¬`Pt4Mux`: a latch is transparent, a flip-flop never loads) | [O] |
| 7 | clock enable = `pt4` when PT4 is `ClockEnable`, else 1 | [O] |
| 8 | async reset = `pt3` (PT3 `Reset`) ∨ GCLR (`GclrMux` = 0; `GCLR = pin ⊕ invert`); async preset = `pt5` (PT5 `Preset`) | [O] |
| 9 | reset beats preset when both are asserted | [I] → option `reset_wins` |
| 10 | storage: D flip-flop (rising edge, if CE), T flip-flop, transparent-high latch | [O] |
| 11 | output = `x` (`OMux` = 1) or Q; feedback = `x` (`FbMux` = 1) or Q | [O] |
| 12 | OE: code 0 → off; 7 ∧ `Pt5Func` → on; 7 ∧ ¬`Pt5Func` → `pt5` **if PT5 is steered to its special function** (`Pt5Mux` = 1), else off; other codes → GOE n = selected source ⊕ invert | [O]; gating [I] → option `oe_pt5_needs_mux` |
| 13 | open drain: the pin is driven 0 when OE ∧ output = 0, otherwise released | [O] |
| 14 | power-up: every register is 0 (alternative `--power-up fuse`: each register starts at its `Invert` fuse, prjbureau's `reset` fuse) | [I] → options `power_up`, `power_up_from_fuse` (see §M7) |
| 15 | an MC with no PTs and no `CasMux` keeps its register at the power-up value | option `idle_mc_holds` |

Rule 12's gating is the one place where the model deliberately departs from `decode.rs` (which
returns `OutputEnable::ProductTerm` regardless of `Pt5Mux`). Reasons: PT3 and PT5 steered into
the sum give 0 to their reset/preset input (rule 8), the vendor writes code 7 with
`Pt5Func` = 0 on buried macrocells, and with the ungated reading the vendor JED of the user's
`rpi-pico-8088` would drive pin 11, which the design does not use; with the gated reading that
JED is proven equivalent to its `.tt2` (worklog 04, 2026-11-24). `--oe-pt5-ungated` restores
the decode reading.

## M4 Options (`model::Options`, CLI flags of every `atf15xx-sim` subcommand)

| option | default | flag | meaning |
|---|---|---|---|
| `reset_wins` | true | `--preset-wins` | priority when AR and AP are both asserted |
| `power_up` | 0 | `--power-up 1` | register value at power-up |
| `power_up_from_fuse` | false | `--power-up fuse` | each register powers up to its `Invert` fuse value |
| `cascade_requires_pts` | true | `--cascade-without-pts` | a cascade from MC−1 needs a PT in MC−1 |
| `lender_a_is_one` | false | `--lender-a-is-one` | a lender with PTs sees A = 1 on its own XOR (decode prose) |
| `idle_mc_holds` | true | `--clock-idle-mcs` | MCs with no PTs/no cascade do not clock |
| `oe_pt5_needs_mux` | true | `--oe-pt5-ungated` | rule 12 gating |

## M5 Cycle simulator

The simulator advances in *steps*. A step applies a new set of external pin levels (pins not
given keep their level) and computes the settled state. Time inside a step is not modelled:
there are no delays; every signal is a fixpoint of the combinational equations.

`Sim::new`: registers at power-up, all external levels 0, settle; clock levels in that state
are the "previous" clock levels (no edge at power-up).

`Sim::step(ext)`:

1. **Settle with latches held.** Iterate (≤ 200 rounds) until `pad`, foldback, `x`, `fb`, the
   pin drives and Q are stable. Inside the iteration asserted async reset/preset force Q (they
   are levels); latches keep their value.
2. **Edges.** Every edge-triggered register whose clock is 1 now and was 0 after the previous
   step, whose CE is 1 and on which no async control is asserted, loads the D of the state from
   (1). So a flip-flop fed by a latch on the same clock sees the latch's value from before the
   edge — the latch's clock-to-output delay. All flip-flops load simultaneously.
3. **Settle with latches transparent.** As (1), and an open latch passes D to Q.

A step whose settle does not converge is flagged (`Sim::last_unstable`, the `unstable` column
of `atf15xx-sim run --print`, a warning at the end). This happens only for a combinational loop
that oscillates, e.g. a transparent latch fed back through its own output pin, or two
bidirectional pins that read each other while both are driven. The state after such a step is
undefined. Harnesses stop comparing there.

Inputs that change together in one step have no defined order (as in hardware, where the result
would depend on skew). Comparisons between models that resolve such races differently use
*single-change* stimulus (one input flips per step).

## M6 History: the Python model

Until 2026-11-25 a second, independent Python model (`jedsim.py`, the fitter workstream's
reference) cross-checked this one step by step: they agreed on all 95 corpus JEDs and the user's
designs, except where the Python model's state was not a valid fixpoint (it settled for at most 30
rounds and sampled latches once per step); the Rust simulator settles to a true fixpoint, as
hardware would. The Python model was retired with the move of every table into atf15xx-arch; the
checks that remain are vendor-JED identity, the emitter against iverilog and the formal
equivalence flow.

## M7 Power-up: evidence (2026-11-24)

* Datasheet: "all registers will be initialized, and the state of each output will depend on the
  polarity of its buffer" (value not given). prjbureau: the `Invert` fuse is shared with a
  `reset` option, "the power-on reset value of the storage element" (fuse 1 = VCC). Both hint that
  the power-up value may follow the fuse. [P]
* Vendor JEDs: the designs proven or bounded equal to their `.tt2` under power-up 0 include
  registers with `Invert` = 1 (`rpi-pico-8088`: 8, `motioncontroller`: 8 registered pin outputs).
  Whether they also pass under `fuse` is recorded in worklog 04. CUPL's `.tt2` semantics
  themselves assume 0, and the vendor fitter chooses `Invert` to minimise product terms, so vendor
  data cannot separate the two readings: a fitter that does not care about power-up fits both.
* The Yosys `blink` case looked like a power-up question but is not: no initial state on either
  side makes the vendor JED equal to its `.tt2` or its Verilog (worklog 04).
* Status: **inferred**; default 0; `--power-up fuse` gives the prjbureau reading. A silicon test
  (read the pins of a JED with `Invert` = 1 registers right after power-on) would settle it.
