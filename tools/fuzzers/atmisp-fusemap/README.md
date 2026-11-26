# atmisp-fusemap: where each fuse goes in ATMISP's programming rows

**Derives:** for every family (1502/1504/1508 AS and BE, plus the 1508RE), which fuse of the JEDEC
array each bit of each SVF scan carries: the programming rows, the configuration / special / UES
registers (AS), the fuses that appear nowhere (reserved) or in two cells, and the row geometry.
This is the content of spec §5 (fuse maps), §4.4 (geometry and register layout) and the Load
inverse map (§8), and it is what `atf15xx-tool/src/isp/fusemap.rs` implements.

**Method:** the JEDEC files encode each fuse's index (`gen_cases.py`): one with every fuse 0, one
with every fuse 1, and one per index bit K with fuse i = bit K of i (17 for the 1508). ATMISP
programs-and-verifies each in SVF mode. A scan bit that copies fuse i then reads, across the
files, 0, 1 and the binary digits of i; one that ignores the fuses is constant. `decode.py` reads
the index of every bit back (and fails on any bit that is neither), and `check.py` requires
`atf15xx-tool` to write byte-identical SVF for every file, which, since every fuse has a distinct
signature, means every cell copies the same fuse in both.

| script | role |
|---|---|
| `gen_cases.py` | 17–19 bit-pattern JEDEC files per device (127 cases for the seven device names) |
| `decode.py` | SVFs → `map.json`: per device the cell → fuse map, the fuse ranges per instruction (rows vs. registers), scan lengths, fuses in no cell / several cells |
| `check.py` | `atf15xx-tool svf` on the same files must equal ATMISP's SVF (except the date line) |

## Run

```
python3 tools/fuzzers/atmisp-fusemap/gen_cases.py .scratch/fuzz/atmisp-fusemap/cases.json
python3 tools/fuzzers/common/atmisp_oracle.py .scratch/fuzz/atmisp-fusemap/cases.json .scratch/fuzz/atmisp-fusemap/out
python3 tools/fuzzers/atmisp-fusemap/decode.py .scratch/fuzz/atmisp-fusemap/out .scratch/fuzz/atmisp-fusemap/map.json
cargo build --release -p atf15xx-tool
python3 tools/fuzzers/atmisp-fusemap/check.py .scratch/fuzz/atmisp-fusemap/out
```

About 25 minutes of ATMISP (10–15 s per case).

## Results (2026-11-25)

See `RESULTS.md` (written from `decode.py` and `check.py` output). Worklog:
`docs/worklog/01-atmisp-svf.md`.
