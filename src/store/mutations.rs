use anyhow::{Result, bail};
use chrono::{DateTime, Local, Utc};
use clap::builder::Str;

use crate::model::{
    StoreIndex, TaskContinuation, TaskEvent, TaskEventKind, TaskNote, TaskRecord, TaskStatus,
    TaskSummary,
};

use super::{
    AddTaskInput, ProgressUpdate, ScheduleUpdate, TaskStore,
    helpers::{
        describe_progress_message, ensure_distinct, ensure_task_exists, has_dependency_path,
        next_scheduled_ready_at, normalize_labels, normalize_optional_text,
        normalize_required_text, resolve_ready_at, slugify, validate_schedule,
    },
};

impl TaskStore {
    pub fn add_task(&self, input: AddTaskInput) -> Result<TaskRecord> {
        let _lock = self.acquire_write_lock()?;
        let mut index = self.read_index()?;
        let now = Utc::now();
        let id = self.next_task_id(input.id.as_deref(), &input.title, &index)?;
        let schedule = validate_schedule(input.schedule)?;
        let task = TaskRecord {
            summary: TaskSummary {
                id: id.clone(),
                title: input.title.trim().to_string(),
                status: TaskStatus::Todo,
                created_at: now,
                updated_at: now,
                ready_at: resolve_ready_at(input.ready_at, schedule.as_ref(), now)?,
                schedule,
                labels: normalize_labels(input.labels),
                depends_on: Vec::new(),
                continuation: TaskContinuation::default(),
            },
            summary_text: normalize_optional_text(input.summary_text),
            blocked_reason: None,
            completed_at: None,
            completed_note: None,
            active_at: None,
            checkpointed_at: None,
            review_requested_at: None,
            notes: Vec::new(),
        };

        index.tasks.insert(id.clone(), task.summary.clone());
        self.write_task(&task)?;
        self.write_index(&index)?;
        self.append_event(TaskEvent {
            at: now,
            task_id: id,
            kind: TaskEventKind::Created,
            status: Some(TaskStatus::Todo),
            message: "task created".to_string(),
        })?;
        Ok(task)
    }

    pub fn start_task(&self, id: &str, note: Option<String>) -> Result<TaskRecord> {
        let note = normalize_optional_text(note);
        let id = self.resolve_task_reference(id)?;
        self.update_task_resolved(&id, TaskEventKind::Started, |task, now| {
            if task.summary.status == TaskStatus::Done {
                bail!("cannot start task '{id}' because it is already done");
            }
            task.summary.status = TaskStatus::Active;
            task.summary.updated_at = now;
            task.summary.continuation = TaskContinuation::default();
            task.active_at = Some(now);
            task.checkpointed_at = None;
            task.blocked_reason = None;
            if let Some(note) = note {
                task.notes.push(TaskNote {
                    at: now,
                    text: note,
                });
            }
            Ok(String::new())
        })
    }

    pub fn checkpoint_task(&self, id: &str, update: ProgressUpdate) -> Result<TaskRecord> {
        let update = update.normalize();
        let id = self.resolve_task_reference(id)?;
        self.update_task_resolved(&id, TaskEventKind::Checkpointed, |task, now| {
            if task.summary.status == TaskStatus::Done {
                bail!("cannot checkpoint task '{id}' because it is already done");
            }
            task.summary.status = TaskStatus::Checkpoint;
            task.summary.updated_at = now;
            task.summary.continuation = update.continuation();
            task.checkpointed_at = Some(now);
            task.blocked_reason = None;
            if let Some(note) = update.note.clone() {
                task.notes.push(TaskNote {
                    at: now,
                    text: note,
                });
            }
            Ok(describe_progress_message(&task.summary.continuation))
        })
    }

    pub fn block_task(&self, id: &str, reason: String) -> Result<TaskRecord> {
        let id = self.resolve_task_reference(id)?;
        self.update_task_resolved(&id, TaskEventKind::Blocked, |task, now| {
            if task.summary.status == TaskStatus::Done {
                bail!("cannot block task '{id}' because it is already done");
            }
            let reason = normalize_required_text(reason.clone(), "block reason")?;
            task.summary.status = TaskStatus::Blocked;
            task.summary.updated_at = now;
            task.blocked_reason = Some(reason.clone());
            task.notes.push(TaskNote {
                at: now,
                text: format!("Blocked: {reason}"),
            });
            Ok(reason)
        })
    }

