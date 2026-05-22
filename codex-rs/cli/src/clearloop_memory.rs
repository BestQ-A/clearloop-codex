use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::process::Output;

use anyhow::Context;
use anyhow::bail;
use clap::Parser;
use codex_clearloop_core::ClearLoopStore;
use codex_clearloop_core::LedgerEvent;
use codex_clearloop_core::MemoryGateDecision;
use codex_clearloop_core::RunStatus;
use codex_clearloop_core::SCHEMA_VERSION;
use codex_clearloop_core::StreamRecord;
use codex_clearloop_core::final_agent_message_from_codex_exec_jsonl;
use serde::Deserialize;

use crate::clearloop_cmd::workspace_root;

const REVIEW_DIR: &str = "memory-reviews";

#[derive(Debug, Parser)]
#[command(
    bin_name = "codex clearloop review",
    after_help = "Examples:\n  codex clearloop review --run-id run-123 --experience-id exp-123 --model gpt-5.5 -C ."
)]
pub struct ReviewArgs {
    /// Workspace root where `.bestqa` should be read and written.
    #[arg(short = 'C', long = "cd", value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Verified source run id whose candidate should be reviewed.
    #[arg(long = "run-id", value_name = "RUN_ID")]
    pub run_id: String,

    /// Candidate experience id to review.
    #[arg(long = "experience-id", value_name = "ID")]
    pub experience_id: String,

    /// Optional model forwarded to `codex exec --model`.
    #[arg(long = "model", value_name = "MODEL")]
    pub model: Option<String>,

    /// Optional `-c key=value` override forwarded to `codex exec`.
    #[arg(long = "exec-config", value_name = "KEY=VALUE")]
    pub config_overrides: Vec<String>,

    /// Additional raw argument forwarded to `codex exec` before the prompt.
    #[arg(long = "exec-arg", value_name = "ARG")]
    pub exec_args: Vec<String>,

    /// Codex binary to execute. Defaults to the current executable.
    #[arg(long = "codex-bin", value_name = "FILE")]
    pub codex_bin: Option<PathBuf>,
}

#[derive(Debug, Parser)]
#[command(
    bin_name = "codex clearloop promote",
    after_help = "Examples:\n  codex clearloop promote --run-id run-123 --experience-id exp-123 -C ."
)]
pub struct PromoteArgs {
    /// Workspace root where `.bestqa` should be read and written.
    #[arg(short = 'C', long = "cd", value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Source run id whose accepted candidate should be promoted.
    #[arg(long = "run-id", value_name = "RUN_ID")]
    pub run_id: String,

