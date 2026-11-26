# 01 — ATMISP 7.3 → specification → `atf15xx-tool`

## Status

**2026-11-25: Load JEDEC in our own layout.** `atf15xx-tool` no longer renders ATMISP's `Load` templates;
it writes the same fuses in our own layout (spec §8.2), and the templates are deleted. Load
conformance is fuse identity (fuses, `G`, checksum); SVF conformance is still byte identity, so the
SVF writer still reproduces ATMISP's header comment `SVF file written by ATMISP version 7.3` (open).

**2026-11-23: complete.** Spec chapters §1–§11 are written. `atf15xx-tool` (Rust) is byte-identical
to ATMISP 7.3 on all 139 recorded vectors and on all 16 Microchip sample designs (Program/Verify).
Load output was checked on 3 sample designs. No open work. Further changes only if the fitter or
simulator workstreams find something.

## Scope and ownership

Conversion of chain files (`.chn`) + JEDEC files into SVF, and ATMISP's `Load` JEDEC writer. Owns:
`docs/architecture/01…11*.md`, `src/`,
`tests/vectors/`, `tests/vectors.rs`, `tools/fuzzers/atmisp-svf/`, `tools/fuzzers/atmisp-fusemap/`, `tools/fuzzers/common/drive.c`.

## Results

| Check | Result | Reproduce |
|---|---|---|
| Recorded ATMISP vectors (synthetic JEDs, every device name × instruction × Rev C/D × state reset, chains, chain-file and JEDEC edge cases) | 139/139: SVF byte-identical, Load JEDEC (9 vectors) fuse-identical (reject cases rejected) | `cargo test` |
| Microchip sample designs shipped with ATMISP, Program/Verify, Rev D | 16/16 byte-identical | `python3 tools/fuzzers/atmisp-svf/verify_jeds.py ~/.wine32/drive_c/ATMISP7/JEDEC/*/*.jed ~/.wine32/drive_c/ATMISP7/JEDEC/*/*/*.jed` |
| Load (JEDEC written by `atf15xx-tool` vs ATMISP) | 3/3 identical (1504AS, 1502BE, 1508BE samples; byte-identical with the templates, 2026-11-23); since 2026-11-25 own layout: 9/9 Load vectors fuse-, `G`- and checksum-identical to ATMISP's files | `verify_jeds.py --instr pv,load …` (wine); `python3 tools/fuzzers/atmisp-svf/rewrite_load_vectors.py` (checks the committed vectors) |
| Fuse-map fuzzing (bit-pattern JEDEC files through ATMISP, every scan bit decoded) | `atf15xx-tool` SVF identical 127/127 | `tools/fuzzers/atmisp-fusemap` (README) |

## Evidence levels (summary; details in the spec chapters)

- **Oracle-verified** (ATMISP 7.3 under wine, driven through its GUI by `tools/fuzzers/common/drive.c`):
  every SVF sequence in §6–§7, chain composition, the header, delays (Rev C/D), state reset,
  chain-file parsing incl. edge cases (§2), JEDEC field handling (§3 except where noted), Load
  templates (§8), the transcript texts (§9.3), and the defects D1–D5, D7 and D9.
- **Fuse-map fuzzing** (`tools/fuzzers/atmisp-fusemap`): fuse→row maps (§5), register layout
  (§4.4), the fuses Load cannot write (§8): every fuse's position read back from ATMISP's SVF for
  bit-pattern JEDEC files; `atf15xx-tool` identical on 127/127.
- **Oracle, added 2026-11-25** (`gen_cases4.py`, 6 vectors): the voltage check for BE + 5V and
  three-class chains (V1, either order) and the JEDEC `QV`/`X`/`V` fields (no effect).
- **Inferred**: nothing normative.

## Decisions

- **2026-11-19: Black-box specification.** ATMISP is treated as an oracle only: the spec is derived
  from the SVF and JEDEC files it writes for inputs we generate (`tools/fuzzers/`).
