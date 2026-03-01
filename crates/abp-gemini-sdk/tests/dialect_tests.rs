// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the Gemini dialect mapping, model names, capabilities, and tool formats.

use abp_core::{AgentEventKind, Capability, SupportLevel};
use abp_gemini_sdk::dialect::{
    CanonicalToolDef, DEFAULT_MODEL, DIALECT_VERSION, FunctionCallingMode, GeminiCandidate,
    GeminiCitationMetadata, GeminiCitationSource, GeminiConfig, GeminiContent,
    GeminiFunctionCallingConfig, GeminiFunctionDeclaration, GeminiGenerationConfig,
    GeminiGroundingConfig, GeminiInlineData, GeminiPart, GeminiResponse, GeminiSafetyRating,
    GeminiSafetySetting, GeminiStreamChunk, GeminiTool, GeminiToolConfig, GeminiUsageMetadata,
    GoogleSearchRetrieval, HarmBlockThreshold, HarmCategory, HarmProbability, capability_manifest,
    from_canonical_model, is_known_model, map_stream_chunk, to_canonical_model,
    tool_def_from_gemini, tool_def_to_gemini,
};

// ---------------------------------------------------------------------------
// Model-name mapping
// ---------------------------------------------------------------------------

#[test]
fn model_roundtrip_known() {
    let vendor = "gemini-2.5-flash";
    let canonical = to_canonical_model(vendor);
    assert_eq!(canonical, "google/gemini-2.5-flash");
    let back = from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

#[test]
fn model_roundtrip_pro() {
    let canonical = to_canonical_model("gemini-2.5-pro");
    assert_eq!(canonical, "google/gemini-2.5-pro");
    assert_eq!(from_canonical_model(&canonical), "gemini-2.5-pro");
}

#[test]
fn unknown_model_roundtrips() {
    let canonical = to_canonical_model("gemini-99-ultra");
    assert_eq!(canonical, "google/gemini-99-ultra");
    assert_eq!(from_canonical_model(&canonical), "gemini-99-ultra");
}

#[test]
fn from_canonical_without_prefix_passes_through() {
    assert_eq!(from_canonical_model("bare-model"), "bare-model");
}

#[test]
fn is_known_model_recognises_known() {
    assert!(is_known_model("gemini-2.5-flash"));
    assert!(is_known_model("gemini-1.5-pro"));
    assert!(!is_known_model("totally-unknown"));
}

// ---------------------------------------------------------------------------
// Default model & version
// ---------------------------------------------------------------------------

#[test]
fn default_model_is_gemini_flash() {
    assert_eq!(DEFAULT_MODEL, "gemini-2.5-flash");
}

#[test]
fn dialect_version_is_set() {
    assert!(DIALECT_VERSION.starts_with("gemini/"));
}

// ---------------------------------------------------------------------------
// Capability mapping
// ---------------------------------------------------------------------------

#[test]
fn capability_manifest_has_streaming_native() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_structured_output_native() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_glob_unsupported() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolGlob),
        Some(SupportLevel::Unsupported)
    ));
}

// ---------------------------------------------------------------------------
// Tool-format conversion
// ---------------------------------------------------------------------------

#[test]
fn tool_def_roundtrip() {
    let canonical = CanonicalToolDef {
        name: "search".into(),
        description: "Search the web".into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "properties": { "query": { "type": "string" } },
            "required": ["query"]
        }),
    };
    let gemini = tool_def_to_gemini(&canonical);
    assert_eq!(gemini.name, "search");
    assert_eq!(gemini.description, "Search the web");

    let back = tool_def_from_gemini(&gemini);
    assert_eq!(back, canonical);
}

#[test]
fn gemini_function_declaration_serde_roundtrip() {
    let decl = GeminiFunctionDeclaration {
        name: "get_weather".into(),
        description: "Get current weather".into(),
        parameters: serde_json::json!({"type": "object"}),
    };
    let json = serde_json::to_string(&decl).unwrap();
    let parsed: GeminiFunctionDeclaration = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, decl);
}

// ---------------------------------------------------------------------------
// Serde roundtrip of dialect types
// ---------------------------------------------------------------------------

#[test]
fn gemini_config_serde_roundtrip() {
    let cfg = GeminiConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: GeminiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, cfg.model);
    assert_eq!(parsed.base_url, cfg.base_url);
}

#[test]
fn gemini_response_serde_roundtrip() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("hello".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 5,
            total_token_count: 15,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: GeminiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.candidates.len(), 1);
    assert_eq!(parsed.usage_metadata.unwrap().total_token_count, 15);
}

// ---------------------------------------------------------------------------
// Safety settings serialization
// ---------------------------------------------------------------------------

#[test]
fn safety_setting_serialization() {
    let setting = GeminiSafetySetting {
        category: HarmCategory::HarmCategoryHarassment,
        threshold: HarmBlockThreshold::BlockMediumAndAbove,
    };
    let json = serde_json::to_string(&setting).unwrap();
    assert!(json.contains("HARM_CATEGORY_HARASSMENT"));
    assert!(json.contains("BLOCK_MEDIUM_AND_ABOVE"));
    let parsed: GeminiSafetySetting = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, setting);
}

