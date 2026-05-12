# `tli` - repo-local task tracking for humans and agents

[![crates.io](https://img.shields.io/crates/v/tli.svg)](https://crates.io/crates/tli)
[![npm](https://img.shields.io/npm/v/@slaveoftime/tli.svg)](https://www.npmjs.com/package/@slaveoftime/tli)
[![Release](https://github.com/slaveoftime/tasks-cli/actions/workflows/release.yml/badge.svg)](https://github.com/slaveoftime/tasks-cli/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

`tli` is a fast file-backed task tracker that keeps work state inside your repository instead of in a hosted service. It is designed to be comfortable in a terminal, cheap to query from hooks and agents, and reliable enough to use as local working memory during day-to-day development.

Use it when you want:

- repo-local task state under versioned project context
- compact terminal output for humans
- machine-readable JSON for hooks, scripts, and agents
- pause/resume and handoff flows without opening another app

## Why `tli`

Most task tools optimize for shared remote dashboards. `tli` optimizes for the working loop inside a codebase:

1. capture work as soon as you notice it
2. ask what is actionable now
3. checkpoint or hand off without losing the thread
4. resume cheaply from the terminal or an AI prompt

The store lives in `.tli/`, so it stays close to the repo and is easy to inspect, back up, or automate.

## Install

### crates.io

```bash
cargo install tli
```

### npm

```bash
npm install -g @slaveoftime/tli
```

The npm package ships the compiled `tli` binary for supported release targets and exposes the same `tli` command.

### From source

```bash
git clone git@github.com:slaveoftime/tasks-cli.git
cd tasks-cli
cargo install --path . --force
```

## Quick start

Create a store in the repository you want to track:

```bash
mkdir .tli
```

Then start using it:

```bash
tli add "Ship first public release" --summary "Finalize docs and publication flow" --label release
tli add "Nightly maintenance" --cron "0 22 * * *"
tli state
tli ready
tli start ship-first-public-release --note "Picking this up now"
tli checkpoint ship-first-public-release --next-step "Verify npm release assets"
tli next
```

## Core command flow

### 1. Start with compact state

```bash
tli state
tli ready
tli next
```

These commands are the fastest way to answer:

- what is ready right now
- what is due but blocked by dependencies
- what is already in flight
- what should continue after a checkpoint or handoff

`tli ready` keeps dependency-blocked tasks out of the list by default. The exception is a scheduled task whose due time has arrived: it still appears in `ready`, but with a warning when unfinished dependencies are preventing the current cycle from actually starting.

### 2. Create and inspect tasks

```bash
tli add "Implement parser cache" --summary "Cache parsed plans by workspace" --label rust
tli add "Nightly cleanup" --every-minutes 1440
tli list
tli show <task-id>
tli --verbose show <task-id>
tli --json show <task-id>
```

Most commands that take a task id also accept a case-insensitive partial id match, so you usually do not need to type the full stored id.

### 3. Move work through the lifecycle

```bash
tli start <task-id> --note "Picked up after triage"
tli checkpoint <task-id> --note "Pause here" --next-step "Resume benchmark wiring"
tli block <task-id> --reason "Waiting on upstream API"
tli review <task-id> --note "Ready for review"
tli done <task-id> --note "Merged and verified"
tli note <task-id> "Need benchmark follow-up"
tli log <task-id> --limit 20
```

### 4. Model prerequisites

```bash
tli dep add <task-id> <dependency-id>
tli dep remove <task-id> <dependency-id>
```

- dependencies gate readiness

## Output modes

`tli` keeps the default terminal view compact, then expands only when you ask:

```bash
tli state
tli --verbose state
tli --json state --limit 6
```

- default output is optimized for terminal scanning
- `--verbose` adds richer human-readable detail
- `--json` keeps the command contract stable for hooks, scripts, and agents

## Agent and hook usage

For automation, the recommended first call is:

```bash
tli --json state --limit 6
```

That payload is meant to stay cheap and actionable. A good pattern for agents is:

1. read `state`
2. surface `ready`, `pending_dependencies`, and `active`
3. use aggregate `next` for real unfinished continuation targets, then drill into `show`, `next <task-id>`, or `log` only for the task that actually needs detail

Useful follow-up commands:

```bash
tli --json show <task-id>
tli --json next <task-id>
tli --json log <task-id> --limit 20
tli skill
tli --json skill
```

The embedded skill guide is compiled into the binary so prompts and hooks can fetch current usage guidance without reading repository files directly.

`--ready-at` accepts RFC3339 timestamps and human-friendly local time: `2026-05-10 12:20:10`, `12:20:10` for today, or `5-10 13:0:0` for the current year.

## Storage model

By default `tli` looks upward from the current working directory until it finds an existing `.tli/` directory. You can also target one directly with `--root`.

```text
.tli/
  index.json
  events.ndjson
  task-events/
    <task-id>.ndjson
  tasks/
    <task-id>.json
```

- `index.json` is the cheap summary file used for fast state queries
- `events.ndjson` is the append-only activity log
- `task-events/<id>.ndjson` speeds up task-specific log reads
- `tasks/<id>.json` stores the canonical task detail

## Contributor setup

```bash
git clone git@github.com:slaveoftime/tasks-cli.git
cd tasks-cli
cargo test
cargo fmt -- --check
cargo clippy --all-targets --all-features
```

The project is intentionally small and file-based. If you are changing behavior, prefer updating the Rust tests in `src/store/tests.rs` and `tests/cli.rs` alongside the implementation.

## Release and publication

GitHub Actions publishes releases from version tags matching `v*`.

When you push a tag such as `v0.2.0`, the release workflow will:

1. validate the repo with `cargo test`, `cargo fmt -- --check`, and `cargo clippy --all-targets --all-features`
2. build release binaries for Linux x64, macOS arm64, and Windows x64
3. upload zipped binaries to the GitHub Release
4. publish the crate `tli` to crates.io
5. publish the npm package `@slaveoftime/tli`

Create a release like this:

```bash
git tag v0.2.0
git push origin v0.2.0
```

Required repository secrets:

| Secret | Purpose |
| --- | --- |
| `CARGO_REGISTRY_TOKEN` | publish `tli` to crates.io |
| `NPM_TOKEN` | publish `@slaveoftime/tli` to npm |

The workflow copies the root `README.md` and `LICENSE` into the npm package during publication so the package page stays aligned with the repo.

## Command summary

| Need | Command |
| --- | --- |
| Print embedded usage help | `tli skill` |
| Create a task | `tli add "Title" [--id id] [--summary text] [--ready-at time] [--cron expr \| --every-minutes n] [--label tag]` |
| Add, change, or clear a schedule | `tli schedule <task-id> [--cron expr \| --every-minutes n] [--ready-at time] [--clear]` |
| List tasks | `tli list [--status todo] [--ready] [--label tag] [--query text] [--limit n] [--all]` |
| Show actionable work | `tli ready [--query text] [--limit n]` |
| Show compact repo state | `tli state [--query text] [--limit n]` |
| Show continuation hints | `tli next [task-id]` (`tli next` resolves done handoffs to unfinished targets; `tli next <task-id>` inspects one task's stored handoff) |
| Inspect a task | `tli show <task-id>` |
| Start work | `tli start <task-id>` |
| Save a checkpoint | `tli checkpoint <task-id>` |
| Block work | `tli block <task-id> --reason "..."` |
| Request review | `tli review <task-id>` |
| Finish work | `tli done <task-id> [--clear-schedule]` |
| Add note | `tli note <task-id> "..."` |
| View history | `tli log [task-id]` |
| Add dependency | `tli dep add <task-id> <dependency-id>` |

## License

MIT. See [LICENSE](LICENSE).
