import json, sys
import os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
from atmisp_oracle import make_jed, random_fuses, FAMILY_QF, family_of, JTAG_FUSE
NAMES = ['ATF1502AS','ATF1502ASL','ATF1502ASV','ATF1502ASVL','ATF1502BE',
         'ATF1504AS','ATF1504ASL','ATF1504ASV','ATF1504ASVL','ATF1504BE',
         'ATF1508AS','ATF1508ASL','ATF1508ASV','ATF1508ASVL','ATF1508BE','ATF1508RE']
cases = []
for n in NAMES:
    cases.append(dict(name='erase_revC_tck1_%s' % n, rev='C', tck=1, devices=[dict(name=n, instr=2)], jeds={}))
    cases.append(dict(name='erase_revD_nosr_%s' % n, rev='D', state_reset=0, devices=[dict(name=n, instr=2)], jeds={}))
# chain rerun
cases.append(dict(name='chain_3x_erase_nosr', state_reset=0, jeds={},
    devices=[dict(name='ATF1508AS', instr=2), dict(name='ATF1504ASV', instr=2), dict(name='ATF1502ASL', instr=3)]))
# JEDEC parser edge cases (1502AS, Verify)
qf = 16808
f = random_fuses(qf, 77)
def jt(body): return '\x02edge*\r\nQF16808* F0*\r\n' + body + 'C0000*\r\n\x030000\r\n'
long_l = 'L0 ' + f[:300] + '*\r\nL300 ' + f[300:16808] + '*\r\n'
cases.append(dict(name='jed_long_lfield', devices=[dict(name='ATF1502AS', instr=4, jed='a.jed')], jeds={'a.jed': jt(long_l)}))
cases.append(dict(name='jed_digit2_and_X', devices=[dict(name='ATF1502AS', instr=4, jed='a.jed')],
    jeds={'a.jed': jt('L0 ' + f[:16808] .replace('0', '0', 1) + '*\r\nL100 2X1X0 21*\r\n')}))
cases.append(dict(name='jed_lowercase_note', devices=[dict(name='ATF1502AS', instr=4, jed='a.jed')],
    jeds={'a.jed': '\x02edge*\r\nqf16808* f1*\r\nl0 0000*\r\nL10 0000 NOTE this field is skipped*\r\nl20 0101*\r\n\x030000\r\n'}))
cases.append(dict(name='jed_cr_in_addr', devices=[dict(name='ATF1502AS', instr=4, jed='a.jed')],
    jeds={'a.jed': '\x02edge*\r\nQF16808* F1*\r\nL5\r\n0000*\r\nL40\t0011*\r\n\x030000\r\n'}))
json.dump(cases, open(sys.argv[1], 'w')); print(len(cases), 'cases')
