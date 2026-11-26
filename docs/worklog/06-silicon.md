# 06 — Silicon: programming real devices

## Status

**2026-11-24 (bench).** With the user's Arduino Uno bench (wiring below) every check is automatic.
Settled on silicon: reset beats preset (PT and GCLR), registers are 0 after programming regardless of
`Invert`, GCLR polarity, foldback polarity, T flip-flops / carry chains / cascades, OE1 as an input,
the open Verilog flow (`vcount`, `vcnt20`, `vblink_slow`: exact). Found: floating dedicated inputs
cause intermittent errors; our Verilog flow's 4-level counter (`vblink`) fails at 66.6 MHz while the
1-/2-level CUPL/vendor versions work (timing). Next: depth-aware synthesis (single-level T
counters), Yosys integration (user request).

**2026-11-24.** First real device programmed with the open toolchain: the user's ATF1508AS-PLCC84
on the `breakout-atf1508as` board, via FT232H + OpenOCD 0.12. Image: the user's CUPL `blink`
(`~/Projects/pld/cpld/atf1508as-blink`, `.tt2` from WinCUPL) fitted by **our** `fit1508`, SVF by
`atf15xx-tool`. Programming and an independent readback verify passed; JTAG still answers after
programming. **Pending: the user's visual confirmation that the LED on pin 4 blinks, and an IDCODE
scan after a power cycle.**

## Scope and ownership

Owns `tools/silicon/` and this log. Uses `fit1508`, `atf15xx-tool`, `tools/sim/equiv.py`.

## Bench

Arduino Uno R3 (`tools/silicon/bench/`: firmware `bench.c`, `make flash`; host `bench.py`). Wiring
(user, 2026-11-24): D2–P4, D3–P5, D4–P81 (GCLK3), D5–P80 through 1 kΩ; D8–P1 (GCLR); D9–P2
(GCLK2/OE2; Timer1 OC1A: `f <hz>` free-running clock up to 8 MHz, `b <n>` exact 1 MHz bursts);
A5–P84 (OE1). 66.6 MHz oscillator on P83 (GCLK1). The part is an ATF1508AS-10JU84 (speed
grade -10, per the user): the timing analyser's default grade. Edge counting uses interrupts: keep counted
signals ≤ ~20 kHz (user note).

## Results

