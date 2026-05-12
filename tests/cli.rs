use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::{contains, is_empty};
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn skill_command_is_natural_and_covers_command_surface() {
    let assert = Command::cargo_bin("tli")
        .unwrap()
        .arg("skill")
        .assert()
        .success();

    for expected in [
        "Quick start",
        "Recommended agent hook flow",
        "tli add",
        "tli schedule",
        "tli list",
        "tli ready",
        "tli state",
        "tli show",
        "tli start",
        "tli checkpoint",
        "tli block",
        "tli review",
        "tli done",
        "tli note",
        "tli log",
        "tli dep add",
        "--ready-at",
        "local time",
        "--verbose",
        "--json",
        "--root",
    ] {
        assert!(String::from_utf8_lossy(&assert.get_output().stdout).contains(expected));
    }
}

#[test]
fn skill_command_has_json_form_for_agents() {
    let output = Command::cargo_bin("tli")
        .unwrap()
        .args(["--json", "skill"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let skill: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(skill["path"], "skills/tli/SKILL.md");
    assert!(
        skill["content"]
            .as_str()
            .unwrap()
            .contains("tli --json state")
    );
}

#[test]
fn help_output_explains_human_and_json_usage() {
    Command::cargo_bin("tli")
        .unwrap()
        .args(["--help"])
        .assert()
        .success()
        .stdout(contains(
            "The default output is optimized for people scanning the terminal",
        ))
        .stdout(contains("--json"))
        .stdout(contains("Examples:"));

    Command::cargo_bin("tli")
        .unwrap()
        .args(["show", "--help"])
        .assert()
        .success()
        .stdout(contains(
            "Task id or case-insensitive partial id to inspect",
        ))
        .stdout(contains("human-friendly terminal output"));

    Command::cargo_bin("tli")
        .unwrap()
        .args(["state", "--help"])
        .assert()
        .success()
        .stdout(contains("review"))
        .stdout(contains("handoff"));
}

#[test]
fn add_query_list_and_show_cover_compact_verbose_and_json_modes() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    let output = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args([
            "--json",
            "add",
            "Ship first slice",
            "--summary",
            "Implement the first useful CLI workflow with enough detail for verbose inspection",
            "--label",
            "rust",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let task: Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = task["id"].as_str().unwrap();
    assert_eq!(task["status"], "todo");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["list", "--query", "rust"])
        .assert()
        .success()
        .stdout(contains("Tasks in"))
        .stdout(contains("labels: rust"))
        .stdout(predicates::str::contains("updated:").not());

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--verbose", "list", "--query", "rust"])
        .assert()
        .success()
        .stdout(contains("labels: rust"))
        .stdout(contains("updated:"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["show", id])
        .assert()
        .success()
        .stdout(contains("Ship first slice"))
        .stdout(contains("Summary"))
        .stdout(predicates::str::contains("created:").not());

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--verbose", "show", id])
        .assert()
        .success()
        .stdout(contains("created:"))
        .stdout(contains("labels: rust"));

    let detail = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "show", id])
        .output()
        .unwrap();
    assert!(detail.status.success());
    let detail: Value = serde_json::from_slice(&detail.stdout).unwrap();
    assert_eq!(detail["task"]["title"], "Ship first slice");
    assert_eq!(detail["ready"], true);
}

#[test]
fn list_supports_label_filters() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args([
            "add",
            "Parser cache",
            "--id",
            "parser-cache",
            "--label",
            "rust",
            "--label",
            "perf",
        ])
        .assert()
        .success();
    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args([
            "add",
            "Docs cleanup",
            "--id",
            "docs-cleanup",
            "--label",
            "docs",
        ])
        .assert()
        .success();

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["list", "--label", "RUST"])
        .assert()
        .success()
        .stdout(contains("parser-cache"))
        .stdout(predicates::str::contains("docs-cleanup").not());

    let output = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "list", "--label", "rust,perf"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let tasks: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(tasks.as_array().unwrap().len(), 1);
    assert_eq!(tasks[0]["id"], "parser-cache");
}

