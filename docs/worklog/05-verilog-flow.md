# 05 — Open Verilog → Yosys → fitter → `.jed` → SVF flow

## Status

**2026-11-24 (final)**: the flow is done and validated (Results, corpus6). Every design that fits
is PROVEN against its Verilog, including the user's `blink.v` and `spi_controller_mode0.v`.
Every equivalence run is time-bounded.
* `atf15xx-synth` works with the pinned **Yosys v0.71**. The default logic mapping is now Yosys'
  own gates (`synth -noabc`); the importer builds the covers. `abc -sop` was removed upstream in
  0.69, and the mapping is still available as `--map abc-sop` for older Yosys.
* **The user's Xilinx designs pass end to end** (fit, `equiv.py`, SVF):
  * `spi_controller_mode0.v` on ATF1504AS-PLCC44: 48/64 MCs, **PROVEN**, sim PASS;
  * `blink.v` on ATF1508AS-PLCC84: 32 MCs, **PROVEN** (corpus6; the earlier BOUNDED 40 / 3000 s
    PDR attempt was stopped at the user's request). Re-verified by the main session on 2026-11-24:
    fresh build, `equiv.py` PROVEN by signal correspondence + induction in 1.5 s, sim PASS.
* **OE-PT5-in-sum resolved**: output never enabled, [I]. Rule 13, §F2.3.2. Our fitter no longer
  emits the combination, and decode has an additive `oe_effective()`.
* Fitter placement improvements: UIM repair moves feeder cells, block avoidance is soft, and
  strategies are retried (enables, ties, spreading, shuffled orders) in the flow.
* Byte-identical regression is unchanged on all oracle corpora: feat/gclk3/glob 1633/2421/2045,
  and the other corpora 649/2749/5084, the same as the pre-change build.

Next: Open issues 1 (a real placer), then 3 (logic optimisation).

## Scope and ownership

A flow from Verilog to `.jed` and `.svf` that uses only open-source tools (Yosys, our fitter
crates, `atf15xx-tool`). The vendor fitter and ATMISP are used only as oracles.

Owns:
* the Yosys-JSON front end, the constraints/attributes format and the flow driver
  (`atf15xx-synth`);
* `docker/yosys/` and `tools/flow/`;
* `docs/architecture/fitter/07-yosys-flow.md` and this log.

May extend: `atf15xx-fit`, `fit150x` and `atf15xx-arch`. Changes to
`atf15xx-arch` decode must be additive only, and are announced here.

Does not touch: `atf15xx-sim`, `tools/sim/`, `docs/architecture/sim/` and `docker/sim/`
(simulator workstream).

## Results

### Final corpus (corpus6, 2026-11-24)

The build ran with the pinned **Yosys v0.71** (default `--map none`). The check used `equiv.py`
against the *source* Verilog, run by `tools/flow/corpus.py` with a hard wall-clock limit
(`--eq-timeout 60`: formal ≤ 60 s, the check killed after 180 s). equiv.py's formal engine is
now signal correspondence + induction (ABC `dprove`, simulator workstream) within a budget,
which proves the wide counters in about 1 s. Simulation is iverilog, 4000 random steps. The
whole check stage took 9 s.

```
X="--extra .scratch/flow/xil/blink.v=ATF1508AS-PLCC84,ATF1504AS-PLCC44 --extra .scratch/flow/xil/spi_controller_mode0.v=ATF1504AS-PLCC44,ATF1508AS-PLCC84 --extra .scratch/flow/xil/blink8.v=ATF1508AS-PLCC84"
docker/yosys/run.sh python3 tools/flow/corpus.py --stage build -j 6 --out .scratch/flow/corpus6 $X
tools/sim/run.sh   python3 tools/flow/corpus.py --stage check -j 6 --eq-timeout 60 --out .scratch/flow/corpus6 $X
```

Table: `.scratch/flow/corpus6/results.md`. MCs = macrocells used.

| design | what it tests | 1502 PLCC44 | 1504 PLCC44 | 1508 PLCC84 |
|---|---|---|---|---|
| counter4 | CE, async reset, tri-state outputs | 5, PROVEN | 5, PROVEN | 5, PROVEN |
| counter16 | up/down, sync load, CE, async reset | 20, PROVEN | 20, PROVEN | 20, PROVEN |
| decoder | 4→16, active-low | 16, PROVEN | 16, PROVEN | 16, PROVEN |
| fsm | Moore FSM, sync reset | 4, PROVEN | 4, PROVEN | 4, PROVEN |
| latchsr | DFF with S/R/S+R, negedge, latches, latch with reset | 7, PROVEN, sim FAIL¹ | same | same |
| mix | latch, preset, bidirectional bus | 5, PROVEN | 5, PROVEN | 5, PROVEN |
| mux16 | 16:1 mux + 8-bit compare, 36 inputs | — | TQFP100: 18, PROVEN | TQFP100: 18, PROVEN |
| parity | 16-bit XOR + registered parity | 10, PROVEN | 10, PROVEN | 10, PROVEN |
| shift32 | 32-bit shift register, shared output bit | no fit (33 > 32 MCs; vendor fails too) | 33, PROVEN | 33, PROVEN |
| spi | port of the user's CUPL `spi.pld` (original pinout) | — | 36, PROVEN | — |
| tribus | 8-bit bidirectional bus + registers | 32, PROVEN | 32, PROVEN | 32, PROVEN |
| twoclk | two clocks, both edges of one GCLK | 9, PROVEN | 9, PROVEN | 9, PROVEN |
| **user `spi_controller_mode0.v`** | Xilinx project | — | **48, PROVEN** | 53, PROVEN |
| **user `blink.v`** | Xilinx project, 28-bit counter | — | 32, PROVEN | **32, PROVEN** |
| blink8 | same RTL, 8-bit counter | — | — | 12, PROVEN |
| counter64 | behavioural 64-bit counter: en, sync clear, async reset (corpus7) | — | — | 87, PROVEN (also TQFP100) |

All simulation runs PASS except latchsr¹. Every fitted design has an SVF from atmisp
(`<top>.svf`, 34/34 checked in corpus4).

¹ The gold model has a race (Open issues 8). The source Verilog gives the same result as our JED
(`.scratch/flow/latchrace/tb.v`).

Earlier: corpus4 used the same flow before the cover-cost cap (`.scratch/flow/corpus4/`), with
PDR as the formal engine. blink was BOUNDED 40 there, and mux16 did not fit within minutes. The
3000 s PDR run on blink was stopped on the user's direction (bounded verification).

### Vendor oracle on the same netlists (corpus5: our `.tt2`, `tools/fuzzers/fitter-conformance/vendor_oracle.sh`)

Macrocells used (vendor: "Total Logic cells used"):

| design | ours | vendor |
|---|---|---|
| counter16 1502 / 1504 / 1508 | 20 / 20 / 20 | **no fit** / 23 / 23 |
| parity | 10 | 13 |
| mix | 5 | 6 |
| spi | 36 | 36 |
| spi_controller_mode0 1504 | 48 | 52 |
| blink 1508 | 32 | 28⁴ |
| shift32 1502 | no fit | no fit |
| all others | equal | equal |

⁴ The vendor puts the 4 inverted LED outputs into the counter registers' macrocells (`Invert`
on a registered pin). That is the configuration the simulator found to be functionally wrong in
the vendor's fit of `blink.v` (worklog 04). We keep separate output cells.

