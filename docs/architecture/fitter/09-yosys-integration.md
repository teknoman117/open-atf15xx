# F9 Deeper Yosys integration (exploration, 2026-11-24)

**Status: exploration and plan, not normative.** Written at the user's request ("explore how to
have deeper integration with yosys to enable better placement") after silicon tests showed what the
current flow gets wrong. Measurements below are reproducible with the commands given.

## F9.1 Today's interface

```
Verilog ─ Yosys (generic `synth`, flatten, dfflegalize) ─► fine-grained JSON ($_AND_/$_OR_/… + FFs)
        ─ Rust importer (atf15xx-synth: SOP covers, collapse ≤ 32 inputs / ≤ max_products,
          toggle and clock-enable recovery) ─► netlist ─ fitter (placement: fill block A first, then
          repair; PT allocation, cascade/foldback, UIM matching) ─► .jed
```

Yosys knows nothing about the ATF15xx; everything CPLD-specific happens after the JSON.

## F9.2 Findings

1. **Upstream Yosys has no CPLD support any more.** v0.71 has no `synth_coolrunner2`, no
   `synth_greenpak4`, no `abc -sop` (removed in 0.69) and no `flowmap`. Removed 2026-08-17..20 (PRs #6126,
   #6132, #6135, first release without: 0.69) as part of upstream's move to `abc9`, with the
   original authors' agreement (docs/research/yosys-cpld-removal.md). What remains: `extract_counter`
   (up/down counters, plain or async-reset only, no sync reset or load; nothing maps its cells any
   more), `$sop` cells with `read_blif -sop`, `yosys-abc`'s `cover` (incl. `cover -x`, XOR of
   products), `extract_fa`, `abc9` (LUT/box-oriented), `dfflegalize`.
   Deeper integration therefore means **our own code**, on either side of the Yosys boundary.
2. **Plugins work.** `tools/yosys-plugin/atf15xx_probe.cc` builds with `yosys-config --build` in
   `docker/yosys` and loads with `yosys -m`. The C++ API tracks the Yosys version (0.71 changed
   module names to `OwnedId`), so a plugin is pinned with Yosys. `pyosys` is not built in our image.
3. **The coarse netlist carries exactly the structure the CPLD mapper needs, and the fine netlist
   has lost it.** After `synth -run :fine`, `vblink` is 6 word-level cells (`$alu`, `$eq`, `$mux`,
   `$not`, `$adff`, `$adffe`) and `spi_controller_mode0` is 64. After fine mapping the counter is a
   Brent–Kung carry-lookahead gate network that our importer must collapse back. Our JSON keeps only
   `src` attributes (122/123 cells) and `LOC` on nets; flattening drops the hierarchy.
4. **Depth is a mapping decision, and it fails on silicon.** The Verilog blink kept 16 carry nodes
   (4 array passes) and **did not run at 66.6 MHz**, while 1- and 2-pass versions did (log 06).
   For a counter bit the right encoding is per bit and CPLD-specific: T = carry (1 product),
   T = carry ⊕ wrap (the macrocell XOR with PT2), or T = carry + c·wrap (2 products). The importer's
   toggle analysis budget (64 → 1024, 2026-11-24) recovers all but one bit; with
   `--max-products 15` the design is single-level and runs at 66.6 MHz. XOR synthesis (PT2) would
   remove the remaining 3-cell cascade.
5. **Placement has large headroom.** `tools/flow/placement_headroom.py` (decode a fitted JED, then
   move only buried cells between blocks by local search, 16 MCs per block):

   | design | worst-block UIM inputs, ours | after local search | blocks used |
   |---|---|---|---|
   | `spi_controller_mode0`, ATF1504AS | 29 (29/27/27/27) | 25 (25/25/25/25) | 4 → 4 |
   | `spi_controller_mode0`, ATF1508AS | 33 (33/25/25/23/11) | 22 (22/16/15/15/14/14/14/13) | 5 → 8 |

   (Approximate: ignores PT capacity, cascade adjacency and foldback hosts.) The placer's
   "fill block A first" leaves blocks empty and overloads A. That is also why `motioncontroller`
   doesn't fit: block A needs 36–38 inputs, the vendor 34.

## F9.3 Options

