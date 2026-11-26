# atmisp-fusemap: supervising a run

Read `../common/CLAUDE.md` first.

* The JEDEC files have the JTAG-enable fuse at 0 in about half the patterns. ATMISP does not check
  it in SVF mode and `atf15xx-tool` only warns; the SVFs are fuzzing output and **must never be
  played into a device**. Keep them in `.scratch/`.
* Smoke-test one case (`atmisp_oracle.py cases.json out/ fm_ATF1504AS_bit03`: `svf=True`, `DONE`),
  then run the whole list in the background with a log. Progress = one line per case; the BE
  families take the same ~10 s per case but write larger SVFs (1508BE ≈ 370 kB).
* `decode.py` refuses incomplete data: a missing pattern, a different scan sequence between
  patterns (that would mean ATMISP's output depends on the data in a way the method does not
  cover: report it, do not paper over it), or a bit that is neither constant nor a clean copy.
  Re-run the named cases alone first.
* `check.py` must print `N/N SVFs identical` for every device. A difference means the fuse map in
  `atf15xx-tool/src/isp/fusemap.rs` (or the register layout in `device.rs`) disagrees with ATMISP:
  find the cells with `decode.py` on both SVF sets and fix the table, then the spec §5.
* After a change to `fusemap.rs` or `device.rs` that is not backed by a new run, re-run
  `check.py` on the existing output: it needs no wine.
* Record the summary lines of `decode.py` and `check.py` in `RESULTS.md` and in
  `docs/worklog/01-atmisp-svf.md`.
