// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the OpenAI ↔ IR translation layer in `ir_translate`.

use abp_sdk_types::ir::{IrContentPart, IrRole};
use abp_sdk_types::ir_response::{IrChatResponse, IrFinishReason};

use openai_bridge::ir_translate::*;
use openai_bridge::openai_types::*;

// ── Helper builders ─────────────────────────────────────────────────────

fn user_msg(text: &str) -> ChatMessage {
    ChatMessage {
        role: ChatMessageRole::User,
        content: Some(text.into()),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn system_msg(text: &str) -> ChatMessage {
    ChatMessage {
        role: ChatMessageRole::System,
        content: Some(text.into()),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn assistant_msg(text: &str) -> ChatMessage {
    ChatMessage {
        role: ChatMessageRole::Assistant,
        content: Some(text.into()),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn tool_result_msg(call_id: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: ChatMessageRole::Tool,
        content: Some(content.into()),
        tool_calls: None,
        tool_call_id: Some(call_id.into()),
    }
}

fn make_tool_def(name: &str) -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: FunctionDefinition {
            name: name.into(),
            description: format!("A {name} tool"),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        },
    }
}

fn minimal_request() -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![user_msg("Hello")],
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
    }
}

fn minimal_response() -> ChatCompletionResponse {
    ChatCompletionResponse {
        id: "chatcmpl-abc".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![ChatCompletionChoice {
            index: 0,
            message: assistant_msg("Hi there!"),
            finish_reason: Some("stop".into()),
        }],
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    }
}

fn text_chunk(id: &str, model: &str, text: &str) -> ChatCompletionChunk {
    ChatCompletionChunk {
        id: id.into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: model.into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: None,
                content: Some(text.into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Request translation tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn request_minimal_roundtrip() {
    let req = minimal_request();
    let ir = openai_request_to_ir(&req);
    assert_eq!(ir.model, "gpt-4o");
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "Hello");

    let back = ir_to_openai_request(&ir);
    assert_eq!(back.model, "gpt-4o");
    assert_eq!(back.messages.len(), 1);
    assert_eq!(back.messages[0].content.as_deref(), Some("Hello"));
    assert_eq!(back.messages[0].role, ChatMessageRole::User);
}

#[test]
fn request_model_preserved() {
    let mut req = minimal_request();
    req.model = "gpt-3.5-turbo".into();
    let ir = openai_request_to_ir(&req);
    assert_eq!(ir.model, "gpt-3.5-turbo");
    let back = ir_to_openai_request(&ir);
    assert_eq!(back.model, "gpt-3.5-turbo");
}

#[test]
fn request_system_message() {
    let mut req = minimal_request();
    req.messages = vec![system_msg("You are helpful"), user_msg("Hi")];
    let ir = openai_request_to_ir(&req);
    assert_eq!(ir.messages.len(), 2);
    assert_eq!(ir.messages[0].role, IrRole::System);
    assert_eq!(ir.messages[0].text_content(), "You are helpful");
}

#[test]
fn request_temperature_roundtrip() {
    let mut req = minimal_request();
    req.temperature = Some(0.7);
    let ir = openai_request_to_ir(&req);
    assert_eq!(ir.sampling.temperature, Some(0.7));
    let back = ir_to_openai_request(&ir);
    assert_eq!(back.temperature, Some(0.7));
}

#[test]
fn request_top_p_roundtrip() {
    let mut req = minimal_request();
    req.top_p = Some(0.95);
    let ir = openai_request_to_ir(&req);
    assert_eq!(ir.sampling.top_p, Some(0.95));
    let back = ir_to_openai_request(&ir);
    assert_eq!(back.top_p, Some(0.95));
}

#[test]
fn request_max_tokens_roundtrip() {
    let mut req = minimal_request();
    req.max_tokens = Some(1024);
    let ir = openai_request_to_ir(&req);
    assert_eq!(ir.max_tokens, Some(1024));
    let back = ir_to_openai_request(&ir);
    assert_eq!(back.max_tokens, Some(1024));
}

#[test]
fn request_stop_sequences_roundtrip() {
    let mut req = minimal_request();
    req.stop = Some(vec!["END".into(), "DONE".into()]);
    let ir = openai_request_to_ir(&req);
    assert_eq!(ir.stop_sequences, vec!["END", "DONE"]);
    let back = ir_to_openai_request(&ir);
    assert_eq!(back.stop, Some(vec!["END".into(), "DONE".into()]));
}

#[test]
fn request_stream_flag_roundtrip() {
    let mut req = minimal_request();
    req.stream = Some(true);
    let ir = openai_request_to_ir(&req);
    assert!(ir.stream.enabled);
    let back = ir_to_openai_request(&ir);
    assert_eq!(back.stream, Some(true));
}

#[test]
fn request_stream_false_omitted() {
    let req = minimal_request();
    let ir = openai_request_to_ir(&req);
    assert!(!ir.stream.enabled);
    let back = ir_to_openai_request(&ir);
    assert_eq!(back.stream, None);
}

#[test]
fn request_frequency_penalty_roundtrip() {
    let mut req = minimal_request();
    req.frequency_penalty = Some(0.5);
    let ir = openai_request_to_ir(&req);
    assert_eq!(ir.sampling.frequency_penalty, Some(0.5));
    let back = ir_to_openai_request(&ir);
    assert_eq!(back.frequency_penalty, Some(0.5));
}

#[test]
fn request_presence_penalty_roundtrip() {
    let mut req = minimal_request();
    req.presence_penalty = Some(-0.3);
    let ir = openai_request_to_ir(&req);
    assert_eq!(ir.sampling.presence_penalty, Some(-0.3));
    let back = ir_to_openai_request(&ir);
    assert_eq!(back.presence_penalty, Some(-0.3));
}

#[test]
fn request_tools_roundtrip() {
    let mut req = minimal_request();
    req.tools = Some(vec![make_tool_def("search"), make_tool_def("read_file")]);
    let ir = openai_request_to_ir(&req);
    assert_eq!(ir.tools.len(), 2);
    assert_eq!(ir.tools[0].name, "search");
    assert_eq!(ir.tools[1].name, "read_file");
    let back = ir_to_openai_request(&ir);
    let tools = back.tools.unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].function.name, "search");
    assert_eq!(tools[1].function.name, "read_file");
}

#[test]
fn request_tool_choice_roundtrip() {
    let mut req = minimal_request();
    req.tool_choice = Some(serde_json::json!("auto"));
    let ir = openai_request_to_ir(&req);
    assert_eq!(ir.tool_choice, Some(serde_json::json!("auto")));
    let back = ir_to_openai_request(&ir);
    assert_eq!(back.tool_choice, Some(serde_json::json!("auto")));
}

#[test]
fn request_n_preserved_in_extra() {
    let mut req = minimal_request();
    req.n = Some(3);
    let ir = openai_request_to_ir(&req);
    assert_eq!(ir.extra.get("n"), Some(&serde_json::json!(3)));
    let back = ir_to_openai_request(&ir);
    assert_eq!(back.n, Some(3));
}

#[test]
fn request_no_tools_yields_none() {
    let req = minimal_request();
    let ir = openai_request_to_ir(&req);
    assert!(ir.tools.is_empty());
    let back = ir_to_openai_request(&ir);
    assert_eq!(back.tools, None);
}

// ═══════════════════════════════════════════════════════════════════════
//  Message translation tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn message_assistant_with_tool_calls() {
    let msg = ChatMessage {
        role: ChatMessageRole::Assistant,
        content: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "search".into(),
                arguments: r#"{"query":"rust"}"#.into(),
            },
        }]),
        tool_call_id: None,
    };
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![msg],
        ..minimal_request()
    };
    let ir = openai_request_to_ir(&req);
    let ir_msg = &ir.messages[0];
    assert_eq!(ir_msg.role, IrRole::Assistant);
    assert_eq!(ir_msg.tool_calls.len(), 1);
    assert_eq!(ir_msg.tool_calls[0].name, "search");
    assert_eq!(ir_msg.tool_calls[0].id, "call_1");
}

