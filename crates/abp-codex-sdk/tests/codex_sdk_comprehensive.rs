#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for abp-codex-sdk public API.

use abp_codex_sdk::api;
use abp_codex_sdk::convert;
use abp_codex_sdk::dialect;
use abp_codex_sdk::lowering;
use abp_codex_sdk::types::{
    CodexChoice, CodexChoiceMessage, CodexCommand, CodexFileChange, CodexFunctionCall,
    CodexFunctionDef as TypesFunctionDef, CodexMessage, CodexRequest, CodexResponse,
    CodexStreamChoice, CodexStreamChunk, CodexStreamDelta, CodexStreamFunctionCall,
    CodexStreamToolCall, CodexTool as TypesTool, CodexToolCall, CodexToolChoice,
    CodexToolChoiceFunctionRef, CodexToolChoiceMode, CodexUsage as TypesUsage, FileOperation,
};

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest, ExecutionMode,
    Outcome, Receipt, ReceiptBuilder, RunMetadata, SupportLevel, UsageNormalized,
    VerificationReport, WorkOrder, WorkOrderBuilder,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use uuid::Uuid;

// =========================================================================
// Helper constructors
// =========================================================================

fn make_api_request() -> CodexRequest {
    CodexRequest {
        model: "codex-mini-latest".into(),
        messages: vec![CodexMessage::User {
            content: "Fix the bug".into(),
        }],
        instructions: Some("Be helpful.".into()),
        temperature: Some(0.7),
        top_p: None,
        max_tokens: Some(4096),
        stream: None,
        tools: None,
        tool_choice: None,
    }
}

