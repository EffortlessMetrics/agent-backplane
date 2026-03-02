// SPDX-License-Identifier: MIT OR Apache-2.0
//! OpenAI Codex dialect: config, request/response types, sandboxing, streaming,
//! and mapping logic between ABP contract types and the Codex/Responses API.

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, SupportLevel, WorkOrder,
};
pub use abp_tooling::CanonicalToolDef;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Version string for this dialect adapter.
pub const DIALECT_VERSION: &str = "codex/v0.1";

/// Default model used when none is specified.
pub const DEFAULT_MODEL: &str = "codex-mini-latest";

// ---------------------------------------------------------------------------
// Model-name mapping
// ---------------------------------------------------------------------------

/// Known Codex/OpenAI model identifiers.
const KNOWN_MODELS: &[&str] = &[
    "codex-mini-latest",
    "o3-mini",
    "o4-mini",
    "gpt-4",
    "gpt-4o",
    "gpt-4.1",
    "gpt-4.1-mini",
    "gpt-4.1-nano",
];

/// Map a vendor model name to the ABP canonical form (`openai/<model>`).
///
/// Known models are prefixed; unknown models pass through with the prefix.
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

/// Returns `true` if `model` is a known Codex/OpenAI model identifier.
#[must_use]
pub fn is_known_model(model: &str) -> bool {
    KNOWN_MODELS.contains(&model)
}

// ---------------------------------------------------------------------------
// Capability mapping
// ---------------------------------------------------------------------------

/// Build a [`CapabilityManifest`] describing what the Codex backend supports.
#[must_use]
pub fn capability_manifest() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Native);
    m.insert(Capability::ToolEdit, SupportLevel::Native);
    m.insert(Capability::ToolBash, SupportLevel::Native);
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
// Tool types
// ---------------------------------------------------------------------------

/// OpenAI-style function tool definition (legacy format).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexToolDef {
    /// Tool type (always `"function"`).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition payload.
    pub function: CodexFunctionDef,
}

/// The function payload inside a [`CodexToolDef`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexFunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// A tool available in the Codex/Responses API.
///
/// Maps the three built-in tool types that OpenAI supports:
/// function (custom), code_interpreter, and file_search.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexTool {
    /// A user-defined function tool.
    Function {
        /// The function definition payload.
        function: CodexFunctionDef,
    },
    /// The built-in code interpreter tool for sandboxed execution.
    CodeInterpreter {},
    /// The built-in file search tool for retrieval over uploaded files.
    FileSearch {
        /// Maximum number of results to return.
        #[serde(skip_serializing_if = "Option::is_none")]
        max_num_results: Option<u32>,
    },
}

/// Convert an ABP canonical tool definition to the OpenAI function tool format.
#[must_use]
pub fn tool_def_to_codex(def: &CanonicalToolDef) -> CodexToolDef {
    CodexToolDef {
        tool_type: "function".into(),
        function: CodexFunctionDef {
            name: def.name.clone(),
            description: def.description.clone(),
            parameters: def.parameters_schema.clone(),
        },
    }
}

/// Convert an OpenAI function tool definition back to the ABP canonical form.
#[must_use]
pub fn tool_def_from_codex(def: &CodexToolDef) -> CanonicalToolDef {
    CanonicalToolDef {
        name: def.function.name.clone(),
        description: def.function.description.clone(),
        parameters_schema: def.function.parameters.clone(),
    }
}

/// Convert a [`CodexTool`] to the ABP canonical form.
///
/// Built-in tools (code_interpreter, file_search) are mapped to
/// canonical definitions with empty parameter schemas.
#[must_use]
pub fn codex_tool_to_canonical(tool: &CodexTool) -> CanonicalToolDef {
    match tool {
        CodexTool::Function { function } => CanonicalToolDef {
            name: function.name.clone(),
            description: function.description.clone(),
            parameters_schema: function.parameters.clone(),
        },
        CodexTool::CodeInterpreter {} => CanonicalToolDef {
            name: "code_interpreter".into(),
            description: "Execute code in a sandboxed environment".into(),
            parameters_schema: serde_json::json!({"type": "object"}),
        },
        CodexTool::FileSearch { .. } => CanonicalToolDef {
            name: "file_search".into(),
            description: "Search over uploaded files".into(),
            parameters_schema: serde_json::json!({"type": "object"}),
        },
    }
}

// ---------------------------------------------------------------------------
// Sandbox configuration
// ---------------------------------------------------------------------------

