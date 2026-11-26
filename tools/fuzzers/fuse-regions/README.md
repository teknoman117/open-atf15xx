# fuse-regions: the names and extents of the AS fuse regions

**Derives:** `.scratch/re/regions.json`, the layout of the AS-family JEDEC arrays as ATMISP labels
it: each product-term row (PT n of MC m), each macrocell-configuration field (S a, S b of block
X), each UIM mux field, the global-OE, configuration, special-purpose, UES and reserved fields,
with address and length. The fitter experiments use it to know where to look in a vendor JEDEC
file.

**Method:** ATMISP's `Load` instruction, run in SVF mode on a device with no JEDEC file, writes
the blank image as a JEDEC file whose NOTE comments label every region (spec §8). `gen_cases.py`
makes one such case per AS family; `regions.py` reads the NOTE labels and the field lengths.
ATMISP's text numbers the product-term rows differently from the fitter (in the 1508, a quarter of
the row groups even carry another macrocell number), so the (macrocell, PT) of each row comes from
atf15xx-arch (`Family::pt_addr`, equal to prjbureau's `pterm_ranges`; printed by `atf15xx-sim fuses`) and is checked
against every row the fitter itself labels in its JEDEC files (`../fitter-macrocell/vendor_labels.py`).

## Run

```
python3 tools/fuzzers/fuse-regions/gen_cases.py .scratch/fuzz/fuse-regions/cases.json
python3 tools/fuzzers/common/atmisp_oracle.py .scratch/fuzz/fuse-regions/cases.json .scratch/fuzz/fuse-regions/out
python3 tools/fuzzers/fuse-regions/regions.py .scratch/re/regions.json .scratch/fuzz/fuse-regions/out \
    [.scratch/re/vendor_labels.json]
```

Three ATMISP runs, under a minute. `regions.py` prints per family the number of PT rows, mcfg
fields and UIM fields and the total length, which must equal the family's QF.

## Results (2026-11-25)

All three families total their QF; the PT numbering agrees with the fitter's labels on all 160 /
305 / 536 rows it labels; the result is identical to the `regions.json` the experiments were run
with.