    /// Candidate experience id to promote.
    #[arg(long = "experience-id", value_name = "ID")]
    pub experience_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ModelReview {
    decision: ModelReviewDecision,
    reviewer: Option<String>,
    note: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ModelReviewDecision {
    Accepted,
    Rejected,
}

impl ModelReviewDecision {
    fn as_memory_gate(&self) -> MemoryGateDecision {
        match self {
            Self::Accepted => MemoryGateDecision::Accepted,
            Self::Rejected => MemoryGateDecision::Rejected,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }
}

pub(crate) fn run_review(args: ReviewArgs) -> anyhow::Result<()> {
    let workspace_root = workspace_root(args.cwd.as_deref())?;
    let store = ClearLoopStore::new(&workspace_root);
    let mut manifest = store
        .load_run_manifest(&args.run_id)
        .with_context(|| format!("failed to load source run manifest for {}", args.run_id))?;
    if manifest.status != RunStatus::Verified {
        bail!(
            "review requires source run '{}' to be verified; current status is {:?}",
            args.run_id,
            manifest.status
        );
    }
    if manifest.memory_gate.decision != MemoryGateDecision::CandidateOnly {
        bail!(
            "review requires memory gate candidate_only for run '{}'; current decision is {:?}",
            args.run_id,
            manifest.memory_gate.decision
        );
    }

    let experience = store
        .load_experience(&args.experience_id)
        .with_context(|| format!("failed to load candidate experience {}", args.experience_id))?;
    if experience.source_session != args.run_id {
        bail!(
            "experience '{}' belongs to source session '{}', not '{}'",
            args.experience_id,
            experience.source_session,
            args.run_id
        );
    }

    let prompt = review_prompt(
        fs::read_to_string(store.run_manifest_path(&args.run_id)?)?.as_str(),
        fs::read_to_string(store.experience_path(&args.experience_id)?)?.as_str(),
        fs::read_to_string(store.run_dir(&args.run_id)?.join("verification.md"))?.as_str(),
    );
    let review_input_path =
        write_review_input(&store, &args.run_id, &args.experience_id, prompt.as_str())?;
    let review_input_ref = review_input_path
        .strip_prefix(&workspace_root)
        .unwrap_or(review_input_path.as_path())
        .display()
        .to_string();
    let review_prompt = format!(
        "Review the ClearLoop memory candidate in {review_input_ref}. Return only JSON with decision, reviewer, and note."
    );
    let output = run_codex_review(&args, &workspace_root, review_prompt.as_str())?;
    if !output.status.success() {
        bail!("review Codex exec failed with status {}", output.status);
    }
    let raw_jsonl = String::from_utf8_lossy(&output.stdout);
    let model_message = final_agent_message_from_codex_exec_jsonl(raw_jsonl.as_ref())
        .context("failed to parse review Codex JSONL")?
        .unwrap_or_else(|| raw_jsonl.to_string());
    let review = parse_model_review(model_message.as_str())?;
    let reviewer = review.reviewer.clone().unwrap_or_else(|| {
        args.model
            .clone()
            .unwrap_or_else(|| "codex-model-reviewer".to_string())
    });
    let review_path = write_review_artifact(
        &store,
        &args.run_id,
        &args.experience_id,
        &review,
        reviewer.as_str(),
        model_message.as_str(),
        raw_jsonl.as_ref(),
    )?;

    manifest.memory_gate.decision = review.decision.as_memory_gate();
    manifest.memory_gate.reviewer = Some(reviewer.clone());
    manifest.memory_gate.note = Some(review.note.clone());
    store
        .save_run_manifest(&manifest)
        .context("failed to update source run memory gate")?;

    let mut record = StreamRecord::new(
        "clearloop-cli",
        format!(
            "Memory candidate {} by model reviewer.",
            review.decision.as_str()
        ),
    );
    record.payload = serde_json::json!({
        "experience_id": args.experience_id.as_str(),
        "decision": review.decision.as_str(),
        "reviewer": reviewer.as_str(),
        "note": review.note.as_str(),
        "review_ref": review_path.display().to_string(),
    });
    store
        .append_event(&args.run_id, &LedgerEvent::Decision(record))
        .context("failed to append memory review decision")?;

    println!("Review artifact: {}", review_path.display());
    println!("Review decision: {}", review.decision.as_str());
    Ok(())
}

pub(crate) fn run_promote(args: PromoteArgs) -> anyhow::Result<()> {
    let workspace_root = workspace_root(args.cwd.as_deref())?;
    let store = ClearLoopStore::new(&workspace_root);
    let manifest = store
        .load_run_manifest(&args.run_id)
        .with_context(|| format!("failed to load source run manifest for {}", args.run_id))?;
    if manifest.memory_gate.decision != MemoryGateDecision::Accepted {
        bail!(
            "promote requires accepted memory review for run '{}'; current decision is {:?}",
            args.run_id,
            manifest.memory_gate.decision
        );
    }

    let experience = store
        .load_experience(&args.experience_id)
        .with_context(|| format!("failed to load candidate experience {}", args.experience_id))?;
    if experience.source_session != args.run_id {
        bail!(
            "experience '{}' belongs to source session '{}', not '{}'",
            args.experience_id,
            experience.source_session,
            args.run_id
        );
    }
    if !experience.verification_result.passed {
        bail!(
            "promote requires candidate experience '{}' to carry passed verification",
            args.experience_id
        );
    }

    let promoted_path = store
        .save_promoted_memory(&experience)
        .context("failed to write promoted memory")?;
    store
        .update_run_status(&args.run_id, RunStatus::PromotedToMemory)
        .context("failed to update source run status")?;

    let mut record = StreamRecord::new("clearloop-cli", "Memory candidate promoted.");
    record.payload = serde_json::json!({
        "experience_id": args.experience_id.as_str(),
        "promoted_ref": promoted_path.display().to_string(),
    });
    store
        .append_event(&args.run_id, &LedgerEvent::Evidence(record))
        .context("failed to append memory promotion evidence")?;

    println!("Promoted memory written: {}", promoted_path.display());
    println!("Run status: promoted_to_memory");
    Ok(())
}

fn run_codex_review(
    args: &ReviewArgs,
    workspace_root: &Path,
    prompt: &str,
) -> anyhow::Result<Output> {
    let codex_bin = match args.codex_bin.as_ref() {
        Some(path) => path.clone(),
        None => env::current_exe().context("failed to resolve current Codex executable")?,
    };
    let mut command = ProcessCommand::new(&codex_bin);
    command
        .arg("exec")
        .arg("--json")
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(workspace_root)
        .current_dir(workspace_root);
    if let Some(model) = args.model.as_ref() {
        command.arg("--model").arg(model);
    }
    for config_override in &args.config_overrides {
        command.arg("-c").arg(config_override);
    }
    for exec_arg in &args.exec_args {
        command.arg(exec_arg);
    }
    command.arg(prompt);

    command
        .output()
        .with_context(|| format!("failed to run Codex executable {}", codex_bin.display()))
}

fn review_prompt(manifest: &str, experience: &str, verification: &str) -> String {
    format!(
        "Review this ClearLoop memory candidate for promotion. Do not expose private chain-of-thought. Return only a JSON object with keys decision, reviewer, and note. decision must be accepted or rejected.\n\nRun manifest:\n```json\n{manifest}\n```\n\nCandidate experience:\n```json\n{experience}\n```\n\nVerification evidence:\n```markdown\n{verification}\n```\n"
    )
}

fn write_review_input(
    store: &ClearLoopStore,
    run_id: &str,
    experience_id: &str,
    prompt: &str,
) -> anyhow::Result<PathBuf> {
    let dir = store.bestqa_root().join(REVIEW_DIR);
    fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let path = dir.join(format!("{run_id}-{experience_id}-input.md"));
    fs::write(&path, prompt).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn parse_model_review(message: &str) -> anyhow::Result<ModelReview> {
    let trimmed = message.trim();
    let json = if let Some(stripped) = trimmed.strip_prefix("```json") {
        stripped.trim().trim_end_matches("```").trim()
    } else if let Some(stripped) = trimmed.strip_prefix("```") {
        stripped.trim().trim_end_matches("```").trim()
    } else if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        &trimmed[start..=end]
    } else {
        trimmed
    };
    serde_json::from_str(json).context("review model did not return valid review JSON")
}

fn write_review_artifact(
    store: &ClearLoopStore,
    run_id: &str,
    experience_id: &str,
    review: &ModelReview,
    reviewer: &str,
    model_message: &str,
    raw_jsonl: &str,
) -> anyhow::Result<PathBuf> {
    let dir = store.bestqa_root().join(REVIEW_DIR);
    fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let path = dir.join(format!("{run_id}-{experience_id}.json"));
    let value = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "run_id": run_id,
        "experience_id": experience_id,
        "decision": review.decision.as_str(),
        "reviewer": reviewer,
        "note": review.note.as_str(),
        "model_message": model_message,
        "raw_codex_jsonl": raw_jsonl,
    });
    let text = serde_json::to_string_pretty(&value).context("failed to serialize review")?;
    fs::write(&path, format!("{text}\n"))
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}
