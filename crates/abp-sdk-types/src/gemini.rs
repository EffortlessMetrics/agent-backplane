// SPDX-License-Identifier: MIT OR Apache-2.0
//! Google Gemini generateContent API type definitions.
//!
//! Mirrors the Gemini API request/response surface.
//! See <https://ai.google.dev/api/generate-content>.

use serde::{Deserialize, Serialize};

// ── Content types ───────────────────────────────────────────────────────

/// A content block in the Gemini API format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiContent {
    /// Role of the content author (`user` or `model`).
    pub role: String,
    /// Content parts.
    pub parts: Vec<GeminiPart>,
}

/// A part within a Gemini content block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum GeminiPart {
    /// Plain text content.
    Text(String),
    /// Inline binary data (e.g. images).
    InlineData(GeminiInlineData),
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

/// Inline binary data embedded in a Gemini content block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiInlineData {
    /// MIME type of the data (e.g. `image/png`).
    pub mime_type: String,
    /// Base64-encoded binary data.
    pub data: String,
}

// ── Tool types ──────────────────────────────────────────────────────────

/// Gemini-style function declaration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiFunctionDeclaration {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// Wraps function declarations for the Gemini `tools` field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiTool {
    /// Function declarations available to the model.
    pub function_declarations: Vec<GeminiFunctionDeclaration>,
}

/// Function-calling mode for Gemini requests.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FunctionCallingMode {
    /// Model decides whether to call functions.
    Auto,
    /// Model must call at least one function.
    Any,
    /// Model must not call any functions.
    None,
}

/// Controls function-calling behavior for Gemini requests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiToolConfig {
    /// Function-calling behaviour configuration.
    pub function_calling_config: GeminiFunctionCallingConfig,
}

/// Detailed function-calling configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiFunctionCallingConfig {
    /// The function-calling mode.
    pub mode: FunctionCallingMode,
    /// Restrict calls to these function names, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_function_names: Option<Vec<String>>,
}

// ── Generation config ───────────────────────────────────────────────────

/// Generation parameters for the Gemini API.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerationConfig {
    /// Maximum number of output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Top-p (nucleus) sampling parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Top-k sampling parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Stop sequences that halt generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
}

// ── Safety settings ─────────────────────────────────────────────────────

/// Harm categories for Gemini safety configuration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmCategory {
    /// Harassment content.
    HarmCategoryHarassment,
    /// Hate speech content.
    HarmCategoryHateSpeech,
    /// Sexually explicit content.
    HarmCategorySexuallyExplicit,
    /// Dangerous content.
    HarmCategoryDangerousContent,
}

/// Threshold levels for blocking harmful content.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmBlockThreshold {
    /// Do not block any content.
    BlockNone,
    /// Block medium-probability harmful content and above.
    BlockMediumAndAbove,
    /// Only block high-probability harmful content.
    BlockOnlyHigh,
}

/// A safety setting applied to a Gemini request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiSafetySetting {
    /// The harm category to configure.
    pub category: HarmCategory,
    /// The blocking threshold for this category.
    pub threshold: HarmBlockThreshold,
}

// ── Request ─────────────────────────────────────────────────────────────

/// Gemini generateContent API request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiRequest {
    /// Model identifier (e.g. `gemini-2.5-flash`).
    pub model: String,
    /// Conversation content blocks.
    pub contents: Vec<GeminiContent>,
    /// Optional system instruction content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiContent>,
    /// Generation configuration parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    /// Safety settings for content filtering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<GeminiSafetySetting>>,
    /// Tool definitions available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiTool>>,
    /// Function-calling configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<GeminiToolConfig>,
}

// ── Response ────────────────────────────────────────────────────────────

/// Gemini generateContent API response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiResponse {
    /// Response candidates from the model.
    pub candidates: Vec<GeminiCandidate>,
    /// Token usage metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<GeminiUsageMetadata>,
}

/// A candidate completion in a Gemini response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCandidate {
    /// The generated content.
    pub content: GeminiContent,
    /// Reason the model stopped generating (e.g. `STOP`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Token usage reported by the Gemini API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiUsageMetadata {
    /// Tokens consumed by the prompt.
    pub prompt_token_count: u64,
    /// Tokens generated across all candidates.
    pub candidates_token_count: u64,
    /// Total tokens (prompt + candidates).
    pub total_token_count: u64,
}

// ── Streaming ───────────────────────────────────────────────────────────

/// A single chunk in a streaming Gemini response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiStreamChunk {
    /// Response candidates in this chunk.
    #[serde(default)]
    pub candidates: Vec<GeminiCandidate>,
    /// Token usage metadata (usually in the final chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<GeminiUsageMetadata>,
}

// ── Model config ────────────────────────────────────────────────────────

/// Vendor-specific configuration for the Google Gemini API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct GeminiConfig {
    /// Base URL for the Gemini API.
    pub base_url: String,
    /// Model identifier (e.g. `gemini-2.5-flash`).
    pub model: String,
    /// Maximum output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Temperature for sampling (0.0–2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
}

impl Default for GeminiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            model: "gemini-2.5-flash".into(),
            max_output_tokens: Some(4096),
            temperature: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serde_roundtrip() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hello".into())],
            }],
            system_instruction: None,
            generation_config: Some(GeminiGenerationConfig {
                max_output_tokens: Some(4096),
                temperature: Some(0.7),
                ..Default::default()
            }),
            safety_settings: None,
            tools: Some(vec![GeminiTool {
                function_declarations: vec![GeminiFunctionDeclaration {
                    name: "search".into(),
                    description: "Search the web".into(),
                    parameters: serde_json::json!({"type": "object"}),
                }],
            }]),
            tool_config: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: GeminiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn response_serde_roundtrip() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("Hi!".into())],
                },
                finish_reason: Some("STOP".into()),
            }],
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 5,
                total_token_count: 15,
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: GeminiResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn stream_chunk_serde_roundtrip() {
        let chunk = GeminiStreamChunk {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("delta".into())],
                },
                finish_reason: None,
            }],
            usage_metadata: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: GeminiStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(chunk, back);
    }

    #[test]
    fn function_call_part_roundtrip() {
        let part = GeminiPart::FunctionCall {
            name: "search".into(),
            args: serde_json::json!({"query": "rust async"}),
        };
        let json = serde_json::to_string(&part).unwrap();
        let back: GeminiPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
    }

    #[test]
    fn config_default_values() {
        let cfg = GeminiConfig::default();
        assert!(cfg.base_url.contains("googleapis.com"));
        assert!(cfg.model.contains("gemini"));
        assert!(cfg.max_output_tokens.unwrap_or(0) > 0);
    }
}
