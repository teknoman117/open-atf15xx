# 5. Fuse maps (fuse array → programming rows)

A device's programming data consists of **rows** (`rows[r]`, `r = 0..nRows-1`, each a string of
`rowLen` characters indexed by column `c`), plus — for AS-type families only — three separate
registers: **special** (4 chars), **UES** (16 chars) and **config** (32 chars).

The tables in this chapter are verified black-box against ATMISP itself
(`tools/fuzzers/atmisp-fusemap`): JEDEC files that give every fuse a distinct bit pattern (all 0,
all 1, and fuse i = bit K of i for every K) are programmed by ATMISP in SVF mode, the fuse carried
by every bit of every scan is read back from the SVFs, and `atf15xx-tool`, which implements these
tables, writes byte-identical SVF for all 127 such files across the seven device names. The
register ranges of §4.4 and the "fuses in no row" of §5.3 come out of the same run. The random-fuse
vectors of §11 check them again. They are normative.

> What the fuses *mean* (product terms, macrocell options, UIM, global configuration) for the
> AS families is specified in [F2](fitter/02-fuse-map.md); this chapter only maps the fuse array
> to programming rows.

## 5.1 General rules

1. The **family** (and therefore which table applies) is chosen from the JEDEC file's `QF` (§4.3).
   Without a JEDEC file the default image of §3.5 is used: every cell `'1'`.
2. Every cell not covered by a table entry is the constant `'1'`.
3. A cell's character is copied verbatim from the fuse array: `rows[r][c] = F[index(r, c)]`.
4. Row strings are used exactly as built: in an SVF scan the row is placed so that **column 0 is the
   least-significant (first-shifted) bit** for AS-type rows; BE-type rows are reversed (column 7
   first) — see §7.4/§7.6.

## 5.2 AS-type families

`index(r, c)` is given by the first matching line (there is no overlap):

### 5.2.1 1502AS (229 rows × 86 columns)

| Rows | Columns | `F[...]` index |
|---|---|---|
| 0–11 | 0–79 | `15439 + 80·r − c` |
| 12–107 | 0–79 | `7584 + (r−12) − 96·c` |
| 128–223 | 0–79 | `15264 + (r−128) − 96·c` |
| 224–228 | 0–79 | `16715 + (r−224) − 5·c` |
| 224–228 | 80–85 | `16745 + (r−224) − 5·(c−80)` |

All other cells: constant `1`.

### 5.2.2 1504AS (233 rows × 166 columns)

| Rows | Columns | `F[...]` index |
|---|---|---|
| 0–11 | 6–165 | `30879 + 160·r − (c−6)` |
| 12–107 | 6–165 | `15264 + (r−12) − 96·(c−6)` |
| 128–223 | 6–165 | `30624 + (r−128) − 96·(c−6)` |
| 224–232 | 0–165 | `34125 + (r−224) − 9·c` |

All other cells: constant `1`.

### 5.2.3 1508AS (251 rows × 326 columns)

| Rows | Columns | `F[...]` index |
|---|---|---|
| 0–11 | 6–325 | `61759 + 320·r − (c−6)` |
| 12–107 | 6–325 | `30624 + (r−12) − 96·(c−6)` |
| 128–223 | 6–325 | `61344 + (r−128) − 96·(c−6)` |
| 224–250 | 0–325 | `74055 + (r−224) − 27·c` |

All other cells: constant `1`.

### 5.2.4 Special, UES and config registers (AS-type)

With `S`, `U`, `K` = first fuse of the special / UES / config ranges of §4.4:

```
special[i] = F[S + 3  − i]    i = 0..3
ues[i]     = F[U + 15 − i]    i = 0..15
config[i]  = F[K + 31 − i]    i = 0..31
```

(i.e. each register holds its JEDEC field **reversed**). Reserved fuses are not used.

## 5.3 BE-type families

BE-type rows are 8 characters. Each table line covers rows `r0..r1`; with `i = r − r0`,
`g = ⌊i / G⌋`, `k = i mod G` (for `G = ∞`: `g = 0`, `k = i`):

```
rows[r][c] = F[ base + g + rstride·k + T[c] ]      c = 0..7
```

