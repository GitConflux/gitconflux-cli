//! Change Proposal metadata, local validation, and git bundle creation.

use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::error::{Error, Result};
use crate::git::{Git, is_full_sha1};

pub const MAX_BUNDLE_BYTES: u64 = 50 * 1024 * 1024;

fn bad<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Proposal(msg.into()))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub schema_version: u8,
    pub task_id: String,
    pub title: String,
    pub summary: String,
    pub base_commit: String,
    pub head_commit: String,
    pub bundle: BundleInfo,
    pub impact: Impact,
    pub tests: Tests,
    pub integration: Integration,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleInfo {
    pub format: &'static str,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Impact {
    pub change_type: String,
    pub risk_level: String,
    pub breaking: bool,
    pub areas: Vec<String>,
    pub files: Vec<FileChange>,
    pub apis: Vec<serde_json::Value>,
    pub dependencies: Vec<serde_json::Value>,
    pub migrations: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub status: &'static str,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tests {
    pub added: u32,
    pub runs: Vec<TestRun>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRun {
    pub command: String,
    pub result: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Integration {
    pub requires: Vec<String>,
    pub likely_conflicts: Vec<String>,
}

impl Metadata {
    /// Mirrors the documented server limits so invalid proposals fail before upload.
    pub fn validate(&self) -> Result<()> {
        let chars = |s: &str| s.trim().chars().count();
        if !(1..=200).contains(&chars(&self.title)) {
            return bad("title must be 1-200 characters");
        }
        if !(1..=4000).contains(&chars(&self.summary)) {
            return bad("summary must be 1-4000 characters");
        }
        if !is_full_sha1(&self.base_commit) || !is_full_sha1(&self.head_commit) {
            return bad("base and head must be full lowercase 40-character commit IDs");
        }
        if self.base_commit == self.head_commit {
            return bad("base and head commits must differ");
        }
        let b = &self.bundle;
        if b.sha256.len() != 64
            || !b
                .sha256
                .bytes()
                .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
        {
            return bad("bundle sha256 must be 64 lowercase hex characters");
        }
        if b.size_bytes == 0 || b.size_bytes > MAX_BUNDLE_BYTES {
            return bad("bundle must be between 1 byte and 50 MiB");
        }
        let i = &self.impact;
        if !matches!(
            i.change_type.as_str(),
            "feature" | "fix" | "refactor" | "docs" | "test" | "chore" | "perf"
        ) {
            return bad("change type must be feature, fix, refactor, docs, test, chore or perf");
        }
        if !matches!(i.risk_level.as_str(), "low" | "medium" | "high") {
            return bad("risk level must be low, medium or high");
        }
        if i.breaking && i.risk_level == "low" {
            return bad("breaking changes cannot have low risk");
        }
        if i.areas.len() > 50 || i.areas.iter().any(|a| a.trim().is_empty()) {
            return bad("at most 50 non-empty areas");
        }
        if i.files.is_empty() || i.files.len() > 2000 {
            return bad("a proposal must change between 1 and 2000 files");
        }
        if i.files
            .iter()
            .any(|f| f.path.is_empty() || !(1..=500).contains(&f.summary.chars().count()))
        {
            return bad("each file needs a path and a 1-500 character summary");
        }
        if self.tests.runs.len() > 100 {
            return bad("at most 100 test runs");
        }
        if self
            .tests
            .runs
            .iter()
            .any(|r| !matches!(r.result.as_str(), "passed" | "failed" | "skipped"))
        {
            return bad("test result must be passed, failed or skipped");
        }
        Ok(())
    }
}

/// Lists changed files between two commits using NUL-delimited, rename-aware output.
pub fn changed_files(git: &Git, base: &str, head: &str) -> Result<Vec<FileChange>> {
    let out = git.run(&[
        "diff",
        "--name-status",
        "-z",
        "-M",
        "--no-ext-diff",
        "--no-textconv",
        base,
        head,
        "--",
    ])?;
    parse_name_status(&out.stdout)
}

fn parse_name_status(raw: &[u8]) -> Result<Vec<FileChange>> {
    let text = std::str::from_utf8(raw)
        .map_err(|_| Error::Proposal("file paths must be valid UTF-8".into()))?;
    let mut parts = text.split('\0').filter(|p| !p.is_empty());
    let mut files = Vec::new();
    while let Some(code) = parts.next() {
        let kind = code.chars().next().unwrap_or('?');
        let mut next_path = || {
            parts
                .next()
                .map(str::to_owned)
                .ok_or_else(|| Error::Proposal("malformed git diff output".into()))
        };
        let file = match kind {
            'A' | 'C' => {
                if kind == 'C' {
                    let _from = next_path()?;
                }
                let path = next_path()?;
                FileChange {
                    summary: format!("Added {path}"),
                    path,
                    status: "added",
                    previous_path: None,
                }
            }
            'M' | 'T' => {
                let path = next_path()?;
                FileChange {
                    summary: format!("Modified {path}"),
                    path,
                    status: "modified",
                    previous_path: None,
                }
            }
            'D' => {
                let path = next_path()?;
                FileChange {
                    summary: format!("Deleted {path}"),
                    path,
                    status: "deleted",
                    previous_path: None,
                }
            }
            'R' => {
                let from = next_path()?;
                let path = next_path()?;
                FileChange {
                    summary: format!("Renamed {from} to {path}"),
                    path,
                    status: "renamed",
                    previous_path: Some(from),
                }
            }
            other => {
                return Err(Error::Proposal(format!(
                    "unsupported diff status `{other}`"
                )));
            }
        };
        files.push(FileChange {
            summary: file.summary.chars().take(500).collect(),
            ..file
        });
    }
    Ok(files)
}

/// A temporary git ref that is deleted on drop, including on error paths.
struct TempRef<'a> {
    git: &'a Git,
    name: String,
}

impl<'a> TempRef<'a> {
    fn create(git: &'a Git, target: &str) -> Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!("refs/gitconflux/cli-{}-{nanos}", std::process::id());
        git.run(&["update-ref", &name, target])?;
        Ok(Self { git, name })
    }
}

impl Drop for TempRef<'_> {
    fn drop(&mut self) {
        let _ = self.git.run(&["update-ref", "-d", &self.name]);
    }
}

