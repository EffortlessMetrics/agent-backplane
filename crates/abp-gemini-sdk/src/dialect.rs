// SPDX-License-Identifier: MIT OR Apache-2.0
//! Google Gemini dialect: config, request/response types, and mapping logic.

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, SupportLevel, WorkOrder,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Version string for this dialect adapter.
pub const DIALECT_VERSION: &str = "gemini/v0.1";

/// Default model used when none is specified.
pub const DEFAULT_MODEL: &str = "gemini-2.5-flash";

// ---------------------------------------------------------------------------
// Model-name mapping
// ---------------------------------------------------------------------------

/// Known Gemini model identifiers.
const KNOWN_MODELS: &[&str] = &[
    "gemini-2.5-flash",
    "gemini-2.5-pro",
    "gemini-2.0-flash",
    "gemini-2.0-flash-lite",
    "gemini-1.5-flash",
    "gemini-1.5-pro",
];

/// Map a vendor model name to the ABP canonical form (`google/<model>`).
#[must_use]
pub fn to_canonical_model(vendor_model: &str) -> String {
    format!("google/{vendor_model}")
}

/// Map an ABP canonical model name back to the vendor model name.
///
/// Strips the `google/` prefix if present; otherwise returns the input unchanged.
#[must_use]
pub fn from_canonical_model(canonical: &str) -> String {
    canonical
        .strip_prefix("google/")
        .unwrap_or(canonical)
        .to_string()
}

/// Returns `true` if `model` is a known Gemini model identifier.
#[must_use]
pub fn is_known_model(model: &str) -> bool {
    KNOWN_MODELS.contains(&model)
}

// ---------------------------------------------------------------------------
// Capability mapping
// ---------------------------------------------------------------------------

/// Build a [`CapabilityManifest`] describing what the Gemini backend supports.
#[must_use]
pub fn capability_manifest() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m.insert(Capability::ToolEdit, SupportLevel::Emulated);
    m.insert(Capability::ToolBash, SupportLevel::Emulated);
    m.insert(Capability::StructuredOutputJsonSchema, SupportLevel::Native);
    m.insert(Capability::ToolGlob, SupportLevel::Unsupported);
    m.insert(Capability::ToolGrep, SupportLevel::Unsupported);
    m.insert(Capability::McpClient, SupportLevel::Unsupported);
    m.insert(Capability::McpServer, SupportLevel::Unsupported);
    m
}

// ---------------------------------------------------------------------------
// Tool-format translation
// ---------------------------------------------------------------------------

/// A vendor-agnostic tool definition used as the ABP canonical form.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalToolDef {
    pub name: String,
    pub description: String,
    pub parameters_schema: serde_json::Value,
}

/// Gemini-style function declaration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeminiFunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Convert an ABP canonical tool definition to the Gemini function declaration format.
#[must_use]
pub fn tool_def_to_gemini(def: &CanonicalToolDef) -> GeminiFunctionDeclaration {
    GeminiFunctionDeclaration {
        name: def.name.clone(),
        description: def.description.clone(),
        parameters: def.parameters_schema.clone(),
    }
}

/// Convert a Gemini function declaration back to the ABP canonical form.
#[must_use]
pub fn tool_def_from_gemini(def: &GeminiFunctionDeclaration) -> CanonicalToolDef {
    CanonicalToolDef {
        name: def.name.clone(),
        description: def.description.clone(),
        parameters_schema: def.parameters.clone(),
    }
}

/// Vendor-specific configuration for the Google Gemini API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiConfig {
    /// Google AI API key.
    pub api_key: String,

    /// Base URL for the Gemini API.
    pub base_url: String,

    /// Model identifier (e.g. `gemini-2.5-flash`).
    pub model: String,

    /// Maximum output tokens.
    pub max_output_tokens: Option<u32>,

    /// Temperature for sampling (0.0–2.0).
    pub temperature: Option<f64>,
}

impl Default for GeminiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            model: "gemini-2.5-flash".into(),
            max_output_tokens: Some(4096),
            temperature: None,
        }
    }
}

/// Simplified representation of a Gemini generateContent request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiRequest {
    pub model: String,
    pub contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<GeminiSafetySetting>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<GeminiToolConfig>,
}

