//! Credential resolution and storage.
//!
//! Resolution order: `GITCONFLUX_TOKEN` environment variable, OS keychain, then the
//! owner-only credentials file. Tokens are stored per API host so a development server
//! token is never sent to production (and vice versa).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::error::{Error, Result};
use crate::token::Token;
use crate::url::ApiUrl;

pub const TOKEN_ENV: &str = "GITCONFLUX_TOKEN";
const KEYRING_SERVICE: &str = "gitconflux-cli";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Keychain,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Environment,
    Keychain,
    File,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Environment => "environment (GITCONFLUX_TOKEN)",
            Self::Keychain => "OS keychain",
            Self::File => "credentials file",
        })
    }
}

/// Identifies the credential slot for an API base URL (`host[:port]`).
fn account(api: &ApiUrl) -> String {
    let url = api.as_url();
    let host = url.host_str().unwrap_or("unknown");
    url.port()
        .map_or_else(|| host.to_owned(), |p| format!("{host}:{p}"))
}

#[derive(Debug)]
pub struct CredentialStore {
    file_path: PathBuf,
}

impl CredentialStore {
    /// Uses the platform config directory for the file fallback.
    pub fn new() -> Result<Self> {
        let dirs = ProjectDirs::from("dev", "GitConflux", "gcflx")
            .ok_or_else(|| Error::Config("cannot determine a config directory".into()))?;
        Ok(Self::with_file_path(
            dirs.config_dir().join("credentials.json"),
        ))
    }

    #[must_use]
    pub fn with_file_path(file_path: PathBuf) -> Self {
        Self { file_path }
    }

    /// Returns the token and where it came from, if any is configured.
    pub fn resolve(&self, api: &ApiUrl) -> Result<Option<(Token, Source)>> {
        if let Ok(raw) = std::env::var(TOKEN_ENV) {
            if !raw.trim().is_empty() {
                return Ok(Some((Token::new(&raw)?, Source::Environment)));
            }
        }
        let acct = account(api);
        if let Some(raw) = keychain_get(&acct)? {
            return Ok(Some((Token::new(&raw)?, Source::Keychain)));
        }
        if let Some(raw) = self.file_read()?.remove(&acct) {
            return Ok(Some((Token::new(&raw)?, Source::File)));
        }
        Ok(None)
    }

    pub fn save(&self, api: &ApiUrl, token: &Token, backend: Backend) -> Result<()> {
        let acct = account(api);
        match backend {
            Backend::Keychain => keychain_entry(&acct)?
                .set_password(token.expose())
                .map_err(|e| Error::Credentials(format!("{e} (use --file-store to fall back)"))),
            Backend::File => {
                let mut map = self.file_read()?;
                map.insert(acct, token.expose().to_owned());
                self.file_write(&map)
            }
        }
    }

    /// Removes the token from every backend. Returns whether anything was removed.
    pub fn delete(&self, api: &ApiUrl) -> Result<bool> {
        let acct = account(api);
        let mut map = self.file_read()?;
        let mut removed = map.remove(&acct).is_some();
        if removed {
            self.file_write(&map)?;
        }
        match keychain_entry(&acct)?.delete_credential() {
            Ok(()) => removed = true,
            Err(error) if is_keychain_unavailable(&error) => {}
            Err(e) => return Err(Error::Credentials(e.to_string())),
        }
        Ok(removed)
    }

    fn file_read(&self) -> Result<BTreeMap<String, String>> {
        match fs::read(&self.file_path) {
            Ok(bytes) => {
                check_permissions(&self.file_path)?;
                serde_json::from_slice(&bytes)
                    .map_err(|e| Error::Credentials(format!("corrupt credentials file: {e}")))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(Error::Io(e.to_string())),
        }
    }

    /// Atomic write: a private temp file in the same directory is renamed into place.
    fn file_write(&self, map: &BTreeMap<String, String>) -> Result<()> {
        let dir = self
            .file_path
            .parent()
            .ok_or_else(|| Error::Config("invalid credentials path".into()))?;
        fs::create_dir_all(dir).map_err(|e| Error::Io(e.to_string()))?;
        restrict_dir(dir)?;
        if map.is_empty() {
            return match fs::remove_file(&self.file_path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(Error::Io(e.to_string())),
            };
        }
        let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| Error::Io(e.to_string()))?;
        let json = serde_json::to_vec_pretty(map).map_err(|e| Error::Io(e.to_string()))?;
        tmp.write_all(&json).map_err(|e| Error::Io(e.to_string()))?;
        tmp.as_file()
            .sync_all()
            .map_err(|e| Error::Io(e.to_string()))?;
        tmp.persist(&self.file_path)
            .map_err(|e| Error::Io(e.error.to_string()))?;
        Ok(())
    }
}

fn keychain_entry(acct: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, acct).map_err(|e| Error::Credentials(e.to_string()))
}

fn is_keychain_unavailable(error: &keyring::Error) -> bool {
    matches!(
        error,
        keyring::Error::NoEntry
            | keyring::Error::NoStorageAccess(_)
            | keyring::Error::PlatformFailure(_)
    )
}

/// A missing entry is `Ok(None)`. An unavailable keychain (e.g. headless Linux) is also
/// treated as empty so the file fallback and environment remain usable.
fn keychain_get(acct: &str) -> Result<Option<String>> {
    match keychain_entry(acct)?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(error) if is_keychain_unavailable(&error) => Ok(None),
        Err(e) => Err(Error::Credentials(e.to_string())),
    }
}

#[cfg(unix)]
fn check_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .map_err(|e| Error::Io(e.to_string()))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(Error::Credentials(format!(
            "{} is accessible by other users (mode {:o}); run `chmod 600` on it",
            path.display(),
            mode & 0o777
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "shares the fallible Unix signature"
)]
fn check_permissions(_path: &Path) -> Result<()> {
    // Windows: the per-user profile directory ACL restricts access to the owner.
    Ok(())
}

#[cfg(unix)]
fn restrict_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| Error::Io(e.to_string()))
}

#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "shares the fallible Unix signature"
)]
fn restrict_dir(_dir: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api() -> ApiUrl {
        ApiUrl::parse("https://gitconflux.dev").unwrap_or_else(|_| unreachable!())
    }

    #[test]
    fn account_includes_port() {
        let a = ApiUrl::parse("http://localhost:8787").unwrap_or_else(|_| unreachable!());
        assert_eq!(account(&a), "localhost:8787");
        assert_eq!(account(&api()), "gitconflux.dev");
    }

    #[test]
    fn file_roundtrip_is_private() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let store =
            CredentialStore::with_file_path(dir.path().join("sub").join("credentials.json"));
        let token = Token::new("gcx_filetoken").unwrap_or_else(|_| unreachable!());
        assert!(store.save(&api(), &token, Backend::File).is_ok());
        let map = store.file_read().unwrap_or_default();
        assert_eq!(
            map.get("gitconflux.dev").map(String::as_str),
            Some("gcx_filetoken")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&store.file_path).map_or(0, |m| m.permissions().mode());
            assert_eq!(mode & 0o077, 0, "file must be owner-only");
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_group_readable_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let path = dir.path().join("credentials.json");
        let _ = fs::write(&path, b"{}");
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o644));
        let store = CredentialStore::with_file_path(path);
        assert!(store.file_read().is_err());
    }

    #[test]
    fn corrupt_file_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let path = dir.path().join("credentials.json");
        let _ = fs::write(&path, b"not json");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
        assert!(CredentialStore::with_file_path(path).file_read().is_err());
    }
}
