# 7. Programming algorithms

This chapter specifies the exact sequence of primitives (§6.4) ATMISP emits. Everything here is
fixed by ATMISP's own SVF output: the oracle vectors reproduce it byte for byte for every family and
instruction (§11).

Notation: `x` is the target device's 1-based chain index, `D = Dx` its record. `CIR`, `CDR`, `CADR`,
`ASADDR`, `BEADDR` are defined in §6.6–§6.7; `op` arguments are opcodes from §4.5.

## 7.1 Top-level run

```
HEADER                                   # §6.3
INIT                                     # §7.2.1
ISC_ENABLE                               # §7.2.2
IDCODE_SCAN                              # §7.2.3

# phase 1 — erase every device that will be erased
for x = 1..N:
    if D.instr ∈ {Erase, Program/Verify, Program/Verify/Secure}:
        ERASE(x); REINIT

# phase 2 — per-device operation
for x = 1..N:
    case D.instr of
      Load:                   REINIT; VERIFY_SECURE(x); VERIFY(x)
      Program/Verify:         PROGRAM(x); VERIFY(x); if D.security = '1' then SECURE(x)
      Program/Verify/Secure:  PROGRAM(x); VERIFY(x); SECURE(x); REINIT; VERIFY_SECURE(x)
      Secure:                 SECURE(x); REINIT; VERIFY_SECURE(x)
      Verify:                 VERIFY(x)
      Verify Secure:          REINIT; VERIFY_SECURE(x)
      Blank Check:            VERIFY(x); REINIT; VERIFY_SECURE(x)
      Erase:                  VERIFY(x)              # the blank check after erase
      Read UES:               READ_UES(x)
      Bypass, "" (unknown):   (nothing)

ISC_DISABLE                               # §7.2.4
FINAL                                     # §7.2.5
```

`D.security` is the JEDEC `G` value (§3.3), so it is `'1'` only if the device's JEDEC file was read.
In SVF mode every read-back "succeeds", so no failure branch ATMISP has for hardware mode is ever
taken; the branches above are complete for SVF.

`REINIT` = `ISC_DISABLE; FINAL; INIT; ISC_ENABLE` (the IDCODE scan is **not** repeated).

## 7.2 Chain-wide procedures

### 7.2.1 INIT
```
DELAY(0,20,50); RESET; IDLE; DELAY(0,20,50)
```

### 7.2.2 ISC_ENABLE
Build `ir` and `dr` by prepending, for `i = 1..N`:

| Device `Di` | IR contribution | DR contribution |
|---|---|---|
| `isBEorRE(name)` | `bits(0x2A4,10)` | `bits(0x1B9,10)` |
| else `isUnknown(name)` | `"1"×width` | `"0"` |
| else | `bits(0x280,10)` | `bits(0x1B9,10)` |

Then `SIR(ir); SDR(dr); IDLE`.

### 7.2.3 IDCODE_SCAN
Build `ir` and `id` by prepending, for `i = 1..N`:
* IR: `bits(0x059,10)` if `isATF150(name)`, else `"1"×width`.
* expected: `bits(IDCODE(name), 32)` if the name has an IDCODE (§4.1), else `"1"`.

Let `ones = "1"×len(id)`. Emit `SIR(ir); SDR_TDO_MASK(ones, id, ones); IDLE`.

(An `UNKNOWN` device is in BYPASS here and really shifts out `0`, but ATMISP expects `1` with
mask `1` — defect D4. Compatibility mode reproduces it.)

### 7.2.4 ISC_DISABLE
As ISC_ENABLE but with: BE/RE → IR `bits(0x2A5,10)`, DR `"0"×10`; UNKNOWN → IR `"1"×width`,
DR `"0"`; others → IR `bits(0x280,10)`, DR `"0"×10`. Then `SIR(ir); SDR(dr); IDLE; DELAY(0,0,10)`.

### 7.2.5 FINAL
```
RESET; DELAY(0,20,50)
```

## 7.3 AS-type procedures (`isAS(name)`)

Let `last` = `0xE4` (1502AS), `0xE8` (1504AS), `0xFA` (1508AS), chosen by the QF the **name**
implies (§4.3); the row ranges are processed in this order:

```
RANGES = [ (0x0C,0x6B), (0x80,0xDF), (0x00,0x0B), (0xE0,last) ]
```

### ERASE
```
SIR(CIR(x,0x2B3)); SIR(CIR(x,0x29E)); IDLE; DELAY(0,0,210)
SIR(CIR(x,0x2BF)); IDLE; RESET; DELAY(0,0,50); IDLE
```

