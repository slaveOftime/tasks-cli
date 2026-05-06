use std::path::Path;

use anyhow::{Context, Result};

use crate::model::{
    ReadyTask, StateSnapshot, StateTask, TaskContinuation, TaskDetail, TaskEvent, TaskRecord,
    TaskSchedule, TaskSummary,
};
use crate::root::{display_path, format_timestamp};

pub(crate) fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).context("failed to serialize JSON output")?
    );
    Ok(())
}

pub(crate) fn print_task_result(
    task: &TaskRecord,
    json: bool,
    verbose: bool,
    verb: &str,
) -> Result<()> {
    if json {
        return print_json(task);
    }
    println!("{}", render_task_result(task, verbose, verb));
    Ok(())
}

pub(crate) fn render_task_list(tasks: &[TaskSummary], verbose: bool, root: &Path) -> String {
    let mut output = format!("Tasks in {} ({})", display_path(root), tasks.len());
    for task in tasks {
        let meta = if verbose {
            join_metadata([
                Some(format!("updated: {}", format_timestamp(&task.updated_at))),
                task.ready_at
                    .as_ref()
                    .map(|value| format!("ready: {}", format_timestamp(value))),
                task.schedule
                    .as_ref()
                    .map(|value| format!("schedule: {}", format_schedule(value))),
                (!task.labels.is_empty()).then(|| format!("labels: {}", task.labels.join(", "))),
                (!task.depends_on.is_empty()).then(|| format!("deps: {}", task.depends_on.len())),
                task.parent
                    .as_deref()
                    .map(|value| format!("parent: {value}")),
                continuation_summary(&task.continuation).map(|value| format!("next: {value}")),
            ])
        } else {
            join_metadata([
                (!task.labels.is_empty()).then(|| format!("labels: {}", task.labels.join(", "))),
                (!task.depends_on.is_empty()).then(|| format!("deps: {}", task.depends_on.len())),
                task.schedule
                    .as_ref()
                    .map(|value| format!("schedule: {}", format_schedule(value))),
                continuation_summary(&task.continuation).map(|value| format!("next: {value}")),
            ])
        };
        push_entry(
            &mut output,
            format!("- {} [{}] {}", task.id, task.status, task.title),
            meta,
        );
    }
    output
}

pub(crate) fn render_ready_list(tasks: &[ReadyTask], verbose: bool, root: &Path) -> String {
    let mut output = format!("Ready tasks in {} ({})", display_path(root), tasks.len());
    for task in tasks {
        let meta = if verbose {
            join_metadata([
                Some(format!(
                    "updated: {}",
                    format_timestamp(&task.task.updated_at)
                )),
                task.task
                    .ready_at
                    .as_ref()
                    .map(|value| format!("ready: {}", format_timestamp(value))),
                (task.dependency_count > 0).then(|| format!("deps: {}", task.dependency_count)),
                (task.child_count > 0).then(|| format!("children: {}", task.child_count)),
                task.task
                    .schedule
                    .as_ref()
                    .map(|value| format!("schedule: {}", format_schedule(value))),
                (!task.task.labels.is_empty())
                    .then(|| format!("labels: {}", task.task.labels.join(", "))),
                continuation_summary(&task.next).map(|value| format!("next: {value}")),
            ])
        } else {
            join_metadata([
                (task.dependency_count > 0).then(|| format!("deps: {}", task.dependency_count)),
                (task.child_count > 0).then(|| format!("children: {}", task.child_count)),
                (!task.task.labels.is_empty())
                    .then(|| format!("labels: {}", task.task.labels.join(", "))),
                continuation_summary(&task.next).map(|value| format!("next: {value}")),
            ])
        };
        push_entry(
            &mut output,
            format!("- {} {}", task.task.id, task.task.title),
            meta,
        );
    }
    output
}

