import gzip, json, os, re, shutil, sys
src, dst = sys.argv[1], sys.argv[2]
REJECT = {'erase_ATF1502SE': 'N1', 'erase_ATF1502SEL': 'N1', 'b2_chn_be_with_as': 'V1', 'b2_chn_missing_jed': 'E3',
          'b2_chn_ndev0': 'E2', 'b2_jed_noqf': 'J1', 'b2_jed_wrongqf': 'J1', 'b2_jed_wrongdev': 'J1', 'b2_tck0_erase': 'TCK',
          # gen_cases4.py: a BE-class device with any other class
          'v_be_5v': 'V1', 'v_5v_be': 'V1', 'v_5v_3v3_be': 'V1', 'v_be_3v3_5v': 'V1'}
def group(n):
    n2 = n[3:] if n.startswith('b2_') else n
    for pre, g in (('erase_', 'erase'), ('pv_', 'program-verify'), ('g1_', 'program-verify'), ('instr', 'instructions'),
                   ('load_', 'load'), ('ues_', 'instructions'), ('rev', 'svf-options'), ('calib', 'svf-options'),
                   ('tck', 'svf-options'), ('chain_', 'chains'), ('chn_', 'chain-file'), ('jed_', 'jedec-input'),
                   ('v_', 'chains'), ('jf_', 'jedec-input')):
        if n2.startswith(pre):
            return g
    return 'misc'
shutil.rmtree(dst, ignore_errors=True)
count = {}
for n in sorted(os.listdir(src)):
    d = os.path.join(src, n)
    meta = json.load(open(os.path.join(d, 'case.json')))
    meta.pop('elapsed_s', None)
    outs = {dev.get('jed') for dev in meta.get('devices', []) if dev.get('instr') == 5 and dev.get('jed')}
    g = group(n)
    od = os.path.join(dst, g, n)
    os.makedirs(od)
    meta['expect'] = 'reject' if n in REJECT else 'ok'
    if n in REJECT: meta['reject_class'] = REJECT[n]
    if n == 'b2_tck1001_erase': meta['compat_tck'] = True
    for fn in os.listdir(d):
        p = os.path.join(d, fn)
        if fn == 'out.svf':
            if n not in REJECT:
                with open(p, 'rb') as f, gzip.open(os.path.join(od, 'expected.svf.gz'), 'wb', 9) as g2:
                    g2.write(f.read())
        elif fn.endswith('.jed') and fn in outs:
            with open(p, 'rb') as f, gzip.open(os.path.join(od, 'expected-' + fn + '.gz'), 'wb', 9) as g2:
                g2.write(f.read())
        elif fn in ('case.json',):
            continue
        else:
            shutil.copy(p, os.path.join(od, fn))
    json.dump(meta, open(os.path.join(od, 'case.json'), 'w'), indent=1)
    count[g] = count.get(g, 0) + 1
print(count, sum(count.values()))
