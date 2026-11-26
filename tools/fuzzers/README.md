# Fuzzers: how everything this project knows about the chips was learned

Every fact about the ATF15xx parts that is not in a public datasheet was learned by running the
vendor programs as black boxes (ATMISP 7.3, the WinCUPL fitters `fit1502/1504/1508`, also through
prjbureau's fuzzers) on inputs we generated, and reading what they wrote: the programs' outputs
are the only evidence.

Each directory is one workflow for one kind of fact: the scripts, a `README.md` (what it derives,
how, the commands, the last results) and a `CLAUDE.md` (the rules for an agent supervising a run).
`common/` holds the wrappers around the vendor programs and the rules every run follows.

## The flow, in order

| # | workflow | black box | derives | feeds |
|---|---|---|---|---|
| 1 | [`atmisp-svf`](atmisp-svf/) | ATMISP | SVF / Load-JEDEC behaviour for every device, instruction, option and bad input | `atf15xx-tool/tests/vectors/` (145), spec §2–§4, §6–§9 |
| 2 | [`atmisp-fusemap`](atmisp-fusemap/) | ATMISP | fuse → programming-row map, register layout, reserved fuses, geometry | `atf15xx-tool` fuse maps (spec §4.4, §5, §8) |
| 3 | [`fuse-regions`](fuse-regions/) | ATMISP (Load), with prjbureau's PT numbering | names and extents of the AS fuse regions | `.scratch/re/regions.json` → 4–7 |
| 4 | [`fitter-pins`](fitter-pins/) | fitter | pin → macrocell, JTAG/PD pins, dedicated inputs, GCLK3 pin | `atf15xx-arch/data/devices.json` |
| 5 | [`fitter-pterms`](fitter-pterms/) | fitter | PT column ↔ UIM mux and polarity | evidence for the fuse map (spec F2.3–F2.4) |
| 6 | [`fitter-macrocell`](fitter-macrocell/) | fitter | macrocell / global option bits, vendor field names | evidence (F2.5–F2.8), `atf15xx-fit/tests/vectors/` |
| 7 | [`fitter-foldback`](fitter-foldback/) | fitter | foldback column, cascade direction | evidence (F2.6) |
| 8 | [`prjbureau`](prjbureau/) | fitter, via prjbureau's fuzzers | prjbureau's fuse database incl. the new ATF1508AS entry | the fuse map and boundary-scan parts `atf15xx-arch` generates at build time from the pinned submodule |
| 9 | [`fitter-timing`](fitter-timing/) | fitter (SDF output) + datasheets | speed-grade timing tables, delay composition | `atf15xx-arch/data/timing.json` (spec F8) |
| 10 | [`fitter-conformance`](fitter-conformance/) | fitter | how closely our fitter reproduces the vendor's | regression numbers |

Step 3 uses the database of step 8 (the submodule's committed `database.json` is enough; step 8
regenerates it). Steps 1–3 need ATMISP, steps 4–7 and 10 the fitters in the host wine prefix, and steps 8 and 9
the `open-atf150x-prjbureau` container. Results go to `.scratch/` (git-ignored); each README
lists what is committed. The architecture database is rebuilt from these results by cargo:

```
python3 tools/fuzzers/fitter-pins/export_devices.py -o atf15xx-arch/data/devices.json   # step 4
# step 9 writes atf15xx-arch/data/timing.json; step 8's database is the pinned submodule
cargo build --release   # atf15xx-arch/build/ generates the Rust tables (docs/plan/prjbureau-import.md)
```

## What you need

* A 32-bit wine prefix with your own copies of ATMISP 7.3 (`C:\ATMISP7`) and WinCUPL
  (`C:\Wincupl`), plus `i686-w64-mingw32-gcc` for the ATMISP driver (`common/README.md`).
* Docker, for the prjbureau container (`docker/prjbureau/`).
* The public datasheets, for step 9 (links in `fitter-timing/README.md`).
* Python 3. No hardware: fuzzing JEDEC files are never programmed into a device.

The Rust code contains no fuzzers: it consumes these results. Its randomized checks
(`atf15xx-fit/tests/oracles.rs`, `atf15xx-sim compare`) are ordinary tests under `cargo test`.
