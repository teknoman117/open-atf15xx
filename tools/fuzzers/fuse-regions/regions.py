"""Fuse-region tables of the AS families from ATMISP's own JEDEC output (black box).

ATMISP's `Load` instruction writes a JEDEC file whose NOTE comments name every fuse region
(product-term rows, macrocell configuration fields, UIM muxes, global OE, configuration, special,
UES, reserved). gen_cases.py makes one Load case per AS family; run it with the ATMISP oracle,
then:
  python3 tools/fuzzers/fuse-regions/regions.py .scratch/re/regions.json OUT_DIR [LABELS.json]
(OUT_DIR/rg_ATF150xAS/a.jed is the Load output). Only the NOTE text and the field addresses and
lengths are used; the JEDEC files stay in .scratch (ATMISP's text is not committed).

ATMISP's NOTE text numbers the product-term rows differently from the fitter (spec F2.2, "Note on
ATMISP's Load labels"). The (macrocell, PT) of every row is therefore taken from atf15xx-arch
(`Family::pt_addr`, the fitter's functional numbering, which equals prjbureau's `pterm_ranges`;
printed by `atf15xx-sim fuses`), and, if LABELS is given (the fitter's own NOTE labels,
tools/fuzzers/fitter-macrocell/vendor_labels.py), checked against every row the fitter labelled.

Output (regions.json): per family
  pt:      list of {mc, pt, addr, len}         product-term rows (mc 1-based global, pt 1..5)
  mcfg:    list of {block, bits:[Sa,Sb] or None, addr, len}   macrocell configuration fields
  uim:     list of {block, mux, addr, len}
  goe:     {addr, len, count}
  config, special, ues, reserved: {addr, len}
"""
import json, re, sys, os

OUT = sys.argv[2] if len(sys.argv) > 2 else '.'
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..')
LABELS = sys.argv[3] if len(sys.argv) > 3 else None
import subprocess  # noqa: E402
SIM = os.path.join(ROOT, 'target', 'release', 'atf15xx-sim')


def renumber_pterms(fam, out, labels):
    """(mc, pt) of every PT row from atf15xx-arch's PT addresses (`atf15xx-sim fuses`); check
    against the fitter's labels."""
    lines = subprocess.run([SIM, 'fuses', fam[:4]], capture_output=True, text=True, check=True).stdout.splitlines()
    where = {int(w[1]): (int(w[2]), int(w[3]), 96) for w in (l.split() for l in lines) if w and w[0] == 'pt'}
    for x in out['pt']:
        mc, pt, n = where[x['addr']]
        assert n == x['len'], (fam, x)
        x['mc'], x['pt'] = mc, pt
    checked = 0
    if labels:
        lab = {int(a): v[0][0] for a, v in labels[fam[:4]].items()}
        for x in out['pt']:
            l = lab.get(x['addr'], '')
            m = re.match(r'PT (\d) of MC (\d+)', l)
            if m:
                assert (int(m.group(2)), int(m.group(1))) == (x['mc'], x['pt']), (fam, x, l)
                checked += 1
    return checked


def parse(fam):
    t = open(os.path.join(OUT, 'rg_ATF' + fam, 'a.jed'), 'rb').read().decode('latin-1').replace('\r', '')
    # fuse digits inside L fields -> '#', so a field's length is its number of '#'
    t = re.sub(r'(L\d+)([\s01]*)\*', lambda m: m.group(1) + re.sub(r'[01]', '#', m.group(2)) + '*', t)
    # fields: L<addr> <#...>* [NOTE ...*]
    out = dict(pt=[], mcfg=[], uim=[], goe=None)
    section = None
    for m in re.finditer(r'(NOTE[^*]*)\*|L(\d+)([\s#]*)\**([ \t]*NOTE([^*]*)\*)?', t):
        if m.group(1):
            s = m.group(1)
            if 'macrocell configurations' in s: section = 'mcfg'
            elif 'UIM' in s: section = 'uim'
            elif 'Global OE' in s: section = 'goe'
            elif 'Device Configuration' in s: section = 'config'
            elif 'Special Purpose' in s: section = 'special'
            elif 'Electronic Signature' in s: section = 'ues'
            elif 'Reserved' in s: section = 'reserved'
            continue
        addr = int(m.group(2)); body = m.group(3); n = body.count('#'); note = (m.group(5) or '').strip()
        if section is None:
            pm = re.match(r'PT (\d+) MC (\d+)', note)
            out['pt'].append(dict(mc=int(pm.group(2)), pt=int(pm.group(1)), addr=addr, len=n))
        elif section == 'mcfg':
            sm = re.match(r'S\s*(\d+)\s*,\s*S\s*(\d+) of block (\w)', note)
            if sm:
                out['mcfg'].append(dict(block=sm.group(3), bits=[int(sm.group(1)), int(sm.group(2))], addr=addr, len=n))
            else:
                out['mcfg'].append(dict(block=None, bits=None, addr=addr, len=n))
        elif section == 'uim':
            um = re.match(r'Mux-(\d+) of block (\w)', note)
            out['uim'].append(dict(block=um.group(2), mux=int(um.group(1)), addr=addr, len=n))
        elif section == 'goe':
            lines = [l for l in body.split('\n') if '#' in l]
            out['goe'] = dict(addr=addr, len=n, count=len(lines), width=len(lines[0]))
        else:
            out[section] = dict(addr=addr, len=n)
    return out


if __name__ == '__main__':
    res = {}
    labels = json.load(open(LABELS)) if LABELS else None
    for fam in ('1502AS', '1504AS', '1508AS'):
        r = parse(fam)
        checked = renumber_pterms(fam, r, labels)
        res[fam] = r
        if labels:
            print(fam, 'PT numbering (atf15xx-arch) agrees with the fitter on all %d labelled rows' % checked, file=sys.stderr)
        tot = sum(x['len'] for x in r['pt']) + sum(x['len'] for x in r['mcfg']) + sum(x['len'] for x in r['uim']) \
            + r['goe']['len'] + sum(r[k]['len'] for k in ('config', 'special', 'ues', 'reserved'))
        print(fam, 'pt rows', len(r['pt']), 'ptlen', set(x['len'] for x in r['pt']), 'mcfg fields', len(r['mcfg']),
              'uim', len(r['uim']), 'uimw', set(x['len'] for x in r['uim']), 'goe', r['goe'], 'total', tot, file=sys.stderr)
    json.dump(res, open(sys.argv[1], 'w'), indent=0)
