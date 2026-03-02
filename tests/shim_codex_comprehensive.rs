// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the `abp-shim-codex` crate.

use abp_codex_sdk::dialect::{
    self, CanonicalToolDef, CodexConfig, CodexContentPart, CodexInputItem, CodexRequest,
    CodexResponse, CodexResponseItem, CodexStreamDelta, CodexStreamEvent, CodexUsage, FileAccess,
    NetworkAccess, ReasoningSummary, capability_manifest, codex_tool_to_canonical,
    from_canonical_model, is_known_model, map_response, map_stream_event, map_work_order,
    to_canonical_model, tool_def_from_codex, tool_def_to_codex,
};
use abp_codex_sdk::lowering;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, SupportLevel, UsageNormalized, WorkOrderBuilder,
};
use abp_shim_codex::{
    CodexClient, CodexFunctionDef, CodexRequestBuilder, CodexShimRequest, CodexTextFormat,
    CodexTool, ProcessFn, SandboxConfig, ShimError, Usage, codex_message, events_to_stream_events,
    ir_to_response_items, ir_usage_to_usage, mock_receipt, mock_receipt_with_usage,
    receipt_to_response, request_to_ir, request_to_work_order, response_to_ir,
};
use chrono::Utc;
use serde_json::json;
use tokio_stream::StreamExt;

// ── Test helpers ────────────────────────────────────────────────────────

fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
    Box::new(move |_wo| mock_receipt(events.clone()))
}

fn make_processor_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> ProcessFn {
    Box::new(move |_wo| mock_receipt_with_usage(events.clone(), usage.clone()))
}

// Use make_processor_with_usage in a test to prevent dead_code warning.
#[test]
fn _ensure_make_processor_with_usage_used() {
    let p = make_processor_with_usage(vec![assistant_event("x")], simple_usage(1, 1));
    let wo = WorkOrderBuilder::new("t").build();
    let receipt = p(&wo);
    assert_eq!(receipt.usage.input_tokens, Some(1));
}

fn assistant_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: text.into() },
        ext: None,
    }
}

fn tool_call_event(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: Some(id.into()),
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
            message: msg.into(),
            error_code: None,
        },
        ext: None,
    }
}

fn simple_usage(input: u64, output: u64) -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Client initialization and configuration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t01_client_new_stores_model() {
    let c = CodexClient::new("codex-mini-latest");
    assert_eq!(c.model(), "codex-mini-latest");
}

#[test]
fn t02_client_new_custom_model() {
    let c = CodexClient::new("o3-mini");
    assert_eq!(c.model(), "o3-mini");
}

#[test]
fn t03_client_debug_impl() {
    let c = CodexClient::new("test-model");
    let dbg = format!("{c:?}");
    assert!(dbg.contains("test-model"));
}

#[tokio::test]
async fn t04_client_no_processor_create_errors() {
    let c = CodexClient::new("m");
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "hi")])
        .build();
    let err = c.create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
}

#[tokio::test]
async fn t05_client_no_processor_stream_errors() {
    let c = CodexClient::new("m");
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "hi")])
        .build();
    assert!(c.create_stream(req).await.is_err());
}

#[tokio::test]
async fn t06_client_with_processor_succeeds() {
    let c = CodexClient::new("m").with_processor(make_processor(vec![assistant_event("ok")]));
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "hi")])
        .build();
    let resp = c.create(req).await.unwrap();
    assert_eq!(resp.output.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Request builder
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t07_builder_default_model() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "x")])
        .build();
    assert_eq!(req.model, "codex-mini-latest");
}

#[test]
fn t08_builder_custom_model() {
    let req = CodexRequestBuilder::new().model("gpt-4").build();
    assert_eq!(req.model, "gpt-4");
}

#[test]
fn t09_builder_temperature() {
    let req = CodexRequestBuilder::new().temperature(0.5).build();
    assert_eq!(req.temperature, Some(0.5));
}

#[test]
fn t10_builder_max_output_tokens() {
    let req = CodexRequestBuilder::new().max_output_tokens(1024).build();
    assert_eq!(req.max_output_tokens, Some(1024));
}

