// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the Codex dialect mapping, model names, capabilities, and tool formats.

use abp_codex_sdk::dialect::{
    CanonicalToolDef, CodexConfig, CodexContentPart, CodexOutputItem, CodexResponse, CodexToolDef,
    CodexUsage, DEFAULT_MODEL, DIALECT_VERSION, capability_manifest, from_canonical_model,
    is_known_model, to_canonical_model, tool_def_from_codex, tool_def_to_codex,
};
use abp_core::{Capability, SupportLevel};

// ---------------------------------------------------------------------------
// Model-name mapping
// ---------------------------------------------------------------------------

#[test]
fn model_roundtrip_known() {
    let vendor = "codex-mini-latest";
    let canonical = to_canonical_model(vendor);
    assert_eq!(canonical, "openai/codex-mini-latest");
    let back = from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

#[test]
fn model_roundtrip_gpt4() {
    let canonical = to_canonical_model("gpt-4");
    assert_eq!(canonical, "openai/gpt-4");
    assert_eq!(from_canonical_model(&canonical), "gpt-4");
}

#[test]
fn unknown_model_roundtrips() {
    let canonical = to_canonical_model("future-model-9000");
    assert_eq!(canonical, "openai/future-model-9000");
    assert_eq!(from_canonical_model(&canonical), "future-model-9000");
}

#[test]
fn from_canonical_without_prefix_passes_through() {
    assert_eq!(from_canonical_model("bare-model"), "bare-model");
}

#[test]
fn is_known_model_recognises_known() {
    assert!(is_known_model("codex-mini-latest"));
    assert!(is_known_model("gpt-4o"));
    assert!(!is_known_model("totally-unknown"));
}

// ---------------------------------------------------------------------------
// Default model & version
// ---------------------------------------------------------------------------

#[test]
fn default_model_is_codex() {
    assert_eq!(DEFAULT_MODEL, "codex-mini-latest");
}

#[test]
fn dialect_version_is_set() {
    assert!(DIALECT_VERSION.starts_with("codex/"));
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
fn capability_manifest_has_tool_bash_native() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolBash),
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
        name: "read_file".into(),
        description: "Read a file from disk".into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        }),
    };
    let codex = tool_def_to_codex(&canonical);
    assert_eq!(codex.tool_type, "function");
    assert_eq!(codex.function.name, "read_file");

    let back = tool_def_from_codex(&codex);
    assert_eq!(back, canonical);
}

#[test]
fn codex_tool_def_serde_roundtrip() {
    let def = CodexToolDef {
        tool_type: "function".into(),
        function: abp_codex_sdk::dialect::CodexFunctionDef {
            name: "shell".into(),
            description: "Run a shell command".into(),
            parameters: serde_json::json!({"type": "object"}),
        },
    };
    let json = serde_json::to_string(&def).unwrap();
    let parsed: CodexToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, def);
}

// ---------------------------------------------------------------------------
// Serde roundtrip of dialect types
// ---------------------------------------------------------------------------

#[test]
fn codex_config_serde_roundtrip() {
    let cfg = CodexConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: CodexConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, cfg.model);
    assert_eq!(parsed.base_url, cfg.base_url);
}

#[test]
fn codex_response_serde_roundtrip() {
    let resp = CodexResponse {
        id: "resp_rt".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexOutputItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText { text: "hi".into() }],
        }],
        usage: Some(CodexUsage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: CodexResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "resp_rt");
    assert_eq!(parsed.usage.unwrap().total_tokens, 15);
}
