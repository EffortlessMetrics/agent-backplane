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
//! Comprehensive deep tests for GitHub Copilot SDK dialect types and lowering.
//!
//! Covers all Copilot message roles, reference types, tool definitions,
//! request/response construction, streaming events, Copilot→IR lowering,
//! IR→Copilot lifting, serde roundtrips, passthrough fidelity, configuration,
//! WorkOrder mapping, and edge cases.

use abp_copilot_sdk::dialect::{
    self, CanonicalToolDef, CopilotConfig, CopilotConfirmation, CopilotError, CopilotFunctionCall,
    CopilotFunctionDef, CopilotMessage, CopilotReference, CopilotReferenceType, CopilotRequest,
    CopilotResponse, CopilotStreamEvent, CopilotTool, CopilotToolType, CopilotTurnEntry,
};
use abp_copilot_sdk::lowering;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEventKind, ContextPacket, ContextSnippet, WorkOrderBuilder};
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn copilot_msg(role: &str, content: &str) -> CopilotMessage {
    CopilotMessage {
        role: role.into(),
        content: content.into(),
        name: None,
        copilot_references: Vec::new(),
    }
}

fn copilot_msg_with_refs(role: &str, content: &str, refs: Vec<CopilotReference>) -> CopilotMessage {
    CopilotMessage {
        role: role.into(),
        content: content.into(),
        name: None,
        copilot_references: refs,
    }
}

fn file_ref(id: &str, path: &str) -> CopilotReference {
    CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: id.into(),
        data: json!({ "path": path }),
        metadata: None,
    }
}

fn snippet_ref(id: &str, name: &str, content: &str) -> CopilotReference {
    CopilotReference {
        ref_type: CopilotReferenceType::Snippet,
        id: id.into(),
        data: json!({ "name": name, "content": content }),
        metadata: None,
    }
}

fn repo_ref(id: &str, owner: &str, name: &str) -> CopilotReference {
    CopilotReference {
        ref_type: CopilotReferenceType::Repository,
        id: id.into(),
        data: json!({ "owner": owner, "name": name }),
        metadata: None,
    }
}

fn web_ref(id: &str, url: &str) -> CopilotReference {
    CopilotReference {
        ref_type: CopilotReferenceType::WebSearchResult,
        id: id.into(),
        data: json!({ "url": url }),
        metadata: None,
    }
}

fn simple_response(message: &str) -> CopilotResponse {
    CopilotResponse {
        message: message.into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    }
}

fn make_canonical_tool(name: &str, desc: &str) -> CanonicalToolDef {
    CanonicalToolDef {
        name: name.into(),
        description: desc.into(),
        parameters_schema: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            }
        }),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Dialect constants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_version_is_copilot_v01() {
    assert_eq!(dialect::DIALECT_VERSION, "copilot/v0.1");
}

#[test]
fn default_model_is_gpt4o() {
    assert_eq!(dialect::DEFAULT_MODEL, "gpt-4o");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Model name mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn to_canonical_model_adds_copilot_prefix() {
    assert_eq!(dialect::to_canonical_model("gpt-4o"), "copilot/gpt-4o");
}

#[test]
fn from_canonical_model_strips_copilot_prefix() {
    assert_eq!(dialect::from_canonical_model("copilot/gpt-4o"), "gpt-4o");
}

#[test]
fn from_canonical_model_leaves_non_prefixed_unchanged() {
    assert_eq!(dialect::from_canonical_model("gpt-4o"), "gpt-4o");
}

#[test]
fn canonical_model_roundtrip() {
    let original = "gpt-4-turbo";
    let canonical = dialect::to_canonical_model(original);
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, original);
}

#[test]
fn is_known_model_for_all_listed_models() {
    let known = &[
        "gpt-4o",
        "gpt-4o-mini",
        "gpt-4-turbo",
        "gpt-4",
        "o1",
        "o1-mini",
        "o3-mini",
        "claude-sonnet-4",
        "claude-3.5-sonnet",
    ];
    for model in known {
        assert!(dialect::is_known_model(model), "{model} should be known");
    }
}

