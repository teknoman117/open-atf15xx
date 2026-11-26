#!/usr/bin/env python3
"""Time-bounded Verilator comparison of a fitted JED against its source, with state injection.

  tools/sim/run.sh python3 tools/sim/vcompare.py JED --device D --verilog src.v --top T --fit X.fit [--budget 60]
  (source options as equiv.py: --tt2 X.tt2 | --verilog ... --top T | --gold-jed Y.jed; pins from --fit/--pin/--pin-file)

Gold (source) and gate (`atf15xx-sim verilog` of the JED) are built with the split pin interface
(docs/architecture/sim/02-verilog.md: pN_in / pN_oe / pN_out, tri-states split by vsplit.py) and
compiled into ONE Verilator model (`vtop`) driven by a generated C++ testbench. Every step drives
the same levels on every input pin of both, and compares every compared pin: OE, and the value
while enabled (masked outputs). A pin the source never drives must not be driven by the JED.

Stimulus: repeated phases of random single-input changes (`--random-len` steps; inputs filtered
by --assume) and clock bursts (`--burst-len` steps toggling the clock pins only: GCLK pins and
--clock N), after an optional reset sequence (--reset pN=V:K, --reset-clock N).

Deep state without 2^n cycles: a register correspondence (source register bit ↔ JED macrocell
register, with polarity) is built from the fitter report's node names (`NAME is placed at ...
(MC m)`) and checked/completed by a calibration run from power-up (equal or complementary
traces). If it is complete, every --inject-every steps the same random state is written into
corresponding registers of both models (VPI) and the comparison continues. A mismatch found
after an injection is reported as FAIL (from injected state): a correct fit computes the same
next state as the source for every corresponding state, unless it exploits unreachable states.

Budgets (never unbounded): --budget total wall-clock seconds (default 60), --compile-budget,
--cycles (cycle budget). The verdict says what was achieved:
  SIMULATED n cycles + k injections      (no difference found)
  FAIL at step s (pin p ...)             (+ <work>/mismatch_pins.vcd, mismatch_full.vcd)
  TIMEOUT in <stage>                     (budget exhausted before the simulation started)
Exit status 0 no difference, 1 FAIL, 2 error/timeout before simulating.
"""
import argparse, json, os, random, re, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(ROOT, 'tools', 'sim'))
import equiv  # noqa: E402
import simlib  # noqa: E402



class Budget:
    def __init__(self, total):
        self.t0 = time.time()
        self.total = total

    def left(self):
        return self.total - (time.time() - self.t0)

    def used(self):
        return time.time() - self.t0


# ------------------------------------------------------------------ register discovery

def gold_regs(text):
    """[(name, width)] of `reg` declarations in module gold (vsplit / tt2v / emitter output)."""
    b = equiv.module_body(text, 'gold')
    out = []
    for m in re.finditer(r'^\s*reg\s+(?:\[(\d+):(\d+)\]\s+)?([A-Za-z_][A-Za-z0-9_$]*)\b', b, re.M):
        hi, lo, n = m.group(1), m.group(2), m.group(3)
        w = abs(int(hi) - int(lo)) + 1 if hi is not None else 1
        out.append((n, w, int(lo) if lo is not None else 0))
    return out


def gold_aliases(text):
    """(reg name, bit) -> [public names 'net[i]' / 'net'] that carry the same value, from the
    `assign {a, b[3:0]} = {r, ...};` lines Yosys write_verilog uses to split and rename registers."""
    b = equiv.module_body(text, 'gold')
    width = {}
    for m in re.finditer(r'^\s*(?:reg|wire|input|output)\s+(?:wire\s+)?(?:\[(\d+):(\d+)\]\s+)?([A-Za-z_][A-Za-z0-9_$]*)\b', b, re.M):
        hi, lo = m.group(1), m.group(2)
        width[m.group(3)] = (int(hi), int(lo)) if hi is not None else None

    def bits(expr):
        expr = expr.strip()
        if expr.startswith('{'):
            parts = [p.strip() for p in expr[1:-1].split(',')]
        else:
            parts = [expr]
        out = []
        for p in parts:
            m = re.fullmatch(r'([A-Za-z_][A-Za-z0-9_$]*)(?:\[(\d+)(?::(\d+))?\])?', p)
            if not m:
                return None
            n, h, l = m.group(1), m.group(2), m.group(3)
            if h is None:
                w = width.get(n, 'unknown')
                if w == 'unknown':
                    return None
                out += [n] if w is None else ['%s[%d]' % (n, i) for i in range(w[0], w[1] - 1, -1)]
            elif l is None:
                out.append('%s[%s]' % (n, h))
            else:
                out += ['%s[%d]' % (n, i) for i in range(int(h), int(l) - 1, -1)]
        return out
    parent = {}

    def find(x):
        while parent.get(x, x) != x:
            x = parent[x]
        return x
    for m in re.finditer(r'^\s*assign\s+(.+?)\s*=\s*(.+?);\s*$', b, re.M):
        l, r = bits(m.group(1)), bits(m.group(2))
        if l and r and len(l) == len(r):
            for x, y in zip(l, r):
                parent[find(x)] = find(y)
    classes = {}
    for x in list(parent):
        classes.setdefault(find(x), []).append(x)
    for root in list(classes):
        if root not in classes[root]:
            classes[root].append(root)
    out = {}
    for members in classes.values():
        public = [x for x in members if not x.startswith('_') and not re.match(r'p\d+_', x)]
        for x in members:
            out[x] = public
    return out


