"""Build atf15xx-arch/data/devices.json (the device tables atf15xx-arch compiles in) from the pin
experiments.

  python3 tools/fuzzers/fitter-pins/export_devices.py [-o atf15xx-arch/data/devices.json] [--compare atf15xx-arch/data/devices.json]

Inputs (written by this directory's experiments under .scratch/exp/): pinmap.json (package pin ->
macrocell), pinroles.json (JTAG / PD pins), dedic.json (dedicated-input functions), gclk3.json (the
I/O pin that drives GCLK3). atf15xx-arch's build script turns the file into its `Device` table.
"""
import argparse, json, os, re

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..'))
S = os.path.join(ROOT, '.scratch', 'exp')
PKG = {'C44': ('PLCC44', 'plcc44'), 'T44': ('TQFP44', 'tqfp44'), 'C68': ('PLCC68', 'plcc68'), 'C84': ('PLCC84', 'plcc84'),
       'Q100': ('PQFP100', 'qfp100'), 'T100': ('TQFP100', 'tqfp100'), 'Q160': ('PQFP160', 'qfp160')}
# Pin-role results seen once under parallel wine load (8 fitters sharing one directory, see
# CLAUDE.md) and not reproduced when re-run serially: not pin roles.
FLAKY = {('P1504Q100', 'jtag', 18), ('P1504T100', 'pd1', 97), ('P1508T100', 'jtag', 37)}


def devices():
    load = lambda n: json.load(open(os.path.join(S, n)))
    PM, PR, DD, G3 = load('pinmap.json'), load('pinroles.json'), load('dedic.json'), load('gclk3.json')
    out = {}
    for dev, v in PM.items():
        fam = dev[1:5]
        pins = {int(k[:-2]): x['mc'] for k, x in v.items() if k.endswith('_o') and isinstance(x, dict) and x['fits']}
        roles = {r: [p for p in ps if (dev, r, p) not in FLAKY] for r, ps in PR.get(dev, {}).items()}
        ded, input_only = {}, {}
        for p, r in DD[dev].items():
            p = int(p)
            if r['ar'] and r['ar'].get('gclr') == '0':
                ded['gclr'] = p
            elif r['oe'] and r['oe'].get('goe', [0, ''])[1] == 'OE1':
                ded['oe1'] = p
            elif r['oe'] and r['oe'].get('goe', [0, ''])[1] == 'OE2':
                ded['gclk2_oe2'] = p
            elif r['clk'] and r['clk'].get('gclk') == '11':
                ded['gclk1'] = p
            else:
                input_only[str(p)] = int(re.match(r'[A-H](\d+)p', r['oe']['goe'][1]).group(1))
        assert set(ded) == {'gclr', 'oe1', 'gclk2_oe2', 'gclk1'}, (dev, ded)
        jtag = roles['jtag']
        assert len(jtag) == 4, (dev, jtag)
        pkg = dev[5:]
        out[dev] = dict(family=fam, package=PKG[pkg][0], tt2_device='f%s%s' % (fam, PKG[pkg][1]),
                        pins={str(p): m for p, m in sorted(pins.items())}, input_only=input_only,
                        jtag=dict(zip(['tdi', 'tms', 'tck', 'tdo'], jtag)), pd1=roles['pd1'][0], pd2=roles['pd2'][0],
                        gclk3=G3[dev][0], **ded)
    return out


if __name__ == '__main__':
    ap = argparse.ArgumentParser()
    ap.add_argument('-o', '--output')
    ap.add_argument('--compare')
    a = ap.parse_args()
    d = devices()
    if a.compare:
        cur = json.load(open(a.compare))
        diff = sorted(k for k in set(d) | set(cur) if d.get(k) != cur.get(k))
        print('%d devices; differences from %s: %s' % (len(d), a.compare, diff or 'none'))
    if a.output:
        with open(a.output, 'w') as f:
            json.dump(d, f, indent=1)
            f.write('\n')