#[test]
fn is_known_model_returns_false_for_unknown() {
    assert!(!dialect::is_known_model("llama-3"));
    assert!(!dialect::is_known_model(""));
    assert!(!dialect::is_known_model("copilot/gpt-4o"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Capability manifest
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
fn capability_manifest_has_tool_read_emulated() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn capability_manifest_has_tool_glob_unsupported() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolGlob),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn capability_manifest_has_web_search_native() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolWebSearch),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_mcp_unsupported() {
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
fn capability_manifest_is_nonempty() {
    let m = dialect::capability_manifest();
    assert!(m.len() >= 10, "expected at least 10 capabilities");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. CopilotConfig
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn default_config_has_empty_token() {
    let cfg = CopilotConfig::default();
    assert!(cfg.token.is_empty());
}

#[test]
fn default_config_base_url_points_to_github() {
    let cfg = CopilotConfig::default();
    assert_eq!(cfg.base_url, "https://api.githubcopilot.com");
}

#[test]
fn default_config_model_is_gpt4o() {
    let cfg = CopilotConfig::default();
    assert_eq!(cfg.model, "gpt-4o");
}

#[test]
fn default_config_system_prompt_is_none() {
    let cfg = CopilotConfig::default();
    assert!(cfg.system_prompt.is_none());
}

#[test]
fn config_serde_roundtrip() {
    let cfg = CopilotConfig {
        token: "ghp_test_token".into(),
        base_url: "https://custom.api.com".into(),
        model: "o1-mini".into(),
        system_prompt: Some("Be helpful.".into()),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: CopilotConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.token, "ghp_test_token");
    assert_eq!(back.base_url, "https://custom.api.com");
    assert_eq!(back.model, "o1-mini");
    assert_eq!(back.system_prompt.as_deref(), Some("Be helpful."));
}

#[test]
fn config_without_system_prompt_serde() {
    let cfg = CopilotConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: CopilotConfig = serde_json::from_str(&json).unwrap();
    assert!(back.system_prompt.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Reference types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn reference_type_file_serde() {
    let r = file_ref("f-0", "src/main.rs");
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["type"], "file");
    let back: CopilotReference = serde_json::from_value(json).unwrap();
    assert_eq!(back.ref_type, CopilotReferenceType::File);
}

#[test]
fn reference_type_snippet_serde() {
    let r = snippet_ref("s-0", "helper", "fn foo() {}");
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["type"], "snippet");
}

#[test]
fn reference_type_repository_serde() {
    let r = repo_ref("r-0", "octocat", "hello-world");
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["type"], "repository");
}

#[test]
fn reference_type_web_search_result_serde() {
    let r = web_ref("w-0", "https://example.com");
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["type"], "web_search_result");
}

#[test]
fn reference_with_metadata_serde_roundtrip() {
    let mut meta = BTreeMap::new();
    meta.insert("label".into(), json!("my-label"));
    meta.insert("uri".into(), json!("https://example.com"));
    let r = CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "f-m".into(),
        data: json!({"path": "a.rs"}),
        metadata: Some(meta),
    };
    let json = serde_json::to_value(&r).unwrap();
    let back: CopilotReference = serde_json::from_value(json).unwrap();
    assert_eq!(back.metadata.as_ref().unwrap().len(), 2);
    assert_eq!(back.metadata.as_ref().unwrap()["label"], json!("my-label"));
}

#[test]
fn reference_without_metadata_omits_field() {
    let r = file_ref("f-0", "x.rs");
    let json = serde_json::to_value(&r).unwrap();
    assert!(json.get("metadata").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Tool types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_def_to_copilot_produces_function_type() {
    let def = make_canonical_tool("read_file", "Read a file");
    let tool = dialect::tool_def_to_copilot(&def);
    assert_eq!(tool.tool_type, CopilotToolType::Function);
    assert!(tool.function.is_some());
    assert!(tool.confirmation.is_none());
}

#[test]
fn tool_def_to_copilot_preserves_name_and_description() {
    let def = make_canonical_tool("write_file", "Write to a file");
    let tool = dialect::tool_def_to_copilot(&def);
    let func = tool.function.as_ref().unwrap();
    assert_eq!(func.name, "write_file");
    assert_eq!(func.description, "Write to a file");
}

#[test]
fn tool_def_to_copilot_preserves_parameters_schema() {
    let def = make_canonical_tool("bash", "Run command");
    let tool = dialect::tool_def_to_copilot(&def);
    let func = tool.function.as_ref().unwrap();
    assert!(func.parameters.get("properties").is_some());
}

#[test]
fn tool_def_from_copilot_roundtrip() {
    let def = make_canonical_tool("glob", "Glob files");
    let tool = dialect::tool_def_to_copilot(&def);
    let back = dialect::tool_def_from_copilot(&tool).unwrap();
    assert_eq!(back.name, def.name);
    assert_eq!(back.description, def.description);
    assert_eq!(back.parameters_schema, def.parameters_schema);
}

#[test]
fn tool_def_from_copilot_returns_none_for_confirmation_tool() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Confirmation,
        function: None,
        confirmation: Some(CopilotConfirmation {
            id: "c1".into(),
            title: "Delete files?".into(),
            message: "This will delete all temp files.".into(),
            accepted: None,
        }),
    };
    assert!(dialect::tool_def_from_copilot(&tool).is_none());
}

#[test]
fn tool_def_from_copilot_returns_none_for_missing_function() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Function,
        function: None,
        confirmation: None,
    };
    assert!(dialect::tool_def_from_copilot(&tool).is_none());
}