#[test]
fn t11_builder_tools() {
    let tool = CodexTool::Function {
        function: CodexFunctionDef {
            name: "shell".into(),
            description: "run a shell command".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let req = CodexRequestBuilder::new().tools(vec![tool]).build();
    assert_eq!(req.tools.len(), 1);
}

#[test]
fn t12_builder_text_format_plain() {
    let req = CodexRequestBuilder::new()
        .text(CodexTextFormat::Text {})
        .build();
    assert!(req.text.is_some());
}

#[test]
fn t13_builder_text_format_json_schema() {
    let req = CodexRequestBuilder::new()
        .text(CodexTextFormat::JsonSchema {
            name: "output".into(),
            schema: json!({"type": "object"}),
            strict: true,
        })
        .build();
    match req.text.unwrap() {
        CodexTextFormat::JsonSchema { name, strict, .. } => {
            assert_eq!(name, "output");
            assert!(strict);
        }
        _ => panic!("expected JsonSchema"),
    }
}

#[test]
fn t14_builder_input() {
    let req = CodexRequestBuilder::new()
        .input(vec![
            codex_message("system", "be short"),
            codex_message("user", "hi"),
        ])
        .build();
    assert_eq!(req.input.len(), 2);
}

#[test]
fn t15_codex_message_helper() {
    let msg = codex_message("user", "hello");
    match msg {
        CodexInputItem::Message { role, content } => {
            assert_eq!(role, "user");
            assert_eq!(content, "hello");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Request translation: Codex → IR
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t16_request_to_ir_user_message() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "Hello")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello");
}

#[test]
fn t17_request_to_ir_system_message() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("system", "Be concise.")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn t18_request_to_ir_multiple_messages() {
    let req = CodexRequestBuilder::new()
        .input(vec![
            codex_message("system", "instructions"),
            codex_message("user", "question"),
            codex_message("assistant", "answer"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
}

#[test]
fn t19_request_to_ir_empty() {
    let req = CodexRequestBuilder::new().build();
    let conv = request_to_ir(&req);
    assert!(conv.is_empty());
}

#[test]
fn t20_request_to_ir_unknown_role_maps_to_user() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("developer", "test")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn t21_request_to_ir_empty_content() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "")])
        .build();
    let conv = request_to_ir(&req);
    assert!(conv.messages[0].content.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Request → WorkOrder
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t22_request_to_work_order_model() {
    let req = CodexRequestBuilder::new()
        .model("o3-mini")
        .input(vec![codex_message("user", "test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
}

#[test]
fn t23_request_to_work_order_task_from_user() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "Fix the bug")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Fix the bug");
}

#[test]
fn t24_request_to_work_order_task_last_user() {
    let req = CodexRequestBuilder::new()
        .input(vec![
            codex_message("user", "First"),
            codex_message("assistant", "ok"),
            codex_message("user", "Second"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Second");
}

#[test]
fn t25_request_to_work_order_fallback_task() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("system", "instructions")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "codex completion");
}

#[test]
fn t26_request_to_work_order_temperature() {
    let req = CodexRequestBuilder::new()
        .temperature(0.7)
        .input(vec![codex_message("user", "x")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(0.7))
    );
}

#[test]
fn t27_request_to_work_order_max_tokens() {
    let req = CodexRequestBuilder::new()
        .max_output_tokens(2048)
        .input(vec![codex_message("user", "x")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("max_output_tokens"),
        Some(&serde_json::Value::from(2048))
    );
}

#[test]
fn t28_request_to_work_order_no_vendor_when_unset() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "x")])
        .build();
    let wo = request_to_work_order(&req);
    assert!(!wo.config.vendor.contains_key("temperature"));
    assert!(!wo.config.vendor.contains_key("max_output_tokens"));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Receipt → CodexResponse
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t29_receipt_to_response_assistant_message() {
    let receipt = mock_receipt(vec![assistant_event("Hello!")]);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    assert_eq!(resp.model, "codex-mini-latest");
    assert_eq!(resp.output.len(), 1);
    match &resp.output[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => assert_eq!(text, "Hello!"),
        },
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn t30_receipt_to_response_delta() {
    let receipt = mock_receipt(vec![delta_event("chunk")]);
    let resp = receipt_to_response(&receipt, "m");
    assert_eq!(resp.output.len(), 1);
    match &resp.output[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => assert_eq!(text, "chunk"),
        },
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn t31_receipt_to_response_tool_call() {
    let receipt = mock_receipt(vec![tool_call_event("shell", "fc_1", json!({"cmd": "ls"}))]);
    let resp = receipt_to_response(&receipt, "m");
    match &resp.output[0] {
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
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn t32_receipt_to_response_error_event() {
    let receipt = mock_receipt(vec![error_event("rate limit")]);
    let resp = receipt_to_response(&receipt, "m");
    match &resp.output[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => assert!(text.contains("rate limit")),
        },
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn t33_receipt_to_response_status_completed() {
    let receipt = mock_receipt(vec![assistant_event("ok")]);
    let resp = receipt_to_response(&receipt, "m");
    assert_eq!(resp.status.as_deref(), Some("completed"));
}

#[test]
fn t34_receipt_to_response_id_format() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "m");
    assert!(resp.id.starts_with("resp_"));
}

#[test]
fn t35_receipt_to_response_usage() {
    let receipt = mock_receipt_with_usage(vec![assistant_event("ok")], simple_usage(100, 50));
    let resp = receipt_to_response(&receipt, "m");
    let u = resp.usage.unwrap();
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

#[test]
fn t36_receipt_to_response_zero_usage() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "m");
    let u = resp.usage.unwrap();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn t37_receipt_to_response_mixed_events() {
    let events = vec![
        assistant_event("analyzing"),
        tool_call_event("read", "fc_1", json!({"path": "a.rs"})),
        error_event("oops"),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "m");
    assert_eq!(resp.output.len(), 3);
}

#[test]
fn t38_receipt_to_response_skips_run_events() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        },
        assistant_event("hello"),
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "m");
    assert_eq!(resp.output.len(), 1);
}

#[test]
fn t39_receipt_to_response_tool_call_no_id_generates_one() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "shell".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        ext: None,
    };
    let receipt = mock_receipt(vec![event]);
    let resp = receipt_to_response(&receipt, "m");
    match &resp.output[0] {
        CodexResponseItem::FunctionCall { id, .. } => assert!(id.starts_with("fc_")),
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Response → IR → Response roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t40_response_to_ir_assistant_message() {
    let resp = CodexResponse {
        id: "r1".into(),
        model: "m".into(),
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
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
}

#[test]
fn t41_response_to_ir_and_back() {
    let resp = CodexResponse {
        id: "r1".into(),
        model: "m".into(),
        output: vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Hello".into(),
            }],
        }],
        usage: None,
        status: None,
    };
    let conv = response_to_ir(&resp);
    let back = ir_to_response_items(&conv);
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => assert_eq!(text, "Hello"),
        },
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn t42_ir_to_response_items_skips_system_user() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "usr"),
        IrMessage::text(IrRole::Assistant, "asst"),
    ]);
    let items = ir_to_response_items(&conv);
    assert_eq!(items.len(), 1);
}

