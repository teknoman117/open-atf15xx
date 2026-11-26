"""Silicon experiments on the ATF1508AS breakout (docs/worklog/06-silicon.md):

state: P4 = A (D flip-flop, D = R, Invert fuse 0), P5 = B (D = R # S, realised as Invert fuse 1 with
       PT1 = !R & !S); clock CK = P81 (GCLK3); no reset/preset. Read right after programming with
       every input held low (no clock edge), then check the logic, then set A = B = 1 and reprogram.
prio:  P4 = A (AR = PT(R), AP = PT(S)), P5 = B (AR = GCLR pin = S, AP = PT(R)), both D = own Q.
Lines: D4 = CK (P81), D5 = R (P80), D8 = S (P1, GCLR); D2 = P4, D3 = P5.
"""
import os, subprocess, sys
from bench import Bench

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
S = os.path.join(ROOT, '.scratch', 'silicon')


def program(name):
    r = subprocess.run([os.path.join(ROOT, 'tools/silicon/program.sh'), '%s/%s/%s.svf' % (S, name, name),
                        '%s/%s/%s.jed' % (S, name, name)], capture_output=True, text=True)
    ok = 'programmed successfully' in r.stdout + r.stderr and '0 errors' in r.stdout + r.stderr
    if not ok:
        print(r.stdout, r.stderr); sys.exit('programming %s failed' % name)


def outs(b):
    """(P4, P5), checking that both are driven (same level with and without the pull-up)."""
    a = b.read(); b.mode(2, 'u'); b.mode(3, 'u'); c = b.read(); b.mode(2, 'i'); b.mode(3, 'i')
    driven = a[2] == c[2] and a[3] == c[3]
    return (a[2], a[3]) if driven else ('Z', 'Z')


def setin(b, ck=None, r=None, s=None):
    if ck is not None: b.mode(4, str(ck))
    if r is not None: b.mode(5, str(r))
    if s is not None: b.mode(8, str(s))


def clock(b):
    b.pulse(4, 1, 20)


b = Bench()
setin(b, ck=0, r=0, s=0)
print('== state')
for trial in range(2):
    program('state')
    print(' after programming (no clock edge yet): P4(A,Invert=0), P5(B,Invert=1) =', outs(b))
    clock(b); print(' clock with R=0 S=0  -> expect (0,0):', outs(b))
    setin(b, r=1); clock(b); setin(b, r=0); print(' clock with R=1      -> expect (1,1):', outs(b))
    setin(b, s=1); clock(b); setin(b, s=0); print(' clock with S=1      -> expect (0,1):', outs(b))
    setin(b, r=1); clock(b); setin(b, r=0); print(' set both to 1 before reprogramming:', outs(b))
print('== prio')
program('prio')
print(' after programming: (A, B) =', outs(b))
seq = [('R', 1), ('R', 0), ('S', 1), ('S', 0),
       ('R', 1), ('S', 1), ('S', 0), ('R', 0),      # R first, then S on top
       ('S', 1), ('R', 1), ('R', 0), ('S', 0),      # S first, then R on top
       ('R', 1), ('S', 1), ('R', 0), ('S', 0)]
st = {'R': 0, 'S': 0}
for k, v in seq:
    st[k] = v; setin(b, **{k.lower(): v})
    print(' R=%d S=%d -> A(AR=R,AP=S)=%s  B(AR=GCLR=S,AP=R)=%s' % ((st['R'], st['S']) + outs(b)))
b.close()
