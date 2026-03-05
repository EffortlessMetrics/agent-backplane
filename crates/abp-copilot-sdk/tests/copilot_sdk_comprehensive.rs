#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

//! Comprehensive tests for the `abp-copilot-sdk` crate covering all public API surfaces.

use abp_copilot_sdk::api::{
    CopilotAssistantMessage, CopilotChoice, CopilotFinishReason,
    CopilotFunctionCall as ApiFunctionCall, CopilotMessage as ApiMessage,
    CopilotRequest as ApiRequest, CopilotResponse as ApiResponse, CopilotUsage as ApiUsage,
};
use abp_copilot_sdk::convert::{
    extract_intent, extract_references, file_reference, filter_references, from_receipt,
    git_diff_reference, repo_reference, selection_reference, to_work_order,
};
use abp_copilot_sdk::dialect::{
    capability_manifest, from_canonical_model, from_passthrough_event, is_known_model,
    map_response, map_stream_event, map_work_order, to_canonical_model, to_passthrough_event,
    tool_def_from_copilot, tool_def_to_copilot, verify_passthrough_fidelity, CanonicalToolDef,
    CopilotConfig, CopilotConfirmation, CopilotError, CopilotFunctionCall as DialectFunctionCall,
    CopilotFunctionDef, CopilotMessage as DialectMessage, CopilotReference, CopilotReferenceType,
    CopilotRequest as DialectRequest, CopilotResponse as DialectResponse, CopilotStreamEvent,
    CopilotTool as DialectTool, CopilotToolType, CopilotTurnEntry, DEFAULT_MODEL, DIALECT_VERSION,
};
use abp_copilot_sdk::lowering::{
    self, extract_references as lowering_extract_references, from_ir, to_ir,
};
use abp_copilot_sdk::types::{
    CopilotChatChoice, CopilotChatChoiceMessage, CopilotChatMessage, CopilotChatRequest,
    CopilotChatResponse, CopilotFunctionCall as TypesFunctionCall, CopilotStreamChoice,
    CopilotStreamChunk, CopilotStreamDelta, CopilotStreamFunctionCall, CopilotStreamToolCall,
    CopilotTool as TypesTool, CopilotToolCall, CopilotToolFunction, CopilotUsage as TypesUsage,
    Reference, ReferenceType,
};
use abp_copilot_sdk::{sidecar_script, BACKEND_NAME, DEFAULT_NODE_COMMAND, HOST_SCRIPT_RELATIVE};

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{
    AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized, WorkOrderBuilder,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

// ═══════════════════════════════════════════════════════════════════════════
// Module: lib.rs constants & registration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lib_backend_name_constant() {
    assert_eq!(BACKEND_NAME, "sidecar:copilot");
}

#[test]
fn lib_host_script_relative_constant() {
    assert_eq!(HOST_SCRIPT_RELATIVE, "hosts/copilot/host.js");
}

#[test]
fn lib_default_node_command_constant() {
    assert_eq!(DEFAULT_NODE_COMMAND, "node");
}

#[test]
fn lib_sidecar_script_builds_correct_path() {
    let root = Path::new("/workspace");
    let script = sidecar_script(root);
    assert_eq!(script, root.join("hosts/copilot/host.js"));
}

#[test]
fn lib_sidecar_script_empty_root() {
    let root = Path::new("");
    let script = sidecar_script(root);
    assert_eq!(script, Path::new("hosts/copilot/host.js"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: types (Copilot Chat Completions wire-format)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn types_reference_type_file_serde() {
    let rt = ReferenceType::File;
    let json = serde_json::to_string(&rt).unwrap();
    assert_eq!(json, r#""file""#);
    let parsed: ReferenceType = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ReferenceType::File);
}

#[test]
fn types_reference_type_all_variants_serde() {
    let variants = vec![
        (ReferenceType::File, "\"file\""),
        (ReferenceType::Selection, "\"selection\""),
        (ReferenceType::Terminal, "\"terminal\""),
        (ReferenceType::WebPage, "\"web_page\""),
        (ReferenceType::GitDiff, "\"git_diff\""),
    ];
    for (variant, expected) in variants {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, expected);
        let parsed: ReferenceType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, variant);
    }
}

#[test]
fn types_reference_full_serde_roundtrip() {
    let r = Reference {
        ref_type: ReferenceType::File,
        id: "file-0".into(),
        uri: Some("file:///src/main.rs".into()),
        content: None,
        metadata: Some({
            let mut m = BTreeMap::new();
            m.insert("lang".into(), json!("rust"));
            m
        }),
    };
    let json = serde_json::to_string(&r).unwrap();
    let parsed: Reference = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, r);
}

#[test]
fn types_reference_optional_fields_omitted() {
    let r = Reference {
        ref_type: ReferenceType::File,
        id: "f1".into(),
        uri: None,
        content: None,
        metadata: None,
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("uri"));
    assert!(!json.contains("content"));
    assert!(!json.contains("metadata"));
}

