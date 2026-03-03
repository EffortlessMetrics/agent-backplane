// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive tests for OpenAI Codex SDK dialect mapping.
//!
//! Covers request format mapping, tool_use/shell execution, streaming,
//! model-name capability mapping, config options, sandbox/container
//! execution context, Codex→OpenAI compatibility, error handling,
//! file system operation events, and conversation context structure.

use std::collections::BTreeMap;

use abp_codex_sdk::dialect::{
    self, CanonicalToolDef, CodexConfig, CodexContentPart, CodexFunctionDef, CodexInputItem,
    CodexRequest, CodexResponse, CodexResponseItem, CodexStreamDelta, CodexStreamEvent,
    CodexTextFormat, CodexTool, CodexUsage, FileAccess, NetworkAccess, ReasoningSummary,
    SandboxConfig,
};
use abp_codex_sdk::lowering;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, Outcome, ReceiptBuilder, SupportLevel,
    WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_dialect::Dialect;
use abp_mapping::{
    features, known_rules, validate_mapping, Fidelity, MappingMatrix, MappingRegistry, MappingRule,
};
use chrono::Utc;
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
        status: Some("completed".into()),
    }
}

fn make_response_with_usage(items: Vec<CodexResponseItem>, usage: CodexUsage) -> CodexResponse {
    CodexResponse {
        id: "resp_usage".into(),
        model: "codex-mini-latest".into(),
        output: items,
        usage: Some(usage),
        status: Some("completed".into()),
    }
}

fn default_config() -> CodexConfig {
    CodexConfig::default()
}

fn config_with_model(model: &str) -> CodexConfig {
    CodexConfig {
        model: model.into(),
        ..CodexConfig::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Codex-specific request format mapping to WorkOrder
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_basic_task_maps_to_work_order() {
    let wo = WorkOrderBuilder::new("Refactor auth module").build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);

    assert_eq!(req.model, "codex-mini-latest");
    assert_eq!(req.input.len(), 1);
    match &req.input[0] {
        CodexInputItem::Message { role, content } => {
            assert_eq!(role, "user");
            assert!(content.contains("Refactor auth module"));
        }
    }
}

#[test]
fn request_model_override_from_work_order() {
    let wo = WorkOrderBuilder::new("task").model("o4-mini").build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "o4-mini");
}

#[test]
fn request_falls_back_to_config_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = config_with_model("gpt-4.1");
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4.1");
}

#[test]
fn request_includes_context_snippets() {
    let ctx = abp_core::ContextPacket {
        files: vec![],
        snippets: vec![abp_core::ContextSnippet {
            name: "README".into(),
            content: "# Project\nHello".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("Improve docs").context(ctx).build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);

    match &req.input[0] {
        CodexInputItem::Message { content, .. } => {
            assert!(content.contains("README"));
            assert!(content.contains("# Project"));
        }
    }
}

#[test]
fn request_max_output_tokens_from_config() {
    let cfg = CodexConfig {
        max_output_tokens: Some(8192),
        ..CodexConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.max_output_tokens, Some(8192));
}

#[test]
fn request_temperature_from_config() {
    let cfg = CodexConfig {
        temperature: Some(0.7),
        ..CodexConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.temperature, Some(0.7));
}

#[test]
fn request_no_tools_by_default() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.tools.is_empty());
}

#[test]
fn request_no_text_format_by_default() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.text.is_none());
}