def gate_regs(text):
    """[(mc, used)] for every `reg q_MC` of the emitted JED model; used = Q is read (fb or pin)."""
    b = equiv.module_body(text, 'gate')
    out = []
    for m in re.finditer(r"reg q_(\d+) = 1'b[01];", b):
        mc = m.group(1)
        used = re.search(r'assign fb_%s = q_%s;' % (mc, mc), b) or re.search(r'wire drv_v_%s = q_%s;' % (mc, mc), b) \
            or re.search(r'drv_en_%s = .*~q_%s;' % (mc, mc), b)
        out.append((int(mc), bool(used)))
    return out


def fit_nodes(fit_text):
    """NAME -> MC from a fitter report ('NAME is placed at pin N (MC m)' / 'at feedback node X (MC m)')."""
    d = {}
    for m in re.finditer(r'^(\S+) is placed at (?:pin \d+|feedback node \d+|\S+ \d+) \(MC (\d+)\)',
                         fit_text.replace('\r', ''), re.M):
        d[m.group(1)] = int(m.group(2))
    return d


def gold_bit_name(reg, bit, width, kind):
    """The fitter's node name of a gold register bit."""
    if kind == 'tt2':
        return reg[len('q_n_'):] if reg.startswith('q_n_') else None
    if kind == 'jed':
        return None
    return equiv_sanitize(reg if width == 1 else '%s[%d]' % (reg, bit))


def equiv_sanitize(n):
    n = re.sub(r'\[(\d+)\]', r'\1', n.lstrip('\\'))
    n = re.sub(r'[^A-Za-z0-9_]', '_', n)
    return n if re.match(r'[A-Za-z_]', n) else 'N' + n


# ------------------------------------------------------------------ code generation

def cpp_expr(py):
    """--assume (Python syntax over pN) -> C++ over in[]."""
    toks = re.findall(r'p\d+|\d+|not|and|or|xor|==|!=|\(|\)|\S', py)
    out = []
    for t in toks:
        if re.fullmatch(r'p\d+', t):
            out.append('PIN(%s)' % t[1:])
        else:
            out.append({'not': '!', 'and': '&&', 'or': '||', 'xor': '^'}.get(t, t))
    return ' '.join(out)


def add_latch_hold(vtext, module):
    """Give every generated latch (emitter / tt2v form `if (clk_X) q_X <= d_X;` in `always @*`) an
    extra enable term `!latch_hold`, and module `module` an input `latch_hold`. The testbench then
    evaluates each step twice: with latches held (flip-flops sample the pre-edge latch values, as
    in hardware and in the Rust model), then with latches transparent. Without it Verilator lets a
    latch and a flip-flop on the same clock race (06-verilator.md §T6)."""
    pat = re.compile(r'(always @\* .*?)if \((clk_\w+)\) (q_\w+) <= (d_\w+);')
    new, n = pat.subn(r'\1if (\2 && !latch_hold) \3 <= \4;', vtext)
    if not n:
        return vtext, False
    new = re.sub(r'(^module %s \(\n)' % module, r'\1  input wire latch_hold,\n', new, count=1, flags=re.M)
    return new, True


