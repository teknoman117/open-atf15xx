"""Silicon test of the open Verilog flow (atf15xx-synth) on the ATF1508AS breakout.

vblink: P4 = led (toggles every 10^6 clocks of GCLK1), P5 = ~led; rst = P1.
vcount: 4-bit counter, ck = P81 (bench D4), en = P80 (D5), async rst = P1 (D8); P4 = q[1], P5 = q[3].
        Compared step by step with a model under random stimulus.
"""
import os, random, subprocess, sys
from bench import Bench

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))


def program(name):
    d = os.path.join(ROOT, '.scratch', 'silicon', name)
    r = subprocess.run([os.path.join(ROOT, 'tools/silicon/program.sh'), '%s/%s.svf' % (d, name),
                        '%s/%s.jed' % (d, name)], capture_output=True, text=True)
    if 'programmed successfully' not in r.stdout + r.stderr or ' 0 errors' not in r.stdout + r.stderr:
        print(r.stdout, r.stderr); sys.exit('programming %s failed' % name)


ok = True
b = Bench()
for p in (4, 5, 8):
    b.mode(p, '0')
program('vblink')
f4 = b.count(2, 3000) / 3; f5 = b.count(3, 3000) / 3
lv = b.read(); b.mode(8, '1'); held = b.count(2, 500); rl = b.read(); b.mode(8, '0')
print('vblink: P4 %.1f Hz, P5 %.1f Hz (expect ~33.3 each); P4/P5 now %d/%d (complementary); '
      'in reset: %d edges, P4/P5 = %d/%d (expect 0/1)' % (f4, f5, lv[2], lv[3], held, rl[2], rl[3]))
ok &= 32 <= f4 <= 35 and 32 <= f5 <= 35 and lv[2] != lv[3] and held == 0 and (rl[2], rl[3]) == (0, 1)

program('vcount')
rnd = random.Random(1)
q = None                     # unknown until the first reset
bad = 0
b.mode(8, '1'); b.mode(8, '0'); q = 0
for step in range(500):
    op = rnd.random()
    if op < 0.05:
        b.mode(8, '1'); b.mode(8, '0'); q = 0
    else:
        en = rnd.random() < 0.7
        b.mode(5, '1' if en else '0'); b.pulse(4, 1, 5)
        if en: q = (q + 1) & 15
    r = b.read()
    if (r[2], r[3]) != ((q >> 1) & 1, (q >> 3) & 1):
        bad += 1
        if bad <= 5: print('  step %d: model q=%d expects P4,P5=%d,%d got %d,%d' % (step, q, (q >> 1) & 1, (q >> 3) & 1, r[2], r[3]))
print('vcount: 500 random steps (clock/enable/async reset), mismatches: %d' % bad)
ok &= bad == 0
b.close()
print('VERILOG FLOW ON SILICON:', 'PASS' if ok else 'FAIL')
sys.exit(0 if ok else 1)
