use std::io::{IsTerminal, Write};
use std::time::Duration;

use gitconflux_core::{ApiUrl, Error as CoreError, Token};
use sha2::{Digest, Sha256};

use crate::cli::LoginArgs;
use crate::commands::{CliError, Result, client};

fn message(value: &str) -> Result<()> {
    writeln!(std::io::stderr(), "{value}").map_err(|error| CoreError::Io(error.to_string()).into())
}

pub async fn login(api: &ApiUrl, args: &LoginArgs) -> Result<Token> {
    if !args.no_browser && !std::io::stdin().is_terminal() {
        return Err(CliError::Usage(
            "Use --no-browser to open the link yourself, or pipe a token with --with-token".into(),
        ));
    }
    let mut website = args
        .browser_url
        .as_deref()
        .map_or_else(|| Ok(api.clone()), ApiUrl::parse)?
        .as_url()
        .clone();
    let mut random = [0u8; 32];
    getrandom::fill(&mut random)
        .map_err(|_| CoreError::Config("Could not generate a secure login verifier".into()))?;
    let verifier = Token::new(&hex::encode(random))?;
    let challenge = hex::encode(Sha256::digest(verifier.expose().as_bytes()));
    let http = client(api, None)?;
    let request = http.start_cli_login(&challenge).await?;
    if request.id.len() != 36
        || !request
            .id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        || request.code.len() != 8
        || !request.code.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !(1..=30).contains(&request.interval)
        || !(1..=300).contains(&request.expires_in)
    {
        return Err(CliError::Usage(
            "Server returned an invalid browser login request".into(),
        ));
    }
    website
        .path_segments_mut()
        .map_err(|()| CliError::Usage("Invalid browser URL".into()))?
        .pop_if_empty()
        .extend(["auth", "cli", &request.id]);
    message(&format!(
        "Open {website}\nEnter this code in the browser: {}",
        request.code
    ))?;
    if !args.no_browser && webbrowser::open(website.as_str()).is_err() {
        message("Could not open the browser. Open the link above to continue.")?;
    }
    tokio::time::timeout(Duration::from_secs(request.expires_in), async {
        loop {
            tokio::time::sleep(Duration::from_secs(request.interval)).await;
            if let Some(token) = http.exchange_cli_login(&request.id, &verifier).await? {
                return Ok(token);
            }
        }
    })
    .await
    .map_err(|_| CliError::Usage("Browser login expired. Start a new login.".into()))?
}
