//! Read-only commands.

use gitconflux_api::sanitize;
use gitconflux_core::{ApiUrl, CredentialStore};
use serde::Serialize;

use crate::cli::{
    AgentsCommand, Command, IntakeCommand, LogArgs, ProjectsCommand, ProposalsCommand, TasksCommand,
};
use crate::commands::{CliError, Result, client, optional_client, out, require_token};
use crate::output::{table, ts_millis};

struct Ctx<'a> {
    api: &'a ApiUrl,
    store: &'a CredentialStore,
    json: bool,
}

fn json<T: Serialize>(value: &T) -> Result<()> {
    let s = serde_json::to_string_pretty(value)
        .map_err(|e| CliError::Core(gitconflux_core::Error::Io(e.to_string())))?;
    out(&s)
}

fn opt(v: Option<&str>) -> String {
    v.unwrap_or("-").to_owned()
}

fn field(label: &str, value: &str) -> Result<()> {
    out(&format!("{label:<12} {}", sanitize(value)))
}

pub async fn run(cmd: &Command, api: &ApiUrl, store: &CredentialStore, json: bool) -> Result<()> {
    let ctx = Ctx { api, store, json };
    match cmd {
        Command::Projects(c) => projects(&ctx, c).await,
        Command::Tasks(c) => tasks(&ctx, c).await,
        Command::Agents(AgentsCommand::List) => agents(&ctx).await,
        Command::Proposals(c) => proposals(&ctx, c).await,
        Command::Intake(c) => intake(&ctx, c).await,
        Command::Log(a) => log(&ctx, a).await,
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
        ProposalsCommand::Show { project, id } => {
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
