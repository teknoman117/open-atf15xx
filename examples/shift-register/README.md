# 64-bit shift register (breakout-atf1508as example)

The example design of the user's ATF1508AS-PLCC84 breakout board, ported from CUPL to Verilog.

**Origin:** <https://github.com/teknoman117/breakout-atf1508as>, directory `Example/` at commit
`b473233` ("add demo"): `shift-register.pld` (CUPL, MPL-2.0 there), `shift-register.ino` (the Arduino
sketch), `build-hdl.bash` (WinCUPL build). The board itself (KiCad, CC-BY-SA 4.0) is described in
that repository's README; this repository keeps only the port and the sketch.

```
shift_register.v      the design (Verilog port of shift-register.pld, ISC like the project)
shift_register.acf    the original build's fitter options
uno/shift-register.ino  the Arduino Uno sketch that drives it (unchanged from the board repository)
```

## What it does

A 64-bit shift register clocked by SCK with data on IN, and a 64-bit output register loaded from
it on LATCH; both clear while `rst_n` (pin 1) is low. Each of the 64 outputs pulls low while its
register bit is 1 and is released otherwise (open collector): the CUPL design's
`[OUT63..0] = ![REGISTER63..0]` with every output in the `open_collector` list. 128 flip-flops on
128 macrocells.

| Arduino Uno | CPLD | signal |
|---|---|---|
| 2 | 83 (GCLK1) | LATCH |
| RST | 1 (GCLR) | rst_n |
| 11 (MOSI) | 84 (OE1) | IN |
| 13 (SCK) | 2 (GCLK2) | SCK |

The sketch sends 64 bits LSB first over hardware SPI, then pulses LATCH, every 20 ms; the first bit
sent ends up on OUT[63] (pin 4). The board's oscillator must be removed (it drives pin 83).

## Build and check

```
docker/yosys/run.sh cargo run --release -p atf15xx-synth -- build examples/shift-register/shift_register.v \
    -c examples/shift-register/shift_register.acf -o .scratch/examples/shift-register
tools/sim/run.sh python3 tools/sim/equiv.py .scratch/examples/shift-register/shift_register.jed \
    --device P1508C84 --verilog examples/shift-register/shift_register.v --top shift_register \
    --fit .scratch/examples/shift-register/shift_register.fit
```

Results (2026-11-25): 128/128 macrocells, fmax 100 MHz at -10; PROVEN equal to the Verilog. Also
PROVEN equal to the original CUPL design (its `.tt2`, every output open collector, all 64 pins
compared). The vendor fitter's JED of the same design differs on OUT44–OUT63: the vendor truncates
the `open_collector` list after 43 entries and leaves those outputs push-pull
(docs/architecture/fitter/06-conformance.md); on the other 44 pins ours is PROVEN equal to it.

**JTAG is off in this image** (as in the original build): four outputs are on the JTAG pins (14
TDI, 23 TMS, 62 TCK, 71 TDO). Once it is programmed, reprogramming needs 12 V on OE1 (the breakout's
JP2 in position 2-3, see its README). `atf15xx-tool program` warns before programming it; the
bench script `tools/silicon/program.sh` refuses it. It has not been programmed on the bench.
