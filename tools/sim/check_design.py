#!/usr/bin/env python3
"""Check a CUPL design's vendor JED and our fitter's JED against its .tt2 (equiv.py, both ways).

  tools/sim/run.sh python3 tools/sim/check_design.py DIR/NAME [--no-vendor] [--no-ours] [equiv.py options]

DIR/NAME.tt2 is the source (or --verilog FILE --top T: the Verilog the .tt2 was made from). If DIR/NAME.jed and DIR/NAME.fit exist (vendor fitter output), the
vendor JED is checked with the vendor pin placement. Our fitter (fit150x) is then run on
a copy of the .tt2 in <work>/ours with the command line the vendor .fit records on its first
line (`fit1504 NAME.tt2 -device P1504C44 ...`), or `-device` from --device, and its JED is
checked with its own .fit placement. Other options (--assume, --reset, --init, --timeout, ...)
are passed to equiv.py. Exit status 1 if any check fails.
"""
import argparse, os, re, shutil, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
sys.path.insert(0, HERE)
from simlib import fit_bin  # noqa: E402


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0], epilog=__doc__.split('\n', 2)[2],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('design', help='path without extension, e.g. ~/Projects/pld/cpld/atf1504as-spi/spi')
    ap.add_argument('--device', help='device (default: from the vendor .fit command line)')
    ap.add_argument('--no-vendor', action='store_true')
    ap.add_argument('--no-ours', action='store_true')
    ap.add_argument('--work', help='default .scratch/sim/designs/NAME')
    ap.add_argument('--verilog', nargs='+', help='check against this Verilog source (with --top) instead of the .tt2')
    ap.add_argument('--top', help='top module of --verilog')
    a, rest = ap.parse_known_args()
    base = os.path.abspath(os.path.expanduser(a.design))
    name = os.path.basename(base)
    work = a.work or os.path.join(ROOT, '.scratch', 'sim', 'designs', name)
    os.makedirs(work, exist_ok=True)
    tt2 = base + '.tt2'
    cmdline = []
    if os.path.exists(base + '.fit'):
        for line in open(base + '.fit', errors='replace').read().replace('\r', '').split('\n')[:20]:
            if re.match(r'fit15\d\d\s', line):
                cmdline = line.split()
                break
    dev = a.device
    if not dev and '-device' in cmdline:
        dev = cmdline[cmdline.index('-device') + 1]
    if not dev:
        sys.exit('check_design: no --device and no vendor .fit command line')
    fam = re.search(r'15(0[248])', dev).group(0)
    results = []

    def equiv(jed, fit, tag):
        src = (['--verilog'] + [os.path.abspath(v) for v in a.verilog] + ['--top', a.top]) if a.verilog else ['--tt2', tt2]
        cmd = [sys.executable, os.path.join(HERE, 'equiv.py'), jed, '--device', dev] + src + [
               '--work', os.path.join(work, tag)] + (['--fit', fit] if fit else []) + rest
        r = subprocess.run(cmd, capture_output=True, text=True)
        out = r.stdout + r.stderr
        print('--- %s %s' % (tag, os.path.relpath(jed, ROOT) if jed.startswith(ROOT) else jed))
        print('\n'.join(l for l in out.splitlines() if 'note:' not in l))
        m = re.search(r'RESULT: (\w+)', out)
        results.append((tag, m.group(1) if m else 'ERROR'))

    if not a.no_vendor and os.path.exists(base + '.jed'):
        equiv(base + '.jed', base + '.fit' if os.path.exists(base + '.fit') else None, 'vendor')
    if not a.no_ours:
        od = os.path.join(work, 'ours')
        os.makedirs(od, exist_ok=True)
        shutil.copy(tt2, os.path.join(od, name + '.tt2'))
        args = cmdline[2:] if cmdline else ['-device', dev]
        r = subprocess.run([fit_bin(fam), name + '.tt2'] + args, cwd=od, capture_output=True, text=True)
        open(os.path.join(od, 'fit.log'), 'w').write(r.stdout + r.stderr)
        jed = os.path.join(od, name + '.jed')
        if r.returncode != 0 or not os.path.exists(jed):
            print('--- ours: fit%s failed (rc %d): %s' % (fam, r.returncode, (r.stdout + r.stderr).strip()[-300:]))
            results.append(('ours', 'FIT-FAILED'))
        else:
            equiv(jed, os.path.join(od, name + '.fit'), 'ours')
    print('SUMMARY %s: %s' % (name, ', '.join('%s %s' % r for r in results)))
    sys.exit(0 if all(r[1] == 'PASS' for r in results) else 1)


if __name__ == '__main__':
    main()
