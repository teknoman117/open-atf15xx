# prjbureau: prjbureau's fuzzers on the WinCUPL fitters

**Derives:** prjbureau's fuse database (`database.json`) for the ATF1502AS, ATF1504AS and,
new with our branches, the ATF1508AS: the product-term, macrocell, UIM and global-OE fuse
semantics from which atf15xx-arch's build script (`atf15xx-arch/build/`) generates the fuse map
we compile in.

**Method:** prjbureau's own fuzzers (`external/prjbureau/fuzzers/0*/fuzzer.py`) fit thousands of
small designs and diff the JEDEC files. The WinCUPL fitter builds lack the library prjbureau's
EDIF route needs, so our branches (`tt2-toolchain`, `atf1508as` on the user's fork, see
`docs/prjbureau/branches.md`) translate each design to TT2 (`util/tt2.py`) and fit in parallel
(`toolchain.prefetch`, `PRJBUREAU_JOBS`). The route is trusted because it reproduces upstream's
1502AS/1504AS databases exactly.

| script | role |
|---|---|
| `suite.sh` | runs 000-init, trims the database to the chosen devices, then every fuzzer, on a worktree |
| `dbdiff.py` | one device's entry in two databases: equal leaves and differences |
| `stress_prefetch.py` | parallel against sequential fitting on N random designs (guards the per-worker fitter directories) |
| `v2jed.sh` | prjbureau's intended EDIF flow (Verilog → EDIF → fitter → JED) for comparison |

## Run

```
git -C external/prjbureau worktree add $PWD/.worktrees/prjbureau/atf1508as atf1508as   # once
cp <WinCUPL>/Fitters/{fit1502.exe,fit1504.exe,fit1508.exe,atmel.std} .worktrees/prjbureau/atf1508as/vendor/
docker/prjbureau/run.sh sh -c "PRJBUREAU_JOBS=14 sh /work/tools/fuzzers/prjbureau/suite.sh \
    /work/.worktrees/prjbureau/atf1508as ATF1508AS" > .scratch/prjbureau-explore/runs/ATF1508AS.log 2>&1
python3 tools/fuzzers/prjbureau/dbdiff.py .worktrees/prjbureau/atf1508as/database.json \
    <(git -C external/prjbureau show origin/main:database.json) ATF1502AS
```

## Results (2026-11-25)

All 34 fuzzers complete for the three devices. 1502AS and 1504AS: 3209 and 7128 values equal to
upstream, 0 differences. 1508AS: complete except the facts the WinCUPL build cannot show (GOE pad
choices, 8 UIM feedback choices never produced; listed in `CLAUDE.md`), none of which any vendor
JED or flow JED uses. Worklog: `docs/worklog/03-prjbureau.md`.
