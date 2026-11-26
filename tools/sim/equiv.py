#!/usr/bin/env python3
"""Check that a fitted JED implements its source design: formal (Yosys + ABC PDR) and simulation (iverilog).

  tools/sim/run.sh python3 tools/sim/equiv.py JED --device P1504C44 --tt2 design.tt2 [--fit design.fit]
  tools/sim/run.sh python3 tools/sim/equiv.py JED --device P1504C44 --verilog src.v [more.v] --top T --fit design.fit
  tools/sim/run.sh python3 tools/sim/equiv.py JED --device P1504C44 --gold-jed other.jed

The JED becomes "gate" Verilog (`atf15xx-sim verilog`); the source becomes "gold" Verilog with
the same split pin interface (pN_in / pN_oe / pN_out, see docs/architecture/sim/02-verilog.md):
`tt2v.py` for a .tt2, `vsplit.py` (Yosys + tri-state split) for Verilog, the emitter for a second JED.
Every pin is compared: gold outputs must match in enable and (when enabled) value; pins that
gold does not drive must not be driven by the JED (bus contention), unless --ignore-pin.

Formal: a miter of gold and gate (both with registers at their power-up values, or free with
--init free), `clk2fflogic` (one SAT step = one change of the inputs; any clocks, async
controls and latches), AIGER, then `pdr` in yosys-abc: PROVEN means equal for every input
sequence. A failure is replayed with `sat -seq` to write a counterexample VCD (<work>/cex.vcd).
If PDR times out, `bmc3` reports how deep it is known equal.
Simulation: both models in iverilog on random stimulus (one input change per step), with a
VCD (<work>/sim.vcd) of the first failure.

Constraints: --assume EXPR (Python syntax over pN, e.g. 'not (p41 and p44)'), and --reset
pN=V:K (pin N held at V for the first K steps; outputs are compared from step K on; usually
with --init free). Exit status: 0 PASS, 1 FAIL, 2 usage or tool error.
"""
import argparse, os, re, shutil, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(ROOT, 'tools', 'sim'))
from simlib import sim_bin, stim_pins, write_stim  # noqa: E402
import simlib  # noqa: E402
import tt2v  # noqa: E402



# ---------------------------------------------------------------- helpers

def sh(cmd, cwd, timeout=None, check=True):
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout)
    if check and r.returncode != 0:
        raise RuntimeError('%s failed (%d):\n%s%s' % (' '.join(cmd), r.returncode, r.stdout[-3000:], r.stderr[-3000:]))
    return r


def ports(vtext, module):
    b = module_body(vtext, module)
    ins = sorted({int(p) for p in re.findall(r'\binput\s+(?:wire\s+)?p(\d+)_in\b', b)})
    outs = sorted({int(p) for p in re.findall(r'\boutput\s+(?:wire\s+)?p(\d+)_oe\b', b)})
    return ins, outs


def module_body(vtext, module):
    a = re.search(r'^module %s\b' % module, vtext, re.M).start()
    return vtext[a:vtext.index('endmodule', a)]


def regs_of(vtext, module):
    """(reg id, init, is_latch, has_ar, has_ap) for every `reg q_<id> = ...` of a module."""
    b = module_body(vtext, module)
    lat = set(re.findall(r'always @\* .*?q_(\w+) <= d_', b))
    ar = set(re.findall(r'wire ar_(\w+) =', b))
    ap = set(re.findall(r'wire ap_(\w+) =', b))
    return [(i, v, i in lat, i in ar, i in ap) for i, v in re.findall(r"reg q_(\w+) = (1'b[01]);", b)]


def py_to_verilog(expr):
    """Translate a constraint in Python syntax over pN into Verilog over pN_in."""
    toks = re.findall(r'p\d+|\d+|not|and|or|xor|==|!=|\(|\)|\S', expr)
    out = []
    for t in toks:
        if re.fullmatch(r'p\d+', t):
            out.append(t + '_in')
        elif t in ('not', 'and', 'or', 'xor'):
            out.append({'not': '!', 'and': '&&', 'or': '||', 'xor': '^'}[t])
        elif t in ('0', '1'):
            out.append("1'b" + t)
        elif t in ('==', '!=', '(', ')'):
            out.append(t)
        else:
            raise SystemExit('--assume: unsupported token %r (use pN, 0, 1, not, and, or, xor, ==, !=, parentheses)' % t)
    return ' '.join(out)


def py_eval(expr, row):
    env = {'p%d' % p: v for p, v in row.items()}
    return bool(eval(expr, {'__builtins__': {}}, env))


# ---------------------------------------------------------------- models

def build_gate(jed, devname, work, model_args):
    path = os.path.join(work, 'gate.v')
    sh([sim_bin(), 'verilog', jed, '-d', devname, '--module', 'gate', '-o', path] + model_args, cwd=ROOT)
    return path


