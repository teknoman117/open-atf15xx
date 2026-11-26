#!/bin/sh
# Oracle comparison: run the *vendor* fitter on the .tt2 that atf15xx-synth wrote (--emit-tt2) for
# each corpus run, so its result can be compared with ours (fit / macrocells / equivalence).
#   docker/prjbureau/run.sh tools/fuzzers/fitter-conformance/vendor_oracle.sh [.scratch/flow/corpus/NAME-PART ...]
# Uses DIR/NAME.tt2 and DIR/NAME.vendor-cmd (the equivalent vendor command line, written by
# atf15xx-synth --emit-tt2 before fitting, also when our fit fails).
# Writes DIR/vendor/NAME.{tt2,jed,fit} and DIR/vendor/status (exit code of the fitter).
# The vendor binaries are mounted read-only from the host's WinCUPL install (never in the repo).
set -u
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
DIRS=${*:-$(ls -d "$ROOT"/.scratch/flow/corpus/*-*)}
for d in $DIRS; do
  [ -d "$d" ] || continue
  cmdf=$(ls "$d"/*.vendor-cmd 2>/dev/null | head -1); [ -n "$cmdf" ] || continue
  name=$(basename "$cmdf" .vendor-cmd)
  # fitNNNN NAME.tt2 -device P... -preassign keep -JTAG ... (written by atf15xx-synth --emit-tt2)
  set -- $(cat "$cmdf"); fit=$1; shift
  mkdir -p "$d/vendor"; cp "$d/$name.tt2" "$d/vendor/"
  ( cd "$d/vendor" && rm -f "$name.jed" "$name.fit" && \
    timeout 120 sh "$ROOT/tools/fuzzers/common/fit.sh" "$fit" "$@" > fitter.log 2>&1; echo $? > status )
  echo "$(basename "$d"): vendor $fit exit $(cat "$d/vendor/status"), jed: $([ -s "$d/vendor/$name.jed" ] && echo yes || echo no)"
  wineserver -k 2>/dev/null || true
done
