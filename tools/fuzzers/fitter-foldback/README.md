# fitter-foldback: foldback expanders and cascades

**Derives:** where a foldback (NAND) expander term sits (which PT column feeds it back into the
UIM) and in which direction a cascade (PT sharing with the neighbouring macrocell) runs, for all
families. Spec F2.6.

**Method:** `fold`: a node `N = !(A & B)` placed at foldback node 300+m and used by an output in
the same block; `cas`: an output that is the sum of 9 two-literal products (more than 5 PTs, so
the fitter must borrow from a neighbour). The vendor JEDEC file shows which PT rows and which
cascade bits the fitter used.

## Run

```
python3 tools/fuzzers/fitter-foldback/exp_fold.py
python3 tools/fuzzers/fitter-conformance/verify_tt2.py --bin-dir $PWD/target/release .scratch/oracle/fold
```

Needs `../fitter-pins`, `../fuse-regions`, and `../fitter-macrocell/exp_feat.py` (it reuses its
device list and target pins).

Writes `.scratch/exp/fold_<fam>.json` and `.scratch/oracle/fold/`. Worklog: `docs/worklog/02-fitter.md`.
