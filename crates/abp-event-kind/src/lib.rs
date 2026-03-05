// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

use abp_core::AgentEventKind;

/// Returns the canonical snake_case name for an [`AgentEventKind`] variant.
#[must_use]
pub fn event_kind_name(kind: &AgentEventKind) -> &'static str {
    match kind {
        AgentEventKind::RunStarted { .. } => "run_started",
        AgentEventKind::RunCompleted { .. } => "run_completed",
        AgentEventKind::AssistantDelta { .. } => "assistant_delta",
        AgentEventKind::AssistantMessage { .. } => "assistant_message",
        AgentEventKind::ToolCall { .. } => "tool_call",
        AgentEventKind::ToolResult { .. } => "tool_result",
        AgentEventKind::FileChanged { .. } => "file_changed",
        AgentEventKind::CommandExecuted { .. } => "command_executed",
        AgentEventKind::Warning { .. } => "warning",
        AgentEventKind::Error { .. } => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::event_kind_name;
    use abp_core::AgentEventKind;

    #[test]
    fn maps_all_event_kinds() {
        let cases = vec![
            (
                AgentEventKind::RunStarted {
                    message: "start".into(),
                },
                "run_started",
            ),
            (
                AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                "run_completed",
            ),
            (
                AgentEventKind::AssistantDelta { text: "x".into() },
                "assistant_delta",
            ),
            (
                AgentEventKind::AssistantMessage { text: "x".into() },
                "assistant_message",
            ),
            (
                AgentEventKind::ToolCall {
                    tool_name: "t".into(),
                    tool_use_id: None,
                    parent_tool_use_id: None,
                    input: serde_json::json!({}),
                },
                "tool_call",
            ),
            (
                AgentEventKind::ToolResult {
                    tool_name: "t".into(),
                    tool_use_id: None,
                    output: serde_json::json!({}),
                    is_error: false,
                },
                "tool_result",
            ),
            (
                AgentEventKind::FileChanged {
                    path: "a.txt".into(),
                    summary: "changed".into(),
                },
                "file_changed",
            ),
            (
                AgentEventKind::CommandExecuted {
                    command: "echo hi".into(),
                    exit_code: Some(0),
                    output_preview: None,
                },
                "command_executed",
            ),
            (
                AgentEventKind::Warning {
                    message: "w".into(),
                },
                "warning",
            ),
            (
                AgentEventKind::Error {
                    message: "e".into(),
                    error_code: None,
                },
                "error",
            ),
        ];

        for (kind, expected) in cases {
            assert_eq!(event_kind_name(&kind), expected);
        }
    }
}
