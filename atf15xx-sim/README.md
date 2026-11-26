# atf15xx-sim

Functional model, cycle simulator and Verilog emitter for ATF1502AS / ATF1504AS / ATF1508AS
JEDEC fuse maps. It is used to check that a fitted `.jed` implements its source design
(`tools/sim/equiv.py`).

Specification: `docs/architecture/sim/` (model and options: 01, emitter: 02, equivalence
method: 03, limitations: 04, harness guide: 05). Results: `docs/worklog/04-simulator.md`.

## CLI

```
atf15xx-sim <COMMAND> JED -d DEVICE [model options]

  verilog    JED → synthesizable Verilog (split pN_in/pN_oe/pN_out interface; --pins-wrapper adds inout pins)
  equations  human-readable product terms and configuration of every used macrocell
  run        simulate random (--cycles, --seed) or CSV (--stim) stimulus; --print CSV, --vcd waveform
  compare    simulate two JEDs on the same random stimulus; PASS, or FAIL (exit 2) with --vcd

model options: --preset-wins  --power-up 0|1|fuse  --cascade-without-pts  --lender-a-is-one
               --clock-idle-mcs  --oe-pt5-ungated
```

`atf15xx-sim --help` and `atf15xx-sim <COMMAND> --help` describe every option. DEVICE is a
fitter name (`P1504C44`) or a CUPL name (`f1504plcc44`).

## Library

```rust
let model = atf15xx_sim::load(Path::new("x.jed"), "P1504C44", Options::default())?;
let mut sim = atf15xx_sim::sim::Sim::new(&model);
sim.step(&BTreeMap::from([(43, true)]));          // pin 43 high, others unchanged
let drive = sim.outputs();                         // pin -> PinOut { oe, value }
let v = atf15xx_sim::verilog::emit(&model, &EmitOptions { module: "dut".into(), pins_wrapper: false });
```

Depends on `atf15xx-arch` (device database and `decode`) only; it has its own JEDEC reader
(`parse_jed`).

## Tests

`cargo test -p atf15xx-sim`: every fitter test vector loads, simulates and emits; simulation is
deterministic; fuse mutations that change the equations change the simulated behaviour; bad
JEDEC input is rejected. The cross-checks against iverilog and the formal flow are in
`tools/sim/` (05-harness.md).