def write_top(work, gi, go, ti, to, pins_in, cmp, hold_g=False, hold_t=False):
    v = ['module vtop (']
    ports = ['input wire latch_hold'] + ['input wire p%d_in' % p for p in pins_in]
    for p in cmp:
        ports += ['output wire g_p%d_oe' % p, 'output wire g_p%d_out' % p, 'output wire t_p%d_oe' % p, 'output wire t_p%d_out' % p]
    v.append('  ' + ',\n  '.join(ports))
    v.append(');')
    for p in go:
        v.append('  wire gw_p%d_oe, gw_p%d_out;' % (p, p))
    for p in to:
        v.append('  wire tw_p%d_oe, tw_p%d_out;' % (p, p))
    v.append('  gold g (%s);' % ', '.join((['.latch_hold(latch_hold)'] if hold_g else []) + ['.p%d_in(p%d_in)' % (p, p) for p in gi] +
                                          ['.p%d_oe(gw_p%d_oe), .p%d_out(gw_p%d_out)' % (p, p, p, p) for p in go]))
    v.append('  gate t (%s);' % ', '.join((['.latch_hold(latch_hold)'] if hold_t else []) + ['.p%d_in(p%d_in)' % (p, p) for p in ti] +
                                          ['.p%d_oe(tw_p%d_oe), .p%d_out(tw_p%d_out)' % (p, p, p, p) for p in to]))
    for p in cmp:
        v.append("  assign g_p%d_oe = %s; assign g_p%d_out = %s;" % (
            p, 'gw_p%d_oe' % p if p in go else "1'b0", p, 'gw_p%d_out' % p if p in go else "1'b0"))
        v.append("  assign t_p%d_oe = %s; assign t_p%d_out = %s;" % (
            p, 'tw_p%d_oe' % p if p in to else "1'b0", p, 'tw_p%d_out' % p if p in to else "1'b0"))
    v.append('endmodule')
    open(os.path.join(work, 'vtop.v'), 'w').write('\n'.join(v) + '\n')


