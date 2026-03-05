// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for Kimi ↔ abp-dialect IR translation.
//!
//! These complement the unit tests in `ir_translate.rs` with cross-module
//! integration scenarios exercised through the public API.

use kimi_bridge::ir_translate::*;
use kimi_bridge::kimi_types::*;

use abp_dialect::ir::{
    IrContentBlock, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason, IrStreamEvent,
};
use serde_json::json;

// ── Helper builders ─────────────────────────────────────────────────────

fn msg(role: Role, text: &str) -> Message {
    Message {
        role,
        content: Some(text.into()),
        tool_call_id: None,
        tool_calls: None,
    }
}

// ── Request roundtrip integration ───────────────────────────────────────

#[test]
fn full_conversation_request_roundtrip() {
    let req = KimiRequest {
        model: "moonshot-v1-128k".into(),
        messages: vec![
            msg(Role::System, "You are a coding assistant."),
            msg(Role::User, "Write a function"),
            Message {
                role: Role::Assistant,
                content: Some("Here's a function:".into()),
                tool_call_id: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: "write_file".into(),
                        arguments: r#"{"path":"main.rs","content":"fn main() {}"}"#.into(),
                    },
                }]),
            },
            Message {
                role: Role::Tool,
                content: Some("File written".into()),
                tool_call_id: Some("call_1".into()),
                tool_calls: None,
            },
            msg(Role::User, "Thanks!"),
        ],
        max_tokens: Some(4096),
        temperature: Some(0.5),
        stream: Some(true),
        tools: Some(vec![ToolDefinition::Function {
            function: FunctionDefinition {
                name: "write_file".into(),
                description: "Write a file".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
            },
        }]),
        use_search: Some(true),
    };

    let ir = kimi_request_to_ir(&req);

    // System message extracted
    assert_eq!(
        ir.system_prompt.as_deref(),
        Some("You are a coding assistant.")
    );
    // 4 non-system messages
    assert_eq!(ir.messages.len(), 4);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
    assert_eq!(ir.messages[2].role, IrRole::Tool);
    assert_eq!(ir.messages[3].role, IrRole::User);

    // Tool definitions preserved
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "write_file");

    // Metadata preserved
    assert_eq!(ir.metadata.get("stream"), Some(&json!(true)));
    assert_eq!(ir.metadata.get("use_search"), Some(&json!(true)));

    // Roundtrip back
    let back = ir_to_kimi_request(&ir);
    assert_eq!(back.model, "moonshot-v1-128k");
    // System message + 4 conversation messages
    assert_eq!(back.messages.len(), 5);
    assert_eq!(back.messages[0].role, Role::System);
    assert_eq!(back.max_tokens, Some(4096));
    assert_eq!(back.temperature, Some(0.5));
    assert_eq!(back.stream, Some(true));
    assert_eq!(back.use_search, Some(true));
}

// ── Response roundtrip with tool calls and refs ─────────────────────────

#[test]
fn response_with_everything_roundtrip() {
    let resp = KimiResponse {
        id: "cmpl-full".into(),
        model: "moonshot-v1-128k".into(),
        choices: vec![Choice {
            index: 0,
            message: ResponseMessage {
                role: "assistant".into(),
                content: Some("I found results [1].".into()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_a".into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: "analyze".into(),
                        arguments: r#"{"data":"test"}"#.into(),
                    },
                }]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: Some(Usage {
            prompt_tokens: 200,
            completion_tokens: 50,
            total_tokens: 250,
        }),
        refs: Some(vec![KimiRef {
            index: 1,
            url: "https://docs.rs".into(),
            title: Some("Rust Docs".into()),
        }]),
    };

    let ir = kimi_response_to_ir(&resp);
    assert_eq!(ir.id.as_deref(), Some("cmpl-full"));
    assert_eq!(ir.stop_reason, Some(IrStopReason::ToolUse));
    assert!(ir.metadata.contains_key("kimi_refs"));

    // Has both text and tool call
    let texts: Vec<_> = ir
        .content
        .iter()
        .filter(|b| matches!(b, IrContentBlock::Text { .. }))
        .collect();
    assert_eq!(texts.len(), 1);
    let calls = ir.tool_calls();
    assert_eq!(calls.len(), 1);

    // Roundtrip
    let back = ir_to_kimi_response(&ir);
    assert_eq!(back.id, "cmpl-full");
    assert!(back
        .choices[0]
        .message
        .content
        .as_ref()
        .unwrap()
        .contains("found results"));
    let tcs = back.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].function.name, "analyze");
    let refs = back.refs.unwrap();
    assert_eq!(refs[0].url, "https://docs.rs");
}

// ── Builtin tool integration ────────────────────────────────────────────

#[test]
fn all_builtin_tools_roundtrip_via_ir() {
    let builtins = ["$web_search", "$file_tool", "$code_tool", "$browser"];
    for name in &builtins {
        let tools = vec![ToolDefinition::BuiltinFunction {
            function: BuiltinFunctionDef {
                name: name.to_string(),
            },
        }];
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![msg(Role::User, "test")],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: Some(tools),
            use_search: None,
        };

        let ir = kimi_request_to_ir(&req);
        assert_eq!(ir.tools.len(), 1, "failed for {name}");
        assert_eq!(ir.tools[0].name, *name);

        let back = ir_to_kimi_request(&ir);
        let bt = &back.tools.unwrap()[0];
        match bt {
            ToolDefinition::BuiltinFunction { function } => {
                assert_eq!(function.name, *name);
            }
            _ => panic!("expected BuiltinFunction for {name}"),
        }
    }
}

