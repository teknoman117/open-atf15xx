# 3. JEDEC input (`.jed`)

ATMISP reads a JEDEC file (JESD3-C style) into a **fuse array** `F[0..QF)` of characters. Only the
behaviour below matters for SVF generation; ATMISP is considerably more lenient than JESD3-C and a
compatible reader MUST reproduce that leniency.

## 3.1 When JEDEC files are read

A device's JEDEC file is read **only** when its instruction is `Program/Verify`, `Verify` or
`Program/Verify/Secure`. For every other instruction (including `Load`, whose path is an *output*)
the device uses a **default fuse image** (§3.5).

## 3.2 Field tokenisation

1. The file is read as a byte stream and split into **fields** terminated by `*` (C++
   `istream::getline(buf, 2000, '*')` into a zero-filled 2000-byte buffer).
2. A field that contains the substring `NOTE` or `Note` anywhere is ignored.
3. Leading bytes that are neither ASCII letters, decimal digits, STX (`0x02`) nor ETX (`0x03`) are
   skipped. ATMISP then copies exactly 200 bytes of the remainder into an *unterminated* 200-byte
   buffer. If the remainder is **200 bytes or longer**, the buffer is not NUL-terminated and ATMISP
   reads and writes past it (observed: corrupted row lengths and non-ASCII bytes in the SVF). Such a
   file is undefined behaviour; a conforming reader MUST reject it with **J2** (§9.2). Files written
   by ATMISP and by the usual fitters (≤ ~110 bytes per field) are unaffected.
4. The field identifier is determined from the first one or two characters, compared
   case-insensitively:

| First char(s) | Field | Code |
|---|---|---|
| STX (`0x02`) | start of transmission (text up to the first `*` is the design specification) | 1 |
| `QF` | fuse count | 2 |
| `QP` | pin count | 3 |
| `QV` | test-vector count | 4 |
| `F` | default fuse state | 5 |
| `G` | security fuse | 6 |
| `N` | note | 7 |
| `L` | fuse list | 8 |
| `C` | fuse checksum | 9 |
| `X` | default test condition | 10 |
| `V` | test vector | 11 |
| anything else | ignored | — |

Fields are processed in file order.

## 3.3 Field semantics

*Numeric value* below means: the concatenation of all decimal digits in the field (every
non-digit is skipped), interpreted in base 10.

| Field | Effect |
|---|---|
| `QF` | `QF ← value`; allocate `F[0..QF)` and set every element to `X` (unset). A second `QF` re-allocates. |
| `F` | `default ← chr(value + '0')` (so `F0`→`'0'`, `F1`→`'1'`). Initial default is `'0'`. |
| `G` | `security ← chr(value + '0')`. Initial value `'0'`. |
| `L` | see below |
| `C` | the checksum is parsed but **never verified** (a wrong checksum is accepted silently). |
| `QP`, `QV`, `N`, `X`, `V`, STX | parsed, no effect on SVF output (vector `jf_qv_x_v`: the same fuses with and without `QV`/`X`/`V` fields give identical SVF). |

**`L` field**: scan the field from its first character.
*Address* = the decimal digits that occur **before** the first space, TAB or LF (CR is *not* a
separator). After that separator, **every decimal digit character** (`0`–`9`) is appended to the
data string; all other characters (CR, `X`, letters, …) are skipped. Digits `2`–`9` are stored
verbatim by ATMISP and later make its SVF hex output undefined (a hex digit whose group sums to
more than 15 is printed as an uninitialised byte — observed `0x13`); a conforming reader MUST
reject an `L` field containing them (**J2**). Then, provided `QF` is one of
the six known sizes (16808, 16814, 34192, 34198, 74136, 74146) — otherwise the field is ignored —
`F[address + k] ← data[k]` for each `k`. Writing past `F[QF-1]` is undefined behaviour in ATMISP; a
conforming reader MUST reject such a file.

Because of §3.2 step 3, an `L` field (counting from the `L`) must be shorter than 200 bytes;
longer fields are rejected (J2).

## 3.4 Finalisation

After the last field: every `F[i]` still equal to `X` is replaced by `default`. A checksum is
computed (§3.6) and stored but not compared with the `C` field.

After a successful read every fuse is `'0'` or `'1'` (assuming the `F` field is `F0`/`F1`; any
other `F` value makes ATMISP's output undefined and MUST be rejected with J2).

## 3.5 Default fuse image

When no JEDEC file is read (§3.1), the device's rows, special bits, UES and configuration bits are
all `'1'` (the erased state), i.e. as if every mapped fuse were `1` (§5.1).

## 3.6 Fuse checksum

The standard JEDEC checksum: for `i = 0, 8, 16, …`, form the byte
`Σ_{k=0..7} (F[i+k] = '1') << k` (missing positions count as 0); sum the bytes; take the low 16 bits.

## 3.7 Values derived from the fuse array

| Value | Derivation |
|---|---|
| `security` | the `G` field character (default `'0'`). If `'1'`, `Program/Verify` also secures the device (§7.3). |
| `jtag_enabled` | `F[J] = '1'` where `J` is the family's JTAG fuse (§4.4). Used only by the hardware-mode pre-check — **SVF generation ignores it** (§9.2). |
| rows / special / UES / config | §5 |

## 3.8 Device compatibility

The family used to map the fuse array is selected by **the JEDEC file's QF** (§4.3), whereas the
programming algorithm and row geometry are selected by **the chain device name**. ATMISP does not
check that they agree when generating SVF (its "JEDEC file is not valid for this device" check
exists only in the GUI device-properties dialog), and a mismatch yields meaningless output or a
crash. A conforming implementation MUST reject a JEDEC file whose QF differs from the expected QF
of the device name (§4.3) with error **J1**:

`JEDEC file is not valid for this device.\nPlease select a valid JEDEC file!`

A JEDEC file with no `QF` field, or with a `QF` that is not one of the six known sizes, MUST also be
rejected with **J1** (ATMISP crashes on such files).
