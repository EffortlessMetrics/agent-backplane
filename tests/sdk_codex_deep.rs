#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive deep tests for the Codex SDK dialect types and lowering.
//!
//! Covers all Codex types, IR lowering/lifting, roundtrips, tool handling,
//! streaming events, sandbox/config, serde, edge cases, and work-order mapping.

use abp_codex_sdk::dialect::{
    self, CanonicalToolDef, CodexConfig, CodexContentPart, CodexFunctionDef, CodexInputItem,
    CodexRequest, CodexResponse, CodexResponseItem, CodexStreamDelta, CodexStreamEvent,
    CodexTextFormat, CodexTool, CodexToolDef, CodexUsage, FileAccess, NetworkAccess,
    ReasoningSummary, SandboxConfig,
};
use abp_codex_sdk::lowering;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEventKind, WorkOrderBuilder};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn assistant_text(text: &str) -> CodexResponseItem {
    CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText { text: text.into() }],
    }
}

fn function_call(id: &str, name: &str, args: &str) -> CodexResponseItem {
    CodexResponseItem::FunctionCall {
        id: id.into(),
        call_id: None,
        name: name.into(),
        arguments: args.into(),
    }
}

fn function_call_output(call_id: &str, output: &str) -> CodexResponseItem {
    CodexResponseItem::FunctionCallOutput {
        call_id: call_id.into(),
        output: output.into(),
    }
}

fn reasoning(texts: &[&str]) -> CodexResponseItem {
    CodexResponseItem::Reasoning {
        summary: texts
            .iter()
            .map(|t| ReasoningSummary {
                text: t.to_string(),
            })
            .collect(),
    }
}

fn make_response(items: Vec<CodexResponseItem>) -> CodexResponse {
    CodexResponse {
        id: "resp_test".into(),
        model: "codex-mini-latest".into(),
        output: items,
        usage: None,
        status: None,
    }
}

