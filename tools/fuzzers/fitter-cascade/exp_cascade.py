"""How the vendor fitter builds sums wider than one macrocell: cascade chains.

Design: Y = OR over i < N of (S == i) & D_i, with a 6-bit select S and N data inputs D_i: N
products that share no factor the fitter could fold back, and whose complement is far larger, so
the sum needs N product terms. Y is combinatorial (`comb`) or a D flip-flop on the GCLK1 pin
(`reg`), floating or pinned (`pin`: Y on a pin in the middle of a block). For every N the vendor
JEDEC file is decoded with our simulator (`atf15xx-sim equations` / `stats`): which macrocells
hold Y's product terms, and the cascade links between them.

Writes .scratch/oracle/cascade/<name>/ (vendor .tt2/.jed/.fit) and .scratch/exp/cascade_<fam>.json.
Usage: exp_cascade.py [FAMILY]   (default 1504; device TQFP100 for 1504/1508)
"""
import json, os, re, subprocess, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
import fitter_oracle as F
import tt2gen as T

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..', '..'))
ROOT = os.path.join(REPO, '.scratch')
SIM = os.path.join(REPO, 'target', 'release', 'atf15xx-sim')
CFG = {'1504': ('f1504tqfp100', 'P1504T100', 46), '1508': ('f1508tqfp100', 'P1508T100', 46), '1502': ('f1502plcc44', 'P1502C44', None)}
NS = [6, 8, 9, 10, 11, 12, 13, 15, 16, 20, 25, 30, 35, 40]
SEL = 6


def design(fam, n, kind, pinned):
    dev, pdev, ypin = CFG[fam]
    name = 'cas%s_%s%s_%d' % (fam, kind, '_pin' if pinned else '', n)
    d = F.fresh_dir(os.path.join(ROOT, 'oracle', 'cascade'), name)
    p = os.path.join(d, name + '.tt2')
    sel = ['S%d' % k for k in range(SEL)]
    data = ['D%d' % i for i in range(n)]
    pins = [(x, None) for x in sel + data] + [('Y', ypin if pinned else None)]
    ins = sel + data
    if kind == 'reg':
        pins.append(('K', 87 if fam != '1502' else 43))
        ins = ins + ['K']
        outs = ['Y.D', 'Y.C']
    else:
        outs = ['Y']
    terms = []
    for i in range(n):
        cube = ''.join('1' if (i >> k) & 1 else '0' for k in range(SEL)) + ''.join('1' if j == i else '-' for j in range(n))
        if kind == 'reg':
            cube += '-'
        terms.append((cube, {0}))
    if kind == 'reg':
        terms.append(('-' * (SEL + n) + '1', {1}))
    T.write_tt2(p, name, dev, pins, [], ins, outs, terms)
    return name, p, pdev


def decode(jed, pdev):
    st = subprocess.run([SIM, 'stats', '-d', pdev, jed], capture_output=True, text=True).stdout
    eq = subprocess.run([SIM, 'equations', '-d', pdev, jed], capture_output=True, text=True).stdout
    return st, eq


def run(fam, n, kind, pinned):
    name, p, pdev = design(fam, n, kind, pinned)
    r = F.run_fitter(p, fam, pdev, ['-preassign', 'keep'])
    res = dict(name=name, n=n, kind=kind, pinned=pinned, fits=bool(r['fits']))
    if not r['fits']:
        res['why'] = (r.get('stdout') or '')[-400:]
        return res
    fit = F.parse_fit(r['fit'])
    res['place'] = {x['name']: x['mc'] for x in fit['place'] if x['name'] == 'Y'}
    jed = os.path.join(os.path.dirname(p), name + '.jed')
    st, eq = decode(jed, pdev)
    res['stats'] = st.strip()
    # the macrocells with product terms, and the cascade links, from the decoded equations
    res['equations'] = eq
    return res


if __name__ == '__main__':
    fam = sys.argv[1] if len(sys.argv) > 1 else '1504'
    jobs = [((fam, n, kind, pinned), run, (fam, n, kind, pinned)) for kind in ('comb', 'reg') for pinned in (False, True) for n in NS]
    # workers=1: few designs, and no shared-directory hazard (common/CLAUDE.md)
    res = F.run_batch(jobs, workers=1)
    out = [v for v in res.values() if isinstance(v, dict)]
    os.makedirs(os.path.join(ROOT, 'exp'), exist_ok=True)
    json.dump(out, open(os.path.join(ROOT, 'exp', 'cascade_%s.json' % fam), 'w'), indent=1)
    for v in sorted(out, key=lambda v: (v['kind'], v['pinned'], v['n'])):
        st = re.sub(r'\s+', ' ', v.get('stats', ''))[:150]
        print('%-4s %-5s n=%2d fits=%-5s Y@%s  %s' % (v['kind'], 'pin' if v['pinned'] else 'float', v['n'], v['fits'], v.get('place', {}).get('Y'), st))
