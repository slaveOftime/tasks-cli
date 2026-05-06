use std::collections::BTreeMap;

use anyhow::Result;
use chrono::Utc;

use crate::model::{
    ReadyTask, StateSnapshot, StateTask, StoreIndex, TaskContinuation, TaskDetail, TaskEvent,
    TaskStatus, TaskSummary,
};

use super::{
    ListFilter, TaskStore,
    helpers::{
        compare_summary_for_list, is_pending_dependency_summary, is_ready_summary, normalize_query,
        task_matches_query,
    },
};

struct QueryContext<'a> {
    index: &'a StoreIndex,
    now: chrono::DateTime<Utc>,
    children_by_parent: BTreeMap<String, Vec<TaskSummary>>,
    ready_top_level: Vec<TaskSummary>,
}

impl<'a> QueryContext<'a> {
    fn new(index: &'a StoreIndex, now: chrono::DateTime<Utc>) -> Self {
        let mut children_by_parent: BTreeMap<String, Vec<TaskSummary>> = BTreeMap::new();
        let mut ready_top_level = Vec::new();
        for task in index.tasks.values() {
            if let Some(parent) = task.parent.as_ref() {
                children_by_parent
                    .entry(parent.clone())
                    .or_default()
                    .push(task.clone());
            } else if is_ready_summary(task, index, now) {
                ready_top_level.push(task.clone());
            }
        }
        for children in children_by_parent.values_mut() {
            children.sort_by(compare_summary_for_list);
        }
        ready_top_level.sort_by(compare_summary_for_list);
        Self {
            index,
            now,
            children_by_parent,
            ready_top_level,
        }
    }

    fn child_count(&self, parent_id: &str) -> usize {
        self.children_by_parent.get(parent_id).map_or(0, Vec::len)
    }

    fn resolve_continuation(&self, task: &TaskSummary) -> TaskContinuation {
        let mut continuation = task.continuation.clone();

        if continuation.next_subtask.is_none()
            && let Some(child) = self
                .children_by_parent
                .get(&task.id)
                .into_iter()
                .flatten()
                .find(|candidate| is_ready_summary(candidate, self.index, self.now))
        {
            continuation.next_subtask = Some(child.id.clone());
        }

        if continuation.next_task.is_none() {
            let mut ready_dependencies = task
                .depends_on
                .iter()
                .filter_map(|dependency_id| self.index.tasks.get(dependency_id))
                .filter(|dependency| is_ready_summary(dependency, self.index, self.now))
                .cloned()
                .collect::<Vec<_>>();
            ready_dependencies.sort_by(compare_summary_for_list);

            if let Some(dependency) = ready_dependencies.first() {
                continuation.next_task = Some(dependency.id.clone());
            } else if let Some(next_task) = self
                .ready_top_level
                .iter()
                .find(|candidate| candidate.id != task.id)
            {
                continuation.next_task = Some(next_task.id.clone());
            }
        }

        continuation
    }

    fn build_state_task(&self, task: TaskSummary) -> StateTask {
        StateTask {
            ready: is_ready_summary(&task, self.index, self.now),
            dependency_count: task.depends_on.len(),
            child_count: self.child_count(&task.id),
            next: self.resolve_continuation(&task),
            task,
        }
    }

    fn build_state_section(&self, mut tasks: Vec<TaskSummary>, limit: usize) -> Vec<StateTask> {
        tasks.sort_by(compare_summary_for_list);
        tasks
            .into_iter()
            .take(limit)
            .map(|task| self.build_state_task(task))
            .collect()
    }

