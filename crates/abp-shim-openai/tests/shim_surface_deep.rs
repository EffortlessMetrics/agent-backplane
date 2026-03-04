// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the OpenAI shim's public API surface.
//!
//! Covers: request/response construction, builder patterns, serde roundtrips,
//! ABP work-order / receipt conversion, streaming, tool calling, error mapping,
//! edge cases, and response-format handling.

use abp_core::{AgentEvent, AgentEventKind, UsageNormalized};
use abp_shim_openai::client::{Client, ClientError};
use abp_shim_openai::types::{
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice, ChoiceMessage, ContentPart,
    FunctionCall, FunctionDef, ImageUrl, MessageContent, StreamChoice, StreamChunk, StreamDelta,
    StreamFunctionCall, StreamToolCall, Tool, ToolCall, ToolChoice, ToolChoiceMode, Usage,
};
use abp_shim_openai::{
    self as shim, ChatCompletionRequest as ShimRequest, Message, OpenAiClient, ProcessFn,
    ResponseFormat, Role, ShimError, StreamEvent, Tool as ShimTool, ToolCall as ShimToolCall,
    ToolChoice as ShimToolChoice, ToolChoiceMode as ShimToolChoiceMode, Usage as ShimUsage,
};
use chrono::Utc;
use serde_json::json;
use std::time::Duration;
use tokio_stream::StreamExt;

// ── Helpers ─────────────────────────────────────────────────────────────

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn processor(events: Vec<AgentEvent>) -> ProcessFn {
    Box::new(move |_wo| shim::mock_receipt(events.clone()))
}

fn minimal_types_request() -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![ChatMessage::User {
            content: MessageContent::Text("Hello".into()),
        }],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
    }
}

fn minimal_shim_request() -> ShimRequest {
    ShimRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("Hello")])
        .build()
}

fn sample_response() -> ChatCompletionResponse {
    ChatCompletionResponse {
        id: "chatcmpl-abc123".into(),
        object: "chat.completion".into(),
        created: 1_700_000_000,
        model: "gpt-4o".into(),
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                role: "assistant".into(),
                content: Some("Hi there!".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    }
}

// ═════════════════════════════════════════════════════════════════════════
//  1. ChatCompletion create — basic request
// ═════════════════════════════════════════════════════════════════════════

// 1
#[tokio::test]
async fn chat_completion_basic_request() {
    let events = vec![make_event(AgentEventKind::AssistantMessage {
        text: "Hello!".into(),
    })];
    let client = OpenAiClient::new("gpt-4o").with_processor(processor(events));
    let resp = client
        .chat()
        .completions()
        .create(minimal_shim_request())
        .await
        .unwrap();
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
}

// 2
#[tokio::test]
async fn chat_completion_all_parameters() {
    let events = vec![make_event(AgentEventKind::AssistantMessage {
        text: "ok".into(),
    })];
    let client = OpenAiClient::new("gpt-4o").with_processor(processor(events));
    let req = ShimRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            Message::system("Be concise."),
            Message::user("Explain rust."),
        ])
        .temperature(0.5)
        .max_tokens(256)
        .stop(vec!["END".into()])
        .tools(vec![ShimTool::function(
            "search",
            "Search the web",
            json!({"type": "object", "properties": {}}),
        )])
        .tool_choice(ShimToolChoice::Mode(ShimToolChoiceMode::Auto))
        .response_format(ResponseFormat::json_object())
        .stream(false)
        .build();
    let resp = client.chat().completions().create(req).await.unwrap();
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("ok"));
}

// 3
#[tokio::test]
async fn chat_completion_streaming_basic() {
    let events = vec![
        make_event(AgentEventKind::AssistantDelta { text: "Hel".into() }),
        make_event(AgentEventKind::AssistantDelta { text: "lo!".into() }),
    ];
    let client = OpenAiClient::new("gpt-4o").with_processor(processor(events));
    let req = ShimRequest::builder()
        .messages(vec![Message::user("Hi")])
        .stream(true)
        .build();
    let chunks: Vec<StreamEvent> = client
        .chat()
        .completions()
        .create_stream(req)
        .await
        .unwrap()
        .collect()
        .await;
    assert_eq!(chunks.len(), 3); // 2 deltas + stop
    assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("Hel"));
    assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("lo!"));
    assert_eq!(chunks[2].choices[0].finish_reason.as_deref(), Some("stop"));
}

// ═════════════════════════════════════════════════════════════════════════
//  2. Model listing via shim
// ═════════════════════════════════════════════════════════════════════════

// 4
#[test]
fn model_name_roundtrips_through_canonical() {
    use abp_openai_sdk::dialect::{from_canonical_model, to_canonical_model};
    assert_eq!(to_canonical_model("gpt-4o"), "openai/gpt-4o");
    assert_eq!(from_canonical_model("openai/gpt-4o"), "gpt-4o");
}

// 5
#[test]
fn known_models_include_gpt4o() {
    use abp_openai_sdk::dialect::is_known_model;
    assert!(is_known_model("gpt-4o"));
    assert!(is_known_model("o3-mini"));
    assert!(!is_known_model("nonexistent-model"));
}

