//! Mutating commands. Every input is validated locally first, and requests are never
//! retried automatically (see `gitconflux-api`).

use gitconflux_api::models::{NewAgent, NewIntake, NewProject, NewTask};
use gitconflux_core::validate;

use crate::cli::{
    ClaimArgs, CreateProjectArgs, CreateTaskArgs, RegisterAgentArgs, ReleaseArgs, SubmitIntakeArgs,
    TokensCommand,
};
use crate::commands::{Result, authed_client, out};
use crate::read::{Ctx, field, json};

pub async fn create_project(ctx: &Ctx<'_>, a: &CreateProjectArgs) -> Result<()> {
    validate::slug(&a.slug)?;
    validate::project(&a.name, &a.description, &a.visibility)?;
    let client = authed_client(ctx.api, ctx.store)?;
    let p = client
        .create_project(&NewProject {
            slug: &a.slug,
            name: a.name.trim(),
            description: &a.description,
            visibility: &a.visibility,
        })
        .await?;
    if ctx.json {
        return json(&p);
    }
    out(&format!(
        "Created project {}",
        gitconflux_api::sanitize(&p.slug)
    ))
}

pub async fn create_task(ctx: &Ctx<'_>, a: &CreateTaskArgs) -> Result<()> {
    validate::task(&a.title, &a.description, a.priority, &a.labels, &a.criteria)?;
    let client = authed_client(ctx.api, ctx.store)?;
    let t = client
        .create_task(
            &a.project,
            &NewTask {
                title: a.title.trim(),
                description: a.description.trim(),
                priority: a.priority,
                labels: &a.labels,
                acceptance_criteria: &a.criteria,
            },
        )
        .await?;
    if ctx.json {
        return json(&t);
    }
    out(&format!("Created task {}", gitconflux_api::sanitize(&t.id)))
}

pub async fn claim(ctx: &Ctx<'_>, a: &ClaimArgs) -> Result<()> {
    validate::lease_seconds(a.lease_seconds)?;
    let client = authed_client(ctx.api, ctx.store)?;
    let claimed = client
        .claim_task(&a.project, &a.agent, a.lease_seconds)
        .await?;
    if ctx.json {
        return json(&claimed);
    }
    match claimed {
        // An empty queue is a normal outcome, not an error.
        None => out("No tasks available."),
        Some(t) => {
            field("Task:", &t.id)?;
            field("Title:", &t.title)?;
            field("Status:", &t.status)?;
            field(
                "Lease ends:",
                &t.lease_expires_at
                    .map_or_else(|| "-".into(), crate::output::ts_millis),
            )
        }
    }
}

pub async fn release(ctx: &Ctx<'_>, a: &ReleaseArgs) -> Result<()> {
    let client = authed_client(ctx.api, ctx.store)?;
    let t = client
        .release_task(&a.project, &a.task_id, &a.agent)
        .await?;
    if ctx.json {
        return json(&t);
    }
    out(&format!(
        "Released task {} (status: {})",
        gitconflux_api::sanitize(&t.id),
        gitconflux_api::sanitize(&t.status)
    ))
}

pub async fn register_agent(ctx: &Ctx<'_>, a: &RegisterAgentArgs) -> Result<()> {
    validate::agent(&a.name, &a.kind, &a.capabilities)?;
    let client = authed_client(ctx.api, ctx.store)?;
    let agent = client
        .register_agent(&NewAgent {
            name: a.name.trim(),
            kind: &a.kind,
            capabilities: &a.capabilities,
        })
        .await?;
    if ctx.json {
        return json(&agent);
    }
    out(&format!(
        "Registered agent {}",
        gitconflux_api::sanitize(&agent.id)
    ))
}

pub async fn heartbeat(ctx: &Ctx<'_>, id: &str) -> Result<()> {
    let agent = authed_client(ctx.api, ctx.store)?.heartbeat(id).await?;
    if ctx.json {
        return json(&agent);
    }
    out(&format!(
        "Heartbeat recorded for {}",
        gitconflux_api::sanitize(&agent.id)
    ))
}

pub async fn submit_intake(ctx: &Ctx<'_>, a: &SubmitIntakeArgs) -> Result<()> {
    validate::intake(&a.kind, &a.title, &a.description)?;
    let client = authed_client(ctx.api, ctx.store)?;
    let i = client
        .submit_intake(
            &a.project,
            &NewIntake {
                kind: &a.kind,
                title: a.title.trim(),
                description: a.description.trim(),
            },
        )
        .await?;
    if ctx.json {
        return json(&i);
    }
    out(&format!(
        "Submitted {} {}",
        gitconflux_api::sanitize(&i.kind),
        gitconflux_api::sanitize(&i.id)
    ))
}

pub async fn tokens(ctx: &Ctx<'_>, cmd: &TokensCommand) -> Result<()> {
    let client = authed_client(ctx.api, ctx.store)?;
    match cmd {
        TokensCommand::List => {
            let items = client.list_tokens().await?;
            if ctx.json {
                return json(&items);
            }
            let rows: Vec<_> = items
                .iter()
                .map(|t| {
                    vec![
                        t.id.clone(),
                        t.name.clone(),
                        t.created_at
                            .map_or_else(|| "-".into(), crate::output::ts_millis),
                        t.last_used_at
                            .map_or_else(|| "-".into(), crate::output::ts_millis),
                    ]
                })
                .collect();
            out(&crate::output::table(
                &["ID", "NAME", "CREATED", "LAST USED"],
                &rows,
            ))
        }
        TokensCommand::Revoke { id } => {
            client.revoke_token(id).await?;
            out("Token revoked.")
        }
    }
}

pub async fn sync_tasks(ctx: &Ctx<'_>, project: &str, git_ref: &str) -> Result<()> {
    let r = authed_client(ctx.api, ctx.store)?
        .sync_tasks(project, git_ref)
        .await?;
    if ctx.json {
        return json(&r);
    }
    out(&format!(
        "created: {}, updated: {}, unchanged: {}, skipped: {}",
        r.created.len(),
        r.updated.len(),
        r.unchanged.len(),
        r.skipped.len()
    ))
}

pub async fn publish_tasks(ctx: &Ctx<'_>, project: &str) -> Result<()> {
    let r = authed_client(ctx.api, ctx.store)?
        .publish_tasks(project)
        .await?;
    if ctx.json {
        return json(&r);
    }
    out(&format!(
        "Publish status: {}",
        gitconflux_api::sanitize(&r.status)
    ))
}

/// Preview output is advisory only: it is not a test result and not authorization to promote.
pub async fn preview(ctx: &Ctx<'_>, project: &str) -> Result<()> {
    let r = authed_client(ctx.api, ctx.store)?
        .integrate_preview(project)
        .await?;
    if ctx.json {
        return json(&r);
    }
    out("ADVISORY ONLY: this preview is not a test pass and does not authorize landing.")?;
    let rows: Vec<_> = r
        .proposals
        .iter()
        .map(|p| vec![p.proposal_id.clone(), p.status.clone(), p.files.join(", ")])
        .collect();
    out(&crate::output::table(
        &["PROPOSAL", "RESULT", "CONFLICTING FILES"],
        &rows,
    ))
}