Equivalence of the vendor JEDs (made from the same `.tt2`) against the Verilog
(`.scratch/flow/vendor_equiv.log`):
* PROVEN: counter16, decoder, fsm, parity, shift32, tribus, twoclk, spi_controller_mode0.
* **FAIL**:
  * counter4, mix, spi: the known pin-feedback defect on OE'd outputs;
  * blink: the `Invert` polarity defect of worklog 04;
  * latchsr: simulation only, the gold race.

Our JEDs pass in every one of these cases.

### User `spi` (CUPL), `.scratch/flow/spi/`

| check | result |
|---|---|
| our JED (from `spi.v`) vs `spi.v` | PROVEN, sim PASS |
| **vendor JED** of the original `spi.pld` (WinCUPL) vs `spi.v` | **PROVEN**, sim PASS: the port is faithful |

### Fitter regression

Byte-identical counts against the vendor are unchanged by all fitter changes (GCLK polarity,
PT power-down, UIM repair, soft avoidance, OE code):
* feat/gclk3/glob: 1502 1633, 1504 2421, 1508 2045;
* all other oracle corpora: 649, 2749, 5084.

These are identical to the pre-change build (`verify_tt2.py --bin-dir`). `cargo test --workspace`
is green.

## Evidence levels

* Corpus equivalence: **formal proof** (PDR, unbounded) of the fitted JED against the source
  Verilog, through the simulator's JED model (`atf15xx-sim`, cross-checked with jedsim and
  vendor JEDs, worklog 04). It is not silicon evidence.
