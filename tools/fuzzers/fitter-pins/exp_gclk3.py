"""Experiment 4d: which I/O pin can drive GCLK3 (clock pinned there gives gclk_mux == GCLK3 instead of a PT clock)."""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
import fitter_oracle as F
import tt2gen as T
import model as M
from exp_pinmap import DEVS
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch')
PM = json.load(open(os.path.join(ROOT, 'exp', 'pinmap.json')))


def run(fam, pdev, dev, pin):
    name = 'g3%s_%d' % (pdev, pin)
    d = F.fresh_dir(os.path.join(ROOT, 'oracle', 'gclk3'), name)
    p = os.path.join(d, name + '.tt2')
    T.write_tt2(p, name, dev, [('A', None), ('X', pin), ('Y', None)], [], ['A', 'X'], ['Y.D', 'Y.C'], [('1-', {0}), ('-1', {1})])
    r = F.run_fitter(p, fam, pdev, ['-preassign', 'keep', '-strategy', 'JTAG', 'off'])
    if not r['fits']:
        return None
    fit = F.parse_fit(r['fit']); fz = F.read_jed(r['jed'])
    y = [x['mc'] for x in fit['place'] if x['name'] == 'Y'][0]
    return fz[M.addr(fam, y, (7, 19), 0)] + fz[M.addr(fam, y, (7, 19), 1)]


if __name__ == '__main__':
    jobs = []
    for fam, devs in DEVS.items():
        for pdev, dev, n in devs:
            io = [int(k[:-2]) for k, x in PM[pdev].items() if k.endswith('_o') and isinstance(x, dict) and x['fits']]
            jobs += [((pdev, p), run, (fam, pdev, dev, p)) for p in io]
    res = F.run_batch(jobs, workers=8)
    out = {}
    for (pdev, p), g in res.items():
        if g == '01':
            out.setdefault(pdev, []).append(p)
    json.dump(out, open(os.path.join(ROOT, 'exp', 'gclk3.json'), 'w'))
    print(out)
