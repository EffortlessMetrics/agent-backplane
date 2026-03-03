// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the abp-shim-codex crate.
//!
//! Covers: SDK type fidelity, request/response translation, streaming,
//! tool use, and edge cases.

use abp_codex_sdk::dialect::{
    self, CanonicalToolDef, CodexConfig, CodexContentPart, CodexInputItem, CodexRequest,
    CodexResponse, CodexResponseItem, CodexStreamDelta, CodexStreamEvent, CodexTextFormat,
    CodexTool, CodexUsage, FileAccess, NetworkAccess, ReasoningSummary, SandboxConfig,
};
use abp_codex_sdk::lowering;
use abp_core::ir::{IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, UsageNormalized};
use abp_shim_codex::{
    codex_message, events_to_stream_events, ir_to_response_items, ir_usage_to_usage,
    mock_receipt, mock_receipt_with_usage, receipt_to_response, request_to_ir,
    request_to_work_order, response_to_ir, CodexClient, CodexRequestBuilder, ShimError, Usage,
};
use chrono::Utc;
use serde_json::json;
use tokio_stream::StreamExt;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Codex SDK types fidelity (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t01_codex_request_serde_roundtrip() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "hello".into(),
        }],
        max_output_tokens: Some(1024),
        temperature: Some(0.5),
        tools: vec![],
        text: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CodexRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "codex-mini-latest");
    assert_eq!(back.max_output_tokens, Some(1024));
    assert_eq!(back.temperature, Some(0.5));
}

