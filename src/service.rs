use std::path::Path;

use anyhow::{Result, bail};
use serde::Deserialize;

use crate::model::{
    ReadyTask, StateSnapshot, StateTask, TaskDetail, TaskEvent, TaskRecord, TaskSchedule,
    TaskStatus, TaskSummary,
};
use crate::root::parse_timestamp;
use crate::store::{AddTaskInput, ListFilter, ProgressUpdate, ScheduleUpdate, TaskStore};

#[derive(Debug, Clone)]
pub(crate) struct TaskService {
    store: TaskStore,
}

impl TaskService {
    pub(crate) fn new(store: TaskStore) -> Self {
        Self { store }
    }

    pub(crate) fn root(&self) -> &Path {
        self.store.root()
    }

    pub(crate) fn add_task(&self, input: AddTaskRequest) -> Result<TaskRecord> {
        self.store.add_task(AddTaskInput {
            id: normalize_optional(input.id),
            title: input.title,
            summary_text: normalize_optional(input.summary),
            ready_at: parse_optional_timestamp(input.ready_at)?,
            schedule: schedule_from_fields(input.every_minutes, input.cron)?,
            labels: split_csv_values(input.labels),
        })
    }

    pub(crate) fn schedule_task(&self, id: &str, input: ScheduleTaskRequest) -> Result<TaskRecord> {
        self.store.configure_schedule(
            id,
            ScheduleUpdate {
                schedule: schedule_from_fields(input.every_minutes, input.cron)?,
                ready_at: parse_optional_timestamp(input.ready_at)?,
                clear: input.clear.unwrap_or(false),
            },
        )
    }

    pub(crate) fn list_tasks(&self, query: TaskListQuery) -> Result<Vec<TaskSummary>> {
        self.store.list_tasks(&ListFilter {
            statuses: query.status,
            include_done_by_default: query.all.unwrap_or(false),
            ready_only: query.ready.unwrap_or(false),
            labels: split_csv_values(query.label),
            query: normalize_optional(query.query),
            limit: query.limit,
        })
    }

    pub(crate) fn ready_tasks(&self, query: ReadyQuery) -> Result<Vec<ReadyTask>> {
        self.store
            .ready_tasks(normalize_optional(query.query), query.limit)
    }

    pub(crate) fn state_snapshot(&self, query: StateQuery) -> Result<StateSnapshot> {
        self.store
            .state_snapshot(normalize_optional(query.query), query.limit.unwrap_or(50))
    }

    pub(crate) fn continuation_tasks(&self, query: ContinuationQuery) -> Result<Vec<StateTask>> {
        self.store.continuation_tasks(query.limit.unwrap_or(50))
    }

    pub(crate) fn task_detail(&self, id: &str) -> Result<TaskDetail> {
        self.store.task_detail(id)
    }

    pub(crate) fn task_events(
        &self,
        id: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<TaskEvent>> {
        self.store.read_events(id, limit)
    }

    pub(crate) fn start_task(&self, id: &str, input: NoteRequest) -> Result<TaskRecord> {
        self.store.start_task(id, normalize_optional(input.note))
    }

    pub(crate) fn checkpoint_task(&self, id: &str, input: ProgressRequest) -> Result<TaskRecord> {
        self.store.checkpoint_task(id, input.into_update(false))
    }

    pub(crate) fn block_task(&self, id: &str, input: BlockTaskRequest) -> Result<TaskRecord> {
        self.store.block_task(id, input.reason)
    }

    pub(crate) fn review_task(&self, id: &str, input: NoteRequest) -> Result<TaskRecord> {
        self.store.review_task(id, normalize_optional(input.note))
    }

    pub(crate) fn complete_task(&self, id: &str, input: DoneTaskRequest) -> Result<TaskRecord> {
        self.store.complete_task(
            id,
            ProgressUpdate {
                note: normalize_optional(input.note),
                next_step: normalize_optional(input.next_step),
                next_task: normalize_optional(input.next_task),
                clear_schedule: input.clear_schedule.unwrap_or(false),
            },
        )
    }

    pub(crate) fn add_note(&self, id: &str, input: AddNoteRequest) -> Result<TaskRecord> {
        self.store.add_note(id, input.text)
    }

    pub(crate) fn add_dependency(
        &self,
        id: &str,
        input: DependencyTaskRequest,
    ) -> Result<TaskRecord> {
        self.store.add_dependency(id, &input.dependency)
    }

    pub(crate) fn remove_dependency(
        &self,
        id: &str,
        input: DependencyTaskRequest,
    ) -> Result<TaskRecord> {
        self.store.remove_dependency(id, &input.dependency)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct AddTaskRequest {
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) ready_at: Option<String>,
    #[serde(default)]
    pub(crate) every_minutes: Option<u32>,
    #[serde(default)]
    pub(crate) cron: Option<String>,
    #[serde(default)]
    pub(crate) labels: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ScheduleTaskRequest {
    #[serde(default)]
    pub(crate) every_minutes: Option<u32>,
    #[serde(default)]
    pub(crate) cron: Option<String>,
    #[serde(default)]
    pub(crate) ready_at: Option<String>,
    #[serde(default)]
    pub(crate) clear: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct TaskListQuery {
    #[serde(default)]
    pub(crate) status: Vec<TaskStatus>,
    #[serde(default)]
    pub(crate) all: Option<bool>,
    #[serde(default)]
    pub(crate) ready: Option<bool>,
    #[serde(default)]
    pub(crate) label: Option<String>,
    #[serde(default)]
    pub(crate) query: Option<String>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ReadyQuery {
    #[serde(default)]
    pub(crate) query: Option<String>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct StateQuery {
    #[serde(default)]
    pub(crate) query: Option<String>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ContinuationQuery {
    #[serde(default)]
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct EventsQuery {
    #[serde(default)]
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct NoteRequest {
    #[serde(default)]
    pub(crate) note: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ProgressRequest {
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default)]
    pub(crate) next_step: Option<String>,
    #[serde(default)]
    pub(crate) next_task: Option<String>,
}

impl ProgressRequest {
    fn into_update(self, clear_schedule: bool) -> ProgressUpdate {
        ProgressUpdate {
            note: normalize_optional(self.note),
            next_step: normalize_optional(self.next_step),
            next_task: normalize_optional(self.next_task),
            clear_schedule,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct DoneTaskRequest {
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default)]
    pub(crate) next_step: Option<String>,
    #[serde(default)]
    pub(crate) next_task: Option<String>,
    #[serde(default)]
    pub(crate) clear_schedule: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct BlockTaskRequest {
    pub(crate) reason: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct AddNoteRequest {
    pub(crate) text: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct DependencyTaskRequest {
    pub(crate) dependency: String,
}

pub(crate) fn schedule_from_fields(
    every_minutes: Option<u32>,
    cron: Option<String>,
) -> Result<Option<TaskSchedule>> {
    match (every_minutes, normalize_optional(cron)) {
        (Some(_), Some(_)) => bail!("every_minutes cannot be combined with cron"),
        (Some(every_minutes), None) => Ok(Some(TaskSchedule::Interval { every_minutes })),
        (None, Some(expression)) => Ok(Some(TaskSchedule::Cron { expression })),
        (None, None) => Ok(None),
    }
}

fn parse_optional_timestamp(
    value: Option<String>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    normalize_optional(value)
        .as_deref()
        .map(parse_timestamp)
        .transpose()
}

fn split_csv_values(value: Option<String>) -> Vec<String> {
    normalize_optional(value)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
