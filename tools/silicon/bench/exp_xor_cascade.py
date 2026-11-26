"""Silicon: the XOR input A of a cascade lender with PT2 routed to the XOR (`CasMux` = 1 and
`Pt2Mux` = 1; spec F2.9 open item). Base design `xorcas.tt2`: MC16 (P4) has PT1 = e & a and
PT2 = d & b (e = P81, a = P84, d = P80, b = P1). Each variant patches (Pt2Mux, CasMux, Invert)
of MC16, is programmed, and Y is recorded for all 16 inputs; the truth table is matched against
candidate readings of the XOR."""
import itertools, os, subprocess, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))
from bench import Bench

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
W = os.path.join(ROOT, '.scratch', 'silicon', 'xorcas')
LINES = {'d': 5, 'e': 4, 'a': 19, 'b': 8}
VARIANTS = [(0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 1, 0), (1, 1, 0), (1, 1, 1)]


def hyps(pt1, pt2, p2, cas, inv):
    """x for candidate readings: A input, B input; x = A ^ B ^ !Invert."""
    s = pt1 | (0 if p2 else pt2)            # sum (PT2 leaves the sum when Pt2Mux = 1)
    B = pt2 if p2 else 1
    out = {}
    out['A=sum (no lender effect)'] = s ^ B ^ (1 - inv)
    out['A=1 when CasMux (ours)'] = (1 if cas else s) ^ B ^ (1 - inv)
    out['A=0 when CasMux'] = (0 if cas else s) ^ B ^ (1 - inv)
    # prjbureau-style: with CasMux and Pt2Mux, A = PT2 and B = PT1
    if cas and p2:
        out['A=PT2, B=PT1 (prjbureau)'] = pt2 ^ pt1 ^ (1 - inv)
    return out


b = Bench(); b.mode(2, 'i'); b.mode(3, 'i')
for p in list(LINES.values()) + [9]:
    b.mode(p, '0')
results = {}
for p2, cas, inv in VARIANTS:
    jed = os.path.join(W, 'v%d%d%d.jed' % (p2, cas, inv)); svf = jed[:-4] + '.svf'
    subprocess.run([os.path.join(ROOT, 'target/release/atf15xx-sim'), 'mc', os.path.join(W, 'xorcas.jed'), '-d', 'P1508C84', '-o', jed,
                    '--set', 'MC16.Pt2Mux=%d' % p2, '--set', 'MC16.CasMux=%d' % cas, '--set', 'MC16.Invert=%d' % inv], check=True, capture_output=True)
    subprocess.run([os.path.join(ROOT, 'target/release/atf15xx-tool'), 'svf', '-d', 'ATF1508AS:pv:' + jed, '-o', svf], check=True, capture_output=True)
    r = subprocess.run([os.path.join(ROOT, 'tools/silicon/program.sh'), svf, jed], capture_output=True, text=True)
    assert ' 0 errors' in r.stdout + r.stderr, r.stdout[-500:] + r.stderr[-500:]
    table = {}
    for d, e, a, bb in itertools.product((0, 1), repeat=4):
        for n, v in zip('deab', (d, e, a, bb)):
            b.mode(LINES[n], str(v))
        pins = b.read()
        table[(d, e, a, bb)] = (pins[2], pins[3])
    zbad = sum(z != k[0] for k, (_, z) in table.items())
    fits = []
    names = hyps(0, 0, p2, cas, inv).keys() | hyps(1, 1, p2, cas, inv).keys()
    for h in sorted(names):
        if all(hyps(e & a, d & bb, p2, cas, inv).get(h) == y for (d, e, a, bb), (y, _) in table.items()):
            fits.append(h)
    ys = ''.join(str(table[k][0]) for k in sorted(table))
    print('Pt2Mux=%d CasMux=%d Invert=%d: Y over d,e,a,b = %s  (Z errors %d)  fits: %s' % (p2, cas, inv, ys, zbad, '; '.join(fits) or 'NONE'))
    results[(p2, cas, inv)] = ys
b.close()
