// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive tests for GitHub Copilot SDK dialect mapping.
//!
//! Covers Copilot chat completion mapping, function calling/tool_use, streaming
//! response mapping, model-name capability mapping, Copilot-specific features,
//! agent mode vs inline mode, Copilot→OpenAI compatibility, error handling,
//! workspace context integration, and tool definitions/execution.

use std::collections::BTreeMap;

use abp_capability::negotiate;
use abp_copilot_sdk::dialect::{
    self, CanonicalToolDef, CopilotConfig, CopilotConfirmation, CopilotError, CopilotFunctionCall,
    CopilotFunctionDef, CopilotMessage, CopilotReference, CopilotReferenceType, CopilotRequest,
    CopilotResponse, CopilotStreamEvent, CopilotTool, CopilotToolType, CopilotTurnEntry,
};
use abp_copilot_sdk::lowering;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, Capability, CapabilityRequirement,
    CapabilityRequirements, ContextPacket, ContextSnippet, MinSupport, Outcome, ReceiptBuilder,
    SupportLevel as CoreSupportLevel, UsageNormalized, WorkOrderBuilder,
};
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, features, known_rules, validate_mapping,
};
use abp_shim_copilot::{
    CopilotRequestBuilder, Message, events_to_stream_events, ir_to_messages, messages_to_ir,
    mock_receipt, mock_receipt_with_usage, receipt_to_response, request_to_work_order,
};
use chrono::Utc;
use serde_json::json;

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

fn default_config() -> CopilotConfig {
    CopilotConfig::default()
}

fn make_function_tool(name: &str, desc: &str) -> CopilotTool {
    CopilotTool {
        tool_type: CopilotToolType::Function,
        function: Some(CopilotFunctionDef {
            name: name.into(),
            description: desc.into(),
            parameters: json!({"type": "object", "properties": {}}),
        }),
        confirmation: None,
    }
}

fn make_confirmation_tool(id: &str, title: &str, msg: &str) -> CopilotTool {
    CopilotTool {
        tool_type: CopilotToolType::Confirmation,
        function: None,
        confirmation: Some(CopilotConfirmation {
            id: id.into(),
            title: title.into(),
            message: msg.into(),
            accepted: None,
        }),
    }
}

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Copilot chat completion request mapping to WorkOrder
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn chat_completion_basic_task_maps_to_work_order() {
    let wo = WorkOrderBuilder::new("Fix the login bug").build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);

    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert_eq!(req.messages[0].content, "Fix the login bug");
    assert_eq!(req.model, "gpt-4o"); // default
}

#[test]
fn chat_completion_with_model_override() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4-turbo").build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4-turbo");
}

#[test]
fn chat_completion_with_system_prompt() {
    let mut cfg = default_config();
    cfg.system_prompt = Some("You are a Rust expert.".into());
    let wo = WorkOrderBuilder::new("Refactor").build();
    let req = dialect::map_work_order(&wo, &cfg);

    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, "system");
    assert_eq!(req.messages[0].content, "You are a Rust expert.");
    assert_eq!(req.messages[1].role, "user");
}

#[test]
fn chat_completion_with_context_files() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("task").context(ctx).build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);

    assert_eq!(req.references.len(), 2);
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::File);
    assert_eq!(req.references[0].id, "file-0");
    assert_eq!(req.references[1].id, "file-1");
}

#[test]
fn chat_completion_with_context_snippets() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "helper.rs".into(),
            content: "fn foo() {}".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("task").context(ctx).build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);

    assert_eq!(req.references.len(), 1);
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::Snippet);
    assert_eq!(req.references[0].id, "snippet-0");
}

#[test]
fn chat_completion_mixed_context() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into()],
        snippets: vec![ContextSnippet {
            name: "b.rs".into(),
            content: "fn b() {}".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("task").context(ctx).build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);

    assert_eq!(req.references.len(), 2);
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::File);
    assert_eq!(req.references[1].ref_type, CopilotReferenceType::Snippet);
}

#[test]
fn chat_completion_user_message_carries_references() {
    let ctx = ContextPacket {
        files: vec!["lib.rs".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("explain").context(ctx).build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);

    assert_eq!(req.messages[0].copilot_references.len(), 1);
}

#[test]
fn chat_completion_no_tools_by_default() {
    let wo = WorkOrderBuilder::new("hello").build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.tools.is_none());
}

#[test]
fn chat_completion_empty_turn_history() {
    let wo = WorkOrderBuilder::new("hello").build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.turn_history.is_empty());
}

