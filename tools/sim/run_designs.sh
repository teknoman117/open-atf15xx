#!/bin/sh
# Check the user's real CUPL designs: vendor JED vs .tt2 and our fitter's JED vs .tt2
# (tools/sim/check_design.py -> equiv.py: formal + simulation). Results: docs/worklog/04-simulator.md.
#
#   tools/sim/run_designs.sh [NAME ...]        (default: all below)
#
# Sources: $PLD_DIR (default ~/Projects/pld/cpld) for the designs with recorded vendor output,
# .scratch/user2 for the ones the fitter workstream compiled with CUPL + the vendor fitter.
# Files are copied to .scratch/sim/real/NAME/ because the container only sees the repository.
# The --assume constraints exclude input combinations under which the design itself has a
# combinational loop through its own bidirectional pins (see docs/architecture/sim/03-equivalence.md);
# they are protocol facts of the boards (an 8088/8051 never asserts ALE with RD, or RD with WR).
set -e
cd "$(dirname "$0")/../.."
SRC=${PLD_DIR:-$HOME/Projects/pld/cpld}
DST=.scratch/sim/real

design() {   # LABEL SOURCE-DIR NAME [check_design/equiv options]
    label=$1; dir=$2; name=$3; shift 3
    if [ -n "$ONLY" ] && ! echo " $ONLY " | grep -q " $label "; then return 0; fi
    mkdir -p "$DST/$label"
    for ext in tt2 fit jed; do
        if [ -f "$dir/$name.$ext" ]; then cp "$dir/$name.$ext" "$DST/$label/"; fi
    done
    if [ ! -f "$DST/$label/$name.tt2" ]; then echo "SKIP $label: no $dir/$name.tt2"; return 0; fi
    tools/sim/run.sh python3 tools/sim/check_design.py "$DST/$label/$name" --work ".scratch/sim/designs/$label" "$@" || true
}

ONLY="$*"
design spi               "$SRC/atf1504as-spi"          spi
design rpi-pico-8088     "$SRC/atf1504as-pico-8088"    rpi-pico-8088
design input             "$SRC/atf1504as-input"        input            --assume 'not (p41 and p44)' --depth 20
design motioncontroller  "$SRC/atf1504as-motioncontroller" motioncontroller --assume 'not (p41 and not p44)' --depth 20
design system-controller "$SRC/atf1508as-8051-system-controller" system-controller \
       --assume 'not ((p84 == 0 or p80 == 0) and p2 == 0)' --depth 12 --timeout 1200
design shift-register    .scratch/user2/atf1508as-example shift-register
design blink-1504        .scratch/user2/atf1504as-blink  blink
design blink-1508        .scratch/user2/atf1508as-blink  blink
