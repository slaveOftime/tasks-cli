# tasks-cli

`tasks-cli` is a Rust task tracker built around the `tli` command. It is designed for fast local use, cheap prompt integration, and reliable file-based storage that works well for both humans and agents.

The CLI keeps task state in a repo-local `.tli/` directory, shows clean human-readable sections by default, and exposes richer inspection only when you ask for it with `--verbose` or `--json`.

## What `tli` is for

`tli` is meant to cover a practical working loop:

1. Create and organize tasks locally in the repo you are working in
2. Ask what is actionable now with cheap compact commands
3. Capture checkpoints and continuation hints when pausing or handing work off
4. Drill into full details only when needed

It is especially useful when:

- you want task state to live with the codebase, not in a remote service
- you want a CLI that is comfortable for humans but cheap for automation
- you want hooks or agents to fetch compact current state and only expand on demand

## Installation

### Install from this local repo

From the repository root:

```powershell
cargo install --path . --force
```

That installs the `tli` binary into Cargo's global bin directory for the current user.

### Build without installing

```powershell
cargo build
```

### Validate the project

```powershell
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features
```

## Storage model

By default `tli` looks for an existing `.tli/` directory starting from the current working directory and walking up parent directories until it finds one.

- If it finds `.tli/`, that becomes the working store
- If it does not find one, it errors clearly and tells you to use `--root`
- If you pass `--root <path>`, that path is used directly

Store layout:

```text
.tli/
  index.json
  events.ndjson
  tasks/
    <task-id>.json
```

This keeps reads fast:

- `index.json` is the cheap summary/index file
- `events.ndjson` is the append-only activity log
- `tasks/<id>.json` stores canonical task detail

## Core command flows

### 1. Start with compact state

```powershell
tli state
tli ready
tli next
```

Use these first when you want to know:

- what is ready now
- what is active or blocked
- what should continue next after a checkpoint or handoff

### 2. Add and inspect tasks

```powershell
tli add "Implement parser cache" --summary "Cache parsed plans by workspace" --label rust
tli add "Nightly cleanup" --cron "0 22 * * *"
tli schedule nightly-cleanup --every-minutes 1440
tli list
tli show <task-id>
tli --verbose show <task-id>
tli --json show <task-id>
```

Guideline:

- use plain `show` for compact inspection
- use `--verbose` for human review
- use `--json` for hooks, scripts, and agents

### Reading the default output

Human-readable commands such as `list`, `ready`, `state`, `show`, and `log` default to compact output that is meant to be scanned in a terminal:

- headings summarize the command scope or counts
- the default view keeps each task short and only shows the most useful metadata
- `--verbose` expands the same commands with timestamps and fuller detail
- `show` keeps the default view short and promotes longer fields into labeled sections

Useful switches:

- `tli <command> --help` explains that command's arguments and defaults
- `tli --verbose <command>` adds timestamps and fuller detail for human review
- `tli --json <command>` keeps the machine-readable contract for automation

### 3. Move work through the lifecycle

```powershell
tli start <task-id> --note "Picked up after triage"
tli checkpoint <task-id> --note "Pause here" --next-step "Resume benchmark wiring"
tli block <task-id> --reason "Waiting on upstream API"
tli review <task-id> --note "Ready for boss review"
tli done <task-id> --note "Merged and verified"
tli done nightly-cleanup --note "Cycle complete"
tli note <task-id> "Need benchmark follow-up"
tli log <task-id> --limit 20
```

### 4. Model dependencies and decomposition

```powershell
tli dep add <task-id> <dependency-id>
tli dep remove <task-id> <dependency-id>
tli subtask add <parent-id> <child-id>
tli subtask remove <parent-id> <child-id>
```

Rules of thumb:

- use **dependencies** for hard prerequisites
- use **subtasks** for decomposition
- subtasks do **not** automatically block the parent
- `ready` means `todo`, schedule arrived, and all dependencies are done

## Continuation and handoff usage

One of the main goals of `tli` is making pause/resume and handoff flows cheap and explicit.

Use continuation hints when checkpointing or finishing work:

```powershell
tli checkpoint <task-id> --next-step "Resume API wiring"
tli checkpoint <task-id> --next-subtask child-task-id
tli done <task-id> --next-task follow-up-task-id
```

There are three continuation lanes:

1. `--next-step` for the next action inside the same task
2. `--next-subtask` for the next child task
3. `--next-task` for the next sibling or follow-up task

Then retrieve them with:

```powershell
tli next
tli next <task-id>
tli --json next <task-id>
```

## Scheduled task usage

Use schedules when a task should return automatically after each completed cycle.

```powershell
tli add "Nightly cleanup" --cron "0 22 * * *"
tli add "Daily review" --every-minutes 1440
tli schedule nightly-cleanup --cron "0 23 * * *"
tli done nightly-cleanup --note "Reviewed current worktree"
```

- `--cron` uses a standard 5-field cron expression (`minute hour day-of-month month day-of-week`).
- `--every-minutes` is useful for fixed-interval loops.
- Scheduled tasks stay `todo`; `tli done` records the completed cycle and re-arms the next `ready_at`.
- Pass `--ready-at` when migrating an existing scheduler so the first due time matches the old system exactly.

## Agent-friendly usage

For agents and hooks, the recommended pattern is:

### Cheap first call

```powershell
tli --json state --limit 6
```

This is the preferred low-token integration surface because it returns compact counts and short actionable entries.

### Drill down only when needed

```powershell
tli --json show <task-id>
tli --json next <task-id>
tli --json log <task-id> --limit 20
```

### Embedded usage help

```powershell
tli skill
tli --json skill
```

The skill doc is embedded in the binary so automation can retrieve the current usage guidance without reading files directly.

## Example day-to-day flow

```powershell
tli state
tli add "Wire parser cache to planning pipeline" --label rust --label perf
tli start 20260502-184257-wire-parser-cache
tli checkpoint 20260502-184257-wire-parser-cache --note "Parsing works" --next-step "Run planner benchmarks"
tli next 20260502-184257-wire-parser-cache
tli --verbose show 20260502-184257-wire-parser-cache
tli done 20260502-184257-wire-parser-cache --note "Merged" --next-task write-release-note
```

## Command summary

| Need | Command |
| --- | --- |
| Print embedded usage help | `tli skill` |
| Create a task | `tli add "Title" [--cron expr \| --every-minutes n]` |
| Add or change a schedule | `tli schedule <task-id> [--cron expr \| --every-minutes n] [--ready-at RFC3339]` |
| List tasks | `tli list` |
| Show actionable tasks | `tli ready` |
| Show compact repo state | `tli state` |
| Show continuation hints | `tli next [task-id]` |
| Inspect a task | `tli show <task-id>` |
| Start work | `tli start <task-id>` |
| Save a checkpoint | `tli checkpoint <task-id>` |
| Block work | `tli block <task-id> --reason "..."` |
| Request review | `tli review <task-id>` |
| Finish work | `tli done <task-id>` |
| Add note | `tli note <task-id> "..."` |
| View history | `tli log [task-id]` |
| Add dependency | `tli dep add <task-id> <dependency-id>` |
| Add subtask | `tli subtask add <parent-id> <child-id>` |

## Notes

- `skill` does not require a `.tli` store
- most task commands expect an existing `.tli` directory unless `--root` is provided
- an existing empty `.tli` is valid and will bootstrap on first write
