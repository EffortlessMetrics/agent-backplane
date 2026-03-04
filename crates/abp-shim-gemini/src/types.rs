// SPDX-License-Identifier: MIT OR Apache-2.0
//! Strongly-typed Gemini API types.
//!
//! These types mirror the Google Gemini REST API wire format, providing
//! a convenient Rust interface for building requests and reading responses.
//! See <https://ai.google.dev/api/rest>.

use abp_gemini_sdk::dialect::{FunctionCallingMode, HarmBlockThreshold, HarmCategory};
use serde::{Deserialize, Serialize};

// ── Part ────────────────────────────────────────────────────────────────

/// A part within a content block, mirroring the Gemini SDK `Part` type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Part {
    /// Plain text content.
    Text(String),
    /// Inline binary data (e.g. images).
    InlineData {
        /// MIME type of the data.
        mime_type: String,
        /// Base64-encoded binary data.
        data: String,
    },
    /// A function call requested by the model.
    FunctionCall {
        /// Name of the function to invoke.
        name: String,
        /// Arguments as a JSON value.
        args: serde_json::Value,
    },
    /// A function response returned to the model.
    FunctionResponse {
        /// Name of the function that was called.
        name: String,
        /// The function's response payload.
        response: serde_json::Value,
    },
}

impl Part {
    /// Create a text part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// Create an inline data part (e.g. image).
    #[must_use]
    pub fn inline_data(mime_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self::InlineData {
            mime_type: mime_type.into(),
            data: data.into(),
        }
    }

    /// Create a function call part.
    #[must_use]
    pub fn function_call(name: impl Into<String>, args: serde_json::Value) -> Self {
        Self::FunctionCall {
            name: name.into(),
            args,
        }
    }

    /// Create a function response part.
    #[must_use]
    pub fn function_response(name: impl Into<String>, response: serde_json::Value) -> Self {
        Self::FunctionResponse {
            name: name.into(),
            response,
        }
    }
}

// ── Content ─────────────────────────────────────────────────────────────

/// A content block in the Gemini API format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    /// Role of the content author (`user` or `model`).
    pub role: String,
    /// Content parts.
    pub parts: Vec<Part>,
}

impl Content {
    /// Create a user-role content block.
    #[must_use]
    pub fn user(parts: Vec<Part>) -> Self {
        Self {
            role: "user".into(),
            parts,
        }
    }

    /// Create a model-role content block.
    #[must_use]
    pub fn model(parts: Vec<Part>) -> Self {
        Self {
            role: "model".into(),
            parts,
        }
    }
}

// ── Safety settings ─────────────────────────────────────────────────────

/// Safety settings applied to a request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SafetySetting {
    /// The harm category to configure.
    pub category: HarmCategory,
    /// The blocking threshold for this category.
    pub threshold: HarmBlockThreshold,
}

// ── Generation config ───────────────────────────────────────────────────

/// Generation configuration parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    /// Maximum number of output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Top-p (nucleus) sampling parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Top-k sampling parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Stop sequences that halt generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// MIME type for the response (e.g. `application/json`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,
    /// JSON Schema for structured output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<serde_json::Value>,
}

// ── Usage metadata ──────────────────────────────────────────────────────

/// Token usage metadata returned in a response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    /// Tokens consumed by the prompt.
    pub prompt_token_count: u64,
    /// Tokens generated across all candidates.
    pub candidates_token_count: u64,
    /// Total tokens (prompt + candidates).
    pub total_token_count: u64,
}

// ── Request ─────────────────────────────────────────────────────────────

/// A request to the Gemini `generateContent` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateContentRequest {
    /// Model identifier (e.g. `gemini-2.5-flash`).
    pub model: String,
    /// Conversation content blocks.
    pub contents: Vec<Content>,
    /// Optional system instruction content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,
    /// Generation configuration parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
    /// Safety settings for content filtering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<SafetySetting>>,
    /// Tool definitions available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDeclaration>>,
    /// Function-calling configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<ToolConfig>,
}

impl GenerateContentRequest {
    /// Create a new request for the given model.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            contents: Vec::new(),
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        }
    }

    /// Add a content block and return `self` for chaining.
    #[must_use]
    pub fn add_content(mut self, content: Content) -> Self {
        self.contents.push(content);
        self
    }

    /// Set the system instruction.
    #[must_use]
    pub fn system_instruction(mut self, content: Content) -> Self {
        self.system_instruction = Some(content);
        self
    }

    /// Set generation config.
    #[must_use]
    pub fn generation_config(mut self, config: GenerationConfig) -> Self {
        self.generation_config = Some(config);
        self
    }

    /// Set safety settings.
    #[must_use]
    pub fn safety_settings(mut self, settings: Vec<SafetySetting>) -> Self {
        self.safety_settings = Some(settings);
        self
    }

    /// Set tool declarations.
    #[must_use]
    pub fn tools(mut self, tools: Vec<ToolDeclaration>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set tool config.
    #[must_use]
    pub fn tool_config(mut self, config: ToolConfig) -> Self {
        self.tool_config = Some(config);
        self
    }
}

// ── Tool declarations ───────────────────────────────────────────────────

/// A tool declaration wrapping function declarations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDeclaration {
    /// Function declarations available to the model.
    pub function_declarations: Vec<FunctionDeclaration>,
}

/// A function declaration for tool use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionDeclaration {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// Controls function-calling behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfig {
    /// Function-calling behaviour configuration.
    pub function_calling_config: FunctionCallingConfig,
}

/// Detailed function-calling configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCallingConfig {
    /// The function-calling mode.
    pub mode: FunctionCallingMode,
    /// Restrict calls to these function names, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_function_names: Option<Vec<String>>,
}

// ── Response types ──────────────────────────────────────────────────────

/// A single candidate in a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    /// The generated content.
    pub content: Content,
    /// Reason the model stopped generating.
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// The response from a `generateContent` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateContentResponse {
    /// Response candidates from the model.
    pub candidates: Vec<Candidate>,
    /// Token usage metadata.
    #[serde(default)]
    pub usage_metadata: Option<UsageMetadata>,
}

impl GenerateContentResponse {
    /// Extract the text from the first candidate's first text part.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.candidates.first().and_then(|c| {
            c.content.parts.iter().find_map(|p| match p {
                Part::Text(t) => Some(t.as_str()),
                _ => None,
            })
        })
    }

    /// Extract all function calls from the first candidate.
    #[must_use]
    pub fn function_calls(&self) -> Vec<(&str, &serde_json::Value)> {
        self.candidates
            .first()
            .map(|c| {
                c.content
                    .parts
                    .iter()
                    .filter_map(|p| match p {
                        Part::FunctionCall { name, args } => Some((name.as_str(), args)),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

// ── Streaming types ─────────────────────────────────────────────────────

/// A streaming response event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    /// Response candidates in this chunk.
    pub candidates: Vec<Candidate>,
    /// Token usage metadata (usually in the final chunk).
    #[serde(default)]
    pub usage_metadata: Option<UsageMetadata>,
}

impl StreamEvent {
    /// Extract the text delta from the first candidate, if any.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.candidates.first().and_then(|c| {
            c.content.parts.iter().find_map(|p| match p {
                Part::Text(t) => Some(t.as_str()),
                _ => None,
            })
        })
    }
}
