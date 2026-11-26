"""Silicon test of XOR synthesis (designs/vxor.v): y = d^e^a^b (P4), z <= ~(d^e^a) on GCLK2 (P5),
for all 16 inputs."""
import itertools, os, subprocess, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))
from bench import Bench

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
o = os.path.join(ROOT, '.scratch', 'silicon', 'vxor')
L = {'d': 5, 'e': 4, 'a': 19, 'b': 8}
b = Bench(); b.mode(2, 'i'); b.mode(3, 'i')
for p in list(L.values()) + [9]: b.mode(p, '0')
r = subprocess.run([os.path.join(ROOT, 'tools/silicon/program.sh'), o + '/vxor.svf', o + '/vxor.jed'], capture_output=True, text=True)
assert ' 0 errors' in r.stdout + r.stderr
bad = 0
for d, e, a, bb in itertools.product((0, 1), repeat=4):
    for n, v in zip('deab', (d, e, a, bb)): b.mode(L[n], str(v))
    b.burst(1)
    pins = b.read(); y, z = pins[2], pins[3]
    ok = (y, z) == (d ^ e ^ a ^ bb, 1 - (d ^ e ^ a))
    bad += not ok
    if not ok: print('d=%d e=%d a=%d b=%d: y=%d z=%d WRONG' % (d, e, a, bb, y, z))
b.close()
print('XOR synthesis on silicon: %d/16 -> %s' % (16 - bad, 'PASS' if not bad else 'FAIL'))
sys.exit(1 if bad else 0)
