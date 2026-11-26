"""Stress test of toolchain.prefetch (parallel fitters): N random 1508 designs of the 026-uim_mux
kind, fitted once sequentially (run_uncached) and once through prefetch with PRJBUREAU_JOBS
workers, must give identical fuse maps. Run in a scratch worktree with an EMPTY cache:

  docker/prjbureau/run.sh sh -c "cd /work/.worktrees/prjbureau/<scratch> && mkdir -p cache && \
      PYTHONPATH=$PWD python3 fuzzers/000-init/fuzzer.py && \
      N=2000 SEED=7 PRJBUREAU_JOBS=14 PYTHONPATH=$PWD python3 /work/tools/fuzzers/prjbureau/stress_prefetch.py"

Reference results (2026-11-25): with per-worker FITTERDIR copies 2000/2000 identical; with one
shared FITTERDIR (the bug this guards against) 399/400.
"""
import os, random, sys, time, shutil
from util import toolchain, database, root_dir
db = database.load()['ATF1508AS']
package, pinout = next(iter(db['pins'].items()))
bonded = [m for m, v in db['macrocells'].items() if v['pad'] in pinout]
rnd = random.Random(int(os.environ.get("SEED", "1")))
calls = []
for i in range(int(os.environ.get("N", "40"))):
    pads = rnd.sample([db['macrocells'][m]['pad'] for m in bonded[1:]], 2)
    fbs = rnd.sample(range(2, 129), 2)
    q = db['macrocells'][bonded[0]]['pad']
    v = (f"module top(output Q, {', '.join('input ' + p for p in pads)}); "
         + ' '.join(f"wire MC{f}; DFF fb_MC{f}(1'b0, 1'b0, MC{f});" for f in fbs)
         + f" wire Y; AND4 a({', '.join(pads)}, MC{fbs[0]}, MC{fbs[1]}, Y); DFF ff(1'b0, Y, Q); endmodule")
    pins = {'Q': pinout[q], **{p: pinout[p] for p in pads}, **{f'fb_MC{f}': str(600 + f) for f in fbs}}
    calls.append((v, pins, f'ATF1508AS-{package}', {}))
t = time.time()
seq = []
for c in calls:
    try: seq.append(toolchain.run_uncached(c[0], c[1], c[2]).tobytes())
    except Exception as e: seq.append(repr(type(e)))
t_seq = time.time() - t
t = time.time()
toolchain.prefetch(calls)
t_par = time.time() - t
par = []
for c in calls:
    try: par.append(toolchain.run(c[0], c[1], c[2]).tobytes())
    except Exception as e: par.append(repr(type(e)))
print(f'jobs {toolchain.jobs}: sequential {t_seq:.1f} s, parallel {t_par:.1f} s; identical {sum(a == b for a, b in zip(seq, par))}/{len(calls)}; failures {sum(isinstance(a, str) for a in seq)}')
