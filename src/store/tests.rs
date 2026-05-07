use chrono::{Timelike, TimeZone};
use tempfile::TempDir;

use super::helpers::{next_scheduled_ready_at, slugify};
use super::*;

#[test]
fn slugify_compacts_non_identifier_characters() {
    assert_eq!(
        slugify("  Build: Agent-First CLI! "),
        "build-agent-first-cli"
    );
    assert_eq!(slugify("###"), "");
}

#[test]
fn store_tracks_dependencies_ready_and_continuation() {
    let temp = TempDir::new().unwrap();
    let store = TaskStore::new(temp.path().join(".tli"));

    for (id, title) in [("alpha", "Alpha"), ("beta", "Beta"), ("child", "Child")] {
        store
            .add_task(AddTaskInput {
                id: Some(id.to_string()),
                title: title.to_string(),
                summary_text: None,
                ready_at: None,
                schedule: None,
                labels: vec![],
            })
            .unwrap();
    }

    store.add_dependency("beta", "alpha").unwrap();
    store.add_subtask("beta", "child").unwrap();

    let ready = store.ready_tasks(None, None).unwrap();
    let mut ids = ready
        .iter()
        .map(|task| task.task.id.as_str())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    assert_eq!(ids, vec!["alpha", "child"]);

    let checkpointed = store
        .checkpoint_task(
            "beta",
            ProgressUpdate {
                note: Some("pause here".to_string()),
                next_step: Some("resume api wiring".to_string()),
                next_subtask: None,
                next_task: None,
            },
        )
        .unwrap();
    assert_eq!(checkpointed.summary.status, TaskStatus::Checkpoint);

    let next = store.next_task("beta").unwrap();
    assert_eq!(next.next.next_step.as_deref(), Some("resume api wiring"));
    assert_eq!(next.next.next_subtask.as_deref(), Some("child"));
    assert_eq!(next.next.next_task.as_deref(), Some("alpha"));
}

#[test]
fn dependency_cycles_are_rejected() {
    let temp = TempDir::new().unwrap();
    let store = TaskStore::new(temp.path().join(".tli"));

    for id in ["one", "two"] {
        store
            .add_task(AddTaskInput {
                id: Some(id.to_string()),
                title: id.to_string(),
                summary_text: None,
                ready_at: None,
                schedule: None,
                labels: vec![],
            })
            .unwrap();
    }

    store.add_dependency("two", "one").unwrap();
    let error = store.add_dependency("one", "two").unwrap_err().to_string();
    assert!(error.contains("would create a cycle"));
}

#[test]
fn scheduled_tasks_rearm_to_todo_with_next_ready_at() {
    let temp = TempDir::new().unwrap();
    let store = TaskStore::new(temp.path().join(".tli"));

    let task = store
        .add_task(AddTaskInput {
            id: Some("daily".to_string()),
            title: "Daily review".to_string(),
            summary_text: None,
            ready_at: Some(
                DateTime::parse_from_rfc3339("2026-05-02T08:00:00+08:00")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            schedule: Some(TaskSchedule::Interval {
                every_minutes: 1440,
            }),
            labels: vec![],
        })
        .unwrap();

    assert_eq!(task.summary.status, TaskStatus::Todo);
    assert!(task.summary.schedule.is_some());

    let completed = store
        .complete_task(
            "daily",
            ProgressUpdate {
                note: Some("Cycle done".to_string()),
                next_step: None,
                next_subtask: None,
                next_task: None,
            },
        )
        .unwrap();

    assert_eq!(completed.summary.status, TaskStatus::Todo);
    assert_eq!(completed.completed_note.as_deref(), Some("Cycle done"));
    assert_eq!(
        completed.summary.ready_at.unwrap(),
        next_scheduled_ready_at(
            Some(task.summary.ready_at.unwrap()),
            task.summary.schedule.as_ref().unwrap(),
            completed.completed_at.unwrap(),
        )
        .unwrap()
    );
}

#[test]
fn cron_schedules_rearm_in_local_time() {
    let now_local = chrono::Local.with_ymd_and_hms(2026, 5, 2, 23, 24, 0).single().unwrap();
    let ready_at_local = chrono::Local.with_ymd_and_hms(2026, 5, 2, 23, 20, 0).single().unwrap();

    let next = next_scheduled_ready_at(
        Some(ready_at_local.with_timezone(&Utc)),
        &TaskSchedule::Cron {
            expression: "20 23 * * *".to_string(),
        },
        now_local.with_timezone(&Utc),
    )
    .unwrap()
    .with_timezone(&chrono::Local);

    assert_eq!(next.hour(), 23);
    assert_eq!(next.minute(), 20);
    assert_eq!(next.date_naive().to_string(), "2026-05-03");
}

#[test]
fn task_event_logs_are_backfilled_from_global_events() {
    let temp = TempDir::new().unwrap();
    let store = TaskStore::new(temp.path().join(".tli"));

    for id in ["alpha", "beta"] {
        store
            .add_task(AddTaskInput {
                id: Some(id.to_string()),
                title: id.to_string(),
                summary_text: None,
                ready_at: None,
                schedule: None,
                labels: vec![],
            })
            .unwrap();
        store.add_note(id, format!("{id} note")).unwrap();
    }

    std::fs::remove_dir_all(store.task_events_dir()).unwrap();

    let events = store.read_events(Some("alpha"), None).unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| event.task_id == "alpha"));
    assert!(store.task_events_path("alpha").is_file());

    let beta_events = store.read_events(Some("beta"), None).unwrap();
    assert_eq!(beta_events.len(), 2);
    assert!(beta_events.iter().all(|event| event.task_id == "beta"));
    assert!(store.task_events_path("beta").is_file());
}
