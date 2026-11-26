#!/usr/bin/env python3
"""Prototype: convert a Yosys JSON netlist (after `abc -sop` + `dfflegalize`) into a CUPL-style
Berkeley-PLA `.tt2` file that the Atmel fitters (fit1502/fit1504/fit1508, `-ifmt` TT) accept.

Supported Yosys cells (exactly what the recommended script in tools/sim/synth_atf15xx.ys
leaves behind):
  $sop                      sum of products (combinational logic)
  $_DFF_P_                  D flip-flop                        -> X.REG, X.C
  $_DFFE_PP_                D flip-flop with clock enable      -> + X.CE
  $_DFF_PP0_ / $_DFF_PP1_   with async reset / set             -> + X.AR / X.AP
  $_DFFE_PP0P_ / _PP1P_     clock enable + async reset / set
  $_DLATCH_P_               transparent-high latch             -> X.L, X.LE
  $_TBUF_                   tri-state output                   -> X.OE
  $_NOT_                    inverter (rare after abc -sop)

Mapping rules:
  * a top-level output/inout port becomes a pin; a storage cell whose Q drives a port (directly
    or through a $_TBUF_) is placed in that pin's macrocell (its feedback is `PORT.Q`);
  * every other storage cell and every $sop whose output is used in more than one place, or that
    feeds a storage control input, becomes a buried node;
  * single-literal $sop cells are treated as aliases (no logic);
  * pin numbers come from `--pin NAME=NUMBER` options (or a `(* LOC="12" *)` port attribute).

usage: yosys2tt2.py design.json -o design.tt2 --device f1504plcc44 [--pin clk=43 ...]
"""
import argparse, json, re, sys


def sanitize(n):
    n = n.lstrip('\\')
    n = re.sub(r'\[(\d+)\]', r'\1', n)
    n = re.sub(r'[^A-Za-z0-9_]', '_', n)
    if not re.match(r'[A-Za-z_]', n):
        n = 'N' + n
    return n


class Netlist:
    def __init__(self, mod, top):
        self.mod = mod
        self.names = {}                                   # bit -> preferred name
        for name, nn in mod['netnames'].items():
            hidden = nn.get('hide_name', 0)
            for i, b in enumerate(nn['bits']):
                if isinstance(b, str):
                    continue
                nm = name if len(nn['bits']) == 1 else f'{name}[{i}]'
                if b not in self.names or (not hidden and self.names[b][1]):
                    self.names[b] = (nm, hidden)
        self.ports = mod['ports']
        self.drivers = {}                                 # bit -> (cellname, cell)
        self.users = {}                                   # bit -> [(cellname, port)]
        for cn, c in mod['cells'].items():
            for p, bits in c['connections'].items():
                d = c['port_directions'][p]
                for b in bits:
                    if isinstance(b, str):
                        continue
                    if d == 'output':
                        self.drivers[b] = (cn, c)
                    else:
                        self.users.setdefault(b, []).append((cn, p))

    def name(self, b):
        return sanitize(self.names.get(b, (f'n{b}', 1))[0])


