// SPDX-License-Identifier: MIT OR Apache-2.0
//! Strongly-typed Gemini API types.
//!
//! These types mirror the Google Gemini REST API wire format, providing
//! a convenient Rust interface for building requests and reading responses.
//! See <https://ai.google.dev/api/rest>.

use abp_gemini_sdk::dialect::{FunctionCallingMode, HarmBlockThreshold, HarmCategory};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Part ────────────────────────────────────────────────────────────────

/// A part within a content block, mirroring the Gemini SDK `Part` type.
///
/// Serialises to the real Gemini JSON format:
/// - `{"text": "…"}`
/// - `{"inlineData": {"mimeType": "…", "data": "…"}}`
/// - `{"functionCall": {"name": "…", "args": {…}}}`
/// - `{"functionResponse": {"name": "…", "response": {…}}}`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Part {
    /// Plain text content.
    Text(String),
    /// Inline binary data (e.g. images).
    InlineData {
        /// MIME type of the data (e.g. `"image/png"`).
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
///
/// Represents a single conversation turn or system instruction.
/// The `role` is typically `"user"` or `"model"`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
///
/// Controls the blocking threshold for a particular [`HarmCategory`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SafetySetting {
    /// The harm category to configure.
    pub category: HarmCategory,
    /// The blocking threshold for this category.
    pub threshold: HarmBlockThreshold,
}

/// Probability rating for a safety category in a response.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmProbability {
    /// Negligible probability of harm.
    Negligible,
    /// Low probability of harm.
    Low,
    /// Medium probability of harm.
    Medium,
    /// High probability of harm.
    High,
}

/// Per-category safety rating on a response candidate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SafetyRating {
    /// The evaluated harm category.
    pub category: HarmCategory,
    /// Assessed probability of harm.
    pub probability: HarmProbability,
}

// ── Finish reason ───────────────────────────────────────────────────────

/// Reason the model stopped generating content.
///
/// Maps to the `finishReason` field in the Gemini API response.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FinishReason {
    /// Natural stop point.
    Stop,
    /// Hit the maximum token limit.
    MaxTokens,
    /// Content was filtered by safety settings.
    Safety,
    /// Model made a recitation (verbatim quote).
    Recitation,
    /// Other / unspecified reason.
    Other,
}

impl FinishReason {
    /// Parse a finish reason from a raw string value.
    ///
    /// Returns `None` if the string does not match a known reason.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "STOP" => Some(Self::Stop),
            "MAX_TOKENS" => Some(Self::MaxTokens),
            "SAFETY" => Some(Self::Safety),
            "RECITATION" => Some(Self::Recitation),
            "OTHER" => Some(Self::Other),
            _ => None,
        }
    }
}

// ── Generation config ───────────────────────────────────────────────────

/// Sampling and output parameters for content generation.
///
/// All fields are optional; omitted fields use the model's defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    /// Maximum number of output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Sampling temperature (0.0–2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Top-p (nucleus) sampling parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Top-k sampling parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Number of candidate completions to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<u32>,
    /// Stop sequences that halt generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// MIME type for the response (e.g. `application/json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,
    /// JSON Schema for structured output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<serde_json::Value>,
}

// ── Usage metadata ──────────────────────────────────────────────────────

/// Token usage metadata returned in a response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    /// Tokens consumed by the prompt.
    pub prompt_token_count: u64,
    /// Tokens generated across all candidates.
    pub candidates_token_count: u64,
    /// Total tokens (prompt + candidates).
    pub total_token_count: u64,
}

// ── Prompt feedback ─────────────────────────────────────────────────────

/// Prompt-level safety feedback returned by the API.
///
/// When the API blocks or filters a prompt, this structure explains why.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PromptFeedback {
    /// Block reason (e.g. `"SAFETY"`), if the prompt was blocked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    /// Safety ratings for the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<SafetyRating>>,
}

// ── Request ─────────────────────────────────────────────────────────────

/// A request to the Gemini `generateContent` endpoint.
///
/// Mirrors the Google Gemini REST API request body. Use the builder
/// methods to construct requests fluently.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GenerateContentRequest {
    /// Model identifier (e.g. `gemini-2.5-flash`).
    pub model: String,
    /// Conversation content blocks.
    pub contents: Vec<Content>,
    /// Optional system instruction content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,
    /// Generation configuration parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
    /// Safety settings for content filtering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<SafetySetting>>,
    /// Tool definitions available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDeclaration>>,
    /// Function-calling configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

/// A tool declaration wrapping one or more function declarations.
///
/// Corresponds to the `tools` array element in the Gemini API request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolDeclaration {
    /// Function declarations available to the model.
    pub function_declarations: Vec<FunctionDeclaration>,
}

/// A function the model may call.
///
/// Describes the function's name, purpose, and parameter schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct FunctionDeclaration {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// Top-level tool-calling configuration.
///
/// Controls function-calling behavior for the entire request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfig {
    /// Function-calling behaviour configuration.
    pub function_calling_config: FunctionCallingConfig,
}

/// Detailed function-calling constraints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCallingConfig {
    /// The function-calling mode.
    pub mode: FunctionCallingMode,
    /// Restrict calls to these function names, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_function_names: Option<Vec<String>>,
}

// ── Response types ──────────────────────────────────────────────────────

/// A single candidate completion in a response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// The generated content.
    pub content: Content,
    /// Reason the model stopped generating (e.g. `"STOP"`, `"MAX_TOKENS"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// Per-category safety ratings for this candidate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<SafetyRating>>,
}

impl Candidate {
    /// Parse the `finish_reason` string into a typed [`FinishReason`].
    #[must_use]
    pub fn finish_reason_typed(&self) -> Option<FinishReason> {
        self.finish_reason
            .as_deref()
            .and_then(FinishReason::from_str_opt)
    }
}

/// The response from a `generateContent` call.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentResponse {
    /// Response candidates from the model.
    pub candidates: Vec<Candidate>,
    /// Token usage metadata.
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
                        Part::FunctionCall { name, args } => Some((name.as_str(), args)),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

// ── Error response ──────────────────────────────────────────────────────

/// Block reason for a prompt that was rejected by the API.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BlockReason {
    /// Blocked due to safety filters.
    Safety,
    /// Blocked due to other reasons.
    Other,
    /// Blocked: the prompt contained blocklisted terms.
    Blocklist,
    /// Blocked: the prompt triggered prohibited-content filters.
    ProhibitedContent,
}

/// A structured error returned by the Gemini API on non-2xx responses.
///
/// The Gemini REST API wraps errors in `{"error": {…}}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct GeminiErrorResponse {
    /// The error detail object.
    pub error: GeminiErrorDetail,
}

/// Detail of a Gemini API error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct GeminiErrorDetail {
    /// HTTP status code.
    pub code: u16,
    /// Human-readable error message.
    pub message: String,
    /// Error status string (e.g. `"INVALID_ARGUMENT"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

impl GeminiErrorResponse {
    /// Try to parse a JSON string as a Gemini API error response.
    ///
    /// Returns `None` if the string is not valid Gemini error JSON.
    #[must_use]
    pub fn parse(body: &str) -> Option<Self> {
        serde_json::from_str(body).ok()
    }
}

// ── Streaming types ─────────────────────────────────────────────────────

/// A streaming response event from `streamGenerateContent`.
///
/// Each chunk may contain incremental text deltas, tool calls,
/// or usage metadata (typically in the final chunk).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamEvent {
    /// Response candidates in this chunk.
    pub candidates: Vec<Candidate>,
    /// Token usage metadata (usually in the final chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
