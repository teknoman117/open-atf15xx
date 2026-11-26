# F2. ATF15xxAS JEDEC fuse map (fitter view)

This chapter gives every fuse of the ATF1502AS, ATF1504AS and ATF1508AS JEDEC file a meaning,
precisely enough to write an assembler and a disassembler. [§5 of the ATMISP spec](../05-fuse-maps.md)
describes how the same fuse array is cut into programming rows for SVF; this chapter says what the
fuses *do*. Together they take a design from `.tt2` to `.jed` (this chapter, [F4](04-fitter-algorithm.md))
to `.svf` ([§5](../05-fuse-maps.md), [§6](../06-svf-writer.md)).

The machine-readable form of everything here is `atf15xx-arch` (`src/lib.rs` for the
formulas; the tables are generated at build time from prjbureau's `database.json` and
`data/devices.json`, docs/plan/prjbureau-import.md).

**Evidence levels** used below:

* **[O]** oracle-verified: derived from, and checked against, outputs of the vendor fitters
  (`fit1502.exe`/`fit1504.exe`/`fit1508.exe` 1.8.7.8 under wine) with zero counterexamples.
  The experiment that establishes it is named (workflows in `tools/fuzzers/`).
* **[L]** labelled: the vendor's own `NOTE` labels in the JEDEC files its fitters and ATMISP write
  (black-box output; `tools/fuzzers/fitter-macrocell/vendor_labels.py`, `tools/fuzzers/fuse-regions`).
* **[P]** taken from whitequark's prjbureau database (1502AS/1504AS only) and *not* contradicted
  by our oracle, but not independently exercised; for the 1508AS it is an extrapolation.
* **[I]** inferred (pattern only, never observed to vary).

Notation: `b` = block index (A = 0 … H = 7), `l` = block-local macrocell index 0…15, global
macrocell number `mc = 16·b + l + 1`. A fuse value **0** means *programmed*: in the product-term
array, "this input is connected". The vendor file always uses `F0` (all unlisted fuses are 0).

## F2.1 Geometry

| | 1502AS | 1504AS | 1508AS |
|---|---|---|---|
| blocks / macrocells | 2 / 32 | 4 / 64 | 8 / 128 |
| `QF` (fuses) | 16808 | 34192 | 74136 |
| UIM selector width `w` | 5 | 9 | 27 |
| PT array `[0, P)` , `P = blocks·80·96` | 0–15359 | 0–30719 | 0–61439 |
| macrocell config `[P, U)`, `U = P + blocks·480` | 15360–16319 | 30720–32639 | 61440–65279 |
| UIM selectors `[U, G)`, `G = U + 40·blocks·w` | 16320–16719 | 32640–34079 | 65280–73919 |
| global OE selectors `[G, C)`, `C = G + 6w` | 16720–16749 | 34080–34133 | 73920–74081 |
| device configuration (32) | 16750–16781 | 34134–34165 | 74082–74113 |
| special-purpose (JTAG) (4) | 16782–16785 | 34166–34169 | 74114–74117 |
| UES (16) | 16786–16801 | 34170–34185 | 74118–74133 |
| reserved (written 0) | 16802–16807 (6) | 34186–34191 (6) | 74134–74135 (2) |

The layout is a partition of the whole array (unit test `layout_is_a_partition`). **[O]**

### Block positions

Blocks are not stored in letter order. Physical position `pos(b)` of logical block `b` (used by
the PT array and the macrocell-config region): **[L][O]**

| family | pos(A…) |
|---|---|
| 1502 | A0 B1 |
| 1504 | A0 B1 D2 C3 (i.e. pos = [0,1,3,2]) |
| 1508 | A0 B1 C2 D3 H4 G5 F6 E7 (pos = [0,1,2,3,7,6,5,4]) |

## F2.2 Product-term array

Each macrocell owns five product terms PT1…PT5; each PT is a 96-fuse row.