#[test]
fn request_to_work_order_extracts_task() {
    let req = CopilotRequestBuilder::new()
        .model("gpt-4o")
        .messages(vec![Message::user("Explain recursion")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Explain recursion");
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn request_to_work_order_uses_last_user_message() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![
            Message::system("system prompt"),
            Message::user("first"),
            Message::assistant("reply"),
            Message::user("second question"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "second question");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Copilot function calling / tool_use mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn function_call_response_maps_to_tool_call_event() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: Some(CopilotFunctionCall {
            name: "read_file".into(),
            arguments: r#"{"path":"src/main.rs"}"#.into(),
            id: Some("call_001".into()),
        }),
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("call_001"));
            assert_eq!(input["path"], "src/main.rs");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn function_call_with_invalid_json_args_becomes_string() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: Some(CopilotFunctionCall {
            name: "do_thing".into(),
            arguments: "not valid json".into(),
            id: None,
        }),
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input.as_str(), Some("not valid json"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn canonical_tool_def_to_copilot_roundtrip() {
    let def = CanonicalToolDef {
        name: "search_files".into(),
        description: "Search files by pattern".into(),
        parameters_schema: json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"}
            }
        }),
    };
    let copilot_tool = dialect::tool_def_to_copilot(&def);
    assert_eq!(copilot_tool.tool_type, CopilotToolType::Function);
    assert!(copilot_tool.function.is_some());
    assert!(copilot_tool.confirmation.is_none());

    let back = dialect::tool_def_from_copilot(&copilot_tool).unwrap();
    assert_eq!(back.name, "search_files");
    assert_eq!(back.description, "Search files by pattern");
    assert_eq!(back.parameters_schema, def.parameters_schema);
}

#[test]
fn confirmation_tool_has_no_canonical_form() {
    let tool = make_confirmation_tool("c1", "Delete file?", "This will delete main.rs");
    let canonical = dialect::tool_def_from_copilot(&tool);
    assert!(canonical.is_none());
}

#[test]
fn function_call_no_id_maps_to_none_tool_use_id() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: Some(CopilotFunctionCall {
            name: "run".into(),
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
fn receipt_to_response_preserves_function_call() {
    let events = vec![make_agent_event(AgentEventKind::ToolCall {
        tool_name: "write_file".into(),
        tool_use_id: Some("tc_42".into()),
        parent_tool_use_id: None,
        input: json!({"path": "out.txt", "content": "hello"}),
    })];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let fc = resp.function_call.unwrap();
    assert_eq!(fc.name, "write_file");
    assert_eq!(fc.id.as_deref(), Some("tc_42"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Copilot streaming response mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_text_delta_maps_to_assistant_delta() {
    let event = CopilotStreamEvent::TextDelta {
        text: "Hello".into(),
    };
    let mapped = dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    match &mapped[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn stream_function_call_maps_to_tool_call() {
    let event = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "bash".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
            id: Some("fc_1".into()),
        },
    };
    let mapped = dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    match &mapped[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            ..
        } => {
            assert_eq!(tool_name, "bash");
            assert_eq!(tool_use_id.as_deref(), Some("fc_1"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn stream_done_maps_to_run_completed() {
    let event = CopilotStreamEvent::Done {};
    let mapped = dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    assert!(matches!(
        &mapped[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn stream_references_maps_to_run_started() {
    let event = CopilotStreamEvent::CopilotReferences {
        references: vec![file_ref("f0", "lib.rs")],
    };
    let mapped = dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    match &mapped[0].kind {
        AgentEventKind::RunStarted { message } => {
            assert!(message.contains("1 reference"));
        }
        other => panic!("expected RunStarted, got {other:?}"),
    }
    assert!(mapped[0].ext.is_some());
}

#[test]
fn stream_empty_references_maps_to_empty() {
    let event = CopilotStreamEvent::CopilotReferences {
        references: vec![],
    };
    let mapped = dialect::map_stream_event(&event);
    assert!(mapped.is_empty());
}

#[test]
fn stream_errors_maps_to_error_events() {
    let event = CopilotStreamEvent::CopilotErrors {
        errors: vec![
            CopilotError {
                error_type: "rate_limit".into(),
                message: "Too many requests".into(),
                code: Some("429".into()),
                identifier: None,
            },
            CopilotError {
                error_type: "internal".into(),
                message: "Server error".into(),
                code: None,
                identifier: None,
            },
        ],
    };
    let mapped = dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 2);
    for m in &mapped {
        assert!(matches!(&m.kind, AgentEventKind::Error { .. }));
    }
}

#[test]
fn stream_confirmation_maps_to_warning_with_ext() {
    let event = CopilotStreamEvent::CopilotConfirmation {
        confirmation: CopilotConfirmation {
            id: "c1".into(),
            title: "Approve deletion".into(),
            message: "Delete file?".into(),
            accepted: None,
        },
    };
    let mapped = dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    match &mapped[0].kind {
        AgentEventKind::Warning { message } => {
            assert!(message.contains("Approve deletion"));
        }
        other => panic!("expected Warning, got {other:?}"),
    }
    let ext = mapped[0].ext.as_ref().unwrap();
    assert!(ext.contains_key("copilot_confirmation"));
}

#[test]
fn events_to_stream_events_produces_references_and_done() {
    let events = vec![make_agent_event(AgentEventKind::AssistantMessage {
        text: "Hello!".into(),
    })];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(stream.len() >= 3);
    assert!(matches!(
        &stream[0],
        CopilotStreamEvent::CopilotReferences { .. }
    ));
    assert!(matches!(
        &stream[stream.len() - 1],
        CopilotStreamEvent::Done {}
    ));
}

#[test]
fn events_to_stream_events_maps_tool_call() {
    let events = vec![make_agent_event(AgentEventKind::ToolCall {
        tool_name: "edit_file".into(),
        tool_use_id: Some("t1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "a.rs"}),
    })];
    let stream = events_to_stream_events(&events, "gpt-4o");
    let fc = stream
        .iter()
        .find(|e| matches!(e, CopilotStreamEvent::FunctionCall { .. }));
    assert!(fc.is_some());
}

#[test]
fn events_to_stream_events_maps_errors() {
    let events = vec![make_agent_event(AgentEventKind::Error {
        message: "backend failure".into(),
        error_code: None,
    })];
    let stream = events_to_stream_events(&events, "gpt-4o");
    let err = stream
        .iter()
        .find(|e| matches!(e, CopilotStreamEvent::CopilotErrors { .. }));
    assert!(err.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Copilot model names capability mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn known_models_are_recognized() {
    let models = [
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
    for m in models {
        assert!(dialect::is_known_model(m), "expected {m} to be known");
    }
}

#[test]
fn unknown_model_not_recognized() {
    assert!(!dialect::is_known_model("llama-70b"));
    assert!(!dialect::is_known_model("gemini-pro"));
    assert!(!dialect::is_known_model(""));
}

#[test]
fn canonical_model_roundtrip() {
    let canonical = dialect::to_canonical_model("gpt-4o");
    assert_eq!(canonical, "copilot/gpt-4o");
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, "gpt-4o");
}

#[test]
fn from_canonical_model_no_prefix() {
    let back = dialect::from_canonical_model("gpt-4o");
    assert_eq!(back, "gpt-4o");
}

#[test]
fn capability_manifest_has_streaming_native() {
    let manifest = dialect::capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::Streaming),
        Some(CoreSupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_emulated_tools() {
    let manifest = dialect::capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::ToolRead),
        Some(CoreSupportLevel::Emulated)
    ));
    assert!(matches!(
        manifest.get(&Capability::ToolWrite),
        Some(CoreSupportLevel::Emulated)
    ));
    assert!(matches!(
        manifest.get(&Capability::ToolEdit),
        Some(CoreSupportLevel::Emulated)
    ));
    assert!(matches!(
        manifest.get(&Capability::ToolBash),
        Some(CoreSupportLevel::Emulated)
    ));
}

#[test]
fn capability_manifest_unsupported_glob_grep() {
    let manifest = dialect::capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::ToolGlob),
        Some(CoreSupportLevel::Unsupported)
    ));
    assert!(matches!(
        manifest.get(&Capability::ToolGrep),
        Some(CoreSupportLevel::Unsupported)
    ));
}

#[test]
fn capability_manifest_web_search_native() {
    let manifest = dialect::capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::ToolWebSearch),
        Some(CoreSupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_mcp_unsupported() {
    let manifest = dialect::capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::McpClient),
        Some(CoreSupportLevel::Unsupported)
    ));
    assert!(matches!(
        manifest.get(&Capability::McpServer),
        Some(CoreSupportLevel::Unsupported)
    ));
}

#[test]
fn capability_negotiation_streaming() {
    let manifest = dialect::capability_manifest();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native, vec![Capability::Streaming]);
}

#[test]
fn capability_negotiation_emulated_tool() {
    let manifest = dialect::capability_manifest();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.emulatable, vec![Capability::ToolRead]);
}

#[test]
fn capability_negotiation_native_only_emulated_goes_to_emulatable() {
    let manifest = dialect::capability_manifest();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let result = negotiate(&manifest, &reqs);
    // negotiate classifies by manifest level, not min_support;
    // ToolRead is Emulated in the manifest, so it lands in emulatable
    assert!(result.is_compatible());
    assert_eq!(result.emulatable, vec![Capability::ToolRead]);
    assert!(result.native.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Copilot-specific features (code suggestions, inline completions)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_reference_types_serde_roundtrip() {
    let refs = vec![
        file_ref("f1", "src/main.rs"),
        snippet_ref("s1", "test.rs", "fn test() {}"),
        repo_ref("r1", "octocat", "hello"),
        web_ref("w1", "https://example.com"),
    ];
    for r in &refs {
        let json = serde_json::to_string(r).unwrap();
        let back: CopilotReference = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, r);
    }
}

#[test]
fn copilot_reference_with_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("label".into(), json!("Main file"));
    meta.insert("language".into(), json!("rust"));
    let r = CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "f1".into(),
        data: json!({"path": "main.rs"}),
        metadata: Some(meta),
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(back.metadata.unwrap().len(), 2);
}

#[test]
fn copilot_confirmation_serde() {
    let conf = CopilotConfirmation {
        id: "conf_1".into(),
        title: "Delete file?".into(),
        message: "This will remove important.rs".into(),
        accepted: Some(true),
    };
    let json = serde_json::to_string(&conf).unwrap();
    let back: CopilotConfirmation = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "conf_1");
    assert_eq!(back.accepted, Some(true));
}

#[test]
fn copilot_confirmation_pending() {
    let conf = CopilotConfirmation {
        id: "conf_2".into(),
        title: "Install dependency?".into(),
        message: "pip install requests".into(),
        accepted: None,
    };
    let json = serde_json::to_string(&conf).unwrap();
    assert!(!json.contains("accepted")); // skip_serializing_if None
}

#[test]
fn copilot_turn_entry_serde() {
    let entry = CopilotTurnEntry {
        request: "How do I sort?".into(),
        response: "Use .sort() method.".into(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: CopilotTurnEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back.request, "How do I sort?");
    assert_eq!(back.response, "Use .sort() method.");
}

#[test]
fn copilot_message_with_name() {
    let msg = CopilotMessage {
        role: "user".into(),
        content: "hello".into(),
        name: Some("alice".into()),
        copilot_references: vec![],
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: CopilotMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name.as_deref(), Some("alice"));
}

#[test]
fn copilot_config_default() {
    let cfg = CopilotConfig::default();
    assert_eq!(cfg.model, "gpt-4o");
    assert!(cfg.base_url.contains("githubcopilot"));
    assert!(cfg.token.is_empty());
    assert!(cfg.system_prompt.is_none());
}

#[test]
fn dialect_version_constant() {
    assert_eq!(dialect::DIALECT_VERSION, "copilot/v0.1");
}

#[test]
fn default_model_constant() {
    assert_eq!(dialect::DEFAULT_MODEL, "gpt-4o");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Copilot agent mode vs inline mode mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn agent_mode_detection_via_field() {
    let detector = DialectDetector::new();
    // agent_mode alone scores 0.45 for Copilot
    let msg = json!({
        "agent_mode": true,
        "references": [{"type": "file", "id": "f1", "data": {}}],
        "messages": [{"role": "user", "content": "fix bug"}]
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Copilot);
    assert!(result.confidence >= 0.45);
}

#[test]
fn references_field_triggers_copilot_detection() {
    let detector = DialectDetector::new();
    let msg = json!({
        "references": [{"type": "file", "id": "f1", "data": {"path": "a.rs"}}],
        "messages": [{"role": "user", "content": "check this"}]
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Copilot);
}

#[test]
fn confirmations_field_triggers_copilot_detection() {
    let detector = DialectDetector::new();
    let msg = json!({
        "confirmations": [{"id": "c1", "title": "ok?", "message": "proceed?"}]
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Copilot);
}

#[test]
fn copilot_without_markers_may_detect_as_openai() {
    let detector = DialectDetector::new();
    let msg = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let result = detector.detect(&msg).unwrap();
    // Without Copilot-specific fields, falls back to OpenAI
    assert_eq!(result.dialect, Dialect::OpenAi);
}

#[test]
fn detect_all_returns_copilot_with_agent_mode() {
    let detector = DialectDetector::new();
    let msg = json!({
        "agent_mode": true,
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "help"}]
    });
    let results = detector.detect_all(&msg);
    let copilot = results.iter().find(|r| r.dialect == Dialect::Copilot);
    assert!(copilot.is_some());
}

#[test]
fn copilot_validator_valid_messages() {
    let validator = DialectValidator::new();
    let msg = json!({
        "messages": [
            {"role": "system", "content": "you are helpful"},
            {"role": "user", "content": "hi"}
        ]
    });
    let result = validator.validate(&msg, Dialect::Copilot);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn copilot_validator_missing_role() {
    let validator = DialectValidator::new();
    let msg = json!({
        "messages": [{"content": "no role here"}]
    });
    let result = validator.validate(&msg, Dialect::Copilot);
    assert!(!result.valid);
    assert!(!result.errors.is_empty());
}

#[test]
fn copilot_validator_non_object_fails() {
    let validator = DialectValidator::new();
    let result = validator.validate(&json!([1, 2, 3]), Dialect::Copilot);
    assert!(!result.valid);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Copilot→OpenAI compatibility (Copilot extends OpenAI API)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_to_openai_tool_use_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Copilot, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn copilot_to_openai_streaming_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Copilot, Dialect::OpenAi, features::STREAMING)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn openai_to_copilot_tool_use_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Copilot, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn openai_to_copilot_streaming_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Copilot, features::STREAMING)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn copilot_to_claude_tool_use_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Copilot, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn copilot_to_gemini_tool_use_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Copilot, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn copilot_to_codex_tool_use_is_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Copilot, Dialect::Codex, features::TOOL_USE)
        .unwrap();
    assert!(matches!(rule.fidelity, Fidelity::LossyLabeled { .. }));
}

#[test]
fn copilot_self_mapping_lossless_for_all_features() {
    let reg = known_rules();
    let feats = [
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ];
    for f in feats {
        let rule = reg.lookup(Dialect::Copilot, Dialect::Copilot, f).unwrap();
        assert!(rule.fidelity.is_lossless(), "self-mapping for {f} not lossless");
    }
}

#[test]
fn copilot_thinking_is_lossy_cross_dialect() {
    let reg = known_rules();
    for &target in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let rule = reg
            .lookup(Dialect::Copilot, target, features::THINKING)
            .unwrap();
        assert!(
            matches!(rule.fidelity, Fidelity::LossyLabeled { .. }),
            "Copilot→{target:?} thinking should be lossy"
        );
    }
}

#[test]
fn copilot_image_input_unsupported() {
    let reg = known_rules();
    for &target in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let rule = reg
            .lookup(Dialect::Copilot, target, features::IMAGE_INPUT)
            .unwrap();
        assert!(
            rule.fidelity.is_unsupported(),
            "Copilot→{target:?} image_input should be unsupported"
        );
    }
}

#[test]
fn copilot_code_exec_lossy_cross_dialect() {
    let reg = known_rules();
    for &target in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let rule = reg
            .lookup(Dialect::Copilot, target, features::CODE_EXEC)
            .unwrap();
        assert!(
            matches!(rule.fidelity, Fidelity::LossyLabeled { .. }),
            "Copilot→{target:?} code_exec should be lossy"
        );
    }
}

#[test]
fn mapping_matrix_copilot_to_openai_supported() {
    let reg = known_rules();
    let matrix = MappingMatrix::from_registry(&reg);
    assert!(matrix.is_supported(Dialect::Copilot, Dialect::OpenAi));
    assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Copilot));
}

#[test]
fn rank_targets_from_copilot() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::Copilot, &[features::TOOL_USE, features::STREAMING]);
    assert!(!ranked.is_empty());
    // OpenAI should rank highly (both lossless)
    let openai_rank = ranked.iter().find(|(d, _)| *d == Dialect::OpenAi);
    assert!(openai_rank.is_some());
    assert_eq!(openai_rank.unwrap().1, 2); // both lossless
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Copilot error handling patterns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_response_maps_to_error_events() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![CopilotError {
            error_type: "rate_limit".into(),
            message: "Too many requests".into(),
            code: Some("429".into()),
            identifier: Some("err_abc".into()),
        }],
        copilot_confirmation: None,
        function_call: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert!(message.contains("rate_limit"));
            assert!(message.contains("Too many requests"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn multiple_errors_produce_multiple_events() {
    let resp = CopilotResponse {
        message: "partial".into(),
        copilot_references: vec![],
        copilot_errors: vec![
            CopilotError {
                error_type: "auth".into(),
                message: "Unauthorized".into(),
                code: None,
                identifier: None,
            },
            CopilotError {
                error_type: "quota".into(),
                message: "Quota exceeded".into(),
                code: None,
                identifier: None,
            },
        ],
        copilot_confirmation: None,
        function_call: None,
    };
    let events = dialect::map_response(&resp);
    // 1 message + 2 errors
    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0].kind, AgentEventKind::AssistantMessage { .. }));
    assert!(matches!(&events[1].kind, AgentEventKind::Error { .. }));
    assert!(matches!(&events[2].kind, AgentEventKind::Error { .. }));
}

