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
        "Recommended agent and Jarvis hook flow",
        "tli add",
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
        "tli subtask add",
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
    assert_eq!(skill["path"], "skills/til/SKILL.md");
    assert!(
        skill["content"]
            .as_str()
            .unwrap()
            .contains("tli --json state")
    );
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
        .stdout(contains("labels=rust"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["show", id])
        .assert()
        .success()
        .stdout(contains("Ship first slice"))
        .stdout(contains("summary:"))
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
        .stdout(contains("next=step:Archive notes"));

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
}

#[test]
fn dependency_and_subtask_links_support_ready_and_removal_flows() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    let alpha = add_task(temp.path(), "alpha", "Alpha");
    let beta = add_task(temp.path(), "beta", "Beta");
    let child = add_task(temp.path(), "child", "Child");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["dep", "add", &beta, &alpha])
        .assert()
        .success();
    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["subtask", "add", &beta, &child])
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
    assert_eq!(ids, vec!["alpha", "child"]);

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["dep", "remove", &beta, &alpha])
        .assert()
        .success();
    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["subtask", "remove", &beta, &child])
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
    assert_eq!(detail["children"].as_array().unwrap().len(), 0);
}

#[test]
fn checkpoint_done_next_and_state_support_continuation_handoffs() {
    let temp = TempDir::new().unwrap();
    init_store(temp.path());

    let alpha = add_task(temp.path(), "alpha", "Alpha");
    let beta = add_task(temp.path(), "beta", "Beta");
    let child = add_task(temp.path(), "child", "Child");
    let handoff = add_task(temp.path(), "handoff", "Handoff");

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["subtask", "add", &alpha, &child])
        .assert()
        .success();

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
    assert_eq!(next_task["next"]["next_subtask"], "child");
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
        .args(["--verbose", "state"])
        .assert()
        .success()
        .stdout(contains("checkpoint:"))
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
        .stdout(contains("counts ready=0"))
        .stdout(contains("active=0"))
        .stdout(contains("done=0"));

    Command::cargo_bin("tli")
        .unwrap()
        .current_dir(temp.path())
        .args(["add", "Bootstrap", "--id", "bootstrap"])
        .assert()
        .success()
        .stdout(contains("created bootstrap"));

    assert!(temp.path().join(".tli").join("index.json").is_file());
    assert!(temp.path().join(".tli").join("events.ndjson").is_file());
    assert!(temp.path().join(".tli").join("tasks").join("bootstrap.json").is_file());

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
