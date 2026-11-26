# 02 — Vendor fitters → fuse semantics → `atf15xx-*` fitter crates

## Status

**2026-11-25 (own JED layout)** — The fitter writes JEDs in our own layout (`jed::write`, spec
F5.0); `write_vendor` (byte-exact vendor layout and NOTE text) is gone. Conformance with the
vendor fitter is now fuse identity (same fuses, security fuse, correct checksum); the 95 vectors
were rewritten in our layout with unchanged fuses. All match counts unchanged (journal).

**2026-11-24 (timing)** — Timing data (spec F8): speed-grade tables (all 3 families ×
-7/-10/-15/-20/-25, 53/51 parameters each, from the datasheets and the fitters' SDF), the delay composition of the
vendor SDF writer (all 6,895 delays of 260 vendor SDFs explained, oracle), and datasheet
cross-checks. No timing analyser yet
(F8.5 says what it must do).

**2026-11-24** — The `.tt2` path is complete for the user's finished designs: both `blink`s,
`atf1508as-example/shift-register` and `spi` fit with our fitters and match their `.tt2` in
simulation; so do `input`, `rpi-pico-8088` and the 1508 `system-controller`. Only
`motioncontroller` (which the user says is unfinished) does not fit (UIM congestion in block A).
Synthetic corpus byte-identical rates unchanged (1502 96.8 %, 1504 98.0 %, 1508 91.2 %). All vendor
JEDs of the user's designs now match their `.tt2` under the model (the 1508 gap was a tied-high
latch enable, `ClockSource::AlwaysHigh`). Next: Yosys-JSON front end; vendor-like UIM heuristic.

## Scope and ownership

Owns: `atf15xx-arch`, `atf15xx-fit`, `fit1502`, `fit1504`,
`fit1508`; `tools/fuzzers/fitter-*/`; `tools/fuzzers/common/fitter_oracle.py`, `tt2gen.py`,
`verify_tt2.py`, `jedsim.py`; `docs/architecture/fitter/`; `docker/Dockerfile`, `docker/run.sh`.
Does not own `atf15xx-sim`, `tools/sim/`, `docs/architecture/sim/` (simulator workstream,
which uses `atf15xx_arch::decode` read-only), nor `external/prjbureau`, `docs/prjbureau/`,
`tools/prjbureau/` (prjbureau workstream).

## Results

