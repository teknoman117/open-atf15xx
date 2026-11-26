# 08 — In-circuit debugger (`atf15xx-tool`)

## Status

**2026-11-25: first version done.** `atf15xx-tool` reads a running ATF15xx over JTAG boundary scan
through OpenOCD (IDCODE, chain, SAMPLE snapshots by pin / macrocell / Verilog name, watch, VCD
capture) and drives pins with EXTEST behind safety guards. Verified on the bench ATF1508AS-PLCC84.
Spec: [D1](../architecture/debug/01-boundary-scan.md). Next: see Open issues.

## Scope and ownership

`atf15xx-tool` (library + binary), `docs/architecture/debug/`, this log,
`tools/silicon/bench/exp_dbg_vcnt40.py`, `exp_dbg_seq_vcnt40.py`; `src/session.rs`, `tests/session.rs`. Reads (never copies) `external/prjbureau/bsdl/*.BSD`.
Builds on `tools/silicon/bscan*.py` (kept; the Rust tool covers their function).

## Results

| what | result | reproduce |
|---|---|---|
| BSDL parsing | all 24 Atmel files parse with the `bsdl` crate (cells, ports, opcodes, IDCODE) | `cargo test -p atf15xx-tool --test parts` |
| cell maps vs arch.json | all 12 BSDL/device pairs (1502 ×2, 1504 ×6, 1508 ×4 packages) build with every pin agreeing | `atf15xx-tool --part ATF1504AS-PLCC68 info` etc.; test `every_part_builds_and_agrees_with_arch` |
| PLCC84 map vs silicon-verified map | identical cell by cell (352 cells: names, kinds, pins) and opcodes | test `bench_map_matches` |
| IDCODE / length on the bench | `IDCODE 0x0150803f (ATF1508AS-PLCC84), boundary register 352 cells` | `atf15xx-tool idcode` |
| chain | `auto0.tap: IDCODE 0x0150803f  1508AS_A100.BSD, 1508AS_J84.BSD, 1508AS_Q100.BSD, 1508AS_Q160.BSD` | `atf15xx-tool probe` |
| SAMPLE vs `bscan.py` | 351/352 cells equal; the one difference is GCLK1, which toggles (a free-running clock on pin 83: 238 changes in 500 captures) | inline (journal) |
| named readout | vcnt40 programmed: 8/8 counter values after random GCLK2 bursts read correctly by name (all 40 bits, across the 40-bit wrap), inputs en/ld/clk correct | `python3 tools/silicon/bench/exp_dbg_vcnt40.py` |
| EXTEST | MC16/MC14 (pins 4/5, design outputs) driven 1/1 and 0/1, pads read back the driven values; afterwards SAMPLE shows the design's values again | `atf15xx-tool drive --yes-drive-pins MC16=1 5=1` |
| guards | refused: without `--yes-drive-pins`; an input pin of the design (pin 81) without `--allow-inputs`; a JTAG pin (71); an unbonded macrocell (MC1); `--speed` above TAP_SCAN_CLOCK | inline |
| capture rate | 821/s at 400 kHz, 3116/s at 4 MHz (5000 captures each); captures at 400 kHz and 4 MHz identical | `atf15xx-tool --speed 4000 capture --count 5000 -o x.vcd` |
| `seq` session (EXTEST held) | vcnt40: `ld`/`en` driven over JTAG, GCLK2 from the Uno: load, 8 random bursts across the 40-bit wrap and a 3-edge hold all read back right (all 40 registers), design outputs P4/P5 follow at the Uno after every step: 18/18 | `python3 tools/silicon/bench/exp_dbg_seq_vcnt40.py` |
| tests | 9 passed (3 unit, 2 parts, 3 mock-TAP, 1 session) | `cargo test -p atf15xx-tool` |

## Evidence levels

* Cell semantics (input / oe / out, out = output-mux value): **silicon** (F2.10, exp_bscan.py) on
  the 1508 PLCC84; for the other parts **external** (Atmel BSDL), not tested on silicon.
