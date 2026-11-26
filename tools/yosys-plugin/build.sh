#!/bin/sh
# Build the ATF15xx Yosys plugin against the pinned Yosys (run inside docker/yosys/run.sh):
#   docker/yosys/run.sh tools/yosys-plugin/build.sh    -> tools/yosys-plugin/atf15xx.so
# The pass and the RTLIL bridge live in atf15xx-synth behind feature `yosys` (Rust + cxx, spec
# F9.7); the static library is linked whole so that the pass's static registration object is kept.
set -e
cd "$(dirname "$0")/../.."
TARGET=${CARGO_TARGET_DIR:-target}/yosys-plugin
cargo rustc --release -p atf15xx-synth --features yosys --lib --crate-type staticlib --target-dir "$TARGET"
LIB=$(cd "$TARGET/release" && pwd)
cd tools/yosys-plugin
yosys-config --build atf15xx.so -Wl,--whole-archive "$LIB/libatf15xx_synth.a" -Wl,--no-whole-archive -ldl -lpthread -lm
