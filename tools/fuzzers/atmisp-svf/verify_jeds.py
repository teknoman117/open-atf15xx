#!/usr/bin/env python3
"""Verify atmisp against the real ATMISP 7.3 using your own JEDEC files.

For every JEDEC file given, this
  1. builds a one-device chain (device name from the file's QF unless --device is given),
  2. runs ATMISP 7.3 under wine (through drive.exe) to produce the reference SVF,
  3. runs atmisp on the very same chain file,
  4. compares the two SVF files byte for byte (the `Created on:` line is made equal by passing
     ATMISP's timestamp to atmisp), and for `load` also the written JEDEC files, fuse for fuse
     (atmisp writes its own layout, spec §8.2: same fuses, same `G`, correct `C` checksum).

Usage:
  python3 tools/fuzzers/atmisp-svf/verify_jeds.py [options] FILE.jed [FILE.jed ...]

Options:
  --instr LIST     comma-separated instructions per file (default: pv). Names: pv, verify, pvs, erase,
                   blank-check, load, secure, verify-secure, read-ues, bypass (load/erase/... ignore the
                   JEDEC contents but are accepted for completeness)
  --device NAME    device name to use instead of the one derived from QF (e.g. ATF1504ASV)
  --rev C|D        SVF revision (default D); may be given twice to test both: --rev C --rev D
  --tck N          TCK period for Rev C (default 1)
  --no-state-reset generate without STATE RESET
  --work DIR       where case directories and outputs are kept (default: ./verify-work)
  --force          re-run ATMISP even if a reference SVF for the case already exists in --work
  --cli PATH       atmisp binary (default: target/release/atf15xx-tool, built with cargo if missing)
  --build-driver   (re)compile drive.c with i686-w64-mingw32-gcc and install it as C:\\probe\\drive.exe

Verdicts: MATCH (byte-identical SVF; Load JEDEC with identical fuses), OK-REJECT (ATMISP wrote nothing and atmisp also refused),
DIVERGE (atmisp refuses an input ATMISP accepts, by design — spec §9.2), MISMATCH, SKIP.
Exit status: 0 if every case is MATCH or OK-REJECT, 1 otherwise.
"""
import argparse, gzip, hashlib, os, re, shutil, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..', '..'))
COMMON = os.path.join(HERE, '..', 'common')
sys.path.insert(0, COMMON)
import atmisp_oracle as oracle  # noqa: E402

INSTR = {'bypass': 0, 'pv': 1, 'program-verify': 1, 'erase': 2, 'blank-check': 3, 'verify': 4, 'load': 5,
         'secure': 6, 'pvs': 7, 'verify-secure': 8, 'read-ues': 9}
# rejections that are deliberate divergences from ATMISP (spec §9.2: J1, J2, N1, N2)
INTENTIONAL = ('JEDEC file is not valid for this device', 'Unsupported device name', 'Bypass must be selected')
QF_DEVICE = {16808: 'ATF1502AS', 16814: 'ATF1502BE', 34192: 'ATF1504AS', 34198: 'ATF1504BE',
             74136: 'ATF1508AS', 74146: 'ATF1508BE'}


def qf_of(data):
    m = re.search(rb'(?i)\*\s*QF\s*(\d+)', b'*' + data)
    return int(m.group(1)) if m else None


def ensure_driver(force):
    dst = os.path.join(oracle.PROBE, 'drive.exe')
    if os.path.exists(dst) and not force:
        return
    os.makedirs(oracle.PROBE, exist_ok=True)
    exe = os.path.join(oracle.PROBE, 'drive.exe')
    subprocess.run(['i686-w64-mingw32-gcc', '-O1', '-o', exe, os.path.join(COMMON, 'drive.c'), '-luser32'], check=True)
    print('installed', dst)


def ensure_cli(path):
    if path:
        return path
    p = os.path.join(ROOT, 'target', 'release', 'atf15xx-tool')
    if not os.path.exists(p):
        subprocess.run(['cargo', 'build', '--release'], cwd=ROOT, check=True)
    return p


def first_diff(a, b):
    al, bl = a.split(b'\n'), b.split(b'\n')
    for i, (x, y) in enumerate(zip(al, bl)):
        if x != y:
            return i + 1, x.decode('latin-1').rstrip('\r'), y.decode('latin-1').rstrip('\r')
    return min(len(al), len(bl)) + 1, '<end>' if len(al) < len(bl) else '', '<end>' if len(bl) < len(al) else ''


