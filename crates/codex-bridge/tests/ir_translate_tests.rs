// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for Codex ↔ abp-dialect IR translation.

use codex_bridge::ir_translate::*;

use abp_codex_sdk::dialect::{
    CodexContentPart, CodexFunctionDef, CodexInputItem, CodexRequest, CodexResponse,
    CodexResponseItem, CodexStreamDelta, CodexStreamEvent, CodexTextFormat, CodexTool, CodexUsage,
    ReasoningSummary,
};
use abp_dialect::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
    IrStreamEvent, IrToolDefinition, IrUsage,
};
use serde_json::json;
use std::collections::BTreeMap;

// ── Helper builders ─────────────────────────────────────────────────────

fn simple_request() -> CodexRequest {
    CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Hello".into(),
        }],
        max_output_tokens: Some(4096),
        temperature: None,
        tools: Vec::new(),
        text: None,
    }
}

fn simple_response() -> CodexResponse {
    CodexResponse {
        id: "resp_01".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Hi there!".into(),
            }],
        }],
        usage: Some(CodexUsage {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
        }),
        status: Some("completed".into()),
    }
}

// ── Request to IR ───────────────────────────────────────────────────────

#[test]
fn request_simple_text() {
    let req = simple_request();
    let ir = codex_request_to_ir(&req);
    assert_eq!(ir.model, Some("codex-mini-latest".into()));
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert!(ir.system_prompt.is_none());
}

#[test]
fn request_system_prompt_extracted() {
    let mut req = simple_request();
    req.input.insert(
        0,
        CodexInputItem::Message {
            role: "system".into(),
            content: "You are helpful.".into(),
        },
    );
    let ir = codex_request_to_ir(&req);
    assert_eq!(ir.system_prompt, Some("You are helpful.".into()));
    // Only the user message remains
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
}

#[test]
fn request_multiple_system_prompts_concatenated() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![
            CodexInputItem::Message {
                role: "system".into(),
                content: "Rule 1.".into(),
            },
            CodexInputItem::Message {
                role: "system".into(),
                content: "Rule 2.".into(),
            },
            CodexInputItem::Message {
                role: "user".into(),
                content: "Go".into(),
            },
        ],
        max_output_tokens: None,
        temperature: None,
        tools: Vec::new(),
        text: None,
    };
    let ir = codex_request_to_ir(&req);
    assert_eq!(ir.system_prompt, Some("Rule 1.\nRule 2.".into()));
}

#[test]
fn request_max_output_tokens_mapped() {
    let mut req = simple_request();
    req.max_output_tokens = Some(8192);
    let ir = codex_request_to_ir(&req);
    assert_eq!(ir.config.max_tokens, Some(8192));
}

#[test]
fn request_temperature() {
    let mut req = simple_request();
    req.temperature = Some(0.7);
    let ir = codex_request_to_ir(&req);
    assert_eq!(ir.config.temperature, Some(0.7));
}

#[test]
fn request_no_temperature_is_none() {
    let req = simple_request();
    let ir = codex_request_to_ir(&req);
    assert!(ir.config.temperature.is_none());
}

#[test]
fn request_function_tools() {
    let mut req = simple_request();
    req.tools = vec![CodexTool::Function {
        function: CodexFunctionDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        },
    }];
    let ir = codex_request_to_ir(&req);
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "read_file");
    assert_eq!(ir.tools[0].description, "Read a file");
}

#[test]
fn request_code_interpreter_tool() {
    let mut req = simple_request();
    req.tools = vec![CodexTool::CodeInterpreter {}];
    let ir = codex_request_to_ir(&req);
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "code_interpreter");
}

#[test]
fn request_file_search_tool() {
    let mut req = simple_request();
    req.tools = vec![CodexTool::FileSearch {
        max_num_results: Some(10),
    }];
    let ir = codex_request_to_ir(&req);
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "file_search");
}

#[test]
fn request_text_format_in_metadata() {
    let mut req = simple_request();
    req.text = Some(CodexTextFormat::JsonObject {});
    let ir = codex_request_to_ir(&req);
    assert!(ir.metadata.contains_key("text_format"));
}

