# atf15xx-arch

Architecture and fuse-map database for the ATF1502AS, ATF1504AS and ATF1508AS, derived from the
vendor fitters' output (prjbureau's fuzzers and our oracle experiments in `tools/fuzzers/`) — see
`docs/architecture/fitter/`.

* Encoding direction: `Family::{pt_addr, pt_input_column, pt_foldback_column, mc_bit_addr,
  uim_addr, uim_choices, goe_addr, goe_choices, config_addr, …}`, `McBit`, `ConfigBit`, `Source`,
  `Device` (pin maps, dedicated/JTAG/PD/GCLK3 pins for 13 devices).
* Decoding direction: `decode::decode(family, &fuses) -> Decoded` — per macrocell PT literals,
  what each PT feeds (`PtUse`), storage, clock (incl. `AlwaysHigh`), D source, output/feedback taps,
  OE; UIM and global-OE selections; global settings. Functional semantics:
  `docs/architecture/fitter/02-fuse-map.md` §F2.8.
* Data: generated at build time by `build/main.rs` into Rust statics, nothing parsed at run time:
  the fuse map from prjbureau's `database.json` and the boundary-scan parts (`bsdl`) from its BSDL
  files (the submodule `external/prjbureau`; `ATF15XX_PRJBUREAU` overrides), the devices from
  `data/devices.json` (`tools/fuzzers/fitter-pins`) and the timing tables (`timing`) from
  `data/timing.json` (`tools/fuzzers/fitter-timing`). Scripts read them through
  `atf15xx-sim device` / `fuses` (text, one fact per line).

`cargo test -p atf15xx-arch` checks that the layout partitions the fuse array and that macrocell
bits are distinct.
