# atf15xx-fit

Fitter core (back end) for ATF150xAS CPLDs, front-end agnostic:

* `tt2::Pla` — CUPL `.tt2` reader (docs/architecture/fitter/03-tt2-format.md);
* `netlist::Design` — IR: SOP cells with D/T/latch storage, clock/CE/AR/AP/OE and pin/node
  constraints, plus vendor-like transformations (comb-node collapse, register absorption,
  special-function legalisation, sum splitting);
* `logic` — SOP algebra and a small minimiser;
* `fit::fit_design(device, design, &Options)` — placement, global nets, PT allocation (cascade,
  foldback, logic doubling), UIM routing, fuse assembly → `FitResult`;
* `jed` — JEDEC writer (`write`, region-structured, our own layout) and reader (`parse`);
* `cli` — the shared command line of the `fit1502`/`fit1504`/`fit1508` binaries (below).

Tests: `tests/conformance.rs` (95 synthetic designs, fuses and checksum identical to the vendor's required; decode test).
Example: `cargo run --example jed_rewrite -- <.jed files>` rewrites JEDs in our layout (fuses kept).

## `atf15xx-fit`

```
atf15xx-fit design.tt2 -d ATF1504AS-PLCC44 [-o out/design] [--preassign try|keep|ignore]
    [--no-jtag] [--security] [--output-fast all|NAMES] [--open-collector all|NAMES]
    [--observable-registers] [--set KEY=VALUE]... [--created TEXT] [-q]
```

The family comes from the device, which may be written as a part (`ATF1504AS-PLCC44`, or an
ordering code such as `ATF1504AS-10JU44`), a fitter name (`P1504C44`) or a CUPL name
(`f1504plcc44`). Writes `<out>.jed`, `.fit`, `.pin` and `.map.json` (default: next to the design).
Every vendor strategy below is available as `--set KEY=VALUE`, and later settings win; defaults are
the vendor's. For the same settings the JEDEC file equals `fit150x`'s byte for byte (all 95
conformance designs, 2026-11-25).

## Binaries: `fit1502`, `fit1504`, `fit1508`

Open replacements for Atmel's `fit1502.exe`/`fit1504.exe`/`fit1508.exe` (ATF1502AS/1504AS/1508AS),
built from this crate (`src/bin/`; `cargo build --release -p atf15xx-fit` puts them in
`target/release/`):

```
fit1504 design.tt2 -device <DEVICE> [-preassign keep|try|ignore] [-strategy KEY VALUE]... [-created TEXT]
```

Reads a CUPL `.tt2` and writes `design.jed`, `design.fit` and `design.pin` next to it. Arguments
follow the vendor fitter: `-strategy KEY VALUE` and `-KEY VALUE` are both accepted; the first
occurrence of a repeated key wins (as in the vendor fitter); `#$ PROPERTY ATMEL` lines in the
`.tt2` apply first. Supported keys: `device`, `preassign`, `JTAG`, `pin_keep`, `output_fast`,
`open_collector`, `MC_power`, `security`, `pd1`, `pd2`, `power_reset`, `TDI_pullup`, `TMS_pullup`,
`GCK_ITD`, `Cascade_Logic`, `Foldback_Logic`, `logic_doubling`; other vendor keys are accepted and
ignored. `-created` sets the JEDEC `Created:` text (default: now, ctime format).

Devices: see `atf15xx-arch/data/devices.json` (e.g. `P1502C44`, `P1504C44`, `P1508C84`, also CUPL
names like `f1504plcc44`). Specification: `docs/architecture/fitter/`; status and match rates:
`docs/worklog/02-fitter.md`.