| check | result | reproduce |
|---|---|---|
| chain scan before programming | IDCODE `0x0150803F` (ATF1508AS), IR length 10 accepted | `openocd … -c scan_chain` (tools/silicon/README.md) |
| JTAG-enable fuse identified | special bit 1 = JED fuse 74115 (1508); `1` = JTAG on | vendor fit `-strategy JTAG on/off` diff (below) |
| our JED vs the user's `.tt2` | BOUNDED 40 + SIMULATED 33.5 M cycles (≈ 33 wraps of the 10⁶ counter), PASS; also PASS vs the vendor JED | `equiv.py blink.jed --device P1508C84 --tt2 blink.tt2 --fit blink.fit [--sim-engine verilator]` |
| guard | blink: JED 1111 / SVF `f f` → safe; negative controls (user `shift-register.jed`, single-bit-flipped SVF, `spi.jed`) refused | `tools/silicon/jtag_guard.py` |
| programming | `svf file programmed successfully for 3569 commands with 0 errors` (13 s, 400 kHz; includes the SVF's own verify) | `tools/silicon/program.sh blink.svf blink.jed` |
| readback verify (separate verify-only SVF) | 0 errors; after programming the IDCODE still reads `0x0150803F` | `atf15xx-tool svf -d ATF1508AS:verify:blink.jed` + OpenOCD |
| blink (our fitter) measured | P4 driven, 33.5 Hz (66.6 MHz / 2·10⁶), held low in reset, recovers; same as the vendor's JED | `bench/probe_blink.py` |
| state after programming | A (`Invert`=0) and B (`Invert`=1) both 0, twice; set to 1 then reprogrammed → 0 | `bench/exp_state_prio.py` |
| reset vs preset | both asserted → 0 for PT/PT and GCLR/PT, either order; GCLR pin high clears | same |
| Verilog flow `vcount` (4-bit, bench clock on GCLK3, enable, async reset) | 500 random steps, 0 mismatches | `bench/exp_verilog.py` |
| Verilog flow `vcnt20` (20-bit on GCLK2, OE1 as select input) | 48 exact bursts (incl. 2¹⁶ ± 1, 10⁶), 0 wrong | inline script (journal) |
| Verilog flow `vblink_slow` (vblink logic on a bench clock), with and without foldback | toggles exactly at clock 1,000,000, 4/4 each | inline script (journal) |
| Verilog flow `vblink` at 66.6 MHz | **fails** (no toggling); 4 array passes per register path vs 1 (our CUPL fit) and 2 (vendor fit) | `.scratch/silicon/depth.py` |
| `vblink` after the toggle-analysis fix (`opt.rs` budget 64 → 1024) + `--max-products 15` | **33.40 Hz at 66.6 MHz**, reset OK: 1 array pass per register (3-stage cascades, silicon-verified) | `.scratch/silicon/vblink_b1024_mp15` |
| OE1 as global output enable (`designs/voe1.v`, Verilog flow) | PASS: driven while OE1 = 1, Z while 0 (4 passes of the table); bus keeper observed | `bench/exp_oe1.py` |
| GCLK network source mux + invert (`designs/vpol.v`, `patch_jed.py`) | GCLK3 network re-sourced from the GCLK2 pin follows its rising edges; inverted, its falling edges (8 edges each, OK) | `bench/exp_gclk_polarity.py` |
| dual-edge on one pin with network sharing (`designs/vedges.v`, flow default) | rise → a, fall → b, ×8, twice; no start-up edge (b = 0 after programming) | inline (journal) |
| cascade lender XOR (`lender`, `lender2` + `patch_jed.py`) | A = 1 with `CasMux` = 1, also with a sum PT: model default changed in both simulators | inline (journal) |
| vendor `blink.v` fit polarity | vendor JED: leds0 = 0 right after start (source 1) ×3; ours: 1 ×3; both 2.00 Hz → vendor defect 3 confirmed | inline (journal) |
| two-level foldback OE (`designs/fold4.tt2`), common-cube De Morgan (`fold3.tt2`) | 64/64 each | `bench/truth.py`, inline |
| input register packing (`designs/vinreg.v`) | 0/200 random steps wrong | inline (journal) |
| 40-bit carry chain (`vcnt40.v`, R4: 39 T flip-flops + 1 carry node), counted across the 40-bit wrap | 36/36, all registers read over JTAG | `bench/exp_vcnt40.py` |
| timing-driven fit (`vblink.v --fmax 66.6`: 66.7 MHz predicted) at 66.6 MHz; the 38.5 / 32.3 MHz fits fail | blink 33.5 Hz, reset OK | `bench/probe_blink.py` |
| XOR synthesis from Verilog (`designs/vxor.v`: 4-input parity, registered inverted 3-input parity) | 16/16 | `bench/exp_vxor.py` |
| XOR inputs: every (`Pt2Mux`, `CasMux`, `Invert`) on MC16 (`xorcas.tt2` patched by `mcbits.py`) | A = CasMux ? P : sum, B = Invert ∧ (CasMux ? 1 : P); old model wrong for `Pt2Mux` = 1, `Invert` = 0 | `bench/exp_xor_cascade.py` |
| P2+ input register + cascade helper in one MC (`inreg_cascade.tt2`) | 32/32 (pins and scan cells) | `bench/exp_inreg_cascade.py` |
| boundary scan map (`bscan_map.py`, `bench/exp_bscan.py`) | BSR 352 cells (= the PQFP100 BSDL); every bench input, a fast input register, comb outputs and EXTEST writes of P4/P5 correct | `bench/exp_bscan.py` |
| registers over JTAG (`vcount` with `observable_registers`) | all 4 counter bits (2 buried) read correctly after 12 random clock bursts | inline (journal) |
| foldback, exhaustive | 64/64 (5 inputs, two orders) | `bench/truth.py fold.svf fold.jed …` |
| floating dedicated inputs | P2 + P84 floating: 1/5 runs miss the 10⁶ toggle; driven: 5/5 | inline script (journal) |
| verify negative control | verify against the vendor's fit of the same design (3,289 fuses differ) **fails**, so the verify really compares | same with the vendor JED |

## Evidence levels

* JTAG fuse 74115 [O]: vendor `fit1508 -strategy JTAG on` vs `off` on one design (pins locked,
  TQFP100) differ in fuse 74115 plus 24 fuses in the macrocell-config region (the four JTAG-pin
  macrocells' pad configuration); all 10 vendor JEDs of the user's designs have fuse 74115 (resp.
  34167 on the 1504) equal to their `-jtag` option.
* SVF bit [O, atmisp = ATMISP byte-identical]: flipping only fuse 74115 changes only the
  special-register writes (`SIR 10 TDI (292)`, `SDR 4 TDI (f)` → `(b)`), i.e. SVF bit 2, at the
  program step (line 1900) and the verify step (line 4013).
* Erased fuses are 1, so erase leaves JTAG on and programming (which writes zeros) cannot clear it
  unless the JED says so [E: blank devices ship with JTAG enabled; consistent with the above].
* 1502/1504 SVF bit position: assumed identical (same 4 special bits), not checked on silicon [I].
* The design's function on silicon: not yet observed (needs the user; or boundary-scan SAMPLE with
  the ATF1508AS BSDL, which is not in the reference directory).

## Decisions

* 2026-11-24: First silicon image = the user's own blink for this board (pins chosen by the user;
  LED pin 4, CLK pin 83 = on-board oscillator, RST pin 1), fitted by our fitter, rather than a new
  design whose outputs could contend with whatever is wired to the breakout.