/// Networking policy for sandboxed execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAccess {
    /// No network access allowed.
    #[default]
    None,
    /// Only allow connections to the specified hosts.
    AllowList(Vec<String>),
    /// Full network access.
    Full,
}

/// File-system access policy for sandboxed execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileAccess {
    /// No file access beyond the workspace directory.
    #[default]
    WorkspaceOnly,
    /// Read-only access to paths outside the workspace.
    ReadOnlyExternal,
    /// Full file-system access (use with caution).
    Full,
}

/// Sandbox configuration for Codex execution environments.
///
/// Codex is execution-oriented: it runs code inside containers with
/// controlled networking and file-system access.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SandboxConfig {
    /// Container image to use for execution (e.g. `"node:20"`, `"python:3.12"`).
    pub container_image: Option<String>,

    /// Networking policy for the sandbox.
    #[serde(default)]
    pub networking: NetworkAccess,

    /// File-system access policy.
    #[serde(default)]
    pub file_access: FileAccess,

    /// Maximum wall-clock time in seconds before the sandbox is killed.
    pub timeout_seconds: Option<u32>,

    /// Maximum memory in megabytes available to the sandbox.
    pub memory_mb: Option<u32>,

    /// Environment variables injected into the sandbox.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            container_image: None,
            networking: NetworkAccess::None,
            file_access: FileAccess::WorkspaceOnly,
            timeout_seconds: Some(300),
            memory_mb: Some(512),
            env: BTreeMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Text format
// ---------------------------------------------------------------------------

/// Output text format configuration for the Responses API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexTextFormat {
    /// Plain text output (default).
    Text {},
    /// JSON object output with an optional schema.
    JsonObject {},
    /// JSON Schema-constrained output.
    JsonSchema {
        /// Name of the schema.
        name: String,
        /// The JSON Schema definition.
        schema: serde_json::Value,
        /// Whether to enforce strict schema validation.
        #[serde(default)]
        strict: bool,
    },
}

impl Default for CodexTextFormat {
    fn default() -> Self {
        Self::Text {}
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

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

    /// Sandbox configuration for execution environments.
    #[serde(default)]
    pub sandbox: SandboxConfig,
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.openai.com/v1".into(),
            model: "codex-mini-latest".into(),
            max_output_tokens: Some(4096),
            temperature: None,
            sandbox: SandboxConfig::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Simplified representation of an OpenAI Responses API request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexRequest {
    /// Model identifier (e.g. `codex-mini-latest`).
    pub model: String,
    /// Input items (messages) for the request.
    pub input: Vec<CodexInputItem>,
    /// Maximum output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Tools available to the model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<CodexTool>,
    /// Output text format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<CodexTextFormat>,
}

/// An input item in the Codex Responses API format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexInputItem {
    /// A conversation message.
    Message {
        /// Message role (`user`, `assistant`, or `system`).
        role: String,
        /// Text content of the message.
        content: String,
    },
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Simplified representation of an OpenAI Responses API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexResponse {
    /// Unique response identifier.
    pub id: String,
    /// Model used for the completion.
    pub model: String,
    /// Output items produced by the model.
    pub output: Vec<CodexResponseItem>,
    /// Token usage statistics.
    pub usage: Option<CodexUsage>,
    /// Response status (`completed`, `in_progress`, `failed`, `cancelled`).
    #[serde(default)]
    pub status: Option<String>,
}

/// A response item in the Codex Responses API format.
///
/// Models the four output item types: message, function_call,
/// function_call_output, and reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexResponseItem {
    /// An assistant message with content parts.
    Message {
        /// Message role.
        role: String,
        /// Content parts of the message.
        content: Vec<CodexContentPart>,
    },
    /// A function call requested by the model.
    FunctionCall {
        /// Unique function call identifier.
        id: String,
        /// Correlation ID linking the call to a prior request.
        #[serde(default)]
        call_id: Option<String>,
        /// Name of the function to invoke.
        name: String,
        /// JSON-encoded arguments.
        arguments: String,
    },
    /// Output from a previously executed function call.
    FunctionCallOutput {
        /// Correlation ID linking back to the function call.
        call_id: String,
        /// The output value from the function.
        output: String,
    },
    /// Internal reasoning / chain-of-thought from the model.
    Reasoning {
        /// Reasoning text fragments.
        #[serde(default)]
        summary: Vec<ReasoningSummary>,
    },
}

/// A summary fragment within a reasoning response item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReasoningSummary {
    /// The reasoning text.
    pub text: String,
}