def sop_terms(c):
    """Return list of terms; each term = list of (input_index, positive?)."""
    par = lambda k: (lambda v: v if isinstance(v, int) else int(v, 2))(c['parameters'][k])
    w, dpt, table = par('WIDTH'), par('DEPTH'), par('TABLE')
    terms = []
    for t in range(dpt):
        lits = []
        for j in range(w):
            pat = (table >> (2 * w * t + 2 * j)) & 3
            if pat == 1:
                lits.append((j, False))
            elif pat == 2:
                lits.append((j, True))
            elif pat == 3:
                lits = None                               # contradictory literal -> term never true
                break
        if lits is not None:
            terms.append(lits)
    return terms


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('json')
    ap.add_argument('-o', '--output', required=True)
    ap.add_argument('--device', required=True, help='CUPL device name, e.g. f1504plcc44')
    ap.add_argument('--top')
    ap.add_argument('--pin', action='append', default=[], help='NAME=PIN')
    a = ap.parse_args()

    j = json.load(open(a.json))
    cands = [n for n, m in j['modules'].items() if not m.get('attributes', {}).get('blackbox')]
    tops = [n for n in cands if j['modules'][n].get('attributes', {}).get('top')]
    top = a.top or (tops or cands)[0]
    mod = j['modules'][top]
    N = Netlist(mod, top)
    pins = dict(p.split('=', 1) for p in a.pin)

    # --- ports ---
    port_bits = {}                                        # bit -> (pin name, direction)
    for pn, p in N.ports.items():
        for i, b in enumerate(p['bits']):
            nm = sanitize(pn if len(p['bits']) == 1 else f'{pn}[{i}]')
            port_bits.setdefault(b, (nm, p['direction']))
    in_pins = {b: nm for b, (nm, d) in port_bits.items() if d == 'input'}
    out_pins = {b: nm for b, (nm, d) in port_bits.items() if d in ('output', 'inout')}

    # --- storage cells and where they live ---
    storage = {}                                          # cellname -> dict(name, kind, conns)
    tbufs = {}                                            # output-port bit -> (A bit, E bit)
    for cn, c in mod['cells'].items():
        t = c['type']
        if t == '$_TBUF_':
            tbufs[c['connections']['Y'][0]] = (c['connections']['A'][0], c['connections']['E'][0])
    q_home = {}                                           # Q bit -> macrocell name
    for cn, c in mod['cells'].items():
        t = c['type']
        if not (t.startswith('$_DFF') or t.startswith('$_DLATCH')):
            continue
        q = c['connections']['Q'][0]
        home = None
        for ob, nm in out_pins.items():                   # Q drives a port directly or via TBUF
            if ob == q or (ob in tbufs and tbufs[ob][0] == q):
                home = nm
                break
        name = home or N.name(q)
        storage[cn] = dict(name=name, type=t, c=c['connections'], pin=home is not None)
        q_home[q] = name

    # --- combinational logic ---
    sops = {c['connections']['Y'][0]: c for c in mod['cells'].values() if c['type'] == '$sop'}
    nots = {c['connections']['Y'][0]: c['connections']['A'][0] for c in mod['cells'].values() if c['type'] == '$_NOT_'}

    def literal(b, pos=True, depth=0):
        """Resolve a bit to an input-column name + polarity, following aliases/inverters."""
        if b in ('0', '1'):
            return ('CONST', (b == '1') == pos)
        if b in in_pins:
            return (in_pins[b], pos)
        if b in q_home:
            return (q_home[b] + '.Q', pos)
        if b in nots:
            return literal(nots[b], not pos, depth + 1)
        if b in sops:
            terms = sop_terms(sops[b])
            if len(terms) == 1 and len(terms[0]) == 1:       # alias / single literal
                idx, p = terms[0][0]
                return literal(sops[b]['connections']['A'][idx], p == pos, depth + 1)
            node = node_for(b)
            return (node, pos)
        if b in out_pins:                                     # bidirectional read-back
            return (out_pins[b] + '.PIN', pos)
        raise SystemExit(f'yosys2tt2: cannot resolve net {N.name(b)} (bit {b})')

    nodes = {}                                            # bit -> node name (combinational buried)

    def node_for(b):
        if b not in nodes:
            nodes[b] = N.name(b)
        return nodes[b]

    def equation(b):
        """SOP for a bit as a list of terms over input-column names; [] = constant 0."""
        if b in ('0', '1'):
            return [[]] if b == '1' else []
        if b in sops:
            c = sops[b]
            out = []
            for term in sop_terms(c):
                lits = []
                ok = True
                for idx, p in term:
                    nm, pol = literal(c['connections']['A'][idx], p)
                    if nm == 'CONST':
                        if not pol:
                            ok = False
                            break
                        continue
                    lits.append((nm, pol))
                if ok:
                    out.append(lits)
            return out
        nm, pol = literal(b, True)
        if nm == 'CONST':
            return [[]] if pol else []
        return [[(nm, pol)]]

    outputs = []                                          # (column name, terms)
    for cn, s in storage.items():
        c, t, nm = s['c'], s['type'], s['name']
        if t.startswith('$_DLATCH'):
            outputs.append((nm + '.L', equation(c['D'][0])))
            outputs.append((nm + '.LE', equation(c['E'][0])))
        else:
            outputs.append((nm + '.REG', equation(c['D'][0])))
            outputs.append((nm + '.C', equation(c['C'][0])))
            if 'E' in c:
                outputs.append((nm + '.CE', equation(c['E'][0])))
            if 'R' in c:
                outputs.append((nm + ('.AP' if t in ('$_DFF_PP1_', '$_DFFE_PP1P_') else '.AR'), equation(c['R'][0])))
    for ob, nm in out_pins.items():
        if ob in tbufs:
            a_bit, e_bit = tbufs[ob]
            if a_bit not in q_home or q_home[a_bit] != nm:
                outputs.append((nm, equation(a_bit)))
            outputs.append((nm + '.OE', equation(e_bit)))
        elif ob not in q_home:
            outputs.append((nm, equation(ob)))
    # buried combinational nodes (discovered while building equations; may grow while iterating)
    done = set()
    while True:
        todo = [b for b in nodes if b not in done]
        if not todo:
            break
        for b in todo:
            done.add(b)
            outputs.append((nodes[b], equation(b)))

    inputs = sorted({nm for _, terms in outputs for term in terms for nm, _ in term})
    col = {nm: i for i, nm in enumerate(inputs)}
    rows = []
    for oi, (_, terms) in enumerate(outputs):
        for term in terms:
            r = ['-'] * len(inputs)
            for nm, pol in term:
                r[col[nm]] = '1' if pol else '0'
            rows.append(''.join(r) + ' ' + ''.join('1' if k == oi else '0' for k in range(len(outputs))))

    pin_decl = sorted(set(in_pins.values()) | set(out_pins.values()))
    reg_nodes = sorted(s['name'] for s in storage.values() if not s['pin'])
    comb_nodes = sorted(set(nodes.values()))
    with open(a.output, 'w', newline='\r\n') as f:
        f.write('#$ TOOL CUPL\n#$ TITLE %s\n#$ MODULE %s\n#$ JEDECFILE %s\n#$ DEVICE %s\n' % (top, top, top, a.device))
        f.write('#$ PINS %d %s\n' % (len(pin_decl), ' '.join(f'{p}+:{pins[p]}' if p in pins else f'{p}+' for p in pin_decl)))
        nd = reg_nodes + comb_nodes
        f.write('#$ NODES %d %s\n' % (len(nd), ' '.join(f'{n}+' for n in nd)))
        f.write('.i %d\n.o %d\n.type f\n' % (len(inputs), len(outputs)))
        f.write('.ilb %s\n.ob %s\n' % (' '.join(inputs), ' '.join(n for n, _ in outputs)))
        f.write('.phase %s\n.p %d\n' % ('1' * len(outputs), len(rows)))
        f.write('\n'.join(rows) + '\n.e\n')
    print(f'yosys2tt2: {len(pin_decl)} pins, {len(reg_nodes)} buried registers, {len(comb_nodes)} buried '
          f'combinational nodes, {len(outputs)} outputs, {len(rows)} product terms -> {a.output}', file=sys.stderr)


if __name__ == '__main__':
    main()