#[test]
fn copilot_tool_type_serde() {
    let ft: CopilotToolType = serde_json::from_str(r#""function""#).unwrap();
    assert_eq!(ft, CopilotToolType::Function);
    let ct: CopilotToolType = serde_json::from_str(r#""confirmation""#).unwrap();
    assert_eq!(ct, CopilotToolType::Confirmation);
}

#[test]
fn copilot_tool_serde_roundtrip() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Function,
        function: Some(CopilotFunctionDef {
            name: "search".into(),
            description: "Search code".into(),
            parameters: json!({"type": "object"}),
        }),
        confirmation: None,
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: CopilotTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back.tool_type, CopilotToolType::Function);
    assert_eq!(back.function.as_ref().unwrap().name, "search");
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. CopilotConfirmation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn confirmation_serde_roundtrip() {
    let conf = CopilotConfirmation {
        id: "conf-1".into(),
        title: "Approve deploy".into(),
        message: "Deploy to production?".into(),
        accepted: Some(true),
    };
    let json = serde_json::to_value(&conf).unwrap();
    let back: CopilotConfirmation = serde_json::from_value(json).unwrap();
    assert_eq!(back.id, "conf-1");
    assert_eq!(back.accepted, Some(true));
}

#[test]
fn confirmation_accepted_none_omitted() {
    let conf = CopilotConfirmation {
        id: "c".into(),
        title: "T".into(),
        message: "M".into(),
        accepted: None,
    };
    let json = serde_json::to_value(&conf).unwrap();
    assert!(json.get("accepted").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. CopilotError
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_error_serde_roundtrip() {
    let err = CopilotError {
        error_type: "rate_limit".into(),
        message: "Rate limited".into(),
        code: Some("429".into()),
        identifier: Some("err-123".into()),
    };
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["type"], "rate_limit");
    let back: CopilotError = serde_json::from_value(json).unwrap();
    assert_eq!(back.error_type, "rate_limit");
    assert_eq!(back.code.as_deref(), Some("429"));
}

#[test]
fn copilot_error_optional_fields_omitted() {
    let err = CopilotError {
        error_type: "internal".into(),
        message: "Something broke".into(),
        code: None,
        identifier: None,
    };
    let json = serde_json::to_value(&err).unwrap();
    assert!(json.get("code").is_none());
    assert!(json.get("identifier").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. CopilotFunctionCall
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn function_call_serde_roundtrip() {
    let fc = CopilotFunctionCall {
        name: "read_file".into(),
        arguments: r#"{"path":"a.rs"}"#.into(),
        id: Some("call-abc".into()),
    };
    let json = serde_json::to_value(&fc).unwrap();
    let back: CopilotFunctionCall = serde_json::from_value(json).unwrap();
    assert_eq!(back.name, "read_file");
    assert_eq!(back.id.as_deref(), Some("call-abc"));
}

#[test]
fn function_call_without_id() {
    let fc = CopilotFunctionCall {
        name: "bash".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
        id: None,
    };
    let json = serde_json::to_value(&fc).unwrap();
    assert!(json.get("id").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. CopilotMessage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_message_serde_roundtrip() {
    let msg = CopilotMessage {
        role: "user".into(),
        content: "Hello".into(),
        name: Some("alice".into()),
        copilot_references: vec![file_ref("f-0", "main.rs")],
    };
    let json = serde_json::to_value(&msg).unwrap();
    let back: CopilotMessage = serde_json::from_value(json).unwrap();
    assert_eq!(back.role, "user");
    assert_eq!(back.name.as_deref(), Some("alice"));
    assert_eq!(back.copilot_references.len(), 1);
}

#[test]
fn copilot_message_empty_refs_omitted() {
    let msg = copilot_msg("user", "hi");
    let json = serde_json::to_value(&msg).unwrap();
    assert!(json.get("copilot_references").is_none());
}

#[test]
fn copilot_message_name_none_omitted() {
    let msg = copilot_msg("user", "hi");
    let json = serde_json::to_value(&msg).unwrap();
    assert!(json.get("name").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. CopilotRequest
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_request_serde_roundtrip() {
    let req = CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![copilot_msg("user", "explain this code")],
        tools: Some(vec![dialect::tool_def_to_copilot(&make_canonical_tool(
            "read_file",
            "Read a file",
        ))]),
        turn_history: vec![CopilotTurnEntry {
            request: "prev question".into(),
            response: "prev answer".into(),
        }],
        references: vec![file_ref("f-0", "lib.rs")],
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CopilotRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "gpt-4o");
    assert_eq!(back.messages.len(), 1);
    assert_eq!(back.tools.as_ref().unwrap().len(), 1);
    assert_eq!(back.turn_history.len(), 1);
    assert_eq!(back.references.len(), 1);
}

#[test]
fn copilot_request_empty_optionals_omitted() {
    let req = CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![copilot_msg("user", "hi")],
        tools: None,
        turn_history: vec![],
        references: vec![],
    };
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("tools").is_none());
    assert!(json.get("turn_history").is_none());
    assert!(json.get("references").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. CopilotTurnEntry
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn turn_entry_serde_roundtrip() {
    let entry = CopilotTurnEntry {
        request: "What is Rust?".into(),
        response: "A systems language.".into(),
    };
    let json = serde_json::to_value(&entry).unwrap();
    let back: CopilotTurnEntry = serde_json::from_value(json).unwrap();
    assert_eq!(back.request, "What is Rust?");
    assert_eq!(back.response, "A systems language.");
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. CopilotResponse
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_response_serde_roundtrip() {
    let resp = CopilotResponse {
        message: "Here is the answer.".into(),
        copilot_references: vec![file_ref("f-0", "output.rs")],
        copilot_errors: vec![CopilotError {
            error_type: "warning".into(),
            message: "Truncated output".into(),
            code: None,
            identifier: None,
        }],
        copilot_confirmation: Some(CopilotConfirmation {
            id: "c-1".into(),
            title: "Proceed?".into(),
            message: "Continue with deployment?".into(),
            accepted: None,
        }),
        function_call: Some(CopilotFunctionCall {
            name: "deploy".into(),
            arguments: "{}".into(),
            id: Some("fc-1".into()),
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: CopilotResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.message, "Here is the answer.");
    assert_eq!(back.copilot_references.len(), 1);
    assert_eq!(back.copilot_errors.len(), 1);
    assert!(back.copilot_confirmation.is_some());
    assert!(back.function_call.is_some());
}

#[test]
fn copilot_response_empty_optional_fields_omitted() {
    let resp = simple_response("ok");
    let json = serde_json::to_value(&resp).unwrap();
    assert!(json.get("copilot_references").is_none());
    assert!(json.get("copilot_errors").is_none());
    assert!(json.get("copilot_confirmation").is_none());
    assert!(json.get("function_call").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. CopilotStreamEvent serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_event_text_delta_serde() {
    let ev = CopilotStreamEvent::TextDelta {
        text: "Hello".into(),
    };
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["type"], "text_delta");
    let back: CopilotStreamEvent = serde_json::from_value(json).unwrap();
    assert_eq!(back, ev);
}

#[test]
fn stream_event_function_call_serde() {
    let ev = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "search".into(),
            arguments: r#"{"q":"rust"}"#.into(),
            id: Some("fc-1".into()),
        },
    };
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["type"], "function_call");
    let back: CopilotStreamEvent = serde_json::from_value(json).unwrap();
    assert_eq!(back, ev);
}

#[test]
fn stream_event_copilot_references_serde() {
    let ev = CopilotStreamEvent::CopilotReferences {
        references: vec![file_ref("f-0", "lib.rs")],
    };
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["type"], "copilot_references");
    let back: CopilotStreamEvent = serde_json::from_value(json).unwrap();
    assert_eq!(back, ev);
}

#[test]
fn stream_event_copilot_errors_serde() {
    let ev = CopilotStreamEvent::CopilotErrors {
        errors: vec![CopilotError {
            error_type: "timeout".into(),
            message: "Request timed out".into(),
            code: None,
            identifier: None,
        }],
    };
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["type"], "copilot_errors");
}

#[test]
fn stream_event_copilot_confirmation_serde() {
    let ev = CopilotStreamEvent::CopilotConfirmation {
        confirmation: CopilotConfirmation {
            id: "c1".into(),
            title: "Delete?".into(),
            message: "Are you sure?".into(),
            accepted: None,
        },
    };
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["type"], "copilot_confirmation");
    let back: CopilotStreamEvent = serde_json::from_value(json).unwrap();
    assert_eq!(back, ev);
}

#[test]
fn stream_event_done_serde() {
    let ev = CopilotStreamEvent::Done {};
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["type"], "done");
    let back: CopilotStreamEvent = serde_json::from_value(json).unwrap();
    assert_eq!(back, ev);
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. map_work_order
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_work_order_uses_task_as_user_message() {
    let wo = WorkOrderBuilder::new("Explain closures").build();
    let cfg = CopilotConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert_eq!(req.messages[0].content, "Explain closures");
}

#[test]
fn map_work_order_defaults_to_config_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CopilotConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4o");
}

#[test]
fn map_work_order_respects_work_order_model() {
    let wo = WorkOrderBuilder::new("task").model("o1-mini").build();
    let cfg = CopilotConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "o1-mini");
}

#[test]
fn map_work_order_with_system_prompt() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CopilotConfig {
        system_prompt: Some("Be concise.".into()),
        ..CopilotConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, "system");
    assert_eq!(req.messages[0].content, "Be concise.");
    assert_eq!(req.messages[1].role, "user");
}

#[test]
fn map_work_order_maps_context_files_to_references() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into(), "src/lib.rs".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("review").context(ctx).build();
    let cfg = CopilotConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.references.len(), 2);
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::File);
    assert_eq!(req.references[0].id, "file-0");
    assert_eq!(req.references[1].id, "file-1");
}

