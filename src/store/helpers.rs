use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Local, Utc};
use cron::Schedule;

use crate::model::{StoreIndex, TaskContinuation, TaskSchedule, TaskStatus, TaskSummary};

use super::ListFilter;

pub(super) fn normalize_labels(labels: Vec<String>) -> Vec<String> {
    let mut normalized = labels
        .into_iter()
        .flat_map(|value| value.split(',').map(str::to_string).collect::<Vec<_>>())
        .map(|value| slugify(&value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

pub(super) fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

pub(super) fn normalize_required_text(value: String, name: &str) -> Result<String> {
    normalize_optional_text(Some(value)).ok_or_else(|| anyhow!("{name} cannot be empty"))
}

pub(super) fn normalize_query(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
}

pub(super) fn compare_summary_for_list(left: &TaskSummary, right: &TaskSummary) -> Ordering {
    left.status
        .sort_rank()
        .cmp(&right.status.sort_rank())
        .then_with(|| right.updated_at.cmp(&left.updated_at))
        .then_with(|| left.id.cmp(&right.id))
}

pub(super) fn match_status_filter(task: &TaskSummary, filter: &ListFilter) -> bool {
    if !filter.statuses.is_empty() {
        return filter.statuses.contains(&task.status);
    }
    filter.include_done_by_default || task.status != TaskStatus::Done
}

pub(super) fn match_label_filter(task: &TaskSummary, labels: &[String]) -> bool {
    labels
        .iter()
        .all(|label| task.labels.iter().any(|task_label| task_label == label))
}

pub(super) fn task_matches_query(task: &TaskSummary, query: &str) -> bool {
    task.id.to_lowercase().contains(query)
        || task.title.to_lowercase().contains(query)
        || schedule_matches_query(task.schedule.as_ref(), query)
        || task
            .labels
            .iter()
            .any(|label| label.to_lowercase().contains(query))
        || continuation_matches_query(&task.continuation, query)
}

fn schedule_matches_query(schedule: Option<&TaskSchedule>, query: &str) -> bool {
    schedule.is_some_and(|schedule| schedule.to_string().to_lowercase().contains(query))
}

fn continuation_matches_query(continuation: &TaskContinuation, query: &str) -> bool {
    [
        continuation.next_step.as_deref(),
        continuation.next_task.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_lowercase().contains(query))
}

pub(super) fn validate_schedule(schedule: Option<TaskSchedule>) -> Result<Option<TaskSchedule>> {
    let Some(schedule) = schedule else {
        return Ok(None);
    };
    match &schedule {
        TaskSchedule::Interval { every_minutes } => {
            if *every_minutes == 0 {
                bail!("interval schedule must be at least 1 minute");
            }
        }
        TaskSchedule::Cron { expression } => {
            if expression.trim().is_empty() {
                bail!("cron schedule cannot be empty");
            }
            parse_cron_schedule(expression)?;
        }
    }
    Ok(Some(schedule))
}

pub(super) fn resolve_ready_at(
    explicit_ready_at: Option<DateTime<Utc>>,
    schedule: Option<&TaskSchedule>,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    match (explicit_ready_at, schedule) {
        (Some(ready_at), _) => Ok(Some(ready_at)),
        (None, Some(schedule)) => Ok(Some(next_scheduled_ready_at(None, schedule, now)?)),
        (None, None) => Ok(None),
    }
}

pub(super) fn next_scheduled_ready_at(
    current_ready_at: Option<DateTime<Utc>>,
    schedule: &TaskSchedule,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    match schedule {
        TaskSchedule::Interval { every_minutes } => {
            let step_seconds = i64::from(*every_minutes) * 60;
            let anchor = current_ready_at.unwrap_or(now);
            let reference = if anchor > now { anchor } else { now };
            let delta_seconds = (reference - anchor).num_seconds();
            let steps = if delta_seconds < 0 {
                1
            } else {
                (delta_seconds / step_seconds) + 1
            };
            Ok(anchor + chrono::TimeDelta::seconds(step_seconds * steps))
        }
        TaskSchedule::Cron { expression } => {
            let schedule = parse_cron_schedule(expression)?;
            let anchor = current_ready_at.unwrap_or(now);
            let reference = if anchor > now { anchor } else { now };
            let reference_local = reference.with_timezone(&Local);
            schedule
                .after(&reference_local)
                .next()
                .map(|occurrence| occurrence.with_timezone(&Utc))
                .ok_or_else(|| {
                    anyhow!("could not compute the next cron occurrence for '{expression}'")
                })
        }
    }
}

fn parse_cron_schedule(expression: &str) -> Result<Schedule> {
    let trimmed = expression.trim();
    let fields = trimmed.split_whitespace().count();
    let normalized = match fields {
        5 => format!("0 {trimmed}"),
        6 => trimmed.to_string(),
        _ => bail!(
            "invalid cron expression '{}': expected 5 fields (minute hour day-of-month month day-of-week) or 6 fields including seconds",
            expression
        ),
    };
    Schedule::from_str(&normalized)
        .with_context(|| format!("invalid cron expression '{expression}'"))
}

pub(super) fn is_ready_summary(task: &TaskSummary, index: &StoreIndex, now: DateTime<Utc>) -> bool {
    is_due_summary(task, now) && !has_unmet_dependencies(task, index)
}

pub(super) fn is_due_scheduled_summary(task: &TaskSummary, now: DateTime<Utc>) -> bool {
    is_due_summary(task, now) && task.schedule.is_some()
}

pub(super) fn is_pending_dependency_summary(
    task: &TaskSummary,
    index: &StoreIndex,
    now: DateTime<Utc>,
) -> bool {
    is_due_summary(task, now) && has_unmet_dependencies(task, index)
}

fn is_due_summary(task: &TaskSummary, now: DateTime<Utc>) -> bool {
    task.status == TaskStatus::Todo && task.ready_at.is_none_or(|ready_at| ready_at <= now)
}

fn has_unmet_dependencies(task: &TaskSummary, index: &StoreIndex) -> bool {
    !unmet_dependency_ids(task, index).is_empty()
}

pub(super) fn unmet_dependency_ids(task: &TaskSummary, index: &StoreIndex) -> Vec<String> {
    task.depends_on
        .iter()
        .filter(|dependency_id| {
            index
                .tasks
                .get(*dependency_id)
                .is_none_or(|dependency| dependency.status != TaskStatus::Done)
        })
        .cloned()
        .collect()
}

pub(super) fn describe_progress_message(continuation: &TaskContinuation) -> String {
    if continuation.is_empty() {
        return String::new();
    }

    let mut parts = Vec::new();
    if let Some(step) = continuation.next_step.as_deref() {
        parts.push(format!("step={step}"));
    }
    if let Some(task) = continuation.next_task.as_deref() {
        parts.push(format!("task={task}"));
    }
    format!("{}", parts.join(", "))
}

pub(super) fn ensure_task_exists(index: &StoreIndex, task_id: &str) -> Result<()> {
    if index.tasks.contains_key(task_id) {
        return Ok(());
    }
    bail!("task '{task_id}' does not exist")
}

pub(super) fn resolve_task_reference(index: &StoreIndex, task_id: &str) -> Result<String> {
    let requested = normalize_required_text(task_id.to_string(), "task id")?;
    let lookup = requested.to_lowercase();

    if index.tasks.contains_key(&lookup) {
        return Ok(lookup);
    }

    let mut matches = index
        .tasks
        .values()
        .filter(|task| task.id.to_lowercase().contains(&lookup))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.id.cmp(&right.id));

    match matches.as_slice() {
        [] => bail!("task '{requested}' does not exist"),
        [task] => Ok(task.id.clone()),
        _ => {
            let preview = matches
                .iter()
                .take(8)
                .map(|task| format!("{} ({})", task.id, task.title))
                .collect::<Vec<_>>()
                .join(", ");
            let suffix = if matches.len() > 8 {
                format!(", +{} more", matches.len() - 8)
            } else {
                String::new()
            };
            bail!("task id match '{requested}' is ambiguous; matches: {preview}{suffix}")
        }
    }
}

pub(super) fn ensure_distinct(left: &str, right: &str, relation_name: &str) -> Result<()> {
    if left == right {
        bail!("cannot link task '{left}' to itself as a {relation_name}");
    }
    Ok(())
}

pub(super) fn has_dependency_path(index: &StoreIndex, start: &str, target: &str) -> bool {
    if start == target {
        return true;
    }
    let mut stack = vec![start.to_string()];
    let mut visited = BTreeSet::new();
    while let Some(current) = stack.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        if current == target {
            return true;
        }
        if let Some(task) = index.tasks.get(&current) {
            stack.extend(task.depends_on.iter().cloned());
        }
    }
    false
}

pub(super) fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}
