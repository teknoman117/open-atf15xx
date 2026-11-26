#!/usr/bin/env python3
"""Make a deliberately broken JED: drop one literal from one product term of a chosen macrocell.

  python3 tools/sim/tests/make_broken.py IN.jed --device D --mc N -o OUT.jed [--seed S]

Flips the fuses of macrocell N's five PT rows one at a time and keeps the first flip (in seeded
random order) whose decoded equations differ from the original in exactly one product term of
MC N, and that term has one literal fewer. For a counter bit this makes the bit toggle too early,
but only in states that need many cycles to reach from power-up, which is what the deep-state
engines (state injection, correspondence induction) are tested with.
"""
import argparse, os, random, re, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, '..'))
from simlib import sim_bin  # noqa: E402
import mutate  # noqa: E402

def addresses(dev, mc):
    """PT row start addresses of `mc`, via the mcbit_addr example (prints PT row starts with 'PT')."""
    out = subprocess.run([os.path.join(os.path.dirname(sim_bin()), 'examples', 'mcbit_addr'), dev, str(mc), 'PT'],
                         capture_output=True, text=True, check=True).stdout.split()
    return [int(x) for x in out]


def pts_of(eq, mc):
    lines = eq.splitlines()
    out, on = {}, False
    for l in lines:
        if re.match(r'^MC%d\b' % mc, l):
            on = True
            continue
        if on and l.startswith('MC'):
            break
        if on:
            m = re.match(r'\s+PT(\d) \[\w+\] = (.*)$', l)
            if m:
                out[int(m.group(1))] = m.group(2).split(' & ')
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('jed')
    ap.add_argument('--device', required=True)
    ap.add_argument('--mc', type=int, required=True)
    ap.add_argument('-o', '--output', required=True)
    ap.add_argument('--seed', type=int, default=1)
    a = ap.parse_args()
    eq0 = subprocess.run([sim_bin(), 'equations', a.jed, '-d', a.device], capture_output=True, text=True, check=True).stdout
    base = pts_of(eq0, a.mc)
    bits = mutate.read_bits(a.jed)
    fam = re.search(r'15(0[248])', a.device).group(0)
    rows = addresses(fam, a.mc)
    cand = [r + c for r in rows for c in range(96)]
    random.Random(a.seed).shuffle(cand)
    for f in cand:
        b = list(bits)
        b[f] = '1' if b[f] == '0' else '0'
        mutate.write_jed(a.output, b)
        eq = subprocess.run([sim_bin(), 'equations', a.output, '-d', a.device], capture_output=True, text=True).stdout
        if eq.replace(pts_of_block(eq, a.mc), '') != eq0.replace(pts_of_block(eq0, a.mc), ''):
            continue                     # something outside MC N changed
        new = pts_of(eq, a.mc)
        diff = [k for k in set(base) | set(new) if base.get(k) != new.get(k)]
        if len(diff) == 1:
            k = diff[0]
            if k in base and k in new and len(new[k]) == len(base[k]) - 1 and set(new[k]) < set(base[k]):
                print('fuse %d: MC%d PT%d drops %s' % (f, a.mc, k, (set(base[k]) - set(new[k])).pop()))
                return
    os.remove(a.output)
    sys.exit('no single-literal drop found for MC%d' % a.mc)


def pts_of_block(eq, mc):
    m = re.search(r'^MC%d\b.*?(?=^MC|\Z)' % mc, eq, re.M | re.S)
    return m.group(0) if m else ''


if __name__ == '__main__':
    main()
