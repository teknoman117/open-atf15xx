# 2. Chain file (`.chn`) format

A chain file is a small line-oriented text file. ATMISP reads it with the C runtime in *text mode*
(so on Windows `CR LF` becomes `LF`) and writes it with MFC `CStdioFile` in text mode (so every `LF`
it writes becomes `CR LF`). A conforming reader MUST accept both `LF` and `CR LF` line endings and
MUST treat a `CR` immediately preceding `LF` as part of the line terminator. A conforming writer
MUST emit `CR LF`.

## 2.1 Example (as written by ATMISP)

```
1 4 1 0 ␍␊
␍␊
ATF1504ASV␍␊
10␍␊
1␍␊
C:\designs\counter.jed␍␊
```

## 2.2 Structure

```
line 1        : HEADER
line 2        : ignored (written empty; read and discarded — see step 9)
then, for each device d = 1..N, a DEVICE RECORD of 3 or 4 lines:
    NAME
    WIDTH [" *"]
    INSTRUCTION
    JEDEC-PATH          (may be empty; may be omitted if the next line is a device name)
anything after the N-th record is ignored
```

### 2.2.1 Header (line 1) — parsed character by character

The header is `N␠P␠K␠C<rest-of-line>` where:

| Field | Syntax | Meaning |
|---|---|---|
| `N` | 1–2 decimal digits, terminated by one space | number of devices, **1…50** |
| `P` | exactly one digit, followed by one space | port type: `4` = USB. `1`,`2`,`3` = legacy LPT1–3 |
| `K` | 1+ characters up to the next space / LF / EOF | calibration constant, **1…1000** |
| `C` | the single character after `K`'s terminator | cable type: `0` = ATDH1150USB |

Exact algorithm (`getc` returns the next byte or EOF):

1. `c ← getc`. If `c` is space, LF, EOF or not a decimal digit → **E1** (abort).
2. Read the device count: `buf ← ""`; loop: if `c` = space → stop; if `len(buf) > 1` → **E2**
   (abort); if `c` is not a digit → **E1** (abort); `buf ← buf ++ c`; `c ← getc`.
3. `N ← atoi(buf)`. If `N ≤ 0` or `N ≥ 51` → **E2** (abort).
4. `c ← getc` (port). If `c` is space/LF/EOF/non-digit → **E1** (abort). If `c` = `4` → port = USB.
   Else if `c` ∈ {`1`,`2`,`3`} → warning **W1**, port = USB. Else → **E1** (abort).
5. `c ← getc`; it MUST be a space, else **E1** (abort).
6. `c ← getc`; if space/LF/EOF/non-digit → **E1** (abort).
7. Calibration: `buf ← ""`, `count ← 0`, `K ← 0`; while `c` ∉ {space, LF, EOF}:
   if `count < 4` then `buf ← buf ++ c`, `K ← atoi(buf)`, else `K ← 1001`; `count ← count+1`; `c ← getc`.
   (Non-digit characters are appended and simply stop `atoi`.)
8. If `K ≤ 0` or `K ≥ 1001` → warning **W2**, `K ← 1`.
9. `c ← getc` (cable). If `c` = LF → warning **W3**, and parsing continues directly with the
   device records (the LF consumed here is normally the empty line 2). Otherwise, if `c` ≠ `0` →
   warning **W4**; then skip bytes up to and including the next LF (or EOF), then read **one more
   line and discard it** (this is line 2, which ATMISP writes empty; its content is never used —
   a file whose line 2 holds the first device name therefore loses that line).

In all cases the cable type is ATDH1150USB and the port is USB; neither value affects SVF output.
The calibration constant only scales hardware-mode busy-wait loops and **does not affect SVF
output** either; it must still be parsed and validated as above.

### 2.2.2 Device records

`readline()` reads bytes up to (not including) the next LF or EOF; a trailing CR is removed.
A *device-name line* is a line exactly equal to one of:

