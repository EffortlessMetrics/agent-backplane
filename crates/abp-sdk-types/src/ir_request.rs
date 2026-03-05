// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Normalized chat request types for the IR layer.
//!
//! `IrChatRequest` captures the full request surface area that every
//! dialect adapter lowers into before the mapping engine re-raises it
//! into the target dialect.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::ir::{IrMessage, IrToolDefinition};

// ── Sampling parameters ─────────────────────────────────────────────────

/// Sampling / generation parameters normalized across dialects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct IrSamplingParams {
    /// Sampling temperature (0.0 – 2.0 typically).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Nucleus-sampling probability mass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Top-k sampling cutoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,

    /// Frequency penalty (−2.0 – 2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,

    /// Presence penalty (−2.0 – 2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
}

// ── Streaming configuration ─────────────────────────────────────────────

/// Streaming configuration for an IR request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct IrStreamConfig {
    /// Whether streaming is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Include token-usage statistics in the final stream chunk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_usage: Option<bool>,

    /// Vendor-specific streaming extensions.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}

// ── Chat request ────────────────────────────────────────────────────────

/// A normalized chat-completions request.
///
/// This is the central IR request type.  Every dialect-specific request
/// struct is lowered into `IrChatRequest` for mapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrChatRequest {
    /// Model identifier.
    pub model: String,

    /// Conversation messages.
    pub messages: Vec<IrMessage>,

    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,

    /// Tool definitions available to the model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<IrToolDefinition>,

    /// How the model should choose tool use (`"auto"`, `"none"`, `"required"`, or a specific name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,

    /// Sampling parameters.
    #[serde(default)]
    pub sampling: IrSamplingParams,

    /// Stop sequences that halt generation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,

    /// Streaming configuration.
    #[serde(default)]
    pub stream: IrStreamConfig,

    /// Response format hint (e.g. `{"type": "json_object"}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,

    /// Vendor-specific extension parameters.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}