// ── Stream event integration ────────────────────────────────────────────

#[test]
fn full_stream_sequence() {
    // First chunk: stream start with role
    let start = StreamChunk {
        id: "stream-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: Some("assistant".into()),
                content: None,
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };
    let events = kimi_stream_to_ir(&start);
    assert!(matches!(&events[0], IrStreamEvent::StreamStart { .. }));

    // Middle chunk: text delta
    let middle = StreamChunk {
        id: "stream-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: None,
                content: Some("Hello world".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };
    let events = kimi_stream_to_ir(&middle);
    assert_eq!(events.len(), 1);
    match &events[0] {
        IrStreamEvent::TextDelta { text, .. } => assert_eq!(text, "Hello world"),
        _ => panic!("expected TextDelta"),
    }

    // Final chunk: finish + usage
    let end = StreamChunk {
        id: "stream-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        }),
        refs: None,
    };
    let events = kimi_stream_to_ir(&end);
    assert!(events
        .iter()
        .any(|e| matches!(e, IrStreamEvent::StreamEnd { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, IrStreamEvent::Usage { .. })));
}

// ── IR → Kimi with no model defaults to moonshot-v1-8k ──────────────────

#[test]
fn ir_request_without_model_gets_default() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "test")]);
    let kimi = ir_to_kimi_request(&ir);
    assert_eq!(kimi.model, "moonshot-v1-8k");
}

// ── IR response with custom content blocks ──────────────────────────────

#[test]
fn ir_response_custom_blocks_rendered_as_text() {
    let ir = IrResponse {
        id: Some("resp-custom".into()),
        model: Some("moonshot-v1-8k".into()),
        content: vec![
            IrContentBlock::Text {
                text: "Prefix ".into(),
            },
            IrContentBlock::Custom {
                custom_type: "thinking".into(),
                data: json!("deep thought"),
            },
        ],
        stop_reason: Some(IrStopReason::EndTurn),
        usage: None,
        metadata: Default::default(),
    };
    let kimi = ir_to_kimi_response(&ir);
    let content = kimi.choices[0].message.content.as_ref().unwrap();
    assert!(content.starts_with("Prefix "));
    assert!(content.contains("[thinking:"));
}

// ── Empty tools list ────────────────────────────────────────────────────

#[test]
fn empty_tools_become_none_on_roundtrip() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")])
        .with_model("moonshot-v1-8k");
    let kimi = ir_to_kimi_request(&ir);
    assert!(kimi.tools.is_none());
}

// ── Stop reason Other variant ───────────────────────────────────────────

#[test]
fn unknown_stop_reason_preserved_as_other() {
    let resp = KimiResponse {
        id: "cmpl-unk".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![Choice {
            index: 0,
            message: ResponseMessage {
                role: "assistant".into(),
                content: Some("done".into()),
                tool_calls: None,
            },
            finish_reason: Some("custom_reason".into()),
        }],
        usage: None,
        refs: None,
    };
    let ir = kimi_response_to_ir(&resp);
    assert_eq!(
        ir.stop_reason,
        Some(IrStopReason::Other("custom_reason".into()))
    );

    let back = ir_to_kimi_response(&ir);
    assert_eq!(
        back.choices[0].finish_reason.as_deref(),
        Some("custom_reason")
    );
}

// ── StopSequence maps to "stop" ─────────────────────────────────────────

#[test]
fn stop_sequence_maps_to_stop() {
    let ir = IrResponse::text("ok").with_stop_reason(IrStopReason::StopSequence);
    let kimi = ir_to_kimi_response(&ir);
    assert_eq!(kimi.choices[0].finish_reason.as_deref(), Some("stop"));
}

// ── Tool result with empty content ──────────────────────────────────────

#[test]
fn tool_result_empty_content() {
    let ir_msg = IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_call_id: "call_empty".into(),
            content: vec![],
            is_error: false,
        }],
    );
    let ir = IrRequest::new(vec![ir_msg]).with_model("moonshot-v1-8k");
    let kimi = ir_to_kimi_request(&ir);
    assert_eq!(kimi.messages[0].role, Role::Tool);
    assert!(kimi.messages[0].content.is_none());
    assert_eq!(
        kimi.messages[0].tool_call_id.as_deref(),
        Some("call_empty")
    );
}

// ── Invalid JSON in tool call arguments ─────────────────────────────────

#[test]
fn invalid_json_arguments_become_null_in_ir() {
    let resp = KimiResponse {
        id: "cmpl-bad".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![Choice {
            index: 0,
            message: ResponseMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_bad".into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: "broken".into(),
                        arguments: "not valid json{{{".into(),
                    },
                }]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
        refs: None,
    };
    let ir = kimi_response_to_ir(&resp);
    let calls = ir.tool_calls();
    assert_eq!(calls.len(), 1);
    if let IrContentBlock::ToolCall { input, .. } = &calls[0] {
        assert!(input.is_null());
    } else {
        panic!("expected ToolCall");
    }
}