#[test]
fn message_tool_result_roundtrip() {
    let msg = tool_result_msg("call_1", "42 results found");
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![msg],
        ..minimal_request()
    };
    let ir = openai_request_to_ir(&req);
    let ir_msg = &ir.messages[0];
    assert_eq!(ir_msg.role, IrRole::Tool);
    assert!(ir_msg.content.iter().any(|p| matches!(
        p,
        IrContentPart::ToolResult { call_id, content, .. }
        if call_id == "call_1" && content == "42 results found"
    )));

    let back = ir_to_openai_request(&ir);
    assert_eq!(back.messages[0].role, ChatMessageRole::Tool);
    assert_eq!(back.messages[0].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(
        back.messages[0].content.as_deref(),
        Some("42 results found")
    );
}

#[test]
fn message_assistant_text_and_tool_calls() {
    let msg = ChatMessage {
        role: ChatMessageRole::Assistant,
        content: Some("Let me search.".into()),
        tool_calls: Some(vec![ToolCall {
            id: "call_2".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "web_search".into(),
                arguments: "{}".into(),
            },
        }]),
        tool_call_id: None,
    };
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![msg],
        ..minimal_request()
    };
    let ir = openai_request_to_ir(&req);
    let ir_msg = &ir.messages[0];
    assert_eq!(ir_msg.text_content(), "Let me search.");
    assert_eq!(ir_msg.tool_calls.len(), 1);

    let back = ir_to_openai_request(&ir);
    assert_eq!(back.messages[0].content.as_deref(), Some("Let me search."));
    assert!(back.messages[0].tool_calls.is_some());
}

