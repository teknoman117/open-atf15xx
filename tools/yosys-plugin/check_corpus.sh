#!/bin/sh
# Run inside the Yosys image after a corpus build (tools/flow/corpus.py --out DIR): every corpus
# design through the cxx plugin with -check-json, fuses compared with the CLI build.
#   docker/yosys/run.sh sh tools/yosys-plugin/check_corpus.sh .scratch/flow/corpus20
cd /work
CORPUS=${1:-.scratch/flow/corpus20}
for d in $CORPUS/*/; do
  n=$(basename $d); top=${n%%-ATF*}; part=${n#*-}
  src=atf15xx-synth/tests/designs/$top.v; [ -f $src ] || continue
  [ -f $d/$top.jed ] || continue
  acf=atf15xx-synth/tests/designs/$top.acf; c=""; [ -f $acf ] && c="-c $acf"
  o=.scratch/flow/plugin/$n; rm -rf $o; mkdir -p .scratch/flow/plugin
  # as the CLI does: Yosys in the source's directory, the source by file name (spec F9.7)
  if (cd "$(dirname $src)" && yosys -q -m /work/tools/yosys-plugin/atf15xx.so -p "read_verilog -sv $(basename $src); synth_atf15xx -top $top -part $part ${c:+-c /work/$acf} -o /work/$o -check-json") > $o.log 2>&1; then
    python3 -c "
import sys; sys.path.insert(0,'tools/silicon')
from jtag_guard import jed_fuses
print('$n', 'identical' if jed_fuses('$o/$top.jed')==jed_fuses('$d/$top.jed') else 'FUSES DIFFER')"
  else echo "$n FAILED: $(grep -m1 ERROR $o.log)"; fi
done
