# tli task CLI skill

Use `tli` to keep repo-local task state in `.tli/`. It is compact by default for agents and hooks, but still comfortable for humans through predictable verbs and optional verbose output.

## Quick start

```powershell
tli add "Implement parser cache" --summary "Cache parsed plans by workspace" --label rust --label perf
tli add "Nightly cleanup" --cron "0 22 * * *"
tli state
tli ready
tli show <task-id>
tli schedule <task-id> --every-minutes 1440
tli start <task-id> --note "Picked up after triage"
tli checkpoint <task-id> --note "Pause here" --next-step "Resume benchmark wiring"
tli done <task-id> --note "Merged" --next-task follow-up-task-id
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

## Recommended agent and Jarvis hook flow

Use `state` first because it reads the summary index and returns cheap counts plus short actionable lines:

```powershell
tli --json state --limit 6
```

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

## Core commands

| Need | Command |
| --- | --- |
| Create a task | `tli add "Title" [--id id] [--summary text] [--ready-at RFC3339] [--cron expr \| --every-minutes n] [--label tag]` |
| Add or change a schedule | `tli schedule <task-id> [--cron expr \| --every-minutes n] [--ready-at RFC3339]` |
| List tasks | `tli list [--status todo] [--ready] [--query text] [--limit n] [--all]` |
| Show actionable work | `tli ready [--query text] [--limit n]` |
| Compact hook state | `tli state [--query text] [--limit n]` |
| Inspect one task | `tli show <task-id> [--verbose]` |
| Start work | `tli start <task-id> [--note text]` |
| Save a pause point | `tli checkpoint <task-id> [--note text] [--next-step text] [--next-subtask id] [--next-task id]` |
| Block work | `tli block <task-id> --reason text` |
| Request review | `tli review <task-id> [--note text]` |
| Finish work | `tli done <task-id> [--note text] [--next-step text] [--next-subtask id] [--next-task id]` |
| Add context | `tli note <task-id> "text"` |
| View history | `tli log [task-id] [--limit n]` |
| Print this guide | `tli skill` |

## Dependencies and subtasks

Use dependencies for hard prerequisites and subtasks for decomposition.

```powershell
tli dep add <task-id> <dependency-id>
tli dep remove <task-id> <dependency-id>
tli subtask add <parent-id> <child-id>
tli subtask remove <parent-id> <child-id>
```

- A task is `ready` only when it is `todo`, its `ready_at` time has arrived, and every dependency is `done`.
- A subtask does not block its parent automatically; use `dep add` when the parent must wait.
- `tli next <task-id>` can infer a ready child as `next_subtask`.

## Continuation flows

Use continuation hints when a task reaches a checkpoint or finished handoff state:

```powershell
tli checkpoint <task-id> --note "Parser works; benchmarks left" --next-step "Run parser benchmarks"
tli checkpoint <task-id> --next-subtask benchmark-parser-cache
tli done <task-id> --next-task write-release-note
tli next <task-id>
```

Continuation hints have three lanes:

1. `--next-step` for the immediate step inside the same task
2. `--next-subtask` for the child task to pick up next
3. `--next-task` for the next sibling, follow-up, or separate task

If `next_subtask` or `next_task` is omitted, `tli` infers them from ready child tasks and ready top-level tasks when possible. Starting a task clears stored continuation hints because the task is active again.

## Scheduled tasks

Use schedules when a task should come back automatically after each completed cycle.

```powershell
tli add "Nightly cleanup" --cron "0 22 * * *"
tli add "Daily review" --every-minutes 1440
tli schedule nightly-cleanup --cron "0 23 * * *"
tli done nightly-cleanup --note "Reviewed current worktree"
```

- `--cron` accepts a standard 5-field cron expression (`minute hour day-of-month month day-of-week`).
- `--every-minutes` is useful for fixed-interval loops.
- Scheduled tasks stay `todo`; when you run `tli done`, the current cycle is recorded and the task is re-armed with the next `ready_at`.
- Pass `--ready-at` when migrating an existing scheduler so the first due time matches the old system exactly.

## Storage and portability

By default `tli` stores files under `.tli/` in the current directory:

```text
.tli/
  index.json
  events.ndjson
  tasks/<task-id>.json
```

Use `--root <path>` when a hook or script should target a specific store:

```powershell
tli --root C:\path\to\.tli --json state
```
