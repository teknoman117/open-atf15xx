#!/usr/bin/env python3
"""Placement headroom: per-block UIM input counts of a fitted JED, and how far a local search that
moves buried (non-pin) cells between blocks can reduce the worst block (capacity 16 MCs/block)."""
import re, subprocess, sys, random, collections
# usage: placement_headroom.py design.jed P1504C44 4   (blocks: 1502 2, 1504 4, 1508 8)
jed, dev, nblk = sys.argv[1], sys.argv[2], int(sys.argv[3])
eq = subprocess.run(['target/release/atf15xx-sim', 'equations', jed, '--device', dev], capture_output=True, text=True).stdout
cells = {}; cur = None
for l in eq.splitlines():
    m = re.match(r'MC(\d+)( \(pin \d+\))?: (\w+)', l)
    if m:
        cur = int(m.group(1)); cells[cur] = {'pin': bool(m.group(2)), 'src': set()}; continue
    if cur and l.startswith('    PT'):
        cells[cur]['src'] |= set(re.findall(r'(?:fb|pad)_\d+|p\d+_in', l))
used = {k: v for k, v in cells.items() if v['src']}
blk = {k: (k - 1) // 16 for k in used}
def cost(b):
    ins = collections.defaultdict(set)
    for k, v in used.items(): ins[b[k]] |= v['src']
    return sorted((len(s) for s in ins.values()), reverse=True)
print('fitted: %d cells with PTs; per-block UIM inputs (desc): %s' % (len(used), cost(blk)))
movable = [k for k, v in used.items() if not v['pin']]
rnd = random.Random(1); best = dict(blk); bc = cost(best)
for it in range(20000):
    b = dict(best); k = rnd.choice(movable); t = rnd.randrange(nblk)
    if sum(1 for x in b.values() if x == t) >= 16: continue
    b[k] = t; c = cost(b)
    if c <= bc: best, bc = b, c
print('after local search (moving %d buried cells): %s' % (len(movable), bc))
