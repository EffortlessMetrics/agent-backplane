// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the `abp-copilot-sdk` crate.

use abp_copilot_sdk::dialect::*;
use abp_core::{AgentEventKind, ContextPacket, ContextSnippet, WorkOrderBuilder};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Config tests
// ---------------------------------------------------------------------------

#[test]
fn default_config_has_github_copilot_base_url() {
    let cfg = CopilotConfig::default();
    assert!(cfg.base_url.contains("githubcopilot"));
}

#[test]
fn default_config_model_is_gpt4o() {
    let cfg = CopilotConfig::default();
    assert_eq!(cfg.model, "gpt-4o");
}

#[test]
fn default_config_token_is_empty() {
    let cfg = CopilotConfig::default();
    assert!(cfg.token.is_empty());
}

// ---------------------------------------------------------------------------
// Model mapping tests
// ---------------------------------------------------------------------------

#[test]
fn to_canonical_model_adds_prefix() {
    assert_eq!(to_canonical_model("gpt-4o"), "copilot/gpt-4o");
}

#[test]
fn from_canonical_model_strips_prefix() {
    assert_eq!(from_canonical_model("copilot/gpt-4o"), "gpt-4o");
}

#[test]
fn from_canonical_model_no_prefix_passthrough() {
    assert_eq!(from_canonical_model("gpt-4o"), "gpt-4o");
}

#[test]
fn is_known_model_recognizes_gpt4o() {
    assert!(is_known_model("gpt-4o"));
    assert!(is_known_model("o3-mini"));
    assert!(!is_known_model("unknown-model"));
}

// ---------------------------------------------------------------------------
// Capability manifest tests
// ---------------------------------------------------------------------------

#[test]
fn capability_manifest_includes_streaming() {
    use abp_core::Capability;
    let manifest = capability_manifest();
    assert!(manifest.contains_key(&Capability::Streaming));
}

// ---------------------------------------------------------------------------
// Reference types tests
// ---------------------------------------------------------------------------

#[test]
fn copilot_reference_serializes_correctly() {
    let reference = CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "file-0".into(),
        data: serde_json::json!({"path": "src/main.rs"}),
        metadata: None,
    };
    let json = serde_json::to_value(&reference).unwrap();
    assert_eq!(json["type"], "file");
    assert_eq!(json["id"], "file-0");
}

#[test]
fn copilot_reference_roundtrips_json() {
    let reference = CopilotReference {
        ref_type: CopilotReferenceType::Snippet,
        id: "snippet-0".into(),
        data: serde_json::json!({"name": "test", "content": "fn main() {}"}),
        metadata: Some(BTreeMap::new()),
    };
    let json = serde_json::to_string(&reference).unwrap();
    let restored: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(reference, restored);
}

#[test]
fn copilot_reference_all_types_serialize() {
    for (ref_type, expected) in [
        (CopilotReferenceType::File, "file"),
        (CopilotReferenceType::Snippet, "snippet"),
        (CopilotReferenceType::Repository, "repository"),
        (CopilotReferenceType::WebSearchResult, "web_search_result"),
    ] {
        let val = serde_json::to_value(&ref_type).unwrap();
        assert_eq!(val.as_str().unwrap(), expected);
    }
}

// ---------------------------------------------------------------------------
// Tool types tests
// ---------------------------------------------------------------------------

#[test]
fn tool_def_to_copilot_creates_function_tool() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: serde_json::json!({"type": "object"}),
    };
    let tool = tool_def_to_copilot(&canonical);
    assert_eq!(tool.tool_type, CopilotToolType::Function);
    assert!(tool.function.is_some());
    assert!(tool.confirmation.is_none());
}

#[test]
fn tool_def_from_copilot_roundtrips() {
    let canonical = CanonicalToolDef {
        name: "write_file".into(),
        description: "Write a file".into(),
        parameters_schema: serde_json::json!({"type": "object", "properties": {}}),
    };
    let tool = tool_def_to_copilot(&canonical);
    let restored = tool_def_from_copilot(&tool).unwrap();
    assert_eq!(canonical, restored);
}

#[test]
fn tool_def_from_copilot_returns_none_for_confirmation_tool() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Confirmation,
        function: None,
        confirmation: Some(CopilotConfirmation {
            id: "conf-1".into(),
            title: "Delete files?".into(),
            message: "This will delete all files.".into(),
            accepted: None,
        }),
    };
    assert!(tool_def_from_copilot(&tool).is_none());
}

// ---------------------------------------------------------------------------
// Message types tests
// ---------------------------------------------------------------------------

