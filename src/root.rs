use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{
    DateTime, Datelike, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc,
};

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
    parse_timestamp_with_now(value, Local::now())
}

fn parse_timestamp_with_now(value: &str, now: DateTime<Local>) -> Result<DateTime<Utc>> {
    let value = value.trim();
    if value.is_empty() {
        bail!("timestamp cannot be empty");
    }

    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(parsed.with_timezone(&Utc));
    }

    let naive = parse_local_naive_timestamp(value, now)?;
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(local) => Ok(local.with_timezone(&Utc)),
        LocalResult::Ambiguous(_, _) => bail!("local timestamp '{value}' is ambiguous"),
        LocalResult::None => bail!("local timestamp '{value}' does not exist"),
    }
}

fn parse_local_naive_timestamp(value: &str, now: DateTime<Local>) -> Result<NaiveDateTime> {
    let parts = value
        .split(['T', ' ', '\t'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    match parts.as_slice() {
        [time] => Ok(NaiveDateTime::new(now.date_naive(), parse_time(time)?)),
        [date, time] => Ok(NaiveDateTime::new(
            parse_date(date, now.year())?,
            parse_time(time)?,
        )),
        _ => bail!(
            "expected RFC3339 timestamp or local time like '2026-05-10 12:20:10', '12:20:10', or '5-10 13:0:0', got '{value}'"
        ),
    }
}

fn parse_date(value: &str, current_year: i32) -> Result<NaiveDate> {
    let parts = parse_number_parts(value, '-')?;
    match parts.as_slice() {
        [year, month, day] => NaiveDate::from_ymd_opt(*year as i32, *month, *day)
            .ok_or_else(|| anyhow!("invalid local date '{value}'")),
        [month, day] => NaiveDate::from_ymd_opt(current_year, *month, *day)
            .ok_or_else(|| anyhow!("invalid local date '{value}'")),
        _ => bail!("invalid local date '{value}'"),
    }
}

fn parse_time(value: &str) -> Result<NaiveTime> {
    let parts = parse_number_parts(value, ':')?;
    match parts.as_slice() {
        [hour, minute] => NaiveTime::from_hms_opt(*hour, *minute, 0)
            .ok_or_else(|| anyhow!("invalid local time '{value}'")),
        [hour, minute, second] => NaiveTime::from_hms_opt(*hour, *minute, *second)
            .ok_or_else(|| anyhow!("invalid local time '{value}'")),
        _ => bail!("invalid local time '{value}'"),
    }
}

fn parse_number_parts(value: &str, separator: char) -> Result<Vec<u32>> {
    value
        .split(separator)
        .map(|part| {
            if part.is_empty() {
                bail!("invalid timestamp component in '{value}'");
            }
            part.parse::<u32>()
                .with_context(|| format!("invalid timestamp component '{part}' in '{value}'"))
        })
        .collect()
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
    fn parse_timestamp_accepts_rfc3339_and_local_friendly_forms() {
        let now = Local
            .with_ymd_and_hms(2026, 5, 9, 8, 0, 0)
            .single()
            .unwrap();

        assert!(parse_timestamp("2026-05-02T18:42:57+08:00").is_ok());
        assert_eq!(
            parse_timestamp_with_now("2026-05-10 12:20:10", now)
                .unwrap()
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            "2026-05-10 12:20:10"
        );
        assert_eq!(
            parse_timestamp_with_now("12:20:10", now)
                .unwrap()
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            "2026-05-09 12:20:10"
        );
        assert_eq!(
            parse_timestamp_with_now("5-10 13:0:0", now)
                .unwrap()
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            "2026-05-10 13:00:00"
        );
        assert_eq!(
            parse_timestamp_with_now("2026-05-10T12:20", now)
                .unwrap()
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            "2026-05-10 12:20:00"
        );
        assert!(parse_timestamp_with_now("2026/05/10 12:20:10", now).is_err());
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
