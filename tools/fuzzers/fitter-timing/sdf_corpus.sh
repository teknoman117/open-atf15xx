#!/bin/sh
# Re-fit a feature-diverse set of corpus designs with the vendor fitters at every speed grade
# (7/10/15/20/25), writing SDF (`-strategy Verilog_sim sdf`), for check_sdf_timing.py.
# Run inside the open-atf150x-prjbureau container from /work:
#   docker/prjbureau/run.sh sh tools/fuzzers/fitter-timing/sdf_corpus.sh      (about 15 minutes, 260 fits)
# Needs the fitter oracle corpus in .scratch/oracle/{feat,fold} (tools/fuzzers/fitter-macrocell, tools/fuzzers/fitter-foldback)
# and, optionally, the user designs in .scratch/sim/user.
OUT=${OUT:-/work/.scratch/timing/corpus}
mkdir -p $OUT
dev_of() { sed -n 's/^#\$ DEVICE *//p' "$1" | head -1 | sed -E 's/^f(150[248])plcc([0-9]+)/P\1C\2/; s/^f(150[248])tqfp([0-9]+)/P\1T\2/'; }
run() { # name tt2 extra-options...
  name=$1; src=$2; shift 2; dev=$(dev_of $src); fam=$(echo $dev | cut -c2-5)
  for g in 7 10 15 20 25; do
    d=$OUT/$name/$g; mkdir -p $d; base=$(basename $src .tt2); cp $src $d/
    (cd $d && timeout 120 sh /work/tools/fuzzers/common/fit.sh fit$fam $base.tt2 -device $dev "$@" -strategy Verilog_sim sdf -strategy tPD = $g >log.txt 2>&1)
    [ -s $d/$base.sdo ] && echo "$name $g ok" || echo "$name $g NOSDO"
  done
}
for fam in 1502 1504 1508; do
  for f in comb_8pt dff_ptclk latch dff_arap dff_ce oe_pt xor combfb bur_reg pinfb dff_ptap sum3; do
    s=$(ls -d .scratch/oracle/feat/ft${fam}_${f}_* | head -1); run ${fam}_$f $s/$(basename $s).tt2
  done
  for f in cas fold; do s=$(ls -d .scratch/oracle/fold/fd${fam}_${f}_* | tail -1); run ${fam}_$f $s/$(basename $s).tt2; done
  s=$(ls -d .scratch/oracle/feat/ft${fam}_comb_8pt_* | head -1)
  run ${fam}_mcpower $s/$(basename $s).tt2 -strategy MC_power on
  run ${fam}_fast $s/$(basename $s).tt2 -strategy output_fast on
done
[ -d .scratch/sim/user ] && for u in spi input rpi-pico-8088; do run user_$u .scratch/sim/user/$u.tt2 -strategy JTAG off; done
[ -d .scratch/sim/user ] && run user_system-controller .scratch/sim/user/system-controller.tt2 -strategy JTAG off
