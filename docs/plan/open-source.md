# Open-source plan (sketch, 2026-11-24)

Everything here is meant to become public. This page sketches **what to contribute upstream**,
**what lives in our own repositories**, **how programming should work** (OpenOCD), and the
**Rust hygiene rules** (idiomatic code, maintained crates instead of hand-rolled logic). Nothing has
been pushed or proposed anywhere yet; each upstream step needs the user's go-ahead.

## 1. Ground rules

* **No vendor files** are redistributed: no fitter/ATMISP binaries, no Microchip sample designs,
  no vendor-written JEDs or reports. Test vectors stay synthetic (our designs, vendor *outputs* only
  where we generated the input and the output is a fuse map, as today).
* **Facts, not copies.** Fuse maps, UIM tables, timing tables and the fuse→row maps were extracted
  from vendor binaries or by oracle experiments. We publish them as data with provenance
  (the extraction tools and the evidence level of each fact), as prjbureau does.
  *Decided 2026-11-25:* **no vendor-written JEDEC text in the repository.** The ATMISP *Load*
  templates (`data/jedec-templates/*.jedt`) were deleted; `atf15xx-tool` writes its *Load* output in our
  own layout (spec §8.2), and the fitter's vendor-layout writer was replaced by our own layout
  (F5.0). Conformance is checked at the fuse level (same fuses, security fuse and checksum); the
  committed vendor-output JEDs (95 fitter vectors, 9 ATMISP *Load* vectors) were rewritten in our
  layout after checking that the fuses are unchanged. ATMISP's Load text is read only
  locally, for its region labels (`tools/fuzzers/fuse-regions`), never committed.
  Still open: the SVF writer reproduces ATMISP's SVF header comment (`SVF file written by ATMISP
  version 7.3`), and the SVF vectors (`expected.svf.gz`) are ATMISP's own output.
* Licences: the project is ISC (`LICENSE`, as Yosys and nextpnr); prjbureau is 0BSD; Yosys is ISC;
  OpenOCD is GPL-2.0-or-later (a driver contributed there is GPL).

## 2. Contributions to prjbureau (whitequark/prjbureau, 0BSD)

prjbureau is the established home for ATF15xx bitstream documentation, so documentation-level
facts should go there, in its schema and naming. Proposed, in order:

| contribution | what | evidence |
|---|---|---|
| **ATF1508AS database** | the 1508 in prjbureau's `database.json` schema (it has 1502/1504 AS/BE only): PT ranges, macrocell option fuses, UIM fields and selectable inputs, GOE fields, config bits, pins for PLCC84/TQFP100/PQFP100/PQFP160 | our oracle-verified map (`atf15xx-arch/data/devices.json`, F2), cross-checked 100 % against prjbureau where both cover a device |
| **BE-series programming** | branch `fuseconv-be` on the fork (`docs/prjbureau/branches.md`) | ATMISP 7.3 SVF semantics (spec §5–§7) |
| **Silicon results** | reset beats preset (PT and GCLR); register state after ISP = 0 independent of `reset`/`xor_invert`; GCLK network source mux + invert (any network from any clock pin, ATF1508AS); cascade lender's XOR A = `y2` confirmed (`VCC` with PT2 in the sum); foldback chaining; fast input register; floating dedicated inputs misbehave with the keeper off | F2.9, `tools/silicon/` experiments, ATF1508AS-PLCC84 |
| **Open items they can help with** | 1502AS GOE invert-bit disagreement (needs a 1502 on silicon), the `CasMux`+`Pt2Mux` XOR corner case | F2.7, F2.9 |
| **Timing model** | per-family speed-grade tables (5 grades) and the datasheet path equations | F8, `timing.json` |
| **Docs** | PT numbering note (ATMISP's Load labels are reversed for half the macrocells) | F2 |

Process: open an issue first describing the data and its provenance, then PRs per item. The
database generator should be a script in prjbureau's style (Python, reading our oracle results),
not a dump.

## 3. What stays in our repositories

Proposed split of this repository (today one repo; history is kept per workstream, so splitting
with `git filter-repo` is straightforward):

| repo | contents | publish |
|---|---|---|
| **`atf15xx`** (Rust workspace) | `atf15xx-arch` (architecture + fuse map data + JED decode), `atf15xx-fit` (fitter library and the `fit1502/1504/1508` vendor-compatible binaries), `atf15xx-sim` (model, simulator, Verilog emitter), `atf15xx-synth` (Yosys JSON → JED/SVF driver) (C ABI removed 2026-11-25), `atf15xx-tool` (JED → SVF, ATMISP-compatible) | crates.io; docs.rs |
| **`yosys-atf15xx`** | the Yosys plugin (`synth_atf15xx`): `atf15xx-synth` feature `yosys` (Rust + `cxx` bridge to RTLIL, F9.7); CI against pinned Yosys versions | source releases; `yosys -m` |
| **`atf15xx-verify`** | equivalence harness (`tools/sim`: formal + iverilog + Verilator, time-bounded) | PyPI or plain repo |
| **`atf15xx-re`** | reverse-engineering record: oracle harnesses (wine drivers), experiment scripts, specs (`docs/architecture`), work logs, STORY, silicon bench (Uno firmware + scripts) | plain repo; the specs could be published as a book (mdBook) |

Rationale: users need the Rust crates and the plugin; the RE tooling needs wine and vendor
installs and is for maintainers. The specs belong with the RE record but should be linked from the
crates' docs.

## 4. Programming: OpenOCD

Today: `atf15xx-tool`/the flow write ATMISP-compatible SVF, and OpenOCD's `svf` command programs it
(verified on the ATF1508AS, with our JTAG-lockout guard). That needs no OpenOCD change.

Proposed: a **native ATF15xx PLD driver in OpenOCD** (C, GPL), so that `pld load atf15xx.pld
design.jed` programs a JED directly:

* OpenOCD's `pld` subsystem already has `pld device` / `pld load` (0.12: virtex2; later
  development versions added drivers for several FPGA families). An `atf15xx` driver would:
  parse the JEDEC file (`QF`, `F`, `L`, `C` fields; small), map fuses to the ISC rows with the
  compact per-family formula tables of spec §5 (a few hundred lines of C), and run the program /
  verify / erase / secure sequences of spec §6–§7 (IR length 10, the ISC instruction set).
* Extra commands: `atf15xx erase`, `atf15xx verify file.jed`, `atf15xx read_ues`, and a
  **refusal to load a JED whose JTAG-enable fuse is 0** unless forced (the guard of
  `tools/silicon/jtag_guard.py`).
* Benefits over SVF: no intermediate file, readback verify with fuse-level error reports, faster
  (no SVF interpretation), multi-device chains from OpenOCD's own TAP configuration (replacing
  ATMISP chain files).
* Test plan: our Uno bench + FT232H on the ATF1508AS, plus the SVF route as the oracle.

Alternative/complement: a small Rust programmer (`atf15xx-prog`) using `jtag-taps` (maintained,
FTDI/J-Link adapters) and the fuse→row code we already have in `atf15xx-tool`, for users who do
not want OpenOCD.

## 5. Rust: idiomatic code, maintained crates

Audit of 2026-11-24 (`.scratch/crate-eval`, `cargo info`, crates.io API):

| area | today | plan |
|---|---|---|
| random numbers | hand-rolled xorshift (2×) | **done**: `rand` + `rand_chacha` (`ChaCha8Rng`: reproducible across versions) |
| CLI, time, JSON | `clap`, `chrono`, `serde_json` | keep |
| JEDEC reading | three readers (`atf15xx-tool`, `atf15xx-fit`, `atf15xx-sim`) | **done** 2026-11-27: one reader, `atf15xx_sim::parse_jed_bytes`, on **`jedec`** 0.1.0, used by fit and sim; identical fuses on 17,318 JEDs (every vendor-fitted and every flow-built one in `.scratch`), regression test `atf15xx-sim/tests/jed_reader.rs` (the old reader as reference, every committed JED; stricter: STX/ETX, the file checksum and a right `C` are required, so ATMISP's `C0000` and `X`-digit quirk inputs are refused); a `jedec` bug (a file with only `QF` + `F` fails) is worked around. Planned: use **`jedec`** (rqou/rust-jedec, 0BSD, from the CoolRunner-II tools) for reading in fit/sim: reads 134/134 of our JEDs to identical fuses and verifies checksums (it found a bug in our `patch_jed.py`). Keep `atf15xx-tool`'s reader: it reproduces ATMISP's quirks byte for byte (spec J-rules). Keep the vendor-exact JED *writer* (byte identity). Offer an ATF15xx quirk set upstream if needed |
| fuse arrays | `Vec<bool>` in several crates | **not adopted** (2026-11-27): the arrays are at most 74,136 fuses, read and written one fuse at a time everywhere; `bitvec` would save 65 kB per fit and cost generic indexing throughout. Converted at the `jedec` boundary |
| SVF | written by `atf15xx-tool`; Python regex in `jtag_guard.py` | **done** 2026-11-27: every SVF the 145 ATMISP vectors generate must parse with **`svf`** 0.3.0 (`tests/vectors.rs`; the 4 with ATMISP's backtick defect are recorded in their `case.json`); the guard is ported (`atf15xx-tool jtag-guard`, `atf15xx_tool::guard`, on `svf`), cross-checked with `jtag_guard.py` on 132 SVFs and 95 JEDs, used by `program.sh`, and `program` checks its own SVF against the JED. Planned: validate every SVF we write with the **`svf`** parser in tests (parses 32/32 of ours; would reject ATMISP's backtick defect); port the guard to Rust on top of it |
| errors | 27 library functions return `Result<_, String>`; `atf15xx-sim` uses `anyhow` | partly **done** 2026-11-27: the hand-written error types (`FitError`, `tt2::ParseError`, `atf15xx_tool::isp::Error`) derive **`thiserror`** 2 (messages pinned by tests); the String results stay until a caller needs to tell failures apart. Planned: **`thiserror`** enums in library crates (`FitError`, flow, arch), `anyhow` only in binaries |
| JSON documents | `serde_json::Value` walking (Yosys netlist, report) | `#[derive(Deserialize)]` structs for the Yosys JSON schema (**done** 2026-11-26 for the netlist: `atf15xx-synth/src/ynet.rs`, F9.7 item 1) |
| graph algorithms | hand-written bipartite matching (UIM) | **keep**: it must reproduce the vendor's preference order (lowest candidate mux first) for byte identity; `petgraph`'s matching would not. Document why |
| SOP minimisation | own (`logic.rs`: unate recursive complement, irredundant cover) | keep for now (no maintained Rust espresso; ABC's `cover` is the reference); consider binding ABC through the Yosys plugin instead |
| JTAG | OpenOCD | optional Rust path via `jtag-taps` (§4) |

General: `clippy -D warnings` in CI, `rustfmt`, docs on every public item, `#![forbid(unsafe_code)]`
everywhere except the cxx bridge of `atf15xx-synth`'s plugin.
