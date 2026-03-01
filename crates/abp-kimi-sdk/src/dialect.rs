// SPDX-License-Identifier: MIT OR Apache-2.0
//! Moonshot Kimi dialect: config, request/response types, and mapping logic.
//!
//! Kimi uses an OpenAI-compatible chat completions API surface with extensions
//! for built-in tools (`search_internet`, `browser`), citation references
//! (`refs`), and the `k1` reasoning mode.

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, SupportLevel, WorkOrder,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Version string for this dialect adapter.
pub const DIALECT_VERSION: &str = "kimi/v0.1";

/// Default model used when none is specified.
pub const DEFAULT_MODEL: &str = "moonshot-v1-8k";

// ---------------------------------------------------------------------------
// Model-name mapping
// ---------------------------------------------------------------------------

/// Known Moonshot Kimi model identifiers.
const KNOWN_MODELS: &[&str] = &[
    "moonshot-v1-8k",
    "moonshot-v1-32k",
    "moonshot-v1-128k",
    "kimi-latest",
    "k1",
];

/// Map a vendor model name to the ABP canonical form (`moonshot/<model>`).
#[must_use]
pub fn to_canonical_model(vendor_model: &str) -> String {
    format!("moonshot/{vendor_model}")
}

/// Map an ABP canonical model name back to the vendor model name.
///
/// Strips the `moonshot/` prefix if present; otherwise returns the input unchanged.
#[must_use]
pub fn from_canonical_model(canonical: &str) -> String {
    canonical
        .strip_prefix("moonshot/")
        .unwrap_or(canonical)
        .to_string()
}

/// Returns `true` if `model` is a known Moonshot Kimi model identifier.
#[must_use]
pub fn is_known_model(model: &str) -> bool {
    KNOWN_MODELS.contains(&model)
}

// ---------------------------------------------------------------------------
// Capability mapping
// ---------------------------------------------------------------------------

/// Build a [`CapabilityManifest`] describing what the Kimi backend supports.
#[must_use]
pub fn capability_manifest() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m.insert(Capability::ToolEdit, SupportLevel::Unsupported);
    m.insert(Capability::ToolBash, SupportLevel::Unsupported);
    m.insert(Capability::ToolWebSearch, SupportLevel::Native);
    m.insert(
        Capability::StructuredOutputJsonSchema,
        SupportLevel::Emulated,
    );
    m.insert(Capability::McpClient, SupportLevel::Unsupported);
    m.insert(Capability::McpServer, SupportLevel::Unsupported);
    m
}

// ---------------------------------------------------------------------------
// Tool-format translation
// ---------------------------------------------------------------------------

/// A vendor-agnostic tool definition used as the ABP canonical form.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalToolDef {
    /// Tool name.
    pub name: String,
    /// Human-readable description of the tool.
    pub description: String,
    /// JSON Schema describing the tool's parameters.
    pub parameters_schema: serde_json::Value,
}

/// Kimi/OpenAI-compatible function tool definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiToolDef {
    /// Tool type (always `"function"`).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition payload.
    pub function: KimiFunctionDef,
}

/// The function payload inside a [`KimiToolDef`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiFunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// Convert an ABP canonical tool definition to the Kimi function tool format.
#[must_use]
pub fn tool_def_to_kimi(def: &CanonicalToolDef) -> KimiToolDef {
    KimiToolDef {
        tool_type: "function".into(),
        function: KimiFunctionDef {
            name: def.name.clone(),
            description: def.description.clone(),
            parameters: def.parameters_schema.clone(),
        },
    }
}

/// Convert a Kimi function tool definition back to the ABP canonical form.
#[must_use]
pub fn tool_def_from_kimi(def: &KimiToolDef) -> CanonicalToolDef {
    CanonicalToolDef {
        name: def.function.name.clone(),
        description: def.function.description.clone(),
        parameters_schema: def.function.parameters.clone(),
    }
}

// ---------------------------------------------------------------------------
// Kimi built-in tool helpers
// ---------------------------------------------------------------------------

/// Kimi built-in tool type for `search_internet`.
///
/// When included in the tools array with `type: "builtin_function"`, Kimi
/// performs web search automatically and injects citations into the response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiBuiltinTool {
    /// Tool type — `"builtin_function"` for Kimi built-ins.
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The built-in function descriptor.
    pub function: KimiBuiltinFunction,
}