#[test]
fn map_work_order_maps_context_snippets_to_references() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "helper".into(),
            content: "fn foo() {}".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("review").context(ctx).build();
    let cfg = CopilotConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.references.len(), 1);
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::Snippet);
    assert_eq!(req.references[0].id, "snippet-0");
}

#[test]
fn map_work_order_user_message_carries_references() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("check").context(ctx).build();
    let cfg = CopilotConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.last().unwrap().copilot_references.len(), 1);
}

#[test]
fn map_work_order_no_tools_by_default() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CopilotConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.tools.is_none());
}

#[test]
fn map_work_order_empty_turn_history() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CopilotConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.turn_history.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. map_response
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_response_assistant_message() {
    let events = dialect::map_response(&simple_response("Hello!"));
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hello!"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn map_response_empty_message_produces_no_event() {
    let events = dialect::map_response(&simple_response(""));
    assert!(events.is_empty());
}

#[test]
fn map_response_with_errors() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![
            CopilotError {
                error_type: "rate_limit".into(),
                message: "Too many requests".into(),
                code: None,
                identifier: None,
            },
            CopilotError {
                error_type: "internal".into(),
                message: "Server error".into(),
                code: None,
                identifier: None,
            },
        ],
        copilot_confirmation: None,
        function_call: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 2);
    for ev in &events {
        match &ev.kind {
            AgentEventKind::Error { message, .. } => {
                assert!(message.contains(": "));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}

#[test]
fn map_response_error_format_includes_type_and_message() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![CopilotError {
            error_type: "auth".into(),
            message: "Invalid token".into(),
            code: None,
            identifier: None,
        }],
        copilot_confirmation: None,
        function_call: None,
    };
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert_eq!(message, "auth: Invalid token");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn map_response_function_call_with_valid_json_args() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: Some(CopilotFunctionCall {
            name: "write_file".into(),
            arguments: r#"{"path":"out.txt","content":"hello"}"#.into(),
            id: Some("fc-1".into()),
        }),
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "write_file");
            assert_eq!(input["path"], "out.txt");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn map_response_function_call_with_invalid_json_falls_back_to_string() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: Some(CopilotFunctionCall {
            name: "bash".into(),
            arguments: "not valid json".into(),
            id: None,
        }),
    };
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input.as_str(), Some("not valid json"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn map_response_function_call_no_id() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: Some(CopilotFunctionCall {
            name: "read".into(),
            arguments: "{}".into(),
            id: None,
        }),
    };
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall { tool_use_id, .. } => {
            assert!(tool_use_id.is_none());
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn map_response_confirmation_produces_warning_with_ext() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: Some(CopilotConfirmation {
            id: "c1".into(),
            title: "Delete all?".into(),
            message: "Irreversible action".into(),
            accepted: None,
        }),
        function_call: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Warning { message } => {
            assert!(message.contains("Confirmation required"));
            assert!(message.contains("Delete all?"));
        }
        other => panic!("expected Warning, got {other:?}"),
    }
    assert!(events[0].ext.is_some());
    let ext = events[0].ext.as_ref().unwrap();
    assert!(ext.contains_key("copilot_confirmation"));
}

