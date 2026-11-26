# 00 — Project overview

## Status

**2026-11-24.** Goal: an open, maintainable toolchain for Microchip/Atmel ATF15xx CPLDs,
**Verilog → Yosys → fitter → `.jed` → SVF**, because the vendor toolchain is old, Windows-only, and
its Verilog compiler can no longer be obtained.

| Stage | State | Log |
|---|---|---|
| `.jed` → SVF (ATMISP 7.3 replacement, `atf15xx-tool`) | **done**, byte-identical to ATMISP | [01](01-atmisp-svf.md) |
| `.tt2` → `.jed` (vendor fitter replacement, `fit1502/1504/1508` binaries of `atf15xx-fit`) | **done for the user's finished designs**: 7/8 real designs fit and match their `.tt2` in simulation (`motioncontroller`, unfinished, does not); 96.8/98.0/91.2 % byte-identical to the vendor on synthetic corpora (1502/1504/1508) | [02](02-fitter.md) |
| prjbureau evaluation; Verilog → SOP (`.tt2`) prototype | **done** | [03](03-prjbureau.md) |
| `.jed` simulator / Verilog equivalence checker (`atf15xx-sim`) | **done**, incl. a time-bounded Verilator engine with register-correspondence state injection (wide counters PROVEN in ~1 s); originally: `tools/sim/equiv.py` (formal + simulation) against Verilog, `.tt2` or another `.jed`; our fitter's output PROVEN/BOUNDED on every design it fits | [04](04-simulator.md) |
| Fully open Verilog → `.jed` → SVF | **done (first version)**: `atf15xx-synth build design.v -d DEVICE -o out/` (pinned Yosys v0.71 → our fitter → `.jed` + `.svf`); 37/38 corpus runs PROVEN against the Verilog, incl. the user's `blink.v` and `spi_controller_mode0.v`; not silicon-tested | [05](05-verilog-flow.md) |

## Components

```
Verilog ──Yosys (tools/sim/synth_atf15xx.ys)──► SOP + storage netlist (Yosys JSON)
            │                                          │
            │ tools/sim/yosys2tt2.py             │ (planned) direct Rust front end
            ▼                                          ▼
          .tt2 ──► fitter: fit1502|1504|1508 (library: atf15xx-fit) ──► .jed
                    (vendor fit150x under wine = oracle)                               │
                                                                                       ▼
                      atf15xx-arch (architecture, fuse map, JED decode)     atmisp ──► .svf
                      atf15xx-sim  (JED → model/Verilog; equivalence)      (ATMISP 7.3 = oracle)
```

Specifications: `docs/architecture/` (§1–§11 SVF/ATMISP, `fitter/` F1–F7, `sim/` once written);
prjbureau study: `docs/prjbureau/`; work logs: `docs/worklog/`; tooling: `tools/` (see
`tools/README.md`); containers: `docker/`.

## Method (all workstreams)

1. **Oracle first**: every behaviour is established by running the original vendor tool (ATMISP 7.3,
   fit150x 1.8.7.8) under wine on generated inputs and comparing outputs byte for byte.
2. **Fuzzing**: each derived fact has its own workflow in `tools/fuzzers/<workflow>/` (generated
   inputs, vendor runs, decoding), reproducible by others.
3. **Specifications are normative** and say how each claim is known (oracle-verified / labelled /
   external / inferred).
4. **Implementations are tested against recorded oracle outputs** (`cargo test`) and can be checked
   live against the vendor tools (`tools/fuzzers/atmisp-svf/verify_jeds.py`, `verify_tt2.py`).
5. **External toolchains are built and run in Docker** (`docker/`). Vendor files never enter the
   repository; test vectors are synthetic.

## Decisions (project level)

