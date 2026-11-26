"""Cases for behaviour that was only known from reading ATMISP's code before 2026-11-25:
the voltage-class check for BE + 5 V and three-class chains (spec §9.1), and the JEDEC fields
QV / X / V having no effect on the SVF (spec §3.3).

  python3 tools/fuzzers/atmisp-svf/gen_cases4.py cases.json
"""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
from atmisp_oracle import make_jed, random_fuses, FAMILY_QF  # noqa: E402

cases = []
for name, devs in (('v_be_5v', ['ATF1504BE', 'ATF1504AS']), ('v_5v_be', ['ATF1508AS', 'ATF1502BE']),
                   ('v_5v_3v3_be', ['ATF1504AS', 'ATF1504ASV', 'ATF1508BE']), ('v_be_3v3_5v', ['ATF1508RE', 'ATF1502ASV', 'ATF1502AS'])):
    cases.append(dict(name=name, devices=[dict(name=d, instr=2, jed=None) for d in devs], jeds={}))
qf = FAMILY_QF['ATF1504AS']
fz = random_fuses(qf, 41)
plain = make_jed(qf, fz, title='ATF1504AS')
# the same fuses with test-vector fields after QF: QV (vector count), X (default test condition), V (vectors)
fields = 'QV2*\r\nX1*\r\nV0001 ' + '01' * 22 + '*\r\nV0002 ' + '10' * 22 + '*\r\n'
withf = plain.replace('F0*\r\n', 'F0*\r\n' + fields, 1)
assert withf != plain
for name, text in (('jf_plain', plain), ('jf_qv_x_v', withf)):
    cases.append(dict(name=name, devices=[dict(name='ATF1504AS', instr=1, jed='a.jed')], jeds={'a.jed': text}))
json.dump(cases, open(sys.argv[1], 'w'))
