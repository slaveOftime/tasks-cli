use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;

use crate::model::{STORE_SCHEMA_VERSION, StoreIndex, TaskEvent, TaskRecord};

use super::TaskStore;

impl TaskStore {
    pub(super) fn read_index(&self) -> Result<StoreIndex> {
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

    pub(super) fn write_index(&self, index: &StoreIndex) -> Result<()> {
        write_json_atomic(self.index_path(), index, false)
    }

    pub(super) fn write_task(&self, task: &TaskRecord) -> Result<()> {
        write_json_atomic(self.task_path(&task.summary.id), task, true)
    }

    pub(super) fn append_event(&self, event: TaskEvent) -> Result<()> {
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

    pub(super) fn acquire_write_lock(&self) -> Result<StoreLock> {
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
}

pub(super) struct StoreLock {
    path: PathBuf,
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
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