#[test]
fn partial_task_id_matches_are_case_insensitive_and_ambiguous_matches_fail_safely() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    add_task(temp.path(), "daily-news-prep", "Prepare daily news");
    add_task(temp.path(), "daily-review", "Review daily plan");
    add_task(temp.path(), "parser-cache", "Wire parser cache");
    add_task(temp.path(), "benchmark-parser", "Benchmark parser");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["show", "NEWS"])
        .assert()
        .success()
        .stdout(contains("Prepare daily news"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["note", "NEWS", "Needs source review"])
        .assert()
        .success()
        .stdout(contains("Updated"))
        .stdout(contains("daily-news-prep"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["dep", "add", "CACHE", "Bench"])
        .assert()
        .success()
        .stdout(contains("Linked parser-cache -> benchmark-parser"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["show", "daily"])
        .assert()
        .failure()
        .stderr(contains("ambiguous"))
        .stderr(contains("daily-news-prep"))
        .stderr(contains("daily-review"));
}

#[test]
fn scheduled_tasks_support_cron_interval_and_auto_rearm() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args([
            "add",
            "Nightly cleanup",
            "--id",
            "nightly",
            "--cron",
            "0 22 * * *",
            "--ready-at",
            "2026-05-02T22:00:00+08:00",
        ])
        .assert()
        .success()
        .stdout(contains("Created"))
        .stdout(contains("schedule: cron 0 22 * * *"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args([
            "schedule",
            "nightly",
            "--every-minutes",
            "1440",
            "--ready-at",
            "2026-05-02T22:00:00+08:00",
        ])
        .assert()
        .success()
        .stdout(contains("Scheduled"))
        .stdout(contains("schedule: every 1440m"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["done", "nightly", "--note", "Cycle complete"])
        .assert()
        .success()
        .stdout(contains("[todo]"))
        .stdout(contains("schedule: every 1440m"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--verbose", "show", "nightly"])
        .assert()
        .success()
        .stdout(contains("schedule: every 1440m"))
        .stdout(contains("Completion note"));

    let state = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "state"])
        .output()
        .unwrap();
    assert!(state.status.success());
    let state: Value = serde_json::from_slice(&state.stdout).unwrap();
    assert_eq!(state["counts"]["done"], 0);
    assert!(state["counts"]["todo"].as_u64().unwrap() >= 1);
}

#[test]
fn done_supports_clear_schedule_for_scheduled_tasks() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args([
            "add",
            "Nightly cleanup",
            "--id",
            "nightly",
            "--every-minutes",
            "60",
        ])
        .assert()
        .success();

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["done", "nightly", "--clear-schedule", "--note", "Retired"])
        .assert()
        .success()
        .stdout(contains("[done]"))
        .stdout(predicates::str::contains("schedule:").not());

    let detail = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "show", "nightly"])
        .output()
        .unwrap();
    assert!(detail.status.success());
    let detail: Value = serde_json::from_slice(&detail.stdout).unwrap();
    assert_eq!(detail["task"]["status"], "done");
    assert!(detail["task"]["schedule"].is_null());
    assert!(detail["task"]["ready_at"].is_null());
    assert_eq!(detail["task"]["completed_note"], "Retired");
}

#[test]
fn schedule_clear_removes_schedule_ready_at_and_uses_clear_message() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args([
            "add",
            "Nightly cleanup",
            "--id",
            "nightly",
            "--every-minutes",
            "60",
        ])
        .assert()
        .success();

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["schedule", "nightly", "--clear"])
        .assert()
        .success()
        .stdout(contains("Cleared schedule"))
        .stdout(predicates::str::contains("schedule:").not());

    let detail = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "show", "nightly"])
        .output()
        .unwrap();
    assert!(detail.status.success());
    let detail: Value = serde_json::from_slice(&detail.stdout).unwrap();
    assert!(detail["task"]["schedule"].is_null());
    assert!(detail["task"]["ready_at"].is_null());
    assert_eq!(detail["ready"], true);
}

#[test]
fn ready_at_accepts_human_friendly_local_time_inputs() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    for (id, ready_at) in [
        ("full-local", "2026-05-10 12:20:10"),
        ("today-local", "12:20:10"),
        ("month-day-local", "5-10 13:0:0"),
    ] {
        Command::cargo_bin("tli")
            .unwrap()
            .current_dir(temp.path())
            .args(["add", id, "--id", id, "--ready-at", ready_at])
            .assert()
            .success();

        let output = Command::cargo_bin("tli")
            .unwrap()
            .current_dir(temp.path())
            .args(["--json", "show", id])
            .output()
            .unwrap();
        assert!(output.status.success());
        let detail: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(detail["task"]["ready_at"].as_str().unwrap().ends_with('Z'));
    }
}

