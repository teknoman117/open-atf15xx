# fitter-macrocell: macrocell and global configuration bits

**Derives:** which configuration bits belong to which macrocell and in which order (`exp_mcpos`,
`exp_dir`), and what every macrocell and global option bit means (`exp_feat`: register type,
clock / reset / preset / OE sources and polarities, fast input, open collector, slew, power-down,
JTAG enable, keeper, security…), plus the vendor's own name for every field (`vendor_labels`).
Spec F2.5–F2.8. The `exp_feat` designs are also the fitter's conformance vectors
(`atf15xx-fit/tests/vectors/<family>/`, 96 per family), which our fitter must reproduce fuse for
fuse.

**Method:** the same tiny design fitted with one option changed at a time; the bits that change
in the vendor JEDEC file, relative to a baseline, are that option's encoding. `exp_mcpos` moves
one output across every pin and records which configuration bits follow it.

| script | asks | writes (`.scratch/`) |
|---|---|---|
| `exp_mcpos.py` | the config bits that move with the macrocell | `exp/mcpos_<fam>.json`, `oracle/mcpos/` |
| `exp_dir.py` | physical direction of each field pair | `exp/dir_<fam>.json`, `oracle/dir/` |
| `exp_feat.py` | every option, all families | `exp/feat_<fam>.json`, `oracle/feat/` |
| `vendor_labels.py` | NOTE labels of every vendor JEDEC file in the corpus | `re/vendor_labels.json` |

## Run

```
python3 tools/fuzzers/fitter-macrocell/exp_mcpos.py
python3 tools/fuzzers/fitter-macrocell/exp_dir.py
python3 tools/fuzzers/fitter-macrocell/exp_feat.py
python3 tools/fuzzers/fitter-macrocell/vendor_labels.py
python3 tools/fuzzers/fitter-conformance/verify_tt2.py --bin-dir $PWD/target/release .scratch/oracle/feat
```

Needs `../fitter-pins` and `../fuse-regions`.

## Results

`feat` corpus, fuse-identical with our fitter: 1502 1591/1615, 1504 2146/2153, 1508 1777/1915
(the rest differ only in placement choices and are functionally equivalent, see
`../fitter-conformance`); vendor labels 268 / 517 / 956 addresses, no conflicting label
(2026-11-25). Worklog: `docs/worklog/02-fitter.md`.