/// A bundle in a private temp directory that is removed on drop.
#[derive(Debug)]
pub struct Bundle {
    _dir: TempDir,
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
}

/// Bundles `base..head` under a temporary named ref (a bare commit ID would advertise no ref).
pub fn create_bundle(git: &Git, base: &str, head: &str) -> Result<Bundle> {
    if !is_full_sha1(base) || !is_full_sha1(head) {
        return bad("bundle endpoints must be full commit IDs");
    }
    if !git.succeeds(&["merge-base", "--is-ancestor", base, head])? {
        return bad("base commit is not an ancestor of head");
    }
    let dir = tempfile::tempdir().map_err(|e| Error::Io(e.to_string()))?;
    let path = dir.path().join("proposal.bundle");
    let path_str = path
        .to_str()
        .ok_or_else(|| Error::Io("temp path is not valid UTF-8".into()))?;
    {
        let temp_ref = TempRef::create(git, head)?;
        let exclude = format!("^{base}");
        git.run(&["bundle", "create", path_str, &temp_ref.name, &exclude])?;
    }
    let (sha256, size) = digest_and_check(&path)?;
    Ok(Bundle {
        _dir: dir,
        path,
        sha256,
        size,
    })
}

fn digest_and_check(path: &PathBuf) -> Result<(String, u64)> {
    let mut file = File::open(path).map_err(|e| Error::Io(e.to_string()))?;
    let mut header: Vec<u8> = Vec::with_capacity(16);
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| Error::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        if header.len() < 16 {
            let take = n.min(16 - header.len());
            header.extend_from_slice(&buf[..take]);
        }
        size += n as u64;
        if size > MAX_BUNDLE_BYTES {
            return bad("bundle exceeds the 50 MiB limit");
        }
        hasher.update(&buf[..n]);
    }
    if !(header.starts_with(b"# v2 git bundle\n") || header.starts_with(b"# v3 git bundle\n")) {
        return bad("generated file is not a v2/v3 git bundle");
    }
    if size == 0 {
        return bad("bundle is empty");
    }
    let mut sha256 = String::with_capacity(64);
    for byte in hasher.finalize() {
        let _ = write!(sha256, "{byte:02x}");
    }
    Ok((sha256, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> Metadata {
        Metadata {
            schema_version: 1,
            task_id: "t".into(),
            title: "T".into(),
            summary: "S".into(),
            base_commit: "a".repeat(40),
            head_commit: "b".repeat(40),
            bundle: BundleInfo {
                format: "git-bundle",
                sha256: "c".repeat(64),
                size_bytes: 10,
            },
            impact: Impact {
                change_type: "feature".into(),
                risk_level: "low".into(),
                breaking: false,
                areas: vec![],
                files: vec![FileChange {
                    path: "a".into(),
                    status: "added",
                    summary: "Added a".into(),
                    previous_path: None,
                }],
                apis: vec![],
                dependencies: vec![],
                migrations: false,
            },
            tests: Tests {
                added: 0,
                runs: vec![],
            },
            integration: Integration::default(),
        }
    }

    #[test]
    fn valid_metadata_passes() {
        assert!(meta().validate().is_ok());
    }

    #[test]
    fn rejects_invalid_metadata() {
        let mut m = meta();
        m.head_commit = m.base_commit.clone();
        assert!(m.validate().is_err());
        let mut m = meta();
        m.impact.breaking = true;
        assert!(m.validate().is_err());
        let mut m = meta();
        m.bundle.sha256 = "ABC".into();
        assert!(m.validate().is_err());
        let mut m = meta();
        m.bundle.size_bytes = MAX_BUNDLE_BYTES + 1;
        assert!(m.validate().is_err());
        let mut m = meta();
        m.impact.files.clear();
        assert!(m.validate().is_err());
    }

    #[test]
    fn parses_name_status() {
        let raw = b"A\0new.rs\0M\0old.rs\0D\0gone.rs\0R100\0from.rs\0to.rs\0";
        let files = parse_name_status(raw).unwrap_or_default();
        assert_eq!(files.len(), 4);
        assert_eq!(files[3].status, "renamed");
        assert_eq!(files[3].previous_path.as_deref(), Some("from.rs"));
        assert!(parse_name_status(b"X\0a\0").is_err());
        assert!(parse_name_status(b"A\0").is_err());
    }

    #[test]
    fn serializes_camel_case_without_null_previous_path() {
        let v = serde_json::to_value(meta()).unwrap_or_default();
        assert_eq!(v["schemaVersion"], 1);
        assert_eq!(v["bundle"]["format"], "git-bundle");
        assert!(v["impact"]["files"][0].get("previousPath").is_none());
    }
}
