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
    /// Browse projects.
    #[command(subcommand)]
    Projects(ProjectsCommand),
    /// Browse tasks.
    #[command(subcommand)]
    Tasks(TasksCommand),
    /// List your registered agents.
    #[command(subcommand)]
    Agents(AgentsCommand),
    /// Browse change proposals.
    #[command(subcommand)]
    Proposals(ProposalsCommand),
    /// Browse bug/enhancement intake proposals.
    #[command(subcommand)]
    Intake(IntakeCommand),
    /// Show a project's commit history.
    Log(LogArgs),
}

#[derive(Debug, Subcommand)]
pub enum ProjectsCommand {
    /// List visible projects.
    List,
    /// Show one project.
    Show { slug: String },
}

#[derive(Debug, Subcommand)]
pub enum TasksCommand {
    /// List a project's tasks.
    List { project: String },
    /// Show a project's task catalog.
    Catalog { project: String },
}

#[derive(Debug, Subcommand)]
pub enum AgentsCommand {
    /// List your agents.
    List,
}

#[derive(Debug, Subcommand)]
pub enum ProposalsCommand {
    /// List a project's change proposals.
    List { project: String },
    /// Show a proposal and its events.
    Show { project: String, id: String },
}

#[derive(Debug, Subcommand)]
pub enum IntakeCommand {
    /// List intake proposals.
    List {
        project: String,
        #[arg(long, value_parser = ["pending", "approved", "declined", "duplicate"])]
        status: Option<String>,
    },
    /// Show one intake proposal.
    Show { project: String, id: String },
}

#[derive(Debug, Args)]
pub struct LogArgs {
    pub project: String,
    /// Git ref to read.
    #[arg(long = "ref", default_value = "main")]
    pub git_ref: String,
    /// Maximum commits to return.
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=500))]
    pub limit: u32,
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
