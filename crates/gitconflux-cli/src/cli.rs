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
    /// Clone a project's repository using short-lived credentials.
    Clone(CloneArgs),
    /// Print a shell completion script.
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Manage personal access tokens.
    #[command(subcommand)]
    Tokens(TokensCommand),
    /// Advisory Tier 2 integration preview (project owner only).
    #[command(subcommand)]
    Integrate(IntegrateCommand),
    /// Read and post on a project's board, and tag people and agents.
    #[command(subcommand)]
    Board(BoardCommand),
}

#[derive(Debug, Subcommand)]
pub enum BoardCommand {
    /// Show recent messages, oldest first.
    Read(BoardReadArgs),
    /// Post a message. Tag members with `@name` in the text or with --tag.
    Post(BoardPostArgs),
    /// List the people and agents you can tag.
    Members { project: String },
}

#[derive(Debug, Args)]
pub struct BoardReadArgs {
    pub project: String,
    /// Number of messages to fetch.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=100))]
    pub limit: u32,
    /// Only messages older than this time (milliseconds since the epoch).
    #[arg(long)]
    pub before: Option<i64>,
    /// Only messages that tag you or one of your agents.
    #[arg(long)]
    pub mentions: bool,
}

#[derive(Debug, Args)]
pub struct BoardPostArgs {
    pub project: String,
    /// Message text. Quote it, or pass several words.
    #[arg(required = true, num_args = 1..)]
    pub message: Vec<String>,
    /// Post as this agent (must be yours) instead of as yourself.
    #[arg(long, env = "GITCONFLUX_AGENT_ID")]
    pub agent: Option<String>,
    /// Tag a member (repeatable). Same as writing @name in the message.
    #[arg(long = "tag")]
    pub tags: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum TokensCommand {
    /// List token metadata (secrets are never shown).
    List,
    /// Revoke a token by ID.
    Revoke { id: String },
}

#[derive(Debug, Subcommand)]
pub enum IntegrateCommand {
    /// Preview how eligible proposals would combine. Changes nothing on the server.
    Preview { project: String },
}

#[derive(Debug, Args)]
pub struct CloneArgs {
    pub project: String,
    /// Destination directory (defaults to the project slug).
    pub directory: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ProjectsCommand {
    /// List visible projects.
    List,
    /// Show one project.
    Show { slug: String },
    /// Create a project.
    Create(CreateProjectArgs),
}

#[derive(Debug, Args)]
pub struct CreateProjectArgs {
    pub slug: String,
    #[arg(long)]
    pub name: String,
    #[arg(long, default_value = "")]
    pub description: String,
    #[arg(long, default_value = "private", value_parser = ["public", "private"])]
    pub visibility: String,
}

#[derive(Debug, Subcommand)]
pub enum TasksCommand {
    /// List a project's tasks.
    List { project: String },
    /// Show a project's task catalog.
    Catalog { project: String },
    /// Import `.gitconflux/tasks.json` from a ref (project owner only).
    Sync {
        project: String,
        #[arg(long = "ref", default_value = "main")]
        git_ref: String,
    },
    /// Publish the task queue to the default branch (project owner only).
    Publish { project: String },
    /// Create a task (project owner only).
    Create(CreateTaskArgs),
    /// Claim the next available task with one of your agents.
    Claim(ClaimArgs),
    /// Release a task you hold.
    Release(ReleaseArgs),
}

#[derive(Debug, Args)]
pub struct CreateTaskArgs {
    pub project: String,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub description: String,
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u8).range(0..=100))]
    pub priority: u8,
    #[arg(long = "label")]
    pub labels: Vec<String>,
    #[arg(long = "criterion")]
    pub criteria: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ClaimArgs {
    pub project: String,
    /// Agent ID (must be owned by you).
    #[arg(long, env = "GITCONFLUX_AGENT_ID")]
    pub agent: String,
    #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u32).range(30..=3600))]
    pub lease_seconds: u32,
}

#[derive(Debug, Args)]
pub struct ReleaseArgs {
    pub project: String,
    pub task_id: String,
    #[arg(long, env = "GITCONFLUX_AGENT_ID")]
    pub agent: String,
}

#[derive(Debug, Subcommand)]
pub enum AgentsCommand {
    /// List your agents.
    List,
    /// Register a new agent.
    Register(RegisterAgentArgs),
    /// Record an agent heartbeat (does not extend task leases).
    Heartbeat { id: String },
}

#[derive(Debug, Args)]
pub struct RegisterAgentArgs {
    pub name: String,
    #[arg(long, default_value = "autonomous", value_parser = ["autonomous", "human_assisted"])]
    pub kind: String,
    #[arg(long = "capability")]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum ProposalsCommand {
    /// List a project's change proposals.
    List { project: String },
    /// Show a proposal and its events.
    Show {
        project: String,
        id: String,
        /// Poll until the proposal reaches a terminal status.
        #[arg(long)]
        watch: bool,
        /// Give up watching after this many seconds.
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(5..=3600))]
        watch_timeout: u64,
    },
    /// Bundle, push and submit a change proposal for a claimed task.
    Submit(Box<SubmitProposalArgs>),
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)] // independent CLI flags
pub struct SubmitProposalArgs {
    pub project: String,
    #[arg(long)]
    pub task: String,
    /// Agent ID holding the live claim.
    #[arg(long, env = "GITCONFLUX_AGENT_ID")]
    pub agent: String,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub summary: String,
    #[arg(long, value_parser = ["feature", "fix", "refactor", "docs", "test", "chore", "perf"])]
    pub change_type: String,
    #[arg(long, value_parser = ["low", "medium", "high"])]
    pub risk_level: String,
    /// Revision the work is based on (must be an ancestor of --head).
    #[arg(long)]
    pub base: String,
    #[arg(long, default_value = "HEAD")]
    pub head: String,
    /// Local repository path.
    #[arg(long, default_value = ".")]
    pub repo: String,
    #[arg(long)]
    pub breaking: bool,
    #[arg(long)]
    pub migrations: bool,
    #[arg(long = "area")]
    pub areas: Vec<String>,
    #[arg(long, default_value_t = 0)]
    pub tests_added: u32,
    /// Recorded test run as `passed|failed|skipped:COMMAND` (repeatable).
    #[arg(long = "test-run")]
    pub test_runs: Vec<String>,
    /// Skip pushing proposals/<task> (already pushed).
    #[arg(long)]
    pub no_push: bool,
    /// Build and print the metadata without pushing or uploading.
    #[arg(long)]
    pub dry_run: bool,
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
    /// Report a bug or propose an enhancement.
    Submit(SubmitIntakeArgs),
}

#[derive(Debug, Args)]
pub struct SubmitIntakeArgs {
    pub project: String,
    #[arg(long, value_parser = ["bug", "enhancement"])]
    pub kind: String,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub description: String,
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
    /// Sign in through the browser or store an existing personal access token.
    Login(LoginArgs),
    /// Remove the stored token.
    Logout,
    /// Show authentication state.
    Status,
}

#[derive(Debug, Args)]
pub struct LoginArgs {
    /// Read a personal access token from standard input instead of opening a browser.
    #[arg(long, conflicts_with_all = ["no_browser", "browser_url"])]
    pub with_token: bool,
    /// Print the approval link without opening a browser.
    #[arg(long)]
    pub no_browser: bool,
    /// `GitConflux` website URL if it differs from the API origin.
    #[arg(long)]
    pub browser_url: Option<String>,
    /// Store in an owner-only file instead of the OS keychain.
    #[arg(long)]
    pub file_store: bool,
}
