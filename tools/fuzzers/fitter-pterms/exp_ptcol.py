"""Experiment 1: PT column layout. Each output = one literal of one input -> exactly one 0 in a PT row."""
import json, os, random, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
import fitter_oracle as F
import tt2gen as T

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch')
REG = json.load(open(os.path.join(ROOT, 're', 'regions.json')))
FAM = {'1502': ('1502AS', 'f1502plcc44', 'P1502C44', list(range(1, 45))),
       '1504': ('1504AS', 'f1504plcc44', 'P1504C44', list(range(1, 45))),
       '1508': ('1508AS', 'f1508plcc84', 'P1508C84', list(range(1, 85)))}


def one(fam, seed, nout=12):
    regfam, dev, pdev, _ = FAM[fam]
    rnd = random.Random(seed)
    nin = nout
    ins = ['I%d' % i for i in range(nin)]
    outs = ['Y%d' % i for i in range(nout)]
    pol = [rnd.choice('01') for _ in range(nout)]
    terms = [(T.cube(ins, {ins[i]: pol[i]}), {i}) for i in range(nout)]
    name = 'pc%s_%d' % (fam, seed)
    d = F.fresh_dir(os.path.join(ROOT, 'oracle', 'ptcol'), name)
    p = os.path.join(d, name + '.tt2')
    T.write_tt2(p, name, dev, [(n, None) for n in ins + outs], [], ins, outs, terms)
    r = F.run_fitter(p, fam, pdev, ['-preassign', 'ignore'])
    if not r['fits']:
        return None
    fit = F.parse_fit(r['fit'])
    fz = F.read_jed(r['jed'])
    place = {x['name']: x for x in fit['place'] if x['kind'] == 'pin'}
    R = REG[regfam]
    obs = []
    rows = {(x['mc'], x['pt']): x['addr'] for x in R['pt']}
    for i in range(nout):
        y = place.get(outs[i]); x = ins[i]
        if not y:
            continue
        mc = y['mc']
        nblk = len(R['pt']) // 5 // 16
        blk = 'ABCDEFGH'[(mc - 1) // 16]
        mux = [u['mux'] for u in fit['uim'].get(blk, []) if u['name'] == x]
        used = []
        for k in range(1, 6):
            row = fz[rows[(mc, k)]:rows[(mc, k)] + 96]
            if '1' in row:
                used.append((k, [j for j, c in enumerate(row) if c == '0']))
        obs.append(dict(out=outs[i], mc=mc, block=blk, input=x, pol=pol[i], mux=mux, pts=used))
    return obs


if __name__ == '__main__':
    fam = sys.argv[1]
    n = int(sys.argv[2])
    res = F.run_batch([(s, one, (fam, s)) for s in range(n)], workers=8)
    allobs = [o for v in res.values() if isinstance(v, list) for o in v]
    json.dump(allobs, open(os.path.join(ROOT, 'exp', 'ptcol_%s.json' % fam), 'w'))
    # summarise: column = f(mux, pol)
    from collections import Counter
    c = Counter()
    for o in allobs:
        if len(o['mux']) == 1 and len(o['pts']) == 1 and len(o['pts'][0][1]) == 1:
            c[(o['mux'][0], o['pol'], o['pts'][0][0], o['pts'][0][1][0])] += 1
    for k in sorted(c):
        print(k, c[k])
