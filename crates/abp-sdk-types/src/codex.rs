// SPDX-License-Identifier: MIT OR Apache-2.0
//! OpenAI Codex / Responses API type definitions.
//!
//! Mirrors the OpenAI Responses API surface (distinct from Chat Completions).
//! Codex is execution-oriented with sandboxing, code_interpreter, and
//! file_search built-in tools.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Input types ─────────────────────────────────────────────────────────

/// An input item in the Codex Responses API format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

// ── Tool types ──────────────────────────────────────────────────────────

/// Function definition inside a Codex tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CodexFunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// A tool available in the Codex/Responses API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexTool {
    /// A user-defined function tool.
    Function {
        /// The function definition payload.
        function: CodexFunctionDef,
    },
    /// The built-in code interpreter tool.
    CodeInterpreter {},
    /// The built-in file search tool.
    FileSearch {
        /// Maximum number of results to return.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_num_results: Option<u32>,
    },
}

/// Output text format configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexTextFormat {
    /// Plain text output (default).
    Text {},
    /// JSON object output.
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

// ── Sandbox config ──────────────────────────────────────────────────────

/// Networking policy for sandboxed execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAccess {
    /// No network access allowed.
    #[default]
    None,
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
    /// Full file-system access.
    Full,
}

/// Sandbox configuration for Codex execution environments.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct SandboxConfig {
    /// Container image to use for execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_image: Option<String>,
    /// Networking policy for the sandbox.
    #[serde(default)]
    pub networking: NetworkAccess,
    /// File-system access policy.
    #[serde(default)]
    pub file_access: FileAccess,
    /// Maximum wall-clock time in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    /// Environment variables injected into the sandbox.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            container_image: None,
            networking: NetworkAccess::None,
            file_access: FileAccess::WorkspaceOnly,
            timeout_seconds: Some(300),
            env: BTreeMap::new(),
        }
    }
}

// ── Request ─────────────────────────────────────────────────────────────

/// OpenAI Codex / Responses API request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CodexRequest {
    /// Model identifier (e.g. `codex-mini-latest`).
    pub model: String,
    /// Input items (messages) for the request.
    pub input: Vec<CodexInputItem>,
    /// Maximum output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Tools available to the model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<CodexTool>,
    /// Output text format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<CodexTextFormat>,
}

// ── Response types ──────────────────────────────────────────────────────

/// A content part within a Codex output message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexContentPart {
    /// Text output from the model.
    OutputText {
        /// The text content.
        text: String,
    },
}

/// A summary fragment within a reasoning response item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ReasoningSummary {
    /// The reasoning text.
    pub text: String,
}

/// A response item in the Codex Responses API format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
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

/// Token usage reported by the Codex / Responses API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CodexUsage {
    /// Tokens consumed by the input.
    pub input_tokens: u64,
    /// Tokens generated in the output.
    pub output_tokens: u64,
    /// Total tokens (input + output).
    pub total_tokens: u64,
}

/// OpenAI Codex / Responses API response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CodexResponse {
    /// Unique response identifier.
    pub id: String,
    /// Model used for the completion.
    pub model: String,
    /// Output items produced by the model.
    pub output: Vec<CodexResponseItem>,
    /// Token usage statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<CodexUsage>,
    /// Response status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

// ── Streaming ───────────────────────────────────────────────────────────

/// Delta payload for incremental streaming updates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
}

/// Server-sent events emitted during a Codex streaming response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexStreamEvent {
    /// The response object has been created.
    ResponseCreated {
        /// The initial (incomplete) response.
        response: CodexResponse,
    },
    /// A new output item has been added.
    OutputItemAdded {
        /// Index of the item in the output array.
        output_index: usize,
        /// The newly added item.
        item: CodexResponseItem,
    },
    /// An output item is being streamed.
    OutputItemDelta {
        /// Index of the item in the output array.
        output_index: usize,
        /// The partial delta payload.
        delta: CodexStreamDelta,
    },
    /// An output item has been finalized.
    OutputItemDone {
        /// Index of the item in the output array.
        output_index: usize,
        /// The finalized item.
        item: CodexResponseItem,
    },
    /// The response has completed successfully.
    ResponseCompleted {
        /// The final response.
        response: CodexResponse,
    },
    /// An error occurred during streaming.
    Error {
        /// Error message.
        message: String,
        /// Error code.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
}

// ── Model config ────────────────────────────────────────────────────────

/// Vendor-specific configuration for the OpenAI Codex / Responses API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CodexConfig {
    /// Base URL for the API.
    pub base_url: String,
    /// Model identifier (e.g. `codex-mini-latest`).
    pub model: String,
    /// Maximum output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Temperature for sampling (0.0–2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Sandbox configuration for execution environments.
    #[serde(default)]
    pub sandbox: SandboxConfig,
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".into(),
            model: "codex-mini-latest".into(),
            max_output_tokens: Some(4096),
            temperature: None,
            sandbox: SandboxConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serde_roundtrip() {
        let req = CodexRequest {
            model: "codex-mini-latest".into(),
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "Write tests".into(),
            }],
            max_output_tokens: Some(4096),
            temperature: None,
            tools: vec![CodexTool::Function {
                function: CodexFunctionDef {
                    name: "shell".into(),
                    description: "Run a command".into(),
                    parameters: serde_json::json!({"type": "object"}),
                },
            }],
            text: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: CodexRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn response_serde_roundtrip() {
        let resp = CodexResponse {
            id: "resp_123".into(),
            model: "codex-mini-latest".into(),
            output: vec![
                CodexResponseItem::Message {
                    role: "assistant".into(),
                    content: vec![CodexContentPart::OutputText {
                        text: "Done!".into(),
                    }],
                },
                CodexResponseItem::FunctionCall {
                    id: "fc_1".into(),
                    call_id: None,
                    name: "shell".into(),
                    arguments: r#"{"command":"ls"}"#.into(),
                },
            ],
            usage: Some(CodexUsage {
                input_tokens: 50,
                output_tokens: 20,
                total_tokens: 70,
            }),
            status: Some("completed".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: CodexResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn stream_event_roundtrip() {
        let event = CodexStreamEvent::OutputItemDelta {
            output_index: 0,
            delta: CodexStreamDelta::OutputTextDelta {
                text: "Hello".into(),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: CodexStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn sandbox_config_default() {
        let cfg = SandboxConfig::default();
        assert_eq!(cfg.networking, NetworkAccess::None);
        assert_eq!(cfg.file_access, FileAccess::WorkspaceOnly);
        assert!(cfg.timeout_seconds.unwrap_or(0) > 0);
    }

    #[test]
    fn config_default_values() {
        let cfg = CodexConfig::default();
        assert!(cfg.base_url.contains("openai.com"));
        assert!(cfg.model.contains("codex"));
        assert!(cfg.max_output_tokens.unwrap_or(0) > 0);
    }
}