def build_gold(a, work, devname, model_args):
    path = os.path.join(work, 'gold.v')
    if a.tt2:
        place, oc = ({}, [])
        if a.fit:
            place, oc = tt2v.fit_info(open(a.fit, errors='replace').read())
        if a.open_collector:
            oc = [x for x in a.open_collector.split(',') if x]
        text, _, _ = tt2v.convert(open(a.tt2, errors='replace').read(), place, oc, 'gold')
        open(path, 'w').write(text)
    elif a.gold_jed:
        sh([sim_bin(), 'verilog', a.gold_jed, '-d', devname, '--module', 'gold', '-o', path] + model_args, cwd=ROOT)
    elif a.verilog:
        cmd = [sys.executable, os.path.join(HERE, 'vsplit.py'), '--top', a.top, '-o', path, '--work', work, '--init', a.init]
        if a.fit:
            cmd += ['--fit', a.fit]
        for p in a.pin or []:
            cmd += ['--pin', p]
        r = subprocess.run(cmd + [os.path.abspath(v) for v in a.verilog], cwd=ROOT)
        if r.returncode:
            raise SystemExit(2)
    else:
        raise SystemExit('give --tt2, --verilog or --gold-jed')
    return path


def compare_set(gold_v, gate_v, ignore):
    gi, go = ports(gold_v, 'gold')
    ti, to = ports(gate_v, 'gate')
    pins_in = sorted(set(gi) | set(ti))
    cmp = sorted((set(go) | set(to)) - set(ignore))
    return gi, go, ti, to, pins_in, cmp


# ---------------------------------------------------------------- formal

def miter(gold_v, gate_v, assume_v, reset, ignore, skip, single_change=False):
    gi, go, ti, to, pins_in, cmp = compare_set(gold_v, gate_v, ignore)
    m = ['module miter (']
    m.append('  ' + ',\n  '.join(['input wire p%d_in' % p for p in pins_in] + ['output wire bad']))
    m.append(');')
    for p in go:
        m.append('  wire g_p%d_oe, g_p%d_out;' % (p, p))
    for p in to:
        m.append('  wire t_p%d_oe, t_p%d_out;' % (p, p))
    m.append('  gold g (%s);' % ', '.join(['.p%d_in(p%d_in)' % (p, p) for p in gi] +
                                          ['.p%d_oe(g_p%d_oe), .p%d_out(g_p%d_out)' % (p, p, p, p) for p in go]))
    m.append('  gate t (%s);' % ', '.join(['.p%d_in(p%d_in)' % (p, p) for p in ti] +
                                          ['.p%d_oe(t_p%d_oe), .p%d_out(t_p%d_out)' % (p, p, p, p) for p in to]))
    terms = []
    for p in cmp:
        goe = 'g_p%d_oe' % p if p in go else "1'b0"
        gout = 'g_p%d_out' % p if p in go else "1'b0"
        toe = 't_p%d_oe' % p if p in to else "1'b0"
        tout = 't_p%d_out' % p if p in to else "1'b0"
        terms.append('(%s ^ %s) | (%s ^ %s)' % (goe, toe, gout, tout))
    m.append('  wire mism = %s;' % (' | '.join(terms) if terms else "1'b0"))
    conds = [assume_v] if assume_v else []
    if single_change and pins_in:
        # at most one input pin changes per step (from step 1 on), like the simulation stimulus:
        # excludes races between a clock edge and the data it samples
        for p in pins_in:
            m.append("  reg was_p%d = 1'b0;" % p)
        m.append('  always @($global_clock) begin %s end' % ' '.join('was_p%d <= p%d_in;' % (p, p) for p in pins_in))
        m.append('  wire [%d:0] chg = {%s};' % (len(pins_in) - 1, ', '.join('p%d_in ^ was_p%d' % (p, p) for p in pins_in)))
        conds.append("(cnt == 8'd0 || (chg & (chg - 1'b1)) == 0)")
    rlen = 0
    for (p, v, k) in reset:
        if v == 'clk':      # toggles during the reset sequence: 0 on even steps, 1 on odd steps
            conds.append("(cnt >= 8'd%d || p%d_in == cnt[0])" % (k, p))
        else:
            conds.append("(cnt >= 8'd%d || p%d_in == 1'b%d)" % (k, p, v))
        rlen = max(rlen, k)
    m.append('  reg ok = 1\'b1;')
    m.append("  reg [7:0] cnt = 8'd0;")
    m.append('  wire a = %s;' % (' && '.join('(%s)' % c for c in conds) if conds else "1'b1"))
    m.append("  always @($global_clock) begin ok <= ok & a; if (cnt != 8'hff) cnt <= cnt + 8'd1; end")
    m.append("  assign bad = ok & a & (cnt >= 8'd%d) & mism;" % max(rlen, skip))
    m.append('endmodule')
    return '\n'.join(m) + '\n'


def yosys_prep(init_free):
    s = ['read_verilog -formal gold.v gate.v miter.v', 'hierarchy -top miter', 'proc']
    if init_free:
        s.append('setattr -unset init gold/w:* gate/w:*')
    s += ['flatten', 'opt_clean', 'clk2fflogic', 'opt -keepdc -fast', 'opt_clean']
    return s


