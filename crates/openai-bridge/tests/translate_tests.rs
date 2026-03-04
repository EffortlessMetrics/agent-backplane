#![allow(clippy::all)]
#![allow(clippy::needless_update)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for translate module (IR round-trips, streaming, tool calls, edge cases).

use abp_core::ir::{IrContentBlock, IrMessage, IrRole, IrToolDefinition, IrUsage};
use openai_bridge::openai_types::*;
use openai_bridge::translate::{
    api_error_to_bridge, conversation_from_ir, conversation_to_ir, extract_usage, merge_usage,
    message_from_ir, message_to_ir, response_content_to_ir, response_to_ir_message,
    role_from_ir, role_to_ir, task_to_request, tool_def_from_ir, tool_def_to_ir, usage_from_ir,
    usage_to_ir, StreamAccumulator, StreamFragment,
};

// ── Role mapping ───────────────────────────────────────────────────

#[test]
fn role_system_round_trip() {
    assert_eq!(role_to_ir(ChatMessageRole::System), IrRole::System);
    assert_eq!(role_from_ir(IrRole::System), ChatMessageRole::System);
}

#[test]
fn role_user_round_trip() {
    assert_eq!(role_to_ir(ChatMessageRole::User), IrRole::User);
    assert_eq!(role_from_ir(IrRole::User), ChatMessageRole::User);
}

#[test]
fn role_assistant_round_trip() {
    assert_eq!(role_to_ir(ChatMessageRole::Assistant), IrRole::Assistant);
    assert_eq!(role_from_ir(IrRole::Assistant), ChatMessageRole::Assistant);
}

#[test]
fn role_tool_round_trip() {
    assert_eq!(role_to_ir(ChatMessageRole::Tool), IrRole::Tool);
    assert_eq!(role_from_ir(IrRole::Tool), ChatMessageRole::Tool);
}

// ── Message mapping ────────────────────────────────────────────────

#[test]
fn simple_user_message_to_ir() {
    let msg = ChatMessage {
        role: ChatMessageRole::User,
        content: Some("Hello".into()),
        tool_calls: None,
        tool_call_id: None,
    };
    let ir = message_to_ir(&msg);
    assert_eq!(ir.role, IrRole::User);
    assert_eq!(ir.content.len(), 1);
    assert!(matches!(&ir.content[0], IrContentBlock::Text { text } if text == "Hello"));
}

#[test]
fn simple_user_message_from_ir() {
    let ir = IrMessage::text(IrRole::User, "Hello");
    let msg = message_from_ir(&ir);
    assert_eq!(msg.role, ChatMessageRole::User);
    assert_eq!(msg.content, Some("Hello".into()));
    assert!(msg.tool_calls.is_none());
}

#[test]
fn assistant_message_with_tool_calls_to_ir() {
    let msg = ChatMessage {
        role: ChatMessageRole::Assistant,
        content: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "get_weather".into(),
                arguments: r#"{"city":"NYC"}"#.into(),
            },
        }]),
        tool_call_id: None,
    };
    let ir = message_to_ir(&msg);
    assert_eq!(ir.role, IrRole::Assistant);
    assert_eq!(ir.content.len(), 1);
    match &ir.content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "get_weather");
            assert_eq!(*input, serde_json::json!({"city": "NYC"}));
        }
        _ => panic!("expected ToolUse block"),
    }
}

#[test]
fn assistant_message_with_tool_calls_from_ir() {
    let ir = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "search".into(),
            input: serde_json::json!({"q": "test"}),
        }],
    );
    let msg = message_from_ir(&ir);
    assert_eq!(msg.role, ChatMessageRole::Assistant);
    assert!(msg.content.is_none());
    let tcs = msg.tool_calls.unwrap();
    assert_eq!(tcs.len(), 1);
    assert_eq!(tcs[0].id, "call_1");
    assert_eq!(tcs[0].function.name, "search");
}

#[test]
fn tool_result_message_to_ir() {
    let msg = ChatMessage {
        role: ChatMessageRole::Tool,
        content: Some("72 degrees".into()),
        tool_calls: None,
        tool_call_id: Some("call_1".into()),
    };
    let ir = message_to_ir(&msg);
    assert_eq!(ir.role, IrRole::Tool);
    assert_eq!(ir.content.len(), 1);
    match &ir.content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "call_1");
            assert!(!is_error);
            assert_eq!(content.len(), 1);
            assert!(matches!(&content[0], IrContentBlock::Text { text } if text == "72 degrees"));
        }
        _ => panic!("expected ToolResult block"),
    }
}

