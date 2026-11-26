#!/bin/sh
# Run a command in the open-atf150x-sim container (Yosys, iverilog, Verilator, z3, Rust) with the
# workspace mounted at /work. Builds the image on first use.
#   tools/sim/run.sh python3 tools/sim/check.py --help
set -e
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
if ! docker image inspect open-atf150x-sim >/dev/null 2>&1; then
  docker image inspect open-atf150x-tools >/dev/null 2>&1 || docker build -t open-atf150x-tools "$ROOT/docker"
  docker build -t open-atf150x-sim "$ROOT/docker/sim"
fi
IMAGE=open-atf150x-sim exec "$ROOT/docker/run.sh" "$@"
