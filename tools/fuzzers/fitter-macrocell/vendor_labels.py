"""Collect address -> label from the NOTE comments of every vendor-written JED in the oracle corpus
(.scratch/oracle/**), into .scratch/re/vendor_labels.json: the vendor's own names for each
product-term row, macrocell-configuration and UIM field, to compare with prjbureau's names.

  python3 tools/fuzzers/fitter-macrocell/vendor_labels.py
"""
import glob, json, os, re, sys
from collections import defaultdict
S = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch')
out = {}
for fam in ('1502', '1504', '1508'):
    lab = defaultdict(set)
    for j in glob.glob(os.path.join(S, 'oracle', '**', '*.jed'), recursive=True):
        t = open(j, errors='replace').read()
        if 'ATF%sAS' % fam not in t[:100]:
            continue
        flat = t.replace('\r', '').replace('\n', ' ')
        for m in re.finditer(r'L(\d+) ([01 ]+)\*\s*NOTE ([^*]*?)\s*\*', flat):
            lab[int(m.group(1))].add((re.sub(r'\s+', ' ', m.group(3)).strip(), len(m.group(2).replace(' ', ''))))
    out[fam] = {a: sorted(v) for a, v in sorted(lab.items())}
    print(fam, 'labelled addresses', len(lab), 'conflicts', sum(1 for v in lab.values() if len(v) > 1))
json.dump(out, open(os.path.join(S, 're', 'vendor_labels.json'), 'w'), indent=0)
