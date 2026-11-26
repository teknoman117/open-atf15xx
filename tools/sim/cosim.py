#!/usr/bin/env python3
"""Co-simulate the Verilog emitted from a JED (iverilog) against the Rust cycle simulator.

  tools/sim/run.sh python3 tools/sim/cosim.py [--device P1504C44] a.jed [b.jed ...]
  tools/sim/run.sh python3 tools/sim/cosim.py --corpus

For every JED: `atf15xx-sim verilog` writes the model as Verilog, a generated testbench applies
the same stimulus CSV that `atf15xx-sim run --stim` gets (one input change per step by
default, so that zero-delay event ordering cannot matter), and the drive of every I/O pin
(0 / 1 / Z) after every step is compared. This checks the emitter against the simulator; both
are built from the same model (atf15xx-sim/src/model.rs), so it does not check the
model itself (equiv.py and the vendor-JED identity checks do that).

Comparison stops at the first step where the Rust simulator reports a combinational loop (the
state afterwards is undefined; iverilog would spin forever on an oscillating loop).
A VCD of the iverilog run is written to <work>/<name>/tb.vcd when a JED fails (or with --vcd).
Exit status 1 if any JED differs.
"""
import argparse, glob, os, re, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(ROOT, 'tools', 'sim'))
from simlib import sim_bin, gen_stim, write_stim, sim_run, device_for  # noqa: E402
import simlib  # noqa: E402



def testbench(vtext, stim_pins, nsteps, vcd):
    """Build a testbench for module `dut` in `vtext`; returns (tb source, input ports, io ports)."""
    a = vtext.index('module dut')
    head = vtext[a:vtext.index(');', a)]
    ins = [int(p) for p in re.findall(r'input wire p(\d+)_in', head)]
    ios = [int(p) for p in re.findall(r'output wire p(\d+)_oe', head)]
    regs = re.findall(r"reg q_(\d+) = (1'b[01]);", vtext)
    latches = set(re.findall(r'always @\* .*q_(\d+) <= d_', vtext))
    has_ar = set(re.findall(r'wire ar_(\d+) =', vtext))
    has_ap = set(re.findall(r'wire ap_(\d+) =', vtext))
    applied = [p for p in stim_pins if p in ins]
    t = ['`timescale 1ns/1ps', 'module tb;']
    t += ['  reg p%d_in = 1\'b0;' % p for p in ins]
    t += ['  wire p%d_oe, p%d_out;' % (p, p) for p in ios]
    conns = ['.p%d_in(p%d_in)' % (p, p) for p in ins] + \
            ['.p%d_oe(p%d_oe), .p%d_out(p%d_out)' % (p, p, p, p) for p in ios]
    t.append('  dut u (%s);' % ', '.join(conns))
    t.append('  reg [%d:0] stim [0:%d];' % (len(applied) - 1, max(nsteps - 1, 0)))
    t.append('  integer i, f;')
    t.append('  initial begin')
    t.append('    $readmemb("stim.mem", stim);')
    if vcd:
        t.append('    $dumpfile("tb.vcd"); $dumpvars(0, tb);')
    # power-up: inputs are 0 (as in the Rust model's initial settle); X->1 transitions at time 0
    # may have clocked flip-flops, so put every flip-flop back to its power-up value unless an
    # async control holds it
    t.append('    #5;')
    for mc, v in regs:
        if mc in latches:
            continue
        cond = ' && '.join(['u.%s_%s !== 1\'b1' % (k, mc) for k, s in (('ar', has_ar), ('ap', has_ap)) if mc in s]) or '1'
        t.append('    if (%s) u.q_%s = %s;' % (cond, mc, v))
    t.append('    #5;')
    t.append('    f = $fopen("out.csv", "w");')
    t.append('    for (i = 0; i < %d; i = i + 1) begin' % nsteps)
    t.append('      {%s} = stim[i];' % ', '.join('p%d_in' % p for p in applied))
    t.append('      #10;')
    t.append('      $fwrite(f, "%0d", i);')
    for p in ios:
        t.append('      if (p%d_oe === 1\'b1) $fwrite(f, ",%%b", p%d_out); else if (p%d_oe === 1\'b0) $fwrite(f, ",Z"); else $fwrite(f, ",X");' % (p, p, p))
    t.append('      $fwrite(f, "\\n");')
    t.append('    end')
    t.append('    $fclose(f);')
    t.append('    $finish;')
    t.append('  end')
    t.append('endmodule')
    return '\n'.join(t) + '\n', applied, ios