#[test]
fn t43_ir_to_response_items_tool_use_block() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        }],
    )]);
    let items = ir_to_response_items(&conv);
    assert_eq!(items.len(), 1);
    assert!(matches!(&items[0], CodexResponseItem::FunctionCall { .. }));
}

#[test]
fn t44_ir_to_response_items_thinking_block() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking { text: "hmm".into() }],
    )]);
    let items = ir_to_response_items(&conv);
    assert_eq!(items.len(), 1);
    assert!(matches!(&items[0], CodexResponseItem::Reasoning { .. }));
}

#[test]
fn t45_ir_to_response_items_tool_result() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "fc_1".into(),
            content: vec![IrContentBlock::Text {
                text: "result".into(),
            }],
            is_error: false,
        }],
    )]);
    let items = ir_to_response_items(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "fc_1");
            assert_eq!(output, "result");
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

#[test]
fn t46_ir_roundtrip_function_call() {
    let resp = CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "read".into(),
            arguments: r#"{"path":"a.rs"}"#.into(),
        }],
        usage: None,
        status: None,
    };
    let conv = response_to_ir(&resp);
    let back = ir_to_response_items(&conv);
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::FunctionCall { name, .. } => assert_eq!(name, "read"),
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn t47_ir_roundtrip_function_call_output() {
    let resp = CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![CodexResponseItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "contents".into(),
        }],
        usage: None,
        status: None,
    };
    let conv = response_to_ir(&resp);
    let back = ir_to_response_items(&conv);
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "fc_1");
            assert_eq!(output, "contents");
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

#[test]
fn t48_ir_roundtrip_reasoning() {
    let resp = CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![CodexResponseItem::Reasoning {
            summary: vec![ReasoningSummary {
                text: "step 1".into(),
            }],
        }],
        usage: None,
        status: None,
    };
    let conv = response_to_ir(&resp);
    let back = ir_to_response_items(&conv);
    assert_eq!(back.len(), 1);
    assert!(matches!(&back[0], CodexResponseItem::Reasoning { .. }));
}