* Power-up 0 is **inferred** (worklog 04); the flow makes it explicit (`--init`).

## Decisions

- **Yosys v0.71**, tag commit `b4c586fd37d0ec61bb38fc1a85fd6a2fde48fcd1`, built from source in
  `docker/yosys/Dockerfile` (CMake release preset, no Python; it asserts the commit). Yosys ≥ 0.6x
  builds with CMake only. **`abc -sop` was removed in Yosys 0.69** ("Removed sum of products
  related parameters from abc pass", CHANGELOG), so the importer must not depend on `$sop` (see
  Journal).
- **Direct JSON → `netlist::Design`**, no `.tt2` in the open path (`--emit-tt2` is for debugging
  and the oracle). Mapping rules: §F7.4.2.
- **The importer accepts only the cells the script leaves** (`$sop` or fine-grained gates,
  `$_NOT_`/`$_BUF_`, the legal FF/latch types, `$_TBUF_`). Anything else is an error naming the
  cell.
- **Comb outputs read their own macrocell feedback**, never the pad: this avoids the vendor's
  pin-feedback defect.
- **Initial values**: dfflegalize passes init values through (`01`). `--init error` (default)
  refuses init = 1, `ignore` warns, and `invert` lets dfflegalize store such registers inverted
  (init 0 only). Power-up 0 stays an assumption (worklog 04).
- **Optimisation order**:
  1. toggle recovery (flattened view, CE folded);
  2. CE recovery;
  3. dead-node removal;
  4. limited collapse;
  5. latch reset recovery;
  6. cover splitting;
  7. `absorb_output_registers` and `legalize_special_functions`;
  8. `fit_design`, over a list of strategies (spec §F7.4.3).
- **Limited collapse**: a node is inlined only if:
  * every reader stays within `--max-products` (10 = one macrocell + one cascade neighbour,
    minus special PTs);
  * the macrocell balance does not get worse;
  * a node with several readers does not increase the total product count.

  "Ties" (the node becomes a cascade neighbour) are allowed in the first strategy, not in a later
  one: allowing them is better for parity (6 vs 11 MCs), forbidding them better for
  spi_controller.
- **Gate mapping by default** (`synth -noabc`, the importer builds covers). It works on every
  Yosys version. `abc -sop` duplicated shared logic: spi_controller 91 cells vs 51.
- **Toggle recovery may leave Q in T** (`T = load·(d⊕Q) + …`): needed for sync loads.
- **Latch reset recovery**: Yosys maps `if (r) q = 0; else if (le) q = d;` to `LE = le + r`,
  `D = !r·d`. This races when `r` falls. It is turned into AR = r, `LE = le`, `D = d`.
- **Cover splitting** (`split_large`) into buried OR nodes, because our fitter can cascade from
  one neighbour only.
- **Fitter fix 1: mixed GCLK polarity.** The GCLK invert bit is per network. When one clock pin
  is used on both edges, the majority polarity (cells with CE weighted first) gets the network
  and the rest use a PT clock. Before the fix the invert bits were OR'ed, so negedge `twoclk` /
  `latchsr` registers clocked on the wrong edge. Found by formal (corpus run 1).
- **Fitter fix 2: PT power-down on constant outputs.** `PtPowerOff` was set for any constant
  data cover, which also killed a PT5 output enable (`assign irq_n = irq ? 0 : 1'bz` was never
  driven). It is now set only when the MC uses no PT. Found by formal on `spi` pin 16.

