# open-atf150x — Architecture & Behavioural Specification

This directory specifies, precisely enough to build a byte-compatible implementation, every data
conversion performed by **Microchip/Atmel ATMISP 7.3** (`ATMISP.exe`, *ATF15xx CPLD ISP Software*,
file version 7.3.0.0, March 2020) when it is used to generate **SVF** files, plus the reference
command-line tool (`atf15xx-tool`) that replaces the GUI.

ATMISP is a native 32-bit MFC/C++ Windows GUI program (it is *not* a .NET assembly; its PE CLR
header is empty). Its only headless mode (`ATMISP.exe file.chn -1`) drives the ATDH1150USB cable and
cannot produce SVF. Everything in this specification is established by black-box comparison
with the real program: SVF and JEDEC files that ATMISP wrote for generated inputs (the *oracle*,
see [11-conformance-testing.md](11-conformance-testing.md) and `tools/fuzzers/`).

## Document map

| # | Document | Contents |
|---|----------|----------|
| 1 | [01-system-overview.md](01-system-overview.md) | Inputs, outputs, the conversions, processing pipeline, terminology |
| 2 | [02-chain-file-format.md](02-chain-file-format.md) | `.chn` chain-file grammar (reader and writer), field semantics |
| 3 | [03-jedec-input.md](03-jedec-input.md) | How JEDEC (`.jed`) fuse files are read into a fuse array |
| 4 | [04-device-database.md](04-device-database.md) | Device names, families, IDCODEs, voltage classes, geometry, opcodes |
| 5 | [05-fuse-maps.md](05-fuse-maps.md) | Fuse-array → programming-row maps for all six families |
| 6 | [06-svf-writer.md](06-svf-writer.md) | SVF text format, primitives, bit/hex encoding, delays, chain composition |
| 7 | [07-programming-algorithms.md](07-programming-algorithms.md) | The run algorithm and every per-instruction / per-family procedure |
| 8 | [08-jedec-output.md](08-jedec-output.md) | The `Load` instruction's JEDEC writer (rows → fuse array → `.jed`) |
| 9 | [09-validation-and-diagnostics.md](09-validation-and-diagnostics.md) | Checks, messages, and the catalogue of ATMISP defects that must be reproduced |
| 10 | [10-cli.md](10-cli.md) | The `atf15xx-tool` command-line interface (Rust reference implementation) |
| 11 | [11-conformance-testing.md](11-conformance-testing.md) | Oracle method, test vectors, acceptance criteria |
| D | [debug/01-boundary-scan.md](debug/01-boundary-scan.md) | In-circuit debugging over boundary scan: cell maps from the BSDL, SAMPLE/EXTEST operations, naming with a design (`atf15xx-tool`) |
| F | [fitter/](fitter/README.md) | The fitters before ATMISP: `.tt2` → `.jed` for 1502AS/1504AS/1508AS — chip architecture, complete fuse semantics, `.tt2` grammar, fitting algorithm, conformance |

## Conformance language

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT** and **MAY** are used as in RFC 2119.

An implementation is **ATMISP-compatible** when, for every valid input accepted by ATMISP 7.3, it
produces an SVF file that is byte-identical to the one ATMISP writes, except for the
`//  Created on:` timestamp line (and, for `Load`, the `Created on:` line of the JEDEC file it writes).
Where ATMISP's own output is defective (for example it prints a non-hexadecimal character into an
SVF scan), compatibility mode reproduces the defect and the defect is listed in
[09-validation-and-diagnostics.md §4](09-validation-and-diagnostics.md#4-catalogue-of-atmisp-defects);
an implementation MAY additionally offer a *strict* mode that corrects it.

## Notation used throughout

* **Bit strings** are written the way ATMISP stores them internally: a string of the characters
  `0`/`1` whose **index 0 is the least-significant bit** (the first bit shifted into TDI).
  `bits(v, n)` denotes the *n*-character LSB-first string of the integer *v*.
  Example: `bits(0x290, 10)` = `"0000100101"`.
* `a ++ b` is string concatenation; `"1"×n` is *n* repetitions.
* **Opcodes** are 10-bit JTAG instruction values written in hex (e.g. `0x2A1`); in a scan they are
  always encoded with `bits(op, 10)`.
* Fuse numbers are the JEDEC `L` addresses (0-based). `F[i]` is the character of fuse *i* in the
  fuse array.
* Behaviour is specified by what ATMISP writes; the evidence for each chapter is a set of oracle
  vectors or fuzzing results (§11).
