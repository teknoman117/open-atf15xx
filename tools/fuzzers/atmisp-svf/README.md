# atmisp-svf: how ATMISP turns chains and JEDEC files into SVF

**Derives:** ATMISP 7.3's SVF and `Load`-JEDEC output for every device name, instruction,
chain shape, SVF option and malformed input: the regression vectors
`atf15xx-tool/tests/vectors/<group>/<case>/` (145 cases), which `atf15xx-tool` must reproduce byte
for byte (SVF) and fuse for fuse (Load JEDEC). They pin the behaviour specified in
`docs/architecture/` §2–§4 and §6–§9: chain-file reading, JEDEC reading, the device table (names,
IDCODEs, IR lengths, opcodes), the run algorithm, the SVF writer and the diagnostics.

**Method:** synthetic inputs (JEDEC files with seeded random fuses, chains of 1–n devices,
deliberately broken files) are fed to the real ATMISP through its GUI
(`../common/atmisp_oracle.py` + `drive.c`), and everything it writes is collected.

| script | role |
|---|---|
| `gen_cases1.py`, `gen_cases2.py`, `gen_cases3.py` | the case lists of the committed vectors (erase / program-verify / instructions / Load / SVF options / chains / chain-file and JEDEC errors) |
| `gen_cases4.py` | the voltage-class check for BE + 5V and three-class chains, and the JEDEC `QV`/`X`/`V` fields (2026-11-25: the facts that had only been read from ATMISP's code) |
| `assemble.py` | oracle output → `tests/vectors/<group>/<case>/` (`case.json`, `chain.chn`, `*.jed`, `expected.svf.gz`, `driver.log`) |
| `rewrite_load_vectors.py` | checks each Load result against `atf15xx-tool`'s file (fuses, `G`, checksum) and, with `--write`, stores ours instead of ATMISP's text |
| `verify_jeds.py` | live check of the user's own JEDEC files: ATMISP's SVF vs `atf15xx-tool`'s |

## Run

```
D=.scratch/fuzz/atmisp-svf
python3 tools/fuzzers/atmisp-svf/gen_cases1.py $D/cases.json      # or gen_cases2.py / gen_cases3.py
python3 tools/fuzzers/common/atmisp_oracle.py $D/cases.json $D/out  # ~10–15 s per case
python3 tools/fuzzers/atmisp-svf/assemble.py $D/out $D/vectors
python3 tools/fuzzers/atmisp-svf/rewrite_load_vectors.py --write $D/vectors
# review, copy the new case directories into atf15xx-tool/tests/vectors/<group>/, then
cargo test --release -p atf15xx-tool
```

## Results

145 vectors, all reproduced by `atf15xx-tool` (`atf15xx-tool/tests/vectors.rs`). Inputs that
ATMISP rejects, crashes on or treats inconsistently are kept with `expect: reject` and the class of
spec §9.2. Worklog: `docs/worklog/01-atmisp-svf.md`.
