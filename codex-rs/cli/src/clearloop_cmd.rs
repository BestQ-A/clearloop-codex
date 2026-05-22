use std::env;
use std::fs;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Cursor;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::process::Output;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::bail;
use clap::Parser;
use codex_clearloop_core::Action;
use codex_clearloop_core::ClearLoopStore;
use codex_clearloop_core::Condition;
use codex_clearloop_core::ConditionKind;
use codex_clearloop_core::EventBridgeReport;
use codex_clearloop_core::Experience;
use codex_clearloop_core::GoalCondition;
use codex_clearloop_core::LedgerEvent;
use codex_clearloop_core::MANIFEST_FILE;
use codex_clearloop_core::MemoryGateDecision;
use codex_clearloop_core::MemoryUpdate;
use codex_clearloop_core::ReasoningMode;
use codex_clearloop_core::RunManifest;
use codex_clearloop_core::RunStatus;
use codex_clearloop_core::StreamRecord;
use codex_clearloop_core::ThinkingProgram;
use codex_clearloop_core::VERIFICATION_FILE;
use codex_clearloop_core::VerificationResult;
use codex_clearloop_core::final_agent_message_from_codex_exec_jsonl;
use codex_clearloop_core::map_codex_exec_event_with_source;

use crate::clearloop_verify::VerifyArgs;

#[derive(Debug, Parser)]
#[command(bin_name = "codex clearloop")]
pub struct ClearLoopCli {
    #[command(subcommand)]
    pub subcommand: ClearLoopSubcommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum ClearLoopSubcommand {
    /// Create a visible thinking program for a task.
    Think(ThinkArgs),

    /// Initialize a ClearLoop run ledger without executing an agent.
    Run(RunArgs),

    /// Ingest observable Codex JSONL events into an existing run ledger.
    Ingest(IngestArgs),

    /// Run Codex exec under ClearLoop and ingest its observable JSONL event stream.
    Execute(ExecuteArgs),

    /// Run a verification command and update the run ledger status.
    Verify(VerifyArgs),

    /// Write a draft reusable experience from an observed run.
    Remember(RememberArgs),
}

#[derive(Debug, Parser)]
#[command(
    bin_name = "codex clearloop think",
    after_help = "Examples:\n  codex clearloop think --task \"Fix failing startup\" -C .\n  codex clearloop think --task \"Fix failing startup\" --target-condition server_ready"
)]
pub struct ThinkArgs {
    /// Workspace root where `.bestqa` should be written.
    #[arg(short = 'C', long = "cd", value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Stable thinking program id. Defaults to a timestamped id.
    #[arg(long = "id", value_name = "ID")]
    pub id: Option<String>,

    /// User task to model.
    #[arg(long = "task", value_name = "TASK")]
    pub task: String,

    /// Existing problem model id, if this task belongs to a known problem type.
    #[arg(long = "problem-model", value_name = "ID")]
    pub problem_model: Option<String>,

    /// Target condition id to make explicit in the thinking program.
    #[arg(long = "target-condition", value_name = "CONDITION")]
    pub target_condition: Option<String>,

    /// Observed condition to include. Can be repeated.
    #[arg(long = "observed-condition", value_name = "CONDITION")]
    pub observed_conditions: Vec<String>,
}

#[derive(Debug, Parser)]
#[command(
    bin_name = "codex clearloop run",
    after_help = "Examples:\n  codex clearloop run --task \"Fix failing startup\" --thinking-program tp-123 -C ."
)]
pub struct RunArgs {
    /// Workspace root where `.bestqa` should be written.
    #[arg(short = 'C', long = "cd", value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Stable run id. Defaults to a timestamped id.
    #[arg(long = "id", value_name = "ID")]
    pub id: Option<String>,

    /// User task represented by this run ledger.
    #[arg(long = "task", value_name = "TASK")]
    pub task: String,

    /// Thinking program id used for this run.
    #[arg(long = "thinking-program", value_name = "ID")]
    pub thinking_program: Option<String>,

    /// Existing problem model id, if this run belongs to a known problem type.
    #[arg(long = "problem-model", value_name = "ID")]
    pub problem_model: Option<String>,

