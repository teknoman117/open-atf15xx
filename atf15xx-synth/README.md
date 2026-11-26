# atf15xx-synth — open Verilog → ATF15xx flow

```
design.v ──Yosys (data/synth_atf15xx.ys)──► netlist.json ──yosys::import──► atf15xx_fit::netlist::Design
   ──opt: limited collapse, toggle recovery──► atf15xx_fit::fit_design ──► design.jed (vendor JEDEC format)
   ──atmisp::generate──► design.svf (Program/Verify, as ATMISP 7.3 writes it)
```

All open source: Yosys (ISC), this workspace's fitter crates and `atf15xx-tool`. The vendor
fitter and ATMISP are only oracles. Spec: `docs/architecture/fitter/07-yosys-flow.md` (§F7.4
mapping rules, §F7.5 constraints). Work log: `docs/worklog/05-verilog-flow.md`.

## Usage

```sh
docker build -t open-atf150x-yosys docker/yosys/          # pinned Yosys v0.71 + iverilog + Rust
docker/yosys/run.sh cargo run --release -p atf15xx-synth -- \
    build design.v --device ATF1504AS-PLCC44 -c design.acf -o out/
# out/<top>.jed  .svf  .fit (fit report)  .pin  .pins.json (port bit -> pin)  [.tt2 with --emit-tt2]
```

Any Yosys from 0.33 to 0.71 works (`--yosys PATH`). `fit-json netlist.json` skips Yosys for a
netlist made with `atf15xx-synth script` (the bundled synthesis script).

| option | meaning |
|---|---|
| `-d, --device` | `ATF1502AS-PLCC44`, `ATF1504ASV-TQFP100`, `ATF1508AS-10JU84` style ordering codes, fitter names (`P1504C44`) or CUPL names (`f1504plcc44`) |
| `-c FILE` | constraints file (`device`, `set_io`, `fast`, `open_collector`, `low_power`, `option`) |
| `--pin q[3]=8` | pin of one port bit (repeatable) |
| `--option jtag=off` | any vendor fitter strategy option (`-strategy KEY VALUE`), repeatable |
| `--max-products N` | product terms per cell incl. one cascade neighbour, for inlining and splitting (default 10; the flow also tries 5) |
| `--no-toggles` | keep counters as D flip-flops |
| `--init error\|ignore\|invert` | Verilog initial values of 1 (default: error; `invert`: Yosys stores them inverted; power-up 0 assumed) |
| `--map none\|abc-gates\|abc-sop` | logic mapping in Yosys (default: Yosys' gates; `abc-sop` needs Yosys ≤ 0.68) |
| `--emit-tt2` | also write the netlist as `.tt2` for the vendor fitter / `fit150x` (oracle) |
| `--no-svf`, `--svf-rev-c`, `--timestamp` | SVF control, reproducible JEDEC/SVF date text |

Verilog attributes on ports: `(* LOC = "43" *)`, `(* LOC = "4 5 6 8" *)` (vector, LSB first),
`(* FAST *)`, `(* OPEN_COLLECTOR *)`, `(* LOW_POWER *)`.

## Tests

`cargo test -p atf15xx-synth` (unit tests). End-to-end with formal equivalence against the source
Verilog: `tools/sim/run.sh python3 tools/flow/corpus.py` over `tests/designs/*.v` on
1502/1504/1508 (see `tools/README.md`).

Debugging: `ATF15XX_FLOW_DEBUG=1` prints the strategies, the per-cell covers and the optimisation
decisions. `ATF15XX_FLOW_TRACE=cell` follows one cell through collapse. `ATF_FIT_DEBUG=1` shows
the fitter's PT demand, UIM routing failures and node placement.
