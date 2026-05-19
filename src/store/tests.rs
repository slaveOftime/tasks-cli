use chrono::{TimeZone, Timelike};
use tempfile::TempDir;

use super::helpers::{next_scheduled_ready_at, slugify};
use super::*;
use crate::root::format_timestamp;

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

    for (id, title) in [("alpha", "Alpha"), ("beta", "Beta")] {
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

    let ready = store.ready_tasks(None, None).unwrap();
    let mut ids = ready
        .iter()
        .map(|task| task.task.id.as_str())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    assert_eq!(ids, vec!["alpha"]);

    let checkpointed = store
        .checkpoint_task(
            "beta",
            ProgressUpdate {
                note: Some("pause here".to_string()),
                next_step: Some("resume api wiring".to_string()),
                next_task: None,
                clear_schedule: false,
            },
        )
        .unwrap();
    assert_eq!(checkpointed.summary.status, TaskStatus::Checkpoint);

    let next = store.next_task("beta").unwrap();
    assert_eq!(next.next.next_step.as_deref(), Some("resume api wiring"));
    assert_eq!(next.next.next_task.as_deref(), Some("alpha"));
}

#[test]
fn tasks_without_dependencies_can_infer_ready_top_level_next_task() {
    let temp = TempDir::new().unwrap();
    let store = TaskStore::new(temp.path().join(".tli"));

    for (id, title) in [("alpha", "Alpha"), ("beta", "Beta")] {
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

    let detail = store.task_detail("alpha").unwrap();
    assert_eq!(detail.next.next_task.as_deref(), Some("beta"));
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
                next_task: None,
                clear_schedule: false,
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
    let events = store.read_events(Some("daily"), Some(1)).unwrap();
    let message = &events[0].message;
    assert!(message.contains("next ready "));
    assert!(message.contains(&format_timestamp(&completed.summary.ready_at.unwrap())));
    assert!(!message.contains('T'));
}

#[test]
fn cron_schedules_rearm_in_local_time() {
    let now_local = chrono::Local
        .with_ymd_and_hms(2026, 5, 2, 23, 24, 0)
        .single()
        .unwrap();
    let ready_at_local = chrono::Local
        .with_ymd_and_hms(2026, 5, 2, 23, 20, 0)
        .single()
        .unwrap();

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
fn clearing_schedule_removes_pending_ready_at() {
    let temp = TempDir::new().unwrap();
    let store = TaskStore::new(temp.path().join(".tli"));

    let task = store
        .add_task(AddTaskInput {
            id: Some("daily".to_string()),
            title: "Daily review".to_string(),
            summary_text: None,
            ready_at: None,
            schedule: Some(TaskSchedule::Interval { every_minutes: 60 }),
            labels: vec![],
        })
        .unwrap();

    assert!(task.summary.schedule.is_some());
    assert!(task.summary.ready_at.is_some());

    let cleared = store
        .configure_schedule(
            "daily",
            ScheduleUpdate {
                schedule: None,
                ready_at: None,
                clear: true,
            },
        )
        .unwrap();

    assert!(cleared.summary.schedule.is_none());
    assert!(cleared.summary.ready_at.is_none());
}

#[test]
fn completing_with_clear_schedule_finishes_scheduled_task_permanently() {
    let temp = TempDir::new().unwrap();
    let store = TaskStore::new(temp.path().join(".tli"));

    store
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

    let completed = store
        .complete_task(
            "daily",
            ProgressUpdate {
                note: Some("Final cycle".to_string()),
                next_step: None,
                next_task: None,
                clear_schedule: true,
            },
        )
        .unwrap();

    assert_eq!(completed.summary.status, TaskStatus::Done);
    assert!(completed.summary.schedule.is_none());
    assert!(completed.summary.ready_at.is_none());
    assert_eq!(completed.completed_note.as_deref(), Some("Final cycle"));
}

#[test]
fn due_scheduled_tasks_with_unmet_dependencies_are_returned_with_warnings() {
    let temp = TempDir::new().unwrap();
    let store = TaskStore::new(temp.path().join(".tli"));

    store
        .add_task(AddTaskInput {
            id: Some("dep".to_string()),
            title: "Dependency".to_string(),
            summary_text: None,
            ready_at: None,
            schedule: None,
            labels: vec![],
        })
        .unwrap();
    store
        .add_task(AddTaskInput {
            id: Some("scheduled".to_string()),
            title: "Scheduled follow-up".to_string(),
            summary_text: None,
            ready_at: Some(
                DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            schedule: Some(TaskSchedule::Interval { every_minutes: 60 }),
            labels: vec![],
        })
        .unwrap();
    store.add_dependency("scheduled", "dep").unwrap();

    let ready = store.ready_tasks(None, None).unwrap();
    let scheduled = ready
        .iter()
        .find(|task| task.task.id == "scheduled")
        .unwrap();

    assert!(!scheduled.ready);
    assert_eq!(scheduled.missing_dependencies, vec!["dep"]);
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
