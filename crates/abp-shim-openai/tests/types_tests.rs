// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the `types` module — serde roundtrips and OpenAI API format fidelity.

use abp_shim_openai::types::*;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// 1. ChatCompletionRequest serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_serde_roundtrip() {
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
        top_p: Some(0.9),
        max_tokens: Some(4096),
        stream: None,
        tools: None,
        tool_choice: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. ChatMessage::System roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn system_message_roundtrip() {
    let msg = ChatMessage::System {
        content: "Be concise.".into(),
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "system");
    assert_eq!(v["content"], "Be concise.");
    let back: ChatMessage = serde_json::from_value(v).unwrap();
    assert_eq!(msg, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. ChatMessage::User with text content
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn user_message_text_content() {
    let msg = ChatMessage::User {
        content: MessageContent::Text("Hello world".into()),
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "user");
    assert_eq!(v["content"], "Hello world");
    let back: ChatMessage = serde_json::from_value(v).unwrap();
    assert_eq!(msg, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. ChatMessage::User with content parts array
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn user_message_content_parts() {
    let msg = ChatMessage::User {
        content: MessageContent::Parts(vec![
            ContentPart::Text {
                text: "What is in this image?".into(),
            },
            ContentPart::ImageUrl {
                image_url: ImageUrl {
                    url: "https://example.com/img.png".into(),
                    detail: Some("high".into()),
                },
            },
        ]),
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "user");
    let parts = v["content"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["type"], "text");
    assert_eq!(parts[1]["type"], "image_url");
    assert_eq!(parts[1]["image_url"]["detail"], "high");
    let back: ChatMessage = serde_json::from_value(v).unwrap();
    assert_eq!(msg, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. ChatMessage::Assistant with content only
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn assistant_message_content_only() {
    let msg = ChatMessage::Assistant {
        content: Some("The answer is 42.".into()),
        tool_calls: None,
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "assistant");
    assert_eq!(v["content"], "The answer is 42.");
    assert!(v.get("tool_calls").is_none());
    let back: ChatMessage = serde_json::from_value(v).unwrap();
    assert_eq!(msg, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. ChatMessage::Assistant with tool calls
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn assistant_message_with_tool_calls() {
    let msg = ChatMessage::Assistant {
        content: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_abc".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "get_weather".into(),
                arguments: r#"{"location":"SF"}"#.into(),
            },
        }]),
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "assistant");
    assert!(v.get("content").is_none());
    assert_eq!(v["tool_calls"][0]["type"], "function");
    assert_eq!(v["tool_calls"][0]["function"]["name"], "get_weather");
    let back: ChatMessage = serde_json::from_value(v).unwrap();
    assert_eq!(msg, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. ChatMessage::Tool roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_message_roundtrip() {
    let msg = ChatMessage::Tool {
        content: "72°F, sunny".into(),
        tool_call_id: "call_abc".into(),
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "tool");
    assert_eq!(v["content"], "72°F, sunny");
    assert_eq!(v["tool_call_id"], "call_abc");
    let back: ChatMessage = serde_json::from_value(v).unwrap();
    assert_eq!(msg, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. ChatCompletionResponse roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_serde_roundtrip() {
    let resp = ChatCompletionResponse {
        id: "chatcmpl-abc".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. StreamChunk roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_chunk_serde_roundtrip() {
    let chunk = StreamChunk {
        id: "chatcmpl-stream".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: Some("assistant".into()),
                content: Some("Hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: StreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Tool definition roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_serde_roundtrip() {
    let tool = Tool {
        tool_type: "function".into(),
        function: FunctionDef {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        },
    };
    let v = serde_json::to_value(&tool).unwrap();
    assert_eq!(v["type"], "function");
    assert!(v.get("tool_type").is_none());
    let back: Tool = serde_json::from_value(v).unwrap();
    assert_eq!(tool, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. ToolCall roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_call_serde_roundtrip() {
    let tc = ToolCall {
        id: "call_xyz".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "read_file".into(),
            arguments: r#"{"path":"main.rs"}"#.into(),
        },
    };
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v["type"], "function");
    assert!(v.get("call_type").is_none());
    let back: ToolCall = serde_json::from_value(v).unwrap();
    assert_eq!(tc, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 12–14. ToolChoice modes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_choice_none() {
    let tc = ToolChoice::Mode(ToolChoiceMode::None);
    assert_eq!(serde_json::to_value(&tc).unwrap(), json!("none"));
}

#[test]
fn tool_choice_auto() {
    let tc = ToolChoice::Mode(ToolChoiceMode::Auto);
    assert_eq!(serde_json::to_value(&tc).unwrap(), json!("auto"));
}

#[test]
fn tool_choice_required() {
    let tc = ToolChoice::Mode(ToolChoiceMode::Required);
    assert_eq!(serde_json::to_value(&tc).unwrap(), json!("required"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. ToolChoice specific function
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_choice_specific_function() {
    let tc = ToolChoice::Function {
        tool_type: "function".into(),
        function: ToolChoiceFunctionRef {
            name: "get_weather".into(),
        },
    };
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v["type"], "function");
    assert_eq!(v["function"]["name"], "get_weather");
    let back: ToolChoice = serde_json::from_value(v).unwrap();
    assert_eq!(tc, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. Usage roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn usage_serde_roundtrip() {
    let usage = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let v = serde_json::to_value(&usage).unwrap();
    assert_eq!(v["prompt_tokens"], 100);
    assert_eq!(v["completion_tokens"], 50);
    assert_eq!(v["total_tokens"], 150);
    let back: Usage = serde_json::from_value(v).unwrap();
    assert_eq!(usage, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. Deserialize realistic OpenAI request JSON
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deserialize_realistic_request() {
    let json = json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "system", "content": "You are a poet."},
            {"role": "user", "content": "Write a haiku about Rust."}
        ],
        "temperature": 0.9,
        "top_p": 0.95,
        "max_tokens": 64,
        "stream": false
    });
    let req: ChatCompletionRequest = serde_json::from_value(json).unwrap();
    assert_eq!(req.model, "gpt-4o");
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.temperature, Some(0.9));
    assert_eq!(req.top_p, Some(0.95));
    assert_eq!(req.max_tokens, Some(64));
    assert_eq!(req.stream, Some(false));
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Deserialize realistic OpenAI response JSON
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deserialize_realistic_response() {
    let json = json!({
        "id": "chatcmpl-9abc123",
        "object": "chat.completion",
        "created": 1700000000,
        "model": "gpt-4o-2024-05-13",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello! How can I help you today?"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 12,
            "completion_tokens": 8,
            "total_tokens": 20
        }
    });
    let resp: ChatCompletionResponse = serde_json::from_value(json).unwrap();
    assert_eq!(resp.id, "chatcmpl-9abc123");
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.created, 1700000000);
    assert_eq!(resp.choices[0].message.role, "assistant");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello! How can I help you today?")
    );
    assert_eq!(resp.usage.unwrap().total_tokens, 20);
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. Deserialize realistic streaming chunk JSON
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deserialize_realistic_stream_chunk() {
    let json = json!({
        "id": "chatcmpl-stream1",
        "object": "chat.completion.chunk",
        "created": 1700000002,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "delta": {
                "role": "assistant",
                "content": "Hello"
            },
            "finish_reason": null
        }]
    });
    let chunk: StreamChunk = serde_json::from_value(json).unwrap();
    assert_eq!(chunk.object, "chat.completion.chunk");
    assert_eq!(chunk.choices[0].delta.role.as_deref(), Some("assistant"));
    assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello"));
    assert!(chunk.choices[0].finish_reason.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. Request skips None fields
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_skips_none_fields() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![ChatMessage::User {
            content: MessageContent::Text("hi".into()),
        }],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
    };
    let v = serde_json::to_value(&req).unwrap();
    assert!(v.get("temperature").is_none());
    assert!(v.get("top_p").is_none());
    assert!(v.get("max_tokens").is_none());
    assert!(v.get("stream").is_none());
    assert!(v.get("tools").is_none());
    assert!(v.get("tool_choice").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 21. Response with tool calls from real API format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deserialize_response_with_tool_calls() {
    let json = json!({
        "id": "chatcmpl-tool1",
        "object": "chat.completion",
        "created": 1700000001,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_abc123",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"location\":\"San Francisco\",\"unit\":\"celsius\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 50,
            "completion_tokens": 30,
            "total_tokens": 80
        }
    });
    let resp: ChatCompletionResponse = serde_json::from_value(json).unwrap();
    assert!(resp.choices[0].message.content.is_none());
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].id, "call_abc123");
    assert_eq!(tcs[0].call_type, "function");
    assert_eq!(tcs[0].function.name, "get_weather");
    assert!(tcs[0].function.arguments.contains("San Francisco"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 22. StreamChoice with delta content
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_choice_delta_content() {
    let sc = StreamChoice {
        index: 0,
        delta: StreamDelta {
            role: None,
            content: Some("world".into()),
            tool_calls: None,
        },
        finish_reason: None,
    };
    let v = serde_json::to_value(&sc).unwrap();
    assert_eq!(v["delta"]["content"], "world");
    assert!(v["delta"].get("role").is_none());
    let back: StreamChoice = serde_json::from_value(v).unwrap();
    assert_eq!(sc, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 23. StreamChoice with finish_reason stop
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_choice_stop() {
    let sc = StreamChoice {
        index: 0,
        delta: StreamDelta::default(),
        finish_reason: Some("stop".into()),
    };
    let v = serde_json::to_value(&sc).unwrap();
    assert_eq!(v["finish_reason"], "stop");
    let back: StreamChoice = serde_json::from_value(v).unwrap();
    assert_eq!(sc, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 24. MessageContent::Text variant
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn message_content_text_variant() {
    let mc = MessageContent::Text("hello".into());
    let v = serde_json::to_value(&mc).unwrap();
    assert_eq!(v, json!("hello"));
    let back: MessageContent = serde_json::from_value(v).unwrap();
    assert_eq!(mc, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 25. MessageContent::Parts variant with text
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn message_content_parts_text() {
    let mc = MessageContent::Parts(vec![ContentPart::Text {
        text: "describe this".into(),
    }]);
    let v = serde_json::to_value(&mc).unwrap();
    assert!(v.is_array());
    assert_eq!(v[0]["type"], "text");
    assert_eq!(v[0]["text"], "describe this");
    let back: MessageContent = serde_json::from_value(v).unwrap();
    assert_eq!(mc, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 26. MessageContent::Parts with image_url
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn message_content_parts_image() {
    let mc = MessageContent::Parts(vec![ContentPart::ImageUrl {
        image_url: ImageUrl {
            url: "https://example.com/cat.jpg".into(),
            detail: None,
        },
    }]);
    let v = serde_json::to_value(&mc).unwrap();
    assert_eq!(v[0]["type"], "image_url");
    assert_eq!(v[0]["image_url"]["url"], "https://example.com/cat.jpg");
    assert!(v[0]["image_url"].get("detail").is_none());
    let back: MessageContent = serde_json::from_value(v).unwrap();
    assert_eq!(mc, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 27. Request with all optional fields set
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_all_optional_fields() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![ChatMessage::User {
            content: MessageContent::Text("hi".into()),
        }],
        temperature: Some(0.5),
        top_p: Some(0.8),
        max_tokens: Some(256),
        stream: Some(true),
        tools: Some(vec![Tool {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "f".into(),
                description: "d".into(),
                parameters: json!({}),
            },
        }]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
    };
    let v = serde_json::to_value(&req).unwrap();
    assert!(v.get("temperature").is_some());
    assert!(v.get("top_p").is_some());
    assert!(v.get("max_tokens").is_some());
    assert!(v.get("stream").is_some());
    assert!(v.get("tools").is_some());
    assert!(v.get("tool_choice").is_some());
    let back: ChatCompletionRequest = serde_json::from_value(v).unwrap();
    assert_eq!(req, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 28. Choice roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn choice_serde_roundtrip() {
    let choice = Choice {
        index: 0,
        message: ChoiceMessage {
            role: "assistant".into(),
            content: Some("ok".into()),
            tool_calls: None,
        },
        finish_reason: Some("stop".into()),
    };
    let json = serde_json::to_string(&choice).unwrap();
    let back: Choice = serde_json::from_str(&json).unwrap();
    assert_eq!(choice, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 29. Stream tool call fragment roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_tool_call_fragment_roundtrip() {
    let stc = StreamToolCall {
        index: 0,
        id: Some("call_x".into()),
        call_type: Some("function".into()),
        function: Some(StreamFunctionCall {
            name: Some("exec".into()),
            arguments: Some(r#"{"cmd":"ls"}"#.into()),
        }),
    };
    let v = serde_json::to_value(&stc).unwrap();
    assert_eq!(v["type"], "function");
    assert!(v.get("call_type").is_none());
    let back: StreamToolCall = serde_json::from_value(v).unwrap();
    assert_eq!(stc, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 30. Streaming chunk with tool call delta from real API format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deserialize_stream_chunk_tool_call_delta() {
    let json = json!({
        "id": "chatcmpl-tc",
        "object": "chat.completion.chunk",
        "created": 1700000003,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_99",
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "arguments": "{\"path\":"
                    }
                }]
            },
            "finish_reason": null
        }]
    });
    let chunk: StreamChunk = serde_json::from_value(json).unwrap();
    let tc = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id.as_deref(), Some("call_99"));
    assert_eq!(tc.call_type.as_deref(), Some("function"));
    assert_eq!(
        tc.function.as_ref().unwrap().name.as_deref(),
        Some("read_file")
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 31. Multi-turn conversation with tool use
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_turn_with_tool_use_roundtrip() {
    let messages = vec![
        ChatMessage::System {
            content: "You have tools.".into(),
        },
        ChatMessage::User {
            content: MessageContent::Text("What's the weather?".into()),
        },
        ChatMessage::Assistant {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_w".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "weather".into(),
                    arguments: r#"{"city":"NYC"}"#.into(),
                },
            }]),
        },
        ChatMessage::Tool {
            content: "75°F".into(),
            tool_call_id: "call_w".into(),
        },
        ChatMessage::Assistant {
            content: Some("It's 75°F in NYC.".into()),
            tool_calls: None,
        },
    ];
    let json = serde_json::to_string(&messages).unwrap();
    let back: Vec<ChatMessage> = serde_json::from_str(&json).unwrap();
    assert_eq!(messages, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 32. Request with tools and tool_choice=required
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_with_tools_and_required_choice() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![ChatMessage::User {
            content: MessageContent::Text("Do something".into()),
        }],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: Some(vec![Tool {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "action".into(),
                description: "Do an action".into(),
                parameters: json!({"type": "object"}),
            },
        }]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Required)),
    };
    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["tool_choice"], json!("required"));
    assert_eq!(v["tools"][0]["type"], "function");
    let back: ChatCompletionRequest = serde_json::from_value(v).unwrap();
    assert_eq!(req, back);
}
