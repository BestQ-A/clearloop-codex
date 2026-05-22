use crate::ledger::LedgerEvent;
use crate::ledger::StreamRecord;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

pub const CODEX_EXEC_JSON_SOURCE: &str = "codex-exec-json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct EventBridgeReport {
    pub events_ingested: usize,
    pub evidence_events: usize,
    pub command_events: usize,
    pub model_visible_output_events: usize,
    pub explicit_reasoning_events: usize,
    pub tool_events: usize,
    pub decision_events: usize,
}

impl EventBridgeReport {
    pub fn record(&mut self, event: &LedgerEvent) {
        self.events_ingested += 1;
        match event {
            LedgerEvent::Evidence(_) => self.evidence_events += 1,
            LedgerEvent::Command(_) => self.command_events += 1,
            LedgerEvent::ModelVisibleOutput(_) => self.model_visible_output_events += 1,
            LedgerEvent::ExplicitReasoning(_) => self.explicit_reasoning_events += 1,
            LedgerEvent::ToolEvent(_) => self.tool_events += 1,
            LedgerEvent::Decision(_) => self.decision_events += 1,
        }
    }
}

pub fn map_codex_exec_event(payload: Value) -> LedgerEvent {
    map_codex_exec_event_with_source(payload, CODEX_EXEC_JSON_SOURCE)
}

pub fn map_codex_exec_event_with_source(payload: Value, source: impl Into<String>) -> LedgerEvent {
    let source = source.into();
    let event_type = string_at(&payload, "/type")
        .unwrap_or("unknown")
        .to_string();
    let item_type = string_at(&payload, "/item/type").map(str::to_string);
    let summary = codex_exec_summary(&payload, event_type.as_str(), item_type.as_deref());
    let record = StreamRecord {
        source,
        summary,
        payload,
        ..StreamRecord::default()
    };

    match (event_type.as_str(), item_type.as_deref()) {
        (_, Some("agent_message")) => LedgerEvent::ModelVisibleOutput(record),
        (_, Some("reasoning")) => LedgerEvent::ExplicitReasoning(record),
        (_, Some("command_execution")) => LedgerEvent::Command(record),
        (_, Some("mcp_tool_call" | "collab_tool_call" | "web_search" | "file_change")) => {
            LedgerEvent::ToolEvent(record)
        }
        (_, Some("todo_list")) => LedgerEvent::Decision(record),
        (_, Some("error")) => LedgerEvent::Evidence(record),
        ("turn.started", _) => LedgerEvent::ExplicitReasoning(record),
        ("turn.completed" | "turn.failed" | "thread.started" | "error", _) => {
            LedgerEvent::Evidence(record)
        }
        _ => LedgerEvent::Evidence(record),
    }
}