| option | what | for | against |
|---|---|---|---|
| A. Smarter Rust behind the fine JSON (today) | better collapse/encoding in the importer | no Yosys coupling; already paid off (finding 4) | word structure must be rediscovered from gates; budgets |
| **B. Cut at the coarse level** | Yosys runs `proc; opt; fsm; memory; wreduce; alumacc; share; opt` and writes coarse JSON; our Rust maps `$alu/$add/$sub` (counters → T/XOR per bit), `$eq/$ne` (one wide product), `$mux/$pmux`, `$reduce_*`, `$logic_*`, `$shift*` with the macrocell cost model, and asks Yosys to `techmap` only the cells it does not handle | keeps word structure (counter = counter, compare = one PT); words and `src` give placement clusters; no C++ | a Rust mapper for each coarse cell type; must match Yosys semantics exactly (equivalence-checked anyway) |
| C. Yosys plugin `atf15xx` | `synth_atf15xx` inside Yosys: counter extraction for up-counters with enable/wrap/load, SOP mapping with 5-PT/40-input limits (the removed `abc -sop`, CPLD-shaped), and optionally `atf15xx_place` calling our Rust fitter through a C ABI so synthesis and placement iterate in one process | deepest integration: re-collapse or duplicate a node when a block overflows its 40 inputs; usable by other Yosys users | C++ against an API that changes per release; Rust FFI; more build machinery |
| D. pyosys | drive passes and read RTLIL from Python | quick experiments | not built here; slow; still not a mapper |
| E. ABC9 / custom ABC library | depth-optimal Boolean restructuring | good restructuring | LUT cost model cannot express "≤ 5 products, ≤ 40 inputs per macrocell" |
| F. nextpnr-generic | reuse its SA/HeAP placer with a custom UIM cost | mature placer | PT allocation/cascades don't fit its model; heavy for a 2–8-block problem |

## F9.4 Placement plan

Placement on ATF15xx is a **partitioning** problem: assign cells to 2/4/8 blocks of 16 macrocells
so that every block needs at most 40 distinct UIM inputs (hard) and the UIM matching succeeds,
subject to pin locks, cascade adjacency, foldback hosts in the reading block, and PT counts.

1. Replace "fill block A, then repair" with a partitioner: seed by clustering (cells of one Yosys
   word / `src` location together, since counter bits read each other; then by shared fan-in), then
   Fiduccia–Mattheyses or annealing on the per-block distinct-input counts (max, then sum), then the
   existing exact UIM matcher per block. Headroom in F9.2 says this alone should fit
   `motioncontroller`.
2. Feed Yosys structure into it: with option B the importer knows words and source locations; with
   hierarchy kept (`hierarchy` without `flatten`, or `flatten -wb`/`hdlname`), module boundaries are
   natural clusters.
3. Close the loop between mapping and placement: when a block overflows, ask the mapper for an
   alternative (keep a node instead of collapsing it, or duplicate it into the reading block). This
   needs the mapper in-process (option B or C).
4. Make it timing-driven with spec F8: a static timing report per register path, a `--fmax` target,
   and depth-vs-area choices (cascade vs node vs XOR) by slack.

## F9.5 Recommended roadmap

| step | change | acceptance |
|---|---|---|
| R1 (done 2026-11-24) | toggle-analysis budget 1024 | vblink single-level with `--max-products 15`, runs at 66.6 MHz on silicon; corpus unchanged |
| R2 (done 2026-11-24, F4.6b) | XOR synthesis (PT2 ⊕ sum) in netlist, fitter and importer | vendor oracle met: its XOR tests 94/95 byte-identical. **Not met:** vblink single-level at the default `--max-products`: its wrap compare's complement is ~20 products, which no XOR split removes; `--fmax` finds the wider-sum fit instead (F8.6) |
| R3 (done 2026-11-24, F4.6a) | partitioning placer (F9.4.1) + UIM-aware positions | `motioncontroller` fits; `spi_controller_mode0`/1508 worst block ≤ 25 inputs; corpus PROVEN, MC counts not worse |
| R4 (done 2026-11-25, F9.8) | coarse-level cut (option B) for `$alu`/`$eq`/`$mux`/`$pmux` — done instead as exact BDD analysis of the gate netlist + carry-chain factoring | **met:** no carry-lookahead nodes left in the counters (counter16 20 → 16 on 1502/1508, counter64 70 → 65 and 87 → 65), corpus 34/34 PROVEN, 40-bit chain on silicon 36/36 |
| R5 (done 2026-11-24, F8.6) | static timing report + `--fmax` (F8.5) | predicts the vblink 4-pass failure and the 1-pass success; silicon check with the 66.6 MHz oscillator and Timer1 clocks |
| R6 (done 2026-11-24, F9.7) | Yosys plugin (option C) once R2–R5 show what must live inside Yosys | `yosys -m atf15xx.so -p synth_atf15xx` produces the same netlist as the Rust path |

