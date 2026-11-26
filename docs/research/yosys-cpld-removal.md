# When and why Yosys removed its CPLD support

**As of 2026-11-24.** Research note answering: when and why did upstream Yosys lose
`synth_coolrunner2`, `synth_greenpak4` and `abc -sop`, what did that code do, what of it can an
ATF15xx flow reuse, and should we pin an older Yosys? Work log:
[`docs/worklog/07-yosys-history.md`](../worklog/07-yosys-history.md). Context:
[`docs/architecture/fitter/09-yosys-integration.md`](../architecture/fitter/09-yosys-integration.md)
§F9.2.

Sources are primary unless marked. *Git* means the upstream repository
`https://github.com/YosysHQ/yosys` cloned into `external/yosys` (git-ignored) at
`ab83f23a2412637edafdc70c72ea7050b1d06ef6` (main, 2026-11-24). *PR* means the GitHub pull request,
read with `gh api` (read-only). Quotes are verbatim.

## 1. Summary

* All three removals happened in **one week, 2026-08-17 to 2026-08-20**, by Lofty
  (GitHub `Ravenslofty`, `lofty@yosyshq.com`), as part of the **"ABC unification"** project. That
  project moves every Yosys flow from the classic `abc` pass (BLIF round trip) to `abc9`
  (XAIGER). They first shipped in **Yosys 0.69** (tag `9f75ca1f9`, 2026-09-09). **Yosys 0.68**
  (tag `38e001a6f`, 2026-08-05) is the **last release that has all of them.**
