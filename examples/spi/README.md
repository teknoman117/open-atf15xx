# SPI master for an 8-bit bus

A Verilog port of the user's CUPL design `atf1504as-spi` (`spi.pld`, MPL-2.0 there; the port here is ISC): an SPI master that a
microprocessor drives through an 8-bit bus (data register, control/status register, interrupt).

```
rtl/spi_master.v        the peripheral, parameterised (prescaler width and taps, options below)
atf1504as/spi.v, .acf   ATF1504AS-PLCC44: the original board's pinout and options
atf1508as/spi.v, .acf   ATF1508AS-PLCC84 on the bench (breakout-atf1508as + the Uno)
host/jtag_spi.py        drives the bench variant over JTAG, with the Uno as the SPI slave
host/uno.py             host side of the Uno firmware (pyserial)
uno/spi_slave.c         the Uno's firmware: SPI slave + reset/tie lines (avr-gcc, Makefile)
```

## Registers and timing

| RS | write | read |
|---|---|---|
| 0 | load the byte and start a transfer | the byte received (clears the interrupt flag) |
| 1 | bit 0 slave select, bits 3:2 clock select (PSEL), bit 6 interrupt enable | bit 0, bits 3:2, bit 4 RUNNING, bit 6, bit 7 interrupt flag |

SPI mode 0, LSB first, 8 bits. SCK for PSEL = 0..3:

| variant | CLK | prescaler | PSEL 0 | 1 | 2 | 3 |
|---|---|---|---|---|---|---|
| atf1504as (original) | board clock on pin 2 | 4 bits, taps 2/3/4 | CLK/2 | CLK/8 | CLK/16 | CLK/32 |
| atf1508as (bench) | 66.6 MHz on GCLK1 | 17 bits, taps 5/15/17, registered tick | 16.7 MHz | 1.04 MHz | **1.02 kHz** | 254 Hz |

The bench variant also synchronises the bus strobes to CLK (`STROBE_SYNC`): with a 66.6 MHz clock
and strobes driven over JTAG, the original's direct edge detection latched a wrong data bit in
about one write in fifty (it sampled the bus in the strobe's switching transient).

## Build and check

```
docker/yosys/run.sh cargo run --release -p atf15xx-synth -- build examples/spi/rtl/spi_master.v \
    examples/spi/atf1508as/spi.v --top spi -c examples/spi/atf1508as/spi.acf -o .scratch/examples/spi-atf1508as
tools/sim/run.sh python3 tools/sim/equiv.py .scratch/examples/spi-atf1508as/spi.jed --device P1508C84 \
    --verilog examples/spi/rtl/spi_master.v examples/spi/atf1508as/spi.v --top spi \
    --fit .scratch/examples/spi-atf1508as/spi.fit
```

(the same with `atf1504as`, device `P1504C44`). Results (2026-11-25): ATF1504AS 35/64 macrocells,
PROVEN equal to the original Verilog port (`atf15xx-synth/tests/designs/spi.v`); ATF1508AS 52/128
macrocells, fmax 100 MHz at -10, PROVEN equal to its Verilog.

The ATF1504AS options are the original `compile.bash`'s, including `jtag off`: that image makes the
JTAG pins I/O and must only be programmed where that is intended (`atf15xx-tool program` warns
first; the bench script `tools/silicon/program.sh` refuses it).

## On the bench: JTAG as the CPU, the Uno as the slave

The ATF1508AS variant puts SCK, MOSI, SS and MISO on the pins the Uno is wired to (pins 4, 5, 80,
81 = D2, D3, D5, D4), reset on GCLR (pin 1 = D8), and the bus on pins nothing is connected to. The
host plays the microprocessor over JTAG:

* a bus cycle is driven by EXTEST (`atf15xx-tool seq`): select, then strobe, one pin at a time;
* for the transfer the session gives the pins back to the design (`release`), so the design drives
  SCK / MOSI / SS itself, at its own speed; the bus keepers (`pin_keep`) hold the bus levels;
* SAMPLE (non-intrusive) watches `RUNNING`; then EXTEST ends the write cycle and reads the byte.

(Running the SPI pins *through* EXTEST instead only works while every SCK level spans a scan,
~0.3 ms: not at 1 kHz.)

The Uno runs the example's own firmware (`uno/spi_slave.c`): the SPI slave, mode 0, LSB first,
bit-banged on INT0 (both SCK edges; fine up to a few tens of kHz), plus the lines the example
needs: rst_n on D8, the unused dedicated inputs on D9 and A5 tied low. It answers a chosen first
byte and then echoes each byte it received, so every exchange checks both directions; it also
counts SCK edges per byte (`y` reply), which shows glitches on the clock. Commands (115200 8N1):
`m <pin> <i|0|1>`, `x <hex>` (slave on), `x -` (off), `y` (bytes received + edge counts).

```
make -C examples/spi/uno flash                     # the slave firmware (host avr-gcc + avrdude)
cargo build --release -p atf15xx-tool
python3 examples/spi/host/jtag_spi.py --test --program --psel 2 --count 64
```

Result (2026-11-25, ATF1508AS-10JU84): 64/64 bytes correct in both directions at 1.02 kHz and at
254 Hz; 0 of 400 bus writes wrong (with strobe synchronisation; 1–10 of 80 without).

Afterwards `make -C tools/silicon/bench flash` returns the Uno to the general bench instrument.
