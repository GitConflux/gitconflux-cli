//! Client-side validation of the limits documented in the API guide. The server remains
//! authoritative; these checks fail fast and avoid sending obviously invalid mutations
//! (which must not be blindly retried).

use crate::error::{Error, Result};

fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Config(msg.into()))
}

fn len_between(field: &str, value: &str, min: usize, max: usize) -> Result<()> {
    let n = value.trim().chars().count();
    if n < min || n > max {
        return invalid(format!("{field} must be {min}-{max} characters"));
    }
    Ok(())
}

fn no_control(field: &str, value: &str) -> Result<()> {
    if value
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return invalid(format!("{field} contains control characters"));
    }
    Ok(())
}

/// 2-64 lowercase letters/digits with single internal dashes.
pub fn slug(value: &str) -> Result<()> {
    let ok = (2..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--");
    if ok {
        Ok(())
    } else {
        invalid("slug must be 2-64 lowercase letters/digits with single internal dashes")
    }
}

pub fn project(name: &str, description: &str, visibility: &str) -> Result<()> {
    len_between("name", name, 1, 100)?;
    no_control("name", name)?;
    if description.chars().count() > 1000 {
        return invalid("description must be at most 1000 characters");
    }
    no_control("description", description)?;
    match visibility {
        "public" | "private" => Ok(()),
        _ => invalid("visibility must be `public` or `private`"),
    }
}

pub fn task(
    title: &str,
    description: &str,
    priority: u8,
    labels: &[String],
    criteria: &[String],
) -> Result<()> {
    len_between("title", title, 1, 200)?;
    len_between("description", description, 1, 20_000)?;
    no_control("title", title)?;
    no_control("description", description)?;
    if priority > 100 {
        return invalid("priority must be 0-100");
    }
    if labels.len() > 20
        || labels
            .iter()
            .any(|l| !(1..=50).contains(&l.chars().count()))
    {
        return invalid("at most 20 labels, each 1-50 characters");
    }
    if criteria.len() > 50 || criteria.iter().any(|c| c.trim().is_empty()) {
        return invalid("at most 50 acceptance criteria, none empty");
    }
    Ok(())
}

pub fn agent(name: &str, kind: &str, capabilities: &[String]) -> Result<()> {
    len_between("name", name, 1, 100)?;
    no_control("name", name)?;
    if !matches!(kind, "autonomous" | "human_assisted") {
        return invalid("kind must be `autonomous` or `human_assisted`");
    }
    if capabilities.len() > 50
        || capabilities
            .iter()
            .any(|c| !(1..=50).contains(&c.chars().count()))
    {
        return invalid("at most 50 capabilities, each 1-50 characters");
    }
    Ok(())
}

pub fn lease_seconds(value: u32) -> Result<()> {
    if (30..=3600).contains(&value) {
        Ok(())
    } else {
        invalid("lease must be 30-3600 seconds")
    }
}

pub fn intake(kind: &str, title: &str, description: &str) -> Result<()> {
    if !matches!(kind, "bug" | "enhancement") {
        return invalid("kind must be `bug` or `enhancement`");
    }
    len_between("title", title, 1, 200)?;
    len_between("description", description, 1, 20_000)?;
    no_control("title", title)?;
    no_control("description", description)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_rules() {
        for ok in ["ab", "demo", "a-b-c", "x1"] {
            assert!(slug(ok).is_ok(), "{ok}");
        }
        for bad in [
            "a",
            "-a",
            "a-",
            "a--b",
            "Abc",
            "a_b",
            "a/b",
            &"a".repeat(65),
        ] {
            assert!(slug(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn task_limits() {
        assert!(task("t", "d", 50, &[], &[]).is_ok());
        assert!(task("", "d", 50, &[], &[]).is_err());
        assert!(task("t", "d", 101, &[], &[]).is_err());
        assert!(task("t", "d", 5, &vec!["x".into(); 21], &[]).is_err());
        assert!(task("t", "d", 5, &[], &["  ".into()]).is_err());
        assert!(task("t\x1b", "d", 5, &[], &[]).is_err());
    }

    #[test]
    fn agent_and_lease() {
        assert!(agent("w", "autonomous", &[]).is_ok());
        assert!(agent("w", "robot", &[]).is_err());
        assert!(lease_seconds(29).is_err());
        assert!(lease_seconds(600).is_ok());
        assert!(lease_seconds(3601).is_err());
    }

    #[test]
    fn intake_rules() {
        assert!(intake("bug", "t", "d").is_ok());
        assert!(intake("idea", "t", "d").is_err());
    }
}
