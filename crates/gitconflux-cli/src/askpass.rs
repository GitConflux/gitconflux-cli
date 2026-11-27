//! `GIT_ASKPASS` helper mode.
//!
//! When git needs credentials it runs this executable with the prompt as its only argument.
//! The token is read from the environment variables set by the parent `gcflx` process, and
//! is only released if the prompt names the expected host.

use std::io::Write;
use std::process::ExitCode;
use url::{Host, Position, Url};

pub fn try_run() -> Option<ExitCode> {
    let host = std::env::var_os("GCFLX_ASKPASS_HOST")?;
    let mut args = std::env::args_os().skip(1);
    let ok = match (host.to_str(), args.next(), args.next()) {
        (Some(host), Some(prompt), None) => prompt
            .to_str()
            .and_then(|prompt| credential_var(prompt, host))
            .and_then(|var| std::env::var(var).ok())
            .is_some_and(|value| writeln!(std::io::stdout(), "{value}").is_ok()),
        _ => false,
    };
    Some(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn credential_var(prompt: &str, host: &str) -> Option<&'static str> {
    // Git uses these prompts in the C locale (set by the parent process).
    let (kind, raw_url) = prompt.split_once(" for '")?;
    let var = match kind {
        "Username" => "GCFLX_ASKPASS_USER",
        "Password" => "GCFLX_ASKPASS_TOKEN",
        _ => return None,
    };
    let raw_url = raw_url.strip_suffix("': ")?;
    let (scheme, authority_and_path) = raw_url.split_once("://")?;
    if !matches!(scheme, "http" | "https")
        || authority_and_path.starts_with('/')
        || raw_url.trim() != raw_url
        || raw_url.bytes().any(|b| b.is_ascii_control() || b == b'\\')
    {
        return None;
    }
    let url = Url::parse(raw_url).ok()?;
    let parsed_host = url.host()?;
    let loopback = match parsed_host {
        Host::Domain(domain) => domain == "localhost",
        Host::Ipv4(ip) => ip.is_loopback(),
        Host::Ipv6(ip) => ip.is_loopback(),
    };
    if (scheme == "http" && !loopback) || url.password().is_some() {
        return None;
    }
    // Compare the canonical host and port, excluding userinfo and path.
    (&url[Position::BeforeHost..Position::AfterPort] == host).then_some(var)
}
