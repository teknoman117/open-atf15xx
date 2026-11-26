# Work logs

This directory records **what has been done, what is being done, and why**, one file per workstream.
The normative specifications live in `docs/architecture/`. These logs explain how they were
obtained, how far each piece has been verified, and what is still open.

| Log | Workstream | State |
|---|---|---|
| [00-project.md](00-project.md) | Project overview: goal, components, history, decisions, roadmap | maintained by the main session |
| [01-atmisp-svf.md](01-atmisp-svf.md) | ATMISP 7.3 black-box analysis → spec §1–§11 → `atf15xx-tool` (JED → SVF) | complete |
| [02-fitter.md](02-fitter.md) | Vendor fitters fit1502/1504/1508 → fuse semantics → `atf15xx-*` fitter crates (`.tt2` → `.jed`) | complete for the user's finished designs |
| [03-prjbureau.md](03-prjbureau.md) | prjbureau evaluation, Yosys → `.tt2` prototype, local prjbureau branch | complete |
| [04-simulator.md](04-simulator.md) | JED simulator and Verilog equivalence checking (`atf15xx-sim`) | complete |
| [07-yosys-history.md](07-yosys-history.md) | When and why Yosys removed its CPLD support (research) | complete |
| [06-silicon.md](06-silicon.md) | Programming real devices (FT232H + OpenOCD), JTAG-lockout guard | in progress |
| [05-verilog-flow.md](05-verilog-flow.md) | Fully open Verilog → Yosys → fitter → `.jed` → SVF flow | in progress |
| [08-debugger.md](08-debugger.md) | In-circuit debugger over JTAG boundary scan (`atf15xx-tool`) | first version done |

## Conventions (every log)

Each log is a Markdown file that is kept up to date **while** the work happens, not written at the
end. It has these sections, in this order:

1. **Status**: a short dated summary at the top: where things stand, what's next, blockers.
2. **Scope and ownership**: what this workstream covers, which files/crates/directories it owns.
3. **Results**: what works, with numbers (match rates, test counts) and how to reproduce them
   (exact commands).
4. **Evidence levels**: for every non-trivial claim, how it is known:
   - *oracle-verified*: compared against the original vendor tool's output, with the corpus named;
   - *labelled*: the vendor's own labels in its output files (NOTE fields, report text), file named;
   - *external*: taken from datasheets, prjbureau or other sources, cited (file, commit, page);
   - *inferred*: reasoned, not yet checked. Say what would confirm it.

   Vendor programs are black boxes: every fact comes from what the programs write for inputs we
   generated (`tools/fuzzers/`, one workflow per derived fact).
5. **Decisions**: dated entries giving what was chosen, the alternatives, and why.
6. **Journal**: dated entries in reverse chronological order: what was tried, what was found,
   dead ends and their reasons, bugs found and fixed.
7. **Open issues**: known gaps, unexplained observations, suspected vendor defects.
8. **Artifacts**: where code, data, tools, Docker images, test vectors and scratch data live.

Rules:
- **All work is tracked in git** (repository root). Commit after every milestone, with a message
  that says what changed and why, and update the log in the same commit. Stage only the files
  your workstream owns (`git add <paths>`, never `git add -A`). No `Co-Authored-By` trailer (the
  user's instruction). The remote `origin` (github.com/teknoman117/open-atf15xx) is public: push,
  tag or rewrite history only when the user asks; the pre-0.1.0 history (local branch `pre`) is
  never pushed. The project rules for every session are in the root `CLAUDE.md`.
  Changes to upstream projects (the `external/prjbureau` submodule) are made on branches of the
  user's fork, checked out as worktrees under `.worktrees/<project>/<branch>` (git-ignored), and
  pushed there; `docs/prjbureau/branches.md` indexes them. There is no `patches/` directory.
- **[`docs/STORY.md`](../STORY.md)** is the narrative version for a future blog post. When a
  milestone lands, add to it: what was tried, what surprised you, what went wrong, the numbers.
  Write it for humans, chronologically, as new chapters or additions to the current one.
- Dates are absolute (YYYY-MM-DD).
- Scratch work in `.scratch/` is git-ignored and disposable. Anything worth keeping is summarised
  here, and the results go into the specs or tests.
- Vendor files (Microchip sample designs, fitter binaries) are never copied into the repository.
  The user's own designs in `~/Projects/pld` may be referenced by path; copy them only into test
  vectors with the user's agreement.
- When a spec chapter changes, the log entry names the chapter and section.