#[test]
fn lifecycle_and_history_cover_start_block_review_note_done() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());
    let task = add_task(temp.path(), "review-me", "Review me");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["start", &task, "--note", "Picked up"])
        .assert()
        .success()
        .stdout(contains("[active]"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["block", &task, "--reason", "Waiting on API"])
        .assert()
        .success()
        .stdout(contains("[blocked]"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["note", &task, "API returned"])
        .assert()
        .success();

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["review", &task, "--note", "Needs boss sign-off"])
        .assert()
        .success()
        .stdout(contains("[review]"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args([
            "done",
            &task,
            "--note",
            "Approved",
            "--next-step",
            "Archive notes",
        ])
        .assert()
        .success()
        .stdout(contains("[done]"))
        .stdout(contains("next: step: Archive notes"))
        .stdout(predicates::str::contains("updated:").not());

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args([
            "--verbose",
            "done",
            &task,
            "--note",
            "Approved again",
            "--next-step",
            "Archive notes",
        ])
        .assert()
        .success()
        .stdout(contains("updated:"));

    let log = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "log", &task])
        .output()
        .unwrap();
    assert!(log.status.success());
    let events: Value = serde_json::from_slice(&log.stdout).unwrap();
    let kinds = events
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(kinds.contains(&"started"));
    assert!(kinds.contains(&"blocked"));
    assert!(kinds.contains(&"note_added"));
    assert!(kinds.contains(&"review_requested"));
    assert!(kinds.contains(&"completed"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["log", &task, "--limit", "2"])
        .assert()
        .success()
        .stdout(contains("Events in"))
        .stdout(predicates::str::contains("status:").not());

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--verbose", "log", &task, "--limit", "2"])
        .assert()
        .success()
        .stdout(contains("status:"));
}

#[test]
fn dependency_links_support_ready_and_removal_flows() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    let alpha = add_task(temp.path(), "alpha", "Alpha");
    let beta = add_task(temp.path(), "beta", "Beta");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["dep", "add", &beta, &alpha])
        .assert()
        .success();

    let ready = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "ready"])
        .output()
        .unwrap();
    assert!(ready.status.success());
    let ready_tasks: Value = serde_json::from_slice(&ready.stdout).unwrap();
    let mut ids = ready_tasks
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    assert_eq!(ids, vec!["alpha"]);

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["show", &beta])
        .assert()
        .success()
        .stdout(contains("Blocked by"))
        .stdout(contains("alpha"));

    let detail = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "show", &beta])
        .output()
        .unwrap();
    assert!(detail.status.success());
    let detail: Value = serde_json::from_slice(&detail.stdout).unwrap();
    let blocked_by = detail["blocked_by"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(blocked_by.contains(&"alpha"));
    assert_eq!(detail["task"]["depends_on"].as_array().unwrap().len(), 1);
    assert_eq!(detail["next"]["next_task"], "alpha");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["dep", "remove", &beta, &alpha])
        .assert()
        .success();

    let detail = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "show", &beta])
        .output()
        .unwrap();
    assert!(detail.status.success());
    let detail: Value = serde_json::from_slice(&detail.stdout).unwrap();
    assert_eq!(detail["task"]["depends_on"].as_array().unwrap().len(), 0);
}

#[test]
fn next_without_id_resolves_done_handoffs_to_unfinished_targets() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    let done_without_handoff =
        add_task(temp.path(), "done-without-handoff", "Done without handoff");
    let done_with_handoff = add_task(temp.path(), "done-with-handoff", "Done with handoff");
    let follow_up = add_task(temp.path(), "follow-up", "Follow up");
    let checkpointed = add_task(temp.path(), "checkpointed", "Checkpointed");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["done", &done_without_handoff])
        .assert()
        .success();
    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["done", &done_with_handoff, "--next-task", &follow_up])
        .assert()
        .success();
    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["checkpoint", &checkpointed])
        .assert()
        .success();

    let next = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "next", "--limit", "10"])
        .output()
        .unwrap();
    assert!(next.status.success());
    let next: Value = serde_json::from_slice(&next.stdout).unwrap();
    let ids = next
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(!ids.contains(&"done-without-handoff"));
    assert!(!ids.contains(&"done-with-handoff"));
    assert!(ids.contains(&"follow-up"));
    assert!(ids.contains(&"checkpointed"));

    let handoff = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "next", &done_with_handoff])
        .output()
        .unwrap();
    assert!(handoff.status.success());
    let handoff: Value = serde_json::from_slice(&handoff.stdout).unwrap();
    assert_eq!(handoff["id"], "done-with-handoff");
    assert_eq!(handoff["status"], "done");
    assert_eq!(handoff["next"]["next_task"], "follow-up");

    let state = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "state", "--limit", "10"])
        .output()
        .unwrap();
    assert!(state.status.success());
    let state: Value = serde_json::from_slice(&state.stdout).unwrap();
    assert_eq!(state["counts"]["handoff"], 1);
}