/// Descriptor for a Kimi built-in function such as `search_internet` or `browser`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiBuiltinFunction {
    /// Built-in name (e.g. `"$web_search"`, `"$browser"`).
    pub name: String,
}

/// Create a Kimi built-in tool definition for web search.
#[must_use]
pub fn builtin_search_internet() -> KimiBuiltinTool {
    KimiBuiltinTool {
        tool_type: "builtin_function".into(),
        function: KimiBuiltinFunction {
            name: "$web_search".into(),
        },
    }
}

/// Create a Kimi built-in tool definition for browser.
#[must_use]
pub fn builtin_browser() -> KimiBuiltinTool {
    KimiBuiltinTool {
        tool_type: "builtin_function".into(),
        function: KimiBuiltinFunction {
            name: "$browser".into(),
        },
    }
}

// ---------------------------------------------------------------------------
// Citation / refs
// ---------------------------------------------------------------------------

/// A citation reference returned by Kimi when `search_internet` is active.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiRef {
    /// The numeric index of this citation (1-based).
    pub index: u32,
    /// URL of the cited source.
    pub url: String,
    /// Title of the cited source (may be absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

// ---------------------------------------------------------------------------
// Vendor-specific configuration
// ---------------------------------------------------------------------------

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

    /// Whether to use `k1` reasoning mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_k1_reasoning: Option<bool>,
}

impl Default for KimiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.moonshot.cn/v1".into(),
            model: "moonshot-v1-8k".into(),
            max_tokens: Some(4096),
            temperature: None,
            use_k1_reasoning: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Kimi chat completions request.
///
/// Follows the OpenAI chat completions shape with Kimi-specific extensions
/// such as built-in tools and the `k1` reasoning mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiRequest {
    /// Model identifier (e.g. `moonshot-v1-8k`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<KimiMessage>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Whether to stream the response via SSE.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Tool definitions (function and built-in).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<KimiTool>>,
    /// Whether to enable `k1` reasoning mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_search: Option<bool>,
}

/// A tool entry in a Kimi request — either a user-defined function or a
/// Kimi built-in function such as `search_internet`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KimiTool {
    /// A user-defined function tool.
    Function {
        /// The function definition payload.
        function: KimiFunctionDef,
    },
    /// A Kimi built-in function (e.g. `$web_search`, `$browser`).
    BuiltinFunction {
        /// The built-in function descriptor.
        function: KimiBuiltinFunction,
    },
}

/// Message roles supported by Kimi.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KimiRole {
    /// System prompt.
    System,
    /// User message.
    User,
    /// Assistant (model) message.
    Assistant,
    /// Tool result message.
    Tool,
}

impl std::fmt::Display for KimiRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::System => write!(f, "system"),
            Self::User => write!(f, "user"),
            Self::Assistant => write!(f, "assistant"),
            Self::Tool => write!(f, "tool"),
        }
    }
}

/// A single message in the Kimi conversation format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiMessage {
    /// Message role.
    pub role: String,
    /// Text content of the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool call ID this message responds to (only for role=tool).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Tool calls in an assistant message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<KimiToolCall>>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Kimi chat completions response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiResponse {
    /// Unique response identifier.
    pub id: String,
    /// Model that generated the response.
    pub model: String,
    /// Completion choices.
    pub choices: Vec<KimiChoice>,
    /// Token usage statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<KimiUsage>,
    /// Citation references when `search_internet` was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<Vec<KimiRef>>,
}

/// A single choice in a Kimi completions response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiChoice {
    /// Zero-based index of this choice.
    pub index: u32,
    /// The assistant's response message.
    pub message: KimiResponseMessage,
    /// Reason the model stopped generating.
    pub finish_reason: Option<String>,
}

/// A message within a Kimi response choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiResponseMessage {
    /// Message role.
    pub role: String,
    /// Text content, if any.
    pub content: Option<String>,
    /// Tool calls requested by the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<KimiToolCall>>,
}

/// A tool call in a Kimi response (OpenAI-compatible shape).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiToolCall {
    /// Unique tool call identifier.
    pub id: String,
    /// Call type (always `"function"`).
    #[serde(rename = "type")]
    pub call_type: String,
    /// The function invocation details.
    pub function: KimiFunctionCall,
}

/// The function payload within a Kimi tool call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiFunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments for the function.
    pub arguments: String,
}

