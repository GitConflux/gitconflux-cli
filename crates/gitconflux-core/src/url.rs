//! API base URL validation: HTTPS required, plaintext HTTP only for loopback development.

use crate::error::{Error, Result};
use url::{Host, Url};

pub const DEFAULT_API_URL: &str = "https://gitconflux.dev";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiUrl(Url);

impl ApiUrl {
    pub fn parse(raw: &str) -> Result<Self> {
        let mut url = Url::parse(raw).map_err(|e| Error::InvalidApiUrl(e.to_string()))?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(Error::InvalidApiUrl(
                "credentials in URL are not allowed".into(),
            ));
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(Error::InvalidApiUrl(
                "query and fragment are not allowed".into(),
            ));
        }
        let loopback = match url.host() {
            Some(Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
            Some(Host::Ipv4(ip)) => ip.is_loopback(),
            Some(Host::Ipv6(ip)) => ip.is_loopback(),
            None => return Err(Error::InvalidApiUrl("missing host".into())),
        };
        match url.scheme() {
            "https" => {}
            "http" if loopback => {}
            "http" => {
                return Err(Error::InvalidApiUrl(
                    "plaintext HTTP is only allowed for localhost".into(),
                ));
            }
            other => {
                return Err(Error::InvalidApiUrl(format!(
                    "unsupported scheme `{other}`"
                )));
            }
        }
        let trimmed = url.path().trim_end_matches('/').to_owned();
        url.set_path(&trimmed);
        Ok(Self(url))
    }

    #[must_use]
    pub fn as_url(&self) -> &Url {
        &self.0
    }
}

impl Default for ApiUrl {
    fn default() -> Self {
        Self(Url::parse(DEFAULT_API_URL).unwrap_or_else(|_| unreachable!("constant is valid")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_https_and_loopback_http() {
        assert!(ApiUrl::parse("https://gitconflux.dev").is_ok());
        assert!(ApiUrl::parse("http://localhost:8787").is_ok());
        assert!(ApiUrl::parse("http://127.0.0.1:8787").is_ok());
        assert!(ApiUrl::parse("http://[::1]:8787").is_ok());
    }

    #[test]
    fn rejects_unsafe_urls() {
        for bad in [
            "http://gitconflux.dev",
            "http://localhost.evil.com",
            "ftp://gitconflux.dev",
            "https://user:pw@gitconflux.dev",
            "https://gitconflux.dev?x=1",
            "not a url",
        ] {
            assert!(ApiUrl::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn default_is_valid() {
        let _ = ApiUrl::default();
    }
}