## Journal

* **2026-11-25** (main session) — F8.7: SELFPROGRAMBOARD **52.6 MHz** by default (60 MCs; was
  33.3 MHz; WinCUPL + vendor 50 MHz): a capacity strategy with neither foldback signals nor
  toggles (CE functions as macrocell nodes, D flip-flops that doubling pairs), node conversion
  only when the strategy asks, and every capacity strategy fitted with the best kept (MCs, then
  fmax). Proven: BOUNDED 25 + sim vs the Verilog, BOUNDED 22 + sim vs the vendor JED.
  Performance (F9.11): speculative parallel strategies limited to the pending capacity set
  (the first, wider speculation was measured slower), both `--map auto` mappings in parallel
  (CLI Yosys runs and fits; plugin fits). SELFPROGRAMBOARD 26 s → 6.3 s; corpus 396 s → 298 s.

* **2026-11-25** (main session) — Path independence (F9.7): Yosys now runs in the sources'
  common directory and reads them by relative path, so a design gives byte-identical JEDs
  whichever way its path is spelled. `check_corpus.sh` reads the same way in the plugin.
  Strategy knob `fold_one_level` with timing targets; SELFPROGRAMBOARD at `clock CLK 50`
  still misses (one-level runs out of macrocells), documented in F8.7.

* **2026-11-25** (main session) — Tighter integration (F9.9): `--map words` (`wordmap.rs`), `--map
  auto` (default: both mappings fitted, the better kept; in the plugin from one RTLIL with
  `design -save/-load`), a failure-driven strategy loop (`Strategy` struct; capacity failures
  queue the cell-reducing strategies; one post-fit alternative). SELFPROGRAMBOARD fits with
  default options; vblink now 22 MCs / 66.7 MHz by default. Fixed: a parity hang in the fitter's
  collapse (product cap 256; vendor identity unchanged). Timing targets (F8.7) in the
  constraints file and `--optimize`. Vendor comparison on SELFPROGRAMBOARD (F9.10): our fitter on
  CUPL's netlist 60 MCs / 55.6 MHz vs vendor 64 / 50 MHz; our synthesis 60 MCs but 33 MHz.

* **2026-11-25** (main session) — **R4 closed** (F9.8). Toggle recovery now falls back to an
  exact BDD analysis (`biodivine-lib-bdd`) where SOP flattening explodes. New pass
  `opt::factor_wide` turns cells wider than a block's inputs into a shared carry chain. Results:
  counter16 20 → 16 (1502/1508), counter64 70 → 65 / 87 → 65 (fmax 43.5 → 100 MHz on PLCC84);
  corpus 34/34 PROVEN, nothing worse; tests 34/34; equivalence suite all ok. Silicon:
  `vcnt40.v` 36/36 across the 40-bit wrap, read over JTAG. R2's vblink single-level acceptance
  stays unmet (wrap-compare complement), recorded in F9.5.

* **2026-11-24** (main session) — **cxx bridge** (user proposal; F9.7): `atf15xx-yosys`
  replaces the plugin's JSON-text + C-ABI path. Rust drives Yosys (`Pass::call`), reads RTLIL
  as shared structs (JSON-backend-exact bit ids and encoding), fits, and writes `atf15xx_mc` /
  `atf15xx_pin` back onto the design's wires. `-check-json` passes on 34/34 corpus builds and
  on SELFPROGRAMBOARD, and the JEDs are fuse-identical to the CLI's. Found a path dependence
  (Yosys `$`-names carry the source path; identical results need `read_verilog -sv <abs>`).
  Importer: `import_value`, flow: `fit_value` (netlist in memory). Tried stripping directories
  from hidden net names: the corpus stayed identical, but parity still differed by path, because
  the path is also in Yosys's cell names, which order the design. Reverted; F9.7 records the
  real fix (a canonical order).