VCPP = r'''
#include "Vdut.h"
#include "verilated.h"
#include <cstdio>
#include <cstring>
int main(int argc, char** argv) {
  VerilatedContext* ctx = new VerilatedContext; Vdut* top = new Vdut{ctx};
  FILE* in = fopen("stim.mem", "r"); FILE* out = fopen("out.csv", "w"); char line[4096]; long i = 0;
@ZERO@
  @HOLD0@
  top->eval();
  while (fgets(line, sizeof line, in)) {
    if (strlen(line) < @NIN@) break;
@APPLY@
    @HOLD1@
    top->eval();
    fprintf(out, "%ld", i++);
@PRINT@
    fprintf(out, "\n");
  }
  fclose(out); top->final(); delete top; delete ctx; return 0;
}
'''


def run_verilator_dut(d, applied, ios, timeout=300):
    import vcompare
    vt = open(os.path.join(d, 'dut.v')).read()
    vt, hold = vcompare.add_latch_hold(vt, 'dut')
    open(os.path.join(d, 'dut.v'), 'w').write(vt)
    zero = '\n'.join('  top->p%d_in = 0;' % p for p in applied)
    apply = '\n'.join("    top->p%d_in = line[%d] == '1';" % (p, i) for i, p in enumerate(applied))
    prn = '\n'.join('    if (top->p%d_oe) fprintf(out, ",%%d", (int)top->p%d_out); else fprintf(out, ",Z");' % (p, p) for p in ios)
    h0 = 'top->latch_hold = 0;' if hold else ''
    h1 = 'top->latch_hold = 1; top->eval(); top->latch_hold = 0;' if hold else ''
    src = VCPP.replace('@HOLD0@', h0).replace('@HOLD1@', h1).replace('@ZERO@', zero).replace('@APPLY@', apply).replace('@PRINT@', prn).replace('@NIN@', str(len(applied)))
    open(os.path.join(d, 'vtb.cpp'), 'w').write(src)
    env = dict(os.environ, OBJCACHE='')
    subprocess.run(['verilator', '--cc', '--exe', '--build', '-j', '0', '-Wno-fatal', '-Wno-lint', '-Wno-style',
                    '-Wno-COMBDLY', '-Wno-UNOPTFLAT', '--x-assign', '0', '--x-initial', '0', '--top-module', 'dut',
                    '-CFLAGS', '-O1', '-Mdir', 'vobj', '-o', 'vtb', 'dut.v', 'vtb.cpp'],
                   cwd=d, check=True, capture_output=True, timeout=timeout, env=env)
    subprocess.run([os.path.join(os.path.abspath(d), 'vobj', 'vtb')], cwd=d, check=True, timeout=timeout)
    return [l.split(',') for l in open(os.path.join(d, 'out.csv')).read().splitlines()]


