# D1. In-circuit debugging over boundary scan (`atf15xx-tool`)

Status: implemented (`atf15xx-tool`), verified on the bench ATF1508AS-PLCC84 (2026-11-25;
work log [08-debugger.md](../../worklog/08-debugger.md)). Builds on the silicon facts of
[F2.10](../fitter/02-fuse-map.md#f210-jtag-boundary-scan-silicon-atf1508as-plcc84-2026-11-24).

## D1.1 What boundary scan shows on an ATF15xx

Instructions (all parts, IR length 10): EXTEST `0000000000`, SAMPLE `0001010101`, IDCODE
`0001011001`, BYPASS `1111111111`. There is no INTEST: registers can be observed, not written.

| cell | SAMPLE captures | EXTEST drives |
|---|---|---|
| input (BC_4) | the pad of a pin, or of a dedicated input (OE1, GCLK1, GCLK2, GCLR) | — |
| `oe` (BC_1, control) | the macrocell's output enable | the pad's enable |
| `out` (BC_1, output3) | the macrocell's output-mux value: register Q when the output taps the register, else the XOR output | the pad's data |

Every macrocell has an `oe`/`out` pair, **including macrocells without a pin** (two `internal`
cells), so SAMPLE reads every register whose output mux taps it, while the device keeps running.
The flow fits buried registers that way by default (`observable_registers`, F2.10).

Per part (all 24 Atmel BSDL files of the prjbureau submodule; [external], read at build time by
`atf15xx-arch` and checked there against the device tables; `atf15xx-tool part` prints the result):

| family | boundary length | input cells | macrocells without an input cell |
|---|---|---|---|
| ATF1502AS | 96 | 32 (4 dedicated) | 4 (the JTAG pins' macrocells) |
| ATF1504AS | 192 | 64 (4 dedicated) | 4 (JTAG) |
| ATF1508AS | 352 | 96 (4 dedicated) | 36 (no pad on the die, and JTAG) |

The register is the same for every package of a family; a package that does not bond a pad has
an `internal` input cell there. Cell 0 is nearest TDO (shifted out first).

## D1.2 Building the cell map

Done at build time by `atf15xx-arch/build/bsdl.rs` (the generated parts are `atf15xx_arch::bsdl`;
a part that fails the checks below is reported by `atf15xx-tool part`, not compiled in).

1. The formal BSDL (cells, ports, pin map, opcodes, IDCODE, control-cell links, TAP_SCAN_CLOCK)
   is parsed with the [`bsdl`](https://codeberg.org/dlharmon/bsdl) crate (MIT; parses all 24 files).
2. Pin cells: port → package pin → macrocell from the device table; a control cell takes the pin of the
   output that names it as control cell; dedicated inputs from the device's dedicated pins.
3. Pinless cells: the macrocell only appears in the comment before the cell. Any comment in the
   register that names `MC<n>` counts (`Input, pin81 MC128`, `Internal, MC124`, `I/O69, MC60`,
   `Scan Cell 65, MC64`, `MC112 TDO`). An `internal` BC_1 pair is (`oe`, `out`) in that order.
4. Macrocells whose pad is a JTAG pin: their cells have no port (the TAP owns the pad); the
   package pin is attached for display and they are never driven.
5. Checks (a failure is an error): every macrocell has exactly one `oe` and one `out` and at most
   one input cell; every pin agrees with the device table; the register length equals BOUNDARY_LENGTH.
   Where a cell has a formal pin, the pin map decides and a disagreeing comment is a **note**
   (one case in the 24 files: `1508AS_Q160` cells 246/247 are commented `MC59` but sit on pin 50 = MC60).
6. A pin the BSDL gives an output cell but the device table has input-only is a note too:
   **ATF1508AS-PQFP160 pin 110 (MC109)**: the BSDL has a full I/O there; the vendor fitter only
   accepted the pin as an input ([O], `exp/pinmap`). Which one silicon does is open.

The ATF1508AS-PLCC84 map built this way equals the map verified with SAMPLE/EXTEST on the bench
chip cell by cell (names, kinds, pins; checked when the committed bench map was retired,
2026-11-25; test `bench_facts` keeps the facts the bench established directly).

## D1.3 Operations

* **IDCODE** is checked against the BSDL's (its `X` bits expanded: 0x0150803f or 0x0150903f for the
  ATF1508AS) before anything else; a wrong part, a missing device or a failing JTAG clock stops the
  tool (the bench chip returns a corrupted IDCODE at 10 MHz TCK, a clean one at 6 MHz).
* **Boundary length** is measured, not trusted: after SAMPLE, a run of ones shifted in after a
  pad of zeros reappears after *L* bits; two pads (40, 43) guard against captured ones. Must equal
  the BSDL's.
* **SAMPLE** loads the instruction once; each further capture is one DR scan.
* **EXTEST** (`drive`): SAMPLE first, so every cell starts from the pins' present state; the cells
  of the driven pins get `oe` = 1 and the value; SAMPLE/PRELOAD of that vector, EXTEST, a second
  scan after the hold time captures the pads (read-back); BYPASS gives the pins back. During EXTEST
  the logic is disconnected from **every** pin; the other pins keep the captured state.
* **Speed**: refused above the BSDL's TAP_SCAN_CLOCK (10 MHz).
* **Program** (`program X.jed`): the SVF of the Program/Verify instruction for the part's device
  (spec §6–§7, with `--strict`), after the IDCODE check, played by OpenOCD's `svf` command through
  the same Tcl connection (`catch {svf -quiet {FILE}}`: a nonzero code, e.g. a TDO mismatch in the
  verify scans, fails the command). An image whose JTAG-enable fuse is 0 (spec §4.4) gets a
  warning and is programmed. Bench (2026-11-25, ATF1508AS, FT232H at 4 MHz): the SPI example
  erased, programmed and verified in 13.1 s; a deliberately wrong TDO expectation is reported as a
  failure.
* **Erase** (`erase`): the Erase instruction's SVF (erase + blank check), played the same way.
  Bench: 5.8 s at 4 MHz; afterwards the SPI image's verify-only SVF fails, as it must.

## D1.4 Naming with a design (`--map <top>.map.json`)

For each fitted cell: registered and the output taps the register → **Q** (complemented if
`q_is_value` is false); registered but the output taps the logic → **D** (the next state, what the
`out` cell captures); combinatorial → **=** (its value). For each input → its pad (**in**). Names
are the Verilog names (`source`).

## D1.5 Transport (OpenOCD Tcl RPC)

OpenOCD runs as a separate server process and is never linked into the tool (decision of
2026-11-25: linking is unsupported by OpenOCD and would put the binary under its GPL-2.0). Other
transports go behind the `Jtag` trait.

Commands are text terminated by `\x1a`; replies likewise. `irscan <tap> 0x<op>`,
`drscan <tap> <n> 0x<hex>` (the hex value's least significant bit is cell 0), `jtag names`,
`jtag cget <tap> -idcode` (chain listing after auto-probing). Each command must be one TCP write
(with TCP_NODELAY): writing the terminator separately waits for the delayed ACK, about 40 ms per
command. OpenOCD's log goes to a file: a pipe nobody reads fills and stalls OpenOCD. Throughput on
the bench (352-cell captures, FT232H): 821/s at 400 kHz, 3116/s at 4 MHz.

A spawned OpenOCD gets, in this order: `-f <--interface>` (unless empty), `-f` for each
`--openocd-file`, `-c` for each `--openocd-cmd`, then `adapter speed`, `transport select jtag`, the
TAP (`jtag newtap cpld tap -irlen L -expected-id ...` from the BSDL), the Tcl port, and `init`.
`--openocd` names the executable. So any adapter OpenOCD supports works (e.g.
`--interface interface/jlink.cfg`), one of several identical adapters is chosen with
`--openocd-cmd "adapter serial X"`, and board files go in with `--openocd-file`. Bench check
(2026-11-25): an extra `ftdi tdo_sample_edge falling` command and an explicit executable path both
work; the interface given only through `--openocd-file` works; `adapter serial NOSUCHSERIAL` stops
with OpenOCD's own error ("unable to open ftdi device ... serial 'NOSUCHSERIAL'").

## D1.6 Safety rules of `drive`

* `--yes-drive-pins` is required;
* by default only pins the design itself drives (sampled `oe` = 1) can be overridden, so nothing
  external should be driving them; `--allow-inputs` lifts that;
* pins without a macrocell pad, JTAG pins and unbonded macrocells are refused;
* the pins are always given back (BYPASS), also when the tool fails half-way.

## D1.7 Scripted sessions (`seq`): stimulating a design in circuit

`atf15xx-tool seq --yes-drive-pins [--allow-inputs] [--hold-outputs] [--map M] [SCRIPT|-]` keeps the
device in EXTEST for the whole script and answers each command with one line (`-` reads stdin, so
a harness can send one command at a time and read the answer):

| command | effect |
|---|---|
| `drive SIG=V ...` | drive pins (persistent until `float`); `SIG` = design signal, `MCn` or pin number; a bus `SIG=0x5A` drives `SIG[0]`, `SIG[1]`, ... (bit 0 least significant) |
| `float SIG ...` | stop driving: the pin goes high-impedance |
| `scan [N]` | one EXTEST scan, or N back to back (the design's outputs keep following) |
| `print SIG ...` | scan, print values (buses in hex) |
| `expect SIG=V ...` | scan, compare; `MISMATCH` lines are counted, exit status 1 at the end |
| `release` | give the pins back to the design (BYPASS); the next `drive`/`float`/`scan`/`print`/`expect` takes them again, preloaded with the design's own values; undriven inputs keep their last level through the bus keepers (`pin_keep`) |
| `sample SIG ...` | SAMPLE (the design keeps its pins; leaves EXTEST), print values |
| `wait MS` | sleep |

Semantics, verified on silicon (`tools/silicon/bench/exp_dbg_seq_vcnt40.py`, D1.8):

* **the logic keeps running during EXTEST and sees the driven pins**: the input cells are
  observe-only (`BC_4`), so a pin driven by the boundary register reaches the macrocell logic
  through its pad like an external signal; registers keep clocking from the dedicated clock
  inputs;
* **the design's own outputs follow it, one scan late**: in EXTEST the output and output-enable
  cells capture what the logic would drive; each scan's vector takes those cells from the previous
  capture (pins being driven, and floated ones, excepted). `--hold-outputs` freezes them at the
  start instead;
* **what cannot be driven**: the dedicated inputs (GCLK1, GCLK2/OE2, GCLR, OE1) have input-only
  cells; on the ATF1508AS-PLCC84 GCLK3 is the I/O pin 81 (MC128) and can be driven while still
  feeding the global clock network. A bus interface to be tested this way therefore puts its
  strobes (`/WR`, `/RD`) on I/O pins (GCLK3, or product-term clocks), or samples them with a
  clock on a dedicated input;
* **reads**: while EXTEST holds the pins, the design cannot drive the pads; what it would drive
  (data and enable) is in the captured `out`/`oe` cells, which `print` shows by name;
* **other drivers**: `--allow-inputs` is needed to drive a design input, and whatever else is on
  that net must not drive it (a CPU holding its bus released: Z80 `/BUSRQ`–`/BUSAK`, 8080
  HOLD/HLDA); two push-pull drivers on one net fight;
* throughput: one scan of the 352-cell register per step (about 0.3 ms at 4 MHz on the bench), so
  roughly a thousand bus writes per second: functional, not at speed.

## D1.8 Bench verification of `seq`

`vcnt40` (40-bit counter, `clk` = GCLK2 from the Uno, `ld` = pin 81, `en` = pin 80, `top`/`mid` =
pins 4/5) with the Uno's lines on pins 4/5/80/81 set to inputs: the session drives `ld` = 1 for one
GCLK2 edge (load 0xFF_FFFF_FFFA), then `en` = 1 for random bursts across the 40-bit wrap, then
`en` = 0 for three edges (hold); every step reads all 40 registers through the same session and the
Uno reads pins 4/5. Result 2026-11-25: **18/18 checks** (10 counter values including the load and
the hold, 8 pin-follow checks), exit 0, pins given back.

