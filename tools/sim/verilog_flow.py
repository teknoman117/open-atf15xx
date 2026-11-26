#!/usr/bin/env python3
"""Verilog → .tt2 → our fitter → JED, then check the JED against the Verilog (equiv.py).

  tools/sim/run.sh python3 tools/sim/verilog_flow.py SRC.v [more.v] --top T --device P1504C44 \\
      [--pin clk=43 ...] [--fit-opt -JTAG --fit-opt on ...] [equiv.py options]

Steps (work directory .scratch/sim/vflow/<top>/):
  1. Yosys: `read_verilog -sv; hierarchy -top T; proc; attrmap -remove init` (initial values must
     be 0 = the device's power-up state) `; script tools/sim/synth_atf15xx.ys` → out.json
  2. tools/sim/yosys2tt2.py → T.tt2 (--pin NAME=N pre-assignments, CUPL device name)
  3. our fit150x T.tt2 -device D -preassign keep [--fit-opt ...] → T.jed, T.fit
  4. equiv.py T.jed --verilog SRC.v --top T --fit T.fit (the *source* Verilog, not the
     synthesized netlist, is the gold model) — plus any remaining options (--assume, --init, ...).
Exit status: that of equiv.py, or 2 if a step before it fails.
"""
import argparse, os, re, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(ROOT, 'tools', 'sim'))
from simlib import fit_bin  # noqa: E402
import simlib  # noqa: E402


def run(cmd, cwd, log):
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    open(os.path.join(cwd, log), 'w').write(r.stdout + r.stderr)
    if r.returncode != 0:
        print('verilog_flow: %s failed (see %s):\n%s' % (cmd[0], os.path.join(cwd, log), (r.stdout + r.stderr)[-1500:]))
        sys.exit(2)
    return r.stdout + r.stderr


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0], epilog=__doc__.split('\n', 2)[2],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('sources', nargs='+')
    ap.add_argument('--top', required=True)
    ap.add_argument('--device', required=True, help='fitter device name, e.g. P1504C44')
    ap.add_argument('--pin', action='append', default=[], help='NAME=N pin pre-assignment (repeatable)')
    ap.add_argument('--fit-opt', action='append', default=[], help='extra fitter argument (repeatable)')
    ap.add_argument('--work')
    a, rest = ap.parse_known_args()
    cupl = simlib.device(a.device)['tt2_device']
    if not cupl:
        sys.exit('verilog_flow: no CUPL device name for %s in the device table' % a.device)
    fam = re.search(r'15(0[248])', a.device).group(0)
    work = a.work or os.path.join(ROOT, '.scratch', 'sim', 'vflow', a.top)
    os.makedirs(work, exist_ok=True)
    srcs = [os.path.abspath(s) for s in a.sources]
    rd = 'read_verilog -sv %s; hierarchy -check -top %s; proc' % (' '.join('"%s"' % s for s in srcs), a.top)
    run(['yosys', '-q', '-p', rd + '; write_json pre.json'], work, 'pre.log')
    import json
    inits = [n for m in json.load(open(os.path.join(work, 'pre.json')))['modules'].values()
             for n, nn in m['netnames'].items() if '1' in str(nn.get('attributes', {}).get('init', ''))]
    if inits:
        sys.exit('verilog_flow: registers initialised to 1 (%s): the device powers up 0' % ', '.join(inits[:5]))
    # initial values of 0 are the device's power-up state: drop them so dfflegalize accepts the FFs
    run(['yosys', '-q', '-p', rd + '; attrmap -remove init; script %s' % os.path.join(ROOT, 'tools', 'sim', 'synth_atf15xx.ys')],
        work, 'synth.log')
    run([sys.executable, os.path.join(ROOT, 'tools', 'sim', 'yosys2tt2.py'), 'out.json', '-o', a.top + '.tt2',
         '--device', cupl, '--top', a.top] + sum((['--pin', p] for p in a.pin), []), work, 'yosys2tt2.log')
    out = run([fit_bin(fam), a.top + '.tt2', '-device', a.device, '-preassign', 'keep'] + a.fit_opt, work, 'fit.log')
    print('verilog_flow: %s → %s' % (a.top, out.strip().splitlines()[-1] if out.strip() else 'fitted'))
    r = subprocess.run([sys.executable, os.path.join(HERE, 'equiv.py'), os.path.join(work, a.top + '.jed'),
                        '--device', a.device, '--verilog'] + srcs + ['--top', a.top, '--fit', os.path.join(work, a.top + '.fit'),
                        '--work', os.path.join(work, 'equiv')] + rest)
    sys.exit(r.returncode)


if __name__ == '__main__':
    main()
