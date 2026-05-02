use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Local, Utc};

use crate::model::{
    ReadyTask, STORE_SCHEMA_VERSION, StateCounts, StateSnapshot, StateTask, StoreIndex,
    TaskContinuation, TaskDetail, TaskEvent, TaskEventKind, TaskNote, TaskRecord, TaskStatus,
    TaskSummary,
};

#[derive(Debug, Clone)]
pub struct AddTaskInput {
    pub id: Option<String>,
    pub title: String,
    pub summary_text: Option<String>,
    pub ready_at: Option<DateTime<Utc>>,
    pub labels: Vec<String>,
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
            note: normalize_optional_text(self.note),
            next_step: normalize_optional_text(self.next_step),
            next_subtask: normalize_optional_text(self.next_subtask),
            next_task: normalize_optional_text(self.next_task),
        }
    }

    fn continuation(&self) -> TaskContinuation {
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

    pub fn add_task(&self, input: AddTaskInput) -> Result<TaskRecord> {
        let _lock = self.acquire_write_lock()?;
        let mut index = self.read_index()?;
        let now = Utc::now();
        let id = self.next_task_id(input.id.as_deref(), &input.title, &index)?;
        let task = TaskRecord {
            summary: TaskSummary {
                id: id.clone(),
                title: input.title.trim().to_string(),
                status: TaskStatus::Todo,
                created_at: now,
                updated_at: now,
                ready_at: input.ready_at,
                labels: normalize_labels(input.labels),
                depends_on: Vec::new(),
                parent: None,
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

    pub fn list_tasks(&self, filter: &ListFilter) -> Result<Vec<TaskSummary>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let index = self.read_index()?;
        let now = Utc::now();
        let query = normalize_query(filter.query.as_deref());
        let mut items = index.tasks.values().cloned().collect::<Vec<_>>();
        items.retain(|task| match_status_filter(task, filter));
        if filter.ready_only {
            items.retain(|task| is_ready_summary(task, &index, now));
        }
        if let Some(query) = query.as_deref() {
            items.retain(|task| task_matches_query(task, query));
        }
        items.sort_by(compare_summary_for_list);
        if let Some(limit) = filter.limit {
            items.truncate(limit);
        }
        Ok(items)
    }

    pub fn ready_tasks(
        &self,
        query: Option<String>,
        limit: Option<usize>,
    ) -> Result<Vec<ReadyTask>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let index = self.read_index()?;
        let now = Utc::now();
        let query = normalize_query(query.as_deref());
        let mut items = index
            .tasks
            .values()
            .filter(|task| is_ready_summary(task, &index, now))
            .cloned()
            .collect::<Vec<_>>();
        if let Some(query) = query.as_deref() {
            items.retain(|task| task_matches_query(task, query));
        }
        items.sort_by(compare_summary_for_list);
        let mut ready = items
            .into_iter()
            .map(|task| ReadyTask {
                dependency_count: task.depends_on.len(),
                child_count: child_count(&index, &task.id),
                next: resolve_continuation(&task, &index, now),
                task,
            })
            .collect::<Vec<_>>();
        if let Some(limit) = limit {
            ready.truncate(limit);
        }
        Ok(ready)
    }

    pub fn state_snapshot(&self, query: Option<String>, limit: usize) -> Result<StateSnapshot> {
        if !self.root.exists() {
            return Ok(StateSnapshot::default());
        }

        let index = self.read_index()?;
        let now = Utc::now();
        let query = normalize_query(query.as_deref());
        let tasks = index
            .tasks
            .values()
            .filter(|task| {
                query
                    .as_deref()
                    .is_none_or(|query| task_matches_query(task, query))
            })
            .cloned()
            .collect::<Vec<_>>();

        let counts = build_counts(&tasks, &index, now);
        let ready = build_state_section(
            tasks
                .iter()
                .filter(|task| is_ready_summary(task, &index, now))
                .cloned()
                .collect(),
            &index,
            now,
            limit,
        );
        let active = build_state_section(
            tasks
                .iter()
                .filter(|task| task.status == TaskStatus::Active)
                .cloned()
                .collect(),
            &index,
            now,
            limit,
        );
        let blocked = build_state_section(
            tasks
                .iter()
                .filter(|task| task.status == TaskStatus::Blocked)
                .cloned()
                .collect(),
            &index,
            now,
            limit,
        );
        let checkpoint = build_state_section(
            tasks
                .iter()
                .filter(|task| task.status == TaskStatus::Checkpoint)
                .cloned()
                .collect(),
            &index,
            now,
            limit,
        );
        let review = build_state_section(
            tasks
                .iter()
                .filter(|task| task.status == TaskStatus::Review)
                .cloned()
                .collect(),
            &index,
            now,
            limit,
        );
        let handoff = build_state_section(
            tasks
                .iter()
                .filter(|task| {
                    task.status == TaskStatus::Done
                        && !resolve_continuation(task, &index, now).is_empty()
                })
                .cloned()
                .collect(),
            &index,
            now,
            limit,
        );

        Ok(StateSnapshot {
            counts,
            ready,
            active,
            blocked,
            checkpoint,
            review,
            handoff,
        })
    }

    pub fn next_task(&self, id: &str) -> Result<StateTask> {
        let index = self.read_index()?;
        let task = index
            .tasks
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("task '{id}' does not exist"))?;
        Ok(build_state_task(task, &index, Utc::now()))
    }

    pub fn continuation_tasks(&self, limit: usize) -> Result<Vec<StateTask>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let index = self.read_index()?;
        let now = Utc::now();
        let mut items = index
            .tasks
            .values()
            .filter(|task| {
                matches!(task.status, TaskStatus::Checkpoint | TaskStatus::Done)
                    && !resolve_continuation(task, &index, now).is_empty()
            })
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(compare_summary_for_list);
        let mut tasks = items
            .into_iter()
            .map(|task| build_state_task(task, &index, now))
            .collect::<Vec<_>>();
        tasks.truncate(limit);
        Ok(tasks)
    }

    pub fn get_task(&self, id: &str) -> Result<TaskRecord> {
        let task_path = self.task_path(id);
        let bytes = fs::read(&task_path)
            .with_context(|| format!("failed to read task file '{}'", task_path.display()))?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse task file '{}'", task_path.display()))
    }

    pub fn task_detail(&self, id: &str) -> Result<TaskDetail> {
        let index = self.read_index()?;
        let task = self.get_task(id)?;
        let mut dependencies = Vec::new();
        let mut blocked_by = Vec::new();
        let mut missing_dependencies = Vec::new();
        for dependency_id in &task.summary.depends_on {
            match index.tasks.get(dependency_id) {
                Some(summary) => {
                    dependencies.push(summary.clone());
                    if summary.status != TaskStatus::Done {
                        blocked_by.push(summary.clone());
                    }
                }
                None => missing_dependencies.push(dependency_id.clone()),
            }
        }
        dependencies.sort_by(compare_summary_for_list);
        blocked_by.sort_by(compare_summary_for_list);
        let mut children = index
            .tasks
            .values()
            .filter(|candidate| candidate.parent.as_deref() == Some(id))
            .cloned()
            .collect::<Vec<_>>();
        children.sort_by(compare_summary_for_list);
        let now = Utc::now();

        Ok(TaskDetail {
            ready: is_ready_summary(&task.summary, &index, now),
            next: resolve_continuation(&task.summary, &index, now),
            task,
            dependencies,
            missing_dependencies,
            blocked_by,
            children,
        })
    }

    pub fn start_task(&self, id: &str, note: Option<String>) -> Result<TaskRecord> {
        let note = normalize_optional_text(note);
        self.update_task(id, TaskEventKind::Started, |task, now| {
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
            Ok("task started".to_string())
        })
    }

    pub fn checkpoint_task(&self, id: &str, update: ProgressUpdate) -> Result<TaskRecord> {
        let update = update.normalize();
        self.update_task(id, TaskEventKind::Checkpointed, |task, now| {
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
            Ok(describe_progress_message(
                "checkpoint saved",
                &task.summary.continuation,
            ))
        })
    }

    pub fn block_task(&self, id: &str, reason: String) -> Result<TaskRecord> {
        self.update_task(id, TaskEventKind::Blocked, |task, now| {
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
            Ok(format!("task blocked: {reason}"))
        })
    }

    pub fn review_task(&self, id: &str, note: Option<String>) -> Result<TaskRecord> {
        let note = normalize_optional_text(note);
        self.update_task(id, TaskEventKind::ReviewRequested, |task, now| {
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
                Ok(format!("review requested: {note}"))
            } else {
                Ok("review requested".to_string())
            }
        })
    }

    pub fn complete_task(&self, id: &str, update: ProgressUpdate) -> Result<TaskRecord> {
        let update = update.normalize();
        self.update_task(id, TaskEventKind::Completed, |task, now| {
            task.summary.status = TaskStatus::Done;
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
            Ok(describe_progress_message(
                "task completed",
                &task.summary.continuation,
            ))
        })
    }

    pub fn add_note(&self, id: &str, text: String) -> Result<TaskRecord> {
        self.update_task(id, TaskEventKind::NoteAdded, |task, now| {
            let text = normalize_required_text(text.clone(), "note")?;
            task.summary.updated_at = now;
            task.notes.push(TaskNote {
                at: now,
                text: text.clone(),
            });
            Ok(format!("note added: {text}"))
        })
    }

    pub fn add_dependency(&self, task_id: &str, dependency_id: &str) -> Result<TaskRecord> {
        let _lock = self.acquire_write_lock()?;
        let mut index = self.read_index()?;
        ensure_distinct(task_id, dependency_id, "dependency")?;
        ensure_task_exists(&index, task_id)?;
        ensure_task_exists(&index, dependency_id)?;
        if has_dependency_path(&index, dependency_id, task_id) {
            bail!(
                "cannot add dependency '{}' -> '{}' because it would create a cycle",
                task_id,
                dependency_id
            );
        }

        let mut task = self.get_task(task_id)?;
        if task
            .summary
            .depends_on
            .iter()
            .any(|value| value == dependency_id)
        {
            bail!("task '{task_id}' already depends on '{dependency_id}'");
        }

        task.summary.depends_on.push(dependency_id.to_string());
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
        ensure_task_exists(&index, task_id)?;
        let mut task = self.get_task(task_id)?;
        let original_len = task.summary.depends_on.len();
        task.summary
            .depends_on
            .retain(|value| value != dependency_id);
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

    pub fn add_subtask(&self, parent_id: &str, child_id: &str) -> Result<TaskRecord> {
        let _lock = self.acquire_write_lock()?;
        let mut index = self.read_index()?;
        ensure_distinct(parent_id, child_id, "subtask")?;
        ensure_task_exists(&index, parent_id)?;
        ensure_task_exists(&index, child_id)?;
        if has_parent_path(&index, parent_id, child_id) {
            bail!(
                "cannot add subtask '{}' under '{}' because it would create a parent cycle",
                child_id,
                parent_id
            );
        }

        let mut child = self.get_task(child_id)?;
        if child.summary.parent.as_deref() == Some(parent_id) {
            bail!("task '{child_id}' is already a subtask of '{parent_id}'");
        }
        if let Some(existing) = child.summary.parent.as_deref() {
            bail!("task '{child_id}' is already a subtask of '{existing}'; remove it first");
        }

        child.summary.parent = Some(parent_id.to_string());
        child.summary.updated_at = Utc::now();
        index
            .tasks
            .insert(child.summary.id.clone(), child.summary.clone());
        self.write_task(&child)?;
        self.write_index(&index)?;
        self.append_event(TaskEvent {
            at: child.summary.updated_at,
            task_id: child.summary.id.clone(),
            kind: TaskEventKind::SubtaskAdded,
            status: Some(child.summary.status),
            message: format!("task is now a subtask of {parent_id}"),
        })?;
        Ok(child)
    }

    pub fn remove_subtask(&self, parent_id: &str, child_id: &str) -> Result<TaskRecord> {
        let _lock = self.acquire_write_lock()?;
        let mut index = self.read_index()?;
        ensure_task_exists(&index, child_id)?;
        let mut child = self.get_task(child_id)?;
        if child.summary.parent.as_deref() != Some(parent_id) {
            bail!("task '{child_id}' is not a subtask of '{parent_id}'");
        }

        child.summary.parent = None;
        child.summary.updated_at = Utc::now();
        index
            .tasks
            .insert(child.summary.id.clone(), child.summary.clone());
        self.write_task(&child)?;
        self.write_index(&index)?;
        self.append_event(TaskEvent {
            at: child.summary.updated_at,
            task_id: child.summary.id.clone(),
            kind: TaskEventKind::SubtaskRemoved,
            status: Some(child.summary.status),
            message: format!("subtask removed from {parent_id}"),
        })?;
        Ok(child)
    }

    pub fn read_events(&self, id: Option<&str>, limit: Option<usize>) -> Result<Vec<TaskEvent>> {
        if !self.events_path().exists() {
            return Ok(Vec::new());
        }

        let file = File::open(self.events_path()).with_context(|| {
            format!(
                "failed to open events log '{}'",
                self.events_path().display()
            )
        })?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line.with_context(|| {
                format!(
                    "failed to read events log '{}'",
                    self.events_path().display()
                )
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let event: TaskEvent = serde_json::from_str(&line).with_context(|| {
                format!(
                    "failed to parse events log '{}'",
                    self.events_path().display()
                )
            })?;
            if id.is_none_or(|task_id| event.task_id == task_id) {
                events.push(event);
            }
        }
        events.sort_by(|left, right| right.at.cmp(&left.at));
        if let Some(limit) = limit {
            events.truncate(limit);
        }
        Ok(events)
    }

    fn update_task<F>(&self, id: &str, event_kind: TaskEventKind, update: F) -> Result<TaskRecord>
    where
        F: FnOnce(&mut TaskRecord, DateTime<Utc>) -> Result<String>,
    {
        let _lock = self.acquire_write_lock()?;
        let mut index = self.read_index()?;
        let mut task = self.get_task(id)?;
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

    fn read_index(&self) -> Result<StoreIndex> {
        if !self.index_path().exists() {
            return Ok(StoreIndex::default());
        }
        let bytes = fs::read(self.index_path()).with_context(|| {
            format!(
                "failed to read index file '{}'",
                self.index_path().display()
            )
        })?;
        let index: StoreIndex = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "failed to parse index file '{}'",
                self.index_path().display()
            )
        })?;
        if index.schema_version != STORE_SCHEMA_VERSION {
            bail!(
                "unsupported store schema version {} in '{}'",
                index.schema_version,
                self.index_path().display()
            );
        }
        Ok(index)
    }

    fn write_index(&self, index: &StoreIndex) -> Result<()> {
        write_json_atomic(self.index_path(), index, false)
    }

    fn write_task(&self, task: &TaskRecord) -> Result<()> {
        write_json_atomic(self.task_path(&task.summary.id), task, true)
    }

    fn append_event(&self, event: TaskEvent) -> Result<()> {
        let serialized = serde_json::to_string(&event).context("failed to serialize event")?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.events_path())
            .with_context(|| {
                format!(
                    "failed to open events log '{}'",
                    self.events_path().display()
                )
            })?;
        writeln!(file, "{serialized}").with_context(|| {
            format!(
                "failed to write events log '{}'",
                self.events_path().display()
            )
        })
    }

    fn acquire_write_lock(&self) -> Result<StoreLock> {
        self.ensure_layout()?;
        let lock_path = self.lock_path();
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| {
                format!(
                    "store is locked by another writer; remove '{}' if the previous process crashed",
                    lock_path.display()
                )
            })?;
        write!(
            file,
            "pid={}\nstarted_at={}\n",
            std::process::id(),
            Utc::now().to_rfc3339()
        )
        .context("failed to write lock metadata")?;
        Ok(StoreLock { path: lock_path })
    }

    fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.tasks_dir()).with_context(|| {
            format!(
                "failed to create task directory '{}'",
                self.tasks_dir().display()
            )
        })
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

