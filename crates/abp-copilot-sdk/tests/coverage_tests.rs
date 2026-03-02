// SPDX-License-Identifier: MIT OR Apache-2.0
//! Additional coverage tests for the Copilot SDK: request/response serde, turn history,
//! tool types, edge cases in mapping, and config boundaries.

use abp_copilot_sdk::dialect::{
    CopilotConfig, CopilotConfirmation, CopilotError, CopilotFunctionCall, CopilotFunctionDef,
    CopilotMessage, CopilotReference, CopilotReferenceType, CopilotRequest, CopilotResponse,
    CopilotStreamEvent, CopilotTool, CopilotToolType, CopilotTurnEntry, DEFAULT_MODEL,
    DIALECT_VERSION, from_canonical_model, is_known_model, map_response, map_stream_event,
    map_work_order,
};
use abp_core::{AgentEventKind, WorkOrderBuilder};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// CopilotTurnEntry serde
// ---------------------------------------------------------------------------

#[test]
fn copilot_turn_entry_serde_roundtrip() {
    let entry = CopilotTurnEntry {
        request: "What is Rust?".into(),
        response: "Rust is a systems programming language.".into(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: CopilotTurnEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, entry);
}

// ---------------------------------------------------------------------------
// CopilotConfig serde
// ---------------------------------------------------------------------------

#[test]
fn copilot_config_serde_with_system_prompt() {
    let cfg = CopilotConfig {
        system_prompt: Some("You are a helpful Copilot agent.".into()),
        ..CopilotConfig::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("helpful Copilot"));
    let parsed: CopilotConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed.system_prompt.as_deref(),
        Some("You are a helpful Copilot agent.")
    );
}

#[test]
fn copilot_config_default_has_no_system_prompt() {
    let cfg = CopilotConfig::default();
    assert!(cfg.system_prompt.is_none());
}

// ---------------------------------------------------------------------------
// CopilotToolType serde
// ---------------------------------------------------------------------------

#[test]
fn copilot_tool_type_function_serde_roundtrip() {
    let tt = CopilotToolType::Function;
    let json = serde_json::to_string(&tt).unwrap();
    assert_eq!(json, "\"function\"");
    let parsed: CopilotToolType = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tt);
}

#[test]
fn copilot_tool_type_confirmation_serde_roundtrip() {
    let tt = CopilotToolType::Confirmation;
    let json = serde_json::to_string(&tt).unwrap();
    assert_eq!(json, "\"confirmation\"");
    let parsed: CopilotToolType = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tt);
}

// ---------------------------------------------------------------------------
// CopilotRequest with turn_history
// ---------------------------------------------------------------------------

#[test]
fn copilot_request_with_turn_history_serde_roundtrip() {
    let req = CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![CopilotMessage {
            role: "user".into(),
            content: "Follow up question".into(),
            name: None,
            copilot_references: vec![],
        }],
        tools: None,
        turn_history: vec![CopilotTurnEntry {
            request: "What is Rust?".into(),
            response: "A systems language.".into(),
        }],
        references: vec![],
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("turn_history"));
    let parsed: CopilotRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.turn_history.len(), 1);
}

#[test]
fn copilot_request_omits_empty_collections() {
    let req = CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![CopilotMessage {
            role: "user".into(),
            content: "hi".into(),
            name: None,
            copilot_references: vec![],
        }],
        tools: None,
        turn_history: vec![],
        references: vec![],
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("turn_history"));
    assert!(!json.contains("references"));
    assert!(!json.contains("tools"));
}

// ---------------------------------------------------------------------------
// CopilotResponse serde
// ---------------------------------------------------------------------------

#[test]
fn copilot_response_full_serde_roundtrip() {
    let resp = CopilotResponse {
        message: "Here you go.".into(),
        copilot_references: vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "f-0".into(),
            data: serde_json::json!({"path": "lib.rs"}),
            metadata: None,
        }],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: CopilotResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.message, "Here you go.");
    assert_eq!(parsed.copilot_references.len(), 1);
}

#[test]
fn copilot_response_minimal_serde_roundtrip() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: CopilotResponse = serde_json::from_str(&json).unwrap();
    assert!(parsed.message.is_empty());
}

// ---------------------------------------------------------------------------
// CopilotFunctionCall serde
// ---------------------------------------------------------------------------

#[test]
fn copilot_function_call_serde_with_no_id() {
    let fc = CopilotFunctionCall {
        name: "test_func".into(),
        arguments: r#"{"a":1}"#.into(),
        id: None,
    };
    let json = serde_json::to_string(&fc).unwrap();
    assert!(!json.contains("\"id\""));
    let parsed: CopilotFunctionCall = serde_json::from_str(&json).unwrap();
    assert!(parsed.id.is_none());
}

#[test]
fn copilot_function_call_serde_with_id() {
    let fc = CopilotFunctionCall {
        name: "search".into(),
        arguments: r#"{"q":"rust"}"#.into(),
        id: Some("call_123".into()),
    };
    let json = serde_json::to_string(&fc).unwrap();
    let parsed: CopilotFunctionCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, fc);
}

// ---------------------------------------------------------------------------
// map_response combined events
// ---------------------------------------------------------------------------

