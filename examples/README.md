# Examples

Verilog designs built with the open flow (`atf15xx-synth`), each with its constraints, a README and,
where it runs on the bench, the tooling to exercise it.

| example | device | what |
|---|---|---|
| [`spi/`](spi/) | ATF1504AS-PLCC44, ATF1508AS-PLCC84 | an SPI master peripheral for an 8-bit bus (the user's `atf1504as-spi` CUPL design), with a bench variant driven over JTAG and the Uno as SPI slave |
| [`shift-register/`](shift-register/) | ATF1508AS-PLCC84 | the breakout-atf1508as board's example: a 64-bit SPI-fed shift register with 64 open-collector outputs (128/128 macrocells), with the board's Arduino sketch |
| [`spm/`](spm/) | ATF1504AS-PLCC44 | SELFPROGRAMBOARD: the CPLD of the user's P80C550 self-programming board, a dense real design (60/64 macrocells) |

Build any of them with `docker/yosys/run.sh cargo run --release -p atf15xx-synth -- build ...` (each
README has the exact line); the outputs are `.jed`, `.svf`, `.fit`, `.pin`, `.map.json` and a timing
report. Equivalence against the Verilog: `tools/sim/equiv.py` (docs/architecture/sim/05-harness.md).
