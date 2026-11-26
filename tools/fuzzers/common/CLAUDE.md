# common: rules for every fuzzing run

* **One wine job per prefix at a time.** ATMISP (`atmisp_oracle.py`) opens a wine virtual
  desktop; two runs on one prefix interfere. Check with `pgrep -fa 'wine|ATMISP|fit15|drive.exe'` first.
  The container has its own prefix, so a container job and a host job may run together.
* **Fitters must not share a directory.** The vendor fitter writes temporary files with fixed
  names into its own directory; concurrent fitters there corrupt about 1 result in 400, silently.
  `fitter_oracle.run_batch(workers=8)` shares one directory: re-run anything surprising with
  `workers=1`. The prjbureau harness gives each worker its own copy (see `../prjbureau/CLAUDE.md`).
* **Never commit vendor material**: programs, `atmel.std`, vendor-written JEDEC files or reports
  (only the synthetic ATMISP test vectors, with ATMISP's Load JEDEC rewritten in our own layout).
  Results go to `.scratch/` unless a workflow's README says what it commits.
* **No hardware.** Fuzzing JEDEC files are synthetic and may disable JTAG; never program one.
  Programming goes through `tools/silicon/program.sh`, which refuses JEDs that disable JTAG.
* **Black box only.** These workflows run the programs and read what they write; the programs'
  output files are the only evidence. Never look inside the vendor binaries.
* Record each run (command, counts, verdict) in the worklog the workflow names.