#[test]
fn t49_empty_response_roundtrip() {
    let resp = CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![],
        usage: None,
        status: None,
    };
    let conv = response_to_ir(&resp);
    assert!(conv.is_empty());
    let back = ir_to_response_items(&conv);
    assert!(back.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Streaming
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t50_stream_events_bookends() {
    let events = vec![delta_event("hi")];
    let stream = events_to_stream_events(&events, "m");
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
fn t51_stream_events_delta() {
    let events = vec![delta_event("a"), delta_event("b")];
    let stream = events_to_stream_events(&events, "m");
    // created + 2 deltas + completed
    assert_eq!(stream.len(), 4);
    assert!(matches!(
        &stream[1],
        CodexStreamEvent::OutputItemDelta { .. }
    ));
}

#[test]
fn t52_stream_events_assistant_message() {
    let events = vec![assistant_event("done")];
    let stream = events_to_stream_events(&events, "m");
    assert_eq!(stream.len(), 3);
    assert!(matches!(
        &stream[1],
        CodexStreamEvent::OutputItemDone { .. }
    ));
}

#[test]
fn t53_stream_events_tool_call() {
    let events = vec![tool_call_event("shell", "fc_1", json!({"cmd": "ls"}))];
    let stream = events_to_stream_events(&events, "m");
    assert_eq!(stream.len(), 3);
    match &stream[1] {
        CodexStreamEvent::OutputItemDone { item, .. } => {
            assert!(matches!(item, CodexResponseItem::FunctionCall { .. }));
        }
        other => panic!("expected OutputItemDone, got {other:?}"),
    }
}

#[test]
fn t54_stream_events_empty() {
    let stream = events_to_stream_events(&[], "m");
    assert_eq!(stream.len(), 2); // created + completed
}

#[test]
fn t55_stream_events_model_preserved() {
    let stream = events_to_stream_events(&[], "o3-mini");
    match &stream[0] {
        CodexStreamEvent::ResponseCreated { response } => {
            assert_eq!(response.model, "o3-mini");
        }
        other => panic!("expected ResponseCreated, got {other:?}"),
    }
}

#[test]
fn t56_stream_created_status_in_progress() {
    let stream = events_to_stream_events(&[], "m");
    match &stream[0] {
        CodexStreamEvent::ResponseCreated { response } => {
            assert_eq!(response.status.as_deref(), Some("in_progress"));
        }
        _ => panic!("expected ResponseCreated"),
    }
}

#[test]
fn t57_stream_completed_status_completed() {
    let stream = events_to_stream_events(&[], "m");
    match stream.last().unwrap() {
        CodexStreamEvent::ResponseCompleted { response } => {
            assert_eq!(response.status.as_deref(), Some("completed"));
        }
        _ => panic!("expected ResponseCompleted"),
    }
}

#[tokio::test]
async fn t58_client_create_stream_collects() {
    let events = vec![delta_event("a"), delta_event("b")];
    let c = CodexClient::new("m").with_processor(make_processor(events));
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "hi")])
        .build();
    let stream = c.create_stream(req).await.unwrap();
    let chunks: Vec<_> = stream.collect().await;
    assert_eq!(chunks.len(), 4);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Model mapping (Codex SDK dialect functions)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t59_to_canonical_model() {
    assert_eq!(
        to_canonical_model("codex-mini-latest"),
        "openai/codex-mini-latest"
    );
}

#[test]
fn t60_from_canonical_model_strips_prefix() {
    assert_eq!(
        from_canonical_model("openai/codex-mini-latest"),
        "codex-mini-latest"
    );
}

#[test]
fn t61_from_canonical_model_no_prefix() {
    assert_eq!(from_canonical_model("custom-model"), "custom-model");
}

#[test]
fn t62_is_known_model_true() {
    assert!(is_known_model("codex-mini-latest"));
    assert!(is_known_model("o3-mini"));
    assert!(is_known_model("gpt-4"));
    assert!(is_known_model("gpt-4o"));
}

#[test]
fn t63_is_known_model_false() {
    assert!(!is_known_model("unknown-model"));
    assert!(!is_known_model("claude-3"));
}

