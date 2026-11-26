"""Check the vendor fitters' SDF back-annotation against the timing tables (oracle).

For every design directory `<corpus>/<name>/<grade>/*.sdo` (one fit per speed grade 7/10/15/20/25,
written with `-strategy Verilog_sim sdf -strategy tPD = <grade>`), every SDF delay of every cell
instance is followed across the five grades. The resulting 5-vector must equal the composition the
SDF writer uses (docs/architecture/fitter/08-timing.md §F8.4), evaluated on the table
(atf15xx-arch/data/timing.json, from derive_timing.py). Prints, per (cell type, path), the compositions that explain all observations,
and every observation no composition explains.

  usage: check_sdf_timing.py timing.json CORPUS_DIR
"""
import collections, glob, json, os, re, sys

GRADES = ['7', '10', '15', '20', '25']

# The compositions of the datasheet timing model (F8.2) that each SDF cell path takes, as observed
# in the corpus (§F8.4); a path may use any of the listed ones depending on the netlist role of the cell.
EXPECTED = {
    # inside a foldback expander the AND and BUF cells are 0 and the closing INV carries tSEXP
    ('BUF', 'IOPATH A Q'): ['tIN+tUIM', 'tIN+tGLOB', 'tCOMB', '0'],
    ('INV', 'IOPATH A QN'): ['tIN+tUIM', 'tIN+tGLOB', 'tCOMB', 'tSEXP', '0'],
    ('AND', 'IOPATH'): ['tLAD', '0'],
    ('OR', 'IOPATH'): ['0', 'tPEXP'],
    ('XOR2', 'IOPATH'): ['0'],
    ('TRI', 'IOPATH A Q'): ['tOD1'],
    ('TRI', 'IOPATH EN Q'): ['tXZ'],
    ('DFF', 'IOPATH (posedge CLK) Q'): ['tRD'],
    ('DFF', 'IOPATH (posedge AR) Q'): ['tCLR'],
    ('DFF', 'IOPATH (posedge AS) Q'): ['tPRE'],
    ('DFF', 'IOPATH CE Q'): ['tEN'],
    ('DFF', 'SETUP D (posedge CLK)'): ['tSU'],
    ('DFF', 'HOLD D (posedge CLK)'): ['tH'],
    ('DFF', 'IOPATH (posedge EN) Q'): ['tRD'],          # latches
    ('DFF', 'SETUP D (posedge EN)'): ['tSU'],
    ('DFF', 'HOLD D (posedge EN)'): ['tH'],
}


def cls(celltype):
    for p in ('AND', 'OR', 'DFF', 'TFF', 'LATCH'):
        if celltype.startswith(p):
            return 'DFF' if p in ('TFF', 'LATCH') else p   # storage cells share one model
    return celltype


def parse(path):
    """{(instance, celltype, pathkey): value} with pathkey normalised (AND/OR inputs merged)."""
    s = open(path).read()
    out = {}
    for c in re.finditer(r'\(CELLTYPE "(\w+)"\)\s*\(INSTANCE (\S+)\)(.*?)(?=\(CELL\b|\Z)', s, re.S):
        ct, inst, body = c.groups()
        for m in re.finditer(r'\((IOPATH|SETUP|HOLD)\s+(\(\w+ \w+\)|\w+)\s+(\(\w+ \w+\)|\w+)\s*\((\d+):', body):
            kind, a, b, v = m.groups()
            k = cls(ct)
            if kind == 'IOPATH' and k in ('AND', 'OR', 'XOR2'):
                key = 'IOPATH'
            else:
                key = '%s %s %s' % (kind, 'D' if a == 'T' else a, b)
            out[(inst, ct, key, a)] = int(v)
    return out


def evaluate(expr, table):
    if expr == '0':
        return [0] * 5
    tot = [0] * 5
    for t in expr.split('+'):
        tot = [x + y for x, y in zip(tot, table[t]['min'])]
    return tot


def main():
    tj, corpus = sys.argv[1], sys.argv[2]
    T = json.load(open(tj))['families']
    seen = collections.defaultdict(collections.Counter)
    bad = []
    designs = sorted(d for d in glob.glob(os.path.join(corpus, '*')) if os.path.isdir(d))
    for d in designs:
        per = {}
        for g in GRADES:
            f = glob.glob(os.path.join(d, g, '*.sdo'))
            if f:
                per[g] = parse(f[0])
        if len(per) != 5:
            print('skip (missing grades):', d)
            continue
        tt2 = glob.glob(os.path.join(d, '7', '*.tt2'))[0]
        dev = open(tt2).read().split('DEVICE', 1)[1].split()[0]
        fam = re.search(r'150[248]', dev).group(0)
        table = T[fam]['params']
        keys = set(per['7'])
        for g in GRADES:
            if set(per[g]) != keys:
                bad.append((d, 'netlist differs between grades 7 and %s' % g))
        for k in sorted(keys):
            inst, ct, key, _ = k
            vec = [per[g].get(k) for g in GRADES]
            exp = EXPECTED.get((cls(ct), key))
            if exp is None:
                bad.append((d, '%s %s %s: no expectation, values %s' % (inst, ct, key, vec)))
                continue
            hit = [e for e in exp if evaluate(e, table) == vec]
            if hit:
                seen[(cls(ct), key)]['|'.join(hit)] += 1
            else:
                bad.append((d, '%s %s %s: %s matches none of %s' % (inst, ct, key, vec, exp)))
    print('%d designs x 5 grades; %d (instance, path) delays followed across the grades'
          % (len(designs), sum(sum(c.values()) for c in seen.values()) + len(bad)))
    for k, c in sorted(seen.items()):
        print('  %-5s %-26s %s' % (k[0], k[1], ', '.join('%s: %d' % kv for kv in c.most_common())))
    print('unexplained: %d' % len(bad))
    for d, m in bad[:40]:
        print('  ', os.path.basename(d), m)
    sys.exit(1 if bad else 0)


if __name__ == '__main__':
    main()
