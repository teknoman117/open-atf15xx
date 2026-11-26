"""Exhaustive truth-table test of a combinational design on the bench.
Inputs (in this order) L=P1/D8, a=P2/D9, b=P84/A5, c=P81/D4, d=P80/D5; outputs P4/D2, P5/D3.
  truth.py design.svf design.jed EXPR_P4 EXPR_P5   (Python expressions over L a b c d)
"""
import os, subprocess, sys
from bench import Bench
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
svf, jed, e4, e5 = sys.argv[1:5]
IN = [('L', 8), ('a', 9), ('b', 19), ('c', 4), ('d', 5)]
b = Bench(); b.mode(2, 'i'); b.mode(3, 'i')
for _, p in IN: b.mode(p, '0')
r = subprocess.run([os.path.join(ROOT, 'tools/silicon/program.sh'), svf, jed], capture_output=True, text=True)
if ' 0 errors' not in r.stdout + r.stderr: print(r.stdout, r.stderr); sys.exit(2)
bad = 0
orders = [list(range(32)), [i ^ (i >> 1) for i in range(32)]]
for order in orders:
    for v in order:
        env = {n: (v >> k) & 1 for k, (n, _) in enumerate(IN)}
        for k, (n, p) in enumerate(IN): b.mode(p, str(env[n]))
        got = b.read(); exp = (int(bool(eval(e4, {}, env))), int(bool(eval(e5, {}, env))))
        if (got[2], got[3]) != exp:
            bad += 1; print('  %s: got P4,P5=%d,%d expected %d,%d' % (env, got[2], got[3], *exp))
b.close()
print('truth table: %d/64 wrong' % bad)
sys.exit(1 if bad else 0)
