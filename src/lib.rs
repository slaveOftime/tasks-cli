mod cli;
mod model;
mod store;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use clap::Parser;

use cli::{
    AddArgs, Cli, Command, DependencyArgs, ListArgs, LogArgs, NextArgs, ProgressArgs, ReadyArgs,
    RelationArgs, RelationCommand, StateArgs, StatusNoteArgs, SubtaskArgs, SubtaskCommand,
    SubtaskLinkArgs,
};
use model::{
    ReadyTask, StateSnapshot, StateTask, TaskContinuation, TaskDetail, TaskEvent, TaskRecord,
    TaskSummary,
};
use store::{AddTaskInput, ListFilter, ProgressUpdate, TaskStore};

const SKILL_DOC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/skills/til/SKILL.md"));

pub fn run<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::parse_from(args);

    match cli.command {
        Command::Skill => handle_skill(cli.json),
        command => {
            let root = resolve_root(cli.root)?;
            let store = TaskStore::new(root);
            match command {
                Command::Add(args) => handle_add(&store, args, cli.json),
                Command::List(args) => handle_list(&store, args, cli.json),
                Command::Ready(args) => handle_ready(&store, args, cli.json),
                Command::State(args) => handle_state(&store, args, cli.json, cli.verbose),
                Command::Next(args) => handle_next(&store, args, cli.json, cli.verbose),
                Command::Show(args) => handle_show(&store, &args.id, cli.json, cli.verbose),
                Command::Start(args) => handle_start(&store, args, cli.json),
                Command::Checkpoint(args) => handle_checkpoint(&store, args, cli.json),
                Command::Block(args) => {
                    let task = store.block_task(&args.id, args.reason)?;
                    print_task_result(&task, cli.json, "blocked")
                }
                Command::Review(args) => handle_review(&store, args, cli.json),
                Command::Done(args) => handle_done(&store, args, cli.json),
                Command::Note(args) => {
                    let task = store.add_note(&args.id, args.text)?;
                    print_task_result(&task, cli.json, "updated")
                }
                Command::Dep(args) => handle_dependency(&store, args, cli.json),
                Command::Subtask(args) => handle_subtask(&store, args, cli.json),
                Command::Log(args) => handle_log(&store, args, cli.json),
                Command::Skill => unreachable!("handled above"),
            }
        }
    }
}

fn handle_skill(json: bool) -> Result<()> {
    if json {
        return print_json(&serde_json::json!({
            "path": "skills/til/SKILL.md",
            "content": SKILL_DOC,
        }));
    }
    println!("{SKILL_DOC}");
    Ok(())
}

fn handle_add(store: &TaskStore, args: AddArgs, json: bool) -> Result<()> {
    let ready_at = match args.ready_at {
        Some(value) => Some(parse_timestamp(&value)?),
        None => None,
    };
    let task = store.add_task(AddTaskInput {
        id: args.id,
        title: args.title,
        summary_text: args.summary,
        ready_at,
        labels: args.labels,
    })?;
    print_task_result(&task, json, "created")
}

fn handle_list(store: &TaskStore, args: ListArgs, json: bool) -> Result<()> {
    let items = store.list_tasks(&ListFilter {
        statuses: args.status,
        include_done_by_default: args.all,
        ready_only: args.ready,
        query: args.query,
        limit: args.limit,
    })?;
    if json {
        print_json(&items)?;
        return Ok(());
    }
    if items.is_empty() {
        println!("No matching tasks in {}", display_path(store.root()));
        return Ok(());
    }

    for task in items {
        println!("{}", format_task_summary(&task));
    }
    Ok(())
}

fn handle_ready(store: &TaskStore, args: ReadyArgs, json: bool) -> Result<()> {
    let items = store.ready_tasks(args.query, args.limit)?;
    if json {
        print_json(&items)?;
        return Ok(());
    }
    if items.is_empty() {
        println!("No ready tasks in {}", display_path(store.root()));
        return Ok(());
    }

    for task in items {
        println!("{}", format_ready_task(&task));
    }
    Ok(())
}

fn handle_state(store: &TaskStore, args: StateArgs, json: bool, verbose: bool) -> Result<()> {
    let snapshot = store.state_snapshot(args.query, args.limit)?;
    if json {
        return print_json(&snapshot);
    }

    println!("{}", format_counts(&snapshot));
    for (label, tasks) in [
        ("ready", &snapshot.ready),
        ("active", &snapshot.active),
        ("blocked", &snapshot.blocked),
        ("checkpoint", &snapshot.checkpoint),
        ("review", &snapshot.review),
        ("handoff", &snapshot.handoff),
    ] {
        if tasks.is_empty() {
            continue;
        }
        if verbose {
            println!();
            println!("{label}:");
            for task in tasks {
                println!("  - {}", format_state_task(label, task));
                if let Some(next) = continuation_summary(&task.next) {
                    println!("    next: {next}");
                }
                println!("    updated: {}", format_timestamp(&task.task.updated_at));
            }
        } else {
            for task in tasks {
                println!("{}", format_state_task(label, task));
            }
        }
    }
    Ok(())
}