#[test]
fn tool_result_message_from_ir() {
    let ir = IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "call_1".into(),
            content: vec![IrContentBlock::Text {
                text: "result text".into(),
            }],
            is_error: false,
        }],
    );
    let msg = message_from_ir(&ir);
    assert_eq!(msg.role, ChatMessageRole::Tool);
    assert_eq!(msg.tool_call_id, Some("call_1".into()));
    assert_eq!(msg.content, Some("result text".into()));
}

#[test]
fn system_message_round_trip_through_ir() {
    let msg = ChatMessage {
        role: ChatMessageRole::System,
        content: Some("You are a helpful assistant.".into()),
        tool_calls: None,
        tool_call_id: None,
    };
    let ir = message_to_ir(&msg);
    let back = message_from_ir(&ir);
    assert_eq!(back.role, ChatMessageRole::System);
    assert_eq!(back.content, Some("You are a helpful assistant.".into()));
}

// ── Conversation mapping ───────────────────────────────────────────

#[test]
fn conversation_to_ir_and_back() {
    let messages = vec![
        ChatMessage {
            role: ChatMessageRole::System,
            content: Some("Be helpful.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: ChatMessageRole::User,
            content: Some("Hi".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: ChatMessageRole::Assistant,
            content: Some("Hello!".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    let conv = conversation_to_ir(&messages);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);

    let back = conversation_from_ir(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, ChatMessageRole::System);
    assert_eq!(back[1].content, Some("Hi".into()));
    assert_eq!(back[2].content, Some("Hello!".into()));
}

#[test]
fn conversation_empty() {
    let conv = conversation_to_ir(&[]);
    assert!(conv.is_empty());
    let back = conversation_from_ir(&conv);
    assert!(back.is_empty());
}

// ── Tool definition mapping ────────────────────────────────────────

#[test]
fn tool_def_round_trip_through_ir() {
    let tool = ToolDefinition {
        tool_type: "function".into(),
        function: FunctionDefinition {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        },
    };
    let ir = tool_def_to_ir(&tool);
    assert_eq!(ir.name, "read_file");
    assert_eq!(ir.description, "Read a file");

    let back = tool_def_from_ir(&ir);
    assert_eq!(back.tool_type, "function");
    assert_eq!(back.function.name, "read_file");
    assert_eq!(back.function.description, "Read a file");
    assert_eq!(back.function.parameters, tool.function.parameters);
}

#[test]
fn tool_def_from_ir_always_function_type() {
    let ir = IrToolDefinition {
        name: "test".into(),
        description: "test tool".into(),
        parameters: serde_json::json!({}),
    };
    let td = tool_def_from_ir(&ir);
    assert_eq!(td.tool_type, "function");
}

// ── Usage mapping ──────────────────────────────────────────────────

#[test]
fn usage_to_ir_basic() {
    let u = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let ir = usage_to_ir(&u);
    assert_eq!(ir.input_tokens, 100);
    assert_eq!(ir.output_tokens, 50);
    assert_eq!(ir.total_tokens, 150);
    assert_eq!(ir.cache_read_tokens, 0);
    assert_eq!(ir.cache_write_tokens, 0);
}

#[test]
fn usage_from_ir_basic() {
    let ir = IrUsage::from_io(100, 50);
    let u = usage_from_ir(&ir);
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

#[test]
fn usage_round_trip_through_ir() {
    let original = Usage {
        prompt_tokens: 42,
        completion_tokens: 18,
        total_tokens: 60,
    };
    let ir = usage_to_ir(&original);
    let back = usage_from_ir(&ir);
    assert_eq!(back, original);
}

#[test]
fn usage_zero() {
    let u = Usage::default();
    let ir = usage_to_ir(&u);
    assert_eq!(ir, IrUsage::default());
    let back = usage_from_ir(&ir);
    assert_eq!(back, u);
}

#[test]
fn merge_usage_basic() {
    let a = Usage {
        prompt_tokens: 10,
        completion_tokens: 20,
        total_tokens: 30,
    };
    let b = Usage {
        prompt_tokens: 5,
        completion_tokens: 15,
        total_tokens: 20,
    };
    let merged = merge_usage(&a, &b);
    assert_eq!(merged.prompt_tokens, 15);
    assert_eq!(merged.completion_tokens, 35);
    assert_eq!(merged.total_tokens, 50);
}

// ── Response mapping ───────────────────────────────────────────────

#[test]
fn response_content_to_ir_text() {
    let resp = ChatCompletionResponse {
        id: "id".into(),
        object: "chat.completion".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![ChatCompletionChoice {
            index: 0,
            message: ChatMessage {
                role: ChatMessageRole::Assistant,
                content: Some("Hello world".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(Usage {
            prompt_tokens: 5,
            completion_tokens: 2,
            total_tokens: 7,
        }),
    };
    let blocks = response_content_to_ir(&resp);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(&blocks[0], IrContentBlock::Text { text } if text == "Hello world"));
}

#[test]
fn response_to_ir_message_with_tool_calls() {
    let resp = ChatCompletionResponse {
        id: "id".into(),
        object: "chat.completion".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![ChatCompletionChoice {
            index: 0,
            message: ChatMessage {
                role: ChatMessageRole::Assistant,
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_x".into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: "search".into(),
                        arguments: r#"{"q":"hello"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
    };
    let ir_msg = response_to_ir_message(&resp);
    assert_eq!(ir_msg.role, IrRole::Assistant);
    assert_eq!(ir_msg.content.len(), 1);
    assert!(matches!(&ir_msg.content[0], IrContentBlock::ToolUse { name, .. } if name == "search"));
}

#[test]
fn extract_usage_present() {
    let resp = ChatCompletionResponse {
        id: "id".into(),
        object: "chat.completion".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    };
    let ir = extract_usage(&resp);
    assert_eq!(ir.input_tokens, 10);
    assert_eq!(ir.output_tokens, 5);
}

#[test]
fn extract_usage_absent() {
    let resp = ChatCompletionResponse {
        id: "id".into(),
        object: "chat.completion".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
    let ir = extract_usage(&resp);
    assert_eq!(ir, IrUsage::default());
}

// ── Stream accumulator ─────────────────────────────────────────────

#[test]
fn stream_accumulator_text_deltas() {
    let mut acc = StreamAccumulator::new();

    let chunk1 = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: Some("assistant".into()),
                content: Some("Hello".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
    };

    let chunk2 = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: None,
                content: Some(" world".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
    };

    let chunk3 = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta::default(),
            finish_reason: Some("stop".into()),
        }],
    };

    let frag1 = acc.feed(&chunk1);
    assert_eq!(frag1, Some(StreamFragment::TextDelta("Hello".into())));

    let frag2 = acc.feed(&chunk2);
    assert_eq!(frag2, Some(StreamFragment::TextDelta(" world".into())));

    let frag3 = acc.feed(&chunk3);
    assert!(frag3.is_none());

    assert_eq!(acc.finish_reason, Some("stop".into()));
    assert_eq!(acc.model, Some("gpt-4o".into()));

    let (blocks, _usage) = acc.finish();
    assert_eq!(blocks.len(), 1);
    assert!(matches!(&blocks[0], IrContentBlock::Text { text } if text == "Hello world"));
}

#[test]
fn stream_accumulator_tool_calls() {
    let mut acc = StreamAccumulator::new();

    // First chunk: start tool call
    let chunk1 = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: None,
                content: None,
                tool_calls: Some(vec![StreamToolCall {
                    index: 0,
                    id: Some("call_1".into()),
                    call_type: Some("function".into()),
                    function: Some(StreamFunctionCall {
                        name: Some("get_weather".into()),
                        arguments: Some(r#"{"ci"#.into()),
                    }),
                }]),
            },
            finish_reason: None,
        }],
    };

    // Second chunk: continue arguments
    let chunk2 = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: None,
                content: None,
                tool_calls: Some(vec![StreamToolCall {
                    index: 0,
                    id: None,
                    call_type: None,
                    function: Some(StreamFunctionCall {
                        name: None,
                        arguments: Some(r#"ty":"NYC"}"#.into()),
                    }),
                }]),
            },
            finish_reason: None,
        }],
    };

    acc.feed(&chunk1);
    acc.feed(&chunk2);

    let (blocks, _usage) = acc.finish();
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "get_weather");
            assert_eq!(*input, serde_json::json!({"city": "NYC"}));
        }
        _ => panic!("expected ToolUse block"),
    }
}