#[test]
fn t64_canonical_roundtrip() {
    let m = "o4-mini";
    let canonical = to_canonical_model(m);
    let back = from_canonical_model(&canonical);
    assert_eq!(back, m);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Usage conversion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t65_ir_usage_to_usage() {
    let ir = IrUsage::from_io(200, 100);
    let u = ir_usage_to_usage(&ir);
    assert_eq!(u.input_tokens, 200);
    assert_eq!(u.output_tokens, 100);
    assert_eq!(u.total_tokens, 300);
}

#[test]
fn t66_ir_usage_zero() {
    let ir = IrUsage::from_io(0, 0);
    let u = ir_usage_to_usage(&ir);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn t67_ir_usage_large_values() {
    let ir = IrUsage::from_io(1_000_000, 500_000);
    let u = ir_usage_to_usage(&ir);
    assert_eq!(u.total_tokens, 1_500_000);
}

#[test]
fn t68_usage_struct_equality() {
    let u1 = Usage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
    };
    let u2 = Usage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
    };
    assert_eq!(u1, u2);
}

#[test]
fn t69_codex_usage_to_ir() {
    let cu = CodexUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
    };
    let ir = lowering::usage_to_ir(&cu);
    assert_eq!(ir.input_tokens, 100);
    assert_eq!(ir.output_tokens, 50);
    assert_eq!(ir.total_tokens, 150);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Dialect: map_work_order / map_response
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t70_map_work_order_uses_task() {
    let wo = WorkOrderBuilder::new("Write tests").build();
    let cfg = CodexConfig::default();
    let req = map_work_order(&wo, &cfg);
    match &req.input[0] {
        CodexInputItem::Message { content, .. } => {
            assert!(content.contains("Write tests"));
        }
    }
}

#[test]
fn t71_map_work_order_model_override() {
    let wo = WorkOrderBuilder::new("task").model("o3-mini").build();
    let cfg = CodexConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, "o3-mini");
}

#[test]
fn t72_map_work_order_default_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CodexConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, cfg.model);
}

#[test]
fn t73_map_response_assistant() {
    let resp = CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText { text: "ok".into() }],
        }],
        usage: None,
        status: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[test]
fn t74_map_response_function_call() {
    let resp = CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "shell".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
        }],
        usage: None,
        status: None,
    };
    let events = map_response(&resp);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn t75_map_response_function_call_output() {
    let resp = CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![CodexResponseItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "data".into(),
        }],
        usage: None,
        status: None,
    };
    let events = map_response(&resp);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn t76_map_response_reasoning() {
    let resp = CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![CodexResponseItem::Reasoning {
            summary: vec![ReasoningSummary {
                text: "thinking".into(),
            }],
        }],
        usage: None,
        status: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
}

