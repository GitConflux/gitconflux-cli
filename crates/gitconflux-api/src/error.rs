//! API error type with sanitized server messages.

use gitconflux_core::ExitCode;
use thiserror::Error;

const MAX_MESSAGE_LEN: usize = 500;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("{status} {code}: {message}")]
    Http {
        status: u16,
        code: String,
        message: String,
    },
    #[error("network error: {0}")]
    Network(String),
    #[error("request timed out")]
    Timeout,
    #[error("response exceeded the {0} byte limit")]
    ResponseTooLarge(usize),
    #[error("invalid response from server: {0}")]
    Decode(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
}

impl ApiError {
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Http { status, .. } => match status {
                401 => ExitCode::Unauthorized,
                403 => ExitCode::Forbidden,
                404 => ExitCode::NotFound,
                409 => ExitCode::Conflict,
                422 => ExitCode::Invalid,
                503 => ExitCode::Unavailable,
                _ => ExitCode::General,
            },
            Self::Timeout | Self::Network(_) => ExitCode::Unavailable,
            Self::InvalidRequest(_) => ExitCode::Usage,
            Self::ResponseTooLarge(_) | Self::Decode(_) => ExitCode::General,
        }
    }

    /// Whether the failure is transient and safe to retry for idempotent requests.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Timeout | Self::Network(_) => true,
            Self::Http { status, .. } => matches!(status, 502..=504),
            _ => false,
        }
    }
}

/// Strips control characters (terminal escape injection) and bounds length.
#[must_use]
pub fn sanitize(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_control() || *c == ' ')
        .take(MAX_MESSAGE_LEN)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_escapes_and_truncates() {
        assert_eq!(sanitize("a\x1b[31mb\r\nc"), "a[31mbc");
        assert_eq!(sanitize(&"x".repeat(2000)).len(), MAX_MESSAGE_LEN);
    }

    #[test]
    fn exit_codes_map() {
        let e = ApiError::Http {
            status: 404,
            code: "not_found".into(),
            message: String::new(),
        };
        assert_eq!(e.exit_code(), ExitCode::NotFound);
    }
}
