# fitter-timing: the speed-grade timing tables

**Derives:** `atf15xx-arch/data/timing.json`, the timing-model parameters (tIN, tUIM, tLAD, tSU,
tCO paths, …) per family and speed grade (-7/-10/-15/-20/-25) in whole nanoseconds, as the
vendor fitters use them, and the rules by which the fitters compose them into path delays (spec
F8, `docs/architecture/fitter/08-timing.md`). `atf15xx-sim`'s timing analyser reads the table.

**Method:** two public sources, one black box.

1. *Datasheets [E]* (not committed; Microchip's public PDFs, converted with `pdftotext -layout`):
   ATF1502AS(L) Atmel-0995L (-7/-10/-25), ATF1502ASV 1615J (-15/-20), ATF1504AS(L) Atmel-0950P
   (-7/-10/-25), ATF1508AS(L) (all five grades). Values are truncated to whole ns, as the fitters'
   output shows they are (0.5 → 0, 7.5 → 7).
2. *The fitters' SDF output [O]*: `sdf_corpus.sh` re-fits 52 feature-diverse designs at every grade
   with `-strategy Verilog_sim sdf`. A delay that is one datasheet parameter (AND = tLAD, cascade
   OR = tPEXP, output buffer = tOD1 / tXZ, register = tRD / tCLR / tPRE / tEN / tSU / tH) fixes that
   parameter for the fitter; where it differs from the datasheet the fitter's value is used.
3. `derive_timing.py` merges both, records the evidence of every value (`datasheet`, `sdf`,
   `inferred:<family>` for a grade no datasheet of the family covers) and fails unless every SDF
   delay equals a composition of the datasheet timing model (`check_sdf_timing.py`'s table).
   `timing_paths.py` checks that the model parameters compose to the datasheets' pin-to-pin specs.

## Run

```
docker/prjbureau/run.sh sh tools/fuzzers/fitter-timing/sdf_corpus.sh       # ~15 min, ~260 fits
cd .scratch/timing     # the datasheet texts: pdftotext -layout <datasheet>.pdf ATF15xxAS.txt
python3 ../../tools/fuzzers/fitter-timing/derive_timing.py --corpus corpus \
    --datasheet 1502:7,10,25=ATF1502AS.txt --datasheet 1502:15,20=ATF1502ASV.txt \
    --datasheet 1504:7,10,25=ATF1504AS.txt --datasheet 1508:7,10,15,20,25=ATF1508AS.txt \
    --compare ../../atf15xx-arch/data/timing.json -o ../../atf15xx-arch/data/timing.json
python3 ../../tools/fuzzers/fitter-timing/check_sdf_timing.py ../../atf15xx-arch/data/timing.json corpus
python3 ../../tools/fuzzers/fitter-timing/timing_paths.py
```

Datasheet sources (2026-11-25):
`https://ww1.microchip.com/downloads/en/DeviceDoc/Atmel-0995-CPLD-ATF1502AS(L)-Datasheet.pdf`,
`https://ww1.microchip.com/downloads/en/DeviceDoc/doc1615.pdf` (ATF1502ASV), and the ATF1504AS(L)
and ATF1508AS(L) datasheets from microchip.com.

## Results (2026-11-25)

6,895 of 6,895 SDF delays (52 designs × 5 grades) explained by the derived table. Evidence of the
810 values: 608 datasheet, 120 SDF, 82 inferred (the 1504AS -15/-20 columns from the 1508AS, six
from the 1504AS). Compared with the table used before (which was read out of the fitter
binaries), 12 values differ, all parameters that the SDF output never shows: the 1502 -25 column
of tEN, tIC, tFH, tFSU, tPEXP, tRPA, tZX1–3 (datasheet 1 ns slower), 1502 tCNT/tACNT at -20
(16 vs 17) and 1504 tPD2 at -25 (25 vs 20). The new table follows the datasheets.
Worklog: `docs/worklog/02-fitter.md`.