Recommendation: **B before C**. The cut at the coarse level gives most of the benefit (structure,
depth, clustering) without C++ coupling. A plugin becomes worthwhile once mapping and placement
must iterate inside Yosys, or to offer `synth_atf15xx` to other Yosys users.

## F9.6 What a Yosys plugin would bring (2026-11-24, with a working proof of concept)

**Proof of concept:** `tools/yosys-plugin/atf15xx.cc` adds `synth_atf15xx` to Yosys. It runs the
flow's synthesis script (obtained from Rust through a C ABI crate, `atf15xx-ffi`, removed 2026-11-25), serialises the netlist
with Yosys's JSON backend *in memory*, and calls the Rust fitter in-process. Its JED equals the CLI
flow's byte for byte (except the timestamp) and is PROVEN against the Verilog. The plugin is a thin
C++ shell (≈ 90 lines); everything CPLD-specific stays in Rust.

Benefits, roughly by value:

1. **One command in standard Yosys flows**: `yosys -m atf15xx.so -p 'synth_atf15xx -part …'`, like
   `synth_ice40`; users and other tools (FuseSoC, Edalize, SpinalHDL/Amaranth backends) can target
   ATF15xx without a separate driver. Upstream removed the CPLD flows; a plugin is the supported way
   to add one back without upstreaming C++.
2. **Mapping with structure**: the pass sees RTLIL before fine mapping: word-level `$alu`, `$eq`,
   `$mux`, `$pmux`, FSMs, memories, hierarchy and attributes. It can hand the Rust mapper the
   coarse netlist (F9.3 option B) without a second JSON dialect, and keep `src`/`hdlname` for
   placement clustering.
3. **Mapping–placement iteration in one process**: when a block overflows (UIM matching, foldback
   hosts), the fitter can ask for a different mapping (keep a node, duplicate it per block, use a
   foldback signal) and retry in milliseconds, without re-running Yosys. This is the loop the
   SELFPROGRAMBOARD Verilog port needs.
4. **Yosys's own engines on demand**: SAT/`equiv_*` to prove a local rewrite correct before using
   it, ABC (`cover -x` for XOR-of-products, `abc9` with box delays for timing-driven restructuring),
   `opt`/`share` after our rewrites.
5. **Back-annotation into Yosys**: the pass can write the mapped netlist back as RTLIL cells
   (macrocells, PTs, foldbacks with LOC attributes), so `stat`, `write_verilog` (post-fit
   simulation) and `sta`-style reports work on the real implementation.
6. **Distribution**: one `.so` per Yosys version, alongside a crates.io package.

Costs: the C++ API changes between Yosys versions (seen in 0.71: `OwnedId`), so the plugin is
built and tested per pinned Yosys; two languages and an FFI boundary (kept to a C ABI with owned
strings; `catch_unwind` at the boundary); larger binaries (10 MB with the fitter linked in).

Recommendation (unchanged): keep the Rust core authoritative and the plugin thin; do option B (the
coarse cut) **through the plugin** rather than a second JSON format, and add the
mapping–placement loop (item 3) when the position-aware placer lands.

## F9.7 Deep integration through `cxx` (2026-11-24, implemented)

The user's proposal: integrate with Yosys through the [`cxx`](https://cxx.rs) crate instead of a
JSON interchange. The F9.6 proof of concept had a thin C++ pass that serialised the netlist with
the JSON backend and handed the *text* to Rust over a C ABI. Rust could only receive, never act
on the design. `atf15xx-synth`'s `plugin` module (feature `yosys`, built as a `staticlib` against the
Yosys headers by `tools/yosys-plugin/build.sh`) replaces that with a `#[cxx::bridge]`.