#[test]
fn t02_codex_response_serde_roundtrip() {
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
fn t03_codex_usage_field_preservation() {
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
fn t04_codex_input_item_message_serde() {
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
fn t05_codex_response_item_function_call_serde() {
    let item = CodexResponseItem::FunctionCall {
        id: "fc_1".into(),
        call_id: Some("corr_1".into()),
        name: "shell".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    match back {
        CodexResponseItem::FunctionCall {
            id,
            call_id,
            name,
            arguments,
        } => {
            assert_eq!(id, "fc_1");
            assert_eq!(call_id.as_deref(), Some("corr_1"));
            assert_eq!(name, "shell");
            assert!(arguments.contains("ls"));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn t06_codex_response_item_function_call_output_serde() {
    let item = CodexResponseItem::FunctionCallOutput {
        call_id: "fc_1".into(),
        output: "file.txt".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    match back {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "fc_1");
            assert_eq!(output, "file.txt");
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

#[test]
fn t07_codex_response_item_reasoning_serde() {
    let item = CodexResponseItem::Reasoning {
        summary: vec![
            ReasoningSummary {
                text: "Step 1: think".into(),
            },
            ReasoningSummary {
                text: "Step 2: act".into(),
            },
        ],
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    match back {
        CodexResponseItem::Reasoning { summary } => {
            assert_eq!(summary.len(), 2);
            assert_eq!(summary[0].text, "Step 1: think");
            assert_eq!(summary[1].text, "Step 2: act");
        }
        other => panic!("expected Reasoning, got {other:?}"),
    }
}

#[test]
fn t08_sandbox_config_serde_defaults() {
    let cfg = SandboxConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: SandboxConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.networking, NetworkAccess::None);
    assert_eq!(back.file_access, FileAccess::WorkspaceOnly);
    assert_eq!(back.timeout_seconds, Some(300));
    assert_eq!(back.memory_mb, Some(512));
    assert!(back.env.is_empty());
}

#[test]
fn t09_text_format_plain_serde() {
    let fmt = CodexTextFormat::Text {};
    let json = serde_json::to_string(&fmt).unwrap();
    let back: CodexTextFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(back, CodexTextFormat::Text {});
}

#[test]
fn t10_text_format_json_schema_serde() {
    let fmt = CodexTextFormat::JsonSchema {
        name: "my_schema".into(),
        schema: json!({"type": "object", "properties": {"x": {"type": "integer"}}}),
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
            assert_eq!(name, "my_schema");
            assert!(strict);
            assert!(schema.get("properties").is_some());
        }
        other => panic!("expected JsonSchema, got {other:?}"),
    }
}

#[test]
fn t11_text_format_json_object_serde() {
    let fmt = CodexTextFormat::JsonObject {};
    let json = serde_json::to_string(&fmt).unwrap();
    let back: CodexTextFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(back, CodexTextFormat::JsonObject {});
}

#[test]
fn t12_codex_config_default_values() {
    let cfg = CodexConfig::default();
    assert!(cfg.base_url.contains("openai.com"));
    assert_eq!(cfg.model, "codex-mini-latest");
    assert_eq!(cfg.max_output_tokens, Some(4096));
    assert!(cfg.api_key.is_empty());
}

#[test]
fn t13_stream_delta_output_text_serde() {
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
fn t14_stream_delta_function_args_serde() {
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
fn t15_stream_event_response_created_serde() {
    let event = CodexStreamEvent::ResponseCreated {
        response: CodexResponse {
            id: "resp_x".into(),
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
            assert_eq!(response.id, "resp_x");
        }
        other => panic!("expected ResponseCreated, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Request translation (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t16_request_to_work_order_basic_task() {
    let req = CodexRequestBuilder::new()
        .model("codex-mini-latest")
        .input(vec![codex_message("user", "Fix the bug")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Fix the bug");
}

#[test]
fn t17_request_to_work_order_model_mapping() {
    let req = CodexRequestBuilder::new()
        .model("o4-mini")
        .input(vec![codex_message("user", "test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("o4-mini"));
}

#[test]
fn t18_request_to_work_order_temperature_vendor() {
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
fn t19_request_to_work_order_max_output_tokens_vendor() {
    let req = CodexRequestBuilder::new()
        .max_output_tokens(4096)
        .input(vec![codex_message("user", "test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("max_output_tokens"),
        Some(&serde_json::Value::from(4096))
    );
}

#[test]
fn t20_request_to_work_order_no_temp_no_vendor_key() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "test")])
        .build();
    let wo = request_to_work_order(&req);
    assert!(!wo.config.vendor.contains_key("temperature"));
}

#[test]
fn t21_request_to_work_order_extracts_last_user_message() {
    let req = CodexRequestBuilder::new()
        .input(vec![
            codex_message("user", "first question"),
            codex_message("assistant", "answer"),
            codex_message("user", "second question"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "second question");
}

#[test]
fn t22_request_to_work_order_system_message_not_task() {
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
fn t23_request_to_ir_preserves_system_role() {
    let req = CodexRequestBuilder::new()
        .input(vec![
            codex_message("system", "System prompt"),
            codex_message("user", "Hello"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.messages.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn t24_request_to_ir_preserves_message_content() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "Tell me a joke")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.messages[0].text_content(), "Tell me a joke");
}

#[test]
fn t25_builder_default_model() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "test")])
        .build();
    assert_eq!(req.model, "codex-mini-latest");
}

#[test]
fn t26_builder_overrides_model() {
    let req = CodexRequestBuilder::new()
        .model("gpt-4o")
        .input(vec![codex_message("user", "test")])
        .build();
    assert_eq!(req.model, "gpt-4o");
}

#[test]
fn t27_codex_message_helper_creates_correct_item() {
    let item = codex_message("user", "Hello");
    match item {
        CodexInputItem::Message { role, content } => {
            assert_eq!(role, "user");
            assert_eq!(content, "Hello");
        }
    }
}

#[test]
fn t28_request_to_work_order_generates_unique_ids() {
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "test")])
        .build();
    let wo1 = request_to_work_order(&req);
    let wo2 = request_to_work_order(&req);
    assert_ne!(wo1.id, wo2.id);
}

#[test]
fn t29_request_builder_sets_tools() {
    let tool = CodexTool::Function {
        function: dialect::CodexFunctionDef {
            name: "search".into(),
            description: "Search files".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let req = CodexRequestBuilder::new()
        .tools(vec![tool])
        .input(vec![codex_message("user", "test")])
        .build();
    assert_eq!(req.tools.len(), 1);
}

#[test]
fn t30_request_builder_sets_text_format() {
    let req = CodexRequestBuilder::new()
        .text(CodexTextFormat::JsonObject {})
        .input(vec![codex_message("user", "test")])
        .build();
    assert!(req.text.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Response translation (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t31_receipt_to_response_assistant_message() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        },
        ext: None,
    }];
    let receipt = mock_receipt(events);
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
fn t32_receipt_to_response_model_preserved() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "o3-mini");
    assert_eq!(resp.model, "o3-mini");
}

#[test]
fn t33_receipt_to_response_status_completed() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    assert_eq!(resp.status.as_deref(), Some("completed"));
}

#[test]
fn t34_receipt_to_response_id_contains_run_id() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    assert!(resp.id.starts_with("resp_"));
    let run_id = receipt.meta.run_id.to_string();
    assert!(resp.id.contains(&run_id));
}

#[test]
fn t35_receipt_to_response_tool_call() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("fc_99".into()),
            parent_tool_use_id: None,
            input: json!({"path": "main.rs"}),
        },
        ext: None,
    }];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    match &resp.output[0] {
        CodexResponseItem::FunctionCall {
            id, name, arguments, ..
        } => {
            assert_eq!(id, "fc_99");
            assert_eq!(name, "read");
            assert!(arguments.contains("main.rs"));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn t36_receipt_to_response_error_event() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "timeout".into(),
            error_code: None,
        },
        ext: None,
    }];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    match &resp.output[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => {
                assert!(text.contains("timeout"));
            }
        },
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn t37_receipt_to_response_usage_mapping() {
    let usage = UsageNormalized {
        input_tokens: Some(200),
        output_tokens: Some(100),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    };
    let receipt = mock_receipt_with_usage(vec![], usage);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    let u = resp.usage.unwrap();
    assert_eq!(u.input_tokens, 200);
    assert_eq!(u.output_tokens, 100);
    assert_eq!(u.total_tokens, 300);
}

#[test]
fn t38_receipt_to_response_zero_usage() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    let u = resp.usage.unwrap();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn t39_receipt_to_response_delta_becomes_message() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "partial".into(),
        },
        ext: None,
    }];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    assert_eq!(resp.output.len(), 1);
    match &resp.output[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => assert_eq!(text, "partial"),
        },
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn t40_receipt_to_response_multiple_events() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "msg1".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "write".into(),
                tool_use_id: Some("fc_a".into()),
                parent_tool_use_id: None,
                input: json!({"path": "out.txt", "content": "data"}),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "msg2".into(),
            },
            ext: None,
        },
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    assert_eq!(resp.output.len(), 3);
}

#[test]
fn t41_response_to_ir_message_preserves_text() {
    let resp = CodexResponse {
        id: "resp_1".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "answer".into(),
            }],
        }],
        usage: None,
        status: None,
    };
    let conv = response_to_ir(&resp);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    assert_eq!(conv.messages[0].text_content(), "answer");
}

