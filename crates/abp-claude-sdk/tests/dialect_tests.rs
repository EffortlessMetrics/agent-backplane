// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the Claude dialect mapping, model names, capabilities, and tool formats.

use abp_claude_sdk::dialect::{
    CanonicalToolDef, ClaudeConfig, ClaudeContentBlock, ClaudeResponse, ClaudeToolDef, ClaudeUsage,
    DEFAULT_MODEL, DIALECT_VERSION, capability_manifest, from_canonical_model, is_known_model,
    to_canonical_model, tool_def_from_claude, tool_def_to_claude,
};
use abp_core::{Capability, SupportLevel};

// ---------------------------------------------------------------------------
// Model-name mapping
// ---------------------------------------------------------------------------

#[test]
fn model_roundtrip_known() {
    let vendor = "claude-sonnet-4-20250514";
    let canonical = to_canonical_model(vendor);
    assert_eq!(canonical, "anthropic/claude-sonnet-4-20250514");
    let back = from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

#[test]
fn model_roundtrip_opus() {
    let canonical = to_canonical_model("claude-opus-4-20250514");
    assert_eq!(canonical, "anthropic/claude-opus-4-20250514");
    assert_eq!(from_canonical_model(&canonical), "claude-opus-4-20250514");
}

#[test]
fn unknown_model_roundtrips() {
    let canonical = to_canonical_model("claude-future-5");
    assert_eq!(canonical, "anthropic/claude-future-5");
    assert_eq!(from_canonical_model(&canonical), "claude-future-5");
}

#[test]
fn from_canonical_without_prefix_passes_through() {
    assert_eq!(from_canonical_model("bare-model"), "bare-model");
}

#[test]
fn is_known_model_recognises_known() {
    assert!(is_known_model("claude-sonnet-4-20250514"));
    assert!(is_known_model("claude-haiku-3-5-20241022"));
    assert!(!is_known_model("totally-unknown"));
}

// ---------------------------------------------------------------------------
// Default model & version
// ---------------------------------------------------------------------------

#[test]
fn default_model_is_sonnet() {
    assert_eq!(DEFAULT_MODEL, "claude-sonnet-4-20250514");
}

#[test]
fn dialect_version_is_set() {
    assert!(DIALECT_VERSION.starts_with("claude/"));
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
fn capability_manifest_has_mcp_client_native() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpClient),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_mcp_server_unsupported() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpServer),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn capability_manifest_hooks_native() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::HooksPreToolUse),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::HooksPostToolUse),
        Some(SupportLevel::Native)
    ));
}

// ---------------------------------------------------------------------------
// Tool-format conversion
// ---------------------------------------------------------------------------

#[test]
fn tool_def_roundtrip() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file from disk".into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        }),
    };
    let claude = tool_def_to_claude(&canonical);
    assert_eq!(claude.name, "read_file");
    assert_eq!(claude.description, "Read a file from disk");

    let back = tool_def_from_claude(&claude);
    assert_eq!(back, canonical);
}

#[test]
fn claude_tool_def_serde_roundtrip() {
    let def = ClaudeToolDef {
        name: "bash".into(),
        description: "Run a bash command".into(),
        input_schema: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}}),
    };
    let json = serde_json::to_string(&def).unwrap();
    let parsed: ClaudeToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, def);
}

// ---------------------------------------------------------------------------
// Serde roundtrip of dialect types
// ---------------------------------------------------------------------------

#[test]
fn claude_config_serde_roundtrip() {
    let cfg = ClaudeConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: ClaudeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, cfg.model);
    assert_eq!(parsed.base_url, cfg.base_url);
    assert_eq!(parsed.max_tokens, cfg.max_tokens);
}

#[test]
fn claude_response_serde_roundtrip() {
    let resp = ClaudeResponse {
        id: "msg_rt".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![
            ClaudeContentBlock::Text {
                text: "Hello!".into(),
            },
            ClaudeContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "bash".into(),
                input: serde_json::json!({"command": "ls"}),
            },
        ],
        stop_reason: Some("end_turn".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 20,
            output_tokens: 10,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: ClaudeResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "msg_rt");
    assert_eq!(parsed.content.len(), 2);
    assert_eq!(parsed.usage.unwrap().output_tokens, 10);
}
