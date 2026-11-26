"""Experiment 1c: saturate one block's UIM so high mux indices are used, then solve column <-> (mux, pol)."""
import json, os, random, sys
from collections import defaultdict
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
import fitter_oracle as F
import tt2gen as T
from exp_ptcol2 import io_pins, DEV, REG

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch')


def run(fam, seed, blk):
    pdev, dev, nmc = DEV[fam]
    rnd = random.Random(seed)
    pins = io_pins(pdev)
    ypins = [p for p, m in pins.items() if (m - 1) // 16 == blk]
    rnd.shuffle(ypins)
    ypins = ypins[:8]
    other = [p for p in pins if p not in ypins]
    rnd.shuffle(other)
    nsrc = rnd.randint(28, 40)
    nodes_mc = [m for m in range(1, nmc + 1) if (m - 1) // 16 != blk]
    rnd.shuffle(nodes_mc)
    srcs = []
    for i in range(nsrc):
        if rnd.random() < 0.5 and other:
            srcs.append(('p', other.pop()))
        else:
            srcs.append(('n', nodes_mc.pop()))
    name = 'pc3%s_%d_%d' % (fam, blk, seed)
    d = F.fresh_dir(os.path.join(ROOT, 'oracle', 'ptcol3'), name)
    pth = os.path.join(d, name + '.tt2')
    pinsd = [('S%d' % i, p) for i, (k, p) in enumerate(srcs) if k == 'p'] + [('Y%d' % j, y) for j, y in enumerate(ypins)] + [('A', None), ('CK', None)]
    nodes = ['S%d' % i for i, (k, p) in enumerate(srcs) if k == 'n']
    ins = ['A', 'CK'] + [('S%d' % i) + ('.Q' if k == 'n' else '') for i, (k, p) in enumerate(srcs)]
    outs = []
    terms = []
    for i, (k, p) in enumerate(srcs):
        if k == 'n':
            outs += ['S%d.REG' % i, 'S%d.C' % i]
            terms += [(T.cube(ins, {'A': '1'}), {len(outs) - 2}), (T.cube(ins, {'CK': '1'}), {len(outs) - 1})]
    # assign sources to Y's: 5 per Y
    order = list(range(nsrc)); rnd.shuffle(order)
    eqs = {}
    for j, y in enumerate(ypins):
        mine = order[5 * j:5 * j + 5]
        if not mine:
            continue
        outs.append('Y%d' % j)
        lits = []
        for i in mine:
            pol = rnd.choice('01')
            terms.append((T.cube(ins, {ins[2 + i]: pol}), {len(outs) - 1}))
            lits.append(('S%d' % i, pol))
        eqs['Y%d' % j] = lits
    T.write_tt2(pth, name, dev, pinsd, nodes, ins, outs, terms)
    txt = open(pth).read()
    for i, (k, p) in enumerate(srcs):
        if k == 'n':
            txt = txt.replace(' S%d+ ' % i, ' S%d+:%d ' % (i, 600 + p), 1)
    open(pth, 'w').write(txt)
    r = F.run_fitter(pth, fam, pdev, ['-preassign', 'keep', '-strategy', 'JTAG', 'off', '-strategy', 'Foldback_Logic', 'off'])
    if not r['fits']:
        return None
    fit = F.parse_fit(r['fit'])
    fz = F.read_jed(r['jed'])
    R = REG[fam + 'AS']
    rows = {(x['mc'], x['pt']): x['addr'] for x in R['pt']}
    out = []
    for yname, lits in eqs.items():
        y = [x for x in fit['place'] if x['name'] == yname]
        if not y:
            continue
        mc = y[0]['mc']; b = 'ABCDEFGH'[(mc - 1) // 16]
        cand = set()
        for s, pol in lits:
            mu = [u['mux'] for u in fit['uim'].get(b, []) if u['name'] == s]
            if len(mu) == 1:
                cand.add((mu[0], pol))
        zcols = []
        for k in range(1, 6):
            row = fz[rows[(mc, k)]:rows[(mc, k)] + 96]
            if '1' in row:
                zcols.append([j for j, c in enumerate(row) if c == '0'])
        out.append(dict(block=b, cand=sorted(cand), zcols=zcols, nlits=len(lits)))
    return out


if __name__ == '__main__':
    fam = sys.argv[1]; n = int(sys.argv[2])
    nblk = DEV[fam][2] // 16
    res = F.run_batch([((s, b), run, (fam, s, b)) for s in range(n) for b in range(nblk)], workers=8)
    obs = [o for v in res.values() if isinstance(v, list) for o in v]
    json.dump(obs, open(os.path.join(ROOT, 'exp', 'ptcol3_%s.json' % fam), 'w'))
    # solve: for each block, column -> intersection of candidate sets of rows containing it
    sol = defaultdict(lambda: None)
    for o in obs:
        if len(o['cand']) != o['nlits']:
            continue
        for zc in o['zcols']:
            if len(zc) != 1:
                continue
            key = (o['block'], zc[0])
            s = set(map(tuple, o['cand']))
            sol[key] = s if sol[key] is None else sol[key] & s
    final = {}
    for (b, c), s in sol.items():
        if s and len(s) == 1:
            final[(b, c)] = next(iter(s))
    per = defaultdict(set)
    for (b, c), mp in final.items():
        per[mp].add(c)
    print(fam, 'obs', len(obs), 'solved cols', len(final), 'distinct (mux,pol)', len(per), 'block-dependent', {k: v for k, v in per.items() if len(v) > 1})
    print(' missing', sorted(set((x, p) for x in range(40) for p in '01') - set(per)))
    json.dump({'%s_%d' % k: v for k, v in final.items()}, open(os.path.join(ROOT, 'exp', 'ptcolsol_%s.json' % fam), 'w'))
