# 04 — Limitations

## L1 Model (inferred semantics)

* (2026-11-24: a macrocell whose register takes D from the pad (`DMux` = `OMux` = 0) is in use even
  without PTs; both models used to treat it as idle.)
* (2026-11-24: cascade lenders: the XOR of a macrocell with `CasMux` = 1 sees A = 1 (silicon);
  this is now the default in both models (`--lender-uses-own-sum` restores the old reading).
  `equiv.py --single-change` restricts the formal engine to one input change per step, like the
  simulator, to exclude races between a clock pin and its data. `vsplit.py` maps vector ports
  with a non-zero LSB correctly.)
* (2026-11-24: reset-over-preset priority and the register value after programming (0, independent of
  `Invert`) are now **silicon-verified**, fitter F2.9; cold power-up is still untested.)
* Reset-over-preset priority, register power-up (0, or the `Invert`/`reset` fuse) and the OE = PT5
  gating (01-model.md rules 9, 12, 14, §M7) are inferred; they are options so a check can be
  repeated under the other reading. A design that relies on its power-up state (no reset) is
  only checked relative to the chosen reading.
* The power-up diagnosis (05-harness.md) is bounded (60 steps of the counterexample's inputs).
* No timing: no delays, no glitches, no setup/hold. Two inputs changing "together" are
  unordered; a design that depends on their order is not checked by this flow.
* Combinational loops that oscillate have no defined result (the simulator flags the step).
* Not modelled: the PD1/PD2 power-down pins, pin-keeper / pull-ups (`-pin_keep`), slew and
  low-power bits, the security fuse, JTAG/ISP behaviour (JTAG pins are simply absent when JTAG is
  enabled), the input transition detector (ITD) bits.
* Bus hold and open-drain pull-ups are outside the device: an undriven pin's level is whatever
  the stimulus says.

## L2 Formal flow

* Loops force the slower `sat` path, which is usually bounded only; PDR cannot handle loops.
* `--assume` is trusted: a wrong constraint can hide real differences.
* `vsplit.py` handles what Yosys elaborates to flip-flops, latches, logic, `$tribuf` and
  memories mapped to registers. Multiple drivers on one port bit (other than the merged tri-states)
  and `inout` ports that are not driven by a tri-state are unsupported. X/Z constants in logic
  become 0 (`setundef -zero`).
* `tt2v.py` implements the documented `.tt2` subset (03-equivalence.md §E2). Unknown suffixes are
  ignored silently. Check new CUPL features before trusting a PASS.
* Unused pins: a JED that drives a pin the source does not use is a FAIL. Vendor JEDs never did
  so in the tested set (with the OE gating of rule 12).

## L2b Correspondence and injection (06-verilator.md)

* Injection needs a complete correspondence. Fits without a `.map.json` whose registers have no
  report name (merged or duplicated registers) and never toggle in calibration stay incomplete.
  Injection is then off and the report says so.
* A mismatch from an injected state is a real difference on corresponding states, but that state
  may be unreachable in designs with state invariants.
* `dprove` proves only what its own correspondence makes inductive. Designs whose JED encodes
  state differently (one-hot vs binary) fall through to PDR/BMC.

## L3 Simulation flow

* Random stimulus can miss deep or rare differences (mutation test: 3 of 25 mutants of the
  counter were found only by formal).
* The source Verilog written back by Yosys uses blocking assignments for latches. With
  single-change stimulus this cannot race; with `cosim.py --burst` it can.
* iverilog spins forever on an oscillating loop; the harness times out after 300 s and reports
  ERROR. Use `--assume`.

## L4 Environment

* Yosys 0.33, ABC (yosys-abc), Icarus Verilog 12.0, Verilator 5.020 from Ubuntu 24.04 in the
  `open-atf150x-sim` image (`docker/sim/Dockerfile`). SymbiYosys is not used (not packaged);
  the PDR/BMC/SAT calls are made directly.
