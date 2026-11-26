# F5. Vendor fitter outputs (`.jed`, `.fit`, `.pin`, `.io`, `.tt3`)

`fit150x <design>.tt2 …` writes, next to the input: `<design>.jed`, `.fit`, `.pin`, `.io`, `.tt3`
(and `.tmv`/`.vt` stubs). Only the `.jed` is needed downstream (ATMISP → SVF); our fitters write
the same fuses in our own `.jed` layout (F5.0), and simplified `.fit`/`.pin`.

## F5.0 `.jed` — our layout

Conformance with the vendor fitter is at the **fuse level** (F6.3): same fuse array, same security
fuse (`G`), correct JEDEC checksum. `atf15xx-fit/src/jed.rs::write` writes every fuse
explicitly, one `L` field per fuse-map item of F2 in address order, each followed by an `N` (note)
field naming it. CRLF line endings; every field is shorter than 200 bytes (ATMISP's reader limit,
spec §3.2), and no field contains `NOTE`.

```
<STX>JEDEC fuse map for ATF1504AS, written by open-atf150x <version>
Created: <date>*                                 -created text, default the ctime of the fit
QF34192* QP<pins>* F0* G0*                       G1 with -strategy security on
L00000 <96 fuses>* N MC1 PT5*                    each PT row (F2.2), labelled with its MC and PT
…
L<addr> <32 fuses>* N block A macrocell configuration*      (F2.3; 16-fuse fillers: "macrocell configuration (unused)")
L<addr> <w fuses>* N block A UIM mux 0*          (F2.4), w = 5 / 9 / 27
L<addr> <w fuses>* N global OE 1*                (F2.5)
L<addr> <32 fuses>* N device configuration*      (F2.6)
L<addr> <4 fuses>* N JTAG special bits*
L<addr> <16 fuses>* N user electronic signature*
L<addr> <6 or 2 fuses>* N reserved*
C<checksum, 4 upper-case hex>*
<ETX>0000
```

The 95 conformance vectors (`atf15xx-fit/tests/vectors/`) hold the vendor's fuses in this
layout (`examples/jed_rewrite.rs`, which checks that the fuses survive the rewrite). The reader
(`jed::parse`) accepts any JESD3 fuse file: ours, the vendor fitters' and ATMISP's.

## F5.1 `.jed` — the vendor's format [O] (reference)

For reading vendor files only; we do not write it. Until 2026-11-25 a `write_vendor` writer
reproduced 3000 of 3000 randomly chosen vendor JED files byte for byte (all three families) from
their fuse arrays, which established the format below; it was removed so that no vendor text is
reproduced. All lines end in CRLF.

```
<STX>JEDEC file for: ATF1504AS
Created on: Mon Nov 23 21:53:22 2026            ctime of the fit ("%a %b %d %H:%M:%S %Y")
*
QF34192* QP<pins> * F0*[ G1*]                    G1 only with -strategy security on
NOTE
0         0         0         0         0         *
L<addr>                                          one block per PT row that contains a 1,
<fuses 0..15>                                    in address order
<fuses 16..55>
<fuses 56..95>*  NOTE PT <k> of MC <mc> *
                                                 (empty line)
NOTE macrocell configurations
        0         0         0         0 *
L<addr> <16 fuses>*                              filler
L<addr> <32 fuses>*  NOTE S16,S12 of block A *   label = "S"+a padded to 3, ",", "S"+b padded to 3; then " * " (trailing space)
…                                                all mcfg fields in address order

NOTE   UIM for block A and D *                   one section per 80-field group (1502: "A and B")
NOTE         0         0         0*
L<addr> <w fuses>*  NOTE Mux-<m> of block <X>*   all 80 fields of the group, address order
…

NOTE  6 global OE 
0         0         0*
L<addr>
<w fuses>                                        six lines
*

NOTE  device configuration bits*
NOTE   0         0         0*
L<addr> <32 fuses>*

NOTE  Special Purpose Bits (JTAG) *
L<addr> <4 fuses>*

NOTE  UES bits*
L<addr> <16 fuses>*

NOTE  Reserved bits *
L<addr> <6 or 2 fuses>*

C<checksum, 4 upper-case hex>*
<ETX>0000
```

The group name of a UIM section lists the two blocks in the order of their `Mux-0` fields. The
checksum is the standard JEDEC fuse checksum (sum of 8-fuse bytes, fuse 0 = LSB, mod 2¹⁶).

## F5.2 `.fit` report (vendor)