impl IrChatRequest {
    /// Create a minimal request with a model and messages.
    #[must_use]
    pub fn new(model: impl Into<String>, messages: Vec<IrMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
            max_tokens: None,
            tools: Vec::new(),
            tool_choice: None,
            sampling: IrSamplingParams::default(),
            stop_sequences: Vec::new(),
            stream: IrStreamConfig::default(),
            response_format: None,
            extra: BTreeMap::new(),
        }
    }

    /// Builder: set max tokens.
    #[must_use]
    pub fn with_max_tokens(mut self, max_tokens: u64) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Builder: add a tool definition.
    #[must_use]
    pub fn with_tool(mut self, tool: IrToolDefinition) -> Self {
        self.tools.push(tool);
        self
    }

    /// Builder: set sampling parameters.
    #[must_use]
    pub fn with_sampling(mut self, sampling: IrSamplingParams) -> Self {
        self.sampling = sampling;
        self
    }

    /// Builder: set streaming config.
    #[must_use]
    pub fn with_stream(mut self, stream: IrStreamConfig) -> Self {
        self.stream = stream;
        self
    }

    /// Returns `true` if streaming is enabled.
    #[must_use]
    pub fn is_streaming(&self) -> bool {
        self.stream.enabled
    }

    /// Returns `true` if any tools are defined.
    #[must_use]
    pub fn has_tools(&self) -> bool {
        !self.tools.is_empty()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrContentPart, IrRole};

    #[test]
    fn sampling_params_default_all_none() {
        let sp = IrSamplingParams::default();
        assert_eq!(sp.temperature, None);
        assert_eq!(sp.top_p, None);
        assert_eq!(sp.top_k, None);
        assert_eq!(sp.frequency_penalty, None);
        assert_eq!(sp.presence_penalty, None);
    }

    #[test]
    fn sampling_params_serde_roundtrip() {
        let sp = IrSamplingParams {
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
            frequency_penalty: Some(0.5),
            presence_penalty: Some(0.1),
        };
        let json = serde_json::to_string(&sp).unwrap();
        let back: IrSamplingParams = serde_json::from_str(&json).unwrap();
        assert_eq!(sp, back);
    }

    #[test]
    fn sampling_params_serde_omits_none() {
        let sp = IrSamplingParams {
            temperature: Some(0.5),
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
        };
        let json = serde_json::to_string(&sp).unwrap();
        assert!(json.contains("temperature"));
        assert!(!json.contains("top_p"));
        assert!(!json.contains("top_k"));
    }

    #[test]
    fn stream_config_default() {
        let sc = IrStreamConfig::default();
        assert!(!sc.enabled);
        assert_eq!(sc.include_usage, None);
        assert!(sc.extra.is_empty());
    }

    #[test]
    fn stream_config_serde_roundtrip() {
        let sc = IrStreamConfig {
            enabled: true,
            include_usage: Some(true),
            extra: BTreeMap::new(),
        };
        let json = serde_json::to_string(&sc).unwrap();
        let back: IrStreamConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(sc, back);
    }

    #[test]
    fn chat_request_minimal() {
        let req = IrChatRequest::new("gpt-4o", vec![IrMessage::text(IrRole::User, "Hello")]);
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 1);
        assert!(!req.is_streaming());
        assert!(!req.has_tools());
        assert_eq!(req.max_tokens, None);
    }

    #[test]
    fn chat_request_serde_roundtrip() {
        let req = IrChatRequest::new(
            "claude-sonnet-4-20250514",
            vec![
                IrMessage::text(IrRole::System, "You are helpful"),
                IrMessage::text(IrRole::User, "Hello"),
            ],
        )
        .with_max_tokens(1024)
        .with_sampling(IrSamplingParams {
            temperature: Some(0.7),
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
        });
        let json = serde_json::to_string(&req).unwrap();
        let back: IrChatRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn chat_request_with_tools() {
        let tool = IrToolDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: serde_json::json!({"type": "object"}),
        };
        let req = IrChatRequest::new("gpt-4o", vec![IrMessage::text(IrRole::User, "Find info")])
            .with_tool(tool);
        assert!(req.has_tools());
        assert_eq!(req.tools.len(), 1);
        assert_eq!(req.tools[0].name, "search");
    }

    #[test]
    fn chat_request_streaming() {
        let req = IrChatRequest::new("gpt-4o", vec![IrMessage::text(IrRole::User, "Hello")])
            .with_stream(IrStreamConfig {
                enabled: true,
                include_usage: Some(true),
                extra: BTreeMap::new(),
            });
        assert!(req.is_streaming());
    }

    #[test]
    fn chat_request_full_roundtrip() {
        let mut extra = BTreeMap::new();
        extra.insert("vendor_flag".into(), serde_json::json!(true));
        let req = IrChatRequest {
            model: "gpt-4o".into(),
            messages: vec![
                IrMessage::text(IrRole::System, "Be helpful"),
                IrMessage::text(IrRole::User, "Hello"),
            ],
            max_tokens: Some(2048),
            tools: vec![IrToolDefinition {
                name: "calc".into(),
                description: "Calculator".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
            tool_choice: Some(serde_json::json!("auto")),
            sampling: IrSamplingParams {
                temperature: Some(0.5),
                top_p: Some(0.95),
                top_k: None,
                frequency_penalty: Some(0.0),
                presence_penalty: Some(0.0),
            },
            stop_sequences: vec!["STOP".into()],
            stream: IrStreamConfig {
                enabled: false,
                include_usage: None,
                extra: BTreeMap::new(),
            },
            response_format: Some(serde_json::json!({"type": "json_object"})),
            extra,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: IrChatRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn chat_request_serde_omits_defaults() {
        let req = IrChatRequest::new("gpt-4o", vec![IrMessage::text(IrRole::User, "Hi")]);
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("tool_choice"));
        assert!(!json.contains("stop_sequences"));
        assert!(!json.contains("response_format"));
    }
}