#[test]
fn request_assistant_message_role() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![
            CodexInputItem::Message {
                role: "user".into(),
                content: "Hi".into(),
            },
            CodexInputItem::Message {
                role: "assistant".into(),
                content: "Hello!".into(),
            },
        ],
        max_output_tokens: None,
        temperature: None,
        tools: Vec::new(),
        text: None,
    };
    let ir = codex_request_to_ir(&req);
    assert_eq!(ir.messages.len(), 2);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
}

// ── IR to Request ───────────────────────────────────────────────────────

#[test]
fn ir_to_request_basic() {
    let ir = IrRequest {
        model: Some("codex-mini-latest".into()),
        system_prompt: Some("Be helpful".into()),
        messages: vec![IrMessage::text(IrRole::User, "Hello")],
        tools: Vec::new(),
        config: IrGenerationConfig {
            max_tokens: Some(2048),
            temperature: Some(0.5),
            ..Default::default()
        },
        metadata: BTreeMap::new(),
    };
    let req = ir_to_codex_request(&ir);
    assert_eq!(req.model, "codex-mini-latest");
    assert_eq!(req.max_output_tokens, Some(2048));
    assert_eq!(req.temperature, Some(0.5));
    // System prompt is first input item
    assert_eq!(req.input.len(), 2);
    match &req.input[0] {
        CodexInputItem::Message { role, content } => {
            assert_eq!(role, "system");
            assert_eq!(content, "Be helpful");
        }
    }
}

#[test]
fn ir_to_request_default_model() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")]);
    let req = ir_to_codex_request(&ir);
    assert_eq!(req.model, "codex-mini-latest");
}

#[test]
fn ir_to_request_no_system_prompt() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")]);
    let req = ir_to_codex_request(&ir);
    assert_eq!(req.input.len(), 1);
    match &req.input[0] {
        CodexInputItem::Message { role, .. } => {
            assert_eq!(role, "user");
        }
    }
}

#[test]
fn ir_to_request_tools() {
    let ir = IrRequest {
        model: Some("codex-mini-latest".into()),
        system_prompt: None,
        messages: vec![IrMessage::text(IrRole::User, "list files")],
        tools: vec![IrToolDefinition {
            name: "ls".into(),
            description: "List directory".into(),
            parameters: json!({"type": "object"}),
        }],
        config: IrGenerationConfig::default(),
        metadata: BTreeMap::new(),
    };
    let req = ir_to_codex_request(&ir);
    assert_eq!(req.tools.len(), 1);
    match &req.tools[0] {
        CodexTool::Function { function } => {
            assert_eq!(function.name, "ls");
            assert_eq!(function.description, "List directory");
        }
        other => panic!("expected Function tool, got {other:?}"),
    }
}

#[test]
fn ir_to_request_no_max_tokens() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")]);
    let req = ir_to_codex_request(&ir);
    assert!(req.max_output_tokens.is_none());
}

#[test]
fn ir_to_request_text_format_roundtrip() {
    let mut metadata = BTreeMap::new();
    metadata.insert("text_format".into(), json!({"type": "json_object"}));
    let ir = IrRequest {
        model: Some("codex-mini-latest".into()),
        system_prompt: None,
        messages: vec![IrMessage::text(IrRole::User, "Give JSON")],
        tools: Vec::new(),
        config: IrGenerationConfig::default(),
        metadata,
    };
    let req = ir_to_codex_request(&ir);
    assert!(req.text.is_some());
}

#[test]
fn ir_to_request_tool_role_becomes_user() {
    let ir = IrRequest {
        model: Some("codex-mini-latest".into()),
        system_prompt: None,
        messages: vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::Text {
                text: "tool output".into(),
            }],
        )],
        tools: Vec::new(),
        config: IrGenerationConfig::default(),
        metadata: BTreeMap::new(),
    };
    let req = ir_to_codex_request(&ir);
    match &req.input[0] {
        CodexInputItem::Message { role, .. } => {
            assert_eq!(role, "user");
        }
    }
}

// ── Response to IR ──────────────────────────────────────────────────────