#[test]
fn request_multiple_context_snippets_concatenated() {
    let ctx = abp_core::ContextPacket {
        files: vec![],
        snippets: vec![
            abp_core::ContextSnippet {
                name: "file1".into(),
                content: "content1".into(),
            },
            abp_core::ContextSnippet {
                name: "file2".into(),
                content: "content2".into(),
            },
        ],
    };
    let wo = WorkOrderBuilder::new("multi-context").context(ctx).build();
    let cfg = default_config();
    let req = dialect::map_work_order(&wo, &cfg);

    match &req.input[0] {
        CodexInputItem::Message { content, .. } => {
            assert!(content.contains("file1"));
            assert!(content.contains("content1"));
            assert!(content.contains("file2"));
            assert!(content.contains("content2"));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Codex tool_use and shell execution event mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_call_shell_maps_to_agent_event() {
    let resp = make_response(vec![function_call(
        "fc_1",
        "shell",
        r#"{"command":"ls -la"}"#,
    )]);
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
            assert_eq!(input["command"], "ls -la");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn tool_call_read_file_maps_correctly() {
    let resp = make_response(vec![function_call(
        "fc_2",
        "read_file",
        r#"{"path":"src/main.rs"}"#,
    )]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "read_file");
            assert_eq!(input["path"], "src/main.rs");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn tool_call_write_file_maps_correctly() {
    let resp = make_response(vec![function_call(
        "fc_3",
        "write_file",
        r#"{"path":"out.txt","content":"hello"}"#,
    )]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "write_file");
            assert_eq!(input["path"], "out.txt");
            assert_eq!(input["content"], "hello");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn function_call_output_maps_to_tool_result() {
    let resp = make_response(vec![function_call_output("fc_1", "file.txt\nREADME.md")]);
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
            assert_eq!(output.as_str().unwrap(), "file.txt\nREADME.md");
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_call_with_json_args_parsed() {
    let resp = make_response(vec![function_call(
        "fc_x",
        "search",
        r#"{"query":"rust async","limit":10}"#,
    )]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input["query"], "rust async");
            assert_eq!(input["limit"], 10);
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn tool_call_with_malformed_args_becomes_string() {
    let resp = make_response(vec![function_call("fc_bad", "foo", "not-valid-json")]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input.as_str().unwrap(), "not-valid-json");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn multiple_tool_calls_in_sequence() {
    let resp = make_response(vec![
        function_call("fc_1", "read_file", r#"{"path":"a.rs"}"#),
        function_call_output("fc_1", "fn main() {}"),
        function_call(
            "fc_2",
            "write_file",
            r#"{"path":"b.rs","content":"// new"}"#,
        ),
    ]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolResult { .. }));
    assert!(matches!(&events[2].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn tool_def_roundtrip_canonical() {
    let canonical = CanonicalToolDef {
        name: "grep".into(),
        description: "Search files".into(),
        parameters_schema: json!({"type": "object", "properties": {"pattern": {"type": "string"}}}),
    };
    let codex = dialect::tool_def_to_codex(&canonical);
    assert_eq!(codex.tool_type, "function");
    assert_eq!(codex.function.name, "grep");

    let back = dialect::tool_def_from_codex(&codex);
    assert_eq!(back.name, canonical.name);
    assert_eq!(back.description, canonical.description);
    assert_eq!(back.parameters_schema, canonical.parameters_schema);
}

#[test]
fn codex_tool_function_to_canonical() {
    let tool = CodexTool::Function {
        function: CodexFunctionDef {
            name: "edit".into(),
            description: "Edit a file".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "edit");
    assert_eq!(canonical.description, "Edit a file");
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
        max_num_results: Some(20),
    };
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "file_search");
    assert!(canonical.description.contains("Search"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Codex streaming response mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_response_created_maps_to_run_started() {
    let event = CodexStreamEvent::ResponseCreated {
        response: make_response(vec![]),
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn stream_in_progress_produces_no_events() {
    let event = CodexStreamEvent::ResponseInProgress {
        response: make_response(vec![]),
    };
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn stream_output_text_delta_maps_to_assistant_delta() {
    let event = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::OutputTextDelta {
            text: "Hello".into(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn stream_function_call_args_delta_produces_no_events() {
    let event = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::FunctionCallArgumentsDelta {
            delta: r#"{"par"#.into(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn stream_reasoning_summary_delta_produces_no_events() {
    let event = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::ReasoningSummaryDelta {
            text: "thinking...".into(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn stream_output_item_added_message() {
    let item = assistant_text("Hi there");
    let event = CodexStreamEvent::OutputItemAdded {
        output_index: 0,
        item,
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hi there"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn stream_output_item_done_function_call() {
    let item = function_call("fc_10", "bash", r#"{"cmd":"echo hi"}"#);
    let event = CodexStreamEvent::OutputItemDone {
        output_index: 0,
        item,
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn stream_response_completed_maps_to_run_completed() {
    let event = CodexStreamEvent::ResponseCompleted {
        response: make_response(vec![]),
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn stream_response_failed_maps_to_error() {
    let event = CodexStreamEvent::ResponseFailed {
        response: CodexResponse {
            id: "resp_fail".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: Some("rate_limited".into()),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert_eq!(message, "rate_limited");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn stream_error_event_maps_to_error() {
    let event = CodexStreamEvent::Error {
        message: "server_error".into(),
        code: Some("500".into()),
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert_eq!(message, "server_error");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn stream_multiple_text_deltas_accumulate() {
    let deltas = ["Hel", "lo ", "wor", "ld!"];
    let mut all_events = Vec::new();
    for d in &deltas {
        let ev = CodexStreamEvent::OutputItemDelta {
            output_index: 0,
            delta: CodexStreamDelta::OutputTextDelta {
                text: d.to_string(),
            },
        };
        all_events.extend(dialect::map_stream_event(&ev));
    }
    assert_eq!(all_events.len(), 4);
    let full: String = all_events
        .iter()
        .map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => text.as_str(),
            _ => "",
        })
        .collect();
    assert_eq!(full, "Hello world!");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Codex model names capability mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn known_model_codex_mini_latest() {
    assert!(dialect::is_known_model("codex-mini-latest"));
}

#[test]
fn known_model_o3_mini() {
    assert!(dialect::is_known_model("o3-mini"));
}

#[test]
fn known_model_o4_mini() {
    assert!(dialect::is_known_model("o4-mini"));
}

#[test]
fn known_model_gpt4() {
    assert!(dialect::is_known_model("gpt-4"));
}

#[test]
fn known_model_gpt4o() {
    assert!(dialect::is_known_model("gpt-4o"));
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

#[test]
fn unknown_model_returns_false() {
    assert!(!dialect::is_known_model("gpt-99-turbo"));
}

#[test]
fn canonical_model_prefix() {
    assert_eq!(
        dialect::to_canonical_model("codex-mini-latest"),
        "openai/codex-mini-latest"
    );
}

#[test]
fn canonical_model_roundtrip() {
    let canonical = dialect::to_canonical_model("o4-mini");
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, "o4-mini");
}

#[test]
fn from_canonical_strips_prefix() {
    assert_eq!(dialect::from_canonical_model("openai/gpt-4"), "gpt-4");
}

#[test]
fn from_canonical_passthrough_without_prefix() {
    assert_eq!(
        dialect::from_canonical_model("custom-model"),
        "custom-model"
    );
}

#[test]
fn capability_manifest_has_streaming() {
    let caps = dialect::capability_manifest();
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_tool_read() {
    let caps = dialect::capability_manifest();
    assert!(matches!(
        caps.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_tool_write() {
    let caps = dialect::capability_manifest();
    assert!(matches!(
        caps.get(&Capability::ToolWrite),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_tool_edit() {
    let caps = dialect::capability_manifest();
    assert!(matches!(
        caps.get(&Capability::ToolEdit),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_tool_bash() {
    let caps = dialect::capability_manifest();
    assert!(matches!(
        caps.get(&Capability::ToolBash),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_glob_emulated() {
    let caps = dialect::capability_manifest();
    assert!(matches!(
        caps.get(&Capability::ToolGlob),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn capability_manifest_grep_emulated() {
    let caps = dialect::capability_manifest();
    assert!(matches!(
        caps.get(&Capability::ToolGrep),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn capability_manifest_mcp_unsupported() {
    let caps = dialect::capability_manifest();
    assert!(matches!(
        caps.get(&Capability::McpClient),
        Some(SupportLevel::Unsupported)
    ));
    assert!(matches!(
        caps.get(&Capability::McpServer),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn capability_manifest_structured_output_native() {
    let caps = dialect::capability_manifest();
    assert!(matches!(
        caps.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Native)
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Codex-specific config options mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn default_config_base_url() {
    let cfg = CodexConfig::default();
    assert!(cfg.base_url.contains("openai.com"));
}

#[test]
fn default_config_model() {
    let cfg = CodexConfig::default();
    assert_eq!(cfg.model, "codex-mini-latest");
}

#[test]
fn default_config_max_output_tokens() {
    let cfg = CodexConfig::default();
    assert_eq!(cfg.max_output_tokens, Some(4096));
}

#[test]
fn default_config_temperature_is_none() {
    let cfg = CodexConfig::default();
    assert!(cfg.temperature.is_none());
}

#[test]
fn default_config_api_key_empty() {
    let cfg = CodexConfig::default();
    assert!(cfg.api_key.is_empty());
}

#[test]
fn config_custom_values() {
    let cfg = CodexConfig {
        api_key: "sk-test-key".into(),
        base_url: "https://custom.api.com/v1".into(),
        model: "gpt-4.1".into(),
        max_output_tokens: Some(16384),
        temperature: Some(1.2),
        sandbox: SandboxConfig::default(),
    };
    assert_eq!(cfg.api_key, "sk-test-key");
    assert_eq!(cfg.base_url, "https://custom.api.com/v1");
    assert_eq!(cfg.model, "gpt-4.1");
    assert_eq!(cfg.max_output_tokens, Some(16384));
    assert_eq!(cfg.temperature, Some(1.2));
}

#[test]
fn config_serde_roundtrip() {
    let cfg = CodexConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: CodexConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, cfg.model);
    assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
    assert_eq!(back.base_url, cfg.base_url);
}

#[test]
fn text_format_default_is_text() {
    let fmt = CodexTextFormat::default();
    assert!(matches!(fmt, CodexTextFormat::Text {}));
}

#[test]
fn text_format_json_object_serde() {
    let fmt = CodexTextFormat::JsonObject {};
    let json = serde_json::to_value(&fmt).unwrap();
    assert_eq!(json["type"], "json_object");
}

#[test]
fn text_format_json_schema_serde() {
    let fmt = CodexTextFormat::JsonSchema {
        name: "output".into(),
        schema: json!({"type": "object", "properties": {"result": {"type": "string"}}}),
        strict: true,
    };
    let json = serde_json::to_value(&fmt).unwrap();
    assert_eq!(json["type"], "json_schema");
    assert_eq!(json["name"], "output");
    assert!(json["strict"].as_bool().unwrap());
}

#[test]
fn dialect_version_constant() {
    assert_eq!(dialect::DIALECT_VERSION, "codex/v0.1");
}

#[test]
fn default_model_constant() {
    assert_eq!(dialect::DEFAULT_MODEL, "codex-mini-latest");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Codex sandbox/container execution context
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sandbox_default_values() {
    let sb = SandboxConfig::default();
    assert!(sb.container_image.is_none());
    assert_eq!(sb.networking, NetworkAccess::None);
    assert_eq!(sb.file_access, FileAccess::WorkspaceOnly);
    assert_eq!(sb.timeout_seconds, Some(300));
    assert_eq!(sb.memory_mb, Some(512));
    assert!(sb.env.is_empty());
}

#[test]
fn sandbox_custom_container_image() {
    let sb = SandboxConfig {
        container_image: Some("node:20".into()),
        ..SandboxConfig::default()
    };
    assert_eq!(sb.container_image.as_deref(), Some("node:20"));
}

#[test]
fn sandbox_full_network_access() {
    let sb = SandboxConfig {
        networking: NetworkAccess::Full,
        ..SandboxConfig::default()
    };
    assert_eq!(sb.networking, NetworkAccess::Full);
}

#[test]
fn sandbox_network_allowlist() {
    let sb = SandboxConfig {
        networking: NetworkAccess::AllowList(vec!["api.github.com".into(), "pypi.org".into()]),
        ..SandboxConfig::default()
    };
    match &sb.networking {
        NetworkAccess::AllowList(hosts) => {
            assert_eq!(hosts.len(), 2);
            assert!(hosts.contains(&"api.github.com".to_string()));
        }
        other => panic!("expected AllowList, got {other:?}"),
    }
}

#[test]
fn sandbox_file_access_readonly_external() {
    let sb = SandboxConfig {
        file_access: FileAccess::ReadOnlyExternal,
        ..SandboxConfig::default()
    };
    assert_eq!(sb.file_access, FileAccess::ReadOnlyExternal);
}

#[test]
fn sandbox_file_access_full() {
    let sb = SandboxConfig {
        file_access: FileAccess::Full,
        ..SandboxConfig::default()
    };
    assert_eq!(sb.file_access, FileAccess::Full);
}

#[test]
fn sandbox_custom_timeout() {
    let sb = SandboxConfig {
        timeout_seconds: Some(600),
        ..SandboxConfig::default()
    };
    assert_eq!(sb.timeout_seconds, Some(600));
}

#[test]
fn sandbox_custom_memory() {
    let sb = SandboxConfig {
        memory_mb: Some(2048),
        ..SandboxConfig::default()
    };
    assert_eq!(sb.memory_mb, Some(2048));
}

#[test]
fn sandbox_env_vars() {
    let mut env = BTreeMap::new();
    env.insert("NODE_ENV".into(), "production".into());
    env.insert("RUST_LOG".into(), "debug".into());
    let sb = SandboxConfig {
        env,
        ..SandboxConfig::default()
    };
    assert_eq!(sb.env.len(), 2);
    assert_eq!(sb.env["NODE_ENV"], "production");
    assert_eq!(sb.env["RUST_LOG"], "debug");
}

#[test]
fn sandbox_serde_roundtrip() {
    let mut env = BTreeMap::new();
    env.insert("KEY".into(), "val".into());
    let sb = SandboxConfig {
        container_image: Some("python:3.12".into()),
        networking: NetworkAccess::AllowList(vec!["example.com".into()]),
        file_access: FileAccess::ReadOnlyExternal,
        timeout_seconds: Some(120),
        memory_mb: Some(1024),
        env,
    };
    let json = serde_json::to_string(&sb).unwrap();
    let back: SandboxConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back, sb);
}

#[test]
fn sandbox_in_config_roundtrip() {
    let cfg = CodexConfig {
        sandbox: SandboxConfig {
            container_image: Some("rust:latest".into()),
            timeout_seconds: Some(60),
            ..SandboxConfig::default()
        },
        ..CodexConfig::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: CodexConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.sandbox.container_image.as_deref(), Some("rust:latest"));
    assert_eq!(back.sandbox.timeout_seconds, Some(60));
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Codex→OpenAI dialect compatibility (close cousin mapping)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_to_openai_streaming_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Codex, Dialect::OpenAi, features::STREAMING)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn openai_to_codex_streaming_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Codex, features::STREAMING)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn codex_to_openai_tool_use_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Codex, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
}

#[test]
fn openai_to_codex_tool_use_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
}

#[test]
fn codex_to_openai_thinking_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Codex, Dialect::OpenAi, features::THINKING)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
}

#[test]
fn codex_to_claude_tool_use_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Codex, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
}

#[test]
fn codex_to_gemini_tool_use_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Codex, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
}

#[test]
fn codex_self_mapping_lossless_all_features() {
    let reg = known_rules();
    for feat in [
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ] {
        let rule = reg.lookup(Dialect::Codex, Dialect::Codex, feat).unwrap();
        assert!(
            rule.fidelity.is_lossless(),
            "self-mapping for {feat} should be lossless"
        );
    }
}

#[test]
fn codex_image_input_to_openai_unsupported() {
    let reg = known_rules();
    // Codex doesn't support image input; check Codex as target
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Codex, features::IMAGE_INPUT)
        .unwrap();
    assert!(rule.fidelity.is_unsupported());
}

#[test]
fn codex_image_input_to_claude_unsupported() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::Codex, features::IMAGE_INPUT)
        .unwrap();
    assert!(rule.fidelity.is_unsupported());
}

#[test]
fn validate_codex_to_openai_features() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Codex,
        Dialect::OpenAi,
        &["tool_use".into(), "streaming".into()],
    );
    assert_eq!(results.len(), 2);
    // streaming should be lossless
    let streaming = results.iter().find(|r| r.feature == "streaming").unwrap();
    assert!(streaming.fidelity.is_lossless());
    assert!(streaming.errors.is_empty());
}

#[test]
fn validate_codex_to_openai_unsupported_feature() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Codex,
        Dialect::OpenAi,
        &["nonexistent_feature".into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert!(!results[0].errors.is_empty());
}

#[test]
fn mapping_matrix_codex_openai_supported() {
    let reg = known_rules();
    let matrix = MappingMatrix::from_registry(&reg);
    assert!(matrix.is_supported(Dialect::Codex, Dialect::OpenAi));
    assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Codex));
}

#[test]
fn mapping_matrix_codex_claude_supported() {
    let reg = known_rules();
    let matrix = MappingMatrix::from_registry(&reg);
    assert!(matrix.is_supported(Dialect::Codex, Dialect::Claude));
}

#[test]
fn rank_targets_from_codex_streaming() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::Codex, &[features::STREAMING]);
    // All other dialects should support streaming losslessly from Codex
    assert!(!ranked.is_empty());
    // Each should have lossless_count of 1
    for &(_, count) in &ranked {
        assert_eq!(count, 1);
    }
}

#[test]
fn codex_code_exec_cross_dialect_lossy() {
    let reg = known_rules();
    for target in [Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let rule = reg
            .lookup(Dialect::Codex, target, features::CODE_EXEC)
            .unwrap();
        assert!(
            !rule.fidelity.is_lossless(),
            "Codex→{target:?} code_exec should be lossy"
        );
        assert!(
            !rule.fidelity.is_unsupported(),
            "Codex→{target:?} code_exec should not be unsupported"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Codex error handling and timeout patterns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_event_in_response() {
    let event = CodexStreamEvent::Error {
        message: "context_length_exceeded".into(),
        code: Some("400".into()),
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert_eq!(message, "context_length_exceeded");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn failed_response_with_no_status() {
    let event = CodexStreamEvent::ResponseFailed {
        response: CodexResponse {
            id: "resp_err".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: None,
        },
    };
    let events = dialect::map_stream_event(&event);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert_eq!(message, "unknown failure");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn error_with_no_code() {
    let event = CodexStreamEvent::Error {
        message: "timeout".into(),
        code: None,
    };
    let events = dialect::map_stream_event(&event);
    match &events[0].kind {
        AgentEventKind::Error {
            message,
            error_code,
        } => {
            assert_eq!(message, "timeout");
            assert!(error_code.is_none());
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn sandbox_timeout_config() {
    let sb = SandboxConfig {
        timeout_seconds: Some(30),
        ..SandboxConfig::default()
    };
    assert_eq!(sb.timeout_seconds, Some(30));
}

#[test]
fn sandbox_no_timeout() {
    let sb = SandboxConfig {
        timeout_seconds: None,
        ..SandboxConfig::default()
    };
    assert!(sb.timeout_seconds.is_none());
}

#[test]
fn receipt_with_error_trace() {
    let error_event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "sandbox_timeout".into(),
            error_code: None,
        },
        ext: None,
    };
    let receipt = ReceiptBuilder::new("sidecar:codex")
        .outcome(Outcome::Failed)
        .add_trace_event(error_event)
        .build();
    assert_eq!(receipt.outcome, Outcome::Failed);
    assert_eq!(receipt.trace.len(), 1);
    assert!(matches!(
        &receipt.trace[0].kind,
        AgentEventKind::Error { .. }
    ));
}

#[test]
fn receipt_partial_outcome() {
    let receipt = ReceiptBuilder::new("sidecar:codex")
        .outcome(Outcome::Partial)
        .build();
    assert_eq!(receipt.outcome, Outcome::Partial);
}

#[test]
fn contract_version_in_receipt() {
    let receipt = ReceiptBuilder::new("sidecar:codex")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Codex file system operation event mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn file_read_tool_call_maps() {
    let resp = make_response(vec![function_call(
        "fc_read",
        "read_file",
        r#"{"path":"Cargo.toml"}"#,
    )]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "read_file");
            assert_eq!(input["path"], "Cargo.toml");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn file_write_tool_call_maps() {
    let resp = make_response(vec![function_call(
        "fc_write",
        "write_file",
        r#"{"path":"new.rs","content":"fn main() {}"}"#,
    )]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "write_file");
            assert_eq!(input["path"], "new.rs");
            assert!(input["content"].as_str().unwrap().contains("fn main"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn file_edit_tool_call_maps() {
    let resp = make_response(vec![function_call(
        "fc_edit",
        "edit_file",
        r#"{"path":"lib.rs","old":"foo","new":"bar"}"#,
    )]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "edit_file");
            assert_eq!(input["old"], "foo");
            assert_eq!(input["new"], "bar");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn glob_tool_call_maps() {
    let resp = make_response(vec![function_call(
        "fc_glob",
        "glob",
        r#"{"pattern":"**/*.rs"}"#,
    )]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "glob");
            assert_eq!(input["pattern"], "**/*.rs");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn grep_tool_call_maps() {
    let resp = make_response(vec![function_call(
        "fc_grep",
        "grep",
        r#"{"pattern":"TODO","path":"src"}"#,
    )]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "grep");
            assert_eq!(input["pattern"], "TODO");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn file_operation_followed_by_result() {
    let resp = make_response(vec![
        function_call("fc_r", "read_file", r#"{"path":"x.rs"}"#),
        function_call_output("fc_r", "use std::io;"),
    ]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn file_operation_ir_roundtrip() {
    let items = vec![
        function_call("fc_1", "read_file", r#"{"path":"a.rs"}"#),
        function_call_output("fc_1", "fn hello() {}"),
    ];
    let conv = lowering::to_ir(&items);
    assert_eq!(conv.messages.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    assert_eq!(conv.messages[1].role, IrRole::Tool);

    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 2);
    assert!(matches!(&back[0], CodexResponseItem::FunctionCall { .. }));
    assert!(matches!(
        &back[1],
        CodexResponseItem::FunctionCallOutput { .. }
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Codex conversation context structure
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn input_user_message_to_ir() {
    let items = vec![CodexInputItem::Message {
        role: "user".into(),
        content: "Fix the bug".into(),
    }];
    let conv = lowering::input_to_ir(&items);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Fix the bug");
}

#[test]
fn input_system_message_to_ir() {
    let items = vec![CodexInputItem::Message {
        role: "system".into(),
        content: "You are a code assistant".into(),
    }];
    let conv = lowering::input_to_ir(&items);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn input_assistant_message_to_ir() {
    let items = vec![CodexInputItem::Message {
        role: "assistant".into(),
        content: "I can help with that".into(),
    }];
    let conv = lowering::input_to_ir(&items);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
}

#[test]
fn input_multi_turn_conversation() {
    let items = vec![
        CodexInputItem::Message {
            role: "system".into(),
            content: "Be concise".into(),
        },
        CodexInputItem::Message {
            role: "user".into(),
            content: "What is Rust?".into(),
        },
        CodexInputItem::Message {
            role: "assistant".into(),
            content: "A systems programming language".into(),
        },
        CodexInputItem::Message {
            role: "user".into(),
            content: "Show me an example".into(),
        },
    ];
    let conv = lowering::input_to_ir(&items);
    assert_eq!(conv.messages.len(), 4);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
    assert_eq!(conv.messages[3].role, IrRole::User);
}

#[test]
fn input_empty_content() {
    let items = vec![CodexInputItem::Message {
        role: "user".into(),
        content: String::new(),
    }];
    let conv = lowering::input_to_ir(&items);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn response_message_to_ir_roundtrip() {
    let items = vec![assistant_text("Hello world")];
    let conv = lowering::to_ir(&items);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);

    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::Message { role, content } => {
            assert_eq!(role, "assistant");
            match &content[0] {
                CodexContentPart::OutputText { text } => assert_eq!(text, "Hello world"),
            }
        }
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn response_reasoning_to_ir_roundtrip() {
    let items = vec![reasoning(&["Step 1: analyze", "Step 2: implement"])];
    let conv = lowering::to_ir(&items);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => {
            assert!(text.contains("Step 1"));
            assert!(text.contains("Step 2"));
        }
        other => panic!("expected Thinking, got {other:?}"),
    }

    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert!(matches!(&back[0], CodexResponseItem::Reasoning { .. }));
}

#[test]
fn response_empty_reasoning_produces_no_ir_text() {
    let items = vec![reasoning(&[])];
    let conv = lowering::to_ir(&items);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => {
            assert!(text.is_empty());
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn system_and_user_skipped_in_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "instructions"),
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::text(IrRole::Assistant, "hi back"),
    ]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        CodexResponseItem::Message { role, .. } => assert_eq!(role, "assistant"),
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn complex_conversation_roundtrip() {
    let items = vec![
        assistant_text("I'll read the file first."),
        function_call("fc_a", "read_file", r#"{"path":"main.rs"}"#),
        function_call_output("fc_a", "fn main() {}"),
        assistant_text("Now I'll modify it."),
        function_call(
            "fc_b",
            "write_file",
            r#"{"path":"main.rs","content":"fn main() { println!(\"hello\"); }"}"#,
        ),
    ];
    let conv = lowering::to_ir(&items);
    assert_eq!(conv.messages.len(), 5);

    let back = lowering::from_ir(&conv);
    // System/user skipped, so we get all assistant + tool items
    assert_eq!(back.len(), 5);
}

#[test]
fn assistant_with_interleaved_text_and_tool_use() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me search...".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "error"}),
            },
            IrContentBlock::Text {
                text: "Found it!".into(),
            },
        ],
    )]);
    let items = lowering::from_ir(&conv);
    // Should produce: Message("Let me search..."), FunctionCall, Message("Found it!")
    assert_eq!(items.len(), 3);
    assert!(matches!(&items[0], CodexResponseItem::Message { .. }));
    assert!(matches!(&items[1], CodexResponseItem::FunctionCall { .. }));
    assert!(matches!(&items[2], CodexResponseItem::Message { .. }));
}

#[test]
fn usage_to_ir_conversion() {
    let usage = CodexUsage {
        input_tokens: 500,
        output_tokens: 200,
        total_tokens: 700,
    };
    let ir = lowering::usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 500);
    assert_eq!(ir.output_tokens, 200);
    assert_eq!(ir.total_tokens, 700);
}

#[test]
fn empty_response_produces_no_events() {
    let resp = make_response(vec![]);
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn empty_items_ir_roundtrip() {
    let conv = lowering::to_ir(&[]);
    assert!(conv.is_empty());
    let back = lowering::from_ir(&conv);
    assert!(back.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional edge cases and serde tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_request_serde_roundtrip() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "hello".into(),
        }],
        max_output_tokens: Some(1024),
        temperature: Some(0.5),
        tools: vec![CodexTool::CodeInterpreter {}],
        text: Some(CodexTextFormat::Text {}),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CodexRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "codex-mini-latest");
    assert_eq!(back.max_output_tokens, Some(1024));
    assert_eq!(back.temperature, Some(0.5));
    assert_eq!(back.tools.len(), 1);
}

#[test]
fn codex_response_serde_roundtrip() {
    let resp = make_response_with_usage(
        vec![assistant_text("ok")],
        CodexUsage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
        },
    );
    let json = serde_json::to_string(&resp).unwrap();
    let back: CodexResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "resp_usage");
    assert_eq!(back.output.len(), 1);
    assert_eq!(back.usage.as_ref().unwrap().total_tokens, 15);
}

#[test]
fn codex_stream_event_serde_roundtrip_text_delta() {
    let event = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::OutputTextDelta {
            text: "chunk".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CodexStreamEvent = serde_json::from_str(&json).unwrap();
    match back {
        CodexStreamEvent::OutputItemDelta { delta, .. } => match delta {
            CodexStreamDelta::OutputTextDelta { text } => assert_eq!(text, "chunk"),
            other => panic!("expected OutputTextDelta, got {other:?}"),
        },
        other => panic!("expected OutputItemDelta, got {other:?}"),
    }
}

#[test]
fn codex_tool_serde_function() {
    let tool = CodexTool::Function {
        function: CodexFunctionDef {
            name: "my_func".into(),
            description: "does stuff".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["type"], "function");
    assert_eq!(json["function"]["name"], "my_func");
}

#[test]
fn codex_tool_serde_code_interpreter() {
    let tool = CodexTool::CodeInterpreter {};
    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["type"], "code_interpreter");
}

#[test]
fn codex_tool_serde_file_search() {
    let tool = CodexTool::FileSearch {
        max_num_results: Some(10),
    };
    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["type"], "file_search");
    assert_eq!(json["max_num_results"], 10);
}

#[test]
fn deterministic_btreemap_serialization() {
    let mut env = BTreeMap::new();
    env.insert("Z_VAR".to_string(), "last".to_string());
    env.insert("A_VAR".to_string(), "first".to_string());
    env.insert("M_VAR".to_string(), "middle".to_string());

    let sb = SandboxConfig {
        env,
        ..SandboxConfig::default()
    };
    let json = serde_json::to_string(&sb).unwrap();
    let a_pos = json.find("A_VAR").unwrap();
    let m_pos = json.find("M_VAR").unwrap();
    let z_pos = json.find("Z_VAR").unwrap();
    assert!(a_pos < m_pos, "BTreeMap should serialize A before M");
    assert!(m_pos < z_pos, "BTreeMap should serialize M before Z");
}

#[test]
fn network_access_none_serde() {
    let na = NetworkAccess::None;
    let json = serde_json::to_value(&na).unwrap();
    assert_eq!(json, json!("none"));
}

#[test]
fn network_access_full_serde() {
    let na = NetworkAccess::Full;
    let json = serde_json::to_value(&na).unwrap();
    assert_eq!(json, json!("full"));
}

#[test]
fn file_access_workspace_only_serde() {
    let fa = FileAccess::WorkspaceOnly;
    let json = serde_json::to_value(&fa).unwrap();
    assert_eq!(json, json!("workspace_only"));
}

#[test]
fn dialect_codex_label() {
    assert_eq!(Dialect::Codex.label(), "Codex");
}

#[test]
fn dialect_codex_display() {
    assert_eq!(format!("{}", Dialect::Codex), "Codex");
}

#[test]
fn dialect_codex_in_all() {
    assert!(Dialect::all().contains(&Dialect::Codex));
}

#[test]
fn dialect_codex_serde_roundtrip() {
    let json = serde_json::to_string(&Dialect::Codex).unwrap();
    assert_eq!(json, r#""codex""#);
    let back: Dialect = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Dialect::Codex);
}

#[test]
fn mapping_rule_codex_construct() {
    let rule = MappingRule {
        source_dialect: Dialect::Codex,
        target_dialect: Dialect::OpenAi,
        feature: "custom_feature".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "schema differences".into(),
        },
    };
    assert_eq!(rule.source_dialect, Dialect::Codex);
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
}

#[test]
fn mapping_registry_insert_and_lookup_codex() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Codex,
        target_dialect: Dialect::Claude,
        feature: "custom".into(),
        fidelity: Fidelity::Lossless,
    });
    let rule = reg
        .lookup(Dialect::Codex, Dialect::Claude, "custom")
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn codex_backend_name() {
    assert_eq!(abp_codex_sdk::BACKEND_NAME, "sidecar:codex");
}
