# 07: Yosys CPLD-support history

## Status

**2026-11-24: complete.** The question was when and why upstream Yosys removed its CPLD support.
It was removed 2026-08-17 to 2026-08-20 (PRs #6126, #6132, #6135), as part of the "ABC
unification" move to `abc9`, with the original authors' consent. First missing in v0.69; **v0.68
is the last release with `abc -sop`, `synth_coolrunner2` and `synth_greenpak4`.** `abc -sop` can
be rebuilt on our pinned v0.71 with a short script (`submod` → `yosys-abc cover` →
`read_blif -sop -wideports`), proven equivalent on a test counter. Recommendation: stay on v0.71
and don't pin 0.68. Findings: [`docs/research/yosys-cpld-removal.md`](../research/yosys-cpld-removal.md).
No blockers.

## Scope and ownership

Research only: upstream Yosys git history, GitHub PRs/issues (read-only `gh api`), the original
CPLD projects' status, the licence of the removed code, and a feasibility check that `abc -sop`
can be rebuilt in Yosys v0.71. Owns `docs/research/yosys-cpld-removal.md` and this log. Scratch:
`external/yosys` (upstream clone, git-ignored, unmodified apart from the untracked
`.scratch-cpld/` directory). Nothing was pushed, and no issue, PR or comment was made upstream.

## Results

| Removed | Commit (author date) | PR (merged) | First release without | Last release with |
|---|---|---|---|---|
| `techlibs/coolrunner2`, `synth_coolrunner2`, `coolrunner2_sop`, `coolrunner2_fixup` | `541d24093` (2026-08-17) | #6126 (2026-08-19) | v0.69 (2026-09-09) | v0.68 (2026-08-05) |
| `techlibs/greenpak4`, `synth_greenpak4`, `greenpak4_dffinv`, `nlutmap` | `edb411a62` (2026-08-18) | #6132 (2026-08-19) | v0.69 | v0.68 |
| `abc -sop`, `-I`, `-P` | `caceaaa2b` (2026-08-19) | #6135 (2026-08-20) | v0.69 | v0.68 |
| (context) classic ABC in all `synth_*` | 15 commits, 2026-07-22 to 08-10 | #6103 (2026-08-19) | v0.69 | v0.68 |
| (context) `flowmap`, `abc -fast` | `f0b522513`, `45e1de36b` | #6061, #5782 | v0.68 | v0.67 |

* **Rationale** (quotes in the research note, §3): `abc -sop` "has no XAIGER equivalent" and
  CoolRunner-II was "the one flow using" it. `nlutmap` "depends on behaviour of `abc -lut` which
  cannot easily be adapted to `abc9`". ArcaneNibble (the coolrunner2 author) and Andrew Zonenberg
  (GreenPAK) both agreed on PR #6103.
* **Deleted, not moved.** Both downstream tools (openfpga's xc2par/gp4par) were last pushed
  2020-03-30.
* **Still in v0.71:** `$sop` cell, `read_blif -sop`, `extract_counter` (up and down), `abc -g`,
  `-dff`, `-script`, `yosys-abc` with `cover` (and `cover -x` ESOP).
* **abc -sop rebuilt on v0.71:** 8-bit loadable counter → 8 `$_DFF_P_`, 9 `$sop`, 3 `$_NOT_`
  (ABC: `nd 26, cube 69, lev 1`). Sequential miter, `sat -tempinduct`: "Induction step proven:
  SUCCESS!". Combinational BLIF-vs-BLIF: "SUCCESS".

Reproduce (from the repository root):

```
git clone https://github.com/YosysHQ/yosys external/yosys          # history, ~18.6k commits
cd external/yosys
git log --diff-filter=D --format='%h %ci %an | %s' -- techlibs/coolrunner2 techlibs/greenpak4
git show caceaaa2b --stat; git tag --contains caceaaa2b | sort -V | head -1      # v0.69
git show v0.71:CHANGELOG | sed -n '/0.68 .. Yosys 0.69/,/^Yosys 0.67/p'
gh api repos/YosysHQ/yosys/pulls/6126 --jq .body
gh api repos/YosysHQ/yosys/issues/6103/comments --jq '.[] | "\(.user.login): \(.body)"'
cd ../..
# abc -sop on v0.71 (scripts in the research note §6; scratch copies in external/yosys/.scratch-cpld)
docker/yosys/run.sh sh -c 'cd external/yosys/.scratch-cpld && yosys -q sop71.ys && yosys eq.ys | grep SUCCESS'
```

## Evidence levels

* Commit hashes, dates, authors, tags, file lists, CHANGELOG text: **external** (upstream git,
  `external/yosys` at `ab83f23a2`, 2026-11-24).
* PR titles, bodies, review comments and merge times, plus repository push dates (openfpga,
  xc2bit, yosys-plugins, hoglet67/atf15xx_yosys): **external** (GitHub REST API via `gh api`,
  2026-11-24).
* The removed code's behaviour (§5 of the research note): **static**, read from
  `git show 541d24093~1:techlibs/coolrunner2/...`. Not run, because we have no v0.68 image.
* `abc -sop` reconstruction on v0.71: **verified** on one design (8-bit counter) by a SAT
  induction proof of the miter. Not run on the corpus. Confirm by running it on
  `atf15xx-synth` corpus designs through the importer.
* "No discussion or complaint exists": **inferred** from GitHub issue search (5 queries, 2026
  only). Discourse was not searched.
* "`abc -sop` maps worse than gates for us": **external to this log**, measured in worklog 05
  (2026-11-24 mapping study).

## Decisions

* **2026-11-24: recommend staying on Yosys v0.71, not pinning v0.68.** Alternatives: (a) pin
  v0.68 for `abc -sop` (only the Dockerfile ARGs change, but it costs a second image or a downgrade,
  plugin API conditionals, and no upstream fixes, for a mapping that measured worse); (b) the v0.71
  `cover` recipe if an SOP mapping is wanted; (c) our own CPLD mapping in importer/plugin (F9
  option C). (b) and (c) are compatible; (a) gains nothing that (b) doesn't give.
* **2026-11-24: research only, no code changes.** Wiring the recipe into
  `atf15xx-synth` (`--map abc-sop` on v0.71) is left to the flow owner (open issue 1).

## Journal

* **2026-11-24**: cloned upstream (no submodules). `git log --diff-filter=D` found the two techlib
  deletions; `git log -S ABC_COMMAND_SOP` found the `-sop` removal and its 2016 origin. Mapped
  commits to PRs with `git log --first-parent --merges --ancestry-path`. Read PRs
  #6126/#6132/#6135/#6137/#6103/#6146/#6061/#5782/#6101/#6077/#6182 and their comments/reviews.
  The consent comments are on #6103, not on the removal PRs themselves. #6146 ("abc: remove -lut")
  was closed unmerged; the classic `abc` pass is to become a dispatcher.
* **2026-11-24**: checked licences (ISC headers, R. Ou 2017/2020 on coolrunner2 `.cc`; `.v` files
  have no header and fall under the repo's ISC COPYING) and downstream project status (openfpga
  dormant since 2020-03-30; xc2bit 2024-07-03).
* **2026-11-24**: found that `extract_counter` supports `-dir up` (R. Ou, 2020-02-17), which
  corrects F9.2's "down-counters only". It handles no sync reset and no load.
* **2026-11-24**: rebuilt `abc -sop` on v0.71. Dead ends: (1) `write_blif` of the whole module →
  ABC keeps `.latch` but drops its clock → `read_blif` gives `$_FF_`. Fixed by `submod` of the
  non-FF cells. (2) `read_blif -sop` without `-wideports` produced bit-named ports (`q[0]`) that
  don't bind to the instance's vector ports, and the result was **wrong without any error**
  (counterexample in 2 cycles). Fixed with `-wideports`, then proven. (3) A `write_verilog`
  round trip turns `$sop` into an unknown module `\$sop`, so use RTLIL/JSON.

## Open issues

1. Wire the v0.71 `cover` recipe into `atf15xx-synth` as `--map abc-sop` (now "Yosys ≤ 0.68
   only") and run the corpus, if anyone still wants that mode. Worklog 05 says it is not the better
   mapping.
2. Try `cover -x` (ESOP) for roadmap item R2 (PT ⊕ sum).
3. F9.2 (`09-yosys-integration.md`) says `extract_counter` is "GreenPAK-shaped down-counters
   only". It has done up-counters since 2020 (async reset/CE only, no sync reset/load). The spec
   owner should correct it; not edited here because other sessions own that file.
4. Upstream may remove `read_blif -sop` or rework `abc` (PR #6135 body, #6146 comment). Recheck on
   every Yosys bump.

## Artifacts

* `docs/research/yosys-cpld-removal.md`: the findings (commits, quotes, code description,
  reuse table, recipe, recommendation).
* `external/yosys/`: upstream clone at `ab83f23a2` (git-ignored). `external/yosys/.scratch-cpld/`:
  `ctr.v`, `sop71.ys`, `eq.ys`, `eqc.ys`, outputs (untracked, disposable).
* Docker image used: `open-atf150x-yosys` (Yosys 0.71 `b4c586fd3`), via `docker/yosys/run.sh`.
