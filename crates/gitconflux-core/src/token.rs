//! Secret-bearing token type: redacted in `Debug`, zeroized on drop, validated on construction.

use crate::error::{Error, Result};
use secrecy::{ExposeSecret, SecretString};

const MAX_TOKEN_LEN: usize = 4096;

pub struct Token(SecretString);

impl Token {
    /// Validates and wraps a token. Rejects empty, oversized, or non-visible-ASCII input,
    /// which also prevents header injection via CR/LF.
    pub fn new(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(Error::InvalidToken("token is empty"));
        }
        if trimmed.len() > MAX_TOKEN_LEN {
            return Err(Error::InvalidToken("token is too long"));
        }
        if !trimmed.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(Error::InvalidToken("token contains invalid characters"));
        }
        Ok(Self(SecretString::from(trimmed.to_owned())))
    }

    #[must_use]
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let t = Token::new("gcx_secret123").unwrap_or_else(|_| unreachable!());
        assert!(!format!("{t:?}").contains("secret123"));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Token::new("").is_err());
        assert!(Token::new("a\r\nX-Evil: 1").is_err());
        assert!(Token::new("has space").is_err());
        assert!(Token::new(&"a".repeat(MAX_TOKEN_LEN + 1)).is_err());
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let t = Token::new("  gcx_abc\n").unwrap_or_else(|_| unreachable!());
        assert_eq!(t.expose(), "gcx_abc");
    }
}
