# F7. Verilog → ATF15xx flow

See [docs/prjbureau/README.md](../../prjbureau/README.md) for the study of whitequark's prjbureau
(fuse database for 1502AS/1504AS and BE parts, JED↔SVF tools; no fitter, no nextpnr; its EDIF flow
needs a fitter build with `aprim.lib` that WinCUPL 1.8.7.8 lacks).

## F7.1 The working flow

```
design.v ──yosys: tools/sim/synth_atf15xx.ys──► out.json  ($sop covers, DFF/DFFE ± async R/S, latches, $_TBUF_)
        ──tools/sim/yosys2tt2.py --pin NAME=N──► design.tt2
        ──fit1504 (ours: fit1504, or the vendor's)──► design.jed
        ──atf15xx-tool svf──► design.svf
```

Status on the four prototype designs (`.scratch/prjbureau-explore/flow*`: 4-bit counter with CE,
async reset and tri-state outputs on 1502AS/1504AS/1508AS; a latch/preset/bidirectional mix on
1504AS): **our fitters fit all four and their JEDs match the `.tt2` semantics in simulation**
(random simulation against the `.tt2`, 1000 cycles; now `tools/sim/equiv.py`). The vendor's JEDs for the same `.tt2` do *not*: for
outputs with an output enable (`q0.OE = oe`) the vendor fitter routes the register feedback
`q0.Q` through the **pad** (`A1p`, its own `.fit` says `q0 (MC1 P) : MUX 1 Ref (A1p)`), so the counter
reads its own pins and breaks whenever `oe` is low. Our fitter routes the feedback (`A1fb`) and
only uses the pad for always-enabled outputs (where the two are equal). Unless the silicon behaves
differently from prjbureau's and our description of the pad entries, this is a vendor-fitter
defect; it is the reason our JEDs are not byte-identical for these designs.

## F7.2 How the crates fit in

* `atf15xx_fit::netlist::Design` is the natural target of the Yosys JSON above: `$sop` → `Sop`,
  `$_DFF_*`/`$_DFFE_*` → `Storage::Dff` with `clk`/`ce`/`ar`/`ap`, `$_DLATCH_P_` → `Storage::Latch`,
  `$_TBUF_` → `oe`, ports + `--pin` → pin constraints. Today the flow goes through `.tt2`
  (`yosys2tt2.py`); a direct JSON front end in Rust (`netlist::Design::from_yosys_json`) is the
  next step and needs no change to placement, routing or assembly.
* The vendor-exact JEDEC writer lets the output go straight into ATMISP or `atf15xx-tool`.
* The fuse map ([F2](02-fuse-map.md)) is complete enough that the vendor fitter is no longer needed
  as an assembler; it remains the oracle for byte-compatibility.

## F7.3 Missing pieces (as of the `.tt2` prototype; see F7.6 for the current list)

* ~~Rust Yosys-JSON front end; constraint file from Verilog attributes~~ — done, F7.4/F7.5.
* ~~Logic doubling~~ — done in `atf15xx-fit` (worklog 02).
* ~~XOR / T-flip-flop synthesis choices~~ — toggle recovery, F7.4.3.
* Silicon testing of anything not exercised by the vendor (e.g. the 1502 GOE invert question, F2.7).

## F7.4 The open flow: `atf15xx-synth`

```
design.v ──yosys + atf15xx-synth/data/synth_atf15xx.ys──► netlist.json
   ──atf15xx_synth::yosys::import──► netlist::Design ──opt (F7.4.3)──► absorb_output_registers
   ──legalize_special_functions──► fit_design ──► .jed ──atmisp::generate──► .svf
```

No `.tt2` is involved; `--emit-tt2` writes the optimised netlist as one, for the vendor fitter
(oracle) or `fit150x`. Driver: `atf15xx-synth build design.v --device ATF1504AS-PLCC44 -o out/`
(crate README). Yosys is pinned to v0.71 in `docker/yosys/` (the script also runs on 0.33).

### F7.4.1 Synthesis script

`proc; tribuf; synth -flatten -noabc -run coarse; synth -flatten -noabc -run fine:;` then
`dfflegalize` to exactly the storage the macrocell has, an optional logic mapping (`--map`), and
`opt_clean -purge`:

| Yosys cell after `dfflegalize` | macrocell |
|---|---|
| `$_DFF_P_`, `$_DFFE_PP_` | D flip-flop, clock = PT or GCLK, CE = PT |
| `$_DFF_PP0_`/`PP1_`, `$_DFFE_PP0P_`/`PP1P_` | + AR / AP (PT or GCLR) |
| `$_DFFSR_PPP_`, `$_DFFSRE_PPPP_` | + both AR and AP (reset wins, as in the macrocell) |
| `$_DLATCH_P_`, `$_DLATCH_PP0_`/`PP1_`, `$_DLATCHSR_PPP_` | transparent latch (LE = clock path) ± AR/AP |
| `$_TBUF_` | output enable (PT or GOE) |
| `$_AND_` `$_OR_` `$_XOR_` `$_XNOR_` `$_NAND_` `$_NOR_` `$_ANDNOT_` `$_ORNOT_` `$_MUX_` `$_NMUX_` `$_AOI3/4_` `$_OAI3/4_`, `$sop`, `$_NOT_`, `$_BUF_` | logic: each gate is a node with its SOP; F7.4.3 builds the covers |

The `dfflegalize` init argument is `01`, so initial values pass through to the importer
(F7.4.2 rule 7). With `--init invert` it is `0`, and Yosys stores init-1 registers inverted.

Logic mapping (`--map`):
* `none` (default): Yosys' own gates from `techmap`. They keep the RTL structure, so a shared
  enable, load or reset stays one net with many readers.
* `abc-gates`: `abc -g AND,NAND,OR,NOR,XOR,XNOR,ANDNOT,ORNOT,MUX`.
* `abc-sop`: `abc -sop -I 40 -P 80`, ABC's two-level covers. These options were **removed in
  Yosys 0.69**, so this mode needs Yosys ≤ 0.68.

Measured (worklog 05): ABC's SOP mapping duplicates shared logic into every reader's cover
(`spi_controller_mode0`: 91 cells against 51 with gates). The gates are equal or better on all
test designs except a 16:1 mux.

Negative-edge clocks and active-low enables/resets appear as an inverter in front of the
positive cell. The importer turns that into an inverted literal, which the fitter maps onto the
GCLK invert bit or the PT polarity. One GCLK pin used on both edges: the invert bit is per
network, so the majority polarity gets the network and the rest use a PT clock.

### F7.4.2 Mapping rules (Yosys JSON → `netlist::Design`)

1. Every bit of an output/inout port is an output cell named after the port bit (`q[3]` → `q3`,
   other punctuation → `_`; the same rule as `yosys2tt2.py` and the vendor `.fit`).
2. A storage element whose Q drives a port bit, directly or through a `$_TBUF_`, lives in that
   port's macrocell (`is_output`, `pin` from constraints). Other storage elements become buried
   register cells.
