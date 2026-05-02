use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Local, Utc};

pub(crate) fn resolve_root(root: Option<PathBuf>) -> Result<PathBuf> {
    match root {
        Some(path) => Ok(path),
        None => {
            let cwd = std::env::current_dir().context("failed to resolve current directory")?;
            find_existing_root(&cwd).ok_or_else(|| {
                anyhow!(
                    "could not find '.tli' from '{}' up to filesystem root; pass --root <path> to create or target a store explicitly",
                    display_path(&cwd)
                )
            })
        }
    }
}

pub(crate) fn find_existing_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        let candidate = dir.join(".tli");
        if candidate.is_dir() {
            return Some(candidate);
        }
        current = dir.parent();
    }
    None
}

pub(crate) fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("expected RFC3339 timestamp, got '{value}'"))?;
    Ok(parsed.with_timezone(&Utc))
}

pub(crate) fn format_timestamp(value: &DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S %:z")
        .to_string()
}

pub(crate) fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_timestamp_requires_rfc3339() {
        assert!(parse_timestamp("2026-05-02T18:42:57+08:00").is_ok());
        assert!(parse_timestamp("2026-05-02 18:42:57").is_err());
    }

    #[test]
    fn format_timestamp_is_not_empty() {
        let formatted = format_timestamp(&Utc::now());
        assert!(!formatted.is_empty());
    }

    #[test]
    fn find_existing_root_walks_up_parent_directories() {
        let temp = tempfile::TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let nested = repo.join("a").join("b");
        std::fs::create_dir_all(repo.join(".tli")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();

        let found = find_existing_root(&nested).unwrap();
        assert_eq!(found, repo.join(".tli"));
    }

    #[test]
    fn find_existing_root_returns_none_when_missing() {
        let temp = tempfile::TempDir::new().unwrap();
        let nested = temp.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();

        assert!(find_existing_root(&nested).is_none());
    }
}
