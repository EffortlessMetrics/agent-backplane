// SPDX-License-Identifier: MIT OR Apache-2.0
//! Moonshot Kimi dialect: config, request/response types, and mapping stubs.
//!
//! Kimi uses an OpenAI-compatible chat completions API surface.

use abp_core::{AgentEvent, AgentEventKind, WorkOrder};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Vendor-specific configuration for the Moonshot Kimi API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiConfig {
    /// Moonshot API key.
    pub api_key: String,

    /// Base URL for the Kimi API.
    pub base_url: String,

    /// Model identifier (e.g. `moonshot-v1-8k`).
    pub model: String,

    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,

    /// Temperature for sampling (0.0–1.0).
    pub temperature: Option<f64>,
}

impl Default for KimiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.moonshot.cn/v1".into(),
            model: "moonshot-v1-8k".into(),
            max_tokens: Some(4096),
            temperature: None,
        }
    }
}

/// Simplified representation of a Kimi chat completions request.
///
/// Kimi follows the OpenAI chat completions shape with minor extensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiRequest {
    pub model: String,
    pub messages: Vec<KimiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
}

/// A single message in the Kimi conversation format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiMessage {
    pub role: String,
    pub content: String,
}

/// Simplified representation of a Kimi chat completions response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<KimiChoice>,
    pub usage: Option<KimiUsage>,
}

/// A single choice in a Kimi completions response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiChoice {
    pub index: u32,
    pub message: KimiResponseMessage,
    pub finish_reason: Option<String>,
}

/// A message within a Kimi response choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiResponseMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<KimiToolCall>>,
}

/// A tool call in a Kimi response (OpenAI-compatible shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: KimiFunctionCall,
}

/// The function payload within a Kimi tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiFunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Token usage reported by the Kimi API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Map an ABP [`WorkOrder`] to a [`KimiRequest`].
///
/// Uses the work order task as the initial user message and applies
/// config defaults where the work order does not specify overrides.
pub fn map_work_order(wo: &WorkOrder, config: &KimiConfig) -> KimiRequest {
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

    KimiRequest {
        model,
        messages: vec![KimiMessage {
            role: "user".into(),
            content: user_content,
        }],
        max_tokens: config.max_tokens,
        temperature: config.temperature,
    }
}

/// Map a [`KimiResponse`] back to a sequence of ABP [`AgentEvent`]s.
pub fn map_response(resp: &KimiResponse) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let now = Utc::now();

    for choice in &resp.choices {
        if let Some(text) = &choice.message.content {
            if !text.is_empty() {
                events.push(AgentEvent {
                    ts: now,
                    kind: AgentEventKind::AssistantMessage {
                        text: text.clone(),
                    },
                    ext: None,
                });
            }
        }

        if let Some(tool_calls) = &choice.message.tool_calls {
            for tc in tool_calls {
                let input = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or(serde_json::Value::String(tc.function.arguments.clone()));
                events.push(AgentEvent {
                    ts: now,
                    kind: AgentEventKind::ToolCall {
                        tool_name: tc.function.name.clone(),
                        tool_use_id: Some(tc.id.clone()),
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
        let cfg = KimiConfig::default();
        assert!(cfg.base_url.contains("moonshot.cn"));
        assert!(cfg.model.contains("moonshot"));
        assert!(cfg.max_tokens.unwrap_or(0) > 0);
    }

    #[test]
    fn map_work_order_uses_task_as_user_message() {
        let wo = WorkOrderBuilder::new("Optimize database queries").build();
        let cfg = KimiConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert!(req.messages[0].content.contains("Optimize database queries"));
    }

    #[test]
    fn map_work_order_respects_model_override() {
        let wo = WorkOrderBuilder::new("task").model("moonshot-v1-128k").build();
        let cfg = KimiConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.model, "moonshot-v1-128k");
    }

    #[test]
    fn map_response_produces_assistant_message() {
        let resp = KimiResponse {
            id: "cmpl_123".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("Here is the answer.".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text, "Here is the answer.");
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        }
    }

    #[test]
    fn map_response_handles_tool_calls() {
        let resp = KimiResponse {
            id: "cmpl_456".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![KimiToolCall {
                        id: "call_1".into(),
                        call_type: "function".into(),
                        function: KimiFunctionCall {
                            name: "web_search".into(),
                            arguments: r#"{"query":"rust async"}"#.into(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall { tool_name, tool_use_id, .. } => {
                assert_eq!(tool_name, "web_search");
                assert_eq!(tool_use_id.as_deref(), Some("call_1"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}