3. Reading an `input` port is `Pad`; reading an `inout` port is `Pad` (pin feedback: the external
   level); reading a storage Q is `Fb(cell, true)`; reading the internal value of a combinatorial
   output is `Fb(cell, false)` (macrocell feedback), never the pad, so a tri-stated output reads its
   own logic correctly (cf. the vendor's pad-feedback defect, F7.1).
4. A `$sop` whose output is read only internally becomes a buried combinatorial node; a single
   literal `$sop` is an alias. Constants fold.
5. `$_TBUF_` data is the cell's function, its enable the `oe`. An inout port that nothing drives
   is a pure input.
6. Anything else (memories, `$alu`, `$mul`, multi-bit cells) is rejected with the cell name: the
   synthesis script must run first.
7. Initial values: the macrocell register has no power-up value fuse; this flow assumes power-up 0
   (as the simulator model does). `init` = 0 is accepted, `init` = 1 is an error unless
   `--init ignore` (then a warning). Whether silicon really powers up at 0 is still open (worklog
   04/05); if it does not, designs must reset explicitly anyway.

### F7.4.3 Optimisation before fitting (`atf15xx-synth/src/opt.rs`)

In order:
1. **Toggle recovery** (`recover_toggles`). The next-state function is analysed with buried nodes
   substituted (up to 64 products) and the clock enable folded in (`D' = e·D + !e·Q`). Then
   `T = Q ? !D'|Q=1 : D'|Q=0`. If T has fewer products than D′, the cell becomes `Storage::Tff`
   (the macrocell XOR with Q) and loses its CE. T may still read Q (synchronous load:
   `T = load·(d⊕Q) + !load·carry`). counter16: 48 → 16 MCs.
2. **Clock-enable recovery** (`recover_enables`). `dfflegalize` turns enables and synchronous
   resets into multiplexers in front of a plain DFF, which makes every bit pay for the hold path.
   Take `D0`/`D1`, the cofactors on Q, through the nodes on Q's path only. Q is held where
   `!D0·D1`. If Q never toggles (`D0 → D1`), then `CE = D0 + !D1` and `D = D0`, minimised with the
   hold region as don't-care. A multi-product CE becomes a buried node shared by every register
   with the same enable (used only when at least two share it).
3. **Limited collapse** (`collapse_limited`). A buried node is inlined into all its readers,
   smallest first, only if all of these hold:
   * every reader stays within `--max-products` (default 10: one macrocell plus one cascade
     neighbour, minus the PTs its special functions use);
   * special functions stay single products;
   * the macrocell count does not grow (a reader that outgrows its macrocell needs a cascade
     neighbour);
   * for a node with several readers, the total product count does not grow. Duplicating shared
     logic such as `!load` into every bit would expand its complement in each reader.

   * the reader's support stays ≤ `opt::MAX_SUPPORT` = 32 distinct inputs. A macrocell's PTs see
     only the 40 UIM inputs of its block, so a carry chain stays a chain of buried nodes: a
     64-bit counter is 64 registers + 23 carry nodes. The flattened analysis views of steps 1–2
     obey the same bound.

   What stays is a buried node (node number 0: the fitter must not collapse it either).
4. **Latch reset recovery** (`recover_latch_resets`). Yosys maps `if (r) q = 0; else if (le) q = d;`
   to `LE = le + r`, `D = !r·d`, which races when `r` falls. If LE has a single-literal product `s`
   and the data is constant when `s` is true, `s` becomes AR (or AP) and is removed from LE and D.
5. **Dead-node removal**, then **cover splitting** (`split_large`). Covers that need more than one
   cascade neighbour become buried OR nodes of ≤ `max_products` products, because our fitter
   cascades from one neighbour only.

Strategies: the flow fits with a sequence of strategies and keeps the first that fits:
* with or without enable recovery;
* collapse ties (a node becoming a cascade neighbour) allowed or not;
* unconstrained output pins spread over the blocks by fan-in, or left to the fitter;
* `max_products` 5 (no cascades);
* shuffled cell orders (8 seeds).

Dense designs (more cells than macrocells) first try the fitter's vendor-style collapse of
combinatorial nodes (the flow's own collapse markers are cleared for it), whose wider sums are
then factored with foldbacks (F4.6a). Placement is the fitter's weakness: it fills block A first, and its UIM repair moves one
floating cell at a time.

Cascade, foldback, logic doubling, global clock/clear/OE selection and pin placement are done by
`fit_design` (F4–F6), exactly as for `.tt2` input.

## F7.5 Constraints

Port attributes (Yosys keeps them on the port wire): `(* LOC = "43" *)` (also `PIN`, `CHIP_PIN`;
a list for a vector, LSB first), `(* FAST *)`, `(* OPEN_COLLECTOR *)`, `(* LOW_POWER *)`.
Constraints file (`-c FILE`, conventionally `.acf`), one directive per line, `#` comments:

```
device  ATF1504AS-PLCC44     # part; --device overrides
set_io  clk 43               # port bit as in Verilog (q[3]) or sanitised (q3)
set_io  q[3:0] 4 5 6 8       # ranges take pins LSB first
fast    q[0]                 # vendor -strategy output_fast = q0
open_collector irq_n
low_power q[3:1]             # vendor mc_power
option  jtag off             # any vendor -strategy KEY VALUE (pin_keep, cascade_logic, ...)
```

File entries override attributes; `--pin NAME=PIN` and `--option KEY=VALUE` override both.
Device names: Microchip ordering codes (`ATF1504AS-10JU44`: J = PLCC, A = TQFP, Q = PQFP, or the
package spelled out), the fitter name (`P1504C44`) or the CUPL name (`f1504plcc44`). The variant
(`AS`, `ASV`, `ASL`, `ASVL`) selects the JTAG IDCODE for the SVF; the fuse map is the same.
BE parts are rejected (different architecture). Outputs next to the `.jed`:
* `<top>.pins.json` maps every Verilog port bit to its pin, for test benches and the simulator's
  equivalence checker.
* `<top>.map.json` (format `atf15xx-map/1`, F5.4) maps every netlist cell and its source
  register/net name to its macrocell and `Invert` fuse, for register correspondence.