#[test]
fn t42_ir_to_response_items_roundtrip() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(
        IrRole::Assistant,
        "roundtrip test",
    )]);
    let items = ir_to_response_items(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => assert_eq!(text, "roundtrip test"),
        },
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn t43_ir_usage_to_shim_usage() {
    let ir = IrUsage::from_io(500, 250);
    let usage = ir_usage_to_usage(&ir);
    assert_eq!(usage.input_tokens, 500);
    assert_eq!(usage.output_tokens, 250);
    assert_eq!(usage.total_tokens, 750);
}

#[test]
fn t44_shim_usage_serde_roundtrip() {
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn t45_receipt_to_response_skips_non_mapped_events() {
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
    // RunStarted and RunCompleted don't map to Codex output items
    assert!(resp.output.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Streaming (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t46_stream_events_bookend_created_completed() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hi".into(),
        },
        ext: None,
    }];
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
fn t47_stream_events_delta_mapped() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        },
        ext: None,
    }];
    let stream = events_to_stream_events(&events, "codex-mini-latest");
    // created + 1 delta + completed = 3
    assert_eq!(stream.len(), 3);
    match &stream[1] {
        CodexStreamEvent::OutputItemDelta { delta, .. } => match delta {
            CodexStreamDelta::OutputTextDelta { text } => assert_eq!(text, "chunk"),
            other => panic!("expected OutputTextDelta, got {other:?}"),
        },
        other => panic!("expected OutputItemDelta, got {other:?}"),
    }
}

#[test]
fn t48_stream_events_message_becomes_item_done() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "done".into(),
        },
        ext: None,
    }];
    let stream = events_to_stream_events(&events, "codex-mini-latest");
    match &stream[1] {
        CodexStreamEvent::OutputItemDone { item, .. } => match item {
            CodexResponseItem::Message { content, .. } => match &content[0] {
                CodexContentPart::OutputText { text } => assert_eq!(text, "done"),
            },
            other => panic!("expected Message, got {other:?}"),
        },
        other => panic!("expected OutputItemDone, got {other:?}"),
    }
}

#[test]
fn t49_stream_events_tool_call_mapped() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("fc_t".into()),
            parent_tool_use_id: None,
            input: json!({"command": "echo hi"}),
        },
        ext: None,
    }];
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
fn t50_stream_events_empty_trace() {
    let stream = events_to_stream_events(&[], "codex-mini-latest");
    // Just created + completed
    assert_eq!(stream.len(), 2);
}

#[test]
fn t51_stream_events_model_preserved_in_created() {
    let stream = events_to_stream_events(&[], "gpt-4");
    match &stream[0] {
        CodexStreamEvent::ResponseCreated { response } => {
            assert_eq!(response.model, "gpt-4");
        }
        other => panic!("expected ResponseCreated, got {other:?}"),
    }
}

#[test]
fn t52_stream_events_model_preserved_in_completed() {
    let stream = events_to_stream_events(&[], "gpt-4");
    match stream.last().unwrap() {
        CodexStreamEvent::ResponseCompleted { response } => {
            assert_eq!(response.model, "gpt-4");
        }
        other => panic!("expected ResponseCompleted, got {other:?}"),
    }
}

