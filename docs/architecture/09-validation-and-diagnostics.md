# 9. Validation, diagnostics and known ATMISP defects

## 9.1 Pre-run checks, in this order

1. **Family/voltage mix.** The classes are 5V
   (`ATF1502AS ATF1502ASL ATF1502SE ATF1502SEL ATF1504AS ATF1504ASL ATF1508AS ATF1508ASL`),
   3V3 (`ATF1502ASV ATF1502ASVL ATF1504ASV ATF1504ASVL ATF1508ASV ATF1508ASVL`) and
   BE (`ATF1502BE ATF1504BE ATF1508BE ATF1508RE`) (exact name matches; other names, including
   `UNKNOWN`, have no class).
   * a BE-class device together with a device of any other class → **error V1**, nothing is
     written: `Cannot program ATF15xxBE with other device types!` (vectors `b2_chn_be_with_as`,
     `v_be_5v`, `v_5v_be`, `v_5v_3v3_be`, `v_be_3v3_5v`: BE with 3V3, with 5V, with both, either
     order);
   * 5V and 3V3 mixed without BE → **V2**: ATMISP asks
     `Mixed voltage programming is not suggested! Do you want to continue?`; answering *Yes*
     continues with unchanged output.
2. **SVF file.** If no SVF path is given: `Please select SVF file before running!`; if it cannot be
   created: `Cannot open SVF file to write.` (both abort).

The hardware-mode check that refuses JEDEC files whose JTAG-enable fuse is `0`
(`At least one JEDEC file has JTAG port disabled. …`) is **not performed in SVF mode** (verified).

## 9.2 Checks a conforming implementation adds

