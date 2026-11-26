"""Experiment 6: foldback-expander column and cascade direction, all families.

fold: node N = !(A & B) placed at foldback node 300+m; Y = N & C (pinned elsewhere in m's block).
cas : Y = sum of 9 two-literal products (needs > 5 PTs), cascade on, Y pinned to each pin.
"""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'fitter-macrocell'))
import fitter_oracle as F
import tt2gen as T
from exp_feat import DEV, target_pins

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch')
REG = json.load(open(os.path.join(ROOT, 're', 'regions.json')))


def run(fam, kind, m, ypin):
    pdev, dev = DEV[fam][:2]
    name = 'fd%s_%s_%d_%d' % (fam, kind, m, ypin)
    d = F.fresh_dir(os.path.join(ROOT, 'oracle', 'fold'), name)
    p = os.path.join(d, name + '.tt2')
    opts = ['-preassign', 'keep']
    if kind == 'fold':
        ins = ['A', 'B', 'C', 'N']
        T.write_tt2(p, name, dev, [('A', None), ('B', None), ('C', None), ('Y', ypin)], ['N'], ins, ['N-', 'Y'],
                    [('11--', {0}), ('--11', {1})])
        txt = open(p).read().replace('#$ NODES 1 N+ ', '#$ NODES 1 N+:%d ' % (300 + m))
        open(p, 'w').write(txt)
    else:
        ins = ['I%d' % i for i in range(10)]
        terms = [(T.cube(ins, {ins[i]: '1', ins[i + 1]: '1'}), {0}) for i in range(9)]
        T.write_tt2(p, name, dev, [(x, None) for x in ins] + [('Y', ypin)], [], ins, ['Y'], terms)
        opts += ['-strategy', 'Cascade_Logic', 'on', '-strategy', 'Foldback_Logic', 'off']
    r = F.run_fitter(p, fam, pdev, opts)
    if not r['fits']:
        return dict(fits=False)
    fit = F.parse_fit(r['fit'])
    fz = F.read_jed(r['jed'])
    R = REG[fam + 'AS']
    rows = {(x['mc'], x['pt']): x['addr'] for x in R['pt']}
    used = {}
    for (mc, k), a in rows.items():
        if '1' in fz[a:a + 96]:
            used['%d.%d' % (mc, k)] = [j for j, c in enumerate(fz[a:a + 96]) if c == '0']
    a0 = min(x['addr'] for x in R['mcfg'])
    return dict(fits=True, place=fit['place'], used=used, cfg=fz[a0:], cfg_base=a0, log=r['fit'][-3000:])


if __name__ == '__main__':
    fam = sys.argv[1]
    tp = target_pins(fam)
    nmc = {'1502': 32, '1504': 64, '1508': 128}[fam]
    byblk = {}
    for p, mc in tp.items():
        byblk.setdefault((mc - 1) // 16, []).append(p)
    jobs = []
    for m in range(1, nmc + 1):
        b = (m - 1) // 16
        if b in byblk:
            jobs.append((('fold', m), run, (fam, 'fold', m, byblk[b][0] if tp[byblk[b][0]] != m else byblk[b][1])))
    for p, mc in tp.items():
        jobs.append((('cas', mc), run, (fam, 'cas', mc, p)))
    res = F.run_batch(jobs, workers=8)
    out = {'%s|%d' % k: v for k, v in res.items() if isinstance(v, dict)}
    json.dump(out, open(os.path.join(ROOT, 'exp', 'fold_%s.json' % fam), 'w'))
    from collections import Counter
    print(fam, Counter((k.split('|')[0], v['fits']) for k, v in out.items()), [str(v) for v in res.values() if not isinstance(v, dict)][:2])