    fn build_counts(&self, tasks: &[TaskSummary]) -> crate::model::StateCounts {
        let mut counts = crate::model::StateCounts::default();
        for task in tasks {
            match task.status {
                TaskStatus::Todo => counts.todo += 1,
                TaskStatus::Active => counts.active += 1,
                TaskStatus::Checkpoint => counts.checkpoint += 1,
                TaskStatus::Blocked => counts.blocked += 1,
                TaskStatus::Review => counts.review += 1,
                TaskStatus::Done => counts.done += 1,
            }
            if is_ready_summary(task, self.index, self.now) {
                counts.ready += 1;
            }
            if is_pending_dependency_summary(task, self.index, self.now) {
                counts.pending_dependencies += 1;
            }
            if task.status == TaskStatus::Done && !self.resolve_continuation(task).is_empty() {
                counts.handoff += 1;
            }
        }
        counts
    }
}

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
        let context = QueryContext::new(&index, now);
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
                child_count: context.child_count(&task.id),
                next: context.resolve_continuation(&task),
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
        let context = QueryContext::new(&index, now);
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

        let counts = context.build_counts(&tasks);
        let ready = context.build_state_section(
            tasks
                .iter()
                .filter(|task| is_ready_summary(task, &index, now))
                .cloned()
                .collect(),
            limit,
        );
        let pending_dependencies = context.build_state_section(
            tasks
                .iter()
                .filter(|task| is_pending_dependency_summary(task, &index, now))
                .cloned()
                .collect(),
            limit,
        );
        let active = context.build_state_section(
            tasks
                .iter()
                .filter(|task| task.status == TaskStatus::Active)
                .cloned()
                .collect(),
            limit,
        );
        let blocked = context.build_state_section(
            tasks
                .iter()
                .filter(|task| task.status == TaskStatus::Blocked)
                .cloned()
                .collect(),
            limit,
        );
        let checkpoint = context.build_state_section(
            tasks
                .iter()
                .filter(|task| task.status == TaskStatus::Checkpoint)
                .cloned()
                .collect(),
            limit,
        );
        let review = context.build_state_section(
            tasks
                .iter()
                .filter(|task| task.status == TaskStatus::Review)
                .cloned()
                .collect(),
            limit,
        );
        let handoff = context.build_state_section(
            tasks
                .iter()
                .filter(|task| {
                    task.status == TaskStatus::Done
                        && !context.resolve_continuation(task).is_empty()
                })
                .cloned()
                .collect(),
            limit,
        );

        Ok(StateSnapshot {
            counts,
            ready,
            pending_dependencies,
            active,
            blocked,
            checkpoint,
            review,
            handoff,
        })
    }

    pub fn next_task(&self, id: &str) -> Result<StateTask> {
        let index = self.read_index()?;
        let resolved_id = self.resolve_task_reference_in_index(&index, id)?;
        let task = index
            .tasks
            .get(&resolved_id)
            .cloned()
            .expect("resolved task must exist");
        let context = QueryContext::new(&index, Utc::now());
        Ok(context.build_state_task(task))
    }

    pub fn continuation_tasks(&self, limit: usize) -> Result<Vec<StateTask>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let index = self.read_index()?;
        let now = Utc::now();
        let context = QueryContext::new(&index, now);
        let mut items = index
            .tasks
            .values()
            .filter(|task| {
                matches!(task.status, TaskStatus::Checkpoint | TaskStatus::Done)
                    && !context.resolve_continuation(task).is_empty()
            })
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(compare_summary_for_list);
        let mut tasks = items
            .into_iter()
            .map(|task| context.build_state_task(task))
            .collect::<Vec<_>>();
        tasks.truncate(limit);
        Ok(tasks)
    }

    pub fn task_detail(&self, id: &str) -> Result<TaskDetail> {
        let index = self.read_index()?;
        let context = QueryContext::new(&index, Utc::now());
        let resolved_id = self.resolve_task_reference_in_index(&index, id)?;
        let task = self.read_task_by_id(&resolved_id)?;
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
            .filter(|candidate| candidate.parent.as_deref() == Some(task.summary.id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        children.sort_by(compare_summary_for_list);

        Ok(TaskDetail {
            ready: is_ready_summary(&task.summary, &index, context.now),
            next: context.resolve_continuation(&task.summary),
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

        let resolved_id = match id {
            Some(task_id) => Some(self.resolve_task_reference(task_id)?),
            None => None,
        };

        let mut events = if let Some(task_id) = resolved_id.as_deref() {
            self.ensure_task_event_log(task_id)?;
            self.read_events_file(self.task_events_path(task_id))?
        } else {
            self.read_events_file(self.events_path())?
        };
        events.sort_by(|left, right| right.at.cmp(&left.at));
        if let Some(limit) = limit {
            events.truncate(limit);
        }
        Ok(events)
    }
}
