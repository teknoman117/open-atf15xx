# F1. ATF15xxAS architecture as seen by the fitter

Sources: the Atmel datasheets (*ATF1504AS(L)* Atmel-0950P, *ATF1508AS(L)* 0784P, *Using the ATF1500
CPLD*, DOC2310) for the block diagram and pinouts, and oracle experiments against the vendor
fitters for everything that the datasheets leave out (numbering, pin ↔ macrocell maps, which pin
feeds which global network). Fuse-level detail is in [F2](02-fuse-map.md).

## F1.1 Structure

```
            pads (I/O pins, dedicated inputs)          macrocell feedbacks
                    │                                         │
                    └───────────────┬─────────────────────────┘
                                    ▼
          ┌──────────── UIM: 40 muxes per block, each picks 1 of w inputs ────────────┐
          ▼                                                                            ▼
   block A: 16 MCs × 5 PTs over 40 true/complement UIM lines + 16 foldback lines   block B …
          │  per MC: PT1..PT5 → sum (OR) / special functions → XOR → D/T/latch → OE → pin
          └── feedback (comb or register) → UIM;  PT1 → foldback line (NAND) of the block
```

* **Logic block** = 16 macrocells. 1502AS: blocks A–B (32 MCs); 1504AS: A–D (64); 1508AS: A–H (128).
  Macrocell `MCn` (1-based) is in block `(n−1) div 16`, local index `(n−1) mod 16`.
* **Product terms.** 5 per macrocell (PT1…PT5). Each PT can use the true or complement of each of
  the block's 40 UIM lines and the 16 foldback lines of its block.
* **Special functions** (each takes one PT, or the PT is added to the sum instead):
  PT1 → foldback expander; PT2 → XOR input (or fast D input); PT3 → asynchronous reset;
  PT4 → clock or clock enable; PT5 → asynchronous preset or output enable.
* **Cascade.** A macrocell can pass its whole sum to the next macrocell (`MCn → MCn+1`, same block
  only), giving > 5 PTs per output.
* **Foldback.** PT1 of any MC may be turned into a shared, inverted product (NAND) visible to every PT
  of its block in foldback column = local index of that MC.
* **Storage.** D/T flip-flop or latch, clocked by one of three global clocks (GCLK1–3, optionally
  inverted) or by PT4; async reset from GCLR (global) and/or PT3; async preset from PT5.
* **Output enable.** Always, never, PT5, or one of six global OE networks GOE1–6. Each GOE network
  selects one input (OE1 pin, OE2 pin or a pad/feedback of certain MCs) and can be inverted.
* **UIM.** For each block, 40 muxes; mux `m` chooses one of `w` inputs (1502: 5, 1504: 9, 1508: 27)
  from a fixed table that is identical for all blocks. Pads and feedbacks of *every* MC are
  reachable from some mux of every block, but each from only 2–5 specific muxes — routing is a
  bipartite matching problem.

## F1.2 Signal and node numbering (the vendor's names)

| name in `.fit`/`.pin` | meaning |
|---|---|
| `pin N` | package pin N |
| `MC n` | macrocell n (global, 1-based) |
| feedback node `600 + n` | the buried (feedback) output of MC n — what `#$ NODES X+:6nn` / CUPL `PINNODE 6nn` refer to |
| foldback expander node `300 + n` | the foldback product of MC n (PT1) |
| `Xnp`, `Xnfb` (UIM refs, X = block letter) | pad / feedback of MC n, e.g. `C44fb` |
| `GCLK`, `GCLR`, `OE1`, `OE2` (UIM refs) | dedicated input pins GCLK1, GCLR, OE1, GCLK2/OE2 |

A *pin* signal occupies its macrocell's pad (input) and/or output buffer. A *buried node* occupies a
macrocell's logic only; it can share a macrocell with an input-only pin (the pad and the logic are
independent), e.g. in the user's `input.tt2` the vendor put input `I5` and latch `A0` both on MC20.

## F1.3 Packages: dedicated pins, JTAG/PD pins, GCLK3 [O]

Oracle-derived (`tools/fuzzers/fitter-pins/exp_pinmap.py`, `exp_pinroles.py`, `exp_dedic.py`, `exp_gclk3.py`):
an output pinned to each pin tells whether it is a user I/O and on which MC; an input pinned to each
non-I/O pin used as clock / OE / reset tells its dedicated function; outputs that fail only with
JTAG (or `pd1`/`pd2`) on identify those pins. All agree with the datasheet tables where these exist.