/// A content block in the Gemini API format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiContent {
    pub role: String,
    pub parts: Vec<GeminiPart>,
}

/// Inline binary data (e.g. images) embedded in a Gemini content block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiInlineData {
    pub mime_type: String,
    pub data: String,
}

/// A part within a Gemini content block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GeminiPart {
    Text(String),
    InlineData(GeminiInlineData),
    FunctionCall {
        name: String,
        args: serde_json::Value,
    },
    FunctionResponse {
        name: String,
        response: serde_json::Value,
    },
}

/// Generation parameters for the Gemini API.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<serde_json::Value>,
}

/// Simplified representation of a Gemini generateContent response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiResponse {
    pub candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    pub usage_metadata: Option<GeminiUsageMetadata>,
}

/// A candidate completion in a Gemini response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiCandidate {
    pub content: GeminiContent,
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<GeminiSafetyRating>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation_metadata: Option<GeminiCitationMetadata>,
}

/// Token usage reported by the Gemini API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiUsageMetadata {
    pub prompt_token_count: u64,
    pub candidates_token_count: u64,
    pub total_token_count: u64,
}

// ---------------------------------------------------------------------------
// Safety settings
// ---------------------------------------------------------------------------

/// Harm categories for Gemini safety configuration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmCategory {
    HarmCategoryHarassment,
    HarmCategoryHateSpeech,
    HarmCategorySexuallyExplicit,
    HarmCategoryDangerousContent,
    HarmCategoryCivicIntegrity,
}

/// Threshold levels for blocking harmful content.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmBlockThreshold {
    BlockNone,
    BlockLowAndAbove,
    BlockMediumAndAbove,
    BlockOnlyHigh,
}

/// A safety setting applied to a Gemini request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiSafetySetting {
    pub category: HarmCategory,
    pub threshold: HarmBlockThreshold,
}

/// Probability rating for a safety category in a response.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmProbability {
    Negligible,
    Low,
    Medium,
    High,
}

/// Safety rating returned for a response candidate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiSafetyRating {
    pub category: HarmCategory,
    pub probability: HarmProbability,
}

// ---------------------------------------------------------------------------
// Grounding configuration
// ---------------------------------------------------------------------------

/// Grounding configuration for augmenting generation with Google Search.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGroundingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_search_retrieval: Option<GoogleSearchRetrieval>,
}

/// Configuration for Google Search retrieval grounding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleSearchRetrieval {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic_retrieval_config: Option<DynamicRetrievalConfig>,
}

/// Dynamic retrieval thresholds for grounding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DynamicRetrievalConfig {
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic_threshold: Option<f64>,
}

// ---------------------------------------------------------------------------
// Tool configuration
// ---------------------------------------------------------------------------

/// Wraps function declarations for the Gemini `tools` field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiTool {
    pub function_declarations: Vec<GeminiFunctionDeclaration>,
}

/// Function-calling mode for Gemini requests.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FunctionCallingMode {
    Auto,
    Any,
    None,
}

/// Controls function-calling behavior for Gemini requests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiToolConfig {
    pub function_calling_config: GeminiFunctionCallingConfig,
}

/// Detailed function-calling configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiFunctionCallingConfig {
    pub mode: FunctionCallingMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_function_names: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Citation metadata
// ---------------------------------------------------------------------------

/// Citation metadata returned with a candidate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCitationMetadata {
    pub citation_sources: Vec<GeminiCitationSource>,
}

/// A single citation source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCitationSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

/// A single chunk in a streaming Gemini response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiStreamChunk {
    #[serde(default)]
    pub candidates: Vec<GeminiCandidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<GeminiUsageMetadata>,
}