#[test]
fn response_simple_text() {
    let resp = simple_response();
    let ir = codex_response_to_ir(&resp);
    assert_eq!(ir.id, Some("resp_01".into()));
    assert_eq!(ir.model, Some("codex-mini-latest".into()));
    assert_eq!(ir.content.len(), 1);
    assert_eq!(ir.content[0].as_text(), Some("Hi there!"));
    assert_eq!(ir.stop_reason, Some(IrStopReason::EndTurn));
}

#[test]
fn response_usage() {
    let resp = simple_response();
    let ir = codex_response_to_ir(&resp);
    let u = ir.usage.unwrap();
    assert_eq!(u.input_tokens, 10);
    assert_eq!(u.output_tokens, 20);
    assert_eq!(u.total_tokens, 30);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
}

#[test]
fn response_function_call() {
    let resp = CodexResponse {
        id: "resp_fc".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "shell".into(),
            arguments: r#"{"command":"ls"}"#.into(),
        }],
        usage: None,
        status: Some("completed".into()),
    };
    let ir = codex_response_to_ir(&resp);
    assert_eq!(ir.content.len(), 1);
    match &ir.content[0] {
        IrContentBlock::ToolCall { id, name, input } => {
            assert_eq!(id, "fc_1");
            assert_eq!(name, "shell");
            assert_eq!(input, &json!({"command": "ls"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn response_function_call_output() {
    let resp = CodexResponse {
        id: "resp_fco".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "file1.txt\nfile2.txt".into(),
        }],
        usage: None,
        status: None,
    };
    let ir = codex_response_to_ir(&resp);
    assert_eq!(ir.content.len(), 1);
    match &ir.content[0] {
        IrContentBlock::ToolResult {
            tool_call_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_call_id, "fc_1");
            assert!(!is_error);
            assert_eq!(content[0].as_text(), Some("file1.txt\nfile2.txt"));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn response_reasoning() {
    let resp = CodexResponse {
        id: "resp_r".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::Reasoning {
            summary: vec![
                ReasoningSummary {
                    text: "Step 1".into(),
                },
                ReasoningSummary {
                    text: "Step 2".into(),
                },
            ],
        }],
        usage: None,
        status: None,
    };
    let ir = codex_response_to_ir(&resp);
    assert_eq!(ir.content.len(), 1);
    match &ir.content[0] {
        IrContentBlock::Thinking { text } => {
            assert_eq!(text, "Step 1\nStep 2");
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn response_empty_reasoning_skipped() {
    let resp = CodexResponse {
        id: "resp_er".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::Reasoning {
            summary: Vec::new(),
        }],
        usage: None,
        status: None,
    };
    let ir = codex_response_to_ir(&resp);
    assert!(ir.content.is_empty());
}

#[test]
fn response_incomplete_status() {
    let mut resp = simple_response();
    resp.status = Some("incomplete".into());
    let ir = codex_response_to_ir(&resp);
    assert_eq!(ir.stop_reason, Some(IrStopReason::MaxTokens));
}

#[test]
fn response_failed_status() {
    let mut resp = simple_response();
    resp.status = Some("failed".into());
    let ir = codex_response_to_ir(&resp);
    assert_eq!(ir.stop_reason, Some(IrStopReason::Other("failed".into())));
}

#[test]
fn response_no_status() {
    let mut resp = simple_response();
    resp.status = None;
    let ir = codex_response_to_ir(&resp);
    assert!(ir.stop_reason.is_none());
}

#[test]
fn response_no_usage() {
    let mut resp = simple_response();
    resp.usage = None;
    let ir = codex_response_to_ir(&resp);
    assert!(ir.usage.is_none());
}

#[test]
fn response_invalid_json_arguments_become_string() {
    let resp = CodexResponse {
        id: "resp_bad".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::FunctionCall {
            id: "fc_bad".into(),
            call_id: None,
            name: "test".into(),
            arguments: "not valid json".into(),
        }],
        usage: None,
        status: None,
    };
    let ir = codex_response_to_ir(&resp);
    match &ir.content[0] {
        IrContentBlock::ToolCall { input, .. } => {
            assert_eq!(input, &json!("not valid json"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

// ── IR to Response ──────────────────────────────────────────────────────

#[test]
fn ir_to_response_basic() {
    let ir = IrResponse {
        id: Some("resp_02".into()),
        model: Some("codex-mini-latest".into()),
        content: vec![IrContentBlock::Text {
            text: "Hello!".into(),
        }],
        stop_reason: Some(IrStopReason::EndTurn),
        usage: Some(IrUsage::from_io(5, 10)),
        metadata: BTreeMap::new(),
    };
    let resp = ir_to_codex_response(&ir);
    assert_eq!(resp.id, "resp_02");
    assert_eq!(resp.model, "codex-mini-latest");
    assert_eq!(resp.status, Some("completed".into()));
    let u = resp.usage.unwrap();
    assert_eq!(u.input_tokens, 5);
    assert_eq!(u.output_tokens, 10);
}

#[test]
fn ir_to_response_tool_call() {
    let ir = IrResponse::new(vec![IrContentBlock::ToolCall {
        id: "fc_1".into(),
        name: "exec".into(),
        input: json!({"cmd": "ls"}),
    }])
    .with_stop_reason(IrStopReason::ToolUse);
    let resp = ir_to_codex_response(&ir);
    assert_eq!(resp.status, Some("completed".into()));
    assert_eq!(resp.output.len(), 1);
    match &resp.output[0] {
        CodexResponseItem::FunctionCall {
            id,
            name,
            arguments,
            ..
        } => {
            assert_eq!(id, "fc_1");
            assert_eq!(name, "exec");
            assert!(arguments.contains("cmd"));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn ir_to_response_tool_result() {
    let ir = IrResponse::new(vec![IrContentBlock::ToolResult {
        tool_call_id: "fc_1".into(),
        content: vec![IrContentBlock::Text {
            text: "output data".into(),
        }],
        is_error: false,
    }]);
    let resp = ir_to_codex_response(&ir);
    assert_eq!(resp.output.len(), 1);
    match &resp.output[0] {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "fc_1");
            assert_eq!(output, "output data");
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

#[test]
fn ir_to_response_thinking() {
    let ir = IrResponse::new(vec![IrContentBlock::Thinking {
        text: "Let me think...".into(),
    }]);
    let resp = ir_to_codex_response(&ir);
    assert_eq!(resp.output.len(), 1);
    match &resp.output[0] {
        CodexResponseItem::Reasoning { summary } => {
            assert_eq!(summary.len(), 1);
            assert_eq!(summary[0].text, "Let me think...");
        }
        other => panic!("expected Reasoning, got {other:?}"),
    }
}

#[test]
fn ir_to_response_mixed_content() {
    let ir = IrResponse::new(vec![
        IrContentBlock::Text {
            text: "Here's the plan:".into(),
        },
        IrContentBlock::ToolCall {
            id: "fc_1".into(),
            name: "shell".into(),
            input: json!({"cmd": "ls"}),
        },
        IrContentBlock::Text {
            text: "Done!".into(),
        },
    ]);
    let resp = ir_to_codex_response(&ir);
    // Text → Message, ToolCall → FunctionCall, Text → Message
    assert_eq!(resp.output.len(), 3);
    assert!(matches!(&resp.output[0], CodexResponseItem::Message { .. }));
    assert!(matches!(
        &resp.output[1],
        CodexResponseItem::FunctionCall { .. }
    ));
    assert!(matches!(&resp.output[2], CodexResponseItem::Message { .. }));
}

#[test]
fn ir_to_response_no_usage() {
    let ir = IrResponse::new(vec![IrContentBlock::Text { text: "ok".into() }]);
    let resp = ir_to_codex_response(&ir);
    assert!(resp.usage.is_none());
}

#[test]
fn ir_to_response_content_filter() {
    let ir = IrResponse::text("blocked").with_stop_reason(IrStopReason::ContentFilter);
    let resp = ir_to_codex_response(&ir);
    assert_eq!(resp.status, Some("failed".into()));
}

// ── Stream events ───────────────────────────────────────────────────────

#[test]
fn stream_response_created() {
    let event = CodexStreamEvent::ResponseCreated {
        response: CodexResponse {
            id: "resp_stream".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: None,
        },
    };
    let ir_events = codex_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::StreamStart { id, model } => {
            assert_eq!(id, &Some("resp_stream".into()));
            assert_eq!(model, &Some("codex-mini-latest".into()));
        }
        other => panic!("expected StreamStart, got {other:?}"),
    }
}

#[test]
fn stream_in_progress_empty() {
    let event = CodexStreamEvent::ResponseInProgress {
        response: CodexResponse {
            id: "resp_ip".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: Some("in_progress".into()),
        },
    };
    let ir_events = codex_stream_to_ir(&event);
    assert!(ir_events.is_empty());
}

#[test]
fn stream_output_item_added_message() {
    let event = CodexStreamEvent::OutputItemAdded {
        output_index: 0,
        item: CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText { text: "".into() }],
        },
    };
    let ir_events = codex_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::ContentBlockStart { index, block } => {
            assert_eq!(*index, 0);
            assert!(matches!(block, IrContentBlock::Text { .. }));
        }
        other => panic!("expected ContentBlockStart, got {other:?}"),
    }
}

#[test]
fn stream_output_item_added_function_call() {
    let event = CodexStreamEvent::OutputItemAdded {
        output_index: 1,
        item: CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "bash".into(),
            arguments: "{}".into(),
        },
    };
    let ir_events = codex_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::ContentBlockStart { index, block } => {
            assert_eq!(*index, 1);
            assert!(matches!(block, IrContentBlock::ToolCall { name, .. } if name == "bash"));
        }
        other => panic!("expected ContentBlockStart, got {other:?}"),
    }
}

#[test]
fn stream_text_delta() {
    let event = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::OutputTextDelta {
            text: "Hello ".into(),
        },
    };
    let ir_events = codex_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::TextDelta { index, text } => {
            assert_eq!(*index, 0);
            assert_eq!(text, "Hello ");
        }
        other => panic!("expected TextDelta, got {other:?}"),
    }
}

#[test]
fn stream_function_call_arguments_delta() {
    let event = CodexStreamEvent::OutputItemDelta {
        output_index: 1,
        delta: CodexStreamDelta::FunctionCallArgumentsDelta {
            delta: r#"{"pa"#.into(),
        },
    };
    let ir_events = codex_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::ToolCallDelta {
            index,
            arguments_delta,
        } => {
            assert_eq!(*index, 1);
            assert_eq!(arguments_delta, r#"{"pa"#);
        }
        other => panic!("expected ToolCallDelta, got {other:?}"),
    }
}

#[test]
fn stream_reasoning_summary_delta() {
    let event = CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: CodexStreamDelta::ReasoningSummaryDelta {
            text: "Let me ".into(),
        },
    };
    let ir_events = codex_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::ThinkingDelta { index, text } => {
            assert_eq!(*index, 0);
            assert_eq!(text, "Let me ");
        }
        other => panic!("expected ThinkingDelta, got {other:?}"),
    }
}

#[test]
fn stream_output_item_done() {
    let event = CodexStreamEvent::OutputItemDone {
        output_index: 2,
        item: CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Done!".into(),
            }],
        },
    };
    let ir_events = codex_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::ContentBlockStop { index } => assert_eq!(*index, 2),
        other => panic!("expected ContentBlockStop, got {other:?}"),
    }
}

