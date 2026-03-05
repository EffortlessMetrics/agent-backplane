// SPDX-License-Identifier: MIT OR Apache-2.0
//! Moonshot Kimi Chat Completions API type definitions.
//!
//! Kimi uses an OpenAI-compatible chat completions surface with extensions
//! for built-in tools (`search_internet`, `browser`), citation references
//! (`refs`), and the `k1` reasoning mode.

use serde::{Deserialize, Serialize};

// ── Message types ───────────────────────────────────────────────────────

/// A single message in the Kimi conversation format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
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

// ── Tool types ──────────────────────────────────────────────────────────

/// Kimi/OpenAI-compatible function tool definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct KimiFunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// A tool entry in a Kimi request.
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

/// Descriptor for a Kimi built-in function.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct KimiBuiltinFunction {
    /// Built-in name (e.g. `"$web_search"`, `"$browser"`).
    pub name: String,
}

/// A tool call in a Kimi response (OpenAI-compatible shape).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
pub struct KimiFunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments for the function.
    pub arguments: String,
}

/// A citation reference returned by Kimi when web search is active.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct KimiRef {
    /// The numeric index of this citation (1-based).
    pub index: u32,
    /// URL of the cited source.
    pub url: String,
    /// Title of the cited source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

// ── Request ─────────────────────────────────────────────────────────────

/// Kimi chat completions request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct KimiRequest {
    /// Model identifier (e.g. `moonshot-v1-8k`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<KimiMessage>,
    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Whether to stream the response via SSE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Tool definitions (function and built-in).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<KimiTool>>,
    /// Whether to enable web search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_search: Option<bool>,
}

// ── Response ────────────────────────────────────────────────────────────

/// Kimi chat completions response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
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
    /// Citation references when web search was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<Vec<KimiRef>>,
}

/// A single choice in a Kimi completions response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct KimiChoice {
    /// Zero-based index of this choice.
    pub index: u32,
    /// The assistant's response message.
    pub message: KimiResponseMessage,
    /// Reason the model stopped generating.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// A message within a Kimi response choice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct KimiResponseMessage {
    /// Message role.
    pub role: String,
    /// Text content, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls requested by the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<KimiToolCall>>,
}

/// Token usage reported by the Kimi API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct KimiUsage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

// ── Streaming ───────────────────────────────────────────────────────────

/// A single SSE chunk from a Kimi streaming response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct KimiStreamChunk {
    /// Chunk identifier.
    pub id: String,
    /// Object type (`chat.completion.chunk`).
    pub object: String,
    /// Model that produced this chunk.
    pub model: String,
    /// Choices with streaming deltas.
    pub choices: Vec<KimiChunkChoice>,
    /// Usage info (only in the final chunk when requested).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<KimiUsage>,
    /// Citation references (may appear in later chunks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<Vec<KimiRef>>,
}

/// A single choice within a streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct KimiChunkChoice {
    /// Zero-based choice index.
    pub index: u32,
    /// The incremental delta for this choice.
    pub delta: KimiChunkDelta,
    /// Finish reason — `None` until the stream ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// An incremental delta within a streaming chunk choice.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct KimiChunkDelta {
    /// Role (usually only in the first chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Text content fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

// ── Model config ────────────────────────────────────────────────────────

/// Vendor-specific configuration for the Moonshot Kimi API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct KimiConfig {
    /// Base URL for the Kimi API.
    pub base_url: String,
    /// Model identifier (e.g. `moonshot-v1-8k`).
    pub model: String,
    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Temperature for sampling (0.0–1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Whether to use `k1` reasoning mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_k1_reasoning: Option<bool>,
}

impl Default for KimiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.moonshot.cn/v1".into(),
            model: "moonshot-v1-8k".into(),
            max_tokens: Some(4096),
            temperature: None,
            use_k1_reasoning: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serde_roundtrip() {
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![KimiMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(4096),
            temperature: Some(0.7),
            stream: None,
            tools: None,
            use_search: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: KimiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn response_serde_roundtrip() {
        let resp = KimiResponse {
            id: "cmpl_123".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("Hello!".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(KimiUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            refs: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: KimiResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn stream_chunk_serde_roundtrip() {
        let chunk = KimiStreamChunk {
            id: "cmpl_123".into(),
            object: "chat.completion.chunk".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChunkChoice {
                index: 0,
                delta: KimiChunkDelta {
                    role: Some("assistant".into()),
                    content: Some("Hi".into()),
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: KimiStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(chunk, back);
    }

    #[test]
    fn tool_call_serde_roundtrip() {
        let tc = KimiToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "web_search".into(),
                arguments: r#"{"query":"rust"}"#.into(),
            },
        };
        let json = serde_json::to_string(&tc).unwrap();
        let back: KimiToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(tc, back);
    }

    #[test]
    fn kimi_ref_serde_roundtrip() {
        let r = KimiRef {
            index: 1,
            url: "https://example.com".into(),
            title: Some("Example".into()),
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: KimiRef = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn config_default_values() {
        let cfg = KimiConfig::default();
        assert!(cfg.base_url.contains("moonshot.cn"));
        assert!(cfg.model.contains("moonshot"));
        assert!(cfg.max_tokens.unwrap_or(0) > 0);
    }
}
