# Handoff: open-atf150x

**As of 2026-11-25.** This is the starting point for anyone (human or agent) picking up this
project. Details live in the linked documents; this page says where everything is, what state it
is in, and what to do next. Keep it current when the state changes.

## 1. Goal

An open, maintainable toolchain for Microchip/Atmel **ATF15xx CPLDs** (ATF1502/1504/1508, AS/ASL/
ASV/ASVL, BE, ATF1508RE): **Verilog → Yosys → our fitter → `.jed` → SVF → JTAG**. The vendor
toolchain (WinCUPL fitters `fit1502/1504/1508` 1.8.7.8, ATMISP 7.3) is old and Windows-only, and the
vendor Verilog compiler is unobtainable. The vendor tools are used **only as oracles** (run under
wine) to verify our output (SVF byte for byte, fuse maps fuse for fuse, or functionally). No
vendor-written JEDEC text is kept in the repository: both our JED writers use our own layout.

## 2. State

| Stage | Component | State | Verified by |
|---|---|---|---|
| `.jed` → SVF | `atf15xx-tool` (module `isp`, `atf15xx-tool/src/isp/`; commands `svf`, `chain`, `jed`) | **done** | SVF byte-identical to ATMISP 7.3 on 145 vectors + all 16 Microchip sample designs; Load JEDEC output fuse-identical (own layout, spec §8.2) |
| `.tt2` → `.jed` | `atf15xx-fit` (binaries `fit1502/1504/1508`) | **done for the user's finished designs** | 7/8 real CUPL designs fit and are equivalent to their `.tt2`; fuse-identical to the vendor (JED in our own layout, F5.0) on the `feat` corpus 98.5 % (1502) / 99.7 % (1504) / 92.8 % (1508) |
| Verilog → `.jed` → SVF | `atf15xx-synth` | **first version done** | every corpus design that fits is PROVEN against its Verilog, incl. the user's Xilinx `blink.v` (ATF1508AS) and `spi_controller_mode0.v` (ATF1504AS) and a behavioural 64-bit counter (87 MCs, ATF1508AS) |
| in-circuit debugging | `atf15xx-tool` (module `dbg`, feature `jtag`; commands `part`, `idcode`, `probe`, `sample`, `watch`, `capture`, `drive`, `seq`) | **first version done** | boundary-scan SAMPLE/watch/VCD capture/EXTEST through OpenOCD, cell maps generated at build time from the prjbureau BSDLs (all 12 part/package pairs agree with the device tables); bench: named readout of a 40-bit counter 8/8 (spec D1, log 08) |
| `.jed` simulation / equivalence | `atf15xx-sim`, `tools/sim/` | **done** | self-checks (emitter vs iverilog, the former Python model 95/95 until 2026-11-25), mutation tests; Verilator engine 100/100; `tools/sim/tests/run_tests.sh` 12/12 |
| Architecture / fuse semantics | `atf15xx-arch`, `docs/architecture/fitter/` | **done**, with evidence level per item (incl. UIM routing fuses, F2.4) | oracle experiments, binary tables, prjbureau cross-check |
| Timing data | `docs/architecture/fitter/08-timing.md`, `atf15xx-arch/data/timing.json` | **data done; no analyser yet** | derived from the datasheets and the vendor's SDF output for all 3 families × 5 grades (`tools/fuzzers/fitter-timing`); 6,895/6,895 vendor SDF delays explained |
| Silicon | `tools/silicon/`, log 06 | **first device programmed and measured automatically** (Arduino bench, log 06): our `.tt2` fits and the Verilog flow work on silicon; reset priority, post-ISP register state, GCLR polarity, foldback verified (F2.9). Our Verilog flow's deep counters fail at 66.6 MHz (timing) | programming + independent readback verify 0 errors; verify negative control fails as it should; JTAG still answers |

`docker/run.sh cargo test --release --workspace`: 36 passed, 0 failed (2026-11-25). Git: branch
`main`, no remote.

## 3. The whole flow (Verilog → SVF)

```
design.v ─Yosys─► netlist.json ─importer─► fitter ─► fuse map ─► .jed ─atmisp─► .svf ─► JTAG player
                                                                   └─ equiv.py / vcompare.py: .jed ≡ design.v
```

1. **Constraints**, either as Verilog port attributes (`(* LOC = "43" *)`, also `PIN`/`CHIP_PIN`;
   a list for a vector, LSB first; `(* FAST *)`, `(* OPEN_COLLECTOR *)`, `(* LOW_POWER *)`) or as a
   constraints file `-c design.acf` (`device ATF1504AS-PLCC44`, `set_io clk 43`, `fast`,
   `open_collector`, `low_power`, `option jtag off`, `option logic_doubling on`, …). File entries
   override attributes; `--pin NAME=PIN` and `--option KEY=VALUE` override both. Spec:
   `docs/architecture/fitter/07-yosys-flow.md` §F7.5.