class Budget:
    """Wall-clock budget shared by all stages (seconds; None = unlimited, kept for library callers)."""
    def __init__(self, total):
        self.t0, self.total = time.time(), total

    def left(self):
        return float('inf') if self.total is None else self.total - (time.time() - self.t0)

    def stage(self, cap, frac=1.0):
        """Time for a stage: at most `cap`, at most `frac` of what is left."""
        return max(0.0, min(cap, self.left() * frac))


def abc_run(work, script, seconds, log):
    try:
        r = sh(['yosys-abc', '-c', script], cwd=work, timeout=seconds + 10, check=False)
        out = r.stdout + r.stderr
    except subprocess.TimeoutExpired:
        out = '(killed at the %.0f s stage budget)' % seconds
    open(os.path.join(work, log), 'w').write(out)
    return out


def formal(work, init_free, timeout, depth, B=None, engines=('dprove', 'pdr', 'bmc'), reuse=False):
    """Formal engines in order, all inside the budget B:
    1. ABC `dprove`: signal correspondence (scorr: proves register/signal equivalences of the two
       designs by induction) + induction on the miter. Proves wide counters in milliseconds.
    2. ABC `pdr` (IC3) for what correspondence cannot prove.
    3. ABC `bmc3` for a bounded result with the remaining time.
    Returns (status, message, vcd)."""
    B = B or Budget(None)
    ys = yosys_prep(init_free) + ['techmap', 'opt -fast', 'dffunmap', 'aigmap', 'check -assert',
                                  'write_aiger -zinit -map miter.aim miter.aig']
    open(os.path.join(work, 'formal.ys'), 'w').write('\n'.join(ys) + '\n')
    if reuse and os.path.exists(os.path.join(work, 'miter.aig')):
        r = subprocess.CompletedProcess([], 0)
    else:
        try:
            r = sh(['yosys', '-q', '-l', 'formal.log', 'formal.ys'], cwd=work, check=False, timeout=max(1, B.stage(300)))
        except subprocess.TimeoutExpired:
            return 'TIMEOUT', 'budget exhausted while building the miter (yosys)', None
    if r.returncode != 0:
        log = open(os.path.join(work, 'formal.log')).read()
        if 'found logic loop' in log:
            return sat_induct(work, init_free, int(max(1, B.stage(timeout, 0.9))), depth)
        return 'ERROR', 'yosys failed (see %s)' % os.path.relpath(os.path.join(work, 'formal.log'), ROOT), None

    def failed(out, engine):
        m = re.search(r'asserted in frame (\d+)', out) or re.search(r'Output \d+ .*?frame (\d+)', out)
        if not m:
            return None
        k = int(m.group(1))
        vcd = cex(work, init_free, k + 1, B) if B.left() > 5 else None
        return 'FAIL', 'counterexample of %d steps (%s)%s' % (k + 1, engine, '' if vcd else '; no time left for the VCD replay'), vcd

    t = B.stage(timeout, 0.35 if len(engines) > 1 else 1.0)
    if 'dprove' in engines and t >= 1:
        out = abc_run(work, 'read_aiger miter.aig; strash; dprove -T %d' % max(1, int(t)), t, 'dprove.log')
        if re.search(r'Networks are equivalent', out):
            return 'PROVEN', 'signal correspondence + induction (ABC dprove)', None
        f = failed(out, 'dprove')
        if f:
            return f
    if 'pdr' not in engines:
        return 'INCONCLUSIVE', 'dprove could not prove it (no complete signal correspondence)', None
    t = B.stage(timeout, 0.6)
    if t >= 1:
        out = abc_run(work, 'read_aiger miter.aig; strash; pdr -T %d' % max(1, int(t)), t, 'pdr.log')
        if re.search(r'Property proved|UNSAT', out):
            return 'PROVEN', 'PDR proved the miter output is never 1', None
        f = failed(out, 'pdr')
        if f:
            return f
    t = B.stage(timeout, 0.8)
    if t < 1:
        return 'TIMEOUT', 'budget exhausted before a bounded check (dprove/pdr inconclusive)', None
    out = abc_run(work, 'read_aiger miter.aig; strash; bmc3 -F %d -T %d' % (depth, max(1, int(t))), t, 'bmc.log')
    f = failed(out, 'bmc3')
    if f:
        return f
    m = re.findall(r'No output asserted in (\d+) frames', out)
    return 'BOUNDED', 'dprove/PDR inconclusive within the budget; no difference in the first %s steps (bmc3)' % (
        m[-1] if m else '0'), None