CPP = r'''
// Generated by tools/sim/vcompare.py — do not edit.
#include "Vvtop.h"
#include "verilated.h"
#include "verilated_vpi.h"
#include "verilated_vcd_c.h"
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <random>
#include <string>
#include <vector>

static Vvtop* top;
static VerilatedContext* ctx;
static const int NIN = @NIN@;
static const int PINS[NIN] = {@PINS@};
static int in_v[NIN];
static int pin_index(int p) { for (int i = 0; i < NIN; i++) if (PINS[i] == p) return i; return -1; }
#define PIN(p) (in_v[pin_index(p)])
static void apply_inputs() {
@APPLY@
}
static const int NCMP = @NCMP@;
static const int CMP[NCMP > 0 ? NCMP : 1] = {@CMP@};
static void outputs(int* go, int* gv, int* to, int* tv) {
@OUTS@
}
static bool assume_ok() { return @ASSUME@; }
static const int NCLK = @NCLK@;
static const int CLK[NCLK > 0 ? NCLK : 1] = {@CLK@};
static const int NRST = @NRST@;           // reset sequence: pin, value (2 = toggle), length
static const int RST[NRST > 0 ? NRST : 1][3] = {@RST@};

// registers: VPI handles for state reading/injection
struct Reg { const char* path; int bit; vpiHandle h; int width; };
static std::vector<Reg> regs = { @REGS@ };
static int reg_get(Reg& r) {
  s_vpi_value v; v.format = vpiBinStrVal; vpi_get_value(r.h, &v);
  int n = strlen(v.value.str); int idx = n - 1 - r.bit; return (idx >= 0 && v.value.str[idx] == '1') ? 1 : 0;
}
static void reg_set(Reg& r, int b) {
  s_vpi_value v; v.format = vpiBinStrVal; vpi_get_value(r.h, &v);
  std::string s(v.value.str); int idx = (int)s.size() - 1 - r.bit;
  if (idx < 0) return;
  s[idx] = b ? '1' : '0';
  s_vpi_value w; w.format = vpiBinStrVal; w.value.str = (PLI_BYTE8*)s.c_str();
  vpi_put_value(r.h, &w, nullptr, vpiNoDelay);
}
// correspondence classes: gold reg index, then (gate reg index, polarity) list
struct Cls { int g; std::vector<std::pair<int,int>> t; };
static std::vector<Cls> classes = { @CLASSES@ };

struct Rec { std::vector<int> in, go, gv, to, tv; };
static void vcd_pins(const char* path, const std::vector<Rec>& recs, long first_step) {
  FILE* f = fopen(path, "w"); if (!f) return;
  fprintf(f, "$timescale 1ns $end\n$scope module pins $end\n");
  int id = 0;
  for (int i = 0; i < NIN; i++) fprintf(f, "$var wire 1 i%d p%d_in $end\n", i, PINS[i]);
  for (int j = 0; j < NCMP; j++) fprintf(f, "$var wire 1 a%d src_p%d_oe $end\n$var wire 1 b%d src_p%d_out $end\n$var wire 1 c%d jed_p%d_oe $end\n$var wire 1 d%d jed_p%d_out $end\n", j, CMP[j], j, CMP[j], j, CMP[j], j, CMP[j]);
  fprintf(f, "$upscope $end\n$enddefinitions $end\n");
  (void)id;
  for (size_t k = 0; k < recs.size(); k++) {
    fprintf(f, "#%ld\n", (first_step + (long)k) * 10);
    for (int i = 0; i < NIN; i++) fprintf(f, "%di%d\n", recs[k].in[i], i);
    for (int j = 0; j < NCMP; j++) fprintf(f, "%da%d\n%db%d\n%dc%d\n%dd%d\n", recs[k].go[j], j, recs[k].gv[j], j, recs[k].to[j], j, recs[k].tv[j], j);
  }
  fclose(f);
}

int main(int argc, char** argv) {
  // argv: mode seed max_cycles seconds inject_every random_len burst_len window outdir
  const char* mode = argv[1];
  unsigned long long seed = strtoull(argv[2], 0, 10);
  long long max_cycles = strtoll(argv[3], 0, 10);
  double seconds = atof(argv[4]);
  long inject_every = atol(argv[5]);
  long random_len = atol(argv[6]), burst_len = atol(argv[7]);
  long window = atol(argv[8]);
  std::string outdir = argv[9];
  ctx = new VerilatedContext; ctx->traceEverOn(true);
  // argv[10]: register initialisation for X (0 = zero, 2 = random from the seed: --init free)
  std::string seedarg = std::string("+verilator+seed+") + std::to_string(seed % 1000000 + 1);
  std::string rr = std::string("+verilator+rand+reset+") + (argc > 10 ? argv[10] : "0");
  const char* vargs[] = {"vsim", seedarg.c_str(), rr.c_str()};
  ctx->commandArgs(3, vargs);
  top = new Vvtop{ctx};
  VerilatedVcdC* tfp = new VerilatedVcdC; top->trace(tfp, 99); bool tracing = false;
  for (int i = 0; i < NIN; i++) in_v[i] = 0;
  apply_inputs(); top->latch_hold = 0; top->eval();
  VerilatedVpi::callValueCbs();
  for (auto& r : regs) { r.h = vpi_handle_by_name((PLI_BYTE8*)r.path, nullptr);
    if (!r.h) { fprintf(stderr, "vpi: no handle for %s\n", r.path); return 3; } }
  std::mt19937_64 rng(seed);
  auto t0 = std::chrono::steady_clock::now();
  auto elapsed = [&]() { return std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count(); };
  FILE* cal = nullptr;
  if (!strcmp(mode, "calib")) cal = fopen((outdir + "/calib.txt").c_str(), "w");
  long rlen = 0; for (int k = 0; k < NRST; k++) if (RST[k][2] > rlen) rlen = RST[k][2];
  std::vector<int> go(NCMP), gv(NCMP), to(NCMP), tv(NCMP);
  std::vector<Rec> ring; size_t ring_cap = window > 0 ? window : 1;
  long long step = 0, injections = 0, mism = -1; int mism_pin = 0, mism_inj = 0; std::string mism_what;
  long long last_inject = -1; const char* reason = "cycles";
  long phase_pos = 0; int in_burst = 0; int clk_rr = 0;
  for (;;) {
    if (step >= max_cycles) { reason = "cycles"; break; }
    if ((step & 1023) == 0 && elapsed() > seconds) { reason = "time"; break; }
    // ---- next inputs
    if (step < rlen) {
      for (int k = 0; k < NRST; k++) if (step < RST[k][2]) { int i = pin_index(RST[k][0]); if (i >= 0) in_v[i] = RST[k][1] == 2 ? (int)(step & 1) : RST[k][1]; }
    } else {
      if (++phase_pos >= (in_burst ? burst_len : random_len)) { phase_pos = 0; in_burst = (NCLK > 0 && burst_len > 0) ? !in_burst : 0; }
      if (in_burst) {
        int i = pin_index(CLK[clk_rr % NCLK]); if ((step & 1) == 1) clk_rr++;
        int old = in_v[i]; in_v[i] ^= 1; if (!assume_ok()) in_v[i] = old;
      } else {
        for (int tries = 0; tries < 50; tries++) {
          int i = (int)(rng() % NIN); in_v[i] ^= 1;
          if (assume_ok()) break;
          in_v[i] ^= 1;
        }
      }
    }
    // two phases per step (as atf15xx-sim): latches held while flip-flops sample, then open
    apply_inputs(); top->latch_hold = 1; top->eval(); top->latch_hold = 0; top->eval(); VerilatedVpi::callValueCbs();
    // ---- injection
    bool injected_now = false;
    if (inject_every > 0 && step >= rlen && step > 0 && step % inject_every == 0 && !classes.empty() && mism < 0) {
      // state mix: uniform, mostly ones, or mostly zeros (counters and shift registers need long
      // runs of equal bits to carry; a uniform state almost never has them)
      int kind = (int)(rng() % 3);
      for (auto& c : classes) {
        int b = kind == 0 ? (int)(rng() & 1) : kind == 1 ? (rng() % 16 != 0) : (rng() % 16 == 0);
        reg_set(regs[c.g], b); for (auto& t : c.t) reg_set(regs[t.first], b ^ t.second);
      }
      top->eval(); VerilatedVpi::callValueCbs(); injections++; last_inject = step; injected_now = true;
    }
    (void)injected_now;
    outputs(go.data(), gv.data(), to.data(), tv.data());
    if (cal) { for (auto& r : regs) fputc('0' + reg_get(r), cal); fputc('\n', cal); }
    Rec rec; rec.in.assign(in_v, in_v + NIN); rec.go = go; rec.gv = gv; rec.to = to; rec.tv = tv;
    if (mism < 0) { ring.push_back(rec); if (ring.size() > ring_cap) ring.erase(ring.begin()); }
    else ring.push_back(rec);
    if (tracing) tfp->dump((uint64_t)step * 10);
    if (mism < 0) {
      for (int j = 0; j < NCMP; j++) if (go[j] != to[j] || gv[j] != tv[j]) {
        mism = step; mism_pin = CMP[j]; mism_inj = last_inject >= 0;
        char buf[160]; snprintf(buf, sizeof buf, "source %s jed %s", go[j] ? (gv[j] ? "1" : "0") : "Z", to[j] ? (tv[j] ? "1" : "0") : "Z");
        mism_what = buf;
        tfp->open((outdir + "/mismatch_full.vcd").c_str()); tracing = true; tfp->dump((uint64_t)step * 10);
        break;
      }
    } else if (step >= mism + window) { reason = "mismatch"; break; }
    step++;
  }
  if (tracing) tfp->close();
  if (mism >= 0) {
    long long first = mism - (long long)ring_cap + 1; if (first < 0) first = 0;
    vcd_pins((outdir + "/mismatch_pins.vcd").c_str(), ring, first);
  }
  if (cal) fclose(cal);
  printf("{\"cycles\": %lld, \"injections\": %lld, \"seconds\": %.3f, \"stop\": \"%s\", \"mismatch_step\": %lld, \"mismatch_pin\": %d, \"mismatch_after_injection\": %d, \"mismatch\": \"%s\", \"last_injection\": %lld}\n",
         step, injections, elapsed(), reason, mism, mism_pin, mism_inj, mism_what.c_str(), last_inject);
  top->final(); delete top; delete ctx;
  return 0;
}
'''


