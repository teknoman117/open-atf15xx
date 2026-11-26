import json, sys
import os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
from atmisp_oracle import make_jed, random_fuses, FAMILY_QF, family_of, JTAG_FUSE, jed_checksum
cases = []
def jed_for(dev, seed, **kw):
    qf = FAMILY_QF[family_of(dev)]
    return make_jed(qf, random_fuses(qf, seed), title=dev, **kw)
def one(name, dev, instr, seed=1, jedtext=None, **kw):
    c = dict(name=name, devices=[dict(name=dev, instr=instr, jed=('a.jed' if instr in (1,4,5,7) else None))], jeds={}, **kw)
    if instr in (1,4,7): c['jeds']['a.jed'] = jedtext or jed_for(dev, seed)
    cases.append(c)
BE = ['ATF1502BE','ATF1504BE','ATF1508BE','ATF1508RE']
for n in BE:
    for k in (6,7,8,9,3): one('b2_instr%d_%s' % (k, n), n, k, seed=31)
for n in ['ATF1502AS','ATF1508AS','ATF1502BE','ATF1508BE','ATF1508RE','ATF1508ASV','ATF1504ASVL']:
    one('b2_load_%s' % n, n, 5)
for n in ['ATF1502AS','ATF1508AS']:
    one('b2_ues_%s' % n, n, 9)
# security fuse G1
for n in ['ATF1502AS', 'ATF1502BE']:
    qf = FAMILY_QF[family_of(n)]
    one('b2_g1_pv_%s' % n, n, 1, jedtext=make_jed(qf, random_fuses(qf, 5), title=n, extra_header='').replace('F0*', 'F0* G1*'))
# rev C on BE (1us delays)
one('b2_revC_tck2_pv_ATF1502BE', 'ATF1502BE', 1, seed=9, rev='C', tck=2)
one('b2_revC_tck1_erase_ATF1504BE_nosr', 'ATF1504BE', 2, rev='C', tck=1, state_reset=0)
# JED validation
qf = 16808
f = random_fuses(qf, 3)
one('b2_jed_badcsum', 'ATF1502AS', 1, jedtext=make_jed(qf, f, checksum=0x1234))
one('b2_jed_wrongqf', 'ATF1502AS', 1, jedtext=make_jed(34192, random_fuses(34192, 3)))
fj = f[:JTAG_FUSE[qf]] + '0' + f[JTAG_FUSE[qf] + 1:]
one('b2_jed_jtagoff', 'ATF1502AS', 1, jedtext=make_jed(qf, fj))
one('b2_jed_jtagoff_verify', 'ATF1502AS', 4, jedtext=make_jed(qf, fj))
one('b2_jed_partial_f1', 'ATF1502AS', 1, jedtext='\x02x*QF16808*F1*L0 0000000011*L16000 0101*C0000*\x030000\r\n')
one('b2_jed_noqf', 'ATF1502AS', 1, jedtext='\x02x*F1*L0 0000000011*\x030000\r\n')
# chain-file validation
def raw(name, text, **kw):
    cases.append(dict(name=name, chn_text=text, devices=[], jeds={}, **kw))
raw('b2_chn_missing_jed', '1 4 1 0 \n\nATF1502AS\n10\n1\nC:\\probe\\nonexist.jed\n')
raw('b2_chn_width_bad', '1 4 1 0 \n\nATF1502AS\n8\n2\n\n')
raw('b2_chn_unk_width0', '2 4 1 0 \n\nUNKNOWN\n0\n0\n\nATF1502AS\n10\n2\n\n')
raw('b2_chn_calib0', '1 4 0 0 \n\nATF1502AS\n10\n2\n\n')
raw('b2_chn_calib5000', '1 4 5000 0 \n\nATF1502AS\n10\n2\n\n')
raw('b2_chn_lpt1', '1 1 1 0 \n\nATF1502AS\n10\n2\n\n')
raw('b2_chn_cable1', '1 4 1 1 \n\nATF1502AS\n10\n2\n\n')
raw('b2_chn_nocable', '1 4 1\n\nATF1502AS\n10\n2\n\n')
raw('b2_chn_ndev0', '0 4 1 0 \n\n')
raw('b2_chn_crlf', '1 4 1 0 \r\n\r\nATF1504AS\r\n10\r\n2\r\n\r\n')
raw('b2_chn_noblank', '1 4 1 0 \nATF1504AS\n10\n2\n')
raw('b2_chn_be_with_as', '2 4 1 0 \n\nATF1504BE\n10\n2\n\nATF1504AS\n10\n2\n\n')
one('b2_tck0_erase', 'ATF1502AS', 2, rev='C', tck=0)
one('b2_tck1001_erase', 'ATF1502AS', 2, rev='C', tck=1001)
# wrong device for jed (1504 jed on 1508)
one('b2_jed_wrongdev', 'ATF1508AS', 1, jedtext=jed_for('ATF1504AS', 3))
json.dump(cases, open(sys.argv[1], 'w'))
print(len(cases), 'cases')