#[test]
fn empty_message_not_emitted() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn copilot_error_serde_roundtrip() {
    let err = CopilotError {
        error_type: "internal_error".into(),
        message: "Something went wrong".into(),
        code: Some("500".into()),
        identifier: Some("err_xyz".into()),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: CopilotError = serde_json::from_str(&json).unwrap();
    assert_eq!(back, err);
}

#[test]
fn receipt_to_response_maps_errors() {
    let events = vec![make_agent_event(AgentEventKind::Error {
        message: "backend crashed".into(),
        error_code: None,
    })];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.copilot_errors.len(), 1);
    assert_eq!(resp.copilot_errors[0].error_type, "backend_error");
    assert!(resp.copilot_errors[0].message.contains("backend crashed"));
}

#[test]
fn mapping_error_feature_unsupported() {
    let err = MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Copilot,
        to: Dialect::Claude,
    };
    assert!(err.to_string().contains("logprobs"));
    assert!(err.to_string().contains("Copilot"));
}

#[test]
fn mapping_error_dialect_mismatch() {
    let err = MappingError::DialectMismatch {
        from: Dialect::Copilot,
        to: Dialect::Gemini,
    };
    assert!(err.to_string().contains("Copilot"));
    assert!(err.to_string().contains("Gemini"));
}