def write_cpp(work, pins_in, cmp, assume, clocks, reset, regs, classes):
    apply = '\n'.join('  top->p%d_in = in_v[%d];' % (p, i) for i, p in enumerate(pins_in))
    outs = '\n'.join('  go[%d] = top->g_p%d_oe; gv[%d] = top->g_p%d_out; to[%d] = top->t_p%d_oe; tv[%d] = top->t_p%d_out;'
                     % (j, p, j, p, j, p, j, p) for j, p in enumerate(cmp))
    rst = ', '.join('{%d, %d, %d}' % (p, 2 if v == 'clk' else v, k) for p, v, k in reset) or '{0, 0, 0}'
    regs_c = ', '.join('{"%s", %d, nullptr, %d}' % (path, bit, w) for path, bit, w in regs)
    cls_c = ', '.join('{%d, {%s}}' % (g, ', '.join('{%d, %d}' % t for t in ts)) for g, ts in classes)
    src = CPP
    for k, v in (('@NIN@', str(len(pins_in))), ('@PINS@', ', '.join(map(str, pins_in)) or '0'), ('@APPLY@', apply),
                 ('@NCMP@', str(len(cmp))), ('@CMP@', ', '.join(map(str, cmp)) or '0'), ('@OUTS@', outs),
                 ('@ASSUME@', '(%s)' % cpp_expr(assume) if assume else 'true'),
                 ('@NCLK@', str(len(clocks))), ('@CLK@', ', '.join(map(str, clocks)) or '0'),
                 ('@NRST@', str(len(reset))), ('@RST@', rst), ('@REGS@', regs_c), ('@CLASSES@', cls_c)):
        src = src.replace(k, v)
    open(os.path.join(work, 'harness.cpp'), 'w').write(src)


def verilate(work, timeout, x_initial):
    cmd = ['verilator', '--cc', '--exe', '--build', '-j', '0', '--vpi', '--public-flat-rw', '--trace',
           '-Wno-fatal', '-Wno-lint', '-Wno-style', '-Wno-COMBDLY', '-Wno-UNOPTFLAT', '-Wno-MULTIDRIVEN',
           '--x-assign', '0', '--x-initial', x_initial, '--top-module', 'vtop',
           '-CFLAGS', '-O1', '-Mdir', 'obj', '-o', 'vsim', 'vtop.v', 'gold.v', 'gate.v', 'harness.cpp']
    t = time.time()
    try:
        env = dict(os.environ, OBJCACHE='')     # no ccache in the image
        r = subprocess.run(cmd, cwd=work, capture_output=True, text=True, timeout=max(timeout, 1), env=env)
    except subprocess.TimeoutExpired:
        return False, 'Verilator compile exceeded its %.0f s budget' % timeout, time.time() - t
    open(os.path.join(work, 'verilator.log'), 'w').write(r.stdout + r.stderr)
    if r.returncode != 0:
        err = [l for l in (r.stdout + r.stderr).splitlines() if '%Error' in l][:5]
        return False, 'Verilator failed: %s' % ('; '.join(err) or 'see verilator.log'), time.time() - t
    return True, 'compiled', time.time() - t


