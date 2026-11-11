//! Read-only commands.

use gitconflux_api::sanitize;
use gitconflux_core::{ApiUrl, CredentialStore};
use serde::Serialize;

use crate::cli::{
    AgentsCommand, Command, IntakeCommand, IntegrateCommand, LogArgs, ProjectsCommand,
    ProposalsCommand, TasksCommand,
};
use crate::commands::{CliError, Result, client, optional_client, out, require_token};
use crate::output::{table, ts_millis};
use crate::write;

pub struct Ctx<'a> {
    pub api: &'a ApiUrl,
    pub store: &'a CredentialStore,
    pub json: bool,
}

pub fn json<T: Serialize>(value: &T) -> Result<()> {
    let s = serde_json::to_string_pretty(value)
        .map_err(|e| CliError::Core(gitconflux_core::Error::Io(e.to_string())))?;
    out(&s)
}

fn opt(v: Option<&str>) -> String {
    v.unwrap_or("-").to_owned()
}

pub fn field(label: &str, value: &str) -> Result<()> {
    out(&format!("{label:<12} {}", sanitize(value)))
}

pub async fn run(cmd: &Command, api: &ApiUrl, store: &CredentialStore, json: bool) -> Result<()> {
    let ctx = Ctx { api, store, json };
    match cmd {
        Command::Projects(c) => projects(&ctx, c).await,
        Command::Tasks(c) => tasks(&ctx, c).await,
        Command::Agents(AgentsCommand::List) => agents(&ctx).await,
        Command::Agents(AgentsCommand::Register(a)) => write::register_agent(&ctx, a).await,
        Command::Agents(AgentsCommand::Heartbeat { id }) => write::heartbeat(&ctx, id).await,
        Command::Proposals(c) => proposals(&ctx, c).await,
        Command::Intake(c) => intake(&ctx, c).await,
        Command::Log(a) => log(&ctx, a).await,
        Command::Completions { shell } => {
            let mut cmd = <crate::cli::Cli as clap::CommandFactory>::command();
            clap_complete::generate(*shell, &mut cmd, "gcflx", &mut std::io::stdout());
            Ok(())
        }
        Command::Tokens(c) => write::tokens(&ctx, c).await,
        Command::Integrate(IntegrateCommand::Preview { project }) => {
            write::preview(&ctx, project).await
        }
        Command::Clone(a) => crate::submit::clone_repo(&ctx, a).await,
        Command::Auth(_) | Command::Whoami => Ok(()),
    }
}

async fn projects(ctx: &Ctx<'_>, cmd: &ProjectsCommand) -> Result<()> {
    let client = optional_client(ctx.api, ctx.store)?;
    match cmd {
        ProjectsCommand::List => {
            let items = client.list_projects().await?;
            if ctx.json {
                return json(&items);
            }
            let rows: Vec<_> = items
                .iter()
                .map(|p| {
                    vec![
                        p.slug.clone(),
                        p.visibility.clone(),
                        p.owner_handle.clone(),
                        p.name.clone(),
                    ]
                })
                .collect();
            out(&table(&["SLUG", "VISIBILITY", "OWNER", "NAME"], &rows))
        }
        ProjectsCommand::Show { slug } => {
            let p = client.get_project(slug).await?;
            if ctx.json {
                return json(&p);
            }
            field("Slug:", &p.slug)?;
            field("Name:", &p.name)?;
            field("Owner:", &p.owner_handle)?;
            field("Visibility:", &p.visibility)?;
            field("Created:", &ts_millis(p.created_at))?;
            field("Description:", &p.description)
        }
        ProjectsCommand::Create(a) => crate::write::create_project(ctx, a).await,
    }
}

async fn tasks(ctx: &Ctx<'_>, cmd: &TasksCommand) -> Result<()> {
    let client = optional_client(ctx.api, ctx.store)?;
    match cmd {
        TasksCommand::List { project } => {
            let items = client.list_tasks(project).await?;
            if ctx.json {
                return json(&items);
            }
            let rows: Vec<_> = items
                .iter()
                .map(|t| {
                    vec![
                        t.id.clone(),
                        opt(t.key.as_deref()),
                        t.status.clone(),
                        t.priority.to_string(),
                        t.title.clone(),
                    ]
                })
                .collect();
            out(&table(&["ID", "KEY", "STATUS", "PRIORITY", "TITLE"], &rows))
        }
        TasksCommand::Catalog { project } => json(&client.task_catalog(project).await?),
        TasksCommand::Sync { project, git_ref } => write::sync_tasks(ctx, project, git_ref).await,
        TasksCommand::Publish { project } => write::publish_tasks(ctx, project).await,
        TasksCommand::Create(a) => crate::write::create_task(ctx, a).await,
        TasksCommand::Claim(a) => crate::write::claim(ctx, a).await,
        TasksCommand::Release(a) => crate::write::release(ctx, a).await,
    }
}