fn handle_next(store: &TaskStore, args: NextArgs, json: bool, verbose: bool) -> Result<()> {
    if let Some(id) = args.id.as_deref() {
        let task = store.next_task(id)?;
        if json {
            return print_json(&task);
        }
        if task.next.is_empty() {
            println!("No continuation hints for {}", task.task.id);
            return Ok(());
        }
        print_single_next(&task, verbose);
        return Ok(());
    }

    let items = store.continuation_tasks(args.limit)?;
    if json {
        return print_json(&items);
    }
    if items.is_empty() {
        println!(
            "No checkpoint or handoff continuations in {}",
            display_path(store.root())
        );
        return Ok(());
    }
    for task in items {
        print_single_next(&task, verbose);
    }
    Ok(())
}

fn handle_show(store: &TaskStore, id: &str, json: bool, verbose: bool) -> Result<()> {
    let detail = store.task_detail(id)?;
    if json {
        return print_json(&detail);
    }
    if verbose {
        print_verbose_detail(&detail);
    } else {
        print_compact_detail(&detail);
    }
    Ok(())
}

fn handle_start(store: &TaskStore, args: StatusNoteArgs, json: bool) -> Result<()> {
    let task = store.start_task(&args.id, args.note)?;
    print_task_result(&task, json, "started")
}

fn handle_checkpoint(store: &TaskStore, args: ProgressArgs, json: bool) -> Result<()> {
    let id = args.id.clone();
    let task = store.checkpoint_task(&id, progress_update(args))?;
    print_task_result(&task, json, "checkpointed")
}

fn handle_review(store: &TaskStore, args: StatusNoteArgs, json: bool) -> Result<()> {
    let task = store.review_task(&args.id, args.note)?;
    print_task_result(&task, json, "ready for review")
}

fn handle_done(store: &TaskStore, args: ProgressArgs, json: bool) -> Result<()> {
    let id = args.id.clone();
    let task = store.complete_task(&id, progress_update(args))?;
    print_task_result(&task, json, "done")
}

fn handle_dependency(store: &TaskStore, args: RelationArgs, json: bool) -> Result<()> {
    match args.command {
        RelationCommand::Add(DependencyArgs { task, dependency }) => {
            let updated = store.add_dependency(&task, &dependency)?;
            print_link_result(
                store,
                &updated.summary.id,
                json,
                &format!("linked {} -> {}", updated.summary.id, dependency),
            )
        }
        RelationCommand::Remove(DependencyArgs { task, dependency }) => {
            let updated = store.remove_dependency(&task, &dependency)?;
            print_link_result(
                store,
                &updated.summary.id,
                json,
                &format!("unlinked {} -> {}", updated.summary.id, dependency),
            )
        }
    }
}

fn handle_subtask(store: &TaskStore, args: SubtaskArgs, json: bool) -> Result<()> {
    match args.command {
        SubtaskCommand::Add(SubtaskLinkArgs { parent, child }) => {
            let updated = store.add_subtask(&parent, &child)?;
            print_link_result(
                store,
                &updated.summary.id,
                json,
                &format!("linked {} under {}", updated.summary.id, parent),
            )
        }
        SubtaskCommand::Remove(SubtaskLinkArgs { parent, child }) => {
            let updated = store.remove_subtask(&parent, &child)?;
            print_link_result(
                store,
                &updated.summary.id,
                json,
                &format!("removed {} from {}", updated.summary.id, parent),
            )
        }
    }
}

fn handle_log(store: &TaskStore, args: LogArgs, json: bool) -> Result<()> {
    let events = store.read_events(args.id.as_deref(), args.limit)?;
    if json {
        return print_json(&events);
    }
    if events.is_empty() {
        println!("No matching events in {}", display_path(store.root()));
        return Ok(());
    }
    for event in events {
        println!("{}", format_event(&event));
    }
    Ok(())
}

fn progress_update(args: ProgressArgs) -> ProgressUpdate {
    ProgressUpdate {
        note: args.note,
        next_step: args.next_step,
        next_subtask: args.next_subtask,
        next_task: args.next_task,
    }
}