def sat_induct(work, init_free, timeout, depth):
    """Fallback for miters with combinational loops (e.g. a transparent latch fed back through
    its own pin), which AIGER cannot express: Yosys `sat` encodes the loop as constraints
    (sound once --assume excludes the input values that make it oscillate) and tries temporal
    induction up to `depth` steps."""
    ys = yosys_prep(init_free) + [
        'sat -tempinduct -prove bad 0 -maxsteps %d %s -show-public -dump_vcd cex.vcd -timeout %d' %
        (depth, '-enable_undef -set-init-def -set-def-inputs' if init_free else '-set-init-zero', timeout)]
    open(os.path.join(work, 'induct.ys'), 'w').write('\n'.join(ys) + '\n')
    p = os.path.join(work, 'cex.vcd')
    if os.path.exists(p):
        os.remove(p)
    timed_out = False
    try:
        sh(['yosys', '-q', '-l', 'induct.log', 'induct.ys'], cwd=work, check=False, timeout=max(1, timeout))
    except subprocess.TimeoutExpired:
        timed_out = True
    log = open(os.path.join(work, 'induct.log')).read() if os.path.exists(os.path.join(work, 'induct.log')) else ''
    if re.search(r'Induction step proven: SUCCESS', log):
        return 'PROVEN', 'combinational loop in the netlist; proven by sat temporal induction', None
    m = re.search(r'model found for base case|base case: FAIL|failed in step (\d+)', log)
    if m or (os.path.exists(p) and 'FAIL' in log and not timed_out):
        return 'FAIL', 'combinational loop in the netlist; sat base case found a counterexample', p if os.path.exists(p) else None
    proven = [int(k) for k in re.findall(r'Base case for induction length (\d+) proven', log)]
    k = max(proven) if proven else 0
    why = 'stopped at the %d s stage budget' % timeout if timed_out or 'TIMEOUT' in log else 'induction inconclusive'
    return 'BOUNDED', 'combinational loop in the netlist; no difference in the first %d steps (sat, %s)' % (k, why), None


def explain(work, logname):
    """Summarise a `sat -show-public` counterexample: per step the inputs and the pins where
    source (g) and JED (t) differ. Written to <work>/cex.txt; returns its first lines."""
    p = os.path.join(work, logname)
    if not os.path.exists(p):
        return ''
    tab = {}
    for m in re.finditer(r'^\s+(\d+)\s+\\(\S+)\s+\S+\s+\S+\s+([01x]+)\s*$', open(p).read(), re.M):
        tab.setdefault(int(m.group(1)), {})[m.group(2)] = m.group(3)
    out = []
    prev = {}
    for t in sorted(tab):
        v = tab[t]
        ins = {k: x for k, x in v.items() if re.fullmatch(r'p\d+_in', k)}
        chg = [k for k in sorted(ins, key=lambda k: int(k[1:-3])) if prev.get(k) != ins[k]]
        prev = ins
        line = 'step %d: inputs %s' % (t - 1, ' '.join('%s=%s' % (k[:-3], ins[k]) for k in chg) or '(unchanged)')
        diffs = []
        for k in sorted(v):
            m = re.fullmatch(r'g_p(\d+)_oe', k)
            if not m:
                continue
            n = m.group(1)
            g = (v.get('g_p%s_oe' % n), v.get('g_p%s_out' % n))
            j = (v.get('t_p%s_oe' % n, '0'), v.get('t_p%s_out' % n, '0'))
            if g != j:
                diffs.append('pin %s source %s jed %s' % (n, 'Z' if g[0] == '0' else g[1], 'Z' if j[0] == '0' else j[1]))
        for k in sorted(v):
            m = re.fullmatch(r't_p(\d+)_oe', k)
            if m and 'g_p%s_oe' % m.group(1) not in v and v[k] == '1':
                diffs.append('pin %s source Z jed %s' % (m.group(1), v.get('t_p%s_out' % m.group(1))))
        out.append(line + ('\n    DIFF ' + '; '.join(diffs) if diffs else ''))
    open(os.path.join(work, 'cex.txt'), 'w').write('\n'.join(out) + '\n')
    return '\n'.join(out)


def sat_table(path):
    tab = {}
    if os.path.exists(path):
        for m in re.finditer(r'^\s+(\d+)\s+\\(\S+)\s+\S+\s+\S+\s+([01x]+)\s*$', open(path).read(), re.M):
            tab.setdefault(int(m.group(1)), {})[m.group(2)] = m.group(3)
    return tab