* 2026-11-24: Every programming run goes through `jtag_guard.py` (refuses JTAG-disabling images).
* 2026-11-24: OpenOCD `-irlen 10` (ATMISP's SVF uses 10-bit SIR; the original notes' `-irlen 3` also
  works because OpenOCD plays the SVF's own lengths).

## Journal

- **2026-11-25**: SPI example on the bench (`examples/spi`, ATF1508AS-PLCC84, 66.6 MHz): the JTAG
  session drives the bus, the Uno is the SPI slave (the example's own firmware,
  `examples/spi/uno/spi_slave.c`: `x`/`y`, bit-banged on INT0; the bench firmware is unchanged). Findings: (1) SPI *through* EXTEST (pins following the design once per scan) fails at
  1 kHz: scans take 0.28-0.39 ms with jitter, more than an SCK level can tolerate; fixed by
  handing the pins back to the design during the transfer (`seq release`), the bus keepers
  holding the bus. (2) About one bus write in fifty latched a wrong data bit, at any TCK
  (400 kHz-4 MHz): the original design latches D on the first CLK after /WR, inside the
  strobe's switching transient (cs_n and wr_n switching in one scan made it worse, 8/120 vs
  2/120). Fixed by a 2-flop strobe synchroniser (`STROBE_SYNC`): 0 of 400 writes wrong. Not a
  keeper problem (the failing pins were driven). (3) TCK 10 MHz corrupts the IDCODE on this
  wiring; 4 MHz is the practical maximum. Result: 64/64 bytes both ways at 1.02 kHz and 254 Hz.