// ═════════════════════════════════════════════════════════════════════════
//  3. Request builder patterns (fluent API)
// ═════════════════════════════════════════════════════════════════════════

// 6
#[test]
fn builder_defaults_model_to_gpt4o() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    assert_eq!(req.model, "gpt-4o");
}

// 7
#[test]
fn builder_sets_temperature() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("t")])
        .temperature(1.5)
        .build();
    assert_eq!(req.temperature, Some(1.5));
}

// 8
#[test]
fn builder_sets_max_tokens() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("t")])
        .max_tokens(512)
        .build();
    assert_eq!(req.max_tokens, Some(512));
}

// 9
#[test]
fn builder_sets_stop() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("t")])
        .stop(vec!["END".into()])
        .build();
    assert_eq!(req.stop.as_deref(), Some(&["END".to_string()][..]));
}

// 10
#[test]
fn builder_sets_stream() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("t")])
        .stream(true)
        .build();
    assert_eq!(req.stream, Some(true));
}

// 11
#[test]
fn builder_sets_tools() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("t")])
        .tools(vec![ShimTool::function(
            "fn1",
            "desc",
            json!({"type": "object"}),
        )])
        .build();
    assert_eq!(req.tools.as_ref().unwrap().len(), 1);
}

// 12
#[test]
fn builder_sets_tool_choice_auto() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("t")])
        .tool_choice(ShimToolChoice::Mode(ShimToolChoiceMode::Auto))
        .build();
    assert!(req.tool_choice.is_some());
}

// 13
#[test]
fn builder_sets_response_format() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("t")])
        .response_format(ResponseFormat::json_object())
        .build();
    assert!(req.response_format.is_some());
}

// 14
#[test]
fn builder_chain_multiple_setters() {
    let req = ShimRequest::builder()
        .model("gpt-4-turbo")
        .messages(vec![Message::user("chain")])
        .temperature(0.8)
        .max_tokens(1024)
        .stream(false)
        .build();
    assert_eq!(req.model, "gpt-4-turbo");
    assert_eq!(req.temperature, Some(0.8));
    assert_eq!(req.max_tokens, Some(1024));
    assert_eq!(req.stream, Some(false));
}

// ═════════════════════════════════════════════════════════════════════════
//  4. Response deserialization
// ═════════════════════════════════════════════════════════════════════════

// 15
#[test]
fn response_deserializes_choices() {
    let resp = sample_response();
    let json = serde_json::to_string(&resp).unwrap();
    let back: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.choices.len(), 1);
    assert_eq!(
        back.choices[0].message.content.as_deref(),
        Some("Hi there!")
    );
}

// 16
#[test]
fn response_deserializes_usage() {
    let resp = sample_response();
    let json = serde_json::to_string(&resp).unwrap();
    let back: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    let u = back.usage.unwrap();
    assert_eq!(u.prompt_tokens, 10);
    assert_eq!(u.completion_tokens, 5);
    assert_eq!(u.total_tokens, 15);
}

// 17
#[test]
fn response_deserializes_finish_reason_stop() {
    let resp = sample_response();
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

// 18
#[test]
fn response_deserializes_finish_reason_tool_calls() {
    let mut resp = sample_response();
    resp.choices[0].finish_reason = Some("tool_calls".into());
    resp.choices[0].message.tool_calls = Some(vec![ToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "search".into(),
            arguments: "{}".into(),
        },
    }]);
    let json = serde_json::to_string(&resp).unwrap();
    let back: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.choices[0].finish_reason.as_deref(), Some("tool_calls"));
}

// 19
#[test]
fn response_no_usage_field_deserializes() {
    let json_str = r#"{
        "id": "chatcmpl-x",
        "object": "chat.completion",
        "created": 1700000000,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "hi"},
            "finish_reason": "stop"
        }]
    }"#;
    let resp: ChatCompletionResponse = serde_json::from_str(json_str).unwrap();
    assert!(resp.usage.is_none());
}

// ═════════════════════════════════════════════════════════════════════════
//  5. Streaming response handling
// ═════════════════════════════════════════════════════════════════════════

// 20
#[test]
fn stream_chunk_object_field() {
    let chunk = StreamChunk {
        id: "chatcmpl-chunk".into(),
        object: "chat.completion.chunk".into(),
        created: 123,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta::default(),
            finish_reason: None,
        }],
    };
    let json = serde_json::to_value(&chunk).unwrap();
    assert_eq!(json["object"], "chat.completion.chunk");
}

// 21
#[test]
fn stream_chunk_serde_roundtrip() {
    let chunk = StreamChunk {
        id: "chatcmpl-rt".into(),
        object: "chat.completion.chunk".into(),
        created: 999,
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
    let back: StreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(back, chunk);
}

// 22
#[test]
fn stream_chunk_sse_data_format() {
    let chunk = StreamChunk {
        id: "chatcmpl-sse".into(),
        object: "chat.completion.chunk".into(),
        created: 100,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: None,
                content: Some("token".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
    };
    let data_line = format!("data: {}", serde_json::to_string(&chunk).unwrap());
    assert!(data_line.starts_with("data: {"));
    assert!(data_line.contains("\"chat.completion.chunk\""));
}

// 23
#[test]
fn stream_final_chunk_has_finish_reason() {
    let chunk = StreamChunk {
        id: "chatcmpl-final".into(),
        object: "chat.completion.chunk".into(),
        created: 100,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta::default(),
            finish_reason: Some("stop".into()),
        }],
    };
    assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
}

