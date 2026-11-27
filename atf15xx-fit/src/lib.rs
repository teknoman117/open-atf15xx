//! Fitter core for ATF150xAS CPLDs: a reusable back end.
//!
//! Pipeline: front end (`tt2::Pla`, or `atf15xx-synth`'s Yosys import) → [`netlist::Design`] (SOP
//! cells with storage, clocks, resets, OE and pin constraints) → optionally [`search::search`]
//! (the netlist passes of [`opt`] in several strategies, the best fit kept against timing
//! targets) → [`fit::fit_design`] (placement, global nets, product-term allocation, UIM routing,
//! fuse assembly) → [`jed::write`]; [`tt2out`] renders a netlist as `.tt2` for the vendor fitter.
pub mod capacity;
pub mod cli;
pub mod fit;
pub mod jed;
pub mod logic;
pub mod netlist;
pub mod opt;
pub mod report;
pub mod search;
pub mod tt2;
pub mod tt2out;

pub use fit::{fit, fit_design, FitError, FitResult, Options, Place, Preassign, Select};