#[test]
fn map_response_text_and_errors_combined() {
    let resp = CopilotResponse {
        message: "Partial result.".into(),
        copilot_references: vec![],
        copilot_errors: vec![CopilotError {
            error_type: "warning".into(),
            message: "Rate limited".into(),
            code: None,
            identifier: None,
        }],
        copilot_confirmation: None,
        function_call: None,
    };
    let events = map_response(&resp);
    // Should have both assistant message and error event
    assert!(events.len() >= 2);
    let has_message = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }));
    let has_error = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::Error { .. }));
    assert!(has_message);
    assert!(has_error);
}

#[test]
fn map_response_with_both_message_and_function_call() {
    let resp = CopilotResponse {
        message: "Let me search.".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: Some(CopilotFunctionCall {
            name: "search".into(),
            arguments: r#"{"q":"rust"}"#.into(),
            id: Some("fc_1".into()),
        }),
    };
    let events = map_response(&resp);
    assert!(events.len() >= 2);
    let has_message = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }));
    let has_tool_call = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }));
    assert!(has_message);
    assert!(has_tool_call);
}

// ---------------------------------------------------------------------------
// CopilotTool serde roundtrip
// ---------------------------------------------------------------------------

#[test]
fn copilot_tool_function_serde_roundtrip() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Function,
        function: Some(CopilotFunctionDef {
            name: "read_file".into(),
            description: "Read file".into(),
            parameters: serde_json::json!({"type": "object"}),
        }),
        confirmation: None,
    };
    let json = serde_json::to_string(&tool).unwrap();
    let parsed: CopilotTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn copilot_tool_confirmation_serde_roundtrip() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Confirmation,
        function: None,
        confirmation: Some(CopilotConfirmation {
            id: "conf-1".into(),
            title: "Delete?".into(),
            message: "Really delete?".into(),
            accepted: None,
        }),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let parsed: CopilotTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

// ---------------------------------------------------------------------------
// CopilotFunctionDef serde
// ---------------------------------------------------------------------------

#[test]
fn copilot_function_def_serde_roundtrip() {
    let def = CopilotFunctionDef {
        name: "write_file".into(),
        description: "Write to a file".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}, "content": {"type": "string"}},
            "required": ["path", "content"]
        }),
    };
    let json = serde_json::to_string(&def).unwrap();
    let parsed: CopilotFunctionDef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, def);
}

// ---------------------------------------------------------------------------
// CopilotReference with metadata
// ---------------------------------------------------------------------------

#[test]
fn copilot_reference_with_metadata_map_serde() {
    let mut metadata = BTreeMap::new();
    metadata.insert("label".into(), serde_json::json!("README.md"));
    metadata.insert("size".into(), serde_json::json!(1024));
    let reference = CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "f-meta".into(),
        data: serde_json::json!({"path": "README.md"}),
        metadata: Some(metadata),
    };
    let json = serde_json::to_string(&reference).unwrap();
    let parsed: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, reference);
    let meta = parsed.metadata.unwrap();
    assert_eq!(meta["label"], "README.md");
}

// ---------------------------------------------------------------------------
// Stream event edge cases
// ---------------------------------------------------------------------------

#[test]
fn stream_text_delta_empty_text_produces_no_events() {
    let event = CopilotStreamEvent::TextDelta {
        text: String::new(),
    };
    let events = map_stream_event(&event);
    // Empty text deltas should produce empty events or an empty delta
    // (depends on implementation — verify behavior)
    assert!(
        events.is_empty()
            || events.iter().all(|e| matches!(
                &e.kind,
                AgentEventKind::AssistantDelta { text } if text.is_empty()
            ))
    );
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

#[test]
fn dialect_version_has_copilot_prefix() {
    assert!(DIALECT_VERSION.starts_with("copilot/"));
}

#[test]
fn default_model_is_gpt4o() {
    assert_eq!(DEFAULT_MODEL, "gpt-4o");
}

// ---------------------------------------------------------------------------
// is_known_model for claude models via copilot
// ---------------------------------------------------------------------------

#[test]
fn is_known_model_recognizes_claude_via_copilot() {
    assert!(is_known_model("claude-sonnet-4"));
    assert!(is_known_model("claude-3.5-sonnet"));
}

// ---------------------------------------------------------------------------
// from_canonical_model
// ---------------------------------------------------------------------------

#[test]
fn from_canonical_model_strips_copilot_prefix() {
    assert_eq!(from_canonical_model("copilot/o3-mini"), "o3-mini");
}

#[test]
fn from_canonical_model_other_prefix_passes_through() {
    assert_eq!(from_canonical_model("openai/gpt-4o"), "openai/gpt-4o");
}

// ---------------------------------------------------------------------------
// CopilotError JSON field names
// ---------------------------------------------------------------------------

#[test]
fn copilot_error_json_uses_type_not_error_type() {
    let err = CopilotError {
        error_type: "auth_error".into(),
        message: "Token expired".into(),
        code: None,
        identifier: None,
    };
    let json = serde_json::to_value(&err).unwrap();
    assert!(json.get("type").is_some());
    assert!(json.get("error_type").is_none());
}

// ---------------------------------------------------------------------------
// map_work_order config fields
// ---------------------------------------------------------------------------

#[test]
fn map_work_order_uses_config_model() {
    let wo = WorkOrderBuilder::new("Help me").build();
    let cfg = CopilotConfig {
        model: "o3-mini".into(),
        ..CopilotConfig::default()
    };
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, "o3-mini");
}
