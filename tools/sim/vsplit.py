#!/usr/bin/env python3
"""Turn a Verilog design into "gold" Verilog with the split pin interface (pN_in / pN_oe / pN_out).

  python3 tools/sim/vsplit.py --top T [--fit design.fit] [--pin PORT=N ...] -o gold.v a.v [b.v ...]

Yosys reads and elaborates the design (`proc; flatten; tribuf -logic; opt; memory`), so the
gold model is the *Verilog semantics* of the source, not anything a fitter did. Then every bit
of every top-level port is bound to a device pin:
  input  bit            -> pN_in drives it;
  output bit            -> pN_oe = 1, pN_out = the bit;
  tri-state output/inout bit (a $tribuf with enable E and data D drives it)
                        -> pN_oe = E, pN_out = E & D, and every reader of the bit inside the
                           design sees the pad: E ? D : pN_in.
Registers without an initial value get 0 (`setundef -zero -init`, the device's power-up
value) unless --init free, where they stay undefined (X in simulation, free in formal).

Pin names: a bit `name[i]` of a vector port is called `namei` (as tools/sim/yosys2tt2.py
and the vendor .fit report name it). Pins come from the .fit placement lines
(`NAME is placed at pin N`, `NAME assigned to pin N`) and/or --pin PORT=N (PORT = name,
name[i] or namei). Unmapped output bits are an error; unmapped unused inputs are dropped.
"""
import argparse, json, os, re, subprocess, sys


def sanitize(n):
    n = n.lstrip('\\')
    n = re.sub(r'\[(\d+)\]', r'\1', n)
    n = re.sub(r'[^A-Za-z0-9_]', '_', n)
    if not re.match(r'[A-Za-z_]', n):
        n = 'N' + n
    return n


