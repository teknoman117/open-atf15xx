# Our prjbureau branches (pull-request candidates)

[prjbureau](https://github.com/whitequark/prjbureau) (0BSD) is the submodule `external/prjbureau`,
cloned from the user's fork `git@github.com:teknoman117/prjbureau.git` and pinned at the fork's
branch `atf1508as` (which atf15xx-arch's 1508 tables are generated from). In the submodule, `origin` is the fork
and `upstream` is whitequark's repository (push URL disabled). Every change lives on a topic branch
based on upstream `main` (stacked only where a change needs another one), so that each can be
proposed upstream on its own; all branches are pushed to the fork (2026-11-25) and tracked there.
**Nothing has been proposed upstream**: each PR needs the user's go-ahead. How the data comes back into open-atf150x:
[`docs/plan/prjbureau-import.md`](../../docs/plan/prjbureau-import.md).

The branches live on the fork only; each is checked out as a worktree under
`.worktrees/prjbureau/<branch>` (git-ignored), tracking `origin/<branch>`:

```
git -C external/prjbureau worktree add $PWD/.worktrees/prjbureau/<branch> <branch>     # an existing branch
git -C external/prjbureau worktree add $PWD/.worktrees/prjbureau/<topic> -b <topic> upstream/main   # a new one
```

Commit there, push to the fork (`git push origin <branch>`), and keep the table below current.
`tt2-toolchain` stacks on the two fix branches (it carries its own copies of them, identical
diffs) and `atf1508as` on `tt2-toolchain`.

| branch | base | commits | what | verified |
|---|---|---|---|---|
| `fix-toolchain-binary-jed` | `main` | 1 | `util.toolchain` reads the fitter's JED as text, but the JESD3 parser takes bytes since d15dbf2: every fuzzer fails with a `ValueError` | the fuzzers run again (all of the TT2 validation below uses it) |
| `fix-fuzzers-tri-keyword` | `main` | 1 | eight fuzzers name an instance `tri`, a Verilog keyword; current Yosys rejects the designs | Yosys 0.33 parses them |
| `fix-cells-nor` | `main` | 1 | NOR2…NOR12 in `cells.v` assign an undeclared `NQ` instead of their output `QN` | Yosys no longer warns about the implicit `NQ` |
| `fuseconv-be` | `main` | 2 | `fuseconv`: JED↔SVF packing and programming for ATF1502BE/1504BE/1508BE/1508RE (`util/device_be.py`), round-trip tests; docs for BE programming and for the verification of the 1504/1508 AS packing | `python3 -m unittest util.test_fuseconv` (in the prjbureau container): 8 passed; the BE word tables reproduce ATMISP's SVF bit for bit (fuse-index bit-pattern JED files and random fuse maps; `tools/fuzzers/atmisp-fusemap`) |
| `docs-vendor-fitter` | `main` | 1 | docs: the two fitter builds (ProChip Designer with `aprim.lib`/EDIF; WinCUPL without `aprim.lib`, TT2 input), `FITTERDIR`, TT2 as input format | docs only |
| `tt2-toolchain` | `fix-toolchain-binary-jed` + `fix-fuzzers-tri-keyword` (stacked; the branch carries its own copies of both, identical diffs) | 2 (+2) | run the fuzzers with the WinCUPL fitter builds: `util/tt2.py` translates the fuzzers' Verilog netlists of `cells.v` cells into TT2 when `vendor/aprim.lib` is absent (gate functions read from `cells.v`); option spellings of those builds translated; fuzzers adapted where TT2 needs another netlist or a narrower comparison | ATF1502AS and ATF1504AS: all 34 fuzzers complete, every value equal to the committed database (3209 / 7128); not obtainable from TT2 input with these builds: GOE pad inputs (so `oe_mux`, and 023's `o_mux`/`d_mux`/`dfast_mux`) |
| `atf1508as` | `tt2-toolchain` (stacked) | 5 | macrocells without a pin (buried-node PT fuzzing, bonded probes), 027 fixes (frozenset reduction; stop when nothing new is found), 030 PD comparison on configuration fuses, parallel prefetch (`PRJBUREAU_JOBS`, per-worker FITTERDIR), and ATF1508AS enabled with its database (all other entries unchanged) | all 34 fuzzers complete on ATF1508AS (fit1508 1.8.7.8, TT2); with the same code ATF1502AS/1504AS still reproduce the committed database exactly (3209 / 7128 values); not obtainable with the WinCUPL build: GOE pad choices, oe_mux, o_mux, d_mux, dfast_mux, o_invert, 8 UIM feedback choices |

The former local branch `open-atf150x` (patches 0001–0004 here before 2026-11-25) is superseded:
`fix-cells-nor` + `fuseconv-be` + `docs-vendor-fitter` merged give exactly its tree.

## Draft PR descriptions

**fix-toolchain-binary-jed** — *util.toolchain: read fitter output in binary mode.*
Since d15dbf2 the JESD3 parser requires `bytes`, but `toolchain.run_uncached` still opens the
fitter's JED in text mode, so every fuzzer stops with "JESD3 lexer requires bytes or bytearray".

**fix-fuzzers-tri-keyword** — *fuzzers: do not name an instance `tri`.*
`tri` is a Verilog net type; Yosys 0.33 rejects `TRI tri(...)` with "syntax error, unexpected
TOK_WIRE". Renamed to `obuf` in the eight fuzzers that use it; no change in behaviour.

**fix-cells-nor** — *cells: fix NOR cells assigning to an undeclared net instead of QN.*

**fuseconv-be** — *fuseconv: JED↔SVF for BE-series devices.* Packing and programming
sequences for the BE series, with round-trip tests, and documentation.

**docs-vendor-fitter** — *docs: notes on running the vendor fitter.*

**tt2-toolchain** — *Run the fuzzers with fitter builds that lack aprim.lib.* (Depends on the
first two.) The fitter builds that come with WinCUPL cannot read EDIF (no `aprim.lib`), but read
TT2. With this change `toolchain.run` translates a fuzzer's Verilog netlist to TT2 when
`vendor/aprim.lib` is missing; the EDIF path is unchanged. On the ATF1502AS and ATF1504AS all 34 fuzzers
reproduce the committed database exactly. Two fitter behaviours had to be accounted for (a
`LATCH` enable has the opposite polarity to TT2 `.LE`; the fitter may allocate the cubes of a sum in
another order than EDIF does). Fuzzers that cannot work from TT2 say so and skip.

**atf1508as** — *Add ATF1508AS.* (Depends on tt2-toolchain.) The ATF1508AS stub in 000-init was
ready but commented out. With the TT2 toolchain, every fuzzer now runs on it; the commits before
the last fix what only the 1508 reaches (macrocells without a pin, net sets larger than 35, a
search that never ended, a power-down side effect) and add parallel prefetching (opt-in), which
took the UIM fuzzing from an estimated six hours to under one. The new entry was produced with the
WinCUPL fitter build through TT2; what that build cannot reveal is listed in the commit message.

