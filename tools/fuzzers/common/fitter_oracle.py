"""Run the WinCUPL tools (cupl.exe, fit1502/1504/1508.exe) under wine as an oracle.

Library functions:
  compile_pld(pld_text, name, workdir)            -> path of the .tt2 produced by cupl.exe
  run_fitter(tt2_path, family, device, options, workdir) -> dict of output files (jed, fit, pin, io, tt3, stdout)
  parse_fit(fit_text)                              -> placement / UIM assignment / logic from a .fit report
  read_jed(jed_text)                               -> fuse string (JEDEC default applied)

Environment: WINEPREFIX (default ~/.wine32), WINCUPL (default C:\\Wincupl).
"""
import os, re, shutil, subprocess, tempfile

PREFIX = os.path.expanduser(os.environ.get('WINEPREFIX', '~/.wine32'))
WINCUPL = os.environ.get('WINCUPL', 'C:\\Wincupl')
ENV = dict(os.environ, WINEPREFIX=PREFIX, WINEARCH='win32', WINEDEBUG='-all')
FITTER = {'1502': 'fit1502.exe', '1504': 'fit1504.exe', '1508': 'fit1508.exe'}


def _wine(args, cwd, timeout=120):
    r = subprocess.run(['wine'] + args, cwd=cwd, env=ENV, capture_output=True, timeout=timeout)
    return r.returncode, r.stdout.decode('latin-1', 'replace')


def compile_pld(pld_text, name, workdir):
    os.makedirs(workdir, exist_ok=True)
    p = os.path.join(workdir, name + '.pld')
    with open(p, 'w', newline='\r\n') as f:
        f.write(pld_text)
    rc, out = _wine([WINCUPL + '\\Shared\\cupl.exe', '-uabm2', WINCUPL + '\\Shared\\cupl.dl', name + '.pld'], workdir)
    tt2 = os.path.join(workdir, name + '.tt2')
    if not os.path.exists(tt2):
        raise RuntimeError('cupl failed for %s:\n%s' % (name, out))
    return tt2


def run_fitter(tt2_path, family, device=None, options=(), workdir=None, timeout=120):
    """options: list of command-line tokens after the device (e.g. ['-preassign','keep','-strategy','jtag','off'])."""
    workdir = workdir or os.path.dirname(os.path.abspath(tt2_path))
    base = os.path.splitext(os.path.basename(tt2_path))[0]
    for ext in ('jed', 'fit', 'pin', 'io', 'tt3'):
        q = os.path.join(workdir, base + '.' + ext)
        if os.path.exists(q):
            os.remove(q)
    args = [WINCUPL + '\\WinCupl\\Fitters\\' + FITTER[family], os.path.basename(tt2_path)]
    if device:
        args += ['-device', device]
    args += list(options)
    rc, out = _wine(args, workdir, timeout)
    res = {'rc': rc, 'stdout': out}
    for ext in ('jed', 'fit', 'pin', 'io', 'tt3'):
        q = os.path.join(workdir, base + '.' + ext)
        if os.path.exists(q):
            res[ext] = open(q, 'rb').read().decode('latin-1')
    res['fits'] = 'jed' in res and 'Design fits successfully' in out
    return res


def read_jed(text):
    """Return the fuse array as a '0'/'1' string. Proper JEDEC field parsing: fields are '*'-terminated."""
    body = text.split('\x02', 1)[-1].split('\x03', 1)[0]
    fields = [f.strip() for f in body.split('*')]
    qf = next(int(f[2:]) for f in fields if f.startswith('QF'))
    default = next((f[1] for f in fields if re.fullmatch(r'F[01]', f)), '0')
    fz = [default] * qf
    for f in fields:
        m = re.fullmatch(r'L(\d+)\s+([01\s]+)', f)
        if m:
            a = int(m.group(1)); bits = re.sub(r'\s', '', m.group(2))
            fz[a:a + len(bits)] = list(bits)
    return ''.join(fz)


def parse_fit(text):
    """Extract placement and UIM routing from a fitter .fit report."""
    text = text.replace('\r', '')
    r = {'place': [], 'uim': {}, 'fanin': {}, 'globals': {}}
    for m in re.finditer(r'^(\S+) is placed at (pin (\d+)|feedback node (\d+)|foldback expander node (\d+))(?: \(MC (\d+)\))?',
                         text, re.M):
        kind = 'pin' if m.group(3) else ('node' if m.group(4) else 'fold')
        num = int(m.group(3) or m.group(4) or m.group(5))
        r['place'].append(dict(name=m.group(1), kind=kind, num=num, mc=int(m.group(6)) if m.group(6) else None))
    for bm in re.finditer(r'Multiplexer assignment for block (\w)\n(.*?)\n\n', text, re.S):
        rows = []
        for l in bm.group(2).split('\n'):
            mm = re.match(r'(\S+)\s+\((\S+)\s+(\S+)\)\s*:\s*MUX (\d+)\s+Ref \((\S+)\)', l)
            if mm:
                rows.append(dict(name=mm.group(1), src=mm.group(2), typ=mm.group(3), mux=int(mm.group(4)), ref=mm.group(5)))
        r['uim'][bm.group(1)] = rows
    for m in re.finditer(r'^(\S+) assigned to pin\s+(\d+)', text, re.M):
        r['globals'][m.group(1)] = int(m.group(2))
    return r


def run_batch(jobs, workers=8):
    """jobs: list of (key, fn, args) -> {key: result}; runs wine jobs concurrently."""
    from concurrent.futures import ThreadPoolExecutor
    out = {}
    with ThreadPoolExecutor(workers) as ex:
        futs = {ex.submit(fn, *args): key for key, fn, args in jobs}
        for f in futs:
            try:
                out[futs[f]] = f.result()
            except Exception as e:  # keep going; record the failure
                out[futs[f]] = e
    return out


def fresh_dir(root, name):
    d = os.path.join(root, name)
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    return d
