// SPDX-License-Identifier: MIT OR Apache-2.0
//! Anthropic Claude dialect: config, request/response types, and mapping logic.

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, SupportLevel, WorkOrder,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Version string for this dialect adapter.
pub const DIALECT_VERSION: &str = "claude/v0.1";

/// Default model used when none is specified.
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";

// ---------------------------------------------------------------------------
// Model-name mapping
// ---------------------------------------------------------------------------

/// Known Anthropic Claude model identifiers.
const KNOWN_MODELS: &[&str] = &[
    "claude-sonnet-4-20250514",
    "claude-opus-4-20250514",
    "claude-haiku-3-5-20241022",
    "claude-sonnet-3-5-20241022",
    "claude-3-5-haiku-latest",
    "claude-sonnet-4-latest",
    "claude-opus-4-latest",
    "claude-4-20250714",
    "claude-4-latest",
];

/// Map a vendor model name to the ABP canonical form (`anthropic/<model>`).
#[must_use]
pub fn to_canonical_model(vendor_model: &str) -> String {
    format!("anthropic/{vendor_model}")
}

/// Map an ABP canonical model name back to the vendor model name.
///
/// Strips the `anthropic/` prefix if present; otherwise returns the input unchanged.
#[must_use]
pub fn from_canonical_model(canonical: &str) -> String {
    canonical
        .strip_prefix("anthropic/")
        .unwrap_or(canonical)
        .to_string()
}

/// Returns `true` if `model` is a known Anthropic Claude model identifier.
#[must_use]
pub fn is_known_model(model: &str) -> bool {
    KNOWN_MODELS.contains(&model)
}

// ---------------------------------------------------------------------------
// Capability mapping
// ---------------------------------------------------------------------------

/// Build a [`CapabilityManifest`] describing what the Claude backend supports.
#[must_use]
pub fn capability_manifest() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Native);
    m.insert(Capability::ToolEdit, SupportLevel::Native);
    m.insert(Capability::ToolBash, SupportLevel::Native);
    m.insert(Capability::ToolGlob, SupportLevel::Native);
    m.insert(Capability::ToolGrep, SupportLevel::Native);
    m.insert(Capability::ToolWebSearch, SupportLevel::Native);
    m.insert(Capability::ToolWebFetch, SupportLevel::Native);
    m.insert(Capability::StructuredOutputJsonSchema, SupportLevel::Native);
    m.insert(Capability::HooksPreToolUse, SupportLevel::Native);
    m.insert(Capability::HooksPostToolUse, SupportLevel::Native);
    m.insert(Capability::McpClient, SupportLevel::Native);
    m.insert(Capability::McpServer, SupportLevel::Unsupported);
    m.insert(Capability::Checkpointing, SupportLevel::Emulated);
    m
}

// ---------------------------------------------------------------------------
// Tool-format translation
// ---------------------------------------------------------------------------

/// A vendor-agnostic tool definition used as the ABP canonical form.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalToolDef {
    pub name: String,
    pub description: String,
    pub parameters_schema: serde_json::Value,
}

/// Anthropic-style tool definition (Messages API `tools` array element).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaudeToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Convert an ABP canonical tool definition to the Anthropic tool format.
#[must_use]
pub fn tool_def_to_claude(def: &CanonicalToolDef) -> ClaudeToolDef {
    ClaudeToolDef {
        name: def.name.clone(),
        description: def.description.clone(),
        input_schema: def.parameters_schema.clone(),
    }
}

/// Convert an Anthropic tool definition back to the ABP canonical form.
#[must_use]
pub fn tool_def_from_claude(def: &ClaudeToolDef) -> CanonicalToolDef {
    CanonicalToolDef {
        name: def.name.clone(),
        description: def.description.clone(),
        parameters_schema: def.input_schema.clone(),
    }
}

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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaudeResponse {
    pub id: String,
    pub model: String,
    pub role: String,
    pub content: Vec<ClaudeContentBlock>,
    pub stop_reason: Option<String>,
    pub usage: Option<ClaudeUsage>,
}

/// A content block in a Claude response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    Thinking {
        thinking: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    Image {
        source: ClaudeImageSource,
    },
}

/// Image source for an image content block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

