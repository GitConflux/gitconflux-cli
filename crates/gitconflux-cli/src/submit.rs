//! Git-backed commands: clone and change proposal submission.

use std::io::Write;
use std::path::Path;

use gitconflux_api::models::SubmitResult;
use gitconflux_api::sanitize;
use gitconflux_core::git::{AskPass, Git, resolve_commit};
use gitconflux_core::proposal::{
    BundleInfo, Impact, Integration, MAX_BUNDLE_BYTES, Metadata, TestRun, Tests, changed_files,
    create_bundle,
};
use gitconflux_core::{ApiUrl, Error as CoreError};

use crate::cli::{CloneArgs, SubmitProposalArgs};
use crate::commands::{CliError, Result, authed_client, out};
use crate::read::{Ctx, field, json};

const CRED_TTL_SECONDS: u32 = 600;

fn askpass(remote: &str, token: &gitconflux_core::Token) -> Result<AskPass> {
    // Same transport rules as the API URL: HTTPS, or HTTP for loopback only, no userinfo.
    let url = ApiUrl::parse(remote)?;
    let u = url.as_url();
    let host = u.host_str().unwrap_or_default();
    let host = u
        .port()
        .map_or_else(|| host.to_owned(), |p| format!("{host}:{p}"));
    let program = std::env::current_exe().map_err(|e| CoreError::Io(e.to_string()))?;
    Ok(AskPass {
        program,
        host,
        username: "git".into(),
        token: token.expose().to_owned(),
    })
}

pub async fn clone_repo(ctx: &Ctx<'_>, a: &CloneArgs) -> Result<()> {
    let client = authed_client(ctx.api, ctx.store)?;
    let creds = client
        .git_credentials(&a.project, "read", CRED_TTL_SECONDS)
        .await?;
    let auth = askpass(&creds.remote, &creds.token)?;
    let dir = a.directory.clone().unwrap_or_else(|| a.project.clone());
    if Path::new(&dir).exists() {
        return Err(CliError::Usage(format!(
            "destination `{dir}` already exists"
        )));
    }
    Git::new(".").run_authenticated(&["clone", "--", &creds.remote, &dir], &auth)?;
    out(&format!(
        "Cloned {} into {}",
        sanitize(&a.project),
        sanitize(&dir)
    ))
}

fn parse_runs(raw: &[String]) -> Result<Vec<TestRun>> {
    raw.iter()
        .map(|r| {
            let (result, command) = r.split_once(':').ok_or_else(|| {
                CliError::Usage(format!("--test-run `{r}` must look like `passed:COMMAND`"))
            })?;
            if command.trim().is_empty() {
                return Err(CliError::Usage("--test-run command is empty".into()));
            }
            Ok(TestRun {
                command: command.trim().to_owned(),
                result: result.to_owned(),
            })
        })
        .collect()
}

fn build_metadata(
    a: &SubmitProposalArgs,
    git: &Git,
    base: &str,
    head: &str,
    bundle: &gitconflux_core::proposal::Bundle,
) -> Result<Metadata> {
    let metadata = Metadata {
        schema_version: 1,
        task_id: a.task.clone(),
        title: a.title.trim().to_owned(),
        summary: a.summary.trim().to_owned(),
        base_commit: base.to_owned(),
        head_commit: head.to_owned(),
        bundle: BundleInfo {
            format: "git-bundle",
            sha256: bundle.sha256.clone(),
            size_bytes: bundle.size,
        },
        impact: Impact {
            change_type: a.change_type.clone(),
            risk_level: a.risk_level.clone(),
            breaking: a.breaking,
            areas: a.areas.clone(),
            files: changed_files(git, base, head)?,
            apis: vec![],
            dependencies: vec![],
            migrations: a.migrations,
        },
        tests: Tests {
            added: a.tests_added,
            runs: parse_runs(&a.test_runs)?,
        },
        integration: Integration::default(),
    };
    metadata.validate()?;
    Ok(metadata)
}

pub async fn submit(ctx: &Ctx<'_>, a: &SubmitProposalArgs) -> Result<()> {
    let git = Git::new(&a.repo);
    let branch = format!("refs/heads/proposals/{}", a.task);
    if a.task.starts_with('-') || !git.succeeds(&["check-ref-format", &branch])? {
        return Err(CliError::Usage(
            "task ID is not valid in a branch name".into(),
        ));
    }
    let base = resolve_commit(&git, &a.base)?;
    let head = resolve_commit(&git, &a.head)?;
    let bundle = create_bundle(&git, &base, &head)?;
    let metadata = build_metadata(a, &git, &base, &head, &bundle)?;

    if a.dry_run {
        return json(&metadata);
    }
    let client = authed_client(ctx.api, ctx.store)?;
    if !a.no_push {
        let creds = client
            .git_credentials(&a.project, "write", CRED_TTL_SECONDS)
            .await?;
        let auth = askpass(&creds.remote, &creds.token)?;
        let refspec = format!("{head}:{branch}");
        git.run_authenticated(&["push", "--", &creds.remote, &refspec], &auth)?;
    }

    let bytes = std::fs::read(&bundle.path).map_err(|e| CoreError::Io(e.to_string()))?;
    if bytes.len() as u64 != bundle.size || bundle.size > MAX_BUNDLE_BYTES {
        return Err(CliError::Core(CoreError::Proposal(
            "bundle changed on disk during submit".into(),
        )));
    }
    let meta_value = serde_json::to_value(&metadata)
        .map_err(|e| CliError::Core(CoreError::Io(e.to_string())))?;
    let result = client
        .submit_proposal(&a.project, &a.agent, &meta_value, bytes)
        .await?;
    report(ctx, &result)
}

/// A 201 can still carry `tier1_failed`; surface it as a failure exit code.
fn report(ctx: &Ctx<'_>, r: &SubmitResult) -> Result<()> {
    let failed = r.proposal.status.ends_with("_failed") || r.checks.iter().any(|c| !c.passed);
    if ctx.json {
        json(&serde_json::json!({ "proposal": r.proposal, "checks": r.checks }))?;
    } else {
        field("Proposal:", &r.proposal.id)?;
        field("Status:", &r.proposal.status)?;
        for c in &r.checks {
            let mark = if c.passed { "pass" } else { "FAIL" };
            writeln!(
                std::io::stdout(),
                "  [{mark}] {}: {}",
                sanitize(&c.name),
                sanitize(&c.message)
            )
            .map_err(|e| CoreError::Io(e.to_string()))?;
        }
    }
    if failed {
        return Err(CliError::ChecksFailed(
            "proposal was recorded but Tier 1 checks failed; the task remains claimed until its lease expires".into(),
        ));
    }
    Ok(())
}
