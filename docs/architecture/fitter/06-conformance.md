# F6. Oracle, experiments and conformance

## F6.1 The oracle

The vendor fitters ship with WinCUPL 5.0a (`C:\Wincupl\WinCupl\Fitters\fit150{2,4,8}.exe`,
version 1.8.7.8) and run headless under wine (`WINEPREFIX=~/.wine32`): one run takes 2–5 s and
runs can be parallelised. `tools/fuzzers/common/fitter_oracle.py` wraps them (`run_fitter`, `compile_pld`
via `cupl.exe`, `read_jed`, `parse_fit`, `run_batch`); `tools/fuzzers/common/tt2gen.py` writes synthetic
`.tt2` files. Under heavy parallel load a few runs fail spuriously (3 cases in ~5000 pin-role runs,
reproducible serially as successes): treat isolated failures as suspect. Check that no other wine
job uses the prefix before a batch (`pgrep -fa 'wine|ATMISP|fit15|drive.exe'`).

## F6.2 Experiments (`tools/fuzzers/`)

One workflow per kind of fact (`tools/fuzzers/README.md` has the order); results go to
`.scratch/exp/` and the vendor outputs to `.scratch/oracle/<experiment>/`. The fuse map of
`atf15xx-arch` is generated at build time from prjbureau's database
(`atf15xx-arch/build/`) and its devices from `fitter-pins` (`data/devices.json`); the other
experiments are the independent evidence that our fitter reproduces (F6.3).

| workflow / script | establishes |
|---|---|
| `fuse-regions/regions.py`, `fitter-macrocell/vendor_labels.py` | region layout; vendor `NOTE` labels for every PT/mcfg/UIM field |
| `fitter-pterms/exp_ptcol.py`, `exp_ptcol2.py`, `exp_ptcol3.py` | PT column formula (single and saturated blocks) |
| `fitter-macrocell/exp_mcpos.py`, `exp_dir.py` | macrocell slot and direction inside config fields |
| `fitter-macrocell/exp_feat.py` | meaning of each macrocell bit and global config bit (~45 feature configs × every pinned MC) |
| `fitter-foldback/exp_fold.py` | foldback column, cascade direction |
| `fitter-pins/exp_pinmap.py`, `exp_pinroles.py`, `exp_dedic.py`, `exp_gclk3.py`, `export_devices.py` | pin maps, JTAG/PD pins, dedicated pins, GCLK3 pin → `atf15xx-arch/data/devices.json` |
| `common/model.py` | the Python reference of the macrocell bit addressing |
| `prjbureau/` | prjbureau's fuzzers on the WinCUPL fitters: the database the fuse map is generated from |

A bug worth recording: an early JED reader consumed the `*` terminator of one `L` field as the
start of the next, silently skipping every other field; all results above were re-derived after
fixing it (`read_jed` now splits on `*` properly).

## F6.3 Harness: `tools/fuzzers/fitter-conformance/verify_tt2.py`

```
tools/fuzzers/fitter-conformance/verify_tt2.py [--bin-dir DIR] [--run-vendor -- ARGS] [-v] [--json OUT] PATH...
```

For each `.tt2` it takes the vendor result from the sibling `.jed`/`.fit` (or runs the vendor now),
runs our `fit150x` with the vendor's command line (read from line 4 of the `.fit`) and its
date, and compares the JEDs fuse for fuse: `identical` = same fuse array and security fuse, and our
`C` field is the correct checksum (`bad-checksum` otherwise); fuse mismatches are classified by
region. (Until 2026-11-25 our JEDs were byte-identical to the vendor's and the harness compared
bytes; the counts are the same under both definitions, F5.0.)
Build our binaries first: `docker/run.sh cargo build --release --workspace` (binaries land in
`.scratch/target-docker/release`).

## F6.4 Results (fuse-identical `.jed`)

