//! Stable error model and process exit codes.

use thiserror::Error;

/// Process exit codes; part of the CLI's public contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    Success = 0,
    General = 1,
    Usage = 2,
    Unauthorized = 3,
    Forbidden = 4,
    NotFound = 5,
    Conflict = 6,
    Invalid = 7,
    Unavailable = 8,
    Io = 9,
    Git = 10,
    ChecksFailed = 11,
}

impl From<ExitCode> for u8 {
    fn from(code: ExitCode) -> Self {
        code as u8
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid API URL: {0}")]
    InvalidApiUrl(String),
    #[error("invalid token: {0}")]
    InvalidToken(&'static str),
    #[error("{0}")]
    Config(String),
    #[error("credential store error: {0}")]
    Credentials(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("git failed: {message}")]
    Git { code: Option<i32>, message: String },
    #[error("invalid proposal: {0}")]
    Proposal(String),
}

impl Error {
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::InvalidApiUrl(_) | Self::InvalidToken(_) | Self::Config(_) => ExitCode::Usage,
            Self::Credentials(_) | Self::Io(_) => ExitCode::Io,
            Self::Git { .. } => ExitCode::Git,
            Self::Proposal(_) => ExitCode::Invalid,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
