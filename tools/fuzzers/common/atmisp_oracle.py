"""Run ATMISP (under wine) as an oracle on generated test cases.

A case is a dict:
  name, devices: [ {name, width, idcode(bool), instr(0-9), jed (case-relative filename or None)} ],
  rev 'C'|'D', state_reset 0|1, tck int, calib int, jeds: {filename: jedtext}
Outputs go to OUT/<name>/: chain.chn, *.jed, case.json, out.svf, driver.log
"""
import json, os, random, shutil, subprocess, sys, time

PREFIX = os.path.expanduser(os.environ.get('WINEPREFIX', '~/.wine32'))
PROBE = os.path.join(PREFIX, 'drive_c', 'probe')
FAMILY_QF = {'ATF1502AS': 16808, 'ATF1504AS': 34192, 'ATF1508AS': 74136,
             'ATF1502BE': 16814, 'ATF1504BE': 34198, 'ATF1508BE': 74146}
FAMILY_QP = {16808: 44, 34192: 84, 74136: 84, 16814: 44, 34198: 100, 74146: 100}
# JTAG-enable fuse per QF: the fitters' JTAG on/off option (tools/fuzzers/fitter-macrocell, exp_feat
# 'nojtag') and prjbureau's database agree on these indices.
JTAG_FUSE = {16808: 16783, 34192: 34167, 74136: 74115, 16814: 16783, 34198: 34167, 74146: 74115}


def family_of(dev):
    d = dev.upper()
    for f in ('1502', '1504', '1508'):
        if f in d:
            return 'ATF%s%s' % (f, 'BE' if ('BE' in d or 'RE' in d) else 'AS')
    return None


def make_jed(qf, fuses, title='ATF15xx', checksum=None, extra_header=''):
    """fuses: string of '0'/'1' of length qf."""
    lines = ['\x02Synthetic test vector', 'Device: %s' % title, extra_header, '*',
             'QF%d* QP%d* F0*' % (qf, FAMILY_QP.get(qf, 44))]
    for i in range(0, qf, 64):
        lines.append('L%05d %s*' % (i, fuses[i:i + 64]))
    if checksum is None:
        checksum = jed_checksum(fuses)
    lines.append('C%04X*' % checksum)
    lines.append('\x030000')
    return '\r\n'.join(l for l in lines if l is not None) + '\r\n'


def jed_checksum(fuses):
    s = 0
    for i in range(0, len(fuses), 8):
        b = 0
        for k, ch in enumerate(fuses[i:i + 8]):
            if ch == '1':
                b |= 1 << k
        s += b
    return s & 0xffff


def random_fuses(qf, seed, density=0.5):
    rnd = random.Random(seed)
    f = ['1' if rnd.random() < density else '0' for _ in range(qf)]
    f[JTAG_FUSE[qf]] = '1'
    return ''.join(f)


def write_chn(case, d):
    ndev = len(case['devices'])
    out = ['%d 4 %d 0 ' % (ndev, case.get('calib', 1)), '']
    for dev in case['devices']:
        out.append(dev['name'])
        out.append('%d%s' % (dev.get('width', 10), '' if dev.get('idcode', True) else ' *'))
        out.append(str(dev['instr']))
        out.append(('C:\\probe\\cases\\%s\\%s' % (case['name'], dev['jed'])) if dev.get('jed') else '')
    return '\n'.join(out) + '\n'


def run_case(case, outdir, driver_timeout=60):
    name = case['name']
    wdir = os.path.join(PROBE, 'cases', name)
    shutil.rmtree(wdir, ignore_errors=True)
    os.makedirs(wdir)
    for fn, text in case.get('jeds', {}).items():
        # str values are written byte-for-byte as latin-1; bytes values verbatim
        data = text if isinstance(text, bytes) else text.encode('latin-1')
        with open(os.path.join(wdir, fn), 'wb') as f:
            f.write(data)
    chn = case.get('chn_text') or write_chn(case, wdir)
    with open(os.path.join(wdir, 'chain.chn'), 'w', newline='') as f:
        f.write(chn)
    env = dict(os.environ, WINEPREFIX=PREFIX, WINEDEBUG='-all', WINEDLLOVERRIDES='winedbg.exe=d')
    subprocess.run(['timeout', '20', 'wineserver', '-w'], env=env)
    svf_win = 'C:\\probe\\cases\\%s\\out.svf' % name
    t0 = time.time()
    subprocess.run(['timeout', str(driver_timeout), 'wine', 'explorer', '/desktop=atmisp,1024x768',
                    'C:\\probe\\drive.exe', 'C:\\probe\\cases\\%s\\chain.chn' % name, svf_win,
                    case.get('rev', 'D'), str(case.get('state_reset', 1)), str(case.get('tck', 1))],
                   env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run(['wineserver', '-k'], env=env)
    subprocess.run(['timeout', '20', 'wineserver', '-w'], env=env)
    dt = time.time() - t0
    od = os.path.join(outdir, name)
    shutil.rmtree(od, ignore_errors=True)
    os.makedirs(od)
    for fn in os.listdir(wdir):
        src = os.path.join(wdir, fn)
        dst = {'out.svf.log': 'driver.log'}.get(fn, fn)
        shutil.copy(src, os.path.join(od, dst))
    meta = {k: v for k, v in case.items() if k not in ('jeds', 'chn_text')}
    meta['elapsed_s'] = round(dt, 1)
    with open(os.path.join(od, 'case.json'), 'w') as f:
        json.dump(meta, f, indent=1)
    log = open(os.path.join(od, 'driver.log')).read() if os.path.exists(os.path.join(od, 'driver.log')) else ''
    svf = os.path.exists(os.path.join(od, 'out.svf'))
    return svf, dt, log


if __name__ == '__main__':
    cases = json.load(open(sys.argv[1]))
    outdir = sys.argv[2]
    only = set(sys.argv[3:])
    for c in cases:
        if only and c['name'] not in only:
            continue
        if os.path.exists(os.path.join(outdir, c['name'], 'case.json')) and not only:
            continue
        svf, dt, log = run_case(c, outdir)
        msgs = [l for l in log.splitlines() if l.startswith(('MSGBOX', 'ERROR', 'DONE'))]
        print('%-40s svf=%s %5.1fs %s' % (c['name'], svf, dt, ' | '.join(msgs)[:200]), flush=True)
