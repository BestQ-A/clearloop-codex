use std::fs;
use std::path::Path;

use anyhow::Result;
use predicates::str::contains;
use pretty_assertions::assert_eq;
use serde_json::Value;
use tempfile::TempDir;

fn codex_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

#[test]
fn clearloop_think_writes_visible_thinking_program() -> Result<()> {
    let codex_home = TempDir::new()?;
    let workspace = TempDir::new()?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args([
        "clearloop",
        "think",
        "--id",
        "tp-demo",
        "--task",
        "Fix startup failure",
        "--target-condition",
        "server_ready",
        "--observed-condition",
        "server_failed",
        "-C",
    ])
    .arg(workspace.path())
    .assert()
    .success()
    .stdout(contains("Thinking program written:"));

    let program = read_json(
        workspace
            .path()
            .join(".bestqa/thinking-programs/tp-demo.json")
            .as_path(),
    )?;
    assert_eq!(program["id"].as_str(), Some("tp-demo"));
    assert_eq!(program["user_task"].as_str(), Some("Fix startup failure"));
    assert_eq!(
        program["reasoning_visibility"].as_str(),
        Some("visible_by_default")
    );
    assert_eq!(
        program["target_condition"]["condition_ref"].as_str(),
        Some("server_ready")
    );

    Ok(())
}

#[test]
fn clearloop_run_writes_observable_run_ledger() -> Result<()> {
    let codex_home = TempDir::new()?;
    let workspace = TempDir::new()?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args([
        "clearloop",
        "run",
        "--id",
        "run-demo",
        "--task",
        "Fix startup failure",
        "--thinking-program",
        "tp-demo",
        "--reasoning-mode",
        "hybrid",
        "-C",
    ])
    .arg(workspace.path())
    .assert()
    .success()
    .stdout(contains("Run ledger created:"));

    let run_dir = workspace.path().join(".bestqa/agent-runs/run-demo");
    let manifest = read_json(run_dir.join("manifest.json").as_path())?;
    assert_eq!(manifest["run_id"].as_str(), Some("run-demo"));
    assert_eq!(manifest["reasoning_mode"].as_str(), Some("hybrid"));
    assert_eq!(manifest["thinking_program_ref"].as_str(), Some("tp-demo"));
    assert!(run_dir.join("evidence.jsonl").exists());
    assert!(run_dir.join("commands.jsonl").exists());
    assert!(run_dir.join("model-visible-output.jsonl").exists());
    assert!(run_dir.join("explicit-reasoning.jsonl").exists());
    assert!(run_dir.join("tool-events.jsonl").exists());
    assert!(run_dir.join("decisions.jsonl").exists());
    assert!(
        fs::read_to_string(run_dir.join("explicit-reasoning.jsonl"))?
            .contains("Run ledger initialized")
    );

    Ok(())
}

#[test]
fn clearloop_remember_writes_draft_experience_only() -> Result<()> {
    let codex_home = TempDir::new()?;
    let workspace = TempDir::new()?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args([
        "clearloop",
        "remember",
        "--id",
        "exp-demo",
        "--run-id",
        "run-demo",
        "--problem-model",
        "pm-startup",
        "--target-condition",
        "server_ready",
        "--claim",
        "Server readiness depends on an observable ready notification.",
        "-C",
    ])
    .arg(workspace.path())
    .assert()
    .success()
    .stdout(contains("Memory gate: draft only, not promoted"));

    let experience = read_json(
        workspace
            .path()
            .join(".bestqa/experiences/exp-demo.json")
            .as_path(),
    )?;
    assert_eq!(experience["id"].as_str(), Some("exp-demo"));
    assert_eq!(experience["source_session"].as_str(), Some("run-demo"));
    assert_eq!(experience["problem_model_ref"].as_str(), Some("pm-startup"));
    assert_eq!(
        experience["verification_result"]["passed"].as_bool(),
        Some(false)
    );
    assert_eq!(
        experience["reusable_update"]["claim"].as_str(),
        Some("Server readiness depends on an observable ready notification.")
    );

    Ok(())
}

fn read_json(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}