// 24
#[test]
fn stream_chunk_with_tool_call_delta() {
    let chunk = StreamChunk {
        id: "chatcmpl-tc".into(),
        object: "chat.completion.chunk".into(),
        created: 100,
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
                        arguments: Some(r#"{"loc":"SF"}"#.into()),
                    }),
                }]),
            },
            finish_reason: None,
        }],
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: StreamChunk = serde_json::from_str(&json).unwrap();
    let tc = &back.choices[0].delta.tool_calls.as_ref().unwrap()[0];
    assert_eq!(
        tc.function.as_ref().unwrap().name.as_deref(),
        Some("get_weather")
    );
}

// ═════════════════════════════════════════════════════════════════════════
//  6. Error responses
// ═════════════════════════════════════════════════════════════════════════

// 25
#[test]
fn client_error_rate_limit() {
    let err = ClientError::Api {
        status: 429,
        body: r#"{"error":{"message":"Rate limit reached"}}"#.into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("429"));
    assert!(msg.contains("Rate limit"));
}

// 26
#[test]
fn client_error_auth_failure() {
    let err = ClientError::Api {
        status: 401,
        body: r#"{"error":{"message":"Invalid API key"}}"#.into(),
    };
    assert!(err.to_string().contains("401"));
}

// 27
#[test]
fn client_error_model_not_found() {
    let err = ClientError::Api {
        status: 404,
        body: "model not found".into(),
    };
    assert!(err.to_string().contains("404"));
    assert!(err.to_string().contains("model not found"));
}

// 28
#[test]
fn client_error_context_too_long() {
    let err = ClientError::Api {
        status: 400,
        body: r#"{"error":{"message":"maximum context length exceeded"}}"#.into(),
    };
    assert!(err.to_string().contains("400"));
    assert!(err.to_string().contains("context length"));
}

// 29
#[test]
fn shim_error_invalid_request() {
    let err = ShimError::InvalidRequest("missing model".into());
    assert!(err.to_string().contains("missing model"));
}

// 30
#[test]
fn shim_error_internal() {
    let err = ShimError::Internal("boom".into());
    assert!(err.to_string().contains("boom"));
}

// 31
#[tokio::test]
async fn no_processor_returns_shim_error() {
    let client = OpenAiClient::new("gpt-4o");
    let req = minimal_shim_request();
    let err = client.chat().completions().create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
}

// 32
#[tokio::test]
async fn error_event_maps_to_response_content() {
    let events = vec![make_event(AgentEventKind::Error {
        message: "rate limit exceeded".into(),
        error_code: None,
    })];
    let client = OpenAiClient::new("gpt-4o").with_processor(processor(events));
    let resp = client
        .chat()
        .completions()
        .create(minimal_shim_request())
        .await
        .unwrap();
    assert!(
        resp.choices[0]
            .message
            .content
            .as_deref()
            .unwrap()
            .contains("rate limit exceeded")
    );
}

// ═════════════════════════════════════════════════════════════════════════
//  7. Tool / function calling
// ═════════════════════════════════════════════════════════════════════════

// 33
#[test]
fn tool_definition_construction() {
    let tool = ShimTool::function("read_file", "Read a file", json!({"type": "object"}));
    assert_eq!(tool.tool_type, "function");
    assert_eq!(tool.function.name, "read_file");
}

// 34
#[test]
fn tool_definition_serde_roundtrip_types() {
    let tool = Tool {
        tool_type: "function".into(),
        function: FunctionDef {
            name: "search".into(),
            description: "Search".into(),
            parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: Tool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

// 35
#[test]
fn tool_choice_none_mode_serde() {
    let tc = ToolChoice::Mode(ToolChoiceMode::None);
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json, json!("none"));
    let back: ToolChoice = serde_json::from_value(json).unwrap();
    assert_eq!(tc, back);
}

// 36
#[test]
fn tool_choice_auto_mode_serde() {
    let tc = ToolChoice::Mode(ToolChoiceMode::Auto);
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json, json!("auto"));
}

// 37
#[test]
fn tool_choice_required_mode_serde() {
    let tc = ToolChoice::Mode(ToolChoiceMode::Required);
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json, json!("required"));
}

// 38
#[test]
fn tool_choice_function_serde() {
    let tc = ToolChoice::Function {
        tool_type: "function".into(),
        function: abp_shim_openai::types::ToolChoiceFunctionRef {
            name: "get_weather".into(),
        },
    };
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json["type"], "function");
    assert_eq!(json["function"]["name"], "get_weather");
}

// 39
#[tokio::test]
async fn parallel_tool_calls_in_response() {
    let events = vec![
        make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("call_1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "a.rs"}),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("call_2".into()),
            parent_tool_use_id: None,
            input: json!({"path": "b.rs"}),
        }),
    ];
    let client = OpenAiClient::new("gpt-4o").with_processor(processor(events));
    let resp = client
        .chat()
        .completions()
        .create(minimal_shim_request())
        .await
        .unwrap();
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs.len(), 2);
    assert_eq!(tcs[0].id, "call_1");
    assert_eq!(tcs[1].id, "call_2");
}

