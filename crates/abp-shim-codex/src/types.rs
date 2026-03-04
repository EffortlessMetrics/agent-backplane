// SPDX-License-Identifier: MIT OR Apache-2.0
//! Codex Responses API types.
//!
//! Contains the builder for [`CodexRequest`] and token usage statistics.

use abp_codex_sdk::dialect::{CodexInputItem, CodexRequest, CodexTextFormat, CodexTool};
use serde::{Deserialize, Serialize};

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
