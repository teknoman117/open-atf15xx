"""Experiment 1b: PT column layout with full source coverage.

For each source S (every I/O pin used as an input, and every macrocell feedback via a preassigned node)
and each block b: output Y (pinned in block b) = S or !S. One PT, one connected column.
"""
import json, os, random, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
import fitter_oracle as F
import tt2gen as T

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch')
REG = json.load(open(os.path.join(ROOT, 're', 'regions.json')))
PINMAP = json.load(open(os.path.join(ROOT, 'exp', 'pinmap.json')))
DEV = {'1502': ('P1502C44', 'f1502plcc44', 32), '1504': ('P1504C84', 'f1504plcc84', 64), '1508': ('P1508Q160', 'f1508qfp160', 128)}


def io_pins(pdev):
    v = PINMAP[pdev]
    return {int(k[:-2]): x['mc'] for k, x in v.items() if k.endswith('_o') and isinstance(x, dict) and x['fits']}


def run(fam, src, ypin, pol):
    pdev, dev, nmc = DEV[fam]
    name = 'pc2%s_%s_%d_%s' % (fam, src, ypin, pol)
    d = F.fresh_dir(os.path.join(ROOT, 'oracle', 'ptcol2'), name)
    p = os.path.join(d, name + '.tt2')
    if src.startswith('p'):
        pins = [('S', int(src[1:])), ('Y', ypin)]
        T.write_tt2(p, name, dev, pins, [], ['S'], ['Y'], [(pol, {0})])
    else:   # node feedback of MC m: N = A (pin floating), registered so it is a real node
        m = int(src[1:])
        T.write_tt2(p, name, dev, [('A', None), ('CK', None), ('Y', ypin)], ['N'], ['A', 'CK', 'N.Q'], ['N.REG', 'N.C', 'Y'],
                    [('1--', {0}), ('-1-', {1}), ('--' + pol, {2})])
        txt = open(p).read().replace('#$ NODES 1 N+ ', '#$ NODES 1 N+:%d ' % (600 + m))
        open(p, 'w').write(txt)
    r = F.run_fitter(p, fam, pdev, ['-preassign', 'keep', '-strategy', 'JTAG', 'off'])
    if not r['fits']:
        return None
    fit = F.parse_fit(r['fit'])
    fz = F.read_jed(r['jed'])
    y = [x for x in fit['place'] if x['name'] == 'Y'][0]
    blk = 'ABCDEFGH'[(y['mc'] - 1) // 16]
    sname = 'S' if src.startswith('p') else 'N'
    mux = [u for u in fit['uim'].get(blk, []) if u['name'] == sname]
    R = REG[fam + 'AS']
    rows = {(x['mc'], x['pt']): x['addr'] for x in R['pt']}
    used = []
    for k in range(1, 6):
        row = fz[rows[(y['mc'], k)]:rows[(y['mc'], k)] + 96]
        if '1' in row:
            used.append((k, [j for j, c in enumerate(row) if c == '0']))
    return dict(src=src, ypin=ypin, ymc=y['mc'], block=blk, pol=pol, mux=[(u['mux'], u['ref']) for u in mux], pts=used)


if __name__ == '__main__':
    fam = sys.argv[1]
    pdev, dev, nmc = DEV[fam]
    pins = io_pins(pdev)
    # one Y pin per block
    yp = {}
    for p, m in sorted(pins.items()):
        yp.setdefault((m - 1) // 16, p)
    srcs = ['p%d' % p for p in pins] + ['n%d' % m for m in range(1, nmc + 1)]
    jobs = []
    for s in srcs:
        for b, y in yp.items():
            if s == 'p%d' % y:
                continue
            for pol in '10':
                jobs.append(((s, y, pol), run, (fam, s, y, pol)))
    res = F.run_batch(jobs, workers=8)
    obs = [v for v in res.values() if isinstance(v, dict)]
    json.dump(obs, open(os.path.join(ROOT, 'exp', 'ptcol2_%s.json' % fam), 'w'))
    from collections import defaultdict
    cols = defaultdict(set)
    for o in obs:
        if len(o['mux']) == 1 and len(o['pts']) == 1 and len(o['pts'][0][1]) == 1:
            cols[(o['block'], o['mux'][0][0], o['pol'])].add(o['pts'][0][1][0])
    per = defaultdict(set)
    for (b, mux, pol), c in cols.items():
        per[(mux, pol)] |= c
    print(fam, 'runs', len(jobs), 'ok', len(obs), 'covered', len(per), 'ambiguous', [k for k, v in per.items() if len(v) > 1])
    blkdep = [(k, v) for k, v in per.items() if len(v) > 1]
    print(' missing', sorted(set((x, p) for x in range(40) for p in '01') - set(per)))
