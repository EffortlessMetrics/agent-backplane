#![allow(clippy::all)]
#![allow(clippy::needless_update)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for openai-bridge types, translation, and edge cases.

use openai_bridge::openai_types::*;

// ── Serde round-trip tests ─────────────────────────────────────────

#[test]
fn chat_message_role_round_trip() {
    for role in [
        ChatMessageRole::System,
        ChatMessageRole::User,
        ChatMessageRole::Assistant,
        ChatMessageRole::Tool,
    ] {
        let json = serde_json::to_string(&role).unwrap();
        let back: ChatMessageRole = serde_json::from_str(&json).unwrap();
        assert_eq!(role, back);
    }
}

#[test]
fn chat_message_system_round_trip() {
    let msg = ChatMessage {
        role: ChatMessageRole::System,
        content: Some("You are helpful.".into()),
        tool_calls: None,
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn chat_message_user_round_trip() {
    let msg = ChatMessage {
        role: ChatMessageRole::User,
        content: Some("Hello!".into()),
        tool_calls: None,
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn chat_message_assistant_with_tool_calls_round_trip() {
    let msg = ChatMessage {
        role: ChatMessageRole::Assistant,
        content: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_123".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "get_weather".into(),
                arguments: r#"{"location":"NYC"}"#.into(),
            },
        }]),
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn chat_message_tool_result_round_trip() {
    let msg = ChatMessage {
        role: ChatMessageRole::Tool,
        content: Some("72°F and sunny".into()),
        tool_calls: None,
        tool_call_id: Some("call_123".into()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn request_minimal_round_trip() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![ChatMessage {
            role: ChatMessageRole::User,
            content: Some("Hi".into()),
            tool_calls: None,
            tool_call_id: None,
        }],
        tools: None,
        temperature: None,
        max_tokens: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        tool_choice: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn request_full_round_trip() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![
            ChatMessage {
                role: ChatMessageRole::System,
                content: Some("You are a helpful assistant.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: ChatMessageRole::User,
                content: Some("What's the weather?".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        tools: Some(vec![ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDefinition {
                name: "get_weather".into(),
                description: "Get weather for a location".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    }
                }),
            },
        }]),
        temperature: Some(0.7),
        max_tokens: Some(1024),
        stream: Some(true),
        top_p: Some(0.9),
        frequency_penalty: Some(0.5),
        presence_penalty: Some(0.3),
        stop: Some(vec!["END".into()]),
        n: Some(1),
        tool_choice: Some(serde_json::json!("auto")),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn response_round_trip() {
    let resp = ChatCompletionResponse {
        id: "chatcmpl-abc123".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![ChatCompletionChoice {
            index: 0,
            message: ChatMessage {
                role: ChatMessageRole::Assistant,
                content: Some("Hello! How can I help?".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn response_with_tool_calls_round_trip() {
    let resp = ChatCompletionResponse {
        id: "chatcmpl-tool".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![ChatCompletionChoice {
            index: 0,
            message: ChatMessage {
                role: ChatMessageRole::Assistant,
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_456".into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: "search".into(),
                        arguments: r#"{"query":"test"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: Some(Usage {
            prompt_tokens: 15,
            completion_tokens: 10,
            total_tokens: 25,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn chunk_round_trip() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-stream1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
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
    let json = serde_json::to_string(&chunk).unwrap();
    let back: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, back);
}

#[test]
fn chunk_with_tool_call_delta_round_trip() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-tc".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: None,
                content: None,
                tool_calls: Some(vec![StreamToolCall {
                    index: 0,
                    id: Some("call_789".into()),
                    call_type: Some("function".into()),
                    function: Some(StreamFunctionCall {
                        name: Some("get_weather".into()),
                        arguments: Some(r#"{"loc"#.into()),
                    }),
                }]),
            },
            finish_reason: None,
        }],
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, back);
}

#[test]
fn chunk_finish_reason_round_trip() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-done".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta::default(),
            finish_reason: Some("stop".into()),
        }],
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, back);
}

#[test]
fn usage_default() {
    let u = Usage::default();
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.completion_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn api_error_round_trip() {
    let err = ApiError {
        error: ApiErrorDetail {
            message: "Rate limit exceeded".into(),
            error_type: "rate_limit_error".into(),
            param: None,
            code: Some("rate_limit".into()),
        },
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn tool_definition_round_trip() {
    let tool = ToolDefinition {
        tool_type: "function".into(),
        function: FunctionDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: ToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

// ── skip_serializing_if tests ──────────────────────────────────────

#[test]
fn request_skips_none_fields() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        tools: None,
        temperature: None,
        max_tokens: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        tool_choice: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("temperature"));
    assert!(!json.contains("max_tokens"));
    assert!(!json.contains("stream"));
    assert!(!json.contains("tools"));
    assert!(!json.contains("stop"));
    assert!(!json.contains("frequency_penalty"));
    assert!(!json.contains("presence_penalty"));
}

#[test]
fn message_skips_none_fields() {
    let msg = ChatMessage {
        role: ChatMessageRole::User,
        content: Some("Hi".into()),
        tool_calls: None,
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains("tool_calls"));
    assert!(!json.contains("tool_call_id"));
}

#[test]
fn stream_delta_skips_none_fields() {
    let delta = StreamDelta::default();
    let json = serde_json::to_string(&delta).unwrap();
    assert_eq!(json, "{}");
}

// ── Edge cases ─────────────────────────────────────────────────────

#[test]
fn empty_messages_request() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        tools: None,
        temperature: None,
        max_tokens: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        tool_choice: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.messages.len(), 0);
}

#[test]
fn response_no_usage() {
    let resp = ChatCompletionResponse {
        id: "id".into(),
        object: "chat.completion".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(!json.contains("usage"));
    let back: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert!(back.usage.is_none());
}

#[test]
fn multiple_tool_calls_in_single_message() {
    let msg = ChatMessage {
        role: ChatMessageRole::Assistant,
        content: None,
        tool_calls: Some(vec![
            ToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"a.txt"}"#.into(),
                },
            },
            ToolCall {
                id: "call_2".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"b.txt"}"#.into(),
                },
            },
        ]),
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.tool_calls.as_ref().unwrap().len(), 2);
}

#[test]
fn assistant_message_with_content_and_tool_calls() {
    let msg = ChatMessage {
        role: ChatMessageRole::Assistant,
        content: Some("I'll check the weather for you.".into()),
        tool_calls: Some(vec![ToolCall {
            id: "call_w".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "get_weather".into(),
                arguments: r#"{"city":"Paris"}"#.into(),
            },
        }]),
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert!(back.content.is_some());
    assert!(back.tool_calls.is_some());
}
