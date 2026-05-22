use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use codex_clearloop_core::ClearLoopStore;
use codex_clearloop_core::Experience;
use codex_clearloop_core::SCHEMA_VERSION;
use serde::Serialize;

use crate::clearloop_cmd::generated_id;
use crate::clearloop_cmd::workspace_root;

#[derive(Debug, Parser)]
#[command(
    bin_name = "codex clearloop retrieve",
    after_help = "Examples:\n  codex clearloop retrieve --task \"Fix startup readiness failure\" -C .\n  codex clearloop retrieve --id ret-123 --task \"Fix startup readiness failure\" --limit 3 -C ."
)]
pub struct RetrieveArgs {
    /// Workspace root where `.bestqa` should be read and written.
    #[arg(short = 'C', long = "cd", value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Stable retrieval id. Defaults to a timestamped id.
    #[arg(long = "id", value_name = "ID")]
    pub id: Option<String>,

    /// New task that should retrieve relevant promoted memory.
    #[arg(long = "task", value_name = "TASK")]
    pub task: String,

    /// Maximum number of matches to write.
    #[arg(long = "limit", value_name = "N", default_value_t = 5)]
    pub limit: usize,

    /// Minimum deterministic token-overlap score required for a match.
    #[arg(long = "min-score", value_name = "N", default_value_t = 1)]
    pub min_score: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct RetrievalArtifact {
    schema_version: String,
    id: String,
    task: String,
    matches: Vec<RetrievalMatch>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct RetrievalMatch {
    experience_id: String,
    source_session: String,
    problem_model_ref: String,
    target_condition: String,
    score: usize,
    claim: Option<String>,
    evidence_refs: Vec<String>,
    memory_ref: String,
}

pub(crate) fn run_retrieve(args: RetrieveArgs) -> anyhow::Result<()> {
    let workspace_root = workspace_root(args.cwd.as_deref())?;
    let store = ClearLoopStore::new(&workspace_root);
    let retrieval_id = args
        .id
        .unwrap_or_else(|| generated_id("retrieval", args.task.as_str()));
    let task_tokens = tokens(args.task.as_str());
    let mut matches = promoted_memory_matches(&store, &task_tokens, args.min_score)
        .context("failed to retrieve promoted memory")?;

    matches.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.experience_id.cmp(&right.experience_id))
    });
    matches.truncate(args.limit);

    let artifact = RetrievalArtifact {
        schema_version: SCHEMA_VERSION.to_string(),
        id: retrieval_id.clone(),
        task: args.task,
        matches,
    };
    let retrieval_path = store.retrieval_path(&retrieval_id)?;
    if let Some(parent) = retrieval_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(&artifact).context("failed to serialize retrieval")?;
    fs::write(&retrieval_path, format!("{text}\n"))
        .with_context(|| format!("failed to write {}", retrieval_path.display()))?;

    println!("Retrieval artifact: {}", retrieval_path.display());
    println!("Matches: {}", artifact.matches.len());
    for candidate in &artifact.matches {
        println!(
            "- {} score={} problem_model={} target={}",
            candidate.experience_id,
            candidate.score,
            candidate.problem_model_ref,
            candidate.target_condition
        );
    }

    Ok(())
}

fn promoted_memory_matches(
    store: &ClearLoopStore,
    task_tokens: &BTreeSet<String>,
    min_score: usize,
) -> anyhow::Result<Vec<RetrievalMatch>> {
    let memory_dir = store.promoted_memory_dir();
    if !memory_dir.exists() {
        return Ok(Vec::new());
    }

    let mut memory_paths = Vec::new();
    for entry in fs::read_dir(&memory_dir)
        .with_context(|| format!("failed to read {}", memory_dir.display()))?
    {
        let path = entry
            .with_context(|| format!("failed to read entry in {}", memory_dir.display()))?
            .path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            memory_paths.push(path);
        }
    }
    memory_paths.sort();

    let mut matches = Vec::new();
    for path in memory_paths {
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let experience: Experience = serde_json::from_str(text.as_str())
            .with_context(|| format!("failed to parse promoted memory {}", path.display()))?;
        let score = score_experience(task_tokens, &experience);
        if score < min_score {
            continue;
        }
        let memory_ref = path
            .strip_prefix(store.workspace_root())
            .unwrap_or(path.as_path())
            .display()
            .to_string();
        let reusable_update = experience.reusable_update.as_ref();
        matches.push(RetrievalMatch {
            experience_id: experience.id,
            source_session: experience.source_session,
            problem_model_ref: experience.problem_model_ref,
            target_condition: experience.target_condition.condition_ref,
            score,
            claim: reusable_update.map(|update| update.claim.clone()),
            evidence_refs: reusable_update
                .map(|update| update.evidence_refs.clone())
                .unwrap_or_default(),
            memory_ref,
        });
    }

    Ok(matches)
}

fn score_experience(task_tokens: &BTreeSet<String>, experience: &Experience) -> usize {
    if task_tokens.is_empty() {
        return 0;
    }
    let memory_tokens = tokens(experience_text(experience).as_str());
    task_tokens.intersection(&memory_tokens).count()
}

fn experience_text(experience: &Experience) -> String {
    let mut text = format!(
        "{} {} {} {} {} {}",
        experience.id,
        experience.source_session,
        experience.problem_model_ref,
        experience.target_condition.condition_ref,
        experience.target_condition.target_value,
        experience.target_condition.success_signal
    );
    text.push(' ');
    text.push_str(experience.verification_result.summary.as_str());

    if let Some(update) = experience.reusable_update.as_ref() {
        text.push(' ');
        text.push_str(update.claim.as_str());
        for condition in &update.applicability_conditions {
            text.push(' ');
            text.push_str(condition.as_str());
        }
    }
    for condition in experience
        .initial_conditions
        .iter()
        .chain(experience.inferred_required_conditions.iter())
    {
        text.push(' ');
        text.push_str(condition.id.as_str());
        text.push(' ');
        text.push_str(condition.name.as_str());
        if let Some(value) = condition.observed_value.as_ref() {
            text.push(' ');
            text.push_str(value.as_str());
        }
        if let Some(value) = condition.target_value.as_ref() {
            text.push(' ');
            text.push_str(value.as_str());
        }
    }
    for action in &experience.actions_taken {
        text.push(' ');
        text.push_str(action.description.as_str());
        text.push(' ');
        text.push_str(action.expected_effect.as_str());
    }
    for observation in &experience.observations {
        text.push(' ');
        text.push_str(observation.source.as_str());
        text.push(' ');
        text.push_str(observation.summary.as_str());
    }

    text
}

fn tokens(text: &str) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    let mut current_ascii = String::new();

    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            current_ascii.push(ch);
        } else {
            if current_ascii.len() > 1 {
                tokens.insert(std::mem::take(&mut current_ascii));
            } else {
                current_ascii.clear();
            }
            if ch.is_alphanumeric() {
                tokens.insert(ch.to_string());
            }
        }
    }
    if current_ascii.len() > 1 {
        tokens.insert(current_ascii);
    }

    tokens
}