#[test]
fn map_response_all_fields_populated() {
    let resp = CopilotResponse {
        message: "output".into(),
        copilot_references: vec![],
        copilot_errors: vec![CopilotError {
            error_type: "warn".into(),
            message: "low memory".into(),
            code: None,
            identifier: None,
        }],
        copilot_confirmation: Some(CopilotConfirmation {
            id: "c".into(),
            title: "Proceed?".into(),
            message: "Continue?".into(),
            accepted: None,
        }),
        function_call: Some(CopilotFunctionCall {
            name: "deploy".into(),
            arguments: "{}".into(),
            id: Some("fc".into()),
        }),
    };
    let events = dialect::map_response(&resp);
    // message + 1 error + function_call + confirmation = 4
    assert_eq!(events.len(), 4);
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. map_stream_event
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_text_delta_maps_to_assistant_delta() {
    let ev = CopilotStreamEvent::TextDelta {
        text: "chunk".into(),
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "chunk"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn stream_function_call_maps_to_tool_call() {
    let ev = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "grep".into(),
            arguments: r#"{"pattern":"TODO"}"#.into(),
            id: Some("fc-2".into()),
        },
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "grep");
            assert_eq!(tool_use_id.as_deref(), Some("fc-2"));
            assert_eq!(input["pattern"], "TODO");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn stream_function_call_invalid_json_falls_back() {
    let ev = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "test".into(),
            arguments: "bad json".into(),
            id: None,
        },
    };
    let events = dialect::map_stream_event(&ev);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input.as_str(), Some("bad json"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn stream_confirmation_maps_to_warning() {
    let ev = CopilotStreamEvent::CopilotConfirmation {
        confirmation: CopilotConfirmation {
            id: "c1".into(),
            title: "Run tests?".into(),
            message: "Will run all tests.".into(),
            accepted: None,
        },
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Warning { message } => {
            assert!(message.contains("Run tests?"));
        }
        other => panic!("expected Warning, got {other:?}"),
    }
}

#[test]
fn stream_errors_maps_to_error_events() {
    let ev = CopilotStreamEvent::CopilotErrors {
        errors: vec![
            CopilotError {
                error_type: "timeout".into(),
                message: "Timed out".into(),
                code: None,
                identifier: None,
            },
            CopilotError {
                error_type: "auth".into(),
                message: "Bad token".into(),
                code: None,
                identifier: None,
            },
        ],
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 2);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => assert_eq!(message, "timeout: Timed out"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn stream_references_maps_to_run_started() {
    let ev = CopilotStreamEvent::CopilotReferences {
        references: vec![file_ref("f-0", "main.rs"), repo_ref("r-0", "org", "repo")],
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::RunStarted { message } => {
            assert!(message.contains("2 reference(s)"));
        }
        other => panic!("expected RunStarted, got {other:?}"),
    }
    let ext = events[0].ext.as_ref().unwrap();
    assert!(ext.contains_key("copilot_references"));
}

#[test]
fn stream_empty_references_produces_no_event() {
    let ev = CopilotStreamEvent::CopilotReferences { references: vec![] };
    let events = dialect::map_stream_event(&ev);
    assert!(events.is_empty());
}

#[test]
fn stream_done_maps_to_run_completed() {
    let ev = CopilotStreamEvent::Done {};
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::RunCompleted { message } => {
            assert!(message.contains("completed"));
        }
        other => panic!("expected RunCompleted, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Passthrough fidelity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_text_delta_roundtrip() {
    let ev = CopilotStreamEvent::TextDelta {
        text: "hello".into(),
    };
    let wrapped = dialect::to_passthrough_event(&ev);
    let back = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(back, ev);
}

#[test]
fn passthrough_function_call_roundtrip() {
    let ev = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "read".into(),
            arguments: r#"{"path":"a"}"#.into(),
            id: Some("fc-1".into()),
        },
    };
    let wrapped = dialect::to_passthrough_event(&ev);
    let back = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(back, ev);
}

#[test]
fn passthrough_confirmation_roundtrip() {
    let ev = CopilotStreamEvent::CopilotConfirmation {
        confirmation: CopilotConfirmation {
            id: "c".into(),
            title: "T".into(),
            message: "M".into(),
            accepted: Some(false),
        },
    };
    let wrapped = dialect::to_passthrough_event(&ev);
    let back = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(back, ev);
}

#[test]
fn passthrough_errors_roundtrip() {
    let ev = CopilotStreamEvent::CopilotErrors {
        errors: vec![CopilotError {
            error_type: "e".into(),
            message: "m".into(),
            code: Some("500".into()),
            identifier: Some("id-1".into()),
        }],
    };
    let wrapped = dialect::to_passthrough_event(&ev);
    let back = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(back, ev);
}

#[test]
fn passthrough_done_roundtrip() {
    let ev = CopilotStreamEvent::Done {};
    let wrapped = dialect::to_passthrough_event(&ev);
    let back = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(back, ev);
}

#[test]
fn passthrough_references_roundtrip() {
    let ev = CopilotStreamEvent::CopilotReferences {
        references: vec![file_ref("f-0", "main.rs")],
    };
    let wrapped = dialect::to_passthrough_event(&ev);
    let back = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(back, ev);
}

#[test]
fn passthrough_event_has_dialect_marker() {
    let ev = CopilotStreamEvent::Done {};
    let wrapped = dialect::to_passthrough_event(&ev);
    let ext = wrapped.ext.as_ref().unwrap();
    assert_eq!(ext["dialect"], "copilot");
}

#[test]
fn passthrough_event_has_raw_message() {
    let ev = CopilotStreamEvent::TextDelta { text: "raw".into() };
    let wrapped = dialect::to_passthrough_event(&ev);
    let ext = wrapped.ext.as_ref().unwrap();
    assert!(ext.contains_key("raw_message"));
}

#[test]
fn from_passthrough_returns_none_for_non_passthrough_event() {
    use abp_core::AgentEvent;
    use chrono::Utc;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "plain".into(),
        },
        ext: None,
    };
    assert!(dialect::from_passthrough_event(&event).is_none());
}

#[test]
fn verify_passthrough_fidelity_all_event_types() {
    let events = vec![
        CopilotStreamEvent::CopilotReferences {
            references: vec![file_ref("f-0", "a.rs")],
        },
        CopilotStreamEvent::TextDelta {
            text: "chunk1".into(),
        },
        CopilotStreamEvent::TextDelta {
            text: "chunk2".into(),
        },
        CopilotStreamEvent::FunctionCall {
            function_call: CopilotFunctionCall {
                name: "tool".into(),
                arguments: "{}".into(),
                id: Some("fc".into()),
            },
        },
        CopilotStreamEvent::CopilotConfirmation {
            confirmation: CopilotConfirmation {
                id: "c".into(),
                title: "T".into(),
                message: "M".into(),
                accepted: None,
            },
        },
        CopilotStreamEvent::CopilotErrors {
            errors: vec![CopilotError {
                error_type: "e".into(),
                message: "m".into(),
                code: None,
                identifier: None,
            }],
        },
        CopilotStreamEvent::Done {},
    ];
    assert!(dialect::verify_passthrough_fidelity(&events));
}

#[test]
fn verify_passthrough_fidelity_empty_list() {
    assert!(dialect::verify_passthrough_fidelity(&[]));
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. Lowering: Copilot→IR (to_ir)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lowering_user_text_to_ir() {
    let msgs = vec![copilot_msg("user", "Hello IR")];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello IR");
}

#[test]
fn lowering_system_text_to_ir() {
    let msgs = vec![copilot_msg("system", "System prompt")];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn lowering_assistant_text_to_ir() {
    let msgs = vec![copilot_msg("assistant", "I can help")];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
}

#[test]
fn lowering_unknown_role_maps_to_user() {
    let msgs = vec![copilot_msg("developer", "hi")];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn lowering_empty_content_produces_no_blocks() {
    let msgs = vec![copilot_msg("user", "")];
    let conv = lowering::to_ir(&msgs);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn lowering_nonempty_content_produces_text_block() {
    let msgs = vec![copilot_msg("user", "data")];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 1);
    match &conv.messages[0].content[0] {
        IrContentBlock::Text { text } => assert_eq!(text, "data"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn lowering_preserves_references_as_metadata() {
    let msgs = vec![copilot_msg_with_refs(
        "user",
        "check",
        vec![file_ref("f-0", "main.rs")],
    )];
    let conv = lowering::to_ir(&msgs);
    assert!(conv.messages[0].metadata.contains_key("copilot_references"));
}

#[test]
fn lowering_no_references_means_no_metadata_key() {
    let msgs = vec![copilot_msg("user", "plain")];
    let conv = lowering::to_ir(&msgs);
    assert!(!conv.messages[0].metadata.contains_key("copilot_references"));
}

#[test]
fn lowering_preserves_name_as_metadata() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "hi".into(),
        name: Some("bob".into()),
        copilot_references: vec![],
    }];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(
        conv.messages[0]
            .metadata
            .get("copilot_name")
            .and_then(|v| v.as_str()),
        Some("bob")
    );
}

#[test]
fn lowering_empty_messages_produces_empty_conversation() {
    let conv = lowering::to_ir(&[]);
    assert!(conv.is_empty());
}

#[test]
fn lowering_multi_turn_conversation() {
    let msgs = vec![
        copilot_msg("system", "You are helpful."),
        copilot_msg("user", "What is Rust?"),
        copilot_msg("assistant", "A systems language."),
        copilot_msg("user", "Thanks"),
    ];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 4);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
    assert_eq!(conv.messages[3].role, IrRole::User);
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. Lifting: IR→Copilot (from_ir)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lifting_ir_user_to_copilot() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
    let msgs = lowering::from_ir(&conv);
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[0].content, "hi");
}

#[test]
fn lifting_ir_system_to_copilot() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::System, "prompt")]);
    let msgs = lowering::from_ir(&conv);
    assert_eq!(msgs[0].role, "system");
}

