//! Reference implementation of the conversions performed by Microchip ATMISP 7.3 (formerly the
//! `atf15xx-tool` crate).
//!
//! The behaviour implemented here is specified in `docs/architecture/`; section numbers in
//! comments (e.g. `§7.3`) refer to that specification.

pub mod bits;
pub mod chain;
pub mod device;
pub mod diag;
pub mod fusemap;
pub mod generate;
pub mod jedec;
pub mod jedout;
pub mod svf;
pub mod timefmt;

pub use diag::{Diagnostics, Error};
