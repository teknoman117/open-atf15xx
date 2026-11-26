# Yosys plugin

`atf15xx.so` adds the pass **`synth_atf15xx`** to Yosys. Its implementation is Rust
(`atf15xx-synth`, feature `yosys`: `src/plugin.rs`, `src/plugin/rtlil.cc`), connected to Yosys through a [`cxx`](https://cxx.rs) bridge rather
than a JSON text interchange (spec F9.7). It works in four steps:

1. **Rust drives Yosys.** The flow's synthesis script (single-sourced in `atf15xx-synth`) runs
   through `Pass::call`.
2. **Rust reads RTLIL in-process.** Modules, ports, cells and wires cross the bridge as shared
   structs, with the JSON backend's bit numbering and value encoding.
3. **The design is fitted.** The flow fits it and writes `<top>.jed`, `.svf`, `.fit`, `.pin`,
   `.pins.json`, `.map.json` and `.timing.rpt`.
4. **The placement goes back into the design.** Every placed wire gets `atf15xx_mc` and
   `atf15xx_pin` attributes (`atf15xx_mc_<bit>` for a bit of a vector), so
   `select a:atf15xx_mc`, `stat` and `write_verilog` see it.

```
docker/yosys/run.sh tools/yosys-plugin/build.sh          # -> tools/yosys-plugin/atf15xx.so
docker/yosys/run.sh yosys -m tools/yosys-plugin/atf15xx.so \
    -p 'read_verilog -sv d.v; synth_atf15xx -top d -part ATF1508AS-PLCC84 [-c d.acf] -o out [-fmax 66.6] [-check-json]'
```

Options: `-top`, `-part`, `-c <acf>`, `-o <dir>`, `-fmax <MHz>`, `-grade <-10>`, `-no-toggles`,
`-option KEY=VALUE` (as the CLI's `--option`), `-no-annotate`, and `-check-json`. The last one
also builds the netlist through Yosys's JSON backend and fails unless the two netlists are
identical.

Checked (2026-11-24): `-check-json` passes on all 34 open-flow corpus builds and on
SELFPROGRAMBOARD, and every JED is fuse-identical to `atf15xx-synth build`'s
(`check_corpus.sh`). Yosys's internal names embed the source path as given, and the fit follows
names, so the CLI runs Yosys in the sources' directory and reads them by file name; read them
the same way in Yosys (`cd` there, `read_verilog -sv design.v`) for results identical to the CLI's
(spec F9.7).


## Feasibility probe

`atf15xx_probe.cc` is a minimal Yosys pass built against the pinned Yosys (v0.71, `docker/yosys`):

```
docker/yosys/run.sh sh -c 'cd tools/yosys-plugin && yosys-config --build atf15xx_probe.so atf15xx_probe.cc'
```

The C++ API follows the Yosys version (0.71 moves names into a "twine" pool:
`design->twines().unescaped_str(id)`; `log_id` is gone), so the bridge must be rebuilt and
checked (`-check-json`, `check_corpus.sh`) on every Yosys bump.
