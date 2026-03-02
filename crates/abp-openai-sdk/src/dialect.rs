// SPDX-License-Identifier: MIT OR Apache-2.0
//! OpenAI Chat Completions dialect: config, request/response types, and mapping logic.

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, SupportLevel, WorkOrder,
};
pub use abp_tooling::CanonicalToolDef;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::response_format::ResponseFormat;

/// Version string for this dialect adapter.
pub const DIALECT_VERSION: &str = "openai/v0.1";

/// Default model used when none is specified.
pub const DEFAULT_MODEL: &str = "gpt-4o";

// ---------------------------------------------------------------------------
// Model-name mapping
// ---------------------------------------------------------------------------

/// Known OpenAI Chat Completions model identifiers.
const KNOWN_MODELS: &[&str] = &[
    "gpt-4o",
    "gpt-4o-mini",
    "gpt-4-turbo",
    "o1",
    "o1-mini",
    "o3-mini",
    "gpt-4.1",
];

/// Map a vendor model name to the ABP canonical form (`openai/<model>`).
#[must_use]
pub fn to_canonical_model(vendor_model: &str) -> String {
    format!("openai/{vendor_model}")
}

/// Map an ABP canonical model name back to the vendor model name.
///
/// Strips the `openai/` prefix if present; otherwise returns the input unchanged.
#[must_use]
pub fn from_canonical_model(canonical: &str) -> String {
    canonical
        .strip_prefix("openai/")
        .unwrap_or(canonical)
        .to_string()
}

/// Returns `true` if `model` is a known OpenAI model identifier.
#[must_use]
pub fn is_known_model(model: &str) -> bool {
    KNOWN_MODELS.contains(&model)
}

// ---------------------------------------------------------------------------
// Capability mapping
// ---------------------------------------------------------------------------

/// Build a [`CapabilityManifest`] describing what the OpenAI Chat Completions backend supports.
#[must_use]
pub fn capability_manifest() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Emulated);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m.insert(Capability::ToolEdit, SupportLevel::Emulated);
    m.insert(Capability::ToolBash, SupportLevel::Emulated);
    m.insert(Capability::ToolGlob, SupportLevel::Emulated);
    m.insert(Capability::ToolGrep, SupportLevel::Emulated);
    m.insert(Capability::StructuredOutputJsonSchema, SupportLevel::Native);
    m.insert(Capability::HooksPreToolUse, SupportLevel::Emulated);
    m.insert(Capability::HooksPostToolUse, SupportLevel::Emulated);
    m.insert(Capability::McpClient, SupportLevel::Unsupported);
    m.insert(Capability::McpServer, SupportLevel::Unsupported);
    m
}

// ---------------------------------------------------------------------------
// Tool-format translation
// ---------------------------------------------------------------------------

/// OpenAI-style function tool definition (Chat Completions `tools` array element).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAIToolDef {
    /// Tool type (always `"function"`).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition payload.
    pub function: OpenAIFunctionDef,
}

/// The function payload inside an [`OpenAIToolDef`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAIFunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Tool choice
// ---------------------------------------------------------------------------

/// Controls which (if any) tool the model should call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ToolChoice {
    /// A string shorthand: `"none"`, `"auto"`, or `"required"`.
    Mode(ToolChoiceMode),
    /// Force a specific function call.
    Function {
        /// Must be `"function"`.
        #[serde(rename = "type")]
        tool_type: String,
        /// The function to force.
        function: ToolChoiceFunctionRef,
    },
}

/// String-form tool choice modes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoiceMode {
    /// Model will not call any tool.
    None,
    /// Model decides whether to call a tool.
    Auto,
    /// Model must call at least one tool.
    Required,
}

/// A reference to a specific function in a [`ToolChoice::Function`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolChoiceFunctionRef {
    /// Name of the function to force.
    pub name: String,
}

/// Convert an ABP canonical tool definition to the OpenAI function tool format.
#[must_use]
pub fn tool_def_to_openai(def: &CanonicalToolDef) -> OpenAIToolDef {
    OpenAIToolDef {
        tool_type: "function".into(),
        function: OpenAIFunctionDef {
            name: def.name.clone(),
            description: def.description.clone(),
            parameters: def.parameters_schema.clone(),
        },
    }
}

