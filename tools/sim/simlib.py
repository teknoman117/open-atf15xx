"""Shared helpers for tools/sim/ (run inside the open-atf150x-sim container: tools/sim/run.sh)."""
import os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
TARGET = os.environ.get('CARGO_TARGET_DIR', os.path.join(ROOT, 'target'))
_built = {}


def cargo_bin(pkg, name):
    """Build `pkg` (release) once per process and return the path of binary `name`."""
    path = os.path.join(TARGET, 'release', name)
    if pkg not in _built and not os.environ.get('SIM_NO_BUILD'):
        r = subprocess.run(['cargo', 'build', '--release', '-q', '-p', pkg], cwd=ROOT)
        if r.returncode != 0:
            # a stale binary silently checks old code: fail unless explicitly allowed
            if not os.path.exists(path) or not os.environ.get('SIM_ALLOW_STALE'):
                raise SystemExit('cargo build -p %s failed (set SIM_ALLOW_STALE=1 to use an existing %s)' % (pkg, path))
            print('warning: cargo build -p %s failed; using the existing %s' % (pkg, path), file=sys.stderr)
        _built[pkg] = True
    return path


def sim_bin():
    return cargo_bin('atf15xx-sim', 'atf15xx-sim')


def fit_bin(family):
    # the fit1502/1504/1508 binaries belong to the atf15xx-fit package
    return cargo_bin('atf15xx-fit', 'fit' + family)


def stim_pins(dev):
    """Pins driven by the stimulus: every I/O pin plus the dedicated inputs."""
    return sorted({int(p) for p in dev['pins']} | {dev['gclk1'], dev['gclk2_oe2'], dev['gclr'], dev['oe1']})


def gen_stim(dev, steps, seed, single=False):
    """Random stimulus rows ({pin: 0/1}).

    Default ("burst"): every pin random each step, GCLK1 toggling every step, GCLK2 toggling every
    other step half of the time, GCLR pulsing 2 % of steps (mirrors atf15xx-sim compare).
    single=True: one input changes per step (GCLR rarely), so models cannot resolve races between
    simultaneous input changes differently; use this to compare models that differ in
    zero-delay ordering (iverilog)."""
    import random
    pins = stim_pins(dev)
    rnd = random.Random(seed)
    rows = []
    g2 = 0
    cur = {p: 0 for p in pins}
    for c in range(steps):
        if single:
            p = rnd.choice(pins)
            if p == dev['gclr'] and rnd.random() < 0.9:
                p = dev['gclk1']
            cur[p] ^= 1
            r = dict(cur)
        else:
            r = {p: rnd.randint(0, 1) for p in pins}
            r[dev['gclk1']] = c % 2
            if rnd.random() < 0.5:
                g2 = (c // 2) % 2
            r[dev['gclk2_oe2']] = g2
            r[dev['gclr']] = 1 if rnd.random() < 0.02 else 0
        rows.append(r)
    return pins, rows


def write_stim(path, pins, rows):
    """CSV for `atf15xx-sim run --stim`: header = pin numbers, one row per step."""
    with open(path, 'w') as f:
        f.write(','.join(str(p) for p in pins) + '\n')
        for r in rows:
            f.write(','.join(str(r[p]) for p in pins) + '\n')


def read_stim(path):
    import csv
    rows = list(csv.reader(open(path)))
    pins = [int(x) for x in rows[0]]
    return pins, [dict(zip(pins, map(int, r))) for r in rows[1:]]


_DEVICES = {}


def device(name):
    """The device record from `atf15xx-sim device` (the tables are atf15xx-arch's): a dict with
    `pins` and `input_only` ({str(pin): mc}), `gclk1`, `oe1`, `gclr`, `gclk2_oe2`, `gclk3`, `pd1`,
    `pd2`, `jtag` ([tdi, tms, tck, tdo]), `tt2_device`, `package`, `family`."""
    if name not in _DEVICES:
        out = subprocess.run([sim_bin(), 'device', name], capture_output=True, text=True, check=True).stdout
        d = {'pins': {}, 'input_only': {}}
        for line in out.splitlines():
            w = line.split()
            if not w:
                continue
            if w[0] == 'device':
                d.update(name=w[1], family=w[3], package=w[5], tt2_device=w[7])
            elif w[0] in ('io', 'input'):
                d['pins' if w[0] == 'io' else 'input_only'][w[1]] = int(w[2])
            elif w[0] == 'jtag':
                d['jtag'] = [int(x) for x in w[1:]]
            else:
                d[w[0]] = int(w[1])
        _DEVICES[name] = d
    return _DEVICES[name]


def sim_run(jed, devname, stim, extra=()):
    """Run `atf15xx-sim run --print`; return (header, rows as dicts)."""
    out = subprocess.run([sim_bin(), 'run', jed, '-d', devname, '--stim', stim, '--print'] + list(extra),
                         capture_output=True, text=True, check=True).stdout.splitlines()
    hdr = out[0].split(',')
    return hdr, [dict(zip(hdr, l.split(','))) for l in out[1:]]


def device_for(path, default=None):
    """Device for a JED: --device, else `-device` in a sibling .args file, else the .fit header."""
    import re
    if default:
        return default
    base = os.path.splitext(path)[0]
    if os.path.exists(base + '.args'):
        a = open(base + '.args').read().split()
        if '-device' in a:
            return a[a.index('-device') + 1]
    if os.path.exists(base + '.fit'):
        m = re.search(r'-device\s+(\S+)', open(base + '.fit', errors='replace').read())
        if m:
            return m.group(1)
    raise SystemExit('%s: no --device, .args or .fit file to take the device from' % path)
