use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gitconflux_core::{ApiUrl, Token};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderValue};
use reqwest::{Method, StatusCode, redirect};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use crate::error::{ApiError, sanitize};
use crate::models::{
    Account, Agent, BoardMessage, BoardPage, Catalog, ChangeProposal, CliLoginRequest, Commit,
    GitCredentials, IntakeProposal, NewAgent, NewIntake, NewProject, NewTask, Preview, Project,
    ProposalEvent, PublishResult, SubmitResult, SyncResult, Task, TokenRecord,
};

const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_GET_ATTEMPTS: u32 = 3;
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub timeout: Duration,
    pub connect_timeout: Duration,
    pub user_agent: String,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            user_agent: format!("gcflx/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

#[derive(Debug)]
pub struct Client {
    http: reqwest::Client,
    base: ApiUrl,
    token: Option<Token>,
}

impl Client {
    pub fn new(base: ApiUrl, token: Option<Token>, opts: &ClientOptions) -> Result<Self, ApiError> {
        let http = reqwest::Client::builder()
            .timeout(opts.timeout)
            .connect_timeout(opts.connect_timeout)
            .user_agent(&opts.user_agent)
            .redirect(redirect::Policy::none())
            .https_only(base.as_url().scheme() == "https")
            .build()
            .map_err(|e| ApiError::Network(sanitize(&e.to_string())))?;
        Ok(Self { http, base, token })
    }

    /// Builds a URL with each segment percent-encoded so user input cannot alter the path.
    fn url(&self, segments: &[&str]) -> Result<Url, ApiError> {
        let mut url = self.base.as_url().clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|()| ApiError::InvalidRequest("API URL cannot be a base".into()))?;
            path.pop_if_empty();
            for s in segments {
                if s.is_empty() || *s == "." || *s == ".." {
                    return Err(ApiError::InvalidRequest(format!(
                        "invalid path segment `{s}`"
                    )));
                }
                path.push(s);
            }
        }
        Ok(url)
    }

    async fn send_once(
        &self,
        method: &Method,
        url: &Url,
        body: Option<&Value>,
    ) -> Result<(StatusCode, Vec<u8>), ApiError> {
        let mut req = self
            .http
            .request(method.clone(), url.clone())
            .header(ACCEPT, "application/json");
        if let Some(token) = &self.token {
            let mut value =
                HeaderValue::from_str(&format!("Bearer {}", token.expose())).map_err(|_| {
                    ApiError::InvalidRequest("token is not a valid header value".into())
                })?;
            value.set_sensitive(true);
            req = req.header(AUTHORIZATION, value);
        }
        if let Some(body) = body {
            req = req.json(body);
        }
        read_response(req.send().await.map_err(map_reqwest)?).await
    }

    /// Uploads a proposal. Sent exactly once; a timed-out upload may have succeeded, so the
    /// caller must re-check proposal state before trying again.
    pub async fn submit_proposal(
        &self,
        slug: &str,
        agent_id: &str,
        metadata: &Value,
        bundle: Vec<u8>,
    ) -> Result<SubmitResult, ApiError> {
        let url = self.url(&["v1", "projects", slug, "proposals"])?;
        let metadata = serde_json::to_string(metadata)
            .map_err(|e| ApiError::InvalidRequest(sanitize(&e.to_string())))?;
        let part = reqwest::multipart::Part::bytes(bundle)
            .file_name("proposal.bundle")
            .mime_str("application/octet-stream")
            .map_err(|e| ApiError::InvalidRequest(sanitize(&e.to_string())))?;
        let form = reqwest::multipart::Form::new()
            .text("metadata", metadata)
            .part("bundle", part);
        let agent = HeaderValue::from_str(agent_id)
            .map_err(|_| ApiError::InvalidRequest("agent ID is not a valid header value".into()))?;
        let mut req = self
            .http
            .post(url)
            .timeout(UPLOAD_TIMEOUT)
            .header(ACCEPT, "application/json")
            .header("x-agent-id", agent)
            .multipart(form);
        req = self.authorize(req)?;
        let (status, bytes) = read_response(req.send().await.map_err(map_reqwest)?).await?;
        if !status.is_success() {
            return Err(http_error(status, &bytes));
        }
        serde_json::from_slice(&bytes).map_err(decode)
    }

    /// Requests short-lived Git credentials. The token is sensitive.
    pub async fn git_credentials(
        &self,
        slug: &str,
        scope: &str,
        ttl_seconds: u32,
    ) -> Result<GitCredentials, ApiError> {
        let body = serde_json::json!({ "scope": scope, "ttlSeconds": ttl_seconds });
        let raw: RawGitCredentials = self
            .post_key_whole(&["v1", "projects", slug, "git-credentials"], &body)
            .await?;
        let token = Token::new(&raw.token)
            .map_err(|_| ApiError::Decode("server returned an invalid Git token".into()))?;
        Ok(GitCredentials {
            remote: raw.remote,
            default_branch: raw.default_branch,
            token,
            expires_at: raw.expires_at,
            scope: raw.scope,
        })
    }

    pub async fn integrate_preview(&self, slug: &str) -> Result<Preview, ApiError> {
        self.post_key_whole(
            &["v1", "projects", slug, "integrate", "preview"],
            &serde_json::json!({}),
        )
        .await
    }

    pub async fn sync_tasks(&self, slug: &str, git_ref: &str) -> Result<SyncResult, ApiError> {
        let mut url = self.url(&["v1", "projects", slug, "tasks", "sync"])?;
        url.query_pairs_mut().append_pair("ref", git_ref);
        let (_, bytes) = self
            .send(Method::POST, url, Some(serde_json::json!({})))
            .await?;
        serde_json::from_slice(&bytes).map_err(decode)
    }

    pub async fn publish_tasks(&self, slug: &str) -> Result<PublishResult, ApiError> {
        self.post_key_whole(
            &["v1", "projects", slug, "tasks", "publish"],
            &serde_json::json!({}),
        )
        .await
    }

    async fn post_key_whole<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        body: &Value,
    ) -> Result<T, ApiError> {
        let url = self.url(segments)?;
        let (_, bytes) = self.send(Method::POST, url, Some(body.clone())).await?;
        serde_json::from_slice(&bytes).map_err(decode)
    }

    fn authorize(&self, req: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder, ApiError> {
        let Some(token) = &self.token else {
            return Ok(req);
        };
        let mut value = HeaderValue::from_str(&format!("Bearer {}", token.expose()))
            .map_err(|_| ApiError::InvalidRequest("token is not a valid header value".into()))?;
        value.set_sensitive(true);
        Ok(req.header(AUTHORIZATION, value))
    }

    /// Mutations (anything but GET) are attempted exactly once: a timed-out request may
    /// have succeeded, and the API has no idempotency keys.
    async fn send(
        &self,
        method: Method,
        url: Url,
        body: Option<Value>,
    ) -> Result<(StatusCode, Vec<u8>), ApiError> {
        let attempts = if method == Method::GET {
            MAX_GET_ATTEMPTS
        } else {
            1
        };
        let mut last = ApiError::Timeout;
        for attempt in 0..attempts {
            if attempt > 0 {
                tokio::time::sleep(backoff(attempt)).await;
            }
            let err = match self.send_once(&method, &url, body.as_ref()).await {
                Ok((status, bytes)) if status.is_success() => return Ok((status, bytes)),
                Ok((status, bytes)) => http_error(status, &bytes),
                Err(e) => e,
            };
            if !err.is_transient() {
                return Err(err);
            }
            last = err;
        }
        Err(last)
    }

    async fn request_key<T: DeserializeOwned>(
        &self,
        method: Method,
        segments: &[&str],
        body: Option<Value>,
        key: &str,
    ) -> Result<T, ApiError> {
        let url = self.url(segments)?;
        let (_, bytes) = self.send(method, url, body).await?;
        extract(&bytes, key)
    }

    async fn get_key<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        key: &str,
    ) -> Result<T, ApiError> {
        self.request_key(Method::GET, segments, None, key).await
    }

    async fn post_key<T: DeserializeOwned, B: Serialize + Sync>(
        &self,
        segments: &[&str],
        body: &B,
        key: &str,
    ) -> Result<T, ApiError> {
        let value = serde_json::to_value(body)
            .map_err(|e| ApiError::InvalidRequest(sanitize(&e.to_string())))?;
        self.request_key(Method::POST, segments, Some(value), key)
            .await
    }

    pub async fn health(&self) -> Result<(), ApiError> {
        let url = self.url(&["healthz"])?;
        self.send(Method::GET, url, None).await.map(|_| ())
    }

    pub async fn me(&self) -> Result<Account, ApiError> {
        let url = self.url(&["v1", "me"])?;
        let (_, bytes) = self.send(Method::GET, url, None).await?;
        serde_json::from_slice(&bytes).map_err(decode)
    }

    pub async fn start_cli_login(&self, challenge: &str) -> Result<CliLoginRequest, ApiError> {
        self.post_key_whole(
            &["v1", "cli-login", "start"],
            &serde_json::json!({ "challenge": challenge }),
        )
        .await
    }

    pub async fn exchange_cli_login(
        &self,
        id: &str,
        verifier: &Token,
    ) -> Result<Option<Token>, ApiError> {
        let url = self.url(&["v1", "cli-login", id, "exchange"])?;
        let body = serde_json::json!({ "verifier": verifier.expose() });
        let (status, bytes) = self.send_once(&Method::POST, &url, Some(&body)).await?;
        if status == StatusCode::ACCEPTED {
            return Ok(None);
        }
        if status != StatusCode::OK {
            return Err(http_error(status, &[]));
        }
        let raw: String = extract(&bytes, "token").map_err(|_| {
            ApiError::Decode("server returned an invalid CLI token response".into())
        })?;
        if !raw.starts_with("gcx_") {
            return Err(ApiError::Decode(
                "server returned an invalid CLI token".into(),
            ));
        }
        Token::new(&raw)
            .map(Some)
            .map_err(|_| ApiError::Decode("server returned an invalid CLI token".into()))
    }

    pub async fn list_tokens(&self) -> Result<Vec<TokenRecord>, ApiError> {
        self.get_key(&["v1", "me", "tokens"], "tokens").await
    }

    pub async fn revoke_token(&self, id: &str) -> Result<(), ApiError> {
        let url = self.url(&["v1", "me", "tokens", id])?;
        self.send(Method::DELETE, url, None).await.map(|_| ())
    }

    pub async fn list_projects(&self) -> Result<Vec<Project>, ApiError> {
        self.get_key(&["v1", "projects"], "projects").await
    }

    pub async fn get_project(&self, slug: &str) -> Result<Project, ApiError> {
        self.get_key(&["v1", "projects", slug], "project").await
    }

    pub async fn create_project(&self, new: &NewProject<'_>) -> Result<Project, ApiError> {
        self.post_key(&["v1", "projects"], new, "project").await
    }

    pub async fn list_tasks(&self, slug: &str) -> Result<Vec<Task>, ApiError> {
        self.get_key(&["v1", "projects", slug, "tasks"], "tasks")
            .await
    }

    pub async fn create_task(&self, slug: &str, new: &NewTask<'_>) -> Result<Task, ApiError> {
        self.post_key(&["v1", "projects", slug, "tasks"], new, "task")
            .await
    }

    pub async fn task_catalog(&self, slug: &str) -> Result<Catalog, ApiError> {
        let url = self.url(&["v1", "projects", slug, "tasks", "catalog"])?;
        let (_, bytes) = self.send(Method::GET, url, None).await?;
        serde_json::from_slice(&bytes).map_err(decode)
    }

    /// Returns `None` when the queue is empty (HTTP 204).
    pub async fn claim_task(
        &self,
        slug: &str,
        agent_id: &str,
        lease_seconds: u32,
    ) -> Result<Option<Task>, ApiError> {
        let url = self.url(&["v1", "projects", slug, "tasks", "claim"])?;
        let body = serde_json::json!({ "agentId": agent_id, "leaseSeconds": lease_seconds });
        let (status, bytes) = self.send(Method::POST, url, Some(body)).await?;
        if status == StatusCode::NO_CONTENT {
            return Ok(None);
        }
        extract(&bytes, "task").map(Some)
    }

    pub async fn release_task(
        &self,
        slug: &str,
        task_id: &str,
        agent_id: &str,
    ) -> Result<Task, ApiError> {
        let body = serde_json::json!({ "agentId": agent_id });
        self.post_key(
            &["v1", "projects", slug, "tasks", task_id, "release"],
            &body,
            "task",
        )
        .await
    }

    pub async fn list_agents(&self) -> Result<Vec<Agent>, ApiError> {
        self.get_key(&["v1", "agents"], "agents").await
    }

    pub async fn register_agent(&self, new: &NewAgent<'_>) -> Result<Agent, ApiError> {
        self.post_key(&["v1", "agents"], new, "agent").await
    }

    pub async fn heartbeat(&self, agent_id: &str) -> Result<Agent, ApiError> {
        let body = serde_json::json!({});
        self.post_key(&["v1", "agents", agent_id, "heartbeat"], &body, "agent")
            .await
    }

    pub async fn list_proposals(&self, slug: &str) -> Result<Vec<ChangeProposal>, ApiError> {
        self.get_key(&["v1", "projects", slug, "proposals"], "proposals")
            .await
    }

    pub async fn get_proposal(
        &self,
        slug: &str,
        id: &str,
    ) -> Result<(ChangeProposal, Vec<ProposalEvent>), ApiError> {
        let url = self.url(&["v1", "projects", slug, "proposals", id])?;
        let (_, bytes) = self.send(Method::GET, url, None).await?;
        Ok((extract(&bytes, "proposal")?, extract(&bytes, "events")?))
    }

    pub async fn list_intake(
        &self,
        slug: &str,
        status: Option<&str>,
    ) -> Result<Vec<IntakeProposal>, ApiError> {
        let mut url = self.url(&["v1", "projects", slug, "intake"])?;
        if let Some(s) = status {
            url.query_pairs_mut().append_pair("status", s);
        }
        let (_, bytes) = self.send(Method::GET, url, None).await?;
        extract(&bytes, "proposals")
    }

    pub async fn get_intake(&self, slug: &str, id: &str) -> Result<IntakeProposal, ApiError> {
        self.get_key(&["v1", "projects", slug, "intake", id], "proposal")
            .await
    }

    pub async fn submit_intake(
        &self,
        slug: &str,
        new: &NewIntake<'_>,
    ) -> Result<IntakeProposal, ApiError> {
        self.post_key(&["v1", "projects", slug, "intake"], new, "proposal")
            .await
    }

    /// Reads a page of board messages, newest first.
    pub async fn board(
        &self,
        slug: &str,
        limit: u32,
        before: Option<i64>,
    ) -> Result<BoardPage, ApiError> {
        let mut url = self.url(&["v1", "projects", slug, "board"])?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        if let Some(before) = before {
            url.query_pairs_mut()
                .append_pair("before", &before.to_string());
        }
        let (_, bytes) = self.send(Method::GET, url, None).await?;
        serde_json::from_slice(&bytes).map_err(decode)
    }

    /// Posts to a project board, as one of the caller's agents when `agent_id` is set.
    /// Sent exactly once.
    pub async fn post_board(
        &self,
        slug: &str,
        body: &str,
        agent_id: Option<&str>,
    ) -> Result<BoardMessage, ApiError> {
        let url = self.url(&["v1", "projects", slug, "board"])?;
        let mut req = self
            .http
            .post(url)
            .header(ACCEPT, "application/json")
            .json(&serde_json::json!({ "body": body }));
        if let Some(agent) = agent_id {
            let agent = HeaderValue::from_str(agent).map_err(|_| {
                ApiError::InvalidRequest("agent ID is not a valid header value".into())
            })?;
            req = req.header("x-agent-id", agent);
        }
        req = self.authorize(req)?;
        let (status, bytes) = read_response(req.send().await.map_err(map_reqwest)?).await?;
        if !status.is_success() {
            return Err(http_error(status, &bytes));
        }
        extract(&bytes, "message")
    }

    pub async fn git_log(
        &self,
        slug: &str,
        git_ref: &str,
        limit: u32,
    ) -> Result<Vec<Commit>, ApiError> {
        let mut url = self.url(&["v1", "projects", slug, "git", "log"])?;
        url.query_pairs_mut()
            .append_pair("ref", git_ref)
            .append_pair("limit", &limit.to_string());
        let (_, bytes) = self.send(Method::GET, url, None).await?;
        extract(&bytes, "commits")
    }
}

