#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;
use sha2::{Digest, Sha256};
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
async fn browser_login_exchanges_the_matching_proof_and_saves_the_token() {
    let server = me_server().await;
    let home = tempfile::tempdir().expect("tmp");
    let id = "91632acd-812f-4ac4-813e-cf5aeae2c1c4";
    let exchange = format!("/v1/cli-login/{id}/exchange");
    Mock::given(method("POST"))
        .and(path("/v1/cli-login/start"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": id, "code": "005cfe12", "interval": 1, "expiresIn": 5
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(&exchange))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "gcx_topsecret"
        })))
        .expect(1)
        .mount(&server)
        .await;
    cmd(&home)
        .args([
            "--api-url",
            &server.uri(),
            "auth",
            "login",
            "--no-browser",
            "--file-store",
        ])
        .assert()
        .success()
        .stdout("Signed in as alice\n")
        .stderr(predicate::str::contains(format!("/auth/cli/{id}")))
        .stderr(predicate::str::contains("005cfe12"))
        .stderr(predicate::str::contains("topsecret").not());
    cmd(&home)
        .args(["--api-url", &server.uri(), "whoami"])
        .assert()
        .success()
        .stdout("alice\n");

    let requests = server.received_requests().await.expect("recorded requests");
    let input = |route: &str| {
        let request = requests
            .iter()
            .find(|request| request.url.path() == route)
            .expect("request");
        serde_json::from_slice::<serde_json::Value>(&request.body).expect("request JSON")
    };
    let start = input("/v1/cli-login/start");
    let redeem = input(&exchange);
    let verifier = redeem["verifier"].as_str().expect("proof");
    assert_eq!(verifier.len(), 64);
    assert_eq!(
        start["challenge"],
        hex::encode(Sha256::digest(verifier.as_bytes()))
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

#[tokio::test(flavor = "multi_thread")]
async fn claim_with_empty_queue_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/demo/tasks/claim"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args([
            "--api-url",
            &server.uri(),
            "tasks",
            "claim",
            "demo",
            "--agent",
            "a1",
        ])
        .env("GITCONFLUX_TOKEN", "gcx_topsecret")
        .assert()
        .success()
        .stdout("No tasks available.\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_input_never_reaches_the_server() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(201))
        .expect(0)
        .mount(&server)
        .await;
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args([
            "--api-url",
            &server.uri(),
            "projects",
            "create",
            "Bad_Slug",
            "--name",
            "x",
        ])
        .env("GITCONFLUX_TOKEN", "gcx_topsecret")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("slug"));
}

#[test]
fn mutations_require_sign_in() {
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args(["--api-url", "http://localhost:1", "agents", "register", "w"])
        .assert()
        .code(3);
}

#[test]
fn lease_out_of_range_is_rejected() {
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args([
            "tasks",
            "claim",
            "demo",
            "--agent",
            "a",
            "--lease-seconds",
            "5",
        ])
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn integrate_preview_prints_advisory_banner() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/demo/integrate/preview"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "baseCommit": "a", "matrixCommit": "b",
            "proposals": [{"proposalId": "pr1", "status": "conflict", "files": ["x.rs"]}]
        })))
        .mount(&server)
        .await;
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args(["--api-url", &server.uri(), "integrate", "preview", "demo"])
        .env("GITCONFLUX_TOKEN", "gcx_t")
        .assert()
        .success()
        .stdout(predicate::str::contains("ADVISORY ONLY"))
        .stdout(predicate::str::contains("x.rs"));
}

#[tokio::test(flavor = "multi_thread")]
async fn tokens_list_never_shows_secrets() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me/tokens"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tokens": [{"id": "tk1", "name": "CLI", "createdAt": 1, "lastUsedAt": null}]
        })))
        .mount(&server)
        .await;
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args(["--api-url", &server.uri(), "tokens", "list"])
        .env("GITCONFLUX_TOKEN", "gcx_topsecret")
        .assert()
        .success()
        .stdout(predicate::str::contains("tk1"))
        .stdout(predicate::str::contains("topsecret").not());
}

#[tokio::test(flavor = "multi_thread")]
async fn proposals_watch_stops_at_terminal_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/demo/proposals/pr1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "proposal": {"id": "pr1", "projectId": "p", "taskId": "t", "agentId": "a",
                "status": "tier1_failed", "metadata": {}, "createdAt": 1, "updatedAt": 2},
            "events": []
        })))
        .expect(1)
        .mount(&server)
        .await;
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args([
            "--api-url",
            &server.uri(),
            "proposals",
            "show",
            "demo",
            "pr1",
            "--watch",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("tier1_failed"));
}

#[tokio::test(flavor = "multi_thread")]
async fn tasks_sync_reports_counts() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/demo/tasks/sync"))
        .and(wiremock::matchers::query_param("ref", "main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "created": ["a"], "updated": [], "unchanged": ["b", "c"], "skipped": []
        })))
        .mount(&server)
        .await;
    let home = tempfile::tempdir().expect("tmp");
    cmd(&home)
        .args(["--api-url", &server.uri(), "tasks", "sync", "demo"])
        .env("GITCONFLUX_TOKEN", "gcx_t")
        .assert()
        .success()
        .stdout("created: 1, updated: 0, unchanged: 2, skipped: 0\n");
}