struct StoreLock {
    path: PathBuf,
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn normalize_labels(labels: Vec<String>) -> Vec<String> {
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

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn normalize_required_text(value: String, name: &str) -> Result<String> {
    normalize_optional_text(Some(value)).ok_or_else(|| anyhow!("{name} cannot be empty"))
}

fn normalize_query(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
}

fn compare_summary_for_list(left: &TaskSummary, right: &TaskSummary) -> Ordering {
    left.status
        .sort_rank()
        .cmp(&right.status.sort_rank())
        .then_with(|| right.updated_at.cmp(&left.updated_at))
        .then_with(|| left.id.cmp(&right.id))
}

fn match_status_filter(task: &TaskSummary, filter: &ListFilter) -> bool {
    if !filter.statuses.is_empty() {
        return filter.statuses.contains(&task.status);
    }
    filter.include_done_by_default || task.status != TaskStatus::Done
}

fn task_matches_query(task: &TaskSummary, query: &str) -> bool {
    task.id.to_lowercase().contains(query)
        || task.title.to_lowercase().contains(query)
        || task
            .labels
            .iter()
            .any(|label| label.to_lowercase().contains(query))
        || continuation_matches_query(&task.continuation, query)
}

fn continuation_matches_query(continuation: &TaskContinuation, query: &str) -> bool {
    [
        continuation.next_step.as_deref(),
        continuation.next_subtask.as_deref(),
        continuation.next_task.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_lowercase().contains(query))
}

fn is_ready_summary(task: &TaskSummary, index: &StoreIndex, now: DateTime<Utc>) -> bool {
    if task.status != TaskStatus::Todo {
        return false;
    }
    if task.ready_at.is_some_and(|ready_at| ready_at > now) {
        return false;
    }
    task.depends_on.iter().all(|dependency_id| {
        index
            .tasks
            .get(dependency_id)
            .is_some_and(|dependency| dependency.status == TaskStatus::Done)
    })
}

fn child_count(index: &StoreIndex, parent_id: &str) -> usize {
    index
        .tasks
        .values()
        .filter(|task| task.parent.as_deref() == Some(parent_id))
        .count()
}

fn resolve_continuation(
    task: &TaskSummary,
    index: &StoreIndex,
    now: DateTime<Utc>,
) -> TaskContinuation {
    let mut continuation = task.continuation.clone();

    if continuation.next_subtask.is_none() {
        let mut children = index
            .tasks
            .values()
            .filter(|candidate| candidate.parent.as_deref() == Some(&task.id))
            .filter(|candidate| is_ready_summary(candidate, index, now))
            .cloned()
            .collect::<Vec<_>>();
        children.sort_by(compare_summary_for_list);
        if let Some(child) = children.first() {
            continuation.next_subtask = Some(child.id.clone());
        }
    }

    if continuation.next_task.is_none() {
        let mut ready_top_level = index
            .tasks
            .values()
            .filter(|candidate| candidate.id != task.id)
            .filter(|candidate| candidate.parent.is_none())
            .filter(|candidate| is_ready_summary(candidate, index, now))
            .cloned()
            .collect::<Vec<_>>();
        ready_top_level.sort_by(compare_summary_for_list);
        if let Some(next_task) = ready_top_level.first() {
            continuation.next_task = Some(next_task.id.clone());
        }
    }

    continuation
}

fn build_counts(tasks: &[TaskSummary], index: &StoreIndex, now: DateTime<Utc>) -> StateCounts {
    let mut counts = StateCounts::default();
    for task in tasks {
        match task.status {
            TaskStatus::Todo => counts.todo += 1,
            TaskStatus::Active => counts.active += 1,
            TaskStatus::Checkpoint => counts.checkpoint += 1,
            TaskStatus::Blocked => counts.blocked += 1,
            TaskStatus::Review => counts.review += 1,
            TaskStatus::Done => counts.done += 1,
        }
        if is_ready_summary(task, index, now) {
            counts.ready += 1;
        }
        if task.status == TaskStatus::Done && !resolve_continuation(task, index, now).is_empty() {
            counts.handoff += 1;
        }
    }
    counts
}

fn build_state_section(
    mut tasks: Vec<TaskSummary>,
    index: &StoreIndex,
    now: DateTime<Utc>,
    limit: usize,
) -> Vec<StateTask> {
    tasks.sort_by(compare_summary_for_list);
    tasks
        .into_iter()
        .take(limit)
        .map(|task| build_state_task(task, index, now))
        .collect()
}

fn build_state_task(task: TaskSummary, index: &StoreIndex, now: DateTime<Utc>) -> StateTask {
    StateTask {
        ready: is_ready_summary(&task, index, now),
        dependency_count: task.depends_on.len(),
        child_count: child_count(index, &task.id),
        next: resolve_continuation(&task, index, now),
        task,
    }
}

fn describe_progress_message(prefix: &str, continuation: &TaskContinuation) -> String {
    if continuation.is_empty() {
        return prefix.to_string();
    }

    let mut parts = Vec::new();
    if let Some(step) = continuation.next_step.as_deref() {
        parts.push(format!("step={step}"));
    }
    if let Some(subtask) = continuation.next_subtask.as_deref() {
        parts.push(format!("subtask={subtask}"));
    }
    if let Some(task) = continuation.next_task.as_deref() {
        parts.push(format!("task={task}"));
    }
    format!("{prefix} ({})", parts.join(", "))
}

fn ensure_task_exists(index: &StoreIndex, task_id: &str) -> Result<()> {
    if index.tasks.contains_key(task_id) {
        return Ok(());
    }
    bail!("task '{task_id}' does not exist")
}

fn ensure_distinct(left: &str, right: &str, relation_name: &str) -> Result<()> {
    if left == right {
        bail!("cannot link task '{left}' to itself as a {relation_name}");
    }
    Ok(())
}

fn has_dependency_path(index: &StoreIndex, start: &str, target: &str) -> bool {
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

fn has_parent_path(index: &StoreIndex, start_parent: &str, target_child: &str) -> bool {
    let mut current = Some(start_parent);
    while let Some(task_id) = current {
        if task_id == target_child {
            return true;
        }
        current = index
            .tasks
            .get(task_id)
            .and_then(|task| task.parent.as_deref());
    }
    false
}

fn write_json_atomic(path: PathBuf, value: &impl serde::Serialize, pretty: bool) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("path '{}' has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create directory '{}'", parent.display()))?;
    let bytes = if pretty {
        serde_json::to_vec_pretty(value)
    } else {
        serde_json::to_vec(value)
    }
    .context("failed to serialize JSON payload")?;
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, bytes)
        .with_context(|| format!("failed to write temp file '{}'", temp_path.display()))?;
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("failed to replace file '{}'", path.display()))?;
    }
    fs::rename(&temp_path, &path).with_context(|| {
        format!(
            "failed to move temp file '{}' into '{}'",
            temp_path.display(),
            path.display()
        )
    })
}

pub fn slugify(input: &str) -> String {
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

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
                    labels: vec![],
                })
                .unwrap();
        }

        store.add_dependency("two", "one").unwrap();
        let error = store.add_dependency("one", "two").unwrap_err().to_string();
        assert!(error.contains("would create a cycle"));
    }
}
