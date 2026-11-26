"""Experiment 4: package pin -> macrocell map for every device the fitters accept.

For each pin p: Y (output, pinned to p) = A & B (A, B floating). Record the MC of Y, or the fitter's
complaint if the pin is not a user I/O. Dedicated inputs are found by pinning an *input* instead.
"""
import json, os, re, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
import fitter_oracle as F
import tt2gen as T

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch')
DEVS = {'1502': [('P1502C44', 'f1502plcc44', 44), ('P1502T44', 'f1502tqfp44', 44)],
        '1504': [('P1504C44', 'f1504plcc44', 44), ('P1504T44', 'f1504tqfp44', 44), ('P1504C68', 'f1504plcc68', 68),
                 ('P1504C84', 'f1504plcc84', 84), ('P1504Q100', 'f1504qfp100', 100), ('P1504T100', 'f1504tqfp100', 100)],
        '1508': [('P1508C68', 'f1508plcc68', 68), ('P1508C84', 'f1508plcc84', 84), ('P1508T100', 'f1508tqfp100', 100),
                 ('P1508Q100', 'f1508qfp100', 100), ('P1508Q160', 'f1508qfp160', 160)]}


def run(fam, pdev, dev, pin, as_input):
    name = 'pm%s_%s_%d_%s' % (fam, pdev, pin, 'i' if as_input else 'o')
    d = F.fresh_dir(os.path.join(ROOT, 'oracle', 'pinmap'), name)
    p = os.path.join(d, name + '.tt2')
    if as_input:
        T.write_tt2(p, name, dev, [('X', pin), ('Y', None)], [], ['X'], ['Y'], [('1', {0})])
        sig = 'X'
    else:
        T.write_tt2(p, name, dev, [('A', None), ('B', None), ('Y', pin)], [], ['A', 'B'], ['Y'], [('11', {0})])
        sig = 'Y'
    r = F.run_fitter(p, fam, pdev, ['-preassign', 'keep', '-strategy', 'JTAG', 'off'])
    res = dict(pin=pin, fits=r['fits'])
    if r['fits']:
        fit = F.parse_fit(r['fit'])
        pl = [x for x in fit['place'] if x['name'] == sig]
        res['mc'] = pl[0]['mc'] if pl else None
        res['placed_pin'] = pl[0]['num'] if pl else None
        res['globals'] = fit['globals']
    else:
        msgs = [l.strip() for l in (r.get('fit', '') + r['stdout']).replace('\r', '').split('\n')
                if re.search(r'(?i)error|warning|cannot|not', l)]
        res['msgs'] = msgs[:6]
    return res


if __name__ == '__main__':
    out = {}
    for fam, devs in DEVS.items():
        for pdev, dev, n in devs:
            jobs = [((pin, k), run, (fam, pdev, dev, pin, k)) for pin in range(1, n + 1) for k in (False, True)]
            res = F.run_batch(jobs, workers=8)
            out[pdev] = {'%d_%s' % (pin, 'i' if k else 'o'): (v if isinstance(v, dict) else str(v)) for (pin, k), v in res.items()}
            io = sorted((v['pin'], v['mc']) for (pin, k), v in res.items() if isinstance(v, dict) and not k and v['fits'])
            print(pdev, 'output pins', len(io))
    json.dump(out, open(os.path.join(ROOT, 'exp', 'pinmap.json'), 'w'), indent=0)