#[test]
fn message_empty_content_assistant() {
    let msg = ChatMessage {
        role: ChatMessageRole::Assistant,
        content: Some(String::new()),
        tool_calls: None,
        tool_call_id: None,
    };
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![msg],
        ..minimal_request()
    };
    let ir = openai_request_to_ir(&req);
    assert!(ir.messages[0].content.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
//  Response translation tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn response_minimal_roundtrip() {
    let resp = minimal_response();
    let ir = openai_response_to_ir(&resp);
    assert_eq!(ir.id.as_deref(), Some("chatcmpl-abc"));
    assert_eq!(ir.model.as_deref(), Some("gpt-4o"));
    assert_eq!(ir.choices.len(), 1);
    assert_eq!(ir.choices[0].message.text_content(), "Hi there!");
    assert_eq!(ir.choices[0].finish_reason, Some(IrFinishReason::Stop));

    let back = ir_to_openai_response(&ir);
    assert_eq!(back.id, "chatcmpl-abc");
    assert_eq!(back.model, "gpt-4o");
    assert_eq!(back.object, "chat.completion");
    assert_eq!(
        back.choices[0].message.content.as_deref(),
        Some("Hi there!")
    );
    assert_eq!(back.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn response_usage_roundtrip() {
    let resp = minimal_response();
    let ir = openai_response_to_ir(&resp);
    let usage = ir.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 10);
    assert_eq!(usage.completion_tokens, 5);
    assert_eq!(usage.total_tokens, 15);

    let back = ir_to_openai_response(&ir);
    let u = back.usage.unwrap();
    assert_eq!(u.prompt_tokens, 10);
    assert_eq!(u.completion_tokens, 5);
    assert_eq!(u.total_tokens, 15);
}

#[test]
fn response_no_usage() {
    let mut resp = minimal_response();
    resp.usage = None;
    let ir = openai_response_to_ir(&resp);
    assert!(ir.usage.is_none());
    let back = ir_to_openai_response(&ir);
    assert!(back.usage.is_none());
}

#[test]
fn response_finish_reason_length() {
    let mut resp = minimal_response();
    resp.choices[0].finish_reason = Some("length".into());
    let ir = openai_response_to_ir(&resp);
    assert_eq!(ir.choices[0].finish_reason, Some(IrFinishReason::Length));
    let back = ir_to_openai_response(&ir);
    assert_eq!(back.choices[0].finish_reason.as_deref(), Some("length"));
}

#[test]
fn response_finish_reason_tool_calls() {
    let mut resp = minimal_response();
    resp.choices[0].finish_reason = Some("tool_calls".into());
    let ir = openai_response_to_ir(&resp);
    assert_eq!(ir.choices[0].finish_reason, Some(IrFinishReason::ToolUse));
    let back = ir_to_openai_response(&ir);
    assert_eq!(back.choices[0].finish_reason.as_deref(), Some("tool_calls"));
}

#[test]
fn response_finish_reason_content_filter() {
    let mut resp = minimal_response();
    resp.choices[0].finish_reason = Some("content_filter".into());
    let ir = openai_response_to_ir(&resp);
    assert_eq!(
        ir.choices[0].finish_reason,
        Some(IrFinishReason::ContentFilter)
    );
}

#[test]
fn response_multiple_choices() {
    let resp = ChatCompletionResponse {
        id: "resp-multi".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![
            ChatCompletionChoice {
                index: 0,
                message: assistant_msg("First"),
                finish_reason: Some("stop".into()),
            },
            ChatCompletionChoice {
                index: 1,
                message: assistant_msg("Second"),
                finish_reason: Some("stop".into()),
            },
        ],
        usage: None,
    };
    let ir = openai_response_to_ir(&resp);
    assert_eq!(ir.choices.len(), 2);
    assert_eq!(ir.choices[0].message.text_content(), "First");
    assert_eq!(ir.choices[1].message.text_content(), "Second");
    assert_eq!(ir.choices[1].index, 1);
}

#[test]
fn response_with_tool_calls() {
    let msg = ChatMessage {
        role: ChatMessageRole::Assistant,
        content: None,
        tool_calls: Some(vec![ToolCall {
            id: "tc_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "get_weather".into(),
                arguments: r#"{"city":"NYC"}"#.into(),
            },
        }]),
        tool_call_id: None,
    };
    let resp = ChatCompletionResponse {
        id: "resp-tc".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![ChatCompletionChoice {
            index: 0,
            message: msg,
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
    };
    let ir = openai_response_to_ir(&resp);
    let ir_msg = &ir.choices[0].message;
    assert_eq!(ir_msg.tool_calls.len(), 1);
    assert_eq!(ir_msg.tool_calls[0].name, "get_weather");
    assert_eq!(ir_msg.tool_calls[0].id, "tc_1");

    let back = ir_to_openai_response(&ir);
    let back_tcs = back.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(back_tcs[0].function.name, "get_weather");
    assert_eq!(back_tcs[0].id, "tc_1");
}

// ═══════════════════════════════════════════════════════════════════════
//  Streaming translation tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stream_text_delta() {
    let chunk = text_chunk("chunk-1", "gpt-4o", "Hello");
    let ir_chunks = openai_stream_to_ir(&chunk);
    assert_eq!(ir_chunks.len(), 1);
    assert_eq!(ir_chunks[0].id.as_deref(), Some("chunk-1"));
    assert_eq!(ir_chunks[0].model.as_deref(), Some("gpt-4o"));
    assert_eq!(ir_chunks[0].delta_content.len(), 1);
    assert_eq!(ir_chunks[0].delta_content[0].as_text(), Some("Hello"));
    assert!(!ir_chunks[0].is_final());
}

#[test]
fn stream_role_chunk() {
    let chunk = ChatCompletionChunk {
        id: "chunk-role".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: Some("assistant".into()),
                content: None,
                tool_calls: None,
            },
            finish_reason: None,
        }],
    };
    let ir = openai_stream_to_ir(&chunk);
    assert_eq!(ir[0].role, Some(IrRole::Assistant));
}