#[test]
fn validate_mapping_copilot_to_openai_tool_use() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Copilot,
        Dialect::OpenAi,
        &["tool_use".into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_lossless());
    assert!(results[0].errors.is_empty());
}

#[test]
fn validate_mapping_copilot_unknown_feature() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Copilot,
        Dialect::OpenAi,
        &["nonexistent_feature".into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert!(!results[0].errors.is_empty());
}

#[test]
fn validate_mapping_empty_feature_name() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Copilot,
        Dialect::OpenAi,
        &[String::new()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert!(
        results[0]
            .errors
            .iter()
            .any(|e| matches!(e, MappingError::InvalidInput { .. }))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Copilot workspace context integration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_with_workspace_root() {
    let wo = WorkOrderBuilder::new("task")
        .root("/home/user/project")
        .build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages[0].content, "task");
    // Workspace root is on the WorkOrder, not the request
    assert_eq!(wo.workspace.root, "/home/user/project");
}

#[test]
fn work_order_context_files_become_references() {
    let ctx = ContextPacket {
        files: vec!["src/lib.rs".into(), "src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("review").context(ctx).build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);

    assert_eq!(req.references.len(), 3);
    for (i, r) in req.references.iter().enumerate() {
        assert_eq!(r.ref_type, CopilotReferenceType::File);
        assert_eq!(r.id, format!("file-{i}"));
    }
}

#[test]
fn work_order_multiple_snippets() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![
            ContextSnippet {
                name: "util.rs".into(),
                content: "pub fn add(a: i32, b: i32) -> i32 { a + b }".into(),
            },
            ContextSnippet {
                name: "test.rs".into(),
                content: "#[test]\nfn it_works() { assert_eq!(2+2, 4); }".into(),
            },
        ],
    };
    let wo = WorkOrderBuilder::new("optimize").context(ctx).build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);

    assert_eq!(req.references.len(), 2);
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::Snippet);
    assert_eq!(req.references[1].ref_type, CopilotReferenceType::Snippet);
}

