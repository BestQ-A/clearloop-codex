use std::env;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;
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
use codex_clearloop_core::MemoryUpdate;
use codex_clearloop_core::ReasoningMode;
use codex_clearloop_core::RunManifest;
use codex_clearloop_core::StreamRecord;
use codex_clearloop_core::ThinkingProgram;
use codex_clearloop_core::VerificationResult;
use codex_clearloop_core::map_codex_exec_event_with_source;

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
    let mut report = EventBridgeReport::default();

    for (index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!(
                "failed to read JSONL line {} from {}",
                index + 1,
                args.jsonl.display()
            )
        })?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let payload = serde_json::from_str(line).with_context(|| {
            format!(
                "failed to parse JSONL line {} from {}",
                index + 1,
                args.jsonl.display()
            )
        })?;
        let event = map_codex_exec_event_with_source(payload, args.source.as_str());
        store
            .append_event(&args.run_id, &event)
            .with_context(|| format!("failed to append event for run {}", args.run_id))?;
        report.record(&event);
    }

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

fn run_remember(args: RememberArgs) -> anyhow::Result<()> {
    let workspace_root = workspace_root(args.cwd.as_deref())?;
    let store = ClearLoopStore::new(workspace_root);
    let id = args
        .id
        .unwrap_or_else(|| generated_id("exp", args.run_id.as_str()));
    let target_condition = goal_condition(args.target_condition.as_str());
    let experience = Experience {
        id,
        source_session: args.run_id,
        problem_model_ref: args.problem_model,
        target_condition,
        verification_result: VerificationResult {
            rule_ref: "manual-verification-required".to_string(),
            passed: false,
            evidence_ref: None,
            summary: "Draft experience only. It is not promoted memory until verification and review pass.".to_string(),
        },
        reusable_update: Some(MemoryUpdate {
            claim: args.claim,
            applicability_conditions: Vec::new(),
            evidence_refs: Vec::new(),
        }),
        ..Experience::default()
    };

    let path = store
        .save_experience(&experience)
        .context("failed to write draft experience")?;
    println!("Draft experience written: {}", path.display());
    println!("Draft experience id: {}", experience.id);
    println!("Memory gate: draft only, not promoted");
    Ok(())
}

fn workspace_root(cwd: Option<&Path>) -> anyhow::Result<PathBuf> {
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