2. **Build** (pinned Yosys v0.71, commit `b4c586fd…`, built in Docker):
   ```
   docker build -t open-atf150x-yosys docker/yosys/
   docker/yosys/run.sh cargo run --release -p atf15xx-synth -- build design.v -d ATF1504AS-PLCC44 [-c design.acf] -o out/
   ```
   Inside: Yosys `synth_atf15xx.ys` (proc; tribuf; synth -noabc; dfflegalize to DFF/DFFE/±async
   R/S/latch; write_json) → Rust importer (SOP covers from Yosys gates; clock-enable/toggle
   recovery; collapse bounded at 32 inputs so carry chains stay buried nodes) → fitter (macrocell
   placement, PT allocation, cascade/foldback, UIM routing, global clock/clear/OE, logic doubling,
   options) → fuse assembly → `.jed` → atmisp library → `.svf`. Outputs in `out/`: `.jed`,
   `.svf`, `.fit`, `.pin`, `.pins.json`, `.map.json` (register → macrocell map, §F5.4).
   The `.jed` is in our own layout (`atf15xx_fit::jed::write`, F5.0: one `L` field per fuse-map
   item with an `N` label, every fuse explicit, fields < 200 bytes so ATMISP loads it too); the
   reader accepts ours, the vendor fitters' and ATMISP's files. atmisp's `Load` output likewise
   (`atf15xx-tool/src/isp/jedout.rs`, spec §8.2). Tests and oracle harnesses compare fuses + checksum.
   Note: Yosys removed `abc -sop` in v0.69; the old mapping is `--map abc-sop` for Yosys ≤ 0.68.
3. **Verify** (time-bounded):
   ```
   tools/sim/run.sh python3 tools/sim/equiv.py out/design.jed --device P1504C44 \
       --verilog design.v --top design --fit out/design.fit --budget 60 [--sim-engine verilator]
   ```
   Formal first (ABC dprove: signal correspondence + induction; then PDR/BMC/sat), then simulation
   (iverilog, or Verilator with state injection). It ends with `GUARANTEE: PROVEN | BOUNDED n |
   SIMULATED n cycles + k injections | FAIL`. Exit 0/1/2 = PASS/FAIL/ERROR. `--device` takes the
   fitter name (`P1504C44`, `P1508C84`, …). Also `--tt2 X.tt2` or `--gold-jed Y.jed` as the gold
   model; `--assume EXPR` for bus-protocol constraints; `--init/--reset/--power-up` for
   initial-state handling. Standalone Verilator: `tools/sim/vcompare.py` (60 s default budget).
4. **Program**: play the `.svf` with any SVF player (OpenOCD `svf`, UrJTAG, ATE). The SVF is
   identical to ATMISP's. **Untested on silicon.**

Alternatives: CUPL `.tt2` → `fit1504 design.tt2 …` (our fitter) → `atf15xx-tool svf -d
ATF1504AS:pv:design.jed -o design.svf`. A vendor `.jed` → `atf15xx-tool svf` alone. Chain files:
`atf15xx-tool svf chain.chn --drive C=$HOME/.wine32/drive_c -o x.svf`.

## 4. Where things are

