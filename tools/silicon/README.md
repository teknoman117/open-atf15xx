# Programming real ATF15xx devices

Hardware: an FT232H (USB `0403:6014`) wired AD0 = TCK, AD1 = TDI, AD2 = TDO, AD3 = TMS (the
user's notes: <https://github.com/teknoman117/P80C550-SPM-CPLD#flashing-cpld-without-an-expensive-programmer>,
board: <https://github.com/teknoman117/breakout-atf1508as>). OpenOCD 0.12 on the host.

| tool | what it does |
|---|---|
| `jtag_guard.py --jed X.jed --svf X.svf` | refuses (exit 1) any image that would disable JTAG: JED special bit 1 (fuse 16783/34167/74115 for 1502/1504/1508) must be 1, and every special-register write in the SVF (`SIR 10 TDI (292)` + `SDR 4`) must have bit 2 set |
| `program.sh X.svf [X.jed] [idcode]` | runs the guard, then plays the SVF with OpenOCD (`-irlen 10`, 400 kHz, IDCODE default `0x0150803f` = ATF1508AS) |

```
# read-only: is the chip there and is JTAG enabled?
openocd -f interface/ftdi/um232h.cfg -c "adapter speed 400" -c "transport select jtag" \
  -c "jtag newtap cpld tap -irlen 10 -expected-id 0x0150803f" -c init -c scan_chain -c shutdown
# build, check, program, verify
target/release/fit1508 blink.tt2 -device P1508C84 -preassign keep -strategy jtag on ...
target/release/atf15xx-tool svf -d ATF1508AS:pv:blink.jed -o blink.svf
tools/silicon/program.sh blink.svf blink.jed
target/release/atf15xx-tool svf -d ATF1508AS:verify:blink.jed -o blink-verify.svf   # readback only
```

**Never program an image built with `jtag off` on the bench** (the vendor examples often are): the
JTAG pins become I/O and the device can only be recovered with 12 V on OE1 (breakout JP2 position
2-3). `program.sh` refuses such images unless `ALLOW_JTAG_DISABLE=1`. (`atf15xx-tool program`, the
general programming command, only warns: a design may need those pins.)

ATMISP-style SVFs use a 10-bit IR; OpenOCD plays the SVF's own lengths, so the `-irlen 3` in the
original notes also works, but `-irlen 10` matches the device. Log: `docs/worklog/06-silicon.md`.

## Bench (Arduino Uno R3)

`bench/` turns an Uno into a test instrument (`make -C tools/silicon/bench flash`): drive/read/
count/pulse on D2–D5, D8, D9, A5, a Timer1 clock on D9 and exact bursts (examples bring their own
Uno firmware where they need more, e.g. the SPI slave in `examples/spi/uno`). Wiring and results:
`docs/worklog/06-silicon.md`. Experiments: `probe_blink.py`, `exp_state_prio.py`, `exp_verilog.py`,
`truth.py` (exhaustive combinational test). Opening the serial port resets the Uno (all lines float
for ~1.5 s): open the bench and drive the CPLD inputs *before* programming.

**Tie every unused dedicated input** (GCLK2/OE2, OE1, GCLR) or keep `pin_keep on`: floating inputs
made a correct design misbehave (F2.9).

## Boundary scan (reading inputs, outputs and registers over JTAG)

`atf15xx-tool` (spec D1) reads the running device over JTAG: `cargo run -p atf15xx-tool -- sample
--map out/design.map.json` prints inputs, outputs and every register whose macrocell output mux taps
the register (all registers of designs built by the flow: `observable_registers`, the default) by
pin and by Verilog name; `watch`, `capture -o trace.vcd`, `drive` and `seq` do the rest. The cell
maps are generated at build time from the BSDL files (spec D1.2). SAMPLE does not disturb the
device; EXTEST drives the pins from the scan cells until BYPASS. There is no INTEST: registers can be
read, not written. The bench experiments (`bench/exp_vcnt40.py`, `exp_inreg_cascade.py`,
`exp_dbg_seq_vcnt40.py`) call it with `--json`.
