#!/usr/bin/env python3
"""Rewrite the `Load` outputs of atmisp test vectors (`expected-*.jed.gz`, spec §11.2) in atmisp's
own JEDEC layout (spec §8.2), so that no ATMISP-written JEDEC text is committed.

For every vector with an expected-*.jed.gz under VECTOR_DIR (default atf15xx-tool/tests/vectors),
atmisp (target/release/atf15xx-tool, or --cli) is run on the vector's chain with the file's timestamp; the
JEDEC file it writes must hold the same fuses and security fuse as the expected file, and both
must state the same, correct checksum (and the 1502BE none, the others a `G0` field). Without
--write it only checks; with --write it replaces each expected file (gzip, mtime 0). Run it after
tools/fuzzers/atmisp-svf/assemble.py when vectors are regenerated (§11.4).

  python3 tools/fuzzers/atmisp-svf/rewrite_load_vectors.py [--write] [--cli PATH] [VECTOR_DIR]
"""
import argparse, glob, gzip, os, re, subprocess, tempfile
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))

def fuses(t):
    body = t.split('\x02', 1)[-1].split('\x03', 1)[0]
    flds = [f.strip() for f in body.split('*')]
    flds = [f for f in flds if 'NOTE' not in f and 'Note' not in f]
    qf = next(int(f[2:]) for f in flds if f.startswith('QF'))
    d = next((f[1] for f in flds if re.fullmatch(r'F[01]', f)), '0')
    fz = [d] * qf
    for f in flds:
        m = re.fullmatch(r'L(\d+)\s+([01\s]+)', f)
        if m:
            b = re.sub(r'\s', '', m.group(2)); a = int(m.group(1)); fz[a:a + len(b)] = list(b)
    g = next((f[1] for f in flds if re.fullmatch(r'G[01]', f)), '0')
    c = next((int(f[1:], 16) for f in flds if re.fullmatch(r'C[0-9A-F]+', f)), None)
    return ''.join(fz), g, c

def csum(f):
    return sum(int(f[i:i + 8][::-1], 2) for i in range(0, len(f), 8)) & 0xffff

ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
ap.add_argument('vectors', nargs='?', default=os.path.join(ROOT, 'atf15xx-tool', 'tests', 'vectors'))
ap.add_argument('--write', action='store_true')
ap.add_argument('--cli', default=os.path.join(ROOT, 'target', 'release', 'atf15xx-tool'))
a = ap.parse_args()
CLI, write = a.cli, a.write
for exp in sorted(glob.glob(os.path.join(a.vectors, '*', '*', 'expected-*.jed.gz'))):
    case = os.path.dirname(exp); name = os.path.basename(case)
    old = gzip.decompress(open(exp, 'rb').read()).decode('latin-1')
    m = re.search(r'Created on:  ([^\r\n]*)', old) or re.search(r'Created: ([^*]*)\*', old)
    ts = m.group(1)
    jname = os.path.basename(exp)[len('expected-'):-3]
    with tempfile.TemporaryDirectory() as d:
        cd = os.path.join(d, 'probe', 'cases', name); os.makedirs(cd)
        for f in os.listdir(case):
            if f.endswith('.jed'):
                open(os.path.join(cd, f), 'wb').write(open(os.path.join(case, f), 'rb').read())
        r = subprocess.run([CLI, 'svf', '-q', os.path.join(case, 'chain.chn'), '-o', os.path.join(d, 'x.svf'),
                            '--timestamp', ts, '--drive', 'C=' + d, '--allow-mixed-voltage'], capture_output=True, text=True)
        assert r.returncode == 0, r.stderr
        new = open(os.path.join(cd, jname), 'rb').read().decode('latin-1')
    fo, go, co = fuses(old); fn, gn, cn = fuses(new)
    ok = fo == fn and go == gn and co == cn == csum(fn) and ('G0*' in old) == ('G0*' in new)
    print(name, jname, 'same fuses' if fo == fn else 'FUSES DIFFER', 'G', go, gn, 'C %X %X' % (co, cn), 'OK' if ok else 'BAD')
    assert ok
    if write:
        open(exp, 'wb').write(gzip.compress(new.encode('latin-1'), mtime=0))