#[test]
fn lowering_user_text_roundtrip() {
    let msgs = vec![copilot_msg("user", "Hello from workspace")];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello from workspace");

    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "Hello from workspace");
}

#[test]
fn lowering_system_message_roundtrip() {
    let msgs = vec![copilot_msg("system", "You are a Rust expert")];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::System);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "system");
}

#[test]
fn lowering_references_preserved() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "check file".into(),
        name: None,
        copilot_references: vec![file_ref("f1", "lib.rs")],
    }];
    let conv = lowering::to_ir(&msgs);
    assert!(conv.messages[0].metadata.contains_key("copilot_references"));

    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].copilot_references.len(), 1);
    assert_eq!(back[0].copilot_references[0].id, "f1");
}

#[test]
fn lowering_name_preserved() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "hi".into(),
        name: Some("dev_alice".into()),
        copilot_references: vec![],
    }];
    let conv = lowering::to_ir(&msgs);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].name.as_deref(), Some("dev_alice"));
}

#[test]
fn lowering_multi_turn() {
    let msgs = vec![
        copilot_msg("system", "Be concise"),
        copilot_msg("user", "Hi"),
        copilot_msg("assistant", "Hello!"),
        copilot_msg("user", "Bye"),
    ];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 4);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 4);
    assert_eq!(back[3].content, "Bye");
}

