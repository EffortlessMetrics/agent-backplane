// SPDX-License-Identifier: MIT OR Apache-2.0
//! Vendor-compatibility tests for the OpenAI shim.
//!
//! Validates that the public surface exactly mirrors the OpenAI SDK's type
//! names and JSON wire format, including the vendor-compat type aliases
//! (`ChatCompletion`, `ChatCompletionChunk`, `ChatCompletionCreateParams`).

use abp_shim_openai::types::{
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ErrorResponse, FunctionDef,
    MessageContent, StreamChunk, Tool, ToolChoice, ToolChoiceMode,
};
use abp_shim_openai::{
    ChatCompletion, ChatCompletionChunk, ChatCompletionCreateParams, Message, OpenAiClient,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Vendor-compat type alias existence and shape
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn chat_completion_alias_matches_response() {
    let resp = abp_shim_openai::ChatCompletionResponse {
        id: "chatcmpl-abc".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
    let _alias: ChatCompletion = resp;
}

#[test]
fn chat_completion_chunk_alias_exists() {
    let _chunk: ChatCompletionChunk = abp_shim_openai::StreamEvent {
        id: "chatcmpl-abc".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
}

#[test]
fn chat_completion_create_params_alias_matches_request() {
    let req: ChatCompletionCreateParams = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("Hello")])
        .build();
    assert_eq!(req.model, "gpt-4o");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Client::chat().completions().create() chain pattern
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn client_chat_completions_create_chain() {
    use abp_core::{AgentEvent, AgentEventKind};
    use chrono::Utc;

    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hi there!".into(),
        },
        ext: None,
    }];
    let receipt = abp_shim_openai::mock_receipt(events);

    let client = OpenAiClient::new("gpt-4o").with_processor(Box::new(move |_| receipt.clone()));
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("Hello")])
        .build();

    let resp = client.chat().completions().create(req).await.unwrap();
    assert_eq!(resp.object, "chat.completion");
    assert!(!resp.choices.is_empty());
}

#[tokio::test]
async fn client_chat_completions_create_stream_chain() {
    use abp_core::{AgentEvent, AgentEventKind};
    use chrono::Utc;
    use tokio_stream::StreamExt;

    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        },
        ext: None,
    }];
    let receipt = abp_shim_openai::mock_receipt(events);

    let client = OpenAiClient::new("gpt-4o").with_processor(Box::new(move |_| receipt.clone()));
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("Hi")])
        .stream(true)
        .build();

    let stream = client
        .chat()
        .completions()
        .create_stream(req)
        .await
        .unwrap();
    let chunks: Vec<_> = stream.collect().await;
    assert!(chunks.len() >= 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Wire-format JSON fidelity — request
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_json_matches_openai_wire_format() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![
            ChatMessage::System {
                content: "You are helpful.".into(),
            },
            ChatMessage::User {
                content: MessageContent::Text("Hello".into()),
            },
        ],
        temperature: Some(0.7),
        top_p: None,
        max_tokens: Some(1024),
        stream: None,
        tools: None,
        tool_choice: None,
    };

    let v: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(v["model"], "gpt-4o");
    assert_eq!(v["messages"][0]["role"], "system");
    assert_eq!(v["messages"][1]["role"], "user");
    assert_eq!(v["temperature"], 0.7);
    assert_eq!(v["max_tokens"], 1024);
    // stream should be omitted when None
    assert!(v.get("stream").is_none());
}

#[test]
fn request_with_tool_choice_auto_json() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![ChatMessage::User {
            content: MessageContent::Text("test".into()),
        }],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: Some(vec![Tool {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "get_weather".into(),
                description: "Get weather".into(),
                parameters: json!({"type": "object"}),
            },
        }]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
    };

    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["tool_choice"], "auto");
    assert_eq!(v["tools"][0]["type"], "function");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Wire-format JSON fidelity — response
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_deserialized_from_real_openai_json() {
    let json_str = r#"{
        "id": "chatcmpl-abc123",
        "object": "chat.completion",
        "created": 1700000000,
        "model": "gpt-4o-2024-08-06",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello! How can I help?",
                "tool_calls": null
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 12,
            "completion_tokens": 8,
            "total_tokens": 20
        }
    }"#;

    let resp: ChatCompletionResponse = serde_json::from_str(json_str).unwrap();
    assert_eq!(resp.id, "chatcmpl-abc123");
    assert_eq!(resp.model, "gpt-4o-2024-08-06");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello! How can I help?")
    );
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    assert_eq!(resp.usage.as_ref().unwrap().total_tokens, 20);
}

#[test]
fn response_with_tool_calls_from_json() {
    let json_str = r#"{
        "id": "chatcmpl-tc123",
        "object": "chat.completion",
        "created": 1700000000,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_abc",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"location\":\"NYC\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": null
    }"#;

    let resp: ChatCompletionResponse = serde_json::from_str(json_str).unwrap();
    assert!(resp.choices[0].message.content.is_none());
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].id, "call_abc");
    assert_eq!(tcs[0].function.name, "get_weather");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Streaming chunk wire format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_chunk_from_real_openai_sse() {
    let json_str = r#"{
        "id": "chatcmpl-abc",
        "object": "chat.completion.chunk",
        "created": 1700000000,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "delta": {"role": "assistant", "content": "Hi"},
            "finish_reason": null
        }]
    }"#;

    let chunk: StreamChunk = serde_json::from_str(json_str).unwrap();
    assert_eq!(chunk.object, "chat.completion.chunk");
    assert_eq!(chunk.choices[0].delta.role.as_deref(), Some("assistant"));
    assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hi"));
    assert!(chunk.choices[0].finish_reason.is_none());
}

#[test]
fn stream_chunk_tool_call_fragment() {
    let json_str = r#"{
        "id": "chatcmpl-tc",
        "object": "chat.completion.chunk",
        "created": 1700000000,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_xyz",
                    "type": "function",
                    "function": {"name": "read_file", "arguments": "{\"p"}
                }]
            },
            "finish_reason": null
        }]
    }"#;

    let chunk: StreamChunk = serde_json::from_str(json_str).unwrap();
    let tc = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id.as_deref(), Some("call_xyz"));
    assert_eq!(
        tc.function.as_ref().unwrap().name.as_deref(),
        Some("read_file")
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Error response wire format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_response_from_real_openai_json() {
    let json_str = r#"{
        "error": {
            "message": "Incorrect API key provided: sk-proj-****1234.",
            "type": "invalid_request_error",
            "param": null,
            "code": "invalid_api_key"
        }
    }"#;

    let err: ErrorResponse = serde_json::from_str(json_str).unwrap();
    assert_eq!(err.error.error_type, "invalid_request_error");
    assert_eq!(err.error.code.as_deref(), Some("invalid_api_key"));
    assert!(err.error.message.contains("Incorrect API key"));
}

#[test]
fn error_response_roundtrip() {
    let err = ErrorResponse::rate_limit("You have exceeded your rate limit");
    let json = serde_json::to_string(&err).unwrap();
    let back: ErrorResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.error.error_type, "rate_limit_error");
}
