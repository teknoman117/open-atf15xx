# Importing prjbureau data into open-atf150x

Status: **done** (2026-11-25, second form). Everything we take from
[prjbureau](https://github.com/whitequark/prjbureau) comes in through the `atf15xx-arch` crate,
whose build script (`atf15xx-arch/build/`) turns prjbureau's files into Rust statics at compile
time. Nothing is parsed at run time and no prjbureau file is stored in this repository; only the
generated Rust (in cargo's `OUT_DIR`) is compiled in. (The first form, 2026-11-25 too, was a Python
generator writing a committed `arch.json` that the crate parsed at run time; replaced at the user's
request.)

## 1. Sources

| source | licence | what is generated from it | where |
|---|---|---|---|
| prjbureau `database.json` (submodule `external/prjbureau`) | 0BSD | the fuse map of each family: macrocell-configuration field addresses, UIM selector addresses, the UIM and global-OE input tables | `build/fuse_map.rs` → `atf15xx_arch::Family` |
| prjbureau `bsdl/*.bsd` | **Atmel copyright** (`bsdl/README.txt`: "not a part of Project Bureau") | the boundary register of every part (cell order and kinds, macrocells, pins), opcodes, IDCODEs, IR length; checked against the device tables | `build/bsdl.rs` → `atf15xx_arch::bsdl` |
| `atf15xx-arch/data/devices.json` | ours | the devices: pin ↔ macrocell, JTAG / power-down / dedicated pins (black-box fitter experiments, `tools/fuzzers/fitter-pins`) | `build/devices.rs` → `atf15xx_arch::Device` |
| `atf15xx-arch/data/timing.json` | ours | the speed-grade tables (datasheets + the fitters' SDF output, `tools/fuzzers/fitter-timing`) | `build/timing.rs` → `atf15xx_arch::timing` |

The build fails if the submodule is not checked out (`git submodule update --init`); set
`ATF15XX_PRJBUREAU=DIR` to build against another prjbureau checkout.

## 2. The pinned version

`external/prjbureau` is the user's fork (`git@github.com:teknoman117/prjbureau.git`), pinned at
branch `atf1508as` (`e634784`, recorded in `.gitmodules`): upstream's 1502/1504 data unchanged
(upstream `8b8a971`), plus the ATF1508AS entry produced by prjbureau's own fuzzers with the
WinCUPL fitter through TT2 (`tools/fuzzers/prjbureau`; pending upstream). It is the only commit
that has every device we need. When upstream takes the branch, the pin moves to upstream's commit.

## 3. What the build script does with `database.json`

| generated | from prjbureau |
|---|---|
| `qf`, `uim_width` | `ranges.reserved[1]`; fuses per `switches.UIMn.mux` |
| macrocell fields (32 fuses, two bits per macrocell) | the macrocell option fuses (`macrocells.MCn.<option>.fuses`), grouped into the fields our decoder uses, the address of each derived from any option fuse in it and every other option fuse checked against `Family::mc_bit_addr` |
| UIM selector addresses | `blocks.<b>.pterm_points.UIMn_P` (PT input column 16 + 2k) + `switches.UIMn.mux.fuses` |
| UIM input table (mux *k*, position *i*) | `switches.UIMn.mux.values`: one-hot-zero value → position; prjbureau names (`MC5_FB`, `M3_PAD`, `C1_PAD`…) → `Source` |
| global-OE input table | `globals.GOEn.mux.values` |

Rules: a macrocell field the database does not determine (one per 1508 block: field (0,1)) is
taken from the ATF1504AS, where field (0,1) = field (2,17) − 32 holds in every block; an input
table entry it does not determine (24 UIM, 57 GOE entries of the 1508, used by no vendor JED and
by none of ours) is `Source::None`, never selected. Any other inconsistency fails the build. The
McBit layout both sides use is one shared file (`atf15xx-arch/src/layout.rs`).

Product-term addresses and the config bits stay formulas in `lib.rs` (ours, oracle-verified); they
equal prjbureau's `pterm_ranges` and config fuses (`tools/fuzzers/fuse-regions` checks the PT
numbering against the fitter's labels too).

Checked when introduced (2026-11-25): the generated tables equal the committed `arch.json` they
replace in every family field and every device, and the generated ATF1508AS-PLCC84 boundary map
equals the map verified with SAMPLE/EXTEST on the bench chip in all 352 cells.

## 4. Scripts

Python scripts get the tables from the Rust tools they already run, as plain text:
`atf15xx-sim device D` (pins), `atf15xx-sim fuses F` (every fuse field and the UIM / GOE inputs),
`atf15xx-sim mc` (a macrocell's bits; `--set` edits a JED), `atf15xx-sim stats`; the debugger's
`atf15xx-tool part` / `sample --json` for boundary scan. No table is copied out of the crates.

## 5. Publishing

The build script reads files outside the crate (`../external/prjbureau`), so `atf15xx-arch` cannot
be published to crates.io as it is. If it is to be published, the package step would have to ship
the generated Rust (or the inputs) inside the crate; not needed while the project is a workspace.

## 6. Device coverage

| device | prjbureau | here |
|---|---|---|
| ATF1502AS, ATF1504AS | upstream, complete | generated |
| ATF1508AS (our bench chip) | our branch `atf1508as` (pending upstream) | generated |
| ATF1502BE, ATF1504BE | upstream | not modelled by the arch crate (the fitter targets AS only) |
| ATF1508BE | stub | — |

Known gap of the TT2 route (WinCUPL fitter builds, no `aprim.lib`): the fitter does not accept I/O
pads as global-OE drivers from TT2 input, so those GOE entries of the 1508 stay undetermined.

## 7. Disagreements

| fact | prjbureau | ours | evidence | status |
|---|---|---|---|---|
| 1502AS GOE invert fuse offsets | `C+10−n` | `C+3+n` | vendor fitter behaviour (`exp_feat.py` `oe_g1n`/`oe_g2n`) | ours is a formula in `lib.rs` (config bits are not taken from prjbureau); silicon test pending; report upstream when settled |

## 8. Contributions flow upstream first

Every fact we can contribute goes to prjbureau as a branch of the fork (`docs/prjbureau/branches.md`,
worktrees in `.worktrees/prjbureau/`), and the build then consumes it from the pinned submodule.
Nothing is proposed upstream without the user's go-ahead.