#[test]
fn stream_accumulator_empty() {
    let acc = StreamAccumulator::new();
    let (blocks, usage) = acc.finish();
    assert!(blocks.is_empty());
    assert_eq!(usage, IrUsage::default());
}

// ── task_to_request ────────────────────────────────────────────────

#[test]
fn task_to_request_basic() {
    let req = task_to_request("Fix the bug", "gpt-4o", 4096);
    assert_eq!(req.model, "gpt-4o");
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, ChatMessageRole::User);
    assert_eq!(req.messages[0].content, Some("Fix the bug".into()));
    assert_eq!(req.max_tokens, Some(4096));
    assert_eq!(req.stream, Some(true));
}

#[test]
fn task_to_request_empty_task() {
    let req = task_to_request("", "gpt-4o-mini", 100);
    assert_eq!(req.messages[0].content, Some("".into()));
    assert_eq!(req.model, "gpt-4o-mini");
}

// ── Error translation ──────────────────────────────────────────────

#[test]
fn api_error_auth_to_bridge() {
    let err = ApiError {
        error: ApiErrorDetail {
            message: "Invalid API key".into(),
            error_type: "authentication_error".into(),
            param: None,
            code: None,
        },
    };
    let bridge = api_error_to_bridge(&err);
    assert!(bridge.to_string().contains("authentication failed"));
}

