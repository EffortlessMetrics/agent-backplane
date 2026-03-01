// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the Codex SDK: serde roundtrips, mapping, sandboxing,
//! tool types, and streaming events.

use abp_codex_sdk::dialect::{
    CanonicalToolDef, CodexConfig, CodexContentPart, CodexFunctionDef, CodexInputItem,
    CodexRequest, CodexResponse, CodexResponseItem, CodexStreamDelta, CodexStreamEvent,
    CodexTextFormat, CodexTool, CodexUsage, FileAccess, NetworkAccess, ReasoningSummary,
    SandboxConfig, codex_tool_to_canonical, map_response, map_stream_event, map_work_order,
    tool_def_from_codex, tool_def_to_codex,
};
use abp_core::{AgentEventKind, ContextPacket, ContextSnippet, WorkOrderBuilder};
use std::collections::BTreeMap;

// ===========================================================================
// 1. Serde roundtrips for all types
// ===========================================================================

#[test]
fn serde_roundtrip_codex_response_item_message() {
    let item = CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText {
            text: "Hello".into(),
        }],
    };
    let json = serde_json::to_string(&item).unwrap();
    let parsed: CodexResponseItem = serde_json::from_str(&json).unwrap();
    if let CodexResponseItem::Message { role, content } = &parsed {
        assert_eq!(role, "assistant");
        assert_eq!(content.len(), 1);
    } else {
        panic!("expected Message variant");
    }
}

#[test]
fn serde_roundtrip_codex_response_item_function_call() {
    let item = CodexResponseItem::FunctionCall {
        id: "fc_42".into(),
        call_id: Some("call_42".into()),
        name: "exec".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    let parsed: CodexResponseItem = serde_json::from_str(&json).unwrap();
    if let CodexResponseItem::FunctionCall {
        id, name, call_id, ..
    } = &parsed
    {
        assert_eq!(id, "fc_42");
        assert_eq!(name, "exec");
        assert_eq!(call_id.as_deref(), Some("call_42"));
    } else {
        panic!("expected FunctionCall variant");
    }
}

#[test]
fn serde_roundtrip_codex_response_item_function_call_output() {
    let item = CodexResponseItem::FunctionCallOutput {
        call_id: "call_1".into(),
        output: "file.txt\ndir/".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    let parsed: CodexResponseItem = serde_json::from_str(&json).unwrap();
    if let CodexResponseItem::FunctionCallOutput { call_id, output } = &parsed {
        assert_eq!(call_id, "call_1");
        assert!(output.contains("file.txt"));
    } else {
        panic!("expected FunctionCallOutput variant");
    }
}

#[test]
fn serde_roundtrip_codex_response_item_reasoning() {
    let item = CodexResponseItem::Reasoning {
        summary: vec![
            ReasoningSummary {
                text: "Step 1: analyze".into(),
            },
            ReasoningSummary {
                text: "Step 2: implement".into(),
            },
        ],
    };
    let json = serde_json::to_string(&item).unwrap();
    let parsed: CodexResponseItem = serde_json::from_str(&json).unwrap();
    if let CodexResponseItem::Reasoning { summary } = &parsed {
        assert_eq!(summary.len(), 2);
        assert_eq!(summary[0].text, "Step 1: analyze");
    } else {
        panic!("expected Reasoning variant");
    }
}

#[test]
fn serde_roundtrip_codex_text_format_text() {
    let fmt = CodexTextFormat::Text {};
    let json = serde_json::to_string(&fmt).unwrap();
    let parsed: CodexTextFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, CodexTextFormat::Text {});
}

#[test]
fn serde_roundtrip_codex_text_format_json_schema() {
    let fmt = CodexTextFormat::JsonSchema {
        name: "my_schema".into(),
        schema: serde_json::json!({"type": "object"}),
        strict: true,
    };
    let json = serde_json::to_string(&fmt).unwrap();
    let parsed: CodexTextFormat = serde_json::from_str(&json).unwrap();
    if let CodexTextFormat::JsonSchema { name, strict, .. } = &parsed {
        assert_eq!(name, "my_schema");
        assert!(strict);
    } else {
        panic!("expected JsonSchema variant");
    }
}

#[test]
fn serde_roundtrip_sandbox_config() {
    let cfg = SandboxConfig {
        container_image: Some("python:3.12".into()),
        networking: NetworkAccess::AllowList(vec!["api.example.com".into()]),
        file_access: FileAccess::ReadOnlyExternal,
        timeout_seconds: Some(120),
        memory_mb: Some(1024),
        env: BTreeMap::from([("FOO".into(), "bar".into())]),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: SandboxConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, cfg);
}

#[test]
fn serde_roundtrip_network_access_variants() {
    for access in [
        NetworkAccess::None,
        NetworkAccess::AllowList(vec!["host.io".into()]),
        NetworkAccess::Full,
    ] {
        let json = serde_json::to_string(&access).unwrap();
        let parsed: NetworkAccess = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, access);
    }
}

