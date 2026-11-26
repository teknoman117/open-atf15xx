"""Case list for the black-box fuse-map fuzzer (ATMISP 7.3, SVF mode).

For each device, one Program/Verify case per JEDEC file:
  zero      every fuse 0
  one       every fuse 1
  bitK      fuse i = bit K of i, for K = 0 .. ceil(log2 QF) - 1
A programming-row cell that copies fuse i then shows the binary expansion of i across the bitK
files (0 in `zero`, 1 in `one`); an inverted cell shows the complement; a cell that ignores the
fuse array is constant. decode.py turns the SVFs into that cell -> fuse map.

  python3 tools/fuzzers/atmisp-fusemap/gen_cases.py cases.json [DEVICE ...]

The JEDEC files are synthetic and many have the JTAG-enable fuse at 0: ATMISP does not check it in
SVF mode, but the SVFs must never be played into a device (they are fuzzing output only).
"""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
from atmisp_oracle import make_jed, FAMILY_QF, family_of  # noqa: E402

DEVICES = ['ATF1502AS', 'ATF1504AS', 'ATF1508AS', 'ATF1502BE', 'ATF1504BE', 'ATF1508BE', 'ATF1508RE']


def patterns(qf):
    yield 'zero', '0' * qf
    yield 'one', '1' * qf
    for k in range((qf - 1).bit_length()):
        yield 'bit%02d' % k, ''.join('1' if (i >> k) & 1 else '0' for i in range(qf))


def cases(devices):
    out = []
    for dev in devices:
        qf = FAMILY_QF[family_of(dev)]
        for tag, fuses in patterns(qf):
            out.append(dict(name='fm_%s_%s' % (dev, tag), devices=[dict(name=dev, instr=1, jed='a.jed')],
                            jeds={'a.jed': make_jed(qf, fuses, title=dev)}, pattern=tag, qf=qf))
    return out


if __name__ == '__main__':
    json.dump(cases(sys.argv[2:] or DEVICES), open(sys.argv[1], 'w'))