| direction | what crosses the bridge | used for |
|---|---|---|
| Rust → Yosys | `run_pass(design, "…")` (`Pass::call`) | Rust runs `hierarchy` and the synthesis script line by line (the script stays single-sourced in `atf15xx-synth`) |
| Yosys → Rust | `read_design(design) -> Vec<ModuleRec>`: ports, cells (type, parameters, attributes, port directions, connections), wires, as shared structs | the netlist without a text round trip; `netlist_value` rebuilds the Yosys JSON *schema* in memory, so the existing importer (`yosys::import_value`) is reused unchanged |
| Rust → Yosys | `annotate(design, module, Vec<Annot>)` | the placement written back: `atf15xx_mc` / `atf15xx_pin` (`…_<bit>` for vector bits) on every placed wire |
| C++ → Rust | `synth_and_fit(Pin<&mut Design>, &FitArgs) -> Result<String>` | the pass body; a Rust `Err` becomes a C++ exception and then `log_error` |

`read_design` mirrors `backends/json/json.cc` exactly: `design->sort()`, a per-module `SigMap`,
bit ids from 2 assigned in the order ports → cell connections → wires, constants `0 1 x z`, and
`write_parameter_value`'s encoding with `-compat-int` (strings that look like bit patterns get a
trailing space). `synth_atf15xx -check-json` builds the netlist both ways in the same process
and fails unless they are equal.

**Checked:** `-check-json` passes on all 34 open-flow corpus builds and on SELFPROGRAMBOARD.
Every JED is fuse-identical to the CLI's (`tools/yosys-plugin/check_corpus.sh`). The annotations
are queryable (`select -list a:atf15xx_mc`: 88 attributes on SELFPROGRAMBOARD).

**Why `cxx` rather than the C ABI:** typed shared structs instead of hand-encoded strings, no
manual `free`, `Result` ↔ exceptions at the boundary, and `Pin<&mut Design>` makes the
mutation explicit. It is also two-way, which the R4 loop needs. Costs: the crate compiles C++
against one Yosys version (0.71's twine API differs from earlier ones), so it is a standalone
crate outside the main workspace, built in the Yosys image. The plugin is 12.7 MB with the flow
linked in.

**Open issue found:** results depend on the path a source was read by. Yosys's internal net
names contain it (`$/work/d.v:4$xor$4_Y`), and the importer's name choice and the fitter's
order follow names. Identical results need `read_verilog -sv <absolute path>` as the CLI uses.
Stripping the directory from the importer's names is not enough (tried 2026-11-24: parity still
differs). The path is also in Yosys's *cell* names (`$…/d.v:91$and$76`), which set
`design->sort()`'s order, and with it the bit numbering and which wire name `opt_clean`
keeps. **Fixed at the source (2026-11-25):** the order flips because path-carrying names
(`$/work/…`, `$crates/…`) sort against Yosys's path-free ones (`$auto$…`) differently depending
on the path's first character, and that sort happens inside Yosys. So the CLI now runs Yosys in
the sources' common directory and reads them by relative path (`flow::synth`). The same design
named by an absolute or a relative path gives byte-identical JEDs. In the plugin, read sources
by file name from their directory for CLI-identical results (`check_corpus.sh` does).

**What it enables next (R4 and beyond), in order:**

1. **Typed netlist IR** (done 2026-11-26). `atf15xx-synth/src/ynet.rs` holds the Yosys netlist
   schema as structs (`Netlist`, `Module`, `Port`, `Cell`, `NetName`, `Bit`, `Const`), filled by
   `serde::Deserialize` from a JSON file or directly from RTLIL by the plugin (`plugin::netlist`,
   replacing `netlist_value`). The importer (`yosys::import_netlist`), the word-level lowering
   (`wordmap`) and the flow (`flow::fit_netlist`) read only these types; `-check-json` compares the
   two producers' typed netlists. Maps are `BTreeMap`s, the iteration order `serde_json`'s default
   map had, so results are unchanged: the 34 corpus JEDs and the four examples are byte-identical
   to the `Value`-walking importer's (apart from the examples' `Created:` date), and the plugin is
   fuse-identical to the CLI on all 34 with `-check-json` passing.
2. **Coarse cut (R4).** `read_design` already carries any cell type and its parameters. Call it
   after `synth -run coarse` and map `$alu`/`$eq`/`$pmux`/FSM cells in Rust with their structure
   (carry chains as cascades and XOR, comparators as single wide products), instead of after
   gate-level flattening.