```
ATF1508AS ATF1508ASV ATF1504AS ATF1504ASV ATF1502AS ATF1502ASV ATF1508ASL ATF1508ASVL
ATF1504ASL ATF1504ASVL ATF1502ASL ATF1502ASVL ATF1502SE ATF1502SEL ATF1502BE ATF1504BE
ATF1508BE ATF1508RE UNKNOWN
```

Let `L ← ""` (the look-ahead line). For `d = 1..N`:

1. If `L` is a device-name line, `NAME ← L`; otherwise `NAME ← readline()`.
2. `WLINE ← readline()`; `ILINE ← readline()`; `L ← readline()`.
3. If `L` is a device-name line then `JED ← ""` (and `L` is kept as the next record's name);
   otherwise `JED ← L` (and on the next iteration `L` is not a name, so a fresh line is read).
4. **Width / IDCODE flag.** If `WLINE` contains `*`: `IDCODE ← no`, else `IDCODE ← yes`. Remove every
   `*` from `WLINE` and set `W ← atoi(WLINE)` (leading whitespace skipped, stops at first non-digit).
5. If `NAME` is one of the 15 supported Atmel names (`ATF1502AS ATF1502ASL ATF1502ASV ATF1502BE
   ATF1504AS ATF1504ASL ATF1504ASV ATF1504ASVL ATF1504BE ATF1508AS ATF1508ASL ATF1508ASV ATF1508ASVL
   ATF1508BE ATF1508RE`) and `W ≠ 10` → warning **W5**(d), `W ← 10`.
6. If `W ≤ 0` or `W ≥ 100` → warning **W5**(d), `W ← 10`. (So `UNKNOWN` devices may have 1–99.)
7. **Instruction.** `ILINE` MUST be exactly one of `0`…`9` (§1.3). Any other value yields the empty
   instruction name, which every later stage treats like `Bypass`.
8. If `ILINE` is exactly `1`, `4` or `7`, the file `JED` MUST be openable for reading, else
   **E3**(d) (abort).

Note that `NAME` itself is not validated: any text is accepted (it simply will not match any
family). `ATF1502ASVL`, `ATF1502SE`, `ATF1502SEL` are accepted by the reader but are not supported
devices; the SE names crash ATMISP during generation (§9.4, D1).

### 2.2.3 Diagnostics

| Id | Kind | Exact ATMISP text |
|---|---|---|
| E1 | error | `This is not a valid Chain File.` (one variant ends in `..`) |
| E2 | error | `Invalid Chain file.\nThe number of devices in Chain file must be between 1 and 50.` |
| E3 | error | `Invalid Chain file.\nJEDEC file specified in Chain file for device # <d> does not exist.` |
| W1 | warning | `Port type specified in Chain file is not supported.\nSetting port type to USB.` |
| W2 | warning | `Calibration constant in Chain file is invalid.\nSetting Calibration constant to 1.` |
| W3 | warning | `Cable type not specified in Chain file.\nSetting Cable type to ATDH1150USB.` |
| W4 | warning | `Cable type specified in Chain file is not supported.\nSetting Cable type to ATDH1150USB.` |
| W5 | warning | `Invalid JTAG instruction width in Chain file.\nSetting JTAG instruction width to 10 for device # <d>` |

## 2.3 Writer

ATMISP's *Save* writes, in text mode:

```
<N> 4 <K> 0 ␊          ← note the trailing space; K = current calibration constant
␊                       ← empty line 2
for each device:
  <NAME>␊
  <W>␊   if IDCODE = yes      |   <W> *␊   if IDCODE = no
  <instruction digit>␊
  <JED>␊   (an empty line if there is no JEDEC path)
```

Integers are written with `itoa(·, 10)`. A conforming writer MUST produce exactly this text (with
`CR LF` terminators). Reading a file written this way and writing it again MUST be the identity.

## 2.4 Data model

```
Chain   { calibration: 1..1000, devices: [Device; 1..50] }
Device  { name: String, ir_width: 1..99, idcode: bool,
          instruction: one of the 10 names | "" , jed_path: String }
```

`idcode` has no effect on SVF output (it only influences hardware-mode chain detection).