pub(crate) fn render_state(snapshot: &StateSnapshot, verbose: bool, root: &Path) -> String {
    let mut output = format!(
        "Store: {}\n\nCounts\n  ready: {} | pending deps: {} | active: {} | blocked: {} | checkpoint: {} | review: {} | handoff: {} | todo: {} | done: {}",
        display_path(root),
        snapshot.counts.ready,
        snapshot.counts.pending_dependencies,
        snapshot.counts.active,
        snapshot.counts.blocked,
        snapshot.counts.checkpoint,
        snapshot.counts.review,
        snapshot.counts.handoff,
        snapshot.counts.todo,
        snapshot.counts.done
    );

    for (label, tasks) in [
        ("Ready", &snapshot.ready),
        ("Pending dependencies", &snapshot.pending_dependencies),
        ("Active", &snapshot.active),
        ("Blocked", &snapshot.blocked),
        ("Checkpoint", &snapshot.checkpoint),
        ("Review", &snapshot.review),
        ("Handoff", &snapshot.handoff),
    ] {
        if tasks.is_empty() {
            continue;
        }
        output.push_str("\n\n");
        output.push_str(label);
        for task in tasks {
            push_entry(
                &mut output,
                render_state_headline(task, verbose),
                render_state_meta(task, verbose),
            );
        }
    }

    output
}

pub(crate) fn render_next_task(task: &StateTask, verbose: bool) -> String {
    let headline = if verbose {
        format!(
            "- {} [{}] {}",
            task.task.id, task.task.status, task.task.title
        )
    } else {
        format!(
            "- {} [{}] {}",
            task.task.id,
            task.task.status,
            continuation_summary(&task.next).unwrap_or_else(|| "next: none".to_string())
        )
    };
    let meta = if verbose {
        join_metadata([
            continuation_summary(&task.next).map(|value| format!("next: {value}")),
            task.task
                .ready_at
                .as_ref()
                .map(|value| format!("ready: {}", format_timestamp(value))),
            task.task
                .schedule
                .as_ref()
                .map(|value| format!("schedule: {}", format_schedule(value))),
            Some(format!(
                "updated: {}",
                format_timestamp(&task.task.updated_at)
            )),
        ])
    } else {
        Some(task.task.title.clone())
    };

    let mut output = headline;
    if let Some(meta) = meta {
        output.push('\n');
        output.push_str("  ");
        output.push_str(&meta);
    }
    output
}

