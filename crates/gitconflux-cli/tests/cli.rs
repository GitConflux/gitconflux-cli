#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cmd(home: &tempfile::TempDir) -> Command {
    let mut c = Command::cargo_bin("gcflx").expect("binary");
    c.env_remove("GITCONFLUX_TOKEN")
        .env_remove("GITCONFLUX_API_URL")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("APPDATA", home.path());
    c
}

async fn me_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .and(header("authorization", "Bearer gcx_topsecret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "userId": "u1", "email": null, "handle": "alice", "joinedAt": 1,
            "handleChangedAt": null, "nextHandleChangeAt": null, "profile": {}
        })))
        .mount(&server)
        .await;
    server
}

#[tokio::test(flavor = "multi_thread")]
async fn whoami_uses_env_token_and_never_echoes_it() {
    let server = me_server().await;
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args(["--api-url", &server.uri(), "whoami"])
        .env("GITCONFLUX_TOKEN", "gcx_topsecret")
        .assert()
        .success()
        .stdout("alice\n")
        .stderr(predicate::str::contains("topsecret").not());
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_status_json_reports_source_without_token() {
    let server = me_server().await;
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args(["--json", "--api-url", &server.uri(), "auth", "status"])
        .env("GITCONFLUX_TOKEN", "gcx_topsecret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"handle\":\"alice\""))
        .stdout(predicate::str::contains("topsecret").not());
}

#[test]
fn not_signed_in_exits_with_unauthorized_code() {
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args(["--api-url", "http://localhost:1", "whoami"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("not signed in"));
}

#[test]
fn plaintext_remote_http_is_rejected() {
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args(["--api-url", "http://example.com", "whoami"])
        .env("GITCONFLUX_TOKEN", "gcx_topsecret")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("only allowed for localhost"));
}

#[test]
fn login_requires_tty_or_with_token() {
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args(["auth", "login"])
        .write_stdin("")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--with-token"));
}

#[tokio::test(flavor = "multi_thread")]
async fn login_with_token_verifies_before_storing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(
                serde_json::json!({"error": "unauthorized", "message": "bad token"}),
            ),
        )
        .mount(&server)
        .await;
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args([
            "--api-url",
            &server.uri(),
            "auth",
            "login",
            "--with-token",
            "--file-store",
        ])
        .write_stdin("gcx_wrong\n")
        .assert()
        .code(3);
    assert!(
        !home.path().join("gcflx").exists(),
        "nothing may be stored on failure"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn projects_list_works_anonymously_and_renders_table() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"projects": [{
                "id": "p1", "slug": "demo", "name": "Demo\u{1b}[31m", "description": "",
                "visibility": "public", "ownerId": "u1", "ownerHandle": "alice", "createdAt": 1
            }]})),
        )
        .mount(&server)
        .await;
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args(["--api-url", &server.uri(), "projects", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SLUG"))
        .stdout(predicate::str::contains("demo"))
        .stdout(predicate::str::contains("\u{1b}").not());
}

#[test]
fn invalid_intake_status_is_rejected_by_parser() {
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args(["intake", "list", "demo", "--status", "bogus"])
        .assert()
        .code(2);
}
