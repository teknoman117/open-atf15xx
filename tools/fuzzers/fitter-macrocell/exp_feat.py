"""Experiment 5: macrocell / global feature encodings, all three families.

Each config is a tiny design with output Y pinned to a target pin. We record the target MC's 24 labelled
config bits (as raw field-relative addresses), the whole global/config region, and the GOE region.
"""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
import fitter_oracle as F
import tt2gen as T

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch')
REG = json.load(open(os.path.join(ROOT, 're', 'regions.json')))
PINMAP = json.load(open(os.path.join(ROOT, 'exp', 'pinmap.json')))
# device, tt2 device, GCLK1, OE1, GCLR, GCLK2/OE2, GCLK3
DEV = {'1502': ('P1502C44', 'f1502plcc44', 43, 44, 1, 2, 41),
       '1504': ('P1504T100', 'f1504tqfp100', 87, 88, 89, 90, 85),
       '1508': ('P1508T100', 'f1508tqfp100', 87, 88, 89, 90, 85)}

# name -> (pins(extra), nodes, inputs, outputs, terms, options); 'Y' is the pinned output.
# inputs: A B C (floating pins), K1 K2 K3 (GCLK pins), R (GCLR), E (OE1), E2 (OE2/GCLK2 as OE)
def cfg(name, d):
    g1, oe1, gclr, g2, g3 = d
    I = ['A', 'B', 'C']
    base = [('A', None), ('B', None), ('C', None)]
    def mk(extra_pins, ins, outs, terms, opts=(), props=()):
        return base + extra_pins, ins, outs, terms, list(opts), list(props)
    c = lambda ins, lits: T.cube(ins, lits)
    if name == 'comb':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})])
    if name == 'comb_n':
        return mk([], I, ['Y-'], [(c(I, {'A': '1', 'B': '1'}), {0})])
    if name == 'comb_2pt':
        return mk([], I, ['Y'], [(c(I, {'A': '1'}), {0}), (c(I, {'B': '1'}), {0})])
    if name == 'comb_5pt':
        ins = I + ['D', 'E5']
        return base + [('D', None), ('E5', None)], ins, ['Y'], [(c(ins, {x: '1'}), {0}) for x in ins], [], []
    if name == 'comb_8pt':
        ins = I + ['D', 'E5', 'F', 'G', 'H']
        return base + [(x, None) for x in ins[3:]], ins, ['Y'], [(c(ins, {x: '1'}), {0}) for x in ins], [], []
    if name == 'vcc':
        return mk([], I, ['Y'], [(c(I, {}), {0})])
    if name == 'gnd':
        return mk([], I, ['Y'], [])
    if name in ('dff_g1', 'dff_g2', 'dff_g3'):
        k = int(name[5]); pin = {1: g1, 2: g2, 3: g3}[k]
        ins = I + ['K']
        return base + [('K', pin)], ins, ['Y.D', 'Y.C'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'K': '1'}), {1})], [], []
    if name == 'dff_g1n':   # inverted global clock
        ins = I + ['K']
        return base + [('K', g1)], ins, ['Y.D', 'Y.C'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'K': '0'}), {1})], [], []
    if name == 'dff_ptclk':
        return mk([], I, ['Y.D', 'Y.C'], [(c(I, {'A': '1'}), {0}), (c(I, {'B': '1', 'C': '1'}), {1})])
    if name == 'dff_ce':
        ins = I + ['K']
        return base + [('K', g1)], ins, ['Y.D', 'Y.C', 'Y.CE'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'K': '1'}), {1}), (c(ins, {'B': '1'}), {2})], [], []
    if name == 'dff_gclr':
        ins = I + ['K', 'R']
        return base + [('K', g1), ('R', gclr)], ins, ['Y.D', 'Y.C', 'Y.AR'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'K': '1'}), {1}), (c(ins, {'R': '1'}), {2})], [], []
    if name == 'dff_gclrn':
        ins = I + ['K', 'R']
        return base + [('K', g1), ('R', gclr)], ins, ['Y.D', 'Y.C', 'Y.AR'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'K': '1'}), {1}), (c(ins, {'R': '0'}), {2})], [], []
    if name == 'dff_ptar':
        ins = I + ['K']
        return base + [('K', g1)], ins, ['Y.D', 'Y.C', 'Y.AR'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'K': '1'}), {1}), (c(ins, {'B': '1'}), {2})], [], []
    if name == 'dff_ptap':
        ins = I + ['K']
        return base + [('K', g1)], ins, ['Y.D', 'Y.C', 'Y.AP'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'K': '1'}), {1}), (c(ins, {'B': '1'}), {2})], [], []
    if name == 'dff_ptoe_ap':
        ins = I + ['K']
        return base + [('K', g1)], ins, ['Y.D', 'Y.C', 'Y.AP', 'Y.OE'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'K': '1'}), {1}), (c(ins, {'B': '1'}), {2}), (c(ins, {'C': '1'}), {3})], [], []
    if name == 'tff':
        ins = I + ['K']
        return base + [('K', g1)], ins, ['Y.T', 'Y.C'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'K': '1'}), {1})], [], []
    if name == 'dff_n':
        ins = I + ['K']
        return base + [('K', g1)], ins, ['Y.D-', 'Y.C'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'K': '1'}), {1})], [], []
    if name == 'latch':
        return mk([], I, ['Y.L', 'Y.LE'], [(c(I, {'A': '1'}), {0}), (c(I, {'B': '1'}), {1})])
    if name == 'latch_g':
        ins = I + ['K']
        return base + [('K', g1)], ins, ['Y.L', 'Y.LE'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'K': '1'}), {1})], [], []
    if name == 'oe_pt':
        return mk([], I, ['Y', 'Y.OE'], [(c(I, {'A': '1'}), {0}), (c(I, {'B': '1', 'C': '1'}), {1})])
    if name.startswith('oe_g'):     # global OE from OE1 / OE2 pins, either polarity
        pin = oe1 if name[4] == '1' else g2
        pol = '0' if name.endswith('n') else '1'
        ins = I + ['E']
        return base + [('E', pin)], ins, ['Y', 'Y.OE'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'E': pol}), {1})], [], []
    if name == 'oe_io':     # OE from an ordinary I/O pin -> global OE via that pin's MC
        ins = I + ['E']
        return base + [('E', None)], ins, ['Y', 'Y.OE'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'E': '1'}), {1})], [], []
    if name == 'opendrain':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'open_collector', '=', 'Y'])
    if name == 'slow':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'output_fast', 'off'])
    if name == 'fast':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'output_fast', 'on'])
    if name == 'keep':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'pin_keep', 'on'])
    if name == 'secure':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'security', 'on'])
    if name == 'nojtag':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'JTAG', 'off'])
    if name == 'pd1':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'pd1', 'on'])
    if name == 'pd2':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'pd2', 'on'])
    if name == 'pwrreset':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'power_reset', 'on'])
    if name == 'tdipu':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'TDI_pullup', 'on'])
    if name == 'tmspu':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'TMS_pullup', 'on'])
    if name.startswith('itd'):
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'GCK_ITD', name[3:]])
    if name == 'mcpower':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '1'}), {0})], ['-strategy', 'MC_power', 'on'])
    if name == 'pinfb':     # output whose pin is read back (.PIN) by a second output Z
        return mk([('Z', None)], I + ['Y.PIN'], ['Y', 'Z'], [(c(I + ['Y.PIN'], {'A': '1'}), {0}), (c(I + ['Y.PIN'], {'Y.PIN': '1'}), {1})])
    if name == 'regfb':
        ins = I + ['K', 'Y.Q']
        return base + [('K', g1), ('Z', None)], ins, ['Y.D', 'Y.C', 'Z'], [(c(ins, {'A': '1'}), {0}), (c(ins, {'K': '1'}), {1}), (c(ins, {'Y.Q': '1'}), {2})], [], []
    if name == 'combfb':
        ins = I + ['Y']
        return base + [('Z', None)], ins, ['Y', 'Z'], [(c(ins, {'A': '1', 'B': '1'}), {0}), (c(ins, {'Y': '1'}), {1})], [], []
    if name == 'xor':
        return mk([], I, ['Y'], [(c(I, {'A': '1', 'B': '0'}), {0}), (c(I, {'A': '0', 'B': '1'}), {0})], ['-strategy', 'XOR_Synthesis', 'on'])
    if name == 'inreg':     # registered input: Y.D = pin Y? use fast input register: Y.D = Y.PIN? (pin as input, reg buried)
        ins = ['K', 'Y']
        return [('K', g1), ('Q', None)], ins, ['Y.D', 'Y.C'], [], [], []
    raise KeyError(name)


