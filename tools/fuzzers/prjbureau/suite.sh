#!/bin/sh
# Run prjbureau's fuzzers on a worktree, for the given devices only.
#   docker/prjbureau/run.sh sh tools/fuzzers/prjbureau/suite.sh /work/.worktrees/prjbureau/<branch> ATF1502AS [DEVICE...]
# Environment: FUZZERS (default: all of fuzzers/0*/fuzzer.py, 000-init excepted),
#              PRJBUREAU_JOBS (parallel fitters for prefetching fuzzers; default 1).
# The worktree's database.json is rewritten (000-init, then trimmed to DEVICES, then every fuzzer);
# do not commit it from such a run: see CLAUDE.md. Fitter results are cached in <worktree>/cache.
set -e
W=$1; shift
[ -n "$1" ] || { echo "usage: suite.sh WORKTREE DEVICE..." >&2; exit 2; }
cd "$W"
export PYTHONPATH="$W" LANG=C.UTF-8
mkdir -p cache
python3 fuzzers/000-init/fuzzer.py
python3 - "$@" <<'PY'
import json, sys
d = json.load(open('database.json'))
missing = [k for k in sys.argv[1:] if k not in d]
assert not missing, f"not in 000-init: {missing}"
json.dump({k: v for k, v in d.items() if k in sys.argv[1:]}, open('database.json', 'w'), indent=2)
PY
for f in ${FUZZERS:-fuzzers/0*/fuzzer.py}; do
  case "$f" in */000-init/*) continue;; esac
  echo "== $f" >&2
  start=$(date +%s)
  if ! python3 "$f"; then echo "FAILED $f" >&2; exit 1; fi
  echo "   $(( $(date +%s) - start )) s" >&2
done
