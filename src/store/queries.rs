use std::fs::{self, File};
use std::io::{BufRead, BufReader};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;

use crate::model::{
    ReadyTask, StateSnapshot, StateTask, TaskDetail, TaskEvent, TaskRecord, TaskStatus,
};

use super::{
    ListFilter, TaskStore,
    helpers::{
        build_counts, build_state_section, build_state_task, child_count, compare_summary_for_list,
        is_ready_summary, normalize_query, resolve_continuation, task_matches_query,
    },
};

impl TaskStore {
    pub fn list_tasks(&self, filter: &ListFilter) -> Result<Vec<crate::model::TaskSummary>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let index = self.read_index()?;
        let now = Utc::now();
        let query = normalize_query(filter.query.as_deref());
        let mut items = index.tasks.values().cloned().collect::<Vec<_>>();
        items.retain(|task| super::helpers::match_status_filter(task, filter));
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
}
