"""Run the open Verilog flow (atf15xx-synth) over the test designs and check every JED against its
Verilog source with the simulator's equivalence checker (tools/sim/equiv.py: Yosys + ABC PDR
formal proof and iverilog random simulation).
  tools/sim/run.sh python3 tools/flow/corpus.py [-j 4] [--only NAME ...] [--device PART ...]
Designs: atf15xx-synth/tests/designs/NAME.v (top module NAME; optional NAME.acf
constraints, NAME.equiv = extra equiv.py arguments, one per line, NAME.parts = the parts to
try instead of the default 1502/1504/1508 list, one per line). Work directory
.scratch/flow/corpus/NAME-PART/. Writes .scratch/flow/corpus/results.{md,json}.
Exit status 1 if any design that fits fails equivalence.
Two stages, so that the build can use the pinned Yosys image and the check the simulator image:
  docker/yosys/run.sh python3 tools/flow/corpus.py --stage build
  tools/sim/run.sh python3 tools/flow/corpus.py --stage check
"""
import argparse, glob, json, os, re, subprocess, sys, time
from concurrent.futures import ThreadPoolExecutor
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
sys.path.insert(0, os.path.join(ROOT, 'tools', 'sim'))
from simlib import cargo_bin, sim_bin  # noqa: E402

DESIGNS = os.path.join(ROOT, 'atf15xx-synth', 'tests', 'designs')
PARTS = ['ATF1502AS-PLCC44', 'ATF1504AS-PLCC44', 'ATF1508AS-PLCC84']
STAMP = 'Mon Nov 24 10:00:00 2026'


EXTRA = {}
EQ_TIMEOUT = 60  # seconds for PDR (then bmc3 depth 30); see --eq-timeout


def run_one(flow, name, part, work, extra_flow, stage='all'):
    os.makedirs(work, exist_ok=True)
    src = EXTRA.get(name, os.path.join(DESIGNS, name + '.v'))
    state = os.path.join(work, 'build.json')
    if stage == 'check':
        if not os.path.exists(state):
            return {'design': name, 'part': part, 'fit': False, 'error': 'not built'}
        res = json.load(open(state))
        if not res['fit']:
            return res
        return check_one(res, name, work, src)
    cmd = [flow, 'build', src, '--top', name, '-d', part, '-o', work, '--timestamp', STAMP, '--emit-tt2']
    acf = os.path.join(DESIGNS, name + '.acf')
    if os.path.exists(acf):
        cmd += ['-c', acf]
    cmd += extra_flow
    t0 = time.time()
    r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
    res = {'design': name, 'part': part, 'work': os.path.relpath(work, ROOT), 'fit': r.returncode == 0,
           'flow_log': r.stderr.strip(), 'flow_s': round(time.time() - t0, 1)}
    if r.returncode != 0:
        res['error'] = (r.stderr.strip().splitlines() or ['?'])[-1]
        json.dump(res, open(state, 'w'))
        return res
    m = re.search(r'— (\d+) of \d+ macrocells used, (.*?); Yosys', r.stderr)
    if m:
        res['cells'], res['strategy'] = int(m.group(1)), m.group(2)
    pins = json.load(open(os.path.join(work, name + '.pins.json')))
    fit_txt = open(os.path.join(work, name + '.fit')).read()
    m = re.search(r'Total product terms used\D*(\d+)', fit_txt)
    res['pts'] = int(m.group(1)) if m else None
    res['svf'] = os.path.exists(os.path.join(work, name + '.svf'))
    m = re.search(r'Yosys (\d+\.\d+)', open(os.path.join(work, 'yosys.log'), errors='replace').read()) if os.path.exists(os.path.join(work, 'yosys.log')) else None
    res['yosys'] = m.group(1) if m else '?'
    json.dump(res, open(state, 'w'))
    if stage == 'build':
        res['result'] = 'built'
        res['formal'] = res['sim'] = '-'
        return res
    return check_one(res, name, work, src)


