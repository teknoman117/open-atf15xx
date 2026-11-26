#!/bin/sh
# Time-bounded equivalence tests (tools/sim/vcompare.py and equiv.py --sim-engine verilator).
#
#   tools/sim/tests/run_tests.sh [BUDGET]        (default 60 s per check; run on the host)
#
# Each check must finish within BUDGET (+15 s slack for container start) and give the expected
# verdict. Inputs:
#   * tools/sim/tests/counter64.v (64-bit counter), built with the open flow (atf15xx-synth) into
#     .scratch/sim/tests/flow/ if missing;
#   * the user's blink.v (28-bit counter) from $PLD_XILINX (default ~/Projects/pld/cpld/xilinx),
#     copied to .scratch/sim/tests/ (user designs are not committed);
#   * broken variants made by tools/sim/tests/make_broken.py (one literal dropped from a product
#     term of a high counter bit: wrong only in states ~2^60 / ~2^26 cycles from power-up);
#   * the vendor-fitted user designs in .scratch/sim/real/ and .scratch/sim/vflow/ (made by
#     tools/sim/run_designs.sh and the vendor fit of blink.v, see docs/worklog/04-simulator.md).
# Exit status 1 if any check gives the wrong verdict or exceeds its budget.
set -u
cd "$(dirname "$0")/../../.."
B=${1:-60}
T=.scratch/sim/tests
mkdir -p $T
X=${PLD_XILINX:-$HOME/Projects/pld/cpld/xilinx}
[ -f $T/blink.v ] || cp "$X/xc9572xl-blink/blink.v" $T/blink.v
C=$T/flow/counter64-ATF1508AS-PLCC84; BL=$T/flow/blink-ATF1508AS-PLCC84
[ -f $C/counter64.jed ] || docker/yosys/run.sh python3 tools/flow/corpus.py --stage build --out $T/flow \
    --only counter64 --device ATF1508AS-PLCC84 --extra tools/sim/tests/counter64.v=ATF1508AS-PLCC84 >/dev/null
[ -f $BL/blink.jed ] || docker/yosys/run.sh python3 tools/flow/corpus.py --stage build --out $T/flow \
    --only blink --device ATF1508AS-PLCC84 --extra $T/blink.v=ATF1508AS-PLCC84 >/dev/null
[ -f $T/counter64_broken.jed ] || tools/sim/run.sh python3 tools/sim/tests/make_broken.py $C/counter64.jed --device P1508C84 --mc 13 -o $T/counter64_broken.jed
[ -f $T/blink_broken.jed ] || tools/sim/run.sh python3 tools/sim/tests/make_broken.py $BL/blink.jed --device P1508C84 --mc 63 -o $T/blink_broken.jed
cp $C/counter64.map.json $T/counter64_broken.map.json; cp $BL/blink.map.json $T/blink_broken.map.json
bad=0
check() {   # NAME EXPECT(regex on the GUARANTEE/verdict line) TOOL ARGS...
    name=$1; expect=$2; tool=$3; shift 3
    s=$(date +%s)
    out=$(tools/sim/run.sh sh -c "export SIM_NO_BUILD=1; python3 tools/sim/$tool \"\$@\" --budget $B --work $T/w_$name" sh "$@" 2>&1)
    t=$(( $(date +%s) - s ))
    line=$(echo "$out" | grep -E '^GUARANTEE:|^vcompare ' | tail -1)
    if echo "$line" | grep -qE "$expect" && [ $t -le $((B + 15)) ]; then r=ok; else r=WRONG; bad=1; fi
    printf '%-26s %-5s %4ss  %s\n' "$name" "$r" "$t" "$(echo "$line" | cut -c1-150)"
}
V="--device P1508C84"
check counter64          'GUARANTEE: PROVEN'  equiv.py    $C/counter64.jed $V --verilog tools/sim/tests/counter64.v --top counter64 --fit $C/counter64.fit --sim-engine verilator
check counter64-broken   'GUARANTEE: FAIL'    equiv.py    $T/counter64_broken.jed $V --verilog tools/sim/tests/counter64.v --top counter64 --fit $C/counter64.fit --sim-engine verilator
check counter64-vcompare 'SIMULATED'          vcompare.py $C/counter64.jed $V --verilog tools/sim/tests/counter64.v --top counter64 --fit $C/counter64.fit
check counter64-brk-vcmp 'FAIL'               vcompare.py $T/counter64_broken.jed $V --verilog tools/sim/tests/counter64.v --top counter64 --fit $C/counter64.fit
check blink              'GUARANTEE: PROVEN'  equiv.py    $BL/blink.jed $V --verilog $T/blink.v --top blink --fit $BL/blink.fit --sim-engine verilator
check blink-broken       'GUARANTEE: FAIL'    equiv.py    $T/blink_broken.jed $V --verilog $T/blink.v --top blink --fit $BL/blink.fit --sim-engine verilator
check blink-vendor       'FAIL'               vcompare.py .scratch/sim/vflow/blink/vendor/blink.jed $V --verilog $T/blink.v --top blink --fit .scratch/sim/vflow/blink/vendor/blink.fit
check spi-vendor         'SIMULATED'          vcompare.py .scratch/sim/real/spi/spi.jed --device P1504C44 --tt2 .scratch/sim/real/spi/spi.tt2 --fit .scratch/sim/real/spi/spi.fit
check spi-corrupt        'GUARANTEE: FAIL'    equiv.py    .scratch/sim/t/spi_bad.jed --device P1504C44 --tt2 .scratch/sim/real/spi/spi.tt2 --fit .scratch/sim/real/spi/spi.fit --sim-engine verilator
check mix-tristate       'GUARANTEE: PROVEN'  equiv.py    .scratch/sim/ours/mix.jed --device P1504C44 --verilog .scratch/prjbureau-explore/flow2/mix.v --top top --fit .scratch/sim/ours/mix.fit --sim-engine verilator
check mix-vendor-pinbug  'GUARANTEE: FAIL'    equiv.py    .scratch/prjbureau-explore/flow2/mix.jed --device P1504C44 --verilog .scratch/prjbureau-explore/flow2/mix.v --top top --fit .scratch/prjbureau-explore/flow2/mix.fit --sim-engine verilator
check input-loop-assume  'GUARANTEE: (BOUNDED|PROVEN)' equiv.py .scratch/sim/real/input/input.jed --device P1504C44 --tt2 .scratch/sim/real/input/input.tt2 --fit .scratch/sim/real/input/input.fit --assume 'not (p41 and p44)' --sim-engine verilator --depth 20
exit $bad
