//! Errors, warnings and the run transcript (§2.2.3, §9).

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Input rejected (chain-file error, JEDEC error, validation) — exit status 1.
    #[error("{0}")]
    Input(String),
    /// Command-line usage error — exit status 2.
    #[error("{0}")]
    Usage(String),
    /// I/O error while writing outputs — exit status 3.
    #[error("{0}")]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The derived `Display` (thiserror) prints the message only, as the hand-written impl did.
    #[test]
    fn messages_are_unchanged() {
        assert_eq!(Error::Input("bad chain".into()).to_string(), "bad chain");
        assert_eq!(Error::Usage("give --jed".into()).to_string(), "give --jed");
        assert_eq!(Error::Io("disk full".into()).exit_code(), 3);
    }
}