Line 4 repeats the command line in normalised form, e.g.
`fit1504 input.tt2 -device P1504C44 -preassign keep -jtag off -pin_keep off -output_fast on -open_collector INT`
(`-strategy` is dropped). Sections, in order: "Initial fitting strategy and property" (all option
values), fitter passes, "Final global control pins assignment", "Performing input pin
pre-assignments", "Attempt to place floating signals" (lines `X is placed at pin N (MC m)`,
`… at feedback node 6nn (MC m)`, `… at foldback expander node 3nn (MC m)`, `X assigned to pin N`
for dedicated pins), an ASCII pin diagram, "Universal-Interconnect-Multiplexer assignments"
(`FanIn assignment for block X [n] { names… }` then `Multiplexer assignment for block X`, lines
`NAME (MCk FB|P) : MUX m Ref (Xnfb|Xnp)` — only *some* routed signals are listed, so the JED is the
authoritative routing record), the programmed logic (post-fit equations in CUPL syntax), the
`Pin/Node Placement` list (`Pin N = X; /* MC m */`, `PINNODE 6nn = X; /* MC m Feedback */`), and
"Resource Usage" (per-MC table: `MCell Pin# Oe PinDrive DCERP FBDrive DCERP Foldback CascadeOut
TotPT output_slew`, where DCERP = type C/D/L/T, clock c/g, CE e, reset g/r, preset p). Reports use
CRLF. `tools/fuzzers/common/fitter_oracle.py::parse_fit` extracts placement, UIM listing and globals.

## F5.3 `.pin`, `.io`, `.tt3`

* `.pin`: `CHIP "<design>" ASSIGNED TO AN <PACKAGE>` followed by `NAME : NUM` lines.
* `.io`: a CUPL-like `CHIP … BEGIN DEVICE = "PLCC44"; "D7" … : OUTPUT_PIN = 12 ; … NODE_NUM … INPUT_PIN …`.
* `.tt3`: the fitted design as a `.tt2`-style PLA (`#$ TOOL CUPL Atmel 1504 1.8.7.8`, pins with
  numbers, nodes with `6nn` numbers, `.type fr`, outputs encoded with `~`). Useful as the fitted
  netlist; not needed for the JED.

Our `.fit`/`.pin` (`atf15xx-fit/src/report.rs`) list placement and UIM routing in the same
line formats as the vendor's placement lines; they are not byte-compatible (not required).

## F5.4 `.map.json`: machine-readable placement (ours only; format `atf15xx-map/1`)

Written next to the `.jed` by our `fit150x` binaries (`<stem>.map.json`) and by `atf15xx-synth`
(`<top>.map.json`). It lets a checker relate source state to macrocells, e.g. register
correspondence or state injection. It is produced by `atf15xx_fit::report::map_json`.

```json
{
  "format": "atf15xx-map/1",
  "device": "P1508C84",
  "design": "blink",
  "init_policy": "error",
  "cells": [
    { "name": "counter5", "source": "counter[5]", "kind": "buried", "mc": 17, "block": "B", "node": 617,
      "pin": null, "shared_mc": false, "storage": "tff", "registered": true, "q_is_value": true,
      "invert_fuse": false, "fuse_storage": "tff", "output_tap": "comb", "feedback_tap": "reg" }
  ],
  "inputs": [ { "name": "clk", "source": "clk", "pin": 83, "mc": null, "dedicated": "Gclk1" } ]
}
```

`init_policy` is written by `atf15xx-synth` only.

Cell fields:

| field | meaning |
|---|---|
| `name` | netlist name, as in the `.fit` placement lines and the `.tt2` |
| `source` | the Verilog net or port bit the cell was made from (`atf15xx-synth`: the Yosys net name, `counter[5]`, `shift_in[0]`; `fit150x`: the `.tt2` name) |
| `kind` | `output` (drives `pin`) or `buried` |
| `mc`, `block`, `node` | macrocell (1-based), block letter, CUPL feedback node number `600 + mc` |
| `shared_mc` | two cells share the macrocell (logic doubling: a buried register hosted in a combinatorial output's macrocell; the register is the one with `registered`) |
| `storage` | netlist storage: `comb`, `dff`, `tff`, `latch` |
| `registered` | the cell is the macrocell's register/latch |
| `q_is_value` | the register's Q equals the cell's value |
| `invert_fuse` | the macrocell's `Invert` fuse (F2.3). The sum is inverted *before* the XOR/D input; Q is still the value |
| `fuse_storage`, `output_tap`, `feedback_tap` | as decoded from the fuses (`atf15xx_arch::decode`) |

`q_is_value` is always true for our fitter, which never stores a complement in a register.

The default `--init error` never inverts a register. With `atf15xx-synth --init invert`, Yosys
stores registers with initial value 1 inverted. Their `source` is the internal Yosys net, and
the source register is then `!Q`.

`inputs` lists the input pins: `name`, `source`, `pin`, `mc` (null for dedicated inputs) and
`dedicated` (`Gclk1`, `Gclr`, `Oe1`, `Gclk2Oe2`, or null).