| corpus | 1502AS | 1504AS | 1508AS |
|---|---|---|---|
| `feat` (feature matrix, all pinned MCs) | 1564/1615 (96.8%) | 2110/2153 (98.0%) | 1746/1915 (91.2%) |
| `ptcol2` (every source × every block) | 246/251 | 1003/1013 | 3522/3550 |
| `dir` (registered/latched node at every node number) | 64/64 | 128/128 | 4/256 |
| `pinmap` + `pinroles` + `dedic` + `gclk3` | 401/403 | 1884/1884 | 1850/2255 |
| user designs (`~/Projects/pld/cpld`), see F6.5 | — | 0/5 identical; 4/5 fit and match their `.tt2` | 0/3 identical; 3/3 fit and match |

Current figures (2026-11-25, after the switch to our own JED layout, fuse identity):
`feat` 1591/1615, 2146/2153, 1777/1915; `ptcol2` 246/251, 1003/1013, 3522/3550; `pinroles`
180/180, 876/876, 856/1071 — the same as the byte-identity counts just before the switch.

Remaining differences: XOR synthesis (not implemented, all of `feat/xor`), the vendor's UIM mux
choice when several sources compete (almost all 1508 differences), and vendor placement in dense
designs.

**Functional check.** `atf15xx-sim compare A.jed B.jed -d D` (same stimulus, compare pins) and
`tools/sim/equiv.py X.jed --tt2 X.tt2 --fit X.fit` (formal + simulation against the `.tt2`
semantics; docs/architecture/sim/). Every non-identical synthetic
case of the `feat` corpus that does not use XOR mode simulates equivalent to the vendor's JED
(59/59 sampled), and our JEDs for `input`, `rpi-pico-8088` and `spi` match their `.tt2` (as do the
vendor's). A deliberately corrupted fuse is detected.

## F6.5 The user's designs

`.tt2` taken from `~/Projects/pld/cpld` (or compiled from the user's `.pld` with the flags of their
`compile.bash`, into `.scratch/user2/`), fitted by the vendor with the user's options and by ours with
the same options, both JEDs checked against the `.tt2` (until 2026-11-25 by random simulation with a Python model;
now `tools/sim/run_designs.sh`, which runs `equiv.py`). The user calls
`atf1508as-example`, both `blink`s and `spi` the finished designs.

| design | device | vendor JED vs `.tt2` | ours: fits | ours vs `.tt2` | fuse-identical |
|---|---|---|---|---|---|
| `atf1504as-blink/blink` | P1504C44 | match | yes | match | no |
| `atf1508as-blink/blink` | P1508C84 | match | yes | match | no |
| `atf1508as-example/shift-register` (logic doubling, 128 flip-flops) | P1508C84 | match **only with OUT0–OUT43 open collector** (see below) | yes | match (all 64 open collector) | no |
| `atf1504as-spi/spi` | P1504C44 | match | yes | match | no |
| `atf1504as-input/input` | P1504C44 | match | yes | match | no |
| `atf1504as-pico-8088/rpi-pico-8088` | P1504C44 | match | yes | match | no |
| `atf1508as-8051-system-controller/system-controller` | P1508C84 | match | yes | match | no |
| `atf1504as-motioncontroller/motioncontroller` | P1504C44 | match | **no** (UIM congestion) | — | — |

**Vendor defect: truncated `open_collector` list.** For `shift-register` the user's build passes
`-strategy open_collector = OUT0 … OUT63`; the vendor's own `.fit` echoes the property as
`Open_collector = OUT0, … OUT43, O,` and its JED sets `OpenDrain` on 44 macrocells only, so
OUT44–OUT63 are push-pull. (The vendor JED reproduced here is identical to the user's
`shift-register.jed`.) Our fitter applies the option to all 64 outputs.

`atf15xx-fit/tests/conformance.rs` keeps 95 of these synthetic cases (designs generated by
our scripts; expected fuses chosen by the vendor fitter, stored in our JED layout, F5.0) as a
regression test (fuses and checksum must be identical):
`docker/run.sh cargo test --workspace`.