* **2026-11-24** (main session) — Timing (R5): `atf15xx-sim timing` (F8.5/F8.6), the flow
  writes `<top>.timing.rpt`, prints fmax, and with `--fmax` keeps trying strategies (adding
  wider sums) until one meets it. `vblink`: default 38.5 MHz (fails at 66.6 MHz on silicon, as
  predicted); `--fmax 66.6` picks a 66.7 MHz fit with 22 instead of 31 MCs, which runs on silicon
  (33.5 Hz blink). XOR synthesis (`xor_synthesis` + `xor_split`) on by default.

* **2026-11-24** (main session) — **SELFPROGRAMBOARD Verilog port fits**: 60 of 64 MCs on the
  ATF1504AS-PLCC44 (`--no-toggles --option special_foldback=on`), after the fitter's UIM-aware
  position search (worklog 02). Sim PASS 4000 steps and formal BOUNDED 27 against the Verilog;
  sim PASS and formal BOUNDED against the vendor's hardware-proven JED. Importer fix: bus bit
  names now use the declared range (`input [15:10] A` → `A[10]`…`A[15]`, from Yosys's
  `offset`/`upto`); before, they were numbered from 0 and `A[0]` collided with the port `A0`
  (renamed `A0_1`), which the equivalence harness then compared against the wrong pin. The
  original CUPL declares the field as `A[15:10, 0]`-style non-contiguous bits, hence the port
  split.

* **2026-11-24** (main session) — Dense-design strategies (vendor-style node collapse first when
  cells > macrocells). SELFPROGRAMBOARD Verilog port: 77 → 65 cells (80 → 64 with `--no-toggles`),
  then UIM block A 45 > 40: not yet fitting (the `.tt2` path fits, with block A at 36). Corpus vs
  corpus7: all PROVEN, no new failures; counter64/PLCC84 87 → 70 MCs, counter16/1504 20 → 16,
  spi 36 → 35 (toggle budget, R1).

* **2026-11-24** (main session, from silicon) — `recover_toggles` flattened D with a 64-product
  budget; for a counter with a synchronous wrap (`c == 999999 ? 0 : c + 1`) D contains the
  complement of the compare and exceeds 64 products, so 7 of 20 bits kept Yosys's carry-lookahead
  network (4 array passes: fails at 66.6 MHz on silicon). Budget now 1024: those bits become 1–2
  product T flip-flops (D' 12–26 products, T 1–2), build time 6.4 s → 1.3 s. Corpus (all built-in
  designs, user `blink.v`/`spi_controller_mode0.v`) unchanged: same results and MC counts, all
  PROVEN; workspace 33/33. With `--max-products 15` vblink is single-level and runs at 66.6 MHz.
- **2026-11-24**: **wide functions** (report from the simulator workstream).
  * `Sop::complement()` panicked above 64 signals; a plain 64-bit counter hit it. It now falls
    back to a symbolic Shannon complement and never panics. `Sop::try_complement(max)` is bounded.
  * `fit_design` rejects any cell with more than 40 distinct inputs, the UIM inputs per block
    (`check_support`), with an error naming the cell and its support size, from the flow and the
    fit150x CLIs alike.
  * Root cause: inlining. `opt::MAX_SUPPORT` = 32 bounds `collapse_limited` and the flattened
    analysis views (toggle/enable recovery), so carry chains stay buried nodes. The fitter's
    vendor-style `collapse_comb_nodes` keeps a node whose collapse would exceed 40 inputs.
  * The fitter now places leftover inputs on unused dedicated input pins when the I/O pins run
    out.
  * Result: `tests/designs/counter64.v` (behavioural, enable + sync clear + async reset) fits
    ATF1508AS-PLCC84 (JTAG off, all 64 I/O used) and TQFP100 in 87 MCs: 64 registers + 23 carry
    nodes, 30 T flip-flops.
    * equiv.py `--budget 60`: PROVEN in 2.7 s (dprove).
    * vcompare.py `--budget 60`: SIMULATED 10.8 M cycles + 10 828 state injections, with 64
      register classes matched by name through `counter64.map.json`.
  * Corpus7 (Yosys 0.71) matches corpus6, plus counter64.
  * All oracle corpora are still byte-identical: 2282 / 5170 / 7129 (= 1633+649, 2421+2749,
    2045+5084), and ours-failed is unchanged at 11 / 67 / 69.
