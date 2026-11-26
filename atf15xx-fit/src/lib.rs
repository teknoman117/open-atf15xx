//! Fitter core for ATF150xAS CPLDs: a reusable back end.
//!
//! Pipeline: front end (`tt2::Pla` today; Yosys BLIF/JSON later) → [`netlist::Design`] (SOP cells
//! with storage, clocks, resets, OE and pin constraints) → [`fit::fit_design`] (placement, global
//! nets, product-term allocation, UIM routing, fuse assembly) → [`jed::write`].
pub mod cli;
pub mod fit;
pub mod jed;
pub mod logic;
pub mod netlist;
pub mod report;
pub mod tt2;

pub use fit::{fit, fit_design, FitError, FitResult, Options, Place, Preassign, Select};
