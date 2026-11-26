# 4. Device database

## 4.1 Device names

| Name | Supported¹ | Family² | Voltage class³ | IDCODE (SVF) | Alt IDCODE⁴ |
|---|---|---|---|---|---|
| `ATF1502AS`   | yes | 1502AS | 5V   | `0150203F` | `0150303F` |
| `ATF1502ASL`  | yes | 1502AS | 5V   | `0150203F` | `0150303F` |
| `ATF1502ASV`  | yes | 1502AS | 3V3  | `0151203F` | `0151303F` |
| `ATF1502ASVL` | **no** | 1502AS | 3V3 | *(none — `"1"`)*⁵ | — |
| `ATF1502SE`   | **no** (crashes) | 1502AS | 5V | `0152203F` | `0153203F` |
| `ATF1502SEL`  | **no** (crashes) | 1502AS | 5V | `0152203F` | `0153203F` |
| `ATF1502BE`   | yes | 1502BE | BE   | `0156203F` | `0156203F` |
| `ATF1504AS`   | yes | 1504AS | 5V   | `0150403F` | `0150503F` |
| `ATF1504ASL`  | yes | 1504AS | 5V   | `0150403F` | `0150503F` |
| `ATF1504ASV`  | yes | 1504AS | 3V3  | `0151403F` | `0151503F` |
| `ATF1504ASVL` | yes | 1504AS | 3V3  | `0151403F` | `0151503F` |
| `ATF1504BE`   | yes | 1504BE | BE   | `0154403F` | `0154403F` |
| `ATF1508AS`   | yes | 1508AS | 5V   | `0150803F` | `0150903F` |
| `ATF1508ASL`  | yes | 1508AS | 5V   | `0150803F` | `0150903F` |
| `ATF1508ASV`  | yes | 1508AS | 3V3  | `0151803F` | `0151903F` |
| `ATF1508ASVL` | yes | 1508AS | 3V3  | `0151803F` | `0151903F` |
| `ATF1508BE`   | yes | 1508BE | BE   | `0154803F` | `0154803F` |
| `ATF1508RE`   | yes | 1508BE | BE   | `0155803F` | `0155803F` |
| `UNKNOWN`     | yes (non-Atmel device, must be `Bypass`) | — | none | *(`"1"`)*⁵ | — |

1. *Supported* = offered by the GUI and validated by the chain reader (§2.2.2 step 5).
2. Family = the QF expected for the name (§4.3), which selects geometry and algorithms.
3. Used only by the mixed-voltage check (§9.1).
4. Alternate silicon IDs present in ATMISP's table; used only for hardware-mode detection. SVF
   output always uses the primary value.
5. The IDCODE scan expects the 1-bit string `"1"` for these names (§7.2.3).

All Atmel devices have a 10-bit instruction register; `UNKNOWN` devices have the width given in the
chain file (1–99).

## 4.2 Name-based dispatch rules

ATMISP selects behaviour by **substring tests** on the device name (MFC `CString::Find`), not by
table lookup. Implementations MUST use exactly these predicates, evaluated on the chain-file name
(case-sensitive), because they determine output for the unsupported names too:

| Predicate | Where used |
|---|---|
| `isAS(n)` = `"AS" ∈ n` | erase / program / verify / secure / verify-secure select the AS-type procedures; otherwise BE-type (*note:* `ATF1502SE` is therefore BE-type here) |
| `isBEorRE(n)` = `"BE" ∈ n` or `"RE" ∈ n` | ISC enable/disable opcodes (§7.2.2, §7.2.4); BE secure / verify-secure |
| `isUnknown(n)` = `"UNKNOWN" ∈ n` | bypass width in chain composition (§6.6); ISC enable/disable; header-level checks |
| `isATF150(n)` = `"ATF150" ∈ n` | IDCODE-scan opcode (§7.2.3) |
| `beKind(n)` | `"1502BE" ∈ n` → 1502BE; else `"1504BE" ∈ n` → 1504BE; else → 1508BE (bank-2 capable) |
| IDCODE | exact name match against the table in §4.1, else `"1"` |
| voltage class | exact name match (§9.1) |

## 4.3 Expected fuse count (QF) from the name