| device | package | I/O pins | GCLK1 | OE1 | GCLR | GCLK2/OE2 | GCLK3 (I/O) | TDI TMS TCK TDO | PD1 PD2 |
|---|---|---|---|---|---|---|---|---|---|
| P1502C44 | PLCC44 | 32 | 43 | 44 | 1 | 2 | 41 (MC17) | 7 13 32 38 | 11 25 |
| P1502T44 | TQFP44 | 32 | 37 | 38 | 39 | 40 | 35 (MC17) | 1 7 26 32 | 5 19 |
| P1504C44 | PLCC44 | 32 | 43 | 44 | 1 | 2 | 41 (MC64) | 7 13 32 38 | 11 25 |
| P1504T44 | TQFP44 | 32 | 37 | 38 | 39 | 40 | 35 (MC64) | 1 7 26 32 | 5 19 |
| P1504C68 | PLCC68 | 48 | 67 | 68 | 1 | 2 | 65 (MC64) | 12 19 50 57 | 17 37 |
| P1504C84 | PLCC84 | 64 | 83 | 84 | 1 | 2 | 81 (MC64) | 14 23 62 71 | 20 46 |
| P1504Q100 | PQFP100 | 64 | 89 | 90 | 91 | 92 | 87 (MC64) | 6 17 64 75 | 14 44 |
| P1504T100 | TQFP100 | 64 | 87 | 88 | 89 | 90 | 85 (MC64) | 4 15 62 73 | 12 42 |
| P1508C68 | PLCC68 | 48 | 67 | 68 | 1 | 2 | 65 (MC128) | 12 19 50 57 | 10 36 |
| P1508C84 | PLCC84 | 64 | 83 | 84 | 1 | 2 | 81 (MC128) | 14 23 62 71 | 12 45 |
| P1508T100 | TQFP100 | 80 | 87 | 88 | 89 | 90 | 85 (MC128) | 4 15 62 73 | 1 41 |
| P1508Q100 | PQFP100 | 80 | 89 | 90 | 91 | 92 | 87 (MC128) | 6 17 64 75 | 3 43 |
| P1508Q160 | PQFP160 | 95 | 139 | 140 | 141 | 142 | 137 (MC128) | 9 22 99 112 | 63 159 |

Notes:
* Packages with fewer pins than macrocells leave MCs unbonded (e.g. 1504 in 44-pin packages has 32
  bonded MCs; 1508 T100 has 80). Unbonded MCs remain usable as buried nodes.
* P1508Q160: pin 110 is input-only — it reaches the pad of MC109 but cannot drive an output.
* The GCLK3 pin is an ordinary I/O pin whose pad also feeds global clock network 3.
* The full pin → MC maps for all 13 devices are in `atf15xx-arch/data/devices.json` (`pins`).
  Example, P1504C44: `MC1:12 MC3:11 MC4:9 MC5:8 MC8:7 MC11:6 MC14:5 MC16:4 MC17:21 MC19:20 MC20:19
  MC21:18 MC24:17 MC25:16 MC30:14 MC32:13 MC33:24 MC35:25 MC36:26 MC37:27 MC40:28 MC41:29 MC46:31
  MC48:32 MC49:33 MC51:34 MC52:36 MC53:37 MC56:38 MC57:39 MC62:40 MC64:41`.
* With JTAG enabled (the vendor default) the four JTAG pins are treated as used input pins (their
  MCs get the "pad used" configuration of [F2.3.3](02-fuse-map.md#f233-what-the-vendor-writes-for-each-kind-of-macrocell-o));
  the same applies to PD1/PD2 when `pd1`/`pd2` are on.

## F1.4 From `.tt2` names to fuses (summary)

| `.tt2` item | resource | fuses |
|---|---|---|
| `#$ PINS Y+:12` output | MC of pin 12 (device map) | MC config, its PTs, OE |
| `#$ PINS A+:4` input | pad of MC of pin 4, or a dedicated input | UIM selectors that read it |
| `#$ NODES N+:605` | MC5 buried logic | MC5 config + PTs |
| `#$ NODES N+:305` | foldback expander of MC5 | MC5 PT1 with `Pt1Mux` = 1 |
| `.ilb X.Q` / `X` / `X.PIN` | feedback register / feedback comb / pad of X | UIM entry `Xnfb` / `Xnfb` / `Xnp` |
| `.ob Y.C` = single GCLK pin literal | global clock | `Gclk0/1` (+ GCLK invert bit) |
| `.ob Y.AR` = single GCLR pin literal | global reset | `GclrMux` = 0 (+ GCLR invert bit) |
| `.ob Y.OE` = single pin literal | a GOE network | GOE selector, OE code, GOE invert bit |
| any other `.C .CE .AR .AP .OE` | PT4 / PT4 / PT3 / PT5 / PT5 | PT row + `PtNMux`/`PtNFunc` |
| `.ob Y`, `.D`, `.REG`, `.T`, `.L` | sum of the remaining PTs (+ cascade) | PT rows, `Invert`, `Storage`, `XorB` |
