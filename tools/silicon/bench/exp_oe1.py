"""OE1 (P84, bench A5) as a global output enable: tools/silicon/designs/voe1.v (P4 = 0, P5 = d = P81,
both enabled by OE1). High impedance is detected by driving the line from the bench through its
1 kOhm to 0 and to 1 and releasing it: an undriven line (held by the bus keeper) reads back what the
bench drove, a driven line reads the CPLD's value. (Pull-up or hum tests mislead with the keeper on.)
"""
import os, subprocess, sys
from bench import Bench
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
d = os.path.join(ROOT, '.scratch', 'silicon', 'voe1')
b = Bench(); b.mode(2, 'i'); b.mode(3, 'i')
for p in (4, 5, 8, 9, 19): b.mode(p, '0')
r = subprocess.run([os.path.join(ROOT, 'tools/silicon/program.sh'), d + '/voe1.svf', d + '/voe1.jed'], capture_output=True, text=True)
if ' 0 errors' not in r.stdout + r.stderr: print(r.stdout, r.stderr); sys.exit(2)
def look(p):
    b.mode(p, '0'); b.mode(p, 'i'); r0 = b.read()[p]
    b.mode(p, '1'); b.mode(p, 'i'); r1 = b.read()[p]
    return 'Z' if (r0, r1) == (0, 1) else (r0 if r0 == r1 else '?')
bad = 0
for oe in (1, 0, 1, 0):
    for dv in (0, 1):
        b.mode(19, str(oe)); b.mode(4, str(dv))
        got = (look(2), look(3)); exp = (0, dv) if oe else ('Z', 'Z')
        bad += got != exp
        print('OE1=%d d=%d -> P4,P5=%s expect %s' % (oe, dv, got, exp))
b.close()
print('OE1 global output enable:', 'PASS' if not bad else 'FAIL (%d)' % bad)
sys.exit(1 if bad else 0)
