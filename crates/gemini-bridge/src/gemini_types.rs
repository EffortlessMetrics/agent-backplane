// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Google Gemini GenerateContent API types for direct integration.
//!
//! These mirror the Google Gemini REST API surface so the bridge can construct
//! requests, parse responses, and process streaming events without depending
//! on external Gemini SDK crates.

use serde::{Deserialize, Serialize};

// ── Part ────────────────────────────────────────────────────────────────

/// Inline binary data (e.g. images) embedded in a Gemini content block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InlineData {
    /// MIME type of the data (e.g. `image/png`).
    pub mime_type: String,
    /// Base64-encoded binary data.
    pub data: String,
}

/// A function call emitted by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionCall {
    /// Function name.
    pub name: String,
    /// Arguments object.
    pub args: serde_json::Value,
}

/// A function result returned to the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionResponse {
    /// Function name.
    pub name: String,
    /// Response payload.
    pub response: serde_json::Value,
}

/// One piece of content inside a [`Content`] block.
///
/// Serialises to the real Gemini JSON format:
/// - `{"text": "…"}`
/// - `{"inlineData": {"mimeType": "…", "data": "…"}}`
/// - `{"functionCall": {"name": "…", "args": {…}}}`
/// - `{"functionResponse": {"name": "…", "response": {…}}}`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Part {
    /// Plain text.
    Text(String),
    /// Base64-encoded inline binary data.
    InlineData(InlineData),
    /// A function call emitted by the model.
    FunctionCall(FunctionCall),
    /// A function result returned to the model.
    FunctionResponse(FunctionResponse),
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
        Self::InlineData(InlineData {
            mime_type: mime_type.into(),
            data: data.into(),
        })
    }

    /// Create a function call part.
    #[must_use]
    pub fn function_call(name: impl Into<String>, args: serde_json::Value) -> Self {
        Self::FunctionCall(FunctionCall {
            name: name.into(),
            args,
        })
    }

    /// Create a function response part.
    #[must_use]
    pub fn function_response(name: impl Into<String>, response: serde_json::Value) -> Self {
        Self::FunctionResponse(FunctionResponse {
            name: name.into(),
            response,
        })
    }
}

// ── Content ─────────────────────────────────────────────────────────────

/// A single conversation turn or system instruction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Content {
    /// `"user"` or `"model"`. Omitted for system instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Ordered content parts.
    pub parts: Vec<Part>,
}

impl Content {
    /// Create a user-role content block.
    #[must_use]
    pub fn user(parts: Vec<Part>) -> Self {
        Self {
            role: Some("user".into()),
            parts,
        }
    }

    /// Create a model-role content block.
    #[must_use]
    pub fn model(parts: Vec<Part>) -> Self {
        Self {
            role: Some("model".into()),
            parts,
        }
    }

    /// Create a system instruction (no role).
    #[must_use]
    pub fn system(parts: Vec<Part>) -> Self {
        Self { role: None, parts }
    }
}

// ── Safety ──────────────────────────────────────────────────────────────

/// Harm category identifiers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmCategory {
    /// Harassment content.
    HarmCategoryHarassment,
    /// Hate speech.
    HarmCategoryHateSpeech,
    /// Sexually explicit content.
    HarmCategorySexuallyExplicit,
    /// Dangerous content.
    HarmCategoryDangerousContent,
    /// Civic integrity.
    HarmCategoryCivicIntegrity,
}

/// Blocking threshold for a harm category.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmBlockThreshold {
    /// Do not block any content.
    BlockNone,
    /// Block low-and-above probability.
    BlockLowAndAbove,
    /// Block medium-and-above probability.
    BlockMediumAndAbove,
    /// Block only high probability.
    BlockOnlyHigh,
}

/// A per-category safety constraint on a request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SafetySetting {
    /// Harm category.
    pub category: HarmCategory,
    /// Blocking threshold.
    pub threshold: HarmBlockThreshold,
}

/// Probability rating for a safety category.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmProbability {
    /// Negligible probability.
    Negligible,
    /// Low probability.
    Low,
    /// Medium probability.
    Medium,
    /// High probability.
    High,
}

/// Per-category safety rating on a response candidate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SafetyRating {
    /// Evaluated category.
    pub category: HarmCategory,
    /// Assessed probability.
    pub probability: HarmProbability,
}

// ── Generation config ───────────────────────────────────────────────────

/// Sampling and output parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Top-p (nucleus) sampling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Top-k sampling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Number of candidates to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<u32>,
    /// Sequences that stop generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// MIME type for the response (e.g. `application/json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,
    /// JSON Schema for structured output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<serde_json::Value>,
}