**Row address** **[L][O]** (from the vendor's `NOTE PT k of MC m` labels, 0 disagreements over all
labelled rows in the corpus; ATMISP's `Load` output numbers PT rows differently, see note below):

```
s      = l            (1502)      s = 15 - l   (1504, 1508)        -- slot of the MC in the block
within = 5 - k   if s even,       k - 1   if s odd                  -- k = PT number 1..5
row    = pos(b)·80 + s·5 + within
addr(mc, k) = 96 · row
```

**Columns** (index `j` within the 96-fuse row) **[O]** (exp_ptcol2/exp_ptcol3: 1502 471, 1504 1572,
1508 4555 observations, 0 counterexamples, all 40 muxes):

| j | input |
|---|---|
| 0…15 | foldback expander of block-local MC `j` (the inverted PT1 of that MC when its `Pt1Mux` = 1) **[O]** (exp_fold) |
| 16 + 2·m + t | UIM mux `m` (0…39) of this block; true literal at `t = 0`, complement at `t = 1` (1504, 1508) |
| 16 + 2·m + t | 1502: as above for even `m`; for odd `m` true is `t = 1` and complement `t = 0` |

A PT is the AND of every column whose fuse is 0; a row of all 1s is the constant 1.
**Unused PTs are all 0** and the vendor omits them from the file (prjbureau reports that a single 1 in
an unpowered PT makes it read high **[P]**; we never write that). The vendor writes a row iff it
contains a 1. A PT row is printed as `L<addr>`, then 16, 40 and 40 fuses on separate lines.

> **Note on ATMISP's `Load` labels.** The JEDEC files that ATMISP writes for `Load` (spec §8)
> label PT rows with a different (MC, PT) numbering (for the 1504, template "MC3 PT1" is vendor
> "MC14 PT5"); the addresses are the same fuses. This spec, the arch crate and the fitter use the
> fitter's numbering, which is also what `.fit` reports use. It is the *functional* numbering (PT5
> is the one that can drive OE / preset, PT4 clock / CE, …) and equals prjbureau's `pterm_ranges`
> for every macrocell of the 1502AS/1504AS (F2.7); see also docs/prjbureau/README.md §3.

## F2.3 Macrocell configuration region

`blocks·12` labelled 32-fuse fields plus `blocks·6` unlabelled 16-fuse fields, in 80-fuse rows:
row `r` occupies `[P + 80r, P + 80r + 80)` = 16-fuse filler, field X (32), field Y (32). **[L]**

* **Fillers** (`L<P+80r>`, 16 fuses): always written as 1s by the vendor; never observed
  otherwise across ~20 000 files including the user's designs. Function unknown. **[I]**
* **Labelled fields**: each holds two configuration bits (the vendor labels them `Sa,Sb`) for all
  16 macrocells of one block. With `H = blocks/2` (1502: `H = 1`), `p = pos(b)`, half `h = p div H`,
  `k = p mod H`, and pair index `q` from the table below, the field is in row
  `r = h·6H + q·H + k` (X or Y as listed). **[L]**

| q | field X labels | field Y labels |
|---|---|---|
| 0 | S16,S12 | S14,S11 |
| 1 | S9,S6 | S13,S10 |
| 2 | S20,S18 | S8,S21 |
| 3 | S7,S19 | S22,S5 |
| 4 | S23,S4 | S3,S15 |
| 5 | S0,S1 | S17,S2 |

**Macrocell column inside a field** **[O]** (exp_dir, exp_mcpos, exp_feat; consistent for every MC
of every family): MC `l` uses fuses `field + 2s` and `field + 2s + 1` where

```
s = l        for pairs {S0/S1, S4/S23, S6/S9, S7/S19, S12/S16, S18/S20} on 1504/1508,
             and for the other six pairs on 1502;
s = 15 - l   otherwise.
```

and the two fuses of the column are swapped when `s` is odd. We name the column's bits `k0`, `k1`
after undoing the swap: `addr(bit) = field + 2s + (k XOR (s AND 1))`.

### F2.3.1 The 24 macrocell bits

Bit names are ours; the prjbureau names are given for cross-reference. Values are raw fuse values.

