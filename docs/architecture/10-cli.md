# 10. `atf15xx-tool` command-line interface

`atf15xx-tool` is the reference, command-line-only replacement for ATMISP's SVF/JEDEC functions,
implemented in Rust (module `atf15xx_tool::isp`, formerly the `atmisp` crate). These commands never
talk to hardware; the same binary also has the in-circuit debugging commands (`part`, `idcode`,
`probe`, `sample`, `watch`, `capture`, `drive`, `seq`), specified in
[D1](debug/01-boundary-scan.md).

## 10.1 Commands

```
atf15xx-tool svf   <CHAIN.chn | --device SPEC ...> -o <OUT.svf> [SVF options] [common options]
atf15xx-tool chain show   <CHAIN.chn>
atf15xx-tool chain create --device SPEC ... -o <OUT.chn> [--calibration K]
atf15xx-tool jed info     <FILE.jed>
```

### `svf` — conversion C1 (and C2 for `Load` devices)

Reads the chain (from a file, or built from `--device` options), validates it (§2, §9), reads the
JEDEC files (§3), writes the SVF (§6, §7) and, for every `Load` device, writes its JEDEC file (§8).

| Option | Default | Meaning |
|---|---|---|
| `-o, --output PATH` | required | SVF file to write |
| `--rev C\|D` | `D` | SVF revision (§6.5) |
| `--tck N` | `1` | TCK period in µs, 1–1000; other values are an error |
| `--compat-tck` | off | replace an out-of-range `--tck` by 1 with a warning, as ATMISP does |
| `--no-state-reset` | state reset on | omit `STATE RESET;` lines |
| `--allow-mixed-voltage` | off | proceed when check V2 (§9.1) fires (equivalent to answering *Yes* in ATMISP); without it V2 is an error |
| `--strict` | off | correct the defects marked † in §9.4 |
| `--timestamp TEXT` | local `ctime()` | text for the `Created on:` lines; if absent and `SOURCE_DATE_EPOCH` is set, that time (UTC) is used |
| `--no-load-output` | off | do not write JEDEC files for `Load` devices |
| `--drive X=DIR` | — | map Windows drive `X:` to directory `DIR` when resolving JEDEC paths (repeatable) |
| `-d, --device SPEC` | — | build the chain from devices instead of a chain file (§10.2, repeatable) |
| `-q, --quiet` | off | suppress the transcript (§9.3) |

### `chain show`

Parses the chain file exactly as §2 specifies, prints every diagnostic (§2.2.3) and a normalised
listing (index, name, width, IDCODE flag, instruction, JEDEC path). Exit status reflects errors only.

### `chain create`

Builds a chain from `--device` options and writes it with the §2.3 writer.

### `jed info`

Parses a JEDEC file (§3) and prints: QF, family, default fuse value, `G`, the checksum stated in
the file, the computed checksum, the JTAG-enable fuse value and whether the file would be accepted
for each device name.

## 10.2 `--device SPEC`

```
SPEC := NAME ':' INSTRUCTION [ ':' JEDPATH ] [ ':w=' WIDTH ] [ ':noid' ]
```

* `NAME` — a device name from §4.1 (case-sensitive) or `UNKNOWN`.
* `INSTRUCTION` — a digit `0`–`9` or a name from §1.3, case-insensitive, with `/` and spaces
  optional (`pv`, `program/verify`, `blank-check`, `read-ues`, … are accepted aliases).
* `w=WIDTH` — IR width (default 10; meaningful for `UNKNOWN`).
* `noid` — clear the IDCODE flag (written as ` *` in the chain file).

Devices are numbered in the order given.

## 10.3 JEDEC path resolution

Chain files written on Windows contain absolute Windows paths. A path is resolved by trying, in
order: (1) the path verbatim; (2) with `\` replaced by `/`, and a leading `X:` replaced via
`--drive`; (3) relative paths are taken relative to the chain file's directory. The first existing
file is used; if none exists the chain error E3 is reported. For `Load`, the output path is
resolved the same way (the file need not exist; its directory must).

## 10.4 Exit status

| Code | Meaning |
|---|---|
| 0 | success (the SVF and any JEDEC outputs were written). An ATMISP transcript ending in `failed` because of SVF-mode artefacts (§9.3) is still success. |
| 1 | input rejected: E1–E3, J1, J2, N1, N2, V1, V2 without `--allow-mixed-voltage`, invalid `--tck` |
| 2 | command-line usage error (reported by the argument parser) |
| 3 | I/O error writing an output |

Warnings (W1–W8) are printed to standard error and do not change the exit status. No output file is
created or modified when the exit status is 1 or 2.

## 10.5 Output guarantees

* In default (compatibility) mode, the SVF is byte-identical to ATMISP 7.3's for the same chain and
  options, except the `Created on:` line (§11).
* Output is written atomically (temporary file + rename).