/// System prompt block with optional cache control.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeSystemBlock {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<ClaudeCacheControl>,
    },
}

/// Cache control directive for prompt caching.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaudeCacheControl {
    #[serde(rename = "type")]
    pub cache_type: String,
}

impl ClaudeCacheControl {
    /// Create an "ephemeral" cache control (the most common variant).
    #[must_use]
    pub fn ephemeral() -> Self {
        Self {
            cache_type: "ephemeral".into(),
        }
    }
}

/// Token usage reported by the Anthropic API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaudeUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
}

// ---------------------------------------------------------------------------
// Streaming event types
// ---------------------------------------------------------------------------

/// Server-sent event types from the Anthropic streaming API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeStreamEvent {
    MessageStart {
        message: ClaudeResponse,
    },
    ContentBlockStart {
        index: u32,
        content_block: ClaudeContentBlock,
    },
    ContentBlockDelta {
        index: u32,
        delta: ClaudeStreamDelta,
    },
    ContentBlockStop {
        index: u32,
    },
    MessageDelta {
        delta: ClaudeMessageDelta,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<ClaudeUsage>,
    },
    MessageStop {},
    Ping {},
    Error {
        error: ClaudeApiError,
    },
}

/// Delta payload within a `content_block_delta` streaming event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeStreamDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
    ThinkingDelta { thinking: String },
    SignatureDelta { signature: String },
}

/// Delta payload within a `message_delta` streaming event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaudeMessageDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

/// Error object returned by the Anthropic API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaudeApiError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
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
        user_content.push_str(&format!(
            "\n\n--- {} ---\n{}",
            snippet.name, snippet.content
        ));
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
                    kind: AgentEventKind::AssistantMessage { text: text.clone() },
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
            ClaudeContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                events.push(AgentEvent {
                    ts: now,
                    kind: AgentEventKind::ToolResult {
                        tool_name: String::new(),
                        tool_use_id: Some(tool_use_id.clone()),
                        output: serde_json::Value::String(content.clone().unwrap_or_default()),
                        is_error: is_error.unwrap_or(false),
                    },
                    ext: None,
                });
            }
            // Thinking and Image blocks don't map to standard ABP events.
            ClaudeContentBlock::Thinking { .. } | ClaudeContentBlock::Image { .. } => {}
        }
    }

    events
}

/// Map a single [`ClaudeStreamEvent`] to zero or more ABP [`AgentEvent`]s.
pub fn map_stream_event(event: &ClaudeStreamEvent) -> Vec<AgentEvent> {
    let now = Utc::now();

    match event {
        ClaudeStreamEvent::ContentBlockDelta {
            delta: ClaudeStreamDelta::TextDelta { text },
            ..
        } => {
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::AssistantDelta { text: text.clone() },
                ext: None,
            }]
        }
        ClaudeStreamEvent::MessageStart { .. } => {
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::RunStarted {
                    message: "Claude stream started".into(),
                },
                ext: None,
            }]
        }
        ClaudeStreamEvent::MessageStop {} => {
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::RunCompleted {
                    message: "Claude stream completed".into(),
                },
                ext: None,
            }]
        }
        ClaudeStreamEvent::Error { error } => {
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::Error {
                    message: format!("{}: {}", error.error_type, error.message),
                },
                ext: None,
            }]
        }
        // Other event types (ping, content_block_start/stop, message_delta) are structural
        // and don't produce ABP events.
        _ => vec![],
    }
}

/// Create a Claude `tool_result` message from ABP tool result data.
///
/// Returns a [`ClaudeMessage`] with role `"user"` containing a single
/// `tool_result` content block, matching the Anthropic Messages API format.
#[must_use]
pub fn map_tool_result(tool_use_id: &str, output: &str, is_error: bool) -> ClaudeMessage {
    let block = ClaudeContentBlock::ToolResult {
        tool_use_id: tool_use_id.to_string(),
        content: Some(output.to_string()),
        is_error: if is_error { Some(true) } else { None },
    };
    let content = serde_json::to_string(&vec![block]).unwrap_or_default();
    ClaudeMessage {
        role: "user".into(),
        content,
    }
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
        let wo = WorkOrderBuilder::new("task")
            .model("claude-opus-4-20250514")
            .build();
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
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tu_1"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}
