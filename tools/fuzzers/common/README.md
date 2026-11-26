# common: the black boxes, wrapped

Libraries and drivers that run the vendor programs. Every workflow in `tools/fuzzers/` uses them;
nothing here learns anything by itself.

| file | runs | used by |
|---|---|---|
| `atmisp_oracle.py` | ATMISP 7.3 (GUI) under wine through `drive.exe`; one case = a chain + JEDEC files → `out.svf`, written JEDEC files, `driver.log`. Also synthetic JEDEC writing (`make_jed`, `random_fuses`) | `atmisp-svf`, `atmisp-fusemap`, `fuse-regions` |
| `drive.c` | Win32 driver for the ATMISP window: loads the chain, sets the SVF controls, presses *Run*, answers dialogs, logs the transcript | `atmisp_oracle.py` |
| `fitter_oracle.py` | WinCUPL `cupl.exe` and `fit1502/1504/1508.exe` under wine; parses `.jed` and `.fit` | the `fitter-*` workflows |
| `tt2gen.py` | writes the synthetic `.tt2` designs the fitter experiments ask about | the `fitter-*` workflows |
| `fit.sh` | one vendor fitter run inside the `open-atf150x-prjbureau` container (own wine prefix, own fitter directory) | `fitter-timing`, `fitter-conformance`, `prjbureau` |
| `model.py` | the macrocell-configuration addressing hypothesis the experiments test | `fitter-pins` |

Build the driver once: `i686-w64-mingw32-gcc -O1 -o drive.exe drive.c -luser32`, then copy it to
`$WINEPREFIX/drive_c/probe/drive.exe`.

## Environment

* `WINEPREFIX` (default `~/.wine32`): a 32-bit prefix with the user's own ATMISP 7.3 in
  `C:\ATMISP7` and WinCUPL in `C:\Wincupl` (`WINCUPL` overrides). The vendor programs are never
  copied into the repository.
* The container (`docker/prjbureau/run.sh`) mounts the repository at `/work` and the user's
  WinCUPL `Fitters` directory read-only at `/vendor` (`VENDOR_DIR`).
