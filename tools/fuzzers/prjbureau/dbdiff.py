"""Recursive diff of one device's entry in two prjbureau databases.

  python3 tools/fuzzers/prjbureau/dbdiff.py OURS.json UPSTREAM.json DEVICE [N]

Only values present in OURS are compared: fuzzers that have not run, or that cannot determine a
value (see CLAUDE.md), leave keys absent or empty; those are skipped, not counted as differences.
Prints the number of equal leaves and up to N differences (default 15). Validation of a toolchain
change: every value equal ("differences 0")."""
import json, sys
ours, up, dev = json.load(open(sys.argv[1]))[sys.argv[3]], json.load(open(sys.argv[2]))[sys.argv[3]], sys.argv[3]
diffs, same = [], [0]
def walk(a, b, path):
    if isinstance(a, dict) and isinstance(b, dict):
        for k in a:
            if k not in b: diffs.append((path + [k], 'only ours', a[k])); continue
            walk(a[k], b[k], path + [k])
        return
    if a in (None, [], {}) and b not in (None, [], {}): return
    if a == b: same[0] += 1
    else: diffs.append((path, a, b))
walk(ours, up, [])
print(dev, 'equal leaves', same[0], 'differences', len(diffs))
for p, a, b in diffs[:int(sys.argv[4]) if len(sys.argv) > 4 else 15]:
    print('  ', '/'.join(map(str, p)), '| ours', json.dumps(a)[:90], '| upstream', json.dumps(b)[:90])
