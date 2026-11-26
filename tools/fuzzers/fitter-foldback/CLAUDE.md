# fitter-foldback: supervising a run

Read `../common/CLAUDE.md` first.

* Prerequisites: WinCUPL in the host prefix (`WINEPREFIX`, default `~/.wine32`), and the
  outputs of `../fuse-regions` (`.scratch/re/regions.json`) and `../fitter-pins`
  (`.scratch/exp/pinmap.json`). Run the scripts in the order of the README.
* The scripts fit with `fitter_oracle.run_batch(..., workers=8)` in one fitter directory. Trust
  a result only when the whole experiment is consistent: every script asserts or prints its
  contradictions, and the bar is zero. Re-run any contradiction serially (`workers=1`) before
  treating it as a fact about the chip: shared-directory corruption looks exactly like one.
* Progress: one line per family / case group; results land in `.scratch/exp/`, the designs and
  vendor outputs in `.scratch/oracle/<experiment>/`.
* After the run, check the architecture database against the new corpus with
  `../fitter-conformance/verify_tt2.py` (see the README); a drop in fuse-identical cases is a
  finding to explain before anything is changed.
* Record results (counts, contradictions, the command) in `docs/worklog/02-fitter.md`.
