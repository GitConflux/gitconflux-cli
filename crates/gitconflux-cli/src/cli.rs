//! Command-line definition.

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "gcflx",
    version,
    about = "The official GitConflux CLI",
    propagate_version = true
)]
pub struct Cli {
    /// API base URL (HTTPS required, except for localhost).
    #[arg(long, global = true, env = "GITCONFLUX_API_URL", value_name = "URL")]
    pub api_url: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage authentication.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Show the signed-in account.
    Whoami,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Store a personal access token (create one on the `GitConflux` website).
    Login(LoginArgs),
    /// Remove the stored token.
    Logout,
    /// Show authentication state.
    Status,
}

#[derive(Debug, Args)]
pub struct LoginArgs {
    /// Read the token from standard input instead of prompting.
    #[arg(long)]
    pub with_token: bool,
    /// Store in an owner-only file instead of the OS keychain.
    #[arg(long)]
    pub file_store: bool,
}