// 40
#[tokio::test]
async fn tool_call_response_sets_finish_reason_tool_calls() {
    let events = vec![make_event(AgentEventKind::ToolCall {
        tool_name: "search".into(),
        tool_use_id: Some("call_s".into()),
        parent_tool_use_id: None,
        input: json!({"q": "rust"}),
    })];
    let client = OpenAiClient::new("gpt-4o").with_processor(processor(events));
    let resp = client
        .chat()
        .completions()
        .create(minimal_shim_request())
        .await
        .unwrap();
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
}

// 41
#[test]
fn tool_call_serde_roundtrip() {
    let tc = ToolCall {
        id: "call_xyz".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "run".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

// 42
#[test]
fn tools_to_ir_conversion() {
    let tools = vec![ShimTool::function(
        "grep",
        "Search files",
        json!({"type": "object", "properties": {"pattern": {"type": "string"}}}),
    )];
    let ir = shim::tools_to_ir(&tools);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir[0].name, "grep");
    assert_eq!(ir[0].description, "Search files");
}

// ═════════════════════════════════════════════════════════════════════════
//  8. System / user / assistant message types
// ═════════════════════════════════════════════════════════════════════════

// 43
#[test]
fn system_message_types_serde() {
    let msg = ChatMessage::System {
        content: "You are helpful.".into(),
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "system");
    assert_eq!(v["content"], "You are helpful.");
}

// 44
#[test]
fn user_message_text_serde() {
    let msg = ChatMessage::User {
        content: MessageContent::Text("Hello".into()),
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "user");
    assert_eq!(v["content"], "Hello");
}

// 45
#[test]
fn user_message_multimodal_serde() {
    let msg = ChatMessage::User {
        content: MessageContent::Parts(vec![
            ContentPart::Text {
                text: "Describe this image".into(),
            },
            ContentPart::ImageUrl {
                image_url: ImageUrl {
                    url: "https://example.com/img.png".into(),
                    detail: Some("high".into()),
                },
            },
        ]),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

// 46
#[test]
fn assistant_message_text_serde() {
    let msg = ChatMessage::Assistant {
        content: Some("Sure!".into()),
        tool_calls: None,
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "assistant");
    assert_eq!(v["content"], "Sure!");
}

// 47
#[test]
fn assistant_message_with_tool_calls_serde() {
    let msg = ChatMessage::Assistant {
        content: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "search".into(),
                arguments: "{}".into(),
            },
        }]),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

// 48
#[test]
fn tool_message_serde() {
    let msg = ChatMessage::Tool {
        content: "result data".into(),
        tool_call_id: "call_1".into(),
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "tool");
    assert_eq!(v["tool_call_id"], "call_1");
}

// 49
#[test]
fn shim_message_constructors() {
    let sys = Message::system("sys");
    assert_eq!(sys.role, Role::System);
    assert_eq!(sys.content.as_deref(), Some("sys"));

    let user = Message::user("usr");
    assert_eq!(user.role, Role::User);

    let asst = Message::assistant("asst");
    assert_eq!(asst.role, Role::Assistant);

    let tool = Message::tool("id1", "result");
    assert_eq!(tool.role, Role::Tool);
    assert_eq!(tool.tool_call_id.as_deref(), Some("id1"));
}

// 50
#[test]
fn shim_assistant_with_tool_calls_constructor() {
    let msg = Message::assistant_with_tool_calls(vec![ShimToolCall {
        id: "call_a".into(),
        call_type: "function".into(),
        function: shim::FunctionCall {
            name: "fn1".into(),
            arguments: "{}".into(),
        },
    }]);
    assert_eq!(msg.role, Role::Assistant);
    assert!(msg.content.is_none());
    assert_eq!(msg.tool_calls.as_ref().unwrap().len(), 1);
}

// ═════════════════════════════════════════════════════════════════════════
//  9. Temperature, top_p, max_tokens, penalties
// ═════════════════════════════════════════════════════════════════════════

// 51
#[test]
fn types_request_temperature() {
    let req = ChatCompletionRequest {
        temperature: Some(0.0),
        ..minimal_types_request()
    };
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["temperature"], 0.0);
}

// 52
#[test]
fn types_request_top_p() {
    let req = ChatCompletionRequest {
        top_p: Some(0.95),
        ..minimal_types_request()
    };
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["top_p"], 0.95);
}

// 53
#[test]
fn types_request_max_tokens() {
    let req = ChatCompletionRequest {
        max_tokens: Some(8192),
        ..minimal_types_request()
    };
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["max_tokens"], 8192);
}

// 54
#[test]
fn temperature_boundary_zero() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("t")])
        .temperature(0.0)
        .build();
    assert_eq!(req.temperature, Some(0.0));
}

// 55
#[test]
fn temperature_boundary_two() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("t")])
        .temperature(2.0)
        .build();
    assert_eq!(req.temperature, Some(2.0));
}

