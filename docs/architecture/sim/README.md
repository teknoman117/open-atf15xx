# ATF15xx JED simulation and equivalence checking

These pages specify how a fitted ATF1502AS/1504AS/1508AS `.jed` is turned into an executable
model and how that model is checked against the design it was fitted from. The goal: Verilog
simulation and synthesis are solved problems, so a fitted JED is correct if it is *equivalent*
to its source design at the device pins.

| page | contents |
|---|---|
| [01-model.md](01-model.md) | the functional model built from the fuse map, its evidence levels and options, the cycle simulator |
| [02-verilog.md](02-verilog.md) | the JED → Verilog emitter: interface, naming, semantics in Verilog |
| [03-equivalence.md](03-equivalence.md) | the equivalence method: gold models (.tt2, Verilog, JED), miter, formal (PDR/BMC/induction), simulation, constraints |
| [04-limitations.md](04-limitations.md) | what is inferred, what is not covered, known tool quirks |
| [05-harness.md](05-harness.md) | how to run everything (commands, outputs, reading a counterexample) |
| [06-verilator.md](06-verilator.md) | time-bounded Verilator engine, register correspondence, state injection, guarantee levels, budgets, Verilator limitations |

Implementation: `atf15xx-sim` (model, simulator, emitter; CLI `atf15xx-sim`) and
`tools/sim/` (Python drivers around Yosys, ABC, Icarus Verilog). Results and the history of the
decisions are in `docs/worklog/04-simulator.md`.

The model is built on `atf15xx_arch::decode` (fuse map → structural netlist,
`docs/architecture/fitter/02-fuse-map.md`) and follows its functional semantics table §F2.8.
