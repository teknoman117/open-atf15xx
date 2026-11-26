# 1. System overview

## 1.1 What ATMISP does

ATMISP programs Atmel/Microchip **ATF15xx** CPLDs over JTAG (IEEE 1149.1 + the ATF15xx ISC
extensions). The user describes the physical JTAG chain in a **chain file** (`.chn`): an ordered list
of devices, each with a device name, an instruction-register width, a *JTAG instruction* (the
operation to perform) and, where needed, a JEDEC fuse file. ATMISP then either

* drives the instructions through an FTDI-based ATDH1150USB cable (hardware mode, *out of scope*), or
* **writes the same JTAG traffic to an SVF file** (Serial Vector Format, revision C or D) for
  playback on ATE / third-party JTAG players (*in scope*).

## 1.2 The conversions

| Conversion | Inputs | Output | Spec |
|---|---|---|---|
| **C1 — chain → SVF** | `.chn` + referenced `.jed` files + SVF options (revision, TCK period, state-reset flag) | `.svf` | §2–§7 |
| **C2 — Load → JEDEC** | a chain device whose instruction is `Load` | `.jed` written to the device's JEDEC path | §8 |
| **C3 — chain-file read/write** | GUI edits / `.chn` | `.chn` | §2 |
| **C4 — JEDEC → programming rows** (internal) | `.jed` | per-device row arrays, consumed by C1 | §3, §5 |

In SVF mode no device is actually read, so the data `Load` writes is a constant "blank" image
(all programmable fuses `1`); C2 is nevertheless fully specified because ATMISP performs it, and
because the same writer would be used with real read-back data.

The *Read UES*, *Verify*, *Blank Check* and *Verify Secure* instructions only produce SVF scans with
expected TDO values; they generate no other file.

## 1.3 Instructions

The chain file stores the instruction as a digit. Names are exactly as ATMISP spells them; they are
used verbatim in the algorithms of §7.

| Code | Name | Needs JEDEC | Effect in SVF |
|---|---|---|---|
| 0 | `Bypass` | no | device kept in BYPASS |
| 1 | `Program/Verify` | yes (read) | erase, program, verify; plus secure if the JEDEC `G` field is `1` |
| 2 | `Erase` | no | erase, then blank-check scans |
| 3 | `Blank Check` | no | blank-check scans, then secure-status read |
| 4 | `Verify` | yes (read) | verify scans with expected data and MASK |
| 5 | `Load` | path only (written) | secure-status read, read-back scans; JEDEC file written (§8) |
| 6 | `Secure` | no | set security bit, then secure-status read |
| 7 | `Program/Verify/Secure` | yes (read) | erase, program, verify, secure, secure-status read |
| 8 | `Verify Secure` | no | secure-status read |
| 9 | `Read UES` | no | User Electronic Signature read scan |

ATMISP logs a warning that SVF generation "cannot be used with" `Secure`, `Program/Verify/Secure`
and `Verify Secure`, but it still writes the SVF; see §9.

## 1.4 Processing pipeline (conversion C1)

```
 .chn ──► chain-file reader (§2) ──► chain: [Device{name, irWidth, idcodeFlag, instr, jedPath}]
                                           │
 .jed ──► JEDEC reader (§3) ──► fuse array F[0..QF) ──► fuse-map builder (§5) ──► rows / config /
                                           │                                     special / UES
                                           ▼
                         run algorithm (§7) — emits primitives (§6):
                         SIR, SDR(+TDO/MASK), STATE, RUNTEST
                                           │
                                           ▼
                                SVF writer (§6) ──► .svf (CRLF text)
```

* The **chain** is processed as a whole: every scan covers every device. The device being operated
  on contributes its opcode/data; every other device is put in BYPASS (§6.6).
* Devices are numbered **1…N in chain-file order**. Device 1 is the device whose TDI is connected
  to the cable's TDI; in every scan its bits are the **most significant** (shifted last).

## 1.5 Terminology

| Term | Meaning |
|---|---|
| **Family** | One of `1502AS`, `1504AS`, `1508AS`, `1502BE`, `1504BE`, `1508BE` (the latter also used for ATF1508RE). Determines fuse count, row geometry and algorithms. |
| **AS-type** / **BE-type** | AS-type families program whole rows (86/166/326 bits) with 11-bit addresses; BE-type families program 8-bit rows in 32-row pages with 16-bit addresses. |
| **QF** | JEDEC fuse count (`QF` field). |
| **Row** | One programming word of the device (`rows[r]`, a string of `rowLen` characters, index = column). |
| **Special bits / UES / config** | AS-type devices have three separately addressed registers: 4 special-purpose bits (security, JTAG enable, …), 16 UES bits and 32 configuration bits. In BE-type devices these live in ordinary rows. |
| **Delay(ns, µs, ms)** | ATMISP's wait primitive; see §6.4. |