| pair, k | name (prjbureau) | meaning | evidence |
|---|---|---|---|
| S0/S1 k0 | `OeMux0` (oe_mux bit 0) | output-enable select, see F2.3.2 | [O] |
| S0/S1 k1 | `OeMux1` (oe_mux bit 1) | | [O] |
| S2/S17 k0 | `Pt5Func` (pt5_func) | PT5 special function: 1 = async preset (AS), 0 = output enable | [O] |
| S2/S17 k1 | `OeMux2` (oe_mux bit 2) | | [O] |
| S3/S15 k0 | `OMux` (o_mux / dfast_mux, shared) | output: 1 = combinatorial, 0 = register | [O] |
| S3/S15 k1 | `GclrMux` (gclr_mux) | 0 = GCLR resets the register, 1 = no global reset | [O] |
| S4/S23 k0 | `Storage` (storage) | 1 = flip-flop, 0 = latch | [O] |
| S4/S23 k1 | `DMux` (d_mux) | register D: 1 = logic (XOR output), 0 = fast path: PT2 if `OMux` = 1, else the pad | [O] (sim) |
| S5/S22 k0 | `Slew` (slew_rate) | 1 = slow, 0 = fast | [O] |
| S5/S22 k1 | `Pt4Func` (pt4_func) | PT4 special function: 1 = clock, 0 = clock enable | [O] |
| S6/S9 k0 | `Pt1Mux` (pt1_mux) | PT1: 1 = foldback expander, 0 = sum | [O] |
| S6/S9 k1 | `CasMux` (cas_mux / xor_a_mux, shared) | 1 = sum drives the cascade into MC+1 | [O] |
| S7/S19 k0,k1 | `Gclk0`,`Gclk1` (gclk_mux) | global clock: (k1,k0) 11 = GCLK1, 00 = GCLK2, 10 = GCLK3 | [O] |
| S8/S21 k0 | `LowPower` (low_power) | 1 = reduced-power macrocell (`MC_power`) | [O] |
| S8/S21 k1 | `PtPowerOff` (pt_power) | 1 = all five PTs powered down | [O] |
| S10/S13 k0 | `Pt2Mux` (pt2_mux) | PT2: 1 = XOR input, 0 = sum | [O] |
| S10/S13 k1 | `XorB` (xor_b_mux) | 1 = XOR with register Q (T flip-flop) | [O] |
| S11/S14 k0 | `Pt5Mux` (pt5_mux) | PT5: 1 = special function, 0 = sum | [O] |
| S11/S14 k1 | `Invert` (xor_invert / reset, shared) | the XOR's B enable: B = `Invert` ∧ (`CasMux` ? 1 : P) (row 6); with `Pt2Mux` = 0 and `CasMux` = 0: 0 = output is the sum, 1 = complement | [O]; full table silicon |
| S12/S16 k0 | `Pt3Mux` (pt3_mux) | PT3: 1 = async reset, 0 = sum | [O] |
| S12/S16 k1 | `Pt4Mux` (pt4_mux) | PT4: 1 = clock / clock enable, 0 = sum | [O] |
| S18/S20 k0 | `OpenDrain` (output_driver) | 1 = open drain, 0 = push-pull | [O] |
| S18/S20 k1 | `FbMux` (fb_mux) | feedback to UIM: 1 = combinatorial, 0 = register | [O] |

prjbureau's routing tables for the PT1/PT2/XOR, PT3, PT4, PT5 and D/Q groups (its
`docs/options/logic.rst`) agree with every combination we observed. **[P]**

### F2.3.2 Output-enable code

`code = OeMux0 + 2·OeMux1 + 4·OeMux2` **[O]** for 0, 7 and GOE1/2/4/5; **[P]** for GOE3/GOE6:

| code | 0 | 7 | 4 | 2 | 6 | 1 | 5 | 3 |
|---|---|---|---|---|---|---|---|---|
| OE | off (GND) | PT5 or always (see `Pt5Func`) | GOE1 | GOE2 | GOE3 | GOE4 | GOE5 | GOE6 |

With code 7: `Pt5Func` = 0 and `Pt5Mux` = 1 → OE is PT5; `Pt5Func` = 1 → output always enabled.

**Code 7, `Pt5Func` = 0, `Pt5Mux` = 0 (PT5 steered into the sum): output never enabled [I]**
(settled 2026-11-24, worklog 05). Model: PT5 goes either to the sum or to its special-function
line (`Pt5Mux`), and `Pt5Func` routes that line to the preset or to the OE mux. With PT5 in the
sum the line is 0, so OE code 7 ∧ ¬`Pt5Func` gives 0, just as the preset gives 0 (rule 11).

Evidence:
* prjbureau's PT5 routing table (`external/prjbureau/docs/options/logic.rst`, [P]) has the row
  `sum / as / VCC_pt5 → IO.EN = VCC` with the footnote that `pt5_func` *must* be `as` to get VCC
  with PT5 in the sum. There is no row for `sum / oe / VCC_pt5`. This fits the special-line model
  (`oe` would select the line, which is 0).
* Vendor usage, scanning all 17 193 vendor JEDs of the oracle corpora and the user's designs
  (`cargo run --release -p atf15xx-fit --example oe_scan`, `.scratch/flow/oe_scan*.tsv`): the
  combination occurs 119 times. 118 are cascade lenders whose pin is unused, and one is a buried
  node of `motioncontroller`. It never occurs on a used pin. Lenders whose pin is an input get
  code 0 (25×), always-enabled outputs with PT5 in their own sum get `Pt5Func` = 1 (1×), and
  OE'd outputs keep PT5 for the OE. The vendor never relies on the ungated reading, and under it
  would drive 118 unused pins.
* Formal: the vendor `rpi-pico-8088` JED is PROVEN equal to its `.tt2` only under this reading
  (worklog 04; under the ungated reading it drives unused pin 11).

Not silicon-tested. `atf15xx_arch::decode` keeps the raw OE-mux reading in
`MacrocellConfig::oe` and adds `MacrocellConfig::oe_effective()` (this rule). `atf15xx-sim` gates
by default (`--oe-pt5-ungated` = raw reading). **Our fitter never
emits the combination**: an MC without an OE whose PT5 is in its sum gets code 0, so its meaning
is the same under both readings (`encode_logic`; cascade lenders already use the unused-MC
encoding, code 0).

