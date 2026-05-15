# tli task CLI skill

Use `tli` to keep repo-local task state in `.tli/`. It is compact by default for agents and hooks, but still comfortable for humans through predictable verbs and optional verbose output.

## Quick start

```powershell
tli add "Implement parser cache" --summary "Cache parsed plans by workspace" --label rust --label perf
tli add "Nightly cleanup" --cron "0 22 * * *"
tli state
tli ready
tli show <task-id>
tli schedule <task-id> --every-minutes 1440 --ready-at "12:20:10"
tli start <task-id> --note "Picked up after triage"
tli checkpoint <task-id> --note "Pause here" --next-step "Resume benchmark wiring"
tli done <task-id> --note "Merged" --next-task follow-up-task-id
tli server start --port 3030
```

## Output modes

- Default output is compact and safe for prompts.
- Use `--verbose` for richer human-readable inspection.
- Use `--json` for hooks, scripts, or agents that need structured data.

```powershell
tli state
tli --verbose state
tli --json state --limit 6
tli show <task-id>
tli --verbose show <task-id>
tli --json show <task-id>
```

## Recommended agent hook flow

Use `state` first because it reads the summary index and returns cheap counts plus short actionable lines:

```powershell
tli --json state --limit 6
```

That cheap state payload keeps three different situations distinct for agents:

- `ready`: fully actionable now
- `pending_dependencies`: due now but still blocked by unfinished dependencies
- `active`: already in flight

Then drill down only when needed:

```powershell
tli --json show <task-id>
tli --json next <task-id>
tli --json log <task-id> --limit 20
```

For a human at the terminal, the same flow works without JSON:

```powershell
tli state
tli next
tli show <task-id> --verbose
```

Aggregate `tli next` resolves completed handoffs through explicit `next_task` hints and the task graph so the default list points at unfinished follow-up work instead of completed wrapper tasks. Use `tli next <task-id>` when you need to inspect the stored handoff on a specific completed task.

Most commands that take a task id also accept a case-insensitive partial id match, so humans usually do not need to type the full stored id:

```powershell
tli show daily-news
tli done nightly
tli dep add parser-cache benchmark
```

If a partial match finds more than one task, `tli` fails safely and shows the matching ids.

## Core commands

| Need | Command |
| --- | --- |
| Create a task | `tli add "Title" [--id id] [--summary text] [--ready-at time] [--cron expr \| --every-minutes n] [--label tag]` |
| Add, change, or clear a schedule | `tli schedule <task-id> [--cron expr \| --every-minutes n] [--ready-at time] [--clear]` |
| List tasks | `tli list [--status todo] [--ready] [--label tag] [--query text] [--limit n] [--all]` |
| Show actionable work | `tli ready [--query text] [--limit n]` |
| Compact hook state | `tli state [--query text] [--limit n]` |
| Inspect one task | `tli show <task-id> [--verbose]` |
| Start work | `tli start <task-id> [--note text]` |
| Save a pause point | `tli checkpoint <task-id> [--note text] [--next-step text] [--next-task id]` |
| Block work | `tli block <task-id> --reason text` |
| Request review | `tli review <task-id> [--note text]` |
| Finish work | `tli done <task-id> [--note text] [--next-step text] [--next-task id] [--clear-schedule]` |
| Add context | `tli note <task-id> "text"` |
| View history | `tli log [task-id] [--limit n]` |
| Start local web UI/API | `tli server start [--port 3030]` |
| Print this guide | `tli skill` |

## Dependencies

Use dependencies when one task is **not actually actionable** until another task finishes.

```powershell
tli dep add <task-id> <dependency-id>
tli dep remove <task-id> <dependency-id>
```

### When to use a dependency

Add a dependency when the answer to **"can I do this now?"** is **no** until another task is done.

Good fits:

- `release-notes` depends on `ship-v0-3-0`
- `benchmark-parser` depends on `parser-cache`
- `deploy-prod` depends on `approve-release`