ATMISP accepts the following inputs in SVF mode but then produces meaningless output, undefined
behaviour, or crashes. A conforming implementation MUST reject them with an error (it diverges from
ATMISP only where ATMISP's behaviour is not reproducible):

| Id | Condition | Message |
|---|---|---|
| J1 | instruction 1/4/7 and the JEDEC `QF` is absent, not one of the six sizes, or ≠ the name's expected QF (§4.3) | `JEDEC file is not valid for this device.\nPlease select a valid JEDEC file!` |
| J2 | a JEDEC field of 200 bytes or more (counted from its identifier, §3.2), an `L` field writing past `QF` or containing digits other than `0`/`1`, or an `F` value other than 0/1 | `JEDEC file is not valid for this device.\nPlease select a valid JEDEC file!` |
| N1 | `ATF1502SE`/`ATF1502SEL` (ATMISP crashes), or a device name not in the table of §4.1 whose instruction is not `Bypass`/empty (names that are merely bypassed are processed exactly as ATMISP does) | `Unsupported device name '<name>' for device # <d>` |
| N2 | `UNKNOWN` device with an instruction other than `Bypass` (ATMISP's help requires Bypass; ATMISP would apply BE-type procedures without data) | `Bypass must be selected for all UNKNOWN devices (device # <d>)` |

It SHOULD warn (without changing output) when:

| Id | Condition | Message |
|---|---|---|
| W6 | `Program/Verify` or `Program/Verify/Secure` with a JEDEC file whose JTAG-enable fuse is `0` (§5.4) | `JEDEC file for device # <d> has the JTAG port disabled; the device cannot be reprogrammed via JTAG afterwards` |
| W7 | instruction `Secure`, `Program/Verify/Secure` or `Verify Secure` | ATMISP's own text: `warning: SVF generation procedure cannot be used with <instr> procedure together.  This is the software limitation.` |
| W8 | ATF1502ASVL is used (no IDCODE known, D6) | `ATF1502ASVL has no IDCODE in ATMISP's table; the IDCODE scan expects a single '1' bit` |

## 9.3 Run transcript (informative)

ATMISP writes a transcript (GUI list box and `jtag.log`). A conforming CLI SHOULD print the same
lines (without ATMISP's 10-space indentation) to standard error. In SVF mode every verification
"passes". Order: phase-1 lines, then phase-2 lines, then the final line.

| Event | Text |
|---|---|
| start | `...  executing JTAG instructions...please wait...` |
| erase (phase 1) | `log: erasing device #<d>.` |
| Bypass / unknown | `log: bypass device #<d>.` |
| Program/Verify, P/V/S | `log: programming device #<d>.` then `log: verifying device #<d>: ... OK ...` |
| Verify | `log: verifying device #<d>: ... OK ...` |
| Erase (phase 2) | `log: blank check device #<d>: ... OK ...device is blank.` |
| Blank Check, AS-type | `log: blank check device #<d>: ... OK ...device is blank.` |
| Blank Check, BE-type | `log: blank check device #<d>: ... failed blank check since device is secured ...` (marks run failed) |
| Load, AS-type | `log: reading device #<d>.` |
| Load, BE-type | `warning: Device #<d> is secured, output jedec is blank.` then `log: reading device #<d>.` (marks run failed) |
| Secure | `log: securing device #<d>.`, `log: verifying if device #<d> secured:`, then W7 |
| P/V/S (after verify) | `log: securing device #<d>.`, `log: verifying secure for device #<d>:`, then W7 |
| Verify Secure | `log: verifying secure for device #<d>:`, then W7 |
| Read UES | `log: read User Electronic Signature from device #<d>:` then `     UES = "` (ATMISP's line is cut short by a NUL character) |
| end | `log: JTAG instructions finished successfully.` or, if any step marked the run failed, `log: JTAG instructions failed.` |

The BE "secured"/"failed" results are artefacts of SVF mode (the secure-status read returns no
data); the SVF and JEDEC files are written completely regardless. The automatic secure step after a
`Program/Verify` with `G1` is silent.

## 9.4 Catalogue of ATMISP defects

Compatibility mode MUST reproduce every defect below; a strict mode MAY correct those marked *†*.

| Id | Defect | Compatibility behaviour | Strict correction |
|---|---|---|---|
| D1 | `ATF1502SE`/`ATF1502SEL` crash ATMISP during generation (empty SVF) | reject (N1) | — |
| D2† | Over-long secure-status scans: AS `VERIFY_SECURE` shifts 33 bits into the 32-bit config register; BE `BE_READ` (bank 1) shifts 9 bits into an 8-bit row register | emit 33 / 9 bits | 32 / 8 bits |
| D3† | BE bank-2 `BE_READ` (1508BE/RE secure) appends an uninitialised byte, printing a backtick in TDI and TDO (`` SDR 9 TDI (`00) ``) — invalid SVF | emit literally (§7.5) | `SDR 8 TDI (00)` / `TDO (00);` |
| D4† | IDCODE scan expects `1` (mask `1`) from `UNKNOWN` devices, which are in BYPASS and shift out `0` | expect `1` | expect `0` |
| D5† | Rev C delays divide by `TCK×1000` instead of `TCK`, making delays 1000× too short (1 µs → `RUNTEST 0 TCK`) | as specified (§6.4) | `⌈t / TCK⌉` |
| D6 | ATF1502ASVL is accepted but absent from the IDCODE table: the IDCODE scan for it is 1 bit | as specified | — |
| D7 | BE `Secure` emits only the read; the security byte is never programmed in SVF | as specified | — |
| D8 | The JEDEC checksum (`C`) is never verified; digits `2`–`9` are accepted as fuse values (making hex output undefined); fields of ≥ 200 bytes overflow a stack buffer | checksum ignored; the other two rejected (J2) | — |
| D9 | Line 2 of a chain file is always discarded, even if it holds the first device name | as specified (§2) | — |
| D10 | `ctime()` is local time; no time zone is recorded | as specified | — |

Strict mode MUST NOT change anything else, so that strict output differs from compatible output
only at the positions affected by the listed defects.