#[test]
fn lifting_ir_assistant_to_copilot() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "answer")]);
    let msgs = lowering::from_ir(&conv);
    assert_eq!(msgs[0].role, "assistant");
}

#[test]
fn lifting_ir_tool_role_becomes_user() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![IrContentBlock::Text {
                text: "result".into(),
            }],
            is_error: false,
        }],
    )]);
    let msgs = lowering::from_ir(&conv);
    assert_eq!(msgs[0].role, "user");
}

#[test]
fn lifting_thinking_block_becomes_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "let me think...".into(),
        }],
    )]);
    let msgs = lowering::from_ir(&conv);
    assert_eq!(msgs[0].content, "let me think...");
}

#[test]
fn lifting_empty_conversation() {
    let conv = IrConversation::from_messages(vec![]);
    let msgs = lowering::from_ir(&conv);
    assert!(msgs.is_empty());
}

#[test]
fn lifting_restores_references_from_metadata() {
    let refs = vec![file_ref("f-0", "lib.rs")];
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "copilot_references".to_string(),
        serde_json::to_value(&refs).unwrap(),
    );
    let conv = IrConversation::from_messages(vec![IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text {
            text: "check".into(),
        }],
        metadata,
    }]);
    let msgs = lowering::from_ir(&conv);
    assert_eq!(msgs[0].copilot_references.len(), 1);
    assert_eq!(msgs[0].copilot_references[0].id, "f-0");
}

