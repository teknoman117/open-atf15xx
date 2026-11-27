//! Error messages are user interface (the flow and the benchmarks match on them): the derived
//! `Display` (thiserror, since 2026-11-27) must print exactly what the hand-written impls did.
use atf15xx_fit::fit::{FitError, FitErrorKind};
use atf15xx_fit::tt2::ParseError;

#[test]
fn messages_are_unchanged() {
    let e = FitError::new(FitErrorKind::Capacity, "no free macrocell for node n1");
    assert_eq!(e.to_string(), "no free macrocell for node n1");
    assert!(std::error::Error::source(&e).is_none());
    assert_eq!(ParseError(12, "bad cube".into()).to_string(), "line 12: bad cube");
}
