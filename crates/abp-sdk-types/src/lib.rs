// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # abp-sdk-types
//!
//! Pure data model crate defining SDK-specific dialect types for the Agent
//! Backplane. Each vendor module mirrors the vendor's actual API surface
//! (messages, tool calls, streaming chunks, model configuration) without
//! any networking or runtime logic.

/// Anthropic Claude Messages API types.
pub mod claude;
/// OpenAI Codex / Responses API types.
pub mod codex;
/// GitHub Copilot Extensions API types.
pub mod copilot;
/// Google Gemini generateContent API types.
pub mod gemini;
/// Moonshot Kimi Chat Completions API types (with extensions).
pub mod kimi;
/// OpenAI Chat Completions API types.
pub mod openai;

/// Shared types used across dialect modules.
pub mod common;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Dialect enum ────────────────────────────────────────────────────────

/// Known agent-protocol dialects supported by the Agent Backplane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    /// OpenAI Chat Completions style.
    OpenAi,
    /// Anthropic Claude Messages API.
    Claude,
    /// Google Gemini generateContent style.
    Gemini,
    /// Moonshot Kimi API style.
    Kimi,
    /// OpenAI Codex / Responses API style.
    Codex,
    /// GitHub Copilot Extensions style.
    Copilot,
}

impl Dialect {
    /// Human-readable label for this dialect.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Kimi => "Kimi",
            Self::Codex => "Codex",
            Self::Copilot => "Copilot",
        }
    }

    /// Returns all known dialects.
    #[must_use]
    pub fn all() -> &'static [Dialect] {
        &[
            Self::OpenAi,
            Self::Claude,
            Self::Gemini,
            Self::Kimi,
            Self::Codex,
            Self::Copilot,
        ]
    }
}

impl std::fmt::Display for Dialect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ── DialectRequest ──────────────────────────────────────────────────────

/// A vendor-specific API request, wrapping each dialect's request type.
///
/// This is the entry point for dialect translation: incoming SDK requests
/// are parsed into a `DialectRequest` before being lowered to a `WorkOrder`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "dialect", rename_all = "snake_case")]
pub enum DialectRequest {
    /// OpenAI Chat Completions request.
    OpenAi(openai::OpenAiRequest),
    /// Anthropic Claude Messages API request.
    Claude(claude::ClaudeRequest),
    /// Google Gemini generateContent request.
    Gemini(gemini::GeminiRequest),
    /// Moonshot Kimi Chat Completions request.
    Kimi(kimi::KimiRequest),
    /// OpenAI Codex / Responses API request.
    Codex(codex::CodexRequest),
    /// GitHub Copilot Extensions request.
    Copilot(copilot::CopilotRequest),
}

impl DialectRequest {
    /// Returns the [`Dialect`] variant for this request.
    #[must_use]
    pub fn dialect(&self) -> Dialect {
        match self {
            Self::OpenAi(_) => Dialect::OpenAi,
            Self::Claude(_) => Dialect::Claude,
            Self::Gemini(_) => Dialect::Gemini,
            Self::Kimi(_) => Dialect::Kimi,
            Self::Codex(_) => Dialect::Codex,
            Self::Copilot(_) => Dialect::Copilot,
        }
    }

    /// Extracts the model identifier from the wrapped request.
    #[must_use]
    pub fn model(&self) -> &str {
        match self {
            Self::OpenAi(r) => &r.model,
            Self::Claude(r) => &r.model,
            Self::Gemini(r) => &r.model,
            Self::Kimi(r) => &r.model,
            Self::Codex(r) => &r.model,
            Self::Copilot(r) => &r.model,
        }
    }
}

// ── DialectResponse ─────────────────────────────────────────────────────

/// A vendor-specific API response, wrapping each dialect's response type.
///
/// After processing, a `Receipt` is projected back into a `DialectResponse`
/// targeting the original caller's SDK format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "dialect", rename_all = "snake_case")]
pub enum DialectResponse {
    /// OpenAI Chat Completions response.
    OpenAi(openai::OpenAiResponse),
    /// Anthropic Claude Messages API response.
    Claude(claude::ClaudeResponse),
    /// Google Gemini generateContent response.
    Gemini(gemini::GeminiResponse),
    /// Moonshot Kimi Chat Completions response.
    Kimi(kimi::KimiResponse),
    /// OpenAI Codex / Responses API response.
    Codex(codex::CodexResponse),
    /// GitHub Copilot Extensions response.
    Copilot(copilot::CopilotResponse),
}

impl DialectResponse {
    /// Returns the [`Dialect`] variant for this response.
    #[must_use]
    pub fn dialect(&self) -> Dialect {
        match self {
            Self::OpenAi(_) => Dialect::OpenAi,
            Self::Claude(_) => Dialect::Claude,
            Self::Gemini(_) => Dialect::Gemini,
            Self::Kimi(_) => Dialect::Kimi,
            Self::Codex(_) => Dialect::Codex,
            Self::Copilot(_) => Dialect::Copilot,
        }
    }
}

// ── DialectStreamChunk ──────────────────────────────────────────────────