async fn read_response(mut resp: reqwest::Response) -> Result<(StatusCode, Vec<u8>), ApiError> {
    let status = resp.status();
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(map_reqwest)? {
        if buf.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(ApiError::ResponseTooLarge(MAX_RESPONSE_BYTES));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok((status, buf))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawGitCredentials {
    remote: String,
    default_branch: Option<String>,
    token: String,
    expires_at: i64,
    scope: String,
}

#[derive(serde::Deserialize)]
struct Probe {
    error: Option<String>,
    message: Option<String>,
}

fn http_error(status: StatusCode, body: &[u8]) -> ApiError {
    let probe: Option<Probe> = serde_json::from_slice(body).ok();
    let (code, message) = probe.map_or_else(
        || (String::new(), String::new()),
        |p| (p.error.unwrap_or_default(), p.message.unwrap_or_default()),
    );
    let code = if code.is_empty() {
        status
            .canonical_reason()
            .unwrap_or("error")
            .to_ascii_lowercase()
            .replace(' ', "_")
    } else {
        sanitize(&code)
    };
    ApiError::Http {
        status: status.as_u16(),
        code,
        message: sanitize(&message),
    }
}

fn extract<T: DeserializeOwned>(bytes: &[u8], key: &str) -> Result<T, ApiError> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(decode)?;
    let field = value
        .get_mut(key)
        .map(Value::take)
        .ok_or_else(|| ApiError::Decode(format!("missing `{key}` in response")))?;
    serde_json::from_value(field).map_err(decode)
}

#[allow(clippy::needless_pass_by_value)] // used as a `map_err` function pointer
fn decode(e: serde_json::Error) -> ApiError {
    ApiError::Decode(sanitize(&e.to_string()))
}

fn map_reqwest(e: reqwest::Error) -> ApiError {
    if e.is_timeout() {
        ApiError::Timeout
    } else {
        // Strip the URL: it is not secret, but keeps messages compact and free of query data.
        ApiError::Network(sanitize(&e.without_url().to_string()))
    }
}

/// Exponential backoff (200ms, 400ms, ...) with up to 100ms of jitter.
fn backoff(attempt: u32) -> Duration {
    let base = 200u64 << attempt.saturating_sub(1).min(5);
    let jitter = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::from(d.subsec_millis()) % 100);
    Duration::from_millis(base + jitter)
}