pub(crate) fn render_task_detail(detail: &TaskDetail, verbose: bool) -> String {
    let mut output = format!(
        "{}\n  id: {}\n  status: {}\n  ready: {}",
        detail.task.summary.title,
        detail.task.summary.id,
        detail.task.summary.status,
        yes_no(detail.ready)
    );

    let base_meta = join_metadata([
        detail
            .task
            .summary
            .ready_at
            .as_ref()
            .map(|value| format!("ready_at: {}", format_timestamp(value))),
        detail
            .task
            .summary
            .schedule
            .as_ref()
            .map(|value| format!("schedule: {}", format_schedule(value))),
        (!detail.task.summary.labels.is_empty())
            .then(|| format!("labels: {}", detail.task.summary.labels.join(", "))),
        detail
            .task
            .summary
            .parent
            .as_deref()
            .map(|value| format!("parent: {value}")),
    ]);
    if let Some(meta) = base_meta {
        output.push('\n');
        output.push_str("  ");
        output.push_str(&meta);
    }

    if verbose {
        output.push('\n');
        output.push_str(&format!(
            "\n  created: {}\n  updated: {}",
            format_timestamp(&detail.task.summary.created_at),
            format_timestamp(&detail.task.summary.updated_at)
        ));
    }

    if let Some(summary) = detail.task.summary_text.as_deref() {
        push_section(
            &mut output,
            "Summary",
            vec![if verbose {
                summary.to_string()
            } else {
                compact_text_hint(summary, 180).unwrap_or_else(|| summary.to_string())
            }],
        );
    }

    let links = vec![
        format_count_or_items(
            "dependencies",
            &detail.dependencies,
            &detail.missing_dependencies,
        ),
        format_count("blocked_by", detail.blocked_by.len()),
        format_count("children", detail.children.len()),
        continuation_summary(&detail.next).map(|value| format!("next: {value}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    push_section(&mut output, "Links", links);

    if verbose {
        push_named_items(&mut output, "Dependencies", &detail.dependencies, true);
        if !detail.missing_dependencies.is_empty() {
            push_section(
                &mut output,
                "Missing dependencies",
                detail
                    .missing_dependencies
                    .iter()
                    .map(|value| format!("- {value}"))
                    .collect(),
            );
        }
        push_named_items(&mut output, "Blocked by", &detail.blocked_by, true);
        push_named_items(&mut output, "Children", &detail.children, true);
    } else {
        push_named_items(&mut output, "Blocked by", &detail.blocked_by, false);
        push_named_items(&mut output, "Children", &detail.children, false);
    }

    if let Some(reason) = detail.task.blocked_reason.as_deref() {
        push_section(&mut output, "Blocked reason", vec![reason.to_string()]);
    }
    if let Some(note) = detail.task.completed_note.as_deref() {
        push_section(&mut output, "Completion note", vec![note.to_string()]);
    }
    if !detail.task.notes.is_empty() {
        push_section(
            &mut output,
            "Notes",
            detail
                .task
                .notes
                .iter()
                .map(|note| format!("- {} {}", format_timestamp(&note.at), note.text))
                .collect(),
        );
    }

    output
}

pub(crate) fn render_events(events: &[TaskEvent], verbose: bool, root: &Path) -> String {
    let mut output = format!("Events in {} ({})", display_path(root), events.len());
    for event in events {
        let headline = if verbose {
            format!(
                "- {} {} {}",
                format_timestamp(&event.at),
                event.task_id,
                event.kind
            )
        } else {
            format!(
                "- {} {} {}",
                format_timestamp(&event.at),
                event.kind,
                event.task_id
            )
        };
        let meta = if verbose {
            join_metadata([
                event.status.map(|status| format!("status: {status}")),
                Some(event.message.clone()),
            ])
        } else {
            Some(event.message.clone())
        };
        push_entry(&mut output, headline, meta);
    }
    output
}

pub(crate) fn continuation_summary(continuation: &TaskContinuation) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(step) = continuation.next_step.as_deref() {
        parts.push(format!("step: {step}"));
    }
    if let Some(subtask) = continuation.next_subtask.as_deref() {
        parts.push(format!("subtask: {subtask}"));
    }
    if let Some(task) = continuation.next_task.as_deref() {
        parts.push(format!("task: {task}"));
    }
    (!parts.is_empty()).then(|| parts.join(" | "))
}

pub(crate) fn format_schedule(schedule: &TaskSchedule) -> String {
    match schedule {
        TaskSchedule::Interval { every_minutes } => format!("every {every_minutes}m"),
        TaskSchedule::Cron { expression } => format!("cron {expression}"),
    }
}

pub(crate) fn compact_text_hint(text: &str, max_len: usize) -> Option<String> {
    if max_len == 0 {
        return None;
    }
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_len {
        return Some(compact);
    }
    let keep = max_len.saturating_sub(3);
    let truncated = compact.chars().take(keep).collect::<String>();
    Some(format!("{truncated}..."))
}

fn render_task_result(task: &TaskRecord, verbose: bool, verb: &str) -> String {
    let mut output = capitalize(verb);
    output.push_str("\n  ");
    output.push_str(&format!(
        "- {} [{}] {}",
        task.summary.id, task.summary.status, task.summary.title
    ));
    let meta = if verbose {
        join_metadata([
            Some(format!(
                "updated: {}",
                format_timestamp(&task.summary.updated_at)
            )),
            task.summary
                .ready_at
                .as_ref()
                .map(|value| format!("ready: {}", format_timestamp(value))),
            task.summary
                .schedule
                .as_ref()
                .map(|value| format!("schedule: {}", format_schedule(value))),
            (!task.summary.labels.is_empty())
                .then(|| format!("labels: {}", task.summary.labels.join(", "))),
            continuation_summary(&task.summary.continuation).map(|value| format!("next: {value}")),
        ])
    } else {
        join_metadata([
            task.summary
                .schedule
                .as_ref()
                .map(|value| format!("schedule: {}", format_schedule(value))),
            (!task.summary.labels.is_empty())
                .then(|| format!("labels: {}", task.summary.labels.join(", "))),
            continuation_summary(&task.summary.continuation).map(|value| format!("next: {value}")),
        ])
    };
    if let Some(meta) = meta {
        output.push('\n');
        output.push_str("    ");
        output.push_str(&meta);
    }
    output
}

fn render_state_headline(task: &StateTask, verbose: bool) -> String {
    if verbose {
        format!("- {} {}", task.task.id, task.task.title)
    } else if let Some(next) = continuation_summary(&task.next) {
        format!("- {} {} ({})", task.task.id, task.task.title, next)
    } else {
        format!("- {} {}", task.task.id, task.task.title)
    }
}

fn render_state_meta(task: &StateTask, verbose: bool) -> Option<String> {
    if !verbose {
        return None;
    }
    join_metadata([
        Some(format!("status: {}", task.task.status)),
        Some(format!(
            "updated: {}",
            format_timestamp(&task.task.updated_at)
        )),
        task.task
            .ready_at
            .as_ref()
            .map(|value| format!("ready: {}", format_timestamp(value))),
        (task.dependency_count > 0).then(|| format!("deps: {}", task.dependency_count)),
        (task.child_count > 0).then(|| format!("children: {}", task.child_count)),
        task.task
            .schedule
            .as_ref()
            .map(|value| format!("schedule: {}", format_schedule(value))),
        (!task.task.labels.is_empty()).then(|| format!("labels: {}", task.task.labels.join(", "))),
        continuation_summary(&task.next).map(|value| format!("next: {value}")),
    ])
}

fn push_named_items(output: &mut String, title: &str, tasks: &[TaskSummary], include_status: bool) {
    if tasks.is_empty() {
        return;
    }

    push_section(
        output,
        title,
        tasks
            .iter()
            .map(|task| {
                if include_status {
                    format!("- {} [{}] {}", task.id, task.status, task.title)
                } else {
                    format!("- {} {}", task.id, task.title)
                }
            })
            .collect(),
    );
}

fn push_section(output: &mut String, title: &str, lines: Vec<String>) {
    if lines.is_empty() {
        return;
    }

    output.push_str("\n\n");
    output.push_str(title);
    for line in lines {
        output.push('\n');
        output.push_str("  ");
        output.push_str(&line);
    }
}

fn push_entry(output: &mut String, headline: String, meta: Option<String>) {
    output.push('\n');
    output.push_str("  ");
    output.push_str(&headline);
    if let Some(meta) = meta {
        output.push('\n');
        output.push_str("    ");
        output.push_str(&meta);
    }
}

fn join_metadata<I>(parts: I) -> Option<String>
where
    I: IntoIterator<Item = Option<String>>,
{
    let parts = parts.into_iter().flatten().collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join(" | "))
}

fn format_count(label: &str, count: usize) -> Option<String> {
    Some(format!("{label}: {count}"))
}

fn format_count_or_items(label: &str, items: &[TaskSummary], missing: &[String]) -> Option<String> {
    if items.is_empty() && missing.is_empty() {
        return Some(format!("{label}: 0"));
    }

    let mut values = items.iter().map(|task| task.id.clone()).collect::<Vec<_>>();
    values.extend(missing.iter().map(|value| format!("{value} [missing]")));
    Some(format!("{label}: {}", values.join(", ")))
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => {
            let mut output = first.to_uppercase().collect::<String>();
            output.push_str(chars.as_str());
            output
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_summary_joins_present_fields() {
        let summary = continuation_summary(&TaskContinuation {
            next_step: Some("resume".to_string()),
            next_subtask: Some("child".to_string()),
            next_task: None,
        })
        .unwrap();
        assert!(summary.contains("step: resume"));
        assert!(summary.contains("subtask: child"));
    }

    #[test]
    fn compact_text_hint_truncates_on_char_boundary() {
        assert_eq!(
            compact_text_hint("缓存 parser cache follow-up", 8),
            Some("缓存 pa...".to_string())
        );
    }
}