#[test]
fn stream_response_completed_with_usage() {
    let event = CodexStreamEvent::ResponseCompleted {
        response: CodexResponse {
            id: "resp_done".into(),
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
    let ir_events = codex_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 2); // Usage + StreamEnd
    match &ir_events[0] {
        IrStreamEvent::Usage { usage } => {
            assert_eq!(usage.input_tokens, 100);
            assert_eq!(usage.output_tokens, 50);
            assert_eq!(usage.total_tokens, 150);
        }
        other => panic!("expected Usage, got {other:?}"),
    }
    match &ir_events[1] {
        IrStreamEvent::StreamEnd { stop_reason } => {
            assert_eq!(*stop_reason, Some(IrStopReason::EndTurn));
        }
        other => panic!("expected StreamEnd, got {other:?}"),
    }
}

#[test]
fn stream_response_completed_no_usage() {
    let event = CodexStreamEvent::ResponseCompleted {
        response: CodexResponse {
            id: "resp_done".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: None,
        },
    };
    let ir_events = codex_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1); // Only StreamEnd
    match &ir_events[0] {
        IrStreamEvent::StreamEnd { stop_reason } => {
            assert!(stop_reason.is_none());
        }
        other => panic!("expected StreamEnd, got {other:?}"),
    }
}

#[test]
fn stream_response_failed() {
    let event = CodexStreamEvent::ResponseFailed {
        response: CodexResponse {
            id: "resp_fail".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: Some("failed".into()),
        },
    };
    let ir_events = codex_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::Error { code, message } => {
            assert_eq!(code, "response_failed");
            assert_eq!(message, "failed");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn stream_error_with_code() {
    let event = CodexStreamEvent::Error {
        message: "Rate limit exceeded".into(),
        code: Some("rate_limit".into()),
    };
    let ir_events = codex_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::Error { code, message } => {
            assert_eq!(code, "rate_limit");
            assert_eq!(message, "Rate limit exceeded");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn stream_error_no_code() {
    let event = CodexStreamEvent::Error {
        message: "Something went wrong".into(),
        code: None,
    };
    let ir_events = codex_stream_to_ir(&event);
    match &ir_events[0] {
        IrStreamEvent::Error { code, .. } => {
            assert_eq!(code, "error");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

// ── Roundtrip tests ─────────────────────────────────────────────────────

#[test]
fn request_roundtrip() {
    let orig = simple_request();
    let ir = codex_request_to_ir(&orig);
    let back = ir_to_codex_request(&ir);
    assert_eq!(back.model, orig.model);
    assert_eq!(back.max_output_tokens, orig.max_output_tokens);
    assert_eq!(back.input.len(), orig.input.len());
}

#[test]
fn response_roundtrip() {
    let orig = simple_response();
    let ir = codex_response_to_ir(&orig);
    let back = ir_to_codex_response(&ir);
    assert_eq!(back.id, orig.id);
    assert_eq!(back.model, orig.model);
    assert_eq!(back.status, orig.status);
    let orig_u = orig.usage.unwrap();
    let back_u = back.usage.unwrap();
    assert_eq!(back_u.input_tokens, orig_u.input_tokens);
    assert_eq!(back_u.output_tokens, orig_u.output_tokens);
}

#[test]
fn request_with_tools_roundtrip() {
    let mut req = simple_request();
    req.tools = vec![CodexTool::Function {
        function: CodexFunctionDef {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        },
    }];
    req.temperature = Some(0.3);
    let ir = codex_request_to_ir(&req);
    let back = ir_to_codex_request(&ir);
    assert_eq!(back.tools.len(), 1);
    match &back.tools[0] {
        CodexTool::Function { function } => {
            assert_eq!(function.name, "search");
        }
        other => panic!("expected Function tool, got {other:?}"),
    }
    assert_eq!(back.temperature, Some(0.3));
}

#[test]
fn response_multiple_output_items_roundtrip() {
    let resp = CodexResponse {
        id: "resp_multi".into(),
        model: "codex-mini-latest".into(),
        output: vec![
            CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "I'll run ls.".into(),
                }],
            },
            CodexResponseItem::FunctionCall {
                id: "fc_1".into(),
                call_id: None,
                name: "shell".into(),
                arguments: r#"{"cmd":"ls"}"#.into(),
            },
        ],
        usage: Some(CodexUsage {
            input_tokens: 5,
            output_tokens: 15,
            total_tokens: 20,
        }),
        status: Some("completed".into()),
    };
    let ir = codex_response_to_ir(&resp);
    assert_eq!(ir.content.len(), 2);
    assert!(matches!(&ir.content[0], IrContentBlock::Text { text } if text == "I'll run ls."));
    assert!(matches!(&ir.content[1], IrContentBlock::ToolCall { name, .. } if name == "shell"));
}