#[test]
fn stream_finish_reason_stop() {
    let chunk = ChatCompletionChunk {
        id: "chunk-fin".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta::default(),
            finish_reason: Some("stop".into()),
        }],
    };
    let ir = openai_stream_to_ir(&chunk);
    assert!(ir[0].is_final());
    assert_eq!(ir[0].finish_reason, Some(IrFinishReason::Stop));
}

#[test]
fn stream_finish_reason_tool_calls() {
    let chunk = ChatCompletionChunk {
        id: "chunk-tc".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta::default(),
            finish_reason: Some("tool_calls".into()),
        }],
    };
    let ir = openai_stream_to_ir(&chunk);
    assert_eq!(ir[0].finish_reason, Some(IrFinishReason::ToolUse));
}

#[test]
fn stream_tool_call_delta() {
    let chunk = ChatCompletionChunk {
        id: "chunk-tcd".into(),
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
                    id: Some("call_42".into()),
                    call_type: Some("function".into()),
                    function: Some(StreamFunctionCall {
                        name: Some("search".into()),
                        arguments: Some(r#"{"q":"test"}"#.into()),
                    }),
                }]),
            },
            finish_reason: None,
        }],
    };
    let ir = openai_stream_to_ir(&chunk);
    assert_eq!(ir[0].delta_tool_calls.len(), 1);
    assert_eq!(ir[0].delta_tool_calls[0].id, "call_42");
    assert_eq!(ir[0].delta_tool_calls[0].name, "search");
}