#[test]
fn serde_roundtrip_file_access_variants() {
    for access in [
        FileAccess::WorkspaceOnly,
        FileAccess::ReadOnlyExternal,
        FileAccess::Full,
    ] {
        let json = serde_json::to_string(&access).unwrap();
        let parsed: FileAccess = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, access);
    }
}

#[test]
fn serde_roundtrip_codex_stream_event_response_created() {
    let resp = CodexResponse {
        id: "r1".into(),
        model: "codex-mini-latest".into(),
        output: vec![],
        usage: None,
        status: None,
    };
    let event = CodexStreamEvent::ResponseCreated {
        response: resp.clone(),
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: CodexStreamEvent = serde_json::from_str(&json).unwrap();
    if let CodexStreamEvent::ResponseCreated { response } = parsed {
        assert_eq!(response.id, "r1");
    } else {
        panic!("expected ResponseCreated");
    }
}

#[test]
fn serde_roundtrip_codex_stream_delta() {
    let delta = CodexStreamDelta::OutputTextDelta {
        text: "chunk".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let parsed: CodexStreamDelta = serde_json::from_str(&json).unwrap();
    if let CodexStreamDelta::OutputTextDelta { text } = &parsed {
        assert_eq!(text, "chunk");
    } else {
        panic!("expected OutputTextDelta");
    }
}

#[test]
fn serde_roundtrip_codex_tool_enum_function() {
    let tool = CodexTool::Function {
        function: CodexFunctionDef {
            name: "search".into(),
            description: "Search files".into(),
            parameters: serde_json::json!({"type": "object"}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let parsed: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn serde_roundtrip_codex_tool_enum_code_interpreter() {
    let tool = CodexTool::CodeInterpreter {};
    let json = serde_json::to_string(&tool).unwrap();
    let parsed: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn serde_roundtrip_codex_tool_enum_file_search() {
    let tool = CodexTool::FileSearch {
        max_num_results: Some(10),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let parsed: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

// ===========================================================================
// 2. WorkOrder → Codex request mapping
// ===========================================================================

#[test]
fn map_work_order_basic() {
    let wo = WorkOrderBuilder::new("Fix bug").build();
    let cfg = CodexConfig::default();
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.model, "codex-mini-latest");
    assert_eq!(req.input.len(), 1);
    let CodexInputItem::Message { role, content } = &req.input[0];
    assert_eq!(role, "user");
    assert!(content.contains("Fix bug"));
}

#[test]
fn map_work_order_with_model_override() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4.1").build();
    let cfg = CodexConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4.1");
}

#[test]
fn map_work_order_includes_context_snippets() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![
            ContextSnippet {
                name: "config.json".into(),
                content: r#"{"debug": true}"#.into(),
            },
            ContextSnippet {
                name: "notes.md".into(),
                content: "Important note".into(),
            },
        ],
    };
    let wo = WorkOrderBuilder::new("Refactor").context(ctx).build();
    let cfg = CodexConfig::default();
    let req = map_work_order(&wo, &cfg);

    let CodexInputItem::Message { content, .. } = &req.input[0];
    assert!(content.contains("config.json"));
    assert!(content.contains("notes.md"));
    assert!(content.contains("Important note"));
}

#[test]
fn map_work_order_applies_max_tokens_from_config() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CodexConfig {
        max_output_tokens: Some(8192),
        ..CodexConfig::default()
    };
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.max_output_tokens, Some(8192));
}

#[test]
fn map_work_order_request_tools_default_empty() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CodexConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert!(req.tools.is_empty());
}

// ===========================================================================
// 3. Codex response → ABP event mapping
// ===========================================================================

#[test]
fn map_response_function_call_output_produces_tool_result() {
    let resp = CodexResponse {
        id: "r1".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::FunctionCallOutput {
            call_id: "call_99".into(),
            output: "success".into(),
        }],
        usage: None,
        status: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    if let AgentEventKind::ToolResult {
        tool_use_id,
        is_error,
        ..
    } = &events[0].kind
    {
        assert_eq!(tool_use_id.as_deref(), Some("call_99"));
        assert!(!is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn map_response_reasoning_produces_delta() {
    let resp = CodexResponse {
        id: "r2".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::Reasoning {
            summary: vec![ReasoningSummary {
                text: "Thinking about the problem".into(),
            }],
        }],
        usage: None,
        status: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    if let AgentEventKind::AssistantDelta { text } = &events[0].kind {
        assert!(text.contains("Thinking about the problem"));
    } else {
        panic!("expected AssistantDelta");
    }
}

#[test]
fn map_response_empty_reasoning_produces_no_events() {
    let resp = CodexResponse {
        id: "r3".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::Reasoning { summary: vec![] }],
        usage: None,
        status: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_mixed_output_preserves_order() {
    let resp = CodexResponse {
        id: "r4".into(),
        model: "codex-mini-latest".into(),
        output: vec![
            CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "First".into(),
                }],
            },
            CodexResponseItem::FunctionCall {
                id: "fc_1".into(),
                call_id: None,
                name: "shell".into(),
                arguments: r#"{"cmd":"echo hi"}"#.into(),
            },
            CodexResponseItem::FunctionCallOutput {
                call_id: "fc_1".into(),
                output: "hi".into(),
            },
            CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Last".into(),
                }],
            },
        ],
        usage: None,
        status: None,
    };
    let events = map_response(&resp);
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

// ===========================================================================
// 4. Sandbox config defaults
// ===========================================================================

#[test]
fn sandbox_config_default_networking_is_none() {
    let cfg = SandboxConfig::default();
    assert_eq!(cfg.networking, NetworkAccess::None);
}

#[test]
fn sandbox_config_default_file_access_is_workspace_only() {
    let cfg = SandboxConfig::default();
    assert_eq!(cfg.file_access, FileAccess::WorkspaceOnly);
}

#[test]
fn sandbox_config_default_timeout_is_set() {
    let cfg = SandboxConfig::default();
    assert_eq!(cfg.timeout_seconds, Some(300));
}

#[test]
fn sandbox_config_default_memory_is_set() {
    let cfg = SandboxConfig::default();
    assert_eq!(cfg.memory_mb, Some(512));
}

#[test]
fn sandbox_config_default_env_is_empty() {
    let cfg = SandboxConfig::default();
    assert!(cfg.env.is_empty());
}

#[test]
fn codex_config_default_includes_sandbox() {
    let cfg = CodexConfig::default();
    assert_eq!(cfg.sandbox.networking, NetworkAccess::None);
    assert_eq!(cfg.sandbox.file_access, FileAccess::WorkspaceOnly);
}

// ===========================================================================
// 5. Tool type mapping
// ===========================================================================

#[test]
fn codex_tool_function_to_canonical() {
    let tool = CodexTool::Function {
        function: CodexFunctionDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}}}),
        },
    };
    let canonical = codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "read_file");
    assert_eq!(canonical.description, "Read a file");
}