async fn agents(ctx: &Ctx<'_>) -> Result<()> {
    let (token, _) = require_token(ctx.api, ctx.store)?;
    let items = client(ctx.api, Some(token))?.list_agents().await?;
    if ctx.json {
        return json(&items);
    }
    let rows: Vec<_> = items
        .iter()
        .map(|a| {
            vec![
                a.id.clone(),
                a.name.clone(),
                a.kind.clone(),
                a.last_seen_at.map_or_else(|| "-".into(), ts_millis),
            ]
        })
        .collect();
    out(&table(&["ID", "NAME", "KIND", "LAST SEEN"], &rows))
}

async fn proposals(ctx: &Ctx<'_>, cmd: &ProposalsCommand) -> Result<()> {
    let client = optional_client(ctx.api, ctx.store)?;
    match cmd {
        ProposalsCommand::List { project } => {
            let items = client.list_proposals(project).await?;
            if ctx.json {
                return json(&items);
            }
            let rows: Vec<_> = items
                .iter()
                .map(|p| {
                    vec![
                        p.id.clone(),
                        p.status.clone(),
                        p.task_id.clone(),
                        ts_millis(p.updated_at),
                    ]
                })
                .collect();
            out(&table(&["ID", "STATUS", "TASK", "UPDATED"], &rows))
        }
        ProposalsCommand::Submit(a) => crate::submit::submit(ctx, a).await,
        ProposalsCommand::Show {
            project,
            id,
            watch,
            watch_timeout,
        } => {
            if *watch {
                return watch_proposal(ctx, &client, project, id, *watch_timeout).await;
            }
            let (p, events) = client.get_proposal(project, id).await?;
            if ctx.json {
                return json(&serde_json::json!({ "proposal": p, "events": events }));
            }
            field("Proposal:", &p.id)?;
            field("Status:", &p.status)?;
            field("Task:", &p.task_id)?;
            let rows: Vec<_> = events
                .iter()
                .map(|e| {
                    vec![
                        ts_millis(e.created_at),
                        e.kind.clone(),
                        e.detail.to_string(),
                    ]
                })
                .collect();
            out(&table(&["WHEN", "EVENT", "DETAIL"], &rows))
        }
    }
}

async fn intake(ctx: &Ctx<'_>, cmd: &IntakeCommand) -> Result<()> {
    let client = optional_client(ctx.api, ctx.store)?;
    match cmd {
        IntakeCommand::List { project, status } => {
            let items = client.list_intake(project, status.as_deref()).await?;
            if ctx.json {
                return json(&items);
            }
            let rows: Vec<_> = items
                .iter()
                .map(|i| {
                    vec![
                        i.id.clone(),
                        i.kind.clone(),
                        i.status.clone(),
                        i.title.clone(),
                    ]
                })
                .collect();
            out(&table(&["ID", "KIND", "STATUS", "TITLE"], &rows))
        }
        IntakeCommand::Show { project, id } => json(&client.get_intake(project, id).await?),
        IntakeCommand::Submit(a) => crate::write::submit_intake(ctx, a).await,
    }
}

async fn log(ctx: &Ctx<'_>, args: &LogArgs) -> Result<()> {
    let client = optional_client(ctx.api, ctx.store)?;
    let items = client
        .git_log(&args.project, &args.git_ref, args.limit)
        .await?;
    if ctx.json {
        return json(&items);
    }
    let rows: Vec<_> = items
        .iter()
        .map(|c| {
            // The API does not state the unit for commit times; accept seconds or ms.
            let ms = if c.timestamp > 100_000_000_000 {
                c.timestamp
            } else {
                c.timestamp.saturating_mul(1000)
            };
            vec![
                c.hash.chars().take(12).collect(),
                c.author.clone(),
                ts_millis(ms),
                c.message.lines().next().unwrap_or("").to_owned(),
            ]
        })
        .collect();
    out(&table(&["COMMIT", "AUTHOR", "DATE", "MESSAGE"], &rows))
}

const TERMINAL_STATUSES: &[&str] = &[
    "merged",
    "rejected",
    "withdrawn",
    "tier1_failed",
    "tier2_failed",
    "tier2_passed",
];

/// Polls a proposal, printing each status change, until it settles or the timeout elapses.
async fn watch_proposal(
    ctx: &Ctx<'_>,
    client: &gitconflux_api::Client,
    project: &str,
    id: &str,
    timeout_secs: u64,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let mut last = String::new();
    loop {
        let (p, _) = client.get_proposal(project, id).await?;
        if p.status != last {
            if ctx.json {
                json(
                    &serde_json::json!({ "id": p.id, "status": p.status, "updatedAt": p.updated_at }),
                )?;
            } else {
                out(&format!(
                    "{}  {}",
                    ts_millis(p.updated_at),
                    sanitize(&p.status)
                ))?;
            }
            last.clone_from(&p.status);
        }
        if TERMINAL_STATUSES.contains(&p.status.as_str()) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(CliError::Usage(format!(
                "still `{}` after {timeout_secs}s; stopped watching",
                sanitize(&p.status)
            )));
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}