fn make_input(role: &str, content: &str) -> CodexInputItem {
    CodexInputItem::Message {
        role: role.into(),
        content: content.into(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Model name mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn to_canonical_model_adds_prefix() {
    assert_eq!(
        dialect::to_canonical_model("codex-mini-latest"),
        "openai/codex-mini-latest"
    );
}

#[test]
fn to_canonical_model_unknown_model() {
    assert_eq!(
        dialect::to_canonical_model("unknown-7b"),
        "openai/unknown-7b"
    );
}

#[test]
fn from_canonical_model_strips_prefix() {
    assert_eq!(dialect::from_canonical_model("openai/o3-mini"), "o3-mini");
}

#[test]
fn from_canonical_model_no_prefix_passthrough() {
    assert_eq!(dialect::from_canonical_model("local-model"), "local-model");
}

#[test]
fn canonical_model_roundtrip() {
    let model = "gpt-4o";
    let canonical = dialect::to_canonical_model(model);
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, model);
}

#[test]
fn known_model_check_codex_mini() {
    assert!(dialect::is_known_model("codex-mini-latest"));
}

#[test]
fn known_model_check_gpt4o() {
    assert!(dialect::is_known_model("gpt-4o"));
}

#[test]
fn known_model_check_o3_mini() {
    assert!(dialect::is_known_model("o3-mini"));
}

#[test]
fn known_model_check_o4_mini() {
    assert!(dialect::is_known_model("o4-mini"));
}

#[test]
fn unknown_model_returns_false() {
    assert!(!dialect::is_known_model("llama-3"));
}

#[test]
fn known_model_gpt41() {
    assert!(dialect::is_known_model("gpt-4.1"));
}

#[test]
fn known_model_gpt41_mini() {
    assert!(dialect::is_known_model("gpt-4.1-mini"));
}

#[test]
fn known_model_gpt41_nano() {
    assert!(dialect::is_known_model("gpt-4.1-nano"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Capability manifest
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_manifest_has_streaming_native() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_tool_read_native() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_tool_write_native() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolWrite),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_tool_edit_native() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolEdit),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_tool_bash_native() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolBash),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_glob_emulated() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolGlob),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn capability_manifest_grep_emulated() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolGrep),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn capability_manifest_json_schema_native() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_mcp_unsupported() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
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
fn capability_manifest_hooks_emulated() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::HooksPreToolUse),
        Some(SupportLevel::Emulated)
    ));
    assert!(matches!(
        m.get(&Capability::HooksPostToolUse),
        Some(SupportLevel::Emulated)
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Tool definition conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_def_to_codex_basic() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let codex = dialect::tool_def_to_codex(&canonical);
    assert_eq!(codex.tool_type, "function");
    assert_eq!(codex.function.name, "read_file");
    assert_eq!(codex.function.description, "Read a file");
    assert_eq!(codex.function.parameters, canonical.parameters_schema);
}

#[test]
fn tool_def_from_codex_basic() {
    let codex = CodexToolDef {
        tool_type: "function".into(),
        function: CodexFunctionDef {
            name: "shell".into(),
            description: "Run shell command".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let canonical = dialect::tool_def_from_codex(&codex);
    assert_eq!(canonical.name, "shell");
    assert_eq!(canonical.description, "Run shell command");
}

#[test]
fn tool_def_roundtrip() {
    let original = CanonicalToolDef {
        name: "edit".into(),
        description: "Edit a file".into(),
        parameters_schema: json!({"type": "object", "properties": {"file": {"type": "string"}}}),
    };
    let codex = dialect::tool_def_to_codex(&original);
    let back = dialect::tool_def_from_codex(&codex);
    assert_eq!(back, original);
}

#[test]
fn codex_tool_function_to_canonical() {
    let tool = CodexTool::Function {
        function: CodexFunctionDef {
            name: "grep".into(),
            description: "Search files".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "grep");
    assert_eq!(canonical.description, "Search files");
}

#[test]
fn codex_tool_code_interpreter_to_canonical() {
    let tool = CodexTool::CodeInterpreter {};
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "code_interpreter");
    assert!(canonical.description.contains("sandboxed"));
}

#[test]
fn codex_tool_file_search_to_canonical() {
    let tool = CodexTool::FileSearch {
        max_num_results: Some(10),
    };
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "file_search");
    assert!(canonical.description.contains("Search"));
}

#[test]
fn codex_tool_file_search_none_results() {
    let tool = CodexTool::FileSearch {
        max_num_results: None,
    };
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "file_search");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Sandbox configuration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sandbox_config_default() {
    let s = SandboxConfig::default();
    assert_eq!(s.container_image, None);
    assert_eq!(s.networking, NetworkAccess::None);
    assert_eq!(s.file_access, FileAccess::WorkspaceOnly);
    assert_eq!(s.timeout_seconds, Some(300));
    assert_eq!(s.memory_mb, Some(512));
    assert!(s.env.is_empty());
}

#[test]
fn sandbox_config_serde_roundtrip() {
    let s = SandboxConfig {
        container_image: Some("node:20".into()),
        networking: NetworkAccess::Full,
        file_access: FileAccess::Full,
        timeout_seconds: Some(600),
        memory_mb: Some(1024),
        env: [("FOO".into(), "bar".into())].into_iter().collect(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: SandboxConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn network_access_none_default() {
    let n = NetworkAccess::default();
    assert_eq!(n, NetworkAccess::None);
}

#[test]
fn network_access_allow_list_serde() {
    let n = NetworkAccess::AllowList(vec!["api.example.com".into()]);
    let json = serde_json::to_string(&n).unwrap();
    let back: NetworkAccess = serde_json::from_str(&json).unwrap();
    assert_eq!(back, n);
}

#[test]
fn file_access_default_is_workspace_only() {
    assert_eq!(FileAccess::default(), FileAccess::WorkspaceOnly);
}

#[test]
fn file_access_read_only_external_serde() {
    let fa = FileAccess::ReadOnlyExternal;
    let json = serde_json::to_string(&fa).unwrap();
    let back: FileAccess = serde_json::from_str(&json).unwrap();
    assert_eq!(back, fa);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. CodexConfig defaults
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_config_default_base_url() {
    let c = CodexConfig::default();
    assert!(c.base_url.contains("openai.com"));
}

#[test]
fn codex_config_default_model() {
    let c = CodexConfig::default();
    assert_eq!(c.model, "codex-mini-latest");
}

#[test]
fn codex_config_default_max_tokens() {
    let c = CodexConfig::default();
    assert_eq!(c.max_output_tokens, Some(4096));
}

#[test]
fn codex_config_default_temperature_none() {
    let c = CodexConfig::default();
    assert!(c.temperature.is_none());
}

#[test]
fn codex_config_default_api_key_empty() {
    let c = CodexConfig::default();
    assert!(c.api_key.is_empty());
}

#[test]
fn codex_config_default_sandbox() {
    let c = CodexConfig::default();
    assert_eq!(c.sandbox, SandboxConfig::default());
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Text format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn text_format_default_is_text() {
    let tf = CodexTextFormat::default();
    assert!(matches!(tf, CodexTextFormat::Text {}));
}

#[test]
fn text_format_json_object_serde() {
    let tf = CodexTextFormat::JsonObject {};
    let json = serde_json::to_string(&tf).unwrap();
    let back: CodexTextFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tf);
}

#[test]
fn text_format_json_schema_serde() {
    let tf = CodexTextFormat::JsonSchema {
        name: "output".into(),
        schema: json!({"type": "object"}),
        strict: true,
    };
    let json = serde_json::to_string(&tf).unwrap();
    let back: CodexTextFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tf);
}

#[test]
fn text_format_json_schema_strict_default_false() {
    let tf: CodexTextFormat =
        serde_json::from_str(r#"{"type":"json_schema","name":"x","schema":{}}"#).unwrap();
    match tf {
        CodexTextFormat::JsonSchema { strict, .. } => assert!(!strict),
        _ => panic!("expected JsonSchema"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. CodexInputItem serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn input_item_message_serde_roundtrip() {
    let item = make_input("user", "hello");
    let json = serde_json::to_string(&item).unwrap();
    let back: CodexInputItem = serde_json::from_str(&json).unwrap();
    match back {
        CodexInputItem::Message { role, content } => {
            assert_eq!(role, "user");
            assert_eq!(content, "hello");
        }
    }
}

#[test]
fn input_item_system_role_serde() {
    let item = make_input("system", "Be concise");
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains("system"));
}

#[test]
fn input_item_has_type_tag() {
    let item = make_input("user", "test");
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"message"#));
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. CodexResponseItem serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_item_message_serde() {
    let item = assistant_text("hello");
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"message"#));
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    match back {
        CodexResponseItem::Message { role, content } => {
            assert_eq!(role, "assistant");
            assert_eq!(content.len(), 1);
        }
        _ => panic!("expected Message"),
    }
}

#[test]
fn response_item_function_call_serde() {
    let item = function_call("fc_1", "shell", r#"{"cmd":"ls"}"#);
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"function_call"#));
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    match back {
        CodexResponseItem::FunctionCall {
            id,
            name,
            arguments,
            ..
        } => {
            assert_eq!(id, "fc_1");
            assert_eq!(name, "shell");
            assert!(arguments.contains("ls"));
        }
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn response_item_function_call_output_serde() {
    let item = function_call_output("fc_1", "result data");
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"function_call_output"#));
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    match back {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "fc_1");
            assert_eq!(output, "result data");
        }
        _ => panic!("expected FunctionCallOutput"),
    }
}

#[test]
fn response_item_reasoning_serde() {
    let item = reasoning(&["step one", "step two"]);
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"reasoning"#));
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    match back {
        CodexResponseItem::Reasoning { summary } => {
            assert_eq!(summary.len(), 2);
            assert_eq!(summary[0].text, "step one");
        }
        _ => panic!("expected Reasoning"),
    }
}

#[test]
fn response_item_function_call_with_call_id() {
    let item = CodexResponseItem::FunctionCall {
        id: "fc_2".into(),
        call_id: Some("corr_1".into()),
        name: "read".into(),
        arguments: r#"{"path":"a.rs"}"#.into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains("corr_1"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. CodexContentPart serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn content_part_output_text_serde() {
    let part = CodexContentPart::OutputText {
        text: "hello world".into(),
    };
    let json = serde_json::to_string(&part).unwrap();
    assert!(json.contains(r#""type":"output_text"#));
    let back: CodexContentPart = serde_json::from_str(&json).unwrap();
    match back {
        CodexContentPart::OutputText { text } => assert_eq!(text, "hello world"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. CodexUsage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_usage_serde() {
    let u = CodexUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: CodexUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.input_tokens, 100);
    assert_eq!(back.output_tokens, 50);
    assert_eq!(back.total_tokens, 150);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. CodexRequest serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_request_minimal_serde() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![make_input("user", "hi")],
        max_output_tokens: None,
        temperature: None,
        tools: Vec::new(),
        text: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CodexRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "codex-mini-latest");
    assert_eq!(back.input.len(), 1);
}

#[test]
fn codex_request_with_tools() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![make_input("user", "search")],
        max_output_tokens: Some(1024),
        temperature: Some(0.5),
        tools: vec![CodexTool::Function {
            function: CodexFunctionDef {
                name: "grep".into(),
                description: "Search".into(),
                parameters: json!({"type":"object"}),
            },
        }],
        text: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("grep"));
}

#[test]
fn codex_request_with_text_format() {
    let req = CodexRequest {
        model: "m".into(),
        input: vec![],
        max_output_tokens: None,
        temperature: None,
        tools: Vec::new(),
        text: Some(CodexTextFormat::JsonObject {}),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("json_object"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. CodexResponse serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_response_minimal_serde() {
    let resp = make_response(vec![assistant_text("ok")]);
    let json = serde_json::to_string(&resp).unwrap();
    let back: CodexResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "resp_test");
    assert_eq!(back.output.len(), 1);
}

#[test]
fn codex_response_with_usage() {
    let resp = CodexResponse {
        id: "r1".into(),
        model: "m".into(),
        output: vec![],
        usage: Some(CodexUsage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
        }),
        status: Some("completed".into()),
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("completed"));
    assert!(json.contains("15"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. ReasoningSummary
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn reasoning_summary_eq() {
    let a = ReasoningSummary {
        text: "hello".into(),
    };
    let b = ReasoningSummary {
        text: "hello".into(),
    };
    assert_eq!(a, b);
}

#[test]
fn reasoning_summary_serde() {
    let s = ReasoningSummary {
        text: "think".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: ReasoningSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Lowering: input_to_ir
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn input_to_ir_user_message() {
    let conv = lowering::input_to_ir(&[make_input("user", "hello")]);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "hello");
}

#[test]
fn input_to_ir_system_message() {
    let conv = lowering::input_to_ir(&[make_input("system", "instructions")]);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn input_to_ir_assistant_message() {
    let conv = lowering::input_to_ir(&[make_input("assistant", "response")]);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
}

#[test]
fn input_to_ir_unknown_role_becomes_user() {
    let conv = lowering::input_to_ir(&[make_input("developer", "hi")]);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn input_to_ir_empty_content_yields_empty_blocks() {
    let conv = lowering::input_to_ir(&[make_input("user", "")]);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn input_to_ir_empty_slice() {
    let conv = lowering::input_to_ir(&[]);
    assert!(conv.is_empty());
}

#[test]
fn input_to_ir_multiple_messages() {
    let items = vec![
        make_input("system", "sys"),
        make_input("user", "u1"),
        make_input("assistant", "a1"),
        make_input("user", "u2"),
    ];
    let conv = lowering::input_to_ir(&items);
    assert_eq!(conv.len(), 4);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
    assert_eq!(conv.messages[3].role, IrRole::User);
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Lowering: to_ir (response items)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn to_ir_message_item() {
    let conv = lowering::to_ir(&[assistant_text("hi")]);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    assert_eq!(conv.messages[0].text_content(), "hi");
}

#[test]
fn to_ir_function_call_item() {
    let conv = lowering::to_ir(&[function_call("fc_1", "shell", r#"{"cmd":"ls"}"#)]);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "fc_1");
            assert_eq!(name, "shell");
            assert_eq!(input, &json!({"cmd": "ls"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn to_ir_function_call_malformed_json() {
    let conv = lowering::to_ir(&[function_call("fc_x", "f", "not-json")]);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-json".into()));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn to_ir_function_call_output_item() {
    let conv = lowering::to_ir(&[function_call_output("fc_1", "result")]);
    assert_eq!(conv.messages[0].role, IrRole::Tool);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "fc_1");
            assert!(!is_error);
            assert_eq!(content.len(), 1);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn to_ir_reasoning_item() {
    let conv = lowering::to_ir(&[reasoning(&["step a", "step b"])]);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => {
            assert!(text.contains("step a"));
            assert!(text.contains("step b"));
            assert!(text.contains('\n'));
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn to_ir_reasoning_single_summary() {
    let conv = lowering::to_ir(&[reasoning(&["only one"])]);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => assert_eq!(text, "only one"),
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn to_ir_empty_items() {
    let conv = lowering::to_ir(&[]);
    assert!(conv.is_empty());
}

#[test]
fn to_ir_multi_content_message() {
    let items = vec![CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![
            CodexContentPart::OutputText {
                text: "part1".into(),
            },
            CodexContentPart::OutputText {
                text: "part2".into(),
            },
        ],
    }];
    let conv = lowering::to_ir(&items);
    assert_eq!(conv.messages[0].content.len(), 2);
    match (&conv.messages[0].content[0], &conv.messages[0].content[1]) {
        (IrContentBlock::Text { text: t1 }, IrContentBlock::Text { text: t2 }) => {
            assert_eq!(t1, "part1");
            assert_eq!(t2, "part2");
        }
        _ => panic!("expected two Text blocks"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. Lifting: from_ir
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn from_ir_assistant_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "hi")]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        CodexResponseItem::Message { role, content } => {
            assert_eq!(role, "assistant");
            assert_eq!(content.len(), 1);
            match &content[0] {
                CodexContentPart::OutputText { text } => assert_eq!(text, "hi"),
            }
        }
        _ => panic!("expected Message"),
    }
}

#[test]
fn from_ir_skips_system_messages() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::Assistant, "out"),
    ]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
}

#[test]
fn from_ir_skips_user_messages() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::text(IrRole::Assistant, "hi"),
    ]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
}

#[test]
fn from_ir_tool_use_block_becomes_function_call() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "shell".into(),
            input: json!({"cmd": "pwd"}),
        }],
    )]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        CodexResponseItem::FunctionCall {
            id,
            name,
            arguments,
            ..
        } => {
            assert_eq!(id, "t1");
            assert_eq!(name, "shell");
            assert!(arguments.contains("pwd"));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn from_ir_thinking_block_becomes_reasoning() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "thinking hard".into(),
        }],
    )]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        CodexResponseItem::Reasoning { summary } => {
            assert_eq!(summary.len(), 1);
            assert_eq!(summary[0].text, "thinking hard");
        }
        other => panic!("expected Reasoning, got {other:?}"),
    }
}

#[test]
fn from_ir_tool_result_becomes_function_call_output() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "fc_1".into(),
            content: vec![IrContentBlock::Text {
                text: "output data".into(),
            }],
            is_error: false,
        }],
    )]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "fc_1");
            assert_eq!(output, "output data");
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

#[test]
fn from_ir_empty_conversation() {
    let conv = IrConversation::new();
    let items = lowering::from_ir(&conv);
    assert!(items.is_empty());
}

#[test]
fn from_ir_text_then_tool_use_flushes() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Looking...".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: json!({}),
            },
        ],
    )]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 2);
    assert!(matches!(&items[0], CodexResponseItem::Message { .. }));
    assert!(matches!(&items[1], CodexResponseItem::FunctionCall { .. }));
}

#[test]
fn from_ir_text_then_thinking_flushes() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text { text: "hmm".into() },
            IrContentBlock::Thinking {
                text: "deep thought".into(),
            },
        ],
    )]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 2);
    assert!(matches!(&items[0], CodexResponseItem::Message { .. }));
    assert!(matches!(&items[1], CodexResponseItem::Reasoning { .. }));
}