#[test]
fn types_chat_message_serde_roundtrip() {
    let msg = CopilotChatMessage {
        role: "user".into(),
        content: Some("Hello".into()),
        name: Some("alice".into()),
        tool_calls: None,
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: CopilotChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn types_chat_message_optional_fields_omitted() {
    let msg = CopilotChatMessage {
        role: "user".into(),
        content: None,
        name: None,
        tool_calls: None,
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains("content"));
    assert!(!json.contains("name"));
    assert!(!json.contains("tool_calls"));
    assert!(!json.contains("tool_call_id"));
}

#[test]
fn types_tool_call_serde_roundtrip() {
    let tc = CopilotToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: TypesFunctionCall {
            name: "read_file".into(),
            arguments: r#"{"path":"main.rs"}"#.into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: CopilotToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn types_chat_request_full_serde_roundtrip() {
    let req = CopilotChatRequest {
        model: "gpt-4o".into(),
        messages: vec![CopilotChatMessage {
            role: "user".into(),
            content: Some("Review code".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        temperature: Some(0.5),
        top_p: Some(0.9),
        max_tokens: Some(2048),
        stream: Some(true),
        tools: Some(vec![TypesTool {
            tool_type: "function".into(),
            function: CopilotToolFunction {
                name: "bash".into(),
                description: "Run command".into(),
                parameters: json!({"type": "object"}),
            },
        }]),
        tool_choice: Some(json!("auto")),
        intent: Some("code-review".into()),
        references: Some(vec![Reference {
            ref_type: ReferenceType::File,
            id: "f0".into(),
            uri: Some("file:///foo.rs".into()),
            content: None,
            metadata: None,
        }]),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: CopilotChatRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn types_chat_request_optional_fields_omitted() {
    let req = CopilotChatRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
        intent: None,
        references: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("temperature"));
    assert!(!json.contains("top_p"));
    assert!(!json.contains("stream"));
    assert!(!json.contains("intent"));
}

#[test]
fn types_chat_response_serde_roundtrip() {
    let resp = CopilotChatResponse {
        id: "resp-1".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![CopilotChatChoice {
            index: 0,
            message: CopilotChatChoiceMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(TypesUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            copilot_tokens: Some(2),
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: CopilotChatResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn types_stream_chunk_serde_roundtrip() {
    let chunk = CopilotStreamChunk {
        id: "chunk-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![CopilotStreamChoice {
            index: 0,
            delta: CopilotStreamDelta {
                role: Some("assistant".into()),
                content: Some("Hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: CopilotStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, chunk);
}

#[test]
fn types_stream_delta_default() {
    let delta = CopilotStreamDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

#[test]
fn types_stream_tool_call_serde_roundtrip() {
    let stc = CopilotStreamToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(CopilotStreamFunctionCall {
            name: Some("read_file".into()),
            arguments: Some(r#"{"path":"x"}"#.into()),
        }),
    };
    let json = serde_json::to_string(&stc).unwrap();
    let parsed: CopilotStreamToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, stc);
}

#[test]
fn types_stream_function_call_partial_serde() {
    let sfc = CopilotStreamFunctionCall {
        name: None,
        arguments: Some(r#"{"pa"#.into()),
    };
    let json = serde_json::to_string(&sfc).unwrap();
    assert!(!json.contains("name"));
    let parsed: CopilotStreamFunctionCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, sfc);
}

#[test]
fn types_usage_with_copilot_tokens() {
    let usage = TypesUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        copilot_tokens: Some(25),
    };
    let json = serde_json::to_string(&usage).unwrap();
    assert!(json.contains("copilot_tokens"));
    let parsed: TypesUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.copilot_tokens, Some(25));
}

#[test]
fn types_usage_without_copilot_tokens() {
    let usage = TypesUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        copilot_tokens: None,
    };
    let json = serde_json::to_string(&usage).unwrap();
    assert!(!json.contains("copilot_tokens"));
}

#[test]
fn types_tool_function_serde_roundtrip() {
    let tool = TypesTool {
        tool_type: "function".into(),
        function: CopilotToolFunction {
            name: "grep".into(),
            description: "Search files".into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let parsed: TypesTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: convert (CopilotChatRequest ↔ WorkOrder, Receipt → Response)
// ═══════════════════════════════════════════════════════════════════════════

fn sample_chat_request() -> CopilotChatRequest {
    CopilotChatRequest {
        model: "gpt-4o".into(),
        messages: vec![CopilotChatMessage {
            role: "user".into(),
            content: Some("Review my code".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        temperature: Some(0.3),
        top_p: None,
        max_tokens: Some(4096),
        stream: Some(true),
        tools: None,
        tool_choice: None,
        intent: Some("code-review".into()),
        references: Some(vec![
            file_reference("f1", "file:///src/main.rs"),
            selection_reference("s1", "fn main() {}"),
            git_diff_reference("d1", "+new line"),
        ]),
    }
}

#[test]
fn convert_to_work_order_extracts_user_message_as_task() {
    let req = sample_chat_request();
    let wo = to_work_order(&req);
    assert_eq!(wo.task, "Review my code");
}

#[test]
fn convert_to_work_order_sets_model() {
    let req = sample_chat_request();
    let wo = to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn convert_to_work_order_stores_intent_in_vendor() {
    let req = sample_chat_request();
    let wo = to_work_order(&req);
    let copilot = wo.config.vendor.get("copilot").unwrap();
    assert_eq!(copilot["intent"], "code-review");
}

#[test]
fn convert_to_work_order_stores_references_in_vendor() {
    let req = sample_chat_request();
    let wo = to_work_order(&req);
    let copilot = wo.config.vendor.get("copilot").unwrap();
    let refs = copilot["references"].as_array().unwrap();
    assert_eq!(refs.len(), 3);
}

#[test]
fn convert_to_work_order_stores_temperature() {
    let req = sample_chat_request();
    let wo = to_work_order(&req);
    let copilot = wo.config.vendor.get("copilot").unwrap();
    assert!(copilot.get("temperature").is_some());
}

#[test]
fn convert_to_work_order_stores_stream_flag() {
    let req = sample_chat_request();
    let wo = to_work_order(&req);
    let copilot = wo.config.vendor.get("copilot").unwrap();
    assert_eq!(copilot["stream"], true);
}

#[test]
fn convert_to_work_order_maps_file_refs_to_context_files() {
    let req = sample_chat_request();
    let wo = to_work_order(&req);
    assert!(wo
        .context
        .files
        .contains(&"file:///src/main.rs".to_string()));
}

#[test]
fn convert_to_work_order_maps_selection_refs_to_context_snippets() {
    let req = sample_chat_request();
    let wo = to_work_order(&req);
    assert_eq!(wo.context.snippets.len(), 1);
    assert!(wo.context.snippets[0].content.contains("fn main()"));
}

#[test]
fn convert_to_work_order_no_user_msg_fallback() {
    let req = CopilotChatRequest {
        model: "gpt-4o".into(),
        messages: vec![CopilotChatMessage {
            role: "system".into(),
            content: Some("sys".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
        intent: None,
        references: None,
    };
    let wo = to_work_order(&req);
    assert_eq!(wo.task, "(empty)");
}

#[test]
fn convert_to_work_order_multiple_user_messages_concatenated() {
    let req = CopilotChatRequest {
        model: "gpt-4o".into(),
        messages: vec![
            CopilotChatMessage {
                role: "user".into(),
                content: Some("First".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            CopilotChatMessage {
                role: "user".into(),
                content: Some("Second".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
        intent: None,
        references: None,
    };
    let wo = to_work_order(&req);
    assert!(wo.task.contains("First"));
    assert!(wo.task.contains("Second"));
}

#[test]
fn convert_to_work_order_without_intent_omits_key() {
    let mut req = sample_chat_request();
    req.intent = None;
    let wo = to_work_order(&req);
    let copilot = wo.config.vendor.get("copilot").unwrap();
    assert!(copilot.get("intent").is_none());
}

#[test]
fn convert_from_receipt_produces_valid_response() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4o").build();
    let receipt = ReceiptBuilder::new("copilot")
        .work_order_id(wo.id)
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..UsageNormalized::default()
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Looks good!".into(),
            },
            ext: None,
        })
        .build();
    let resp = from_receipt(&receipt, &wo);
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.model, "gpt-4o");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Looks good!")
    );
}

#[test]
fn convert_from_receipt_stop_on_complete() {
    let wo = WorkOrderBuilder::new("task").build();
    let receipt = ReceiptBuilder::new("copilot")
        .outcome(Outcome::Complete)
        .build();
    let resp = from_receipt(&receipt, &wo);
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn convert_from_receipt_length_on_partial() {
    let wo = WorkOrderBuilder::new("task").build();
    let receipt = ReceiptBuilder::new("copilot")
        .outcome(Outcome::Partial)
        .build();
    let resp = from_receipt(&receipt, &wo);
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("length"));
}

#[test]
fn convert_from_receipt_usage_present() {
    let wo = WorkOrderBuilder::new("task").build();
    let receipt = ReceiptBuilder::new("copilot")
        .usage(UsageNormalized {
            input_tokens: Some(300),
            output_tokens: Some(120),
            ..UsageNormalized::default()
        })
        .build();
    let resp = from_receipt(&receipt, &wo);
    let usage = resp.usage.as_ref().unwrap();
    assert_eq!(usage.prompt_tokens, 300);
    assert_eq!(usage.completion_tokens, 120);
    assert_eq!(usage.total_tokens, 420);
}

#[test]
fn convert_from_receipt_no_usage_when_zero() {
    let wo = WorkOrderBuilder::new("task").build();
    let receipt = ReceiptBuilder::new("copilot")
        .outcome(Outcome::Complete)
        .build();
    let resp = from_receipt(&receipt, &wo);
    assert!(resp.usage.is_none());
}

// ── Reference helpers ───────────────────────────────────────────────────

#[test]
fn convert_file_reference_creates_correctly() {
    let r = file_reference("f1", "file:///foo.rs");
    assert_eq!(r.ref_type, ReferenceType::File);
    assert_eq!(r.id, "f1");
    assert_eq!(r.uri.as_deref(), Some("file:///foo.rs"));
    assert!(r.content.is_none());
    assert!(r.metadata.is_none());
}

#[test]
fn convert_selection_reference_creates_correctly() {
    let r = selection_reference("s1", "selected text");
    assert_eq!(r.ref_type, ReferenceType::Selection);
    assert_eq!(r.content.as_deref(), Some("selected text"));
    assert!(r.uri.is_none());
}

#[test]
fn convert_git_diff_reference_creates_correctly() {
    let r = git_diff_reference("d1", "+new line");
    assert_eq!(r.ref_type, ReferenceType::GitDiff);
    assert!(r.content.as_deref().unwrap().contains("+new line"));
}

#[test]
fn convert_repo_reference_creates_correctly() {
    let mut meta = BTreeMap::new();
    meta.insert("owner".into(), json!("octocat"));
    let r = repo_reference("r1", meta);
    assert_eq!(r.ref_type, ReferenceType::Terminal);
    assert!(r.metadata.is_some());
}

#[test]
fn convert_filter_references_by_type() {
    let refs = vec![
        file_reference("f1", "a.rs"),
        selection_reference("s1", "x"),
        file_reference("f2", "b.rs"),
        git_diff_reference("d1", "+y"),
    ];
    let files = filter_references(&refs, ReferenceType::File);
    assert_eq!(files.len(), 2);
    let selections = filter_references(&refs, ReferenceType::Selection);
    assert_eq!(selections.len(), 1);
    let diffs = filter_references(&refs, ReferenceType::GitDiff);
    assert_eq!(diffs.len(), 1);
}

#[test]
fn convert_extract_references_from_work_order() {
    let req = sample_chat_request();
    let wo = to_work_order(&req);
    let refs = extract_references(&wo).unwrap();
    assert_eq!(refs.len(), 3);
}

#[test]
fn convert_extract_references_returns_none_when_absent() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(extract_references(&wo).is_none());
}

#[test]
fn convert_extract_intent_from_work_order() {
    let req = sample_chat_request();
    let wo = to_work_order(&req);
    let intent = extract_intent(&wo).unwrap();
    assert_eq!(intent, "code-review");
}

#[test]
fn convert_extract_intent_returns_none_when_absent() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(extract_intent(&wo).is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: dialect
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_version_constant() {
    assert_eq!(DIALECT_VERSION, "copilot/v0.1");
}

#[test]
fn dialect_default_model_constant() {
    assert_eq!(DEFAULT_MODEL, "gpt-4o");
}

#[test]
fn dialect_to_canonical_model() {
    assert_eq!(to_canonical_model("gpt-4o"), "copilot/gpt-4o");
    assert_eq!(to_canonical_model("o1-mini"), "copilot/o1-mini");
}

#[test]
fn dialect_from_canonical_model_strips_prefix() {
    assert_eq!(from_canonical_model("copilot/gpt-4o"), "gpt-4o");
}

#[test]
fn dialect_from_canonical_model_no_prefix_passthrough() {
    assert_eq!(from_canonical_model("gpt-4o"), "gpt-4o");
}

#[test]
fn dialect_is_known_model_true_for_known() {
    assert!(is_known_model("gpt-4o"));
    assert!(is_known_model("gpt-4o-mini"));
    assert!(is_known_model("o1"));
    assert!(is_known_model("o1-mini"));
    assert!(is_known_model("o3-mini"));
    assert!(is_known_model("claude-sonnet-4"));
    assert!(is_known_model("claude-3.5-sonnet"));
}

#[test]
fn dialect_is_known_model_false_for_unknown() {
    assert!(!is_known_model("gpt-5"));
    assert!(!is_known_model("unknown-model"));
    assert!(!is_known_model(""));
}

#[test]
fn dialect_capability_manifest_has_streaming() {
    use abp_core::Capability;
    let m = capability_manifest();
    assert!(m.get(&Capability::Streaming).is_some());
}

#[test]
fn dialect_capability_manifest_has_tool_read() {
    use abp_core::Capability;
    let m = capability_manifest();
    assert!(m.get(&Capability::ToolRead).is_some());
}

#[test]
fn dialect_capability_manifest_has_glob() {
    use abp_core::Capability;
    let m = capability_manifest();
    assert!(m.get(&Capability::ToolGlob).is_some());
}

// ── Dialect reference types ─────────────────────────────────────────────

#[test]
fn dialect_reference_type_serde_roundtrip() {
    let variants = vec![
        (CopilotReferenceType::File, "\"file\""),
        (CopilotReferenceType::Snippet, "\"snippet\""),
        (CopilotReferenceType::Repository, "\"repository\""),
        (
            CopilotReferenceType::WebSearchResult,
            "\"web_search_result\"",
        ),
    ];
    for (variant, expected) in variants {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, expected);
        let parsed: CopilotReferenceType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, variant);
    }
}

#[test]
fn dialect_copilot_reference_serde_roundtrip() {
    let r = CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "file-0".into(),
        data: json!({"path": "src/main.rs"}),
        metadata: None,
    };
    let json = serde_json::to_string(&r).unwrap();
    let parsed: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, r);
}

// ── Dialect tool types ──────────────────────────────────────────────────

#[test]
fn dialect_tool_type_serde_roundtrip() {
    let ft = CopilotToolType::Function;
    let json = serde_json::to_string(&ft).unwrap();
    assert_eq!(json, r#""function""#);

    let ct = CopilotToolType::Confirmation;
    let json = serde_json::to_string(&ct).unwrap();
    assert_eq!(json, r#""confirmation""#);
}

#[test]
fn dialect_tool_def_to_copilot_conversion() {
    let def = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let tool = tool_def_to_copilot(&def);
    assert_eq!(tool.tool_type, CopilotToolType::Function);
    let func = tool.function.unwrap();
    assert_eq!(func.name, "read_file");
    assert_eq!(func.description, "Read a file");
    assert!(tool.confirmation.is_none());
}

#[test]
fn dialect_tool_def_from_copilot_roundtrip() {
    let def = CanonicalToolDef {
        name: "bash".into(),
        description: "Run bash".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let tool = tool_def_to_copilot(&def);
    let back = tool_def_from_copilot(&tool).unwrap();
    assert_eq!(back.name, "bash");
    assert_eq!(back.description, "Run bash");
}

#[test]
fn dialect_tool_def_from_copilot_returns_none_for_confirmation() {
    let tool = DialectTool {
        tool_type: CopilotToolType::Confirmation,
        function: None,
        confirmation: Some(CopilotConfirmation {
            id: "c1".into(),
            title: "Delete".into(),
            message: "Are you sure?".into(),
            accepted: None,
        }),
    };
    assert!(tool_def_from_copilot(&tool).is_none());
}

// ── Dialect config ──────────────────────────────────────────────────────

#[test]
fn dialect_copilot_config_default() {
    let cfg = CopilotConfig::default();
    assert_eq!(cfg.model, "gpt-4o");
    assert!(cfg.base_url.contains("githubcopilot"));
    assert!(cfg.token.is_empty());
    assert!(cfg.system_prompt.is_none());
}

// ── Dialect message types ───────────────────────────────────────────────

#[test]
fn dialect_copilot_message_serde_roundtrip() {
    let msg = DialectMessage {
        role: "user".into(),
        content: "Hello".into(),
        name: Some("alice".into()),
        copilot_references: vec![],
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: DialectMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn dialect_turn_entry_serde_roundtrip() {
    let entry = CopilotTurnEntry {
        request: "Hello".into(),
        response: "Hi!".into(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: CopilotTurnEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, entry);
}

#[test]
fn dialect_confirmation_serde_roundtrip() {
    let conf = CopilotConfirmation {
        id: "c1".into(),
        title: "Delete file".into(),
        message: "Are you sure?".into(),
        accepted: Some(true),
    };
    let json = serde_json::to_string(&conf).unwrap();
    let parsed: CopilotConfirmation = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, conf);
}

#[test]
fn dialect_confirmation_accepted_none_omitted() {
    let conf = CopilotConfirmation {
        id: "c1".into(),
        title: "X".into(),
        message: "Y".into(),
        accepted: None,
    };
    let json = serde_json::to_string(&conf).unwrap();
    assert!(!json.contains("accepted"));
}

#[test]
fn dialect_error_serde_roundtrip() {
    let err = CopilotError {
        error_type: "rate_limit".into(),
        message: "Too many requests".into(),
        code: Some("429".into()),
        identifier: Some("err-1".into()),
    };
    let json = serde_json::to_string(&err).unwrap();
    let parsed: CopilotError = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, err);
}

// ── Dialect: map_work_order ─────────────────────────────────────────────

#[test]
fn dialect_map_work_order_uses_task_as_user_message() {
    let wo = WorkOrderBuilder::new("Refactor auth").build();
    let cfg = CopilotConfig::default();
    let req = map_work_order(&wo, &cfg);
    let user_msgs: Vec<_> = req.messages.iter().filter(|m| m.role == "user").collect();
    assert_eq!(user_msgs.len(), 1);
    assert!(user_msgs[0].content.contains("Refactor auth"));
}

#[test]
fn dialect_map_work_order_respects_model_override() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4-turbo").build();
    let cfg = CopilotConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4-turbo");
}

#[test]
fn dialect_map_work_order_defaults_to_config_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CopilotConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4o");
}

#[test]
fn dialect_map_work_order_includes_system_prompt() {
    let wo = WorkOrderBuilder::new("task").build();
    let mut cfg = CopilotConfig::default();
    cfg.system_prompt = Some("Be helpful".into());
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.messages[0].role, "system");
    assert_eq!(req.messages[0].content, "Be helpful");
}

#[test]
fn dialect_map_work_order_maps_context_files_to_references() {
    let wo = WorkOrderBuilder::new("task")
        .context(abp_core::ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![],
        })
        .build();
    let cfg = CopilotConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert!(!req.references.is_empty());
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::File);
}

#[test]
fn dialect_map_work_order_maps_context_snippets_to_references() {
    let wo = WorkOrderBuilder::new("task")
        .context(abp_core::ContextPacket {
            files: vec![],
            snippets: vec![abp_core::ContextSnippet {
                name: "helper".into(),
                content: "fn foo() {}".into(),
            }],
        })
        .build();
    let cfg = CopilotConfig::default();
    let req = map_work_order(&wo, &cfg);
    let snippet_refs: Vec<_> = req
        .references
        .iter()
        .filter(|r| r.ref_type == CopilotReferenceType::Snippet)
        .collect();
    assert_eq!(snippet_refs.len(), 1);
}

// ── Dialect: map_response ───────────────────────────────────────────────

#[test]
fn dialect_map_response_produces_assistant_message() {
    let resp = DialectResponse {
        message: "Hello from Copilot!".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hello from Copilot!"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn dialect_map_response_empty_message_produces_no_events() {
    let resp = DialectResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn dialect_map_response_handles_errors() {
    let resp = DialectResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![CopilotError {
            error_type: "rate_limit".into(),
            message: "Too many requests".into(),
            code: None,
            identifier: None,
        }],
        copilot_confirmation: None,
        function_call: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert!(message.contains("rate_limit"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn dialect_map_response_handles_function_call() {
    let resp = DialectResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: Some(DialectFunctionCall {
            name: "read_file".into(),
            arguments: r#"{"path":"main.rs"}"#.into(),
            id: Some("call_1".into()),
        }),
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            ..
        } => {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("call_1"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn dialect_map_response_handles_confirmation() {
    let resp = DialectResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: Some(CopilotConfirmation {
            id: "c1".into(),
            title: "Delete".into(),
            message: "Sure?".into(),
            accepted: None,
        }),
        function_call: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Warning { message } => {
            assert!(message.contains("Confirmation required"));
        }
        other => panic!("expected Warning, got {other:?}"),
    }
}

// ── Dialect: map_stream_event ───────────────────────────────────────────

#[test]
fn dialect_map_stream_event_text_delta() {
    let event = CopilotStreamEvent::TextDelta {
        text: "Hello".into(),
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_event_function_call() {
    let event = CopilotStreamEvent::FunctionCall {
        function_call: DialectFunctionCall {
            name: "bash".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
            id: Some("call_2".into()),
        },
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { tool_name, .. } => assert_eq!(tool_name, "bash"),
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_event_errors() {
    let event = CopilotStreamEvent::CopilotErrors {
        errors: vec![
            CopilotError {
                error_type: "auth".into(),
                message: "Unauthorized".into(),
                code: None,
                identifier: None,
            },
            CopilotError {
                error_type: "limit".into(),
                message: "Rate limited".into(),
                code: None,
                identifier: None,
            },
        ],
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 2);
}

#[test]
fn dialect_map_stream_event_references() {
    let event = CopilotStreamEvent::CopilotReferences {
        references: vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "f1".into(),
            data: json!({"path": "main.rs"}),
            metadata: None,
        }],
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::RunStarted { message } => {
            assert!(message.contains("1 reference"));
        }
        other => panic!("expected RunStarted, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_event_empty_references() {
    let event = CopilotStreamEvent::CopilotReferences { references: vec![] };
    let events = map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn dialect_map_stream_event_done() {
    let event = CopilotStreamEvent::Done {};
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::RunCompleted { message } => {
            assert!(message.contains("completed"));
        }
        other => panic!("expected RunCompleted, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_event_confirmation() {
    let event = CopilotStreamEvent::CopilotConfirmation {
        confirmation: CopilotConfirmation {
            id: "c1".into(),
            title: "Approve".into(),
            message: "OK?".into(),
            accepted: None,
        },
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Warning { message } => {
            assert!(message.contains("Confirmation required"));
        }
        other => panic!("expected Warning, got {other:?}"),
    }
}

// ── Dialect: passthrough fidelity ───────────────────────────────────────

#[test]
fn dialect_passthrough_text_delta_roundtrip() {
    let event = CopilotStreamEvent::TextDelta {
        text: "hello".into(),
    };
    let wrapped = to_passthrough_event(&event);
    let ext = wrapped.ext.as_ref().unwrap();
    assert_eq!(ext.get("dialect").unwrap(), "copilot");
    assert!(ext.contains_key("raw_message"));

    let recovered = from_passthrough_event(&wrapped).unwrap();
    assert_eq!(recovered, event);
}

#[test]
fn dialect_passthrough_done_roundtrip() {
    let event = CopilotStreamEvent::Done {};
    let wrapped = to_passthrough_event(&event);
    let recovered = from_passthrough_event(&wrapped).unwrap();
    assert_eq!(recovered, event);
}

#[test]
fn dialect_passthrough_fidelity_all_events() {
    let events = vec![
        CopilotStreamEvent::TextDelta { text: "hi".into() },
        CopilotStreamEvent::FunctionCall {
            function_call: DialectFunctionCall {
                name: "f".into(),
                arguments: "{}".into(),
                id: None,
            },
        },
        CopilotStreamEvent::Done {},
    ];
    assert!(verify_passthrough_fidelity(&events));
}

#[test]
fn dialect_from_passthrough_event_returns_none_for_non_passthrough() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "test".into(),
        },
        ext: None,
    };
    assert!(from_passthrough_event(&event).is_none());
}

// ── Dialect: stream events serde ────────────────────────────────────────

#[test]
fn dialect_stream_event_text_delta_serde() {
    let event = CopilotStreamEvent::TextDelta { text: "hi".into() };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn dialect_stream_event_done_serde() {
    let event = CopilotStreamEvent::Done {};
    let json = serde_json::to_string(&event).unwrap();
    let parsed: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: api (Extensions API surface types and From conversions)
// ═══════════════════════════════════════════════════════════════════════════

fn make_api_request(messages: Vec<ApiMessage>) -> ApiRequest {
    ApiRequest {
        model: "gpt-4o".into(),
        messages,
        stream: None,
        temperature: None,
        max_tokens: None,
        references: vec![],
        copilot_metadata: None,
    }
}

#[test]
fn api_request_to_work_order_uses_last_user_message() {
    let req = make_api_request(vec![
        ApiMessage {
            role: "user".into(),
            content: "First".into(),
            name: None,
            function_call: None,
            copilot_references: vec![],
        },
        ApiMessage {
            role: "user".into(),
            content: "Second".into(),
            name: None,
            function_call: None,
            copilot_references: vec![],
        },
    ]);
    let wo: abp_core::WorkOrder = req.into();
    assert_eq!(wo.task, "Second");
}

#[test]
fn api_request_to_work_order_preserves_model() {
    let mut req = make_api_request(vec![ApiMessage {
        role: "user".into(),
        content: "Hello".into(),
        name: None,
        function_call: None,
        copilot_references: vec![],
    }]);
    req.model = "gpt-4-turbo".into();
    let wo: abp_core::WorkOrder = req.into();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
}

#[test]
fn api_request_to_work_order_empty_messages() {
    let req = make_api_request(vec![]);
    let wo: abp_core::WorkOrder = req.into();
    assert_eq!(wo.task, "");
}

#[test]
fn api_request_to_work_order_maps_system_to_snippets() {
    let req = make_api_request(vec![
        ApiMessage {
            role: "system".into(),
            content: "Be concise.".into(),
            name: None,
            function_call: None,
            copilot_references: vec![],
        },
        ApiMessage {
            role: "user".into(),
            content: "Hi".into(),
            name: None,
            function_call: None,
            copilot_references: vec![],
        },
    ]);
    let wo: abp_core::WorkOrder = req.into();
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].content, "Be concise.");
}

#[test]
fn api_request_to_work_order_maps_file_refs_to_context() {
    let mut req = make_api_request(vec![ApiMessage {
        role: "user".into(),
        content: "Check files".into(),
        name: None,
        function_call: None,
        copilot_references: vec![],
    }]);
    req.references = vec![CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "f-0".into(),
        data: json!({"path": "src/main.rs"}),
        metadata: None,
    }];
    let wo: abp_core::WorkOrder = req.into();
    assert_eq!(wo.context.files.len(), 1);
    assert_eq!(wo.context.files[0], "src/main.rs");
}

#[test]
fn api_finish_reason_serde_roundtrip() {
    for (reason, expected) in [
        (CopilotFinishReason::Stop, "\"stop\""),
        (CopilotFinishReason::Length, "\"length\""),
        (CopilotFinishReason::FunctionCall, "\"function_call\""),
        (CopilotFinishReason::ContentFilter, "\"content_filter\""),
    ] {
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, expected);
        let parsed: CopilotFinishReason = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, reason);
    }
}

#[test]
fn api_usage_serde_roundtrip() {
    let usage = ApiUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: ApiUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}

#[test]
fn api_response_serde_roundtrip() {
    let resp = ApiResponse {
        id: "copilot-abc".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![CopilotChoice {
            index: 0,
            message: CopilotAssistantMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                function_call: None,
                copilot_references: vec![],
            },
            finish_reason: CopilotFinishReason::Stop,
        }],
        usage: None,
        copilot_confirmation: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: ApiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: lowering (Copilot ↔ IR)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lowering_user_text_to_ir_and_back() {
    let msgs = vec![DialectMessage {
        role: "user".into(),
        content: "Hello".into(),
        name: None,
        copilot_references: vec![],
    }];
    let conv = to_ir(&msgs);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello");
    let back = from_ir(&conv);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "Hello");
}

#[test]
fn lowering_system_text_roundtrip() {
    let msgs = vec![DialectMessage {
        role: "system".into(),
        content: "Be helpful.".into(),
        name: None,
        copilot_references: vec![],
    }];
    let conv = to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::System);
    let back = from_ir(&conv);
    assert_eq!(back[0].role, "system");
}

#[test]
fn lowering_assistant_text_roundtrip() {
    let msgs = vec![DialectMessage {
        role: "assistant".into(),
        content: "Sure!".into(),
        name: None,
        copilot_references: vec![],
    }];
    let conv = to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    let back = from_ir(&conv);
    assert_eq!(back[0].role, "assistant");
    assert_eq!(back[0].content, "Sure!");
}

#[test]
fn lowering_references_preserved_through_roundtrip() {
    let refs = vec![CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "file-0".into(),
        data: json!({"path": "src/main.rs"}),
        metadata: None,
    }];
    let msgs = vec![DialectMessage {
        role: "user".into(),
        content: "Read this file".into(),
        name: None,
        copilot_references: refs.clone(),
    }];
    let conv = to_ir(&msgs);
    assert!(conv.messages[0].metadata.contains_key("copilot_references"));
    let back = from_ir(&conv);
    assert_eq!(back[0].copilot_references.len(), 1);
    assert_eq!(back[0].copilot_references[0].id, "file-0");
}

#[test]
fn lowering_name_preserved_through_roundtrip() {
    let msgs = vec![DialectMessage {
        role: "user".into(),
        content: "Hi".into(),
        name: Some("alice".into()),
        copilot_references: vec![],
    }];
    let conv = to_ir(&msgs);
    let back = from_ir(&conv);
    assert_eq!(back[0].name.as_deref(), Some("alice"));
}

#[test]
fn lowering_empty_messages() {
    let conv = to_ir(&[]);
    assert!(conv.is_empty());
    let back = from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn lowering_unknown_role_defaults_to_user() {
    let msgs = vec![DialectMessage {
        role: "developer".into(),
        content: "hi".into(),
        name: None,
        copilot_references: vec![],
    }];
    let conv = to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn lowering_tool_role_mapped_to_user_on_output() {
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
    let back = from_ir(&conv);
    assert_eq!(back[0].role, "user");
}

#[test]
fn lowering_extract_references_across_messages() {
    let msgs = vec![
        DialectMessage {
            role: "user".into(),
            content: "msg1".into(),
            name: None,
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "f1".into(),
                data: json!({}),
                metadata: None,
            }],
        },
        DialectMessage {
            role: "user".into(),
            content: "msg2".into(),
            name: None,
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::Snippet,
                id: "s1".into(),
                data: json!({}),
                metadata: None,
            }],
        },
    ];
    let conv = to_ir(&msgs);
    let all_refs = lowering_extract_references(&conv);
    assert_eq!(all_refs.len(), 2);
}

#[test]
fn lowering_multi_turn_conversation() {
    let msgs = vec![
        DialectMessage {
            role: "system".into(),
            content: "Be concise.".into(),
            name: None,
            copilot_references: vec![],
        },
        DialectMessage {
            role: "user".into(),
            content: "Hi".into(),
            name: None,
            copilot_references: vec![],
        },
        DialectMessage {
            role: "assistant".into(),
            content: "Hello!".into(),
            name: None,
            copilot_references: vec![],
        },
        DialectMessage {
            role: "user".into(),
            content: "Bye".into(),
            name: None,
            copilot_references: vec![],
        },
    ];
    let conv = to_ir(&msgs);
    assert_eq!(conv.len(), 4);
    let back = from_ir(&conv);
    assert_eq!(back.len(), 4);
    assert_eq!(back[3].content, "Bye");
}

#[test]
fn lowering_empty_content_roundtrip() {
    let msgs = vec![DialectMessage {
        role: "user".into(),
        content: String::new(),
        name: None,
        copilot_references: vec![],
    }];
    let conv = to_ir(&msgs);
    assert!(conv.messages[0].content.is_empty());
    let back = from_ir(&conv);
    assert!(back[0].content.is_empty());
}

#[test]
fn lowering_thinking_block_becomes_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "reasoning...".into(),
        }],
    )]);
    let back = from_ir(&conv);
    assert_eq!(back[0].content, "reasoning...");
}