#[test]
fn lowering_extract_references_across_messages() {
    let msgs = vec![
        CopilotMessage {
            role: "user".into(),
            content: "msg1".into(),
            name: None,
            copilot_references: vec![file_ref("f1", "a.rs")],
        },
        CopilotMessage {
            role: "user".into(),
            content: "msg2".into(),
            name: None,
            copilot_references: vec![repo_ref("r1", "org", "repo")],
        },
    ];
    let conv = lowering::to_ir(&msgs);
    let all_refs = lowering::extract_references(&conv);
    assert_eq!(all_refs.len(), 2);
}

#[test]
fn lowering_unknown_role_defaults_to_user() {
    let msgs = vec![copilot_msg("developer", "hello")];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn lowering_empty_content() {
    let msgs = vec![copilot_msg("user", "")];
    let conv = lowering::to_ir(&msgs);
    assert!(conv.messages[0].content.is_empty());
    let back = lowering::from_ir(&conv);
    assert!(back[0].content.is_empty());
}

#[test]
fn lowering_tool_role_becomes_user() {
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
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "user");
}

#[test]
fn lowering_thinking_block_becomes_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "reasoning...".into(),
        }],
    )]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content, "reasoning...");
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Copilot tool definitions and execution
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn function_tool_to_canonical_roundtrip() {
    let tool = make_function_tool("read_file", "Read a file from disk");
    let canonical = dialect::tool_def_from_copilot(&tool).unwrap();
    assert_eq!(canonical.name, "read_file");
    assert_eq!(canonical.description, "Read a file from disk");

    let back = dialect::tool_def_to_copilot(&canonical);
    assert_eq!(back.tool_type, CopilotToolType::Function);
    let func = back.function.unwrap();
    assert_eq!(func.name, "read_file");
}