/// Convert an OpenAI function tool definition back to the ABP canonical form.
#[must_use]
pub fn tool_def_from_openai(def: &OpenAIToolDef) -> CanonicalToolDef {
    CanonicalToolDef {
        name: def.function.name.clone(),
        description: def.function.description.clone(),
        parameters_schema: def.function.parameters.clone(),
    }
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Vendor-specific configuration for the OpenAI Chat Completions API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIConfig {
    /// OpenAI API key (e.g. `sk-...`).
    pub api_key: String,

    /// Base URL for the Chat Completions API.
    pub base_url: String,

    /// Model identifier (e.g. `gpt-4o`).
    pub model: String,

    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,

    /// Temperature for sampling (0.0–2.0).
    pub temperature: Option<f64>,
}

impl Default for OpenAIConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o".into(),
            max_tokens: Some(4096),
            temperature: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Simplified representation of an OpenAI Chat Completions API request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIRequest {
    /// Model identifier (e.g. `gpt-4o`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<OpenAIMessage>,
    /// Tool definitions available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAIToolDef>>,
    /// Controls which tool the model should call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Response format constraint (e.g. JSON mode, JSON Schema).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

/// A single message in the OpenAI Chat Completions format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIMessage {
    /// Message role (`system`, `user`, `assistant`, or `tool`).
    pub role: String,
    /// Text content of the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls requested by the assistant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
    /// ID of the tool call this message is responding to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A tool call emitted by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAIToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// Call type (always `"function"`).
    #[serde(rename = "type")]
    pub call_type: String,
    /// The function invocation details.
    pub function: OpenAIFunctionCall,
}

/// The function invocation inside an [`OpenAIToolCall`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAIFunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments for the function.
    pub arguments: String,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Simplified representation of an OpenAI Chat Completions API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIResponse {
    /// Unique response identifier.
    pub id: String,
    /// Object type (e.g. `chat.completion`).
    pub object: String,
    /// Model used for the completion.
    pub model: String,
    /// Completion choices.
    pub choices: Vec<OpenAIChoice>,
    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<OpenAIUsage>,
}

/// A single choice in the Chat Completions response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIChoice {
    /// Zero-based index of this choice.
    pub index: u32,
    /// The assistant's response message.
    pub message: OpenAIMessage,
    /// Reason the model stopped generating (e.g. `stop`, `tool_calls`).
    pub finish_reason: Option<String>,
}

/// Token usage reported by the OpenAI API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIUsage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

// ---------------------------------------------------------------------------
// Mapping functions
// ---------------------------------------------------------------------------

/// Map an ABP [`WorkOrder`] to an [`OpenAIRequest`].
///
/// Uses the work order task as the initial user message and applies
/// config defaults where the work order does not specify overrides.
pub fn map_work_order(wo: &WorkOrder, config: &OpenAIConfig) -> OpenAIRequest {
    let model = wo
        .config
        .model
        .as_deref()
        .unwrap_or(&config.model)
        .to_string();

    let mut user_content = wo.task.clone();
    for snippet in &wo.context.snippets {
        user_content.push_str(&format!(
            "\n\n--- {} ---\n{}",
            snippet.name, snippet.content
        ));
    }

    OpenAIRequest {
        model,
        messages: vec![OpenAIMessage {
            role: "user".into(),
            content: Some(user_content),
            tool_calls: None,
            tool_call_id: None,
        }],
        tools: None,
        tool_choice: None,
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        response_format: None,
    }
}

/// Map an [`OpenAIResponse`] back to a sequence of ABP [`AgentEvent`]s.
pub fn map_response(resp: &OpenAIResponse) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let now = Utc::now();

    for choice in &resp.choices {
        let msg = &choice.message;

        // Emit assistant text if present.
        if let Some(text) = &msg.content
            && !text.is_empty()
        {
            events.push(AgentEvent {
                ts: now,
                kind: AgentEventKind::AssistantMessage { text: text.clone() },
                ext: None,
            });
        }

        // Emit tool calls if present.
        if let Some(tool_calls) = &msg.tool_calls {
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
        let cfg = OpenAIConfig::default();
        assert!(cfg.base_url.contains("openai.com"));
        assert!(cfg.model.contains("gpt"));
        assert!(cfg.max_tokens.unwrap_or(0) > 0);
    }

    #[test]
    fn map_work_order_uses_task_as_user_message() {
        let wo = WorkOrderBuilder::new("Refactor auth module").build();
        let cfg = OpenAIConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert!(
            req.messages[0]
                .content
                .as_deref()
                .unwrap()
                .contains("Refactor auth module")
        );
    }

    #[test]
    fn map_work_order_respects_model_override() {
        let wo = WorkOrderBuilder::new("task").model("gpt-4-turbo").build();
        let cfg = OpenAIConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.model, "gpt-4-turbo");
    }

    #[test]
    fn map_response_produces_assistant_message() {
        let resp = OpenAIResponse {
            id: "chatcmpl-123".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIMessage {
                    role: "assistant".into(),
                    content: Some("Hello!".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".into()),
            }],
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
    fn map_response_handles_tool_calls() {
        let resp = OpenAIResponse {
            id: "chatcmpl-456".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![OpenAIToolCall {
                        id: "call_abc".into(),
                        call_type: "function".into(),
                        function: OpenAIFunctionCall {
                            name: "read_file".into(),
                            arguments: r#"{"path": "src/main.rs"}"#.into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".into()),
            }],
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
                assert_eq!(tool_use_id.as_deref(), Some("call_abc"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}
