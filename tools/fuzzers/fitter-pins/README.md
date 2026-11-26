# fitter-pins: package pins, pin roles and dedicated inputs

**Derives:** for every device the vendor fitters accept (13: 1502/1504/1508 in PLCC/TQFP/PQFP
packages), the package pin → macrocell map, which pins are JTAG / power-down pins, what each
dedicated input does (GCLK1, GCLK2/OE2, GCLR, OE1, or plain input with its UIM source) and which
I/O pin can drive GCLK3. This is `atf15xx-arch/data/devices.json`, which atf15xx-arch compiles in.

**Method:** tiny designs pinned to every pin in turn (`Y = A & B` with Y on pin p; a clock, OE or
reset input on a dedicated pin; JTAG and PD features on and off) are fitted by the vendor fitter;
the answers are read from the JEDEC file and the fit report: the macrocell the output landed in,
the fitter's refusal when a pin is not a user I/O or is taken by a feature, the global clock /
OE / clear entry a dedicated pin selects.

| script | asks | writes (`.scratch/`) |
|---|---|---|
| `exp_pinmap.py` | pin → macrocell, every device | `exp/pinmap.json`, `oracle/pinmap/` |
| `exp_pinroles.py` | JTAG / PD1 / PD2 pins (output pinned there fails only with the feature on) | `exp/pinroles.json` |
| `exp_dedic.py` | the function of each dedicated input | `exp/dedic.json` |
| `exp_gclk3.py` | the I/O pin that drives GCLK3 | `exp/gclk3.json` |
| `export_devices.py` | the four results → the device tables | `atf15xx-arch/data/devices.json` |

## Run (from the repository root, in this order)

```
python3 tools/fuzzers/fitter-pins/exp_pinmap.py
python3 tools/fuzzers/fitter-pins/exp_pinroles.py
python3 tools/fuzzers/fitter-pins/exp_dedic.py
python3 tools/fuzzers/fitter-pins/exp_gclk3.py
python3 tools/fuzzers/fitter-pins/export_devices.py --compare atf15xx-arch/data/devices.json   # then -o the same file
```

`exp_dedic.py` also needs the global-OE input table (atf15xx-arch, through `atf15xx-sim fuses`)
and the `fuse-regions` output. Writing `devices.json` and rebuilding puts the result into the
crates (docs/plan/prjbureau-import.md).

## Results

13 devices; `export_devices.py --compare` reports no difference from the committed `devices.json`
(2026-11-25). Worklog: `docs/worklog/02-fitter.md`.
