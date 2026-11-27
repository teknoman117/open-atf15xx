# F4. Fitting: what the vendor does, and what our crates do

The fitter turns a netlist of sum-of-products cells into a fuse array. Our implementation
(`atf15xx-fit`) is organised as a reusable back end so that the `.tt2` front end and a future
Yosys front end feed the same IR:

```
.tt2 ──tt2::Pla──► netlist::Design ──► fit::fit_design ──► jed::Fuses ──► jed::write_vendor ──► .jed
 (Yosys BLIF/JSON ──► netlist::Design, TODO)        placement · global nets · PT allocation ·
                                                     UIM routing · assembly
```

## F4.1 The IR (`netlist.rs`)

* `Design { inputs: name → pin?, cells: [Cell] }`
* `Cell { name, is_output, pin?, node?, storage: Comb|Dff|Tff|Latch, data: Sop, active_low,
  clk?, ce?, ar?, ap?, oe?: Sop }`
* `Sop` = OR of products; a product is a sorted list of `(Signal, polarity)`;
  `Signal = Pad(name) | Fb(name, registered) | Fold(key)`.

This is the natural target for Yosys: after `abc -sop` (or `techmap` to SOP cells) and flip-flop
mapping to DFF/DFFE/latch with async set/reset, each Yosys output/flop maps to one `Cell`;
`$_TBUF_`/`$tribuf` gives `oe`; pin constraints come from a constraints file.

## F4.2 Netlist transformations (vendor behaviour reproduced) [O]

1. **Collapse combinatorial buried nodes** into their readers (vendor default; nodes with a node
   constraint are kept). Complements use the minimiser in `logic.rs`.
   **Exception (ours):** a node is kept, with node number 0, if collapsing it would give a
   reader more than 40 distinct inputs (`UIM_INPUTS_PER_BLOCK`), because such a function can never
   be placed. Functions of more than 64 signals are complemented symbolically (Shannon expansion,
   `Sop::complement`); they never panic.
