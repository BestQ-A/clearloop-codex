use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningMode {
    #[default]
    LlmPrimary,
    Hybrid,
    DbPrimary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct ProblemModelMaturity {
    pub repeated_occurrences: u32,
    pub verified_success_paths: u32,
    pub verified_failed_paths: u32,
    pub condition_coverage: f32,
    pub relation_consistency: f32,
    pub verification_pass_rate: f32,
}

impl Default for ProblemModelMaturity {
    fn default() -> Self {
        Self {
            repeated_occurrences: 0,
            verified_success_paths: 0,
            verified_failed_paths: 0,
            condition_coverage: 0.0,
            relation_consistency: 0.0,
            verification_pass_rate: 0.0,
        }
    }
}

impl ProblemModelMaturity {
    pub fn verified_paths(&self) -> u32 {
        self.verified_success_paths + self.verified_failed_paths
    }

    pub fn suggested_reasoning_mode(&self) -> ReasoningMode {
        if self.repeated_occurrences < 3 || self.verified_paths() < 2 {
            return ReasoningMode::LlmPrimary;
        }

        let enough_boundary_evidence =
            self.verified_success_paths >= 3 && self.verified_failed_paths >= 2;
        let enough_structure = self.condition_coverage >= 0.75
            && self.relation_consistency >= 0.75
            && self.verification_pass_rate >= 0.75;

        if enough_boundary_evidence && enough_structure {
            ReasoningMode::DbPrimary
        } else {
            ReasoningMode::Hybrid
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningVisibility {
    #[default]
    VisibleByDefault,
    RedactedByDefault,
}