def run_sim(work, mode, seed, cycles, seconds, inject_every, random_len, burst_len, window, rand_reset=0):
    work = os.path.abspath(work)
    cmd = [os.path.join(work, 'obj', 'vsim'), mode, str(seed), str(cycles), '%.3f' % max(seconds, 0.1),
           str(inject_every), str(random_len), str(burst_len), str(window), work, str(rand_reset)]
    try:
        r = subprocess.run(cmd, cwd=work, capture_output=True, text=True, timeout=seconds + 15)
    except subprocess.TimeoutExpired:
        return None, 'simulation did not return within its budget (oscillating combinational loop?)'
    line = [l for l in r.stdout.splitlines() if l.startswith('{')]
    if r.returncode != 0 or not line:
        msg = (r.stderr or r.stdout).strip().splitlines()
        conv = [l for l in msg if 'converge' in l.lower()]
        return None, ('combinational logic did not converge (loop): ' + conv[0]) if conv else ('simulation failed: ' + ' | '.join(msg[-3:]))
    return json.loads(line[-1]), None


# ------------------------------------------------------------------ correspondence

def map_pairs(map_path):
    """[(source net, mc, polarity)] from an atf15xx-map/1 file (docs/architecture/fitter/05-vendor-outputs.md
    §F5.4): every registered cell; polarity 1 when Q is the complement of the source register."""
    m = json.load(open(map_path))
    if m.get('format') != 'atf15xx-map/1':
        return []
    out = []
    for c in m.get('cells', []):
        if c.get('registered') and c.get('mc'):
            pol = 0 if c.get('q_is_value', True) else 1
            out.append((c.get('source') or c['name'], int(c['mc']), pol, c['name']))
    return out


def build_regs(gold_v, gate_v, kind, fit_text, map_path=None):
    """VPI register list and candidate classes: from the .map.json (primary), else .fit names."""
    regs, gold_bits, gate_idx = [], [], {}
    for name, w, lo in gold_regs(gold_v):
        for b in range(w):
            regs.append(('TOP.vtop.g.%s' % name, b, w))
            gold_bits.append((len(regs) - 1, name, b + lo, w))
    for mc, used in gate_regs(gate_v):
        if used:
            regs.append(('TOP.vtop.t.q_%d' % mc, 0, 1))
            gate_idx[mc] = len(regs) - 1
    nodes = fit_nodes(fit_text) if fit_text else {}
    named = {}
    aliases = gold_aliases(gold_v)
    if map_path:
        by_src = {}
        for src, mc, pol, nm in map_pairs(map_path):
            by_src[src] = (mc, pol)
            by_src.setdefault(equiv_sanitize(src), (mc, pol))
            by_src.setdefault(nm, (mc, pol))
        for gi, name, bit, w in gold_bits:
            keys = ['%s[%d]' % (name, bit), name] if w > 1 or kind == 'verilog' else [name]
            keys = aliases.get('%s[%d]' % (name, bit) if w > 1 else name, []) + keys
            if kind == 'tt2' and name.startswith('q_n_'):
                keys = [name[4:]]
            for k in keys + [gold_bit_name(name, bit, w, kind) or '']:
                if k in by_src and by_src[k][0] in gate_idx:
                    named[gi] = [(gate_idx[by_src[k][0]], by_src[k][1])]
                    break
        if named:
            return regs, gold_bits, gate_idx, named
    for gi, name, bit, w in gold_bits:
        n = gold_bit_name(name, bit, w, kind)
        if kind == 'jed':
            m = re.fullmatch(r'q_(\d+)', name)
            if m and int(m.group(1)) in gate_idx:
                named[gi] = [(gate_idx[int(m.group(1))], None)]
            continue
        if n and n in nodes and nodes[n] in gate_idx:
            named[gi] = [(gate_idx[nodes[n]], None)]
    return regs, gold_bits, gate_idx, named