fn make_api_response() -> CodexResponse {
    CodexResponse {
        id: "resp_123".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "codex-mini-latest".into(),
        choices: vec![CodexChoice {
            index: 0,
            message: CodexChoiceMessage {
                role: "assistant".into(),
                content: Some("Done!".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(TypesUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        }),
    }
}

fn make_receipt_with_trace(trace: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::new_v4(),
            contract_version: "abp/v0.1".into(),
            started_at: now,
            finished_at: now,
            duration_ms: 42,
        },
        backend: BackendIdentity {
            id: "openai/codex-mini-latest".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage,
        trace,
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn make_session_request(input: Vec<api::CodexInputItem>) -> api::CodexSessionRequest {
    api::CodexSessionRequest {
        model: "codex-mini-latest".into(),
        instructions: None,
        input,
        tools: None,
        stream: None,
        previous_response_id: None,
        max_output_tokens: None,
        temperature: None,
    }
}

// =========================================================================
// 1. api::CodexRequest serde
// =========================================================================

#[test]
fn api_request_serde_roundtrip() {
    let req = make_api_request();
    let json = serde_json::to_string(&req).unwrap();
    let parsed: CodexRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn api_request_omits_none_fields() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        messages: vec![],
        instructions: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("instructions"));
    assert!(!json.contains("temperature"));
    assert!(!json.contains("top_p"));
    assert!(!json.contains("max_tokens"));
    assert!(!json.contains("stream"));
    assert!(!json.contains("tools"));
    assert!(!json.contains("tool_choice"));
}

#[test]
fn api_request_with_all_optional_fields() {
    let req = CodexRequest {
        model: "gpt-4".into(),
        messages: vec![],
        instructions: Some("inst".into()),
        temperature: Some(1.0),
        top_p: Some(0.9),
        max_tokens: Some(2048),
        stream: Some(true),
        tools: Some(vec![]),
        tool_choice: Some(CodexToolChoice::Mode(CodexToolChoiceMode::Auto)),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: CodexRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

// =========================================================================
// 2. api::CodexMessage serde
// =========================================================================

#[test]
fn api_message_system_serde() {
    let msg = CodexMessage::System {
        content: "System prompt".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"system"#));
    let parsed: CodexMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn api_message_user_serde() {
    let msg = CodexMessage::User {
        content: "Hello".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"user"#));
    let parsed: CodexMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn api_message_assistant_with_content_serde() {
    let msg = CodexMessage::Assistant {
        content: Some("Reply".into()),
        tool_calls: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"assistant"#));
    let parsed: CodexMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn api_message_assistant_with_tool_calls_serde() {
    let msg = CodexMessage::Assistant {
        content: None,
        tool_calls: Some(vec![CodexToolCall {
            id: "tc_1".into(),
            call_type: "function".into(),
            function: CodexFunctionCall {
                name: "bash".into(),
                arguments: r#"{"cmd":"ls"}"#.into(),
            },
        }]),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: CodexMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn api_message_tool_serde() {
    let msg = CodexMessage::Tool {
        content: "result data".into(),
        tool_call_id: "tc_1".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"tool"#));
    let parsed: CodexMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

// =========================================================================
// 3. api::CodexResponse serde
// =========================================================================

#[test]
fn api_response_serde_roundtrip() {
    let resp = make_api_response();
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: CodexResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn api_response_no_usage_serde() {
    let resp = CodexResponse {
        id: "r1".into(),
        object: "chat.completion".into(),
        created: 0,
        model: "m".into(),
        choices: vec![],
        usage: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(!json.contains("usage"));
}

// =========================================================================
// 4. api::CodexChoice / CodexChoiceMessage serde
// =========================================================================

#[test]
fn api_choice_serde() {
    let choice = CodexChoice {
        index: 0,
        message: CodexChoiceMessage {
            role: "assistant".into(),
            content: Some("Hi".into()),
            tool_calls: None,
        },
        finish_reason: Some("stop".into()),
    };
    let json = serde_json::to_string(&choice).unwrap();
    let parsed: CodexChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, choice);
}

#[test]
fn api_choice_message_tool_calls() {
    let msg = CodexChoiceMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![CodexToolCall {
            id: "t1".into(),
            call_type: "function".into(),
            function: CodexFunctionCall {
                name: "read".into(),
                arguments: "{}".into(),
            },
        }]),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: CodexChoiceMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

// =========================================================================
// 5. api::CodexUsage serde
// =========================================================================

#[test]
fn api_usage_serde() {
    let usage = TypesUsage {
        prompt_tokens: 200,
        completion_tokens: 100,
        total_tokens: 300,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: TypesUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}

// =========================================================================
// 6. api::CodexTool / CodexFunctionDef / CodexToolCall / CodexFunctionCall
// =========================================================================

#[test]
fn api_tool_serde() {
    let tool = TypesTool {
        tool_type: "function".into(),
        function: TypesFunctionDef {
            name: "search".into(),
            description: "Search code".into(),
            parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains(r#""type":"function"#));
    let parsed: TypesTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn api_tool_call_serde() {
    let tc = CodexToolCall {
        id: "tc_123".into(),
        call_type: "function".into(),
        function: CodexFunctionCall {
            name: "bash".into(),
            arguments: r#"{"command":"echo hi"}"#.into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: CodexToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

// =========================================================================
// 7. api::CodexToolChoice serde
// =========================================================================

#[test]
fn api_tool_choice_mode_none() {
    let tc = CodexToolChoice::Mode(CodexToolChoiceMode::None);
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: CodexToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn api_tool_choice_mode_auto() {
    let tc = CodexToolChoice::Mode(CodexToolChoiceMode::Auto);
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: CodexToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn api_tool_choice_mode_required() {
    let tc = CodexToolChoice::Mode(CodexToolChoiceMode::Required);
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: CodexToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn api_tool_choice_function() {
    let tc = CodexToolChoice::Function {
        tool_type: "function".into(),
        function: CodexToolChoiceFunctionRef {
            name: "my_func".into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: CodexToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

// =========================================================================
// 8. api::FileOperation / CodexFileChange serde
// =========================================================================

#[test]
fn api_file_operation_create_serde() {
    let op = FileOperation::Create;
    let json = serde_json::to_string(&op).unwrap();
    assert_eq!(json, r#""create""#);
    let parsed: FileOperation = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, op);
}

#[test]
fn api_file_operation_update_serde() {
    let op = FileOperation::Update;
    let json = serde_json::to_string(&op).unwrap();
    assert_eq!(json, r#""update""#);
}

#[test]
fn api_file_operation_delete_serde() {
    let op = FileOperation::Delete;
    let json = serde_json::to_string(&op).unwrap();
    assert_eq!(json, r#""delete""#);
}

#[test]
fn api_file_operation_patch_serde() {
    let op = FileOperation::Patch;
    let json = serde_json::to_string(&op).unwrap();
    assert_eq!(json, r#""patch""#);
}

#[test]
fn api_file_change_full_serde() {
    let fc = CodexFileChange {
        path: "src/main.rs".into(),
        operation: FileOperation::Create,
        content: Some("fn main() {}".into()),
        diff: None,
    };
    let json = serde_json::to_string(&fc).unwrap();
    let parsed: CodexFileChange = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, fc);
}

#[test]
fn api_file_change_patch_with_diff() {
    let fc = CodexFileChange {
        path: "lib.rs".into(),
        operation: FileOperation::Patch,
        content: None,
        diff: Some("--- a/lib.rs\n+++ b/lib.rs\n@@ -1 +1 @@\n-old\n+new".into()),
    };
    let json = serde_json::to_string(&fc).unwrap();
    let parsed: CodexFileChange = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, fc);
}

#[test]
fn api_file_change_delete_omits_content() {
    let fc = CodexFileChange {
        path: "tmp.txt".into(),
        operation: FileOperation::Delete,
        content: None,
        diff: None,
    };
    let json = serde_json::to_string(&fc).unwrap();
    assert!(!json.contains("content"));
    assert!(!json.contains("diff"));
}

// =========================================================================
// 9. api::CodexCommand serde
// =========================================================================

#[test]
fn api_command_full_serde() {
    let cmd = CodexCommand {
        command: "cargo test".into(),
        cwd: Some("src".into()),
        timeout_seconds: Some(60),
        stdout: Some("ok".into()),
        stderr: None,
        exit_code: Some(0),
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: CodexCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, cmd);
}

#[test]
fn api_command_minimal_serde() {
    let cmd = CodexCommand {
        command: "ls".into(),
        cwd: None,
        timeout_seconds: None,
        stdout: None,
        stderr: None,
        exit_code: None,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    assert!(!json.contains("cwd"));
    assert!(!json.contains("timeout_seconds"));
}

// =========================================================================
// 10. api streaming types serde
// =========================================================================

#[test]
fn api_stream_chunk_serde() {
    let chunk = CodexStreamChunk {
        id: "chunk_1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "codex-mini-latest".into(),
        choices: vec![CodexStreamChoice {
            index: 0,
            delta: CodexStreamDelta {
                role: Some("assistant".into()),
                content: Some("Hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: CodexStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, chunk);
}

#[test]
fn api_stream_delta_default() {
    let delta = CodexStreamDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

#[test]
fn api_stream_tool_call_serde() {
    let stc = CodexStreamToolCall {
        index: 0,
        id: Some("tc_1".into()),
        call_type: Some("function".into()),
        function: Some(CodexStreamFunctionCall {
            name: Some("bash".into()),
            arguments: Some(r#"{"cmd":"ls"}"#.into()),
        }),
    };
    let json = serde_json::to_string(&stc).unwrap();
    let parsed: CodexStreamToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, stc);
}

#[test]
fn api_stream_function_call_partial() {
    let sfc = CodexStreamFunctionCall {
        name: None,
        arguments: Some(r#"{"part"#.into()),
    };
    let json = serde_json::to_string(&sfc).unwrap();
    assert!(!json.contains("name"));
    let parsed: CodexStreamFunctionCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, sfc);
}

#[test]
fn api_stream_choice_with_finish_reason() {
    let sc = CodexStreamChoice {
        index: 0,
        delta: CodexStreamDelta::default(),
        finish_reason: Some("stop".into()),
    };
    let json = serde_json::to_string(&sc).unwrap();
    let parsed: CodexStreamChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, sc);
}

// =========================================================================
// 11. types module (Responses API style)
// =========================================================================

#[test]
fn types_session_request_serde_roundtrip() {
    let req = api::CodexSessionRequest {
        model: "codex-mini-latest".into(),
        instructions: Some("System prompt".into()),
        input: vec![api::CodexInputItem::Message {
            role: "user".into(),
            content: "hello".into(),
        }],
        tools: None,
        stream: Some(true),
        previous_response_id: Some("prev_1".into()),
        max_output_tokens: Some(4096),
        temperature: Some(0.5),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: api::CodexSessionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn types_session_request_omits_none_fields() {
    let req = make_session_request(vec![]);
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("instructions"));
    assert!(!json.contains("tools"));
    assert!(!json.contains("stream"));
    assert!(!json.contains("previous_response_id"));
}

#[test]
fn types_input_item_message_serde() {
    let item = api::CodexInputItem::Message {
        role: "user".into(),
        content: "Hi".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"message"#));
    let parsed: api::CodexInputItem = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, item);
}

#[test]
fn types_input_item_function_call_output_serde() {
    let item = api::CodexInputItem::FunctionCallOutput {
        call_id: "fc_1".into(),
        output: "result".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"function_call_output"#));
    let parsed: api::CodexInputItem = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, item);
}

#[test]
fn types_session_response_serde_roundtrip() {
    let resp = api::CodexSessionResponse {
        id: "resp_abc".into(),
        object: "response".into(),
        status: "completed".into(),
        output: vec![api::CodexOutputItem::Message {
            role: "assistant".into(),
            content: vec![api::CodexContentPart::OutputText {
                text: "Done!".into(),
            }],
        }],
        usage: Some(api::CodexUsage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
        }),
        model: "codex-mini-latest".into(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: api::CodexSessionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn types_output_item_message_serde() {
    let item = api::CodexOutputItem::Message {
        role: "assistant".into(),
        content: vec![api::CodexContentPart::OutputText {
            text: "Hello".into(),
        }],
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"message"#));
}

#[test]
fn types_output_item_function_call_serde() {
    let item = api::CodexOutputItem::FunctionCall {
        id: "fc_1".into(),
        call_id: Some("corr_1".into()),
        name: "read".into(),
        arguments: r#"{"path":"a.rs"}"#.into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"function_call"#));
    let parsed: api::CodexOutputItem = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, item);
}

#[test]
fn types_output_item_function_call_no_call_id() {
    let item = api::CodexOutputItem::FunctionCall {
        id: "fc_2".into(),
        call_id: None,
        name: "bash".into(),
        arguments: "{}".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(!json.contains("call_id"));
}

#[test]
fn types_content_part_output_text_serde() {
    let part = api::CodexContentPart::OutputText {
        text: "Hello world".into(),
    };
    let json = serde_json::to_string(&part).unwrap();
    assert!(json.contains(r#""type":"output_text"#));
    let parsed: api::CodexContentPart = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, part);
}

#[test]
fn types_usage_serde() {
    let usage = api::CodexUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: api::CodexUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}

#[test]
fn types_tool_serde() {
    let tool = api::CodexTool {
        tool_type: "function".into(),
        function: api::CodexFunctionDef {
            name: "my_tool".into(),
            description: Some("desc".into()),
            parameters: Some(json!({"type": "object"})),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let parsed: api::CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn types_tool_minimal_omits_none() {
    let tool = api::CodexTool {
        tool_type: "function".into(),
        function: api::CodexFunctionDef {
            name: "noop".into(),
            description: None,
            parameters: None,
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(!json.contains("description"));
    assert!(!json.contains("parameters"));
}

// =========================================================================
// 12. types::From<CodexSessionRequest> for WorkOrder
// =========================================================================

#[test]
fn types_request_to_work_order_last_user_message_as_task() {
    let req = make_session_request(vec![
        api::CodexInputItem::Message {
            role: "user".into(),
            content: "First".into(),
        },
        api::CodexInputItem::Message {
            role: "user".into(),
            content: "Second".into(),
        },
    ]);
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Second");
}

#[test]
fn types_request_to_work_order_empty_input() {
    let req = make_session_request(vec![]);
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "");
}

#[test]
fn types_request_to_work_order_preserves_model() {
    let mut req = make_session_request(vec![api::CodexInputItem::Message {
        role: "user".into(),
        content: "hi".into(),
    }]);
    req.model = "o4-mini".into();
    let wo: WorkOrder = req.into();
    assert_eq!(wo.config.model.as_deref(), Some("o4-mini"));
}

#[test]
fn types_request_to_work_order_instructions_as_snippet() {
    let mut req = make_session_request(vec![api::CodexInputItem::Message {
        role: "user".into(),
        content: "hi".into(),
    }]);
    req.instructions = Some("Be concise.".into());
    let wo: WorkOrder = req.into();
    assert_eq!(wo.context.snippets[0].name, "instructions");
    assert_eq!(wo.context.snippets[0].content, "Be concise.");
}

#[test]
fn types_request_to_work_order_system_messages_as_snippets() {
    let req = make_session_request(vec![
        api::CodexInputItem::Message {
            role: "system".into(),
            content: "System prompt".into(),
        },
        api::CodexInputItem::Message {
            role: "user".into(),
            content: "Hi".into(),
        },
    ]);
    let wo: WorkOrder = req.into();
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].content, "System prompt");
}

#[test]
fn types_request_to_work_order_ignores_function_call_output() {
    let req = make_session_request(vec![api::CodexInputItem::FunctionCallOutput {
        call_id: "fc_1".into(),
        output: "data".into(),
    }]);
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "");
}

#[test]
fn types_request_to_work_order_previous_response_id() {
    let mut req = make_session_request(vec![api::CodexInputItem::Message {
        role: "user".into(),
        content: "continue".into(),
    }]);
    req.previous_response_id = Some("resp_prev".into());
    let wo: WorkOrder = req.into();
    assert_eq!(
        wo.config.vendor.get("previous_response_id"),
        Some(&serde_json::Value::String("resp_prev".into()))
    );
}

#[test]
fn types_request_to_work_order_stream_flag() {
    let mut req = make_session_request(vec![api::CodexInputItem::Message {
        role: "user".into(),
        content: "hi".into(),
    }]);
    req.stream = Some(true);
    let wo: WorkOrder = req.into();
    assert_eq!(
        wo.config.vendor.get("stream"),
        Some(&serde_json::Value::Bool(true))
    );
}

// =========================================================================
// 13. types::From<Receipt> for CodexSessionResponse
// =========================================================================

#[test]
fn types_receipt_to_response_maps_assistant_text() {
    let trace = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        },
        ext: None,
    }];
    let receipt = make_receipt_with_trace(trace, UsageNormalized::default());
    let resp: api::CodexSessionResponse = receipt.into();
    assert_eq!(resp.status, "completed");
    assert_eq!(resp.output.len(), 1);
}

#[test]
fn types_receipt_to_response_maps_tool_calls() {
    let trace = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("call_1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "main.rs"}),
        },
        ext: None,
    }];
    let receipt = make_receipt_with_trace(trace, UsageNormalized::default());
    let resp: api::CodexSessionResponse = receipt.into();
    assert_eq!(resp.output.len(), 1);
    match &resp.output[0] {
        api::CodexOutputItem::FunctionCall { name, .. } => assert_eq!(name, "read_file"),
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn types_receipt_to_response_failed_outcome() {
    let mut receipt = make_receipt_with_trace(vec![], UsageNormalized::default());
    receipt.outcome = Outcome::Failed;
    let resp: api::CodexSessionResponse = receipt.into();
    assert_eq!(resp.status, "failed");
}

#[test]
fn types_receipt_to_response_partial_outcome() {
    let mut receipt = make_receipt_with_trace(vec![], UsageNormalized::default());
    receipt.outcome = Outcome::Partial;
    let resp: api::CodexSessionResponse = receipt.into();
    assert_eq!(resp.status, "incomplete");
}

#[test]
fn types_receipt_to_response_maps_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..UsageNormalized::default()
    };
    let receipt = make_receipt_with_trace(vec![], usage);
    let resp: api::CodexSessionResponse = receipt.into();
    let u = resp.usage.unwrap();
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

#[test]
fn types_receipt_to_response_no_usage_when_none() {
    let receipt = make_receipt_with_trace(vec![], UsageNormalized::default());
    let resp: api::CodexSessionResponse = receipt.into();
    assert!(resp.usage.is_none());
}

#[test]
fn types_receipt_to_response_id_prefix() {
    let receipt = make_receipt_with_trace(vec![], UsageNormalized::default());
    let run_id = receipt.meta.run_id;
    let resp: api::CodexSessionResponse = receipt.into();
    assert!(resp.id.starts_with("resp_"));
    assert!(resp.id.contains(&run_id.to_string()));
}

#[test]
fn types_receipt_to_response_empty_trace() {
    let receipt = make_receipt_with_trace(vec![], UsageNormalized::default());
    let resp: api::CodexSessionResponse = receipt.into();
    assert!(resp.output.is_empty());
}

#[test]
fn types_receipt_to_response_concatenates_assistant_messages() {
    let trace = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Part 1. ".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Part 2.".into(),
            },
            ext: None,
        },
    ];
    let receipt = make_receipt_with_trace(trace, UsageNormalized::default());
    let resp: api::CodexSessionResponse = receipt.into();
    assert_eq!(resp.output.len(), 1);
}

// =========================================================================
// 14. convert module
// =========================================================================

#[test]
fn convert_to_work_order_extracts_task() {
    let req = make_api_request();
    let wo = convert::to_work_order(&req);
    assert_eq!(wo.task, "Fix the bug");
}

#[test]
fn convert_to_work_order_sets_model() {
    let req = make_api_request();
    let wo = convert::to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
}

#[test]
fn convert_to_work_order_stores_instructions() {
    let req = make_api_request();
    let wo = convert::to_work_order(&req);
    let codex = wo.config.vendor.get("codex").unwrap();
    assert_eq!(codex["instructions"], "Be helpful.");
}

#[test]
fn convert_to_work_order_stores_dialect() {
    let req = make_api_request();
    let wo = convert::to_work_order(&req);
    let codex = wo.config.vendor.get("codex").unwrap();
    assert!(codex.get("dialect").is_some());
}

#[test]
fn convert_to_work_order_stores_temperature() {
    let req = make_api_request();
    let wo = convert::to_work_order(&req);
    let codex = wo.config.vendor.get("codex").unwrap();
    let t = codex["temperature"].as_f64().unwrap();
    assert!((t - 0.7).abs() < f64::EPSILON);
}

#[test]
fn convert_to_work_order_no_user_message_fallback() {
    let req = CodexRequest {
        model: "m".into(),
        messages: vec![CodexMessage::System {
            content: "sys".into(),
        }],
        instructions: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
    };
    let wo = convert::to_work_order(&req);
    assert_eq!(wo.task, "(empty)");
}

#[test]
fn convert_to_work_order_concatenates_user_messages() {
    let req = CodexRequest {
        model: "m".into(),
        messages: vec![
            CodexMessage::User {
                content: "Part 1".into(),
            },
            CodexMessage::User {
                content: "Part 2".into(),
            },
        ],
        instructions: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
    };
    let wo = convert::to_work_order(&req);
    assert!(wo.task.contains("Part 1"));
    assert!(wo.task.contains("Part 2"));
}

#[test]
fn convert_to_work_order_no_instructions_omits_key() {
    let mut req = make_api_request();
    req.instructions = None;
    let wo = convert::to_work_order(&req);
    let codex = wo.config.vendor.get("codex").unwrap();
    assert!(codex.get("instructions").is_none());
}

#[test]
fn convert_to_work_order_stream_flag() {
    let mut req = make_api_request();
    req.stream = Some(true);
    let wo = convert::to_work_order(&req);
    let codex = wo.config.vendor.get("codex").unwrap();
    assert_eq!(codex["stream"], true);
}

#[test]
fn convert_to_work_order_top_p() {
    let mut req = make_api_request();
    req.top_p = Some(0.95);
    let wo = convert::to_work_order(&req);
    let codex = wo.config.vendor.get("codex").unwrap();
    let tp = codex["top_p"].as_f64().unwrap();
    assert!((tp - 0.95).abs() < f64::EPSILON);
}

#[test]
fn convert_from_receipt_valid_response() {
    let wo = WorkOrderBuilder::new("task")
        .model("codex-mini-latest")
        .build();
    let receipt = ReceiptBuilder::new("codex")
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
                text: "Done.".into(),
            },
            ext: None,
        })
        .build();
    let resp = convert::from_receipt(&receipt, &wo);
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.model, "codex-mini-latest");
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Done."));
}

#[test]
fn convert_from_receipt_finish_reason_stop() {
    let wo = WorkOrderBuilder::new("t").build();
    let receipt = ReceiptBuilder::new("codex")
        .outcome(Outcome::Complete)
        .build();
    let resp = convert::from_receipt(&receipt, &wo);
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn convert_from_receipt_finish_reason_length() {
    let wo = WorkOrderBuilder::new("t").build();
    let receipt = ReceiptBuilder::new("codex")
        .outcome(Outcome::Partial)
        .build();
    let resp = convert::from_receipt(&receipt, &wo);
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("length"));
}

#[test]
fn convert_from_receipt_no_usage_when_zero() {
    let wo = WorkOrderBuilder::new("t").build();
    let receipt = ReceiptBuilder::new("codex")
        .outcome(Outcome::Complete)
        .build();
    let resp = convert::from_receipt(&receipt, &wo);
    assert!(resp.usage.is_none());
}

#[test]
fn convert_from_receipt_usage_computed() {
    let wo = WorkOrderBuilder::new("t").build();
    let receipt = ReceiptBuilder::new("codex")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(200),
            output_tokens: Some(100),
            ..UsageNormalized::default()
        })
        .build();
    let resp = convert::from_receipt(&receipt, &wo);
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 200);
    assert_eq!(u.completion_tokens, 100);
    assert_eq!(u.total_tokens, 300);
}

// =========================================================================
// 15. convert file-change helpers
// =========================================================================

#[test]
fn convert_file_change_to_event_create() {
    let fc = CodexFileChange {
        path: "new.rs".into(),
        operation: FileOperation::Create,
        content: Some("code".into()),
        diff: None,
    };
    let event = convert::file_change_to_event(&fc);
    match &event.kind {
        AgentEventKind::FileChanged { path, summary } => {
            assert_eq!(path, "new.rs");
            assert!(summary.contains("Created"));
        }
        other => panic!("expected FileChanged, got {other:?}"),
    }
}

#[test]
fn convert_file_change_to_event_update() {
    let fc = CodexFileChange {
        path: "f.rs".into(),
        operation: FileOperation::Update,
        content: None,
        diff: None,
    };
    let event = convert::file_change_to_event(&fc);
    if let AgentEventKind::FileChanged { summary, .. } = &event.kind {
        assert!(summary.contains("Updated"));
    }
}

#[test]
fn convert_file_change_to_event_delete() {
    let fc = CodexFileChange {
        path: "old.txt".into(),
        operation: FileOperation::Delete,
        content: None,
        diff: None,
    };
    let event = convert::file_change_to_event(&fc);
    if let AgentEventKind::FileChanged { summary, .. } = &event.kind {
        assert!(summary.contains("Deleted"));
    }
}

#[test]
fn convert_file_change_to_event_patch() {
    let fc = CodexFileChange {
        path: "x.rs".into(),
        operation: FileOperation::Patch,
        content: None,
        diff: Some("diff".into()),
    };
    let event = convert::file_change_to_event(&fc);
    if let AgentEventKind::FileChanged { summary, .. } = &event.kind {
        assert!(summary.contains("Patched"));
    }
}

#[test]
fn convert_event_to_file_change_roundtrip() {
    let fc = CodexFileChange {
        path: "lib.rs".into(),
        operation: FileOperation::Update,
        content: None,
        diff: None,
    };
    let event = convert::file_change_to_event(&fc);
    let reconstructed = convert::event_to_file_change(&event).unwrap();
    assert_eq!(reconstructed.path, "lib.rs");
    assert_eq!(reconstructed.operation, FileOperation::Update);
}

#[test]
fn convert_event_to_file_change_none_for_non_file() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    assert!(convert::event_to_file_change(&event).is_none());
}

#[test]
fn convert_extract_file_changes() {
    let receipt = ReceiptBuilder::new("codex")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "a.rs".into(),
                summary: "Created a.rs".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "b.rs".into(),
                summary: "Deleted b.rs".into(),
            },
            ext: None,
        })
        .build();
    let changes = convert::extract_file_changes(&receipt);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].operation, FileOperation::Create);
    assert_eq!(changes[1].operation, FileOperation::Delete);
}

// =========================================================================
// 16. dialect module - constants and model mapping
// =========================================================================

#[test]
fn dialect_version_constant() {
    assert_eq!(dialect::DIALECT_VERSION, "codex/v0.1");
}

#[test]
fn dialect_default_model_constant() {
    assert_eq!(dialect::DEFAULT_MODEL, "codex-mini-latest");
}

#[test]
fn dialect_to_canonical_model() {
    let canonical = dialect::to_canonical_model("codex-mini-latest");
    assert_eq!(canonical, "openai/codex-mini-latest");
}

#[test]
fn dialect_from_canonical_model_strips_prefix() {
    let vendor = dialect::from_canonical_model("openai/gpt-4");
    assert_eq!(vendor, "gpt-4");
}

#[test]
fn dialect_from_canonical_model_no_prefix() {
    let vendor = dialect::from_canonical_model("custom-model");
    assert_eq!(vendor, "custom-model");
}

#[test]
fn dialect_is_known_model_true() {
    assert!(dialect::is_known_model("codex-mini-latest"));
    assert!(dialect::is_known_model("o3-mini"));
    assert!(dialect::is_known_model("gpt-4"));
}

#[test]
fn dialect_is_known_model_false() {
    assert!(!dialect::is_known_model("unknown-model"));
}

// =========================================================================
// 17. dialect capability manifest
// =========================================================================

#[test]
fn dialect_capability_manifest_has_streaming() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn dialect_capability_manifest_has_tool_read() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn dialect_capability_manifest_has_tool_bash() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolBash),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn dialect_capability_manifest_glob_emulated() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolGlob),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn dialect_capability_manifest_mcp_unsupported() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpClient),
        Some(SupportLevel::Unsupported)
    ));
    assert!(matches!(
        m.get(&Capability::McpServer),
        Some(SupportLevel::Unsupported)
    ));
}

// =========================================================================
// 18. dialect tool def conversions
// =========================================================================

#[test]
fn dialect_tool_def_to_codex() {
    let canonical = dialect::CanonicalToolDef {
        name: "search".into(),
        description: "Search code".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let codex = dialect::tool_def_to_codex(&canonical);
    assert_eq!(codex.tool_type, "function");
    assert_eq!(codex.function.name, "search");
    assert_eq!(codex.function.description, "Search code");
}

#[test]
fn dialect_tool_def_from_codex() {
    let codex = dialect::CodexToolDef {
        tool_type: "function".into(),
        function: dialect::CodexFunctionDef {
            name: "read".into(),
            description: "Read file".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let canonical = dialect::tool_def_from_codex(&codex);
    assert_eq!(canonical.name, "read");
    assert_eq!(canonical.description, "Read file");
}

#[test]
fn dialect_tool_def_roundtrip() {
    let original = dialect::CanonicalToolDef {
        name: "tool".into(),
        description: "desc".into(),
        parameters_schema: json!({"type": "object", "properties": {"x": {"type": "string"}}}),
    };
    let codex = dialect::tool_def_to_codex(&original);
    let back = dialect::tool_def_from_codex(&codex);
    assert_eq!(back, original);
}

#[test]
fn dialect_codex_tool_function_to_canonical() {
    let tool = dialect::CodexTool::Function {
        function: dialect::CodexFunctionDef {
            name: "bash".into(),
            description: "Run command".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "bash");
}

#[test]
fn dialect_codex_tool_code_interpreter_to_canonical() {
    let tool = dialect::CodexTool::CodeInterpreter {};
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "code_interpreter");
}

#[test]
fn dialect_codex_tool_file_search_to_canonical() {
    let tool = dialect::CodexTool::FileSearch {
        max_num_results: Some(10),
    };
    let canonical = dialect::codex_tool_to_canonical(&tool);
    assert_eq!(canonical.name, "file_search");
}

// =========================================================================
// 19. dialect sandbox/network/file access types
// =========================================================================

#[test]
fn dialect_network_access_default_is_none() {
    let na = dialect::NetworkAccess::default();
    assert_eq!(na, dialect::NetworkAccess::None);
}

#[test]
fn dialect_file_access_default_is_workspace_only() {
    let fa = dialect::FileAccess::default();
    assert_eq!(fa, dialect::FileAccess::WorkspaceOnly);
}

#[test]
fn dialect_sandbox_config_default() {
    let sc = dialect::SandboxConfig::default();
    assert!(sc.container_image.is_none());
    assert_eq!(sc.networking, dialect::NetworkAccess::None);
    assert_eq!(sc.file_access, dialect::FileAccess::WorkspaceOnly);
    assert_eq!(sc.timeout_seconds, Some(300));
    assert_eq!(sc.memory_mb, Some(512));
    assert!(sc.env.is_empty());
}

#[test]
fn dialect_sandbox_config_serde_roundtrip() {
    let sc = dialect::SandboxConfig {
        container_image: Some("node:20".into()),
        networking: dialect::NetworkAccess::Full,
        file_access: dialect::FileAccess::Full,
        timeout_seconds: Some(600),
        memory_mb: Some(1024),
        env: {
            let mut m = BTreeMap::new();
            m.insert("KEY".into(), "VALUE".into());
            m
        },
    };
    let json = serde_json::to_string(&sc).unwrap();
    let parsed: dialect::SandboxConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, sc);
}

#[test]
fn dialect_network_access_allow_list_serde() {
    let na = dialect::NetworkAccess::AllowList(vec!["example.com".into()]);
    let json = serde_json::to_string(&na).unwrap();
    let parsed: dialect::NetworkAccess = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, na);
}

// =========================================================================
// 20. dialect CodexConfig
// =========================================================================

#[test]
fn dialect_codex_config_default() {
    let cfg = dialect::CodexConfig::default();
    assert!(cfg.api_key.is_empty());
    assert!(cfg.base_url.contains("openai.com"));
    assert_eq!(cfg.model, "codex-mini-latest");
    assert_eq!(cfg.max_output_tokens, Some(4096));
    assert!(cfg.temperature.is_none());
}

// =========================================================================
// 21. dialect CodexTextFormat
// =========================================================================

#[test]
fn dialect_text_format_default_is_text() {
    let tf = dialect::CodexTextFormat::default();
    assert!(matches!(tf, dialect::CodexTextFormat::Text {}));
}

#[test]
fn dialect_text_format_json_schema_serde() {
    let tf = dialect::CodexTextFormat::JsonSchema {
        name: "my_schema".into(),
        schema: json!({"type": "object"}),
        strict: true,
    };
    let json = serde_json::to_string(&tf).unwrap();
    let parsed: dialect::CodexTextFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tf);
}

#[test]
fn dialect_text_format_json_object_serde() {
    let tf = dialect::CodexTextFormat::JsonObject {};
    let json = serde_json::to_string(&tf).unwrap();
    let parsed: dialect::CodexTextFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tf);
}

// =========================================================================
// 22. dialect map_work_order
// =========================================================================

#[test]
fn dialect_map_work_order_task_as_user_message() {
    let wo = WorkOrderBuilder::new("Write tests").build();
    let cfg = dialect::CodexConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.input.len(), 1);
    match &req.input[0] {
        dialect::CodexInputItem::Message { role, content } => {
            assert_eq!(role, "user");
            assert!(content.contains("Write tests"));
        }
    }
}

#[test]
fn dialect_map_work_order_model_override() {
    let wo = WorkOrderBuilder::new("t").model("o3-mini").build();
    let cfg = dialect::CodexConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "o3-mini");
}

#[test]
fn dialect_map_work_order_uses_config_model_default() {
    let wo = WorkOrderBuilder::new("t").build();
    let cfg = dialect::CodexConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "codex-mini-latest");
}

// =========================================================================
// 23. dialect map_response
// =========================================================================

#[test]
fn dialect_map_response_message() {
    let resp = dialect::CodexResponse {
        id: "r1".into(),
        model: "m".into(),
        output: vec![dialect::CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![dialect::CodexContentPart::OutputText { text: "Hi".into() }],
        }],
        usage: None,
        status: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hi"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn dialect_map_response_function_call() {
    let resp = dialect::CodexResponse {
        id: "r1".into(),
        model: "m".into(),
        output: vec![dialect::CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "bash".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
        }],
        usage: None,
        status: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
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
fn dialect_map_response_function_call_output() {
    let resp = dialect::CodexResponse {
        id: "r1".into(),
        model: "m".into(),
        output: vec![dialect::CodexResponseItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "result".into(),
        }],
        usage: None,
        status: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolResult {
            tool_use_id,
            output,
            ..
        } => {
            assert_eq!(tool_use_id.as_deref(), Some("fc_1"));
            assert_eq!(output, &serde_json::Value::String("result".into()));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn dialect_map_response_reasoning() {
    let resp = dialect::CodexResponse {
        id: "r1".into(),
        model: "m".into(),
        output: vec![dialect::CodexResponseItem::Reasoning {
            summary: vec![dialect::ReasoningSummary {
                text: "Thinking...".into(),
            }],
        }],
        usage: None,
        status: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert!(text.contains("Thinking")),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn dialect_map_response_empty_reasoning_no_event() {
    let resp = dialect::CodexResponse {
        id: "r1".into(),
        model: "m".into(),
        output: vec![dialect::CodexResponseItem::Reasoning { summary: vec![] }],
        usage: None,
        status: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

// =========================================================================
// 24. dialect map_stream_event
// =========================================================================

#[test]
fn dialect_map_stream_event_response_created() {
    let resp = dialect::CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![],
        usage: None,
        status: None,
    };
    let ev = dialect::CodexStreamEvent::ResponseCreated { response: resp };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn dialect_map_stream_event_in_progress_no_events() {
    let resp = dialect::CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![],
        usage: None,
        status: None,
    };
    let ev = dialect::CodexStreamEvent::ResponseInProgress { response: resp };
    let events = dialect::map_stream_event(&ev);
    assert!(events.is_empty());
}

#[test]
fn dialect_map_stream_event_output_text_delta() {
    let ev = dialect::CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: dialect::CodexStreamDelta::OutputTextDelta {
            text: "chunk".into(),
        },
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "chunk"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_event_function_args_delta_empty() {
    let ev = dialect::CodexStreamEvent::OutputItemDelta {
        output_index: 0,
        delta: dialect::CodexStreamDelta::FunctionCallArgumentsDelta {
            delta: r#"{"pa"#.into(),
        },
    };
    let events = dialect::map_stream_event(&ev);
    assert!(events.is_empty());
}

#[test]
fn dialect_map_stream_event_completed() {
    let resp = dialect::CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![],
        usage: None,
        status: None,
    };
    let ev = dialect::CodexStreamEvent::ResponseCompleted { response: resp };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn dialect_map_stream_event_failed() {
    let resp = dialect::CodexResponse {
        id: "r".into(),
        model: "m".into(),
        output: vec![],
        usage: None,
        status: Some("failed".into()),
    };
    let ev = dialect::CodexStreamEvent::ResponseFailed { response: resp };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => assert_eq!(message, "failed"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_event_error() {
    let ev = dialect::CodexStreamEvent::Error {
        message: "rate limit".into(),
        code: Some("429".into()),
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => assert_eq!(message, "rate limit"),
        other => panic!("expected Error, got {other:?}"),
    }
}

// =========================================================================
// 25. dialect streaming serde
// =========================================================================

#[test]
fn dialect_stream_delta_output_text_serde() {
    let d = dialect::CodexStreamDelta::OutputTextDelta { text: "hi".into() };
    let json = serde_json::to_string(&d).unwrap();
    let parsed: dialect::CodexStreamDelta = serde_json::from_str(&json).unwrap();
    match parsed {
        dialect::CodexStreamDelta::OutputTextDelta { text } => assert_eq!(text, "hi"),
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn dialect_stream_delta_func_args_serde() {
    let d = dialect::CodexStreamDelta::FunctionCallArgumentsDelta {
        delta: r#"{"x"#.into(),
    };
    let json = serde_json::to_string(&d).unwrap();
    let parsed: dialect::CodexStreamDelta = serde_json::from_str(&json).unwrap();
    match parsed {
        dialect::CodexStreamDelta::FunctionCallArgumentsDelta { delta } => {
            assert_eq!(delta, r#"{"x"#)
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn dialect_stream_delta_reasoning_serde() {
    let d = dialect::CodexStreamDelta::ReasoningSummaryDelta {
        text: "thought".into(),
    };
    let json = serde_json::to_string(&d).unwrap();
    let parsed: dialect::CodexStreamDelta = serde_json::from_str(&json).unwrap();
    match parsed {
        dialect::CodexStreamDelta::ReasoningSummaryDelta { text } => assert_eq!(text, "thought"),
        other => panic!("wrong variant: {other:?}"),
    }
}

// =========================================================================
// 26. lowering module
// =========================================================================

#[test]
fn lowering_input_to_ir_user() {
    let items = vec![dialect::CodexInputItem::Message {
        role: "user".into(),
        content: "Hello".into(),
    }];
    let conv = lowering::input_to_ir(&items);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello");
}

#[test]
fn lowering_input_to_ir_system() {
    let items = vec![dialect::CodexInputItem::Message {
        role: "system".into(),
        content: "Be helpful".into(),
    }];
    let conv = lowering::input_to_ir(&items);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn lowering_input_to_ir_empty_content() {
    let items = vec![dialect::CodexInputItem::Message {
        role: "user".into(),
        content: String::new(),
    }];
    let conv = lowering::input_to_ir(&items);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn lowering_to_ir_message() {
    let items = vec![dialect::CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![dialect::CodexContentPart::OutputText {
            text: "Done!".into(),
        }],
    }];
    let conv = lowering::to_ir(&items);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    assert_eq!(conv.messages[0].text_content(), "Done!");
}

#[test]
fn lowering_to_ir_function_call() {
    let items = vec![dialect::CodexResponseItem::FunctionCall {
        id: "fc_1".into(),
        call_id: None,
        name: "shell".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
    }];
    let conv = lowering::to_ir(&items);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "fc_1");
            assert_eq!(name, "shell");
            assert_eq!(input, &json!({"cmd": "ls"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn lowering_to_ir_function_call_output() {
    let items = vec![dialect::CodexResponseItem::FunctionCallOutput {
        call_id: "fc_1".into(),
        output: "result".into(),
    }];
    let conv = lowering::to_ir(&items);
    assert_eq!(conv.messages[0].role, IrRole::Tool);
}

#[test]
fn lowering_to_ir_reasoning() {
    let items = vec![dialect::CodexResponseItem::Reasoning {
        summary: vec![
            dialect::ReasoningSummary {
                text: "Step 1".into(),
            },
            dialect::ReasoningSummary {
                text: "Step 2".into(),
            },
        ],
    }];
    let conv = lowering::to_ir(&items);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => {
            assert!(text.contains("Step 1"));
            assert!(text.contains("Step 2"));
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn lowering_from_ir_assistant_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Hello")]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        dialect::CodexResponseItem::Message { role, content } => {
            assert_eq!(role, "assistant");
            match &content[0] {
                dialect::CodexContentPart::OutputText { text } => assert_eq!(text, "Hello"),
            }
        }
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn lowering_from_ir_skips_system_and_user() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "instructions"),
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::text(IrRole::Assistant, "hi"),
    ]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
}

#[test]
fn lowering_from_ir_tool_use() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "bash".into(),
            input: json!({"cmd": "ls"}),
        }],
    )]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        dialect::CodexResponseItem::FunctionCall { id, name, .. } => {
            assert_eq!(id, "t1");
            assert_eq!(name, "bash");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn lowering_from_ir_tool_result() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: vec![IrContentBlock::Text { text: "ok".into() }],
            is_error: false,
        }],
    )]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        dialect::CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "t1");
            assert_eq!(output, "ok");
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

#[test]
fn lowering_from_ir_thinking() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "Hmm...".into(),
        }],
    )]);
    let items = lowering::from_ir(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        dialect::CodexResponseItem::Reasoning { summary } => {
            assert_eq!(summary[0].text, "Hmm...");
        }
        other => panic!("expected Reasoning, got {other:?}"),
    }
}

#[test]
fn lowering_empty_items_roundtrip() {
    let conv = lowering::to_ir(&[]);
    assert!(conv.is_empty());
    let back = lowering::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn lowering_usage_to_ir() {
    let usage = dialect::CodexUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
    };
    let ir = lowering::usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 100);
    assert_eq!(ir.output_tokens, 50);
    assert_eq!(ir.total_tokens, 150);
}

#[test]
fn lowering_malformed_function_arguments() {
    let items = vec![dialect::CodexResponseItem::FunctionCall {
        id: "fc_bad".into(),
        call_id: None,
        name: "foo".into(),
        arguments: "not-json".into(),
    }];
    let conv = lowering::to_ir(&items);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-json".into()));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// =========================================================================
// 27. lib.rs constants and sidecar_script
// =========================================================================

#[test]
fn lib_backend_name_constant() {
    assert_eq!(abp_codex_sdk::BACKEND_NAME, "sidecar:codex");
}

#[test]
fn lib_host_script_relative_constant() {
    assert_eq!(abp_codex_sdk::HOST_SCRIPT_RELATIVE, "hosts/codex/host.js");
}

#[test]
fn lib_default_node_command_constant() {
    assert_eq!(abp_codex_sdk::DEFAULT_NODE_COMMAND, "node");
}

#[test]
fn lib_sidecar_script_path() {
    let root = Path::new("/fake/root");
    let script = abp_codex_sdk::sidecar_script(root);
    assert_eq!(script, root.join("hosts/codex/host.js"));
}

// =========================================================================
// 28. dialect CodexTool serde (tagged enum)
// =========================================================================

#[test]
fn dialect_codex_tool_function_serde() {
    let tool = dialect::CodexTool::Function {
        function: dialect::CodexFunctionDef {
            name: "search".into(),
            description: "Search".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains(r#""type":"function"#));
    let parsed: dialect::CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn dialect_codex_tool_code_interpreter_serde() {
    let tool = dialect::CodexTool::CodeInterpreter {};
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains(r#""type":"code_interpreter"#));
    let parsed: dialect::CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn dialect_codex_tool_file_search_serde() {
    let tool = dialect::CodexTool::FileSearch {
        max_num_results: Some(5),
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains(r#""type":"file_search"#));
    let parsed: dialect::CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn dialect_codex_tool_file_search_no_max() {
    let tool = dialect::CodexTool::FileSearch {
        max_num_results: None,
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(!json.contains("max_num_results"));
}

// =========================================================================
// 29. dialect response item serde
// =========================================================================

#[test]
fn dialect_response_item_reasoning_serde() {
    let item = dialect::CodexResponseItem::Reasoning {
        summary: vec![dialect::ReasoningSummary {
            text: "thinking".into(),
        }],
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"reasoning"#));
}

#[test]
fn dialect_response_item_function_call_output_serde() {
    let item = dialect::CodexResponseItem::FunctionCallOutput {
        call_id: "fc_1".into(),
        output: "data".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"function_call_output"#));
}

// =========================================================================
// 30. dialect reasoning summary
// =========================================================================

#[test]
fn dialect_reasoning_summary_serde() {
    let rs = dialect::ReasoningSummary {
        text: "Step 1: analyze".into(),
    };
    let json = serde_json::to_string(&rs).unwrap();
    let parsed: dialect::ReasoningSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, rs);
}

// =========================================================================
// 31. dialect legacy type alias
// =========================================================================

#[test]
fn dialect_output_item_alias_is_response_item() {
    let item: dialect::CodexOutputItem = dialect::CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![],
    };
    let _ = item;
}