def fit_place(text):
    place = {}
    for m in re.finditer(r'^(\S+) (?:is placed at pin|assigned to pin)\s+(\d+)', text.replace('\r', ''), re.M):
        place[m.group(1)] = int(m.group(2))
    return place


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0], epilog=__doc__.split('\n', 2)[2],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('sources', nargs='+')
    ap.add_argument('--top', required=True)
    ap.add_argument('--fit', help='fitter report with pin placement')
    ap.add_argument('--pin', action='append', default=[], help='PORT=N (repeatable)')
    ap.add_argument('--init', choices=['zero', 'free'], default='zero')
    ap.add_argument('--module', default='gold')
    ap.add_argument('-o', '--output', required=True)
    ap.add_argument('--work', default='.', help='directory for intermediate files')
    a = ap.parse_args()
    os.makedirs(a.work, exist_ok=True)
    place = fit_place(open(a.fit, errors='replace').read()) if a.fit else {}
    for p in a.pin:
        k, _, v = p.partition('=')
        place[sanitize(k)] = int(v)
    pre = os.path.join(a.work, 'vsplit_pre.json')
    ys = ['read_verilog -sv %s' % ' '.join('"%s"' % s for s in a.sources),
          'hierarchy -check -top %s' % a.top, 'proc', 'flatten', 'tribuf -logic', 'opt -keepdc', 'memory', 'opt_clean']
    if a.init == 'zero':
        ys.append('setundef -zero -init')
    else:
        ys.append('setundef -zero')      # x constants are don't-cares; initial values stay undefined
    ys.append('write_json %s' % pre)
    r = subprocess.run(['yosys', '-q', '-l', os.path.join(a.work, 'vsplit.log'), '-p', '; '.join(ys)],
                       capture_output=True, text=True)
    if r.returncode:
        sys.exit('vsplit: yosys failed:\n' + r.stdout[-2000:] + r.stderr[-2000:])
    j = json.load(open(pre))
    mod = j['modules'][a.top]
    nxt = [max([b for nn in mod['netnames'].values() for b in nn['bits'] if isinstance(b, int)] + [1]) + 1]

    def new_bit():
        nxt[0] += 1
        return nxt[0]

    tri = {}                      # bit -> (cell name, enable bit, data bit)
    for cn, c in mod['cells'].items():
        if c['type'] == '$tribuf':
            en = c['connections']['EN'][0]
            for d, y in zip(c['connections']['A'], c['connections']['Y']):
                tri[y] = (cn, en, d)
    new_ports, missing = {}, []
    cells = mod['cells']
    drop_tri = set()
    used = {}
    for pn, p in mod['ports'].items():
        # Verilog bit index: Yosys lists bits LSB first; `offset`/`upto` give the declared range
        # (e.g. `input [15:10] a` -> offset 10), so bit k of the list is a[offset + k]
        nn = mod['netnames'].get(pn, {})
        off, upto, w = nn.get('offset', 0), nn.get('upto', 0), len(p['bits'])
        for i, b in enumerate(p['bits']):
            idx = off + (w - 1 - i if upto else i)
            nm = sanitize(pn if len(p['bits']) == 1 else '%s[%d]' % (pn, idx))
            pin = place.get(nm)
            if pin is None:
                if p['direction'] != 'input':
                    missing.append(nm)
                continue
            if pin in used:
                sys.exit('vsplit: %s and %s are both on pin %d' % (used[pin], nm, pin))
            used[pin] = nm
            if p['direction'] == 'input':
                new_ports['p%d_in' % pin] = {'direction': 'input', 'bits': [b]}
                continue
            pin_in = new_bit()
            new_ports['p%d_in' % pin] = {'direction': 'input', 'bits': [pin_in]}
            if b in tri:
                cn, en, d = tri[b]
                drop_tri.add(cn)
                o = new_bit()
                cells['vsplit_out_%d' % pin] = {
                    'hide_name': 0, 'type': '$and', 'parameters': {'A_SIGNED': 0, 'B_SIGNED': 0, 'A_WIDTH': 1, 'B_WIDTH': 1, 'Y_WIDTH': 1},
                    'attributes': {}, 'port_directions': {'A': 'input', 'B': 'input', 'Y': 'output'},
                    'connections': {'A': [en], 'B': [d], 'Y': [o]}}
                cells['vsplit_pad_%d' % pin] = {
                    'hide_name': 0, 'type': '$mux', 'parameters': {'WIDTH': 1}, 'attributes': {},
                    'port_directions': {'A': 'input', 'B': 'input', 'S': 'input', 'Y': 'output'},
                    'connections': {'A': [pin_in], 'B': [d], 'S': [en], 'Y': [b]}}
                new_ports['p%d_oe' % pin] = {'direction': 'output', 'bits': [en]}
                new_ports['p%d_out' % pin] = {'direction': 'output', 'bits': [o]}
            else:
                new_ports['p%d_oe' % pin] = {'direction': 'output', 'bits': ['1']}
                new_ports['p%d_out' % pin] = {'direction': 'output', 'bits': [b]}
    if missing:
        sys.exit('vsplit: no pin for output(s) %s (give --fit or --pin)' % ', '.join(missing))
    for cn in drop_tri:
        c = cells[cn]
        left = [y for y in c['connections']['Y'] if y not in {b for b in tri if tri[b][0] == cn and
                                                             any(b in np['bits'] for np in new_ports.values())}]
        del cells[cn]
        if left:
            pass  # bits of a tri-state bus that are not on a pin: undriven (Z) inside the design
    mod['ports'] = dict(sorted(new_ports.items(), key=lambda kv: (int(re.match(r'p(\d+)', kv[0]).group(1)), kv[0])))
    for nn in mod['netnames'].values():
        nn['hide_name'] = nn.get('hide_name', 0)
    for pn, p in mod['ports'].items():
        mod['netnames'][pn] = {'hide_name': 0, 'bits': p['bits'], 'attributes': {}}
    out = {'creator': 'vsplit', 'modules': {a.module: mod}}
    post = os.path.join(a.work, 'vsplit_post.json')
    json.dump(out, open(post, 'w'))
    r = subprocess.run(['yosys', '-q', '-p', 'read_json %s; opt_clean; write_verilog -noattr %s' % (post, a.output)],
                       capture_output=True, text=True)
    if r.returncode:
        sys.exit('vsplit: yosys write failed:\n' + r.stdout[-2000:] + r.stderr[-2000:])
    print('vsplit: %s -> %s (%d pins)' % (a.top, a.output, len(used)))


if __name__ == '__main__':
    main()
