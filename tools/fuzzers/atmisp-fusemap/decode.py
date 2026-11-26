"""Decode the fuse-map fuzzer's SVFs into a cell -> fuse map, per device.

  python3 tools/fuzzers/atmisp-fusemap/decode.py OUT_DIR MAP.json [DEVICE ...]

OUT_DIR is the oracle output of gen_cases.py's case list (one fm_<DEVICE>_<pattern>/out.svf per
JEDEC pattern). Every SDR scan is identified by its position in the SVF (the
scan sequence must be the same for every pattern, which decode.py checks) and labelled with the
instruction before it and the row address (the last short scan under another instruction). Each bit of each scan field is then classified from its values
across the patterns: constant, a copy of fuse i (0 in `zero`, 1 in `one`, i in binary across the
`bitK` patterns), or an inverted copy.

MAP.json, per device: qf; cells [scan key, bit, fuse, inverted] (bit 0 = first shifted); per
instruction, the scan lengths and the fuses its TDI scans carry (rows vs. config / special / UES
registers); the fuses that appear in no TDI cell, and in more TDI cells than most fuses; counts. decode.py fails if any cell is
inconsistent (neither constant nor a clean copy) or if a scan is missing from some pattern.
"""
import collections, json, os, re, sys


def scans(path):
    """[(key, field, nbits, value, label)] for every SDR field (TDI/TDO) of the SVF.

    key = 'NNNNN:SIR': the scan's position in the file and the instruction before it; label = the
    TDI of the last short (<= 16 bit) scan under another instruction (the row address, for rows)."""
    text = re.sub(r'//[^\n]*', '', open(path, encoding='latin-1').read())
    sir, n_sdr, addr = None, 0, {}
    out = []
    for stmt in text.split(';'):
        s = ' '.join(stmt.split())
        if s.startswith('SIR '):
            sir = re.search(r'TDI \(([0-9a-fA-F]+)\)', s).group(1).lower()
        elif s.startswith('SDR '):
            n = int(s.split()[1])
            fields = dict(re.findall(r'(TDI|TDO|MASK) \(([0-9a-fA-F]+)\)', s))
            others = [(q, o, a) for o, (q, a) in addr.items() if o != sir]
            ctx = '%s=%s' % max(others)[1:] if others else '-'
            key = '%05d:%s' % (n_sdr, sir)
            for f in ('TDI', 'TDO'):
                if f in fields:
                    out.append((key, f, n, int(fields[f], 16), ctx))
            if n <= 16 and 'TDI' in fields:
                addr[sir] = (n_sdr, fields['TDI'].lower())
            n_sdr += 1
    return out


def decode(outdir, dev):
    pats = {}
    for d in sorted(os.listdir(outdir)):
        if d.startswith('fm_%s_' % dev):
            tag = d[len('fm_%s_' % dev):]
            svf = os.path.join(outdir, d, 'out.svf')
            if not os.path.exists(svf):
                raise SystemExit('%s: no SVF (rerun that case)' % d)
            sc = scans(svf)
            pats[tag] = {(k, f): (n, v) for k, f, n, v, _ in sc}
            if tag == 'zero':
                labels = {k: lab for k, f, n, v, lab in sc}
            qf = json.load(open(os.path.join(outdir, d, 'case.json')))['qf']
    nbits = (qf - 1).bit_length()
    need = ['zero', 'one'] + ['bit%02d' % k for k in range(nbits)]
    missing = [t for t in need if t not in pats]
    if missing:
        raise SystemExit('%s: patterns missing: %s' % (dev, missing))
    keys = set(pats['zero'])
    for t in need:
        if set(pats[t]) != keys or any(pats[t][k][0] != pats['zero'][k][0] for k in keys):
            raise SystemExit('%s: scan structure of %s differs from zero (%d vs %d scans)'
                             % (dev, t, len(pats[t]), len(keys)))
    cells, const, bad = [], 0, []
    for key in sorted(keys):
        n = pats['zero'][key][0]
        vals = {t: pats[t][key][1] for t in need}
        for b in range(n):
            bit = {t: (v >> b) & 1 for t, v in vals.items()}
            if bit['zero'] == bit['one']:
                if any(bit[t] != bit['zero'] for t in need[2:]):
                    bad.append((key, b))
                const += 1
                continue
            inv = bit['zero'] == 1
            idx = sum((bit['bit%02d' % k] ^ inv) << k for k in range(nbits))
            if idx >= qf:
                bad.append((key, b))
                continue
            cells.append(['%s@%s|%s' % (key[0], labels[key[0]], key[1]), b, idx, inv])
    if bad:
        raise SystemExit('%s: %d inconsistent cells, e.g. %s' % (dev, len(bad), bad[:5]))
    tdi = collections.Counter(c[2] for c in cells if c[0].endswith('|TDI'))
    by_sir = collections.defaultdict(set)
    for c in cells:
        if c[0].endswith('|TDI'):
            by_sir[c[0].split(':')[1].split('@')[0]].add(c[2])
    shapes = collections.defaultdict(collections.Counter)
    for (k, f), (n, _) in pats['zero'].items():
        if f == 'TDI':
            shapes[k.split(':')[1]][n] += 1
    return dict(qf=qf, cells=cells, constant_cells=const,
                fuses_by_instruction={s: ranges(sorted(v)) for s, v in sorted(by_sir.items())},
                scans_by_instruction={s: dict(c) for s, c in sorted(shapes.items())},
                fuses_in_no_tdi_cell=ranges(sorted(set(range(qf)) - set(tdi))),
                tdi_cells_per_fuse=dict(sorted(collections.Counter(tdi.values()).items())),
                fuses_in_more_tdi_cells_than_usual=ranges(sorted(
                    i for i, k in tdi.items() if k > collections.Counter(tdi.values()).most_common(1)[0][0])),
                inverted_cells=sum(c[3] for c in cells),
                tdi_cells=sum(tdi.values()), tdo_cells=sum(1 for c in cells if c[0].endswith('|TDO')))


def ranges(xs):
    out = []
    for x in xs:
        if out and out[-1][1] == x - 1:
            out[-1][1] = x
        else:
            out.append([x, x])
    return out


if __name__ == '__main__':
    outdir, dst = sys.argv[1], sys.argv[2]
    devs = sys.argv[3:] or sorted({d.split('_')[1] for d in os.listdir(outdir) if d.startswith('fm_')})
    res = json.load(open(dst)) if os.path.exists(dst) else {}
    for dev in devs:
        r = decode(outdir, dev)
        res[dev] = r
        print('%-10s QF %5d: %6d TDI cells, %6d TDO cells, %d inverted, %d constant; TDI cells per fuse %s;'
              ' fuses in no cell %s; in more cells than usual %s'
              % (dev, r['qf'], r['tdi_cells'], r['tdo_cells'], r['inverted_cells'], r['constant_cells'],
                 r['tdi_cells_per_fuse'], r['fuses_in_no_tdi_cell'], r['fuses_in_more_tdi_cells_than_usual']))
        for s, rg in r['fuses_by_instruction'].items():
            print('    SIR %s: TDI scans %s; fuses %s' % (s, r['scans_by_instruction'][s],
                  rg if len(rg) <= 4 else '%d ranges, %d..%d' % (len(rg), rg[0][0], rg[-1][1])))
    json.dump(res, open(dst, 'w'))
