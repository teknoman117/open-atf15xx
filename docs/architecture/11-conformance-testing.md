# 11. Conformance testing

## 11.1 How this specification was derived

| Technique | Used for |
|---|---|
| **Oracle runs**: the real ATMISP 7.3 under wine, driven through its own GUI by a small Win32 program that loads a chain, sets the SVF controls and presses *Run* (`tools/fuzzers/atmisp-svf`) | ground truth for every output byte: chain and JEDEC reading, the device table, the run algorithm, the SVF writer, the diagnostics (§2–§4, §6–§9) |
| **Fuse-map fuzzing**: JEDEC files that give every fuse a distinct bit pattern, programmed by ATMISP; the fuse behind every scan bit read back from the SVFs (`tools/fuzzers/atmisp-fusemap`) | fuse maps (§5), register layout and geometry (§4.4), the fuses Load cannot write (§8) |

Nothing was taken from the program's code. (An early phase of the project also read ATMISP's
binary; that work was retired on 2026-11-25 and every fact it had contributed was re-derived and
re-verified by the two techniques above, `docs/worklog/00-project.md`.)

The oracle tooling lives in [`tools/fuzzers/`](../../tools/fuzzers/). `common/drive.c` (cross-compiled with
`i686-w64-mingw32-gcc`) starts `ATMISP.exe <chain>`, ticks *Write SVF file* (control 1020), types
the path (1019), selects the revision (combo 1022: index 0 = Rev C, 1 = Rev D), sets *Use state
reset* (1024) and the TCK period (1023), clicks *Run* (1007), answers dialogs, waits for the file to
stop growing, and records dialogs and the transcript list box (1013). `common/atmisp_oracle.py`
runs the cases inside a wine virtual desktop and collects the outputs.

## 11.2 Test vectors

`tests/vectors/<group>/<case>/` contains, for each case:

| File | Content |
|---|---|
| `case.json` | the SVF options (`rev`, `tck`, `state_reset`), the device list used to build the chain, and `expect`: `ok`, or `reject` with `reject_class` (E2, E3, J1, J2, N1, V1, TCK) for inputs ATMISP refused, crashed on, or handled with undefined behaviour |
| `chain.chn` | the chain file given to ATMISP (paths are `C:\probe\cases\<case>\…`) |
| `*.jed` | input JEDEC files (synthetic, pseudo-random fuse data — no vendor design files are used) |
| `expected.svf.gz` | the SVF ATMISP 7.3 wrote (gzip) |
| `expected-<name>.jed.gz` | for `Load` cases, the fuses of the JEDEC file ATMISP wrote, rewritten in atmisp's own layout (§8.2; gzip). Verified fuse for fuse against ATMISP's file before it was replaced (`tools/fuzzers/atmisp-svf/rewrite_load_vectors.py`) |
| `driver.log` | dialogs and transcript observed |

Coverage: every device name × `Erase` in Rev D (state reset on and off) and Rev C; every family ×
`Program/Verify` with random fuse data (this validates the fuse maps end to end); every instruction
on AS-type and BE-type devices; `G1` auto-secure; Rev C with TCK 1, 2, 3, 1000; multi-device chains
with `UNKNOWN` devices (with and without IDCODE flag, widths 5–12) and mixed families; chain-file
edge cases (CR LF, missing line 2, bad widths, calibration and port/cable codes); JEDEC edge cases
(bad checksum, default `F1`, `X`/`2` characters, over-long `L` fields (rejected), lower-case
identifiers, `NOTE` fields, CR inside an `L` address, JTAG fuse 0).

## 11.3 Acceptance criteria

An implementation conforms if, for every vector:

1. it accepts the chain (or rejects it with the expected error class when `driver.log` shows ATMISP
   rejected or crashed on it), and
2. its SVF equals the decompressed `expected.svf.gz` after removing line 2 (`//  Created on: …`) from both, comparing
   bytes (CR LF included), and
3. for `Load` cases, its JEDEC output holds the same fuse array and security fuse as the decompressed
   `expected-*.jed.gz` and states the correct fuse checksum (§3.6). The text layout is not compared
   (§8.2).

`cargo test` runs all vectors (`tests/vectors.rs`). Strict mode (§9.4) is not covered by vectors,
since ATMISP never produces strict output.

## 11.4 Regenerating vectors

```
WINEPREFIX=~/.wine32 wine 'C:\ATMISP7\ATMISP.exe'      # ATMISP 7.3 must be installed in this prefix
i686-w64-mingw32-gcc -O1 -o drive.exe tools/fuzzers/common/drive.c -luser32
cp drive.exe ~/.wine32/drive_c/probe/
python3 tools/fuzzers/atmisp-svf/gen_cases.py cases.json
python3 tools/fuzzers/common/atmisp_oracle.py cases.json tests/vectors/<group>
python3 tools/fuzzers/atmisp-svf/rewrite_load_vectors.py --write    # Load outputs: check fuses, rewrite in our layout
```

Never commit ATMISP's own `Load` JEDEC files: `rewrite_load_vectors.py` checks each against the
file atmisp writes (fuses, `G`, checksum) and replaces it with atmisp's. The SVF vectors
(`expected.svf.gz`) are ATMISP's output, compared byte for byte; they include ATMISP's SVF header
comment.

Each case takes about 10–15 s. Kill `wineserver` between cases (the runner does this): some inputs
crash ATMISP and leave a debugger attached.

## 11.5 Verifying with your own JEDEC files

`tools/fuzzers/atmisp-svf/verify_jeds.py` runs arbitrary JEDEC files through both ATMISP 7.3 (under wine) and
`atf15xx-tool` and compares the results (SVF byte for byte, `Load` JEDEC fuse for fuse), reporting `MATCH`, `OK-REJECT`, `DIVERGE`
(a deliberate rejection per §9.2) or `MISMATCH` per file × instruction × revision. Setup, options and
the meaning of each verdict are documented in [`tools/README.md`](../../tools/README.md#verifying-your-own-real-jedec-files).