// ── Tool / function calling ─────────────────────────────────────────────

/// A function the model may call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionDeclaration {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the parameters.
    pub parameters: serde_json::Value,
}

/// A tool definition wrapping one or more function declarations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiTool {
    /// Declared functions.
    pub function_declarations: Vec<FunctionDeclaration>,
}

/// Function-calling mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FunctionCallingMode {
    /// Model decides.
    Auto,
    /// Model must call at least one function.
    Any,
    /// Model must not call functions.
    None,
}

/// Detailed function-calling constraints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCallingConfig {
    /// Calling mode.
    pub mode: FunctionCallingMode,
    /// Restrict to specific function names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_function_names: Option<Vec<String>>,
}

/// Top-level tool-calling configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfig {
    /// Function-calling behaviour.
    pub function_calling_config: FunctionCallingConfig,
}

// ── Token usage ─────────────────────────────────────────────────────────

/// Token usage metadata returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    /// Tokens consumed by the prompt.
    pub prompt_token_count: u64,
    /// Tokens generated across candidates.
    pub candidates_token_count: u64,
    /// Total tokens.
    pub total_token_count: u64,
}

// ── Prompt feedback ─────────────────────────────────────────────────────

/// Prompt-level safety feedback.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PromptFeedback {
    /// Block reason (e.g. `"SAFETY"`), if the prompt was blocked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    /// Safety ratings for the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<SafetyRating>>,
}

// ── Citation metadata ───────────────────────────────────────────────────

/// Citation source for grounded content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CitationSource {
    /// Start index in the response text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_index: Option<u32>,
    /// End index in the response text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_index: Option<u32>,
    /// URI of the cited source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// License for the cited source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

/// Citation metadata for a candidate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CitationMetadata {
    /// Citation sources.
    pub citation_sources: Vec<CitationSource>,
}

// ── Candidate ───────────────────────────────────────────────────────────

/// A single candidate completion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// Generated content.
    pub content: Content,
    /// Why the model stopped (e.g. `"STOP"`, `"MAX_TOKENS"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// Per-category safety ratings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<SafetyRating>>,
    /// Citation metadata for grounded content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation_metadata: Option<CitationMetadata>,
}

// ── Request ─────────────────────────────────────────────────────────────

/// Body of a `generateContent` request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentRequest {
    /// Model identifier (e.g. `gemini-2.5-flash`).
    pub model: String,
    /// Conversation turns.
    pub contents: Vec<Content>,
    /// Tool declarations available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiTool>>,
    /// Sampling and output parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
    /// Per-category safety thresholds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<SafetySetting>>,
    /// Optional system-level instruction (no role required).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,
}

impl GenerateContentRequest {
    /// Create a new request for the given model.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            contents: Vec::new(),
            tools: None,
            generation_config: None,
            safety_settings: None,
            system_instruction: None,
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
    pub fn tools(mut self, tools: Vec<GeminiTool>) -> Self {
        self.tools = Some(tools);
        self
    }
}

// ── Response ────────────────────────────────────────────────────────────

/// Body of a `generateContent` response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentResponse {
    /// Candidate completions.
    pub candidates: Vec<Candidate>,
    /// Token usage statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<UsageMetadata>,
    /// Prompt-level safety feedback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_feedback: Option<PromptFeedback>,
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
                        Part::FunctionCall(fc) => Some((fc.name.as_str(), &fc.args)),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

// ── Streaming ───────────────────────────────────────────────────────────

/// A streaming response event from `streamGenerateContent`.
///
/// Each chunk may contain incremental text deltas, tool calls,
/// or usage metadata (typically in the final chunk).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StreamGenerateContentResponse {
    /// Response candidates in this chunk.
    pub candidates: Vec<Candidate>,
    /// Token usage metadata (usually in the final chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<UsageMetadata>,
}

impl StreamGenerateContentResponse {
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

// ── API errors ──────────────────────────────────────────────────────────

/// Detail of a Gemini API error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeminiErrorDetail {
    /// HTTP status code.
    pub code: u16,
    /// Human-readable error message.
    pub message: String,
    /// Error status string (e.g. `"INVALID_ARGUMENT"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// A structured error returned by the Gemini API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeminiErrorResponse {
    /// The error detail object.
    pub error: GeminiErrorDetail,
}

impl GeminiErrorResponse {
    /// Try to parse a JSON string as a Gemini API error response.
    #[must_use]
    pub fn parse(body: &str) -> Option<Self> {
        serde_json::from_str(body).ok()
    }
}