#[test]
fn next_without_id_follows_done_handoff_chains_and_dependency_graph() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    let wrapper = add_task(temp.path(), "wrapper", "Wrapper");
    let middle = add_task(temp.path(), "middle", "Middle");
    let final_target = add_task(temp.path(), "final-target", "Final target");
    let dependency = add_task(temp.path(), "dependency", "Dependency");
    let dependent = add_task(temp.path(), "dependent", "Dependent");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["dep", "add", &dependent, &dependency])
        .assert()
        .success();
    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["done", &middle, "--next-task", &final_target])
        .assert()
        .success();
    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["done", &wrapper, "--next-task", &middle])
        .assert()
        .success();
    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["done", &dependency, "--next-step", "Dependency complete"])
        .assert()
        .success();

    let next = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "next", "--limit", "10"])
        .output()
        .unwrap();
    assert!(next.status.success());
    let next: Value = serde_json::from_slice(&next.stdout).unwrap();
    let ids = next
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(!ids.contains(&"wrapper"));
    assert!(!ids.contains(&"middle"));
    assert!(!ids.contains(&"dependency"));
    assert!(ids.contains(&"final-target"));
    assert!(ids.contains(&"dependent"));
}

#[test]
fn state_surfaces_due_tasks_with_unmet_dependencies_without_marking_them_ready() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    let dependency = add_task(temp.path(), "dep", "Dependency");
    add_task(temp.path(), "other", "Other");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args([
            "add",
            "Target",
            "--id",
            "target",
            "--ready-at",
            "2020-01-01T00:00:00Z",
        ])
        .assert()
        .success();

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["dep", "add", "target", &dependency])
        .assert()
        .success();

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["ready"])
        .assert()
        .success()
        .stdout(contains("dep"))
        .stdout(predicates::str::contains("target").not());

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["state"])
        .assert()
        .success()
        .stdout(contains("pending deps: 1"))
        .stdout(contains("Pending dependencies"))
        .stdout(contains("target"))
        .stdout(contains("task: dep"));

    let state = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "state", "--limit", "10"])
        .output()
        .unwrap();
    assert!(state.status.success());
    let state: Value = serde_json::from_slice(&state.stdout).unwrap();
    assert_eq!(state["counts"]["ready"], 2);
    assert_eq!(state["counts"]["pending_dependencies"], 1);
    let ready_ids = state["ready"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(ready_ids.contains(&"dep"));
    assert!(ready_ids.contains(&"other"));
    assert_eq!(state["pending_dependencies"][0]["id"], "target");
    assert_eq!(state["pending_dependencies"][0]["ready"], false);
    assert_eq!(state["pending_dependencies"][0]["next"]["next_task"], "dep");
}

#[test]
fn ready_command_has_distinct_verbose_mode() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());
    add_task(temp.path(), "alpha", "Alpha");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["ready"])
        .assert()
        .success()
        .stdout(contains("Ready tasks in"))
        .stdout(predicates::str::contains("updated:").not());

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--verbose", "ready"])
        .assert()
        .success()
        .stdout(contains("updated:"));
}

#[test]
fn ready_lists_due_scheduled_tasks_with_dependency_warnings() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    let dependency = add_task(temp.path(), "dep", "Dependency");
    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args([
            "add",
            "Scheduled cleanup",
            "--id",
            "scheduled-cleanup",
            "--every-minutes",
            "60",
            "--ready-at",
            "2020-01-01T00:00:00Z",
        ])
        .assert()
        .success();
    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["dep", "add", "scheduled-cleanup", &dependency])
        .assert()
        .success();

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["ready"])
        .assert()
        .success()
        .stdout(contains("scheduled-cleanup"))
        .stdout(contains("warning: scheduled but blocked by dep"));

    let ready = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "ready"])
        .output()
        .unwrap();
    assert!(ready.status.success());
    let ready: Value = serde_json::from_slice(&ready.stdout).unwrap();
    let scheduled = ready
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "scheduled-cleanup")
        .unwrap();
    assert_eq!(scheduled["ready"], false);
    assert_eq!(scheduled["missing_dependencies"][0], "dep");
}

