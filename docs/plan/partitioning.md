# Hypergraph partitioning for the block assignment (2026-11-26)

User request: "explore literature surrounding hyper-partitioning to see if that might help the place
cells load". This note maps our placement problem onto the partitioning literature, says what the
fitter's time actually goes to, and recommends what to adopt. Evidence levels as in the specs:
*measured* (our runs), *external* (cited).

## 1. Our problem in partitioning terms

`place_cells` assigns the floating cells to 2/4/8 blocks of 16 macrocells (`partition_blocks`, spec
F4.6a/F9.4). As a hypergraph:

| hypergraph partitioning | ATF15xx block assignment |
|---|---|
| vertex, vertex weight | cell; 1 + its cascade helpers |
| hyperedge (net) | a UIM source: pad, feedback, dedicated input (`uim_ids`) |
| k blocks, block capacity | 2/4/8 blocks, 16 macrocells minus pinned cells and reserved helpers (individual block weights) |
| fixed vertices | cells pinned by pin / node constraints |
| per-block **pin constraint** | at most 40 distinct sources per block (UIM fan-in) |
| — (no analogue) | the UIM *matching*: each of a block's 40 multiplexers reaches only some sources |

The pin constraint is the classic multi-FPGA formulation: blocks limited by logic size *and* by the
nets incident to them, as opposed to plain cut minimisation ([UCSD, FPGA partitioning](https://vlsicad.ucsd.edu/Publications/Conferences/40/c40.pdf)).
The matching constraint has no counterpart in the generic literature; it is what makes a CPLD fit
fail below 40 inputs (spec F4.6).

## 2. What the literature offers

* **Pin-constrained move-based partitioning (FM family).** Woo and Kim adapt Fiduccia–Mattheyses
  to k-way partitioning that minimises total partition pins under size and pin constraints, and
  commercial FPGA partitioners are mostly FM-style ([UCSD](https://vlsicad.ucsd.edu/Publications/Conferences/40/c40.pdf));
  Kuznar et al. make the pin count, not the net cut, the target ([US 5675500](https://patents.google.com/patent/US5675500A/en));
  Krupnova and Saucier apply iterative improvement to multi-way partitioning with size and pin
  limits per device ([DATE 99](https://past.date-conference.com/proceedings-archive/1999/DATE99/pdffiles/09b_3.pdf)).
  **Our `partition_blocks` is already this family**: greedy seed, then moves and swaps on a
  lexicographic cost (inputs over 40, the largest block, the sum).
* **Multilevel partitioners (hMETIS, KaHyPar, Mt-KaHyPar).** Mt-KaHyPar supports fixed vertices,
  individual block weights and the connectivity objective km1 = Σ(λ(e) − 1)
  ([Mt-KaHyPar](https://github.com/kahypar/mt-kahypar); [paper](https://arxiv.org/pdf/2010.10272.pdf)).
  km1 equals the sum over blocks of incident nets minus the number of nets, so it minimises the
  *total* block fan-in, not the per-block maximum we must bound. Multilevel coarsening exists for
  instances of 10⁴–10⁸ vertices; ours have at most 128 cells in 8 blocks. RePart (2026) notes that
  modelling device limits through the imbalance factor ε underuses them ([arXiv 2604.00780](https://arxiv.org/abs/2604.00780)).
* **Replication.** Duplicating logic into the blocks that read it lowers pin demand: Kring and
  Newton extend FM with explicit node duplication, after Charney and Plato
  ([replication cut](https://cseweb.ucsd.edu/classes/fa23/cse248-a/papers/partition/A_replication_cut_for_two-way_partitioning.pdf));
  FPGAs are more pin-limited than logic-limited, which replication addresses
  ([UWO notes](https://www.eng.uwo.ca/people/wwang/ece616a/616_extra/notes_web/8_dmultiFPGAsystem.pdf));
  RePart builds replication into a multilevel partitioner ([arXiv 2604.00780](https://arxiv.org/html/2604.00780)).
* **CPLD-specific work.** TEMPLA maps onto PLA-style blocks by partial collapsing rather than
  library matching ([Anderson, Brown, DAC 98](https://www.eecg.utoronto.ca/~brown/papers/dac08-anderson.html));
  Manohararajah, Brown and Vranesic partition before place and route with Rent's rule and
  simulated annealing ([FPL 02](https://www.eecg.utoronto.ca/~brown/papers/fpl02-manohararajah.html)).
  Neither models a routing matrix with partial reachability like the UIM.

## 3. Where `place_cells`' time goes (measured)

* SELFPROGRAMBOARD (dense, ATF1504AS), frame-pointer profile 2026-11-26: the partitioner's
  `Counts::add`/`remove` are 6–12 % of a 1.5 s fit; `fanin_count` and `place_cells` ~6 %; the
  rest is cover minimisation and allocation. The partitioner is not the bottleneck there, and the
  exact ILP (spec F4.6c) shows its assignment is optimal on the dense designs (38 = 38).
* The 18-bit registered compare (ATF1504AS-TQFP100), 34 s: three strategies each spend ~11 s in the
  **UIM repair loop**: the matching fails after a full placement, one floating node is moved out
  of the congested block, and the *whole attempt* is refitted, up to 96 times (~0.1 s each).

So the load comes from the outer loop, not from the quality or speed of the partition itself: the
matching constraint (the row with no analogue in section 1) is checked only after placement.

## 4. Recommendations

1. **Put the UIM matching into the partitioner's cost** (the CPLD-specific gap): evaluate moves on
   `uim_unmatched` per block (a bipartite matching of ≤ 40 muxes against ≤ ~60 sources, cheap
   incrementally) instead of fan-in counts alone, so the local search avoids unmatchable blocks
   and the repair loop's full refits become rare. Expected: the 11 s repair loops drop to the
   partitioner's own time.
2. **Replication moves** (Kring–Newton style): let the search duplicate a buried combinatorial
   node into a reading block when that lowers the block's unmatched or fan-in count. This is the
   mapping–placement loop's "duplicate a node per block" (spec F9.4 item 3, F9.7 item 3), now with
   literature behind it.
3. **Multilevel partitioners: not as a dependency.** At ≤ 128 vertices coarsening buys nothing, km1
   optimises the sum rather than our per-block bound, the matching is not modelled, and
   Mt-KaHyPar would add a C++/TBB build for little-used bindings (`docs/plan/libraries.md`). Like
   Espresso and HiGHS, it could serve as an *oracle* in tests (compare our assignments' total
   fan-in on the corpus) if that question ever matters.
4. **Exact methods stay viable at our scale**: the ILP of `fit-partition-bound` (F4.6c) could grow
   the matching constraints and run, time-bounded, as a last strategy for designs the heuristics
   fail on, which would also prove infeasibility where it holds.

Sources: [UCSD FPGA partitioning](https://vlsicad.ucsd.edu/Publications/Conferences/40/c40.pdf),
[Krupnova & Saucier, DATE 99](https://past.date-conference.com/proceedings-archive/1999/DATE99/pdffiles/09b_3.pdf),
[US 5675500 (multi-chip partitioning)](https://patents.google.com/patent/US5675500A/en),
[Mt-KaHyPar](https://github.com/kahypar/mt-kahypar), [Mt-KaHyPar paper](https://arxiv.org/pdf/2010.10272.pdf),
[RePart, arXiv 2604.00780](https://arxiv.org/abs/2604.00780),
[Replication cut](https://cseweb.ucsd.edu/classes/fa23/cse248-a/papers/partition/A_replication_cut_for_two-way_partitioning.pdf),
[UWO multi-FPGA notes](https://www.eng.uwo.ca/people/wwang/ece616a/616_extra/notes_web/8_dmultiFPGAsystem.pdf),
[Anderson & Brown, TEMPLA](https://www.eecg.utoronto.ca/~brown/papers/dac08-anderson.html),
[Manohararajah, Brown, Vranesic, FPL 02](https://www.eecg.utoronto.ca/~brown/papers/fpl02-manohararajah.html).
