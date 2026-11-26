"""Global clock network source mux and invert, on silicon (designs/vpol.v + patched JEDs):
base: a on GCLK2 (P2), b on GCLK3 (P81). v1: GCLK3 network sourced from the GCLK2 pin (config
C+18,19 = 1,0). v2: v1 + GCLK3 invert (C+17 = 1). Toggles P2 edge by edge (P81 held low) and
records (a, b) after each edge."""
import os, subprocess, sys
from bench import Bench
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
O = os.path.join(ROOT, '.scratch', 'silicon', 'vpol')
b = Bench(); b.mode(2, 'i'); b.mode(3, 'i')
for p in (4, 5, 8, 9, 19): b.mode(p, '0')
ok = True
for v, expect in (('vpol', 'rise:a'), ('v1', 'rise:a,b'), ('v2', 'rise:a fall:b')):
    b.mode(9, '0')
    r = subprocess.run([os.path.join(ROOT, 'tools/silicon/program.sh'), '%s/%s.svf' % (O, v), '%s/%s.jed' % (O, v)], capture_output=True, text=True)
    if ' 0 errors' not in r.stdout + r.stderr: print(r.stdout, r.stderr); sys.exit(2)
    trace = []; prev = b.read(); s = (prev[2], prev[3])
    log = []
    for k in range(8):
        lvl = 1 - (k % 2) if False else (k + 1) % 2   # 1,0,1,0...
        b.mode(9, str(lvl)); cur = b.read(); n = (cur[2], cur[3])
        ch = [x for x, i in (('a', 0), ('b', 1)) if n[i] != s[i]]
        log.append('%s:%s' % ('rise' if lvl else 'fall', ','.join(ch) or '-')); s = n
    got = ' '.join(log)
    pat = {'vpol': ['rise:a', 'fall:-'], 'v1': ['rise:a,b', 'fall:-'], 'v2': ['rise:a', 'fall:b']}[v] * 4
    good = log == pat
    ok &= good
    print('%-5s %s   %s' % (v, got, 'OK' if good else 'expected ' + ' '.join(pat)))
b.close()
sys.exit(0 if ok else 1)