#[test]
fn lifting_restores_name_from_metadata() {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "copilot_name".to_string(),
        serde_json::Value::String("alice".into()),
    );
    let conv = IrConversation::from_messages(vec![IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata,
    }]);
    let msgs = lowering::from_ir(&conv);
    assert_eq!(msgs[0].name.as_deref(), Some("alice"));
}

#[test]
fn lifting_no_metadata_means_no_refs_no_name() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "plain")]);
    let msgs = lowering::from_ir(&conv);
    assert!(msgs[0].copilot_references.is_empty());
    assert!(msgs[0].name.is_none());
}

#[test]
fn lifting_image_block_produces_empty_content() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "base64data".into(),
        }],
    )]);
    let msgs = lowering::from_ir(&conv);
    // Image blocks are not text or thinking, so they contribute nothing
    assert!(msgs[0].content.is_empty());
}

#[test]
fn lifting_mixed_text_and_thinking_concatenated() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "hmm ".into(),
            },
            IrContentBlock::Text {
                text: "the answer".into(),
            },
        ],
    )]);
    let msgs = lowering::from_ir(&conv);
    assert_eq!(msgs[0].content, "hmm the answer");
}

// ═══════════════════════════════════════════════════════════════════════════
// 21. Lowering roundtrips
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_user_message() {
    let original = vec![copilot_msg("user", "roundtrip test")];
    let back = lowering::from_ir(&lowering::to_ir(&original));
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "roundtrip test");
}

#[test]
fn roundtrip_system_message() {
    let original = vec![copilot_msg("system", "be helpful")];
    let back = lowering::from_ir(&lowering::to_ir(&original));
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content, "be helpful");
}

#[test]
fn roundtrip_assistant_message() {
    let original = vec![copilot_msg("assistant", "sure!")];
    let back = lowering::from_ir(&lowering::to_ir(&original));
    assert_eq!(back[0].role, "assistant");
    assert_eq!(back[0].content, "sure!");
}

#[test]
fn roundtrip_with_references() {
    let refs = vec![
        file_ref("f-0", "main.rs"),
        snippet_ref("s-0", "util", "fn bar() {}"),
    ];
    let original = vec![copilot_msg_with_refs("user", "check these", refs)];
    let back = lowering::from_ir(&lowering::to_ir(&original));
    assert_eq!(back[0].copilot_references.len(), 2);
    assert_eq!(back[0].copilot_references[0].id, "f-0");
    assert_eq!(back[0].copilot_references[1].id, "s-0");
}

#[test]
fn roundtrip_with_name() {
    let original = vec![CopilotMessage {
        role: "user".into(),
        content: "hi".into(),
        name: Some("charlie".into()),
        copilot_references: vec![],
    }];
    let back = lowering::from_ir(&lowering::to_ir(&original));
    assert_eq!(back[0].name.as_deref(), Some("charlie"));
}

#[test]
fn roundtrip_multi_turn() {
    let original = vec![
        copilot_msg("system", "instructions"),
        copilot_msg("user", "question"),
        copilot_msg("assistant", "answer"),
        copilot_msg("user", "followup"),
        copilot_msg("assistant", "more detail"),
    ];
    let back = lowering::from_ir(&lowering::to_ir(&original));
    assert_eq!(back.len(), 5);
    for (a, b) in original.iter().zip(back.iter()) {
        assert_eq!(a.role, b.role);
        assert_eq!(a.content, b.content);
    }
}

