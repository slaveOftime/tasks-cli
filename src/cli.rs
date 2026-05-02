use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::model::TaskStatus;

#[derive(Debug, Parser)]
#[command(
    name = "tli",
    version,
    about = "Fast file-backed task tracker for humans and agents",
    long_about = "Track work in a repo-local .tli store. The default output is optimized for people scanning the terminal, while --json keeps the same commands scriptable for hooks and agents.",
    after_help = "Examples:\n  tli state\n  tli add \"Ship first slice\" --summary \"Wire the CLI flow\" --label rust\n  tli show <task-id>\n  tli checkpoint <task-id> --next-step \"Resume API wiring\"\n  tli --json state --limit 6"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Use this .tli directory directly instead of searching upward from the current directory"
    )]
    pub root: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Emit machine-readable JSON instead of human-friendly terminal output"
    )]
    pub json: bool,
    #[arg(
        long,
        short = 'v',
        global = true,
        help = "Show extra timestamps and detail in human-readable output"
    )]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Print the embedded usage guide for humans and agents")]
    Skill,
    #[command(about = "Create a new task in the current store")]
    Add(AddArgs),
    #[command(about = "Set, replace, or clear a recurring schedule for a task")]
    Schedule(ScheduleArgs),
    #[command(alias = "ls", about = "List tasks with optional filters")]
    List(ListArgs),
    #[command(about = "List only tasks that are actionable right now")]
    Ready(ReadyArgs),
    #[command(about = "Show a high-level snapshot of ready, active, blocked, and handoff work")]
    State(StateArgs),
    #[command(about = "Show continuation hints for one task or all paused/handoff tasks")]
    Next(NextArgs),
    #[command(alias = "get", about = "Inspect one task in compact or verbose detail")]
    Show(TaskIdArgs),
    #[command(about = "Move a task into active work")]
    Start(StatusNoteArgs),
    #[command(about = "Save a checkpoint and optional continuation hints")]
    Checkpoint(ProgressArgs),
    #[command(about = "Mark a task blocked with a required reason")]
    Block(BlockArgs),
    #[command(about = "Mark a task as ready for review")]
    Review(StatusNoteArgs),
    #[command(about = "Complete a task or finish one cycle of a scheduled task")]
    Done(ProgressArgs),
    #[command(about = "Attach a note to a task without changing its status")]
    Note(NoteArgs),
    #[command(about = "Manage dependency links between tasks")]
    Dep(RelationArgs),
    #[command(about = "Manage parent/child task relationships")]
    Subtask(SubtaskArgs),
    #[command(
        alias = "history",
        about = "Read the event log for one task or the whole store"
    )]
    Log(LogArgs),
}

#[derive(Debug, Args)]
pub struct AddArgs {
    /// Short human-readable task title.
    pub title: String,
    #[arg(
        long,
        help = "Explicit task id to use instead of generating one from the title"
    )]
    pub id: Option<String>,
    #[arg(long, help = "Longer summary shown in show output and JSON detail")]
    pub summary: Option<String>,
    #[arg(
        long,
        value_name = "RFC3339",
        help = "Delay when the task becomes ready; must be an RFC3339 timestamp"
    )]
    pub ready_at: Option<String>,
    #[arg(
        long,
        value_name = "MINUTES",
        help = "Recurring interval in minutes; cannot be combined with --cron"
    )]
    pub every_minutes: Option<u32>,
    #[arg(
        long,
        value_name = "CRON",
        help = "Recurring cron schedule using a standard 5-field expression"
    )]
    pub cron: Option<String>,
    #[arg(
        long = "label",
        short = 'l',
        value_name = "LABEL",
        help = "Label to attach. Repeat the flag or pass a comma-separated list."
    )]
    pub labels: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ScheduleArgs {
    /// Task id to update.
    pub id: String,
    #[arg(
        long,
        value_name = "MINUTES",
        help = "Recurring interval in minutes; cannot be combined with --cron"
    )]
    pub every_minutes: Option<u32>,
    #[arg(
        long,
        value_name = "CRON",
        help = "Recurring cron schedule using a standard 5-field expression"
    )]
    pub cron: Option<String>,
    #[arg(
        long,
        value_name = "RFC3339",
        help = "Override the next ready time; must be an RFC3339 timestamp"
    )]
    pub ready_at: Option<String>,
    #[arg(
        long,
        help = "Remove the current schedule instead of setting a new one"
    )]
    pub clear: bool,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(long, value_enum, help = "Only show the selected status values")]
    pub status: Vec<TaskStatus>,
    #[arg(
        long,
        help = "Include done tasks when no explicit --status filter is given"
    )]
    pub all: bool,
    #[arg(long, help = "Only show tasks that are ready right now")]
    pub ready: bool,
    #[arg(
        long,
        help = "Filter by id, title, labels, schedule text, or continuation hints"
    )]
    pub query: Option<String>,
    #[arg(long, help = "Maximum number of tasks to show")]
    pub limit: Option<usize>,
}

