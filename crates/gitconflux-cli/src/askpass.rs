//! `GIT_ASKPASS` helper mode.
//!
//! When git needs credentials it runs this executable with the prompt as its only argument.
//! The token is read from the environment variables set by the parent `gcflx` process, and
//! is only released if the prompt names the expected host.

use std::io::Write;
use std::process::ExitCode;

pub fn try_run() -> Option<ExitCode> {
    let host = std::env::var("GCFLX_ASKPASS_HOST").ok()?;
    let mut args = std::env::args().skip(1);
    let prompt = args.next()?;
    if args.next().is_some() {
        return None;
    }
    let lower = prompt.to_ascii_lowercase();
    let is_user = lower.starts_with("username");
    let is_password = lower.starts_with("password");
    if !(is_user || is_password) || !prompt.contains(&host) {
        return Some(ExitCode::FAILURE);
    }
    let var = if is_user {
        "GCFLX_ASKPASS_USER"
    } else {
        "GCFLX_ASKPASS_TOKEN"
    };
    let value = std::env::var(var).ok()?;
    let ok = writeln!(std::io::stdout(), "{value}").is_ok();
    Some(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
