//! Command implementations.

use std::io::{Read, Write};

use gitconflux_api::{ApiError, Client, ClientOptions, sanitize};
use gitconflux_core::{ApiUrl, Backend, CredentialStore, Error as CoreError, ExitCode, Token};

use crate::cli::{AuthCommand, Cli, Command, LoginArgs};

const MAX_STDIN_TOKEN_BYTES: u64 = 8 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    ChecksFailed(String),
    #[error("not signed in; run `gcflx auth login` or set GITCONFLUX_TOKEN")]
    NotSignedIn,
}

impl CliError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Core(e) => e.exit_code(),
            Self::Api(e) => e.exit_code(),
            Self::Usage(_) => ExitCode::Usage,
            Self::ChecksFailed(_) => ExitCode::ChecksFailed,
            Self::NotSignedIn => ExitCode::Unauthorized,
        }
    }
}

pub type Result<T> = std::result::Result<T, CliError>;

pub fn out(line: &str) -> Result<()> {
    writeln!(std::io::stdout(), "{line}").map_err(|e| CliError::Core(CoreError::Io(e.to_string())))
}

fn api_url(cli: &Cli) -> Result<ApiUrl> {
    Ok(cli
        .api_url
        .as_deref()
        .map_or_else(|| Ok(ApiUrl::default()), ApiUrl::parse)?)
}

pub async fn run(cli: Cli) -> Result<()> {
    let api = api_url(&cli)?;
    let store = CredentialStore::new()?;
    match &cli.command {
        Command::Auth(AuthCommand::Login(args)) => login(&api, &store, args, cli.json).await,
        Command::Auth(AuthCommand::Logout) => logout(&api, &store),
        Command::Auth(AuthCommand::Status) => status(&api, &store, cli.json).await,
        Command::Whoami => whoami(&api, &store, cli.json).await,
        other => crate::read::run(other, &api, &store, cli.json).await,
    }
}

/// Mutations always require credentials.
pub fn authed_client(api: &ApiUrl, store: &CredentialStore) -> Result<Client> {
    let (token, _) = require_token(api, store)?;
    client(api, Some(token))
}

/// Public projects permit anonymous reads, so a missing token is not an error here.
pub fn optional_client(api: &ApiUrl, store: &CredentialStore) -> Result<Client> {
    client(api, store.resolve(api)?.map(|(t, _)| t))
}

fn read_token() -> Result<Token> {
    let mut buf = String::new();
    std::io::stdin()
        .take(MAX_STDIN_TOKEN_BYTES)
        .read_to_string(&mut buf)
        .map_err(|e| CliError::Core(CoreError::Io(e.to_string())))?;
    Ok(Token::new(&buf)?)
}

pub fn client(api: &ApiUrl, token: Option<Token>) -> Result<Client> {
    Ok(Client::new(api.clone(), token, &ClientOptions::default())?)
}

async fn login(api: &ApiUrl, store: &CredentialStore, args: &LoginArgs, json: bool) -> Result<()> {
    let token = if args.with_token {
        read_token()?
    } else {
        crate::browser_login::login(api, args).await?
    };
    // Verify before persisting so a typo never overwrites a working credential.
    let verifier = Token::new(token.expose())?;
    let account = client(api, Some(verifier))?.me().await?;
    let backend = if args.file_store {
        Backend::File
    } else {
        Backend::Keychain
    };
    store.save(api, &token, backend)?;
    let handle = sanitize(&account.handle);
    if json {
        return out(&serde_json::json!({ "handle": handle }).to_string());
    }
    out(&format!("Signed in as {handle}"))
}

fn logout(api: &ApiUrl, store: &CredentialStore) -> Result<()> {
    if store.delete(api)? {
        out("Signed out; stored token removed.")
    } else {
        out("No stored token found.")
    }
}

pub fn require_token(
    api: &ApiUrl,
    store: &CredentialStore,
) -> Result<(Token, gitconflux_core::Source)> {
    store.resolve(api)?.ok_or(CliError::NotSignedIn)
}

async fn status(api: &ApiUrl, store: &CredentialStore, json: bool) -> Result<()> {
    let (token, source) = require_token(api, store)?;
    let account = client(api, Some(token))?.me().await?;
    if json {
        let v = serde_json::json!({
            "api": api.as_url().as_str(),
            "source": source.to_string(),
            "handle": sanitize(&account.handle),
        });
        return out(&v.to_string());
    }
    out(&format!("API:    {}", api.as_url()))?;
    out(&format!("Token:  {source}"))?;
    out(&format!("User:   {}", sanitize(&account.handle)))
}

async fn whoami(api: &ApiUrl, store: &CredentialStore, json: bool) -> Result<()> {
    let (token, _) = require_token(api, store)?;
    let account = client(api, Some(token))?.me().await?;
    if json {
        let v = serde_json::to_string(&account)
            .map_err(|e| CliError::Core(CoreError::Io(e.to_string())))?;
        return out(&v);
    }
    out(&sanitize(&account.handle))
}
