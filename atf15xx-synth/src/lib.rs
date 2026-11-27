//! Open Verilog → ATF15xx synthesis (spec: `docs/architecture/fitter/07-yosys-flow.md`); with
//! feature `yosys`, also the Yosys plugin core (`plugin`, spec F9.7).
//!
//! ```text
//! Verilog ──yosys + data/synth_atf15xx.ys──► Yosys JSON ──yosys::import──► netlist::Design
//!   ──atf15xx_fit::search (netlist passes + strategies)──► atf15xx_fit::fit_design ──► .jed (vendor format)
//!   ──atf15xx_tool::isp::generate──► .svf
//! ```
pub mod constraints;
pub mod flow;
#[cfg(feature = "yosys")]
pub mod plugin;
pub use atf15xx_arch::part;
pub mod wordmap;
pub mod ynet;
pub mod yosys;