#[test]
fn stream_empty_choices() {
    let chunk = ChatCompletionChunk {
        id: "chunk-empty".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![],
    };
    let ir = openai_stream_to_ir(&chunk);
    assert!(ir.is_empty());
}

#[test]
fn stream_multiple_choices() {
    let chunk = ChatCompletionChunk {
        id: "chunk-multi".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![
            StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: None,
                    content: Some("A".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            },
            StreamChoice {
                index: 1,
                delta: StreamDelta {
                    role: None,
                    content: Some("B".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            },
        ],
    };
    let ir = openai_stream_to_ir(&chunk);
    assert_eq!(ir.len(), 2);
    assert_eq!(ir[0].index, 0);
    assert_eq!(ir[1].index, 1);
    assert_eq!(ir[0].delta_content[0].as_text(), Some("A"));
    assert_eq!(ir[1].delta_content[0].as_text(), Some("B"));
}

// ═══════════════════════════════════════════════════════════════════════
//  Full conversation roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn full_conversation_roundtrip() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![
            system_msg("You are a helpful assistant."),
            user_msg("What is 2+2?"),
            assistant_msg("4"),
            user_msg("And 3+3?"),
        ],
        tools: Some(vec![make_tool_def("calculator")]),
        temperature: Some(0.5),
        max_tokens: Some(2048),
        stream: Some(true),
        top_p: Some(0.9),
        frequency_penalty: Some(0.1),
        presence_penalty: Some(0.2),
        stop: Some(vec!["STOP".into()]),
        n: Some(2),
        tool_choice: Some(serde_json::json!("auto")),
    };

    let ir = openai_request_to_ir(&req);
    let back = ir_to_openai_request(&ir);

    assert_eq!(back.model, "gpt-4o");
    assert_eq!(back.messages.len(), 4);
    assert_eq!(back.messages[0].role, ChatMessageRole::System);
    assert_eq!(back.messages[1].role, ChatMessageRole::User);
    assert_eq!(back.messages[2].role, ChatMessageRole::Assistant);
    assert_eq!(back.messages[3].role, ChatMessageRole::User);
    assert_eq!(back.temperature, Some(0.5));
    assert_eq!(back.max_tokens, Some(2048));
    assert_eq!(back.stream, Some(true));
    assert_eq!(back.top_p, Some(0.9));
    assert_eq!(back.frequency_penalty, Some(0.1));
    assert_eq!(back.presence_penalty, Some(0.2));
    assert_eq!(back.stop, Some(vec!["STOP".into()]));
    assert_eq!(back.n, Some(2));
    assert_eq!(back.tool_choice, Some(serde_json::json!("auto")));
    assert_eq!(back.tools.as_ref().unwrap().len(), 1);
}

#[test]
fn response_roundtrip_preserves_identity() {
    let resp = minimal_response();
    let ir = openai_response_to_ir(&resp);
    let back = ir_to_openai_response(&ir);
    assert_eq!(back.id, resp.id);
    assert_eq!(back.model, resp.model);
    assert_eq!(
        back.choices[0].message.content,
        resp.choices[0].message.content
    );
    assert_eq!(back.choices[0].finish_reason, resp.choices[0].finish_reason);
    assert_eq!(back.usage.unwrap().prompt_tokens, 10);
    assert_eq!(back.usage.unwrap().completion_tokens, 5);
}

#[test]
fn tool_definition_roundtrip_preserves_schema() {
    let td = make_tool_def("my_tool");
    let mut req = minimal_request();
    req.tools = Some(vec![td.clone()]);
    let ir = openai_request_to_ir(&req);
    let back = ir_to_openai_request(&ir);
    let back_td = &back.tools.unwrap()[0];
    assert_eq!(back_td.function.name, "my_tool");
    assert_eq!(back_td.function.description, "A my_tool tool");
    assert_eq!(back_td.tool_type, "function");
    assert_eq!(back_td.function.parameters, td.function.parameters);
}

#[test]
fn ir_response_default_fields() {
    let ir = IrChatResponse::text("Hello");
    let oai = ir_to_openai_response(&ir);
    assert_eq!(oai.id, "");
    assert_eq!(oai.model, "");
    assert_eq!(oai.object, "chat.completion");
    assert_eq!(oai.created, 0);
}
