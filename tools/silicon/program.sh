#!/bin/sh
# Program an ATF15xx over JTAG with OpenOCD and an FT232H (wiring: AD0=TCK, AD1=TDI, AD2=TDO,
# AD3=TMS; see the breakout-atf1508as / P80C550-SPM-CPLD notes). Refuses any image that would
# disable JTAG (`atf15xx-tool jtag-guard` if built, else tools/silicon/jtag_guard.py; the two are
# cross-checked on the committed vectors) unless ALLOW_JTAG_DISABLE=1.
#   usage: tools/silicon/program.sh design.svf [design.jed] [idcode]     (idcode default ATF1508AS)
#   env:   SPEED (kHz, default 400), INTERFACE (default interface/ftdi/um232h.cfg)
set -e
SVF=$1; JED=$2; ID=${3:-0x0150803f}
[ -f "$SVF" ] || { echo "usage: $0 design.svf [design.jed] [idcode]" >&2; exit 2; }
HERE=$(dirname "$0")
if [ "${ALLOW_JTAG_DISABLE:-0}" != 1 ]; then
  TOOL="$HERE/../../target/release/atf15xx-tool"
  if [ -x "$TOOL" ]; then GUARD="$TOOL jtag-guard"; else GUARD="python3 $HERE/jtag_guard.py"; fi
  $GUARD --svf "$SVF" ${JED:+--jed "$JED"} || {
    echo "program.sh: refusing: this image disables JTAG (set ALLOW_JTAG_DISABLE=1 to override;" >&2
    echo "            recovery then needs 12 V on OE1)" >&2; exit 1; }
fi
exec openocd -f "${INTERFACE:-interface/ftdi/um232h.cfg}" \
  -c "adapter speed ${SPEED:-400}" \
  -c "transport select jtag" \
  -c "jtag newtap cpld tap -irlen 10 -expected-id $ID" \
  -c init \
  -c "svf -quiet $SVF" \
  -c "sleep 200" \
  -c shutdown
