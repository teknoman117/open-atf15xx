//! Errors, warnings and the run transcript (§2.2.3, §9).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Input rejected (chain-file error, JEDEC error, validation) — exit status 1.
    Input(String),
    /// Command-line usage error — exit status 2.
    Usage(String),
    /// I/O error while writing outputs — exit status 3.
    Io(String),
}

impl Error {
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Input(_) => 1,
            Error::Usage(_) => 2,
            Error::Io(_) => 3,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Input(m) | Error::Usage(m) | Error::Io(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

/// Warnings and transcript lines collected while processing.
#[derive(Debug, Default, Clone)]
pub struct Diagnostics {
    pub warnings: Vec<String>,
    pub transcript: Vec<String>,
    /// True if a transcript step "failed" (an SVF-mode artefact, §9.3).
    pub run_failed: bool,
}

impl Diagnostics {
    pub fn warn(&mut self, m: impl Into<String>) {
        self.warnings.push(m.into());
    }
    pub fn log(&mut self, m: impl Into<String>) {
        self.transcript.push(m.into());
    }
}
