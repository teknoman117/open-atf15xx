# fitter-pterms: product-term columns and the UIM inputs behind them

**Derives:** the product-term array's column layout: which column of a PT row is which UIM mux
output in which polarity, for every block of every family, and (by saturating one block) the mux
indices the fitter only uses under pressure. Spec F2.3–F2.4 (`docs/architecture/fitter/02-fuse-map.md`);
prjbureau's database holds the same facts and is the source of atf15xx-arch's fuse map since 2026-11-25, so
this is the independent oracle evidence for it.

**Method:** one-literal designs. Output Y (pinned in block b) = S or !S for every source S (each
I/O pin as an input, each macrocell feedback through a preassigned node): the vendor JEDEC file
has exactly one programmed fuse in Y's PT row, at the column of (S's mux, polarity); the `.fit`
report names the UIM mux the fitter chose.

| script | asks | writes (`.scratch/`) |
|---|---|---|
| `exp_ptcol.py` | one literal of one input per output | `exp/ptcol_<fam>.json`, `oracle/ptcol/` |
| `exp_ptcol2.py` | every source × every block | `exp/ptcol2_<fam>.json`, `oracle/ptcol2/` |
| `exp_ptcol3.py` | one block's UIM saturated; solve column ↔ (mux, polarity) | `exp/ptcol3_<fam>.json`, `exp/ptcolsol_<fam>.json` |

## Run

```
python3 tools/fuzzers/fitter-pterms/exp_ptcol.py
python3 tools/fuzzers/fitter-pterms/exp_ptcol2.py
python3 tools/fuzzers/fitter-pterms/exp_ptcol3.py
python3 tools/fuzzers/fitter-conformance/verify_tt2.py --bin-dir $PWD/target/release .scratch/oracle/ptcol2
```

Needs `../fitter-pins` (`exp/pinmap.json`) and `../fuse-regions` (`re/regions.json`).

## Results

`exp_ptcol3.py` solves every column of every family with no contradiction; our fitter
(fuse map generated from prjbureau's database) reproduces the vendor JEDs of the `ptcol2` corpus fuse for fuse:
1502 246, 1504 1003, 1508 3522 (2026-11-25). Worklog: `docs/worklog/02-fitter.md`.
