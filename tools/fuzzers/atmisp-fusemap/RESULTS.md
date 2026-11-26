# atmisp-fusemap results (2026-11-25)

127 cases (17–19 bit-pattern JEDEC files × 7 device names), ATMISP 7.3 under wine, Program/Verify
in SVF mode, Rev D, TCK 1. Every scan bit decoded cleanly (no inconsistent bit, no inverted copy).

## `check.py`: atf15xx-tool vs ATMISP

| device | SVFs identical |
|---|---|
| ATF1502AS | 17/17 |
| ATF1504AS | 18/18 |
| ATF1508AS | 19/19 |
| ATF1502BE | 17/17 |
| ATF1504BE | 18/18 |
| ATF1508BE | 19/19 |
| ATF1508RE | 19/19 |

Every fuse has a distinct value sequence over a device's patterns, so 127/127 identical SVFs mean
that the programming-row and register cells of `atf15xx-tool/src/isp/fusemap.rs` copy the same
fuse as ATMISP's, in every family. The 1508RE uses the 1508BE map.

## `decode.py`: layout (fuse ranges carried by each instruction's TDI scans)

| family | rows (data instruction, scan length × count) | registers | fuses in no cell |
|---|---|---|---|
| 1502AS | `290`: 86 × 418 (209 rows, program + verify), fuses 0–16749 | config `291` 16750–16781 (32), special `292` 16782–16785 (4), UES `293` 16786–16801 (16) | 16802–16807 |
| 1504AS | 166-bit rows, fuses 0–34133 | config 34134–34165, special 34166–34169, UES 34170–34185 | 34186–34191 |
| 1508AS | 326-bit rows, fuses 0–74081 | config 74082–74113, special 74114–74117, UES 74118–74133 | 74134–74135 |
| 1502BE | `2a2` pages 256 × 130 and `2a3` bytes 8 × 24 | (rows) | 15792–15839, 16272–16319, 16806–16813 |
| 1504BE | pages 256 × 264, bytes 8 × 56 | (rows) | 31152–31199, 31632–31679, 32112–32159, 32592–32639, 34190–34197 |
| 1508BE / RE | pages 256 × 574, bytes 8 × 84 | (rows) | 61872–61919, 62352–62399, 62832–62879, 63312–63359, 63792–63839, 64272–64319, 64752–64799, 65232–65279, 74138–74145 |

The register ranges equal `Family::layout()` in `device.rs` (configuration start, special start, UES
start, reserved start); the fuses in no cell are the AS reserved fuses and exactly the BE "fuses in
no row" of spec §5.3. In the BE families some row data is scanned by both the page and the byte
instruction, so fuses there appear in more than two scan cells; that is scan multiplicity, not the
row-level "fuses in two cells" of §5.3, and the identical SVFs cover both.