3. **Mapping–placement loop.** Rust can now run Yosys passes between fit attempts (re-run
   `abc`/`opt` on a block's logic, keep or duplicate a node) without leaving the process.
4. **Write back the implementation.** Instead of attributes only, write macrocell / PT /
   foldback cells (`$__ATF15XX_MC` with the PT covers as parameters) back into the module, so
   `stat`, `write_verilog` (post-fit simulation inside Yosys) and `sta`-style passes see the real
   implementation.

## F9.8 R4: word structure recovered exactly instead of cut out of Yosys (2026-11-25)

**Problem.** R4 asked for counters and compares without Yosys's carry-lookahead (`$lcu`)
nodes. The lookahead nodes survived because toggle recovery (`opt::recover_toggles`) analyses
the next-state function as a flattened sum of products. For an up/down counter bit that cover
is exponential: counter16's bits 12–15 did not flatten within 1024 products, though their
toggle functions are 3 products (load, count up, count down). The bits stayed D flip-flops on top of a
lookahead tree (counter16: 4 extra nodes). Wider counters also exceed a block's UIM inputs
(bit 40 of counter64 toggles on 40 lower bits), which the lookahead tree "solved" with 6–23 nodes.

**Instead of a coarse cut**, the structure is recovered exactly from the gate netlist:

1. **BDD toggle analysis** (`opt::BddNet`, `opt::bdd_toggle`, crate `biodivine-lib-bdd`). When the
   SOP flattening exceeds its budget, the next state is built as a BDD through the buried nodes
   (linear in size for carry and compare chains), with the clock enable folded in
   (`D' = e·D + !e·Q`). The toggle function is `T = D' ⊕ Q`, and its cover comes from the BDD's
   optimised DNF, made irredundant by `Sop::minimize`. It is used only where flattening fails,
   so every design that converted before is unchanged.
2. **Carry-chain factoring** (`opt::factor_wide`). A cell reading more than `MAX_SUPPORT` (32)
   signals has its longest product split. Its most widely shared literals (up to 31) become a node
   (`carryN`), substituted into every product that contains them; this repeats until all cells
   fit. For a counter this is a chain, `carry0 = en·!clr·q0…q28`, then `T_n = carry0·q29…q(n-1)`,
   instead of a lookahead tree.

**Results** (open-flow corpus, all 34 builds PROVEN, nothing worse):

| design | part | before | after | fmax (-10) |
|---|---|---|---|---|
| counter16 | ATF1502AS-PLCC44 | 20 | **16** | 100 MHz |
| counter16 | ATF1508AS-PLCC84 | 20 | **16** | 100 MHz |
| counter64 | ATF1508AS-PLCC84 | 70 | **65** (64 + 1 carry node) | 43.5 → **100** MHz |
| counter64 | ATF1508AS-TQFP100 | 87 | **65** | 23.8 → **55.6** MHz |

Silicon: `tools/silicon/designs/vcnt40.v` (40-bit counter, 41 MCs: 39 T flip-flops, 2 output
registers, 1 carry node), loaded just below the 40-bit wrap and counted across it: all 40
registers read back over boundary scan, 36/36 checks (`bench/exp_vcnt40.py`).

**Why not the coarse cut after all.** Reading `$alu`/`$eq` cells (possible now through
`read_design`, F9.7) would need a mapper per coarse cell type, matching Yosys's semantics exactly.
The BDD route reaches the same covers from any netlist, Yosys-generated or not, and is checked by
the same equivalence proofs. The coarse cut remains available for structure the gate netlist
cannot show, such as word grouping for placement clusters.

**Follow-ups** (not part of R4): the typed netlist IR (F9.7 item 1, done 2026-11-26), the mapping–placement loop
inside the plugin (F9.7 item 3), and a canonical cell order so results do not depend on the
source path (F9.7 open issue).

## F9.9 Tighter integration: word-level mapping, both mappings in one process, a failure-driven loop (2026-11-25)

The user asked what tighter integration with Yosys would bring if the plugin skipped the gate
netlist, whether the fitter and flow stages should merge, and whether fitter primitives should
move into a library. The vendor-interchange modes stay as they are: our frontend → `.tt2` →
vendor fitter (`--emit-tt2`, `<top>.vendor-cmd`), and CUPL `.tt2` → our fitter (`fit150x`).

**Where the gate netlist loses** (measured on the corpus before changing anything). Time is not
the issue: everything fits at strategy 0 in under 2 s, apart from one infeasible build. Macrocells
are. Yosys's techmap turns `$reduce_xor` into a tree of 2-input XORs (parity: 10 MCs, about 6
achievable), and `$eq`/`$lt` into `$alu`/`$lcu` structure that hides the per-bit `a_i ⊕ b_i`
both share (mux16: 18 MCs).