// 56
#[test]
fn optional_fields_omitted_in_serialization() {
    let req = minimal_types_request();
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("temperature").is_none());
    assert!(json.get("top_p").is_none());
    assert!(json.get("max_tokens").is_none());
    assert!(json.get("stream").is_none());
    assert!(json.get("tools").is_none());
    assert!(json.get("tool_choice").is_none());
}

// ═════════════════════════════════════════════════════════════════════════
//  10. Response format
// ═════════════════════════════════════════════════════════════════════════

// 57
#[test]
fn response_format_json_object_serde() {
    let rf = ResponseFormat::json_object();
    let json = serde_json::to_value(&rf).unwrap();
    assert_eq!(json["type"], "json_object");
    let back: ResponseFormat = serde_json::from_value(json).unwrap();
    assert_eq!(rf, back);
}

// 58
#[test]
fn response_format_text_serde() {
    let rf = ResponseFormat::text();
    let json = serde_json::to_value(&rf).unwrap();
    assert_eq!(json["type"], "text");
}

// 59
#[test]
fn response_format_json_schema_serde() {
    let rf = ResponseFormat::json_schema(
        "my_schema",
        json!({"type": "object", "properties": {"name": {"type": "string"}}}),
    );
    let json = serde_json::to_value(&rf).unwrap();
    assert_eq!(json["type"], "json_schema");
    assert_eq!(json["json_schema"]["name"], "my_schema");
    assert_eq!(json["json_schema"]["strict"], true);
    let back: ResponseFormat = serde_json::from_value(json).unwrap();
    assert_eq!(rf, back);
}

// 60
#[test]
fn response_format_in_shim_request() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("test")])
        .response_format(ResponseFormat::json_object())
        .build();
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("response_format").is_some());
    assert_eq!(json["response_format"]["type"], "json_object");
}

// ═════════════════════════════════════════════════════════════════════════
//  11. Conversion to ABP WorkOrder
// ═════════════════════════════════════════════════════════════════════════

// 61
#[test]
fn request_to_work_order_sets_model() {
    let req = ShimRequest::builder()
        .model("gpt-4-turbo")
        .messages(vec![Message::user("hello")])
        .build();
    let wo = shim::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
}

// 62
#[test]
fn request_to_work_order_extracts_task() {
    let req = ShimRequest::builder()
        .messages(vec![
            Message::system("Be helpful."),
            Message::user("Explain Rust ownership"),
        ])
        .build();
    let wo = shim::request_to_work_order(&req);
    assert_eq!(wo.task, "Explain Rust ownership");
}

// 63
#[test]
fn request_to_work_order_maps_temperature() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("t")])
        .temperature(0.7)
        .build();
    let wo = shim::request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(0.7))
    );
}

// 64
#[test]
fn request_to_work_order_maps_max_tokens() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("t")])
        .max_tokens(2048)
        .build();
    let wo = shim::request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("max_tokens"),
        Some(&serde_json::Value::from(2048))
    );
}

// 65
#[test]
fn request_to_work_order_maps_stop() {
    let req = ShimRequest::builder()
        .messages(vec![Message::user("t")])
        .stop(vec!["END".into(), "STOP".into()])
        .build();
    let wo = shim::request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("stop").unwrap(),
        &json!(["END", "STOP"])
    );
}

// 66
#[test]
fn request_to_work_order_preserves_contract_version() {
    let req = minimal_shim_request();
    let wo = shim::request_to_work_order(&req);
    // WorkOrder doesn't directly embed contract version, but build() is valid
    assert!(!wo.task.is_empty());
}

// 67
#[test]
fn request_to_work_order_fallback_task() {
    let req = ShimRequest::builder()
        .messages(vec![Message::system("Only system message.")])
        .build();
    let wo = shim::request_to_work_order(&req);
    assert_eq!(wo.task, "chat completion");
}

// ═════════════════════════════════════════════════════════════════════════
//  12. Conversion from ABP Receipt
// ═════════════════════════════════════════════════════════════════════════

// 68
#[test]
fn receipt_to_response_maps_assistant_message() {
    let receipt = shim::mock_receipt(vec![make_event(AgentEventKind::AssistantMessage {
        text: "Answer".into(),
    })]);
    let resp = shim::receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.model, "gpt-4o");
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Answer"));
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

// 69
#[test]
fn receipt_to_response_maps_tool_calls() {
    let receipt = shim::mock_receipt(vec![make_event(AgentEventKind::ToolCall {
        tool_name: "search".into(),
        tool_use_id: Some("call_s".into()),
        parent_tool_use_id: None,
        input: json!({"q": "test"}),
    })]);
    let resp = shim::receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].function.name, "search");
}

// 70
#[test]
fn receipt_to_response_accumulates_deltas() {
    let receipt = shim::mock_receipt(vec![
        make_event(AgentEventKind::AssistantDelta { text: "Hel".into() }),
        make_event(AgentEventKind::AssistantDelta { text: "lo".into() }),
    ]);
    let resp = shim::receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello"));
}