    /// Reasoning mode for this run: llm-primary, hybrid, or db-primary.
    #[arg(
        long = "reasoning-mode",
        value_name = "MODE",
        default_value = "llm-primary"
    )]
    pub reasoning_mode: ReasoningModeArg,
}

#[derive(Debug, Parser)]
#[command(
    bin_name = "codex clearloop ingest",
    after_help = "Examples:\n  codex exec --json \"Fix failing startup\" > codex-events.jsonl\n  codex clearloop ingest --run-id run-123 --jsonl codex-events.jsonl -C ."
)]
pub struct IngestArgs {
    /// Workspace root where `.bestqa` should be written.
    #[arg(short = 'C', long = "cd", value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Existing ClearLoop run id whose ledger receives these events.
    #[arg(long = "run-id", value_name = "RUN_ID")]
    pub run_id: String,

    /// JSONL file emitted by `codex exec --json`.
    #[arg(long = "jsonl", value_name = "FILE")]
    pub jsonl: PathBuf,

    /// Source label written into each stream record.
    #[arg(
        long = "source",
        value_name = "SOURCE",
        default_value = "codex-exec-json"
    )]
    pub source: String,
}

#[derive(Debug, Parser)]
#[command(
    bin_name = "codex clearloop execute",
    after_help = "Examples:\n  codex clearloop execute --task \"Fix failing startup\" -C .\n  codex clearloop execute --id run-123 --task \"Fix failing startup\" --model gpt-5.5 -C ."
)]
pub struct ExecuteArgs {
    /// Workspace root where `.bestqa` should be written and Codex exec should run.
    #[arg(short = 'C', long = "cd", value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Stable run id. Defaults to a timestamped id.
    #[arg(long = "id", value_name = "ID")]
    pub id: Option<String>,

    /// User task to send to `codex exec`.
    #[arg(long = "task", value_name = "TASK")]
    pub task: String,

    /// Thinking program id used for this run.
    #[arg(long = "thinking-program", value_name = "ID")]
    pub thinking_program: Option<String>,

    /// Existing problem model id, if this run belongs to a known problem type.
    #[arg(long = "problem-model", value_name = "ID")]
    pub problem_model: Option<String>,

    /// Reasoning mode for this run: llm-primary, hybrid, or db-primary.
    #[arg(
        long = "reasoning-mode",
        value_name = "MODE",
        default_value = "llm-primary"
    )]
    pub reasoning_mode: ReasoningModeArg,

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
    bin_name = "codex clearloop remember",
    after_help = "Examples:\n  codex clearloop remember --run-id run-123 --problem-model pm-startup --target-condition server_ready --claim \"startup fails when the ready notification is absent\""
)]
pub struct RememberArgs {
    /// Workspace root where `.bestqa` should be written.
    #[arg(short = 'C', long = "cd", value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Stable experience id. Defaults to a timestamped id.
    #[arg(long = "id", value_name = "ID")]
    pub id: Option<String>,

    /// Source run id that produced this draft experience.
    #[arg(long = "run-id", value_name = "RUN_ID")]
    pub run_id: String,

    /// Problem model id this draft experience belongs to.
    #[arg(long = "problem-model", value_name = "ID")]
    pub problem_model: String,

    /// Target condition id whose desired state was being pursued.
    #[arg(long = "target-condition", value_name = "CONDITION")]
    pub target_condition: String,

