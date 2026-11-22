mod askpass;
mod browser_login;
mod cli;
mod commands;
mod output;
mod read;
mod submit;
mod write;

use std::io::Write;

use clap::Parser;
use gitconflux_core::ExitCode;

fn main() -> std::process::ExitCode {
    if let Some(code) = askpass::try_run() {
        return code;
    }
    let cli = cli::Cli::parse();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "error: failed to start runtime: {e}");
            return std::process::ExitCode::from(u8::from(ExitCode::General));
        }
    };
    match runtime.block_on(commands::run(cli)) {
        Ok(()) => std::process::ExitCode::from(u8::from(ExitCode::Success)),
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "error: {e}");
            std::process::ExitCode::from(u8::from(e.exit_code()))
        }
    }
}
