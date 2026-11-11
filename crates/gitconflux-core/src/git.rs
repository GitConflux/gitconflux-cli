//! Hardened `git` subprocess runner.
//!
//! `git` is invoked with an argv array (never through a shell), a scrubbed environment,
//! no terminal prompts, a wall-clock timeout, and bounded output capture. Network
//! credentials are delivered through `GIT_ASKPASS` and never appear in argv or URLs.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

const MAX_CAPTURE: u64 = 8 * 1024 * 1024;
#[cfg(windows)]
const NULL_DEVICE: &str = "NUL";
#[cfg(not(windows))]
const NULL_DEVICE: &str = "/dev/null";
const POLL: Duration = Duration::from_millis(20);

/// Environment variables that could redirect git to unintended repositories or configs.
const SCRUBBED_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_CONFIG",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG_COUNT",
    "GIT_EXTERNAL_DIFF",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_PROXY_COMMAND",
    "GIT_ASKPASS",
    "SSH_ASKPASS",
];

/// Credentials handed to git through the askpass helper.
#[derive(Clone)]
pub struct AskPass {
    pub program: PathBuf,
    pub host: String,
    pub username: String,
    pub token: String,
}

impl std::fmt::Debug for AskPass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AskPass")
            .field("host", &self.host)
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct Git {
    dir: PathBuf,
    timeout: Duration,
}

impl Git {
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            timeout: Duration::from_secs(120),
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Runs git locally; fails with the (sanitized) stderr on a non-zero exit.
    pub fn run(&self, args: &[&str]) -> Result<Output> {
        self.exec(args, None)
    }

    /// Runs a network command with credentials supplied via askpass.
    pub fn run_authenticated(&self, args: &[&str], auth: &AskPass) -> Result<Output> {
        self.exec(args, Some(auth))
    }

    /// Returns `Ok(true/false)` for commands whose exit status 0/1 carries meaning.
    pub fn succeeds(&self, args: &[&str]) -> Result<bool> {
        match self.exec(args, None) {
            Ok(_) => Ok(true),
            Err(Error::Git { code: Some(1), .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn exec(&self, args: &[&str], auth: Option<&AskPass>) -> Result<Output> {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.dir)
            .args([
                "-c",
                "credential.helper=",
                "-c",
                "protocol.ext.allow=never",
                "-c",
                "protocol.file.allow=never",
                "-c",
                "core.fsmonitor=false",
            ])
            .args(auth.map_or_else(Vec::new, |_| {
                // The token is in the environment: keep repository hooks from ever running.
                vec!["-c".to_owned(), format!("core.hooksPath={NULL_DEVICE}")]
            }))
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C");
        for var in SCRUBBED_ENV {
            cmd.env_remove(var);
        }
        if let Some(a) = auth {
            cmd.env("GIT_ASKPASS", &a.program)
                .env("GCFLX_ASKPASS_HOST", &a.host)
                .env("GCFLX_ASKPASS_USER", &a.username)
                .env("GCFLX_ASKPASS_TOKEN", &a.token);
        }
        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Config("`git` was not found on PATH; install Git to use this command".into())
            } else {
                Error::Io(e.to_string())
            }
        })?;
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let out_thread = std::thread::spawn(move || read_capped(stdout.as_mut()));
        let err_thread = std::thread::spawn(move || read_capped(stderr.as_mut()));

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait().map_err(|e| Error::Io(e.to_string()))? {
                Some(s) => break s,
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::Git {
                        code: None,
                        message: format!("git timed out after {}s", self.timeout.as_secs()),
                    });
                }
                None => std::thread::sleep(POLL),
            }
        };
        let stdout = out_thread
            .join()
            .map_err(|_| Error::Io("reader thread failed".into()))?;
        let stderr = err_thread
            .join()
            .map_err(|_| Error::Io("reader thread failed".into()))?;
        let stderr = clean(&String::from_utf8_lossy(&stderr));
        if status.success() {
            Ok(Output { stdout, stderr })
        } else {
            Err(Error::Git {
                code: status.code(),
                message: stderr,
            })
        }
    }
}

fn read_capped<R: Read>(reader: Option<&mut R>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(r) = reader {
        let _ = r.take(MAX_CAPTURE).read_to_end(&mut buf);
    }
    buf
}

/// Strips control characters and bounds length of text echoed from git.
fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | ' '))
        .take(2000)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// True for a full lowercase 40-character SHA-1 object ID.
#[must_use]
pub fn is_full_sha1(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Resolves a revision to a full commit ID. Option-like input is rejected to prevent
/// argument injection.
pub fn resolve_commit(git: &Git, rev: &str) -> Result<String> {
    if rev.is_empty() || rev.starts_with('-') || rev.chars().any(char::is_control) {
        return Err(Error::Config(format!("invalid revision `{}`", clean(rev))));
    }
    let spec = format!("{rev}^{{commit}}");
    let out = git.run(&[
        "rev-parse",
        "--verify",
        "--quiet",
        "--end-of-options",
        &spec,
    ])?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if is_full_sha1(&id) {
        Ok(id)
    } else {
        Err(Error::Git {
            code: None,
            message: format!("`{}` did not resolve to a SHA-1 commit", clean(rev)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_check() {
        assert!(is_full_sha1(&"a".repeat(40)));
        assert!(!is_full_sha1(&"A".repeat(40)));
        assert!(!is_full_sha1("abc"));
    }

    #[test]
    fn rejects_option_like_revisions() {
        let git = Git::new(".");
        assert!(resolve_commit(&git, "--upload-pack=evil").is_err());
        assert!(resolve_commit(&git, "").is_err());
    }

    #[test]
    fn askpass_debug_redacts() {
        let a = AskPass {
            program: "x".into(),
            host: "h".into(),
            username: "u".into(),
            token: "supersecret".into(),
        };
        assert!(!format!("{a:?}").contains("supersecret"));
    }
}