def diagnose_init(work, logname, steps=60, seed=1, timeout=900):
    """After a formal FAIL: can *any* pair of initial states (source and JED registers both free)
    make the pins equal under the counterexample's inputs, extended with seeded random inputs to
    `steps` steps? UNSAT means the difference is not a power-up/initial-state question."""
    import random
    tab = sat_table(os.path.join(work, logname))
    if not tab:
        return None, 'no counterexample table'
    ins = sorted({k for v in tab.values() for k in v if re.fullmatch(r'p\d+_in', k)}, key=lambda k: int(k[1:-3]))
    rnd = random.Random(seed)
    last = dict(tab[max(tab)])
    sets = []
    for t in range(1, steps + 1):
        row = tab.get(t)
        if row is None:
            row = dict(last)
            k = rnd.choice(ins)
            row[k] = '1' if row.get(k) == '0' else '0'
        last = row
        sets += ['-set-at %d %s %s' % (t, k, row[k]) for k in ins if row.get(k) in ('0', '1')]
    ys = ['read_verilog -formal gold.v gate.v miter.v', 'hierarchy -top miter', 'proc',
          'setattr -unset init gold/w:* gate/w:*', 'flatten', 'opt_clean', 'clk2fflogic', 'opt -keepdc -fast',
          'sat -seq %d -set mism 0 %s -enable_undef -set-init-def -set-def-inputs -timeout 600' % (steps, ' '.join(sets))]
    open(os.path.join(work, 'init_diag.ys'), 'w').write('\n'.join(ys) + '\n')
    try:
        sh(['yosys', '-q', '-l', 'init_diag.log', 'init_diag.ys'], cwd=work, check=False, timeout=max(1, timeout))
    except subprocess.TimeoutExpired:
        return None, 'timed out'
    log = open(os.path.join(work, 'init_diag.log')).read()
    if 'no model found' in log:
        return False, ('no initial state of either side makes the pins equal over %d steps of the '
                       'counterexample inputs: the logic differs (not a power-up question)' % steps)
    if 'model found' in log:
        return True, ('some initial state makes the pins equal over %d steps: the difference may be only the '
                      'power-up state (try --power-up fuse, or --init free --reset pN=V:K)' % steps)
    return None, 'inconclusive (see init_diag.log)'


def cex(work, init_free, steps, B=None):
    ys = yosys_prep(init_free) + [
        'sat -seq %d -prove bad 0 %s -show-public -dump_vcd cex.vcd -timeout 600' %
        (steps, '-enable_undef -set-init-def -set-def-inputs' if init_free else '-set-init-zero')]
    open(os.path.join(work, 'cex.ys'), 'w').write('\n'.join(ys) + '\n')
    try:
        sh(['yosys', '-q', '-l', 'cex.log', 'cex.ys'], cwd=work, check=False,
           timeout=max(1, (B or Budget(None)).stage(600)))
    except subprocess.TimeoutExpired:
        return None
    p = os.path.join(work, 'cex.vcd')
    return p if os.path.exists(p) else None


# ---------------------------------------------------------------- simulation