#[test]
fn from_ir_ignores_image_blocks() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "base64data".into(),
        }],
    )]);
    let items = lowering::from_ir(&conv);
    assert!(items.is_empty());
}

#[test]
fn from_ir_multiple_tool_results() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![
            IrContentBlock::ToolResult {
                tool_use_id: "fc_1".into(),
                content: vec![IrContentBlock::Text { text: "r1".into() }],
                is_error: false,
            },
            IrContentBlock::ToolResult {
                tool_use_id: "fc_2".into(),
                content: vec![IrContentBlock::Text { text: "r2".into() }],
                is_error: true,
            },
        ],
    )]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 2);
    match &items[0] {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "fc_1");
            assert_eq!(output, "r1");
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
    match &items[1] {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "fc_2");
            assert_eq!(output, "r2");
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. Roundtrips: to_ir + from_ir
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_assistant_message() {
    let items = vec![assistant_text("Hello, world!")];
    let back = lowering::from_ir(&lowering::to_ir(&items));
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::Message { content, .. } => {
            assert_eq!(content.len(), 1);
            match &content[0] {
                CodexContentPart::OutputText { text } => assert_eq!(text, "Hello, world!"),
            }
        }
        _ => panic!("expected Message"),
    }
}

#[test]
fn roundtrip_function_call() {
    let items = vec![function_call(
        "fc_10",
        "write",
        r#"{"path":"f.rs","data":"x"}"#,
    )];
    let back = lowering::from_ir(&lowering::to_ir(&items));
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::FunctionCall { id, name, .. } => {
            assert_eq!(id, "fc_10");
            assert_eq!(name, "write");
        }
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn roundtrip_function_call_output() {
    let items = vec![function_call_output("fc_99", "done")];
    let back = lowering::from_ir(&lowering::to_ir(&items));
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "fc_99");
            assert_eq!(output, "done");
        }
        _ => panic!("expected FunctionCallOutput"),
    }
}