    pub fn review_task(&self, id: &str, note: Option<String>) -> Result<TaskRecord> {
        let note = normalize_optional_text(note);
        let id = self.resolve_task_reference(id)?;
        self.update_task_resolved(&id, TaskEventKind::ReviewRequested, |task, now| {
            if task.summary.status == TaskStatus::Done {
                bail!("cannot send task '{id}' to review because it is already done");
            }
            task.summary.status = TaskStatus::Review;
            task.summary.updated_at = now;
            task.review_requested_at = Some(now);
            task.blocked_reason = None;
            if let Some(note) = note.clone() {
                task.notes.push(TaskNote {
                    at: now,
                    text: note.clone(),
                });
                Ok(note)
            } else {
                Ok("".to_string())
            }
        })
    }

    pub fn configure_schedule(&self, id: &str, update: ScheduleUpdate) -> Result<TaskRecord> {
        let schedule = validate_schedule(update.schedule)?;
        let id = self.resolve_task_reference(id)?;
        self.update_task_resolved(&id, TaskEventKind::ScheduleUpdated, move |task, now| {
            if update.clear {
                if schedule.is_some() || update.ready_at.is_some() {
                    bail!("--clear cannot be combined with --cron, --every-minutes, or --ready-at");
                }
                task.summary.schedule = None;
                task.summary.ready_at = None;
                return Ok("schedule cleared".to_string());
            }

            let Some(schedule) = schedule.clone() else {
                bail!("schedule update requires --cron, --every-minutes, or --clear");
            };
            task.summary.schedule = Some(schedule.clone());
            task.summary.ready_at = resolve_ready_at(update.ready_at, Some(&schedule), now)?;
            Ok(format!(
                "updated at: {} next={}",
                schedule,
                task.summary
                    .ready_at
                    .as_ref()
                    .map(DateTime::<Utc>::to_rfc3339)
                    .unwrap_or_else(|| "none".to_string())
            ))
        })
    }

    pub fn complete_task(&self, id: &str, update: ProgressUpdate) -> Result<TaskRecord> {
        let update = update.normalize();
        let id = self.resolve_task_reference(id)?;
        self.update_task_resolved(&id, TaskEventKind::Completed, |task, now| {
            task.summary.updated_at = now;
            task.summary.continuation = update.continuation();
            task.completed_at = Some(now);
            task.blocked_reason = None;
            task.completed_note = update.note.clone();
            if let Some(note) = update.note.clone() {
                task.notes.push(TaskNote {
                    at: now,
                    text: note.clone(),
                });
            }

            if update.clear_schedule {
                task.summary.schedule = None;
                task.summary.ready_at = None;
                task.summary.status = TaskStatus::Done;
                return Ok(describe_progress_message(&task.summary.continuation));
            }

            if let Some(schedule) = task.summary.schedule.as_ref() {
                let next_ready_at = next_scheduled_ready_at(task.summary.ready_at, schedule, now)?;
                task.summary.status = TaskStatus::Todo;
                task.summary.ready_at = Some(next_ready_at);
                return Ok(format!(
                    "{}; next ready at {}",
                    describe_progress_message(&task.summary.continuation),
                    next_ready_at.to_rfc3339()
                ));
            }

            task.summary.status = TaskStatus::Done;
            Ok(describe_progress_message(&task.summary.continuation))
        })
    }

    pub fn add_note(&self, id: &str, text: String) -> Result<TaskRecord> {
        let id = self.resolve_task_reference(id)?;
        self.update_task_resolved(&id, TaskEventKind::NoteAdded, |task, now| {
            let text = normalize_required_text(text.clone(), "note")?;
            task.summary.updated_at = now;
            task.notes.push(TaskNote {
                at: now,
                text: text.clone(),
            });
            Ok(text)
        })
    }