/// A vendor-specific streaming chunk, wrapping each dialect's stream type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "dialect", rename_all = "snake_case")]
pub enum DialectStreamChunk {
    /// OpenAI Chat Completions streaming chunk.
    OpenAi(openai::OpenAiStreamChunk),
    /// Anthropic Claude streaming event.
    Claude(claude::ClaudeStreamEvent),
    /// Google Gemini streaming chunk.
    Gemini(gemini::GeminiStreamChunk),
    /// Moonshot Kimi streaming chunk.
    Kimi(kimi::KimiStreamChunk),
    /// OpenAI Codex / Responses API streaming event.
    Codex(codex::CodexStreamEvent),
    /// GitHub Copilot streaming event.
    Copilot(copilot::CopilotStreamEvent),
}

impl DialectStreamChunk {
    /// Returns the [`Dialect`] variant for this chunk.
    #[must_use]
    pub fn dialect(&self) -> Dialect {
        match self {
            Self::OpenAi(_) => Dialect::OpenAi,
            Self::Claude(_) => Dialect::Claude,
            Self::Gemini(_) => Dialect::Gemini,
            Self::Kimi(_) => Dialect::Kimi,
            Self::Codex(_) => Dialect::Codex,
            Self::Copilot(_) => Dialect::Copilot,
        }
    }
}

// ── ModelConfig ──────────────────────────────────────────────────────────

/// Vendor-agnostic model configuration parameters.
///
/// Used as a common denominator when projecting between dialects.
/// Individual dialect modules may have richer configuration types.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ModelConfig {
    /// Model identifier (e.g. `gpt-4o`, `claude-sonnet-4-20250514`).
    pub model: String,

    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Top-p (nucleus) sampling parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Stop sequences that halt generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Vendor-specific extension parameters.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, serde_json::Value>,
}

// ── Canonical tool definition ───────────────────────────────────────────

/// A vendor-agnostic tool definition used as the ABP canonical form.
///
/// Each dialect module provides conversion functions to/from this type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CanonicalToolDef {
    /// Tool name.
    pub name: String,
    /// Human-readable description of the tool.
    pub description: String,
    /// JSON Schema describing the tool's parameters.
    pub parameters_schema: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialect_serde_roundtrip() {
        for d in Dialect::all() {
            let json = serde_json::to_string(d).unwrap();
            let back: Dialect = serde_json::from_str(&json).unwrap();
            assert_eq!(*d, back);
        }
    }

    #[test]
    fn dialect_display() {
        assert_eq!(Dialect::OpenAi.to_string(), "OpenAI");
        assert_eq!(Dialect::Claude.to_string(), "Claude");
        assert_eq!(Dialect::Gemini.to_string(), "Gemini");
        assert_eq!(Dialect::Kimi.to_string(), "Kimi");
        assert_eq!(Dialect::Codex.to_string(), "Codex");
        assert_eq!(Dialect::Copilot.to_string(), "Copilot");
    }

    #[test]
    fn model_config_serde_roundtrip() {
        let cfg = ModelConfig {
            model: "gpt-4o".into(),
            max_tokens: Some(4096),
            temperature: Some(0.7),
            top_p: None,
            stop_sequences: Some(vec!["STOP".into()]),
            extra: BTreeMap::new(),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: ModelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn canonical_tool_def_serde_roundtrip() {
        let def = CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            }),
        };
        let json = serde_json::to_string(&def).unwrap();
        let back: CanonicalToolDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, back);
    }

    #[test]
    fn dialect_request_openai_roundtrip() {
        let req = DialectRequest::OpenAi(openai::OpenAiRequest {
            model: "gpt-4o".into(),
            messages: vec![openai::OpenAiMessage {
                role: "user".into(),
                content: Some("hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: None,
            tool_choice: None,
            temperature: Some(0.5),
            max_tokens: Some(1024),
            response_format: None,
            stream: None,
        });
        let json = serde_json::to_string(&req).unwrap();
        let back: DialectRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
        assert_eq!(back.dialect(), Dialect::OpenAi);
        assert_eq!(back.model(), "gpt-4o");
    }

    #[test]
    fn dialect_response_claude_roundtrip() {
        let resp = DialectResponse::Claude(claude::ClaudeResponse {
            id: "msg_123".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![claude::ClaudeContentBlock::Text {
                text: "Hello!".into(),
            }],
            stop_reason: Some("end_turn".into()),
            usage: None,
        });
        let json = serde_json::to_string(&resp).unwrap();
        let back: DialectResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
        assert_eq!(back.dialect(), Dialect::Claude);
    }

    #[test]
    fn dialect_stream_chunk_gemini_roundtrip() {
        let chunk = DialectStreamChunk::Gemini(gemini::GeminiStreamChunk {
            candidates: vec![gemini::GeminiCandidate {
                content: gemini::GeminiContent {
                    role: "model".into(),
                    parts: vec![gemini::GeminiPart::Text("delta".into())],
                },
                finish_reason: None,
            }],
            usage_metadata: None,
        });
        let json = serde_json::to_string(&chunk).unwrap();
        let back: DialectStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(chunk, back);
        assert_eq!(back.dialect(), Dialect::Gemini);
    }
}