Do **not** add a dependency just because tasks are related, in the same area, or probably happen in sequence. Use separate tasks without a dependency when either task could still be worked on independently.

### How dependencies affect behavior

- A task is `ready` only when it is `todo`, its `ready_at` time has arrived, and **all** dependencies are `done`.
- A due task with unfinished dependencies moves into `pending_dependencies` in `tli state` instead of `ready`.
- `tli show <task-id>` exposes `blocked_by` so you can see exactly what is preventing work.
- `tli next <task-id>` can infer a ready dependency as the next thing to pick up when that helps resume work.

### Practical workflow

```powershell
tli add "Parser cache" --id parser-cache
tli add "Benchmark parser" --id benchmark-parser
tli dep add benchmark-parser parser-cache
tli state
tli show benchmark-parser
```

In that flow:

1. `benchmark-parser` stays out of `ready` until `parser-cache` is done.
2. `tli state` shows it under `pending_dependencies` once it is otherwise due.
3. `tli show benchmark-parser` shows `blocked_by: parser-cache`.

### Dependency direction

Read `tli dep add <task> <dependency>` as:

> **task** depends on **dependency**

Example:

```powershell
tli dep add benchmark-parser parser-cache
```

means:

- work on `parser-cache` first
- `benchmark-parser` is blocked until `parser-cache` is done

If you ever hesitate about order, ask:

> "Which task must be finished first?"

That earlier task is the **dependency**.

## Continuation flows

Use continuation hints when a task reaches a checkpoint or finished handoff state:

```powershell
tli checkpoint <task-id> --note "Parser works; benchmarks left" --next-step "Run parser benchmarks"
tli done <task-id> --next-task write-release-note
tli next <task-id>
```

Continuation hints have two lanes:

1. `--next-step` for the immediate step inside the same task
2. `--next-task` for the next sibling, follow-up, or separate task

If `next_task` is omitted, `tli` can infer it from ready dependencies and ready top-level tasks when possible for unfinished tasks. Aggregate `tli next` follows completed handoffs to unfinished dependent, sibling, or follow-up tasks, while `tli next <task-id>` still shows an individual done task's explicit continuation hints. Starting a task clears stored continuation hints because the task is active again.

## Scheduled tasks

Use schedules when a task should come back automatically after each completed cycle.

```powershell
tli add "Nightly cleanup" --cron "0 22 * * *"
tli add "Daily review" --every-minutes 1440
tli schedule nightly-cleanup --cron "0 23 * * *"
tli done nightly-cleanup --note "Reviewed current worktree"
tli done nightly-cleanup --clear-schedule --note "Retired this recurring task"
```

- `--cron` accepts a standard 5-field cron expression (`minute hour day-of-month month day-of-week`).
- `--every-minutes` is useful for fixed-interval loops.
- `--clear` removes the recurring schedule and its pending scheduled `ready_at`.
- `tli done --clear-schedule` completes a scheduled task permanently instead of re-arming the next cycle.
- `--ready-at` accepts RFC3339 timestamps plus human-friendly local time: `2026-05-10 12:20:10`, `12:20:10` for today, and `5-10 13:0:0` for the current year.
- Scheduled tasks stay `todo`; when you run `tli done`, the current cycle is recorded and the task is re-armed with the next `ready_at`.
- Pass `--ready-at` when migrating an existing scheduler so the first due time matches the old system exactly.

## Storage and portability

By default `tli` stores files under `.tli/` in the current directory:

```text
.tli/
  index.json
  events.ndjson
  task-events/<task-id>.ndjson
  tasks/<task-id>.json
```

`tasks/<task-id>.json` is the canonical task detail. `index.json` is the summary index used for cheap state queries. `task-events/<task-id>.ndjson` is a file-based read view for faster task-specific logs.

Use `--root <path>` when a hook or script should target a specific store:

```powershell
tli --root C:\path\to\.tli --json state
```
