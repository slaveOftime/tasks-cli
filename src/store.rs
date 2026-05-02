use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::model::{TaskContinuation, TaskSchedule, TaskStatus};

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
}