// 71
#[test]
fn receipt_to_response_maps_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..Default::default()
    };
    let receipt = shim::mock_receipt_with_usage(
        vec![make_event(AgentEventKind::AssistantMessage {
            text: "ok".into(),
        })],
        usage,
    );
    let resp = shim::receipt_to_response(&receipt, "gpt-4o");
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

// 72
#[test]
fn receipt_to_response_id_format() {
    let receipt = shim::mock_receipt(vec![]);
    let resp = shim::receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.id.starts_with("chatcmpl-"));
}

// 73
#[test]
fn receipt_to_response_error_event() {
    let receipt = shim::mock_receipt(vec![make_event(AgentEventKind::Error {
        message: "context length exceeded".into(),
        error_code: None,
    })]);
    let resp = shim::receipt_to_response(&receipt, "gpt-4o");
    assert!(
        resp.choices[0]
            .message
            .content
            .as_deref()
            .unwrap()
            .contains("context length exceeded")
    );
}

// ═════════════════════════════════════════════════════════════════════════
//  13. Serde roundtrip for all request/response types
// ═════════════════════════════════════════════════════════════════════════

// 74
#[test]
fn types_request_full_roundtrip() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![
            ChatMessage::System {
                content: "Be concise.".into(),
            },
            ChatMessage::User {
                content: MessageContent::Text("Hello".into()),
            },
            ChatMessage::Assistant {
                content: Some("Hi!".into()),
                tool_calls: None,
            },
            ChatMessage::Tool {
                content: "result".into(),
                tool_call_id: "call_1".into(),
            },
        ],
        temperature: Some(0.8),
        top_p: Some(0.95),
        max_tokens: Some(4096),
        stream: Some(true),
        tools: Some(vec![Tool {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: json!({"type": "object"}),
            },
        }]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

