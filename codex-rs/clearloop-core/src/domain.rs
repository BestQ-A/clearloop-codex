use crate::SCHEMA_VERSION;
use crate::maturity::ProblemModelMaturity;
use crate::maturity::ReasoningMode;
use crate::maturity::ReasoningVisibility;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionKind {
    Observed,
    Target,
    Required,
    Constraint,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Condition {
    pub id: String,
    pub name: String,
    pub kind: ConditionKind,
    pub observed_value: Option<String>,
    pub target_value: Option<String>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Relation {
    pub from_condition: String,
    pub to_condition: String,
    pub rule: String,
    pub confidence: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Constraint {
    pub id: String,
    pub rule: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Observable {
    pub id: String,
    pub source: String,
    pub inspection_method: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct GoalCondition {
    pub condition_ref: String,
    pub target_value: String,
    pub success_signal: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct VerificationRule {
    pub id: String,
    pub target_condition: String,
    pub operator: String,
    pub evidence_required: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct FailureBoundary {
    pub id: String,
    pub condition_set: Vec<String>,
    pub observed_failure: String,
    pub minimal_difference: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct SuccessBoundary {
    pub id: String,
    pub condition_set: Vec<String>,
    pub observed_success: String,
    pub minimal_difference: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct ProblemModel {
    pub schema_version: String,
    pub id: String,
    pub name: String,
    pub conditions: Vec<Condition>,
    pub relations: Vec<Relation>,
    pub constraints: Vec<Constraint>,
    pub observables: Vec<Observable>,
    pub goal_conditions: Vec<GoalCondition>,
    pub verification_rules: Vec<VerificationRule>,
    pub failure_boundaries: Vec<FailureBoundary>,
    pub maturity: ProblemModelMaturity,
}

impl Default for ProblemModel {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            id: String::new(),
            name: String::new(),
            conditions: Vec::new(),
            relations: Vec::new(),
            constraints: Vec::new(),
            observables: Vec::new(),
            goal_conditions: Vec::new(),
            verification_rules: Vec::new(),
            failure_boundaries: Vec::new(),
            maturity: ProblemModelMaturity::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Action {
    pub id: String,
    pub description: String,
    pub command_ref: Option<String>,
    pub expected_effect: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Observation {
    pub id: String,
    pub source: String,
    pub summary: String,
    pub evidence_ref: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct VerificationResult {
    pub rule_ref: String,
    pub passed: bool,
    pub evidence_ref: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct MemoryUpdate {
    pub claim: String,
    pub applicability_conditions: Vec<String>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Experience {
    pub schema_version: String,
    pub id: String,
    pub source_session: String,
    pub problem_model_ref: String,
    pub initial_conditions: Vec<Condition>,
    pub target_condition: GoalCondition,
    pub inferred_required_conditions: Vec<Condition>,
    pub actions_taken: Vec<Action>,
    pub observations: Vec<Observation>,
    pub verification_result: VerificationResult,
    pub success_boundary: Option<SuccessBoundary>,
    pub failure_boundary: Option<FailureBoundary>,
    pub reusable_update: Option<MemoryUpdate>,
}

impl Default for Experience {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            id: String::new(),
            source_session: String::new(),
            problem_model_ref: String::new(),
            initial_conditions: Vec::new(),
            target_condition: GoalCondition::default(),
            inferred_required_conditions: Vec::new(),
            actions_taken: Vec::new(),
            observations: Vec::new(),
            verification_result: VerificationResult::default(),
            success_boundary: None,
            failure_boundary: None,
            reusable_update: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct ThinkingProgram {
    pub schema_version: String,
    pub id: String,
    pub user_task: String,
    pub problem_model_ref: Option<String>,
    pub reasoning_mode: ReasoningMode,
    pub reasoning_visibility: ReasoningVisibility,
    pub current_conditions: Vec<Condition>,
    pub target_condition: Option<GoalCondition>,
    pub planned_actions: Vec<Action>,
    pub verification_rules: Vec<VerificationRule>,
}

impl Default for ThinkingProgram {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            id: String::new(),
            user_task: String::new(),
            problem_model_ref: None,
            reasoning_mode: ReasoningMode::LlmPrimary,
            reasoning_visibility: ReasoningVisibility::VisibleByDefault,
            current_conditions: Vec::new(),
            target_condition: None,
            planned_actions: Vec::new(),
            verification_rules: Vec::new(),
        }
    }
}