- **2026-11-24**: final corpus6 (Results). Every fitting design is PROVEN with the new equiv
  engine, in 9 s in total.
- **2026-11-24**: **`<top>.map.json` / `<stem>.map.json`** (format `atf15xx-map/1`, spec
  F5.4), from `atf15xx-synth` and `fit150x`. For each cell it gives the netlist name, the source
  (Verilog net) name, the MC/block/node, `invert_fuse`, the storage and the taps; it also lists
  the input pins. Requested for the simulator's register-correspondence engine.
- **2026-11-24**: bounded verification (user direction).
  * Stopped the 3000 s blink PDR run.
  * `corpus.py --eq-timeout` (default 60 s) passes `--timeout`/`--depth 30` to equiv.py and kills
    a check after 2×+60 s (result TIMEOUT).
  * The fitter got `Options::deadline`, and the flow `--fit-time` (default 120 s) bounds all
    strategies.
  * Found that `cost()` complemented wide covers (exponential: mux16 spent minutes there before
    fitting). It now takes the given polarity for > 4 products over > 12 variables. mux16 then
    fits in 0.2 s (18 MCs).
- **2026-11-24**: final corpus4 (Yosys 0.71) and vendor oracle corpus5 (Results).
  * `.tt2` writer: copies for internally read comb outputs (the vendor rejected the output name
    in `.ilb`).
  * `<top>.vendor-cmd` is written for the oracle.
  * `--fit-time` budget added (mux16 strategies took minutes each).
- **2026-11-24**: OE-PT5-in-sum (coordinator request). Scanned 17 193 vendor JEDs with
  `examples/oe_scan.rs` and read the prjbureau PT5 routing table → gated reading [I]
  (02-fuse-map §F2.3.2, rule 13). The fitter now uses code 0 for MCs without an OE whose PT5 is
  in the sum; it emitted the ambiguous combination 5× on rpi-pico/system-controller buried
  registers before. Decode: additive `MacrocellConfig::oe_effective()`; `oe` is unchanged
  (raw). Simulator: its default already gated; only its doc comment changed; `--oe-pt5-ungated`
  unchanged. Regression unchanged.
- **2026-11-24**: CE recovery, MC-cost-aware collapse with a shared-logic product guard, and
  flow strategies (spread pins, shuffled orders, no-cascade). In the fitter: feeders as UIM
  repair candidates, avoidance bounded and soft. `spi_controller_mode0` fits: 48 MCs, PROVEN.
  Gate mapping made the default; `abc -sop` is optional (`--map`).
- **2026-11-24**: mapping study on spi_controller / spi / parity / mux16 (`.scratch/flow/xil/mtry.sh`):

  | mapping | spi_controller | spi | parity | mux16 (1508 PLCC84) |
  |---|---|---|---|---|
  | RTL gates (`synth -noabc`) | 51 cells, no fit (placement) | 36 MCs | 6 | UIM fail |
  | `abc -g` gates | 50 cells, UIM fail | 36 | 6 | UIM fail |
  | `abc -sop -P 4` | 60, no fit | 50 | 11 | 10 |
  | `abc -sop -P 80` | 91, no fit | 42 | 10 | 12 |

  The gate mappings work on Yosys 0.71 and are better except for mux16, where the fitter's UIM
  routing fails. Next: fitter placement.
- **2026-11-24**: the coordinator's targets.
  * `blink.v` fits on 1508/1504 (32 MCs; 27 T flip-flops). Vendor/yosys2tt2 failed it and our
    old path did not fit.
  * `spi_controller_mode0.v`: Yosys failed first (`$_SDFF_PN0_` with init could not be
    legalized under init `x`); init policy changed to `01`. Then the fit failed: 90 cells with
    `abc -sop -P 80`, because ABC duplicates shared logic (shift_edge, load, reset) into every
    bit's SOP.
- **2026-11-24**: corpus run 2/3: all pass except latchsr sim (gold race) and shift32 on 1502.
  Parameter study → `-P 80`.
