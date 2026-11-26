"""Silicon test of the carry-chain mapping (designs/vcnt40.v, R4): load 0xFF_FFFF_FFFA, count
random bursts on GCLK2 across the 40-bit wrap, and compare all 40 registers (boundary-scan
SAMPLE through `atf15xx-tool sample --map`) and P4 = c[39], P5 = c[31] with the expected count;
then check that the count holds with enable low.
Run from anywhere after `cargo build -p atf15xx-tool`; programs .scratch/silicon/vcnt40/vcnt40.{svf,jed}."""
import json, os, random, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from bench import Bench

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
o = os.path.join(ROOT, '.scratch', 'silicon', 'vcnt40')
TOOL = os.path.join(ROOT, 'target', 'debug', 'atf15xx-tool')
LD, EN = 4, 5
L, M = 0xFF_FFFF_FFFA, (1 << 40) - 1


def count():
    """The 40-bit count read over JTAG (c[31] and c[39] are the output registers mid and top)."""
    r = subprocess.run([TOOL, 'sample', '--json', '--map', o + '/vcnt40.map.json'], capture_output=True, text=True, timeout=60, check=True)
    s = {x['name']: x['value'] for x in json.loads(r.stdout)['signals']}
    bit = lambda i: s['mid'] if i == 31 else s['top'] if i == 39 else s['c[%d]' % i]
    return sum(int(bit(i)) << i for i in range(40))


b = Bench(); b.mode(2, 'i'); b.mode(3, 'i')
for p in (LD, EN, 9, 8, 19): b.mode(p, '0')
r = subprocess.run([os.path.join(ROOT, 'tools/silicon/program.sh'), o + '/vcnt40.svf', o + '/vcnt40.jed'], capture_output=True, text=True)
assert ' 0 errors' in r.stdout + r.stderr, r.stdout + r.stderr
rnd = random.Random(40); bad = 0; n = 0
for trial in range(6):
    b.mode(LD, '1'); b.burst(1); b.mode(LD, '0'); val = L
    b.mode(EN, '1')
    for step in range(5):
        k = rnd.randint(1, 4); b.burst(k); val = (val + k) & M
        got = count(); pins = b.read()
        ok = got == val and pins[2] == (val >> 39) & 1 and pins[3] == (val >> 31) & 1
        n += 1; bad += not ok
        if not ok or trial == 0:
            print('+%d -> expect %010x, JTAG reads %010x, P4/P5 %d/%d %s' % (k, val, got, pins[2], pins[3], 'OK' if ok else 'WRONG'))
    b.mode(EN, '0'); b.burst(3)          # enable low: must hold
    n += 1; bad += count() != val
b.close()
print('40-bit carry chain on silicon: %d/%d checks -> %s' % (n - bad, n, 'PASS' if not bad else 'FAIL'))
sys.exit(1 if bad else 0)
