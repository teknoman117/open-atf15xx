# prjbureau: how it is meant to be used, what works, and how it fits with open-atf150x

*Project Bureau* (whitequark) documents the fuse maps and programming of the Microchip/Atmel ATF15xx
CPLDs. This document records how it is intended to be used (from its own sources), what was
verified here against the vendor tools, the recommended Verilog flow, and the changes made on a
local branch.

* Upstream: <https://github.com/whitequark/prjbureau>, checked out at commit
  `8b8a97122ec238b6a09868a4bc7def20acef798c` (2025-03-10, "util.fuseconv: update to track changes in
  JESD3 parser") into `external/prjbureau/`. Licence: 0-clause BSD (`LICENSE.txt`). 160 commits,
  first 2019-12-12.
* Local branch `open-atf150x` with our changes (§6). Nothing was pushed.
* Container: `docker/prjbureau/` (Ubuntu 24.04: Yosys 0.33, Python 3 + bitarray 2.9, 32-bit wine 9.0).
  Run anything with `docker/prjbureau/run.sh <command>`.

## 1. What prjbureau is (primary sources)

* **A chip database plus documentation.** `docs/intro.rst` ("Installation"): *"the ultimate artefact
  of Project Bureau is a single chip database file … containing a machine-readable description of
  every known fuse in every supported device"* — `database.json`, schema in `docs/schema.rst` /
  `schema.json`. It names every fuse group (product terms `MCx.PTy`, macrocell options such as
  `pt1_mux`, `xor_b_mux`, `storage`, `fb_mux`, `oe_mux`, the interconnect switches `UIMn` with the
  input/feedback signal each mux code selects, global networks `GCLK1..3`/`GCLR`/`GOE1..6`, device
  configuration, UES) and gives pinouts per package and special pins (`docs/naming.rst`).
* **Experimental utilities** (`docs/tools/`, *"provided for experimental purposes only and
  intentionally not packaged"*): `util.fuseconv` (JED ↔ SVF), `util.fusecli` (decode/modify fuses
  symbolically), `util.jesd3` (JED parser), `util.svf` (SVF parser), `util.genhtml` (database
  explorer).
* **No fitter, no place & route, no nextpnr.** The repository contains no placement/routing code;
  `nextpnr` occurs only in the acknowledgements (`docs/intro.rst`: Myrtle Shah *"for building
  nextpnr-generic, and revealing that place & route tools don't have to be opaque"*), as does R. Ou's
  Yosys CPLD (CoolRunner-II) work. The database is meant to enable such tools downstream.
* **How the database was produced.** `bootstrap.sh` runs every `fuzzers/NNN-*/fuzzer.py`; each
  generates small designs, runs the **vendor fitter** through `util/toolchain.py` (Verilog → Yosys
  `write_edif` → `fit15xx.exe -strategy ifmt=edif -strategy optimize=off`, or raw TT2 where EDIF
  cannot express the feature — `fuzzers/024-foldback`, `fuzzers/021-goe_mux`), and diffs the fuse
  maps (`util/bitdiff.py`).
* **The intended Verilog flow** is `v2svf.sh`:

  ```
  yosys -p "read_verilog -lib cells.v; read_verilog NAME.v; hierarchy" -o NAME.edif
  wine fit1502.exe -i NAME.edif -o NAME.jed -strategy ifmt=edif -strategy optimize=off  (+ //OPT: lines)
  python3 -m util.fuseconv NAME.jed NAME.svf
  ```

  Note that it runs **no synthesis**: designs are written structurally with the vendor primitives
  declared in `cells.v` (`AND2`, `DFFE`, `TRI`, …; see `examples/toggle.v`), Yosys only flattens
  and writes EDIF, and the vendor fitter does the rest. It hard-codes `fit1502.exe`.
* **Vendor files** (`vendor/README.txt`): `fit1502/1504/1508.exe`, `aprim.lib`, `atmel.std` with
  pinned SHA-1s — the ProChip Designer builds. The WinCUPL builds installed here (fitter 1.8.7.8,
  2003; `~/.wine32/drive_c/Wincupl/WinCupl/Fitters/`) have different hashes and **no `aprim.lib`**.
* **Status according to upstream** (`docs/intro.rst`, before our changes):

  | Device | Fuse database | Programming |
  |---|---|---|
  | ATF1502AS/ASV/ASL | Complete | Complete |
  | ATF1504AS/ASV/ASL | Near-complete | Untested |
  | ATF1508AS/ASV/ASL | Partial | Untested |
  | ATF1502BE / ATF1504BE | Near-complete | None |
  | ATF1508BE | Partial | None |

  In fact `database.json` contains **only ATF1502AS, ATF1502BE, ATF1504AS and ATF1504BE** — there is
  no ATF1508 entry at all. `docs/jtag/be.rst` says BE programming *"has not been investigated"*.
  `util.fuseconv` supports `ATF1502AS`, `ATF1504AS`, `ATF1508AS` only.

## 2. Running it here

* The vendor fitter runs in the container with `tools/fuzzers/common/fit.sh` (needs the fitter directory
  mounted at `/vendor`, default WinCUPL's). Two non-obvious requirements were found:
  `FITTERDIR` must name a **writable** directory containing `atmel.std` (the fitter puts `*.tmp`/`*.ren`
  there; unset, it uses the drive root) — the WinCUPL installer sets it in `~/.wine32`'s registry,
  `v2svf.sh` sets it too.
  **Check:** the containerised fitter reproduces the user's `atf1504as-spi/spi.jed` byte for byte
  (apart from the date line).
* `tools/fuzzers/prjbureau/v2jed.sh` is `v2svf.sh` generalised to any density. With the WinCUPL fitters it
  stops at `Error - File not found : aprim.lib`: **the prjbureau EDIF flow needs the ProChip Designer
  fitter build**; the user's install cannot run it.

## 3. Cross-checks against open-atf150x's verified knowledge

| Check | Result |
|---|---|
| `util/device.py` JED↔SVF permutations vs spec §5 (ATMISP-derived, verified with random fuse maps) | **identical for every fuse, both directions, ATF1502AS/1504AS/1508AS** (16802/34186/74134 fuses) |
| Words programmed by `fuseconv` SVF vs ATMISP SVF, user's real designs (`spi`, `motioncontroller` ATF1504AS; `system-controller` ATF1508AS) | **identical** (216/216, 216/216, 234/234 words) |
| Same, for Yosys-flow designs on ATF1502AS/1504AS/1508AS (§4) | identical (212, 216, 234 words) |
| `database.json` UIM mux fuse ranges vs the `Mux-n of block X` labels in ATMISP's Load templates | **identical** (80/80 ATF1502AS/BE, 160/160 ATF1504AS/BE) |
| `database.json` product-term ranges vs ATMISP's `PT n MC m` labels | same five ranges per macrocell, but for half the macrocells (alternating in pairs) the numbering is reversed (prjbureau PT1..PT5 = ATMISP PT5..PT1) |
| Which numbering is functionally right | **prjbureau's.** A design with a product-term output enable (`Y.OE = B & C`) placed by the vendor fitter on MC1, MC3, MC4 of ATF1504AS always puts the OE term in the range prjbureau calls PT5 (the only PT that can drive OE), including MC1 where ATMISP's note calls it PT1. ATMISP's template notes number PTs by position only. |
| `fusecli` decode of a vendor-fitted ATF1504AS design (§4) | coherent: GCLK1 clock, GCLR reset, PT4 as clock enable from the `en` switch, GOE4 output enable, D flip-flop |
| SVF sequence | `fuseconv` writes a simplified version of ATMISP's sequence (no extra 1 µs/50 ms waits, `RUNTEST IDLE … SEC` syntax, one erase/program/verify pass); the programmed data is identical |

**Coverage gaps:** no ATF1508AS/ATF1508BE database entries (so `fusecli` cannot decode 1508 files);
1504 entries are "near-complete" upstream. The fuzzers that would extend them use EDIF and therefore
need the ProChip Designer fitter build (with `aprim.lib`); the TT2 input path (§4) could be used
instead.

## 4. Verilog → sum of products: recommended approach

Options evaluated (all in the container):

| Option | Result |
|---|---|
| prjbureau's `v2svf.sh` (structural `cells.v` netlist → EDIF → vendor fitter) | no synthesis; needs `aprim.lib` (absent from WinCUPL); EDIF cannot express foldback or buried-node global OE (prjbureau's own fuzzer comments) |
| Yosys `synth` + `abc -sop` → `.tt2` → vendor fitter | **works on all three densities with the WinCUPL fitters; verified end to end through ATMISP** |
| Yosys techmap to custom macrocell cells (à la `synth_coolrunner2`) | needs our own fitter; the SOP netlist below is the natural input to it, so it can come later without changing the front end |

**Recommended:** Yosys produces a technology-independent *sum-of-products + legal storage* netlist,
written as Yosys JSON:

```
proc; tribuf; synth -flatten
dfflegalize -cell $_DFF_P_ x -cell $_DFFE_PP_ x -cell $_DFF_PP0_ x -cell $_DFF_PP1_ x \
            -cell $_DFFE_PP0P_ x -cell $_DFFE_PP1P_ x -cell $_DLATCH_P_ x
abc -sop -I 40 -P 80; opt_clean -purge; write_json -compat-int out.json
```

(`tools/sim/synth_atf15xx.ys`). What remains is exactly the macrocell vocabulary: `$sop`
covers, D flip-flops with optional clock enable and asynchronous reset *or* set, transparent
latches, and `$_TBUF_` (output enable). This one netlist can feed:

* **the vendor fitter**, via `tools/sim/yosys2tt2.py` (prototype), which writes CUPL-style TT2
  (`X.REG/.C/.CE/.AR/.AP/.L/.LE/.OE`, `X.Q` register and `X.PIN` pin feedback, `#$ PINS`/`#$ NODES`);
  a register whose Q drives a port is placed in that pin's macrocell, other registers and shared
  logic become buried nodes; pins via `--pin NAME=NUMBER`;
* **our fitter crates** (SOP + registers + pin constraints IR) directly;
* prjbureau's database for the final assembly (fuse names/values) once an open fitter places it.

Prototype status (`.scratch/prjbureau-explore/flow*/`):

| Design | Device | Fits | ATMISP SVF vs `atf15xx-tool` | `fuseconv` words vs ATMISP |
|---|---|---|---|---|
| 4-bit counter, clock enable, async reset, tri-state outputs | ATF1504AS PLCC44 | yes | MATCH | identical |
| same | ATF1508AS PLCC84 | yes | MATCH | identical |
| same | ATF1502AS PLCC44 (synthetic only; no samples) | yes | MATCH | identical |
| latches, async preset, shared node, bidirectional bus with OE and pin read-back | ATF1504AS PLCC44 | yes | MATCH | — |

The fitter's report (`*.fit`, "programmed logic") shows the expected equations in every case.
Not yet done: timing/functional simulation of the fitted result, passing fitter properties (pin
keepers, slew, open collector, foldback) from Verilog attributes, T flip-flops (the fitter chooses
them itself), multi-bit `inout` beyond the tested case, and placement constraints other than pin
numbers. Nothing has been programmed into silicon.

Full run (in the container):

```
yosys -p "read_verilog design.v; hierarchy -top top; script tools/sim/synth_atf15xx.ys"
python3 tools/sim/yosys2tt2.py out.json -o design.tt2 --device f1504plcc44 --pin clk=43 ...
tools/fuzzers/common/fit.sh fit1504 design.tt2 -device P1504C44 -preassign keep -strategy JTAG on
# host:
atf15xx-tool svf -d ATF1504AS:pv:design.jed -o design.svf
```

## 5. What the fitter crates should use or cross-check

* PT identities: use **prjbureau's** `pterm_ranges` (functionally verified above), not the PT
  numbers in ATMISP's template notes.
* UIM mux fuse positions agree between prjbureau and ATMISP; prjbureau additionally gives the
  input selected by each mux code for 1502/1504 — cross-check, and extend to 1508 with the TT2 path.
* AS JED→word packing: prjbureau's `util/device.py` = our spec §5 (proven equal).
* Use `tools/fuzzers/common/fit.sh` + `yosys2tt2.py` to generate oracle cases from Verilog.

## 6. Changes on the local `external/prjbureau` branch `open-atf150x`

| Commit | Change |
|---|---|
| `09141f6` | `fuseconv`: JED↔SVF packing, programming sequence and SVF reading for ATF1502BE, ATF1504BE, ATF1508BE, ATF1508RE (`util/device_be.py`), with round-trip tests (`util/test_fuseconv.py`). Words written equal ATMISP's for every BE device. AS output unchanged. |
| `303db3d` | `cells.v`: the `NOR*` cells assigned an undeclared net `NQ`; `QN` was undriven (proved with Yosys `sat`). |
| `9d6a51e` | docs: BE programming chapter (`docs/jtag/be.rst`), verification notes (`docs/jtag/as.rst`), status table (`docs/intro.rst`: 1504/1508/BE programming "matches vendor SVF", not silicon-tested). |
| `1807a50` | docs: notes on running the vendor fitter (`docs/fitter.rst`: fitter builds, `aprim.lib`, `FITTERDIR`, TT2 input). |

The documentation builds with Sphinx (`docs/requirements.txt`) in the container. One upstream unit
test (`util.svf`, `test_hir_sir_tir_hdr_sdr_tdr`) fails with bitarray 2.9 both before and after these
changes (an error-message length difference) — not caused by the branch.