#[test]
fn copilot_message_serializes_with_references() {
    let msg = CopilotMessage {
        role: "user".into(),
        content: "Help me".into(),
        name: Some("testuser".into()),
        copilot_references: vec![CopilotReference {
            ref_type: CopilotReferenceType::Repository,
            id: "repo-0".into(),
            data: serde_json::json!({"owner": "octocat", "name": "hello-world"}),
            metadata: None,
        }],
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "user");
    assert_eq!(json["name"], "testuser");
    assert_eq!(json["copilot_references"].as_array().unwrap().len(), 1);
}

#[test]
fn copilot_message_omits_empty_references() {
    let msg = CopilotMessage {
        role: "assistant".into(),
        content: "Sure!".into(),
        name: None,
        copilot_references: vec![],
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert!(json.get("copilot_references").is_none());
}

// ---------------------------------------------------------------------------
// map_work_order tests
// ---------------------------------------------------------------------------

#[test]
fn map_work_order_populates_file_references() {
    let ctx = ContextPacket {
        files: vec!["src/lib.rs".into(), "Cargo.toml".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("Fix bug").context(ctx).build();
    let cfg = CopilotConfig::default();
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.references.len(), 2);
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::File);
    assert_eq!(req.references[0].data["path"], "src/lib.rs");
    assert_eq!(req.references[1].data["path"], "Cargo.toml");
}

#[test]
fn map_work_order_populates_snippet_references() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "error_handler".into(),
            content: "fn handle() {}".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("Improve handler")
        .context(ctx)
        .build();
    let cfg = CopilotConfig::default();
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.references.len(), 1);
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::Snippet);
    assert_eq!(req.references[0].data["name"], "error_handler");
}

#[test]
fn map_work_order_includes_system_prompt() {
    let cfg = CopilotConfig {
        system_prompt: Some("You are a helpful coding assistant.".into()),
        ..CopilotConfig::default()
    };
    let wo = WorkOrderBuilder::new("Help").build();
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, "system");
    assert_eq!(
        req.messages[0].content,
        "You are a helpful coding assistant."
    );
    assert_eq!(req.messages[1].role, "user");
}

#[test]
fn map_work_order_without_system_prompt_has_one_message() {
    let cfg = CopilotConfig::default();
    let wo = WorkOrderBuilder::new("Just a task").build();
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
}

// ---------------------------------------------------------------------------
// map_response tests
// ---------------------------------------------------------------------------

#[test]
fn map_response_empty_message_produces_no_assistant_event() {
    let resp = CopilotResponse {
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
fn map_response_errors_produce_error_events() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![
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
        copilot_confirmation: None,
        function_call: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 2);
    for event in &events {
        assert!(matches!(&event.kind, AgentEventKind::Error { .. }));
    }
}

#[test]
fn map_response_confirmation_produces_warning_with_ext() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: Some(CopilotConfirmation {
            id: "conf-1".into(),
            title: "Delete repo?".into(),
            message: "This action cannot be undone.".into(),
            accepted: None,
        }),
        function_call: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Warning { message } => {
            assert!(message.contains("Delete repo?"));
        }
        other => panic!("expected Warning, got {other:?}"),
    }
    assert!(events[0].ext.is_some());
    let ext = events[0].ext.as_ref().unwrap();
    assert!(ext.contains_key("copilot_confirmation"));
}

#[test]
fn map_response_function_call_with_json_args() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: Some(CopilotFunctionCall {
            name: "search".into(),
            arguments: r#"{"query": "rust async"}"#.into(),
            id: Some("call_abc".into()),
        }),
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "search");
            assert_eq!(input["query"], "rust async");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn map_response_function_call_with_malformed_args_falls_back() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: Some(CopilotFunctionCall {
            name: "broken".into(),
            arguments: "not valid json".into(),
            id: None,
        }),
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input.as_str().unwrap(), "not valid json");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// map_stream_event tests
// ---------------------------------------------------------------------------

