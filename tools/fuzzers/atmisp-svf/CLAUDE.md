# atmisp-svf: supervising a run

Read `../common/CLAUDE.md` first (one wine job per prefix, no vendor files, no hardware).

* Prerequisites: ATMISP 7.3 in `~/.wine32` (`C:\ATMISP7`), `drive.exe` built from
  `../common/drive.c` and copied to `C:\probe\drive.exe`. Smoke-test one case before a batch
  (`python3 atmisp_oracle.py cases.json out/ <case-name>`): it must print `svf=True` and `DONE`.
* Run batches in the background, logging to a file; `atmisp_oracle.py` prints one line per case
  (`name svf=True|False seconds DONE/MSGBOX/ERROR …`). It skips cases already in the output
  directory, so a stopped run resumes where it left off. Expect 10–15 s per case.
* `svf=False` is not always a failure: rejected inputs (spec §9.2) show their dialog as `MSGBOX`.
  Compare with `case['expect']` in the generator. A hang (timeout 60 s) or a missing `DONE` line on
  an `ok` case: re-run that case alone before believing it (GUI timing glitches exist).
* A difference between ATMISP and `atf15xx-tool` is a finding. Decide it from the spec section it
  touches, then fix the code or the spec; record the case, the bytes that differ and the decision
  in `docs/worklog/01-atmisp-svf.md`.
* Commit only the assembled vectors (synthetic inputs, ATMISP's SVF, Load JEDEC rewritten by
  `rewrite_load_vectors.py --write`), never the raw oracle output directory.
* `assemble.py OUT DST` **deletes DST first**. Always assemble into `.scratch/fuzz/atmisp-svf/vectors`
  and copy the reviewed case directories into `atf15xx-tool/tests/vectors/` by hand.
