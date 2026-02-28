// SPDX-License-Identifier: MIT OR Apache-2.0
//! Anthropic Claude dialect: config, request/response types, and mapping stubs.

use abp_core::{AgentEvent, AgentEventKind, WorkOrder};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Vendor-specific configuration for the Anthropic Claude API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeConfig {
    /// Anthropic API key (e.g. `sk-ant-...`).
    pub api_key: String,

    /// Base URL for the Messages API.
    pub base_url: String,

    /// Model identifier (e.g. `claude-sonnet-4-20250514`).
    pub model: String,

    /// Maximum tokens to generate.
    pub max_tokens: u32,

    /// System prompt override (merged with work order task if set).
    pub system_prompt: Option<String>,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.anthropic.com/v1".into(),
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            system_prompt: None,
        }
    }
}

/// Simplified representation of an Anthropic Messages API request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub max_tokens: u32,
    pub system: Option<String>,
    pub messages: Vec<ClaudeMessage>,
}

/// A single message in the Claude conversation format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeMessage {
    pub role: String,
    pub content: String,
}

/// Simplified representation of an Anthropic Messages API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeResponse {
    pub id: String,
    pub model: String,
    pub role: String,
    pub content: Vec<ClaudeContentBlock>,
    pub stop_reason: Option<String>,
    pub usage: Option<ClaudeUsage>,
}

/// A content block in a Claude response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
}

/// Token usage reported by the Anthropic API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Map an ABP [`WorkOrder`] to a [`ClaudeRequest`].
///
/// Uses the work order task as the initial user message and applies
/// config defaults where the work order does not specify overrides.
pub fn map_work_order(wo: &WorkOrder, config: &ClaudeConfig) -> ClaudeRequest {
    let model = wo
        .config
        .model
        .as_deref()
        .unwrap_or(&config.model)
        .to_string();

    let system = config.system_prompt.clone();

    let mut user_content = wo.task.clone();
    for snippet in &wo.context.snippets {
        user_content.push_str(&format!("\n\n--- {} ---\n{}", snippet.name, snippet.content));
    }

    ClaudeRequest {
        model,
        max_tokens: config.max_tokens,
        system,
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: user_content,
        }],
    }
}

/// Map a [`ClaudeResponse`] back to a sequence of ABP [`AgentEvent`]s.
pub fn map_response(resp: &ClaudeResponse) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let now = Utc::now();

    for block in &resp.content {
        match block {
            ClaudeContentBlock::Text { text } => {
                events.push(AgentEvent {
                    ts: now,
                    kind: AgentEventKind::AssistantMessage {
                        text: text.clone(),
                    },
                    ext: None,
                });
            }
            ClaudeContentBlock::ToolUse { id, name, input } => {
                events.push(AgentEvent {
                    ts: now,
                    kind: AgentEventKind::ToolCall {
                        tool_name: name.clone(),
                        tool_use_id: Some(id.clone()),
                        parent_tool_use_id: None,
                        input: input.clone(),
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
        let cfg = ClaudeConfig::default();
        assert!(cfg.base_url.contains("anthropic.com"));
        assert!(cfg.model.contains("claude"));
        assert!(cfg.max_tokens > 0);
    }

    #[test]
    fn map_work_order_uses_task_as_user_message() {
        let wo = WorkOrderBuilder::new("Refactor auth module").build();
        let cfg = ClaudeConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert!(req.messages[0].content.contains("Refactor auth module"));
    }

    #[test]
    fn map_work_order_respects_model_override() {
        let wo = WorkOrderBuilder::new("task").model("claude-opus-4-20250514").build();
        let cfg = ClaudeConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.model, "claude-opus-4-20250514");
    }

    #[test]
    fn map_response_produces_assistant_message() {
        let resp = ClaudeResponse {
            id: "msg_123".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![ClaudeContentBlock::Text {
                text: "Hello!".into(),
            }],
            stop_reason: Some("end_turn".into()),
            usage: None,
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hello!"),
            other => panic!("expected AssistantMessage, got {other:?}"),
        }
    }

    #[test]
    fn map_response_handles_tool_use() {
        let resp = ClaudeResponse {
            id: "msg_456".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![ClaudeContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read_file".into(),
                input: serde_json::json!({"path": "src/main.rs"}),
            }],
            stop_reason: Some("tool_use".into()),
            usage: None,
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall { tool_name, tool_use_id, .. } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tu_1"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}