CONFIGS = ['comb', 'comb_n', 'comb_2pt', 'comb_5pt', 'comb_8pt', 'vcc', 'gnd', 'dff_g1', 'dff_g2', 'dff_g3', 'dff_g1n',
           'dff_ptclk', 'dff_ce', 'dff_gclr', 'dff_gclrn', 'dff_ptar', 'dff_ptap', 'dff_ptoe_ap', 'tff', 'dff_n', 'latch',
           'latch_g', 'oe_pt', 'oe_g1', 'oe_g1n', 'oe_g2', 'oe_g2n', 'oe_io', 'opendrain', 'slow', 'mcpower', 'pinfb',
           'regfb', 'combfb', 'xor']


def run(fam, cname, ypin, extra_opts=()):
    pdev, dev = DEV[fam][:2]
    pins, ins, outs, terms, opts, props = cfg(cname, DEV[fam][2:])
    name = 'ft%s_%s_%d' % (fam, cname, ypin)
    d = F.fresh_dir(os.path.join(ROOT, 'oracle', 'feat'), name)
    p = os.path.join(d, name + '.tt2')
    T.write_tt2(p, name, dev, pins + [('Y', ypin)], [], ins, outs, terms, props)
    r = F.run_fitter(p, fam, pdev, ['-preassign', 'keep', '-strategy', 'JTAG', 'on'] + opts + list(extra_opts))
    if not r['fits']:
        return dict(fits=False, stdout=r['stdout'][-400:])
    fit = F.parse_fit(r['fit'])
    y = [x for x in fit['place'] if x['name'] == 'Y' and x['kind'] == 'pin']
    fz = F.read_jed(r['jed'])
    R = REG[fam + 'AS']
    a = min(x['addr'] for x in R['mcfg'])
    ymc = y[0]['mc'] if y else None
    rows = {(x['mc'], x['pt']): x['addr'] for x in R['pt']}
    pts = {mc: [fz[rows[(mc, k)]:rows[(mc, k)] + 96] for k in range(1, 6)] for mc in {x['mc'] for x in fit['place']}}
    return dict(fits=True, ymc=ymc, cfg=fz[a:], cfg_base=a, pts=pts, place=fit['place'], globals=fit['globals'],
                uim=fit['uim'], tt3=r.get('tt3', ''))


def target_pins(fam):
    pdev = DEV[fam][0]
    v = PINMAP[pdev]
    pins = {int(k[:-2]): x['mc'] for k, x in v.items() if k.endswith('_o') and isinstance(x, dict) and x['fits']}
    ded = set(DEV[fam][2:])
    return {p: m for p, m in pins.items() if p not in ded}


if __name__ == '__main__':
    fam = sys.argv[1]
    sel = sys.argv[2].split(',') if len(sys.argv) > 2 else CONFIGS
    tp = target_pins(fam)
    # all MCs of blocks A and B (if pinned) plus every 5th MC elsewhere
    pins = sorted(p for p, m in tp.items() if m <= 32 or m % 5 == 0)
    jobs = [((c, p), run, (fam, c, p)) for c in sel for p in pins]
    res = F.run_batch(jobs, workers=8)
    out = {'%s|%d' % k: v for k, v in res.items() if isinstance(v, dict)}
    path = os.path.join(ROOT, 'exp', 'feat_%s.json' % fam)
    old = json.load(open(path)) if os.path.exists(path) else {}
    old.update(out)
    json.dump(old, open(path, 'w'))
    from collections import Counter
    print(fam, Counter((k.split('|')[0], v['fits']) for k, v in out.items()))