#[test]
fn codex_tool_code_interpreter_to_canonical() {
    let tool = CodexTool::CodeInterpreter {};
    let canonical = codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "code_interpreter");
}

#[test]
fn codex_tool_file_search_to_canonical() {
    let tool = CodexTool::FileSearch {
        max_num_results: Some(5),
    };
    let canonical = codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "file_search");
}

#[test]
fn legacy_tool_def_roundtrip() {
    let canonical = CanonicalToolDef {
        name: "write_file".into(),
        description: "Write a file".into(),
        parameters_schema: serde_json::json!({"type": "object"}),
    };
    let codex = tool_def_to_codex(&canonical);
    let back = tool_def_from_codex(&codex);
    assert_eq!(back, canonical);
}

// ===========================================================================
// 6. Stream event mapping
// ===========================================================================

#[test]
fn stream_response_created_produces_run_started() {
    let event = CodexStreamEvent::ResponseCreated {
        response: CodexResponse {
            id: "r1".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: None,
        },
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn stream_response_in_progress_produces_no_events() {
    let event = CodexStreamEvent::ResponseInProgress {
        response: CodexResponse {
            id: "r1".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: Some("in_progress".into()),
        },
    };
    let events = map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn stream_output_item_added_message_produces_assistant_message() {
    let event = CodexStreamEvent::OutputItemAdded {
        output_index: 0,
        item: CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText { text: "Hi!".into() }],
        },
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[test]
fn stream_output_item_delta_text_produces_assistant_delta() {
    let event = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::OutputTextDelta {
            text: "partial ".into(),
        },
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    if let AgentEventKind::AssistantDelta { text } = &events[0].kind {
        assert_eq!(text, "partial ");
    } else {
        panic!("expected AssistantDelta");
    }
}

#[test]
fn stream_output_item_delta_function_args_produces_no_events() {
    let event = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::FunctionCallArgumentsDelta {
            delta: r#"{"par"#.into(),
        },
    };
    let events = map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn stream_output_item_done_function_call_produces_tool_call() {
    let event = CodexStreamEvent::OutputItemDone {
        output_index: 0,
        item: CodexResponseItem::FunctionCall {
            id: "fc_done".into(),
            call_id: None,
            name: "shell".into(),
            arguments: r#"{"cmd":"pwd"}"#.into(),
        },
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn stream_response_completed_produces_run_completed() {
    let event = CodexStreamEvent::ResponseCompleted {
        response: CodexResponse {
            id: "r1".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: Some(CodexUsage {
                input_tokens: 100,
                output_tokens: 50,
                total_tokens: 150,
            }),
            status: Some("completed".into()),
        },
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn stream_response_failed_produces_error_event() {
    let event = CodexStreamEvent::ResponseFailed {
        response: CodexResponse {
            id: "r1".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: Some("rate_limit_exceeded".into()),
        },
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    if let AgentEventKind::Error { message } = &events[0].kind {
        assert!(message.contains("rate_limit_exceeded"));
    } else {
        panic!("expected Error event");
    }
}

#[test]
fn stream_error_produces_error_event() {
    let event = CodexStreamEvent::Error {
        message: "server error".into(),
        code: Some("500".into()),
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    if let AgentEventKind::Error { message } = &events[0].kind {
        assert_eq!(message, "server error");
    } else {
        panic!("expected Error event");
    }
}

#[test]
fn stream_reasoning_delta_produces_no_events() {
    let event = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::ReasoningSummaryDelta {
            text: "thinking...".into(),
        },
    };
    let events = map_stream_event(&event);
    assert!(events.is_empty());
}

// ===========================================================================
// 7. Text format default
// ===========================================================================

#[test]
fn text_format_default_is_text() {
    let fmt = CodexTextFormat::default();
    assert_eq!(fmt, CodexTextFormat::Text {});
}

// ===========================================================================
// 8. CodexRequest serialization
// ===========================================================================

#[test]
fn codex_request_skips_empty_tools_in_json() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "hello".into(),
        }],
        max_output_tokens: None,
        temperature: None,
        tools: vec![],
        text: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("tools"));
}

#[test]
fn codex_request_includes_tools_when_present() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "hello".into(),
        }],
        max_output_tokens: None,
        temperature: None,
        tools: vec![CodexTool::CodeInterpreter {}],
        text: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("code_interpreter"));
}
