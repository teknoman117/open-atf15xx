#!/usr/bin/env python3
"""Mutation test of the equivalence flow: corrupted JEDs must be caught.

  tools/sim/run.sh python3 tools/sim/mutate.py JED --device D --tt2 X.tt2 --fit X.fit [-n 30] [--seed 1]
  (any source option of equiv.py works: --verilog ... --top T, --gold-jed ...)

Flips one random fuse of a JED that passes equiv.py, keeps the mutant only if its decoded
equations (`atf15xx-sim equations`) change (a flip the model cannot see is not a test), and runs
equiv.py on it. Reports per mutant the formal and simulation verdicts. Expected: most mutants
FAIL; a mutant PROVEN equal is functionally benign (e.g. a literal added to a product term that
is never used); "formal FAIL / sim PASS" means random simulation missed a difference that
formal found (a coverage limit of simulation, not an error); "formal PROVEN / sim FAIL" would be
a bug in the flow. Exit status 1 if any such inconsistency is seen.
"""
import argparse, os, random, re, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
sys.path.insert(0, HERE)
from simlib import sim_bin  # noqa: E402


def read_bits(path):
    t = open(path, 'rb').read().decode('latin-1')
    qf = int(re.search(r'\*\s*QF(\d+)', t).group(1))
    dflt = '1' if re.search(r'\*\s*F1\s*\*', t) else '0'
    bits = [dflt] * qf
    for m in re.finditer(r'\*\s*L(\d+)\s+([01\s]+)', t):
        a = int(m.group(1))
        v = re.sub(r'\s', '', m.group(2))
        bits[a:a + len(v)] = list(v)
    return bits


def write_jed(path, bits):
    body = ''.join('L%05d %s*\n' % (i, ''.join(bits[i:i + 64])) for i in range(0, len(bits), 64))
    open(path, 'w').write('\x02mutant\n*QF%d*\n*F0*\n%s\x030000\n' % (len(bits), body))


def equations(jed, dev):
    return subprocess.run([sim_bin(), 'equations', jed, '-d', dev], capture_output=True, text=True).stdout


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0], epilog=__doc__.split('\n', 2)[2],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('jed')
    ap.add_argument('--device', required=True)
    ap.add_argument('-n', type=int, default=30, help='number of (model-visible) mutants')
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--work', default=os.path.join(ROOT, '.scratch', 'sim', 'mutate'))
    a, rest = ap.parse_known_args()
    os.makedirs(a.work, exist_ok=True)
    bits = read_bits(a.jed)
    base = equations(a.jed, a.device)
    rnd = random.Random(a.seed)
    rows, tries = [], 0
    while len(rows) < a.n and tries < a.n * 200:
        tries += 1
        i = rnd.randrange(len(bits))
        mb = list(bits)
        mb[i] = '1' if mb[i] == '0' else '0'
        mj = os.path.join(a.work, 'mut_%d.jed' % i)
        write_jed(mj, mb)
        if equations(mj, a.device) == base:
            os.remove(mj)
            continue
        r = subprocess.run([sys.executable, os.path.join(HERE, 'equiv.py'), mj, '--device', a.device,
                            '--work', os.path.join(a.work, 'w_%d' % i), '--steps', '2000'] + rest,
                           capture_output=True, text=True)
        f = re.search(r'formal: (\w+)', r.stdout)
        s = re.search(r'sim:\s+(\w+)', r.stdout)
        rows.append((i, f.group(1) if f else '?', s.group(1) if s else '?'))
        print('fuse %6d  formal %-8s sim %s' % rows[-1], flush=True)
    bad = [r for r in rows if r[1] == 'PROVEN' and r[2] == 'FAIL']
    missed = [r for r in rows if r[1] == 'FAIL' and r[2] == 'PASS']
    caught = [r for r in rows if 'FAIL' in (r[1], r[2])]
    print('%d mutants (from %d flips): %d caught, %d proven benign, %d missed by simulation only, %d inconsistent' % (
        len(rows), tries, len(caught), sum(r[1] == 'PROVEN' and r[2] == 'PASS' for r in rows), len(missed), len(bad)))
    sys.exit(1 if bad else 0)


if __name__ == '__main__':
    main()