#[test]
fn roundtrip_empty_content() {
    let original = vec![copilot_msg("user", "")];
    let back = lowering::from_ir(&lowering::to_ir(&original));
    assert!(back[0].content.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 22. extract_references
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn extract_references_collects_from_multiple_messages() {
    let msgs = vec![
        copilot_msg_with_refs("user", "a", vec![file_ref("f-0", "a.rs")]),
        copilot_msg_with_refs("user", "b", vec![repo_ref("r-0", "org", "repo")]),
    ];
    let conv = lowering::to_ir(&msgs);
    let refs = lowering::extract_references(&conv);
    assert_eq!(refs.len(), 2);
}

#[test]
fn extract_references_empty_when_no_refs() {
    let msgs = vec![copilot_msg("user", "no refs")];
    let conv = lowering::to_ir(&msgs);
    let refs = lowering::extract_references(&conv);
    assert!(refs.is_empty());
}

#[test]
fn extract_references_preserves_ref_types() {
    let msgs = vec![copilot_msg_with_refs(
        "user",
        "mixed",
        vec![
            file_ref("f-0", "a.rs"),
            snippet_ref("s-0", "snip", "code"),
            repo_ref("r-0", "o", "n"),
            web_ref("w-0", "https://example.com"),
        ],
    )];
    let conv = lowering::to_ir(&msgs);
    let refs = lowering::extract_references(&conv);
    assert_eq!(refs.len(), 4);
    assert_eq!(refs[0].ref_type, CopilotReferenceType::File);
    assert_eq!(refs[1].ref_type, CopilotReferenceType::Snippet);
    assert_eq!(refs[2].ref_type, CopilotReferenceType::Repository);
    assert_eq!(refs[3].ref_type, CopilotReferenceType::WebSearchResult);
}

// ═══════════════════════════════════════════════════════════════════════════
// 23. Edge cases and stress
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unicode_content_roundtrip() {
    let original = vec![copilot_msg("user", "こんにちは 🦀 café résumé")];
    let back = lowering::from_ir(&lowering::to_ir(&original));
    assert_eq!(back[0].content, "こんにちは 🦀 café résumé");
}

#[test]
fn very_long_content_roundtrip() {
    let long_text = "x".repeat(100_000);
    let original = vec![copilot_msg("user", &long_text)];
    let back = lowering::from_ir(&lowering::to_ir(&original));
    assert_eq!(back[0].content.len(), 100_000);
}

#[test]
fn newlines_preserved_in_content() {
    let original = vec![copilot_msg("user", "line1\nline2\nline3")];
    let back = lowering::from_ir(&lowering::to_ir(&original));
    assert_eq!(back[0].content, "line1\nline2\nline3");
}

#[test]
fn reference_with_complex_data() {
    let r = CopilotReference {
        ref_type: CopilotReferenceType::Snippet,
        id: "s-complex".into(),
        data: json!({
            "name": "test.rs",
            "content": "fn main() {\n    println!(\"hello\");\n}",
            "language": "rust",
            "line_range": [1, 3],
        }),
        metadata: Some({
            let mut m = BTreeMap::new();
            m.insert("display_name".into(), json!("Test file"));
            m.insert("tags".into(), json!(["rust", "main"]));
            m
        }),
    };
    let json = serde_json::to_value(&r).unwrap();
    let back: CopilotReference = serde_json::from_value(json).unwrap();
    assert_eq!(back.data["line_range"][1], 3);
}

#[test]
fn many_references_roundtrip() {
    let refs: Vec<CopilotReference> = (0..50)
        .map(|i| file_ref(&format!("f-{i}"), &format!("file_{i}.rs")))
        .collect();
    let msgs = vec![copilot_msg_with_refs("user", "many files", refs)];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    assert_eq!(back[0].copilot_references.len(), 50);
}

#[test]
fn stream_event_sequence_fidelity() {
    // Simulate a realistic stream: refs → deltas → function call → done
    let events = vec![
        CopilotStreamEvent::CopilotReferences {
            references: vec![file_ref("f-0", "src/lib.rs")],
        },
        CopilotStreamEvent::TextDelta {
            text: "Let me ".into(),
        },
        CopilotStreamEvent::TextDelta {
            text: "analyze ".into(),
        },
        CopilotStreamEvent::TextDelta {
            text: "this.".into(),
        },
        CopilotStreamEvent::FunctionCall {
            function_call: CopilotFunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"src/lib.rs"}"#.into(),
                id: Some("call-1".into()),
            },
        },
        CopilotStreamEvent::Done {},
    ];

    // All events map correctly
    let agent_events: Vec<_> = events.iter().flat_map(dialect::map_stream_event).collect();
    assert!(agent_events.len() >= 5); // refs(1) + 3 deltas + tool(1) + done(1) = 6

    // All events pass passthrough fidelity
    assert!(dialect::verify_passthrough_fidelity(&events));
}

#[test]
fn map_work_order_with_files_and_snippets_combined() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into(), "b.rs".into()],
        snippets: vec![
            ContextSnippet {
                name: "s1".into(),
                content: "code1".into(),
            },
            ContextSnippet {
                name: "s2".into(),
                content: "code2".into(),
            },
        ],
    };
    let wo = WorkOrderBuilder::new("review all").context(ctx).build();
    let cfg = CopilotConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    // 2 files + 2 snippets = 4 references
    assert_eq!(req.references.len(), 4);
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::File);
    assert_eq!(req.references[2].ref_type, CopilotReferenceType::Snippet);
}

#[test]
fn multiple_tool_defs_roundtrip() {
    let defs = [
        make_canonical_tool("read_file", "Read"),
        make_canonical_tool("write_file", "Write"),
        make_canonical_tool("bash", "Run shell"),
    ];
    let tools: Vec<CopilotTool> = defs.iter().map(dialect::tool_def_to_copilot).collect();
    let back: Vec<CanonicalToolDef> = tools
        .iter()
        .filter_map(dialect::tool_def_from_copilot)
        .collect();
    assert_eq!(back.len(), 3);
    for (orig, recovered) in defs.iter().zip(back.iter()) {
        assert_eq!(orig.name, recovered.name);
        assert_eq!(orig.description, recovered.description);
    }
}

#[test]
fn response_with_only_confirmation_and_no_message() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: Some(CopilotConfirmation {
            id: "c1".into(),
            title: "Approve?".into(),
            message: "Proceed with changes?".into(),
            accepted: None,
        }),
        function_call: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Warning { .. } => {}
        other => panic!("expected Warning, got {other:?}"),
    }
}

#[test]
fn config_custom_model_propagates_through_work_order() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CopilotConfig {
        model: "claude-sonnet-4".into(),
        ..CopilotConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "claude-sonnet-4");
}

#[test]
fn lowering_tool_use_block_not_extracted_as_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "search".into(),
            input: json!({"q": "test"}),
        }],
    )]);
    let msgs = lowering::from_ir(&conv);
    // ToolUse is not text or thinking, so content is empty
    assert!(msgs[0].content.is_empty());
}

#[test]
fn lowering_tool_result_block_not_extracted_as_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: vec![IrContentBlock::Text {
                text: "result data".into(),
            }],
            is_error: false,
        }],
    )]);
    let msgs = lowering::from_ir(&conv);
    // ToolResult is not text or thinking at the top level
    assert!(msgs[0].content.is_empty());
}
