"""Experiment 4b: which I/O pins are JTAG / PD1 / PD2 pins (output pinned there fails only when the feature is on)."""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
import fitter_oracle as F
import tt2gen as T
from exp_pinmap import DEVS
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch')
PM = json.load(open(os.path.join(ROOT, 'exp', 'pinmap.json')))
FEAT = {'jtag': ['-strategy', 'JTAG', 'on'], 'pd1': ['-strategy', 'JTAG', 'off', '-strategy', 'pd1', 'on'],
        'pd2': ['-strategy', 'JTAG', 'off', '-strategy', 'pd2', 'on']}


def run(fam, pdev, dev, pin, feat):
    name = 'pr%s_%d_%s' % (pdev, pin, feat)
    d = F.fresh_dir(os.path.join(ROOT, 'oracle', 'pinroles'), name)
    p = os.path.join(d, name + '.tt2')
    T.write_tt2(p, name, dev, [('A', None), ('B', None), ('Y', pin)], [], ['A', 'B'], ['Y'], [('11', {0})])
    r = F.run_fitter(p, fam, pdev, ['-preassign', 'keep'] + FEAT[feat])
    return r['fits']


if __name__ == '__main__':
    out = {}
    jobs = []
    for fam, devs in DEVS.items():
        for pdev, dev, n in devs:
            io = [int(k[:-2]) for k, x in PM[pdev].items() if k.endswith('_o') and isinstance(x, dict) and x['fits']]
            jobs += [((pdev, p, f), run, (fam, pdev, dev, p, f)) for p in io for f in FEAT]
    res = F.run_batch(jobs, workers=8)
    for (pdev, p, f), ok in res.items():
        if ok is False:
            out.setdefault(pdev, {}).setdefault(f, []).append(p)
    for k in out:
        for f in out[k]:
            out[k][f].sort()
    json.dump(out, open(os.path.join(ROOT, 'exp', 'pinroles.json'), 'w'), indent=1)
    for k, v in out.items():
        print(k, v)
