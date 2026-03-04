// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Codex Responses API types.
//!
//! Contains Codex-specific request/response types, tool call/result types,
//! sandbox configuration, the request builder, and token usage statistics.
//!
//! These types extend the base Codex SDK types with shim-specific fields
//! such as `instructions`, `context`, and `sandbox`.

use abp_codex_sdk::dialect::{
    CodexContentPart, CodexInputItem, CodexRequest, CodexResponseItem, CodexStreamDelta,
    CodexTextFormat, CodexTool, SandboxConfig,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Codex-specific request ──────────────────────────────────────────────

/// A file context entry attached to a Codex request.
///
/// Represents a file (or snippet) that the agent should have access to
/// during execution, such as source files in the workspace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexContextItem {
    /// Path to the file (relative to workspace root).
    pub path: String,
    /// Content of the file, if pre-loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Extended Codex request with shim-specific fields.
///
/// Wraps the base [`CodexRequest`] and adds Codex-specific fields like
/// `instructions` (system-level prompt), `context` (file context), and
/// `sandbox` (execution environment settings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexExtendedRequest {
    /// Model identifier (e.g. `codex-mini-latest`, `o3-mini`).
    pub model: String,
    /// Input items (messages) for the request.
    pub input: Vec<CodexInputItem>,
    /// System-level instructions prepended to the conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// File context items the model should consider.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<CodexContextItem>,
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
    /// Sandbox configuration for execution environments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<CodexSandboxConfig>,
    /// Vendor-specific extension fields.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl CodexExtendedRequest {
    /// Convert this extended request into the base [`CodexRequest`].
    ///
    /// Instructions are prepended as a system message in the input.
    /// Context items and sandbox config are not represented in the base type.
    #[must_use]
    pub fn to_base_request(&self) -> CodexRequest {
        let mut input = Vec::new();
        if let Some(instructions) = &self.instructions {
            input.push(CodexInputItem::Message {
                role: "system".into(),
                content: instructions.clone(),
            });
        }
        input.extend(self.input.clone());
        CodexRequest {
            model: self.model.clone(),
            input,
            max_output_tokens: self.max_output_tokens,
            temperature: self.temperature,
            tools: self.tools.clone(),
            text: self.text.clone(),
        }
    }
}

// ── Codex-specific response ─────────────────────────────────────────────

/// Extended Codex response with shim-specific metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexExtendedResponse {
    /// Unique response identifier.
    pub id: String,
    /// Model used for the completion.
    pub model: String,
    /// Output items produced by the model.
    pub output: Vec<CodexResponseItem>,
    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Response status (`completed`, `in_progress`, `failed`, `cancelled`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Sandbox execution metadata, if the request used a sandbox.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_result: Option<CodexSandboxResult>,
    /// Vendor-specific extension fields.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

/// Result metadata from sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexSandboxResult {
    /// Exit code from the sandbox process, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Wall-clock execution time in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Files modified during sandbox execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_modified: Vec<String>,
}

// ── Codex stream event ──────────────────────────────────────────────────

/// A Codex-specific stream event with shim metadata.
///
/// Wraps the base stream event kinds and adds a sequence number for
/// ordering and an optional error detail field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexShimStreamEvent {
    /// The response object has been created.
    ResponseCreated {
        /// Sequence number of this event.
        sequence: u64,
        /// The initial response ID.
        response_id: String,
        /// Model name.
        model: String,
    },
    /// Incremental text content delta.
    TextDelta {
        /// Sequence number of this event.
        sequence: u64,
        /// Index of the output item being streamed.
        output_index: usize,
        /// The text fragment.
        text: String,
    },
    /// A function call delta (incremental arguments).
    FunctionCallDelta {
        /// Sequence number of this event.
        sequence: u64,
        /// Index of the output item.
        output_index: usize,
        /// The arguments fragment.
        delta: String,
    },
    /// A complete output item has been finalized.
    OutputItemDone {
        /// Sequence number of this event.
        sequence: u64,
        /// Index of the output item.
        output_index: usize,
        /// The finalized item.
        item: CodexResponseItem,
    },
    /// The response has completed.
    ResponseCompleted {
        /// Sequence number of this event.
        sequence: u64,
        /// Final response ID.
        response_id: String,
        /// Token usage, if available.
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
    },
    /// An error occurred during streaming.
    Error {
        /// Sequence number of this event.
        sequence: u64,
        /// Error message.
        message: String,
        /// Machine-readable error code.
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
}

