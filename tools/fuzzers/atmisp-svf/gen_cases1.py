import json, sys
import os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'common'))
from atmisp_oracle import make_jed, random_fuses, FAMILY_QF, family_of
NAMES = ['ATF1502AS','ATF1502ASL','ATF1502ASV','ATF1502ASVL','ATF1502BE','ATF1502SE','ATF1502SEL',
         'ATF1504AS','ATF1504ASL','ATF1504ASV','ATF1504ASVL','ATF1504BE',
         'ATF1508AS','ATF1508ASL','ATF1508ASV','ATF1508ASVL','ATF1508BE','ATF1508RE']
cases = []
def jed_for(dev, seed):
    qf = FAMILY_QF[family_of(dev)]
    return make_jed(qf, random_fuses(qf, seed), title=dev)
def one(name, dev, instr, seed=1, **kw):
    c = dict(name=name, devices=[dict(name=dev, instr=instr, jed=('a.jed' if instr in (1,4,5,7) else None))], jeds={}, **kw)
    if instr in (1,4,7): c['jeds']['a.jed'] = jed_for(dev, seed)
    cases.append(c)
for n in NAMES: one('erase_%s' % n, n, 2)
for n in ['ATF1502AS','ATF1504AS','ATF1508AS','ATF1502BE','ATF1504BE','ATF1508BE','ATF1508RE']:
    one('pv_%s' % n, n, 1, seed=hash(n) & 0xffff)
for k in (0,3,4,5,6,7,8,9): one('instr%d_ATF1504AS' % k, 'ATF1504AS', k, seed=7)
for k in (3,4,5,9): one('instr%d_ATF1504BE' % k, 'ATF1504BE', k, seed=8)
for t in (1,3,1000): one('revC_tck%d_pv_ATF1502AS' % t, 'ATF1502AS', 1, seed=11, rev='C', tck=t)
one('revD_nosr_pv_ATF1502AS', 'ATF1502AS', 1, seed=11, state_reset=0)
one('revC_nosr_erase_ATF1502AS', 'ATF1502AS', 2, rev='C', state_reset=0, tck=2)
one('calib5_pv_ATF1502AS', 'ATF1502AS', 1, seed=11, calib=5)
# chains
j02 = jed_for('ATF1502AS', 21); j04 = jed_for('ATF1504AS', 22)
cases.append(dict(name='chain_unk_pv_erase', jeds={'b.jed': j02},
    devices=[dict(name='UNKNOWN', width=8, idcode=True, instr=0), dict(name='ATF1502AS', instr=1, jed='b.jed'), dict(name='ATF1504AS', instr=2)]))
cases.append(dict(name='chain_byp_verify', jeds={'c.jed': j04},
    devices=[dict(name='ATF1502AS', instr=0), dict(name='ATF1504AS', instr=4, jed='c.jed')]))
cases.append(dict(name='chain_unk_noid_blank', jeds={},
    devices=[dict(name='ATF1504AS', instr=3), dict(name='UNKNOWN', width=5, idcode=False, instr=0), dict(name='UNKNOWN', width=12, idcode=True, instr=0)]))
cases.append(dict(name='chain_3x_erase_nosr', state_reset=0, jeds={},
    devices=[dict(name='ATF1508AS', instr=2), dict(name='ATF1504ASV', instr=2), dict(name='ATF1502ASL', instr=3)]))
cases.append(dict(name='chain_be_unk', jeds={},
    devices=[dict(name='UNKNOWN', width=10, idcode=True, instr=0), dict(name='ATF1504BE', instr=2)]))
json.dump(cases, open(sys.argv[1], 'w'))
print(len(cases), 'cases')