def calibrate(work, regs, gold_bits, gate_idx, named):
    """Use the calibration trace (power-up, no injection) to fix polarity and match the rest."""
    rows = [l.strip() for l in open(os.path.join(work, 'calib.txt')) if l.strip()]
    if not rows:
        return [], 'no calibration trace'
    col = lambda i: ''.join(r[i] for r in rows)
    inv = lambda s: s.translate(str.maketrans('01', '10'))
    classes, used_t, report = [], set(), []
    g_unmatched, conflicts = [], 0
    for gi, name, bit, w in gold_bits:
        gs = col(gi)
        ts = []
        for ti, pol in named.get(gi, []):
            t = col(ti)
            if t == gs and pol in (None, 0):
                ts.append((ti, 0))
            elif t == inv(gs) and pol in (None, 1):
                ts.append((ti, 1))
            else:
                conflicts += 1
        if ts:
            classes.append((gi, ts))
            used_t.update(t for t, _ in ts)
        else:
            g_unmatched.append(gi)
    # signature matching for the rest: a unique equal / complementary trace that toggles
    t_free = [ti for ti in gate_idx.values() if ti not in used_t]
    sig = {}
    for ti in t_free:
        sig.setdefault(col(ti), []).append((ti, 0))
        sig.setdefault(inv(col(ti)), []).append((ti, 1))
    still = []
    for gi in g_unmatched:
        gs = col(gi)
        cand = sig.get(gs, [])
        if '0' in gs and '1' in gs and cand and len({gs for _ in cand}) == 1:
            others = [g for g in g_unmatched if g != gi and col(g) in (gs, inv(gs))]
            if not others:
                classes.append((gi, cand))
                used_t.update(t for t, _ in cand)
                continue
        still.append(gi)
    t_left = [ti for ti in gate_idx.values() if ti not in used_t]
    complete = not still and not t_left
    report = ('%d source register bits, %d JED registers: %d classes (%d by name), %d source bits and %d JED '
              'registers unmatched, %d name conflicts' % (len(gold_bits), len(gate_idx), len(classes), len(named),
                                                         len(still), len(t_left), conflicts))
    return (classes if complete else []), report + ('' if complete else ' -> injection disabled')


# ------------------------------------------------------------------ main entry (also used by equiv.py)

def vcompare(a, work, budget):
    """Run the Verilator comparison; returns (status, message, details dict)."""
    os.makedirs(work, exist_ok=True)
    B = Budget(budget)
    model_args = (['--preset-wins'] if a.preset_wins else []) + (['--power-up', a.power_up] if a.power_up else []) + list(a.model_opt or [])
    gate_v = open(equiv.build_gate(os.path.abspath(a.jed), a.device, work, model_args)).read()
    if not os.path.exists(os.path.join(work, 'gold.v')) or not getattr(a, '_gold_ready', False):
        gold_v = open(equiv.build_gold(a, work, a.device, model_args)).read()
    else:
        gold_v = open(os.path.join(work, 'gold.v')).read()
    gi, go, ti, to, pins_in, cmp = equiv.compare_set(gold_v, gate_v, a.ignore_pin)
    dev = simlib.device(a.device)
    clocks = [p for p in (dev['gclk1'], dev['gclk2_oe2'], dev['gclk3']) if p in gi and p not in go]
    clocks += [c for c in (a.clock or []) if c in pins_in and c not in clocks]
    reset = list(a._reset)
    kind = 'tt2' if a.tt2 else 'jed' if a.gold_jed else 'verilog'
    fit_text = open(a.fit, errors='replace').read() if a.fit and os.path.exists(a.fit) else ''
    mp = a.map or (os.path.splitext(a.jed)[0] + '.map.json')
    mp = mp if os.path.exists(mp) else None
    regs, gold_bits, gate_idx, named = build_regs(gold_v, gate_v, kind, fit_text, mp)
    d_src = 'map ' + os.path.relpath(mp, ROOT) if mp else ('fit report' if fit_text else 'signatures only')
    gate_h, hold_t = add_latch_hold(gate_v, 'gate')
    gold_h, hold_g = add_latch_hold(gold_v, 'gold') if kind != 'verilog' else (gold_v, False)
    open(os.path.join(work, 'gate.v'), 'w').write(gate_h)
    open(os.path.join(work, 'gold.v'), 'w').write(gold_h)
    write_top(work, gi, go, ti, to, pins_in, cmp, hold_g, hold_t)
    d = {'pins_compared': len(cmp), 'clock_pins': clocks}
    # compile once with an empty class list for calibration, then again with the classes?  No:
    # classes are data in the C++; compile once with all candidates written as a runtime file
    # would need parsing — simpler: two compiles of harness.cpp only (Verilator reuses the model).
    write_cpp(work, pins_in, cmp, a.assume, clocks, reset, regs, [])
    ok, msg, t = verilate(work, min(a.compile_budget, B.left() - 1), 'unique' if a.init == 'free' else '0')
    d['compile_seconds'] = round(t, 1)
    if not ok:
        return ('TIMEOUT' if 'budget' in msg else 'ERROR'), msg, d
    classes, corr = [], 'injection off'
    if a.inject_every > 0 and B.left() > 3:
        res, err = run_sim(work, 'calib', a.seed, a.calib_cycles, min(B.left() * 0.15, 20), 0,
                           a.random_len, a.burst_len, 1)
        if res:
            classes, corr = calibrate(work, regs, gold_bits, gate_idx, named)
            if res.get('mismatch_step', -1) >= 0:
                classes = []
        else:
            corr = 'calibration failed: %s' % err
        if classes:
            write_cpp(work, pins_in, cmp, a.assume, clocks, reset, regs, classes)
            ok, msg, t2 = verilate(work, min(a.compile_budget, B.left() - 1), 'unique' if a.init == 'free' else '0')
            d['compile_seconds'] = round(t + t2, 1)
            if not ok:
                return ('TIMEOUT' if 'budget' in msg else 'ERROR'), msg, d
    d['correspondence'] = corr + ' [names: %s]' % d_src
    d['classes'] = len(classes)
    left = B.left() - 1
    if left <= 0.5:
        return 'TIMEOUT', 'no time left to simulate after compiling (%.1f s used)' % B.used(), d
    res, err = run_sim(work, 'run', a.seed, a.cycles, left, a.inject_every if classes else 0,
                       a.random_len, a.burst_len, a.window, 2 if a.init == 'free' else 0)
    if res is None:
        return 'ERROR', err, d
    d.update(res)
    rlen = max([k for _, _, k in reset] + [0, a.skip])
    if res['mismatch_step'] >= 0 and res['mismatch_step'] >= rlen:
        where = 'from injected state' if res['mismatch_after_injection'] else 'from power-up'
        return 'FAIL', 'step %d pin %d: %s (%s; after %d cycles + %d injections); VCD %s, %s' % (
            res['mismatch_step'], res['mismatch_pin'], res['mismatch'], where, res['cycles'], res['injections'],
            os.path.relpath(os.path.join(work, 'mismatch_pins.vcd'), ROOT),
            os.path.relpath(os.path.join(work, 'mismatch_full.vcd'), ROOT)), d
    return 'SIMULATED', '%d cycles + %d injections in %.1f s (stopped by %s budget; %s)' % (
        res['cycles'], res['injections'], res['seconds'], res['stop'], corr), d