// ── Tool call / result types ────────────────────────────────────────────

/// A Codex-specific tool call with execution metadata.
///
/// Extends the basic function call with sandbox and approval fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// Name of the tool/function to invoke.
    pub name: String,
    /// JSON-encoded arguments for the function.
    pub arguments: String,
    /// Whether this tool call requires user approval before execution.
    #[serde(default)]
    pub requires_approval: bool,
    /// Sandbox configuration override for this specific call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_override: Option<CodexSandboxConfig>,
}

/// The result of a Codex tool call execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexToolResult {
    /// The tool call ID this result corresponds to.
    pub call_id: String,
    /// The output from the tool execution.
    pub output: String,
    /// Whether the tool execution resulted in an error.
    #[serde(default)]
    pub is_error: bool,
    /// Exit code from sandbox execution, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Execution duration in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

// ── Sandbox configuration ───────────────────────────────────────────────

/// Shim-level sandbox configuration for Codex execution.
///
/// This is a simplified view of sandbox settings suitable for the shim
/// layer. It maps to/from the SDK's [`SandboxConfig`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexSandboxConfig {
    /// Container image to use (e.g. `"node:20"`, `"python:3.12"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_image: Option<String>,
    /// Whether network access is enabled in the sandbox.
    #[serde(default)]
    pub network_enabled: bool,
    /// Maximum wall-clock time in seconds before the sandbox is killed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    /// Maximum memory in megabytes available to the sandbox.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u32>,
    /// Environment variables injected into the sandbox.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

impl Default for CodexSandboxConfig {
    fn default() -> Self {
        Self {
            container_image: None,
            network_enabled: false,
            timeout_seconds: Some(300),
            memory_mb: Some(512),
            env: BTreeMap::new(),
        }
    }
}

impl CodexSandboxConfig {
    /// Convert to the SDK [`SandboxConfig`].
    #[must_use]
    pub fn to_sdk_config(&self) -> SandboxConfig {
        use abp_codex_sdk::dialect::{FileAccess, NetworkAccess};
        SandboxConfig {
            container_image: self.container_image.clone(),
            networking: if self.network_enabled {
                NetworkAccess::Full
            } else {
                NetworkAccess::None
            },
            file_access: FileAccess::WorkspaceOnly,
            timeout_seconds: self.timeout_seconds,
            memory_mb: self.memory_mb,
            env: self.env.clone(),
        }
    }

    /// Create from the SDK [`SandboxConfig`].
    #[must_use]
    pub fn from_sdk_config(sdk: &SandboxConfig) -> Self {
        use abp_codex_sdk::dialect::NetworkAccess;
        Self {
            container_image: sdk.container_image.clone(),
            network_enabled: !matches!(sdk.networking, NetworkAccess::None),
            timeout_seconds: sdk.timeout_seconds,
            memory_mb: sdk.memory_mb,
            env: sdk.env.clone(),
        }
    }
}

// ── Request builder ─────────────────────────────────────────────────────

/// Builder for [`CodexRequest`].
#[derive(Debug, Default)]
pub struct CodexRequestBuilder {
    model: Option<String>,
    input: Vec<CodexInputItem>,
    max_output_tokens: Option<u32>,
    temperature: Option<f64>,
    tools: Vec<CodexTool>,
    text: Option<CodexTextFormat>,
    instructions: Option<String>,
    context: Vec<CodexContextItem>,
    sandbox: Option<CodexSandboxConfig>,
}