| result | numbers | reproduce |
|---|---|---|
| JED writer (own layout, F5.0) | fuses round-trip for all 95 vectors; every field < 200 bytes. (Until 2026-11-25: the vendor-layout writer re-emitted 3000/3000 vendor JEDs byte-identical; removed) | `cargo run --release -p atf15xx-fit --example jed_rewrite -- FILE.jed…` (rewrites in place, asserts the fuses survive) |
| fitter vs vendor, feature corpus | 1502 1591/1615, 1504 2146/2153, 1508 1777/1915 fuse-identical (2026-11-25) | `python3 tools/fuzzers/fitter-conformance/verify_tt2.py --bin-dir target/release .scratch/oracle/feat` |
| fitter vs vendor, source×block corpus | 1502 246/251, 1504 1003/1013, 1508 3522/3550 | `verify_tt2.py .scratch/oracle/ptcol2` |
| fitter vs vendor, node-number corpus | 1502 64/64, 1504 128/128, 1508 4/256 | `verify_tt2.py .scratch/oracle/dir` |
| fitter vs vendor, pin experiments | 1502 401/403, 1504 1884/1884, 1508 1850/2255 | `verify_tt2.py .scratch/oracle/{pinmap,pinroles,dedic,gclk3}` |
| non-identical synthetic cases, functional | 59/59 sampled equivalent to vendor JED | `jedsim.py equiv vendor.jed ours.jed --device D` |
| vendor JEDs vs their `.tt2` (model check) | user 1504 `input`, `motioncontroller`, `rpi-pico-8088`, `spi`: match; 95/95 XOR designs match; 1508 `system-controller`: 45 mismatching pin samples (open) | `jedsim.py golden X.jed X.tt2 --fit X.fit --device D` |
| our JEDs vs `.tt2` (user designs) | 7/8 fit and match: `blink` ×2, `shift-register`, `spi`, `input`, `rpi-pico-8088`, `system-controller`; `motioncontroller` does not fit | `tools/fuzzers/fitter-conformance/check_user_designs.sh` |
| vendor JEDs vs `.tt2` (user designs) | 8/8 match (`shift-register` only with the vendor's truncated open-collector list) | same |
| Yosys-flow prototypes | ours 4/4 match `.tt2`; vendor 0/4 (pad feedback defect) | see F7 |
| regression tests | 95 vectors with vendor-identical fuses + checksum (`vendor_identical_fuses`) + arch unit tests + decode test; atmisp 139 vectors still pass | `docker/run.sh cargo test --workspace` |
| timing tables (F8.3) | 1502/1504/1508 tables from the datasheets and the fitters' SDF, 5 grades × 51/53/53 rows | `python3 tools/fuzzers/fitter-timing/derive_timing.py` |
| vendor SDF composition (F8.4) | 6,895/6,895 delays in 52 designs × 5 grades explained, 0 unexplained | `docker/prjbureau/run.sh sh tools/fuzzers/fitter-timing/sdf_corpus.sh`; `python3 tools/fuzzers/fitter-timing/check_sdf_timing.py atf15xx-arch/data/timing.json .scratch/timing/corpus` |
| prjbureau cross-check | 1502/1504: every PT range, option fuse and value, UIM range and input, GOE field, config bit agrees; 1502 GOE invert bits disagree | `python3 tools/arch/xcheck_prjbureau.py` |

## Evidence levels

Per item; the same markings are used in `docs/architecture/fitter/02-fuse-map.md`.

| item | level | how |
|---|---|---|
| region layout, mcfg/UIM field labels | labelled | ATMISP Load `NOTE` labels (`fuse-regions`) and vendor-fitter `NOTE` labels from ~20 000 vendor JEDs (`vendor_labels.py`); 0 conflicts |
| PT row addresses | labelled + external + oracle-verified | vendor `NOTE PT k of MC m` labels; formula has 0 disagreements; = prjbureau `pterm_ranges` (external, commit 8b8a971) |
| PT column formula (all 40 muxes) | oracle-verified | `exp_ptcol2.py`, `exp_ptcol3.py`: 1502 471, 1504 1572, 1508 4555 observations, 0 counterexamples |
| foldback column = local MC index | oracle-verified | `exp_fold.py`, all families |
| cascade MC−1 → MC | oracle-verified | `exp_fold.py` (`cas`), all families |
| MC field slot/direction/parity | oracle-verified | `exp_dir.py`, `exp_mcpos.py`, `exp_feat.py` (consistent for every MC of every family) |
| 24 MC bit meanings | oracle-verified | `exp_feat.py`, ~45 configurations × all pinned MCs × 3 families; = prjbureau option fuses for 1502/1504 (external) |
| OE codes GOE1/2/4/5, 0, 7 | oracle-verified | `exp_feat.py` `oe_*` |
| OE codes GOE3/GOE6 | external (prjbureau); inferred for 1508 | would be confirmed by a design using ≥3 GOE networks |
| UIM / GOE input tables | external (prjbureau) + oracle | since 2026-11-25 generated by `tools/arch/import_prjbureau.py` from prjbureau's database: 1502/1504 from upstream `8b8a971`, 1508 from prjbureau's own fuzzers run on branch `atf1508as` of the fork the submodule is pinned at; identical to the tables formerly read from fit150x.exe wherever determined; the 1508's 24 UIM and 57 GOE entries the fuzzers cannot determine are marked unconnected (used by none of 8,727 vendor JEDs or our outputs); the binary-derived tables are no longer used |
| config bits (termination, PD, GOE/GCLR/GCLK invert, ITD, pull-ups, power_reset, JTAG) | oracle-verified | `exp_feat.py` global options; `-strategy security on` → JEDEC `G1` only |
| config bits 26–31, special 0/2/3, mcfg fillers | inferred (never observed ≠ default) | — |
| 1502 GOE invert offset `C+3+n` | oracle-verified (vendor behaviour) | `exp_feat.py` `oe_g1n`/`oe_g2n`; prjbureau says `C+10−n` — silicon untested |
| pin maps, dedicated/JTAG/PD/GCLK3 pins, 13 devices | oracle-verified | `exp_pinmap.py`, `exp_pinroles.py`, `exp_dedic.py`, `exp_gclk3.py`; agree with datasheets |
| functional rules (F2.8: sum, XOR polarity, D source, storage, clocks, AR/AP, OE, feedback/pad, open drain, foldback inversion, fast PT2) | oracle-verified by simulation | `jedsim.py golden` on vendor JEDs (user designs, XOR corpus) |
| reset over preset priority; power-up state 0 | inferred | see Open issues |

## Decisions

* **2026-11-25 — Fuse identity, not byte identity, with the vendor fitter.** For copyright
  hygiene no vendor-written JEDEC text stays in the repository: our writer uses its own layout
  (F5.0) and every comparison (conformance test, `verify_tt2.py`) checks fuses, security fuse and
  checksum. Alternatives: keep `write_vendor` (reproduces the vendor's NOTE text verbatim), or
  keep vendor files as vectors and only change the writer (vendor text would stay in git).
  Byte identity added no functional strength: every byte-level difference would also be a fuse,
  `G`, `QP` or checksum difference, and the counts are the same under both definitions.
* **2026-11-23 — Vendor labels over ATMISP templates for PT numbering.** The ATMISP Load templates
  number PT rows differently; the fitter's `.fit` and JED labels are the functional numbering
  (confirmed independently by the prjbureau workstream). All code and docs use the fitter's.
* **2026-11-23 — Data from our own experiments only.** `arch.json` is generated from
  oracle results; prjbureau's database is read for cross-checking only (licence and provenance kept clean).
* **2026-11-23 — Crate split.** `atf15xx-arch` (database + formulas + decode, no fitter logic),
  `atf15xx-fit` (front-end-agnostic IR `netlist::Design`, `fit_design` library API, `tt2` front end,
  vendor-exact `jed` writer), thin `fit150x` binaries with vendor-compatible arguments. Chosen so a
  Yosys front end and the simulator can reuse pieces without the CLI.
* **2026-11-23 — Byte-identity as a target, function as the requirement.** Where the vendor's
  output is functionally wrong (register feedback of OE'd outputs via the pad) we do not copy it.
* **2026-11-23 — Reproduce vendor "don't care" bits by rule**, learned from the feature corpus
  (`encode_unused`/`encode_logic` in `fit.rs`), rather than a lookup table: generalises to new
  combinations and documents intent.
* **2026-11-23 — Placement heuristics mirror observed vendor order** (inputs lowest MC first,
  outputs highest MC of the lowest block, nodes on unusable-pin MCs) with fan-in-budget retries and
  sum splitting as fallbacks for dense designs.
* **2026-11-23 — UIM routing by bipartite matching**, candidates in ascending mux order (vendor's
  single-source choice); the vendor's multi-source heuristic is not reproduced.
* **2026-11-23 — Builds in Docker** (`docker/Dockerfile`, `docker/run.sh`, toolchain world-readable
  so the container runs as the host user; caches in `.scratch/`). Vendor fitters run under wine on
  the host (`~/.wine32`); the prjbureau workstream's Docker fitter (`tools/fuzzers/common/fit.sh`) is an
  alternative.

## Journal

* **2026-11-26** — Clone pass (user: "ensure we're not widely using borrow checker appeasement in
  ways that slows our execution"), batch 1, by profile (SELFPROGRAMBOARD, frame-pointer build):
  `Sop::signals` / `Cell::inputs` cloned every literal's `Signal` (a heap `String`) before
  deduplicating; now they sort references (`signal_refs`, `input_refs`) and clone only the
  distinct signals, and `support()` clones nothing. `fanin_count` replaces the "how far over
  budget" loop that called `fanin_ok` with limits 0, 1, 2, ... (one union instead of one per
  step). The general XOR-split search is memoised (`general_xor_split`, pure in the cover and the
  polarity; it ran again in every strategy's `fit_design`). SELFPROGRAMBOARD fit 1.98 → 1.46 s
  (Verilog build 3 s, was 14 s this morning); 18-bit compare 43 → 34 s. Checks: vendor identity
  unchanged, corpus and examples byte-identical, SELFPROGRAMBOARD unchanged, tests 48/48, clippy
  count unchanged. Edge sweep (`edge-v3.json`, before this batch): 3,712 s for 198 builds (first
  sweep 15,537 s), failures median 0.2 s, 90 % 48 s, max 137 s; fits median 15.5 s.
* **2026-11-26** — Time to failure, continued (18-bit registered compare, ATF1504AS-TQFP100,
  one thread 74 → 50 s, same fit 60/64): (1) `Design::collapse_comb_nodes`: the foldback check of
  every candidate node computed both its covers even when no foldback product reads the node (then
  the answer is always yes): 21 s of complements skipped. (2) `plan_pts`' flipped polarity and
  `pt_demand`'s alternative cost ask `Sop::complement_within` (memoised now, `COMPLEMENT_CHOICE_CUBES`
  = 16384): a complement too large to compute is never the cheaper polarity; within the plain
  complement's 4096 cubes the answer is unchanged. Remaining: the UIM repair loop (96 moves, ~0.11
  s per attempt in placement: `fanin_ok`, `uim_ids`) in failing strategies, ~38 of the 50 s.
  `ATF15XX_FLOW_TIMING` now also times the netlist passes, the vendor-style collapse and
  absorb/legalize. Checks: vendor identity unchanged, corpus (37) and examples byte-identical,
  SELFPROGRAMBOARD unchanged, tests 48/48.
* **2026-11-26** — `fit_attempt` (1165 lines) split into its passes (user request): `place_cells`
  → `global_nets` → `plan_pts` → `host_foldbacks` → `route_uim` → `assemble`, with the state
  they hand on in `Placed`, `Nets`, `Folds`, `Routing`; `fit_attempt` is the 15-line driver.
  The pass bodies moved unchanged (each destructures its inputs into the old local names); the
  closures shared across passes became functions (`signal_source`, `placed_pin`,
  `gclk_net_of_pin`); the `FOLD_MC` thread-local is gone (`write_pt` takes the expander map).
  Checks: vendor identity unchanged (2017 / 4025 / 6155), corpus (37) and examples
  byte-identical, SELFPROGRAMBOARD unchanged, tests 48/48. Edge sweep after the time work
  (`.scratch/mploop/edge-v2.json`): 198 builds 15,537 → 4,190 s, timeouts (180 s) 48 → 0; the
  registered compare now fits up to 30 bits on the ATF1508AS (was 14).
* **2026-11-26** — Time to failure, continued (16-bit registered compare, ATF1508AS-TQFP100):
  gate mapping 126 → 5.0 s, word mapping 324 → 8.3 s, same fits (54 / 65 MCs). (1) `pt_demand`'s
  foldback-factoring estimate is memoised per thread (`factored_len`): each of the 7 fit attempts
  of each strategy re-factored the same 128-product cover (3.9 s per attempt). (2) A cell whose
  estimate exceeds what one block's cascade chain holds (`MAX_CHAIN_PTS` = 80) is rejected as soon
  as the estimate is known, with that reason (placement uses the same estimate, so it could never
  be placed; before, the attempt failed later as "no free macrocell for node ..."). (3)
  `split_large` asks `Sop::complement_within(16384 cubes)` whether the complement is cheaper: the
  16-product, 32-signal `lt` cover's complement took 281 s to compute and was not cheaper. Within
  the plain complement's 4096 cubes the answer is unchanged. (4) `ATF15XX_FLOW_DEBUG` times each
  netlist pass (`pass toggles|enables|collapse|split`). Checks: vendor identity unchanged, corpus
  (37) and examples byte-identical, SELFPROGRAMBOARD unchanged, tests 48/48.
* **2026-11-26** — Time to failure (user: "the time bounds before failure are not so horrible on
  designs that won't fit"). (1) `logic::complement`: the plain Shannon expansion runs as before up
  to 4096 raw cubes; beyond that `complement_folded` recombines the halves at every split (a cube
  in both halves, or contained in a cube of the other, loses the split variable), test
  `folded_complement_is_exact` (truth tables; an 8-pair equality's complement stays ≤ 64 cubes).
  (2) The flow fails at once when the design cannot fit whatever the strategies do: more
  registers than macrocells, or more outputs / inputs than (non-JTAG) I/O / input pins (dedicated
  GCLK/OE/GCLR pins counted). Measured: 14-input registered mux on the ATF1502AS 198 → 0.02 s;
  72-bit accumulator on the ATF1508AS-TQFP100 133 → 0.02 s (82 pins for 80); 12-bit registered
  compare (word mapping) 95 → 20 s; the 16-bit one now fits (was > 400 s; 126 s gates, 324 s
  words: open). Checks: vendor identity unchanged (2017 / 4025 / 6155), corpus (37) and examples
  byte-identical, SELFPROGRAMBOARD unchanged (flow 60 MCs 52.6 MHz, BOUNDED 21 vs the vendor JED;
  fit1504 on the vendor `.tt2` 60 MCs 55.6 MHz; `.scratch/spm_oracle.sh`), tests 48/48.
* **2026-11-26** — `logic::merge` (the merge/containment/expand loop behind `Sop::minimize` and
  `Sop::complement`) rewritten to give *exactly* the old result faster: the first mergeable pair
  of the sorted cover is found by looking up distance-1 neighbours in a sorted set, resuming
  from a frontier (only partners of the new cube can pair below it), and once the cover is
  containment-free only the new cube is checked. The old algorithm stays as `merge_general` (the
  fallback for cubes with bits outside their care set, and the test reference:
  `merge_equals_the_plain_algorithm`, 460 random covers and their complements, identical).
  Why: the mapping–placement edge sweep's 12-bit registered compare timed out (> 180 s): its
  complements reach 20,590 raw cubes (the Shannon complement does not merge halves) and the
  old merge was ~n³. Now: gate mapping 15 → 9.2 s, word mapping 139 → 95 s (single thread
  82 s: the vendor-style collapse of `lt` takes 36 s, open). Checks: vendor identity unchanged
  (2017 / 4025 / 6155), corpus and examples byte-identical, workspace tests 47/47.
* **2026-11-26** — **Bug fixed** (found by the mapping–placement edge sweep,
  `.scratch/mploop/edge.py`): a registered 6-bit compare panicked on the ATF1502AS
  (`write_pt`, "no entry found for key"). A macrocell holding a cell *and* the cascade helper of
  its neighbour gets the helper's PTs written into it, but the block's UIM `need` list only took
  the cell's own PTs, so the helper's inputs were routed only when another PT happened to need
  them. Fix: those helper PTs' inputs are added to the list (after the others, so blocks that
  already routed them keep their order). The two map lookups in `write_pt` now return internal
  errors (the flow then tries the next strategy) instead of panicking. Regression design
  `atf15xx-synth/tests/designs/cmpreg.v`: 19 MCs on all three parts, PROVEN (52.6 MHz on the
  1502 by strategy 0; before, the first strategies panicked). Checks: vendor identity unchanged
  (2017 / 4025 / 6155, `verify_tt2.py` on feat + ptcol2 + pinroles), corpus and examples
  byte-identical, workspace tests 46/46.
* **2026-11-26** — Typed fit errors (F9.9 item 1, prerequisite of the mapping–placement loop):
  `FitError` carries a `FitErrorKind` set at each of the 28 raise sites instead of being
  classified from its text afterwards; "no free input pin" is its own kind `InputPins` (counted as
  capacity by `is_capacity()`, as before), so the flow's capacity trigger is exactly `Capacity`
  (the same three messages it matched). Check: workspace tests 46/46 (95 vendor-identity vectors
  included); 34 corpus JEDs and the 4 examples byte-identical (`.scratch/build_all.sh`).
- **2026-11-25**: New binary `atf15xx-fit` (user request): `-d` takes a part, ordering code, fitter or
  CUPL name and selects the family; typed flags for the common settings, `--set KEY=VALUE` for every
  vendor strategy (later wins), `-o` for the output stem; `fit150x` keep the vendor command line. JED
  byte-identical to `fit150x` on all 95 conformance designs (the three `nojtag` vectors differ only
  in option precedence: the vendor honours the first `-JTAG`, `--set` the last).
- **2026-11-25**: `arch.json` retired: the fuse map is generated at build time from prjbureau's
  database (`atf15xx-arch/build/fuse_map.rs`, the port of `import_prjbureau.py`, same checks; any
  inconsistency fails the build), devices from `data/devices.json`, timing from `data/timing.json`.
  Identical to the retired file in every field (docs/plan/prjbureau-import.md §3).
- **2026-11-25**: Timing table derived as a workflow
  (`tools/fuzzers/fitter-timing/derive_timing.py`): datasheets (1502AS 0995L, 1502ASV 1615J,
  1504AS 0950P, 1508AS) truncated to whole ns, overridden by the fitters' SDF for every parameter
  a delay shows (120 values), 1504 -15/-20 from the 1508 [I]; all 6,895 SDF delays explained;
  SDF-invisible parameters are the datasheet's (F8.3.4). `regions.json` rebuilt from
  ATMISP Load output + prjbureau `pterm_ranges` (ATMISP's text numbers PT rows differently, the
  fitter's labels agree with prjbureau on 160/305/536 rows), identical to the old one. Experiments
  moved to `tools/fuzzers/fitter-{pins,pterms,macrocell,foldback,timing,conformance}`; vendor
  identity unchanged (2017 / 4025 / 6155). F2's evidence is [L] (vendor labels) or [P] (the UIM
  tables, from prjbureau).
* **2026-11-25** — `arch.json` fuse map now generated from prjbureau (`tools/arch/import_prjbureau.py
  --write`, with a `provenance` section). Full
  regression unchanged: vendor identity on all three corpora (1502 1591/1615, 1504 2146/2153, 1508
  1777/1915; ptcol2 246/1003/3522; pinroles 180/876/856), user designs 8/8, workspace tests 45/45,
  corpus results identical to corpus29, plugin == CLI 34/34, equivalence suite ok.

* **2026-11-25** — Own JED layout (task "JED writers without vendor text"). `jed::write`: header
  `JEDEC fuse map for <chip>, written by open-atf150x <version>`, `Created:`, `QF QP F0 G`, then one
  `L` field per fuse-map item in address order with an `N` label (`MC7 PT2`, `block C UIM mux 13`,
  `global OE 1`, `device configuration`, `JTAG special bits`, …), `C`, ETX `0000`; CRLF; every fuse
  explicit; fields < 200 bytes (ATMISP's reader limit, spec §3.2). `write_vendor` removed (cli.rs,
  atf15xx-synth switched); `jed::parse` also recognises our header (device only if its fuse count
  matches QF); `created_on` reads our `Created:` line. `examples/jed_roundtrip.rs` → `jed_rewrite.rs`
  (rewrites files, asserting unchanged fuses). The 95 vectors were rewritten with it; the
  conformance test (`vendor_identical_fuses`) passed on the vendor files before and on ours after
  **[O]**. `tools/fuzzers/fitter-conformance/verify_tt2.py`: `identical` = same fuses + same `G` + correct `C` in ours
  (`bad-checksum` = same fuses, wrong `C`; `same-fuses` no longer exists). Regression, all
  unchanged against the figures just before the switch **[O]**: `verify_tt2.py` `feat` 1591/1615,
  2146/2153, 1777/1915; `ptcol2` 246/251, 1003/1013, 3522/3550; `pinroles` 180/180, 876/876,
  856/1071; `check_user_designs.sh` 8 × `ours: matches`; workspace tests 36 passed, 0 failed;
  corpus (`.scratch/flow/corpus29` vs `corpus28`) `results.md` identical (559 macrocells, 34/35
  PROVEN); Yosys plugin 34/34 fuse-identical; `tools/sim/tests/run_tests.sh 60` 12/12 ok. JTAG
  guard on our files: `vblink.v` built for ATF1508AS-PLCC84 → fuse 74115 = 1, accepted;
  the same with `--option jtag=off` → 74115 = 0, refused (JED and SVF), **[O]** on our own
  output (no hardware programmed). Reproduce:
  ```
  cargo build --release --workspace; B=$PWD/target/release
  for c in feat ptcol2 pinroles; do python3 tools/fuzzers/fitter-conformance/verify_tt2.py --bin-dir $B .scratch/oracle/$c | tail -3; done
  docker/yosys/run.sh .scratch/target-yosys/release/atf15xx-synth build tools/silicon/designs/vblink.v -d ATF1508AS-PLCC84 -o /tmp/vb
  python3 tools/silicon/jtag_guard.py --jed /tmp/vb/vblink.jed --svf /tmp/vb/vblink.svf
  ```
* **2026-11-25** — Memoisation (user request): `Sop::minimize`/`complement` cached per thread
  (hit rate 50–65%, small gain); `fanin_ok` from memoised interned UIM inputs and a bitset (the
  real win: SELFPROGRAMBOARD 6.1 s → 2.6 s); strategy-prefix cache in the flow (2.3 s). Corpus
  298 s → 47 s. Fuses and vendor identity unchanged.

* **2026-11-25** — Performance (user request; profile first, F9.11): `tautology` from whole-word
  masks; the partitioner's signals interned (dense counts, no clones, `Vec` block map):
  SELFPROGRAMBOARD 26 s → 8.9 s. `FitError::kind()` (typed failure kinds); after a capacity
  failure the budget levels that only lower the fan-in limit are skipped. Fail-fast macrocell
  bound (`cells − min(eligible registers, hosts)`): shift32/1502 75 s → 10 s. All fuse-identical;
  vendor corpora unchanged.

* **2026-11-25** — Exact block-assignment oracle (`atf15xx-oracle`, HiGHS via `good_lp`;
  `fit::block_problem`; F4.6c): our partition is optimal on every tight case (counter64,
  SELFPROGRAMBOARD, motioncontroller) and unbalanced but feasible on easy ones; no missed fit.
  `nodes_to_foldbacks_with(…, one_level)` for timing (F8.7). The tools image gains cmake.

* **2026-11-25** — Library oracles (user request; `docs/plan/libraries.md`):
  `tests/oracles.rs` checks `logic.rs` against Berkeley Espresso (exact on 1000 random
  functions; `minimize` within one cube; `complement` larger on 381 of 978) and the UIM matcher
  against petgraph's maximum matching (agrees on 1200 random source sets of the real UIM
  tables). Adoption attempts: Espresso in the planner stalls (57.7 s on a 15-variable
  counter-bit cover; Berkeley's own C program too). A BDD-derived "smaller of the two" cover
  changed nothing on 70 corpus fits, so it was removed again. The tools image gains libclang
  (bindgen).

* **2026-11-24** — XOR synthesis (F4.6b). The vendor's XOR tests all ran with
  `-XOR_Synthesis on` (102 of the 6120 feat designs; no other corpus uses it), so the
  two-literal pattern follows that flag (`Options::xor_synthesis`); the general split is ours
  (`xor_split`, flow default). Vendor XOR tests 94/95 byte-identical (were 0/95). Byte identity
  feat 1564/2110/1746 → **1591/2146/1777**; ptcol2, pinroles, user designs unchanged. Silicon
  `designs/vxor.v` 16/16.

* **2026-11-24** — Packing P2+ (user question: use the fast input register but keep the
  combinational part). The shared `OMux`/dfast fuse forces pin = Q with D = pad, so `x` has no
  exit while Q is on the feedback, but the PTs, sum and cascade stay free. Now a PT-free input register
  may sit in a cascade-helper macrocell (`helpers_free` share set, packer, helper encoding
  overlay); foldback hosting on fast-input MCs already worked (`fold_movable`). Silicon 32/32
  (`designs/inreg_cascade.tt2`).

* **2026-11-24** — UIM-aware position search (F4.6a) inside the partitioned attempt; kept only
  when it makes every block routable. First version ran as a third attempt with 4000 full
  re-matchings per call: counter64/TQFP100 lost its fit because the flow's 120 s budget ran out
  before strategy 3 (28 searches, none helping). Now incremental (only blocks reading the moved
  cells are re-matched), bounded (500 stale moves, hopeless starts skipped) and with no extra
  attempt: counter64 back at 87 MCs (101 s), SELFPROGRAMBOARD Verilog fits in 9 s. Vendor byte
  identity unchanged (feat 1564/2110/1746, ptcol2 246/1003/3522, pinroles 180/876/856), user
  designs 8/8, open-flow corpus identical, tests 34/34.
* **2026-11-24** — `observable_registers` (flow default): buried registers get `OMux` = 0 so the
  boundary-scan output cell captures Q (F2.10); the vendor writes `OMux` = 1 there.

* **2026-11-24** — Read the vendor's SELFPROGRAMBOARD `.fit` (user suggestion): 41 buried + 23
  pin cells = 64, no logic doubling; all 10 combinatorial nodes collapsed; `SPI_Count_Enable`
  lowered to a two-level foldback network (`XXL_375` = NAND of `FB_410..413`) **replicated in
  all four blocks**; per-block fan-in 38/38/36/32; foldbacks 6/16/16/9. Implemented shared
  foldback signals (F4.6a) and the fixes they needed (fold products may read nodes; collapse
  respects them; no inverted foldback literals; self-hosting; partition to the attempt's fan-in
  budget). The Verilog port now has **64 cells** like the vendor and every block ≤ 40 inputs, but
  UIM matching of block A fails at 37–39 inputs: which sources a block's muxes can select depends
  on the macrocell each source sits in, and our placement chooses blocks, not positions. Next:
  UIM-aware placement within blocks. Regression: byte identity, user designs (8/8), corpus,
  workspace tests unchanged; the `.tt2` SELFPROGRAMBOARD still fits and matches.

* **2026-11-24** — Macrocell packing study (user: "stealing the flip flop … as an input register,
  output register, or buried flip flop"): D/Q group allows P1–P4 (F4.6a table). Implemented P2
  (input registers, `pack_input_registers`, flow default) and fixed both models, which treated a
  PT-less fast-input macrocell as idle. Corpus identical; feature corpus byte identity unchanged;
  8/8 user designs. SELFPROGRAMBOARD Verilog port: 64 cells for 64 MCs with
  `--no-toggles --option special_foldback=on`, now short of foldback hosts (next: host foldbacks
  on any cell with a free PT slot, as the vendor does with 47 foldbacks).

* **2026-11-24** — Real-design benchmark SELFPROGRAMBOARD (user, P80C550-SPM-CPLD): vendor fits
  64/64 MCs with 47 foldbacks. Fixes found on the way: identical special-function nodes are now
  shared (16 per-bit `MISOn_CE$` nodes → 1); a multi-product asynchronous reset goes through
  `one_product` (foldback / two-level) instead of an error; the GCLR polarity is fixed by its first
  user and the other polarity becomes a PT reset (before: silently overwritten per cell); logic
  doubling only hosts registers clocked by a global clock pin. Still does not fit (UIM block A 46 >
  40): needs the partitioning placer (F9.5 R3). Model: cascade-lender XOR input is 1 (silicon).

* **2026-11-24** — (main session, user request "make foldback terms work … demorganizing")
  Foldback factoring generalised to any common cube (De Morgan: an OR of literals is one NAND
  foldback) and applied to both polarities; two-level foldback (a fold PT reading folds) for
  multi-product special functions behind `special_foldback`. Feature/ptcol2 corpora byte identity
  unchanged (1564/2110/1746, 246/1003/3522); fold corpus never vendor-identical before or after;
  workspace tests pass. **`motioncontroller` now fits** and matches its `.tt2` (jedsim golden; the
  vendor JED passes the same check) → 8/8 user designs. Silicon: fold3 (common cube) and fold4
  (two-level OE) 64/64. Note: `verify_tt2.py` and `check_user_designs.sh` default to the Docker
  binaries in `.scratch/target-docker`; pass an absolute `--bin-dir` to test a host build.

* **2026-11-24** — Timing (F8), main session, on the user's request. Found `-strategy Verilog_sim sdf`
  and `-strategy tPD = N` in the fitters' help text; the vendor writes `.vo` + `.sdo` (SDF 1.0). The
  SDF delays match the datasheets' AC tables truncated to whole ns; their composition was checked
  on 260 vendor SDFs (all explained after adding the foldback rule, latches and T flip-flops).
  `MC_power on` and `output_fast on` don't change the SDF: the vendor annotation ignores slow slew
  (tOD3, the default!) and the reduced-power adder (tRPA). The datasheet internal parameters sum
  exactly to its pin-to-pin specs at -7/-10/-15; -20/-25 specs have margin. (Re-derived as a
  workflow on 2026-11-25: `tools/fuzzers/fitter-timing`.)
* **2026-11-24** — Fitter capacity work: logic doubling (`-strategy logic_doubling`), hosted
  T→D+CE conversion, foldback factoring of large sums, foldback hosting on used macrocells,
  pad-or-feedback routing alternatives, routing repair loop; `open_collector` lists split on commas.
  `shift-register` (128 flip-flops, 1508) and `system-controller` now fit and match their `.tt2`.
  `motioncontroller` still fails (block A needs 36–38 UIM inputs; vendor 34).
* **2026-11-24** — Vendor defect found: the fitter truncates long `open_collector` lists (its `.fit`
  echoes `OUT0, … OUT43, O,`), so the user's `shift-register.jed` has OUT44–OUT63 push-pull.
  Reproduced the user's `shift-register.jed` byte-identically (apart from the date) from their
  `.pld` with their build flags (`.scratch/user2/atf1508as-example`). Note: zsh does not split
  `$OC` unquoted — use `${=OC}`.
* **2026-11-24** — Reset/preset priority and power-up state researched (datasheet, *Using the
  ATF1500 CPLD*, prjbureau naming): still inferred; notes added to F2.8.

* **2026-11-23** — **Decode API change (for `atf15xx-sim`)**: `ClockSource::AlwaysHigh` added
  (`Pt4Func` = 1, `Pt4Mux` = 0: clock/enable tied high). jedsim models it; with it the vendor's
  1508 `system-controller` JED matches its `.tt2` (was 45 mismatches). F2.8 rule 10 updated.
  Latches with a real enable are sampled once per stimulus step (loops through transparent latches,
  as in `input`, are order-dependent otherwise).

* **2026-11-23** — User: the finished real designs are `atf1508as-example` (shift register, many
  flip-flops), `atf1504as-blink`, `atf1508as-blink` and `atf1504as-spi`. Compiled the blink designs
  and the shift register with CUPL + vendor fitter in `.scratch/user2/` (same flags as the user's
  build scripts) to add them to the real-design set.
* **2026-11-23** — `system-controller` mismatch traced: no register diverges; the vendor turned the
  combinational node `CPU_Peripheral_Select` (`-global_fold`) into a **latch with no enable**
  (`L----`, `Storage` = 0, `Gclk` = GCLK2 code, `Pt4Mux` = 0) read through `FbMux` = 0. jedsim
  updates latches only at clock time, so this "transparent" latch lagged one step. Fix in progress.
* **2026-11-23** — XOR semantics settled: `x = sum ⊕ B ⊕ ¬Invert`, `B = PT2` in XOR mode else 1;
  95/95 vendor XOR designs match their `.tt2` under it.
* **2026-11-23** — Added `atf15xx_arch::decode` (fuse array → structural netlist) for the simulator
  workstream; added F2.8 functional semantics table.
* **2026-11-23** — Suspected vendor defect found with the Yosys-flow counter: register feedback
  `q0.Q` of an output with `q0.OE = oe` routed through the pad (`A1p`); breaks when `oe` = 0
  (with `oe` held high the vendor JED matches the source).
* **2026-11-23** — prjbureau findings arrived; cross-check script written (all agree except 1502
  GOE invert bits). Documented in F2.7.
* **2026-11-23** — jedsim: golden mode (simulate the `.tt2`) added; found that the foldback line is
  inverted, and that `DMux` = 0 ∧ `OMux` = 1 removes PT2 from the sum (vendor `spi` fails
  otherwise); empty OE cover means "always enabled" in the vendor (`rpi-pico-8088` `RQGT`).
* **2026-11-23** — Fitter: comb-node collapse, output-register absorption, `.PIN` substitution,
  pad routing for always-enabled outputs, special-function legalisation (foldback or helper node),
  cascade reservation, fan-in budgets, sum splitting; the vendor honours the *first* of repeated
  `-strategy` options.
* **2026-11-23** — **Bug fixed** in `tools/fuzzers/common/fitter_oracle.py::read_jed`: its regex consumed
  the `*` that terminates one `L` field as the start of the next, so every other field was skipped
  (mcfg fields pairwise, PT rows). All earlier analyses were re-run after the fix.
* **2026-11-23** — Template PT labels found inconsistent with vendor labels (e.g. 1504 template
  "MC3 PT1" = vendor "MC14 PT5"); region table rebuilt from vendor labels.
* **2026-11-23** — Experiments: PT columns, MC positions/directions, features, foldback/cascade,
  pin maps/roles/dedicated/GCLK3. Oracle flakiness under
  parallel wine load observed (3 spurious failures in ~5000 runs).
* **2026-11-23** — Start: surveyed the user's examples, fitter invocations, datasheets; reproduced
  the user's `input.jed` exactly with the vendor fitter under wine; cloned prjbureau
  (`.scratch/upstream/prjbureau`, 8b8a971) before the prjbureau pause; built `open-atf150x-tools`.

## Open issues

* ~~`motioncontroller` does not fit~~: fits since 2026-11-24 (De Morgan foldback factoring).
* **Vendor defect**: truncated `open_collector` lists (`shift-register`).
* **Vendor UIM mux choice** with competing sources (main source of 1508 byte differences).
* **XOR synthesis** (`-strategy XOR_Synthesis on`) not implemented in the fitter.
* **Suspected vendor defect**: pad routing of register feedback for outputs with OE (F7.1).
* **1502 GOE invert bits**: vendor vs prjbureau disagree; silicon test needed.
* **Reset vs preset priority, power-up state**: inferred (reset wins; registers start at 0).
  Datasheet statement and/or a vendor-fitted design with both AR and AP asserted would confirm.
* **Timing analyser** for our fitter (F8.5) not implemented; tPEXP branch of the vendor SDF
  writer never observed (no OR wider than 5 inputs in the corpus); 1502 row names for tZX1–3
  inferred (no ATF1502AS datasheet at hand).
* **Config bits 26–31, special bits 0/2/3, mcfg fillers**: never observed non-default.

## Artifacts

* Specs: `docs/architecture/fitter/README.md`, `01`–`08`.
* Crates: `atf15xx-arch` (`src/lib.rs`, `src/decode.rs`, `data/arch.json`, `data/timing.json`),
  `atf15xx-fit` (`src/{tt2,netlist,logic,fit,jed,report,cli}.rs`, `tests/conformance.rs`,
  `tests/vectors/<family>/` — 95 synthetic designs generated by our scripts with the vendor's JEDs),
  `fit1502|fit1504|fit1508`.
* Tools: `tools/fuzzers/` (one workflow per derived fact; `common/` has `fitter_oracle.py`,
  `tt2gen.py`, `verify_tt2.py`), `tools/arch/xcheck_prjbureau.py`.
* Docker: `docker/Dockerfile` (image `open-atf150x-tools`: Ubuntu 24.04, Yosys, python3-bitarray,
  Rust stable with clippy/rustfmt), `docker/run.sh`.
* Scratch (git-ignored, disposable): `.scratch/oracle/` (vendor runs), `.scratch/exp/`
  (experiment outputs), `.scratch/sim/`, `.scratch/user2/`, `.scratch/upstream/prjbureau` (clone,
  untouched since the pause).
