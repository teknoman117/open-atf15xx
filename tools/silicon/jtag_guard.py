"""Refuse to program an ATF15xx if the image would disable JTAG (recovery needs 12 V on OE1).

Checks, for a .jed and/or the .svf made from it:
  * JED: the JTAG-enable fuse = special-purpose bit 1 (1 = JTAG pins reserved for JTAG, the
    erased state; 0 = JTAG pins become I/O). Fuse 16783 (1502), 34167 (1504), 74115 (1508);
    docs/architecture/fitter/02-fuse-map.md F2.6. Oracle: vendor fit1508 `-strategy JTAG on/off`
    differs in exactly this special bit (plus the 4 JTAG-pin macrocell configs), and the bit
    matches the `-jtag` option in all 10 vendor JEDs of the user's designs.
  * SVF: every 4-bit write to the special register (`SIR 10 TDI (292)` then `SDR 4 TDI (x)`) must
    have bit 2 set. Bit 2 of that SDR is JED fuse 74115 on the ATF1508AS (verified by flipping only
    that fuse and diffing the atmisp SVF); the same layout is assumed for 1502/1504.
Exit 0 = safe, 1 = would disable JTAG (or cannot tell), 2 = usage error.

  usage: jtag_guard.py [--jed X.jed] [--svf X.svf]
"""
import argparse, re, sys

SPECIAL = {16808: ('ATF1502', 16782), 34192: ('ATF1504', 34166), 74136: ('ATF1508', 74114)}


def jed_fuses(path):
    s = open(path, encoding='latin-1').read()
    if '\x02' in s:
        s = s[s.index('\x02') + 1:]
    if '\x03' in s:
        s = s[:s.index('\x03')]
    qf, f, dflt = None, None, '0'
    for fld in s.split('*'):
        t = fld.strip()
        if t.startswith('QF'):
            qf = int(t[2:]); f = [dflt] * qf
        elif re.fullmatch(r'F[01]', t):
            dflt = t[1]; f = [dflt] * qf
        elif t.startswith('L'):
            m = re.fullmatch(r'L(\d+)\s+([01\s]+)', t, re.S)
            if not m:
                raise SystemExit('%s: cannot parse field %r' % (path, t[:40]))
            a = int(m.group(1)); b = re.sub(r'\s', '', m.group(2))
            f[a:a + len(b)] = list(b)
    if f is None:
        raise SystemExit('%s: no QF field' % path)
    return f


def check_jed(path):
    f = jed_fuses(path)
    if len(f) not in SPECIAL:
        print('jtag_guard: %s: unknown device (QF%d) -- refusing' % (path, len(f)))
        return False
    dev, s = SPECIAL[len(f)]
    bits = ''.join(f[s:s + 4])
    ok = f[s + 1] == '1'
    print('jtag_guard: %s: %s special bits %s, JTAG fuse %d = %s -> %s'
          % (path, dev, bits, s + 1, f[s + 1], 'JTAG stays ENABLED' if ok else 'JTAG would be DISABLED'))
    return ok


def check_svf(path):
    lines = [l.strip() for l in open(path, encoding='latin-1')]
    writes = []
    for i, l in enumerate(lines):
        if l.startswith('SIR 10 TDI (292)'):
            for l2 in lines[i + 1:i + 4]:
                m = re.match(r'SDR 4 TDI \(([0-9a-fA-F])\)', l2)
                if m:
                    writes.append(int(m.group(1), 16)); break
    if not writes:
        print('jtag_guard: %s: no special-register write found -- refusing (cannot tell)' % path)
        return False
    bad = [w for w in writes if not (w >> 2) & 1]
    print('jtag_guard: %s: %d special-register write(s) %s -> %s'
          % (path, len(writes), ' '.join('%x' % w for w in writes),
             'JTAG stays ENABLED' if not bad else 'JTAG would be DISABLED'))
    return not bad


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--jed'); ap.add_argument('--svf')
    a = ap.parse_args()
    if not (a.jed or a.svf):
        ap.error('give --jed and/or --svf')
    ok = True
    if a.jed:
        ok &= check_jed(a.jed)
    if a.svf:
        ok &= check_svf(a.svf)
    sys.exit(0 if ok else 1)


if __name__ == '__main__':
    main()
