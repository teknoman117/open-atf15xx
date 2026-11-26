# 8. JEDEC output (the `Load` instruction)

After the SVF run completes (§7.1), ATMISP writes a JEDEC file for **every device whose instruction
is `Load`**, to that device's chain-file JEDEC path (overwriting it). This happens in SVF mode too.

## 8.1 Data flow

1. **Rows.** In hardware mode the rows would hold read-back data; in SVF mode nothing is read, so
   the rows are the default image: every cell `'1'` (and, for AS-type, special/UES/config all `'1'`).
2. **Inverse map.** The fuse array `F[0..QF)` is initialised to `X`, then:
   * every fuse that the forward map (§5) places in a row/register cell is set from that cell (for
     fuses mapped to two cells, either cell — they are always equal when written by the same image);
   * AS-type reserved fuses (§4.4) are set to `'0'`; 1502BE reserved fuses are set to `'0'`;
   * BE-type fuses that appear in no row (§5.3 table) are set to `'1'`;
   * 1504BE/1508BE reserved fuses are left `X`.
3. **Finalise** (§3.4 rule): remaining `X` → `'0'` (the default character `F0`); compute the
   checksum (§3.6).

With SVF-mode data this yields: every fuse `'1'` except the reserved fuses, which are `'0'`.

## 8.2 File format

**Conformance is at the fuse level** (§11.3): a conforming implementation writes a JESD3-C file
that ATMISP's own reader (§3) and any other JEDEC reader turns back into exactly the fuse array of
§8.1, with the security fuse and fuse checksum below. The text layout is the implementation's own;
`atf15xx-tool` does **not** reproduce ATMISP's layout or its annotation text (removed 2026-11-25 for
copyright hygiene; until then the layout was copied byte for byte from ATMISP output). The
functional fields ATMISP writes, which every implementation MUST write too:

| Field | Value |
|---|---|
| STX (`0x02`) | first byte; the design specification (free text) runs to the first `*` |
| `QF` | the family's fuse count (§4.3) |
| `F0` | default fuse state `0` |
| `G0` | security fuse `0` (`Load` never sets it); **no `G` field for 1502BE**, as ATMISP writes it |
| `L` | every fuse `F[0..QF)` exactly once, in address order |
| `C` | the fuse checksum (§3.6), 4 upper-case hex digits (ATMISP writes `%X`, unpadded; readers parse the hex value) |
| ETX (`0x03`) | followed by the transmission checksum `0000` (a literal, as ATMISP writes it) |

Each field (counted from its identifier to the `*`) MUST be shorter than 200 bytes (§3.2: ATMISP's
own reader rejects longer fields), and no field may contain `NOTE`/`Note` unless it is meant to be
ignored. Lines end in `CR LF`.

`atf15xx-tool`'s layout (`atf15xx-tool/src/isp/jedout.rs`), one family-independent rule built from the
family layout of §4.4 (first fuse of the configuration, special, UES and reserved regions):

```
<STX>JEDEC fuse map for <names>, written by open-atf150x atmisp <version> (Load)
Created: <ctime>*                                   ctime() of the local time (§6.3)
QF<qf>* F0* G0*                                     1502BE: "QF16814* F0*"
N logic array, fuses 0-<config-1>*
L<addr:05> <96 fuses>*                              fuses 0 .. config-1, 96 per field (the last shorter)
…
L<config:05> <config fuses>* N configuration bits*
L<special:05> <special fuses>* N special purpose bits*
L<ues:05> <16 fuses>* N user electronic signature*
L<reserved:05> <reserved fuses>* N reserved*
C<checksum:04X>*
<ETX>0000
```

`<names>` is `ATF15xxAS/ASL/ASV/ASVL` for the AS families, `ATF1502BE`, `ATF1504BE` and
`ATF1508BE/ATF1508RE` for the BE families. The file is written for every device name the family is
used for:

| Family (by QF) | Used for device names |
|---|---|
| 1502AS | ATF1502AS, ATF1502ASL, ATF1502ASV, ATF1502ASVL |
| 1504AS | ATF1504AS, ATF1504ASL, ATF1504ASV, ATF1504ASVL |
| 1508AS | ATF1508AS, ATF1508ASL, ATF1508ASV, ATF1508ASVL |
| 1502BE | ATF1502BE |
| 1504BE | ATF1504BE |
| 1508BE | ATF1508BE, ATF1508RE |

ATMISP's own layout (for reference, not reproduced): a three-line header naming the device (the
1508BE header names both 1508BE and 1508RE), the `QF`/`F0`/`G0` fields on one line, then `L`
fields with ATMISP's product-term, macrocell, UIM and region annotations in `NOTE` fields, and the
checksum printed unpadded (`%X`). Its product-term labels are reversed for half the macrocells
(HANDOFF §7). `tools/fuzzers/fuse-regions` reads the region labels from such a file; ATMISP's text itself is
never kept in the repository.

## 8.3 Algorithm

```
F ← fuse array of §8.1                  (QF characters '0'/'1')
write the header, QF, F0 and (except 1502BE) G0 fields
for each region of §4.4 in address order (logic array in 96-fuse fields):
    write "L" ++ address (5 digits) ++ " " ++ F[region] ++ "*"
write "C" ++ checksum(F) (4 hex digits) ++ "*", then ETX "0000"
write the bytes to D.jed_path
```

Verification (§11): for every `Load` vector, the file `atf15xx-tool` writes holds the same fuses, the
same security fuse and the same checksum as the file ATMISP 7.3 wrote (checked on all 9 `Load`
vectors, every family and name variant, before the expected files were rewritten in this layout,
2026-11-25; `tools/fuzzers/atmisp-svf/rewrite_load_vectors.py`).