def jed_fuses(data):
    """(fuse string, G value, stated C value) of a JEDEC file, read as ATMISP does (spec §3)."""
    t = data.decode('latin-1')
    body = t.split('\x02', 1)[-1].split('\x03', 1)[0]
    flds = [f.strip() for f in body.split('*') if 'NOTE' not in f and 'Note' not in f]
    qf = next(int(f[2:]) for f in flds if f.startswith('QF'))
    fz = [next((f[1] for f in flds if re.fullmatch(r'F[01]', f)), '0')] * qf
    for f in flds:
        m = re.fullmatch(r'L(\d+)\s+([01\s]+)', f)
        if m:
            b = re.sub(r'\s', '', m.group(2)); a = int(m.group(1)); fz[a:a + len(b)] = list(b)
    g = next((f[1] for f in flds if re.fullmatch(r'G[01]', f)), '0')
    c = next((int(f[1:], 16) for f in flds if re.fullmatch(r'C[0-9A-Fa-f]{1,4}', f)), None)
    return ''.join(fz), g, c


def load_jed_differs(ours, theirs):
    """None if two Load JEDEC outputs hold the same fuses and security fuse and ours states the
    correct checksum; otherwise the reason."""
    (fo, go, co), (ft, gt, _) = jed_fuses(ours), jed_fuses(theirs)
    if fo != ft:
        return 'fuses differ (%d)' % sum(a != b for a, b in zip(fo, ft)) if len(fo) == len(ft) else 'QF differs'
    if go != gt:
        return 'security fuse differs'
    want = sum(int(fo[i:i + 8][::-1], 2) for i in range(0, len(fo), 8)) & 0xffff
    return None if co == want else 'checksum %s, want %04X' % (co, want)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('jeds', nargs='+')
    ap.add_argument('--instr', default='pv')
    ap.add_argument('--device')
    ap.add_argument('--rev', action='append', choices=['C', 'D', 'c', 'd'])
    ap.add_argument('--tck', type=int, default=1)
    ap.add_argument('--no-state-reset', action='store_true')
    ap.add_argument('--work', default='verify-work')
    ap.add_argument('--force', action='store_true')
    ap.add_argument('--cli')
    ap.add_argument('--build-driver', action='store_true')
    a = ap.parse_args()

    ensure_driver(a.build_driver)
    cli = ensure_cli(a.cli)
    revs = [r.upper() for r in (a.rev or ['D'])]
    instrs = [i.strip().lower() for i in a.instr.split(',') if i.strip()]
    for i in instrs:
        if i not in INSTR:
            ap.error('unknown instruction %r' % i)
    work = os.path.abspath(a.work)
    os.makedirs(work, exist_ok=True)

    results = []
    for path in a.jeds:
        data = open(path, 'rb').read()
        dev = a.device or QF_DEVICE.get(qf_of(data))
        if not dev:
            results.append((path, '-', '-', 'SKIP', 'cannot derive device from QF=%s; use --device' % qf_of(data)))
            continue
        tag = re.sub(r'[^A-Za-z0-9]+', '_', os.path.splitext(os.path.basename(path))[0])[:24]
        digest = hashlib.sha1(data).hexdigest()[:8]
        for instr in instrs:
            for rev in revs:
                name = 'v_%s_%s_%s_%s%s%s' % (tag, digest, dev, instr.replace('-', ''), rev,
                                              '_nosr' if a.no_state_reset else '')
                case = dict(name=name, rev=rev, tck=a.tck, state_reset=0 if a.no_state_reset else 1,
                            devices=[dict(name=dev, instr=INSTR[instr], jed='a.jed')],
                            jeds={} if INSTR[instr] == 5 else {'a.jed': data})
                cdir = os.path.join(work, name)
                ref = os.path.join(cdir, 'out.svf')
                if a.force or not os.path.exists(ref):
                    print('running ATMISP: %s' % name, flush=True)
                    oracle.run_case(case, work)
                if not os.path.exists(ref) or os.path.getsize(ref) == 0:
                    log = open(os.path.join(cdir, 'driver.log'), errors='replace').read() if os.path.exists(
                        os.path.join(cdir, 'driver.log')) else ''
                    msgs = ' | '.join(l for l in log.splitlines() if l.startswith(('MSGBOX', 'ERROR')))
                    r = subprocess.run([cli, 'svf', '-q', os.path.join(cdir, 'chain.chn'),
                                        '--drive', 'C=' + os.path.join(oracle.PREFIX, 'drive_c'),
                                        '-o', os.path.join(cdir, 'cli.svf'), '--timestamp', 'x'],
                                       capture_output=True, text=True)
                    verdict = 'OK-REJECT' if r.returncode != 0 else 'MISMATCH'
                    results.append((path, instr, rev, verdict,
                                    'ATMISP wrote no SVF (%s); atmisp %s' % (msgs or 'no dialog',
                                    'rejected: ' + r.stderr.strip()[:100] if r.returncode else 'accepted')))
                    continue
                want = open(ref, 'rb').read()
                ts = want.split(b'\r\n')[1].decode('latin-1')[len('//  Created on: '):]
                out = os.path.join(cdir, 'cli.svf')
                for j in os.listdir(cdir):
                    if j.startswith('cli-') and j.endswith('.jed'):
                        os.remove(os.path.join(cdir, j))
                # Load writes the JEDEC next to the chain; keep ATMISP's copy aside first
                if INSTR[instr] == 5:
                    # ATMISP's Load output was copied into the case dir by oracle.py; keep it aside, and
                    # point atmisp's output at a fresh drive_c location so the two cannot alias.
                    shutil.copy(os.path.join(cdir, 'a.jed'), os.path.join(cdir, 'atmisp-a.jed'))
                    cli_root = os.path.join(cdir, 'cli_drive_c')
                    shutil.rmtree(cli_root, ignore_errors=True)
                    cli_case = os.path.join(cli_root, 'probe', 'cases', name)
                    os.makedirs(cli_case)
                drive_c = os.path.join(cdir, 'cli_drive_c') if INSTR[instr] == 5 else os.path.join(oracle.PREFIX, 'drive_c')
                cmd = [cli, 'svf', '-q', os.path.join(cdir, 'chain.chn'), '-o', out, '--timestamp', ts,
                       '--drive', 'C=' + drive_c, '--allow-mixed-voltage',
                       '--rev', rev, '--tck', str(a.tck)] + (['--no-state-reset'] if a.no_state_reset else [])
                r = subprocess.run(cmd, capture_output=True, text=True)
                if r.returncode != 0:
                    known = any(k in r.stderr for k in INTENTIONAL)
                    results.append((path, instr, rev, 'DIVERGE' if known else 'MISMATCH',
                                    ('atmisp rejects this input on purpose (spec §9.2), ATMISP accepted it: '
                                     if known else 'atmisp rejected: ') + r.stderr.strip()[:200]))
                    continue
                got = open(out, 'rb').read()
                if got != want:
                    ln, g, w = first_diff(got, want)
                    results.append((path, instr, rev, 'MISMATCH',
                                    'SVF line %d: atmisp %r vs ATMISP %r (files in %s)' % (ln, g[:80], w[:80], cdir)))
                    continue
                if INSTR[instr] == 5:
                    written = os.path.join(cdir, 'cli_drive_c', 'probe', 'cases', name, 'a.jed')
                    if not os.path.exists(written):
                        results.append((path, instr, rev, 'MISMATCH', 'atmisp wrote no Load JEDEC at ' + written))
                        continue
                    why = load_jed_differs(open(written, 'rb').read(), open(os.path.join(cdir, 'atmisp-a.jed'), 'rb').read())
                    if why:
                        results.append((path, instr, rev, 'MISMATCH', 'Load JEDEC output: ' + why))
                        continue
                results.append((path, instr, rev, 'MATCH', '%d bytes' % len(want)))

    w = max(len(r[0]) for r in results)
    print()
    for p, i, rev, v, info in results:
        print('%-*s  %-13s %s  %-9s %s' % (w, p, i, rev, v, info))
    bad = [r for r in results if r[3] in ('MISMATCH', 'SKIP', 'DIVERGE')]
    print('\n%d case(s): %d match, %d rejected by both, %d intentional divergence(s), %d mismatch(es)/skip(s)' % (
        len(results), sum(r[3] == 'MATCH' for r in results), sum(r[3] == 'OK-REJECT' for r in results),
        sum(r[3] == 'DIVERGE' for r in results), sum(r[3] in ('MISMATCH', 'SKIP') for r in results)))
    sys.exit(1 if bad else 0)


if __name__ == '__main__':
    main()
