"""Silicon test of packing P2+ (designs/inreg_cascade.tt2): one macrocell (MC126, pad P80) is at
the same time a fast input register (r <= P80 on GCLK2, D/Q fast path) and the cascade helper
lending its whole 5-PT sum to F (MC127, 4-input parity, 8 PTs). Checks Y = F (P4), Z = r (P5)
and, over boundary scan (`atf15xx-tool sample`), MC127's and MC126's output cells, for every input
and both r. Run after `cargo build -p atf15xx-tool`."""
import itertools, json, os, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from bench import Bench

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
d = os.path.join(ROOT, '.scratch', 'silicon', 'inreg_cascade')
TOOL = os.path.join(ROOT, 'target', 'debug', 'atf15xx-tool')


def out_cells():
    """{macrocell: value of its output cell} from one SAMPLE."""
    r = subprocess.run([TOOL, 'sample', '--json'], capture_output=True, text=True, timeout=60, check=True)
    return {m['mc']: int(m['out']) for m in json.loads(r.stdout)['macrocells']}


D, E, A, B, CK = 5, 4, 19, 8, 9          # Uno lines: P80, P81, P84, P1, P2
b = Bench(); b.mode(2, 'i'); b.mode(3, 'i')
for p in (D, E, A, B, CK): b.mode(p, '0')
r = subprocess.run([os.path.join(ROOT, 'tools/silicon/program.sh'), d + '/inreg_cascade.svf', d + '/inreg_cascade.jed'], capture_output=True, text=True)
assert ' 0 errors' in r.stdout + r.stderr, r.stdout + r.stderr
bad = n = 0
for rv in (0, 1, 0, 1):
    b.mode(D, str(rv)); b.burst(1); b.mode(D, str(1 - rv))   # load r, then flip the pad
    for e, a, bb in itertools.product((0, 1), repeat=3):
        b.mode(E, str(e)); b.mode(A, str(a)); b.mode(B, str(bb))
        pins = b.read(); cells = out_cells()
        f = e ^ a ^ bb ^ rv
        got = (pins[2], pins[3], cells[127], cells[126])
        ok = got == (f, rv, f, rv)
        n += 1; bad += not ok
        if not ok:
            print('r=%d e=%d a=%d b=%d: expect Y=%d Z=%d, got P4/P5=%d/%d MC127.out=%d MC126.out=%d' % ((rv, e, a, bb, f, rv) + got))
b.close()
print('input register + cascade helper in one macrocell: %d/%d correct -> %s' % (n - bad, n, 'PASS' if not bad else 'FAIL'))
sys.exit(1 if bad else 0)
