use std::ffi::OsString;

use anyhow::{Result, bail};
use clap::Parser;

use crate::cli::{
    AddArgs, Cli, Command, DependencyArgs, DoneArgs, ListArgs, LogArgs, NextArgs, ProgressArgs,
    ReadyArgs, RelationArgs, RelationCommand, ScheduleArgs, StateArgs, StatusNoteArgs,
};
use crate::model::TaskSchedule;
use crate::output::{
    print_json, print_task_result, render_events, render_next_task, render_ready_list,
    render_state, render_task_detail, render_task_list,
};
use crate::root::{parse_timestamp, resolve_root};
use crate::server::{ServerOptions, start_server};
use crate::service::TaskService;
use crate::store::{AddTaskInput, ListFilter, ProgressUpdate, ScheduleUpdate, TaskStore};

const SKILL_DOC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/skills/tli/SKILL.md"));

pub fn run<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::parse_from(args);

    match cli.command {
        Command::Skill => handle_skill(cli.json),
        command => {
            let root = resolve_root(cli.root)?;
            let store = TaskStore::new(root);
            match command {
                Command::Add(args) => handle_add(&store, args, cli.json, cli.verbose),
                Command::Schedule(args) => handle_schedule(&store, args, cli.json, cli.verbose),
                Command::List(args) => handle_list(&store, args, cli.json, cli.verbose),
                Command::Ready(args) => handle_ready(&store, args, cli.json, cli.verbose),
                Command::State(args) => handle_state(&store, args, cli.json, cli.verbose),
                Command::Next(args) => handle_next(&store, args, cli.json, cli.verbose),
                Command::Show(args) => handle_show(&store, &args.id, cli.json, cli.verbose),
                Command::Start(args) => handle_start(&store, args, cli.json, cli.verbose),
                Command::Checkpoint(args) => handle_checkpoint(&store, args, cli.json, cli.verbose),
                Command::Block(args) => {
                    let task = store.block_task(&args.id, args.reason)?;
                    print_task_result(&task, cli.json, cli.verbose, "blocked")
                }
                Command::Review(args) => handle_review(&store, args, cli.json, cli.verbose),
                Command::Done(args) => handle_done(&store, args, cli.json, cli.verbose),
                Command::Note(args) => {
                    let task = store.add_note(&args.id, args.text)?;
                    print_task_result(&task, cli.json, cli.verbose, "updated")
                }
                Command::Dep(args) => handle_dependency(&store, args, cli.json, cli.verbose),
                Command::Log(args) => handle_log(&store, args, cli.json, cli.verbose),
                Command::Server(args) => match args.command {
                    crate::cli::ServerCommand::Start(start) => {
                        start_server(TaskService::new(store), ServerOptions { port: start.port })
                    }
                },
                Command::Skill => unreachable!("handled above"),
            }
        }
    }
}

fn handle_skill(json: bool) -> Result<()> {
    if json {
        return print_json(&serde_json::json!({
            "path": "skills/tli/SKILL.md",
            "content": SKILL_DOC,
        }));
    }
    println!("{SKILL_DOC}");
    Ok(())
}

fn handle_add(store: &TaskStore, args: AddArgs, json: bool, verbose: bool) -> Result<()> {
    let schedule = schedule_from_args(args.every_minutes, args.cron.as_deref())?;
    let ready_at = match args.ready_at {
        Some(value) => Some(parse_timestamp(&value)?),
        None => None,
    };
    let task = store.add_task(AddTaskInput {
        id: args.id,
        title: args.title,
        summary_text: args.summary,
        ready_at,
        schedule,
        labels: args.labels,
    })?;
    print_task_result(&task, json, verbose, "created")
}

fn handle_schedule(store: &TaskStore, args: ScheduleArgs, json: bool, verbose: bool) -> Result<()> {
    let schedule = schedule_from_args(args.every_minutes, args.cron.as_deref())?;
    let ready_at = match args.ready_at {
        Some(value) => Some(parse_timestamp(&value)?),
        None => None,
    };
    let task = store.configure_schedule(
        &args.id,
        ScheduleUpdate {
            schedule,
            ready_at,
            clear: args.clear,
        },
    )?;
    let verb = if args.clear {
        "cleared schedule"
    } else {
        "scheduled"
    };
    print_task_result(&task, json, verbose, verb)
}

fn handle_list(store: &TaskStore, args: ListArgs, json: bool, verbose: bool) -> Result<()> {
    let items = store.list_tasks(&ListFilter {
        statuses: args.status,
        include_done_by_default: args.all,
        ready_only: args.ready,
        labels: args.labels,
        query: args.query,
        limit: args.limit,
    })?;
    if json {
        return print_json(&items);
    }
    if items.is_empty() {
        println!("No matching tasks in {}", store.root().display());
        return Ok(());
    }

    println!("{}", render_task_list(&items, verbose, store.root()));
    Ok(())
}

fn handle_ready(store: &TaskStore, args: ReadyArgs, json: bool, verbose: bool) -> Result<()> {
    let items = store.ready_tasks(args.query, args.limit)?;
    if json {
        return print_json(&items);
    }
    if items.is_empty() {
        println!("No ready tasks in {}", store.root().display());
        return Ok(());
    }

    println!("{}", render_ready_list(&items, verbose, store.root()));
    Ok(())
}

fn handle_state(store: &TaskStore, args: StateArgs, json: bool, verbose: bool) -> Result<()> {
    let snapshot = store.state_snapshot(args.query, args.limit)?;
    if json {
        return print_json(&snapshot);
    }

    println!("{}", render_state(&snapshot, verbose, store.root()));
    Ok(())
}

