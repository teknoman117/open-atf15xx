# Fitter specification: `.tt2` → `.jed` for ATF1502AS / ATF1504AS / ATF1508AS

This part of the specification covers what happens *before* ATMISP: the Atmel fitters
(`fit1502.exe`, `fit1504.exe`, `fit1508.exe`, WinCUPL 5.0a, version 1.8.7.8) that place a CUPL
`.tt2` netlist into a device and write the JEDEC file. With [§5 fuse maps](../05-fuse-maps.md) and
[§6 SVF writer](../06-svf-writer.md) it gives the complete chain `.tt2 → .jed → .svf`.

| # | Document | Contents |
|---|---|---|
| F1 | [01-chip-architecture.md](01-chip-architecture.md) | blocks, macrocells, PTs, UIM, globals; pin / node / buried-node numbering; per-package pin tables |
| F2 | [02-fuse-map.md](02-fuse-map.md) | every JEDEC fuse: PT array, macrocell bits, UIM, global OE, configuration; evidence levels |
| F3 | [03-tt2-format.md](03-tt2-format.md) | the `.tt2` input grammar and suffix semantics |
| F4 | [04-fitter-algorithm.md](04-fitter-algorithm.md) | netlist IR, vendor transformations, PT allocation, globals, placement, routing |
| F5 | [05-vendor-outputs.md](05-vendor-outputs.md) | exact `.jed` format; `.fit`/`.pin`/`.io`/`.tt3` |
| F6 | [06-conformance.md](06-conformance.md) | oracle, experiments, `verify_tt2.py` harness, match rates |
| F7 | [07-yosys-flow.md](07-yosys-flow.md) | Verilog → ATF15xx flow and prior art |
| F9 | [09-yosys-integration.md](09-yosys-integration.md) | Deeper Yosys integration and placement: findings, options, roadmap (exploration) |
| F8 | [08-timing.md](08-timing.md) | Timing: datasheet model, vendor speed-grade tables (all families/grades), vendor SDF composition, model for our tools |

Implementation:

* `atf15xx-arch` — architecture database (formulas + tables generated at build time from prjbureau and `data/`), no fitter logic.
* `atf15xx-fit` — back end library: `tt2` front end, `netlist` IR, `logic` minimiser,
  `fit` (placement, routing, assembly), `jed` (vendor-exact writer, reader), `cli`.
* `fit1502`, `fit1504`, `fit1508` — the three command-line fitters (vendor-compatible
  arguments: `fit1504 design.tt2 -device P1504C44 -preassign keep -strategy JTAG off …`, plus
  `-created TEXT` to fix the JEDEC timestamp).
* `tools/fuzzers/` — the experiments (one workflow per fact), `tools/fuzzers/common/fitter_oracle.py`,
  `tt2gen.py` (oracle wrapper) and `tools/fuzzers/fitter-conformance/verify_tt2.py` (conformance harness).

Builds run in Docker (`docker/Dockerfile`, `docker/run.sh cargo build --release --workspace`).

## Prior art and licences

whitequark's **prjbureau** (<https://github.com/whitequark/prjbureau>, 0BSD licence; clone at
commit `8b8a97122ec238b6a09868a4bc7def20acef798c` in `.scratch/upstream/prjbureau`, not vendored)
documents the ATF1502AS/1504AS (and BE) fuse database. We used its `database.json` as a
checklist and cross-check ([F2.7](02-fuse-map.md#f27-cross-check-against-prjbureau-1502as-1504as)):
it agrees with our oracle-derived map on every fuse it covers except the ATF1502AS global-OE invert
bits. No code or data from it is included. The study of its tools and Yosys flow is
[docs/prjbureau/README.md](../../prjbureau/README.md) (checkout `external/prjbureau`).