impl CodexRequestBuilder {
    /// Create a new builder for a Codex request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the model.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the input items.
    #[must_use]
    pub fn input(mut self, input: Vec<CodexInputItem>) -> Self {
        self.input = input;
        self
    }

    /// Set the maximum output tokens.
    #[must_use]
    pub fn max_output_tokens(mut self, max: u32) -> Self {
        self.max_output_tokens = Some(max);
        self
    }

    /// Set the temperature.
    #[must_use]
    pub fn temperature(mut self, temp: f64) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set the tools.
    #[must_use]
    pub fn tools(mut self, tools: Vec<CodexTool>) -> Self {
        self.tools = tools;
        self
    }

    /// Set the text format.
    #[must_use]
    pub fn text(mut self, text: CodexTextFormat) -> Self {
        self.text = Some(text);
        self
    }

    /// Set system-level instructions.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Set file context items.
    #[must_use]
    pub fn context(mut self, context: Vec<CodexContextItem>) -> Self {
        self.context = context;
        self
    }

    /// Set sandbox configuration.
    #[must_use]
    pub fn sandbox(mut self, sandbox: CodexSandboxConfig) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    /// Build the request, defaulting model to `"codex-mini-latest"` if unset.
    #[must_use]
    pub fn build(self) -> CodexRequest {
        CodexRequest {
            model: self.model.unwrap_or_else(|| "codex-mini-latest".into()),
            input: self.input,
            max_output_tokens: self.max_output_tokens,
            temperature: self.temperature,
            tools: self.tools,
            text: self.text,
        }
    }

    /// Build an extended request with Codex-specific fields.
    #[must_use]
    pub fn build_extended(self) -> CodexExtendedRequest {
        CodexExtendedRequest {
            model: self.model.unwrap_or_else(|| "codex-mini-latest".into()),
            input: self.input,
            instructions: self.instructions,
            context: self.context,
            max_output_tokens: self.max_output_tokens,
            temperature: self.temperature,
            tools: self.tools,
            text: self.text,
            sandbox: self.sandbox,
            metadata: BTreeMap::new(),
        }
    }
}

// ── Token usage ─────────────────────────────────────────────────────────

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    /// Tokens consumed by the input.
    pub input_tokens: u64,
    /// Tokens generated in the output.
    pub output_tokens: u64,
    /// Total tokens (input + output).
    pub total_tokens: u64,
}

// ── ResponseItem ────────────────────────────────────────────────────────

/// A response output item matching the Codex Responses API surface.
///
/// Covers all item types returned in the `output` array: messages,
/// function calls/outputs, file search calls, code interpreter calls, etc.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseItem {
    /// An assistant or system message.
    Message {
        /// Role of the message author.
        role: String,
        /// Content parts of the message.
        content: Vec<ResponseContentPart>,
    },
    /// A function call emitted by the model.
    FunctionCall {
        /// Unique ID for this function call.
        id: String,
        /// Call ID for correlation with function call output.
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        /// Function name.
        name: String,
        /// JSON-encoded arguments.
        arguments: String,
    },
    /// Output from a function call (fed back as input).
    FunctionCallOutput {
        /// The call ID this output corresponds to.
        call_id: String,
        /// The function output text.
        output: String,
    },
    /// A file search call result.
    FileSearchCall {
        /// Unique ID for this call.
        id: String,
        /// The search queries issued.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        queries: Vec<String>,
        /// Search results returned.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        results: Vec<FileSearchResult>,
    },
    /// A code interpreter call result.
    CodeInterpreterCall {
        /// Unique ID for this call.
        id: String,
        /// The code that was executed.
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        /// Output logs from execution.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        outputs: Vec<CodeInterpreterOutput>,
    },
    /// A reasoning summary item.
    Reasoning {
        /// Summary text fragments.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        summary: Vec<ReasoningSummaryPart>,
    },
}

