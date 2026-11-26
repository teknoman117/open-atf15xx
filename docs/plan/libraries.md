# Libraries for the unproven parts (survey, 2026-11-25)

The user's goal: remove unproven home-grown logic by leaning on maintained libraries, first as
**oracles** that check our code, then **adopted** where they are better. There is no maintained
open-source CPLD fitter to adopt. The closest prior art is `xc2par` (Rust, Xilinx
CoolRunner-II place-and-route, from the community behind the `jedec` crate): worth reading for
its product-term allocation and routing, but it targets a different architecture.

Maintenance data from the crates.io API (2026-11-25: latest version, last update, downloads in
the last 90 days).

| library | version, licence | updated | recent dl | role here | status |
|---|---|---|---|---|---|
| `biodivine-lib-bdd` (BDDs) | 0.6.3, MIT | 2026-02 | 47 k | exact toggle analysis through carry networks (F9.8) | **adopted** |
| `espresso-logic` (Berkeley Espresso, C, bindgen) | 5.6.4, MIT | 2026-08 | 665 | oracle for `logic.rs` on small functions (`tests/oracles.rs`, dev-dependency) | **oracle only**: adoption rejected (below) |
| `petgraph` (graph algorithms) | 0.8.3, MIT/Apache | 2025-09 | 155 M | oracle for the UIM matcher (we keep ours: it reproduces the vendor's mux order) | **oracle** |
| `oxidd` (decision diagrams) | 0.13.0, MIT/Apache | 2026-09 | 14 k | faster, fuller BDD/ZDD alternative if BDD use grows | candidate |
| `good_lp` + `highs` (ILP) | 1.15.3 / 2.4.0, MIT | 2026-08 / 07 | 552 k / 300 k | exact block assignment (`fit-partition-bound`, `atf15xx-synth` feature `ilp`, F4.6c): our partition optimal on every tight case | **oracle** |
| `rustsat` (+ `cadical`) (SAT) | 0.7.5 / 0.1.16, MIT | 2026-01 / 2025-04 | 37 k / 54 k | exact seat assignment and UIM feasibility with side constraints | candidate |
| `splr` (pure-Rust SAT) | 0.19.0, MPL-2.0 | 2026-08 | 4 k | as above without C | candidate |
| `varisat` (pure-Rust SAT) | 0.2.2, MIT/Apache | 2020-09 | 382 k | unmaintained since 2020 | no |
| `metis` (METIS bindings) | 0.2.2, MIT/Apache | 2024-10 | 129 k | graph partitioning baseline for the block assignment | candidate |
| `mt-kahypar` (bindings) | 0.2.0, MIT/Apache | 2025-06 | 376 | hypergraph partitioning (the right model for block fan-in), but bindings barely used | watch |
| `egg` / `egglog` (e-graphs) | 0.11.0 / 3.0.0, MIT | 2025-12 / 2026-08 | 1.2 M / 59 k | exploring rewrites (De Morgan, factoring, XOR splits) under a macrocell cost | experimental |
| ABC (in Yosys: `yosys-abc`) | (Yosys 0.71) | | | cover-size oracle (`strash; collapse; sop` on BLIF/AIGER), `cec`/`dprove` already used | oracle (planned) |
| mockturtle (EPFL, C++ header-only) | | | | XOR-aware rewriting (XAGs, ESOP): our weak spot. No Rust bindings; through `cxx` like the Yosys bridge | candidate |

## What is proven now

* **`logic.rs`**: `Sop::minimize` and `Sop::complement` give exactly the function on 1000
  random functions of 4–10 variables, checked on exhaustive truth tables. `minimize` matches
  Espresso's cube count on 991 and is one cube larger on 9. Of the 978 functions with a
  non-empty complement, `complement` matches or beats Espresso on 597 and is larger on 381, by
  up to 10 cubes (`cargo test -p atf15xx-fit --test oracles`).
* **UIM matcher** (`route_block_alt`): on 1200 random source sets drawn from the real UIM tables
  of all three families, it finds a routing exactly when petgraph's maximum matching is complete
  (634 routable, 566 not), and every routing it returns is valid.
* **Toggle recovery through carry networks**: computed by a BDD library, not by us.

## What stays ours, and how it is checked

* The vendor-compatible fitter: byte identity on the oracle corpora (thousands of vendor fits).
* Architecture facts: the vendor oracles and the silicon bench.
* Every JED: formal equivalence (Yosys/ABC) against its source.
* Placement heuristics (partitioner, positions): correct by construction (the router checks
  them); the ILP oracle (F4.6c) shows them optimal on every tight design measured, unbalanced on
  easy ones.

## Espresso: an oracle, not a replacement

Adopting Espresso in the fitter's planner looked attractive (our complements are larger than
Espresso's on 381 of 978 random functions). Measured on the corpus, the first fit (counter16)
never finished. The cover it was given was ordinary, 15 variables and 24 cubes (a counter bit's
toggle function), and Espresso took **57.7 s** for it, growing about 8× per variable (20
variables: over 2 minutes). This is not the binding: the same cover, as a PLA file, through
Berkeley's own C program built from the bundled source (`gcc -std=gnu99`) takes the same 57.7 s.
Our minimiser handles that family of covers in microseconds up to 40 variables, and its
complement there (3 cubes) is optimal.

Then a BDD-derived cover was tried instead: the complement read off a `biodivine-lib-bdd` BDD
as an optimised DNF, made irredundant, with the planner keeping the smaller of it and ours for
each polarity. Both are exact. On the random set the BDD cover is smaller in 140 cases, larger
in 184 and equal in 676. Measured on every corpus netlist (gates and words) and SELFPROGRAMBOARD,
**it changed nothing**: 70 fits, 1260 macrocells either way, identical fmax. Real covers are
small and structured, and there our complements are already minimal. The option was removed
again, since it would add code and runtime for no measured gain.

**Outcome:** our minimiser and matcher stay, now with machine-checked evidence (exact on random
functions, quality within one cube of Espresso for `minimize`, matching-optimal routing). The
known weakness (`complement` on random functions) is measured and has no effect on real
designs.
