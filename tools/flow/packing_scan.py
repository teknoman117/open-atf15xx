"""Count D/Q packing opportunities (d) output register + buried node = its D, and (e) comb output +
buried register of the same value, in pre-fit .tt2 netlists (spec F4.6a). Usage: packing_scan.py X.tt2...
"""
import re, sys, glob, collections
def parse(path):
    t = open(path, errors='replace').read()
    ilb = re.search(r'\.ilb\s+(.*)', t).group(1).split()
    ob = re.search(r'\.ob\s+(.*)', t).group(1).split()
    pins = set(x.split('+')[0].split('-')[0].split(':')[0] for x in (re.search(r'#\$ PINS\s+\d+\s+(.*)', t) or re.search('()', '')).group(1).split())
    fn = collections.defaultdict(set)
    for l in t.split('\n'):
        m = re.match(r'^([01\-]+)\s+([01\-~]+)$', l.strip())
        if not m: continue
        inp, out = m.groups()
        cube = tuple((ilb[i], c) for i, c in enumerate(inp) if c != '-')
        for j, c in enumerate(out):
            if c == '1': fn[ob[j]].add(cube)
    return ilb, ob, pins, fn
tot = collections.Counter()
for p in sys.argv[1:]:
    ilb, ob, pins, fn = parse(p)
    regs = {o[:-2] for o in ob if o.endswith('.D')}
    combs = {o for o in ob if '.' not in o}
    for r in regs:
        d = fn[r + '.D']
        lit = None
        if len(d) == 1:
            (cube,) = d
            if len(cube) == 1 and cube[0][1] == '1': lit = cube[0][0]
        for c in combs:
            same = fn[c] == d and d
            via = lit in (c, c + '.Q', c + '.PIN')
            if not (same or via): continue
            if r in pins and c not in pins: tot['(d) output reg %s + node %s' % (r, c) if False else '(d)'] += 1; print(p.split('/')[-2], '(d) pin-reg', r, 'D=', c)
            elif c in pins and r not in pins: tot['(e)'] += 1; print(p.split('/')[-2], '(e) comb-pin', c, 'buried reg', r)
print(dict(tot))
