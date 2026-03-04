// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep surface-area tests for the Codex shim — validates that ABP faithfully
//! mirrors the OpenAI Codex / Responses API wire format, conversions, streaming,
//! function calling, model names, sandbox configuration, client setup,
//! request → WorkOrder, Receipt → response, dialect detection, and OpenAI
//! compatibility.

use abp_codex_sdk::dialect::{
    self, CanonicalToolDef, CodexConfig, CodexContentPart, CodexInputItem, CodexRequest,
    CodexResponse, CodexResponseItem, CodexStreamDelta, CodexStreamEvent, CodexTextFormat,
    CodexTool, CodexUsage, FileAccess, NetworkAccess, ReasoningSummary, SandboxConfig,
};
use abp_codex_sdk::lowering;
use abp_core::ir::{IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, UsageNormalized, WorkOrderBuilder};
use abp_shim_codex::client::Client;
use abp_shim_codex::{
    CodexClient, CodexRequestBuilder, ShimError, Usage, codex_message, events_to_stream_events,
    ir_to_response_items, ir_usage_to_usage, mock_receipt, mock_receipt_with_usage,
    receipt_to_response, request_to_ir, request_to_work_order, response_to_ir,
};
use chrono::Utc;
use serde_json::json;
use std::time::Duration;
use tokio_stream::StreamExt;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn make_processor(events: Vec<AgentEvent>) -> abp_shim_codex::ProcessFn {
    Box::new(move |_wo| mock_receipt(events.clone()))
}

fn make_processor_with_usage(
    events: Vec<AgentEvent>,
    usage: UsageNormalized,
) -> abp_shim_codex::ProcessFn {
    Box::new(move |_wo| mock_receipt_with_usage(events.clone(), usage.clone()))
}

fn assistant_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn tool_call_event(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.to_string(),
            tool_use_id: Some(id.to_string()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn error_event(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.to_string(),
            error_code: None,
        },
        ext: None,
    }
}

fn sample_usage() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Request format — Chat completions matching OpenAI format (7 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t01_request_serializes_all_required_fields() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Hello".into(),
        }],
        max_output_tokens: Some(2048),
        temperature: Some(0.7),
        tools: vec![],
        text: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"model\":\"codex-mini-latest\""));
    assert!(json.contains("\"max_output_tokens\":2048"));
    assert!(json.contains("\"temperature\":0.7"));
}

#[test]
fn t02_request_omits_optional_none_fields() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![],
        max_output_tokens: None,
        temperature: None,
        tools: vec![],
        text: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("max_output_tokens"));
    assert!(!json.contains("temperature"));
    assert!(!json.contains("text"));
}

#[test]
fn t03_request_json_roundtrip() {
    let req = CodexRequest {
        model: "o4-mini".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Fix bug".into(),
        }],
        max_output_tokens: Some(4096),
        temperature: Some(0.5),
        tools: vec![],
        text: Some(CodexTextFormat::JsonObject {}),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CodexRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "o4-mini");
    assert_eq!(back.max_output_tokens, Some(4096));
    assert_eq!(back.temperature, Some(0.5));
}

#[test]
fn t04_request_input_item_message_tagged_serde() {
    let item = CodexInputItem::Message {
        role: "system".into(),
        content: "Be helpful".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains("\"type\":\"message\""));
    let back: CodexInputItem = serde_json::from_str(&json).unwrap();
    match back {
        CodexInputItem::Message { role, content } => {
            assert_eq!(role, "system");
            assert_eq!(content, "Be helpful");
        }
    }
}

#[test]
fn t05_request_with_tools_serialization() {
    let tool = CodexTool::Function {
        function: dialect::CodexFunctionDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        },
    };
    let req = CodexRequestBuilder::new()
        .tools(vec![tool])
        .input(vec![codex_message("user", "test")])
        .build();
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("read_file"));
    assert!(json.contains("\"type\":\"function\""));
}