| Path | Contents |
|---|---|
| `docs/architecture/` §1–§11 | normative spec of ATMISP 7.3 (chain files, JEDEC reader, fuse→row maps, SVF writer, algorithms, Load writer, validation/defects, CLI, conformance) |
| `docs/architecture/fitter/` F1–F7 | chip architecture, complete fuse map with evidence levels (§F2.8 functional semantics table), `.tt2` grammar, fitting algorithm, vendor outputs (+ `.map.json` §F5.4), conformance, Yosys flow |
| `docs/architecture/sim/` | simulator model, Verilog emitter, equivalence method, limitations, harness guide (stable entry point), Verilator engine |
| `docs/prjbureau/README.md` | prjbureau study (how it's meant to be used, cross-checks) |
| `docs/worklog/` | per-workstream logs (00 project, 01 ATMISP/SVF, 02 fitter, 03 prjbureau, 04 simulator, 05 Verilog flow); conventions in its README |
| `Cargo.toml` | virtual workspace: `[workspace.package]` (version, edition, licence) and `[workspace.dependencies]` (all shared and internal crates); the members are listed explicitly |
| crates (top-level directories since 2026-11-25) | `atf15xx-tool` (ATMISP replacement + its 145 oracle vectors, `atf15xx-tool/tests/vectors.rs`), `atf15xx-arch`, `atf15xx-fit`, `atf15xx-sim`, `atf15xx-synth` (Verilog → JED/SVF; feature `yosys` = the Yosys plugin, built in the Yosys image), `atf15xx-tool` (also the in-circuit debugger) |
| `tools/` | `fuzzers/` (every analysis that runs the vendor programs, one workflow per derived fact, each with `README.md` + `CLAUDE.md`; shared wrappers in `fuzzers/common/`), `sim/` (`equiv.py`, `vcompare.py`, the Verilog → `.tt2` route), `flow/` (corpus runner), `silicon/` (bench). Index: `tools/README.md` |
| `docker/` | `Dockerfile`+`run.sh` (image `open-atf150x-tools`: Rust + Yosys 0.33 + bitarray; `docker/run.sh cargo test --workspace`), `yosys/` (`open-atf150x-yosys`, pinned Yosys v0.71 + Rust), `sim/` (`open-atf150x-sim`: Verilator 5.054 (commit `bbb60a40`), iverilog, Yosys; via `tools/sim/run.sh`), `prjbureau/` (`open-atf150x-prjbureau`: Yosys 0.33, wine32, bitarray; own wine prefix) |
| `external/` | `external/prjbureau` is a **submodule** from the user's fork `git@github.com:teknoman117/prjbureau.git` (remote `origin`; whitequark's repository is remote `upstream`, push disabled), pinned at branch `atf1508as`; topic-branch worktrees in `.worktrees/prjbureau/` (ignored) track the fork; others git-ignored |
| `.worktrees/` | git-ignored worktrees (of this repository and of submodules, e.g. `.worktrees/prjbureau/<branch>`) |
| `.scratch/` | git-ignored scratch work and caches (e.g. `.scratch/flow/corpus*/results.md`, `.scratch/sim/`) |

## 5. Environment

* Host: Gentoo Linux. Docker 29.8 (podman also present). Rust 1.99 on the host (atmisp builds
  with host cargo too). MinGW `i686-w64-mingw32-gcc` for the ATMISP GUI driver.
* Wine prefix `~/.wine32` (32-bit): ATMISP 7.3 in `C:\ATMISP7` (note the upper case), WinCUPL with
  the vendor fitters in `~/.wine32/drive_c/Wincupl/WinCupl/Fitters/`. ATMISP's SVF output needs the
  GUI. `tools/fuzzers/common/drive.c` (installed as `C:\probe\drive.exe`) drives it inside
  `wine explorer /desktop=…` (control IDs: 1019 SVF path, 1020 write-SVF checkbox, 1022 revision combo
  sorted C=0/D=1, 1023 TCK, 1024 state reset, 1007 Run). The runner kills wineserver after each
  case. Only one wine job at a time (`pgrep -fa 'wine|ATMISP|fit15|drive.exe'` first). The
  containerised vendor fitter (`tools/fuzzers/common/fit.sh`) uses its own prefix.
* User data (not in the repo): CUPL designs in `~/Projects/pld/cpld/` (`atf1504as-*`, `atf1508as-*`;
  `motioncontroller` is unfinished), Xilinx Verilog in `~/Projects/pld/cpld/xilinx/` (`blink.v`,
  `spi-experiments-xilinx/spi-controller/spi_controller_mode0.v`). Datasheets: `/home/nlewis/Obsidian/
  Nathan/Reference/Datasheets/Unsorted/Atmel PLD Stuff/`. The only user-derived file in the repo is the
  Verilog port `atf15xx-synth/tests/designs/spi.*` (a regression test).

## 6. Conventions (the user's standing instructions)

* **Spec first**, normative and precise enough to implement from; every claim marked oracle-verified
  / labelled / external / inferred.
* **Black box only**: facts come from what the vendor programs write (`tools/fuzzers/`, one workflow
  per derived fact with a `README.md` and a `CLAUDE.md`); never look inside the vendor binaries.
* **Rust**, using widely used upstream crates (clap, chrono, serde_json, flate2, …), not hand-rolled.
* **Docker** for all external toolchain builds; pin versions and record commits.
* **Document everything** in `docs/worklog/` per its README (dated Status, scope, results with
  reproduce commands, evidence levels, decisions, journal incl. dead ends, open issues, artifacts),
  kept current while working, plus usage docs for every tool.
* **Keep `docs/STORY.md` up to date**: the narrative of the project for a blog post. New
  chapter per milestone, with surprises, mistakes and numbers.
* **Track all work in git**: commit after each milestone, stage only owned paths, `Co-Authored-By`
  trailer, no push or history rewrite without asking. External checkouts are kept as patches.