fn print_link_result(store: &TaskStore, id: &str, json: bool, message: &str) -> Result<()> {
    if json {
        return print_json(&store.task_detail(id)?);
    }
    println!("{message}");
    Ok(())
}

fn print_task_result(task: &TaskRecord, json: bool, verb: &str) -> Result<()> {
    if json {
        return print_json(task);
    }
    let labels = if task.summary.labels.is_empty() {
        String::new()
    } else {
        format!(" labels={}", task.summary.labels.join(","))
    };
    let continuation = continuation_summary(&task.summary.continuation)
        .map(|value| format!(" next={value}"))
        .unwrap_or_default();
    println!(
        "{} {} [{}] {}{}{}",
        verb, task.summary.id, task.summary.status, task.summary.title, labels, continuation
    );
    Ok(())
}

fn print_single_next(task: &StateTask, verbose: bool) {
    if verbose {
        println!(
            "{} [{}] {}",
            task.task.id, task.task.status, task.task.title
        );
        if let Some(next) = continuation_summary(&task.next) {
            println!("  next: {next}");
        }
        println!("  updated: {}", format_timestamp(&task.task.updated_at));
    } else {
        println!(
            "{} [{}] {}",
            task.task.id,
            task.task.status,
            continuation_summary(&task.next).unwrap_or_else(|| "none".to_string())
        );
    }
}

fn print_compact_detail(detail: &TaskDetail) {
    let deps = detail.task.summary.depends_on.len();
    let children = detail.children.len();
    println!(
        "{} [{}] {} ready={} deps={} children={}",
        detail.task.summary.id,
        detail.task.summary.status,
        detail.task.summary.title,
        if detail.ready { "yes" } else { "no" },
        deps,
        children
    );
    if let Some(summary) = detail.task.summary_text.as_deref() {
        println!(
            "summary: {}",
            compact_text_hint(summary, 180).unwrap_or_else(|| summary.to_string())
        );
    }
    if !detail.blocked_by.is_empty() {
        let blocked = detail
            .blocked_by
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>()
            .join(",");
        println!("blocked_by: {blocked}");
    }
    if let Some(next) = continuation_summary(&detail.next) {
        println!("next: {next}");
    }
}

fn print_verbose_detail(detail: &TaskDetail) {
    println!("id: {}", detail.task.summary.id);
    println!("title: {}", detail.task.summary.title);
    println!("status: {}", detail.task.summary.status);
    println!("ready: {}", if detail.ready { "yes" } else { "no" });
    println!(
        "created: {}",
        format_timestamp(&detail.task.summary.created_at)
    );
    println!(
        "updated: {}",
        format_timestamp(&detail.task.summary.updated_at)
    );
    if let Some(ready_at) = detail.task.summary.ready_at.as_ref() {
        println!("ready_at: {}", format_timestamp(ready_at));
    }
    if !detail.task.summary.labels.is_empty() {
        println!("labels: {}", detail.task.summary.labels.join(", "));
    }
    if let Some(parent) = detail.task.summary.parent.as_deref() {
        println!("parent: {parent}");
    }
    if let Some(next) = continuation_summary(&detail.next) {
        println!("next: {next}");
    }
    if let Some(summary) = detail.task.summary_text.as_deref() {
        println!();
        println!("summary:");
        println!("  {summary}");
    }
    if !detail.dependencies.is_empty() || !detail.missing_dependencies.is_empty() {
        println!();
        println!("dependencies:");
        for dependency in &detail.dependencies {
            println!(
                "  - {} [{}] {}",
                dependency.id, dependency.status, dependency.title
            );
        }
        for missing in &detail.missing_dependencies {
            println!("  - {} [missing]", missing);
        }
    }
    if !detail.blocked_by.is_empty() {
        println!();
        println!("blocked_by:");
        for dependency in &detail.blocked_by {
            println!(
                "  - {} [{}] {}",
                dependency.id, dependency.status, dependency.title
            );
        }
    }
    if !detail.children.is_empty() {
        println!();
        println!("children:");
        for child in &detail.children {
            println!("  - {} [{}] {}", child.id, child.status, child.title);
        }
    }
    if let Some(reason) = detail.task.blocked_reason.as_deref() {
        println!();
        println!("blocked_reason:");
        println!("  {reason}");
    }
    if let Some(note) = detail.task.completed_note.as_deref() {
        println!();
        println!("completed_note:");
        println!("  {note}");
    }
    if !detail.task.notes.is_empty() {
        println!();
        println!("notes:");
        for note in &detail.task.notes {
            println!("  - {} {}", format_timestamp(&note.at), note.text);
        }
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).context("failed to serialize JSON output")?
    );
    Ok(())
}

