# fitter-cascade: cascade chains for wide sums

**Derives:** how the vendor fitter implements a sum of more product terms than one macrocell has
(5): how many cascade helpers it takes, where they are, and the longest chain it builds. Spec
F4.3 (`docs/architecture/fitter/04-fitter-algorithm.md`); the fitter's capacity model
(`atf15xx-fit/src/capacity.rs`) is built on it.

**Method:** Y = OR over i < N of (S == i) & D_i with a 6-bit select S and N data inputs: N
products with no common factor and a far larger complement, so the sum needs exactly N terms.
Y combinatorial or a D flip-flop (clock on GCLK1), floating or pinned mid-block (TQFP100 pin 46
= MC38 on the ATF1504AS), N = 6 … 40. The vendor JEDEC file is decoded with our simulator
(`atf15xx-sim stats` / `equations`): the macrocells holding Y's terms and the cascade links. The
same designs check our fitter: `../fitter-conformance/verify_tt2.py` (fuses) and
`tools/sim/check_design.py` (our JED and the vendor's against the `.tt2`, formally).

| script | asks | writes (`.scratch/`) |
|---|---|---|
| `exp_cascade.py [FAMILY]` | N-term sums, 4 variants × 14 widths | `exp/cascade_<fam>.json`, `oracle/cascade/` |

## Run

```
python3 tools/fuzzers/fitter-cascade/exp_cascade.py 1504
python3 tools/fuzzers/fitter-conformance/verify_tt2.py --bin-dir $PWD/target/release .scratch/oracle/cascade
for d in .scratch/oracle/cascade/*/; do n=$(basename $d); \
  tools/sim/run.sh python3 tools/sim/check_design.py /work/$d$n --budget 30; done
```

## Results (ATF1504AS, 2026-11-26)

* Every width 6–40 fits in all four variants.
* **6–30 terms: a cascade chain**, one helper per further 5 terms (6–10: 1 helper, 11–15: 2, 16–20:
  3, 21–25: 4, 26–30: 5), the helpers directly below the cell in its block (12 terms on MC38:
  helpers MC37 and MC36; 16 terms on MC16: MC15, MC14, MC13).
* **31–40 terms: no cascade at all**; the vendor builds the sum from more product terms in
  other macrocells (31 terms: 38 product terms, no cascade link). The longest chain the vendor builds is
  therefore 6 macrocells; the fitter uses the same limit (`MAX_CHAIN_MCS`).
* Our fitter (with chains, 2026-11-26) against the vendor: every design that we fit (6–25 terms,
  44) is PROVEN equivalent to its `.tt2`, as is every vendor JED; the fuses differ because our
  input pins differ (the vendor's order of input placement is not yet modelled: it places D0
  last), so these designs are not fuse-identical. 30 terms: our UIM routing fails (36 inputs in
  one block) where the vendor fits; 31–40: ours fails (sums from nodes not implemented in the
  `.tt2` path).
