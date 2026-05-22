use crate::domain::Experience;
use crate::domain::ProblemModel;
use crate::domain::ThinkingProgram;
use crate::error::ClearLoopError;
use crate::error::Result;
use crate::ledger::CHANGES_FILE;
use crate::ledger::CODEX_EXEC_EVENTS_FILE;
use crate::ledger::COMMAND_STREAM;
use crate::ledger::DECISION_STREAM;
use crate::ledger::EVIDENCE_STREAM;
use crate::ledger::EXPLICIT_REASONING_STREAM;
use crate::ledger::LedgerEvent;
use crate::ledger::MANIFEST_FILE;
use crate::ledger::MODEL_VISIBLE_OUTPUT_STREAM;
use crate::ledger::RESULT_FILE;
use crate::ledger::RunManifest;
use crate::ledger::TOOL_EVENT_STREAM;
use crate::ledger::VERIFICATION_FILE;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ClearLoopStore {
    workspace_root: PathBuf,
}

impl ClearLoopStore {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn bestqa_root(&self) -> PathBuf {
        self.workspace_root.join(".bestqa")
    }

    pub fn models_dir(&self) -> PathBuf {
        self.bestqa_root().join("models")
    }

    pub fn thinking_programs_dir(&self) -> PathBuf {
        self.bestqa_root().join("thinking-programs")
    }

    pub fn experiences_dir(&self) -> PathBuf {
        self.bestqa_root().join("experiences")
    }

    pub fn agent_runs_dir(&self) -> PathBuf {
        self.bestqa_root().join("agent-runs")
    }

    pub fn problem_model_path(&self, id: &str) -> Result<PathBuf> {
        Ok(self.models_dir().join(format!("{}.json", safe_id(id)?)))
    }

    pub fn thinking_program_path(&self, id: &str) -> Result<PathBuf> {
        Ok(self
            .thinking_programs_dir()
            .join(format!("{}.json", safe_id(id)?)))
    }

    pub fn experience_path(&self, id: &str) -> Result<PathBuf> {
        Ok(self
            .experiences_dir()
            .join(format!("{}.json", safe_id(id)?)))
    }

    pub fn run_dir(&self, run_id: &str) -> Result<PathBuf> {
        Ok(self.agent_runs_dir().join(safe_id(run_id)?))
    }

    pub fn run_manifest_path(&self, run_id: &str) -> Result<PathBuf> {
        Ok(self.run_dir(run_id)?.join(MANIFEST_FILE))
    }

    pub fn codex_exec_events_path(&self, run_id: &str) -> Result<PathBuf> {
        Ok(self.run_dir(run_id)?.join(CODEX_EXEC_EVENTS_FILE))
    }

    pub fn save_problem_model(&self, model: &ProblemModel) -> Result<PathBuf> {
        let path = self.problem_model_path(&model.id)?;
        write_json(&path, model)?;
        Ok(path)
    }

    pub fn load_problem_model(&self, id: &str) -> Result<ProblemModel> {
        read_json(&self.problem_model_path(id)?)
    }

    pub fn save_thinking_program(&self, program: &ThinkingProgram) -> Result<PathBuf> {
        let path = self.thinking_program_path(&program.id)?;
        write_json(&path, program)?;
        Ok(path)
    }

    pub fn load_thinking_program(&self, id: &str) -> Result<ThinkingProgram> {
        read_json(&self.thinking_program_path(id)?)
    }

    pub fn save_experience(&self, experience: &Experience) -> Result<PathBuf> {
        let path = self.experience_path(&experience.id)?;
        write_json(&path, experience)?;
        Ok(path)
    }

    pub fn load_experience(&self, id: &str) -> Result<Experience> {
        read_json(&self.experience_path(id)?)
    }