#[test]
fn safety_rating_roundtrip() {
    let rating = GeminiSafetyRating {
        category: HarmCategory::HarmCategoryHateSpeech,
        probability: HarmProbability::Low,
    };
    let json = serde_json::to_string(&rating).unwrap();
    assert!(json.contains("HARM_CATEGORY_HATE_SPEECH"));
    assert!(json.contains("LOW"));
    let parsed: GeminiSafetyRating = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, rating);
}

// ---------------------------------------------------------------------------
// Generation config defaults and customization
// ---------------------------------------------------------------------------

#[test]
fn generation_config_defaults_are_none() {
    let cfg = GeminiGenerationConfig::default();
    assert!(cfg.max_output_tokens.is_none());
    assert!(cfg.temperature.is_none());
    assert!(cfg.top_p.is_none());
    assert!(cfg.top_k.is_none());
    assert!(cfg.response_mime_type.is_none());
    assert!(cfg.response_schema.is_none());
}

#[test]
fn generation_config_custom_fields() {
    let cfg = GeminiGenerationConfig {
        max_output_tokens: Some(8192),
        temperature: Some(0.7),
        top_p: Some(0.95),
        top_k: Some(40),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(serde_json::json!({"type": "object"})),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("maxOutputTokens"));
    assert!(json.contains("topP"));
    assert!(json.contains("topK"));
    assert!(json.contains("responseMimeType"));
    assert!(json.contains("responseSchema"));
    let parsed: GeminiGenerationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.top_k, Some(40));
    assert_eq!(
        parsed.response_mime_type.as_deref(),
        Some("application/json")
    );
}

// ---------------------------------------------------------------------------
// Function declaration roundtrip
// ---------------------------------------------------------------------------

#[test]
fn function_declaration_via_tool_wrapper_roundtrip() {
    let tool = GeminiTool {
        function_declarations: vec![GeminiFunctionDeclaration {
            name: "get_weather".into(),
            description: "Get current weather".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "location": { "type": "string" } },
                "required": ["location"]
            }),
        }],
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains("functionDeclarations"));
    let parsed: GeminiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

// ---------------------------------------------------------------------------
// Multi-part content handling
// ---------------------------------------------------------------------------

#[test]
fn multi_part_content_with_inline_data() {
    let content = GeminiContent {
        role: "user".into(),
        parts: vec![
            GeminiPart::Text("Describe this image.".into()),
            GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            }),
        ],
    };
    let json = serde_json::to_string(&content).unwrap();
    assert!(json.contains("inlineData"));
    assert!(json.contains("mimeType"));
    let parsed: GeminiContent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.parts.len(), 2);
}

#[test]
fn multi_part_content_function_call_and_response() {
    let parts = vec![
        GeminiPart::FunctionCall {
            name: "search".into(),
            args: serde_json::json!({"q": "rust"}),
        },
        GeminiPart::FunctionResponse {
            name: "search".into(),
            response: serde_json::json!({"results": []}),
        },
    ];
    let content = GeminiContent {
        role: "model".into(),
        parts,
    };
    let json = serde_json::to_string(&content).unwrap();
    let parsed: GeminiContent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.parts.len(), 2);
}

// ---------------------------------------------------------------------------
// Stream chunk → AgentEvent mapping
// ---------------------------------------------------------------------------

#[test]
fn stream_chunk_text_maps_to_assistant_delta() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Hello".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn stream_chunk_function_call_maps_to_tool_call() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "read_file".into(),
                    args: serde_json::json!({"path": "main.rs"}),
                }],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { tool_name, .. } => assert_eq!(tool_name, "read_file"),
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn stream_chunk_with_safety_ratings_and_citations() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("result".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: Some(vec![GeminiSafetyRating {
                category: HarmCategory::HarmCategoryDangerousContent,
                probability: HarmProbability::Negligible,
            }]),
            citation_metadata: Some(GeminiCitationMetadata {
                citation_sources: vec![GeminiCitationSource {
                    start_index: Some(0),
                    end_index: Some(10),
                    uri: Some("https://example.com".into()),
                    license: None,
                }],
            }),
        }],
        usage_metadata: None,
    };
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
}

// ---------------------------------------------------------------------------
// Tool config modes
// ---------------------------------------------------------------------------

#[test]
fn tool_config_auto_mode() {
    let cfg = GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode: FunctionCallingMode::Auto,
            allowed_function_names: None,
        },
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("AUTO"));
    let parsed: GeminiToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, cfg);
}

#[test]
fn tool_config_any_mode_with_allowed_functions() {
    let cfg = GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["search".into(), "read_file".into()]),
        },
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("ANY"));
    assert!(json.contains("allowedFunctionNames"));
    let parsed: GeminiToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, cfg);
}

#[test]
fn tool_config_none_mode() {
    let cfg = GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode: FunctionCallingMode::None,
            allowed_function_names: None,
        },
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("NONE"));
    let parsed: GeminiToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, cfg);
}

// ---------------------------------------------------------------------------
// Grounding config
// ---------------------------------------------------------------------------

#[test]
fn grounding_config_roundtrip() {
    let cfg = GeminiGroundingConfig {
        google_search_retrieval: Some(GoogleSearchRetrieval {
            dynamic_retrieval_config: None,
        }),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("googleSearchRetrieval"));
    let parsed: GeminiGroundingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, cfg);
}
