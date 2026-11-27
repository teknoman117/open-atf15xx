# Handoff: open-atf150x

**As of 2026-11-26** (release 0.1.0). Project rules for every session: the root `CLAUDE.md`. This is the starting point for anyone (human or agent) picking up this
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
| programming + in-circuit debugging | `atf15xx-tool` (module `dbg`, feature `jtag`; commands `program`, `erase`, `part`, `idcode`, `probe`, `sample`, `watch`, `capture`, `drive`, `seq`, `release`/`sample` inside `seq`) | **first version done**; `program`/`erase` checked on the bench (13 s / 6 s at 4 MHz, verify failure detected) | boundary-scan SAMPLE/watch/VCD capture/EXTEST through OpenOCD, cell maps generated at build time from the prjbureau BSDLs (all 12 part/package pairs agree with the device tables); bench: named readout of a 40-bit counter 8/8 (spec D1, log 08) |
| `.jed` simulation / equivalence | `atf15xx-sim`, `tools/sim/` | **done** | self-checks (emitter vs iverilog, the former Python model 95/95 until 2026-11-25), mutation tests; Verilator engine 100/100; `tools/sim/tests/run_tests.sh` 12/12 |
| Architecture / fuse semantics | `atf15xx-arch`, `docs/architecture/fitter/` | **done**, with evidence level per item (incl. UIM routing fuses, F2.4) | oracle experiments (`tools/fuzzers/`), prjbureau database and cross-check, silicon |
| Timing data | `docs/architecture/fitter/08-timing.md`, `atf15xx-arch/data/timing.json` | **data done; no analyser yet** | derived from the datasheets and the vendor's SDF output for all 3 families × 5 grades (`tools/fuzzers/fitter-timing`); 6,895/6,895 vendor SDF delays explained |
| Silicon | `tools/silicon/`, log 06 | **first device programmed and measured automatically** (Arduino bench, log 06): our `.tt2` fits and the Verilog flow work on silicon; reset priority, post-ISP register state, GCLR polarity, foldback verified (F2.9). Our Verilog flow's deep counters fail at 66.6 MHz (timing) | programming + independent readback verify 0 errors; verify negative control fails as it should; JTAG still answers |

`cargo test --release --workspace`: 46 passed, 0 failed (2026-11-26). Git: `main` (one release
commit for 0.1.0, tag `v0.1.0`, plus later commits), remote `origin` = github.com/teknoman117/open-atf15xx
(public; push only when the user asks); the pre-0.1.0 history is the local branch `pre`, never pushed.

## 3. The whole flow (Verilog → SVF)