    pub fn add_dependency(&self, task_id: &str, dependency_id: &str) -> Result<TaskRecord> {
        let _lock = self.acquire_write_lock()?;
        let mut index = self.read_index()?;
        let task_id = self.resolve_task_reference_in_index(&index, task_id)?;
        let dependency_id = self.resolve_task_reference_in_index(&index, dependency_id)?;
        ensure_distinct(&task_id, &dependency_id, "dependency")?;
        ensure_task_exists(&index, &task_id)?;
        ensure_task_exists(&index, &dependency_id)?;
        if has_dependency_path(&index, &dependency_id, &task_id) {
            bail!(
                "cannot add dependency '{}' -> '{}' because it would create a cycle",
                task_id,
                dependency_id
            );
        }

        let mut task = self.read_task_by_id(&task_id)?;
        if task
            .summary
            .depends_on
            .iter()
            .any(|value| value == &dependency_id)
        {
            bail!("task '{task_id}' already depends on '{dependency_id}'");
        }

        task.summary.depends_on.push(dependency_id.clone());
        task.summary.depends_on.sort();
        task.summary.updated_at = Utc::now();
        index
            .tasks
            .insert(task.summary.id.clone(), task.summary.clone());
        self.write_task(&task)?;
        self.write_index(&index)?;
        self.append_event(TaskEvent {
            at: task.summary.updated_at,
            task_id: task.summary.id.clone(),
            kind: TaskEventKind::DependencyAdded,
            status: Some(task.summary.status),
            message: format!("task now depends on {dependency_id}"),
        })?;
        Ok(task)
    }

    pub fn remove_dependency(&self, task_id: &str, dependency_id: &str) -> Result<TaskRecord> {
        let _lock = self.acquire_write_lock()?;
        let mut index = self.read_index()?;
        let task_id = self.resolve_task_reference_in_index(&index, task_id)?;
        let dependency_id = self.resolve_task_reference_in_index(&index, dependency_id)?;
        ensure_task_exists(&index, &task_id)?;
        let mut task = self.read_task_by_id(&task_id)?;
        let original_len = task.summary.depends_on.len();
        task.summary
            .depends_on
            .retain(|value| value != &dependency_id);
        if task.summary.depends_on.len() == original_len {
            bail!("task '{task_id}' does not depend on '{dependency_id}'");
        }

        task.summary.updated_at = Utc::now();
        index
            .tasks
            .insert(task.summary.id.clone(), task.summary.clone());
        self.write_task(&task)?;
        self.write_index(&index)?;
        self.append_event(TaskEvent {
            at: task.summary.updated_at,
            task_id: task.summary.id.clone(),
            kind: TaskEventKind::DependencyRemoved,
            status: Some(task.summary.status),
            message: format!("dependency removed: {dependency_id}"),
        })?;
        Ok(task)
    }

    fn update_task_resolved<F>(
        &self,
        id: &str,
        event_kind: TaskEventKind,
        update: F,
    ) -> Result<TaskRecord>
    where
        F: FnOnce(&mut TaskRecord, DateTime<Utc>) -> Result<String>,
    {
        let _lock = self.acquire_write_lock()?;
        let mut index = self.read_index()?;
        let mut task = self.read_task_by_id(id)?;
        let now = Utc::now();
        let message = update(&mut task, now)?;
        task.summary.updated_at = now;
        index
            .tasks
            .insert(task.summary.id.clone(), task.summary.clone());
        self.write_task(&task)?;
        self.write_index(&index)?;
        self.append_event(TaskEvent {
            at: now,
            task_id: task.summary.id.clone(),
            kind: event_kind,
            status: Some(task.summary.status),
            message,
        })?;
        Ok(task)
    }

    fn next_task_id(
        &self,
        preferred: Option<&str>,
        title: &str,
        index: &StoreIndex,
    ) -> Result<String> {
        let base = if let Some(preferred) = preferred {
            let normalized = slugify(preferred);
            if normalized.is_empty() {
                bail!("task id '{preferred}' does not contain usable characters");
            }
            normalized
        } else {
            let title_slug = slugify(title);
            if title_slug.is_empty() {
                bail!("task title must contain letters or numbers");
            }
            format!("{}-{}", Local::now().format("%Y%m%d-%H%M%S"), title_slug)
        };

        if !index.tasks.contains_key(&base) && !self.task_path(&base).exists() {
            return Ok(base);
        }
        if preferred.is_some() {
            bail!("task id '{base}' already exists");
        }

        for counter in 2.. {
            let candidate = format!("{base}-{counter}");
            if !index.tasks.contains_key(&candidate) && !self.task_path(&candidate).exists() {
                return Ok(candidate);
            }
        }
        unreachable!("monotonic integer suffix should eventually become unique")
    }
}
