//! `atf15xx-tool`: programming files and in-circuit debugging for ATF15xx CPLDs.
//!
//! * [`isp`]: the conversions of Microchip ATMISP 7.3 (chain files, JEDEC, SVF), specified in
//!   `docs/architecture/` (§1–§11);
//! * [`dbg`] (feature `jtag`, default): in-circuit debugging over JTAG boundary scan through
//!   OpenOCD, with the Atmel BSDL files (spec D1).
//!
//! Library users that only need SVF generation (the flow) depend on this crate without default
//! features.

pub mod isp;

#[cfg(feature = "jtag")]
pub mod dbg;