fn codex_exec_summary(payload: &Value, event_type: &str, item_type: Option<&str>) -> String {
    match (event_type, item_type) {
        ("thread.started", _) => {
            let thread_id = string_at(payload, "/thread_id").unwrap_or("unknown");
            format!("Codex thread started: {thread_id}")
        }
        ("turn.started", _) => "Codex turn started.".to_string(),
        ("turn.completed", _) => {
            let input = integer_at(payload, "/usage/input_tokens").unwrap_or(0);
            let output = integer_at(payload, "/usage/output_tokens").unwrap_or(0);
            let reasoning = integer_at(payload, "/usage/reasoning_output_tokens").unwrap_or(0);
            format!(
                "Codex turn completed: input_tokens={input}, output_tokens={output}, reasoning_output_tokens={reasoning}"
            )
        }
        ("turn.failed", _) => {
            let message = string_at(payload, "/error/message").unwrap_or("unknown error");
            format!("Codex turn failed: {}", compact(message))
        }
        ("error", _) => {
            let message = string_at(payload, "/message").unwrap_or("unknown error");
            format!("Codex stream error: {}", compact(message))
        }
        (_, Some("agent_message")) => {
            let text = string_at(payload, "/item/text").unwrap_or("");
            format!("Codex model-visible output: {}", compact(text))
        }
        (_, Some("reasoning")) => {
            let text = string_at(payload, "/item/text").unwrap_or("");
            format!("Codex visible reasoning summary: {}", compact(text))
        }
        (_, Some("command_execution")) => {
            let command = string_at(payload, "/item/command").unwrap_or("");
            let status = string_at(payload, "/item/status").unwrap_or("unknown");
            format!("Codex command {status}: {}", compact(command))
        }
        (_, Some("mcp_tool_call")) => {
            let server = string_at(payload, "/item/server").unwrap_or("unknown");
            let tool = string_at(payload, "/item/tool").unwrap_or("unknown");
            let status = string_at(payload, "/item/status").unwrap_or("unknown");
            format!("Codex MCP tool {status}: {server}.{tool}")
        }
        (_, Some("collab_tool_call")) => {
            let status = string_at(payload, "/item/status").unwrap_or("unknown");
            format!("Codex collaboration tool {status}.")
        }
        (_, Some("web_search")) => {
            let query = string_at(payload, "/item/query").unwrap_or("");
            format!("Codex web search: {}", compact(query))
        }
        (_, Some("file_change")) => {
            let status = string_at(payload, "/item/status").unwrap_or("unknown");
            format!("Codex file change {status}.")
        }
        (_, Some("todo_list")) => "Codex plan/todo state changed.".to_string(),
        (_, Some("error")) => {
            let message = string_at(payload, "/item/message").unwrap_or("unknown error");
            format!("Codex item error: {}", compact(message))
        }
        _ => format!("Codex exec event: {event_type}"),
    }
}

fn string_at<'a>(payload: &'a Value, pointer: &str) -> Option<&'a str> {
    payload.pointer(pointer).and_then(Value::as_str)
}

fn integer_at(payload: &Value, pointer: &str) -> Option<i64> {
    payload.pointer(pointer).and_then(Value::as_i64)
}

fn compact(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 120 {
        return normalized;
    }

    let mut truncated = normalized.chars().take(117).collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn maps_agent_message_to_model_visible_output() {
        let event = map_codex_exec_event(json!({
            "type": "item.completed",
            "item": {
                "id": "msg-1",
                "type": "agent_message",
                "text": "Done"
            }
        }));

        let LedgerEvent::ModelVisibleOutput(record) = event else {
            panic!("expected model-visible output event");
        };
        assert_eq!(record.source, CODEX_EXEC_JSON_SOURCE);
        assert_eq!(record.summary, "Codex model-visible output: Done");
    }

    #[test]
    fn maps_command_execution_to_command_stream() {
        let event = map_codex_exec_event(json!({
            "type": "item.completed",
            "item": {
                "id": "cmd-1",
                "type": "command_execution",
                "command": "cargo test -p codex-clearloop-core",
                "aggregated_output": "ok",
                "exit_code": 0,
                "status": "completed"
            }
        }));

        let LedgerEvent::Command(record) = event else {
            panic!("expected command event");
        };
        assert_eq!(
            record.summary,
            "Codex command completed: cargo test -p codex-clearloop-core"
        );
    }

    #[test]
    fn report_counts_routed_events() {
        let events = [
            map_codex_exec_event(json!({"type": "thread.started", "thread_id": "abc"})),
            map_codex_exec_event(json!({
                "type": "item.completed",
                "item": {"id": "reason-1", "type": "reasoning", "text": "Need verify first."}
            })),
            map_codex_exec_event(json!({
                "type": "item.updated",
                "item": {"id": "todo-1", "type": "todo_list", "items": []}
            })),
        ];
        let mut report = EventBridgeReport::default();
        for event in &events {
            report.record(event);
        }

        assert_eq!(report.events_ingested, 3);
        assert_eq!(report.evidence_events, 1);
        assert_eq!(report.explicit_reasoning_events, 1);
        assert_eq!(report.decision_events, 1);
    }
}
