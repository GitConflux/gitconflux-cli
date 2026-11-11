#![allow(clippy::expect_used)]

use gitconflux_api::{ApiError, Client, ClientOptions};
use gitconflux_core::{ApiUrl, ExitCode, Token};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer, token: Option<&str>) -> Client {
    let base = ApiUrl::parse(&server.uri()).expect("mock url");
    let token = token.map(|t| Token::new(t).expect("token"));
    Client::new(base, token, &ClientOptions::default()).expect("client")
}

fn project_json() -> serde_json::Value {
    serde_json::json!({
        "id": "p1", "slug": "demo", "name": "Demo", "description": "",
        "visibility": "private", "ownerId": "u1", "ownerHandle": "alice", "createdAt": 1
    })
}

fn task_json(status: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "t1", "projectId": "p1", "key": null, "title": "T", "description": "D",
        "priority": 50, "labels": [], "acceptanceCriteria": [], "status": status,
        "claimedBy": "a1", "leaseExpiresAt": 99, "proposalId": null,
        "createdAt": 1, "updatedAt": 2
    })
}

#[tokio::test]
async fn sends_bearer_token_and_parses_projects() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects"))
        .and(header("authorization", "Bearer gcx_abc"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"projects": [project_json()]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let projects = client(&server, Some("gcx_abc"))
        .list_projects()
        .await
        .expect("ok");
    assert_eq!(projects[0].slug, "demo");
}

#[tokio::test]
async fn path_segments_are_encoded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/a%2Fb%3F/tasks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"tasks": []})))
        .expect(1)
        .mount(&server)
        .await;
    client(&server, None).list_tasks("a/b?").await.expect("ok");
}

#[tokio::test]
async fn dot_segments_are_rejected() {
    let server = MockServer::start().await;
    let err = client(&server, None)
        .get_project("..")
        .await
        .expect_err("rejected");
    assert!(matches!(err, ApiError::InvalidRequest(_)));
}

#[tokio::test]
async fn claim_204_is_empty_queue() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/demo/tasks/claim"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    assert!(
        client(&server, Some("t"))
            .claim_task("demo", "a1", 600)
            .await
            .expect("ok")
            .is_none()
    );
}

#[tokio::test]
async fn claim_returns_task() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/demo/tasks/claim"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"task": task_json("claimed")})),
        )
        .mount(&server)
        .await;
    let task = client(&server, Some("t"))
        .claim_task("demo", "a1", 600)
        .await
        .expect("ok");
    assert_eq!(task.expect("task").status, "claimed");
}

#[tokio::test]
async fn error_mapping_and_sanitization() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .respond_with(ResponseTemplate::new(401).set_body_json(
            serde_json::json!({"error": "unauthorized", "message": "bad\u{1b}[31m token"}),
        ))
        .mount(&server)
        .await;
    let err = client(&server, Some("t")).me().await.expect_err("401");
    assert_eq!(err.exit_code(), ExitCode::Unauthorized);
    assert!(!err.to_string().contains('\u{1b}'));
}

#[tokio::test]
async fn non_json_error_body_is_handled() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents"))
        .respond_with(ResponseTemplate::new(404).set_body_string("<html>nope</html>"))
        .mount(&server)
        .await;
    let err = client(&server, Some("t"))
        .list_agents()
        .await
        .expect_err("404");
    assert_eq!(err.exit_code(), ExitCode::NotFound);
}

#[tokio::test]
async fn get_retries_transient_failures() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"projects": []})))
        .mount(&server)
        .await;
    assert!(
        client(&server, None)
            .list_projects()
            .await
            .expect("retried")
            .is_empty()
    );
}

#[tokio::test]
async fn mutations_are_never_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/agents"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server, Some("t"))
        .register_agent(&gitconflux_api::models::NewAgent {
            name: "w",
            kind: "autonomous",
            capabilities: &[],
        })
        .await
        .expect_err("503");
    assert_eq!(err.exit_code(), ExitCode::Unavailable);
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", "http://evil.example/steal"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server, Some("t"))
        .me()
        .await
        .expect_err("redirect rejected");
    assert!(matches!(err, ApiError::Http { status: 302, .. }));
}

#[tokio::test]
async fn query_parameters_are_encoded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/demo/git/log"))
        .and(query_param("ref", "feat/x&y=1"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"commits": []})))
        .expect(1)
        .mount(&server)
        .await;
    client(&server, None)
        .git_log("demo", "feat/x&y=1", 5)
        .await
        .expect("ok");
}

#[tokio::test]
async fn debug_output_never_contains_token() {
    let server = MockServer::start().await;
    let c = client(&server, Some("gcx_supersecret"));
    assert!(!format!("{c:?}").contains("supersecret"));
}
