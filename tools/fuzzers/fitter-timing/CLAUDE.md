# fitter-timing: supervising a run

See README.md for what this derives and how. Read `../common/CLAUDE.md` first.

## Rules

* Needs the fitter-experiment corpus in `.scratch/oracle/{feat,fold}` (`../fitter-macrocell`, `../fitter-foldback`) and,
  optionally, the user's designs in `.scratch/sim/user`. Output: `.scratch/timing/corpus`
  (override with `OUT=`).
* Runs inside the `open-atf150x-prjbureau` container with its own wine prefix; the vendor fitters
  are mounted read-only from the user's WinCUPL installation (`VENDOR_DIR`). One fitter at a time.
* The datasheets are Microchip's documents: keep the PDFs and their text in `.scratch/timing/`,
  never in the repository. `derive_timing.py` reads only their AC-characteristics rows; check its
  parse when a datasheet revision changes (a row with an unexpected number count is skipped, and
  the value then comes from another family, marked `inferred`).
* Never take timing values from the fitter binaries (that route was removed); the SDF output and
  the datasheets are the only sources.

## Checking a run

The bar is zero unexplained delays (`derive_timing.py` exits non-zero otherwise). A new
unexplained delay means either a new path kind (extend F8.4, `check_sdf_timing.py`'s EXPECTED and
`atf15xx-sim`'s timing model) or a wrong value. Every reported "SDF x, datasheet y" line and every
difference from the committed table goes into `docs/worklog/02-fitter.md` with the command, before
`timing.json` is replaced; then run `cargo test --release --workspace`.