/// Content part within a [`ResponseItem::Message`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseContentPart {
    /// A text output part.
    OutputText {
        /// The text content.
        text: String,
    },
    /// A refusal part (when the model declines).
    Refusal {
        /// The refusal reason.
        refusal: String,
    },
}

/// A single file search result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileSearchResult {
    /// File ID or path.
    pub file_id: String,
    /// The file name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// Relevance score (0.0 to 1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// Matched text snippet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Output from a code interpreter execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodeInterpreterOutput {
    /// Log/text output.
    Logs {
        /// The log text.
        logs: String,
    },
    /// An image output.
    Image {
        /// File ID of the generated image.
        file_id: String,
    },
}

/// A part of a reasoning summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReasoningSummaryPart {
    /// The summary text.
    pub text: String,
}

// ── ResponseConfig ──────────────────────────────────────────────────────

/// Configuration for a Codex Responses API request.
///
/// Matches the top-level parameters of the `/v1/responses` endpoint,
/// providing a typed config object separate from the input messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseConfig {
    /// System-level instructions prepended to the conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Model identifier (e.g. `"codex-mini-latest"`, `"o3-mini"`).
    pub model: String,
    /// Tool definitions available to the model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<crate::tools::ToolDefinition>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Maximum output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Top-p (nucleus) sampling parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Reasoning configuration (effort level).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
    /// Output text format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<CodexTextFormat>,
    /// Whether to stream the response.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stream: bool,
    /// Previous response ID for multi-turn conversations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    /// Arbitrary metadata key-value pairs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl Default for ResponseConfig {
    fn default() -> Self {
        Self {
            instructions: None,
            model: "codex-mini-latest".into(),
            tools: Vec::new(),
            temperature: None,
            max_output_tokens: None,
            top_p: None,
            reasoning: None,
            text: None,
            stream: false,
            previous_response_id: None,
            metadata: BTreeMap::new(),
        }
    }
}

/// Reasoning configuration for models that support it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReasoningConfig {
    /// Reasoning effort level.
    pub effort: ReasoningEffort,
    /// Whether to include reasoning summary in the response.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub summary: bool,
}

/// Reasoning effort levels.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    /// Low reasoning effort — faster, less thorough.
    Low,
    /// Medium (default) reasoning effort.
    Medium,
    /// High reasoning effort — slower, more thorough.
    High,
}

// ── Response ────────────────────────────────────────────────────────────

/// The top-level response object from the Codex Responses API.
///
/// Returned by both the synchronous and streaming (final event) endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Unique response identifier (e.g. `"resp_abc123"`).
    pub id: String,
    /// Object type, always `"response"`.
    #[serde(default = "default_object_type")]
    pub object: String,
    /// Response status (`"completed"`, `"in_progress"`, `"failed"`, `"cancelled"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Model used for the response.
    pub model: String,
    /// Output items produced by the model.
    pub output: Vec<ResponseItem>,
    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Arbitrary metadata echoed back from the request.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

fn default_object_type() -> String {
    "response".into()
}

impl Response {
    /// Get all text content from message output items.
    #[must_use]
    pub fn text(&self) -> String {
        let mut parts = Vec::new();
        for item in &self.output {
            if let ResponseItem::Message { content, .. } = item {
                for part in content {
                    if let ResponseContentPart::OutputText { text } = part {
                        parts.push(text.as_str());
                    }
                }
            }
        }
        parts.join("")
    }

    /// Get all function calls from the output.
    #[must_use]
    pub fn function_calls(&self) -> Vec<&ResponseItem> {
        self.output
            .iter()
            .filter(|item| matches!(item, ResponseItem::FunctionCall { .. }))
            .collect()
    }

    /// Whether the response completed successfully.
    #[must_use]
    pub fn is_completed(&self) -> bool {
        self.status.as_deref() == Some("completed")
    }
}