#[test]
fn api_error_invalid_request_to_bridge() {
    let err = ApiError {
        error: ApiErrorDetail {
            message: "Bad model".into(),
            error_type: "invalid_request_error".into(),
            param: Some("model".into()),
            code: None,
        },
    };
    let bridge = api_error_to_bridge(&err);
    assert!(bridge.to_string().contains("invalid request"));
}

#[test]
fn api_error_rate_limit_to_bridge() {
    let err = ApiError {
        error: ApiErrorDetail {
            message: "Too many requests".into(),
            error_type: "rate_limit_error".into(),
            param: None,
            code: Some("rate_limit".into()),
        },
    };
    let bridge = api_error_to_bridge(&err);
    assert!(bridge.to_string().contains("rate limited"));
}

#[test]
fn api_error_server_to_bridge() {
    let err = ApiError {
        error: ApiErrorDetail {
            message: "Internal error".into(),
            error_type: "server_error".into(),
            param: None,
            code: None,
        },
    };
    let bridge = api_error_to_bridge(&err);
    assert!(bridge.to_string().contains("API server error"));
}

#[test]
fn api_error_unknown_type_to_bridge() {
    let err = ApiError {
        error: ApiErrorDetail {
            message: "Something weird".into(),
            error_type: "unknown_error".into(),
            param: None,
            code: None,
        },
    };
    let bridge = api_error_to_bridge(&err);
    assert!(bridge.to_string().contains("unknown_error"));
}

// ── Thinking / Image blocks from IR ────────────────────────────────

#[test]
fn thinking_block_from_ir_renders_as_text() {
    let ir = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "Let me think...".into(),
        }],
    );
    let msg = message_from_ir(&ir);
    assert_eq!(msg.content, Some("[thinking: Let me think...]".into()));
}

#[test]
fn image_block_from_ir_renders_as_text() {
    let ir = IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "abc123".into(),
        }],
    );
    let msg = message_from_ir(&ir);
    assert!(msg.content.unwrap().contains("[image: image/png"));
}

// ── Mixed content ──────────────────────────────────────────────────

#[test]
fn assistant_text_and_tool_calls_from_ir() {
    let ir = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me check. ".into(),
            },
            IrContentBlock::ToolUse {
                id: "call_x".into(),
                name: "search".into(),
                input: serde_json::json!({"q": "test"}),
            },
        ],
    );
    let msg = message_from_ir(&ir);
    assert_eq!(msg.role, ChatMessageRole::Assistant);
    assert_eq!(msg.content, Some("Let me check. ".into()));
    assert_eq!(msg.tool_calls.as_ref().unwrap().len(), 1);
}

#[test]
fn empty_content_message_to_ir() {
    let msg = ChatMessage {
        role: ChatMessageRole::Assistant,
        content: Some("".into()),
        tool_calls: None,
        tool_call_id: None,
    };
    let ir = message_to_ir(&msg);
    assert!(ir.content.is_empty()); // empty strings are skipped
}

#[test]
fn invalid_json_arguments_to_ir() {
    let msg = ChatMessage {
        role: ChatMessageRole::Assistant,
        content: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_bad".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "broken".into(),
                arguments: "not valid json".into(),
            },
        }]),
        tool_call_id: None,
    };
    let ir = message_to_ir(&msg);
    match &ir.content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(*input, serde_json::Value::Null);
        }
        _ => panic!("expected ToolUse"),
    }
}