* Pinless macrocells' cells: **external** (BSDL comments) and, for the 1508 PLCC84, **silicon**
  (the counter's buried registers read correctly).
* ATF1508AS-PQFP160 pin 110: BSDL says I/O (MC109), arch.json input-only (**oracle**, vendor
  fitter): conflicting, open.
* Bench TCK limit (corrupted IDCODE at 10 MHz, clean at 6 MHz): **silicon**, this wiring only.

## Decisions

* **2026-11-25 — OpenOCD as a server process only, never linked (user decision).** OpenOCD is
  driven through its Tcl RPC port (started by the tool, or a running one with `--connect`).
  Linking it as a library is not done: it is not a supported use (no stable API; its sources call
  it "not mainstream yet"), and OpenOCD is GPL-2.0, which would make our ISC binary GPL; a
  separate process keeps the licences apart. A native transport, if ever wanted, goes behind the
  `Jtag` trait (e.g. FTDI MPSSE directly), not through libopenocd.
* **2026-11-25 — `bsdl` crate + a comment scanner.** The crate (MIT, codeberg dlharmon/bsdl)
  parses all 24 files; macrocell numbers of pinless cells exist only in comments, read by our
  scanner. Where a cell has a formal pin, the pin map decides (comments have typos). Rejected:
  our own full BSDL parser (the crate already covers the formal part correctly).
* **2026-11-25 — OpenOCD Tcl RPC** behind a `Jtag` trait (mock for tests; other backends can be
  added): no OpenOCD library dependency, works with a running OpenOCD or one we start.
* **2026-11-25 — `vcd` crate** for captures (maintained, widely used).
* **2026-11-25 — JTAG pins keep their package pin** in the map for display (the bench map did the
  same), and are never driven.

## Journal

- **2026-11-25**: `atf15xx-tool program X.jed` (user request: program the hardware with
  atf15xx-tool): Program/Verify SVF from the JED (`--strict`), IDCODE check, OpenOCD `svf` through
  the Tcl connection, failure on a nonzero `catch` code. JTAG-disabling images get a warning and
  are programmed (user: "sometimes you need to consume the IOs"); the bench script `program.sh`
  keeps refusing them. Bench: SPI example programmed + verified in 13.1 s at 4 MHz, SVF identical to
  `atf15xx-tool svf --strict`, self-test 16/16 after; a wrong-TDO SVF is reported as a failure.
  `examples/spi/host/jtag_spi.py --program` uses it.
  `atf15xx-tool erase` (user request): the Erase instruction's SVF (erase + blank check); bench
  5.8 s, SVF identical to `svf --strict -d ATF1508AS:erase`, the SPI image's verify fails after it.
  OpenOCD pass-through (user request: choose the JTAG hardware): `--openocd-file FILE` (`-f`),
  `--openocd-cmd CMD` (`-c`, before init), `--openocd PATH`, empty `--interface` allowed; checked
  on the bench (spec D1.5).
- **2026-11-25**: `seq` commands `release` (BYPASS: the pins back to the design; the next
  EXTEST command takes them again, preloaded with the design's values) and `sample SIG...`
  (non-intrusive capture), `scan N`; `Scanner::sample` now marks EXTEST as left. Bus names also
  match `D0`, `D1`, ... (the flow's names for inout bus bits). Used by `examples/spi/host`.
- **2026-11-25**: The part model is generated at build time by `atf15xx-arch` (`build/bsdl.rs`, the
  former `dbg/part.rs` parser; `atf15xx_arch::bsdl`); the debugger no longer reads BSDL files or
  takes `--bsdl-dir`. The generated ATF1508AS-PLCC84 map equals the bench-verified map in all 352
  cells; test `bench_facts` keeps the facts measured on the chip; `bscan.py` loads its map from
  atf15xx-arch through `tools/arch/archdata.py`.
* **2026-11-25** — Merged into the crate `atf15xx-tool` (user request): the ATMISP-compatible
  conversions are its module `isp` and the commands `svf`, `chain`, `jed` of the binary
  `atf15xx-tool` (same options and exit codes as `atmisp`); the in-circuit debugger is its module
  `dbg` (feature `jtag`, default) with the commands `part` (was `info`), `idcode`, `probe` (was
  `chain`, which now means the chain files), `sample`, `watch`, `capture`, `drive`, `seq`. The flow
  depends on the crate without default features (no BSDL/VCD dependencies). Tests unchanged in
  substance: 139 ATMISP vectors, the BSDL/bench-map and mock-TAP tests all pass.

* **2026-11-25** (main session) — `seq`: scripted EXTEST sessions (`session.rs`, spec D1.7) for
  stimulating a design in circuit, e.g. an 8080/Z80-style bus: drive/float/scan/print/expect/wait,
  buses as `d=0x5A`, the design's outputs following it one scan late. Verified on silicon (D1.8,
  18/18): during EXTEST the 1508's logic keeps running and sees the driven pins (observe-only
  `BC_4` input cells), which had been an assumption. Mock test with a TAP whose pads follow the
  driven pins (`tests/session.rs`).

* **2026-11-25** — Built the crate (main session's fork). Found and fixed on the way: annotation
  styles of the 1504 files (`Scan Cell 65, MC64`) and `I/O69, MC60`; macrocells without input cells
  (JTAG pins; 36 on the 1508); one comment typo in `1508AS_Q160` (cells 246/247 `MC59`, pin 50 =
  MC60), made a note; pin 110 on the PQFP160 (above). Throughput: first 12 captures/s; caching the
  loaded IR doubled it; the rest was Nagle (command and terminator in two writes: one 40 ms delayed
  ACK per command) — one write + TCP_NODELAY gave 821/s. A long capture then hung: OpenOCD's stderr
  was a pipe nobody read; it now logs to a file.

## Open issues

* ATF1508AS-PQFP160 pin 110: silicon test needed (a part in that package).
* Other parts than the 1508 PLCC84 not tested on silicon.
* `watch`/`capture` sample at the adapter's pace, unsynchronised to the design clock (aliasing:
  GCLK1 on the bench toggles faster than 3 kHz); a triggered capture (wait for a cell value) and
  a JED-decoded view without a map (from `atf15xx-arch::decode`) would be next.
* `chain` with several devices: only one device on the bench; untested with more.

## Artifacts

`atf15xx-tool/` (src: `part.rs`, `jtag.rs`, `design.rs`, `snapshot.rs`, `main.rs`; tests:
`parts.rs`, `mock.rs`), `docs/architecture/debug/01-boundary-scan.md`,
`tools/silicon/bench/exp_dbg_vcnt40.py`.
