mod cli;
mod commands;

use std::io::Write;

use clap::Parser;
use gitconflux_core::ExitCode;

fn main() -> std::process::ExitCode {
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