### F2.3.3 What the vendor writes for each kind of macrocell [O]

The fitter is byte-compatible only if it reproduces these "don't care" choices
(`atf15xx-fit/src/fit.rs`, `encode_unused` / `encode_logic`). Bit strings below list the
24 bits in the order of the table in F2.3.1 (k0 k1 per pair):

| role | bits |
|---|---|
| unused | `00 10 11 01 11 00 00 01 00 00 00 01` |
| pad used as input only (input pin, JTAG pin, PD pin) | `00 00 11 11 11 01 00 01 00 10 11 01` |
| comb output, OE always, 1-PT sum | `11 11 11 11 11 00 00 00 00 10 11 01` |
| D flip-flop output, GCLK1, no reset | `11 11 01 11 10 00 11 00 00 10 10 00` |
| buried register on an input-pad MC | `00 00 11 11 10 00 11 00 00 10 10 00` |

Rules that generate these (all oracle-derived): unused PT3/PT4/PT5 are routed to their special
function except PT4 of a register on a global clock (`Pt4Mux` = 0, `Pt4Func` = 0); `OMux` = 1
except for a registered *pin* output; buried cells use OE code 0 if their pad is read, else 7 with
`Pt5Func` = 0; constant-1 outputs use no PT with `CasMux` = 1 and `PtPowerOff` = 1; `output_fast on`
clears `Slew` on *every* macrocell, used or not; `MC_power on` sets `LowPower` on every macrocell.

## F2.4 UIM selectors [P][L][O]

Each block has 40 muxes; mux `m` of block `b` is a `w`-fuse one-cold field (all 1 = unused; a 0
at position `i` selects entry `i`). **The table of selectable inputs is the same for every block**
(`uim_table`, generated at build time from prjbureau's database by `atf15xx-arch/build/fuse_map.rs`
**[P]**; for the 1508 from our run of prjbureau's fuzzers, `tools/fuzzers/prjbureau`). Entries are `Xnp`
(pad of MC n), `Xnfb` (feedback of MC n), `GND`, and the dedicated inputs `GCLK` (GCLK1 pin), `GCLR`,
`OE1`, `OE2` (GCLK2/OE2 pin). Verified against every routing the vendor reported in `.fit` files:
1502 8036, 1504 13617, 1508 5391 observations, 0 mismatches.

Field address `U + w·i`: **[L]**

* 1502: `i = 78 - 2m + b`.
* 1504/1508: the blocks are paired (1504: A+D, B+C; 1508: A+H, B+G, C+F, D+E); pair `g`
  occupies `i ∈ [80g, 80g + 80)`. For the first block of the pair `i = 80g + 4(m div 2) + (m even ? 0 : 3)`,
  for the second `i = 80g + 4(m div 2) + (m even ? 2 : 1)`.

## F2.5 Global output enables [P][L][O]

Six selectors of width `w` follow the UIM region. Global OE network `n` (1…6) uses field index
`n − 1` (1504, 1508) or `6 − n` (1502), and table row `39 + n` of the input table (`goe_table`).
Verified for GOE1, GOE2 (1502/1504), GOE1, GOE4 (1508), GOE5 (1502). **[O]**

## F2.6 Device configuration (32 fuses at `C`) and special bits

Offsets relative to `C`. "Vendor" is what `fit150x` writes by default. **[O]** unless marked.

| offset | meaning | vendor default |
|---|---|---|
| 0 | (prjbureau "arming switch") **[P]** | 0 |
| 1 | pin termination: 0 = bus keeper on all I/O (`pin_keep on`, default), 1 = off | 0 |
| 2, 3 | PD1, PD2 pin function: 0 = power-down input | 1, 1 |
| 3+n (n = 1…6) | GOE n polarity: 1 = active high, 0 = inverted | 1 |
| 10 | GCLR polarity: 1 = active high, 0 = inverted | 1 |
| 11, 14, 17 | GCLK1/2/3 network inversion: 1 = inverted (per network, applied after the source select) **[O][Si]** | 0 |
| 12–13, 15–16, 18–19 | GCLK1/2/3 **network source select** (bit0, bit1): 11 = GCLK1 pin, 10 = GCLK2/OE2 pin, 00 = GCLK3 I/O pin (01 unobserved). Vendor always writes 11, 10, 00 (each network from its own pin) **[O]**; **any network can take any clock pin [Si]** (ATF1508AS, F2.9), as in prjbureau's 1502 database **[P]** | 11, 10, 00 |
| 20, 21, 22 | input-transition detection on GCLK1/2/3 pins (`GCK_ITD`): 0 = on | 0 0 0 |
| 23 | TMS pull-up: 0 = on | 1 |
| 24 | TDI pull-up: 0 = on | 1 |
| 25 | `power_reset on` → 0 (prjbureau: reset hysteresis) | 1 |
| 26–31 | never observed ≠ 1 **[I]** | 1 |
| special 0 | never observed ≠ 1 (security is the JEDEC `G1` field, not a fuse) | 1 |
| special 1 | JTAG pins reserved for JTAG: 1 | 1 (JTAG on is the default) |
| special 2, 3 | never observed ≠ 1 **[I]** | 1 1 |
| UES | user signature, all 1 by default | 1… |
| reserved | | 0… |

`-strategy security on` adds the `G1*` field after `F0*` in the header; no fuse changes. **[O]**

### F2.3.4 Semantics checked by simulation [O]

A pin-level simulation built from this chapter alone was compared with the `.tt2` each JED was
fitted from (first with a Python model, retired 2026-11-25; now `tools/sim/equiv.py`, formal +
simulation, with `atf15xx-sim`). The vendor's
JEDs for the user's four ATF1504AS designs (`input`, `motioncontroller`, `rpi-pico-8088`, `spi`,
which use latches, T/D flip-flops, global and PT clocks, CE, AR, GOE/PT OE, open collector,
foldback expanders, cascades and logic doubling) all match their `.tt2` over 1000 random cycles.
Two semantic points were settled this way:

* the foldback line is the **complement** of the host's PT1 (`input` fails otherwise);
* with `DMux` = 0 and `OMux` = 1 ("logic doubling": a combinational pin output and a buried
  register in one macrocell, as the vendor does under `-logic_doubling on`), PT2 drives the
  register's D **and is removed from the sum** even though `Pt2Mux` = 0 (`spi` fails otherwise).

The vendor's ATF1508AS `system-controller` JED also matches its `.tt2`, once the third point is
modelled:

* `Pt4Func` = 1 with PT4 in the sum ties the clock/enable **high**. The vendor implements the
  combinational `-strategy global_fold` node `CPU_Peripheral_Select` as a latch that is always
  transparent (`.fit`: `L----`); treating its enable as GCLK2 (the `Gclk` code it carries) caused
  45 mismatching pin samples per 1000 cycles.

Simulation note: latches with a real enable are sampled once per stimulus step; some designs
(`input`) contain loops through transparent latches whose result depends on evaluation order.

## F2.7 Cross-check against prjbureau (1502AS, 1504AS)

While this map was still built from our own experiments, a cross-check against prjbureau's
`database.json` (upstream commit `8b8a97122ec238b6a09868a4bc7def20acef798c`) gave the table below.
Since 2026-11-25 the fuse-map tables are generated from that database at build time, where every
option fuse is checked against the formulas of this chapter (a disagreement fails the build;
docs/plan/prjbureau-import.md):

| item | ATF1502AS | ATF1504AS |
|---|---|---|
| PT row ranges (`pterm_ranges`, functional PT numbering) | 160/160 agree | 320/320 agree |
| macrocell option fuses → our bits (F2.3.1) | 768/768 | 1532/1532 |
| option values → our raw encodings | 1504/1504 | 2996/2996 |
| UIM field ranges | 80/80 | 160/160 |
| UIM selectable inputs (every named mux value) | 336/336 | 1328/1328 |
| GOE selector fields, GCLK invert/mux, GCLR invert, config/pin bits | all agree | all agree |
| GOE *invert* bit per network | **6/6 disagree** | 6/6 agree |