/// A content part within a Codex output message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexContentPart {
    /// Text output from the model.
    OutputText {
        /// The text content.
        text: String,
    },
}

/// Token usage reported by the OpenAI API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexUsage {
    /// Tokens consumed by the input.
    pub input_tokens: u64,
    /// Tokens generated in the output.
    pub output_tokens: u64,
    /// Total tokens (input + output).
    pub total_tokens: u64,
}

// ---------------------------------------------------------------------------
// Legacy type aliases (backward compatibility)
// ---------------------------------------------------------------------------

/// Legacy alias for [`CodexResponseItem`].
pub type CodexOutputItem = CodexResponseItem;

// ---------------------------------------------------------------------------
// Streaming types
// ---------------------------------------------------------------------------

/// Server-sent events emitted during a Codex streaming response.
///
/// Event names follow the OpenAI convention: `response.created`,
/// `response.in_progress`, `response.output_item.*`, `response.completed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexStreamEvent {
    /// The response object has been created (`response.created`).
    ResponseCreated {
        /// The initial (incomplete) response.
        response: CodexResponse,
    },
    /// The response is being processed (`response.in_progress`).
    ResponseInProgress {
        /// The in-progress response snapshot.
        response: CodexResponse,
    },
    /// A new output item has been added (`response.output_item.added`).
    OutputItemAdded {
        /// Index of the item in the output array.
        output_index: usize,
        /// The newly added item.
        item: CodexResponseItem,
    },
    /// An output item is being streamed (`response.output_item.delta`).
    OutputItemDelta {
        /// Index of the item in the output array.
        output_index: usize,
        /// The partial delta payload.
        delta: CodexStreamDelta,
    },
    /// An output item has been finalized (`response.output_item.done`).
    OutputItemDone {
        /// Index of the item in the output array.
        output_index: usize,
        /// The finalized item.
        item: CodexResponseItem,
    },
    /// The response has completed successfully (`response.completed`).
    ResponseCompleted {
        /// The final response.
        response: CodexResponse,
    },
    /// The response has failed (`response.failed`).
    ResponseFailed {
        /// The failed response with error information.
        response: CodexResponse,
    },
    /// An error occurred during streaming.
    Error {
        /// Error message.
        message: String,
        /// Error code.
        #[serde(default)]
        code: Option<String>,
    },
}

/// Delta payload for incremental streaming updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexStreamDelta {
    /// Incremental text content.
    OutputTextDelta {
        /// The text fragment.
        text: String,
    },
    /// Incremental function call arguments.
    FunctionCallArgumentsDelta {
        /// The arguments fragment.
        delta: String,
    },
    /// Incremental reasoning summary.
    ReasoningSummaryDelta {
        /// The reasoning fragment.
        text: String,
    },
}

// ---------------------------------------------------------------------------
// Mapping: WorkOrder → CodexRequest
// ---------------------------------------------------------------------------

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
        user_content.push_str(&format!(
            "\n\n--- {} ---\n{}",
            snippet.name, snippet.content
        ));
    }

    CodexRequest {
        model,
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: user_content,
        }],
        max_output_tokens: config.max_output_tokens,
        temperature: config.temperature,
        tools: Vec::new(),
        text: None,
    }
}

// ---------------------------------------------------------------------------
// Mapping: CodexResponse → Vec<AgentEvent>
// ---------------------------------------------------------------------------

/// Map a [`CodexResponse`] back to a sequence of ABP [`AgentEvent`]s.
pub fn map_response(resp: &CodexResponse) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let now = Utc::now();

    for item in &resp.output {
        match item {
            CodexResponseItem::Message { content, .. } => {
                for part in content {
                    match part {
                        CodexContentPart::OutputText { text } => {
                            events.push(AgentEvent {
                                ts: now,
                                kind: AgentEventKind::AssistantMessage { text: text.clone() },
                                ext: None,
                            });
                        }
                    }
                }
            }
            CodexResponseItem::FunctionCall {
                id,
                name,
                arguments,
                ..
            } => {
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
            CodexResponseItem::FunctionCallOutput { call_id, output } => {
                events.push(AgentEvent {
                    ts: now,
                    kind: AgentEventKind::ToolResult {
                        tool_name: "function".into(),
                        tool_use_id: Some(call_id.clone()),
                        output: serde_json::Value::String(output.clone()),
                        is_error: false,
                    },
                    ext: None,
                });
            }
            CodexResponseItem::Reasoning { summary } => {
                let text = summary
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    events.push(AgentEvent {
                        ts: now,
                        kind: AgentEventKind::AssistantDelta { text },
                        ext: None,
                    });
                }
            }
        }
    }

    events
}