def cosim(jed, devname, steps, seed, work, burst=False, vcd=False, engine='iverilog'):
    dev = simlib.device(devname)
    name = os.path.splitext(os.path.basename(jed))[0]
    d = os.path.join(work, name)
    os.makedirs(d, exist_ok=True)
    pins, rows = gen_stim(dev, steps, seed, single=not burst)
    stim = os.path.join(d, 'stim.csv')
    write_stim(stim, pins, rows)
    hdr, rust = sim_run(jed, devname, stim)
    n = len(rust)
    loop_at = next((i for i, r in enumerate(rust) if r.get('unstable') == '1'), None)
    if loop_at is not None:
        n = loop_at
    vfile = os.path.join(d, 'dut.v')
    subprocess.run([sim_bin(), 'verilog', jed, '-d', devname, '--module', 'dut', '-o', vfile], check=True)
    vtext = open(vfile).read()
    tb, applied, ios = testbench(vtext, pins, max(n, 1), vcd)
    open(os.path.join(d, 'tb.v'), 'w').write(tb)
    with open(os.path.join(d, 'stim.mem'), 'w') as f:
        for r in rows[:max(n, 1)]:
            f.write(''.join(str(r[p]) for p in applied) + '\n')

    def run_iverilog(with_vcd):
        if with_vcd and not vcd:
            open(os.path.join(d, 'tb.v'), 'w').write(testbench(vtext, pins, max(n, 1), True)[0])
        subprocess.run(['iverilog', '-g2005', '-o', 'tb.vvp', 'tb.v', 'dut.v'], cwd=d, check=True)
        subprocess.run(['vvp', '-n', 'tb.vvp'], cwd=d, check=True, capture_output=True, timeout=600)
        return [l.split(',') for l in open(os.path.join(d, 'out.csv')).read().splitlines()]

    if engine == 'verilator':
        _, applied2, ios2 = testbench(vtext, pins, max(n, 1), False)
        vout = run_verilator_dut(d, applied2, ios2)
    else:
        vout = run_iverilog(False)
    mism = []
    for i in range(n):
        vr = dict(zip(['p%d' % p for p in ios], vout[i][1:]))
        for p in ios:
            k = 'p%d' % p
            if rust[i][k] != vr[k]:
                mism.append((i, p, rust[i][k], vr[k]))
    if mism and not vcd and engine == 'iverilog':
        run_iverilog(True)
    return mism, loop_at, n, d


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0],
                                 epilog='Run inside the sim image: tools/sim/run.sh python3 tools/sim/cosim.py ...')
    ap.add_argument('jeds', nargs='*', help='JED files')
    ap.add_argument('--device', help='device (default: from a sibling .args or .fit file)')
    ap.add_argument('--corpus', action='store_true', help='all JEDs in atf15xx-fit/tests/vectors')
    ap.add_argument('--steps', type=int, default=1000)
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--burst', action='store_true',
                    help='change all inputs every step (zero-delay races may then differ legitimately)')
    ap.add_argument('--vcd', action='store_true', help='always write tb.vcd')
    ap.add_argument('--engine', choices=['iverilog', 'verilator'], default='iverilog',
                    help='simulator for the emitted Verilog (default iverilog)')
    ap.add_argument('--work', default=os.path.join(ROOT, '.scratch', 'sim', 'cosim'))
    a = ap.parse_args()
    jeds = list(a.jeds)
    if a.corpus:
        jeds += sorted(glob.glob(os.path.join(ROOT, 'atf15xx-fit', 'tests', 'vectors', '*', '*.jed')))
    if not jeds:
        ap.error('no JED files (give paths or --corpus)')
    bad = 0
    for j in jeds:
        dn = device_for(j, a.device)
        m, loop_at, n, d = cosim(j, dn, a.steps, a.seed, a.work, a.burst, a.vcd, a.engine)
        res = 'PASS' if not m else 'FAIL %d (first: step %d pin %d rust=%s verilog=%s; VCD %s)' % (
            (len(m),) + m[0] + (os.path.relpath(os.path.join(d, 'tb.vcd'), ROOT),))
        if loop_at is not None:
            res += ' (steps 0..%d; combinational loop at step %d)' % (n - 1, loop_at)
        print('%-70s %-9s %s' % (os.path.relpath(j, ROOT) if j.startswith(ROOT) else j, dn, res), flush=True)
        bad += bool(m)
    print('%d JED(s), %d fail' % (len(jeds), bad))
    sys.exit(1 if bad else 0)


if __name__ == '__main__':
    main()