fn resolve_root(root: Option<PathBuf>) -> Result<PathBuf> {
    match root {
        Some(path) => Ok(path),
        None => {
            let cwd = std::env::current_dir().context("failed to resolve current directory")?;
            find_existing_root(&cwd).ok_or_else(|| {
                anyhow::anyhow!(
                    "could not find '.tli' from '{}' up to filesystem root; pass --root <path> to create or target a store explicitly",
                    display_path(&cwd)
                )
            })
        }
    }
}

fn find_existing_root(start: &Path) -> Option<PathBuf> {
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

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("expected RFC3339 timestamp, got '{value}'"))?;
    Ok(parsed.with_timezone(&Utc))
}

fn format_timestamp(value: &DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S %:z")
        .to_string()
}

fn format_task_summary(task: &TaskSummary) -> String {
    let ready_at = task
        .ready_at
        .as_ref()
        .map(|value| format!(" ready={}", format_timestamp(value)))
        .unwrap_or_default();
    let labels = if task.labels.is_empty() {
        String::new()
    } else {
        format!(" labels={}", task.labels.join(","))
    };
    let deps = if task.depends_on.is_empty() {
        String::new()
    } else {
        format!(" deps={}", task.depends_on.len())
    };
    let next = continuation_summary(&task.continuation)
        .map(|value| format!(" next={value}"))
        .unwrap_or_default();
    let mut output = format!(
        "{:<28} {:<10} {}{}{}{}",
        task.id, task.status, task.title, ready_at, labels, deps
    );
    output.push_str(&next);
    output
}

fn format_ready_task(task: &ReadyTask) -> String {
    let labels = if task.task.labels.is_empty() {
        String::new()
    } else {
        format!(" labels={}", task.task.labels.join(","))
    };
    let deps = if task.dependency_count == 0 {
        String::new()
    } else {
        format!(" deps={}", task.dependency_count)
    };
    let children = if task.child_count == 0 {
        String::new()
    } else {
        format!(" children={}", task.child_count)
    };
    let mut output = format!(
        "{:<28} {}{}{}",
        task.task.id, task.task.title, deps, children
    );
    output.push_str(&labels);
    output
}

fn format_counts(snapshot: &StateSnapshot) -> String {
    format!(
        "counts ready={} active={} blocked={} checkpoint={} review={} handoff={} todo={} done={}",
        snapshot.counts.ready,
        snapshot.counts.active,
        snapshot.counts.blocked,
        snapshot.counts.checkpoint,
        snapshot.counts.review,
        snapshot.counts.handoff,
        snapshot.counts.todo,
        snapshot.counts.done
    )
}

fn format_state_task(label: &str, task: &StateTask) -> String {
    let deps = if task.dependency_count == 0 {
        String::new()
    } else {
        format!(" deps={}", task.dependency_count)
    };
    let children = if task.child_count == 0 {
        String::new()
    } else {
        format!(" children={}", task.child_count)
    };
    let next = continuation_summary(&task.next)
        .map(|value| format!(" next={value}"))
        .unwrap_or_default();
    format!(
        "{} {} | {}{}{}{}",
        label, task.task.id, task.task.title, deps, children, next
    )
}

fn continuation_summary(continuation: &TaskContinuation) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(step) = continuation.next_step.as_deref() {
        parts.push(format!("step:{step}"));
    }
    if let Some(subtask) = continuation.next_subtask.as_deref() {
        parts.push(format!("subtask:{subtask}"));
    }
    if let Some(task) = continuation.next_task.as_deref() {
        parts.push(format!("task:{task}"));
    }
    (!parts.is_empty()).then(|| parts.join(" | "))
}

fn compact_text_hint(text: &str, max_len: usize) -> Option<String> {
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

fn format_event(event: &TaskEvent) -> String {
    let status = event
        .status
        .map(|status| format!(" [{}]", status))
        .unwrap_or_default();
    format!(
        "{} {:<28} {:<16}{} {}",
        format_timestamp(&event.at),
        event.task_id,
        event.kind,
        status,
        event.message
    )
}

fn display_path(path: &Path) -> String {
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
    fn continuation_summary_joins_present_fields() {
        let summary = continuation_summary(&TaskContinuation {
            next_step: Some("resume".to_string()),
            next_subtask: Some("child".to_string()),
            next_task: None,
        })
        .unwrap();
        assert!(summary.contains("step:resume"));
        assert!(summary.contains("subtask:child"));
    }

    #[test]
    fn compact_text_hint_truncates_on_char_boundary() {
        assert_eq!(
            compact_text_hint("缓存 parser cache follow-up", 8),
            Some("缓存 pa...".to_string())
        );
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
