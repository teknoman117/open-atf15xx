#!/bin/sh
# Run a command in the open-atf150x-prjbureau container.
#   - the workspace is mounted at /work (as the host user),
#   - the vendor fitter directory is mounted read-only at /vendor
#     (VENDOR_DIR, default: WinCUPL's Fitters directory in ~/.wine32),
#   - a private 32-bit wine prefix lives in .scratch/prjbureau-explore/wineprefix.
# Examples:
#   docker/prjbureau/run.sh yosys -V
#   docker/prjbureau/run.sh tools/fuzzers/prjbureau/v2jed.sh design.v ATF1504AS PLCC44
set -e
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
VENDOR_DIR=${VENDOR_DIR:-$HOME/.wine32/drive_c/Wincupl/WinCupl/Fitters}
STATE="$ROOT/.scratch/prjbureau-explore"
mkdir -p "$STATE/home"
TTY=; [ -t 0 ] && TTY=-it
exec docker run --rm $TTY -u "$(id -u):$(id -g)" \
  -v "$ROOT":/work -v "$VENDOR_DIR":/vendor:ro -w /work \
  -e HOME=/work/.scratch/prjbureau-explore/home \
  -e WINEPREFIX=/work/.scratch/prjbureau-explore/wineprefix \
  -e PYTHONPATH=/work/external/prjbureau \
  "${IMAGE:-open-atf150x-prjbureau}" "$@"
