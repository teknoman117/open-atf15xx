# prjbureau fuzzers on the TT2 route: supervising a run

prjbureau's own fuzzers (`fuzzers/0*/fuzzer.py` in the submodule) learn the fuse map by fitting
thousands of small designs with the vendor fitter. Our branches (`tt2-toolchain`, `atf1508as`, on
the user's fork, see `docs/prjbureau/branches.md`) let them run with the WinCUPL fitter builds,
which lack `aprim.lib`: `util/tt2.py` translates each design to TT2. The ATF1508AS entry of
`database.json` (and through atf15xx-arch's build script, our fuse map) was produced this way.

## Where things are

* Submodule `external/prjbureau` (pinned at `atf1508as`); work happens in worktrees under
  `.worktrees/prjbureau/<branch>` (untracked), e.g.
  `git -C external/prjbureau worktree add $PWD/.worktrees/prjbureau/<topic> -b <topic> origin/main`.
* Each worktree needs the vendor fitters in its `vendor/` (git-ignored; copy `fit1502.exe`,
  `fit1504.exe`, `fit1508.exe`, `atmel.std` from the user's WinCUPL `Fitters` directory; never
  commit them) and a `cache/` directory (created by `suite.sh`).
* `suite.sh` (here) runs the fuzzers for chosen devices; `dbdiff.py` compares a device's entry with
  another database; `stress_prefetch.py` checks parallel fitting against sequential fitting.

## Running

```
docker/prjbureau/run.sh sh -c "PRJBUREAU_JOBS=14 sh /work/tools/fuzzers/prjbureau/suite.sh \
    /work/.worktrees/prjbureau/atf1508as ATF1508AS" > .scratch/prjbureau-explore/runs/ATF1508AS.log 2>&1
python3 tools/fuzzers/prjbureau/dbdiff.py .worktrees/prjbureau/atf1508as/database.json \
    <(git -C external/prjbureau show origin/main:database.json) ATF1502AS
```

* Run it in the background and watch the log: it prints `== fuzzers/NNN` per fuzzer, its time, and
  `FAILED` with a traceback; `026`/`027` print `known/total` UIM counts. The ATF1508AS full suite
  takes about 30 minutes with 14 jobs (026 alone: ~6 hours sequentially).
* `suite.sh` rewrites the worktree's `database.json` (000-init, trimmed to the devices, then every
  fuzzer). **Do not commit a database written by a validation run.** Copy results out
  (`cp database.json .scratch/prjbureau-explore/runs/database.tt2-<DEV>.json`) and
  `git checkout database.json` before committing code.
* Only the container's own wine prefix is used (`.scratch/prjbureau-explore/home/.wine_atf`); one
  container at a time. If a run must be stopped, stop the `docker run` client *and* the container
  (`docker stop <id>` by id: a killed client leaves the container running).

## The cache

* `cache/` holds every fitter result keyed by the design text, pins, device and strategy. It makes
  reruns after a crash cheap (everything already fitted replays in minutes).
* **Clear it (`rm -rf cache`) whenever the translation changes** (`util/tt2.py`, the strategy
  translation in `util/toolchain.py`): the key is the Verilog input, not the TT2 that was fitted, so
  stale results would come back. Fuzzer-only changes do not need it.
* Failures are cached too: a fixed bug in the translation can still return the old exception.

## Parallelism (`PRJBUREAU_JOBS`)

`toolchain.prefetch` fits independent designs in parallel into the cache; the fuzzer then reads them
in its usual order, so its decisions are unchanged. Each worker has its own copy of `vendor/` as
`FITTERDIR` (`work/fitter<pid>/`): the fitter writes temporary files with fixed names there, and a
shared directory corrupted about 1 in 400 results (silently: a design "fitted" with 0 product
terms). After any change to `prefetch`, run `stress_prefetch.py` in a scratch worktree with an empty
cache; the bar is N of N identical (2000/2000 on 2026-11-25). Only `026-uim_mux` prefetches today.

## Validating a change

The TT2 route is trusted because it reproduces upstream exactly. After any toolchain or fuzzer
change, run the full suite for **ATF1502AS and ATF1504AS** and require `dbdiff.py ... differences 0`
against upstream `main` (last: 3209 and 7128 values equal). New fuzzer adaptations must be gated on
`toolchain.has_aprim` (or be harmless on the EDIF path, such as a `scope=` on a comparison) and say
why in a comment; upstream must be able to take them.

## What the WinCUPL build cannot reveal through TT2 (do not chase)

GOE mux pad choices (the fitter only accepts dedicated pins and nodes as GOE drivers: exit code
248), and therefore `oe_mux` (022) and 023's `o_mux`/`d_mux`/`dfast_mux`; `015-o_invert` on the
1508 (skipped upstream); 8 UIM feedback choices of the 1508 that the fitter never produces (027
gives up after 8 rounds without progress). None of these are used by any vendor JED or design in
the corpora (`docs/plan/prjbureau-import.md`).

## Recording

Results and decisions go to `docs/worklog/03-prjbureau.md` (evidence and the command), branch
changes are pushed to the branch on the fork and noted in `docs/prjbureau/branches.md`. Pushing to the
user's fork is fine; proposing anything upstream (whitequark/prjbureau) needs the user's go-ahead.
