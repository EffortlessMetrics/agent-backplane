// SPDX-License-Identifier: MIT OR Apache-2.0
//! Gemini GenerateContent API request and response types.
//!
//! These types mirror the Google Gemini REST API surface for the
//! `generateContent` and `streamGenerateContent` endpoints. All structs
//! use `camelCase` field names to match the wire format exactly.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

/// Body of a `generateContent` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentRequest {
    /// Conversation turns.
    pub contents: Vec<Content>,

    /// Optional system-level instruction (no role required).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,

    /// Tool declarations available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiTool>>,

    /// Function-calling configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<ToolConfig>,

    /// Sampling and output parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,

    /// Per-category safety thresholds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<SafetySetting>>,
}

// ---------------------------------------------------------------------------
// Content / Part
// ---------------------------------------------------------------------------

/// A single conversation turn or system instruction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Content {
    /// `"user"` or `"model"`. Omitted for system instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,

    /// Ordered content parts.
    pub parts: Vec<Part>,
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
    InlineData {
        /// MIME type (e.g. `"image/png"`).
        #[serde(rename = "mimeType")]
        mime_type: String,
        /// Base64 payload.
        data: String,
    },

    /// A function call emitted by the model.
    FunctionCall {
        /// Function name.
        name: String,
        /// Arguments object.
        args: serde_json::Value,
    },

    /// A function result returned to the model.
    FunctionResponse {
        /// Function name.
        name: String,
        /// Response payload.
        response: serde_json::Value,
    },
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

/// Body of a `generateContent` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

// ---------------------------------------------------------------------------
// Generation config
// ---------------------------------------------------------------------------

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
}

// ---------------------------------------------------------------------------
// Safety
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Token usage
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Prompt feedback
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Finish reason
// ---------------------------------------------------------------------------

/// The reason a model stopped generating tokens.
///
/// Maps to the `finishReason` field in a Gemini [`Candidate`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FinishReason {
    /// Natural stop.
    Stop,
    /// Maximum output token limit reached.
    MaxTokens,
    /// Safety filter triggered.
    Safety,
    /// Recitation filter triggered.
    Recitation,
    /// Other / unspecified reason.
    Other,
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

/// A single chunk in a `streamGenerateContent` SSE response.
///
/// Each line of the SSE stream deserialises into one of these. The final
/// chunk typically includes `usage_metadata` and a `finish_reason`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StreamGenerateContentResponse {
    /// Candidate completions in this chunk.
    #[serde(default)]
    pub candidates: Vec<Candidate>,

    /// Token usage metadata (usually only in the final chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<UsageMetadata>,
}

// ---------------------------------------------------------------------------
// Tool / function calling
// ---------------------------------------------------------------------------

/// A tool definition wrapping one or more function declarations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiTool {
    /// Declared functions.
    pub function_declarations: Vec<FunctionDeclaration>,
}

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

/// Top-level tool-calling configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfig {
    /// Function-calling behaviour.
    pub function_calling_config: FunctionCallingConfig,
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
