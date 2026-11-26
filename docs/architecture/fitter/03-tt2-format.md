# F3. The CUPL `.tt2` input format

`.tt2` is what CUPL (`cupl.exe -... ` from WinCUPL 5.0a) hands to the fitters: a Berkeley PLA
(espresso) file with CUPL-specific `#$` header lines and dotted signal suffixes. Lines end in CRLF.
The grammar below covers everything CUPL emitted for the user's designs and our synthetic corpus;
`atf15xx-fit/src/tt2.rs` implements it.

```
file      := line*
line      := comment | header | directive | cube | blank
comment   := '#' text                       -- '#' not followed by '$'; ignored
header    := '#$' WS key rest
directive := '.' key args
cube      := inbits WS outbits
```

## Header lines (`#$`)

| key | form | meaning |
|---|---|---|
| `TOOL` | `#$ TOOL CUPL` | producer |
| `TITLE`, `MODULE`, `JEDECFILE` | name | design name (MODULE/JEDECFILE are truncated to 8 characters by CUPL) |
| `DEVICE` | `#$ DEVICE  f1504plcc44` | CUPL device name `f<family><package>` (`plcc44`, `tqfp44`, `plcc68`, `plcc84`, `qfp100`, `tqfp100`, `qfp160`); the fitter's `-device` (`P1504C44` …) overrides it |
| `PROPERTY` | `#$ PROPERTY ATMEL <option> <value…>` | a fitter option, same vocabulary as `-strategy` (e.g. `JTAG = OFF`, `pin_keep OFF`, `security on`, `preassign keep`) |
| `PINS` | `#$ PINS  <n> name±[:pin] …` | all pin signals; `+`/`-` is the declared polarity, `:pin` the pin constraint (absent = floating) |
| `NODES` | `#$ NODES <n> name±[:node] …` | buried nodes; `:6nn` = feedback node of MC nn, `:3nn` = foldback node |

The `PINS`/`NODES` lists are single (possibly very long) lines; CUPL lists signals alphabetically.

## Directives

| directive | meaning |
|---|---|
| `.i n` / `.o n` | number of input / output columns |
| `.type f` | PLA type (`f` from CUPL; the fitter's own `.tt3` output uses `fr`) |
| `.ilb l1 l2 …` | input labels (see suffixes) |
| `.ob l1 l2 …` | output labels (see suffixes) |
| `.phase bits` | one character per output; CUPL always writes `1` |
| `.p n` | number of cubes |
| `.e` | end |

## Cubes

`inbits` has one character per `.ilb` label: `1` (true literal), `0` (complement), `-` (absent).
`outbits` has one character per `.ob` label: `1` = this product term belongs to that output's sum,
`0` = not. Each output is the OR of its cubes; an output with no cube is constant 0; a cube with no
literals is constant 1.

## Suffixes

Inputs (`.ilb`):

| label | means | fitter source |
|---|---|---|
| `X` (X an input pin) | pad of X | `Xp` |
| `X` (X a node or output) | combinatorial feedback of X | `Xfb` (the vendor routes the pad instead when X is an always-enabled output) |
| `X.Q` | register output of X | `Xfb` (or the pad, as above) |
| `X.PIN` | pad of output X | `Xp` |

Outputs (`.ob`); a trailing `-` (e.g. `Y-`, `Y.REG-`) marks the function as active-low (the pin /
register gets the complement):

| label | function |
|---|---|
| `Y` | combinatorial output / node |
| `Y.D`, `Y.REG` | D flip-flop input |
| `Y.T` | T flip-flop input |
| `Y.L` | latch data |
| `Y.C` (`.CK`) | clock (flip-flop) |
| `Y.LE` | latch enable |
| `Y.CE` | clock enable |
| `Y.AR` | asynchronous reset |
| `Y.AP` | asynchronous preset |
| `Y.OE` | output enable |

Vendor interpretation quirks (reproduced by our fitter): an `.OE` output with no cube (CUPL
`Y.OE = 'b'0`) is treated as **always enabled**; `-strategy` lists may be comma- or
space-separated (`-open_collector OUT0,OUT1` or `= OUT0 OUT1`), but the vendor silently truncates
long lists (F6.5).

## Minimal example

```
#$ DEVICE  f1504plcc44
#$ PINS  3 A+:4 B+ Y+:12
.i 2
.o 1
.type f
.ilb  A B
.ob   Y
.phase 1
.p 1
11 1
.e
```

`tools/fuzzers/common/tt2gen.py` writes files in this form for experiments.
