// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the Gemini dialect mapping, model names, capabilities, and tool formats.

use abp_core::{Capability, SupportLevel};
use abp_gemini_sdk::dialect::{
    CanonicalToolDef, DEFAULT_MODEL, DIALECT_VERSION, GeminiCandidate, GeminiConfig, GeminiContent,
    GeminiFunctionDeclaration, GeminiPart, GeminiResponse, GeminiUsageMetadata,
    capability_manifest, from_canonical_model, is_known_model, to_canonical_model,
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