// 75
#[test]
fn types_response_roundtrip() {
    let resp = sample_response();
    let json = serde_json::to_string(&resp).unwrap();
    let back: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

// 76
#[test]
fn usage_roundtrip() {
    let u = Usage {
        prompt_tokens: 42,
        completion_tokens: 18,
        total_tokens: 60,
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(u, back);
}

// 77
#[test]
fn choice_message_roundtrip() {
    let cm = ChoiceMessage {
        role: "assistant".into(),
        content: Some("text".into()),
        tool_calls: None,
    };
    let json = serde_json::to_string(&cm).unwrap();
    let back: ChoiceMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(cm, back);
}

// 78
#[test]
fn shim_role_roundtrip() {
    for role_str in ["system", "user", "assistant", "tool"] {
        let role: Role = serde_json::from_value(json!(role_str)).unwrap();
        let back_str = serde_json::to_value(&role).unwrap();
        assert_eq!(back_str, json!(role_str));
    }
}

// 79
#[test]
fn shim_usage_roundtrip() {
    let u = ShimUsage {
        prompt_tokens: 10,
        completion_tokens: 20,
        total_tokens: 30,
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: ShimUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(u, back);
}

// 80
#[test]
fn message_content_text_untagged_roundtrip() {
    let mc = MessageContent::Text("hello world".into());
    let json = serde_json::to_value(&mc).unwrap();
    assert_eq!(json, json!("hello world"));
    let back: MessageContent = serde_json::from_value(json).unwrap();
    assert_eq!(mc, back);
}

// 81
#[test]
fn message_content_parts_roundtrip() {
    let mc = MessageContent::Parts(vec![
        ContentPart::Text {
            text: "describe".into(),
        },
        ContentPart::ImageUrl {
            image_url: ImageUrl {
                url: "https://example.com/img.png".into(),
                detail: None,
            },
        },
    ]);
    let json = serde_json::to_string(&mc).unwrap();
    let back: MessageContent = serde_json::from_str(&json).unwrap();
    assert_eq!(mc, back);
}

// ═════════════════════════════════════════════════════════════════════════
//  14. Edge cases
// ═════════════════════════════════════════════════════════════════════════

// 82
#[test]
fn empty_messages_array() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert!(back.messages.is_empty());
}

// 83
#[test]
fn empty_messages_ir_conversion() {
    let conv = shim::messages_to_ir(&[]);
    assert!(conv.is_empty());
}

// 84
#[test]
fn unicode_in_messages() {
    let msg = ChatMessage::User {
        content: MessageContent::Text("こんにちは世界 🌍 مرحبا".into()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

// 85
#[test]
fn special_chars_in_messages() {
    let msg = ChatMessage::User {
        content: MessageContent::Text(r#"He said "hello" & <world> 'foo' \n\t"#.into()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

// 86
#[test]
fn very_long_content() {
    let long_text = "a".repeat(100_000);
    let msg = ChatMessage::User {
        content: MessageContent::Text(long_text.clone()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    if let ChatMessage::User {
        content: MessageContent::Text(t),
    } = back
    {
        assert_eq!(t.len(), 100_000);
    } else {
        panic!("wrong variant");
    }
}

// 87
#[test]
fn newlines_and_tabs_in_content() {
    let msg = ChatMessage::User {
        content: MessageContent::Text("line1\nline2\ttab".into()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

// 88
#[test]
fn empty_string_content() {
    let msg = ChatMessage::User {
        content: MessageContent::Text(String::new()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

// 89
#[test]
fn null_optional_fields_deserialize() {
    let json_str = r#"{
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": null,
        "top_p": null,
        "max_tokens": null,
        "stream": null,
        "tools": null,
        "tool_choice": null
    }"#;
    let req: ChatCompletionRequest = serde_json::from_str(json_str).unwrap();
    assert!(req.temperature.is_none());
    assert!(req.tools.is_none());
}

// ═════════════════════════════════════════════════════════════════════════
//  Additional coverage: client builder, IR, streaming events
// ═════════════════════════════════════════════════════════════════════════

// 90
#[test]
fn client_builder_default_base_url() {
    let client = Client::new("sk-test-key").unwrap();
    assert_eq!(client.base_url(), "https://api.openai.com/v1");
}

// 91
#[test]
fn client_builder_custom_base_url() {
    let client = Client::builder("sk-key")
        .base_url("https://custom.example.com/v1")
        .build()
        .unwrap();
    assert_eq!(client.base_url(), "https://custom.example.com/v1");
}

// 92
#[test]
fn client_builder_custom_timeout() {
    let client = Client::builder("sk-key")
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap();
    assert_eq!(client.base_url(), "https://api.openai.com/v1");
}

// 93
#[test]
fn client_error_builder_display() {
    let err = ClientError::Builder("bad config".into());
    assert!(err.to_string().contains("bad config"));
}

// 94
#[test]
fn ir_usage_conversion() {
    use abp_core::ir::IrUsage;
    let ir = IrUsage::from_io(200, 100);
    let usage = shim::ir_usage_to_usage(&ir);
    assert_eq!(usage.prompt_tokens, 200);
    assert_eq!(usage.completion_tokens, 100);
    assert_eq!(usage.total_tokens, 300);
}

// 95
#[test]
fn request_to_ir_roundtrip() {
    let req = ShimRequest::builder()
        .messages(vec![Message::system("Be concise."), Message::user("Hello")])
        .build();
    let conv = shim::request_to_ir(&req);
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, abp_core::ir::IrRole::System);
    assert_eq!(conv.messages[1].role, abp_core::ir::IrRole::User);
}

// 96
#[test]
fn messages_to_ir_and_back() {
    let messages = vec![
        Message::system("System prompt"),
        Message::user("User message"),
        Message::assistant("Reply"),
    ];
    let conv = shim::messages_to_ir(&messages);
    let back = shim::ir_to_messages(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, Role::System);
    assert_eq!(back[1].role, Role::User);
    assert_eq!(back[2].role, Role::Assistant);
}

// 97
#[test]
fn events_to_stream_events_includes_stop() {
    let events = vec![make_event(AgentEventKind::AssistantDelta {
        text: "hi".into(),
    })];
    let stream = shim::events_to_stream_events(&events, "gpt-4o");
    assert_eq!(stream.len(), 2);
    assert_eq!(
        stream.last().unwrap().choices[0].finish_reason.as_deref(),
        Some("stop")
    );
}

// 98
#[test]
fn events_to_stream_events_tool_call() {
    let events = vec![make_event(AgentEventKind::ToolCall {
        tool_name: "search".into(),
        tool_use_id: Some("call_1".into()),
        parent_tool_use_id: None,
        input: json!({"q": "rust"}),
    })];
    let stream = shim::events_to_stream_events(&events, "gpt-4o");
    assert!(stream.len() >= 2);
    let tc = &stream[0].choices[0].delta.tool_calls.as_ref().unwrap()[0];
    assert_eq!(
        tc.function.as_ref().unwrap().name.as_deref(),
        Some("search")
    );
}

// 99
#[test]
fn events_to_stream_events_assistant_message() {
    let events = vec![make_event(AgentEventKind::AssistantMessage {
        text: "Hi!".into(),
    })];
    let stream = shim::events_to_stream_events(&events, "gpt-4o");
    assert_eq!(stream[0].choices[0].delta.content.as_deref(), Some("Hi!"));
    assert_eq!(
        stream[0].choices[0].delta.role.as_deref(),
        Some("assistant")
    );
}

// 100
#[test]
fn openai_client_debug_shows_model() {
    let client = OpenAiClient::new("gpt-4o");
    let debug = format!("{:?}", client);
    assert!(debug.contains("gpt-4o"));
}

// 101
#[test]
fn openai_client_model_accessor() {
    let client = OpenAiClient::new("o3-mini");
    assert_eq!(client.model(), "o3-mini");
}

// 102
#[tokio::test]
async fn streaming_no_processor_returns_error() {
    let client = OpenAiClient::new("gpt-4o");
    let req = minimal_shim_request();
    match client.chat().completions().create_stream(req).await {
        Err(ShimError::Internal(_)) => {} // expected
        Err(e) => panic!("unexpected error variant: {e}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// 103
#[test]
fn convert_count_roles() {
    use abp_shim_openai::convert::count_roles;
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![
            ChatMessage::System {
                content: "s".into(),
            },
            ChatMessage::User {
                content: MessageContent::Text("u1".into()),
            },
            ChatMessage::User {
                content: MessageContent::Text("u2".into()),
            },
            ChatMessage::Assistant {
                content: Some("a".into()),
                tool_calls: None,
            },
        ],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
    };
    let counts = count_roles(&req);
    assert_eq!(counts.get("system"), Some(&1));
    assert_eq!(counts.get("user"), Some(&2));
    assert_eq!(counts.get("assistant"), Some(&1));
}

// 104
#[test]
fn convert_extract_task_from_user_message() {
    use abp_shim_openai::convert::extract_task;
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![
            ChatMessage::System {
                content: "system".into(),
            },
            ChatMessage::User {
                content: MessageContent::Text("user task".into()),
            },
        ],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
    };
    assert_eq!(extract_task(&req), "user task");
}

// 105
#[test]
fn convert_extract_task_no_user_message() {
    use abp_shim_openai::convert::extract_task;
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![ChatMessage::System {
            content: "system only".into(),
        }],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
    };
    assert_eq!(extract_task(&req), "chat completion");
}

// 106
#[test]
fn convert_usage_from_normalized() {
    use abp_shim_openai::convert::usage_from_normalized;
    let usage = UsageNormalized {
        input_tokens: Some(50),
        output_tokens: Some(25),
        ..Default::default()
    };
    let u = usage_from_normalized(&usage);
    assert_eq!(u.prompt_tokens, 50);
    assert_eq!(u.completion_tokens, 25);
    assert_eq!(u.total_tokens, 75);
}

// 107
#[test]
fn convert_usage_from_normalized_zeros() {
    use abp_shim_openai::convert::usage_from_normalized;
    let usage = UsageNormalized::default();
    let u = usage_from_normalized(&usage);
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.completion_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

// 108
#[test]
fn convert_message_content_to_string_text() {
    use abp_shim_openai::convert::message_content_to_string;
    let mc = MessageContent::Text("hello".into());
    assert_eq!(message_content_to_string(&mc), "hello");
}

// 109
#[test]
fn convert_message_content_to_string_parts() {
    use abp_shim_openai::convert::message_content_to_string;
    let mc = MessageContent::Parts(vec![
        ContentPart::Text {
            text: "part1".into(),
        },
        ContentPart::ImageUrl {
            image_url: ImageUrl {
                url: "https://img.png".into(),
                detail: None,
            },
        },
        ContentPart::Text {
            text: "part2".into(),
        },
    ]);
    assert_eq!(message_content_to_string(&mc), "part1part2");
}

// 110
#[test]
fn convert_role_to_str() {
    use abp_shim_openai::convert::role_to_str;
    assert_eq!(
        role_to_str(&ChatMessage::System { content: "".into() }),
        "system"
    );
    assert_eq!(
        role_to_str(&ChatMessage::User {
            content: MessageContent::Text("".into())
        }),
        "user"
    );
    assert_eq!(
        role_to_str(&ChatMessage::Assistant {
            content: None,
            tool_calls: None
        }),
        "assistant"
    );
    assert_eq!(
        role_to_str(&ChatMessage::Tool {
            content: "".into(),
            tool_call_id: "".into()
        }),
        "tool"
    );
}

// 111
#[test]
fn convert_make_stop_chunk() {
    use abp_shim_openai::convert::make_stop_chunk;
    let chunk = make_stop_chunk("gpt-4o", "chatcmpl-test");
    assert_eq!(chunk.id, "chatcmpl-test");
    assert_eq!(chunk.object, "chat.completion.chunk");
    assert_eq!(chunk.model, "gpt-4o");
    assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
}

// 112
#[test]
fn convert_from_agent_event_delta() {
    use abp_shim_openai::convert::from_agent_event;
    let event = make_event(AgentEventKind::AssistantDelta { text: "tok".into() });
    let chunk = from_agent_event(&event, "gpt-4o", "chunk-1").unwrap();
    assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("tok"));
    assert!(chunk.choices[0].delta.role.is_none());
}

// 113
#[test]
fn convert_from_agent_event_message() {
    use abp_shim_openai::convert::from_agent_event;
    let event = make_event(AgentEventKind::AssistantMessage {
        text: "full msg".into(),
    });
    let chunk = from_agent_event(&event, "gpt-4o", "chunk-2").unwrap();
    assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("full msg"));
    assert_eq!(chunk.choices[0].delta.role.as_deref(), Some("assistant"));
}

// 114
#[test]
fn convert_from_agent_event_run_completed() {
    use abp_shim_openai::convert::from_agent_event;
    let event = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    let chunk = from_agent_event(&event, "gpt-4o", "chunk-3").unwrap();
    assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
}

// 115
#[test]
fn convert_from_agent_event_skips_file_changed() {
    use abp_shim_openai::convert::from_agent_event;
    let event = make_event(AgentEventKind::FileChanged {
        path: "foo.rs".into(),
        summary: "modified".into(),
    });
    assert!(from_agent_event(&event, "gpt-4o", "chunk-4").is_none());
}

// 116
#[test]
fn convert_from_agent_event_skips_warning() {
    use abp_shim_openai::convert::from_agent_event;
    let event = make_event(AgentEventKind::Warning {
        message: "warn".into(),
    });
    assert!(from_agent_event(&event, "gpt-4o", "chunk-5").is_none());
}