### PROGRAM
```
for (r0,r1) in RANGES:
    for r = r0..r1:
        SIR(CIR(x,0x2A1)); SDR(CADR(x, ASADDR("000", r)))
        SIR(CIR(x,0x290)); SDR(CDR(x, rows[r]))
        SIR(CIR(x,0x29E)); IDLE; DELAY(0,0,30)
        SIR(CIR(x,0x2BF))
    IDLE
PROG_REG(special, 0x292, "010")
PROG_REG(ues,     0x293, "110")
PROG_REG(config,  0x291, "100")

PROG_REG(data, op, prefix):
    SIR(CIR(x,0x2A1)); SDR(CADR(x, ASADDR(prefix, 0)))
    SIR(CIR(x,op));    SDR(CDR(x, data))
    SIR(CIR(x,0x29E)); IDLE; DELAY(0,0,30)
    SIR(CIR(x,0x2BF)); IDLE
```

### VERIFY
Used by `Verify`, `Program/Verify`, `Program/Verify/Secure` (with MASK), and by `Erase`,
`Blank Check`, `Load` (TDO only). Define

```
CHECK(b): if D.instr ∈ {Program/Verify, Verify, Program/Verify/Secure}
              then SDR_TDO_MASK(b, b, "1"×len(b))
              else SDR_TDO(b, b)
```
Then:
```
for (r0,r1) in RANGES:
    for r = r0..r1:
        SIR(CIR(x,0x2A1)); SDR(CADR(x, ASADDR("000", r)))
        SIR(CIR(x,0x28C)); IDLE; DELAY(0,0,20)
        SIR(CIR(x,0x290)); CHECK(CDR(x, rows[r]))
    IDLE
VER_REG(special, 0x292, "010"); VER_REG(ues, 0x293, "110"); VER_REG(config, 0x291, "100")

VER_REG(data, op, prefix):
    SIR(CIR(x,0x2A1)); SDR(CADR(x, ASADDR(prefix, 0)))
    SIR(CIR(x,0x28C)); IDLE; DELAY(0,0,20)
    SIR(CIR(x,op)); CHECK(CDR(x, data)); IDLE
```
For `Erase`, `Blank Check` and `Load`, `rows`/registers are the default image (all `'1'`, §3.5).

### SECURE
```
PROG_REG("1110", 0x292, "010")       # constant, not taken from the JEDEC file
```

### VERIFY_SECURE
```
SIR(CIR(x,0x2A1)); SDR(CADR(x, ASADDR("100", 0)))
SIR(CIR(x,0x28C)); IDLE; DELAY(0,0,20)
SIR(CIR(x,0x291)); SDR(CDR(x, "0"×33)); IDLE      # 33 bits: defect D2
```

## 7.4 BE-type data helpers

* `ROW8(r)` = `reverse(rows[r])` (string index 0 = column 7).
* `PAGE(p, off)` = `ROW8(off+32p) ++ ROW8(off+32p+1) ++ … ++ ROW8(off+32p+31)` (256 chars).
* Bank offsets: bank 1 `off = 0`, bank 2 `off = 4812` (1508BE/RE only). Bank select opcode
  `SEL` = `0x290` (bank 1) or `0x294` (bank 2).
* `HS(sel)` = `SIR(CIR(x,sel)); IDLE; DELAY(100,0,0)` — prints `RUNTEST 1E-6 SEC;` in Rev D.
* In BE sequences below, `S(op)` means `SIR(CIR(x,op))` and `D(bits)` means `SDR(CDR(x,bits))`.

Per-family ranges (bounds inclusive):

| `beKind` | Pages (bank 1) | Rows (bank 1) | Pages (bank 2) | Rows (bank 2, relative) | Secure row |
|---|---|---|---|---|---|
| 1502BE | 0…0x40 | 0x81E…0x829 | — | — | 0x827 |
| 1504BE | 0…0x83 | 0x1080…0x109B | — | — | 0x1099 |
| 1508 (BE/RE) | 0…0x95 | 0x12C0…0x12CB | 0…0x88 | 0x1120…0x113D | 0x113B (bank 2) |

Bank-2 row numbers are relative: the address sent is the relative number, the data comes from
`rows[4812 + r]`. Pages and single rows overlap (e.g. 1502BE page 0x40 covers rows 2048–2079 and
rows 2078/2079 are written again individually); this is ATMISP behaviour.

## 7.5 BE-type procedures (`not isAS(name)`)

### ERASE
```
S(0x2A4); D(bits(0x1B9,10)); IDLE
S(0x293); IDLE; DELAY(0,0,1800)
S(0x2A5); D("0"×10); IDLE
```

### PROGRAM
```
PROG_PAGES(bank1 pages); PROG_ROWS(bank1 rows)
if beKind = 1508: PROG_PAGES(bank2 pages, bank 2); PROG_ROWS(bank2 rows, bank 2)
```

