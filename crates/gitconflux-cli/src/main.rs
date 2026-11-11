use clap::Parser;
use gitconflux_core::ExitCode;

#[derive(Debug, Parser)]
#[command(name = "gcflx", version, about = "The official GitConflux CLI")]
struct Cli {}

fn main() -> std::process::ExitCode {
    let _cli = Cli::parse();
    std::process::ExitCode::from(u8::from(ExitCode::Success))
}