**Word-level mapping** (`--map words`; `atf15xx-synth/src/wordmap.rs`, script
`data/synth_atf15xx_words.ys`). Yosys stops at its coarse cells. Rust lowers `$eq/$ne/$lt/$le/$gt/$ge`
(shared `a_i ⊕ b_i` nodes, single products against constants), `$add/$sub/$neg` (carry *products*
for `+1`, majority ripple otherwise), `$reduce_*` (XOR trees of 4-input groups: one macrocell each
with XOR synthesis), `$mux`, `$pmux`, `$shiftx` (one product per case), bitwise and logic
operators, into `$sop` cells. These are the fine-cell form the importer already reads, so
storage, tri-states, names and every later pass are reused. Cell types it does not lower go to
Yosys's techmap, named in `wordmap::UNSUPPORTED`. Corpus (34 builds, all PROVEN): mux16 18 → 11
(1504 and 1508), parity 10 → 8 (all three parts); counter64/TQFP100 65 → 67 and spi 35 → 36; total
579 → 562 macrocells. latchsr's simulation, which fails in gate mode, passes.

**Both mappings, the better fit kept** (`--map auto`, the new default for the CLI and the
plugin; corpus 579 → **559** macrocells, nothing worse, every fitting build PROVEN, the plugin
fuse-identical to the CLI on all 34). Neither mapping wins everywhere, so both are fitted and `flow::better` keeps the
better: the timing target first, then fewer macrocells, then fmax. The plugin does this in one
process from the same RTLIL: `design -save` after `hierarchy`, each mapping's script from
`design -load`, each result saved, and the kept design reloaded for the annotations. The CLI
runs Yosys once per mapping.

**Merging the flow and the fitter: a failure-driven strategy loop.** They already run in one
process, so what merging adds is acting on the fitter's reasons. Strategies are now a struct
(`Strategy`, with toggle recovery and special foldbacks as knobs, which the old 6-tuple could
not hold). A fit that fails for lack of macrocells, pins or product terms moves the
cell-reducing strategies to the front of the queue: vendor-style collapse with foldback signals,
with and without toggle recovery. After the first successful fit, one alternative (the
vendor-style collapse, if not tried yet) is fitted and the smaller fit kept. Effect:
SELFPROGRAMBOARD fits **with default options** (60 MCs, 33 s). It used to need
`--no-toggles --option special_foldback=on`, and with defaults it failed after 133 s.

**A bug found on the way:** the alternative ran the vendor-style collapse on parity, and
`Design::collapse_comb_nodes` flattened the 16-input XOR into 2^15 products, hanging the fitter.
The collapse now keeps a node when a reader's cover would exceed 256 products; a macrocell with
cascades holds at most about 80, so no placeable result is lost. Vendor byte identity is unchanged
(feat 1591/2146/1777, ptcol2 246/1003/3522, pinroles 180/876/856; user designs 8/8). Separately,
`timeout` around `docker run` does not stop the container; long jobs now put the timeout inside
the container.

**Fitter primitives as a library: not needed yet.** The integration so far calls the whole
`fit_design`; nothing in the plugin needed a finer fitter API. Two changes would make one
worthwhile, in this order: (1) typed fit errors instead of the string matching the strategy
loop used (**done 2026-11-26**: `FitError { kind, msg }`, the kind set where the error is raised:
`Capacity`, `InputPins`, `Routing`, `Cascade`, `Netlist`, `Other`; `is_capacity()` covers the first
two; the flow queues its capacity strategies on `Capacity`; corpus and examples byte-identical); (2) cost queries for mapping decisions (block
fan-in of a candidate cell set: `uim_unmatched`; PT demand of a cover: `pt_demand`) exposed from
`atf15xx-fit`, once a mapper wants placement-aware choices. `atf15xx-fit` is already a library
crate, so this is an API, not a new crate.

## F9.10 Against the vendor on the dense design (SELFPROGRAMBOARD, 2026-11-25)

The user's request: compare our synthesis and fitter with WinCUPL + the vendor fitter on the
dense SELFPROGRAMBOARD (ATF1504AS-PLCC44). All four combinations were measured with the same
tools (`tools/flow/jed_stats.py`: resources decoded from the JED, timing by `atf15xx-sim timing`
at -10) and proven against a reference:

