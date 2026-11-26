#!/usr/bin/env python3
"""Compare our fit150x replacements against the vendor fitters on .tt2 designs.

For every .tt2 given (files or directories, searched recursively):
  * the vendor result is taken from a sibling .jed/.fit written by the vendor fitter (the command line
    is read back from line 4 of the .fit), or produced now with --run-vendor (wine, see fitter_oracle.py);
  * our fitter (target/release/fit150x, or --bin-dir) is run with the same arguments and the vendor's
    date, in a scratch directory;
  * the two JEDEC files are compared fuse for fuse (our layout differs from the vendor's): `identical`
    means the same fuse array, the same security fuse (`G`) and a correct fuse checksum (`C`) in ours;
    `bad-checksum` means the same fuses with a missing or wrong `C` field; on a fuse mismatch the
    differing fuses are classified by region (PT array / macrocell config / UIM / global OE / config).

Usage:
  tools/fuzzers/fitter-conformance/verify_tt2.py [--bin-dir DIR] [--run-vendor -- vendor args] [-v] PATH...
"""
import argparse, collections, glob, json, os, re, shutil, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..', '..'))
sys.path.insert(0, os.path.join(HERE, '..', 'common'))
import fitter_oracle as F  # noqa: E402

FAM_QF = {'1502': 16808, '1504': 34192, '1508': 74136}
BLOCKS = {'1502': 2, '1504': 4, '1508': 8}
WIDTH = {'1502': 5, '1504': 9, '1508': 27}


def region(fam, a):
    pt_end = BLOCKS[fam] * 80 * 96
    uim = pt_end + BLOCKS[fam] * 480
    goe = uim + 40 * BLOCKS[fam] * WIDTH[fam]
    cfg = goe + 6 * WIDTH[fam]
    if a < pt_end:
        return 'pt'
    if a < uim:
        return 'mcfg'
    if a < goe:
        return 'uim'
    if a < cfg:
        return 'goe'
    return 'config'


def security(text):
    m = re.search(r'\*\s*G([01])\s*\*', text)
    return m.group(1) if m else '0'


def checksum_ok(text, fuses):
    """True if the JED's C field is the JEDEC fuse checksum (sum of 8-fuse bytes, fuse 0 = LSB)."""
    m = re.search(r'\*\s*C([0-9A-Fa-f]{4})\s*\*', text)
    want = sum(int(fuses[i:i + 8][::-1], 2) for i in range(0, len(fuses), 8)) & 0xffff
    return bool(m) and int(m.group(1), 16) == want


def vendor_cmd(fit_text):
    for l in fit_text.replace('\r', '').split('\n')[:8]:
        if re.match(r'fit150[248] ', l):
            return l.split()
    return None


def find_designs(paths):
    for p in paths:
        if os.path.isdir(p):
            yield from sorted(glob.glob(os.path.join(p, '**', '*.tt2'), recursive=True))
        else:
            yield p


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('paths', nargs='+')
    ap.add_argument('--bin-dir', default=None)
    ap.add_argument('--run-vendor', nargs=argparse.REMAINDER, default=None,
                    help='run the vendor fitter now with these arguments (after the .tt2)')
    ap.add_argument('-v', '--verbose', action='store_true')
    ap.add_argument('--json', help='write per-design results here')
    a = ap.parse_args()
    bindir = a.bin_dir or next((d for d in (os.path.join(ROOT, '.scratch', 'target-docker', 'release'),
                                            os.path.join(ROOT, 'target', 'release')) if os.path.exists(d)), None)
    stats = collections.defaultdict(collections.Counter)
    results = []
    for tt2 in find_designs(a.paths):
        base = tt2[:-4]
        txt = open(tt2, errors='replace').read()
        if a.run_vendor is not None:
            m = re.search(r'#\$ DEVICE\s+f(150[248])', txt)
            fam = m.group(1) if m else None
            r = F.run_fitter(tt2, fam, None, a.run_vendor)
            vjed, vfit = r['jed'], r['fit']
        else:
            if not (os.path.exists(base + '.jed') and os.path.exists(base + '.fit')):
                continue
            vjed = open(base + '.jed', 'rb').read().decode('latin-1')
            vfit = open(base + '.fit', errors='replace').read()
        cmd = vendor_cmd(vfit)
        if not vjed or not cmd:
            stats['?']['vendor-failed'] += 1
            continue
        fam = cmd[0][3:7]
        m = re.search(r'Created(?: on)?: ([^*\r\n]*)', vjed)
        created = m.group(1).strip() if m else 'unknown'
        with tempfile.TemporaryDirectory() as d:
            name = os.path.basename(tt2)
            shutil.copy(tt2, os.path.join(d, name))
            p = subprocess.run([os.path.join(bindir, 'fit' + fam), name] + cmd[2:] + ['-created', created], cwd=d,
                               capture_output=True, text=True)
            ojed_path = os.path.join(d, name[:-4] + '.jed')
            ojed = open(ojed_path, 'rb').read().decode('latin-1') if p.returncode == 0 and os.path.exists(ojed_path) else None
        res = dict(tt2=tt2, family=fam)
        if ojed is None:
            res['status'] = 'ours-failed'
            res['error'] = (p.stderr or p.stdout).strip()[:200]
        else:
            vf, of = F.read_jed(vjed), F.read_jed(ojed)
            if vf == of and security(vjed) == security(ojed):
                res['status'] = 'identical' if checksum_ok(ojed, of) else 'bad-checksum'
            else:
                diff = [i for i in range(len(vf)) if vf[i] != of[i]]
                res['status'] = 'different'
                res['regions'] = dict(collections.Counter(region(fam, i) for i in diff)) or {'security': 1}
                res['first'] = diff[:12]
        stats[fam][res['status']] += 1
        if res['status'] == 'different':
            for k in res['regions']:
                stats[fam]['diff-in-' + k] += 1
        results.append(res)
        if a.verbose and res['status'] != 'identical':
            print(res)
    for fam in sorted(stats):
        tot = sum(v for k, v in stats[fam].items() if not k.startswith('diff-in-'))
        print('ATF%sAS: %d designs: %s' % (fam, tot, ', '.join('%s %d' % kv for kv in sorted(stats[fam].items()))))
    if a.json:
        json.dump(results, open(a.json, 'w'), indent=1)


if __name__ == '__main__':
    main()
