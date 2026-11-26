"""Check atf15xx-tool's fuse maps against the black-box map: for every fuzzing case, the SVF that
atf15xx-tool writes for the same device and JEDEC file must equal ATMISP's (apart from the
`Created on` line). Since the patterns give every fuse a distinct value sequence, equality on all
of them means every programming-row cell copies the same fuse in both.

  python3 tools/fuzzers/atmisp-fusemap/check.py OUT_DIR [--tool target/release/atf15xx-tool] [DEVICE ...]
"""
import argparse, json, os, subprocess, sys, tempfile

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..')


def body(path):
    lines = open(path, encoding='latin-1').read().splitlines()
    return '\n'.join(l for l in lines if not l.startswith('//  Created on: '))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('out_dir')
    ap.add_argument('devices', nargs='*')
    ap.add_argument('--tool', default=os.path.join(ROOT, 'target', 'release', 'atf15xx-tool'))
    a = ap.parse_args()
    total, bad = {}, {}
    with tempfile.TemporaryDirectory() as tmp:
        for d in sorted(os.listdir(a.out_dir)):
            if not d.startswith('fm_'):
                continue
            dev = d.split('_')[1]
            if a.devices and dev not in a.devices:
                continue
            case = os.path.join(a.out_dir, d)
            meta = json.load(open(os.path.join(case, 'case.json')))
            ours = os.path.join(tmp, d + '.svf')
            subprocess.run([a.tool, 'svf', '-q', '-d', '%s:pv:%s' % (dev, os.path.join(case, 'a.jed')),
                            '--rev', meta.get('rev', 'D').lower(), '--tck', str(meta.get('tck', 1)),
                            '-o', ours], check=True, stderr=subprocess.DEVNULL)   # JTAG-off warnings expected
            total[dev] = total.get(dev, 0) + 1
            if body(ours) != body(os.path.join(case, 'out.svf')):
                bad.setdefault(dev, []).append(meta['pattern'])
    for dev in sorted(total):
        print('%-10s %2d/%2d SVFs identical%s' % (dev, total[dev] - len(bad.get(dev, [])), total[dev],
                                                  ('  differ: %s' % bad[dev]) if dev in bad else ''))
    sys.exit(1 if bad else 0)


if __name__ == '__main__':
    main()
