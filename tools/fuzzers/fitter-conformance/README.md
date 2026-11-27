# fitter-conformance: our fitter against the vendor's

**Derives:** how closely `fit1502/fit1504/fit1508` (atf15xx-fit) reproduce the vendor fitters: the
fraction of designs fitted to the identical fuse array, the regions where the others differ, and
whether those are functionally equivalent. It is the end-to-end check of everything the other
workflows derived (fuse map, pins, options, foldbacks), because one wrong fact shows up as a
mismatching fuse.

| script | compares |
|---|---|
| `verify_tt2.py` | every `.tt2` under the given paths: the vendor's `.jed` next to it (or a fresh vendor run with `--run-vendor`) against ours with the same arguments; `identical` / `bad-checksum` / `different` with the differing fuses by region; `--hashes FILE` also writes a hash of our JED (or our error) per design, a byte-level snapshot of our fitter to diff between two versions |
| `vendor_oracle.sh` | the open Verilog flow's `.tt2` (`atf15xx-synth --emit-tt2`) fitted by the vendor, for corpus runs: fit / macrocells / equivalence next to ours |

## Run

```
cargo build --release -p atf15xx-fit
python3 tools/fuzzers/fitter-conformance/verify_tt2.py --bin-dir $PWD/target/release .scratch/oracle/feat .scratch/oracle/ptcol2 .scratch/oracle/pinroles
docker/prjbureau/run.sh sh tools/fuzzers/fitter-conformance/vendor_oracle.sh .scratch/flow/corpus/<NAME-PART>
```

`--bin-dir` must be absolute (the comparison runs in a scratch directory).

## Results (2026-11-25)

Fuse-identical: `feat` 1502 1591/1615, 1504 2146/2153, 1508 1777/1915; `ptcol2` 246 / 1003 / 3522;
`pinroles` 180 / 876 / 856. User designs 8/8 golden-equivalent. Synthetic corpus byte-identical
rates 96.8 % / 98.0 % / 91.2 %; non-identical cases sampled are functionally equivalent (59/59,
`atf15xx-sim compare`). The user's designs: `tools/sim/run_designs.sh` (formal + simulation against
the `.tt2`). Worklog: `docs/worklog/02-fitter.md`.