* **Time-bound all verification** (no unbounded proofs); report guarantee levels honestly.
* **Never program an image that disables JTAG** (fuse 74115 on the 1508 = 0, e.g. anything built with
  `jtag off`): recovery needs 12 V on OE1. Use `tools/silicon/program.sh` (runs `jtag_guard.py`).
* Never commit vendor files (Microchip samples, fitter/ATMISP binaries). User designs only with the
  user's agreement. Never push to prjbureau or contact upstream.

## 7. Findings to keep in mind

* **Vendor fitter defects** (our fitter avoids all three):
  1. register feedback of an output that has an OE is routed via the pin, so the logic breaks while
     the OE is off;
  2. the `-open_collector` list is silently truncated after 43 entries. **The user's
     `shift-register` board has OUT44–OUT63 push-pull, not open collector.**
  3. the pass-2 CASCADE refit (after "Placement fail") sets the wrong `Invert` polarity (seen on the
     user's `blink.v` on 1508).
* **ATMISP defects** are reproduced by default (`--strict` corrects the safe ones), §9.4.
* **PT numbering:** ATMISP's own Load-output "PT n MC m" labels are reversed for half the macrocells;
  prjbureau's/vendor numbering is the functional one.
* **Inferred, need silicon:** register power-up value (default 0; `--power-up fuse` alternative),
  reset-vs-preset priority (reset assumed), 1502AS GOE invert bits (prjbureau `C+10−n` vs vendor
  `C+3+n`), OE-PT5-when-in-sum = never enabled.
* prjbureau: fuse DB for 1502AS/BE, 1504AS/BE only (no 1508), no fitter, no nextpnr; its EDIF flow
  needs `aprim.lib` (ProChip build), which the user doesn't have.

## 8. Open issues / next steps (in order)

Plans: spec F9.5 (roadmap), F9.6 (Yosys plugin), `docs/plan/open-source.md` (upstream
contributions, repo split, OpenOCD driver, crate audit).

1. ~~Position-aware placement~~ **done** (F4.6a UIM-aware positions): the SELFPROGRAMBOARD
   Verilog port fits (60/64). Next: the mapping–placement loop through the plugin (F9.6 item 3).
   Boundary scan is done too (F2.10, `atf15xx-tool sample`).
2. ~~STA / `--fmax` (R5)~~ **done** (F8.5/F8.6, `atf15xx-sim timing`, flow `--fmax`/`--grade`,
   silicon-consistent). Open: clock skew between networks, async paths, datasheet fractional
   values instead of the vendor's whole ns.
2. **Roadmap R1–R6 closed** (F9.5; R4 via BDD toggles + carry chains, F9.8). Since: word-level
   mapping and `--map auto` (F9.9), timing targets (F8.7), library oracles
   (`docs/plan/libraries.md`), exact placement bound (F4.6c, `atf15xx-synth --features ilp`, bin `fit-partition-bound`), path
   independence (F9.7). Next: synthesis structure for dense designs (SELFPROGRAMBOARD 33 vs
   55.6 MHz from CUPL's netlist, F8.7, F9.10); typed netlist IR (also the serde item below); the
   mapping–placement loop in the plugin.
2. **Crate hygiene** (plan §5): `jedec` crate for reading (verified identical on 134 JEDs),
   `bitvec`, `thiserror`, `svf` parser in tests, serde structs for the Yosys JSON.
3. ~~XOR synthesis (R2)~~ **done** (F4.6b; XOR semantics F2.8 row 6, silicon). Possible
   extension: use a cascade lender's own PT2 output (`x = PT2 ⊕ Invert`, F2.8) as a packing gain.
4. **Packing**: P3 (output register + buried node), P1 eligibility (AP via PT5, D via foldback).
5. **Static timing report + `--fmax`** (F8.5), silicon check.
6. **Coarse-level mapping** through the plugin (F9.3 B + C).
7. Upstream: prjbureau contributions (1508 database, BE programming, silicon facts), OpenOCD
   `atf15xx` PLD driver — each only with the user's go-ahead.
8. Silicon still to do: cold power-up, slow vs fast slew, 1502AS GOE invert bits.

## 9. Quick checks

```
docker/run.sh cargo test --release --workspace            # everything (36 tests incl. 145 SVF vectors)
tools/sim/tests/run_tests.sh 60                            # equivalence engine suite (12 checks, ≤60 s each)
tools/sim/run_designs.sh spi                               # user design: vendor + our JED vs .tt2
python3 tools/fuzzers/atmisp-svf/verify_jeds.py some.jed               # live check of atmisp vs ATMISP (wine)
python3 tools/fuzzers/fitter-conformance/verify_tt2.py <dir>                   # our fitter vs vendor fitter (wine)
```
