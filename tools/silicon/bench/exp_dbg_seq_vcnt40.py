"""Bench check of `atf15xx-tool seq` (docs/worklog/08-debugger.md, spec D1.6): with designs/vcnt40.v
programmed (see exp_vcnt40.py; this script does not program), drive the design's inputs ld (P81)
and en (P80) over JTAG EXTEST instead of from the Uno, clock GCLK2 from the Uno (a dedicated input
cannot be driven by EXTEST), and read all 40 counter registers back through the same session.
Shows that during EXTEST the logic keeps running, sees the driven pins, and that the design's own
outputs (top = P4, mid = P5) follow it, one scan late.

Safety: the Uno lines on P80/P81 (D5/D4) and P4/P5 (D2/D3) are inputs while the session drives;
the dedicated inputs P1/P2/P84 stay with the Uno (EXTEST cannot drive them). Each Uno line is in
series with 1 kOhm anyway. Run from the repository root after `cargo build -p atf15xx-tool`."""
import os
os.chdir(os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..')))
import random, subprocess, sys
sys.path.insert(0, os.path.join('tools', 'silicon', 'bench'))
from bench import Bench

MAP = '.scratch/silicon/vcnt40/vcnt40.map.json'
L, M = 0xFF_FFFF_FFFA, (1 << 40) - 1

b = Bench()
for p in (2, 3, 4, 5):          # P4, P5 (outputs of the design), P81, P80 (driven by the session)
    b.mode(p, 'i')
for p in (8, 9, 19):            # GCLR inactive, GCLK2 low, OE1 low (as in exp_vcnt40.py)
    b.mode(p, '0')

dbg = subprocess.Popen(['target/debug/atf15xx-tool', 'seq', '--map', MAP, '--yes-drive-pins', '--allow-inputs', '-'],
                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)


def tx(cmd):
    dbg.stdin.write(cmd + '\n')
    dbg.stdin.flush()
    while True:
        line = dbg.stdout.readline()
        if not line:
            raise RuntimeError('atf15xx-tool exited')
        line = line.strip()
        if line.startswith(('ok', 'MISMATCH', 'ERROR')) or '=' in line:
            if line.startswith('ERROR'):
                raise RuntimeError(line)
            return line


def count():
    """the 40-bit counter from one capture: bus c (all bits but 31 and 39) plus mid and top"""
    v = dict(kv.split('=') for kv in tx('print c mid top').split())
    return int(v['c'], 16) | int(v['mid']) << 31 | int(v['top']) << 39, v


checks = bad = 0


def check(what, got, want):
    global checks, bad
    checks += 1
    ok = got == want
    bad += not ok
    print('%-34s expect 0x%010x got 0x%010x %s' % (what, want, got, 'OK' if ok else 'WRONG'))


tx('drive ld=1 en=0')           # the session drives P81/P80
b.burst(1)                      # one GCLK2 edge: load
tx('drive ld=0 en=1')
val = L
check('load (ld driven over JTAG)', count()[0], val)
rnd = random.Random(8)
for step in range(8):
    k = rnd.randint(1, 4)
    b.burst(k)
    val = (val + k) & M
    got, v = count()
    check('+%d with en driven over JTAG' % k, got, val)
    tx('scan')                  # the outputs follow the design one scan late
    pins = b.read()
    checks += 1
    follow = pins[2] == (val >> 39) & 1 and pins[3] == (val >> 31) & 1
    bad += not follow
    print('   P4/P5 at the Uno %d/%d, design top/mid %s/%s %s' % (pins[2], pins[3], v['top'], v['mid'], 'OK' if follow else 'WRONG'))
tx('drive en=0')                # en driven low: the count must hold
b.burst(3)
check('3 edges with en=0 driven over JTAG', count()[0], val)
print(tx('expect ld=0 en=0'))
dbg.stdin.close()
rc = dbg.wait(timeout=60)       # the session ends with BYPASS: the pins are the design's again
for p in (4, 5):
    b.mode(p, '0')
b.close()
print('seq session on silicon: %d/%d checks, exit %d -> %s' % (checks - bad, checks, rc, 'PASS' if not bad and rc == 0 else 'FAIL'))
