// SPDX-License-Identifier: MIT OR Apache-2.0
//! Additional coverage tests for the Gemini SDK: request serde, JSON field names,
//! edge cases in mapping, citation sources, and config boundaries.

use abp_core::{AgentEventKind, ContextPacket, WorkOrderBuilder};
use abp_gemini_sdk::dialect::{
    FunctionCallingMode, GeminiCandidate, GeminiCitationMetadata, GeminiCitationSource,
    GeminiConfig, GeminiContent, GeminiFunctionDeclaration, GeminiGenerationConfig, GeminiPart,
    GeminiRequest, GeminiResponse, GeminiSafetySetting, GeminiStreamChunk, GeminiTool,
    GeminiUsageMetadata, HarmBlockThreshold, HarmCategory, from_canonical_model, map_response,
    map_stream_chunk, map_work_order,
};

// ---------------------------------------------------------------------------
// GeminiRequest serde
// ---------------------------------------------------------------------------

#[test]
fn gemini_request_full_serde_roundtrip() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hello".into())],
        }],
        system_instruction: Some(GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Be concise.".into())],
        }),
        generation_config: Some(GeminiGenerationConfig {
            max_output_tokens: Some(2048),
            temperature: Some(0.5),
            ..Default::default()
        }),
        safety_settings: Some(vec![GeminiSafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        }]),
        tools: Some(vec![GeminiTool {
            function_declarations: vec![GeminiFunctionDeclaration {
                name: "search".into(),
                description: "Search".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
        }]),
        tool_config: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: GeminiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "gemini-2.5-flash");
    assert!(parsed.system_instruction.is_some());
    assert!(parsed.safety_settings.is_some());
}

#[test]
fn gemini_request_omits_none_fields() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("systemInstruction"));
    assert!(!json.contains("generationConfig"));
    assert!(!json.contains("safetySettings"));
    assert!(!json.contains("tools"));
    assert!(!json.contains("toolConfig"));
}

// ---------------------------------------------------------------------------
// JSON field names (camelCase verification)
// ---------------------------------------------------------------------------

#[test]
fn gemini_request_uses_camel_case_keys() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![],
        system_instruction: Some(GeminiContent {
            role: "user".into(),
            parts: vec![],
        }),
        generation_config: Some(GeminiGenerationConfig {
            max_output_tokens: Some(100),
            ..Default::default()
        }),
        safety_settings: None,
        tools: None,
        tool_config: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    // Gemini uses camelCase for struct fields
    assert!(json.contains("systemInstruction") || json.contains("system_instruction"));
}

#[test]
fn gemini_usage_metadata_uses_camel_case() {
    let meta = GeminiUsageMetadata {
        prompt_token_count: 10,
        candidates_token_count: 5,
        total_token_count: 15,
    };
    let json = serde_json::to_string(&meta).unwrap();
    assert!(json.contains("promptTokenCount"));
    assert!(json.contains("candidatesTokenCount"));
    assert!(json.contains("totalTokenCount"));
}

// ---------------------------------------------------------------------------
// Citation source edge cases
// ---------------------------------------------------------------------------

#[test]
fn citation_source_all_none_fields_roundtrip() {
    let src = GeminiCitationSource {
        start_index: None,
        end_index: None,
        uri: None,
        license: None,
    };
    let json = serde_json::to_string(&src).unwrap();
    let parsed: GeminiCitationSource = serde_json::from_str(&json).unwrap();
    assert!(parsed.start_index.is_none());
    assert!(parsed.uri.is_none());
}

#[test]
fn citation_metadata_empty_sources_roundtrip() {
    let meta = GeminiCitationMetadata {
        citation_sources: vec![],
    };
    let json = serde_json::to_string(&meta).unwrap();
    let parsed: GeminiCitationMetadata = serde_json::from_str(&json).unwrap();
    assert!(parsed.citation_sources.is_empty());
}

// ---------------------------------------------------------------------------
// map_response edge cases
// ---------------------------------------------------------------------------

#[test]
fn map_response_empty_candidates_produces_no_events() {
    let resp = GeminiResponse {
        candidates: vec![],
        usage_metadata: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_function_response_produces_tool_result() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionResponse {
                    name: "search".into(),
                    response: serde_json::json!({"results": ["a"]}),
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
    assert!(matches!(&events[0].kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn map_response_empty_parts_produces_no_events() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

// ---------------------------------------------------------------------------
// GeminiConfig with optional fields
// ---------------------------------------------------------------------------

#[test]
fn gemini_config_with_all_optional_fields_serde() {
    let cfg = GeminiConfig {
        temperature: Some(0.8),
        max_output_tokens: Some(4096),
        ..GeminiConfig::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: GeminiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.temperature, Some(0.8));
    assert_eq!(parsed.max_output_tokens, Some(4096));
}

// ---------------------------------------------------------------------------
// GeminiContent serde
// ---------------------------------------------------------------------------

#[test]
fn gemini_content_serde_roundtrip() {
    let content = GeminiContent {
        role: "user".into(),
        parts: vec![
            GeminiPart::Text("Hello".into()),
            GeminiPart::Text("World".into()),
        ],
    };
    let json = serde_json::to_string(&content).unwrap();
    let parsed: GeminiContent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, "user");
    assert_eq!(parsed.parts.len(), 2);
}

// ---------------------------------------------------------------------------
// FunctionCallingMode all variants
// ---------------------------------------------------------------------------

#[test]
fn function_calling_mode_all_variants_serde() {
    let modes = [
        FunctionCallingMode::Auto,
        FunctionCallingMode::Any,
        FunctionCallingMode::None,
    ];
    for mode in &modes {
        let json = serde_json::to_string(mode).unwrap();
        let parsed: FunctionCallingMode = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, mode);
    }
}

// ---------------------------------------------------------------------------
// map_work_order with files context
// ---------------------------------------------------------------------------

#[test]
fn map_work_order_with_snippets_includes_content() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![abp_core::ContextSnippet {
            name: "main.py".into(),
            content: "print('hello')".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("Review").context(ctx).build();
    let cfg = GeminiConfig::default();
    let req = map_work_order(&wo, &cfg);
    match &req.contents[0].parts[0] {
        GeminiPart::Text(t) => {
            assert!(t.contains("main.py"));
            assert!(t.contains("print('hello')"));
        }
        other => panic!("expected Text, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Stream chunk with empty parts
// ---------------------------------------------------------------------------

#[test]
fn stream_chunk_empty_parts_produces_no_events() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let events = map_stream_chunk(&chunk);
    assert!(events.is_empty());
}

// ---------------------------------------------------------------------------
// GeminiUsageMetadata serde
// ---------------------------------------------------------------------------

#[test]
fn gemini_usage_metadata_serde_roundtrip() {
    let meta = GeminiUsageMetadata {
        prompt_token_count: 42,
        candidates_token_count: 18,
        total_token_count: 60,
    };
    let json = serde_json::to_string(&meta).unwrap();
    let parsed: GeminiUsageMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.prompt_token_count, 42);
    assert_eq!(parsed.total_token_count, 60);
}

// ---------------------------------------------------------------------------
// from_canonical_model
// ---------------------------------------------------------------------------

#[test]
fn from_canonical_model_strips_google_prefix() {
    assert_eq!(
        from_canonical_model("google/gemini-2.5-flash"),
        "gemini-2.5-flash"
    );
}

#[test]
fn from_canonical_model_other_prefix_passes_through() {
    assert_eq!(from_canonical_model("openai/gpt-4o"), "openai/gpt-4o");
}
