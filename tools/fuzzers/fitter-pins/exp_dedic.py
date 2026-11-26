"""Experiment 4c: function of each dedicated input pin (clock -> which GCLK; OE -> which GOE table entry; AR -> GCLR)."""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
import fitter_oracle as F
import tt2gen as T
import model as M
from exp_pinmap import DEVS
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch')
PM = json.load(open(os.path.join(ROOT, 'exp', 'pinmap.json')))
# the global-OE input tables (atf15xx-arch, generated from prjbureau's database; `atf15xx-sim fuses`)
import subprocess  # noqa: E402
SIM = os.path.join(ROOT, '..', 'target', 'release', 'atf15xx-sim')


def goe_inputs(fam):
    """{(network, position): input name}."""
    out = subprocess.run([SIM, 'fuses', fam], capture_output=True, text=True, check=True).stdout
    return {(int(w[1]), int(w[2])): w[3] for w in (l.split() for l in out.splitlines()) if w and w[0] == 'goe-input'}
W = {'1502': 5, '1504': 9, '1508': 27}


def run(fam, pdev, dev, pin, use):
    name = 'dd%s_%d_%s' % (pdev, pin, use)
    d = F.fresh_dir(os.path.join(ROOT, 'oracle', 'dedic'), name)
    p = os.path.join(d, name + '.tt2')
    outs = {'clk': ['Y.D', 'Y.C'], 'oe': ['Y', 'Y.OE'], 'ar': ['Y.D', 'Y.C', 'Y.AR']}[use]
    terms = [('1-', {0}), ('-1', {1})] + ([('-1', {2})] if use == 'ar' else [])
    if use == 'ar':
        terms = [('1--', {0}), ('-1-', {1}), ('--1', {2})]
        T.write_tt2(p, name, dev, [('A', None), ('K', None), ('X', pin), ('Y', None)], [], ['A', 'K', 'X'], outs, terms)
    else:
        T.write_tt2(p, name, dev, [('A', None), ('X', pin), ('Y', None)], [], ['A', 'X'], outs, terms)
    r = F.run_fitter(p, fam, pdev, ['-preassign', 'keep', '-strategy', 'JTAG', 'off'])
    if not r['fits']:
        return None
    fit = F.parse_fit(r['fit'])
    fz = F.read_jed(r['jed'])
    y = [x['mc'] for x in fit['place'] if x['name'] == 'Y'][0]
    R = M.REG[fam + 'AS']
    res = {'ptcount': sum(1 for x in R['pt'] if x['mc'] == y and '1' in fz[x['addr']:x['addr'] + 96])}
    if use == 'clk':
        res['gclk'] = fz[M.addr(fam, y, (7, 19), 0)] + fz[M.addr(fam, y, (7, 19), 1)]
    if use == 'oe':
        g = R['goe']; w = g['width']
        for i in range(6):
            s = fz[g['addr'] + i * w:g['addr'] + (i + 1) * w]
            if '0' in s:
                n = 6 - i if fam == '1502' else i + 1
                res['goe'] = (n, goe_inputs(fam)[(n, s.index('0'))])
    if use == 'ar':
        res['gclr'] = fz[M.addr(fam, y, (3, 15), 1)]
    return res


if __name__ == '__main__':
    jobs = []
    for fam, devs in DEVS.items():
        for pdev, dev, n in devs:
            v = PM[pdev]
            ded = sorted(int(k[:-2]) for k, x in v.items() if k.endswith('_i') and isinstance(x, dict) and x['fits']
                         and not (isinstance(v.get(k[:-2] + '_o'), dict) and v[k[:-2] + '_o']['fits']))
            jobs += [((pdev, p, u), run, (fam, pdev, dev, p, u)) for p in ded for u in ('clk', 'oe', 'ar')]
    res = F.run_batch(jobs, workers=8)
    out = {}
    for (pdev, p, u), r in sorted(res.items()):
        out.setdefault(pdev, {}).setdefault(str(p), {})[u] = r if not isinstance(r, Exception) else str(r)
    json.dump(out, open(os.path.join(ROOT, 'exp', 'dedic.json'), 'w'), indent=1)
    for k, v in out.items():
        print(k, {p: (x['clk'] and x['clk'].get('gclk'), x['clk'] and x['clk']['ptcount'], x['oe'] and x['oe'].get('goe'), x['ar'] and x['ar'].get('gclr')) for p, x in v.items()})