* **2026-11-25** — `bscan_map.py` reads the Atmel BSDL from the prjbureau submodule
  (`external/prjbureau/bsdl/`, Atmel's copyright, read only). The PLCC84 file (`1508AS_J84.BSD`)
  gives exactly the same 352-cell map as the PQFP100 file the silicon-verified map was built from
  (cells, kinds, opcodes identical), and the 60 pin–macrocell pairs it states agree with `arch.json`
  (the other 4 PLCC84 I/O pins are the JTAG pins, listed as internal cells). Reproduce:
  `python3 tools/silicon/bscan_map.py external/prjbureau/bsdl/1508AS_J84.BSD P1508C84 ATF1508AS-PLCC84`.

* **2026-11-24 (bench)** — Flashed the Uno with `bench.c` (all lines start as inputs). First read: P4
  "toggled" at exactly 60 Hz — mains hum on undriven lines (the pull-up test showed all lines
  floating). The vendor JED then drove P4 at 33 Hz and the vendor run's first IDCODE read
  0xffffffff, i.e. the chip had not been responding when our image was measured (bench wiring in
  progress): reprogrammed ours → 33.5 Hz, identical to the vendor. **Not a fitter bug** (first
  suspicion retracted). Then: state/priority experiments (hand `.tt2`, our fitter); Verilog-flow
  designs. `vblink` (66.6 MHz) did not toggle; the same logic on a bench clock toggled 21–1020
  clocks late, non-deterministically. Ruled out the bench clock (vcount: 36 random bursts exact)
  and the counter (vcnt20: exact); the remaining difference turned out to be the floating P2/P84
  (driving them fixed it; floating them again reintroduced misses). With inputs driven,
  `vblink_slow` is exact with or without foldback, and `vblink` at 66.6 MHz still fails: logic depth
  4 array passes (timing data F8: f_max ≈ 25–43 MHz). Forced-foldback truth table 64/64.
  Dead end: suspecting the foldback host (MC46 = pin 24, floating pad) — the no-foldback build
  behaved the same once inputs were driven.

* **2026-11-24** — Read the user's notes (breakout-atf1508as README: JP2 2-3 puts 12 V on OE1 for
  recovery; FT232H wiring and OpenOCD commands in P80C550-SPM-CPLD). Noticed the board's example
  (`Example/build-hdl.bash`) fits with `-strategy jtag off` — programming its JED would lock JTAG.
  Scanned the chain read-only, traced the JTAG fuse (vendor diff, all user JEDs, SVF bit), wrote the
  guard with negative controls, fitted `blink` with our fitter, checked equivalence, programmed,
  verified, and ran a verify negative control.

## Open issues

* Simulation harness start-up artefacts with negative/inverted clocks: iverilog's testbench
  (`reg x = 1'b0` → x→0 counts as a negedge in the source model) and Verilator (an inverted network
  starts at 1 → counted as a posedge in the JED model) both report a step-0 FAIL for `vedges`;
  formal is PROVEN and silicon agrees with formal. Fix: define both models' state at time 0 without
  edges (tools/sim).

* Cold power-up state (needs power cycling the board); slow vs fast slew; f_max measurement (the
  bench can clock GCLK2 up to 8 MHz only; the 66.6 MHz oscillator is the only fast clock).
* Depth: fixed for vblink by a larger toggle-analysis budget (every counter bit but one becomes a 1–2
  product T flip-flop) plus longer cascades (`--max-products 15`). Still to do: XOR synthesis
  (PT2 ⊕ sum; would make c6 two PTs instead of a 3-cell cascade), timing-driven choice of the
  cascade length, and a timing check (F8.5). See docs/architecture/fitter/09-yosys-integration.md.
* Floating dedicated inputs: warn in the flow when `pin_keep off` leaves GCLK/OE/GCLR pins unused.

## Artifacts

* `tools/silicon/{jtag_guard.py,program.sh,README.md}`, `tools/silicon/bench/` (Uno firmware + host
  library + experiment scripts), `tools/silicon/designs/` (Verilog test designs).
* Scratch: `.scratch/silicon/blink-ours/` (JED, SVF, verify SVF, OpenOCD log), `.scratch/jtag/`
  (JTAG on/off experiment). The user's board repositories are not kept: their designs are
  in `examples/shift-register` (breakout-atf1508as `Example/`) and `examples/spm`
  (P80C550-SPM-CPLD), whose READMEs give the source and commit.