#[test]
fn t06_request_with_text_format_json_schema() {
    let req = CodexRequestBuilder::new()
        .text(CodexTextFormat::JsonSchema {
            name: "output".into(),
            schema: json!({"type": "object", "properties": {"result": {"type": "string"}}}),
            strict: true,
        })
        .input(vec![codex_message("user", "test")])
        .build();
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("json_schema"));
    assert!(json.contains("\"strict\":true"));
}

#[test]
fn t07_request_multiple_input_messages() {
    let req = CodexRequestBuilder::new()
        .input(vec![
            codex_message("system", "Be concise"),
            codex_message("user", "Hello"),
            codex_message("assistant", "Hi"),
            codex_message("user", "How are you?"),
        ])
        .build();
    assert_eq!(req.input.len(), 4);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Response format — Same as OpenAI responses (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t08_response_serde_roundtrip() {
    let resp = CodexResponse {
        id: "resp_abc".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Done".into(),
            }],
        }],
        usage: Some(CodexUsage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
        }),
        status: Some("completed".into()),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: CodexResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "resp_abc");
    assert_eq!(back.usage.as_ref().unwrap().total_tokens, 15);
    assert_eq!(back.status.as_deref(), Some("completed"));
}

#[test]
fn t09_response_item_function_call_roundtrip() {
    let item = CodexResponseItem::FunctionCall {
        id: "fc_1".into(),
        call_id: Some("corr_1".into()),
        name: "shell".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains("\"type\":\"function_call\""));
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    match back {
        CodexResponseItem::FunctionCall { id, name, .. } => {
            assert_eq!(id, "fc_1");
            assert_eq!(name, "shell");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn t10_response_item_function_call_output_roundtrip() {
    let item = CodexResponseItem::FunctionCallOutput {
        call_id: "fc_1".into(),
        output: "file.txt\nREADME.md".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    match back {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "fc_1");
            assert!(output.contains("file.txt"));
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

#[test]
fn t11_response_item_reasoning_roundtrip() {
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
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    match back {
        CodexResponseItem::Reasoning { summary } => {
            assert_eq!(summary.len(), 2);
            assert_eq!(summary[0].text, "Step 1: analyze");
        }
        other => panic!("expected Reasoning, got {other:?}"),
    }
}

#[test]
fn t12_response_usage_field_preservation() {
    let usage = CodexUsage {
        input_tokens: 42,
        output_tokens: 17,
        total_tokens: 59,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: CodexUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.input_tokens, 42);
    assert_eq!(back.output_tokens, 17);
    assert_eq!(back.total_tokens, 59);
}

#[test]
fn t13_response_with_no_usage() {
    let resp = CodexResponse {
        id: "resp_1".into(),
        model: "codex-mini-latest".into(),
        output: vec![],
        usage: None,
        status: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: CodexResponse = serde_json::from_str(&json).unwrap();
    assert!(back.usage.is_none());
    assert!(back.status.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Streaming — SSE format matching OpenAI (7 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t14_stream_events_bookend_created_completed() {
    let events = vec![assistant_event("hi")];
    let stream = events_to_stream_events(&events, "codex-mini-latest");
    assert!(matches!(
        &stream[0],
        CodexStreamEvent::ResponseCreated { .. }
    ));
    assert!(matches!(
        stream.last().unwrap(),
        CodexStreamEvent::ResponseCompleted { .. }
    ));
}

#[test]
fn t15_stream_delta_mapped_to_output_text_delta() {
    let events = vec![delta_event("chunk")];
    let stream = events_to_stream_events(&events, "codex-mini-latest");
    assert_eq!(stream.len(), 3); // created + delta + completed
    match &stream[1] {
        CodexStreamEvent::OutputItemDelta { delta, .. } => match delta {
            CodexStreamDelta::OutputTextDelta { text } => assert_eq!(text, "chunk"),
            other => panic!("expected OutputTextDelta, got {other:?}"),
        },
        other => panic!("expected OutputItemDelta, got {other:?}"),
    }
}

#[test]
fn t16_stream_message_becomes_output_item_done() {
    let events = vec![assistant_event("complete")];
    let stream = events_to_stream_events(&events, "codex-mini-latest");
    match &stream[1] {
        CodexStreamEvent::OutputItemDone { item, .. } => match item {
            CodexResponseItem::Message { content, .. } => match &content[0] {
                CodexContentPart::OutputText { text } => assert_eq!(text, "complete"),
            },
            other => panic!("expected Message, got {other:?}"),
        },
        other => panic!("expected OutputItemDone, got {other:?}"),
    }
}

#[test]
fn t17_stream_tool_call_in_stream() {
    let events = vec![tool_call_event("bash", "fc_t", json!({"cmd": "echo hi"}))];
    let stream = events_to_stream_events(&events, "codex-mini-latest");
    match &stream[1] {
        CodexStreamEvent::OutputItemDone { item, .. } => match item {
            CodexResponseItem::FunctionCall { name, id, .. } => {
                assert_eq!(name, "bash");
                assert_eq!(id, "fc_t");
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        },
        other => panic!("expected OutputItemDone, got {other:?}"),
    }
}

#[test]
fn t18_stream_empty_trace_only_bookends() {
    let stream = events_to_stream_events(&[], "codex-mini-latest");
    assert_eq!(stream.len(), 2);
}

#[test]
fn t19_stream_status_in_progress_on_created() {
    let stream = events_to_stream_events(&[], "codex-mini-latest");
    match &stream[0] {
        CodexStreamEvent::ResponseCreated { response } => {
            assert_eq!(response.status.as_deref(), Some("in_progress"));
        }
        other => panic!("expected ResponseCreated, got {other:?}"),
    }
}

#[test]
fn t20_stream_status_completed_on_completed() {
    let stream = events_to_stream_events(&[], "codex-mini-latest");
    match stream.last().unwrap() {
        CodexStreamEvent::ResponseCompleted { response } => {
            assert_eq!(response.status.as_deref(), Some("completed"));
        }
        other => panic!("expected ResponseCompleted, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Model names — codex-mini, o1, o3, o4-mini (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t21_known_model_codex_mini_latest() {
    assert!(dialect::is_known_model("codex-mini-latest"));
}

#[test]
fn t22_known_model_o3_mini() {
    assert!(dialect::is_known_model("o3-mini"));
}

#[test]
fn t23_known_model_o4_mini() {
    assert!(dialect::is_known_model("o4-mini"));
}

#[test]
fn t24_known_model_gpt4o() {
    assert!(dialect::is_known_model("gpt-4o"));
}

#[test]
fn t25_canonical_model_mapping_roundtrip() {
    for model in &[
        "codex-mini-latest",
        "o3-mini",
        "o4-mini",
        "gpt-4",
        "gpt-4.1",
    ] {
        let canonical = dialect::to_canonical_model(model);
        assert!(canonical.starts_with("openai/"));
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(&back, model);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Sandbox mode — Codex-specific sandbox execution (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t26_sandbox_config_defaults() {
    let cfg = SandboxConfig::default();
    assert_eq!(cfg.networking, NetworkAccess::None);
    assert_eq!(cfg.file_access, FileAccess::WorkspaceOnly);
    assert_eq!(cfg.timeout_seconds, Some(300));
    assert_eq!(cfg.memory_mb, Some(512));
    assert!(cfg.container_image.is_none());
    assert!(cfg.env.is_empty());
}

#[test]
fn t27_sandbox_config_serde_roundtrip() {
    let mut env = std::collections::BTreeMap::new();
    env.insert("NODE_ENV".into(), "production".into());
    let cfg = SandboxConfig {
        container_image: Some("node:20".into()),
        networking: NetworkAccess::Full,
        file_access: FileAccess::Full,
        timeout_seconds: Some(600),
        memory_mb: Some(1024),
        env,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: SandboxConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.container_image.as_deref(), Some("node:20"));
    assert_eq!(back.networking, NetworkAccess::Full);
    assert_eq!(back.file_access, FileAccess::Full);
    assert_eq!(back.timeout_seconds, Some(600));
    assert_eq!(back.memory_mb, Some(1024));
    assert_eq!(back.env.get("NODE_ENV").unwrap(), "production");
}

#[test]
fn t28_sandbox_network_allow_list_serde() {
    let cfg = SandboxConfig {
        networking: NetworkAccess::AllowList(vec!["api.example.com".into()]),
        ..SandboxConfig::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: SandboxConfig = serde_json::from_str(&json).unwrap();
    match back.networking {
        NetworkAccess::AllowList(hosts) => {
            assert_eq!(hosts.len(), 1);
            assert_eq!(hosts[0], "api.example.com");
        }
        other => panic!("expected AllowList, got {other:?}"),
    }
}

#[test]
fn t29_sandbox_file_access_read_only_external() {
    let cfg = SandboxConfig {
        file_access: FileAccess::ReadOnlyExternal,
        ..SandboxConfig::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: SandboxConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.file_access, FileAccess::ReadOnlyExternal);
}

#[test]
fn t30_sandbox_codex_config_includes_sandbox() {
    let cfg = CodexConfig::default();
    assert_eq!(cfg.sandbox.networking, NetworkAccess::None);
    assert_eq!(cfg.sandbox.file_access, FileAccess::WorkspaceOnly);
    assert_eq!(cfg.sandbox.timeout_seconds, Some(300));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Client configuration — API key, base URL (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t31_client_default_base_url() {
    let client = Client::new("sk-test-key").unwrap();
    assert_eq!(client.base_url(), "https://api.openai.com/v1");
}

#[test]
fn t32_client_custom_base_url() {
    let client = Client::builder("sk-key")
        .base_url("https://custom.api.example.com/v1")
        .build()
        .unwrap();
    assert_eq!(client.base_url(), "https://custom.api.example.com/v1");
}

#[test]
fn t33_client_custom_timeout() {
    let client = Client::builder("sk-key")
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap();
    // Construction should succeed — no public getter for timeout, just verifying build works.
    assert_eq!(client.base_url(), "https://api.openai.com/v1");
}

#[test]
fn t34_codex_config_default_api_settings() {
    let cfg = CodexConfig::default();
    assert!(cfg.base_url.contains("openai.com"));
    assert_eq!(cfg.model, "codex-mini-latest");
    assert_eq!(cfg.max_output_tokens, Some(4096));
    assert!(cfg.api_key.is_empty());
    assert!(cfg.temperature.is_none());
}

#[test]
fn t35_codex_client_model_accessor() {
    let client = CodexClient::new("o3-mini");
    assert_eq!(client.model(), "o3-mini");
}

#[test]
fn t36_codex_client_debug_impl() {
    let client = CodexClient::new("codex-mini-latest");
    let debug = format!("{client:?}");
    assert!(debug.contains("codex-mini-latest"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Request → WorkOrder (7 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t37_request_to_work_order_basic_task() {
    let req = CodexRequestBuilder::new()
        .model("codex-mini-latest")
        .input(vec![codex_message("user", "Fix the bug")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Fix the bug");
    assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
}

#[test]
fn t38_request_to_work_order_extracts_last_user_message() {
    let req = CodexRequestBuilder::new()
        .input(vec![
            codex_message("user", "first"),
            codex_message("assistant", "ok"),
            codex_message("user", "second"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "second");
}

#[test]
fn t39_request_to_work_order_system_not_task() {
    let req = CodexRequestBuilder::new()
        .input(vec![
            codex_message("system", "Be concise"),
            codex_message("user", "Actual task"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Actual task");
}

#[test]
fn t40_request_to_work_order_empty_input_default_task() {
    let req = CodexRequestBuilder::new().build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "codex completion");
}

#[test]
fn t41_request_to_work_order_temperature_in_vendor() {
    let req = CodexRequestBuilder::new()
        .temperature(0.9)
        .input(vec![codex_message("user", "test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(0.9))
    );
}

#[test]
fn t42_request_to_work_order_max_tokens_in_vendor() {
    let req = CodexRequestBuilder::new()
        .max_output_tokens(8192)
        .input(vec![codex_message("user", "test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("max_output_tokens"),
        Some(&serde_json::Value::from(8192))
    );
}

#[test]
fn t43_request_to_work_order_unique_ids() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "test")])
        .build();
    let wo1 = request_to_work_order(&req);
    let wo2 = request_to_work_order(&req);
    assert_ne!(wo1.id, wo2.id);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Receipt → Response (7 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t44_receipt_to_response_assistant_message() {
    let receipt = mock_receipt(vec![assistant_event("Hello!")]);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    assert_eq!(resp.output.len(), 1);
    match &resp.output[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => assert_eq!(text, "Hello!"),
        },
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn t45_receipt_to_response_model_preserved() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "o4-mini");
    assert_eq!(resp.model, "o4-mini");
}

#[test]
fn t46_receipt_to_response_status_completed() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    assert_eq!(resp.status.as_deref(), Some("completed"));
}

#[test]
fn t47_receipt_to_response_id_format() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    assert!(resp.id.starts_with("resp_"));
    assert!(resp.id.contains(&receipt.meta.run_id.to_string()));
}

#[test]
fn t48_receipt_to_response_tool_call() {
    let receipt = mock_receipt(vec![tool_call_event(
        "read",
        "fc_99",
        json!({"path": "main.rs"}),
    )]);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    match &resp.output[0] {
        CodexResponseItem::FunctionCall {
            id,
            name,
            arguments,
            ..
        } => {
            assert_eq!(id, "fc_99");
            assert_eq!(name, "read");
            assert!(arguments.contains("main.rs"));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn t49_receipt_to_response_error_event() {
    let receipt = mock_receipt(vec![error_event("rate limit exceeded")]);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    match &resp.output[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => {
                assert!(text.contains("rate limit"));
            }
        },
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn t50_receipt_to_response_usage_mapping() {
    let receipt = mock_receipt_with_usage(vec![], sample_usage());
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    let u = resp.usage.unwrap();
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Dialect detection — Identify Codex dialect (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t51_dialect_version_constant() {
    assert_eq!(dialect::DIALECT_VERSION, "codex/v0.1");
}

#[test]
fn t52_default_model_constant() {
    assert_eq!(dialect::DEFAULT_MODEL, "codex-mini-latest");
}

#[test]
fn t53_capability_manifest_has_streaming() {
    use abp_core::{Capability, SupportLevel};
    let manifest = dialect::capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn t54_capability_manifest_has_tool_capabilities() {
    use abp_core::{Capability, SupportLevel};
    let manifest = dialect::capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        manifest.get(&Capability::ToolWrite),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        manifest.get(&Capability::ToolBash),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn t55_capability_manifest_mcp_unsupported() {
    use abp_core::{Capability, SupportLevel};
    let manifest = dialect::capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::McpClient),
        Some(SupportLevel::Unsupported)
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. OpenAI compatibility — Exact compatibility with OpenAI format (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t56_openai_compatible_request_to_ir() {
    let req = CodexRequestBuilder::new()
        .input(vec![
            codex_message("system", "System prompt"),
            codex_message("user", "Hello"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.messages.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "System prompt");
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[1].text_content(), "Hello");
}

#[test]
fn t57_openai_compatible_response_to_ir_roundtrip() {
    let resp = CodexResponse {
        id: "resp_1".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Done".into(),
            }],
        }],
        usage: None,
        status: None,
    };
    let conv = response_to_ir(&resp);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    let back = ir_to_response_items(&conv);
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => assert_eq!(text, "Done"),
        },
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn t58_openai_compatible_function_call_ir_roundtrip() {
    let items = vec![CodexResponseItem::FunctionCall {
        id: "fc_42".into(),
        call_id: None,
        name: "read".into(),
        arguments: r#"{"path":"a.rs"}"#.into(),
    }];
    let conv = lowering::to_ir(&items);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::FunctionCall { id, name, .. } => {
            assert_eq!(id, "fc_42");
            assert_eq!(name, "read");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn t59_openai_compatible_ir_usage_conversion() {
    let ir = IrUsage::from_io(200, 100);
    let usage = ir_usage_to_usage(&ir);
    assert_eq!(usage.input_tokens, 200);
    assert_eq!(usage.output_tokens, 100);
    assert_eq!(usage.total_tokens, 300);
}

#[test]
fn t60_openai_compatible_shim_usage_serde() {
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, usage);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Additional tool use tests (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t61_tool_def_canonical_roundtrip() {
    let canonical = CanonicalToolDef {
        name: "search".into(),
        description: "Search files".into(),
        parameters_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    let codex_def = dialect::tool_def_to_codex(&canonical);
    assert_eq!(codex_def.tool_type, "function");
    let back = dialect::tool_def_from_codex(&codex_def);
    assert_eq!(back.name, "search");
    assert_eq!(back.parameters_schema, canonical.parameters_schema);
}

#[test]
fn t62_code_interpreter_tool_canonical() {
    let tool = CodexTool::CodeInterpreter {};
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "code_interpreter");
    assert_eq!(
        canonical.description,
        "Execute code in a sandboxed environment"
    );
}

#[test]
fn t63_file_search_tool_canonical() {
    let tool = CodexTool::FileSearch {
        max_num_results: Some(10),
    };
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "file_search");
}

#[test]
fn t64_tool_call_without_id_gets_generated() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        ext: None,
    }];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    match &resp.output[0] {
        CodexResponseItem::FunctionCall { id, .. } => {
            assert!(id.starts_with("fc_"));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn t65_multi_tool_calls_in_response() {
    let events = vec![
        tool_call_event("read", "fc_1", json!({"path": "a.rs"})),
        tool_call_event("read", "fc_2", json!({"path": "b.rs"})),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    assert_eq!(resp.output.len(), 2);
    assert!(matches!(
        &resp.output[0],
        CodexResponseItem::FunctionCall { .. }
    ));
    assert!(matches!(
        &resp.output[1],
        CodexResponseItem::FunctionCall { .. }
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Client roundtrip tests (async) (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t66_client_create_roundtrip() {
    let client = CodexClient::new("codex-mini-latest")
        .with_processor(make_processor(vec![assistant_event("Hello!")]));
    let req = CodexRequestBuilder::new()
        .model("codex-mini-latest")
        .input(vec![codex_message("user", "Hi")])
        .build();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.model, "codex-mini-latest");
    assert_eq!(resp.output.len(), 1);
}

#[tokio::test]
async fn t67_client_stream_collects_all_events() {
    let events = vec![delta_event("a"), delta_event("b"), delta_event("c")];
    let client = CodexClient::new("codex-mini-latest").with_processor(make_processor(events));
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "test")])
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<CodexStreamEvent> = stream.collect().await;
    // 1 created + 3 deltas + 1 completed
    assert_eq!(chunks.len(), 5);
}

#[tokio::test]
async fn t68_client_no_processor_create_error() {
    let client = CodexClient::new("codex-mini-latest");
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "test")])
        .build();
    let err = client.create(req).await.unwrap_err();
    match err {
        ShimError::Internal(msg) => assert!(msg.contains("processor")),
        other => panic!("expected Internal error, got {other:?}"),
    }
}

#[tokio::test]
async fn t69_client_no_processor_stream_error() {
    let client = CodexClient::new("codex-mini-latest");
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "test")])
        .build();
    assert!(client.create_stream(req).await.is_err());
}

#[tokio::test]
async fn t70_client_with_usage_tracking() {
    let client = CodexClient::new("codex-mini-latest").with_processor(make_processor_with_usage(
        vec![assistant_event("ok")],
        sample_usage(),
    ));
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "test")])
        .build();
    let resp = client.create(req).await.unwrap();
    let u = resp.usage.unwrap();
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Edge cases and additional coverage (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t71_receipt_skips_run_started_completed_events() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    assert!(resp.output.is_empty());
}

#[test]
fn t72_large_task_preserved() {
    let large = "x".repeat(100_000);
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", &large)])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task.len(), 100_000);
}

#[test]
fn t73_unknown_model_passes_through() {
    let req = CodexRequestBuilder::new()
        .model("future-model-v99")
        .input(vec![codex_message("user", "test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("future-model-v99"));
    assert!(!dialect::is_known_model("future-model-v99"));
}

#[test]
fn t74_mixed_events_produce_correct_output() {
    let events = vec![
        assistant_event("msg1"),
        tool_call_event("write", "fc_a", json!({"path": "out.txt"})),
        assistant_event("msg2"),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    assert_eq!(resp.output.len(), 3);
}

#[test]
fn t75_builder_default_model_is_codex_mini() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "test")])
        .build();
    assert_eq!(req.model, "codex-mini-latest");
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Dialect mapping — WorkOrder → CodexRequest (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t76_map_work_order_uses_task_as_user_message() {
    let wo = WorkOrderBuilder::new("Write tests").build();
    let cfg = CodexConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.input.len(), 1);
    match &req.input[0] {
        CodexInputItem::Message { role, content } => {
            assert_eq!(role, "user");
            assert!(content.contains("Write tests"));
        }
    }
}

#[test]
fn t77_map_work_order_respects_model_override() {
    let wo = WorkOrderBuilder::new("task").model("o3-mini").build();
    let cfg = CodexConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "o3-mini");
}

#[test]
fn t78_map_work_order_uses_config_defaults() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CodexConfig {
        max_output_tokens: Some(2048),
        temperature: Some(0.3),
        ..CodexConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.max_output_tokens, Some(2048));
    assert_eq!(req.temperature, Some(0.3));
}

#[test]
fn t79_map_response_produces_agent_events() {
    let resp = CodexResponse {
        id: "resp_1".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Done!".into(),
            }],
        }],
        usage: None,
        status: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Done!"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn t80_map_stream_event_created_produces_run_started() {
    let event = CodexStreamEvent::ResponseCreated {
        response: CodexResponse {
            id: "resp_x".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: Some("in_progress".into()),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Stream delta serde (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t81_stream_delta_output_text_serde() {
    let delta = CodexStreamDelta::OutputTextDelta {
        text: "chunk".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let back: CodexStreamDelta = serde_json::from_str(&json).unwrap();
    match back {
        CodexStreamDelta::OutputTextDelta { text } => assert_eq!(text, "chunk"),
        other => panic!("expected OutputTextDelta, got {other:?}"),
    }
}

#[test]
fn t82_stream_delta_function_args_serde() {
    let delta = CodexStreamDelta::FunctionCallArgumentsDelta {
        delta: r#"{"pa"#.into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let back: CodexStreamDelta = serde_json::from_str(&json).unwrap();
    match back {
        CodexStreamDelta::FunctionCallArgumentsDelta { delta } => {
            assert!(delta.contains("pa"));
        }
        other => panic!("expected FunctionCallArgumentsDelta, got {other:?}"),
    }
}

#[test]
fn t83_stream_delta_reasoning_summary_serde() {
    let delta = CodexStreamDelta::ReasoningSummaryDelta {
        text: "thinking...".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let back: CodexStreamDelta = serde_json::from_str(&json).unwrap();
    match back {
        CodexStreamDelta::ReasoningSummaryDelta { text } => assert_eq!(text, "thinking..."),
        other => panic!("expected ReasoningSummaryDelta, got {other:?}"),
    }
}

#[test]
fn t84_stream_event_response_created_serde() {
    let event = CodexStreamEvent::ResponseCreated {
        response: CodexResponse {
            id: "resp_z".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: Some("in_progress".into()),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CodexStreamEvent = serde_json::from_str(&json).unwrap();
    match back {
        CodexStreamEvent::ResponseCreated { response } => {
            assert_eq!(response.id, "resp_z");
        }
        other => panic!("expected ResponseCreated, got {other:?}"),
    }
}

#[test]
fn t85_stream_event_error_serde() {
    let event = CodexStreamEvent::Error {
        message: "timeout".into(),
        code: Some("timeout_error".into()),
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CodexStreamEvent = serde_json::from_str(&json).unwrap();
    match back {
        CodexStreamEvent::Error { message, code } => {
            assert_eq!(message, "timeout");
            assert_eq!(code.as_deref(), Some("timeout_error"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. Text format variants (3 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t86_text_format_plain_serde() {
    let fmt = CodexTextFormat::Text {};
    let json = serde_json::to_string(&fmt).unwrap();
    let back: CodexTextFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(back, CodexTextFormat::Text {});
}

#[test]
fn t87_text_format_json_object_serde() {
    let fmt = CodexTextFormat::JsonObject {};
    let json = serde_json::to_string(&fmt).unwrap();
    let back: CodexTextFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(back, CodexTextFormat::JsonObject {});
}

#[test]
fn t88_text_format_json_schema_serde() {
    let fmt = CodexTextFormat::JsonSchema {
        name: "out".into(),
        schema: json!({"type": "object"}),
        strict: true,
    };
    let json = serde_json::to_string(&fmt).unwrap();
    let back: CodexTextFormat = serde_json::from_str(&json).unwrap();
    match back {
        CodexTextFormat::JsonSchema {
            name,
            strict,
            schema,
        } => {
            assert_eq!(name, "out");
            assert!(strict);
            assert!(schema.get("type").is_some());
        }
        other => panic!("expected JsonSchema, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. Lowering and IR roundtrips (4 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t89_lowering_function_call_output_roundtrip() {
    let items = vec![CodexResponseItem::FunctionCallOutput {
        call_id: "fc_42".into(),
        output: "result data".into(),
    }];
    let conv = lowering::to_ir(&items);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "fc_42");
            assert_eq!(output, "result data");
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

#[test]
fn t90_lowering_reasoning_roundtrip() {
    let items = vec![CodexResponseItem::Reasoning {
        summary: vec![ReasoningSummary {
            text: "analyzing...".into(),
        }],
    }];
    let conv = lowering::to_ir(&items);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::Reasoning { summary } => {
            assert_eq!(summary[0].text, "analyzing...");
        }
        other => panic!("expected Reasoning, got {other:?}"),
    }
}

#[test]
fn t91_lowering_empty_items() {
    let conv = lowering::to_ir(&[]);
    assert!(conv.is_empty());
    let back = lowering::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn t92_lowering_system_user_skipped_in_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "instructions"),
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::text(IrRole::Assistant, "hi"),
    ]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        CodexResponseItem::Message { role, .. } => assert_eq!(role, "assistant"),
        other => panic!("expected Message, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Stream model preservation (3 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t93_stream_model_in_created() {
    let stream = events_to_stream_events(&[], "o4-mini");
    match &stream[0] {
        CodexStreamEvent::ResponseCreated { response } => {
            assert_eq!(response.model, "o4-mini");
        }
        other => panic!("expected ResponseCreated, got {other:?}"),
    }
}

#[test]
fn t94_stream_model_in_completed() {
    let stream = events_to_stream_events(&[], "o4-mini");
    match stream.last().unwrap() {
        CodexStreamEvent::ResponseCompleted { response } => {
            assert_eq!(response.model, "o4-mini");
        }
        other => panic!("expected ResponseCompleted, got {other:?}"),
    }
}

#[test]
fn t95_stream_multiple_deltas_count() {
    let events = vec![
        delta_event("a"),
        delta_event("b"),
        delta_event("c"),
        delta_event("d"),
        delta_event("e"),
    ];
    let stream = events_to_stream_events(&events, "codex-mini-latest");
    // 1 created + 5 deltas + 1 completed
    assert_eq!(stream.len(), 7);
}