def add_args(ap, standalone=True):
    g = ap.add_argument_group('Verilator engine (vcompare)')
    g.add_argument('--cycles', type=int, default=50_000_000, help='cycle budget (default 5e7)')
    g.add_argument('--compile-budget', type=float, default=40.0, help='Verilator compile budget, s (default 40)')
    g.add_argument('--inject-every', type=int, default=997, help='state injection period in steps (0 = off; default 997)')
    g.add_argument('--calib-cycles', type=int, default=20000, help='calibration steps for the correspondence')
    g.add_argument('--random-len', type=int, default=200, help='steps per random phase (default 200)')
    g.add_argument('--burst-len', type=int, default=400, help='steps per clock burst (default 400; 0 = none)')
    g.add_argument('--window', type=int, default=64, help='VCD window around the first mismatch, steps')
    g.add_argument('--clock', type=int, action='append', default=[], help='extra clock pin for bursts')
    g.add_argument('--map', help='atf15xx-map/1 placement map (default: <jed stem>.map.json if present)')


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0], epilog=__doc__.split('\n', 2)[2],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('jed')
    ap.add_argument('--device', required=True)
    ap.add_argument('--tt2')
    ap.add_argument('--verilog', nargs='+')
    ap.add_argument('--gold-jed')
    ap.add_argument('--top')
    ap.add_argument('--fit')
    ap.add_argument('--pin', action='append')
    ap.add_argument('--pin-file')
    ap.add_argument('--open-collector', default='')
    ap.add_argument('--init', choices=['zero', 'free'], default='zero')
    ap.add_argument('--reset', action='append', default=[])
    ap.add_argument('--reset-clock', action='append', default=[], type=int)
    ap.add_argument('--assume')
    ap.add_argument('--ignore-pin', action='append', type=int, default=[])
    ap.add_argument('--skip', type=int, default=0)
    ap.add_argument('--preset-wins', action='store_true')
    ap.add_argument('--power-up', choices=['0', '1', 'fuse'])
    ap.add_argument('--model-opt', action='append', default=[])
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--budget', type=float, default=60.0, help='total wall-clock budget, s (default 60)')
    ap.add_argument('--work')
    ap.add_argument('--json')
    add_args(ap)
    a = ap.parse_args()
    equiv.normalise_args(a, ap)
    work = a.work or os.path.join(ROOT, '.scratch', 'sim', 'vcompare', os.path.splitext(os.path.basename(a.jed))[0])
    st, msg, d = vcompare(a, work, a.budget)
    print('vcompare %s (%s): %s  %s' % (a.jed, a.device, st, msg))
    if a.json:
        json.dump(dict(d, status=st, message=msg), open(a.json, 'w'), indent=1)
    sys.exit(1 if st == 'FAIL' else 0 if st == 'SIMULATED' else 2)


if __name__ == '__main__':
    main()
