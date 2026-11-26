# 02 — JED → Verilog emitter

Source: `atf15xx-sim/src/verilog.rs`; CLI `atf15xx-sim verilog JED -d DEVICE [-o out.v]
[--module NAME] [--pins-wrapper]`. The output is plain Verilog-2005 that Icarus Verilog,
Verilator and Yosys accept, and it is a direct transcription of the model in 01-model.md.

## V1 Interface ("split" pins)

The core module has, for every modelled pin N (01-model.md §M2):

| port | when | meaning |
|---|---|---|
| `input pN_in` | every pin | the level something outside the device puts on the pin |
| `output pN_oe` | I/O pins | the device drives pin N |
| `output pN_out` | I/O pins | the driven value, masked: `pN_oe & value` |

Masking makes two models with the same pin behaviour produce identical port values, so a
miter can compare the ports directly. Inside, the pad of MC m on pin N is
`pad_m = drv_en ? drv_v : pN_in` (pin feedback sees the driven value while the device drives).
An open-drain output has `drv_en = oe & ~out`, `drv_v = 0`.

`--pins-wrapper` adds `<module>_pins` with real `inout wire pN` (`assign pN = oe ? out : 1'bz`)
and `input wire pN` ports for board-level simulation.

## V2 Names

`gclk1..3`, `gclr`, `goe1..6` (global nets); per MC m: `pt{k}_m` (k = 1…5), `fold_m` (foldback
line hosted by m), `sum_m`, `x_m`, `d_m`, `clk_m`, `ar_m`, `ap_m`, `q_m` (reg), `fb_m`, `pad_m`,
`drv_en_m`, `drv_v_m`. A comment before each MC gives block, local index, pin, storage, clock,
OE and flags. Unused macrocells emit only constant wires.

## V3 Storage in Verilog

* D/T flip-flop: `always @(posedge clk_m or posedge ar_m or posedge ap_m) if (ar_m) q_m <= 0;
  else if (ap_m) q_m <= 1; else if (CE) q_m <= d_m;` (the `if`s are swapped with
  `--preset-wins`; only existing controls appear). Clock tied high: no clock edge in the list.
* Latch: `always @* if (ar_m) q_m <= 0; else if (ap_m) q_m <= 1; else if (clk_m) q_m <= d_m;`
  — **non-blocking**, so in a zero-delay event simulator a latch output changes after every
  flip-flop clocked by the same edge has sampled its D. This matches step 2 of the simulator
  (01-model.md §M5) and real clock-to-output delay; with a blocking assignment iverilog would race.
* Every register is declared `reg q_m = <power-up>;`.
* A register whose MC is inactive (rule 15) is the constant power-up value.

Known Verilog idiom limit: when AR and AP are both asserted and the winning one is released
first, an edge-triggered `always` block does not re-evaluate, so Q keeps the winner's value
until the next event, whereas the level semantics of the model would switch to the other one.
The simulator and the formal flow (`clk2fflogic` turns the controls into levels) do not have
this limit.

## V4 Verification of the emitter

`tools/sim/cosim.py` runs the emitted Verilog in iverilog and `atf15xx-sim run` on the same
single-change stimulus and compares every I/O pin after every step. The testbench drives all
inputs to 0 first and then resets every flip-flop to its power-up value (unless an async control
holds it), because a time-0 X→1 transition is a `posedge` in Verilog. Results: worklog 04.