def check_one(res, name, work, src):
    pins = json.load(open(os.path.join(work, name + '.pins.json')))
    eq = [sys.executable, os.path.join(ROOT, 'tools', 'sim', 'equiv.py'), os.path.join(work, name + '.jed'),
          '--device', pins['device'], '--verilog', src, '--top', name, '--fit', os.path.join(work, name + '.fit'),
          '--work', os.path.join(work, 'equiv'), '--timeout', str(EQ_TIMEOUT), '--depth', '30']
    ef = os.path.join(DESIGNS, name + '.equiv')
    if os.path.exists(ef):
        eq += [a for a in open(ef).read().split('\n') if a.strip() and not a.startswith('#')]
    t0 = time.time()
    try:
        # hard wall-clock limit per check (formal PDR/BMC limit + simulation + slack)
        e = subprocess.run(eq, cwd=ROOT, capture_output=True, text=True, env=dict(os.environ, SIM_NO_BUILD='1'),
                           timeout=2 * EQ_TIMEOUT + 60)
        out = e.stdout + e.stderr
    except subprocess.TimeoutExpired as ex:
        out = (ex.stdout or b'').decode(errors='replace') if isinstance(ex.stdout, bytes) else (ex.stdout or '')
        out += '\nformal: TIMEOUT\nsim: -\nRESULT: TIMEOUT'
    res['equiv_s'] = round(time.time() - t0, 1)
    res['equiv_log'] = out.strip()
    m = re.search(r'formal:\s+(\S+)', out)
    res['formal'] = m.group(1) if m else '-'
    m = re.search(r'sim:\s+(\S+)', out)
    res['sim'] = m.group(1) if m else '-'
    m = re.search(r'RESULT:\s+(\S+)', out)
    res['result'] = m.group(1) if m else 'ERROR'
    return res


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('-j', '--jobs', type=int, default=4)
    ap.add_argument('--only', nargs='*')
    ap.add_argument('--device', nargs='*', default=PARTS)
    ap.add_argument('--out', default=os.path.join(ROOT, '.scratch', 'flow', 'corpus'))
    ap.add_argument('--stage', choices=['all', 'build', 'check'], default='all')
    ap.add_argument('--eq-timeout', type=int, default=60,
                    help='formal time limit per design (s); the whole check is killed after 2x+60 s')
    ap.add_argument('--extra', action='append', default=[], metavar='FILE.v=PART[,PART]',
                    help='also run an external design (top module = file name), e.g. a copy of a user design under .scratch/')
    ap.add_argument('flow_args', nargs='*', help='extra atf15xx-synth arguments (after --)')
    a = ap.parse_args()
    global EQ_TIMEOUT
    EQ_TIMEOUT = a.eq_timeout
    flow = cargo_bin('atf15xx-synth', 'atf15xx-synth') if a.stage != 'check' else None
    if a.stage != 'build':
        sim_bin()
    names = sorted(os.path.basename(f)[:-2] for f in glob.glob(os.path.join(DESIGNS, '*.v')))
    if a.only:
        names = [n for n in names if n in a.only]
    def parts(n):
        f = os.path.join(DESIGNS, n + '.parts')
        if os.path.exists(f) and a.device == PARTS:
            return [l.strip() for l in open(f) if l.strip() and not l.startswith('#')]
        return a.device
    jobs = [(n, p) for n in names for p in parts(n)]
    for e in a.extra:
        f, ps = e.split('=', 1)
        n = os.path.basename(f)[:-2]
        EXTRA[n] = os.path.abspath(f)
        jobs += [(n, p) for p in ps.split(',')]
    with ThreadPoolExecutor(a.jobs) as ex:
        results = list(ex.map(lambda j: run_one(flow, j[0], j[1], os.path.join(a.out, '%s-%s' % j), a.flow_args, a.stage), jobs))
    os.makedirs(a.out, exist_ok=True)
    json.dump(results, open(os.path.join(a.out, 'results.json'), 'w'), indent=1)
    rows = ['| design | part | fit | MCs | yosys | formal | sim | result |', '|---|---|---|---|---|---|---|---|']
    bad = 0
    for r in results:
        if not r['fit']:
            rows.append('| %s | %s | no: %s | | | | | |' % (r['design'], r['part'], r['error'].replace('|', '/')))
            continue
        bad += r['result'] not in ('PASS', 'built')  # BOUNDED formal + sim PASS is PASS (equiv.py); TIMEOUT counts as bad
        rows.append('| %s | %s | yes | %s | %s | %s | %s | %s |' % (r['design'], r['part'], r.get('cells', '?'),
                    r.get('yosys', '?'), r['formal'], r['sim'], r['result']))
    open(os.path.join(a.out, 'results.md'), 'w').write('\n'.join(rows) + '\n')
    print('\n'.join(rows))
    sys.exit(1 if bad else 0)


if __name__ == '__main__':
    main()