#[test]
fn t77_map_response_empty_reasoning() {
    let resp = CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![CodexResponseItem::Reasoning { summary: vec![] }],
        usage: None,
        status: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Stream event mapping (SDK dialect)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t78_map_stream_created() {
    let evt = CodexStreamEvent::ResponseCreated {
        response: CodexResponse {
            id: "r".into(),
            model: "m".into(),
            output: vec![],
            usage: None,
            status: None,
        },
    };
    let events = map_stream_event(&evt);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn t79_map_stream_completed() {
    let evt = CodexStreamEvent::ResponseCompleted {
        response: CodexResponse {
            id: "r".into(),
            model: "m".into(),
            output: vec![],
            usage: None,
            status: None,
        },
    };
    let events = map_stream_event(&evt);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn t80_map_stream_in_progress_empty() {
    let evt = CodexStreamEvent::ResponseInProgress {
        response: CodexResponse {
            id: "r".into(),
            model: "m".into(),
            output: vec![],
            usage: None,
            status: None,
        },
    };
    let events = map_stream_event(&evt);
    assert!(events.is_empty());
}

#[test]
fn t81_map_stream_text_delta() {
    let evt = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::OutputTextDelta {
            text: "chunk".into(),
        },
    };
    let events = map_stream_event(&evt);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "chunk"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn t82_map_stream_function_args_delta_empty() {
    let evt = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::FunctionCallArgumentsDelta { delta: "{".into() },
    };
    let events = map_stream_event(&evt);
    assert!(events.is_empty());
}

#[test]
fn t83_map_stream_error() {
    let evt = CodexStreamEvent::Error {
        message: "boom".into(),
        code: Some("500".into()),
    };
    let events = map_stream_event(&evt);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => assert_eq!(message, "boom"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn t84_map_stream_failed() {
    let evt = CodexStreamEvent::ResponseFailed {
        response: CodexResponse {
            id: "r".into(),
            model: "m".into(),
            output: vec![],
            usage: None,
            status: Some("failed".into()),
        },
    };
    let events = map_stream_event(&evt);
    assert!(matches!(&events[0].kind, AgentEventKind::Error { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Tool definitions
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t85_tool_def_to_codex_roundtrip() {
    let canonical = CanonicalToolDef {
        name: "shell".into(),
        description: "run cmd".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let codex = tool_def_to_codex(&canonical);
    assert_eq!(codex.tool_type, "function");
    let back = tool_def_from_codex(&codex);
    assert_eq!(back.name, "shell");
    assert_eq!(back.description, "run cmd");
}

#[test]
fn t86_codex_tool_function_to_canonical() {
    let tool = CodexTool::Function {
        function: CodexFunctionDef {
            name: "search".into(),
            description: "search files".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let c = codex_tool_to_canonical(&tool);
    assert_eq!(c.name, "search");
}

#[test]
fn t87_codex_tool_code_interpreter() {
    let tool = CodexTool::CodeInterpreter {};
    let c = codex_tool_to_canonical(&tool);
    assert_eq!(c.name, "code_interpreter");
}

#[test]
fn t88_codex_tool_file_search() {
    let tool = CodexTool::FileSearch {
        max_num_results: Some(10),
    };
    let c = codex_tool_to_canonical(&tool);
    assert_eq!(c.name, "file_search");
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Capability manifest
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t89_capability_manifest_streaming() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn t90_capability_manifest_tool_read() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn t91_capability_manifest_mcp_unsupported() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpClient),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn t92_capability_manifest_glob_emulated() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolGlob),
        Some(SupportLevel::Emulated)
    ));
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Sandbox and config
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t93_sandbox_default() {
    let s = SandboxConfig::default();
    assert!(s.container_image.is_none());
    assert_eq!(s.networking, NetworkAccess::None);
    assert_eq!(s.file_access, FileAccess::WorkspaceOnly);
    assert_eq!(s.timeout_seconds, Some(300));
    assert_eq!(s.memory_mb, Some(512));
}

#[test]
fn t94_codex_config_default() {
    let cfg = CodexConfig::default();
    assert!(cfg.base_url.contains("openai.com"));
    assert_eq!(cfg.model, "codex-mini-latest");
    assert!(cfg.max_output_tokens.is_some());
}

#[test]
fn t95_text_format_default() {
    let tf = CodexTextFormat::default();
    assert!(matches!(tf, CodexTextFormat::Text {}));
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Error types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t96_shim_error_invalid_request_display() {
    let e = ShimError::InvalidRequest("bad input".into());
    assert!(format!("{e}").contains("bad input"));
}

#[test]
fn t97_shim_error_internal_display() {
    let e = ShimError::Internal("oops".into());
    assert!(format!("{e}").contains("oops"));
}

#[test]
fn t98_shim_error_serde() {
    let bad: std::result::Result<serde_json::Value, _> = serde_json::from_str("{bad}");
    let e: ShimError = bad.unwrap_err().into();
    assert!(matches!(e, ShimError::Serde(_)));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t99_malformed_function_arguments_preserved() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "foo".into(),
            tool_use_id: Some("fc_1".into()),
            parent_tool_use_id: None,
            input: json!("not-an-object"),
        },
        ext: None,
    }];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "m");
    match &resp.output[0] {
        CodexResponseItem::FunctionCall { arguments, .. } => {
            assert!(!arguments.is_empty());
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn t100_unicode_content_roundtrip() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "日本語のテスト 🚀")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.messages[0].text_content(), "日本語のテスト 🚀");
}

#[test]
fn t101_very_long_content() {
    let long = "x".repeat(100_000);
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", &long)])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.messages[0].text_content().len(), 100_000);
}

#[test]
fn t102_multiline_content() {
    let text = "line1\nline2\nline3";
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", text)])
        .build();
    let conv = request_to_ir(&req);
    assert!(conv.messages[0].text_content().contains('\n'));
}

#[test]
fn t103_special_characters_in_tool_args() {
    let input =
        json!({"path": "file with spaces.rs", "content": "fn main() { println!(\"hello\"); }"});
    let events = vec![tool_call_event("write", "fc_1", input.clone())];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "m");
    match &resp.output[0] {
        CodexResponseItem::FunctionCall { arguments, .. } => {
            let parsed: serde_json::Value = serde_json::from_str(arguments).unwrap();
            assert_eq!(parsed["path"], "file with spaces.rs");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[tokio::test]
async fn t104_multiple_requests_same_client() {
    let c = CodexClient::new("m").with_processor(make_processor(vec![assistant_event("ok")]));
    let req1 = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "a")])
        .build();
    let req2 = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "b")])
        .build();
    let r1 = c.create(req1).await.unwrap();
    let r2 = c.create(req2).await.unwrap();
    assert_eq!(r1.output.len(), 1);
    assert_eq!(r2.output.len(), 1);
}

#[test]
fn t105_mock_receipt_has_valid_metadata() {
    let receipt = mock_receipt(vec![]);
    assert!(!receipt.meta.run_id.is_nil());
    assert!(!receipt.meta.work_order_id.is_nil());
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
}

#[test]
fn t106_mock_receipt_backend_identity() {
    let receipt = mock_receipt(vec![]);
    assert_eq!(receipt.backend.id, "mock");
}

#[test]
fn t107_codex_request_serde_roundtrip() {
    let req = CodexRequestBuilder::new()
        .model("codex-mini-latest")
        .input(vec![codex_message("user", "hello")])
        .temperature(0.5)
        .max_output_tokens(1024)
        .build();
    let json = serde_json::to_string(&req).unwrap();
    let back: CodexRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "codex-mini-latest");
    assert_eq!(back.temperature, Some(0.5));
    assert_eq!(back.max_output_tokens, Some(1024));
}