| synthesis + fitter | MCs with logic (+ foldback-only) | PTs | foldbacks | fmax | worst tPD | time | equivalence |
|---|---|---|---|---|---|---|---|
| WinCUPL + vendor fitter | 64 | 191 | 47 | 50.0 MHz | 14.0 ns | ~5 s + 1.5 s | the reference (works in the user's hardware) |
| WinCUPL + our fitter (`fit1504`) | **60** | **180** | 1 | **55.6 MHz** | **10.0 ns** | 3.3 s | BOUNDED 22 + sim 4000 vs the vendor JED |
| Verilog + our flow (`--map auto`, gates kept) | 60 (+3) | 183 | 20 | 33.3 MHz | 10.0 ns | 62 s | BOUNDED 25 + sim 4000 vs the Verilog |
| Verilog + our flow (`--map words`) | 61 (+2) | 186 | 23 | 32.3 MHz | 10.0 ns | not timed | BOUNDED 25 + sim 4000 vs the Verilog |
| Verilog + our synthesis + **vendor fitter** (`--emit-tt2`) | does not fit (block fan-in 40/40 in all four blocks) | | | | | 1.3 s | |

What this says:

* **Our fitter beats the vendor's on the vendor's own netlist**: 4 macrocells fewer, 11
  product terms fewer, 46 foldbacks fewer (the vendor replicates its foldback networks per
  block), faster (55.6 vs 50 MHz, tPD 10 vs 14 ns: foldbacks cost tSEXP).
* **Our synthesis is not yet as good as CUPL's**: the same fit size (60 + 3 foldback hosts), but
  slower (33 MHz): the gate-level netlist has 289 products before fitting against CUPL's 132,
  and our fitter recovers the area (foldback signals, placement) but not the depth. The
  hand-written CUPL equations are flatter.
* **The vendor fitter cannot fit our netlist**: our `.tt2` is written before our own
  collapse/foldback conversion (which `.tt2` cannot express) and is too wide for it. The
  vendor-interchange mode still works where the netlist is less dense.

## F9.11 Execution performance (2026-11-25)

Profiled first (`perf record` on a release build with debug symbols, SELFPROGRAMBOARD through
`fit-json`, 16 cores). At the start the fit took 26 s, of which 25.2 s were 26 complete fit
attempts of about 1 s each, single-threaded: 78% in `logic::tautology` (the irredundant pass's
containment check scanned all 64 bit positions over all cubes at every recursion step) and
about 30% (on another sample group) in SipHash over `Signal` names, from the partitioner's
per-block `HashMap<Signal, usize>` counts, updated on every one of 8 × 20 000 moves.

| change | SELFPROGRAMBOARD fit | fuses |
|---|---|---|
| start | 26.0 s | |
| `tautology`: binate variables from whole-word masks, only those scanned | no wall-time change on its own (its share of samples 78% → 33%: the partitioner then dominated) | identical |
| partitioner: signals interned once, dense per-block count arrays, no clone per move | **8.9 s** | identical |
| strategies speculatively in parallel (rayon), first try: the next 16 of the queue | 18.0 s wall, 175 s CPU: slower (the batch waits for its slowest strategy, mostly ones the search never needs) | identical |
| speculation limited to strategies known to be needed (the pending capacity strategies of a dense design, all of which are fitted) | 7.5 s | identical |
| typed failures (`FitError::kind()`); after a capacity failure the fitter skips the budget levels that only lower the fan-in limit (a tighter limit cannot create macrocells) | 6.8 s | identical |
| partitioner's cell → block map a `Vec` instead of a `HashMap` | 6.3 s | identical |
| memoised covers: `Sop::minimize` / `complement` cached per thread (`rustc-hash` keys; hit rate 50–65%) | 6.1 s | identical |
| `fanin_ok` (run for every candidate macrocell) from memoised UIM inputs as interned ids and a reusable bitset, instead of rebuilding string sets from every placed cell | **2.6 s** | identical |
| strategy transforms cached: the netlist passes up to the split depend only on (max products, toggles, enables, ties), computed once per key and shared between strategies and threads (`OnceLock`) | **2.3 s** | identical |

Also:

* **Fail fast** (`fit_design`): inside the fitter only logic doubling makes cells share a
  macrocell, and each pair needs an eligible register and a combinational output host, so
  `cells − min(eligible registers, hosts)` macrocells are needed at least. If that exceeds the
  device, the attempt fails at once. shift32 on the ATF1502AS (33 cells, no doubling possible:
  every bit's D has two products through the load mux) now fails in 10 s end to end (Docker and
  two Yosys runs included), instead of 75 s.
* **Both `--map auto` mappings concurrently**: two Yosys processes side by side in the CLI, and
  the two fits in parallel (rayon) in the CLI and in the plugin (Yosys itself stays sequential:
  it is not thread-safe).
* The strategy loop's decisions are unchanged: the driver is sequential and consumes cached
  outcomes, so the chosen fit is the same as without parallelism. The only exception is the
  wall-clock fit-time budget.

Whole regression: vendor byte identity unchanged on all three corpora, user designs 8/8, the
open-flow corpus unchanged (559 macrocells, 34 PROVEN) at 298 s of flow time against 396 s
(builds run four at a time, so the per-build gain is partly hidden), the plugin fuse-identical
to the CLI on all 34.

Whole corpus after these: flow time 298 s → **47 s** (396 s at the start of F9.11), every
result identical.

Measured split after these (`ATF15XX_FLOW_TIMING=1`): netlist passes 0.01–0.4 s per strategy,
successful fits 0.2 s, failing fits about 0.9 s. Overall SELFPROGRAMBOARD 26 s → 2.3 s (11×). An
ILP pre-check of block feasibility for dense designs remains possible; the cheap bound already
catches the corpus's only infeasible build.

## F9.12 Wide compares from pieces (2026-11-26)

**Problem.** The word-level mapping lowered `a < b` as one flat cover `Σ_i !a_i·b_i·Π_{j>i}(a_j ≡ b_j)`
over an XOR node per bit pair, and `a == b` as one product of them. Such a cell reads 3 signals per
bit, so beyond about 13 bits no block can deliver its inputs (40 UIM inputs). Measured on
registered compares (`.scratch/mploop/edge.py`): the placement failures were blocks 10 or more
inputs over 40 (`docs/plan/partitioning.md` §5), not a partitioning weakness.

**Form.** Compares wider than `WIDE_COMPARE` = 8 bits (`wordmap.rs`) are built from pieces: per
piece c a less-than `l_c` and an equality `e_c`, then `a < b = Σ_c l_c·Π_{d>c} e_d` and
`a == b = Π_c e_c` (the `e_c` shared when both compares read the same operands). Two piece forms
(`ChunkForm`):

* `CHUNK_XOR4`: XOR nodes per bit pair, 4-bit pieces (`e_c` one product, `l_c` four);
* `CHUNK_FLAT3`: no XOR nodes, 3-bit pieces flat over the operand bits (`e_c` 8 products, `l_c` 7,
  each one cascade helper; input-register macrocells lend their free PTs).

Neither wins everywhere (word mapping alone, 8–38-bit registered compares, MCs): flat pieces save
a macrocell per bit and fit 20 bits on the ATF1504AS (XOR4 does not); XOR4 fits 38 bits on the
ATF1508AS (flat pieces do not). So the flow lowers a netlist with wide compares in both forms,
fits both (in parallel, each under the fit budget) and keeps the better (`flow::better`; the first
on a tie); designs without wide compares take the single path unchanged (corpus byte-identical).

**Results** (full flow, both mappings, every fit PROVEN against its Verilog):

| registered compare | before | now |
|---|---|---|
| 10 bits, ATF1502AS-PLCC44 | 31 MCs | 27 MCs (flat) |
| 16 bits, ATF1504AS-TQFP100 | 54 MCs | 44 MCs (flat) |
| 18 bits, ATF1504AS-TQFP100 | 60 MCs | 49 MCs (flat) |
| 20 bits, ATF1504AS-TQFP100 | does not fit | 54 MCs (flat) |
| 26 / 30 bits, ATF1508AS-TQFP100 | 87 / 101 MCs | 71 / 81 MCs (flat) |
| 34 / 38 bits, ATF1508AS-TQFP100 | does not fit | 112 / 127 MCs (XOR4) |

Regression design: `atf15xx-synth/tests/designs/cmpwide.v` (20 bits, both TQFP100 parts, 54 MCs,
PROVEN). fmax is lower than the flat cover's where that fitted (two logic levels: 35–43 MHz vs
38–55 MHz); a timing target (`fmax` / `--fmax`) makes `better` prefer the faster fit.

