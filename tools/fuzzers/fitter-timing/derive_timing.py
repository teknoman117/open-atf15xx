"""Derive the speed-grade timing tables (atf15xx-arch/data/timing.json) from public datasheets and
the vendor fitters' SDF output only.

  python3 tools/fuzzers/fitter-timing/derive_timing.py --corpus .scratch/timing/corpus \
      --datasheet 1502:7,10,25=ATF1502AS.txt --datasheet 1502:15,20=ATF1502ASV.txt \
      --datasheet 1504:7,10,25=ATF1504AS.txt --datasheet 1508:7,10,15,20,25=ATF1508AS.txt \
      [-o timing.json] [--compare atf15xx-arch/data/timing.json]

1. Datasheet [E]: every AC-characteristics row of each datasheet (`pdftotext -layout` text), its
   numbers assigned to the grades the datasheet covers and truncated to whole nanoseconds, as the
   fitters' SDF output shows they are (0.5 -> 0, 7.5 -> 7). A grade no datasheet of the family
   covers is taken from the nearest family that has it ([I], 1504AS -15/-20 from the 1508AS).
2. SDF [O]: the fitters' delays per cell and path across the five grades (sdf_corpus.sh). A delay
   that is one datasheet parameter (logic array AND = tLAD, cascade OR = tPEXP, output buffer =
   tOD1 / tXZ, register clock-to-out / reset / preset / enable / setup / hold = tRD / tCLR / tPRE /
   tEN / tSU / tH) sets that parameter for every grade: where it differs from the truncated
   datasheet value the fitter's value is used and the difference reported.
3. Check: every SDF delay must then equal one of the compositions of the datasheet timing model
   (check_sdf_timing.py's EXPECTED); otherwise the run fails.

Each parameter records its evidence per grade: "sdf", "datasheet" or "inferred:<family>".
"""
import argparse, collections, glob, json, math, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_sdf_timing as C  # noqa: E402

GRADES = ['7', '10', '15', '20', '25']
# delays that are exactly one parameter: (cell class, path) -> parameter
SINGLE = {
    ('AND', 'IOPATH'): 'tLAD',
    ('OR', 'IOPATH'): 'tPEXP',
    ('TRI', 'IOPATH A Q'): 'tOD1',
    ('TRI', 'IOPATH EN Q'): 'tXZ',
    ('DFF', 'IOPATH (posedge CLK) Q'): 'tRD',
    ('DFF', 'IOPATH (posedge AR) Q'): 'tCLR',
    ('DFF', 'IOPATH (posedge AS) Q'): 'tPRE',
    ('DFF', 'IOPATH CE Q'): 'tEN',
    ('DFF', 'SETUP D (posedge CLK)'): 'tSU',
    ('DFF', 'HOLD D (posedge CLK)'): 'tH',
}
NEAREST = {'1502': ['1504', '1508'], '1504': ['1508', '1502'], '1508': ['1504', '1502']}


def datasheet_rows(path):
    """(name, numbers) per AC-table row; the first tSU/tH/tFSU/tFH rows are the global-clock ones."""
    seen = collections.Counter()
    for line in open(path, encoding='utf-8', errors='replace'):
        m = re.match(r'\s*(t[A-Z]+[0-9]?)\s+(.*)', line)
        if not m:
            continue
        sym, rest = m.groups()
        rest = re.sub(r'\(\d\)', ' ', rest)                       # footnote markers such as "(2)"
        nums = [float(x) for x in re.findall(r'(?<![\w.])(\d+(?:\.\d+)?)(?![\w.])', rest)]
        if not nums:
            continue
        name = sym
        if sym in ('tSU', 'tH', 'tFSU', 'tFH') and seen[sym] == 0:
            name = sym + '_global'
        seen[sym] += 1
        yield name, nums


def from_datasheet(path, grades):
    """{param: {grade: value}} for the grades the datasheet covers (first occurrence wins)."""
    out = {}
    for name, nums in datasheet_rows(path):
        if name in out:
            continue
        if len(nums) == len(grades):
            gs = grades
        elif len(nums) == 5:                                       # e.g. a power-down table with all grades
            gs = GRADES
        elif len(nums) == len(grades) + 1 and name in ('tPD1', 'tPD2'):
            # one grade also prints a minimum: drop the first number smaller than its predecessor,
            # or the first number when it is the smallest (7.5 10 [3] 15 20 25; [3] 15 20)
            i = next((i for i in range(1, len(nums)) if nums[i] < nums[i - 1]), 0)
            nums = nums[:i] + nums[i + 1:]
            gs = grades
        else:
            continue
        out[name] = {g: int(math.floor(x + 1e-9)) for g, x in zip(gs, nums)}
    return out


