// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the OpenAI dialect mapping, model names, capabilities, and tool formats.

use abp_core::{Capability, SupportLevel};
use abp_openai_sdk::dialect::{
    CanonicalToolDef, DEFAULT_MODEL, DIALECT_VERSION, OpenAIChoice, OpenAIConfig,
    OpenAIFunctionCall, OpenAIFunctionDef, OpenAIMessage, OpenAIResponse, OpenAIToolCall,
    OpenAIToolDef, OpenAIUsage, capability_manifest, from_canonical_model, is_known_model,
    to_canonical_model, tool_def_from_openai, tool_def_to_openai,
};

// ---------------------------------------------------------------------------
// Model-name mapping
// ---------------------------------------------------------------------------

#[test]
fn model_roundtrip_known() {
    let vendor = "gpt-4o";
    let canonical = to_canonical_model(vendor);
    assert_eq!(canonical, "openai/gpt-4o");
    let back = from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

#[test]
fn model_roundtrip_gpt4_turbo() {
    let canonical = to_canonical_model("gpt-4-turbo");
    assert_eq!(canonical, "openai/gpt-4-turbo");
    assert_eq!(from_canonical_model(&canonical), "gpt-4-turbo");
}

#[test]
fn unknown_model_roundtrips() {
    let canonical = to_canonical_model("gpt-future-99");
    assert_eq!(canonical, "openai/gpt-future-99");
    assert_eq!(from_canonical_model(&canonical), "gpt-future-99");
}

#[test]
fn from_canonical_without_prefix_passes_through() {
    assert_eq!(from_canonical_model("bare-model"), "bare-model");
}

#[test]
fn is_known_model_recognises_known() {
    assert!(is_known_model("gpt-4o"));
    assert!(is_known_model("gpt-4o-mini"));
    assert!(is_known_model("o1"));
    assert!(is_known_model("gpt-4.1"));
    assert!(!is_known_model("totally-unknown"));
}

// ---------------------------------------------------------------------------
// Default model & version
// ---------------------------------------------------------------------------

#[test]
fn default_model_is_gpt4o() {
    assert_eq!(DEFAULT_MODEL, "gpt-4o");
}

#[test]
fn dialect_version_is_set() {
    assert!(DIALECT_VERSION.starts_with("openai/"));
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
fn capability_manifest_mcp_unsupported() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpClient),
        Some(SupportLevel::Unsupported)
    ));
    assert!(matches!(
        m.get(&Capability::McpServer),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn capability_manifest_tools_emulated() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolBash),
        Some(SupportLevel::Emulated)
    ));
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
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
    let openai = tool_def_to_openai(&canonical);
    assert_eq!(openai.tool_type, "function");
    assert_eq!(openai.function.name, "read_file");

    let back = tool_def_from_openai(&openai);
    assert_eq!(back, canonical);
}

#[test]
fn openai_tool_def_serde_roundtrip() {
    let def = OpenAIToolDef {
        tool_type: "function".into(),
        function: OpenAIFunctionDef {
            name: "bash".into(),
            description: "Run a bash command".into(),
            parameters: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}}),
        },
    };
    let json = serde_json::to_string(&def).unwrap();
    let parsed: OpenAIToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, def);
}

#[test]
fn function_calling_roundtrip() {
    let tc = OpenAIToolCall {
        id: "call_abc123".into(),
        call_type: "function".into(),
        function: OpenAIFunctionCall {
            name: "get_weather".into(),
            arguments: r#"{"location":"London"}"#.into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: OpenAIToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

// ---------------------------------------------------------------------------
// Serde roundtrip of dialect types
// ---------------------------------------------------------------------------

#[test]
fn openai_config_serde_roundtrip() {
    let cfg = OpenAIConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: OpenAIConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, cfg.model);
    assert_eq!(parsed.base_url, cfg.base_url);
}

#[test]
fn openai_response_serde_roundtrip() {
    let resp = OpenAIResponse {
        id: "chatcmpl-rt".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(OpenAIUsage {
            prompt_tokens: 20,
            completion_tokens: 10,
            total_tokens: 30,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: OpenAIResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "chatcmpl-rt");
    assert_eq!(parsed.choices.len(), 1);
    assert_eq!(parsed.usage.unwrap().total_tokens, 30);
}

#[test]
fn openai_response_with_tool_calls_serde_roundtrip() {
    let resp = OpenAIResponse {
        id: "chatcmpl-tc".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "search".into(),
                        arguments: r#"{"query":"rust"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: OpenAIResponse = serde_json::from_str(&json).unwrap();
    let tc = parsed.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tc.len(), 1);
    assert_eq!(tc[0].function.name, "search");
}

#[test]
fn openai_message_serde_roundtrip() {
    let msg = OpenAIMessage {
        role: "tool".into(),
        content: Some("result data".into()),
        tool_calls: None,
        tool_call_id: Some("call_xyz".into()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: OpenAIMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, "tool");
    assert_eq!(parsed.tool_call_id.as_deref(), Some("call_xyz"));
}
