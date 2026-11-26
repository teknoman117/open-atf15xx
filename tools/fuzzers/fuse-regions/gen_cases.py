"""One ATMISP `Load` case per AS family (no input JEDEC file: in SVF mode Load writes the blank
image, whose NOTE comments label every fuse region; see regions.py).

  python3 tools/fuzzers/fuse-regions/gen_cases.py cases.json
"""
import json, sys

json.dump([dict(name='rg_%s' % d, devices=[dict(name=d, instr=5, jed='a.jed')], jeds={})
           for d in ('ATF1502AS', 'ATF1504AS', 'ATF1508AS')], open(sys.argv[1], 'w'))
