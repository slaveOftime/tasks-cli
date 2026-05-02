use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::model::TaskStatus;

#[derive(Debug, Parser)]
#[command(
    name = "tli",
    version,
    about = "Fast file-backed task tracker for humans and agents"
)]
pub struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    pub root: Option<PathBuf>,
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Skill,
    Add(AddArgs),
    #[command(alias = "ls")]
    List(ListArgs),
    Ready(ReadyArgs),
    State(StateArgs),
    Next(NextArgs),
    #[command(alias = "get")]
    Show(TaskIdArgs),
    Start(StatusNoteArgs),
    Checkpoint(ProgressArgs),
    Block(BlockArgs),
    Review(StatusNoteArgs),
    Done(ProgressArgs),
    Note(NoteArgs),
    Dep(RelationArgs),
    Subtask(SubtaskArgs),
    #[command(alias = "history")]
    Log(LogArgs),
}

#[derive(Debug, Args)]
pub struct AddArgs {
    pub title: String,
    #[arg(long)]
    pub id: Option<String>,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long, value_name = "RFC3339")]
    pub ready_at: Option<String>,
    #[arg(long = "label", short = 'l', value_name = "LABEL")]
    pub labels: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(long, value_enum)]
    pub status: Vec<TaskStatus>,
    #[arg(long)]
    pub all: bool,
    #[arg(long)]
    pub ready: bool,
    #[arg(long)]
    pub query: Option<String>,
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(Debug, Args)]
pub struct ReadyArgs {
    #[arg(long)]
    pub query: Option<String>,
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(Debug, Args)]
pub struct StateArgs {
    #[arg(long)]
    pub query: Option<String>,
    #[arg(long, default_value_t = 5)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct NextArgs {
    pub id: Option<String>,
    #[arg(long, default_value_t = 8)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct TaskIdArgs {
    pub id: String,
}

#[derive(Debug, Args)]
pub struct StatusNoteArgs {
    pub id: String,
    #[arg(long)]
    pub note: Option<String>,
}

#[derive(Debug, Args)]
pub struct ProgressArgs {
    pub id: String,
    #[arg(long)]
    pub note: Option<String>,
    #[arg(long = "next-step")]
    pub next_step: Option<String>,
    #[arg(long = "next-subtask")]
    pub next_subtask: Option<String>,
    #[arg(long = "next-task")]
    pub next_task: Option<String>,
}

#[derive(Debug, Args)]
pub struct BlockArgs {
    pub id: String,
    #[arg(long)]
    pub reason: String,
}

#[derive(Debug, Args)]
pub struct NoteArgs {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Args)]
pub struct LogArgs {
    pub id: Option<String>,
    #[arg(long)]
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
    pub task: String,
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
    pub parent: String,
    pub child: String,
}