#[test]
fn roundtrip_reasoning() {
    let items = vec![reasoning(&["thought"])];
    let back = lowering::from_ir(&lowering::to_ir(&items));
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::Reasoning { summary } => {
            assert_eq!(summary[0].text, "thought");
        }
        _ => panic!("expected Reasoning"),
    }
}

#[test]
fn roundtrip_multi_item_conversation() {
    let items = vec![
        assistant_text("Let me check."),
        function_call("fc_1", "read", r#"{"path":"x"}"#),
        function_call_output("fc_1", "data"),
        assistant_text("Got it."),
    ];
    let back = lowering::from_ir(&lowering::to_ir(&items));
    assert_eq!(back.len(), 4);
    assert!(matches!(&back[0], CodexResponseItem::Message { .. }));
    assert!(matches!(&back[1], CodexResponseItem::FunctionCall { .. }));
    assert!(matches!(
        &back[2],
        CodexResponseItem::FunctionCallOutput { .. }
    ));
    assert!(matches!(&back[3], CodexResponseItem::Message { .. }));
}

#[test]
fn roundtrip_empty() {
    let back = lowering::from_ir(&lowering::to_ir(&[]));
    assert!(back.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Usage lowering
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn usage_to_ir_basic() {
    let u = CodexUsage {
        input_tokens: 200,
        output_tokens: 100,
        total_tokens: 300,
    };
    let ir = lowering::usage_to_ir(&u);
    assert_eq!(ir.input_tokens, 200);
    assert_eq!(ir.output_tokens, 100);
    assert_eq!(ir.total_tokens, 300);
}

#[test]
fn usage_to_ir_zero() {
    let u = CodexUsage {
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
    };
    let ir = lowering::usage_to_ir(&u);
    assert_eq!(ir.total_tokens, 0);
}

#[test]
fn usage_to_ir_large_values() {
    let u = CodexUsage {
        input_tokens: u64::MAX / 2,
        output_tokens: u64::MAX / 2,
        total_tokens: u64::MAX,
    };
    let ir = lowering::usage_to_ir(&u);
    assert_eq!(ir.input_tokens, u64::MAX / 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. WorkOrder mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_work_order_basic() {
    let wo = WorkOrderBuilder::new("Fix the bug").build();
    let cfg = CodexConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "codex-mini-latest");
    assert_eq!(req.input.len(), 1);
    match &req.input[0] {
        CodexInputItem::Message { role, content } => {
            assert_eq!(role, "user");
            assert!(content.contains("Fix the bug"));
        }
    }
}

#[test]
fn map_work_order_model_override() {
    let wo = WorkOrderBuilder::new("task").model("o4-mini").build();
    let cfg = CodexConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "o4-mini");
}

#[test]
fn map_work_order_uses_config_model_when_no_override() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CodexConfig {
        model: "gpt-4".into(),
        ..CodexConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4");
}

#[test]
fn map_work_order_max_output_tokens_from_config() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CodexConfig {
        max_output_tokens: Some(2048),
        ..CodexConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.max_output_tokens, Some(2048));
}

#[test]
fn map_work_order_temperature_from_config() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CodexConfig {
        temperature: Some(0.7),
        ..CodexConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.temperature, Some(0.7));
}

#[test]
fn map_work_order_tools_empty() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CodexConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.tools.is_empty());
}

