//! Project board: read, post, and tag people and agents.

use std::collections::HashSet;

use gitconflux_api::models::{BoardMember, BoardMessage};
use gitconflux_api::sanitize;

use crate::cli::{BoardCommand, BoardPostArgs, BoardReadArgs};
use crate::commands::{CliError, Result, authed_client, out};
use crate::output::{table, ts_millis};
use crate::read::{Ctx, json};

const MAX_BODY_CHARS: usize = 4000;

pub async fn run(ctx: &Ctx<'_>, cmd: &BoardCommand) -> Result<()> {
    match cmd {
        BoardCommand::Read(a) => read(ctx, a).await,
        BoardCommand::Post(a) => post(ctx, a).await,
        BoardCommand::Members { project } => members(ctx, project).await,
    }
}

/// Strips terminal control characters but keeps line breaks, which messages may contain.
fn clean_body(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(MAX_BODY_CHARS)
        .collect()
}

pub fn author_label(m: &BoardMember) -> String {
    if m.kind == "agent" {
        match &m.owner_handle {
            Some(owner) => format!("{} (agent of @{})", sanitize(&m.name), sanitize(owner)),
            None => format!("{} (agent)", sanitize(&m.name)),
        }
    } else {
        format!("@{}", sanitize(&m.name))
    }
}

/// Puts `@tag` prefixes in front of the message after checking each tag is a member.
pub fn with_tags(message: &str, tags: &[String], members: &[BoardMember]) -> Result<String> {
    let mut prefix = String::new();
    for raw in tags {
        let tag = raw.trim().trim_start_matches('@').to_lowercase();
        if !members.iter().any(|m| m.mention == tag) {
            let known: Vec<String> = members.iter().map(|m| format!("@{}", m.mention)).collect();
            return Err(CliError::Usage(format!(
                "`{}` is not on this board. Members: {}",
                sanitize(raw),
                sanitize(&known.join(", "))
            )));
        }
        prefix.push('@');
        prefix.push_str(&tag);
        prefix.push(' ');
    }
    let body = format!("{prefix}{}", message.trim());
    if body.trim().is_empty() {
        return Err(CliError::Usage("the message is empty".into()));
    }
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(CliError::Usage(format!(
            "messages are limited to {MAX_BODY_CHARS} characters"
        )));
    }
    Ok(body)
}

fn print_message(m: &BoardMessage, tagged_me: bool) -> Result<()> {
    let marker = if tagged_me { "  [tags you]" } else { "" };
    out(&format!(
        "{}  {}{marker}",
        ts_millis(m.created_at),
        author_label(&m.author)
    ))?;
    for line in clean_body(&m.body).lines() {
        out(&format!("    {line}"))?;
    }
    Ok(())
}

async fn read(ctx: &Ctx<'_>, a: &BoardReadArgs) -> Result<()> {
    let client = authed_client(ctx.api, ctx.store)?;
    let page = client.board(&a.project, a.limit, a.before).await?;
    let me = client.me().await?;
    let mine: HashSet<String> = client
        .list_agents()
        .await?
        .into_iter()
        .map(|agent| agent.id)
        .collect();
    let tags_me = |m: &BoardMessage| {
        m.mentions.iter().any(|t| {
            (t.kind == "user" && t.id == me.user_id) || (t.kind == "agent" && mine.contains(&t.id))
        })
    };
    let mut messages: Vec<&BoardMessage> = page
        .messages
        .iter()
        .filter(|m| !a.mentions || tags_me(m))
        .collect();
    messages.reverse();
    if ctx.json {
        return json(&messages);
    }
    if messages.is_empty() {
        return out(if a.mentions {
            "No messages tag you or your agents."
        } else {
            "No messages yet."
        });
    }
    for m in messages {
        print_message(m, tags_me(m))?;
    }
    Ok(())
}

async fn post(ctx: &Ctx<'_>, a: &BoardPostArgs) -> Result<()> {
    let client = authed_client(ctx.api, ctx.store)?;
    let message = a.message.join(" ");
    let body = if a.tags.is_empty() {
        with_tags(&message, &[], &[])?
    } else {
        let page = client.board(&a.project, 1, None).await?;
        with_tags(&message, &a.tags, &page.members)?
    };
    let posted = client
        .post_board(&a.project, &body, a.agent.as_deref())
        .await?;
    if ctx.json {
        return json(&posted);
    }
    let tagged: Vec<String> = posted
        .mentions
        .iter()
        .map(|m| format!("@{}", sanitize(&m.name)))
        .collect();
    if tagged.is_empty() {
        out(&format!("Posted as {}", author_label(&posted.author)))
    } else {
        out(&format!(
            "Posted as {}, tagging {}",
            author_label(&posted.author),
            tagged.join(", ")
        ))
    }
}

async fn members(ctx: &Ctx<'_>, project: &str) -> Result<()> {
    let client = authed_client(ctx.api, ctx.store)?;
    let page = client.board(project, 1, None).await?;
    if ctx.json {
        return json(&page.members);
    }
    let rows: Vec<Vec<String>> = page
        .members
        .iter()
        .map(|m| {
            vec![
                format!("@{}", m.mention),
                if m.kind == "agent" {
                    "agent".into()
                } else {
                    "person".into()
                },
                m.owner_handle
                    .clone()
                    .map_or_else(String::new, |h| format!("@{h}")),
            ]
        })
        .collect();
    out(&table(&["TAG", "TYPE", "RUN BY"], &rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(kind: &str, mention: &str) -> BoardMember {
        BoardMember {
            kind: kind.into(),
            id: mention.into(),
            name: mention.into(),
            mention: mention.into(),
            owner_handle: None,
        }
    }

    #[test]
    fn tags_are_checked_and_prefixed() {
        let members = vec![member("user", "alice"), member("agent", "build-bot")];
        assert_eq!(
            with_tags(
                "please review",
                &["@Alice".into(), "build-bot".into()],
                &members
            )
            .ok(),
            Some("@alice @build-bot please review".into())
        );
        assert!(matches!(
            with_tags("hi", &["nobody".into()], &members),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            with_tags("   ", &[], &members),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn bodies_keep_newlines_but_drop_escapes() {
        assert_eq!(clean_body("a\x1b[31m\nb\r"), "a[31m\nb");
    }
}