A line marked *explicit* gives `index(r, c)` for `c = 0..7` directly. Config, special and UES fuses
are ordinary row data in these families (they are the last rows of each table). Some fuses appear
in no row and are therefore never programmed, and some appear in two cells. Both effects are ATMISP
behaviour, follow automatically from the tables, and MUST be reproduced:

| Family | Fuses in no row | Fuses in two cells |
|---|---|---|
| 1502BE | 15792–15839, 16272–16319, 16806–16813 (reserved) | 16720–16724 |
| 1504BE | 31152–31199, 31632–31679, 32112–32159, 32592–32639, 34190–34197 (reserved) | 34080–34088 |
| 1508BE | 61872–61919, 62352–62399, 62832–62879, 63312–63359, 63792–63839, 64272–64319, 64752–64799, 65232–65279, 74138–74145 (reserved) | 73920–73946 |

### 5.3.1 1502BE (2090 rows × 8 columns)

| Rows | base | G | rstride | T[0..7] |
|---|---|---|---|---|
| 0–53 | 15360 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 54–107 | 15840 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 108–1067 | 0 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 1068–1117 | 16320 | 10 | 40 | `35 30 25 20 15 10 5 0` |
| 1118–2077 | 7680 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 2078 | — | — | — | explicit: `16745 16740 16735 16730 16725 16720 16720 16724` |
| 2079–2082 | 16721 | ∞ | 1 | `25 20 15 10 5 0 0 0` |
| 2083–2089 | 16750 | ∞ | 8 | `7 6 5 4 3 2 1 0` |

### 5.3.2 1504BE (4252 rows × 8 columns)

| Rows | base | G | rstride | T[0..7] |
|---|---|---|---|---|
| 0–53 | 30720 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 54–107 | 31200 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 108–161 | 31680 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 162–215 | 32160 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 216–1175 | 0 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 1176–1265 | 32640 | 10 | 72 | `63 54 45 36 27 18 9 0` |
| 1266–2225 | 15360 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 2226–3185 | 7680 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 3186–3275 | 33360 | 10 | 72 | `63 54 45 36 27 18 9 0` |
| 3276–4235 | 23040 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 4236 | — | — | — | explicit: `34125 34116 34107 34098 34089 34080 34080 34088` |
| 4237–4244 | 34081 | ∞ | 1 | `45 36 27 18 9 0 0 0` |
| 4245–4251 | 34134 | ∞ | 8 | `7 6 5 4 3 2 1 0` |

### 5.3.3 1508BE (9226 rows × 8 columns)

| Rows | base | G | rstride | T[0..7] |
|---|---|---|---|---|
| 0–53 | 61440 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 54–107 | 61920 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 108–161 | 62400 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 162–215 | 62880 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 216–269 | 63360 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 270–323 | 63840 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 324–377 | 64320 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 378–431 | 64800 | ∞ | 8 | `7 6 5 4 3 2 1 0` |
| 432–1391 | 7680 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 1392–1661 | 65280 | 10 | 216 | `189 2268 2349 108 81 2160 2241 0` |
| 1662–2621 | 0 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 2622–3581 | 30720 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 3582–3851 | 65307 | 10 | 216 | `2268 135 108 2295 2160 27 0 2187` |
| 3852–4811 | 38400 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 4812–5771 | 15360 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 5772–6041 | 69600 | 10 | 216 | `189 2268 2349 108 81 2160 2241 0` |
| 6042–7001 | 23040 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 7002–7961 | 53760 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 7962–8231 | 69627 | 10 | 216 | `2268 135 108 2295 2160 27 0 2187` |
| 8232–9191 | 46080 | 10 | 768 | `672 576 480 384 288 192 96 0` |
| 9192 | — | — | — | explicit: `74055 74028 74001 73974 73947 73920 73920 73946` |
| 9193–9218 | 73921 | ∞ | 1 | `135 108 81 54 27 0 0 0` |
| 9219–9225 | 74082 | ∞ | 8 | `7 6 5 4 3 2 1 0` |

The 1508BE table is also used for ATF1508RE. 1508BE rows 0–4811 form *bank 1* and rows 4812–9225
*bank 2* (programmed with a different select opcode, §7.6).

## 5.4 JTAG-enable fuse

`jtag_enabled = (F[J] = '1')` with `J` from §4.4. SVF generation does not use this flag (§9.2).