#[test]
fn stream_text_delta_maps_to_assistant_delta() {
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
fn stream_function_call_maps_to_tool_call() {
    let event = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "edit_file".into(),
            arguments: r#"{"path": "test.rs"}"#.into(),
            id: Some("fc-1".into()),
        },
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            ..
        } => {
            assert_eq!(tool_name, "edit_file");
            assert_eq!(tool_use_id.as_deref(), Some("fc-1"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn stream_confirmation_maps_to_warning_with_ext() {
    let event = CopilotStreamEvent::CopilotConfirmation {
        confirmation: CopilotConfirmation {
            id: "c-1".into(),
            title: "Approve action?".into(),
            message: "Details here".into(),
            accepted: None,
        },
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::Warning { .. }));
    assert!(events[0].ext.is_some());
}

#[test]
fn stream_errors_map_to_error_events() {
    let event = CopilotStreamEvent::CopilotErrors {
        errors: vec![CopilotError {
            error_type: "auth".into(),
            message: "Unauthorized".into(),
            code: None,
            identifier: None,
        }],
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert!(message.contains("Unauthorized"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn stream_references_map_to_run_started() {
    let event = CopilotStreamEvent::CopilotReferences {
        references: vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "f-0".into(),
            data: serde_json::json!({"path": "lib.rs"}),
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
fn stream_empty_references_produces_no_events() {
    let event = CopilotStreamEvent::CopilotReferences { references: vec![] };
    let events = map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn stream_done_maps_to_run_completed() {
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

// ---------------------------------------------------------------------------
// Passthrough fidelity tests
// ---------------------------------------------------------------------------

#[test]
fn passthrough_roundtrip_text_delta() {
    let event = CopilotStreamEvent::TextDelta {
        text: "roundtrip".into(),
    };
    let wrapped = to_passthrough_event(&event);
    let ext = wrapped.ext.as_ref().unwrap();
    assert_eq!(ext["dialect"], "copilot");
    let restored = from_passthrough_event(&wrapped).unwrap();
    assert_eq!(event, restored);
}

#[test]
fn passthrough_roundtrip_done() {
    let event = CopilotStreamEvent::Done {};
    assert!(verify_passthrough_fidelity(&[event]));
}

#[test]
fn passthrough_fidelity_multiple_events() {
    let events = vec![
        CopilotStreamEvent::TextDelta {
            text: "hello".into(),
        },
        CopilotStreamEvent::FunctionCall {
            function_call: CopilotFunctionCall {
                name: "test".into(),
                arguments: "{}".into(),
                id: None,
            },
        },
        CopilotStreamEvent::Done {},
    ];
    assert!(verify_passthrough_fidelity(&events));
}

#[test]
fn from_passthrough_event_returns_none_without_ext() {
    use abp_core::AgentEvent;
    use chrono::Utc;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "test".into(),
        },
        ext: None,
    };
    assert!(from_passthrough_event(&event).is_none());
}

// ---------------------------------------------------------------------------
// Serialization roundtrip tests
// ---------------------------------------------------------------------------

#[test]
fn copilot_request_serializes_and_deserializes() {
    let req = CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![CopilotMessage {
            role: "user".into(),
            content: "Hello".into(),
            name: None,
            copilot_references: vec![],
        }],
        tools: None,
        turn_history: vec![],
        references: vec![],
    };
    let json = serde_json::to_string(&req).unwrap();
    let restored: CopilotRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.model, "gpt-4o");
    assert_eq!(restored.messages.len(), 1);
}

#[test]
fn copilot_stream_event_roundtrips_all_variants() {
    let variants: Vec<CopilotStreamEvent> = vec![
        CopilotStreamEvent::CopilotReferences { references: vec![] },
        CopilotStreamEvent::CopilotErrors {
            errors: vec![CopilotError {
                error_type: "test".into(),
                message: "msg".into(),
                code: None,
                identifier: None,
            }],
        },
        CopilotStreamEvent::TextDelta {
            text: "chunk".into(),
        },
        CopilotStreamEvent::FunctionCall {
            function_call: CopilotFunctionCall {
                name: "fn".into(),
                arguments: "{}".into(),
                id: Some("id-1".into()),
            },
        },
        CopilotStreamEvent::CopilotConfirmation {
            confirmation: CopilotConfirmation {
                id: "c-1".into(),
                title: "title".into(),
                message: "body".into(),
                accepted: Some(true),
            },
        },
        CopilotStreamEvent::Done {},
    ];
    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let restored: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, &restored);
    }
}

#[test]
fn copilot_error_serializes_with_optional_fields() {
    let err = CopilotError {
        error_type: "agent_error".into(),
        message: "Something failed".into(),
        code: Some("E001".into()),
        identifier: Some("id-abc".into()),
    };
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["type"], "agent_error");
    assert_eq!(json["code"], "E001");
    assert_eq!(json["identifier"], "id-abc");
}

#[test]
fn copilot_confirmation_accepted_field() {
    let conf = CopilotConfirmation {
        id: "c-2".into(),
        title: "Confirm?".into(),
        message: "Are you sure?".into(),
        accepted: Some(true),
    };
    let json = serde_json::to_value(&conf).unwrap();
    assert_eq!(json["accepted"], true);

    let pending = CopilotConfirmation {
        id: "c-3".into(),
        title: "Confirm?".into(),
        message: "Details".into(),
        accepted: None,
    };
    let json = serde_json::to_value(&pending).unwrap();
    assert!(json.get("accepted").is_none());
}
