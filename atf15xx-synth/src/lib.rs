//! Open Verilog → ATF15xx synthesis (spec: `docs/architecture/fitter/07-yosys-flow.md`); with
//! feature `yosys`, also the Yosys plugin core (`plugin`, spec F9.7).
//!
//! ```text
//! Verilog ──yosys + data/synth_atf15xx.ys──► Yosys JSON ──yosys::import──► netlist::Design
//!   ──opt (limited collapse, toggle recovery)──► atf15xx_fit::fit_design ──► .jed (vendor format)
//!   ──atf15xx_tool::isp::generate──► .svf
//! ```
pub mod constraints;
pub mod flow;
pub mod opt;
#[cfg(feature = "yosys")]
pub mod plugin;
pub use atf15xx_arch::part;
pub mod tt2out;
pub mod yosys;
pub mod wordmap;
