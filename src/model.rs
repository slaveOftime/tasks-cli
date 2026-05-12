use std::collections::BTreeMap;
use std::fmt::{self, Display};

use chrono::{DateTime, Utc};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

pub const STORE_SCHEMA_VERSION: u32 = 3;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    Active,
    Checkpoint,
    Blocked,
    Review,
    Done,
}

impl TaskStatus {
    pub fn sort_rank(self) -> u8 {
        match self {
            Self::Active => 0,
            Self::Todo => 1,
            Self::Checkpoint => 2,
            Self::Blocked => 3,
            Self::Review => 4,
            Self::Done => 5,
        }
    }
}

impl Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Todo => "todo",
            Self::Active => "active",
            Self::Checkpoint => "checkpoint",
            Self::Blocked => "blocked",
            Self::Review => "review",
            Self::Done => "done",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TaskSchedule {
    Interval { every_minutes: u32 },
    Cron { expression: String },
}

impl Display for TaskSchedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interval { every_minutes } => write!(f, "every {every_minutes}m"),
            Self::Cron { expression } => write!(f, "cron {expression}"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskContinuation {
    #[serde(default)]
    pub next_step: Option<String>,
    #[serde(default)]
    pub next_task: Option<String>,
}

impl TaskContinuation {
    pub fn is_empty(&self) -> bool {
        self.next_step.is_none() && self.next_task.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub ready_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub schedule: Option<TaskSchedule>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub continuation: TaskContinuation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNote {
    pub at: DateTime<Utc>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    #[serde(flatten)]
    pub summary: TaskSummary,
    #[serde(default)]
    pub summary_text: Option<String>,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub completed_note: Option<String>,
    #[serde(default)]
    pub active_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub checkpointed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub review_requested_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub notes: Vec<TaskNote>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventKind {
    Created,
    Started,
    ScheduleUpdated,
    Checkpointed,
    Blocked,
    ReviewRequested,
    Completed,
    NoteAdded,
    DependencyAdded,
    DependencyRemoved,
    SubtaskAdded,
    SubtaskRemoved,
}

impl Display for TaskEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Created => "created",
            Self::Started => "started",
            Self::ScheduleUpdated => "schedule_updated",
            Self::Checkpointed => "checkpointed",
            Self::Blocked => "blocked",
            Self::ReviewRequested => "review_requested",
            Self::Completed => "completed",
            Self::NoteAdded => "note_added",
            Self::DependencyAdded => "dependency_added",
            Self::DependencyRemoved => "dependency_removed",
            Self::SubtaskAdded => "subtask_added",
            Self::SubtaskRemoved => "subtask_removed",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEvent {
    pub at: DateTime<Utc>,
    pub task_id: String,
    pub kind: TaskEventKind,
    #[serde(default)]
    pub status: Option<TaskStatus>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDetail {
    pub task: TaskRecord,
    #[serde(default)]
    pub dependencies: Vec<TaskSummary>,
    #[serde(default)]
    pub missing_dependencies: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<TaskSummary>,
    pub ready: bool,
    #[serde(default)]
    pub next: TaskContinuation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyTask {
    #[serde(flatten)]
    pub task: TaskSummary,
    pub ready: bool,
    pub dependency_count: usize,
    #[serde(default)]
    pub missing_dependencies: Vec<String>,
    #[serde(default)]
    pub next: TaskContinuation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTask {
    #[serde(flatten)]
    pub task: TaskSummary,
    pub ready: bool,
    pub dependency_count: usize,
    #[serde(default)]
    pub next: TaskContinuation,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateCounts {
    pub todo: usize,
    pub active: usize,
    pub checkpoint: usize,
    pub blocked: usize,
    pub review: usize,
    pub done: usize,
    pub ready: usize,
    pub pending_dependencies: usize,
    pub handoff: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateSnapshot {
    #[serde(default)]
    pub counts: StateCounts,
    #[serde(default)]
    pub ready: Vec<StateTask>,
    #[serde(default)]
    pub pending_dependencies: Vec<StateTask>,
    #[serde(default)]
    pub active: Vec<StateTask>,
    #[serde(default)]
    pub blocked: Vec<StateTask>,
    #[serde(default)]
    pub checkpoint: Vec<StateTask>,
    #[serde(default)]
    pub review: Vec<StateTask>,
    #[serde(default)]
    pub handoff: Vec<StateTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreIndex {
    pub schema_version: u32,
    pub tasks: BTreeMap<String, TaskSummary>,
}

impl Default for StoreIndex {
    fn default() -> Self {
        Self {
            schema_version: STORE_SCHEMA_VERSION,
            tasks: BTreeMap::new(),
        }
    }
}