    pub fn create_run_ledger(&self, manifest: &RunManifest) -> Result<PathBuf> {
        let run_dir = self.run_dir(&manifest.run_id)?;
        create_dir(&run_dir)?;

        self.save_run_manifest(manifest)?;
        write_text(
            &run_dir.join(CHANGES_FILE),
            "{\n  \"changed_files\": []\n}\n",
        )?;
        write_text(
            &run_dir.join(VERIFICATION_FILE),
            "# Verification\n\nNot run yet.\n",
        )?;
        write_text(
            &run_dir.join(RESULT_FILE),
            "# Result\n\nNo result recorded yet.\n",
        )?;
        write_text(&run_dir.join(CODEX_EXEC_EVENTS_FILE), "")?;

        for stream in [
            EVIDENCE_STREAM,
            COMMAND_STREAM,
            MODEL_VISIBLE_OUTPUT_STREAM,
            EXPLICIT_REASONING_STREAM,
            TOOL_EVENT_STREAM,
            DECISION_STREAM,
        ] {
            write_text(&run_dir.join(stream), "")?;
        }

        Ok(run_dir)
    }

    pub fn save_run_manifest(&self, manifest: &RunManifest) -> Result<PathBuf> {
        let path = self.run_manifest_path(&manifest.run_id)?;
        write_json(&path, manifest)?;
        Ok(path)
    }

    pub fn load_run_manifest(&self, run_id: &str) -> Result<RunManifest> {
        read_json(&self.run_manifest_path(run_id)?)
    }

    pub fn write_codex_exec_events(&self, run_id: &str, text: &str) -> Result<PathBuf> {
        let path = self.codex_exec_events_path(run_id)?;
        write_text(&path, text)?;
        Ok(path)
    }

    pub fn append_event(&self, run_id: &str, event: &LedgerEvent) -> Result<PathBuf> {
        let run_dir = self.run_dir(run_id)?;
        create_dir(&run_dir)?;
        let path = run_dir.join(event.stream_file());
        append_jsonl(&path, event)?;
        Ok(path)
    }
}

fn safe_id(id: &str) -> Result<&str> {
    if id.is_empty()
        || id.contains("..")
        || id.contains('/')
        || id.contains('\\')
        || id.contains(':')
    {
        return Err(ClearLoopError::InvalidId(id.to_string()));
    }
    Ok(id)
}