- **2026-11-25: prjbureau data comes in through atf15xx-arch at build time** (user: "any data we
  derive from prjbureau should come in through atf15xx-arch ... no runtime parsing of json tables.
  no storage of prjbureau assets in our projects essentially, only derivatives"). The crate's
  build script generates Rust statics from the pinned submodule (database.json, BSDL files) and
  from our own `data/devices.json` and `data/timing.json`. Supersedes the 2026-11-25 decision to
  commit a generated `arch.json` (rejected alternative then: "a build.rs reading the submodule").
  Consequence: every build needs the submodule; `atf15xx-arch` cannot be published to crates.io
  without shipping the generated code (docs/plan/prjbureau-import.md §5).
- **2026-11-25: Black box only** (user request: "we want to be able to reproduce the entire
  fuzzing flow we took for others to see"). Every fact comes from what ATMISP and the fitters write for inputs we generated, organised as one
  workflow per derived fact in `tools/fuzzers/<workflow>/` (scripts + `README.md` + `CLAUDE.md`), so
  the whole fuzzing flow can be reproduced by others.
- **2026-11-19: Specification before implementation**, in `docs/architecture/`, precise enough to
  build from (user requirement).
- **2026-11-19: Rust for the implementations**, using widely used upstream crates (user requirement;
  see the `prefer-upstream-crates` memory).
- **2026-11-23: Cargo workspace**: `atf15xx-tool` (root crate), `atf15xx-arch` (shared architecture
  model), `atf15xx-fit` (fitter library), `fit1502/1504/1508` (per-family binaries), `atf15xx-sim`
  (simulator).
- **2026-11-23: prjbureau is a cross-check and reference, not a dependency.** It has no fitter and no
  ATF1508 data; its EDIF flow needs a fitter build the user lacks. Our changes to it stay on a local
  branch (`external/prjbureau`, `open-atf150x`) until the user decides otherwise.
- **2026-11-23: Yosys JSON (SOP + legal storage) is the front-end interchange format.**
- **2026-11-23: Validation of fitted designs is functional**, by simulation and formal
  equivalence against the source Verilog (simulator workstream), as well as byte identity with
  the vendor where that's achievable.

## Roadmap

1. ~~Close the `.tt2`-path gaps~~: done 2026-11-24 (02). `motioncontroller` is left until the user
   finishes the design.
2. ~~Simulator and equivalence checker (04)~~: done 2026-11-24. Open: the OE-PT5-in-sum decode
   ambiguity, handed to 05.
3. ~~Fully open Verilog flow~~: first version done 2026-11-24 (05). Next for production use: a
   UIM-aware placer, multi-neighbour cascade, better logic optimisation, a silicon test, and a timing model.
4. Possible later work: a prjbureau ATF1508 database from our oracle data; silicon testing of the
   inferred and disputed semantics (1502AS GOE invert, reset/preset priority, power-up state).

## Journal

- **2026-11-25**: Release 0.1.0 (user request): the workspace version is 0.1.0 and `main` is one
  commit, tagged `v0.1.0`. Licence: ISC (user decision: "the same that yosys and nextpnr use"; was
  MIT OR Apache-2.0); the examples ported from the user's MPL-2.0 CUPL sources are relicensed ISC too (user request). `examples/shift-register` added (user request): the
  breakout-atf1508as board's example (`Example/` @ `b473233`) ported to Verilog with the board's
  Arduino sketch; 128/128 macrocells, PROVEN equal to its Verilog and to the original CUPL
  design's `.tt2` (all 64 outputs open collector); on the 44 outputs the vendor makes open
  collector, PROVEN equal to the vendor JED (the vendor leaves OUT44-63 push-pull). The clones
  `external/breakout-atf1508as` and `external/P80C550-SPM-CPLD` were removed; the example READMEs
  say where the designs came from.
- **2026-11-25**: the SPI example carries its own Uno firmware (user request): `examples/spi/uno`
  (slave + reset/tie lines, Makefile) and `examples/spi/host/uno.py`; the SPI additions are taken
  back out of the bench firmware (`tools/silicon/bench` is bench 3 again). Re-tested: 64/64 both
  ways at 1.02 kHz and 254 Hz.
- **2026-11-25**: `examples/` (user request): `examples/spi` (the user's `atf1504as-spi` as a
  parameterised Verilog core with an ATF1504AS variant = the original, PROVEN equal to the old
  port, and an ATF1508AS bench variant: 17-bit prescaler for 1.02 kHz / 254 Hz SCK at 66.6 MHz,
  registered tick, synchronised strobes; host tooling that drives the bus over JTAG with the Uno
  as an echoing SPI slave) and `examples/spm` (SELFPROGRAMBOARD, moved from
  `tools/silicon/designs/spm`; now fits the open flow, 60/64). Bench 2026-11-25: 64/64 bytes both
  ways at 1.02 kHz and 254 Hz. `atf15xx-tool seq` gained `release`, `sample` and `scan N`; bus
  names accept `D0`-style bits. `atf15xx-synth` got `default-run` (the `ilp` binary had broken
  plain `cargo run -p atf15xx-synth`).
- **2026-11-25**: `atf15xx-ffi` removed (no caller of its C ABI was left); `atf15xx-yosys` merged into
  `atf15xx-flow`, renamed `atf15xx-synth` (user request): the plugin core is `src/plugin.rs` +
  `src/plugin/rtlil.{cc,h}` behind feature `yosys` (optional `cxx` / `cxx-build`; `build.rs` compiles
  the bridge only with the feature), so the workspace builds without Yosys and the plugin is
  `cargo rustc -p atf15xx-synth --features yosys --lib --crate-type staticlib` inside the Yosys
  image (`tools/yosys-plugin/build.sh`). The part-name parser moved to `atf15xx_arch::part`.
  Checked: workspace tests pass; the plugin builds; `check_corpus.sh` against a fresh CLI corpus
  build: 34/34 fuse-identical (against the stale 2026-11-24 corpus 11 differed, from fitter changes
  since).
- **2026-11-25**: Python duplicates of the Rust tools retired (user request), and with them
  `tools/arch/archdata.py` and atf15xx-arch's JSON export binary: scripts now ask the Rust tools
  they already run, in plain text. New `atf15xx-sim` subcommands (`src/info.rs`): `device D`
  (pins), `fuses F` (every fuse field, UIM / GOE inputs), `mc X.jed -d D --mc N` (a macrocell's
  bits and PTs; `--set MC16.CasMux=1 -o OUT.jed` edits in place, checksum recomputed) and
  `stats X.jed -d D`. Removed: `tools/sim/jedsim.py` (the Python model) + `xcheck_jedsim.py`,
  `tools/silicon/bscan.py`, `bscan_dump.py`, `mcbits.py`, `patch_jed.py`, `bench/exp_bscan.py`,
  `bench/exp_dbg_vcnt40.py`, `tools/fuzzers/fitter-conformance/check_user_designs.sh` (duplicate of
  `tools/sim/run_designs.sh`). Ported to `atf15xx-tool sample --json`: `bench/exp_vcnt40.py`,
  `exp_inreg_cascade.py` (not rerun on the bench). Checks before deleting: `stats` = the Python
  counts on 95 conformance + 300 vendor foldback JEDs; `mc` show = `mcbits.py show`; `mc --set`
  byte-identical to `patch_jed.py`; `device`/`fuses` = the exported tables (13 devices, 3 families);
  `regions.json` reproduced; simulator suite `tools/sim/tests/run_tests.sh` 12/12 correct.
- **2026-11-25**: atf15xx-arch generates its tables at build time (decision above). New
  `atf15xx-arch/build/` (`fuse_map.rs`: the former `tools/arch/import_prjbureau.py`, ported; 
  `bsdl.rs`: the former runtime BSDL reader of `atf15xx-tool`; `devices.rs`, `timing.rs`), shared
  McBit layout `src/layout.rs`, new modules `atf15xx_arch::{bsdl, timing}`. Removed:
  `atf15xx-arch/data/arch.json`, `tools/arch/import_prjbureau.py`, `xcheck_prjbureau.py` (its
  check is now in the build), `tools/silicon/bscan_map.py` + `bscan_maps/`, the debugger's
  `--bsdl-dir`/`ATF15XX_BSDL_DIR`, the runtime JSON in `atf15xx-sim` (timing) and the `bsdl`
  dependency of `atf15xx-tool`. Python tools get the tables through `tools/arch/archdata.py` (runs
  the crate's export binary `atf15xx-arch json|bscan PART`, caches under `target/`). Submodule
  pin unchanged (`e634784`, branch `atf1508as`, now recorded in `.gitmodules`). Checks: generated
  tables = the committed `arch.json` in every family field and device; generated
  ATF1508AS-PLCC84 map = the silicon-verified bench map in all 352 cells; all 12 parts build;
  workspace tests pass; `pinroles` vendor identity 180/876/856 through archdata; `regions.json`
  reproduced with the PT numbering from atf15xx-arch.
- **2026-11-25**: `patches/` removed (user: "the patches/ folder should not exist"). The prjbureau
  branches live on the user's fork only; all seven are checked out as worktrees under
  `.worktrees/prjbureau/<branch>` tracking `origin/<branch>`. The branch index and draft PR texts
  moved to `docs/prjbureau/branches.md`; `refresh.sh` is gone with the patches.
- **2026-11-25**: `patches/prjbureau/` cleaned up (user request): every directory holds only its
  branch's own commits (`tt2-toolchain` no longer repeats the two fix patches it stacks on),
  generated with `--zero-commit` by the new `patches/prjbureau/refresh.sh` from the fork; the
  README documents the stacking and is up to date (1504AS full run, fuseconv-be evidence). Every
  branch rebuilt from the patches onto `8b8a971` has exactly the fork branch's tree.
- **2026-11-25**: `atf15xx-oracle` is no longer a crate: it is the binary `fit-partition-bound` of
  `atf15xx-synth`, behind the optional `ilp` feature (user request), so the default workspace build
  no longer compiles HiGHS. Renamed because "oracle" in this project means the vendor programs.
  `cargo run --release -p atf15xx-synth --features ilp --bin fit-partition-bound -- partition ...`;
  checked on a feature design (ILP optimum = ours).
- **2026-11-25**: Black-box derivation of every fact (decision above).
  (1) ATMISP's fuse maps, new `tools/fuzzers/atmisp-fusemap`: 127 JEDEC files encoding every
  fuse's index bit by bit, through ATMISP; `atf15xx-tool` writes byte-identical SVF for 127/127
  across the seven device names, the register layout and the fuses in no row equal spec
  §4.4/§5.3. (2) The timing tables, `tools/fuzzers/fitter-timing/derive_timing.py`: the public
  datasheets (incl. the ATF1502AS and ATF1502ASV ones, downloaded from microchip.com) and the
  fitters' SDF output; 6,895/6,895 SDF delays explained; parameters no SDF delay shows (1502 -25
  column, 1502 tCNT -20, 1504 tPD2 -25) are the datasheet's. (3) `regions.json` from ATMISP's
  Load output with prjbureau's PT numbering, checked against the fitter's labels on 1001 rows.
  (4) `arch.json` `devices` from the pin experiments (`fitter-pins/export_devices.py`). (5) The
  UIM / GOE input tables from prjbureau. (6) The 1508BE JTAG fuse (only our warning uses it)
  follows the layout of every other family (configuration start + 33).
  *Reorganised:* vendor-running tools and their dependencies moved to `tools/fuzzers/`: `common/`
  (ATMISP and fitter runners, `drive.c`, `fit.sh`, `tt2gen.py`, `model.py`), `atmisp-svf`,
  `atmisp-fusemap`, `fuse-regions`, `fitter-pins`, `fitter-pterms`, `fitter-macrocell`,
  `fitter-foldback`, `fitter-timing`, `fitter-conformance`, `prjbureau`; `tools/oracle/`,
  `tools/fitter-re/`, `tools/prjbureau/` are gone (`jedsim.py`, `yosys2tt2.py`,
  `synth_atf15xx.ys` → `tools/sim/`, `xcheck_prjbureau.py` → `tools/arch/`).
  *Checks:* workspace tests 45/45; vendor identity unchanged (feat + ptcol2 + pinroles: 2017 /
  4025 / 6155 fuse-identical); `arch.json` regenerates identically from the fuzzers' outputs;
  `vendor_labels.json` regenerated (more labels from the grown corpus, none conflicting).
  *Follow-up (user approved):* the prjbureau branch `fuseconv-be` was rewritten so that its
  commit message, `util/device_be.py` and `docs/jtag/as.rst` describe the black-box derivation
  (`ffed17f`, `d3bbcc1`; tests 8/8 in the container) and force-pushed to the fork.
- **2026-11-25**: Repository cleanup (user request). Every black-box fuzzer moved to
  `tools/fuzzers/<name>/`, each with a `CLAUDE.md` holding the instructions for supervising a run:
  `fitter/` (the `exp_*.py` vendor-fitter experiments + `model.py`, `regions.py`; `ROOT` is now
  `<repo>/.scratch`, where their inputs really are, and `exp_dedic.py` reads the GOE table from
  `arch.json`), `fitter-timing/` (`sdf_corpus.sh`,
  `check_sdf_timing.py`), `atmisp/` (`gen_cases*.py`, `assemble.py`), `prjbureau/` (`suite.sh`,
  `dbdiff.py`, `stress_prefetch.py`, until now only in `.scratch`). There are no Rust fuzzers (the
  randomized Rust checks are plain `cargo test`s). The prjbureau worktrees moved from
  `external/prjbureau-wt/` to `.worktrees/prjbureau/` (git-ignored) with `git worktree move`;
  their caches came along (315,596 entries in `atf1508as`). Checks: every moved script compiles and
  resolves its imports and inputs; `gen_cases1.py` writes its 48 cases; `arch.json` regenerates
  byte-identically; workspace tests pass.
- **2026-11-25**: `fit1502`/`fit1504`/`fit1508` are no longer crates: they are binaries of
  `atf15xx-fit` (`atf15xx-fit/src/bin/`, same names, same command lines; user request).
- **2026-11-25**: Crates moved from `crates/` to the top level (user request): `atf15xx-arch/`,
  `atf15xx-fit/`, ..., `atf15xx-tool/`, `fit1502/` etc. next to `docs/` and `tools/`; workspace members listed
  explicitly. Regression unchanged (vendor identity on all corpora, user designs 8/8, tests 45/45,
  corpus identical, plugin == CLI 34/34, equivalence suite).
* **2026-11-24** — Renamed the project `open-atmisp7` → **open-atf150x** (user request), and the
  ATMISP crate `open-atmisp` → **`atf15xx-tool`** (binary `atf15xx-tool`, library `atf15xx-tool`), moved from the
  repository root to `atf15xx-tool`. Root `Cargo.toml` is now a virtual workspace
  (`members = ["crates/*"]`, `[workspace.package]`, all shared and internal dependencies in
  `[workspace.dependencies]`). Docker images retagged `open-atf150x-*` (the old tags remain);
  local prjbureau branch renamed `open-atf150x`. All docs, logs included, now use the new names
  (so commands such as `target/release/atf15xx-tool` work as written); only the generated test vectors'
  header comments (`generated by open-atmisp7 tt2gen`) keep the old name, as recorded data.
  The directory moved to `open-atf150x` (next to the old one); a temporary symlink `open-atmisp7 ->
  open-atf150x` was removed at the user's request.
  `.scratch/venv` scripts were re-pointed. Re-verified from the new path: tests 34/34, plugin
  build, user designs 8/8, `vxor` PROVEN.

- **2026-11-24**: First silicon (06): the user's ATF1508AS-PLCC84 breakout programmed with the user's
  `blink` fitted by our `fit1508`, SVF by `atf15xx-tool`, OpenOCD + FT232H; 0 errors, readback verify
  OK. JTAG-enable fuse traced (1508 fuse 74115 = SVF special-register bit 2) and guarded
  (`tools/silicon/jtag_guard.py`). Awaiting the user's confirmation that the LED blinks.
- **2026-11-24**: Timing (02, main session): vendor speed-grade tables extracted from all three
  fitters, vendor SDF composition verified on 260 vendor SDFs, datasheet cross-checks; spec F8
  (`docs/architecture/fitter/08-timing.md`), data `atf15xx-arch/data/timing.json`. Confirmed
  for the user that the UIM (interconnect → 40 block inputs) fuses are fully mapped (F2.4).
- **2026-11-24**: Fitter workstream closed the `.tt2` gaps: logic doubling, foldback expanders on
  occupied macrocells, and the 1508 mismatch, which was a model gap (tied-high latch enable, §F2.8
  rule 10). Found two vendor-fitter defects: register feedback via the pin on OE'd outputs, and the
  `-open_collector` list truncated after OUT43. The latter means the user's `shift-register` board
  has OUT44–OUT63 push-pull. Started the open Verilog flow workstream (05).
- **2026-11-24**: Simulator workstream (04) delivered a stable equivalence entry point
  (`tools/sim/equiv.py`, formal + simulation). It found a third vendor-fitter defect: on the user's
  Xilinx `blink.v` retargeted to ATF1508AS via `yosys2tt2`, the vendor's pass-2 cascade fit inverts 4
  LED registers, so the JED doesn't implement the design. Power-up value remains inferred (default 0;
  `--power-up fuse` alternative) pending silicon. The user's Xilinx `blink.v` and
  `spi_controller_mode0.v` became the target designs for the open Verilog flow (05); our fitter
  doesn't fit them yet (capacity).
- **2026-11-24**: Flow (05) fixed wide functions (`8b4b08b`): no panic on >64-input functions,
  cells with more than 40 inputs rejected with a clear error, collapse bounded (32 inputs) so
  carry chains stay as buried nodes. A plain behavioural 64-bit counter (enable, sync clear, async
  reset) fits ATF1508AS-PLCC84 in 87 MCs. Main re-verified: PROVEN in 2.6 s.
- **2026-11-24**: Put the project under git (branch `main`, no remote), with one commit per workstream.
  prjbureau changes are kept as `patches/prjbureau/`. A fresh clone passes all 31 tests.
- **2026-11-24**: Simulator (04): time-bounded Verilator engine (`vcompare.py`, `equiv.py
  --sim-engine verilator --budget S`), ABC dprove correspondence proofs first, state injection from
  `.map.json`; explicit `GUARANTEE:` line. Main re-ran `tools/sim/tests/run_tests.sh`: 12/12 correct,
  each ≤ 60 s. It found a fitter crash on functions with > 64 inputs (logic.rs:133), assigned to 05.
- **2026-11-24**: Open Verilog flow (05) done: pinned Yosys v0.71 (`abc -sop` was removed in 0.69,
  so the importer builds the covers from Yosys gates), Rust Yosys-JSON front end, `atf15xx-synth` driver,
  `<top>.map.json` register map (§F5.4). It found and fixed two fitter bugs (mixed-polarity GCLK, PT
  power-down killing a PT5 OE). OE-PT5-in-sum settled as "never enabled" [I]. Main session
  re-verified `blink.v` end to end (PROVEN in 1.5 s; flow SVF = CLI SVF). User asked for a
  time-bounded Verilator comparison harness; assigned to 04.
- **2026-11-24**: Simulator workstream finished. The main session independently re-ran `spi`: vendor
  and our JED both PROVEN equal to the `.tt2`. Workspace tests: 31 passed.
- **2026-11-23**: User set the end goal (Yosys → CPLD). Started a fitter black-box analysis agent
  (02), a prjbureau agent (03) and a simulator agent (04). Paused prjbureau work in 02 until 03
  finished. Fixed the `atf15xx-tool` Windows-path bug (01). Introduced these work logs.
- **2026-11-19**: ATMISP 7.3 analysed black-box; spec §1–§11 and `atf15xx-tool` completed and verified (01).