fn handle_next(store: &TaskStore, args: NextArgs, json: bool, verbose: bool) -> Result<()> {
    if let Some(id) = args.id.as_deref() {
        let task = store.next_task(id)?;
        if json {
            return print_json(&task);
        }
        if task.next.is_empty() {
            println!("No continuation hints for {}", task.task.id);
            return Ok(());
        }
        println!("{}", render_next_task(&task, verbose));
        return Ok(());
    }

    let items = store.continuation_tasks(args.limit)?;
    if json {
        return print_json(&items);
    }
    if items.is_empty() {
        println!(
            "No checkpoint or handoff continuations in {}",
            store.root().display()
        );
        return Ok(());
    }
    for (index, task) in items.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!("{}", render_next_task(task, verbose));
    }
    Ok(())
}

fn handle_show(store: &TaskStore, id: &str, json: bool, verbose: bool) -> Result<()> {
    let detail = store.task_detail(id)?;
    if json {
        return print_json(&detail);
    }
    println!("{}", render_task_detail(&detail, verbose));
    Ok(())
}

fn handle_start(store: &TaskStore, args: StatusNoteArgs, json: bool, verbose: bool) -> Result<()> {
    let task = store.start_task(&args.id, args.note)?;
    print_task_result(&task, json, verbose, "started")
}

fn handle_checkpoint(
    store: &TaskStore,
    args: ProgressArgs,
    json: bool,
    verbose: bool,
) -> Result<()> {
    let id = args.id.clone();
    let task = store.checkpoint_task(&id, progress_update(args))?;
    print_task_result(&task, json, verbose, "checkpointed")
}

fn handle_review(store: &TaskStore, args: StatusNoteArgs, json: bool, verbose: bool) -> Result<()> {
    let task = store.review_task(&args.id, args.note)?;
    print_task_result(&task, json, verbose, "ready for review")
}

fn handle_done(store: &TaskStore, args: DoneArgs, json: bool, verbose: bool) -> Result<()> {
    let id = args.id.clone();
    let task = store.complete_task(&id, done_update(args))?;
    print_task_result(&task, json, verbose, "done")
}

fn handle_dependency(
    store: &TaskStore,
    args: RelationArgs,
    json: bool,
    verbose: bool,
) -> Result<()> {
    match args.command {
        RelationCommand::Add(DependencyArgs { task, dependency }) => {
            let task = store.resolve_task_reference(&task)?;
            let dependency = store.resolve_task_reference(&dependency)?;
            let updated = store.add_dependency(&task, &dependency)?;
            print_link_result(
                store,
                &updated.summary.id,
                json,
                verbose,
                &format!("Linked {} -> {}", updated.summary.id, dependency),
            )
        }
        RelationCommand::Remove(DependencyArgs { task, dependency }) => {
            let task = store.resolve_task_reference(&task)?;
            let dependency = store.resolve_task_reference(&dependency)?;
            let updated = store.remove_dependency(&task, &dependency)?;
            print_link_result(
                store,
                &updated.summary.id,
                json,
                verbose,
                &format!(
                    "Removed dependency {} -> {}",
                    updated.summary.id, dependency
                ),
            )
        }
    }
}

fn handle_log(store: &TaskStore, args: LogArgs, json: bool, verbose: bool) -> Result<()> {
    let events = store.read_events(args.id.as_deref(), args.limit)?;
    if json {
        return print_json(&events);
    }
    if events.is_empty() {
        println!("No matching events in {}", store.root().display());
        return Ok(());
    }
    println!("{}", render_events(&events, verbose, store.root()));
    Ok(())
}

fn progress_update(args: ProgressArgs) -> ProgressUpdate {
    ProgressUpdate {
        note: args.note,
        next_step: args.next_step,
        next_task: args.next_task,
        clear_schedule: false,
    }
}

fn done_update(args: DoneArgs) -> ProgressUpdate {
    ProgressUpdate {
        note: args.note,
        next_step: args.next_step,
        next_task: args.next_task,
        clear_schedule: args.clear_schedule,
    }
}

fn print_link_result(
    store: &TaskStore,
    id: &str,
    json: bool,
    verbose: bool,
    message: &str,
) -> Result<()> {
    if json {
        return print_json(&store.task_detail(id)?);
    }
    if verbose {
        println!("{message}");
        println!();
        println!("{}", render_task_detail(&store.task_detail(id)?, true));
    } else {
        println!("{message}");
    }
    Ok(())
}

fn schedule_from_args(
    every_minutes: Option<u32>,
    cron: Option<&str>,
) -> Result<Option<TaskSchedule>> {
    match (
        every_minutes,
        cron.map(str::trim).filter(|value| !value.is_empty()),
    ) {
        (Some(_), Some(_)) => bail!("--every-minutes cannot be combined with --cron"),
        (Some(every_minutes), None) => Ok(Some(TaskSchedule::Interval { every_minutes })),
        (None, Some(expression)) => Ok(Some(TaskSchedule::Cron {
            expression: expression.to_string(),
        })),
        (None, None) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_from_args_requires_one_mode() {
        assert!(schedule_from_args(Some(10), Some("0 7 * * *")).is_err());
        assert!(matches!(
            schedule_from_args(Some(10), None).unwrap(),
            Some(TaskSchedule::Interval { every_minutes: 10 })
        ));
    }
}
