# fitter-pins: supervising a run

Read `../common/CLAUDE.md` first.

* Prerequisites: WinCUPL in the host prefix; `.scratch/re/regions.json` from `../fuse-regions`.
  Run `exp_pinmap.py` first: the other three read its result.
* The experiments use `fitter_oracle.run_batch(..., workers=8)`, i.e. 8 fitters in one directory.
  Pin-role results have been seen to flip under that load (`FLAKY` in `export_devices.py`). For a
  result that matters, re-run with `workers=1` (edit the call) and keep the serial answer; add a
  disagreement to `FLAKY` only after a serial re-run, with a comment.
* Progress: each script prints one line per device; a full `exp_pinmap.py` takes a few minutes.
* Done when `export_devices.py --compare atf15xx-arch/data/devices.json` reports no differences, or
  every difference is explained and recorded in `docs/worklog/02-fitter.md` before `devices.json` is
  rewritten (`-o`) and the workspace rebuilt and tested.