Evaluated in this order: `"1502AS"` or `"1502SE"` ∈ n → **16808**; `"1502BE"` → **16814**;
`"1504AS"` → **34192**; `"1504BE"` → **34198**; `"1508AS"` → **74136**; `"1508BE"` or
`"1508RE"` → **74146**; otherwise none.

The JEDEC-file family is selected from the file's `QF` with the same six values:

| QF | Family |
|---|---|
| 16808 | 1502AS |
| 16814 | 1502BE |
| 34192 | 1504AS |
| 34198 | 1504BE |
| 74136 | 1508AS |
| 74146 | 1508BE (also ATF1508RE) |

## 4.4 Family geometry

| Family | QF | Rows × row length | Config bits | Special bits | UES bits | Reserved | JTAG fuse `J` |
|---|---|---|---|---|---|---|---|
| 1502AS | 16808 | 229 × 86  | 16750–16781 | 16782–16785 | 16786–16801 | 16802–16807 | 16783 |
| 1504AS | 34192 | 233 × 166 | 34134–34165 | 34166–34169 | 34170–34185 | 34186–34191 | 34167 |
| 1508AS | 74136 | 251 × 326 | 74082–74113 | 74114–74117 | 74118–74133 | 74134–74135 | 74115 |
| 1502BE | 16814 | 2090 × 8  | 16750–16781 | 16782–16789 | 16790–16805 | 16806–16813 | 16783 |
| 1504BE | 34198 | 4252 × 8  | 34134–34165 | 34166–34173 | 34174–34189 | 34190–34197 | 34167 |
| 1508BE | 74146 | 9226 × 8  | 74082–74113 | 74114–74121 | 74122–74137 | 74138–74145 | 74115 |

For BE-type families config/special/UES fuses are mapped into ordinary rows (§5.3); the ranges above
are the JEDEC layout ATMISP uses when writing (§8). Rows, row lengths and the AS register ranges are
those ATMISP's SVF scans carry (`tools/fuzzers/atmisp-fusemap`, RESULTS.md); the reserved fuses are
the fuses no scan carries. `J` is the second configuration fuse (configuration start + 33) in every
family, the fuse the fitters' JTAG option sets for the AS families; ATMISP does not check it in SVF
mode, so only the warning W6 (§9.3) depends on it, and for the BE families `J` is inferred from that
pattern.

## 4.5 JTAG instruction opcodes

10-bit values, encoded LSB-first with `bits(op, 10)` (ATMISP stores them as literal strings).

| Opcode | Mnemonic (descriptive) | Used for |
|---|---|---|
| `0x059` | IDCODE | IDCODE scan (all ATF150x) |
| `0x280` | ISC enable/disable (AS) | AS ISC enable (DR `0x1B9`), AS ISC disable (DR `0x000`) |
| `0x2A4` | ISC enable (BE) | BE ISC enable (DR `0x1B9`) |
| `0x2A5` | ISC disable (BE) | BE ISC disable (DR `0x000`) |
| `0x2A1` | address | AS 11-bit address; BE 16-bit address |
| `0x290` | data (AS) / bank-1 select (BE) | AS 86/166/326-bit row data; BE handshake |
| `0x294` | bank-2 select (BE) | 1508BE rows ≥ 4812 |
| `0x291` | AS config register / BE handshake | 32-bit config row (AS); BE handshake |
| `0x292` | AS special register / BE control | 4-bit special row (AS); 8-bit BE control byte |
| `0x293` | AS UES register / BE bulk erase | 16-bit UES row (AS); BE erase |
| `0x29E` | program (AS) | AS program pulse |
| `0x2BF` | end program (AS) | after each AS program pulse |
| `0x2B3` | bulk erase (AS) | AS erase |
| `0x28C` | read (AS) | AS verify/read |
| `0x2A2` | page data (BE) | 256-bit BE page |
| `0x2A3` | byte data (BE) | 8-bit BE row |
| `0x2A6` | program (BE) | BE program pulse |
| `0x270` | UES read | Read UES (all families) |
| all-ones | BYPASS | non-target devices |

The mnemonics are descriptive only; implementations rely on the numeric values.