    /// Reusable claim proposed by the run.
    #[arg(long = "claim", value_name = "CLAIM")]
    pub claim: String,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ReasoningModeArg {
    LlmPrimary,
    Hybrid,
    DbPrimary,
}

impl From<ReasoningModeArg> for ReasoningMode {
    fn from(value: ReasoningModeArg) -> Self {
        match value {
            ReasoningModeArg::LlmPrimary => Self::LlmPrimary,
            ReasoningModeArg::Hybrid => Self::Hybrid,
            ReasoningModeArg::DbPrimary => Self::DbPrimary,
        }
    }
}

impl ClearLoopCli {
    pub fn run(self) -> anyhow::Result<()> {
        match self.subcommand {
            ClearLoopSubcommand::Think(args) => run_think(args),
            ClearLoopSubcommand::Run(args) => run_run(args),
            ClearLoopSubcommand::Ingest(args) => run_ingest(args),
            ClearLoopSubcommand::Execute(args) => run_execute(args),
            ClearLoopSubcommand::Verify(args) => crate::clearloop_verify::run_verify(args),
            ClearLoopSubcommand::Remember(args) => run_remember(args),
        }
    }
}

fn run_think(args: ThinkArgs) -> anyhow::Result<()> {
    let workspace_root = workspace_root(args.cwd.as_deref())?;
    let store = ClearLoopStore::new(&workspace_root);
    let id = args
        .id
        .unwrap_or_else(|| generated_id("tp", args.task.as_str()));

    let target_condition = args
        .target_condition
        .as_ref()
        .map(|condition| goal_condition(condition));
    let current_conditions = args
        .observed_conditions
        .iter()
        .map(|condition| observed_condition(condition))
        .collect::<Vec<_>>();

    let program = ThinkingProgram {
        id,
        user_task: args.task,
        problem_model_ref: args.problem_model,
        current_conditions,
        target_condition,
        planned_actions: vec![Action {
            id: "define-problem-model".to_string(),
            description: "Make conditions, relations, constraints, and verification explicit."
                .to_string(),
            command_ref: None,
            expected_effect: "A visible thinking program exists before execution.".to_string(),
        }],
        ..ThinkingProgram::default()
    };

    let path = store
        .save_thinking_program(&program)
        .context("failed to write thinking program")?;
    println!("Thinking program written: {}", path.display());
    println!("Thinking program id: {}", program.id);
    Ok(())
}

fn run_run(args: RunArgs) -> anyhow::Result<()> {
    let workspace_root = workspace_root(args.cwd.as_deref())?;
    let store = ClearLoopStore::new(&workspace_root);
    let run_id = args
        .id
        .unwrap_or_else(|| generated_id("run", args.task.as_str()));
    let reasoning_mode = ReasoningMode::from(args.reasoning_mode);

    let manifest = RunManifest {
        run_id,
        task: args.task,
        workspace_root: workspace_root.display().to_string(),
        reasoning_mode,
        problem_model_ref: args.problem_model,
        thinking_program_ref: args.thinking_program,
        ..RunManifest::default()
    };
    let run_dir = store
        .create_run_ledger(&manifest)
        .context("failed to initialize run ledger")?;
    store
        .append_event(
            &manifest.run_id,
            &LedgerEvent::ExplicitReasoning(StreamRecord::new(
                "clearloop-cli",
                "Run ledger initialized; no agent execution has happened yet.",
            )),
        )
        .context("failed to append initial explicit reasoning event")?;

    println!("Run ledger created: {}", run_dir.display());
    println!("Run id: {}", manifest.run_id);
    Ok(())
}

fn run_ingest(args: IngestArgs) -> anyhow::Result<()> {
    let workspace_root = workspace_root(args.cwd.as_deref())?;
    let store = ClearLoopStore::new(&workspace_root);
    let run_dir = store.run_dir(&args.run_id)?;
    let manifest_path = run_dir.join(MANIFEST_FILE);
    if !manifest_path.exists() {
        bail!(
            "run ledger does not exist for '{}'; run `codex clearloop run --id {} --task ... -C {}` first",
            args.run_id,
            args.run_id,
            workspace_root.display()
        );
    }

    let file = File::open(&args.jsonl)
        .with_context(|| format!("failed to open JSONL file {}", args.jsonl.display()))?;
    let reader = BufReader::new(file);
    let report = ingest_codex_exec_jsonl_reader(
        &store,
        args.run_id.as_str(),
        args.source.as_str(),
        reader,
        args.jsonl.display().to_string().as_str(),
    )?;

    println!("Events ingested: {}", report.events_ingested);
    println!("Run ledger updated: {}", run_dir.display());
    println!(
        "Streams: evidence={}, commands={}, model_visible_output={}, explicit_reasoning={}, tools={}, decisions={}",
        report.evidence_events,
        report.command_events,
        report.model_visible_output_events,
        report.explicit_reasoning_events,
        report.tool_events,
        report.decision_events
    );
    Ok(())
}

fn run_execute(args: ExecuteArgs) -> anyhow::Result<()> {
    let workspace_root = workspace_root(args.cwd.as_deref())?;
    let store = ClearLoopStore::new(&workspace_root);
    let run_id = args
        .id
        .clone()
        .unwrap_or_else(|| generated_id("run", args.task.as_str()));
    let reasoning_mode = ReasoningMode::from(args.reasoning_mode);

    let mut manifest = RunManifest {
        run_id,
        task: args.task.clone(),
        workspace_root: workspace_root.display().to_string(),
        status: RunStatus::Running,
        reasoning_mode,
        problem_model_ref: args.problem_model.clone(),
        thinking_program_ref: args.thinking_program.clone(),
        ..RunManifest::default()
    };
    let run_dir = store
        .create_run_ledger(&manifest)
        .context("failed to initialize controlled run ledger")?;
    store
        .append_event(
            &manifest.run_id,
            &LedgerEvent::ExplicitReasoning(StreamRecord::new(
                "clearloop-cli",
                "Controlled Codex exec started; JSONL output will be captured and ingested.",
            )),
        )
        .context("failed to append controlled run start event")?;

    let output = run_codex_exec(&args, &workspace_root)?;
    let stdout_text = String::from_utf8_lossy(&output.stdout).to_string();
    let raw_events_path = store
        .write_codex_exec_events(&manifest.run_id, &stdout_text)
        .context("failed to write raw Codex exec JSONL events")?;
    if let Some(final_message) = final_agent_message_from_codex_exec_jsonl(&stdout_text)
        .with_context(|| {
            format!(
                "failed to extract final Codex agent message from {}",
                raw_events_path.display()
            )
        })?
    {
        store
            .write_run_result(
                &manifest.run_id,
                &format!("# Result\n\n{}\n", final_message.trim()),
            )
            .context("failed to write controlled run result")?;
    }

    let stderr_text = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr_text.is_empty() {
        let mut record = StreamRecord::new("codex-exec", "Codex exec wrote stderr output.");
        record.payload = serde_json::json!({ "stderr": stderr_text });
        store
            .append_event(&manifest.run_id, &LedgerEvent::Evidence(record))
            .context("failed to append Codex exec stderr evidence")?;
    }

    let report_result = ingest_codex_exec_jsonl_reader(
        &store,
        manifest.run_id.as_str(),
        "codex-exec-json",
        Cursor::new(stdout_text.as_bytes()),
        raw_events_path.display().to_string().as_str(),
    );
    manifest.status = if output.status.success() && report_result.is_ok() {
        RunStatus::WaitingForReview
    } else {
        RunStatus::FailedExecution
    };
    store
        .save_run_manifest(&manifest)
        .context("failed to update controlled run manifest")?;
    let report = report_result?;

    println!("Controlled run ledger: {}", run_dir.display());
    println!("Raw Codex events: {}", raw_events_path.display());
    println!("Events ingested: {}", report.events_ingested);
    println!(
        "Streams: evidence={}, commands={}, model_visible_output={}, explicit_reasoning={}, tools={}, decisions={}",
        report.evidence_events,
        report.command_events,
        report.model_visible_output_events,
        report.explicit_reasoning_events,
        report.tool_events,
        report.decision_events
    );

    if !output.status.success() {
        bail!("controlled Codex exec failed with status {}", output.status);
    }

    Ok(())
}

fn run_remember(args: RememberArgs) -> anyhow::Result<()> {
    let workspace_root = workspace_root(args.cwd.as_deref())?;
    let store = ClearLoopStore::new(&workspace_root);
    let mut manifest = store
        .load_run_manifest(&args.run_id)
        .with_context(|| format!("failed to load source run manifest for {}", args.run_id))?;
    if manifest.status != RunStatus::Verified {
        bail!(
            "remember requires source run '{}' to be verified; current status is {:?}",
            args.run_id,
            manifest.status
        );
    }
    let verification_ref = format!(".bestqa/agent-runs/{}/{}", args.run_id, VERIFICATION_FILE);
    let verification_path = store.run_dir(&args.run_id)?.join(VERIFICATION_FILE);
    let verification_text = fs::read_to_string(&verification_path)
        .with_context(|| format!("failed to read {}", verification_path.display()))?;
    if verification_text.trim().is_empty() || verification_text.contains("Not run yet.") {
        bail!(
            "remember requires non-empty verification evidence for run '{}'",
            args.run_id
        );
    }

    let id = args
        .id
        .unwrap_or_else(|| generated_id("exp", args.run_id.as_str()));
    let target_condition = goal_condition(args.target_condition.as_str());
    let experience = Experience {
        id,
        source_session: args.run_id.clone(),
        problem_model_ref: args.problem_model,
        target_condition,
        verification_result: VerificationResult {
            rule_ref: "run-verification".to_string(),
            passed: true,
            evidence_ref: Some(verification_ref.clone()),
            summary:
                "Source run is verified. Candidate still requires human review before promotion."
                    .to_string(),
        },
        reusable_update: Some(MemoryUpdate {
            claim: args.claim,
            applicability_conditions: Vec::new(),
            evidence_refs: vec![verification_ref.clone()],
        }),
        ..Experience::default()
    };

    let path = store
        .save_experience(&experience)
        .context("failed to write draft experience")?;
    manifest.memory_gate.decision = MemoryGateDecision::CandidateOnly;
    store
        .save_run_manifest(&manifest)
        .context("failed to update source run memory gate")?;
    let mut decision_record = StreamRecord::new(
        "clearloop-cli",
        "Memory candidate created from verified run.",
    );
    decision_record.payload = serde_json::json!({
        "experience_id": experience.id.as_str(),
        "verification_ref": verification_ref,
        "memory_gate": "candidate_only",
    });
    store
        .append_event(&args.run_id, &LedgerEvent::Decision(decision_record))
        .context("failed to append memory candidate decision")?;

    println!("Draft experience written: {}", path.display());
    println!("Draft experience id: {}", experience.id);
    println!("Memory gate: candidate only, not promoted");
    Ok(())
}

fn ingest_codex_exec_jsonl_reader<R: BufRead>(
    store: &ClearLoopStore,
    run_id: &str,
    source: &str,
    reader: R,
    origin: &str,
) -> anyhow::Result<EventBridgeReport> {
    let mut report = EventBridgeReport::default();

    for (index, line) in reader.lines().enumerate() {
        let line =
            line.with_context(|| format!("failed to read JSONL line {} from {origin}", index + 1))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let payload = serde_json::from_str(line)
            .with_context(|| format!("failed to parse JSONL line {} from {origin}", index + 1))?;
        let event = map_codex_exec_event_with_source(payload, source);
        store
            .append_event(run_id, &event)
            .with_context(|| format!("failed to append event for run {run_id}"))?;
        report.record(&event);
    }

    Ok(report)
}

fn run_codex_exec(args: &ExecuteArgs, workspace_root: &Path) -> anyhow::Result<Output> {
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
    command.arg(&args.task);

    command
        .output()
        .with_context(|| format!("failed to run Codex executable {}", codex_bin.display()))
}

pub(crate) fn workspace_root(cwd: Option<&Path>) -> anyhow::Result<PathBuf> {
    match cwd {
        Some(cwd) => Ok(cwd.to_path_buf()),
        None => env::current_dir().context("failed to read current directory"),
    }
}

fn goal_condition(condition: &str) -> GoalCondition {
    GoalCondition {
        condition_ref: condition.to_string(),
        target_value: "true".to_string(),
        success_signal: format!("{condition} is satisfied"),
    }
}

fn observed_condition(condition: &str) -> Condition {
    Condition {
        id: id_fragment(condition),
        name: condition.to_string(),
        kind: ConditionKind::Observed,
        observed_value: Some("present".to_string()),
        ..Condition::default()
    }
}

fn generated_id(prefix: &str, seed: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("{prefix}-{millis}-{}", id_fragment(seed))
}

fn id_fragment(seed: &str) -> String {
    let mut fragment = String::new();
    let mut previous_dash = false;
    for ch in seed.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            fragment.push(ch);
            previous_dash = false;
        } else if !previous_dash && !fragment.is_empty() {
            fragment.push('-');
            previous_dash = true;
        }
    }
    let fragment = fragment.trim_matches('-');
    if fragment.is_empty() {
        "item".to_string()
    } else {
        fragment.chars().take(48).collect()
    }
}
