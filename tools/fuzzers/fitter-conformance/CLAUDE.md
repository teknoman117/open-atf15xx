# fitter-conformance: supervising a run

Read `../common/CLAUDE.md` first.

* `verify_tt2.py` uses the vendor JEDs already in the corpus unless `--run-vendor` is given, so it
  needs no wine and can run any time; with `--run-vendor` it runs the host fitter (one wine job).
* Rebuild the fitter first (`cargo build --release -p atf15xx-fit`): stale binaries give stale
  numbers. Pass an absolute `--bin-dir`.
* Compare with the last recorded numbers (README). A drop in `identical` is a regression unless
  the change was meant to alter placement; inspect the `different` cases' regions (`-v`) and prove
  the new results equivalent (`atf15xx-sim compare vendor.jed ours.jed -d D`, or `tools/sim/equiv.py`) before
  accepting it.
* `vendor_oracle.sh` runs inside the container (`docker/prjbureau/run.sh`); one container at a time.
* Record numbers and commands in `docs/worklog/02-fitter.md` (fitter) or `05-verilog-flow.md`
  (flow corpus).