#[test]
fn map_work_order_text_is_none() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CodexConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.text.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. Response mapping to AgentEvents
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_response_assistant_message_event() {
    let resp = make_response(vec![assistant_text("Done!")]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { text } if text == "Done!"
    ));
}

#[test]
fn map_response_function_call_event() {
    let resp = make_response(vec![function_call("fc_1", "shell", r#"{"cmd":"ls"}"#)]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "shell");
            assert_eq!(tool_use_id.as_deref(), Some("fc_1"));
            assert_eq!(input, &json!({"cmd": "ls"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn map_response_function_call_output_event() {
    let resp = make_response(vec![function_call_output("fc_1", "result")]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolResult {
            tool_name,
            tool_use_id,
            output,
            is_error,
        } => {
            assert_eq!(tool_name, "function");
            assert_eq!(tool_use_id.as_deref(), Some("fc_1"));
            assert_eq!(output, &serde_json::Value::String("result".into()));
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn map_response_reasoning_event() {
    let resp = make_response(vec![reasoning(&["thinking about it"])]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { text } if text.contains("thinking about it")
    ));
}

#[test]
fn map_response_empty_reasoning_no_event() {
    let resp = make_response(vec![CodexResponseItem::Reasoning { summary: vec![] }]);
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_multi_items() {
    let resp = make_response(vec![
        assistant_text("msg1"),
        function_call("fc_1", "shell", r#"{}"#),
        function_call_output("fc_1", "out"),
        assistant_text("msg2"),
    ]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 4);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
    assert!(matches!(&events[2].kind, AgentEventKind::ToolResult { .. }));
    assert!(matches!(
        &events[3].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[test]
fn map_response_empty() {
    let resp = make_response(vec![]);
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_malformed_function_args() {
    let resp = make_response(vec![function_call("fc_1", "f", "bad-json")]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("bad-json".into()));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn map_response_multi_content_parts() {
    let resp = make_response(vec![CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![
            CodexContentPart::OutputText {
                text: "part1".into(),
            },
            CodexContentPart::OutputText {
                text: "part2".into(),
            },
        ],
    }]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { text } if text == "part1"
    ));
    assert!(matches!(
        &events[1].kind,
        AgentEventKind::AssistantMessage { text } if text == "part2"
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 21. Stream event mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_response_created_maps_to_run_started() {
    let ev = CodexStreamEvent::ResponseCreated {
        response: make_response(vec![]),
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn stream_response_in_progress_produces_no_events() {
    let ev = CodexStreamEvent::ResponseInProgress {
        response: make_response(vec![]),
    };
    let events = dialect::map_stream_event(&ev);
    assert!(events.is_empty());
}

#[test]
fn stream_output_item_added_message() {
    let ev = CodexStreamEvent::OutputItemAdded {
        output_index: 0,
        item: assistant_text("hello"),
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { text } if text == "hello"
    ));
}

#[test]
fn stream_output_item_added_function_call() {
    let ev = CodexStreamEvent::OutputItemAdded {
        output_index: 0,
        item: function_call("fc_1", "shell", r#"{"a":"b"}"#),
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn stream_output_item_delta_text() {
    let ev = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::OutputTextDelta {
            text: "fragment".into(),
        },
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "fragment"
    ));
}

#[test]
fn stream_output_item_delta_function_args_no_event() {
    let ev = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::FunctionCallArgumentsDelta {
            delta: r#"{"pa"#.into(),
        },
    };
    let events = dialect::map_stream_event(&ev);
    assert!(events.is_empty());
}

#[test]
fn stream_output_item_delta_reasoning_no_event() {
    let ev = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::ReasoningSummaryDelta {
            text: "partial".into(),
        },
    };
    let events = dialect::map_stream_event(&ev);
    assert!(events.is_empty());
}

#[test]
fn stream_output_item_done_message() {
    let ev = CodexStreamEvent::OutputItemDone {
        output_index: 0,
        item: assistant_text("final"),
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { text } if text == "final"
    ));
}

#[test]
fn stream_response_completed_maps_to_run_completed() {
    let ev = CodexStreamEvent::ResponseCompleted {
        response: make_response(vec![]),
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn stream_response_failed_maps_to_error() {
    let ev = CodexStreamEvent::ResponseFailed {
        response: CodexResponse {
            id: "r".into(),
            model: "m".into(),
            output: vec![],
            usage: None,
            status: Some("rate_limit".into()),
        },
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => assert_eq!(message, "rate_limit"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn stream_response_failed_no_status() {
    let ev = CodexStreamEvent::ResponseFailed {
        response: make_response(vec![]),
    };
    let events = dialect::map_stream_event(&ev);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => assert_eq!(message, "unknown failure"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn stream_error_event() {
    let ev = CodexStreamEvent::Error {
        message: "bad request".into(),
        code: Some("400".into()),
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => assert_eq!(message, "bad request"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn stream_error_event_no_code() {
    let ev = CodexStreamEvent::Error {
        message: "fail".into(),
        code: None,
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 22. Stream delta serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_delta_output_text_serde() {
    let d = CodexStreamDelta::OutputTextDelta { text: "hi".into() };
    let json = serde_json::to_string(&d).unwrap();
    assert!(json.contains("output_text_delta"));
    let back: CodexStreamDelta = serde_json::from_str(&json).unwrap();
    match back {
        CodexStreamDelta::OutputTextDelta { text } => assert_eq!(text, "hi"),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn stream_delta_function_args_serde() {
    let d = CodexStreamDelta::FunctionCallArgumentsDelta {
        delta: r#"{"x":"#.into(),
    };
    let json = serde_json::to_string(&d).unwrap();
    assert!(json.contains("function_call_arguments_delta"));
}

#[test]
fn stream_delta_reasoning_serde() {
    let d = CodexStreamDelta::ReasoningSummaryDelta {
        text: "step".into(),
    };
    let json = serde_json::to_string(&d).unwrap();
    assert!(json.contains("reasoning_summary_delta"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 23. Stream event serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_event_response_created_serde() {
    let ev = CodexStreamEvent::ResponseCreated {
        response: make_response(vec![]),
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("response_created"));
    let _back: CodexStreamEvent = serde_json::from_str(&json).unwrap();
}

#[test]
fn stream_event_response_completed_serde() {
    let ev = CodexStreamEvent::ResponseCompleted {
        response: make_response(vec![]),
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("response_completed"));
}

#[test]
fn stream_event_error_serde() {
    let ev = CodexStreamEvent::Error {
        message: "err".into(),
        code: Some("500".into()),
    };
    let json = serde_json::to_string(&ev).unwrap();
    let back: CodexStreamEvent = serde_json::from_str(&json).unwrap();
    match back {
        CodexStreamEvent::Error { message, code } => {
            assert_eq!(message, "err");
            assert_eq!(code.as_deref(), Some("500"));
        }
        _ => panic!("wrong variant"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 24. CodexTool serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_tool_function_serde() {
    let t = CodexTool::Function {
        function: CodexFunctionDef {
            name: "write".into(),
            description: "Write file".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let json = serde_json::to_string(&t).unwrap();
    assert!(json.contains(r#""type":"function"#));
    let back: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back, t);
}

#[test]
fn codex_tool_code_interpreter_serde() {
    let t = CodexTool::CodeInterpreter {};
    let json = serde_json::to_string(&t).unwrap();
    assert!(json.contains("code_interpreter"));
    let back: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back, t);
}

#[test]
fn codex_tool_file_search_serde_with_max() {
    let t = CodexTool::FileSearch {
        max_num_results: Some(5),
    };
    let json = serde_json::to_string(&t).unwrap();
    assert!(json.contains("file_search"));
    assert!(json.contains("5"));
    let back: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back, t);
}

#[test]
fn codex_tool_file_search_serde_without_max() {
    let t = CodexTool::FileSearch {
        max_num_results: None,
    };
    let json = serde_json::to_string(&t).unwrap();
    assert!(!json.contains("max_num_results"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 25. Edge cases and special values
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unicode_content_roundtrip_through_ir() {
    let items = vec![assistant_text("こんにちは 🌍 émojis")];
    let back = lowering::from_ir(&lowering::to_ir(&items));
    match &back[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => {
                assert_eq!(text, "こんにちは 🌍 émojis");
            }
        },
        _ => panic!("expected Message"),
    }
}

#[test]
fn newlines_in_content_preserved() {
    let items = vec![assistant_text("line1\nline2\nline3")];
    let back = lowering::from_ir(&lowering::to_ir(&items));
    match &back[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => assert!(text.contains('\n')),
        },
        _ => panic!("expected Message"),
    }
}

#[test]
fn empty_string_function_call_output() {
    let items = vec![function_call_output("fc_1", "")];
    let back = lowering::from_ir(&lowering::to_ir(&items));
    match &back[0] {
        CodexResponseItem::FunctionCallOutput { output, .. } => assert!(output.is_empty()),
        _ => panic!("expected FunctionCallOutput"),
    }
}

#[test]
fn function_call_with_complex_json_args() {
    let args = json!({"nested": {"array": [1, 2, 3], "bool": true, "null_val": null}});
    let args_str = serde_json::to_string(&args).unwrap();
    let items = vec![function_call("fc_1", "complex", &args_str)];
    let conv = lowering::to_ir(&items);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &args);
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn reasoning_empty_summary_vec() {
    let items = vec![CodexResponseItem::Reasoning { summary: vec![] }];
    let conv = lowering::to_ir(&items);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => assert!(text.is_empty()),
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn long_reasoning_multiple_summaries_joined_with_newlines() {
    let items = vec![reasoning(&["A", "B", "C", "D"])];
    let conv = lowering::to_ir(&items);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => {
            assert_eq!(text, "A\nB\nC\nD");
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn dialect_version_constant() {
    assert_eq!(dialect::DIALECT_VERSION, "codex/v0.1");
}

#[test]
fn default_model_constant() {
    assert_eq!(dialect::DEFAULT_MODEL, "codex-mini-latest");
}

#[test]
fn codex_output_item_is_alias_for_response_item() {
    let item: dialect::CodexOutputItem = assistant_text("alias test");
    match item {
        CodexResponseItem::Message { content, .. } => {
            assert_eq!(content.len(), 1);
        }
        _ => panic!("type alias broken"),
    }
}

#[test]
fn stream_output_item_added_function_call_output() {
    let ev = CodexStreamEvent::OutputItemAdded {
        output_index: 1,
        item: function_call_output("fc_1", "result"),
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn stream_output_item_done_reasoning() {
    let ev = CodexStreamEvent::OutputItemDone {
        output_index: 0,
        item: reasoning(&["final reasoning"]),
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
}

#[test]
fn stream_output_item_done_empty_reasoning_no_event() {
    let ev = CodexStreamEvent::OutputItemDone {
        output_index: 0,
        item: CodexResponseItem::Reasoning { summary: vec![] },
    };
    let events = dialect::map_stream_event(&ev);
    assert!(events.is_empty());
}

#[test]
fn stream_output_item_added_empty_message_no_event() {
    let ev = CodexStreamEvent::OutputItemAdded {
        output_index: 0,
        item: CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![],
        },
    };
    let events = dialect::map_stream_event(&ev);
    assert!(events.is_empty());
}

#[test]
fn from_ir_tool_result_concatenates_multiple_text_blocks() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "fc_1".into(),
            content: vec![
                IrContentBlock::Text {
                    text: "first".into(),
                },
                IrContentBlock::Text {
                    text: "second".into(),
                },
            ],
            is_error: false,
        }],
    )]);
    let items = lowering::from_ir(&conv);
    match &items[0] {
        CodexResponseItem::FunctionCallOutput { output, .. } => {
            assert_eq!(output, "firstsecond");
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

#[test]
fn from_ir_tool_result_ignores_non_text_content() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "fc_1".into(),
            content: vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64".into(),
            }],
            is_error: false,
        }],
    )]);
    let items = lowering::from_ir(&conv);
    match &items[0] {
        CodexResponseItem::FunctionCallOutput { output, .. } => {
            assert!(output.is_empty());
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

#[test]
fn from_ir_tool_msg_non_tool_result_blocks_ignored() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::Text {
            text: "stray text".into(),
        }],
    )]);
    let items = lowering::from_ir(&conv);
    assert!(items.is_empty());
}

#[test]
fn from_ir_assistant_trailing_text_flushed() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "f".into(),
                input: json!({}),
            },
            IrContentBlock::Text {
                text: "after tool".into(),
            },
        ],
    )]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 2);
    assert!(matches!(&items[0], CodexResponseItem::FunctionCall { .. }));
    match &items[1] {
        CodexResponseItem::Message { content, .. } => {
            assert_eq!(content.len(), 1);
            match &content[0] {
                CodexContentPart::OutputText { text } => assert_eq!(text, "after tool"),
            }
        }
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn from_ir_assistant_empty_content() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(IrRole::Assistant, vec![])]);
    let items = lowering::from_ir(&conv);
    assert!(items.is_empty());
}
