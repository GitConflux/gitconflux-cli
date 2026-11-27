#![allow(clippy::expect_used)]

use std::fs;
use std::process::Command as Std;

use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Std::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn repo() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().expect("tmp");
    git(dir.path(), &["init", "-q", "-b", "main"]);
    fs::write(dir.path().join("a.txt"), "one\n").expect("write");
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "base"]);
    let base = git(dir.path(), &["rev-parse", "HEAD"]);
    fs::write(dir.path().join("b.txt"), "two\n").expect("write");
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "feature"]);
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    (dir, base, head)
}

fn submit_cmd(
    home: &tempfile::TempDir,
    repo: &tempfile::TempDir,
    base: &str,
    api: &str,
) -> Command {
    let mut c = Command::cargo_bin("gcflx").expect("binary");
    c.env_remove("GITCONFLUX_API_URL")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("GITCONFLUX_TOKEN", "gcx_topsecret")
        .args([
            "--api-url",
            api,
            "proposals",
            "submit",
            "demo",
            "--task",
            "tsk_1",
            "--agent",
            "a1",
        ])
        .args([
            "--title",
            "Add b",
            "--summary",
            "Adds b.txt",
            "--change-type",
            "feature",
        ])
        .args(["--risk-level", "low", "--base", base, "--no-push"])
        .arg("--repo")
        .arg(repo.path());
    c
}

fn proposal(status: &str) -> serde_json::Value {
    serde_json::json!({
        "proposal": {
            "id": "pr1", "projectId": "p1", "taskId": "tsk_1", "agentId": "a1",
            "status": status, "metadata": {}, "createdAt": 1, "updatedAt": 2
        },
        "checks": [{"name": "bundle", "passed": status == "tier1_passed", "message": "m"}]
    })
}

#[test]
fn dry_run_builds_valid_metadata_offline() {
    let (repo, base, head) = repo();
    let home = tempfile::tempdir().expect("tmp");
    let out = submit_cmd(&home, &repo, &base, "http://localhost:1")
        .arg("--dry-run")
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(v["baseCommit"], base);
    assert_eq!(v["headCommit"], head);
    assert_eq!(v["bundle"]["sha256"].as_str().expect("sha").len(), 64);
    assert_eq!(v["impact"]["files"][0]["path"], "b.txt");
    let refs = git(repo.path(), &["for-each-ref", "refs/gitconflux/"]);
    assert!(refs.is_empty(), "temp ref must be cleaned up");
}

#[tokio::test(flavor = "multi_thread")]
async fn submits_bundle_and_reports_success() {
    let (repo, base, _) = repo();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/demo/proposals"))
        .and(header("x-agent-id", "a1"))
        .respond_with(ResponseTemplate::new(201).set_body_json(proposal("tier1_passed")))
        .expect(1)
        .mount(&server)
        .await;
    let home = tempfile::tempdir().expect("tmp");
    submit_cmd(&home, &repo, &base, &server.uri())
        .assert()
        .success()
        .stdout(predicate::str::contains("tier1_passed"));
    let req = &server.received_requests().await.expect("reqs")[0];
    assert!(String::from_utf8_lossy(&req.body).contains("# v2 git bundle"));
}

#[tokio::test(flavor = "multi_thread")]
async fn tier1_failure_exits_nonzero_even_on_201() {
    let (repo, base, _) = repo();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/demo/proposals"))
        .respond_with(ResponseTemplate::new(201).set_body_json(proposal("tier1_failed")))
        .mount(&server)
        .await;
    let home = tempfile::tempdir().expect("tmp");
    submit_cmd(&home, &repo, &base, &server.uri())
        .assert()
        .code(11)
        .stderr(predicate::str::contains("Tier 1"));
}

#[test]
fn option_like_base_is_rejected() {
    let (repo, _, _) = repo();
    let home = tempfile::tempdir().expect("tmp");
    submit_cmd(&home, &repo, "--upload-pack=evil", "http://localhost:1")
        .arg("--dry-run")
        .assert()
        .failure();
}

#[test]
fn askpass_releases_token_only_for_expected_host() {
    let bin = assert_cmd::cargo::cargo_bin("gcflx");
    let run = |host: &str, prompt: &str| {
        Std::new(&bin)
            .arg(prompt)
            .env("GCFLX_ASKPASS_HOST", host)
            .env("GCFLX_ASKPASS_USER", "git")
            .env("GCFLX_ASKPASS_TOKEN", "test-token")
            .output()
            .expect("run")
    };
    let ok = run(
        "git.example",
        "Password for 'https://git@GIT.EXAMPLE/repo': ",
    );
    assert!(ok.status.success());
    assert_eq!(String::from_utf8_lossy(&ok.stdout).trim(), "test-token");
    let user = run("git.example", "Username for 'https://git.example': ");
    assert!(user.status.success());
    assert_eq!(String::from_utf8_lossy(&user.stdout).trim(), "git");
    let loopback = run("[::1]:8080", "Password for 'http://git@[::1]:8080': ");
    assert!(loopback.status.success());

    for (host, prompt) in [
        ("git.example", "Password for 'https://other.example': "),
        (
            "git.example",
            "Password for 'https://git.example.invalid': ",
        ),
        (
            "git.example",
            "Password for 'https://other.example/git.example': ",
        ),
        (
            "git.example",
            "Password for 'https://git.example@other.example': ",
        ),
        ("git.example", "Password for 'https://git.example:8443': "),
        ("git.example", "Password for 'http://git.example': "),
        ("git.example", "Password for 'git.example': "),
        ("git.example", "Password for 'https://git.example ': "),
        ("git.example", "Password https://git.example: "),
        ("[::1]:8080", "Password for 'http://[::1]:8081': "),
        ("", "Password for 'https://git.example': "),
    ] {
        let denied = run(host, prompt);
        assert!(!denied.status.success(), "{prompt}");
        assert!(denied.stdout.is_empty(), "{prompt}");
    }
}
