// SPDX-License-Identifier: MIT OR Apache-2.0
//! OpenAI Codex dialect: config, request/response types, and mapping stubs.

use abp_core::{AgentEvent, AgentEventKind, WorkOrder};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Vendor-specific configuration for the OpenAI Codex / Responses API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexConfig {
    /// OpenAI API key (e.g. `sk-...`).
    pub api_key: String,

    /// Base URL for the API.
    pub base_url: String,

    /// Model identifier (e.g. `codex-mini-latest`).
    pub model: String,

    /// Maximum output tokens.
    pub max_output_tokens: Option<u32>,

    /// Temperature for sampling (0.0–2.0).
    pub temperature: Option<f64>,
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.openai.com/v1".into(),
            model: "codex-mini-latest".into(),
            max_output_tokens: Some(4096),
            temperature: None,
        }
    }
}

/// Simplified representation of an OpenAI Responses API request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexRequest {
    pub model: String,
    pub input: Vec<CodexInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
}

/// An input item in the Codex Responses API format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexInputItem {
    Message { role: String, content: String },
}

/// Simplified representation of an OpenAI Responses API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexResponse {
    pub id: String,
    pub model: String,
    pub output: Vec<CodexOutputItem>,
    pub usage: Option<CodexUsage>,
}

/// An output item in the Codex Responses API format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexOutputItem {
    Message {
        role: String,
        content: Vec<CodexContentPart>,
    },
    FunctionCall {
        id: String,
        name: String,
        arguments: String,
    },
}

/// A content part within a Codex output message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexContentPart {
    OutputText { text: String },
}

/// Token usage reported by the OpenAI API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Map an ABP [`WorkOrder`] to a [`CodexRequest`].
///
/// Uses the work order task as the initial user message and applies
/// config defaults where the work order does not specify overrides.
pub fn map_work_order(wo: &WorkOrder, config: &CodexConfig) -> CodexRequest {
    let model = wo
        .config
        .model
        .as_deref()
        .unwrap_or(&config.model)
        .to_string();

    let mut user_content = wo.task.clone();
    for snippet in &wo.context.snippets {
        user_content.push_str(&format!("\n\n--- {} ---\n{}", snippet.name, snippet.content));
    }

    CodexRequest {
        model,
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: user_content,
        }],
        max_output_tokens: config.max_output_tokens,
        temperature: config.temperature,
    }
}

/// Map a [`CodexResponse`] back to a sequence of ABP [`AgentEvent`]s.
pub fn map_response(resp: &CodexResponse) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let now = Utc::now();

    for item in &resp.output {
        match item {
            CodexOutputItem::Message { content, .. } => {
                for part in content {
                    match part {
                        CodexContentPart::OutputText { text } => {
                            events.push(AgentEvent {
                                ts: now,
                                kind: AgentEventKind::AssistantMessage {
                                    text: text.clone(),
                                },
                                ext: None,
                            });
                        }
                    }
                }
            }
            CodexOutputItem::FunctionCall { id, name, arguments } => {
                let input = serde_json::from_str(arguments)
                    .unwrap_or(serde_json::Value::String(arguments.clone()));
                events.push(AgentEvent {
                    ts: now,
                    kind: AgentEventKind::ToolCall {
                        tool_name: name.clone(),
                        tool_use_id: Some(id.clone()),
                        parent_tool_use_id: None,
                        input,
                    },
                    ext: None,
                });
            }
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::WorkOrderBuilder;

    #[test]
    fn default_config_has_sensible_values() {
        let cfg = CodexConfig::default();
        assert!(cfg.base_url.contains("openai.com"));
        assert!(cfg.model.contains("codex"));
        assert!(cfg.max_output_tokens.unwrap_or(0) > 0);
    }

    #[test]
    fn map_work_order_uses_task_as_user_message() {
        let wo = WorkOrderBuilder::new("Write unit tests").build();
        let cfg = CodexConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.input.len(), 1);
        match &req.input[0] {
            CodexInputItem::Message { role, content } => {
                assert_eq!(role, "user");
                assert!(content.contains("Write unit tests"));
            }
        }
    }

    #[test]
    fn map_work_order_respects_model_override() {
        let wo = WorkOrderBuilder::new("task").model("o3-mini").build();
        let cfg = CodexConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.model, "o3-mini");
    }

    #[test]
    fn map_response_produces_assistant_message() {
        let resp = CodexResponse {
            id: "resp_123".into(),
            model: "codex-mini-latest".into(),
            output: vec![CodexOutputItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Done!".into(),
                }],
            }],
            usage: None,
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Done!"),
            other => panic!("expected AssistantMessage, got {other:?}"),
        }
    }

    #[test]
    fn map_response_handles_function_call() {
        let resp = CodexResponse {
            id: "resp_456".into(),
            model: "codex-mini-latest".into(),
            output: vec![CodexOutputItem::FunctionCall {
                id: "fc_1".into(),
                name: "shell".into(),
                arguments: r#"{"command":"ls"}"#.into(),
            }],
            usage: None,
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall { tool_name, tool_use_id, .. } => {
                assert_eq!(tool_name, "shell");
                assert_eq!(tool_use_id.as_deref(), Some("fc_1"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}