def simulate(work, gold_v, gate_v, devname, steps, seed, assume_py, reset, ignore, init_free, skip, timeout=300):
    import random
    dev = simlib.device(devname)
    gi, go, ti, to, pins_in, cmp = compare_set(gold_v, gate_v, ignore)
    spins = [p for p in stim_pins(dev) if p in pins_in] + [p for p in pins_in if p not in stim_pins(dev)]
    rnd = random.Random(seed)
    cur = {p: 0 for p in spins}
    rows = []
    rlen = max([k for _, _, k in reset] + [0, skip])
    for c in range(steps):
        for _ in range(50):
            r = dict(cur)
            p = rnd.choice(spins)
            if p == dev['gclr'] and rnd.random() < 0.9:
                p = dev['gclk1'] if dev['gclk1'] in spins else p
            r[p] ^= 1
            for (rp, v, k) in reset:
                if c < k and rp in r:
                    r[rp] = (c % 2) if v == 'clk' else v
            if not assume_py or py_eval(assume_py, r):
                break
        else:
            r = dict(cur)
        cur = r
        rows.append(r)
    write_stim(os.path.join(work, 'sim_stim.csv'), spins, rows)
    with open(os.path.join(work, 'sim_stim.mem'), 'w') as f:
        for r in rows:
            f.write(''.join(str(r[p]) for p in spins) + '\n')
    t = ['`timescale 1ns/1ps', 'module tb;']
    t += ['  reg p%d_in = 1\'b0;' % p for p in pins_in]
    t += ['  wire g_p%d_oe, g_p%d_out;' % (p, p) for p in go]
    t += ['  wire t_p%d_oe, t_p%d_out;' % (p, p) for p in to]
    t.append('  gold g (%s);' % ', '.join(['.p%d_in(p%d_in)' % (p, p) for p in gi] +
                                          ['.p%d_oe(g_p%d_oe), .p%d_out(g_p%d_out)' % (p, p, p, p) for p in go]))
    t.append('  gate u (%s);' % ', '.join(['.p%d_in(p%d_in)' % (p, p) for p in ti] +
                                          ['.p%d_oe(t_p%d_oe), .p%d_out(t_p%d_out)' % (p, p, p, p) for p in to]))
    t.append('  reg [%d:0] stim [0:%d];' % (len(spins) - 1, steps - 1))
    t.append('  integer i, f;')
    t.append('  initial begin')
    t.append('    $readmemb("sim_stim.mem", stim);')
    t.append('    $dumpfile("sim.vcd"); $dumpvars(0, tb);')
    t.append('    #5;')
    for inst, text, mod in (('g', gold_v, 'gold'), ('u', gate_v, 'gate')):
        for i, v, lat, ar, ap in regs_of(text, mod):
            if init_free:
                v = "1'b%d" % rnd.randint(0, 1)
            elif lat:
                continue
            cond = ' && '.join(['%s.%s_%s !== 1\'b1' % (inst, k, i) for k, h in (('ar', ar), ('ap', ap)) if h]) or '1'
            t.append('    if (%s) %s.q_%s = %s;' % (cond, inst, i, v))
    t.append('    #5;')
    t.append('    f = $fopen("sim_out.csv", "w");')
    t.append('    for (i = 0; i < %d; i = i + 1) begin' % steps)
    t.append('      {%s} = stim[i];' % ', '.join('p%d_in' % p for p in spins))
    t.append('      #10;')
    t.append('      $fwrite(f, "%0d", i);')
    for p in cmp:
        for side, outs in (('g', go), ('t', to)):
            if p in outs:
                t.append('      if (%s_p%d_oe === 1\'b1) $fwrite(f, ",%%b", %s_p%d_out); else if (%s_p%d_oe === 1\'b0) $fwrite(f, ",Z"); else $fwrite(f, ",X");' % (side, p, side, p, side, p))
            else:
                t.append('      $fwrite(f, ",Z");')
    t.append('      $fwrite(f, "\\n");')
    t.append('    end')
    t.append('    $fclose(f); $finish;')
    t.append('  end')
    t.append('endmodule')
    open(os.path.join(work, 'sim_tb.v'), 'w').write('\n'.join(t) + '\n')
    sh(['iverilog', '-g2005', '-o', 'sim.vvp', 'sim_tb.v', 'gold.v', 'gate.v'], cwd=work)
    try:
        sh(['vvp', '-n', 'sim.vvp'], cwd=work, timeout=timeout)
    except subprocess.TimeoutExpired:
        return 'ERROR', 'iverilog did not finish (oscillating combinational loop?)', None
    lines = open(os.path.join(work, 'sim_out.csv')).read().splitlines()
    undefined = 0
    for line in lines:
        f = line.split(',')
        i = int(f[0])
        if i < rlen:
            continue
        for j, p in enumerate(cmp):
            g, tv = f[1 + 2 * j], f[2 + 2 * j]
            if g in ('x', 'X'):
                undefined += 1      # the source leaves this pin undefined (X): any JED value is allowed
                continue
            if g != tv:
                return 'FAIL', 'step %d pin %d: source=%s jed=%s (stimulus %s)' % (
                    i, p, g, tv, os.path.join(work, 'sim_stim.csv')), os.path.join(work, 'sim.vcd')
    return 'PASS', '%d steps, %d pins compared%s' % (
        steps, len(cmp), ', %d samples where the source is X skipped' % undefined if undefined else ''), None


# ---------------------------------------------------------------- main

def run_verilator(a, work, B, rep, share):
    import vcompare
    vw = os.path.join(work, 'vcompare')
    os.makedirs(vw, exist_ok=True)
    shutil.copy(os.path.join(work, 'gold.v'), os.path.join(vw, 'gold.v'))
    a._gold_ready = True
    st, msg, d = vcompare.vcompare(a, vw, max(1.0, B.stage(1e9, share)))
    print('  sim:    %s  %s' % (st, msg))
    rep['sim'] = dict(d, status=st, message=msg, engine='verilator')
    return st


