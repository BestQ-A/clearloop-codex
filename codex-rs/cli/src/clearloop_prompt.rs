use codex_clearloop_core::ThinkingProgram;

pub(crate) fn build_execution_prompt(task: &str, program: Option<&ThinkingProgram>) -> String {
    let Some(program) = program else {
        return task.to_string();
    };

    let mut prompt = String::new();
    prompt.push_str("# ClearLoop execution prompt\n\n");
    prompt.push_str("## User task\n");
    prompt.push_str(task);
    prompt.push_str("\n\n## Explicit thinking program\n");
    prompt.push_str(&format!("- id: {}\n", program.id));
    if let Some(problem_model_ref) = program.problem_model_ref.as_ref() {
        prompt.push_str(&format!("- problem_model_ref: {problem_model_ref}\n"));
    }
    if let Some(retrieval_ref) = program.retrieval_ref.as_ref() {
        prompt.push_str(&format!("- retrieval_ref: {retrieval_ref}\n"));
    }
    prompt.push_str(&format!(
        "- reasoning_mode: {:?}\n- reasoning_visibility: {:?}\n",
        program.reasoning_mode, program.reasoning_visibility
    ));

    if let Some(target_condition) = program.target_condition.as_ref() {
        prompt.push_str("\n## Target condition\n");
        prompt.push_str(&format!(
            "- {} must become {}. Success signal: {}\n",
            target_condition.condition_ref,
            target_condition.target_value,
            target_condition.success_signal
        ));
    }

    if !program.current_conditions.is_empty() {
        prompt.push_str("\n## Current observed conditions\n");
        for condition in &program.current_conditions {
            prompt.push_str(&format!(
                "- {}: {} observed={:?} target={:?}\n",
                condition.id, condition.name, condition.observed_value, condition.target_value
            ));
        }
    }

    if !program.planned_actions.is_empty() {
        prompt.push_str("\n## Planned actions\n");
        for action in &program.planned_actions {
            prompt.push_str(&format!(
                "- {}: {} Expected effect: {}",
                action.id, action.description, action.expected_effect
            ));
            if let Some(command_ref) = action.command_ref.as_ref() {
                prompt.push_str(&format!(" Command/ref: {command_ref}"));
            }
            prompt.push('\n');
        }
    }

    if !program.retrieved_memories.is_empty() {
        prompt.push_str("\n## Retrieved promoted memory\n");
        for memory in &program.retrieved_memories {
            prompt.push_str(&format!(
                "- experience_id: {} score={} problem_model={} target={}\n",
                memory.experience_id,
                memory.score,
                memory.problem_model_ref,
                memory.target_condition
            ));
            prompt.push_str(&format!("  source_session: {}\n", memory.source_session));
            prompt.push_str(&format!("  memory_ref: {}\n", memory.memory_ref));
            if let Some(retrieval_ref) = memory.retrieval_ref.as_ref() {
                prompt.push_str(&format!("  retrieval_ref: {retrieval_ref}\n"));
            }
            if let Some(claim) = memory.claim.as_ref() {
                prompt.push_str(&format!("  reusable_claim: {claim}\n"));
            }
            if !memory.evidence_refs.is_empty() {
                prompt.push_str("  evidence_refs:\n");
                for evidence_ref in &memory.evidence_refs {
                    prompt.push_str(&format!("  - {evidence_ref}\n"));
                }
            }
        }
    }

    prompt.push_str(
        "\n## Execution rules\n\
         - Use the explicit thinking program as the working model for this run.\n\
         - Treat retrieved memories as prior verified evidence only when current conditions match.\n\
         - If a retrieved memory does not apply, state the mismatch in the observable result.\n\
         - Prefer actions that can be verified by files, commands, logs, tests, or other observable evidence.\n\
         - Keep reasoning visible through explicit summaries, decisions, commands, and verification evidence.\n",
    );

    prompt
}
