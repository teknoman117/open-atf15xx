# 03 — prjbureau evaluation and the Yosys → `.tt2` prototype

## Status

**2026-11-23: complete.** The full technical write-up is [`docs/prjbureau/README.md`](../prjbureau/README.md),
with citations into prjbureau's sources. This log summarises it and records the process. Follow-up
work moves to the fitter (02), simulator (04) and the planned open Verilog flow (00, roadmap).

## Scope and ownership

Understanding how prjbureau (whitequark, <https://github.com/whitequark/prjbureau>, 0BSD) is meant to
be used, checking it against this project's verified ATMISP knowledge and the vendor fitters, finding
a Verilog → sum-of-products route, and folding our knowledge into prjbureau on a local branch.
Owns: `external/prjbureau/` (checkout, local branch `open-atf150x`, **never pushed**),
`docs/prjbureau/`, `docker/prjbureau/`, `tools/prjbureau/`, `.scratch/prjbureau-explore/`.

## Results

| Finding | Evidence |
|---|---|
| prjbureau is a fuse database (`database.json`: ATF1502AS/BE, ATF1504AS/BE only; no ATF1508) plus docs and experimental tools (`fuseconv` JED↔SVF, `fusecli` symbolic decode/edit). No fitter; nextpnr is only acknowledged. | external: prjbureau `docs/intro.rst`, `docs/tools/`, repository contents at `8b8a971` |
| Its Verilog flow (`v2svf.sh`) is a hand-written structural netlist of vendor primitives → Yosys EDIF → vendor `fit1502 -strategy ifmt=edif`. No synthesis. | external: `v2svf.sh`, `cells.v`, `examples/` |
| That flow cannot run here: it needs `aprim.lib` from the ProChip Designer fitter build. The user's WinCUPL fitters (1.8.7.8) lack it. | oracle: `tools/fuzzers/prjbureau/v2jed.sh` stops at `File not found : aprim.lib` |
| AS JED→programming-word packing equals spec §5 for every fuse (1502/1504/1508) | verified: permutation comparison, both directions |
| `fuseconv` programs the same words as ATMISP for the user's `spi`, `motioncontroller` (1504AS), `system-controller` (1508AS) | oracle: ATMISP SVF vs fuseconv SVF (216/216, 216/216, 234/234 words) |
| UIM mux fuse ranges equal ATMISP's template labels | verified: 80/80 (1502), 160/160 (1504) |
| Product-term numbering: ATMISP's template "PT n MC m" labels are reversed for half the macrocells; **prjbureau's numbering is the functional one** | oracle: PT-driven OE design on the vendor fitter always uses prjbureau's PT5 |
| Recommended Verilog → SOP: Yosys `synth_atf15xx.ys` (proc; tribuf; synth; dfflegalize; abc -sop; write_json) → `yosys2tt2.py` → fitter | oracle: counter on 1502/1504/1508 plus a mixed 1504 design; fit; ATMISP SVF = atmisp; fitter equations match |
| Vendor fitter runs in Docker (own wine prefix) and reproduces the user's `spi.jed` | oracle: byte-identical apart from the date |

Reproduce: see `docs/prjbureau/README.md` §2 and §4 (`docker/prjbureau/run.sh …`,
`tools/fuzzers/common/fit.sh`, `tools/sim/yosys2tt2.py`).

## Decisions

- **2026-11-23: Use prjbureau as a cross-check and a citation, not as code or data in this
  repository.** Our own fuse semantics come from our oracle runs (fitter workstream). prjbureau
  confirms them for 1502/1504.
- **2026-11-23: Yosys JSON (SOP + legal storage) is the common front-end format**: it can feed the
  vendor fitter (via `.tt2`) today and our fitter crates directly later.
- **2026-11-23: Run vendor tools in Docker with a private wine prefix** to avoid clashing with
  `~/.wine32` runs and to follow the "builds in Docker" rule.

## Journal

* **2026-11-25** — Submodule moved to the user's fork (`git@github.com:teknoman117/prjbureau.git`,
  remote `origin`; whitequark's repository is `upstream`, push disabled) and pinned at `atf1508as`
  (`e634784`); all seven topic branches pushed to the fork and tracked there, worktrees included.
  `arch.json` now takes the 1508 data from the submodule itself (identical to the former copy,
  which is removed; families unchanged).

* **2026-11-25** — `tools/arch/import_prjbureau.py`: the fuse-map part of `arch.json` generated from
  prjbureau's database. From upstream `8b8a971`, the 1502AS and 1504AS families come out identical to
  the current `arch.json` (24/24 and 48/48 macrocell fields, 80/160 UIM muxes, 200/360 UIM table
  entries, 30/54 GOE entries, 0 problems, 0 differences), i.e. the UIM/GOE tables come from 0BSD data. Reproduce:
  `python3 tools/arch/import_prjbureau.py`. TT2-route fuzzer status: all 34 fuzzers complete on the
  1502AS (3209 values, all equal to upstream; gaps: `oe_mux`, `o_mux`, `d_mux`, `dfast_mux`, GOE pad
  inputs), and 24 of them also on the 1504AS (6819 values equal); `atf1508as` branch created.

* **2026-11-25** (main session) — prjbureau became a submodule (`2d4ec6c`, pinned at upstream
  `8b8a971`, push URL disabled). Our changes split into PR branches off upstream `main`
  (indexed in `docs/prjbureau/branches.md`; worktrees in `.worktrees/prjbureau/`): `fix-toolchain-binary-jed`,
  `fix-fuzzers-tri-keyword` (two upstream bugs that stop every fuzzer / current Yosys),
  `fix-cells-nor`, `fuseconv-be`, `docs-vendor-fitter` (the former local branch, re-split; the three
  merged give exactly its tree), and `tt2-toolchain` (stacked on the first two): prjbureau's own
  fuzzers run with the WinCUPL fitter builds through a TT2 translation of their Verilog netlists.
  Evidence: on the ATF1502AS, fuzzers 001–013, 015–020, 024–027 reproduce upstream's committed
  database exactly (2911 values, `dbdiff` 0 differences); reproduce with
  `docker/prjbureau/run.sh sh -c "FUZZERS=... tools/fuzzers/prjbureau/suite.sh <worktree> ATF1502AS"`.
  Known TT2 limits (fitter behaviour, recorded in the fuzzers): 014 (comb node read only by OEs is
  dissolved), 021 (pads not accepted as GOE drivers from TT2 input). Import design:
  `docs/plan/prjbureau-import.md`.

- **2026-11-23**: Explored prjbureau (sources, history, fuzzers). Built its container. Ran `v2svf.sh`
  and found the `aprim.lib` blocker. Wrote the Yosys → `.tt2` prototype. Cross-checked against
  ATMISP and the vendor fitter. Committed four changes on the local branch: `09141f6` (BE-series
  JED↔SVF and programming in fuseconv; words equal ATMISP's for 1502BE/1504BE/1508BE/1508RE),
  `303db3d` (NOR cells in `cells.v` left QN undriven), `9d6a51e` (BE programming and verification docs),
  `1807a50` (running the vendor fitter).

## Open issues

- prjbureau has no ATF1508 database; our fitter workstream's oracle data could generate one (a
  suggested contribution for the local branch).
- 1502AS global-OE invert bits: prjbureau `C+10−n` vs the vendor fitter `C+3+n`. Unresolved without
  silicon (recorded in `docs/architecture/fitter/02-fuse-map.md` §F2.7).
- One upstream unit test (`util.svf`) fails with bitarray 2.9, independent of our changes.
- The prototype doesn't yet pass fitter properties (pin keepers, slew, open collector, foldback)
  from Verilog attributes, and supports only pin-number constraints.

## Artifacts

`docs/prjbureau/README.md`; `external/prjbureau/` (branch `open-atf150x`); `docker/prjbureau/`;
`tools/prjbureau/{synth_atf15xx.ys,yosys2tt2.py,fit.sh,v2jed.sh}`; scratch: `.scratch/prjbureau-explore/`.