def normalise_args(a, ap):
    """Shared by equiv.py and vcompare.py: expand --pin-file, parse --reset/--reset-clock into a._reset."""
    if a.pin_file:
        a.pin = list(a.pin or [])
        for line in open(a.pin_file):
            line = line.split('#')[0].strip()
            if line:
                k, v = re.split(r'\s*=\s*|\s+', line, maxsplit=1)
                a.pin.append('%s=%s' % (k, v))
    reset = []
    for r in a.reset:
        m = re.fullmatch(r'p?(\d+)=([01]):(\d+)', r)
        if not m:
            ap.error('--reset %s: expected pN=V:K' % r)
        reset.append((int(m.group(1)), int(m.group(2)), int(m.group(3))))
    rlen = max([k for _, _, k in reset] + [0])
    for p in a.reset_clock:
        if not rlen:
            ap.error('--reset-clock needs --reset')
        reset.append((p, 'clk', rlen))
    a._reset = reset


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0], epilog=__doc__.split('\n', 2)[2],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('jed', help='fitted JEDEC file to check')
    ap.add_argument('--device', required=True, help='device, e.g. P1504C44')
    g = ap.add_argument_group('source design (one of)')
    g.add_argument('--tt2', help='CUPL .tt2 netlist')
    g.add_argument('--verilog', nargs='+', help='Verilog source files (needs --top and a pin map)')
    g.add_argument('--gold-jed', help='another JED (e.g. the vendor fitter output) for the same pins')
    ap.add_argument('--top', help='--verilog: top module')
    ap.add_argument('--fit', help='fitter report: pin placement (NAME is placed at pin N)')
    ap.add_argument('--pin', action='append', help='--verilog: PORT=N pin assignment (repeatable)')
    ap.add_argument('--pin-file', help='--verilog: file of PORT=N (or "PORT N") lines; # comments')
    ap.add_argument('--open-collector', default='', help='--tt2: comma-separated open-collector outputs')
    ap.add_argument('--init', choices=['zero', 'free'], default='zero',
                    help='initial register state: zero = source registers without an initial value start at 0, the '
                         'JED at its modelled power-up (--power-up); free = both sides start anywhere (use with --reset)')
    ap.add_argument('--reset', action='append', default=[], help='pN=V:K: hold pin N at V for the first K steps, compare after')
    ap.add_argument('--reset-clock', action='append', default=[], type=int, metavar='N',
                    help='pin N toggles (0,1,0,...) during the --reset steps (for synchronous resets)')
    ap.add_argument('--assume', help="input constraint in Python syntax over pN, e.g. 'not (p41 and p44)'")
    ap.add_argument('--single-change', action='store_true',
                    help='formal: at most one input pin changes per step (as in the simulation stimulus); '
                         'excludes races between a clock edge and its data (bus protocols, data used as clocks)')
    ap.add_argument('--ignore-pin', action='append', type=int, default=[], help='do not compare this pin')
    ap.add_argument('--skip', type=int, default=0, help='do not compare the first SKIP steps')
    ap.add_argument('--no-formal', action='store_true')
    ap.add_argument('--no-sim', action='store_true')
    ap.add_argument('--timeout', type=int, default=300, help='PDR / BMC time limit in seconds')
    ap.add_argument('--depth', type=int, default=40, help='bmc3 depth when PDR is inconclusive')
    ap.add_argument('--steps', type=int, default=4000, help='simulation steps')
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--preset-wins', action='store_true', help='model option (see atf15xx-sim --help)')
    ap.add_argument('--power-up', choices=['0', '1', 'fuse'], help='model option: register power-up of the JED: 0, 1, or fuse = each register\'s Invert/reset fuse')
    ap.add_argument('--model-opt', action='append', default=[],
                    help='extra atf15xx-sim model flag for the JED, e.g. --model-opt=--lender-a-is-one (repeatable)')
    ap.add_argument('--work', help='work directory (default .scratch/sim/equiv/<jed name>)')
    ap.add_argument('--json', help='also write the result as JSON to this file')
    ap.add_argument('--no-init-diag', action='store_true', help='skip the power-up diagnosis after a formal FAIL')
    ap.add_argument('--budget', type=float, default=600.0,
                    help='total wall-clock budget for all stages, s (default 600); formal gets at most --timeout and '
                         '70%% of what is left, simulation the rest')
    ap.add_argument('--sim-engine', choices=['iverilog', 'verilator'], default='iverilog',
                    help='simulation engine: iverilog (--steps random steps) or verilator (tools/sim/vcompare.py: '
                         'cycle budget, clock bursts, state injection)')
    import vcompare
    vcompare.add_args(ap)
    a = ap.parse_args()
    normalise_args(a, ap)
    work = a.work or os.path.join(ROOT, '.scratch', 'sim', 'equiv', os.path.splitext(os.path.basename(a.jed))[0])
    os.makedirs(work, exist_ok=True)
    model_args = (['--preset-wins'] if a.preset_wins else []) + (['--power-up', a.power_up] if a.power_up else []) + a.model_opt
    reset = a._reset
    B = Budget(a.budget)
    gate = open(build_gate(os.path.abspath(a.jed), a.device, work, model_args)).read()
    gold = open(build_gold(a, work, a.device, model_args)).read()
    open(os.path.join(work, 'miter.v'), 'w').write(
        miter(gold, gate, py_to_verilog(a.assume) if a.assume else None, reset, a.ignore_pin, a.skip, a.single_change))
    _, go, _, to, _, cmp = compare_set(gold, gate, a.ignore_pin)
    print('equiv %s (%s): %d pins compared; source drives %d, JED drives %d; work %s' % (
        a.jed, a.device, len(cmp), len(go), len(to), os.path.relpath(work, ROOT)))
    extra = sorted(set(to) - set(go) - set(a.ignore_pin))
    if extra:
        print('  note: the JED can drive pins the source does not: %s (compared as "never driven")' % extra)
    fail = False
    rep = {'jed': a.jed, 'device': a.device, 'work': os.path.relpath(work, ROOT), 'pins_compared': cmp,
           'source_drives': go, 'options': {'init': a.init, 'power_up': a.power_up or '0', 'assume': a.assume,
                                            'reset': a.reset, 'ignore_pin': a.ignore_pin}}
    for f in ('cex.log', 'induct.log', 'cex.vcd', 'cex.txt', 'init_diag.log'):
        if os.path.exists(os.path.join(work, f)):
            os.remove(os.path.join(work, f))
    staged = not a.no_formal and not a.no_sim and a.sim_engine == 'verilator'
    pre_sim = None
    if staged:
        # quick unbounded attempt first; then simulation with state injection; then PDR/BMC
        FB = Budget(B.stage(a.timeout, 0.25))
        st0, msg0, vcd0 = formal(work, a.init == 'free', a.timeout, a.depth, FB, engines=('dprove',))
        if st0 in ('PROVEN', 'FAIL', 'ERROR', 'TIMEOUT') or st0.startswith('N/A') or \
                (st0 == 'BOUNDED' and 'combinational loop' in msg0):
            pre_sim = (st0, msg0, vcd0)
    if not a.no_formal and staged and pre_sim is None:
        vres = run_verilator(a, work, B, rep, share=0.6)
        fail |= vres == 'FAIL'
        errors_v = vres == 'ERROR'
        if vres == 'FAIL':
            st, msg, vcd = 'SKIPPED', 'not run: simulation already found a difference', None
        else:
            FB = Budget(B.stage(a.timeout * 3, 0.95))
            st, msg, vcd = formal(work, a.init == 'free', a.timeout, a.depth, FB, engines=('pdr', 'bmc'), reuse=True)
    elif not a.no_formal:
        if pre_sim:
            st, msg, vcd = pre_sim
        else:
            FB = Budget(B.stage(a.timeout * 3, 0.7 if not a.no_sim else 0.95))
            st, msg, vcd = formal(work, a.init == 'free', a.timeout, a.depth, FB)
    if not a.no_formal:
        print('  formal: %s  %s%s' % (st, msg, ('  VCD ' + os.path.relpath(vcd, ROOT)) if vcd else ''))
        rep['formal'] = {'status': st, 'message': msg, 'vcd': os.path.relpath(vcd, ROOT) if vcd else None}
        if st == 'FAIL':
            logname = 'cex.log' if os.path.exists(os.path.join(work, 'cex.log')) else 'induct.log'
            e = explain(work, logname)
            for l in e.splitlines()[-12:]:
                print('    ' + l)
            rep['formal']['counterexample'] = e
            if not a.no_init_diag and B.left() > 10:
                ok, why = diagnose_init(work, logname, timeout=B.stage(300, 0.5))
                print('  power-up: %s' % why)
                rep['formal']['power_up_explains'] = ok
                rep['formal']['power_up_diagnosis'] = why
        fail |= st == 'FAIL'
        errors = st == 'ERROR'
    else:
        errors = False
    errors_v = locals().get('errors_v', False)
    if not a.no_sim and a.sim_engine == 'verilator':
        if 'sim' not in rep:
            proven = rep.get('formal', {}).get('status') == 'PROVEN'
            vres = run_verilator(a, work, B, rep, share=0.2 if proven else 1.0)
            fail |= vres == 'FAIL'
            errors |= vres == 'ERROR'
        else:
            errors |= errors_v
    elif not a.no_sim:
        st, msg, vcd = simulate(work, gold, gate, a.device, a.steps, a.seed, a.assume, reset, a.ignore_pin,
                                a.init == 'free', a.skip, timeout=max(1, B.stage(300)))
        print('  sim:    %s  %s%s' % (st, msg, ('  VCD ' + os.path.relpath(vcd, ROOT)) if vcd else ''))
        rep['sim'] = {'status': st, 'message': msg, 'vcd': os.path.relpath(vcd, ROOT) if vcd else None}
        fail |= st == 'FAIL'
        errors |= st == 'ERROR'
    rep['result'] = 'FAIL' if fail else 'ERROR' if errors else 'PASS'
    fs = rep.get('formal', {}).get('status')
    ss = rep.get('sim', {})
    if fail:
        g = 'FAIL'
    elif fs == 'PROVEN':
        g = 'PROVEN (%s)' % rep['formal']['message']
    elif fs == 'BOUNDED':
        m = re.search(r'first (\d+) steps', rep['formal']['message'])
        g = 'BOUNDED %s' % (m.group(1) if m else '?')
        if ss.get('status') == 'SIMULATED':
            g += ' + SIMULATED %d cycles + %d injections' % (ss.get('cycles', 0), ss.get('injections', 0))
    elif ss.get('status') == 'SIMULATED':
        g = 'SIMULATED %d cycles + %d injections' % (ss.get('cycles', 0), ss.get('injections', 0))
    elif ss.get('status') == 'PASS':
        g = 'SIMULATED %d steps (iverilog)' % a.steps
    else:
        g = 'NONE (no engine reached a result within the budget)'
    rep['guarantee'] = g
    rep['seconds'] = round(time.time() - B.t0, 1)
    print('GUARANTEE: %s  (%.1f s of %.0f s budget)' % (g, rep['seconds'], a.budget))
    if a.json:
        import json
        json.dump(rep, open(a.json, 'w'), indent=1)
    print('RESULT: %s' % rep['result'])
    sys.exit(1 if fail else 2 if errors else 0)


if __name__ == '__main__':
    main()
