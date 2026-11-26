# CLAUDE.md: rules for every session and agent in this repository

## Orientation

open-atf150x is an open toolchain for the Microchip/Atmel ATF1502/1504/1508 CPLDs:
Verilog → Yosys → our fitter → `.jed` → SVF → JTAG, with the vendor tools used only as oracles.
- Start at `docs/HANDOFF.md` (state, whole flow, environment, next steps).
- Spec: `docs/architecture/`. Work logs: `docs/worklog/` (conventions in its `README.md`).
- Tools index: `tools/README.md`. Example designs: `examples/`.

## Common commands

```
git submodule update --init           # external/prjbureau (atf15xx-arch generates its tables from it)
cargo build --release
cargo test --release --workspace      # or: docker/run.sh cargo test --release --workspace
docker/yosys/run.sh cargo run --release -p atf15xx-synth -- build design.v -d ATF1504AS-PLCC44 [-c design.acf] -o out/
tools/sim/run.sh python3 tools/sim/equiv.py out/design.jed --device P1504C44 \
    --verilog design.v --top design --fit out/design.fit --budget 60
```

## Black box only

- Every fact about the parts comes from what the vendor programs (ATMISP 7.3, the WinCUPL fitters
  `fit1502`/`fit1504`/`fit1508`) write for inputs we generate. Never look inside the vendor binaries.
- Every derived fact has its own workflow directory `tools/fuzzers/<workflow>/` with its scripts, a
  `README.md` (what it derives, method, commands, results) and a `CLAUDE.md` (supervision rules),
  so others can reproduce it. Shared wrappers go in `tools/fuzzers/common/`; follow its `CLAUDE.md`.
- Fuzzing results go to `.scratch/` unless the workflow's README says what is committed.

## Vendor material and user data

- Never commit vendor files: Microchip sample designs, vendor programs or libraries (`atmel.std`),
  vendor-written JEDEC/SVF/report text. The only exception is the recorded test vectors already in
  the repository (ATMISP Load JEDEC rewritten in our own layout).
- Use the user's own designs (outside the repo) by reference only; copy one into the repo only with
  the user's agreement.

## prjbureau

- prjbureau data enters only through `atf15xx-arch`'s build script (`atf15xx-arch/build/`), which
  generates Rust statics from the pinned submodule `external/prjbureau` (branch `atf1508as` of the
  user's fork) and from `atf15xx-arch/data/`. No run-time parsing of JSON tables or BSDL files; no
  prjbureau assets (or JSON copies of them) stored in this repository. Scripts that need the tables
  ask the Rust tools for them (e.g. `atf15xx-sim device`); never commit a JSON export.
- prjbureau changes go on branches of the user's fork, checked out as worktrees in
  `.worktrees/prjbureau/<branch>`; keep `docs/prjbureau/branches.md` current. There must be no
  `patches/` directory.
- Pushing to the user's fork is allowed. Contacting or pushing upstream (whitequark/prjbureau,
  OpenOCD) needs the user's explicit go-ahead.

## Spec first

- `docs/architecture/` is normative and precise enough to implement from. Change the spec with the
  code, and name the chapter/section in the work log.
- Every claim states how it is known: *oracle-verified* (corpus named), *labelled* (vendor's own
  labels, file named), *external* (datasheet/prjbureau, cited) or *inferred* (say what would confirm it).

## Code

- Rust. Use widely used upstream crates (clap, chrono, serde/serde_json, flate2, …) instead of
  hand-rolled code or libc FFI; check the current version before adding one.
- Workspace crates (top-level directories, version 0.1.0, licence ISC): `atf15xx-arch`,
  `atf15xx-fit`, `atf15xx-synth`, `atf15xx-sim`, `atf15xx-tool`. Shared and internal dependencies go
  in `[workspace.dependencies]` of the root `Cargo.toml`.
- OpenOCD is used only as a separate process (its Tcl RPC server), never linked (it is GPL).

## External toolchains

- Yosys, Verilator, iverilog and prjbureau build and run in Docker (`docker/`), never installed on
  the host. Pin versions and record commits in the Dockerfile and the work log.
- Entry points: `docker/run.sh` (image `open-atf150x-tools`), `docker/yosys/run.sh` (pinned Yosys),
  `tools/sim/run.sh` (Verilator, iverilog, Yosys).

## Documentation

- Keep the matching `docs/worklog/NN-*.md` current while working, per `docs/worklog/README.md`
  (dated Status, scope, results with reproduce commands, evidence levels, decisions, journal incl.
  dead ends, open issues, artifacts), and commit it in the same commit as the work.
- Update `docs/STORY.md` (narrative for a blog post: what was tried, surprises, mistakes, numbers)
  at each milestone.
- Update `docs/HANDOFF.md` whenever the state changes. Every tool has usage docs.
- Dates are absolute (YYYY-MM-DD).

## Git

- Commit all work on `main`, after each milestone, with an informative message (what and why).
- Stage with explicit pathspecs (`git add <paths>`), only files you own; never `git add -A` or
  `git add .`. Other sessions may be editing other parts of the tree.
- Do NOT add a `Co-Authored-By` trailer (the user's instruction).
- Remote `origin` is github.com/teknoman117/open-atf15xx (public). Push, force-push, tag or rewrite
  history only when the user asks.
- History before 0.1.0 is on the local branch `pre`; never push it.
- Scratch work goes in the git-ignored `.scratch/`; worktrees in `.worktrees/`. A fresh clone must
  build and pass the tests.

## Verification

- Time-bound every check (e.g. `equiv.py --budget`); no unbounded proofs.
- Report guarantee levels honestly, exactly as `tools/sim/equiv.py` states them: PROVEN,
  BOUNDED n, SIMULATED n cycles + k injections, FAIL. Never round BOUNDED or SIMULATED up to PROVEN.
- Report failures with their output; don't hide or retry them away silently.

## Hardware bench and wine

- Bench: ATF1508AS breakout + FT232H + Arduino Uno (`tools/silicon/README.md`).
- Never program an image that disables JTAG (anything built with `jtag off`; JED special bit 1 = 0):
  recovery needs 12 V on OE1. Use `tools/silicon/program.sh`, which refuses such images. The
  user-facing `atf15xx-tool program` only warns (by design: designs may need those pins), so check
  the image (`tools/silicon/jtag_guard.py`) before programming with it.
- Never program fuzzing JEDEC files into a device.
- Only one wine job at a time per prefix: check `pgrep -fa 'wine|ATMISP|fit15|drive.exe'` first.

## Privacy

- Never put the user's email or personal details into requests to outside services.
- Don't add absolute local paths (e.g. `/home/<user>/...`) to committed files; use repo-relative
  paths or `~`/`$HOME`.

## Agents

- Don't start subagents unless the user asks. Any agent that is started follows this file.