2. **Absorb output registers**: `Y = N.Q` where N is a buried register read only by Y → Y becomes
   the register (the vendor ignores N's node constraint in this case).
3. **`X.PIN` of an always-enabled combinational output whose logic is one literal** is replaced by
   that literal (vendor optimisation observed in `pinfb`).
4. **Feedback of an always-enabled output is routed through its pad** (`Xnp`, not `Xnfb`), for both
   plain and `.Q` references (`combfb`, `regfb`).
5. **Special functions needing more than one PT**: if the complement is a single product, use a
   foldback expander (vendor `FB_5nn` nodes, e.g. `BUS_A0.OE = FB_599` in the user's 1508 design);
   otherwise move the function into a new buried node and read its feedback.

## F4.3 Polarity and product-term allocation [O]

* Target = data (or its complement for `-` outputs). The vendor uses the complement when it needs
  strictly fewer PTs, and sets `Invert` (e.g. `A # B` → one PT `!A & !B`, inverted; the 3-term sum
  in `sum3` → 2 PTs inverted). Ties keep the true form.
* Special PTs: clock → PT4 (`Pt4Func` = 1), CE → PT4 (`Pt4Func` = 0), AR → PT3, AP → PT5
  (`Pt5Func` = 1), OE → PT5 (`Pt5Func` = 0).
* Sum PTs fill PT1, PT2, … skipping specials, in the order of the `.tt2` cubes.
* More than the free PTs → cascade from MC−1 (helper gets `CasMux` = 1; observed direction is
  always MC−1 → MC). Wider sums chain further down: MC−2 into MC−1, and so on, one helper per
  further 5 terms, all in the cell's block, at most 6 macrocells (30 terms); the vendor builds
  wider sums without cascades (*oracle-verified*, `tools/fuzzers/fitter-cascade`, ATF1504AS,
  6–40 terms, 2026-11-26). Our fitter puts sum term k beyond the cell's own free PTs into
  helper MC−h, h = ⌈k / 5⌉, slot (k − 1) mod 5 + 1 (`atf15xx-fit/src/capacity.rs`, shared by
  the netlist passes, placement and the planner); the chained JEDs are PROVEN equivalent to
  their `.tt2` (44 designs), not fuse-identical (the input pin order differs).

## F4.4 Global nets [O]

* A clock that is a single literal of an input on the GCLK1 pin, GCLK2/OE2 pin or GCLK3 I/O pin
  uses that network (complement literal → GCLK invert bit). **Floating** clock inputs are moved to
  the GCLK1 pin, then GCLK2 (the vendor did this for `K` and even for a latch-enable `B`).
* Single-literal AR from the GCLR pin → `GclrMux` = 0 (+ invert); floating ones are moved to GCLR.
* Single-literal OE → the first GOE network whose table contains the source (and the same network is
  shared by equal enables); floating OE inputs go to OE1, then OE2.

## F4.5 Placement (vendor order reproduced for small designs) [O]

1. Pinned inputs/outputs, then node constraints (`6nn`) unless the MC already drives a pinned
   output (then the constraint is ignored, as the vendor does for `SPI_STATE0:624` in `spi.tt2`).
2. JTAG pins (if JTAG on) and PD pins (if on) occupy their pads.
3. Floating **outputs**: lowest block first, **highest** free bonded MC of the block first
   (observed: Z goes to MC16, or 15/14 when 16 is taken/unbonded).
4. Floating **buried nodes**: lowest block first, highest MC whose pin is unusable (unbonded or
   reserved JTAG/PD) — 1502 C44 → MC9 (TMS), 1504 T100 → MC8 (TDI), 1508 T100 → MC15 (unbonded);
   then (ours) MCs whose pad is an input, then any free MC; subject to a per-block fan-in budget.
5. Floating **inputs**: cells are visited block by block (outputs before nodes); each unplaced input
   goes to the **lowest** free bonded MC in order of first use (observed `C` before `A`, `B` when
   `Y = N & C` is in block A).
6. Cascade helpers (MC−1…) are reserved before floating placement.

If routing fails, placement is retried with smaller fan-in budgets (40, 36, …, 20).

## F4.5a Clock networks

Each GCLK network n is a (clock pin, polarity) pair: config source select + invert bit (F2.6). The
polarity of a clock pin used by more cells (clock-enable users weigh 1000) gets the pin's own
network. With `share_clock_networks` (fitter `-strategy clock_networks on`; **default on in the
Verilog flow**, off in the vendor-compatible `fit150x` binaries) the other polarity gets a free
network (one whose own pin clocks nothing), sourced from the same pin and inverted, instead of a
PT clock (faster: tCOP vs tACOP, and frees PT4 for a clock enable). Silicon-verified (F2.9).

## F4.6 UIM routing

Per block, the set of sources (pads/feedbacks/dedicated inputs) read by its PTs is matched onto the
40 muxes (augmenting-path bipartite matching, trying each source's candidate muxes in ascending
order). For a single source the vendor also takes the **lowest** candidate mux (1502 184/187,
1504 749/757, 1508 2506/2526 cases). With several sources the vendor's assignment differs (e.g.
1508 `comb_5pt`: C→1, B→7, A→11, E5→21, D→33, not first-fit) — this is the main remaining cause of
non-identical 1508 output (**open**).

## F4.6a Capacity features (dense designs)

* **Foldback factoring (De Morgan).** A foldback is the block-local NAND of one product (the
  host's PT1, inverted), so an OR of literals is one foldback: `r1 + … + rk = !(!r1 & … & !rk)`.
  When a sum does not fit, products that share a common cube C and differ in one literal each are
  factored, `C&r1 # … # C&rk` → `C & FOLD`, `FOLD = !(!r1 & … & !rk)`; C may be any cube (the vendor
  only does |C| = 1: `L&a # L&b # L&!c` → `L & !(!a & !b & c)`), and the grouping that saves the
  most products wins (ties: |C| = 1, vendor-like). **Both polarities** are factored and the smaller
  result is kept (`L&a&(b+c+!d) + !L&(b⊕c⊕d)`: 7 products, complement 6, factored 5). Since
  2026-11-24; this made the user's `motioncontroller` fit (8/8 user designs).
* **Two-level foldback for special functions** (`special_foldback`, `-strategy special_foldback
  on`, off by default): a clock/CE/AR/AP/OE that is an OR of k ≤ 15 products, `p1 + … + pk =
  !(fold(p1) & … & fold(pk))`, becomes one foldback literal whose host's PT1 reads k foldbacks:
  k + 1 foldback PTs of the block instead of a buried node (saves its macrocell and UIM input;
  delay 2·tSEXP instead of tUIM + tLAD). Foldback expanders go on free macrocells, or (with
  `logic_doubling`, or when none is free) on a macrocell with logic whose PT1 product is moved to
  a free PT.
* **Logic doubling** (`-strategy logic_doubling on`, the vendor's option name): a buried
  register with a single-product D, a global clock, an optional CE product and AR (global and/or
  one product) shares the macrocell of a combinatorial pin output: D comes from PT2 through the fast
  path (`DMux` = 0), feedback taps the register (`FbMux` = 0), the output stays combinatorial
  (`OMux` = 1). Hosted T flip-flops become `D = !Q`, `CE = T` first (the vendor's conversion, seen
  for `TIMEBASE*`). Registers with the most inputs are paired first.
* **Routing alternatives.** An always-enabled output can be routed through its pad or its
  feedback (same value); the matcher prefers the pad (vendor choice) and falls back to the feedback.
* **Macrocell packing.** The D/Q group (F2.3.1: `DMux`, the shared `OMux`/dfast fuse, `FbMux`)
  lets one macrocell hold two functions:

  | packing | pin | register D | feedback | status |
  |---|---|---|---|---|
  | P1 comb output + buried register | XOR | PT2 (fast) | Q | logic doubling (vendor) |
  | P2 **input register** | input (OE off) | the pad | Q | `pack_input_registers` (`-strategy input_registers on`; default in the flow); silicon-verified |
  | P2+ **input register + PT donor** | input (OE off) | the pad | Q | the macrocell's PTs, sum and cascade still serve another cell: cascade helper (all 5 PTs lent to MC+1) or foldback host (PT1); flow default; **silicon-verified** (`bench/exp_inreg_cascade.py`) |
  | P3 output register + buried node = its D | Q | XOR | XOR | not implemented: **0 occurrences** in the 35 open-flow netlists, the SELFPROGRAMBOARD `.tt2` and the 9 user designs (`tools/flow/packing_scan.py`, 2026-11-24) |
  | P3′ comb output + buried register of the same value | XOR | XOR | Q | not implemented: 0 occurrences (same scan) |
  | P4 buried register + buried node on a free pin | Q (pin driven!) | PT2/XOR | XOR | not implemented (needs an unconnected pin) |

  The D/Q group allows no other independent pairing: D = PT2 forces pin = XOR (`OMux` = 1),
  and D = pad forces pin = Q, so an *output* register always has D = XOR or the pad. P3/P3′ wait
  for a design that needs them, and so do the P1 extensions (a hosted register with a PT5
  preset, or D through a foldback); `packing_scan.py` finds such designs.

  P2 places a D flip-flop or latch whose D is an input pin into that pin's macrocell (D from the
  pad: no PT and no UIM input for D, fast setup tFSU); the macrocell's PTs remain available.

  **Why the combinational stage cannot become a second, independent output** (and why CUPL
  never offers it). The fast path is selected by `DMux` = 0, and *which* fast path by the shared
  `OMux`/dfast fuse: `OMux` = 0 → D = pad, but `OMux` = 0 is also "pin data = Q". So a pad-input
  register forces the pin mux to Q, and the XOR output `x` can only leave through the
  feedback (`FbMux` = 1), which takes it from Q. The two outputs of the macrocell, pin and
  feedback, are both spoken for: one carries Q and the other x. Which of them is useful decides
  the cases:

  * `FbMux` = 0 (Q to the UIM, as P2): `x` has no exit, but everything *before* `x` is free.
    The sum can go to MC+1 through the cascade (`CasMux` = 1) and PT1 can be a foldback
    (`Pt1Mux` = 1). That is P2+: a register that costs no product term, in a macrocell that is
    still a full 5-PT donor. Conditions: the register needs no PT itself (pad D, a clock pin,
    reset at most from GCLR, no CE/preset: `pt_free_input_register`).
  * `FbMux` = 1 (x to the UIM as a buried node): Q reaches only the pin. The pin is the register's
    own input, so Q is useful only on a bidirectional pin whose OE is controlled: while the OE is
    off the register samples the bus, and while it is on the pin drives Q (the pad, and so D,
    then reads back Q: the register holds). That is a bus-hold register plus a free buried node.
    It has a narrow use and is not implemented.
* **XOR synthesis** (F4.6b, 2026-11-24; XOR semantics: F2.8 row 6, silicon). A cell's
  function `f` may be implemented as `g ⊕ p`: `p` one product in PT2 (`Pt2Mux` = 1), `g` in the
  sum, `Invert` = 1 (the XOR's B enable, so there is no free output inversion: an active-low cell
  uses the cover of `!g`, as `!f = !g ⊕ p`). `Design::find_xor_splits`:
  * **vendor pattern** (`-XOR_Synthesis on`, off by default as in the vendor): a combinatorial
    function that is the XOR/XNOR of two literals → PT1 = the first literal, PT2 = the second.
    Byte-identical on 94 of the 95 vendor XOR tests (feat corpus; before: 0). The last one differs
    only in which of two equivalent UIM multiplexers carries a source.
  * **general** (`xor_split`, ours; flow default): for cells of support ≤ 12 whose best plain
    cover has ≥ 3 products, try every literal and every product of the covers of `f` and `!f` as
    `p`; take the `p` minimising `1 + |g|` (`g = f·!p + !f·p`, minimised) if it beats the plain
    cover. The PT-demand estimate uses the split.
  Not with logic doubling (PT2 carries the hosted register's D), fast input registers, foldback
  readers or constants. Silicon: `designs/vxor.v` (16/16, `bench/exp_vxor.py`).
* **Shared foldback signals** (`Design::nodes_to_foldbacks`, from reading the vendor's
  SELFPROGRAMBOARD fit: its `XXL_` nodes). A shared combinatorial node of 2–4 products that
  feeds control functions becomes a foldback signal: `N` = the outer foldback of the products'
  foldbacks, `!N` = the product of those foldbacks; a single-literal product needs no foldback
  (its complement comes from the UIM). Readers keep their other literals in one PT (`CE = N & c0`),
  the node needs no macrocell and no UIM input, and the fitter hosts the foldbacks **in every
  reading block** (the vendor fit replicates `XXL_375` and its four inner foldbacks in all four
  blocks: 20 of its 47 foldbacks). Nodes read by foldback products are inlined into them only when
  the polarity used is one product. Foldback hosts can now be the reading cell itself. A foldback
  line exists in one polarity only: polarity choices never produce an inverted foldback literal
  (the PT encoder rejects one).
* **Partitioning placer** (2026-11-24, spec F9.4). When the vendor-like greedy placement fails
  at a fan-in budget (out of macrocells or UIM inputs), the same attempt is repeated with a block
  assignment for every floating buried cell: capacity per block (free macrocells minus cascade
  helpers), cost = UIM inputs over 40 summed over blocks, then the largest block, then the sum
  (foldbacks are block-local and free); greedy seed (most inputs first, least growth), then 20 000
  deterministic move/swap steps. The greedy placement then picks macrocells inside the assigned
  block, falling back to any block. Fitting designs are untouched (vendor byte identity unchanged
  on the feature, ptcol2 and pinroles corpora). It made the user's SELFPROGRAMBOARD `.tt2` fit
  (vendor: 64/64 MCs), matching the vendor JED and the `.tt2`.
* **UIM-aware positions** (2026-11-24). Which of a block's 40 UIM multiplexers can select a
  source depends on the *macrocell* the source sits in (1504: 3 muxes per feedback, 2 per pad;
  1508: 5 and 4), so a block whose input count is under 40 can still fail to match. In the
  partitioned attempt, after the greedy placement inside the assigned blocks, a local search moves
  and swaps floating buried cells within their blocks (block membership, and so every block's
  signal set, stays fixed) to minimise the unmatched sources summed over blocks (maximum bipartite
  matching per block, re-matching only the blocks that read a moved cell). It stops at zero, after
  500 moves without improvement, or at once if the start averages over 4 unmatched per block, and
  the result is kept **only if every block becomes routable**: otherwise the original positions
  are restored, so it never changes a placement it cannot fix and adds no fit attempt. It made the
  SELFPROGRAMBOARD Verilog port fit (4 → 0 unmatched; 60/64 MCs).
* **Repair loop.** If a block cannot be routed, a floating buried cell is barred from that block
  and placement is redone, up to 96 times; then the fan-in budgets tighten. The candidates are
  the block's own buried cells with the most inputs first, then the floating cells elsewhere that
  feed it (another MC puts their feedback on other UIM multiplexers). A cell is never barred from
  more than all blocks but two. The bars are a preference: the last-resort node placement
  ignores them. `Options::deadline` ends the retries.
* **Support check.** `fit_design` first rejects any cell whose functions use more than the 40
  UIM inputs of a block (`check_support`), naming the cell and its support size.
* **Dedicated inputs as plain inputs.** When the I/O pins run out, a floating input goes to an
  unused dedicated input pin (GCLK1, OE1, GCLR, GCLK2/OE2); its signal reaches every UIM.

## F4.7 Not implemented / open

* Vendor UIM assignment heuristic for multi-source blocks; vendor placement for dense designs
  (`motioncontroller` still does not fit: block A needs 36–38 UIM inputs where the vendor's
  placement needs 34).
* `XOR_Synthesis on` (vendor uses PT2 → XOR with `Pt2Mux` = 1, `Invert` = 1).
* Logic doubling, `Global_Fold`, `Soft_Buffer`, `Latch_Synthesis`, `fast_inlatch` options are accepted
  and ignored.
* Cascade chains longer than one helper: built up to 6 macrocells (F4.3); equivalence-checked, not fuse-identical with the vendor.

## F4.6c Block assignment against an exact solver (2026-11-25)

`fit-partition-bound` (a binary of `atf15xx-synth` behind its `ilp` feature: `cargo run --release -p
atf15xx-synth --features ilp --bin fit-partition-bound -- partition --netlist X.json -d PART` or
`--tt2 X.tt2 -d DEVICE`) fits a design, takes the block-assignment problem of that fit
(`fit::block_problem`: one item per used macrocell with its cells' UIM inputs as the fitter
counts them, its cascade helpers, the block it is pinned to; per block 16 macrocells minus
foldback-only hosts) and solves it exactly with HiGHS (`good_lp`): minimise the largest number
of distinct UIM inputs in any block. The model ignores UIM mux matching, cascade adjacency and PT
limits, so it is a relaxation: its optimum is a lower bound, and an optimum above 40 would
prove that no fit with these cells exists.

Results (70 open-flow fits in both mappings, SELFPROGRAMBOARD, the 8 user designs through the
vendor-compatible fitter):

* **optimal**: every counter, decoder, FSM, latch, mix and twoclk build; counter64 (39, all
  pinned); SELFPROGRAMBOARD from Verilog (38 = 38, the dense one); motioncontroller (42 = 42,
  the user's densest `.tt2`); blink.
* **headroom on easy designs**: parity (up to 20 inputs above the optimum on the 1508), mux16
  (10), tribus (10–11), shift32/1508 (15–16), spi/tribus/1502 (1), the user's spi (29 vs 20),
  system-controller (33 vs 29), shift-register (27 vs 20), rpi-pico-8088 (20 vs 12), the CUPL
  SELFPROGRAMBOARD (38 vs 36). The greedy placement fills blocks in order and stops at the first
  assignment within 40; it does not balance. That costs nothing while every block routes. It
  would matter only if UIM matching failed in a full block, which is when the partitioner runs.
* No fit we failed was a fit the model allows: the only failure (shift32 on the 1502) is a
  capacity failure (33 cells, 32 macrocells).

(The fan-in measure counts every signal a cell reads, global clocks included, as the fitter's
`fanin_ok` does, so some fits show 41–42 while routing fine.)

