use std::path::Path;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::process::Output;

use anyhow::Context;
use anyhow::bail;
use clap::Parser;
use codex_clearloop_core::ClearLoopStore;
use codex_clearloop_core::LedgerEvent;
use codex_clearloop_core::RunStatus;
use codex_clearloop_core::StreamRecord;

use crate::clearloop_cmd::workspace_root;

#[derive(Debug, Parser)]
#[command(
    bin_name = "codex clearloop verify",
    after_help = "Examples:\n  codex clearloop verify --run-id run-123 --command \"cargo test -p codex-clearloop-core\" -C ."
)]
pub struct VerifyArgs {
    /// Workspace root where `.bestqa` should be read and written.
    #[arg(short = 'C', long = "cd", value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Existing ClearLoop run id whose result should be verified.
    #[arg(long = "run-id", value_name = "RUN_ID")]
    pub run_id: String,

    /// Verification command to run in the workspace root.
    #[arg(long = "command", value_name = "COMMAND")]
    pub command: String,
}

pub(crate) fn run_verify(args: VerifyArgs) -> anyhow::Result<()> {
    let workspace_root = workspace_root(args.cwd.as_deref())?;
    let store = ClearLoopStore::new(&workspace_root);
    let manifest_path = store.run_manifest_path(&args.run_id)?;
    if !manifest_path.exists() {
        bail!(
            "run ledger does not exist for '{}'; run `codex clearloop execute --id {} --task ... -C {}` first",
            args.run_id,
            args.run_id,
            workspace_root.display()
        );
    }

    let output = run_verification_command(args.command.as_str(), &workspace_root)?;
    let verification_markdown = verification_markdown(args.command.as_str(), &output);
    let verification_path = store
        .write_run_verification(&args.run_id, verification_markdown.as_str())
        .context("failed to write verification artifact")?;
    let status = if output.status.success() {
        RunStatus::Verified
    } else {
        RunStatus::FailedVerification
    };
    store
        .update_run_status(&args.run_id, status)
        .context("failed to update run verification status")?;

    let mut command_record = StreamRecord::new(
        "clearloop-cli",
        format!("Verification command exited with status {}.", output.status),
    );
    command_record.payload = serde_json::json!({
        "command": args.command.as_str(),
        "exit_code": output.status.code(),
    });
    store
        .append_event(&args.run_id, &LedgerEvent::Command(command_record))
        .context("failed to append verification command event")?;

    let mut evidence_record = StreamRecord::new(
        "clearloop-cli",
        if output.status.success() {
            "Verification passed."
        } else {
            "Verification failed."
        },
    );
    evidence_record.payload = serde_json::json!({
        "exit_code": output.status.code(),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
    });
    store
        .append_event(&args.run_id, &LedgerEvent::Evidence(evidence_record))
        .context("failed to append verification evidence event")?;

    println!("Verification written: {}", verification_path.display());
    println!(
        "Verification status: {}",
        if output.status.success() {
            "verified"
        } else {
            "failed_verification"
        }
    );

    if !output.status.success() {
        bail!("verification command failed with status {}", output.status);
    }

    Ok(())
}

fn run_verification_command(command: &str, workspace_root: &Path) -> anyhow::Result<Output> {
    let mut process = if cfg!(windows) {
        let mut process = ProcessCommand::new("cmd");
        process.arg("/C").arg(command);
        process
    } else {
        let mut process = ProcessCommand::new("sh");
        process.arg("-lc").arg(command);
        process
    };
    process.current_dir(workspace_root);
    process
        .output()
        .with_context(|| format!("failed to run verification command `{command}`"))
}

fn verification_markdown(command: &str, output: &Output) -> String {
    let status = if output.status.success() {
        "passed"
    } else {
        "failed"
    };
    let exit_code = output
        .status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    format!(
        "# Verification\n\nStatus: {status}\n\nCommand:\n```text\n{command}\n```\n\nExit code: {exit_code}\n\nStdout:\n```text\n{stdout}\n```\n\nStderr:\n```text\n{stderr}\n```\n"
    )
}
