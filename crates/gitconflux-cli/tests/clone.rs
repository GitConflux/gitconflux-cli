#![allow(clippy::expect_used)]

use std::fs;
use std::process::Command as Std;

use base64::Engine;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Drives a real `git clone` through the askpass helper from a binary path containing a
/// space, and verifies the short-lived token is delivered over HTTP Basic auth only.
#[tokio::test(flavor = "multi_thread")]
async fn clone_authenticates_via_askpass_from_path_with_spaces() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/demo/git-credentials"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "remote": format!("{}/demo.git", server.uri()), "defaultBranch": "main",
            "token": "short_tok", "expiresAt": 1, "scope": "read"
        })))
        .mount(&server)
        .await;
    let basic = base64::engine::general_purpose::STANDARD.encode("git:short_tok");
    Mock::given(method("GET"))
        .and(path("/demo.git/info/refs"))
        .and(header("authorization", format!("Basic {basic}").as_str()))
        .respond_with(ResponseTemplate::new(404))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/demo.git/info/refs"))
        .respond_with(
            ResponseTemplate::new(401).insert_header("www-authenticate", "Basic realm=\"x\""),
        )
        .with_priority(10)
        .mount(&server)
        .await;

    let work = tempfile::tempdir().expect("tmp");
    let bin_dir = work.path().join("dir with space");
    fs::create_dir_all(&bin_dir).expect("mkdir");
    let bin = bin_dir.join("gcflx");
    fs::copy(assert_cmd::cargo::cargo_bin("gcflx"), &bin).expect("copy");

    let out = Std::new(&bin)
        .current_dir(work.path())
        .env("HOME", work.path())
        .env("XDG_CONFIG_HOME", work.path())
        .env("GITCONFLUX_TOKEN", "gcx_pat")
        .args(["--api-url", &server.uri(), "clone", "demo"])
        .output()
        .expect("run");

    let requests = server.received_requests().await.expect("reqs");
    let authed = requests
        .iter()
        .filter(|r| r.url.path() == "/demo.git/info/refs")
        .any(|r| {
            r.headers
                .get("authorization")
                .is_some_and(|v| v.to_str().is_ok_and(|s| s == format!("Basic {basic}")))
        });
    assert!(authed, "git never sent the askpass credentials");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!combined.contains("short_tok") && !combined.contains("gcx_pat"));
}
