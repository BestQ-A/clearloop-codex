use crate::SCHEMA_VERSION;
use crate::maturity::ReasoningMode;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

pub const MANIFEST_FILE: &str = "manifest.json";
pub const CHANGES_FILE: &str = "changes.json";
pub const VERIFICATION_FILE: &str = "verification.md";
pub const RESULT_FILE: &str = "result.md";
pub const EVIDENCE_STREAM: &str = "evidence.jsonl";
pub const COMMAND_STREAM: &str = "commands.jsonl";
pub const MODEL_VISIBLE_OUTPUT_STREAM: &str = "model-visible-output.jsonl";
pub const EXPLICIT_REASONING_STREAM: &str = "explicit-reasoning.jsonl";
pub const TOOL_EVENT_STREAM: &str = "tool-events.jsonl";
pub const DECISION_STREAM: &str = "decisions.jsonl";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    #[default]
    WaitingForExecution,
    Running,
    WaitingForReview,
    Verified,
    FailedVerification,
    PromotedToMemory,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryGateDecision {
    #[default]
    Pending,
    CandidateOnly,
    Accepted,
    Rejected,
    Blocked,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct MemoryGate {
    pub decision: MemoryGateDecision,
    pub reviewer: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct RunManifest {
    pub schema_version: String,
    pub run_id: String,
    pub task: String,
    pub workspace_root: String,
    pub status: RunStatus,
    pub reasoning_mode: ReasoningMode,
    pub problem_model_ref: Option<String>,
    pub thinking_program_ref: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub memory_gate: MemoryGate,
}

impl Default for RunManifest {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            run_id: String::new(),
            task: String::new(),
            workspace_root: String::new(),
            status: RunStatus::WaitingForExecution,
            reasoning_mode: ReasoningMode::LlmPrimary,
            problem_model_ref: None,
            thinking_program_ref: None,
            created_at: now,
            updated_at: now,
            memory_gate: MemoryGate::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct StreamRecord {
    pub at: DateTime<Utc>,
    pub source: String,
    pub summary: String,
    pub payload: serde_json::Value,
}

impl StreamRecord {
    pub fn new(source: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            at: Utc::now(),
            source: source.into(),
            summary: summary.into(),
            payload: serde_json::Value::Null,
        }
    }
}

impl Default for StreamRecord {
    fn default() -> Self {
        Self::new("", "")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event_type", content = "record", rename_all = "snake_case")]
pub enum LedgerEvent {
    Evidence(StreamRecord),
    Command(StreamRecord),
    ModelVisibleOutput(StreamRecord),
    ExplicitReasoning(StreamRecord),
    ToolEvent(StreamRecord),
    Decision(StreamRecord),
}

impl LedgerEvent {
    pub fn stream_file(&self) -> &'static str {
        match self {
            Self::Evidence(_) => EVIDENCE_STREAM,
            Self::Command(_) => COMMAND_STREAM,
            Self::ModelVisibleOutput(_) => MODEL_VISIBLE_OUTPUT_STREAM,
            Self::ExplicitReasoning(_) => EXPLICIT_REASONING_STREAM,
            Self::ToolEvent(_) => TOOL_EVENT_STREAM,
            Self::Decision(_) => DECISION_STREAM,
        }
    }
}