#[test]
fn t108_codex_response_serde_roundtrip() {
    let resp = CodexResponse {
        id: "resp_123".into(),
        model: "m".into(),
        output: vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "hello".into(),
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
    assert_eq!(back.id, "resp_123");
    assert_eq!(back.output.len(), 1);
    assert_eq!(back.usage.unwrap().total_tokens, 15);
}

#[test]
fn t109_stream_event_output_indices() {
    let events = vec![delta_event("a"), delta_event("b"), assistant_event("c")];
    let stream = events_to_stream_events(&events, "m");
    // created, delta(idx=0), delta(idx=1), done(idx=2), completed
    match &stream[1] {
        CodexStreamEvent::OutputItemDelta { output_index, .. } => {
            assert_eq!(*output_index, 0);
        }
        other => panic!("expected OutputItemDelta, got {other:?}"),
    }
    match &stream[2] {
        CodexStreamEvent::OutputItemDelta { output_index, .. } => {
            assert_eq!(*output_index, 1);
        }
        other => panic!("expected OutputItemDelta, got {other:?}"),
    }
}

#[test]
fn t110_stream_events_skip_non_mapped_events() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "a.rs".into(),
                summary: "modified".into(),
            },
            ext: None,
        },
        assistant_event("done"),
    ];
    let stream = events_to_stream_events(&events, "m");
    // created + 1 done (FileChanged skipped) + completed = 3
    assert_eq!(stream.len(), 3);
}

#[test]
fn t111_shim_request_type_alias() {
    // CodexShimRequest is an alias for CodexRequest
    let req: CodexShimRequest = CodexRequestBuilder::new()
        .model("m")
        .input(vec![codex_message("user", "hi")])
        .build();
    assert_eq!(req.model, "m");
}

#[test]
fn t112_work_order_from_empty_input_uses_fallback() {
    let req = CodexRequestBuilder::new().build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "codex completion");
}