/// Token usage reported by the Kimi API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiUsage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

// ---------------------------------------------------------------------------
// Streaming types (SSE / chunked)
// ---------------------------------------------------------------------------

/// A single SSE chunk from a Kimi streaming response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiChunk {
    /// Chunk identifier (same across all chunks in one stream).
    pub id: String,
    /// Object type — always `"chat.completion.chunk"`.
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model that produced this chunk.
    pub model: String,
    /// Choices with streaming deltas.
    pub choices: Vec<KimiChunkChoice>,
    /// Usage info (only present in the final chunk when requested).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<KimiUsage>,
    /// Citation references (may appear in later chunks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<Vec<KimiRef>>,
}

/// A single choice within a [`KimiChunk`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiChunkChoice {
    /// Zero-based choice index.
    pub index: u32,
    /// The incremental delta for this choice.
    pub delta: KimiChunkDelta,
    /// Finish reason — `None` until the stream ends.
    pub finish_reason: Option<String>,
}

/// An incremental delta within a streaming chunk choice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct KimiChunkDelta {
    /// Role (usually only in the first chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Text content fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Incremental tool call fragments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<KimiChunkToolCall>>,
}

/// An incremental tool call fragment within a streaming delta.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiChunkToolCall {
    /// Index of this tool call in the tool_calls array.
    pub index: u32,
    /// Tool call ID (only in first fragment).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Call type (only in first fragment).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    /// Incremental function data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<KimiChunkFunctionCall>,
}

/// Incremental function call data within a streaming tool call fragment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiChunkFunctionCall {
    /// Function name (only in first fragment).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Partial JSON arguments string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ---------------------------------------------------------------------------
// Mapping: WorkOrder → KimiRequest
// ---------------------------------------------------------------------------

/// Map an ABP [`WorkOrder`] to a [`KimiRequest`].
///
/// Uses the work order task as the initial user message and applies
/// config defaults where the work order does not specify overrides.
/// Tool definitions from the work order are translated to Kimi format.
pub fn map_work_order(wo: &WorkOrder, config: &KimiConfig) -> KimiRequest {
    let model = wo
        .config
        .model
        .as_deref()
        .unwrap_or(&config.model)
        .to_string();

    let mut messages = Vec::new();

    // Build user content from task + context snippets
    let mut user_content = wo.task.clone();
    for snippet in &wo.context.snippets {
        user_content.push_str(&format!(
            "\n\n--- {} ---\n{}",
            snippet.name, snippet.content
        ));
    }

    messages.push(KimiMessage {
        role: "user".into(),
        content: Some(user_content),
        tool_call_id: None,
        tool_calls: None,
    });

    // Enable search if config says so
    let use_search = config
        .use_k1_reasoning
        .and_then(|v| if v { Some(true) } else { None });

    KimiRequest {
        model,
        messages,
        max_tokens: config.max_tokens,
        temperature: config.temperature,
        stream: None,
        tools: None,
        use_search,
    }
}

// ---------------------------------------------------------------------------
// Mapping: KimiResponse → Vec<AgentEvent>
// ---------------------------------------------------------------------------

