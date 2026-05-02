# tli

`tli` is a fast, repo-local task CLI for humans and agents. It stores state in `.tli/` so task context stays with the worktree and can be reviewed without a daemon or database.

## Commands

```powershell
tli skill
tli add "Implement parser cache" --summary "Move cache ownership into the Rust service" --label rust --label perf
tli state
tli list
tli ready
tli dep add 20260502-184257-implement-parser-cache 20260502-184300-benchmark-cache
tli subtask add 20260502-184257-implement-parser-cache 20260502-184315-write-bench
tli checkpoint 20260502-184257-implement-parser-cache --note "Pause here" --next-step "Resume benchmark wiring"
tli next 20260502-184257-implement-parser-cache
tli show 20260502-184257-implement-parser-cache --verbose
tli start 20260502-184257-implement-parser-cache --note "Picked up after triage"
tli review 20260502-184257-implement-parser-cache --note "Ready for boss review"
tli done 20260502-184257-implement-parser-cache --note "Merged and verified" --next-task "follow-up-release-note"
tli note 20260502-184257-implement-parser-cache "Need benchmark follow-up"
tli log --limit 20
```

## Storage layout

```text
.tli/
  index.json       # fast summary reads for list/filter operations
  events.ndjson    # append-only activity log for review and agents
  tasks/
    <task-id>.json # canonical task record
```

Use `--root <path>` to point at a different store directory.

## Agent-first conventions

- Use `tli dep add <task> <dependency>` to express hard prerequisites.
- Use `tli subtask add <parent> <child>` for decomposition without changing completion semantics.
- Use `tli ready` to ask what is actionable now: `todo` tasks whose schedule has arrived and whose dependencies are all done.
- Use `tli state` as the cheap default hook surface: compact counts plus short actionable lines.
- Use `tli show <id> --verbose` or `tli --json show <id>` for richer inspection only when needed.
- Use `tli checkpoint` and `tli done --next-step/--next-subtask/--next-task` to preserve clean continuation hints.