/// Map an ABP [`WorkOrder`] to a [`GeminiRequest`].
///
/// Uses the work order task as the initial user message and applies
/// config defaults where the work order does not specify overrides.
pub fn map_work_order(wo: &WorkOrder, config: &GeminiConfig) -> GeminiRequest {
    let model = wo
        .config
        .model
        .as_deref()
        .unwrap_or(&config.model)
        .to_string();

    let mut user_text = wo.task.clone();
    for snippet in &wo.context.snippets {
        user_text.push_str(&format!(
            "\n\n--- {} ---\n{}",
            snippet.name, snippet.content
        ));
    }

    let generation_config = if config.max_output_tokens.is_some() || config.temperature.is_some() {
        Some(GeminiGenerationConfig {
            max_output_tokens: config.max_output_tokens,
            temperature: config.temperature,
            ..Default::default()
        })
    } else {
        None
    };

    GeminiRequest {
        model,
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text(user_text)],
        }],
        system_instruction: None,
        generation_config,
        safety_settings: None,
        tools: None,
        tool_config: None,
    }
}

/// Map a [`GeminiResponse`] back to a sequence of ABP [`AgentEvent`]s.
pub fn map_response(resp: &GeminiResponse) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let now = Utc::now();

    for candidate in &resp.candidates {
        for part in &candidate.content.parts {
            match part {
                GeminiPart::Text(text) => {
                    events.push(AgentEvent {
                        ts: now,
                        kind: AgentEventKind::AssistantMessage { text: text.clone() },
                        ext: None,
                    });
                }
                GeminiPart::FunctionCall { name, args } => {
                    events.push(AgentEvent {
                        ts: now,
                        kind: AgentEventKind::ToolCall {
                            tool_name: name.clone(),
                            tool_use_id: None,
                            parent_tool_use_id: None,
                            input: args.clone(),
                        },
                        ext: None,
                    });
                }
                GeminiPart::FunctionResponse { .. } | GeminiPart::InlineData(_) => {
                    // Function responses and inline data are not output events.
                }
            }
        }
    }

    events
}

/// Map a [`GeminiStreamChunk`] to a sequence of ABP [`AgentEvent`]s.
///
/// Unlike [`map_response`], text parts are emitted as [`AgentEventKind::AssistantDelta`]
/// since streaming delivers incremental content.
pub fn map_stream_chunk(chunk: &GeminiStreamChunk) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let now = Utc::now();

    for candidate in &chunk.candidates {
        for part in &candidate.content.parts {
            match part {
                GeminiPart::Text(text) => {
                    events.push(AgentEvent {
                        ts: now,
                        kind: AgentEventKind::AssistantDelta { text: text.clone() },
                        ext: None,
                    });
                }
                GeminiPart::FunctionCall { name, args } => {
                    events.push(AgentEvent {
                        ts: now,
                        kind: AgentEventKind::ToolCall {
                            tool_name: name.clone(),
                            tool_use_id: None,
                            parent_tool_use_id: None,
                            input: args.clone(),
                        },
                        ext: None,
                    });
                }
                GeminiPart::FunctionResponse { .. } | GeminiPart::InlineData(_) => {}
            }
        }
    }

    events
}
#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::WorkOrderBuilder;

    #[test]
    fn default_config_has_sensible_values() {
        let cfg = GeminiConfig::default();
        assert!(cfg.base_url.contains("googleapis.com"));
        assert!(cfg.model.contains("gemini"));
        assert!(cfg.max_output_tokens.unwrap_or(0) > 0);
    }

    #[test]
    fn map_work_order_uses_task_as_user_content() {
        let wo = WorkOrderBuilder::new("Migrate to async").build();
        let cfg = GeminiConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.contents.len(), 1);
        assert_eq!(req.contents[0].role, "user");
        match &req.contents[0].parts[0] {
            GeminiPart::Text(t) => assert!(t.contains("Migrate to async")),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn map_work_order_respects_model_override() {
        let wo = WorkOrderBuilder::new("task")
            .model("gemini-2.5-pro")
            .build();
        let cfg = GeminiConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.model, "gemini-2.5-pro");
    }

    #[test]
    fn map_response_produces_assistant_message() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("Here you go.".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Here you go."),
            other => panic!("expected AssistantMessage, got {other:?}"),
        }
    }

    #[test]
    fn map_response_handles_function_call() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::FunctionCall {
                        name: "search".into(),
                        args: serde_json::json!({"query": "rust async"}),
                    }],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall { tool_name, .. } => {
                assert_eq!(tool_name, "search");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}
