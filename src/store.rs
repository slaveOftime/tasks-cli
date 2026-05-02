use std::path::{Path, PathBuf};

use std::fs as stdfs;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::model::{StoreIndex, TaskContinuation, TaskRecord, TaskSchedule, TaskStatus};

mod fs;
mod helpers;
mod mutations;
mod queries;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub struct AddTaskInput {
    pub id: Option<String>,
    pub title: String,
    pub summary_text: Option<String>,
    pub ready_at: Option<DateTime<Utc>>,
    pub schedule: Option<TaskSchedule>,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScheduleUpdate {
    pub schedule: Option<TaskSchedule>,
    pub ready_at: Option<DateTime<Utc>>,
    pub clear: bool,
}

#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    pub note: Option<String>,
    pub next_step: Option<String>,
    pub next_subtask: Option<String>,
    pub next_task: Option<String>,
}

impl ProgressUpdate {
    pub fn normalize(self) -> Self {
        Self {
            note: helpers::normalize_optional_text(self.note),
            next_step: helpers::normalize_optional_text(self.next_step),
            next_subtask: helpers::normalize_optional_text(self.next_subtask),
            next_task: helpers::normalize_optional_text(self.next_task),
        }
    }

    pub(crate) fn continuation(&self) -> TaskContinuation {
        TaskContinuation {
            next_step: self.next_step.clone(),
            next_subtask: self.next_subtask.clone(),
            next_task: self.next_task.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ListFilter {
    pub statuses: Vec<TaskStatus>,
    pub include_done_by_default: bool,
    pub ready_only: bool,
    pub query: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct TaskStore {
    root: PathBuf,
}

impl TaskStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn tasks_dir(&self) -> PathBuf {
        self.root.join("tasks")
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }

    fn events_path(&self) -> PathBuf {
        self.root.join("events.ndjson")
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join(".lock")
    }

    fn task_path(&self, id: &str) -> PathBuf {
        self.tasks_dir().join(format!("{id}.json"))
    }

    pub fn resolve_task_reference(&self, id: &str) -> Result<String> {
        let index = self.read_index()?;
        self.resolve_task_reference_in_index(&index, id)
    }

    fn resolve_task_reference_in_index(&self, index: &StoreIndex, id: &str) -> Result<String> {
        helpers::resolve_task_reference(index, id)
    }

    fn read_task_by_id(&self, id: &str) -> Result<TaskRecord> {
        let task_path = self.task_path(id);
        let bytes = stdfs::read(&task_path)
            .with_context(|| format!("failed to read task file '{}'", task_path.display()))?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse task file '{}'", task_path.display()))
    }
}