#[test]
fn confirmation_tool_type() {
    let tool = make_confirmation_tool("c1", "Delete?", "Are you sure?");
    assert_eq!(tool.tool_type, CopilotToolType::Confirmation);
    assert!(tool.function.is_none());
    assert!(tool.confirmation.is_some());
}

#[test]
fn tool_def_with_complex_schema() {
    let def = CanonicalToolDef {
        name: "edit_file".into(),
        description: "Edit a file".into(),
        parameters_schema: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"},
                "old_text": {"type": "string"},
                "new_text": {"type": "string"}
            },
            "required": ["path", "old_text", "new_text"]
        }),
    };
    let copilot_tool = dialect::tool_def_to_copilot(&def);
    let func = copilot_tool.function.unwrap();
    assert_eq!(func.parameters["required"].as_array().unwrap().len(), 3);
}

#[test]
fn tool_serde_roundtrip_function() {
    let tool = make_function_tool("bash", "Run a shell command");
    let json = serde_json::to_string(&tool).unwrap();
    let back: CopilotTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tool);
}

#[test]
fn tool_serde_roundtrip_confirmation() {
    let tool = make_confirmation_tool("c2", "Install?", "Install package?");
    let json = serde_json::to_string(&tool).unwrap();
    let back: CopilotTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tool);
}

#[test]
fn request_with_tools() {
    let req = CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![copilot_msg("user", "help me")],
        tools: Some(vec![
            make_function_tool("read_file", "Read file"),
            make_function_tool("write_file", "Write file"),
        ]),
        turn_history: vec![],
        references: vec![],
    };
    assert_eq!(req.tools.as_ref().unwrap().len(), 2);
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("read_file"));
    assert!(json.contains("write_file"));
}

#[test]
fn function_call_with_json_object_args() {
    let fc = CopilotFunctionCall {
        name: "search".into(),
        arguments: r#"{"query": "hello", "limit": 10}"#.into(),
        id: Some("fc_001".into()),
    };
    let args: serde_json::Value = serde_json::from_str(&fc.arguments).unwrap();
    assert_eq!(args["query"], "hello");
    assert_eq!(args["limit"], 10);
}

// ═══════════════════════════════════════════════════════════════════════════
// Passthrough fidelity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_text_delta_roundtrip() {
    let event = CopilotStreamEvent::TextDelta {
        text: "code snippet".into(),
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let ext = wrapped.ext.as_ref().unwrap();
    assert_eq!(ext.get("dialect").unwrap().as_str(), Some("copilot"));
    assert!(ext.contains_key("raw_message"));

    let restored = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(restored, event);
}

#[test]
fn passthrough_function_call_roundtrip() {
    let event = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "grep".into(),
            arguments: r#"{"pattern":"TODO"}"#.into(),
            id: Some("fc_99".into()),
        },
    };
    assert!(dialect::verify_passthrough_fidelity(&[event.clone()]));
}

#[test]
fn passthrough_done_roundtrip() {
    let event = CopilotStreamEvent::Done {};
    assert!(dialect::verify_passthrough_fidelity(&[event]));
}