#[test]
fn t113_ir_conversation_accessors() {
    let req = CodexRequestBuilder::new()
        .input(vec![
            codex_message("system", "sys"),
            codex_message("user", "usr"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.system_message().unwrap().text_content(), "sys");
    assert!(conv.last_assistant().is_none());
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
}

#[test]
fn t114_receipt_to_response_multiple_deltas() {
    let events = vec![
        delta_event("Hello"),
        delta_event(", "),
        delta_event("world!"),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "m");
    assert_eq!(resp.output.len(), 3);
}

#[test]
fn t115_map_work_order_includes_context_snippets() {
    let wo = WorkOrderBuilder::new("task")
        .context(abp_core::ContextPacket {
            files: vec![],
            snippets: vec![abp_core::ContextSnippet {
                name: "readme".into(),
                content: "important info".into(),
            }],
        })
        .build();
    let cfg = CodexConfig::default();
    let req = map_work_order(&wo, &cfg);
    match &req.input[0] {
        CodexInputItem::Message { content, .. } => {
            assert!(content.contains("readme"));
            assert!(content.contains("important info"));
        }
    }
}

#[test]
fn t116_network_access_variants() {
    let none = NetworkAccess::None;
    let full = NetworkAccess::Full;
    let allow = NetworkAccess::AllowList(vec!["api.openai.com".into()]);
    assert_eq!(none, NetworkAccess::default());
    assert_ne!(none, full);
    assert_ne!(none, allow);
}

#[test]
fn t117_file_access_variants() {
    let ws = FileAccess::WorkspaceOnly;
    assert_eq!(ws, FileAccess::default());
    assert_ne!(ws, FileAccess::Full);
    assert_ne!(ws, FileAccess::ReadOnlyExternal);
}

#[test]
fn t118_dialect_version_constant() {
    assert_eq!(dialect::DIALECT_VERSION, "codex/v0.1");
}

#[test]
fn t119_default_model_constant() {
    assert_eq!(dialect::DEFAULT_MODEL, "codex-mini-latest");
}

#[test]
fn t120_map_stream_output_item_added() {
    let evt = CodexStreamEvent::OutputItemAdded {
        output_index: 0,
        item: CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText { text: "hi".into() }],
        },
    };
    let events = map_stream_event(&evt);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[test]
fn t121_map_stream_output_item_done_function() {
    let evt = CodexStreamEvent::OutputItemDone {
        output_index: 0,
        item: CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "shell".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
        },
    };
    let events = map_stream_event(&evt);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn t122_reasoning_summary_delta_maps_empty() {
    let evt = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::ReasoningSummaryDelta {
            text: "thinking...".into(),
        },
    };
    let events = map_stream_event(&evt);
    assert!(events.is_empty());
}

#[test]
fn t123_codex_tool_serde() {
    let tool = CodexTool::Function {
        function: CodexFunctionDef {
            name: "test".into(),
            description: "desc".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tool);
}

#[test]
fn t124_usage_serde() {
    let u = Usage {
        input_tokens: 42,
        output_tokens: 17,
        total_tokens: 59,
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, u);
}

#[test]
fn t125_response_item_reasoning_empty_summary() {
    let resp = CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![CodexResponseItem::Reasoning { summary: vec![] }],
        usage: None,
        status: None,
    };
    let conv = response_to_ir(&resp);
    assert_eq!(conv.len(), 1);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => assert!(text.is_empty()),
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn t126_ir_message_is_text_only() {
    let msg = IrMessage::text(IrRole::User, "hello");
    assert!(msg.is_text_only());

    let msg2 = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t".into(),
            name: "n".into(),
            input: json!({}),
        }],
    );
    assert!(!msg2.is_text_only());
}

#[test]
fn t127_ir_conversation_tool_calls() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "checking".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: json!({}),
            },
            IrContentBlock::ToolUse {
                id: "t2".into(),
                name: "write".into(),
                input: json!({}),
            },
        ],
    )]);
    assert_eq!(conv.tool_calls().len(), 2);
}

#[test]
fn t128_assistant_with_text_and_tool_use_roundtrip() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me check.".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "rust"}),
            },
        ],
    )]);
    let items = ir_to_response_items(&conv);
    assert_eq!(items.len(), 2);
    assert!(matches!(&items[0], CodexResponseItem::Message { .. }));
    assert!(matches!(&items[1], CodexResponseItem::FunctionCall { .. }));
}

#[tokio::test]
async fn t129_client_stream_with_mixed_events() {
    let events = vec![
        delta_event("Hel"),
        delta_event("lo"),
        assistant_event("Done"),
        tool_call_event("shell", "fc_1", json!({})),
    ];
    let c = CodexClient::new("m").with_processor(make_processor(events));
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "go")])
        .build();
    let stream = c.create_stream(req).await.unwrap();
    let chunks: Vec<_> = stream.collect().await;
    // created + 2 deltas + 1 done(msg) + 1 done(tool) + completed = 6
    assert_eq!(chunks.len(), 6);
}

#[test]
fn t130_known_models_exhaustive() {
    let known = [
        "codex-mini-latest",
        "o3-mini",
        "o4-mini",
        "gpt-4",
        "gpt-4o",
        "gpt-4.1",
        "gpt-4.1-mini",
        "gpt-4.1-nano",
    ];
    for m in &known {
        assert!(is_known_model(m), "{m} should be known");
    }
}
