#!/bin/sh
# prjbureau's intended flow (after external/prjbureau/v2svf.sh), runnable in the
# open-atf150x-prjbureau container for any ATF15xx density:
#   Verilog using prjbureau's cells.v primitives --yosys--> EDIF --fit150x--> JED --fuseconv--> SVF
# usage: v2jed.sh NAME DEVICE  (reads NAME.v; DEVICE = ATF1502AS | ATF1504AS | ATF1508AS)
#   //OPT: lines in NAME.v give extra fitter options, //PIN: lines the pin file (as in prjbureau).
set -e
NAME=$1; DEV=${2:-ATF1502AS}
ROOT=${PRJBUREAU:-/work/external/prjbureau}
HERE=$(cd "$(dirname "$0")" && pwd)
OPTS=$(grep '//OPT:' "$NAME.v" | cut -d' ' -f2- | tr '\n' ' ')
yosys -q -p "read_verilog -lib $ROOT/cells.v; read_verilog $NAME.v; hierarchy; write_edif -attrprop $NAME.edif"
grep '//PIN:' "$NAME.v" | cut -d' ' -f2- > "$NAME.pin" || true
FIT=fit$(echo "$DEV" | cut -c4-7)
# shellcheck disable=SC2086
"$HERE/../common/fit.sh" "$FIT" -i "$NAME.edif" -o "$NAME.jed" -tech "$DEV" -strategy ifmt=edif -strategy optimize=off $OPTS
PYTHONPATH=$ROOT python3 -m util.fuseconv -d "$DEV" "$NAME.jed" "$NAME.svf"