fn create_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|source| ClearLoopError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|source| ClearLoopError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    write_text(path, &format!("{text}\n"))
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).map_err(|source| ClearLoopError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| ClearLoopError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    fs::write(path, text).map_err(|source| ClearLoopError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| ClearLoopError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let line = serde_json::to_string(value).map_err(|source| ClearLoopError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    writeln!(file, "{line}").map_err(|source| ClearLoopError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Condition;
    use crate::domain::ConditionKind;
    use crate::domain::GoalCondition;
    use crate::domain::ProblemModel;
    use crate::domain::ThinkingProgram;
    use crate::ledger::EXPLICIT_REASONING_STREAM;
    use crate::ledger::LedgerEvent;
    use crate::ledger::RunManifest;
    use crate::ledger::RunStatus;
    use crate::ledger::StreamRecord;
    use crate::maturity::ProblemModelMaturity;
    use crate::maturity::ReasoningMode;
    use crate::maturity::ReasoningVisibility;
    use pretty_assertions::assert_eq;

    #[test]
    fn problem_model_roundtrip_preserves_maturity() -> Result<()> {
        let temp = tempfile::tempdir().map_err(|source| ClearLoopError::Io {
            path: PathBuf::from("tempdir"),
            source,
        })?;
        let store = ClearLoopStore::new(temp.path());
        let model = ProblemModel {
            id: "pm-vscode-server-startup".to_string(),
            name: "VS Code extension backend startup".to_string(),
            goal_conditions: vec![GoalCondition {
                condition_ref: "server_ready_observed".to_string(),
                target_value: "true".to_string(),
                success_signal: "ready notification".to_string(),
            }],
            maturity: ProblemModelMaturity {
                repeated_occurrences: 8,
                verified_success_paths: 3,
                verified_failed_paths: 2,
                condition_coverage: 0.8,
                relation_consistency: 0.8,
                verification_pass_rate: 0.8,
            },
            ..ProblemModel::default()
        };

        store.save_problem_model(&model)?;
        let loaded = store.load_problem_model(&model.id)?;

        assert_eq!(loaded, model);
        assert_eq!(
            loaded.maturity.suggested_reasoning_mode(),
            ReasoningMode::DbPrimary
        );
        Ok(())
    }

    #[test]
    fn thinking_program_defaults_to_visible_llm_primary_for_new_problem() -> Result<()> {
        let temp = tempfile::tempdir().map_err(|source| ClearLoopError::Io {
            path: PathBuf::from("tempdir"),
            source,
        })?;
        let store = ClearLoopStore::new(temp.path());
        let program = ThinkingProgram {
            id: "tp-new-problem".to_string(),
            user_task: "Investigate a new failure mode".to_string(),
            current_conditions: vec![Condition {
                id: "unknown_error".to_string(),
                name: "Unknown error observed".to_string(),
                kind: ConditionKind::Observed,
                observed_value: Some("present".to_string()),
                ..Condition::default()
            }],
            ..ThinkingProgram::default()
        };

        store.save_thinking_program(&program)?;
        let loaded = store.load_thinking_program(&program.id)?;

        assert_eq!(loaded.reasoning_mode, ReasoningMode::LlmPrimary);
        assert_eq!(
            loaded.reasoning_visibility,
            ReasoningVisibility::VisibleByDefault
        );
        assert_eq!(loaded, program);
        Ok(())
    }

    #[test]
    fn run_ledger_creates_streams_and_routes_explicit_reasoning() -> Result<()> {
        let temp = tempfile::tempdir().map_err(|source| ClearLoopError::Io {
            path: PathBuf::from("tempdir"),
            source,
        })?;
        let store = ClearLoopStore::new(temp.path());
        let manifest = RunManifest {
            run_id: "run-001".to_string(),
            task: "record visible reasoning".to_string(),
            ..RunManifest::default()
        };

        let run_dir = store.create_run_ledger(&manifest)?;
        let event = LedgerEvent::ExplicitReasoning(StreamRecord::new(
            "agent",
            "A visible reasoning step was recorded",
        ));
        let event_path = store.append_event(&manifest.run_id, &event)?;

        assert_eq!(event_path, run_dir.join(EXPLICIT_REASONING_STREAM));
        assert!(run_dir.join(MANIFEST_FILE).exists());
        assert!(run_dir.join(CODEX_EXEC_EVENTS_FILE).exists());
        assert!(run_dir.join(MODEL_VISIBLE_OUTPUT_STREAM).exists());
        assert!(run_dir.join(TOOL_EVENT_STREAM).exists());
        assert!(run_dir.join(DECISION_STREAM).exists());
        Ok(())
    }

    #[test]
    fn run_manifest_and_raw_codex_events_roundtrip() -> Result<()> {
        let temp = tempfile::tempdir().map_err(|source| ClearLoopError::Io {
            path: PathBuf::from("tempdir"),
            source,
        })?;
        let store = ClearLoopStore::new(temp.path());
        let mut manifest = RunManifest {
            run_id: "run-raw-events".to_string(),
            task: "capture codex exec json".to_string(),
            ..RunManifest::default()
        };

        let run_dir = store.create_run_ledger(&manifest)?;
        manifest.status = RunStatus::WaitingForReview;
        store.save_run_manifest(&manifest)?;
        store.write_codex_exec_events(&manifest.run_id, "{\"type\":\"turn.started\"}\n")?;
        let raw_events_path = run_dir.join(CODEX_EXEC_EVENTS_FILE);
        let raw_events =
            fs::read_to_string(&raw_events_path).map_err(|source| ClearLoopError::Io {
                path: raw_events_path.clone(),
                source,
            })?;

        assert_eq!(store.load_run_manifest(&manifest.run_id)?, manifest);
        assert_eq!(raw_events, "{\"type\":\"turn.started\"}\n");
        Ok(())
    }
}
