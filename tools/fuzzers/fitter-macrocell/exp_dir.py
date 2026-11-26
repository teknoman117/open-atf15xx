"""Experiment 3: physical column direction of each macrocell-config field pair (needed for 1508AS).

A registered buried node (D = A & B, clocked by the GCLK1 pin, reset by the GCLR pin, plus a PT OE'd output
so most option pairs change) is preassigned to node 600+m; changed config bits reveal column c(l).
"""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
import fitter_oracle as F
import tt2gen as T

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch')
REG = json.load(open(os.path.join(ROOT, 're', 'regions.json')))
DEV = {'1502': ('1502AS', 'f1502plcc44', 'P1502C44', 43, 1, 32), '1504': ('1504AS', 'f1504plcc84', 'P1504C84', 83, 1, 64),
       '1508': ('1508AS', 'f1508plcc84', 'P1508C84', 83, 1, 128)}


def run(fam, m, variant):
    regfam, dev, pdev, gclk, gclr, nmc = DEV[fam]
    name = 'dir%s_%d_%s' % (fam, m, variant)
    d = F.fresh_dir(os.path.join(ROOT, 'oracle', 'dir'), name)
    p = os.path.join(d, name + '.tt2')
    ins = ['A', 'B', 'CK', 'R']
    outs = ['N.REG', 'N.C', 'N.AR'] if variant == 'reg' else ['N.L', 'N.LE']
    terms = [('11--', {0}), ('--1-', {1})] + ([('---1', {2})] if variant == 'reg' else [])
    T.write_tt2(p, name, dev, [('A', None), ('B', None), ('CK', gclk), ('R', gclr)], ['N'], ins, outs, terms)
    txt = open(p).read().replace('#$ NODES 1 N+ ', '#$ NODES 1 N+:%d ' % (600 + m))
    open(p, 'w').write(txt)
    r = F.run_fitter(p, fam, pdev, ['-preassign', 'keep', '-strategy', 'JTAG', 'on'])
    if not r['fits']:
        return None
    fit = F.parse_fit(r['fit'])
    pl = [x for x in fit['place'] if x['name'] == 'N']
    return dict(m=m, placed=pl[0]['mc'] if pl else None, jed=F.read_jed(r['jed']))


if __name__ == '__main__':
    fam = sys.argv[1]
    nmc = DEV[fam][5]
    jobs = [((m, v), run, (fam, m, v)) for m in range(1, nmc + 1) for v in ('reg', 'latch')]
    res = F.run_batch(jobs, workers=8)
    out = {'%d_%s' % k: v for k, v in res.items() if isinstance(v, dict)}
    json.dump(out, open(os.path.join(ROOT, 'exp', 'dir_%s.json' % fam), 'w'))
    print(fam, 'ok', len(out), 'of', len(jobs), 'placed-as-requested', sum(1 for v in out.values() if v['placed'] == v['m']))