`PROG_PAGES` — for each page `p`:
```
HS(SEL)
S(0x292); D("00000110"); IDLE; CLOCKS(8); CLOCKS(10); [bank 2 only: DELAY(100,0,0)]
HS(0x291)
HS(SEL)
S(0x292); D("00000010"); IDLE; CLOCKS(8); CLOCKS(5)
S(0x2A1); D(BEADDR(32·p)); IDLE; CLOCKS(16)
S(0x2A2); D(PAGE(p, off)); IDLE; CLOCKS(256)
S(0x2A6); IDLE; DELAY(100,0,10)
HS(0x291)
```

`PROG_ROWS` — for each row `r`:
```
HS(SEL)
S(0x292); D("00000110"); IDLE;
    bank 1: CLOCKS(8); DELAY(100,0,0); CLOCKS(10)
    bank 2: CLOCKS(8); CLOCKS(10); DELAY(100,0,0)
HS(0x291)
HS(SEL)
S(0x292); D("00000010"); IDLE; CLOCKS(8); [bank 2 only: CLOCKS(5)]
S(0x2A1); D(BEADDR(r)); IDLE; CLOCKS(16)
S(0x2A3); D(ROW8(off + r)); IDLE; CLOCKS(8)
S(0x2A6); IDLE; DELAY(100,0,10)
HS(0x291)
```

### VERIFY
Same ranges as PROGRAM. `CHECK` is as in §7.3.

For each page `p`:
```
HS(SEL)
S(0x292); D("00000011"); IDLE; CLOCKS(8)
S(0x2A1); D(BEADDR(32·p)); IDLE; CLOCKS(16)
S(0x2A2); CHECK(CDR(x, PAGE(p, off)))
IDLE; DELAY(100,0,0); HS(0x291)
```
For each row `r`: identical, but with `BEADDR(r)`, opcode `0x2A3` and `CHECK(CDR(x, ROW8(off+r)))`.

### BE_READ(r, bank)
```
HS(SEL)
S(0x292); D("00000011"); IDLE; CLOCKS(8)
S(0x2A1); D(BEADDR(r)); IDLE; CLOCKS(16)
S(0x2A3)
bank 1:  b = CDR(x, "0"×9);  SDR_TDO(b, b)        # 9 bits: defect D2
bank 2:  defect D3 — see below
IDLE; DELAY(100,0,0); HS(0x291)
```
Bank-2 read (defect D3): ATMISP fills an 8-byte buffer with `'0'` without a terminator and a
ninth, uninitialised byte follows. With the chain composed by `CDR`, the scan is 9 bits for the
device plus one bit per other device, and every hex digit whose 4-bit group contains the ninth bit
is printed as `` ` `` (backtick). For a single-device chain the output is exactly
```
SDR 9 TDI (`00)
	TDO (`00);
```

### SECURE
The three tests are independent (not else-if):
```
if "1502BE" ∈ name: BE_READ(0x827, bank 1)
if "1504BE" ∈ name: BE_READ(0x1099, bank 1)
if "1508BE" ∈ name or "1508RE" ∈ name: BE_READ(0x113B, bank 2)
```
In hardware mode ATMISP would then program the security byte depending on the read value; in SVF
mode the read value is never set, so **no program step is emitted** (verified for all BE families).
(For names that are neither AS-type nor contain `BE`/`RE` — only `ATF1502SE`/`SEL` — nothing is
emitted.)

### VERIFY_SECURE
```
BE_READ(0, bank 1)                      # only if isBEorRE(name)
```

## 7.6 Read UES
```
SIR(CIR(x,0x270)); IDLE; DELAY(0,0,20)
CHECK(CDR(x, "0"×16)); IDLE
```
(`CHECK` uses the `TDO`-only form, since the instruction is `Read UES`.)

## 7.7 Worked example (single ATF1504AS, `Verify`, Rev D, state reset on)

```
<header>
RUNTEST 50021E-6 SEC;          INIT
STATE RESET;
STATE IDLE;
RUNTEST 50021E-6 SEC;
SIR 10 TDI (280);              ISC_ENABLE
SDR 10 TDI (1b9);
STATE IDLE;
SIR 10 TDI (059);              IDCODE_SCAN
SDR 32 TDI (ffffffff)
	TDO (0150403f)
	MASK (ffffffff);
STATE IDLE;
SIR 10 TDI (2a1);              VERIFY, row 0x0C
SDR 11 TDI (00c);
SIR 10 TDI (28c);
STATE IDLE;
RUNTEST 20001E-6 SEC;
SIR 10 TDI (290);
SDR 166 TDI (<row 12>)
	TDO (<row 12>)
	MASK (3fffffffffffffffffffffffffffffffffffffffff);
…
```