def sdf_observations(corpus):
    """{family: {(cell class, path): Counter(5-grade vector)}} over the SDF corpus."""
    obs = collections.defaultdict(lambda: collections.defaultdict(collections.Counter))
    for d in sorted(glob.glob(os.path.join(corpus, '*'))):
        per = {}
        for g in GRADES:
            f = glob.glob(os.path.join(d, g, '*.sdo'))
            if f:
                per[g] = C.parse(f[0])
        if len(per) != 5:
            continue
        tt2 = glob.glob(os.path.join(d, '7', '*.tt2'))[0]
        fam = re.search(r'150[248]', open(tt2).read().split('DEVICE', 1)[1].split()[0]).group(0)
        for k in per['7']:
            _, ct, key, _ = k
            vec = tuple(per[g].get(k) for g in GRADES)
            if None not in vec:
                obs[fam][(C.cls(ct), key)][vec] += 1
    return obs


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--corpus', required=True)
    ap.add_argument('--datasheet', action='append', required=True, help='FAMILY:GRADES=TEXT, e.g. 1508:7,10,15,20,25=ATF1508AS.txt')
    ap.add_argument('-o', '--output')
    ap.add_argument('--compare')
    a = ap.parse_args()

    sheets = collections.defaultdict(dict)                 # family -> param -> grade -> value
    sources = collections.defaultdict(list)
    for spec in a.datasheet:
        lhs, path = spec.split('=', 1)
        fam, gl = lhs.split(':')
        for p, vals in from_datasheet(path, gl.split(',')).items():
            sheets[fam].setdefault(p, {}).update({g: v for g, v in vals.items() if g not in sheets[fam].get(p, {})})
        sources[fam].append(os.path.basename(path))

    obs = sdf_observations(a.corpus)
    params = sorted(set().union(*(set(s) for s in sheets.values())))
    out = {'source': 'black box: datasheets %s; vendor-fitter SDF output, %s (tools/fuzzers/fitter-timing)'
                     % ('; '.join('%s: %s' % (f, ', '.join(s)) for f, s in sorted(sources.items())), a.corpus),
           'grades': ['-' + g for g in GRADES],
           'note': 'whole ns per grade (-7 -10 -15 -20 -25): datasheet values truncated as the fitters do; '
                   'values visible in the fitters\' SDF output are the fitters\' own',
           'families': {}}
    report = []
    for fam in ('1502', '1504', '1508'):
        P = {}
        for p in params:
            vals, ev = [], []
            for g in GRADES:
                v = sheets[fam].get(p, {}).get(g)
                e = 'datasheet'
                if v is None:
                    for other in NEAREST[fam]:
                        v = sheets[other].get(p, {}).get(g)
                        if v is not None:
                            e = 'inferred:' + other
                            break
                vals.append(v)
                ev.append(e if v is not None else 'unknown')
            P[p] = [vals, ev]
        for key, param in SINGLE.items():
            vecs = [v for v in obs[fam].get(key, {}) if any(v)]
            if not vecs:
                continue
            if len(vecs) > 1:
                report.append('%s %s %s: several SDF values %s, keeping the datasheet' % (fam, key[0], key[1], vecs))
                continue
            vals, ev = P[param]
            for i, g in enumerate(GRADES):
                if vals[i] != vecs[0][i]:
                    report.append('%s %s -%s: SDF %s, datasheet/inferred %s (%s)' % (fam, param, g, vecs[0][i], vals[i], ev[i]))
                vals[i] = vecs[0][i]
                ev[i] = 'sdf'
        out['families'][fam] = {'params': {p: {'min': v, 'max': v, 'unit': 'ns', 'evidence': e}
                                           for p, (v, e) in P.items() if any(x is not None for x in v)}}

    # 3. every SDF delay is a datasheet-model composition of the derived table
    unexplained = 0
    for fam, by in obs.items():
        table = {p: {'min': [x or 0 for x in d['min']]} for p, d in out['families'][fam]['params'].items()}
        for key, vecs in by.items():
            exp = C.EXPECTED.get(key)
            for vec in vecs:
                if exp is None or not any(C.evaluate(e, table) == list(vec) for e in exp):
                    unexplained += vecs[vec]
                    report.append('%s %s %s: SDF %s is none of %s' % (fam, key[0], key[1], vec, exp))
    n = sum(sum(c.values()) for by in obs.values() for c in by.values())
    print('SDF delays: %d, explained by the derived table: %d' % (n, n - unexplained))
    for r in report:
        print('  ' + r)

    if a.compare and os.path.exists(a.compare):
        old = json.load(open(a.compare))['families']
        diff = []
        for fam, d in out['families'].items():
            for p, v in d['params'].items():
                o = old.get(fam, {}).get('params', {}).get(p, {}).get('max')
                if o is not None and o != v['max']:
                    diff.append('%s %-11s new %s (%s)  current %s' % (fam, p, v['max'], ','.join(sorted(set(v['evidence']))), o))
        print('differences from %s: %d' % (a.compare, len(diff)))
        for x in diff:
            print('  ' + x)
    if a.output:
        json.dump(out, open(a.output, 'w'), indent=1)
    sys.exit(1 if unexplained else 0)


if __name__ == '__main__':
    main()
