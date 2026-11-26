# SELFPROGRAMBOARD (P80C550-SPM-CPLD): a real design as an example and benchmark

Source: <https://github.com/teknoman117/P80C550-SPM-CPLD> @ `28f54f3a70aab4ea67e1fbe9c610cf5f14592d08`
(`SELFPROGRAMBOARD.pld`, the user's design, MPL-2.0 there; the port here is ISC like the rest of
the project). `selfprogramboard.v` is a line-by-line Verilog
port; `selfprogramboard.acf` reproduces the original build settings (`compile.bash`).

| | result |
|---|---|
| vendor (WinCUPL → fit1504 1.8.7.8) | fits: **64/64** MCs, 32/32 I/O, 4/4 dedicated inputs, 47/64 foldbacks; the user reports it works in hardware |
| vendor JED vs its `.tt2` | match (random simulation against the `.tt2`; equiv.py with the silicon-corrected cascade semantics) |
| Verilog port vs the vendor JED | no difference in 23 formal steps (sat; loops prevent induction) + 4000 simulated steps; `equiv.py … --single-change --reset p1=0:2` |
| our fitter on the vendor `.tt2` | fits (2026-11-26): **60/64** MCs, 180 PTs, 1 foldback, fmax 55.6 MHz at -10 (the vendor's: 64 MCs, 191 PTs, 47 foldbacks, 50 MHz; `tools/flow/jed_stats.py`); 2026-11-24 it did not (UIM routing of block A, 46 inputs > 40) |
| our Verilog flow | fits (2026-11-25): **60/64** MCs, fmax 52.6 MHz at -10 (`atf15xx-synth build`, below); on 2026-11-24 it did not (77 cells for 64 MCs) |

Checking it reproduced two harness issues and one model error, all fixed 2026-11-24: vector ports
with a non-zero LSB (`input [15:10] A`) were not mapped by `tools/sim/vsplit.py`; the formal engine
let a clock pin and its data change in the same step (`--single-change` now gives it the
simulator's input model); and the cascade-lender XOR semantics (silicon: a macrocell with
`CasMux` = 1 sees A = 1 on its own XOR).

```
docker/yosys/run.sh cargo run --release -p atf15xx-synth -- build examples/spm/selfprogramboard.v \
    -c examples/spm/selfprogramboard.acf -o .scratch/examples/spm
tools/sim/run.sh python3 tools/sim/equiv.py VENDOR.jed --device P1504C44 \
    --verilog examples/spm/selfprogramboard.v --top selfprogramboard --fit VENDOR.fit \
    --single-change --reset p1=0:2 --budget 600
```
