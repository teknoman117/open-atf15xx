#!/bin/sh
# Run a command in the open-atf150x-tools container with the workspace mounted at /work,
# as the host user, with cargo's registry cache and target directory kept in .scratch/.
#   docker/run.sh cargo test --workspace
#   docker/run.sh yosys -V
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$ROOT/.scratch/cargo-home" "$ROOT/.scratch/target-docker"
TTY=; [ -t 0 ] && TTY=-it
exec docker run --rm $TTY -u "$(id -u):$(id -g)" -v "$ROOT":/work -w /work \
  -e HOME=/work/.scratch -e CARGO_HOME=/work/.scratch/cargo-home -e CARGO_TARGET_DIR=/work/.scratch/target-docker \
  "${IMAGE:-open-atf150x-tools}" "$@"