The one disagreement: prjbureau places the 1502AS GOE`n` invert bit at `C + 10 − n`; the vendor
fitter 1.8.7.8 writes it at `C + 3 + n` (oracle: OE from the OE1 pin, inverted, selects GOE5's field
and clears `C + 8`; from the OE2 pin, GOE2's field and `C + 5`). On the 1504AS both agree on
`C + 3 + n`. Our fitter follows the vendor; which one the silicon honours is untested.

Upstream prjbureau has no ATF1508AS data; the 1508 tables come from our run of prjbureau's fuzzers
on the 1508 (branch `atf1508as` of the fork, `tools/fuzzers/prjbureau`), and the formulas of this
chapter are confirmed for it by our own oracle experiments, except the GOE3/GOE6 output-enable
codes, which are extrapolated.

## F2.8 Functional semantics (for simulators)

This is the model implemented by `atf15xx-sim` and exposed structurally by
`atf15xx_arch::decode::decode` (fuse array → `Decoded`: per-MC PT literals, `PtUse`, storage, clock,
D source, output/feedback taps, OE; UIM and GOE selections; global settings). Evidence:
**[O-sim]** = the vendor's JEDs simulate identically to their `.tt2` source under this rule
(4 user ATF1504AS designs, 95 XOR designs, 59 other synthetic designs; see F2.3.4).

| # | rule | evidence |
|---|---|---|
| 1 | `pt(k)` = AND of its connected columns; a PT row that is all 0, or any PT of an MC with `PtPowerOff` = 1, is constant 0; an all-1 row is constant 1 | O-sim (all-0 = off: [P] for powered PTs) |
| 2 | UIM line `m` of block `b` carries the selected input (pad, feedback, dedicated pin; `GND`/unused = 0); true column = line, complement column = !line | O |
| 3 | foldback column `l` of block `b` = `!pt1(MC 16b+l+1)` if that MC has `Pt1Mux` = 1, else 1 | O-sim |
| 4 | `sum` = OR of the PTs routed to the sum (`PtkMux` = 0; PT2 excluded when `DMux` = 0 ∧ `OMux` = 1) | O-sim |
| 5 | cascade: if MC−1 (same block) has `CasMux` = 1, its sum is ORed into this MC's sum | O-sim (one-level chains) |
| 6 | XOR (**silicon**, 2026-11-24, all 6 combinations of `Pt2Mux`/`CasMux`/`Invert`, `bench/exp_xor_cascade.py`): P = `pt2` if `Pt2Mux` = 1 else 1; `CasMux` (xor_a_mux) swaps the inputs: **A = `CasMux` ? P : sum, B = `Invert` ∧ (`CasMux` ? 1 : P), x = A ⊕ B**. So: no XOR: `x = sum ⊕ Invert`; XOR mode (`Pt2Mux` = `Invert` = 1): `x = sum ⊕ pt2`; `Pt2Mux` = 1, `Invert` = 0: `x = sum` (PT2 dropped; the old reading `sum ⊕ pt2 ⊕ 1` was wrong, the vendor never writes it); lender (`CasMux` = 1): `x = P ⊕ Invert`, so a lender with `Pt2Mux` = 1 still outputs PT2 or its complement while its other PTs go to the cascade | silicon; vendor corpus: `Pt2Mux` = 1 only with `Invert` = 1, `CasMux` = 0 (95 MCs in 17 193 JEDs) |
| 7 | an MC with `CasMux` = 1 and no PTs outputs constant 1 (row 6 with P = 1, `Invert` = 0) | O (vendor `vcc`); lender's own output: silicon (row 6) |
| 8 | D input: `DMux` = 1 → `x` (T flip-flop when `XorB` = 1: `D = x ⊕ Q`); `DMux` = 0 → `pt2` if `OMux` = 1, else the MC's own pad | O-sim (pad case [P]) |
| 9 | storage: `Storage` = 1 edge-triggered on the **rising** clock edge, gated by CE (`pt4` if `Pt4Mux` ∧ ¬`Pt4Func`, else 1); `Storage` = 0 transparent latch, open while the clock/enable is 1 | O-sim |
| 10 | clock: `pt4` if `Pt4Mux` ∧ `Pt4Func`; **constant 1** if ¬`Pt4Mux` ∧ `Pt4Func` (a latch is then always transparent, a flip-flop never clocks — the vendor's encoding of `-strategy global_fold` nodes); global network if ¬`Pt4Func`: (`Gclk1`,`Gclk0`) 11 → GCLK1, 00 → GCLK2, 10 → GCLK3; network n = its pin ⊕ GCLK n invert bit | O-sim (1508 `system-controller` `CPU_Peripheral_Select`; agrees with prjbureau's PT4 table [P]; invert bit: O by encoding only) |
| 11 | async reset `AR = (Pt3Mux ? pt3 : 0) ∨ (GclrMux = 0 ? GCLR pin ⊕ ¬GclrInvert : 0)`; async preset `AP = (Pt5Mux ∧ Pt5Func) ? pt5 : 0`; **reset has priority** over preset | O-sim; **priority and GCLR polarity silicon-verified** (F2.9) |
| 12 | pin data = `x` if `OMux` = 1 else `Q`; feedback (`Xnfb`) = `x` if `FbMux` = 1 else `Q`; pad (`Xnp`) = the pin's value, i.e. the driven value when enabled, else the external level | O-sim |
| 13 | OE (F2.3.2): code 0 → off; 7 ∧ `Pt5Func` → on; 7 ∧ ¬`Pt5Func` → `pt5` if `Pt5Mux` = 1, **off if PT5 is in the sum**; else GOE n = selected input ⊕ ¬(GOE n invert bit) | O-sim; the PT5-in-sum case [I] (F2.3.2) |
| 14 | `OpenDrain` = 1: drives 0 when data is 0 and OE true, otherwise high-Z | O-sim (motioncontroller/spi `INT`) |
| 15 | power-up state of registers: 0, independent of `Invert` | **silicon, after ISP** (F2.9); cold power-up not yet tested |
| 16 | the vendor routes register feedback of an output *with* an OE through the pad (F7.1) — a JED written that way reads the pin, not Q | O |

**Notes on the two inferred rules (11, 15).**

* *Reset vs preset priority.* Neither the datasheets (ATF1504AS(L) §"Flip-flop": AR is GCLEAR, a PT,
  their OR, or off; AP is a PT or off) nor *Using the ATF1500 CPLD* ("Asynchronous Reset and
  Preset") state what happens when both are asserted, and no vendor-fitted design in the corpus
  asserts both at once with the outputs observed. Reset-wins is the conventional choice for this
  family of macrocells (22V10 heritage) and is what `atf15xx-sim` should assume, flagged.
  A silicon test (both PTs driven from pins) is the only real confirmation.
* *Power-up state.* The datasheet: "at a point delayed slightly from VCC crossing VRST, all
  registers will be initialized, and the state of each output will depend on the polarity of its
  buffer". It does not give the value. prjbureau names the fuse shared with `Invert` `reset`
  (values `GND`/`VCC`), suggesting the power-up value of the register may follow that fuse
  [P]; the vendor fitter, however, picks `Invert` purely to minimise PTs (e.g. `dff_sum3` gets
  `Invert` = 1, `dff_n` gets 0), which would be odd if it also set the power-up value. All our
  simulations start registers at 0 and nothing in the corpus depends on it (every compared design
  resets its registers or only compares after they are written). Open; a silicon test is needed.
* *GCLR polarity.* *Using the ATF1500 CPLD* says the global reset acts "when the pin is low"; the
  vendor fitter writes `GclrInvert` = 1 for `Y.AR = R` and 0 for `Y.AR = !R`, and rule 11 (AR =
  pin ⊕ ¬GclrInvert) reproduces the `.tt2` semantics in every vendor design. Rule 11 is therefore
  exact *relative to the vendor's encoding*; the absolute pin polarity in silicon is not
  independently confirmed.

## F2.9 Disassembly recipe

1. For each MC read the 24 bits (F2.3); OE code, storage, clock selection etc. follow from F2.3.1.
2. For each of its five PTs that is not all-0, collect connected columns; map `16+2m+t` back to
   `(mux m, polarity)` and the mux field's selected entry (F2.4) to a pad/feedback/dedicated input;
   map columns 0–15 to foldback expanders of the same block.
3. Assign PTs to sum or special function with `Pt1Mux…Pt5Mux` and `Pt4Func`/`Pt5Func`; add the PTs of
   MC−1 if MC−1 has `CasMux` = 1; apply `Invert`/`Pt2Mux`/`XorB`.

`atf15xx-fit/src/jed.rs` parses JEDEC files (and writes them vendor-exactly);
`atf15xx_arch::decode::decode` performs steps 1–3 and returns the structural netlist; turning it
into equations or Verilog is left to the simulator / equivalence checker built on it.

## F2.9 Silicon results (ATF1508AS-PLCC84, 2026-11-24)

Measured on the user's breakout board with an Arduino bench (`tools/silicon/`, log
`docs/worklog/06-silicon.md`). Each item is **[Si]**: observed on silicon.

| item | result |
|---|---|
| reset vs preset, both asserted | **reset wins**: PT reset vs PT preset, and GCLR reset vs PT preset, in either assertion order; both are level-sensitive (the preset reasserts when reset is released while preset is still held) |
| GCLR pin polarity | with `GclrInvert` (config C+10) = 1 (the vendor default for `Y.AR = R`), the register clears while the **pin is high** |
| register state after programming (ISP exit) | **0** for `Invert` = 0 and `Invert` = 1 alike; registers set to 1 read 0 again after reprogramming. Rule 15 holds after ISP exit; the `--power-up fuse` reading is ruled out there. A cold power-up is untested (needs power cycling) |
| foldback | the foldback line is the complement of the host's PT1 (exhaustive 5-input truth table, discriminating); common-cube De Morgan factoring `L&a&(b+c+!d)` 64/64; **a foldback PT may read other foldbacks** (two-level NAND-NAND OE = `L&b + !b&c&d`, 64/64 incl. high-impedance) |
| **cascade lender XOR input** | with `CasMux` = 1 the sum goes to the cascade only and the macrocell's own XOR sees **A = 1**: a constant-1 register (`CasMux` = 1, empty sum, a PT4 clock enable) loads 1; `Y = X` with `CasMux` patched to 1 outputs a constant 1 (`.scratch/silicon/lender*`). The old simulator default (own sum) was wrong; the vendor's SELFPROGRAMBOARD fit, which works in the user's hardware, matches its `.tt2` only under this reading |
| vendor fitter defect 3 (pass-2 cascade inverts registers, `blink.v` on 1508) | **confirmed**: right after programming the vendor JED drives `leds0` = 0 where the source gives 1 (~counter[24]); our fit gives 1; both blink at 2.00 Hz |
| `CasMux` = 1 with PT2 routed to the XOR (`Pt2Mux` = 1), settled | MC16 patched to every (`Pt2Mux`, `CasMux`, `Invert`) with PT1 = e∧a, PT2 = d∧b, 16 inputs each: `CasMux` = `Pt2Mux` = 1 gives `pt2` (`Invert` = 0) / `¬pt2` (`Invert` = 1); `Pt2Mux` = 1 alone gives `sum` (`Invert` = 0) / `sum ⊕ pt2` (`Invert` = 1). Model: row 6 (both simulators and jedsim updated; each variant JED PROVEN equal to its silicon truth table). prjbureau's "A = PT2" is right for `Invert` = 0 |
| fast input register **and** cascade helper in one macrocell (P2+) | `designs/inreg_cascade.tt2`: MC126 is the fast input register of P80 (GCLK2) and lends its 5-PT sum (`CasMux` = 1, all `PtkMux` = 0) to an 8-PT parity in MC127; 4 register loads × 8 inputs, pad flipped after each load, P4/P5 and both macrocells' scan cells: 32/32 correct (`bench/exp_inreg_cascade.py`) |
| fast input register (`DMux` = 0, `OMux` = 0: D = pad) | `designs/vinreg.v`: register in the input pin's own macrocell, 200 random clock/reset steps, 0 wrong |
| T flip-flops, carry chains with combinational buried nodes, cascades | correct (exact counts up to 10⁶ at 1 MHz) |
| JTAG-enable bit | special bit 1 (fuse 74115); JTAG survives every image programmed with it = 1 |
| dedicated input OE1 used as an array input | works |
| **per-macrocell clock polarity** | the three GCLK networks each have a source mux (any clock pin) and an invert bit: two networks from one pin with opposite inversion give rising- and falling-edge registers on one pin without a PT clock; a re-sourced, inverted network shows no spurious edge after programming (`designs/vpol.v` + `patch_jed.py`, `designs/vedges.v`). The vendor fitter never does this (it uses a PT clock, oracle `.scratch/polarity`) |
| GCLR and OE polarity | GCLR has one network with one invert bit and no source mux (the vendor gives the other polarity a PT3 reset). On the 1508 the OE1 pin reaches only GOE1 and OE2 only GOE4 (`goe_table`), so an OE pin cannot feed two GOE networks of opposite polarity; the other polarity needs a PT5 OE (vendor) or a macrocell buffering the pin into another GOE's feedback source |
| OE1 as the global output enable (GOE1 ← pin 84) | outputs drive while the pin is high, high-impedance while low (GOE polarity bit = 1, vendor default) |
| bus keeper is device-wide | the vendor's `pin_keep = A X` writes the same fuses as `pin_keep on` (oracle `.scratch/keeper`); only C+1 changes between on and off. No per-pin keeper fuse is known |
| bus keeper (config C+1 = 0, `pin_keep on`) | holds an undriven pin at its last level (no hum); the Arduino's ~35 kΩ pull-up overpowers it |
| **floating dedicated inputs** | with the bus keeper off (`pin_keep off`) and GCLK2/OE2 and OE1 left floating, a correct design misbehaved intermittently (missed counts); driving the pins fixed it. Tie unused dedicated inputs, or keep `pin_keep on` |
| timing | a design with 4 array passes per register path fails at 66.6 MHz while 1- and 2-pass versions of the same function work (F8, log 06) |

## F2.10 JTAG boundary scan (silicon, ATF1508AS-PLCC84, 2026-11-24)

Instructions (IR length 10, vendor BSDL): EXTEST `0000000000`, SAMPLE `0001010101`, IDCODE
`0001011001`, BYPASS `1111111111`; no INTEST. The boundary register is **352 cells on the
ATF1508AS whatever the package** (measured on the PLCC84; the vendor's PQFP100 BSDL also says
352): cells 0–95 are input cells (OE1, GCLK1, then the pads of 92 macrocells in a fixed order, with
`internal` entries for pads a package does not bond, then GCLK2, GCLR), cells 96–351 one pair per
macrocell, MC128 down to MC1: `oe` then `out` (BC_1). The `out` cell captures the macrocell's
**output mux** (`OMux`): the register Q of a registered or `OMux` = 0 macrocell, else the XOR
output; `oe` captures the OE. Verified cell by cell for the bench's lines, a fast input register,
combinatorial outputs and EXTEST writes (the bench's boundary-scan checks, 2026-11-24; their facts are kept by test `bench_facts`), and for a counter's
buried registers fitted with `observable_registers` (`OMux` = 0 for buried registers, pin
unaffected: OE off). Map: `atf15xx_arch::bsdl` (generated at build time from the BSDL of the die, spec D1.2; the
1502/1504 vendor BSDLs follow the same structure).