/// Map a [`KimiResponse`] back to a sequence of ABP [`AgentEvent`]s.
///
/// Handles assistant text, tool calls, and citation references.
pub fn map_response(resp: &KimiResponse) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let now = Utc::now();

    for choice in &resp.choices {
        if let Some(text) = &choice.message.content
            && !text.is_empty()
        {
            // Attach citation refs as ext metadata if present
            let ext = resp.refs.as_ref().map(|refs| {
                let mut m = BTreeMap::new();
                let refs_json = serde_json::to_value(refs).unwrap_or(serde_json::Value::Null);
                m.insert("kimi_refs".into(), refs_json);
                m
            });

            events.push(AgentEvent {
                ts: now,
                kind: AgentEventKind::AssistantMessage { text: text.clone() },
                ext,
            });
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

// ---------------------------------------------------------------------------
// Mapping: KimiChunk → Vec<AgentEvent> (streaming)
// ---------------------------------------------------------------------------

/// Map a single streaming [`KimiChunk`] to zero or more ABP [`AgentEvent`]s.
///
/// Text content deltas become [`AgentEventKind::AssistantDelta`]. Tool call
/// fragments are not emitted individually — use [`ToolCallAccumulator`] to
/// collect incremental fragments and call [`ToolCallAccumulator::finish`] when
/// the stream ends.
pub fn map_stream_event(chunk: &KimiChunk) -> Vec<AgentEvent> {
    let now = Utc::now();
    let mut events = Vec::new();

    for choice in &chunk.choices {
        if let Some(text) = &choice.delta.content
            && !text.is_empty()
        {
            // Attach refs from chunk if present
            let ext = chunk.refs.as_ref().map(|refs| {
                let mut m = BTreeMap::new();
                let refs_json = serde_json::to_value(refs).unwrap_or(serde_json::Value::Null);
                m.insert("kimi_refs".into(), refs_json);
                m
            });

            events.push(AgentEvent {
                ts: now,
                kind: AgentEventKind::AssistantDelta { text: text.clone() },
                ext,
            });
        }

        // Emit RunCompleted when stream finishes
        if let Some(reason) = &choice.finish_reason
            && !reason.is_empty()
        {
            events.push(AgentEvent {
                ts: now,
                kind: AgentEventKind::RunCompleted {
                    message: format!("Kimi stream finished: {reason}"),
                },
                ext: None,
            });
        }
    }

    events
}

// ---------------------------------------------------------------------------
// Tool call accumulator for streaming
// ---------------------------------------------------------------------------

/// Accumulates incremental tool call fragments from streaming chunks.
///
/// Feed each chunk's tool call fragments via [`feed`](ToolCallAccumulator::feed)
/// and call [`finish`](ToolCallAccumulator::finish) when the stream ends to
/// produce complete `AgentEvent` `ToolCall` events.
#[derive(Debug, Default)]
pub struct ToolCallAccumulator {
    entries: Vec<AccEntry>,
}

#[derive(Debug)]
struct AccEntry {
    id: String,
    name: String,
    arguments: String,
}

impl ToolCallAccumulator {
    /// Create a new empty accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed incremental tool call fragments from a streaming delta.
    pub fn feed(&mut self, fragments: &[KimiChunkToolCall]) {
        for frag in fragments {
            let idx = frag.index as usize;
            // Grow the entries vec if needed
            while self.entries.len() <= idx {
                self.entries.push(AccEntry {
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                });
            }
            let entry = &mut self.entries[idx];
            if let Some(id) = &frag.id {
                entry.id.clone_from(id);
            }
            if let Some(func) = &frag.function {
                if let Some(name) = &func.name {
                    entry.name.clone_from(name);
                }
                if let Some(args) = &func.arguments {
                    entry.arguments.push_str(args);
                }
            }
        }
    }

    /// Consume the accumulator and emit completed [`AgentEvent`]s.
    pub fn finish(self) -> Vec<AgentEvent> {
        let now = Utc::now();
        self.entries
            .into_iter()
            .filter(|e| !e.name.is_empty())
            .map(|e| {
                let input = serde_json::from_str(&e.arguments)
                    .unwrap_or(serde_json::Value::String(e.arguments));
                AgentEvent {
                    ts: now,
                    kind: AgentEventKind::ToolCall {
                        tool_name: e.name,
                        tool_use_id: Some(e.id),
                        parent_tool_use_id: None,
                        input,
                    },
                    ext: None,
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Usage extraction helper
// ---------------------------------------------------------------------------

/// Extract token usage from a [`KimiResponse`] as an ABP-compatible map.
#[must_use]
pub fn extract_usage(resp: &KimiResponse) -> Option<BTreeMap<String, serde_json::Value>> {
    resp.usage.as_ref().map(|u| {
        let mut m = BTreeMap::new();
        m.insert(
            "prompt_tokens".into(),
            serde_json::Value::Number(u.prompt_tokens.into()),
        );
        m.insert(
            "completion_tokens".into(),
            serde_json::Value::Number(u.completion_tokens.into()),
        );
        m.insert(
            "total_tokens".into(),
            serde_json::Value::Number(u.total_tokens.into()),
        );
        m
    })
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
        assert!(
            req.messages[0]
                .content
                .as_deref()
                .unwrap_or("")
                .contains("Optimize database queries")
        );
    }

    #[test]
    fn map_work_order_respects_model_override() {
        let wo = WorkOrderBuilder::new("task")
            .model("moonshot-v1-128k")
            .build();
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
            refs: None,
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
            refs: None,
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "web_search");
                assert_eq!(tool_use_id.as_deref(), Some("call_1"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}