- **2026-11-24**: corpus run 1: 15/36 pass. Failures:
  * fit: wide covers; parity (16-PT node); a register driving two ports (shift32: importer bug,
    fixed: a port that reads another cell's bit is a copy of its feedback);
  * formal FAIL: twoclk and latchsr (GCLK polarity, fitter fix 1); spi (pin 16, fitter fix 2).

  Added: cover splitting, flattened toggle recovery with CE folding and Q in T, `--init`,
  per-design part lists (`NAME.parts`).
- **2026-11-24**: Yosys image: v0.71 has no Makefile (CMake now); Dockerfile switched to
  `cmake --preset release`. Built.
- **2026-11-24**: crate written (importer, opt, part names, constraints, `.tt2` writer, flow,
  CLI). First run: counter4 PROVEN on 1504.
- **2026-11-24**: workstream started; surveyed the IR, prjbureau prototype and simulator tools.

## Open issues

Gaps for production use:
1. **Placement/UIM routing** is the limit on dense or wide-fan-in designs.
   * The fitter fills blocks in order, and its UIM repair moves one floating cell per retry.
   * The flow compensates with strategy retries: output-pin spreading, shuffled orders, and no
     cascade.
   * Failing strategies cost seconds; `--fit-time` (default 120 s) bounds the total.
   * Needed: a real placer (fan-in-aware, UIM-checked, e.g. annealing over the
     `node`/`pin` constraints the fitter already honours).
2. **Cascade from one neighbour only.** Wider covers are split into buried OR nodes (one extra
   pass of delay).
3. **Logic optimisation** is greedy:
   * no common-subexpression extraction;
   * no XOR-aware decomposition beyond what the gates give;
   * counter16 takes 20 MCs with gates vs 16 with `abc -sop` (Yosys ≤ 0.68), and mux16 18 vs 10;
   * blink takes 32 MCs vs the vendor's 28, because the inverted LED outputs are not merged into
     their registers.
4. **Power-up value** unknown on silicon (`--init` is explicit; worklog 04).
5. **Timing**: none. No delay model, no report of passes/cascade depth, no fmax.
6. **Not supported**:
   * memories (Yosys must map them to FFs, which works only for tiny ones);
   * open-drain via attribute only on outputs;
   * `(* KEEP *)` or node-number constraints for buried logic from Verilog;
   * GCLK3/ITD options untested;
   * the BE parts.
7. **Silicon**: no device has been programmed with an open-flow JED yet. Evidence is the
   simulator's model.
8. **Gold-model race** in `tools/sim/vsplit.py` (simulator workstream): a Verilog latch with reset
   (`always @* if (r) q = 0; else if (le) q = d;`) becomes `if (!(le|r)) … = d & !r` in gold.v.
   iverilog resolves it wrongly when `r` falls (gold 1, source 0); formal is unaffected. Repro:
   corpus `latchsr`, step 3843; `.scratch/flow/latchrace/tb.v` shows the source gives 0.
9. The `.tt2` writer (oracle only) gives combinatorial outputs that are read internally a buried
   copy `<name>_fb`, because CUPL has no name for that feedback.

## Artifacts

* `atf15xx-synth/`: crate, `data/synth_atf15xx.ys`, `tests/designs/*.v|.acf|.parts`
* `docker/yosys/`: pinned Yosys v0.71 image
* `tools/flow/`: `corpus.py`, `experiment.py`, `vendor_oracle.sh`
* `.scratch/flow/`: corpus runs (`corpus/results.md`), experiments, spi checks, Xilinx designs
  (`xil/`)
* Fitter changes in `atf15xx-fit/src/fit.rs`:
  * GCLK polarity votes;
  * `PtPowerOff` only without PTs;
  * OE code 0 with PT5 in the sum;
  * UIM repair feeders;
  * bounded/soft block avoidance;
  * debug output (`ATF_FIT_DEBUG`).
* `atf15xx-fit/examples/oe_scan.rs`; scans in `.scratch/flow/oe_scan*.tsv`
* `atf15xx-arch/src/decode.rs`: additive `MacrocellConfig::oe_effective()`
* `.scratch/flow/corpus4/` (final table), `.scratch/flow/corpus5/` (vendor oracle),
  `.scratch/flow/xil/` (user Xilinx designs, copies; `out/` = fits and proofs)
