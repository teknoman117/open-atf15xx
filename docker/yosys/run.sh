#!/bin/sh
# Run a command in the open-atf150x-yosys image (pinned Yosys, iverilog, Rust) with the workspace
# mounted at /work, as the host user. Cargo caches and targets live in .scratch/ (git-ignored).
#   docker/yosys/run.sh yosys -V
#   docker/yosys/run.sh cargo run -p atf15xx-synth -- build design.v --device ATF1504AS-PLCC44 -o out/
set -e
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
mkdir -p "$ROOT/.scratch/cargo-home" "$ROOT/.scratch/target-yosys"
TTY=; [ -t 0 ] && TTY=-it
exec docker run --rm $TTY -u "$(id -u):$(id -g)" -v "$ROOT":/work -w /work \
  -e HOME=/work/.scratch -e CARGO_HOME=/work/.scratch/cargo-home -e CARGO_TARGET_DIR=/work/.scratch/target-yosys \
  "${IMAGE:-open-atf150x-yosys}" "$@"
