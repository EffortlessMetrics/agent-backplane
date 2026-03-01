// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the Kimi dialect mapping, model names, capabilities, and tool formats.

use abp_core::{Capability, SupportLevel};
use abp_kimi_sdk::dialect::{
    CanonicalToolDef, DEFAULT_MODEL, DIALECT_VERSION, KimiChoice, KimiConfig, KimiFunctionCall,
    KimiFunctionDef, KimiResponse, KimiResponseMessage, KimiToolCall, KimiToolDef, KimiUsage,
    capability_manifest, from_canonical_model, is_known_model, to_canonical_model,
    tool_def_from_kimi, tool_def_to_kimi,
};

// ---------------------------------------------------------------------------
// Model-name mapping
// ---------------------------------------------------------------------------

#[test]
fn model_roundtrip_known() {
    let vendor = "moonshot-v1-8k";
    let canonical = to_canonical_model(vendor);
    assert_eq!(canonical, "moonshot/moonshot-v1-8k");
    let back = from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

#[test]
fn model_roundtrip_128k() {
    let canonical = to_canonical_model("moonshot-v1-128k");
    assert_eq!(canonical, "moonshot/moonshot-v1-128k");
    assert_eq!(from_canonical_model(&canonical), "moonshot-v1-128k");
}

#[test]
fn unknown_model_roundtrips() {
    let canonical = to_canonical_model("moonshot-v2-mega");
    assert_eq!(canonical, "moonshot/moonshot-v2-mega");
    assert_eq!(from_canonical_model(&canonical), "moonshot-v2-mega");
}

#[test]
fn from_canonical_without_prefix_passes_through() {
    assert_eq!(from_canonical_model("bare-model"), "bare-model");
}

#[test]
fn is_known_model_recognises_known() {
    assert!(is_known_model("moonshot-v1-8k"));
    assert!(is_known_model("moonshot-v1-128k"));
    assert!(!is_known_model("totally-unknown"));
}

// ---------------------------------------------------------------------------
// Default model & version
// ---------------------------------------------------------------------------

#[test]
fn default_model_is_moonshot_8k() {
    assert_eq!(DEFAULT_MODEL, "moonshot-v1-8k");
}

#[test]
fn dialect_version_is_set() {
    assert!(DIALECT_VERSION.starts_with("kimi/"));
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
fn capability_manifest_has_web_search_native() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolWebSearch),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_mcp_unsupported() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpClient),
        Some(SupportLevel::Unsupported)
    ));
}

// ---------------------------------------------------------------------------
// Tool-format conversion
// ---------------------------------------------------------------------------

#[test]
fn tool_def_roundtrip() {
    let canonical = CanonicalToolDef {
        name: "web_search".into(),
        description: "Search the web".into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "properties": { "query": { "type": "string" } },
            "required": ["query"]
        }),
    };
    let kimi = tool_def_to_kimi(&canonical);
    assert_eq!(kimi.tool_type, "function");
    assert_eq!(kimi.function.name, "web_search");

    let back = tool_def_from_kimi(&kimi);
    assert_eq!(back, canonical);
}

#[test]
fn kimi_tool_def_serde_roundtrip() {
    let def = KimiToolDef {
        tool_type: "function".into(),
        function: KimiFunctionDef {
            name: "calculator".into(),
            description: "Do math".into(),
            parameters: serde_json::json!({"type": "object"}),
        },
    };
    let json = serde_json::to_string(&def).unwrap();
    let parsed: KimiToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, def);
}

// ---------------------------------------------------------------------------
// Serde roundtrip of dialect types
// ---------------------------------------------------------------------------

#[test]
fn kimi_config_serde_roundtrip() {
    let cfg = KimiConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: KimiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, cfg.model);
    assert_eq!(parsed.base_url, cfg.base_url);
}

#[test]
fn kimi_response_serde_roundtrip() {
    let resp = KimiResponse {
        id: "cmpl_rt".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("hi".into()),
                tool_calls: Some(vec![KimiToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "search".into(),
                        arguments: r#"{"q":"rust"}"#.into(),
                    },
                }]),
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 8,
            completion_tokens: 4,
            total_tokens: 12,
        }),
        refs: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: KimiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "cmpl_rt");
    assert_eq!(parsed.choices.len(), 1);
    assert_eq!(parsed.usage.unwrap().total_tokens, 12);
}