- **2026-11-19: GUI oracle via a Win32 driver.** `ATMISP.exe file.chn -1` only drives the USB
  cable; SVF needs the GUI. A MinGW-built `drive.exe` sets the dialog controls by ID (1019 path,
  1020 checkbox, 1022 revision (sorted: C = 0, D = 1), 1023 TCK, 1024 state reset, 1007 Run), runs
  in `wine explorer /desktop=…`, and records dialogs and the transcript.
- **2026-11-19: Reproduce ATMISP defects by default; `--strict` corrects the safe ones** (§9.4).
  Byte compatibility is the conformance criterion.
- **2026-11-19: Reject, rather than imitate, undefined behaviour** (§9.2 J1/J2/N1/N2): wrong-family
  JEDEC, JEDEC fields ≥ 200 bytes, fuse digits other than 0/1, SE device names, UNKNOWN devices with
  non-Bypass instructions. ATMISP crashes or emits garbage on these.
- **2026-11-25: Load writer in our own layout (supersedes the next entry).** For copyright hygiene
  no ATMISP-written JEDEC text stays in the repository: the six templates are deleted and
  `jedout::render` generates the file from atmisp's family layout (§4.4): header, `QF`, `F0`, `G0`
  (none for 1502BE, as ATMISP), the logic array in 96-fuse `L` fields, the configuration, special,
  UES and reserved regions each in one labelled `L` field, `C` (4 hex digits), ETX `0000`.
  Conformance (§11.3) is fuse identity. Alternatives: keep the templates out of git and load them
  at run time from a user directory (byte-exact mode; not done: no function needs the vendor's
  layout, and the files would have to come from each user's ATMISP install), or strip only the
  NOTE text from the templates (still ATMISP's layout and header).
- **2026-11-19: Load writer as templates.** ATMISP's annotated JEDEC layout is fixed per family, so it
  was stored as six byte-exact templates with `#` per fuse (`docs/architecture/data/jedec-templates/`,
  deleted 2026-11-25).
- **2026-11-19: Test vectors use synthetic, seeded-random JEDEC files only**, stored gzip
  (1.6 MB). Microchip's sample designs are used for verification but never committed.
- **2026-11-19: Upstream crates** (clap, chrono; serde_json and flate2 for tests) instead of
  hand-rolled parsing and time formatting (user preference).

## Journal

- **2026-11-25**: Fuse maps re-derived black-box (`tools/fuzzers/atmisp-fusemap`, RESULTS.md):
  bit-pattern JEDEC files (all 0, all 1, fuse i = bit K of i) through ATMISP Program/Verify for
  ATF1502AS/1504AS/1508AS/1502BE/1504BE/1508BE/1508RE, 127 runs (~10.5 s each); every scan bit
  decodes to one fuse or a constant (0 inverted); `atf15xx-tool` SVF identical 127/127. AS
  register ranges and reserved fuses = §4.4; BE fuses in no row = §5.3; spec §5, §8.1 and §11.1
  cite this evidence. The 1508BE JTAG fuse is 74115 like the 1508AS (configuration start + 33);
  it only drives our warning W6.
* **2026-11-25** — Merged into the crate `atf15xx-tool` (user request): the ATMISP-compatible
  conversions are its module `isp` and the commands `svf`, `chain`, `jed` of the binary
  `atf15xx-tool` (same options and exit codes as `atmisp`); the in-circuit debugger is its module
  `dbg` (feature `jtag`, default) with the commands `part` (was `info`), `idcode`, `probe` (was
  `chain`, which now means the chain files), `sample`, `watch`, `capture`, `drive`, `seq`. The flow
  depends on the crate without default features (no BSDL/VCD dependencies). Tests unchanged in
  substance: 139 ATMISP vectors, the BSDL/bench-map and mock-TAP tests all pass.

- **2026-11-25**: Load JEDEC writer without ATMISP text (task "JED writers without vendor text").
  `atf15xx-tool/src/isp/jedout.rs`: templates and `include_bytes!` removed; `render` writes our layout
  (spec §8.2/§8.3 rewritten). New unit test `render_round_trips_through_the_reader` (all 6 families,
  pseudo-random fuses: the reader returns the same fuses, family, `G`, stated checksum; `G0`
  present except 1502BE; every field < 200 bytes). `tests/vectors.rs` compares Load outputs fuse for
  fuse (`load_output_matches`: fuses, QF, security fuse, stated = computed checksum); a mutated
  expected fuse fails the test (checked). The 9 Load vectors' `expected-a.jed.gz` (7 in `load/`,
  `instructions/instr5_ATF1504AS`, `instr5_ATF1504BE`) were rewritten by the new
  `tools/fuzzers/atmisp-svf/rewrite_load_vectors.py`, after it confirmed for each that the file atmisp writes
  has the same fuses, `G` and checksum as ATMISP's (2BCF, 2C0B, A052 ×2, A08E, E0D ×2, ED0 ×2)
  **[O]** (against the ATMISP files recorded 2026-11-19). `verify_jeds.py` compares Load outputs
  the same way. `tools/fuzzers/fuse-regions/regions.py` (read the templates) now takes a template directory
  made from one's own ATMISP output. Spec §11.2/§11.3/§11.4 updated. `cargo test -p atf15xx-tool`: all
  pass; workspace 36 passed, 0 failed. Reproduce: `cargo test --release -p atf15xx-tool`;
  `python3 tools/fuzzers/atmisp-svf/rewrite_load_vectors.py` (check only).
- **2026-11-23**: Fixed `resolve()` in `src/main.rs`: Windows-style JEDEC paths were used verbatim on
  Linux, so `Load` wrote stray files named `C:\probe\…\a.jed` into the working directory. This also
  made `verify_jeds.py`'s Load check compare ATMISP's file with itself. Both are fixed; Load
  re-verified (3/3).
- **2026-11-19**: Added `tools/fuzzers/atmisp-svf/verify_jeds.py` (live comparison of any JEDEC file against
  ATMISP) and its documentation.
- **2026-11-19**: JEDEC edge cases established with the oracle: over-long fields crash ATMISP or corrupt its
  output (rejected, J2); digits 2–9 are stored and make hex output undefined (rejected); `NOTE`
  fields are skipped; identifiers are case-insensitive; CR inside an `L` address is ignored.
- **2026-11-19**: Initially assumed that fields ≥ 1999 bytes are rejected and that L fields are
  "truncated at 200 bytes". Both were wrong; corrected after the oracle showed undefined output.
- **2026-11-19**: Prototype bug: the first JEDEC regex skipped every other `L` field. Replaced by
  ATMISP-style field splitting.
- **2026-11-19**: Oracle runs stalled when a case crashed ATMISP (winedbg kept wineserver alive). The
  runner now kills wineserver after each case.
- **2026-11-19**: Chain file line 2 is discarded unconditionally (found when a file without the
  blank line lost its first device).

## Open issues

- The 1502AS global-OE invert-bit disagreement between prjbureau and the vendor fitter (fitter
  workstream) doesn't affect this workstream: SVF generation copies fuses verbatim.
- Nothing is silicon-tested; correctness is defined as "identical to ATMISP 7.3".
- The SVF writer reproduces ATMISP's header comment `SVF file written by ATMISP version 7.3` (§6),
  and the SVF vectors are ATMISP's own output; both remain while SVF conformance is byte identity
  (2026-11-25).

## Artifacts

- Spec: `docs/architecture/README.md` and §1–§11. (The Load templates were deleted 2026-11-25.)
- Implementation: `src/` (crate `atf15xx-tool`). Tests: `tests/vectors/` (139 cases), `tests/vectors.rs`.
- Oracle tooling: `tools/fuzzers/atmisp-svf/`, `tools/fuzzers/atmisp-fusemap/` (see `tools/README.md`).
- Oracle environment: ATMISP 7.3 in `~/.wine32` (`C:\ATMISP7`); driver at `C:\probe\drive.exe`.
