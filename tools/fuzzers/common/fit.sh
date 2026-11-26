#!/bin/sh
# Run a vendor ATF15xx fitter (fit1502/fit1504/fit1508) on files in the current directory.
# Intended for the open-atf150x-prjbureau container (docker/prjbureau/run.sh), but works with any
# wine prefix. The fitter writes temporary files (*.tmp, *.ren) into the directory that holds the
# directory named by %FITTERDIR% and reads atmel.std from there, so the vendor files are copied into a writable
# directory of the wine prefix (C:\fitters) on first use. The current directory is mapped to
# its own wine drive (W:) and the fitter runs from W:\.
#   usage: fit.sh fit1504 design.tt2 -device P1504C44 [fitter options...]
# Fitter binaries are taken from $VENDOR (default /vendor).
set -e
FIT=$1; shift
VENDOR=${VENDOR:-/vendor}
: "${WINEPREFIX:?set WINEPREFIX}"
[ -d "$WINEPREFIX/dosdevices" ] || wineboot -i >/dev/null 2>&1 || true
rm -f "$WINEPREFIX/dosdevices/w:"
ln -s "$(pwd)" "$WINEPREFIX/dosdevices/w:"
FDIR="$WINEPREFIX/drive_c/fitters"
mkdir -p "$FDIR"
for f in "$VENDOR"/*; do
  case "$f" in *.tmp|*.TMP|*.ren|*.REN) continue;; esac
  [ -e "$FDIR/$(basename "$f")" ] || cp "$f" "$FDIR/"
done
exe=$(ls "$FDIR"/* | grep -i "/$FIT.exe\$" | head -1)
[ -n "$exe" ] || { echo "fit.sh: $FIT.exe not found in $VENDOR" >&2; exit 2; }
# the fitter locates atmel.std and its temporary files via %FITTERDIR% (set by the WinCUPL
# installer in its own prefix; prjbureau's v2svf.sh sets it too)
export FITTERDIR='C:\fitters'
cd "$WINEPREFIX/dosdevices/w:/"
exec wine "$exe" "$@"
