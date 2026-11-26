# 6. SVF writer

## 6.1 File encoding

ATMISP writes the SVF with C `fprintf` on a stream opened in **text mode** (`"w"`), so every `\n`
below is written as `CR LF` (`0D 0A`). The file is ASCII. There is no trailing data after the last
line. A conforming implementation MUST write `CR LF` line endings.

## 6.2 Bit-string → hex

Scans are built as LSB-first bit strings (README *Notation*). A bit string `s` of length `n` is
rendered as follows:

```
digits = []
for i in 0, 4, 8, … while i < n:
    v = Σ_{k=0..3, i+k<n} (code(s[i+k]) − 48) · 2^k        # code() = byte value
    digits.append( "0123456789abcdef"[v] )  if 0 ≤ v ≤ 15  else  UNDEFINED
hex = reverse(digits)                                       # most significant digit first
```

* The number of digits is `⌈n/4⌉`; there is **no zero-padding and no `0x`**; digits are lower case.
* If `v` is outside 0–15 ATMISP prints an uninitialised byte. After the input checks of §3/§9 the
  only case reachable is defect D3 (§9.4), whose observed output (a backtick `` ` ``) is reproduced
  literally.

Example: `bits(0x2A1, 10)` = `"1000010101"` → groups `1000`=1, `0101`=10, `01`=2 → `"2a1"`.

## 6.3 Header

```
//  SVF file written by ATMISP version 7.3\n
//  Created on: <CTIME>\n
//  SVF Rev. <R>  \n
TRST ABSENT;\n
ENDIR IDLE;\n
ENDDR IDLE;\n
HDR 0;\n
HIR 0;\n
TDR 0;\n
TIR 0;\n
```

* Two spaces after `//`; `<R>` is `C` or `D`; the revision line ends with **two spaces** before `\n`.
* `<CTIME>` is the C `ctime()` text of the local time at generation, without its trailing newline:
  `Www Mmm dd hh:mm:ss yyyy` with the day of month **space-padded to width 2** (e.g.
  `Thu Nov  5 09:03:07 2026`). Conformance comparisons ignore this line (README).

## 6.4 Primitives

All SVF content after the header is produced by these primitives. *Rev*, *TCK* and *SR* are the SVF
options (§6.5).

| Primitive | Output |
|---|---|
| `SIR(b)` | `SIR <len(b)> TDI (<hex(b)>);\n` |
| `SDR(b)` | `SDR <len(b)> TDI (<hex(b)>);\n` |
| `SDR_TDO(b, t)` | `SDR <len(b)> TDI (<hex(b)>)\n` then `\tTDO (<hex(t)>);\n` |
| `SDR_TDO_MASK(b, t, m)` | `SDR <len(b)> TDI (<hex(b)>)\n` then `\tTDO (<hex(t)>)\n` then `\tMASK (<hex(m)>);\n` |
| `IDLE` | `STATE IDLE;\n` |
| `RESET` | `STATE RESET;\n` **only if SR is on**; nothing otherwise |
| `DELAY(ns, us, ms)` | let `t = ⌊ns/1000⌋ + us + 1 + 1000·ms` (µs). Rev D: `RUNTEST <t>E-6 SEC;\n`. Rev C: `RUNTEST <⌊t / (TCK·1000)⌋> TCK;\n` |
| `CLOCKS(n)` | `RUNTEST <n> TCK;\n` (same for both revisions) |

`\t` is a single TAB. Integers are printed with `%d` (no padding).

Notes, all reproduced from ATMISP:

* `DELAY` always adds 1 µs, so a 50 ms wait prints `50001E-6`, and `DELAY(0,20,50)` prints `50021E-6`.
* In Rev C the divisor is `TCK × 1000` although `TCK` is entered in microseconds, so Rev C delays
  are 1000× too short for the stated TCK (defect D5). A 1 µs delay becomes `RUNTEST 0 TCK;`.
* In every place ATMISP emits `TDO`, the TDI data and the expected TDO data are the same string
  (`t = b`), except in the IDCODE scan (§7.2.3). `MASK`, where present, is all ones of the scan length.

## 6.5 SVF options

| Option | Values | ATMISP default | Effect |
|---|---|---|---|
| revision | `C`, `D` | `D` | header line; `DELAY` format |
| TCK period | integer 1…1000 (µs) | 1 | Rev C `DELAY` divisor. Out-of-range values are replaced by 1 (ATMISP: `Invalid Tck period.\nDefault Tck value is set to 1.\nValid Tck range: 1 - 1000`) |
| use state reset | on/off | on | whether `RESET` emits `STATE RESET;` |

## 6.6 Chain composition

Let the chain have devices `D1…DN` and let `x` be the index of the device being operated on.
Strings are built by **prepending** each device's contribution in order `i = 1..N`, so the final
string is `contrib(DN) ++ … ++ contrib(D1)`: device 1 occupies the most-significant bits.

| Function | Contribution of the target `Dx` | Contribution of every other `Di` |
|---|---|---|
| `CIR(x, op)` — instruction scan | `bits(op, 10)` | `"1"×width(Di)` if `isUnknown(Di)`, else `"1"×10` |
| `CDR(x, data)` — data scan | `data` | `"0"` |
| `CADR(x, a)` — AS address scan | `a` | `"1"` |

Note the different BYPASS fill for address (`1`) and data (`0`) scans, and that a non-target Atmel
device always contributes 10 IR bits regardless of its declared width.

Chain-wide scans (ISC enable/disable, IDCODE) are composed per device as specified in §7.2.

## 6.7 Address encodings

* **AS-type address** (11 bits): `ASADDR(prefix, r) = bits(r, 8) ++ prefix`, where `prefix` is a
  3-character string: `"000"` for array rows, `"100"` config, `"010"` special, `"110"` UES. Hence
  row `0x0C` → `00c`, config → `100`, special → `200`, UES → `300`.
* **BE-type address** (16 bits): `BEADDR(v)` = the 16-bit value `v` written **MSB first**
  (`s[i] = bit (15−i) of v`). Because scans are LSB-first, the printed hex is the bit-reversed value:
  `BEADDR(0x20)` prints `0400`.
