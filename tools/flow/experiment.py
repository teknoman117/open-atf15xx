"""Parameter study of the open flow: the Yosys logic mapping (--map), --max-products and toggle
recovery, measured as fit success and macrocells used on every test design.
  tools/sim/run.sh python3 tools/flow/experiment.py [--parts P ...] [--out .scratch/flow/exp]
Writes <out>/experiment.md. No equivalence checking (corpus.py does that for the defaults).
"""
import argparse, glob, os, re, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
sys.path.insert(0, os.path.join(ROOT, 'tools', 'sim'))
from simlib import cargo_bin  # noqa: E402
DESIGNS = os.path.join(ROOT, 'atf15xx-synth', 'tests', 'designs')


def synth(flow, name, mode, out):
    os.makedirs(out, exist_ok=True)
    ys = subprocess.run([flow, 'script', '--map', mode], capture_output=True, text=True, check=True).stdout
    open(os.path.join(out, 's.ys'), 'w').write(ys)
    j = os.path.join(out, 'n.json')
    if not os.path.exists(j):
        subprocess.run(['yosys', '-q', '-p', 'read_verilog -sv %s; hierarchy -check -top %s; script %s; write_json -compat-int %s'
                        % (os.path.join(DESIGNS, name + '.v'), name, os.path.join(out, 's.ys'), j)], check=True,
                       capture_output=True)
    return j


def fit(flow, j, part, extra, out):
    r = subprocess.run([flow, 'fit-json', j, '-d', part, '-o', out, '--no-svf', '--init', 'ignore'] + extra,
                       capture_output=True, text=True)
    m = re.search(r"— (\d+) of \d+ macrocells used", r.stderr)
    if r.returncode or not m:
        return 'no'
    return m.group(1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--parts', nargs='*', default=['ATF1502AS-PLCC44', 'ATF1504AS-PLCC44', 'ATF1508AS-PLCC84'])
    ap.add_argument('--out', default=os.path.join(ROOT, '.scratch', 'flow', 'exp'))
    ap.add_argument('--abc-sop', action='store_true', help='also abc -sop (Yosys <= 0.68 only)')
    a = ap.parse_args()
    flow = cargo_bin('atf15xx-synth', 'atf15xx-synth')
    names = sorted(os.path.basename(f)[:-2] for f in glob.glob(os.path.join(DESIGNS, '*.v')))
    maps = ['none', 'abc-gates'] + (['abc-sop'] if a.abc_sop else [])
    variants = [('none', 'none', []), ('none noT', 'none', ['--no-toggles']), ('none mp=5', 'none', ['--max-products', '5']),
                ('none mp=20', 'none', ['--max-products', '20']), ('abc-gates', 'abc-gates', [])]
    if a.abc_sop:
        variants.append(('abc-sop P80', 'abc-sop', []))
    for n in names:
        for mode in maps:
            synth(flow, n, mode, os.path.join(a.out, n, mode))
    jobs = [(n, p, v) for n in names for p in a.parts for v in variants]
    def run(job):
        n, p, (label, mode, extra) = job
        return (n, p, label, fit(flow, os.path.join(a.out, n, mode, 'n.json'), p, extra,
                                 os.path.join(a.out, n, mode, re.sub(r'\W+', '_', label + p))))
    with ThreadPoolExecutor(8) as ex:
        res = list(ex.map(run, jobs))
    labels = [v[0] for v in variants]
    rows = ['| design | part | ' + ' | '.join(labels) + ' |', '|---|---|' + '---|' * len(labels)]
    tab = {(n, p, l): x for n, p, l, x in res}
    for n in names:
        for p in a.parts:
            rows.append('| %s | %s | %s |' % (n, p, ' | '.join(tab[(n, p, l)] for l in labels)))
    open(os.path.join(a.out, 'experiment.md'), 'w').write('\n'.join(rows) + '\n')
    print('\n'.join(rows))


if __name__ == '__main__':
    main()