#[derive(Debug, Args)]
pub struct ReadyArgs {
    #[arg(
        long,
        help = "Filter by id, title, labels, schedule text, or continuation hints"
    )]
    pub query: Option<String>,
    #[arg(long, help = "Maximum number of ready tasks to show")]
    pub limit: Option<usize>,
}

#[derive(Debug, Args)]
pub struct StateArgs {
    #[arg(
        long,
        help = "Filter by id, title, labels, schedule text, or continuation hints"
    )]
    pub query: Option<String>,
    #[arg(
        long,
        default_value_t = 5,
        help = "Maximum number of tasks to show per section"
    )]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct NextArgs {
    /// Optional task id. Omit it to list continuation hints across the store.
    pub id: Option<String>,
    #[arg(
        long,
        default_value_t = 8,
        help = "Maximum number of continuation entries to show"
    )]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct TaskIdArgs {
    /// Task id to inspect.
    pub id: String,
}

#[derive(Debug, Args)]
pub struct StatusNoteArgs {
    /// Task id to update.
    pub id: String,
    #[arg(long, help = "Optional note to append while changing the task status")]
    pub note: Option<String>,
}

#[derive(Debug, Args)]
pub struct ProgressArgs {
    /// Task id to update.
    pub id: String,
    #[arg(long, help = "Optional note to append while updating progress")]
    pub note: Option<String>,
    #[arg(
        long = "next-step",
        help = "The next action to take inside this same task"
    )]
    pub next_step: Option<String>,
    #[arg(
        long = "next-subtask",
        help = "The child task that should be picked up next"
    )]
    pub next_subtask: Option<String>,
    #[arg(
        long = "next-task",
        help = "The follow-up or sibling task to work on next"
    )]
    pub next_task: Option<String>,
}

#[derive(Debug, Args)]
pub struct BlockArgs {
    /// Task id to block.
    pub id: String,
    #[arg(long, help = "Why the task is blocked")]
    pub reason: String,
}

#[derive(Debug, Args)]
pub struct NoteArgs {
    /// Task id to update.
    pub id: String,
    /// Note text to append.
    pub text: String,
}

#[derive(Debug, Args)]
pub struct LogArgs {
    /// Optional task id. Omit it to read the whole store event log.
    pub id: Option<String>,
    #[arg(long, help = "Maximum number of events to show")]
    pub limit: Option<usize>,
}

#[derive(Debug, Args)]
pub struct RelationArgs {
    #[command(subcommand)]
    pub command: RelationCommand,
}

#[derive(Debug, Subcommand)]
pub enum RelationCommand {
    Add(DependencyArgs),
    #[command(alias = "rm")]
    Remove(DependencyArgs),
}

#[derive(Debug, Args)]
pub struct DependencyArgs {
    /// Task that depends on the other task.
    pub task: String,
    /// Task that must be completed first.
    pub dependency: String,
}

#[derive(Debug, Args)]
pub struct SubtaskArgs {
    #[command(subcommand)]
    pub command: SubtaskCommand,
}

#[derive(Debug, Subcommand)]
pub enum SubtaskCommand {
    Add(SubtaskLinkArgs),
    #[command(alias = "rm")]
    Remove(SubtaskLinkArgs),
}

#[derive(Debug, Args)]
pub struct SubtaskLinkArgs {
    /// Parent task id.
    pub parent: String,
    /// Child task id.
    pub child: String,
}