* The CoolRunner-II flow was the **only user of `abc -sop`**, and `abc -sop` "has no XAIGER
  equivalent" (PR #6126). GreenPAK4 depended on `nlutmap`, which depends on `abc -lut` behaviour
  that "cannot easily be adapted to `abc9`" (PR #6132). Both original authors agreed in public
  comments on PR #6103 before the merges (quotes in §3).
* The code was **deleted, not moved**: the commits contain only deletions (1,572 and 2,043 lines).
  There is no successor repository, plugin or branch upstream (§4).
* Still present in v0.71 and in main: the **`$sop` cell type** (simlib, simplemap, satgen),
  **`read_blif -sop`**, `extract_counter` (which **does do up-counters**, `-dir up|down|both`,
  contrary to our F9.2 wording), `abc -g`, `abc -dff`, `extract`, `dfflibmap`, `submod`, and the
  `yosys-abc` binary with ABC's `cover` command. Together these **rebuild `abc -sop` on v0.71
  without patching Yosys**. A 16-line script does it, and on a test counter the result was proven
  equivalent to the source Verilog (§6).
* **Recommendation:** do not pin Yosys ≤ 0.68 for `abc -sop`. Our own measurement (worklog 05,
  2026-11-24) already showed `abc -sop` mapping worse than the default gate mapping (spi_controller
  91 cells vs 51). If a two-level ABC cover is wanted as an option again, use the v0.71 recipe in
  §6, or move it into our plugin. The CPLD-specific work (counters, T/D choice, XOR, 5-PT / 40-input
  limits) stays in our own code, as F9 already concludes. More in §7.

## 2. The removal commits

| What | Commit | Author date (commit date) | PR, merged (GitHub `merged_at`) | First tag without it | Last tag with it | Added |
|---|---|---|---|---|---|---|
| `techlibs/coolrunner2/` + `synth_coolrunner2`, `coolrunner2_sop`, `coolrunner2_fixup` (11 files, −1,572) | `541d240930c2e2cd52dd8acd44aa36a07e9161c3` "synth_coolrunner2: remove" | 2026-08-17 15:28 +0100 (2026-08-18 10:04) | **#6126** "synth_coolrunner2: remove", merge `2578106368ed`, 2026-08-19T10:20Z | v0.69 | v0.68 | `6e0fb889f` "coolrunner2: Initial commit", Robert Ou, 2017-06-24 (first release yosys-0.8) |
| `techlibs/greenpak4/` + `synth_greenpak4`, `greenpak4_dffinv`; `passes/techmap/nlutmap.cc`; `rmports` mention (16 files, −2,043) | `edb411a62281871bc675a350d864a99eb2f6f2c3` "synth_greenpak4, nlutmap: remove" | 2026-08-18 15:13 +0100 (2026-08-19 10:56) | **#6132** "synth_greenpak4, nlutmap: remove", merge `fc6433bbae52`, 2026-08-19T10:39Z | v0.69 | v0.68 | `c5352f45c` "Added GreenPAK4 skeleton", Clifford Wolf, 2015-09-16 (yosys-0.6) |
| `abc -sop`, `-I <num>`, `-P <num>`, scratchpad `abc.sop`/`abc.I`/`abc.P`, `ABC_COMMAND_SOP` (`passes/techmap/abc.cc`, −46) | `caceaaa2bb0b2416d204540490a8efc7cb13356d` "abc: remove -sop" | 2026-08-19 12:47 +0100 | **#6135** "abc: remove -sop", merge `be69f88e557f`, 2026-08-20T09:52Z | v0.69 | v0.68 | `52bb1b968` "Added $sop cell type and "abc -sop"" + `f498204ae` "Added "abc -I <num> -P <num>"", Clifford Wolf, 2016-06-17 (yosys-0.7) |
| Context: classic-ABC mapping removed from every `synth_*`, plus `-retime/-noabc9/-abc2/-abc` options | 15 commits `44bab117d` (synth_ice40) … `4cb0a5e2f` (synth), 2026-07-22 to 2026-08-10 | | **#6103** "synth_*: remove classic ABC mapping", merged 2026-08-19T11:33Z | v0.69 | v0.68 | |
| Context: `flowmap` (LUT mapper, not CPLD) | `f0b522513` "flowmap: remove" | 2026-07-22 | **#6061**, merged 2026-07-22 | v0.68 | v0.67 | |
| Context: `abc -fast` | `45e1de36b` | 2026-03-27 | **#5782**, merged 2026-07-21 | v0.68 | v0.67 | |

Release dates (tag commit dates): v0.67 2026-07-09, v0.68 2026-08-05, v0.69 2026-09-09, v0.70
2026-10-13, v0.71 2026-11-12. Nothing was reverted. The September revert PR #6182 covers only #6101
and #6077 (xaiger2), and `techlibs/coolrunner2`, `techlibs/greenpak4` and `-sop` are all absent in
main `ab83f23a2`.

CHANGELOG, section "Yosys 0.68 .. Yosys 0.69" (file `CHANGELOG` at v0.71):

> - Removed sum of products related parameters from "abc" pass.
> - Removed coolrunner2 and greenpak4 synthesis and related passes.
> - Removed "nlutmap" pass.
> - Removed "-retime", "-noabc9", "-abc2" and "-abc" options from synthesis passes.

The docs (`docs/source`) dropped `cmd/index_techlibs_coolrunner2.rst` and
`index_techlibs_greenpak4.rst` in the same commits. Neither README nor docs in v0.68 or v0.71
mention "CPLD" anywhere. The `$sop` cell is still documented (`docs/source/cell/word_logic.rst`).

## 3. Stated rationale

**PR #6126** (synth_coolrunner2, by Ravenslofty, approved by hexratcc):

> Sort-of-related to the ABC unification work.
> Per [this comment](https://github.com/YosysHQ/yosys/pull/6103#issuecomment-5316971530) the
> author of `synth_coolrunner2` is okay with the pass being removed.
> This pass is an awkward edge case because it's the one flow using `abc -sop`, which has no XAIGER
> equivalent. Originally the plan was to keep a minimal version of the `abc` pass around to support
> this, but with @ArcaneNibble's approval, I will be able to tear out the whole `abc` pass instead.
> *Explain how this is achieved.* Yeet.

**The original author's consent** (PR #6103, comment 5316971530, ArcaneNibble, the GitHub account
of the `coolrunner2` author, R. Ou, 2026-08-17):

> I have discussed, in a private conversation with @Ravenslofty , that I am okay with
> `synth_coolrunner2` being deprecated and removed as a result of this migration.
>
> I do want sum-of-products/PLD workflows to _theoretically_ be possible in Yosys (for retrodev,
> teaching, or other purposes), but the flow as currently implemented does not actually work all
> that well. As long as these architectures don't get _completely_ conceptually forgotten, I can
> accept needing to reimplement them to a higher standard.

**PR #6132** (synth_greenpak4 + nlutmap):

> `synth_greenpak4` has `nlutmap` which iteratively maps logic to smaller LUTs to deal with the
> extremely limited resources of the chip, but `nlutmap` depends on behaviour of `abc -lut` which
> cannot easily be adapted to `abc9`. Removing both `nlutmap` and `synth_greenpak4` is the easiest
> of resolving this issue and unblocking #6103.

**GreenPAK point of contact** (PR #6103, comment 5329170455, azonenberg, Andrew Zonenberg,
2026-08-18):

> Same as above, if synth_greenpak is blocking work for more modern architectures I'm OK with it
> being removed. I can always port it over to the new abc9 flow if we find that it's necessary but
> I haven't touched a greenpak in quite a while

**PR #6135** (abc -sop):

> With #6126 merged, nothing in the codebase uses this any more, so it can be removed. I decided to
> keep `read_blif -sop` around, but maybe that's also a candidate for removal.

**The wider plan.** PR #6103 says "I intend to remove `abc -lut`, so all users of it must be moved
over to use `abc9 -lut` instead". PR #6061 (flowmap) says "I'm in the process of refactoring the
ABC mapping flows for FPGA/ASIC". The follow-up PR #6146 "abc: remove -lut" was **closed
unmerged**. Its author commented on 2026-09-02: "I think instead of removing this option, I'll turn
the `abc` pass into something like `synth_ecp5`; a pass which delegates to other passes." So in
v0.71 the classic `abc` pass still exists (`-g`, `-liberty`, `-lut`, `-dff`, `-D`, `-script`), but
upstream intends to change it.

No issue, discussion or complaint about the removals was found. A search of YosysHQ/yosys issues
and PRs created in 2026 for "coolrunner2", "abc -sop", "CPLD", "sum-of-products" and "greenpak"
returns only the removal PRs themselves. The PR template points to Discourse for larger efforts;
Discourse was not searched.

In short: **the CPLD flows were not removed for being CPLD flows.** They were the last users of
two classic-`abc` features (`-sop`, `-lut` via `nlutmap`) that block the move to a single
XAIGER/`abc9` path. Their authors had stopped working on them and agreed to the removal. Both
authors left the door open to a better reimplementation.

## 4. Was the code moved elsewhere? Are the original projects alive?

* **Not moved.** `541d24093` and `edb411a62` are pure deletions, with no additions anywhere. No
  YosysHQ repository, plugin or branch carries them. `YosysHQ/yosys-plugins` was last pushed
  2019-07-16. A GitHub repository search for `coolrunner2`, `synth_coolrunner2` and
  `greenpak4 yosys` finds no fork that carries them on. The files survive only in history, e.g.
  `git show 541d24093~1:techlibs/coolrunner2/coolrunner2_sop.cc`.
* **azonenberg/openfpga** (gp4par, gp4prog, xc2par, xc2bit, xbpar; LGPL-2.1-or-later per its
  LICENSE, while the Rust crates `src/xc2par` and `src/xc2bit` declare `BSD-2-Clause` and author
  Robert Ou): not archived, but the **last push was 2020-03-30** (`f9ae535d0`, "Merge pull request
  #137 from rqou/gp4-fixes", a fix "for new yosys JSON integers"). It is dormant. Its README still
  says "Active work is underway for Xilinx CoolRunner-II".
* **ArcaneNibble/xc2bit** (a CoolRunner-II bitstream crate, "Rewrite of existing crate, using new
  spreadsheet-powered DSL"): last push 2024-07-03. **ArcaneNibble/rust-jedec**: last push
  2024-06-12. There is no maintained xc2par successor.
* So **both consumers of `synth_coolrunner2` / `synth_greenpak4` output had been unmaintained for
  about 6 years** when the Yosys side was removed.
* Related, unaffected: `hoglet67/atf15xx_yosys` (Yosys plus the Atmel fitter via EDIF, last push
  2025-12-30) maps with `abc -liberty cells.lib`, not `-sop`. It keeps working on v0.71 as far as
  this removal is concerned.

## 5. What the removed code did

`synth_coolrunner2` (v0.68, `techlibs/coolrunner2/synth_coolrunner2.cc`, help text: "This work is
experimental. It is intended to be used with https://github.com/azonenberg/openfpga as the
place-and-route.") ran these script labels:

| Label | Commands | Purpose |
|---|---|---|
| begin/flatten/coarse | `read_verilog -lib +/coolrunner2/cells_sim.v`, `hierarchy`, `proc`, `flatten`, `tribuf -logic`, `synth -run coarse` | generic |
| fine | `extract_counter -dir up -allow_arst no`; `techmap -map +/coolrunner2/cells_counter_map.v`; `opt -fast -full`; `techmap +/techmap.v + cells_latch.v`; `dfflibmap -prepare -liberty xc2_dff.lib` | **counter extraction** before the generic mapper shreds it |
| map_tff | `abc -g AND,XOR`; `clean`; `extract -map +/coolrunner2/tff_extract.v` | **T flip-flop recovery**: ABC with only AND/XOR gates tends to leave `D = T ^ Q`, which `extract` matches against `FTCP`/`FTCP_N` (= `$_XOR_` + `$_DFFSR_`). Source comment: "This is quite hacky." |
| map_pla | `abc -sop -I 40 -P 56 [-dff -D 1]` | **two-level PLA mapping**: 40 = CoolRunner-II function-block inputs, 56 = product terms per function block |
| map_cells | `dfflibmap -liberty xc2_dff.lib`, `dffinit`, `coolrunner2_sop`, `iopadmap …`, `attrmvcp`, `coolrunner2_fixup`, `splitnets` | device cells, buffers |
| json | `write_json` | input to xc2par |

* **`coolrunner2_sop`** (234 lines, © 2017 Robert Ou): splits each `$sop` (params
  `WIDTH`, `DEPTH`, `TABLE`, 2 bits per literal) into one `ANDTERM` per product (ports `IN`/`IN_B`,
  params `TRUE_INP`/`COMP_INP`), an `ORTERM` when DEPTH > 1, and a `MACROCELL_XOR` (`IN_PTC`,
  `IN_ORTERM`, `INVERT_OUT`). It absorbs an output `$_NOT_` into `INVERT_OUT`. Single-product
  functions that drive FF `C`/`G`/`CE`/`PRE`/`CLR` become "special" product terms: the PT connects
  directly to the FF pin, and the FF cell type flips (`FDCP` ↔ `FDCP_N`, …) when inversion is
  needed.
* **`coolrunner2_fixup`** (520 lines, © 2020 R. Ou): inserts the buffers that the CR-II routing
  needs. FF D inputs must come from an XOR or an IO; clocks from a PT or `BUFG`; set/reset from a
  PT or `BUFGSR`. It also packs an input register into its IBUF when that is the only fanout.
* **`cells_counter_map.v`** (162 lines): maps `$__COUNT_` (from `extract_counter`) up-counters
  to one `FDCP` per bit plus an AND-chain XOR, i.e. T flip-flops with "all previous bits" product
  terms. `cells_sim.v` (310 lines) has simulation models for `ANDTERM`, `ORTERM`,
  `MACROCELL_XOR`, `FDCP[_N]`, `FDDCP`, `FTCP[_N]`, `FTDCP`, `FDCPE[_N]`, `FDDCPE`, `LDCP[_N]`,
  `BUFG`, `BUFGSR`, `BUFGTS`, `IBUF`, `IOBUFE`.
* **`abc -sop -I n -P m`** (classic `abc`, `ABC_COMMAND_SOP`): wrote the combinational logic as
  BLIF and ran `strash; &get -n; &fraig -x; &put; scorr; dc2; dretime; strash; dch -f; cover -I n -P m`.
  It then read the result back with `parse_blif(..., sop_mode=true)`, so each ABC node became a
  `$sop` cell with ≤ n inputs and ≤ m cubes. ABC's own help for `cover` says: "decomposition into a
  network of SOP/ESOP PLAs (this command is known to have bugs)". It also has `-x` for ESOPs (XOR
  of products), which was never exposed through Yosys.
* **`synth_greenpak4`**: LUT-based (`nlutmap` mapped to the chip's mixed 2/3/4-input LUTs), plus
  `greenpak4_dffinv` and GreenPAK counters. It is an FPGA-like flow and not relevant to sum-of-products.
* **`extract_counter`** (survives): originally `greenpak4_counters` (Andrew Zonenberg, 2017;
  renamed and made generic in `b5c15636c`, 2017-08-28). R. Ou added up-counters, inverted CE, the
  `-allow_arst` switch and parallel-output fixes on 2020-02-17 (`fec7dc5c9`, `940bab684`,
  `5fc180ed2`, `12fa4a312`, `508f1ff6a`, `7b922c0d8`) for CoolRunner-II. In v0.71 it extracts
  non-resettable or async-resettable counters (optional CE, `-dir up|down|both`, default `down`,
  `-minwidth`/`-maxwidth`, `-pout`) into `$__COUNT_` cells. It does not handle sync reset or load.
  **Nothing in v0.71 maps `$__COUNT_` any more**, so a user must supply a techmap.

## 6. Rebuilding `abc -sop` on Yosys v0.71 (no patches)

Everything `abc -sop` used except the option itself is still in v0.71. This script was run in
the pinned image (`docker/yosys/run.sh`, Yosys 0.71 `b4c586fd3`) on an 8-bit loadable counter with
enable and a terminal-count output:

```
read_verilog ctr.v
synth -noabc -top ctr
dfflegalize -cell $_DFF_P_ x
opt_clean
submod -name comb ctr/t:$_DFF_P_ %n        # all non-FF cells -> module "comb"
design -save full
delete ctr
write_blif comb.blif
!yosys-abc -q "read_blif comb.blif; strash; &get -n; &fraig -x; &put; scorr; dc2; dretime; strash; dch -f; cover -I 40 -P 56; print_stats; write_blif comb_sop.blif"
design -load full
delete comb
read_blif -sop -wideports comb_sop.blif     # -wideports is required, see below
hierarchy -top ctr
flatten
opt_clean
stat                                         # 8 $_DFF_P_, 9 $sop, 3 $_NOT_
```

ABC reported `nd = 26  cube = 69  lev = 1`. **Proof:** `miter -equiv -flatten` of the source and
the result (with `$sop` techmapped), then `sat -verify -tempinduct -prove trigger 0
-set-init-zero -seq 1` gives "Induction step proven: SUCCESS!". The combinational step alone
(`comb.blif` vs `comb_sop.blif`, `sat -prove`) also gives "SUCCESS". Pitfalls found:

* Without `-wideports`, `read_blif` creates single-bit ports named `q[0]` …, which do not bind to
  the `comb` instance's vector ports. The result was then **silently wrong**: the miter found a
  counterexample in 2 cycles (q went 0 → 0x2f with en = ld = 0). Always prove this step.
* Do not hand ABC the FFs. `write_blif` of a whole module emits `.latch`, and ABC writes it back
  without its clock, which `read_blif` turns into `$_FF_`. The `submod` cut avoids this. It is
  also what the classic `abc` pass did internally.
* The `write_verilog` → `read_verilog` round trip of `$sop` cells does not work (they come back
  as an unknown module `\$sop`). Use RTLIL or JSON. Our importer reads `$sop` from JSON already
  (`atf15xx-synth/src/yosys.rs`).
* Risk: the #6135 author called `read_blif -sop` "maybe also a candidate for removal", and the
  `abc` pass is to be reworked (#6146 comment). The recipe is good for v0.71. It depends on these
  two staying, or on our plugin carrying an equivalent.

## 7. Reuse for ATF15xx, and the pinning question

**Licence.** Yosys is ISC (`COPYING`: "Copyright (C) 2012 - 2026 Claire Xenia Wolf"). The removed
`.cc` files carry ISC headers ("Copyright (C) 2017 Robert Ou <rqou@robertou.com>" for
`coolrunner2_sop.cc`/`synth_coolrunner2.cc`, "(C) 2020 R. Ou" for `coolrunner2_fixup.cc`, Claire
Xenia Wolf 2012 for the greenpak4 `.cc` and `nlutmap.cc`). The `.v`/`.lib` files have no header
and fall under the repository's ISC licence. Copying or adapting any of it is fine if we keep the
copyright and permission notice. (openfpga's xc2par is LGPL-2.1+/BSD-2; we don't need it.)

**What maps onto the ATF15xx** (5 PTs per macrocell plus cascade and foldback, 40 UIM inputs per
LAB, 16 MCs per LAB, D/T/latch macrocell with an XOR, PT clock/reset/OE):

| Removed piece | Reuse value for us |
|---|---|
| `abc -sop -I 40 -P …` | **Low.** Measured worse than gates for us (worklog 05: spi_controller 91 cells vs 51, because ABC copies shared logic into every SOP). Its `-I` is per node, not per LAB, so it can't express our 40-input-per-block limit. If wanted as an option, use the §6 recipe on v0.71. |
| ABC `cover -x` (ESOP) | **Worth an experiment** for R2 (PT ⊕ sum): ESOPs fit the macrocell XOR. It is an ABC feature, still available through `yosys-abc`. Never used by Yosys. |
| `coolrunner2_sop` special-PT logic | **Idea reusable**: single-PT functions feeding clock/reset/CE go straight to the MC's PT clock/reset, with FF-polarity flipping for inversion. Our fitter already does PT clock/reset/OE; the Yosys-side version is about 100 lines. |
| `extract_counter -dir up` + `cells_counter_map.v` | **Partially reusable**: the pass survives in v0.71, and a ~50-line techmap of `$__COUNT_` to T-FF + AND-chain would be ours. But it handles no sync reset or load, so it misses most real counters (our `blink`, `spi_controller`). F9's plan of reading coarse `$alu`/`$adff` cells is more general. |
| `tff_extract.v` + `abc -g AND,XOR` | **Low**: our importer's toggle recovery plus the 1024-product flattening already makes T flip-flops (worklog 05, silicon chapter 9). The `extract` pattern is a neat fallback. |
| `coolrunner2_fixup` | **None directly**: CR-II routing rules (ZIA, BUFG) differ from the ATF15xx UIM. Our fitter does this. |
| `synth_greenpak4`, `nlutmap`, `greenpak4_dffinv` | **None** (LUT architecture). |

**Pinning Yosys 0.68 for `abc -sop` (option evaluated, not recommended).**

* *Possible:* v0.68 (`38e001a6ff74ca434bf4cc02c053f53619160ab0`, 2026-08-05) is the last release
  with `abc -sop`, `synth_coolrunner2` and `synth_greenpak4`. v0.67 is the last with `flowmap` and
  `abc -fast`. v0.68 already builds with CMake, so `docker/yosys/Dockerfile` would only need
  different `YOSYS_TAG`/`YOSYS_COMMIT` arguments. Our flow already supports `--map abc-sop` for it
  (`atf15xx-synth/src/flow.rs`).
* *Cost:* a second image, or a downgrade that loses the 0.69 to 0.71 fixes (e.g. the 0.71
  casez/priority-case OOM fix). Our plugin (`tools/yosys-plugin`) is written against the 0.71 API
  (`OwnedId` changes), so it would need version conditionals. A dead end for upstream fixes. And
  the only thing gained is a mapping that measured worse than our default.
* *Better:* stay on v0.71. Rebuild the SOP cover if needed with §6 (already proven on v0.71), and
  keep the CPLD-specific work in our own importer and plugin (F9 option C). ArcaneNibble's comment
  ("I can accept needing to reimplement them to a higher standard") suggests a well-made
  sum-of-products/CPLD pass would be welcome upstream one day. That is not something this project
  does now (no upstream contact).

**Correction for F9.2:** `extract_counter` is not "down-counters only". It has done up-counters
since `fec7dc5c9` (2020-02-17); its default is `-dir down`. It handles only non-resettable or
async-reset counters, with no sync reset or load.

## 8. Reproduce

```
git clone https://github.com/YosysHQ/yosys external/yosys && cd external/yosys
git log --diff-filter=D --format='%h %ci %an | %s' -- techlibs/coolrunner2 techlibs/greenpak4
git log -S'ABC_COMMAND_SOP' --format='%h %ad %an | %s' -- passes/techmap/abc.cc
for c in 541d24093 edb411a62 caceaaa2b; do
  git log --first-parent --merges --ancestry-path --format='%h %s' $c..origin/HEAD | tail -1
  git tag --contains $c | sort -V | head -1; done
git ls-tree -r --name-only v0.68 | grep -E 'coolrunner2|greenpak4'      # present
git grep -n '"-sop"' v0.68 v0.71 -- passes/techmap/abc.cc frontends/blif  # abc in 0.68 only
git show v0.71:CHANGELOG | sed -n '/0.68 .. Yosys 0.69/,/^Yosys 0.67/p'
gh api repos/YosysHQ/yosys/pulls/6126 --jq .body        # also 6132 6135 6103 6146 6061
gh api repos/YosysHQ/yosys/issues/6103/comments --jq '.[] | "\(.user.login): \(.body)"'
```

The §6 scripts were kept in `external/yosys/.scratch-cpld/` (`sop71.ys`, `eq.ys`, `eqc.ys`, `ctr.v`;
scratch, git-ignored). Run: `docker/yosys/run.sh sh -c 'cd external/yosys/.scratch-cpld && yosys -q
sop71.ys && yosys eq.ys | grep SUCCESS'`. The full script text is in §6 above. `eq.ys` reads
`ctr.v` and the written `sop71_out.il` (from `write_rtlil`), techmaps the `$sop` cells, builds
`miter -equiv -flatten -make_assert gold gate miter`, flattens, and runs the `sat` line quoted in §6.
