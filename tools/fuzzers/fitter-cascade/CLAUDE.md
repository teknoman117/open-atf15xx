# fitter-cascade: supervising a run

Read `../common/CLAUDE.md` first.

* Prerequisites: WinCUPL in the host prefix (`WINEPREFIX`, default `~/.wine32`) and a release
  build of `atf15xx-sim` (`cargo build --release`), which decodes the vendor JEDs.
* The script fits serially (`workers=1`, 56 designs, about 5 minutes): no shared-directory risk.
* A chain length that does not follow "one helper per further 5 terms", or a helper that is not
  directly below the cell, is a finding to re-run and explain before the capacity model
  (`atf15xx-fit/src/capacity.rs`) changes.
* Record results (counts, the command) in `docs/worklog/02-fitter.md`.