#[tokio::test]
async fn t53_client_stream_collects_all_events() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "a".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "b".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "c".into(),
            },
            ext: None,
        },
    ];
    let processor: abp_shim_codex::ProcessFn =
        Box::new(move |_wo| mock_receipt(events.clone()));
    let client = CodexClient::new("codex-mini-latest").with_processor(processor);
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "test")])
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<CodexStreamEvent> = stream.collect().await;
    // 1 created + 3 deltas + 1 completed
    assert_eq!(chunks.len(), 5);
}

#[test]
fn t54_stream_events_status_in_progress_on_created() {
    let stream = events_to_stream_events(&[], "codex-mini-latest");
    match &stream[0] {
        CodexStreamEvent::ResponseCreated { response } => {
            assert_eq!(response.status.as_deref(), Some("in_progress"));
        }
        other => panic!("expected ResponseCreated, got {other:?}"),
    }
}

#[test]
fn t55_stream_events_status_completed_on_completed() {
    let stream = events_to_stream_events(&[], "codex-mini-latest");
    match stream.last().unwrap() {
        CodexStreamEvent::ResponseCompleted { response } => {
            assert_eq!(response.status.as_deref(), Some("completed"));
        }
        other => panic!("expected ResponseCompleted, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Tool use (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t56_tool_def_to_codex_roundtrip() {
    let canonical = CanonicalToolDef {
        name: "search".into(),
        description: "Search files".into(),
        parameters_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    let codex_def = dialect::tool_def_to_codex(&canonical);
    assert_eq!(codex_def.tool_type, "function");
    assert_eq!(codex_def.function.name, "search");

    let back = dialect::tool_def_from_codex(&codex_def);
    assert_eq!(back.name, "search");
    assert_eq!(back.description, "Search files");
    assert_eq!(back.parameters_schema, canonical.parameters_schema);
}

#[test]
fn t57_codex_tool_function_to_canonical() {
    let tool = CodexTool::Function {
        function: dialect::CodexFunctionDef {
            name: "write_file".into(),
            description: "Write a file".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "write_file");
    assert_eq!(canonical.description, "Write a file");
}

#[test]
fn t58_codex_tool_code_interpreter_to_canonical() {
    let tool = CodexTool::CodeInterpreter {};
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "code_interpreter");
}

#[test]
fn t59_codex_tool_file_search_to_canonical() {
    let tool = CodexTool::FileSearch {
        max_num_results: Some(10),
    };
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "file_search");
}

#[test]
fn t60_tool_call_event_to_function_call_response() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "shell".into(),
            tool_use_id: Some("fc_1".into()),
            parent_tool_use_id: None,
            input: json!({"command": "ls -la"}),
        },
        ext: None,
    }];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    match &resp.output[0] {
        CodexResponseItem::FunctionCall {
            name, arguments, ..
        } => {
            assert_eq!(name, "shell");
            let parsed: serde_json::Value = serde_json::from_str(arguments).unwrap();
            assert_eq!(parsed["command"], "ls -la");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn t61_tool_call_without_id_gets_generated() {
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
fn t62_function_call_output_ir_roundtrip() {
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
fn t63_codex_tool_serde_function_variant() {
    let tool = CodexTool::Function {
        function: dialect::CodexFunctionDef {
            name: "test".into(),
            description: "A test tool".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains("\"type\":\"function\""));
    let back: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tool);
}

#[test]
fn t64_codex_tool_serde_code_interpreter_variant() {
    let tool = CodexTool::CodeInterpreter {};
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains("code_interpreter"));
    let back: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tool);
}

#[test]
fn t65_codex_tool_serde_file_search_variant() {
    let tool = CodexTool::FileSearch {
        max_num_results: Some(5),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: CodexTool = serde_json::from_str(&json).unwrap();
    match back {
        CodexTool::FileSearch { max_num_results } => {
            assert_eq!(max_num_results, Some(5));
        }
        other => panic!("expected FileSearch, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Edge cases (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t66_empty_input_produces_default_task() {
    let req = CodexRequestBuilder::new().build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "codex completion");
}

#[test]
fn t67_large_task_preserved() {
    let large_text = "x".repeat(100_000);
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", &large_text)])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task.len(), 100_000);
}

#[test]
fn t68_unknown_model_passes_through() {
    let req = CodexRequestBuilder::new()
        .model("totally-unknown-model-v99")
        .input(vec![codex_message("user", "test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("totally-unknown-model-v99"));
}

#[tokio::test]
async fn t69_no_processor_create_returns_internal_error() {
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
async fn t70_no_processor_stream_returns_internal_error() {
    let client = CodexClient::new("codex-mini-latest");
    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "test")])
        .build();
    let result = client.create_stream(req).await;
    assert!(result.is_err());
}
