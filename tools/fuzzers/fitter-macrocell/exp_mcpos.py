"""Experiment 2: which bits of the shared macrocell-config fields belong to which macrocell.

Fit the same tiny design (Y = A & B, Y on pin p) for every user pin p; record the config-region bits
that differ from the all-unused baseline, keyed by the MC Y landed on.
"""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
import fitter_oracle as F
import tt2gen as T

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch')
REG = json.load(open(os.path.join(ROOT, 're', 'regions.json')))
CFG = {'1502': ('1502AS', 'f1502plcc44', 'P1502C44', 44), '1504': ('1504AS', 'f1504plcc44', 'P1504C44', 44),
       '1508': ('1508AS', 'f1508plcc84', 'P1508C84', 84)}


def cfg_range(fam):
    R = REG[CFG[fam][0]]
    a = min(x['addr'] for x in R['mcfg']); b = max(x['addr'] + x['len'] for x in R['mcfg'])
    return a, b


def run(fam, ypin, extra=''):
    regfam, dev, pdev, npins = CFG[fam]
    name = 'mp%s_%d' % (fam, ypin)
    d = F.fresh_dir(os.path.join(ROOT, 'oracle', 'mcpos'), name)
    p = os.path.join(d, name + '.tt2')
    ins = ['A', 'B']
    T.write_tt2(p, name, dev, [('A', None), ('B', None), ('Y', ypin)], [], ins, ['Y'], [('11', {0})])
    r = F.run_fitter(p, fam, pdev, ['-preassign', 'keep', '-strategy', 'JTAG', 'off'])
    if not r['fits']:
        return None
    fit = F.parse_fit(r['fit'])
    y = [x for x in fit['place'] if x['name'] == 'Y'][0]
    a, b = cfg_range(fam)
    fz = F.read_jed(r['jed'])
    return dict(pin=ypin, mc=y['mc'], cfg=fz[a:b], a_mc=[x['mc'] for x in fit['place'] if x['name'] == 'A'][0],
                b_mc=[x['mc'] for x in fit['place'] if x['name'] == 'B'][0])


if __name__ == '__main__':
    fam = sys.argv[1]
    n = CFG[fam][3]
    res = F.run_batch([(p, run, (fam, p)) for p in range(1, n + 1)], workers=8)
    ok = {k: v for k, v in res.items() if isinstance(v, dict)}
    json.dump(ok, open(os.path.join(ROOT, 'exp', 'mcpos_%s.json' % fam), 'w'))
    print(fam, 'fitted pins', len(ok), 'MCs', sorted(v['mc'] for v in ok.values()))