// ---------------------------------------------------------------------------
// Mapping: CodexStreamEvent → Vec<AgentEvent>
// ---------------------------------------------------------------------------

/// Map a [`CodexStreamEvent`] to a sequence of ABP [`AgentEvent`]s.
///
/// Some events (like `response.in_progress`) produce no ABP events.
pub fn map_stream_event(event: &CodexStreamEvent) -> Vec<AgentEvent> {
    let now = Utc::now();

    match event {
        CodexStreamEvent::ResponseCreated { .. } => {
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::RunStarted {
                    message: "Codex stream started".into(),
                },
                ext: None,
            }]
        }
        CodexStreamEvent::ResponseInProgress { .. } => {
            // No ABP event for in-progress status updates.
            vec![]
        }
        CodexStreamEvent::OutputItemAdded { item, .. } => map_response_item(item, now),
        CodexStreamEvent::OutputItemDelta { delta, .. } => match delta {
            CodexStreamDelta::OutputTextDelta { text } => {
                vec![AgentEvent {
                    ts: now,
                    kind: AgentEventKind::AssistantDelta { text: text.clone() },
                    ext: None,
                }]
            }
            CodexStreamDelta::FunctionCallArgumentsDelta { .. }
            | CodexStreamDelta::ReasoningSummaryDelta { .. } => {
                // Partial function args and reasoning fragments don't map
                // to a top-level ABP event; they accumulate server-side.
                vec![]
            }
        },
        CodexStreamEvent::OutputItemDone { item, .. } => map_response_item(item, now),
        CodexStreamEvent::ResponseCompleted { .. } => {
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::RunCompleted {
                    message: "Codex stream completed".into(),
                },
                ext: None,
            }]
        }
        CodexStreamEvent::ResponseFailed { response } => {
            let message = response
                .status
                .as_deref()
                .unwrap_or("unknown failure")
                .to_string();
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::Error { message },
                ext: None,
            }]
        }
        CodexStreamEvent::Error { message, .. } => {
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::Error {
                    message: message.clone(),
                },
                ext: None,
            }]
        }
    }
}

/// Helper: map a single [`CodexResponseItem`] to agent events.
fn map_response_item(item: &CodexResponseItem, ts: chrono::DateTime<Utc>) -> Vec<AgentEvent> {
    match item {
        CodexResponseItem::Message { content, .. } => content
            .iter()
            .map(|part| match part {
                CodexContentPart::OutputText { text } => AgentEvent {
                    ts,
                    kind: AgentEventKind::AssistantMessage { text: text.clone() },
                    ext: None,
                },
            })
            .collect(),
        CodexResponseItem::FunctionCall {
            id,
            name,
            arguments,
            ..
        } => {
            let input = serde_json::from_str(arguments)
                .unwrap_or(serde_json::Value::String(arguments.clone()));
            vec![AgentEvent {
                ts,
                kind: AgentEventKind::ToolCall {
                    tool_name: name.clone(),
                    tool_use_id: Some(id.clone()),
                    parent_tool_use_id: None,
                    input,
                },
                ext: None,
            }]
        }
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            vec![AgentEvent {
                ts,
                kind: AgentEventKind::ToolResult {
                    tool_name: "function".into(),
                    tool_use_id: Some(call_id.clone()),
                    output: serde_json::Value::String(output.clone()),
                    is_error: false,
                },
                ext: None,
            }]
        }
        CodexResponseItem::Reasoning { summary } => {
            let text = summary
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if text.is_empty() {
                return vec![];
            }
            vec![AgentEvent {
                ts,
                kind: AgentEventKind::AssistantDelta { text },
                ext: None,
            }]
        }
    }
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
            output: vec![CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Done!".into(),
                }],
            }],
            usage: None,
            status: None,
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
            output: vec![CodexResponseItem::FunctionCall {
                id: "fc_1".into(),
                call_id: None,
                name: "shell".into(),
                arguments: r#"{"command":"ls"}"#.into(),
            }],
            usage: None,
            status: None,
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "shell");
                assert_eq!(tool_use_id.as_deref(), Some("fc_1"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}