```
design.v ─Yosys─► netlist (ynet) ─importer─► fitter ─► fuse map ─► .jed ─atf15xx-tool─► .svf ─► chip
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
   options) → fuse assembly → `.jed` → `atf15xx-tool`'s `isp` module → `.svf`. Outputs in `out/`: `.jed`,
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
4. **Program**: `atf15xx-tool --part ATF1508AS-PLCC84 program out/design.jed` (starts OpenOCD,
   checks the IDCODE, erases, programs and verifies; `--interface`/`--openocd-file`/`--openocd-cmd`
   select the adapter; warns on JTAG-off images), or play the `.svf` with any SVF player. Silicon:
   ATF1508AS on the bench (log 06). On the bench use `tools/silicon/program.sh`, which refuses
   JTAG-off images.

Alternatives: CUPL `.tt2` → `atf15xx-fit design.tt2 -d ATF1504AS-PLCC44` (our fitter; `fit1504` takes the vendor's options) → `atf15xx-tool svf -d
ATF1504AS:pv:design.jed -o design.svf`. A vendor `.jed` → `atf15xx-tool svf` alone. Chain files:
`atf15xx-tool svf chain.chn --drive C=$HOME/.wine32/drive_c -o x.svf`.

## 4. Where things are

| Path | Contents |
|---|---|
| `docs/architecture/` §1–§11 | normative spec of ATMISP 7.3 (chain files, JEDEC reader, fuse→row maps, SVF writer, algorithms, Load writer, validation/defects, CLI, conformance) |
| `docs/architecture/fitter/` F1–F7 | chip architecture, complete fuse map with evidence levels (§F2.8 functional semantics table), `.tt2` grammar, fitting algorithm, vendor outputs (+ `.map.json` §F5.4), conformance, Yosys flow |
| `docs/architecture/sim/` | simulator model, Verilog emitter, equivalence method, limitations, harness guide (stable entry point), Verilator engine |
| `docs/prjbureau/README.md` | prjbureau study (how it's meant to be used, cross-checks) |
| `docs/worklog/` | per-workstream logs (00 project, 01 ATMISP/SVF, 02 fitter, 03 prjbureau, 04 simulator, 05 Verilog flow, 06 silicon, 07 Yosys history, 08 debugger); conventions in its README |
| `Cargo.toml` | virtual workspace: `[workspace.package]` (version, edition, licence) and `[workspace.dependencies]` (all shared and internal crates); the members are listed explicitly |
| crates (top-level directories) | `atf15xx-arch` (architecture, generated at build time), `atf15xx-fit` (fitter: netlist optimisation `opt`, strategy search `search`, capacity model `capacity`, fitting `fit/` with one module per pass, `.tt2` in and out; binaries `atf15xx-fit`, `fit1502/1504/1508`), `atf15xx-synth` (Verilog front end: Yosys, import, `wordmap`, constraints, outputs; `ynet` typed netlist; feature `yosys` = the Yosys plugin, built in the Yosys image), `atf15xx-sim` (simulator, timing), `atf15xx-tool` (SVF/chain/JEDEC with its 145 oracle vectors, programming, in-circuit debugger) |
| `examples/` | `spi` (ATF1504AS + bench ATF1508AS driven over JTAG, Uno firmware), `spm` (SELFPROGRAMBOARD), `shift-register` (breakout-atf1508as demo); each README names its source |
| `tools/` | `fuzzers/` (every analysis that runs the vendor programs, one workflow per derived fact, each with `README.md` + `CLAUDE.md`; shared wrappers in `fuzzers/common/`), `sim/` (`equiv.py`, `vcompare.py`, the Verilog → `.tt2` route), `flow/` (corpus runner), `silicon/` (bench). Index: `tools/README.md` |
| `docker/` | `Dockerfile`+`run.sh` (image `open-atf150x-tools`: Rust + Yosys 0.33 + bitarray; `docker/run.sh cargo test --workspace`), `yosys/` (`open-atf150x-yosys`, pinned Yosys v0.71 + Rust), `sim/` (`open-atf150x-sim`: Verilator 5.054 (commit `bbb60a40`), iverilog, Yosys; via `tools/sim/run.sh`), `prjbureau/` (`open-atf150x-prjbureau`: Yosys 0.33, wine32, bitarray; own wine prefix) |
| `external/` | `external/prjbureau` is a **submodule** from the user's fork `git@github.com:teknoman117/prjbureau.git` (remote `origin`; whitequark's repository is remote `upstream`, push disabled), pinned at branch `atf1508as`; topic-branch worktrees in `.worktrees/prjbureau/` (ignored) track the fork; others git-ignored |
| `.worktrees/` | git-ignored worktrees (of this repository and of submodules, e.g. `.worktrees/prjbureau/<branch>`) |
| `.scratch/` | git-ignored scratch work and caches (e.g. `.scratch/flow/corpus*/results.md`, `.scratch/sim/`) |

## 5. Environment

* Host: Gentoo Linux. Docker 29.8 (podman also present). Rust 1.99 on the host (everything but the
  Yosys plugin builds with host cargo). MinGW `i686-w64-mingw32-gcc` for the ATMISP GUI driver.
* Wine prefix `~/.wine32` (32-bit): ATMISP 7.3 in `C:\ATMISP7` (note the upper case), WinCUPL with
  the vendor fitters in `~/.wine32/drive_c/Wincupl/WinCupl/Fitters/`. ATMISP's SVF output needs the
  GUI. `tools/fuzzers/common/drive.c` (installed as `C:\probe\drive.exe`) drives it inside
  `wine explorer /desktop=…` (control IDs: 1019 SVF path, 1020 write-SVF checkbox, 1022 revision combo
  sorted C=0/D=1, 1023 TCK, 1024 state reset, 1007 Run). The runner kills wineserver after each
  case. Only one wine job at a time (`pgrep -fa 'wine|ATMISP|fit15|drive.exe'` first). The
  containerised vendor fitter (`tools/fuzzers/common/fit.sh`) uses its own prefix.
* User data (not in the repo): CUPL designs in `~/Projects/pld/cpld/` (`atf1504as-*`, `atf1508as-*`;
  `motioncontroller` is unfinished), Xilinx Verilog in `~/Projects/pld/cpld/xilinx/` (`blink.v`,
  `spi-experiments-xilinx/spi-controller/spi_controller_mode0.v`). Datasheets: the public Microchip
  ones (the user also keeps a local collection). User-derived files in the repo: the regression test
  `atf15xx-synth/tests/designs/spi.*` and the ports in `examples/` (ISC, with the user's agreement).

## 6. Conventions (the user's standing instructions)

The rules are in the root **`CLAUDE.md`** (loaded into every Claude Code session): black box
only with one `tools/fuzzers/<workflow>/` per derived fact; no vendor files in git; prjbureau
only through `atf15xx-arch`'s build script, fork branches in `.worktrees/`, no `patches/`; spec
first with evidence levels; Rust with upstream crates; Docker for external toolchains;
document everything (worklog, STORY, this file); git on `main` without `Co-Authored-By`, push
only when asked; time-bounded verification reported honestly; bench safety (never program a
JTAG-off image on the bench); OpenOCD as a separate process only; privacy.

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
* prjbureau upstream: fuse DB for 1502AS/BE, 1504AS/BE only, no fitter, no nextpnr; its EDIF flow
  needs `aprim.lib` (ProChip build), which the user doesn't have. The user's fork (branch
  `atf1508as`, our submodule) adds the ATF1508AS database from its own fuzzers and a `.tt2` route.

## 8. Open issues / next steps (in order)

Plans: spec F9.5 (roadmap), F9.6/F9.7 (Yosys plugin), `docs/plan/open-source.md` (upstream
contributions, repo split, OpenOCD driver, crate audit). Done since the roadmap (R1–R6, F9.5):
position-aware placement, STA and `--fmax`, XOR synthesis, packing P1/P3, word-level mapping and
`--map auto` (F9.9), timing targets (F8.7), the exact placement bound (F4.6c), path independence of
the CLI (F9.7), the typed netlist IR (F9.7 item 1), typed fit errors (F9.9), programming over JTAG.

1. **Synthesis structure for dense designs**: SELFPROGRAMBOARD now 52.6 MHz from Verilog vs 55.6
   MHz from CUPL's netlist (F8.7, F9.10).
2. **Mapping for wide cells** (`docs/plan/partitioning.md` §5): the hard placement failures are
   cells that read too many signals. Done for compares (F9.12, 2026-11-26: pieces in two forms);
   next the same for wide adders / accumulators (their remaining failures are pins, see the
   sweep) and wide `$pmux` / mux trees.
3. **Mapping–placement loop in the plugin** (F9.7 item 3): re-run Yosys passes on an overflowing
   block's logic between fit attempts. Its prerequisites are in place (typed netlist IR, typed fit
   errors `FitErrorKind`, both 2026-11-26); next is choosing what to re-map on a `Routing` /
   `Capacity` failure (F9.9 item 2: cost queries such as block fan-in and PT demand).
4. **Canonical cell order** for the plugin: results there still depend on the path a source was
   read by (Yosys orders cells by path-carrying names, F9.7); the CLI avoids it by reading
   relative paths.
5. **Crate hygiene** (plan §5), done 2026-11-27 except: `Result<_, String>` in the library crates
   (typed errors where a caller needs to tell failures apart), and an upstream fix for `jedec`
   0.1.0's default-only files (worked around in `atf15xx_sim::parse_jed_bytes`).
6. **Cascade follow-ups** (F4.3, `tools/fuzzers/fitter-cascade`): the vendor's input placement
   order (it places the data inputs of the cascade designs as D1…D5, D0), so that chained designs
   become fuse-identical; 30-term sums fail our UIM routing where the vendor fits; wide cells that
   reach the UIM repair loop cost time on designs that fail anyway (20-bit compare, gate mapping:
   69 s).
7. **XOR extension**: use a cascade lender's own PT2 output (`x = PT2 ⊕ Invert`, F2.8) as a
   packing gain.
8. **Coarse-level mapping** through the plugin (F9.3 B + C).
8. Upstream: prjbureau contributions (1508 database, BE programming, silicon facts), OpenOCD
   `atf15xx` PLD driver — each only with the user's go-ahead.
9. Silicon still to do: cold power-up, slow vs fast slew, 1502AS GOE invert bits.

## 9. Quick checks

```
cargo test --release --workspace                          # everything (46 tests incl. 145 SVF vectors)
tools/sim/tests/run_tests.sh 60                            # equivalence engine suite (12 checks, ≤60 s each)
tools/sim/run_designs.sh spi                               # user design: vendor + our JED vs .tt2
python3 tools/fuzzers/atmisp-svf/verify_jeds.py some.jed  # live check of atf15xx-tool vs ATMISP (wine)
python3 tools/fuzzers/fitter-conformance/verify_tt2.py <dir>                   # our fitter vs vendor fitter (wine)
```