#[test]
fn checkpoint_done_next_and_state_support_continuation_handoffs() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    let alpha = add_task(temp.path(), "alpha", "Alpha");
    let beta = add_task(temp.path(), "beta", "Beta");
    let handoff = add_task(temp.path(), "handoff", "Handoff");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args([
            "checkpoint",
            &alpha,
            "--note",
            "Pause here",
            "--next-step",
            "Resume API wiring",
            "--next-task",
            &beta,
        ])
        .assert()
        .success();

    let next = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "next", &alpha])
        .output()
        .unwrap();
    assert!(next.status.success());
    let next_task: Value = serde_json::from_slice(&next.stdout).unwrap();
    assert_eq!(next_task["status"], "checkpoint");
    assert_eq!(next_task["next"]["next_step"], "Resume API wiring");
    assert_eq!(next_task["next"]["next_task"], "beta");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["done", &handoff, "--next-task", &beta])
        .assert()
        .success();

    let state = Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--json", "state", "--limit", "2"])
        .output()
        .unwrap();
    assert!(state.status.success());
    let state: Value = serde_json::from_slice(&state.stdout).unwrap();
    assert_eq!(state["counts"]["checkpoint"], 1);
    assert_eq!(state["counts"]["handoff"], 1);
    assert_eq!(state["checkpoint"][0]["id"], "alpha");
    assert_eq!(state["handoff"][0]["id"], "handoff");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["state"])
        .assert()
        .success()
        .stdout(contains("Checkpoint"))
        .stdout(predicates::str::contains("updated:").not());

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["--verbose", "state"])
        .assert()
        .success()
        .stdout(contains("Checkpoint"))
        .stdout(contains("updated:"));
}

#[test]
fn root_override_targets_specific_store() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("custom-store");

    Command::cargo_bin("tli")
        .unwrap()
        .args([
            "--root",
            root.to_str().unwrap(),
            "add",
            "Rooted",
            "--id",
            "rooted",
        ])
        .assert()
        .success();

    Command::cargo_bin("tli")
        .unwrap()
        .args(["--root", root.to_str().unwrap(), "show", "rooted"])
        .assert()
        .success()
        .stdout(contains("Rooted"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["show", "rooted"])
        .assert()
        .failure()
        .stdout(is_empty());
}

#[test]
fn implicit_root_walks_up_to_parent_store() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let nested = repo.join("deep").join("child");
    init_store(&repo);
    fs::create_dir_all(&nested).unwrap();

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(&repo)
        .args(["add", "Nested task", "--id", "nested-task"])
        .assert()
        .success();

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(&nested)
        .args(["show", "nested-task"])
        .assert()
        .success()
        .stdout(contains("Nested task"));
}

#[test]
fn empty_store_supports_compact_reads_and_first_write_bootstrap() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["state"])
        .assert()
        .success()
        .stdout(contains("Counts"))
        .stdout(contains("ready: 0"))
        .stdout(contains("done: 0"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["add", "Bootstrap", "--id", "bootstrap"])
        .assert()
        .success()
        .stdout(contains("Created"))
        .stdout(contains("bootstrap"));

    assert!(temp.path().join(".tli").join("index.json").is_file());
    assert!(temp.path().join(".tli").join("events.ndjson").is_file());
    assert!(
        temp.path()
            .join(".tli")
            .join("task-events")
            .join("bootstrap.ndjson")
            .is_file()
    );
    assert!(
        temp.path()
            .join(".tli")
            .join("tasks")
            .join("bootstrap.json")
            .is_file()
    );

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["show", "bootstrap"])
        .assert()
        .success()
        .stdout(contains("Bootstrap"));
}

#[test]
fn implicit_root_errors_when_no_store_exists() {
    let temp = TempDir::new().unwrap();
    let nested = temp.path().join("no").join("store");
    fs::create_dir_all(&nested).unwrap();

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(&nested)
        .args(["state"])
        .assert()
        .failure()
        .stdout(is_empty())
        .stderr(contains("could not find '.tli'"))
        .stderr(contains("--root"));
}

fn init_store(cwd: &Path) {
    fs::create_dir_all(cwd.join(".tli")).unwrap();
}

fn add_task(cwd: &Path, id: &str, title: &str) -> String {
    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(cwd)
        .args(["add", title, "--id", id])
        .assert()
        .success();
    id.to_string()
}
