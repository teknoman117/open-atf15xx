#!/usr/bin/env python3
"""Resource and timing figures of fitted JEDs, measured the same way whoever fitted them
(vendor or ours): macrocells used, product terms, foldbacks, cascades (`atf15xx-sim stats`) and
fmax / worst tPD from our timing analyser (`atf15xx-sim timing`).

  jed_stats.py --device P1504C44 [--grade -10] NAME=X.jed [NAME=Y.jed ...]
"""
import argparse, os, re, subprocess

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
SIM = os.path.join(ROOT, 'target/release/atf15xx-sim')


def stats(jed, device, grade):
    out = subprocess.run([SIM, 'stats', jed, '-d', device], capture_output=True, text=True, check=True).stdout
    c = {k: int(v) for k, v in (l.split() for l in out.splitlines() if l)}
    t = subprocess.run([SIM, 'timing', jed, '-d', device, '--grade', grade, '--limit', '1'],
                       capture_output=True, text=True).stdout
    fmax = re.search(r'fmax: ([0-9.]+) MHz', t)
    tpd = re.search(r'pin to pin \(tPD\):\n\s+([0-9.]+) ns', t)
    return {'mcs': c['macrocells'], 'only_fold': c['foldback_only'], 'pts': c['product_terms'], 'folds': c['foldbacks'], 'cascades': c['cascades'],
            'fmax': float(fmax.group(1)) if fmax else None, 'tpd': float(tpd.group(1)) if tpd else None}


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--device', required=True)
    ap.add_argument('--grade', default='-10')
    ap.add_argument('jeds', nargs='+', metavar='NAME=X.jed')
    a = ap.parse_args()
    print('| fit | macrocells with logic | + foldback-only | product terms | foldbacks | cascades | fmax (%s) | worst tPD |' % a.grade)
    print('|---|---|---|---|---|---|---|---|')
    for item in a.jeds:
        name, jed = item.split('=', 1)
        s = stats(jed, a.device, a.grade)
        print('| %s | %d | %d | %d | %d | %d | %s | %s |' % (name, s['mcs'], s['only_fold'], s['pts'], s['folds'], s['cascades'],
              '%.1f MHz' % s['fmax'] if s['fmax'] else '-', '%.1f ns' % s['tpd'] if s['tpd'] else '-'))


if __name__ == '__main__':
    main()