#[test]
fn passthrough_errors_roundtrip() {
    let event = CopilotStreamEvent::CopilotErrors {
        errors: vec![CopilotError {
            error_type: "timeout".into(),
            message: "Request timed out".into(),
            code: Some("504".into()),
            identifier: None,
        }],
    };
    assert!(dialect::verify_passthrough_fidelity(&[event]));
}

#[test]
fn passthrough_full_stream_roundtrip() {
    let events = vec![
        CopilotStreamEvent::CopilotReferences {
            references: vec![file_ref("f1", "main.rs")],
        },
        CopilotStreamEvent::TextDelta {
            text: "Hello ".into(),
        },
        CopilotStreamEvent::TextDelta {
            text: "world!".into(),
        },
        CopilotStreamEvent::Done {},
    ];
    assert!(dialect::verify_passthrough_fidelity(&events));
}

#[test]
fn passthrough_confirmation_roundtrip() {
    let event = CopilotStreamEvent::CopilotConfirmation {
        confirmation: CopilotConfirmation {
            id: "c99".into(),
            title: "Format code?".into(),
            message: "Run rustfmt on all files".into(),
            accepted: Some(false),
        },
    };
    assert!(dialect::verify_passthrough_fidelity(&[event]));
}

// ═══════════════════════════════════════════════════════════════════════════
// Shim integration (messages_to_ir, ir_to_messages)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn shim_messages_to_ir_roundtrip() {
    let msgs = vec![
        Message::system("Be helpful"),
        Message::user("What is Rust?"),
        Message::assistant("Rust is a systems language."),
    ];
    let conv = messages_to_ir(&msgs);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);

    let back = ir_to_messages(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[1].content, "What is Rust?");
}

#[test]
fn shim_user_with_refs() {
    let refs = vec![file_ref("f1", "src/lib.rs")];
    let msg = Message::user_with_refs("Check this", refs);
    assert_eq!(msg.copilot_references.len(), 1);
}

#[test]
fn receipt_with_copilot_capabilities() {
    let manifest = dialect::capability_manifest();
    let receipt = ReceiptBuilder::new("sidecar:copilot")
        .outcome(Outcome::Complete)
        .capabilities(manifest.clone())
        .build();

    assert_eq!(receipt.backend.id, "sidecar:copilot");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.capabilities.contains_key(&Capability::Streaming));
}

#[test]
fn receipt_to_response_with_assistant_message() {
    let events = vec![make_agent_event(AgentEventKind::AssistantMessage {
        text: "Here is the refactored code.".into(),
    })];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Here is the refactored code.");
    assert!(resp.copilot_errors.is_empty());
    assert!(resp.function_call.is_none());
}

#[test]
fn receipt_to_response_with_deltas_concatenated() {
    let events = vec![
        make_agent_event(AgentEventKind::AssistantDelta {
            text: "Hello ".into(),
        }),
        make_agent_event(AgentEventKind::AssistantDelta {
            text: "world!".into(),
        }),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Hello world!");
}

#[test]
fn copilot_response_serde_deterministic() {
    let resp = CopilotResponse {
        message: "done".into(),
        copilot_references: vec![file_ref("f1", "a.rs")],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let json1 = serde_json::to_string(&resp).unwrap();
    let json2 = serde_json::to_string(&resp).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn copilot_request_serde_roundtrip() {
    let req = CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![copilot_msg("user", "test")],
        tools: None,
        turn_history: vec![],
        references: vec![],
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CopilotRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "gpt-4o");
    assert_eq!(back.messages.len(), 1);
}

#[test]
fn stream_event_serde_text_delta() {
    let event = CopilotStreamEvent::TextDelta {
        text: "chunk".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn stream_event_serde_done() {
    let event = CopilotStreamEvent::Done {};
    let json = serde_json::to_string(&event).unwrap();
    let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn contract_version_in_receipt() {
    let receipt = ReceiptBuilder::new("sidecar:copilot")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn copilot_dialect_label() {
    assert_eq!(Dialect::Copilot.label(), "Copilot");
    assert_eq!(Dialect::Copilot.to_string(), "Copilot");
}

#[test]
fn copilot_in_all_dialects() {
    let all = Dialect::all();
    assert!(all.contains(&Dialect::Copilot));
}

#[test]
fn copilot_dialect_serde_roundtrip() {
    let d = Dialect::Copilot;
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(json, r#""copilot""#);
    let back: Dialect = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Dialect::Copilot);
}

#[test]
fn usage_normalized_with_request_units() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: Some(1),
        estimated_cost_usd: None,
    };
    let receipt = mock_receipt_with_usage(vec![], usage.clone());
    assert_eq!(receipt.usage.request_units, Some(1));
    assert_eq!(receipt.usage.input_tokens, Some(100));
}
