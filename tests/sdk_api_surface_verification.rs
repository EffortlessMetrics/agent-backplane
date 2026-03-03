// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive API surface verification tests for all SDK shim crates.
//!
//! Validates that OpenAI, Claude, and Gemini shim crates expose the correct
//! types, serialization formats, builders, error types, and cross-SDK
//! compatibility.

use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// 1. OpenAI API surface (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_chat_completion_request_builder_defaults() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .messages(vec![abp_shim_openai::Message::user("Hello")])
        .build();
    assert_eq!(
        req.model, "gpt-4o",
        "builder should default model to gpt-4o"
    );
    assert_eq!(req.messages.len(), 1);
    assert!(req.temperature.is_none());
    assert!(req.max_tokens.is_none());
    assert!(req.tools.is_none());
    assert!(req.stream.is_none());
    assert!(req.response_format.is_none());
}

#[test]
fn openai_chat_completion_request_builder_full() {
    let tool = abp_shim_openai::Tool::function(
        "get_weather",
        "Get weather",
        json!({"type": "object", "properties": {"city": {"type": "string"}}}),
    );
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4-turbo")
        .messages(vec![
            abp_shim_openai::Message::system("You are helpful"),
            abp_shim_openai::Message::user("What's the weather?"),
        ])
        .tools(vec![tool])
        .tool_choice(abp_shim_openai::ToolChoice::Mode(
            abp_shim_openai::ToolChoiceMode::Auto,
        ))
        .temperature(0.7)
        .max_tokens(1024)
        .stop(vec!["END".into()])
        .stream(true)
        .response_format(abp_shim_openai::ResponseFormat::json_object())
        .build();

    assert_eq!(req.model, "gpt-4-turbo");
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.tools.as_ref().unwrap().len(), 1);
    assert_eq!(req.temperature, Some(0.7));
    assert_eq!(req.max_tokens, Some(1024));
    assert_eq!(req.stop.as_ref().unwrap(), &["END"]);
    assert_eq!(req.stream, Some(true));
}

#[test]
fn openai_response_serde_roundtrip() {
    let resp = abp_shim_openai::ChatCompletionResponse {
        id: "chatcmpl-abc".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![abp_shim_openai::Choice {
            index: 0,
            message: abp_shim_openai::Message::assistant("Hi there!"),
            finish_reason: Some("stop".into()),
        }],
        usage: Some(abp_shim_openai::Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: abp_shim_openai::ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "chatcmpl-abc");
    assert_eq!(back.object, "chat.completion");
    assert_eq!(
        back.choices[0].message.content.as_deref(),
        Some("Hi there!")
    );
    assert_eq!(back.usage.as_ref().unwrap().total_tokens, 15);
}

#[test]
fn openai_streaming_event_format() {
    let event = abp_shim_openai::StreamEvent {
        id: "chunk-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![abp_shim_openai::StreamChoice {
            index: 0,
            delta: abp_shim_openai::Delta {
                role: Some("assistant".into()),
                content: Some("Hello".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["object"], "chat.completion.chunk");
    assert_eq!(json["choices"][0]["delta"]["content"], "Hello");
    assert!(json["choices"][0]["finish_reason"].is_null());
}

#[test]
fn openai_tool_call_format() {
    let tc = abp_shim_openai::ToolCall {
        id: "call_123".into(),
        call_type: "function".into(),
        function: abp_shim_openai::FunctionCall {
            name: "search".into(),
            arguments: r#"{"q":"rust"}"#.into(),
        },
    };
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json["type"], "function");
    assert_eq!(json["function"]["name"], "search");
    // Verify "type" rename from "call_type"
    assert!(json.get("call_type").is_none());
}

#[test]
fn openai_tool_definition_factory() {
    let tool = abp_shim_openai::Tool::function(
        "read_file",
        "Read a file from disk",
        json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
    );
    assert_eq!(tool.tool_type, "function");
    assert_eq!(tool.function.name, "read_file");
    assert_eq!(tool.function.description, "Read a file from disk");

    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["type"], "function");
    assert!(json.get("tool_type").is_none());
}

#[test]
fn openai_token_usage_format() {
    let usage = abp_shim_openai::Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let json = serde_json::to_value(&usage).unwrap();
    assert_eq!(json["prompt_tokens"], 100);
    assert_eq!(json["completion_tokens"], 50);
    assert_eq!(json["total_tokens"], 150);

    let back: abp_shim_openai::Usage = serde_json::from_value(json).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn openai_error_variants() {
    let e1 = abp_shim_openai::ShimError::InvalidRequest("bad request".into());
    assert!(e1.to_string().contains("bad request"));

    let e2 = abp_shim_openai::ShimError::Internal("boom".into());
    assert!(e2.to_string().contains("boom"));

    let e3: abp_shim_openai::ShimError = serde_json::from_str::<i32>("nope").unwrap_err().into();
    assert!(e3.to_string().contains("serde"));
}

#[test]
fn openai_model_name_handling() {
    assert!(abp_openai_sdk::dialect::is_known_model("gpt-4o"));
    assert!(abp_openai_sdk::dialect::is_known_model("gpt-4o-mini"));
    assert!(!abp_openai_sdk::dialect::is_known_model("unknown-model"));

    assert_eq!(
        abp_openai_sdk::dialect::to_canonical_model("gpt-4o"),
        "openai/gpt-4o"
    );
    assert_eq!(
        abp_openai_sdk::dialect::from_canonical_model("openai/gpt-4o"),
        "gpt-4o"
    );
    // Passthrough if prefix absent
    assert_eq!(
        abp_openai_sdk::dialect::from_canonical_model("gpt-4o"),
        "gpt-4o"
    );
}

#[test]
fn openai_message_constructors() {
    let sys = abp_shim_openai::Message::system("Be helpful");
    assert_eq!(sys.role, abp_shim_openai::Role::System);
    assert_eq!(sys.content.as_deref(), Some("Be helpful"));

    let user = abp_shim_openai::Message::user("Hi");
    assert_eq!(user.role, abp_shim_openai::Role::User);

    let asst = abp_shim_openai::Message::assistant("Hello");
    assert_eq!(asst.role, abp_shim_openai::Role::Assistant);
    assert!(asst.tool_calls.is_none());

    let tool = abp_shim_openai::Message::tool("call_1", "result");
    assert_eq!(tool.role, abp_shim_openai::Role::Tool);
    assert_eq!(tool.tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn openai_assistant_with_tool_calls() {
    let tc = abp_shim_openai::ToolCall {
        id: "call_abc".into(),
        call_type: "function".into(),
        function: abp_shim_openai::FunctionCall {
            name: "search".into(),
            arguments: "{}".into(),
        },
    };
    let msg = abp_shim_openai::Message::assistant_with_tool_calls(vec![tc]);
    assert_eq!(msg.role, abp_shim_openai::Role::Assistant);
    assert!(msg.content.is_none());
    assert_eq!(msg.tool_calls.as_ref().unwrap().len(), 1);
}

#[test]
fn openai_response_format_variants() {
    let text = abp_shim_openai::ResponseFormat::text();
    let json_val = serde_json::to_value(&text).unwrap();
    assert_eq!(json_val["type"], "text");

    let json_obj = abp_shim_openai::ResponseFormat::json_object();
    let json_val = serde_json::to_value(&json_obj).unwrap();
    assert_eq!(json_val["type"], "json_object");

    let schema = abp_shim_openai::ResponseFormat::json_schema(
        "person",
        json!({"type": "object", "properties": {"name": {"type": "string"}}}),
    );
    let json_val = serde_json::to_value(&schema).unwrap();
    assert_eq!(json_val["type"], "json_schema");
    assert_eq!(json_val["json_schema"]["name"], "person");
    assert_eq!(json_val["json_schema"]["strict"], true);
}

#[test]
fn openai_streaming_tool_call_format() {
    let stc = abp_shim_openai::StreamToolCall {
        index: 0,
        id: Some("call_xyz".into()),
        call_type: Some("function".into()),
        function: Some(abp_shim_openai::StreamFunctionCall {
            name: Some("calculator".into()),
            arguments: Some(r#"{"expr":"1+1"}"#.into()),
        }),
    };
    let json = serde_json::to_value(&stc).unwrap();
    assert_eq!(json["index"], 0);
    assert_eq!(json["id"], "call_xyz");
    assert_eq!(json["type"], "function");
    assert_eq!(json["function"]["name"], "calculator");
}

#[test]
fn openai_tool_choice_serde() {
    // Mode variant
    let auto = abp_shim_openai::ToolChoice::Mode(abp_shim_openai::ToolChoiceMode::Auto);
    let json = serde_json::to_value(&auto).unwrap();
    assert_eq!(json, json!("auto"));

    let none = abp_shim_openai::ToolChoice::Mode(abp_shim_openai::ToolChoiceMode::None);
    let json = serde_json::to_value(&none).unwrap();
    assert_eq!(json, json!("none"));

    let req = abp_shim_openai::ToolChoice::Mode(abp_shim_openai::ToolChoiceMode::Required);
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json, json!("required"));
}

#[test]
fn openai_request_optional_fields_omitted_in_json() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![abp_shim_openai::Message::user("hi")])
        .build();
    let json = serde_json::to_value(&req).unwrap();
    assert!(
        json.get("tools").is_none(),
        "tools should be skipped when None"
    );
    assert!(
        json.get("temperature").is_none(),
        "temperature should be skipped when None"
    );
    assert!(
        json.get("max_tokens").is_none(),
        "max_tokens should be skipped when None"
    );
    assert!(
        json.get("stream").is_none(),
        "stream should be skipped when None"
    );
    assert!(
        json.get("response_format").is_none(),
        "response_format should be skipped when None"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Claude API surface (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_message_request_construction() {
    let req = abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text {
                text: "Hello".into(),
            }],
        }],
        system: Some("You are helpful".into()),
        temperature: Some(0.5),
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    assert_eq!(req.model, "claude-sonnet-4-20250514");
    assert_eq!(req.max_tokens, 1024);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.system.as_deref(), Some("You are helpful"));
}

#[test]
fn claude_message_request_serde_roundtrip() {
    let req = abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 2048,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text { text: "Hi".into() }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: abp_shim_claude::MessageRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "claude-sonnet-4-20250514");
    assert_eq!(back.max_tokens, 2048);
    // Optional fields should be omitted
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(val.get("system").is_none());
    assert!(val.get("temperature").is_none());
}

#[test]
fn claude_content_blocks_serde() {
    let text = abp_shim_claude::ContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_value(&text).unwrap();
    assert_eq!(json["type"], "text");
    assert_eq!(json["text"], "hello");

    let tool_use = abp_shim_claude::ContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "search".into(),
        input: json!({"query": "rust"}),
    };
    let json = serde_json::to_value(&tool_use).unwrap();
    assert_eq!(json["type"], "tool_use");
    assert_eq!(json["name"], "search");
    assert_eq!(json["input"]["query"], "rust");
}

#[test]
fn claude_tool_result_format() {
    let result = abp_shim_claude::ContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("42".into()),
        is_error: Some(false),
    };
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["type"], "tool_result");
    assert_eq!(json["tool_use_id"], "tu_1");
    assert_eq!(json["content"], "42");
    assert_eq!(json["is_error"], false);

    // Roundtrip
    let back: abp_shim_claude::ContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, result);
}

#[test]
fn claude_thinking_block_format() {
    let thinking = abp_shim_claude::ContentBlock::Thinking {
        thinking: "Let me think about this...".into(),
        signature: Some("sig_abc".into()),
    };
    let json = serde_json::to_value(&thinking).unwrap();
    assert_eq!(json["type"], "thinking");
    assert_eq!(json["thinking"], "Let me think about this...");
    assert_eq!(json["signature"], "sig_abc");

    // Without signature
    let thinking_no_sig = abp_shim_claude::ContentBlock::Thinking {
        thinking: "hmm".into(),
        signature: None,
    };
    let json = serde_json::to_value(&thinking_no_sig).unwrap();
    assert!(json.get("signature").is_none());
}

#[test]
fn claude_thinking_config() {
    use abp_claude_sdk::dialect::ThinkingConfig;
    let cfg = ThinkingConfig::new(10000);
    assert_eq!(cfg.budget_tokens, 10000);
    assert_eq!(cfg.thinking_type, "enabled");

    let json = serde_json::to_value(&cfg).unwrap();
    assert_eq!(json["type"], "enabled");
    assert_eq!(json["budget_tokens"], 10000);
}

#[test]
fn claude_stream_events_format() {
    let ping = abp_shim_claude::StreamEvent::Ping {};
    let json = serde_json::to_value(&ping).unwrap();
    assert_eq!(json["type"], "ping");

    let stop = abp_shim_claude::StreamEvent::MessageStop {};
    let json = serde_json::to_value(&stop).unwrap();
    assert_eq!(json["type"], "message_stop");

    let block_start = abp_shim_claude::StreamEvent::ContentBlockStart {
        index: 0,
        content_block: abp_shim_claude::ContentBlock::Text {
            text: String::new(),
        },
    };
    let json = serde_json::to_value(&block_start).unwrap();
    assert_eq!(json["type"], "content_block_start");
    assert_eq!(json["index"], 0);
}

#[test]
fn claude_stream_delta_types() {
    let text_d = abp_shim_claude::StreamDelta::TextDelta {
        text: "Hello".into(),
    };
    let json = serde_json::to_value(&text_d).unwrap();
    assert_eq!(json["type"], "text_delta");

    let json_d = abp_shim_claude::StreamDelta::InputJsonDelta {
        partial_json: r#"{"key":"#.into(),
    };
    let json = serde_json::to_value(&json_d).unwrap();
    assert_eq!(json["type"], "input_json_delta");

    let think_d = abp_shim_claude::StreamDelta::ThinkingDelta {
        thinking: "reasoning...".into(),
    };
    let json = serde_json::to_value(&think_d).unwrap();
    assert_eq!(json["type"], "thinking_delta");

    let sig_d = abp_shim_claude::StreamDelta::SignatureDelta {
        signature: "sig_partial".into(),
    };
    let json = serde_json::to_value(&sig_d).unwrap();
    assert_eq!(json["type"], "signature_delta");
}

#[test]
fn claude_system_prompt_handling() {
    let with_system = abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text { text: "Hi".into() }],
        }],
        system: Some("Be concise".into()),
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let json = serde_json::to_value(&with_system).unwrap();
    assert_eq!(json["system"], "Be concise");

    let without_system = abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text { text: "Hi".into() }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let json = serde_json::to_value(&without_system).unwrap();
    assert!(
        json.get("system").is_none(),
        "system should be omitted when None"
    );
}

#[test]
fn claude_response_format() {
    let resp = abp_shim_claude::MessageResponse {
        id: "msg_abc".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![abp_shim_claude::ContentBlock::Text {
            text: "Hello!".into(),
        }],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: Some("end_turn".into()),
        stop_sequence: None,
        usage: abp_shim_claude::Usage {
            input_tokens: 10,
            output_tokens: 25,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["type"], "message");
    assert_eq!(json["role"], "assistant");
    assert_eq!(json["content"][0]["type"], "text");
    assert_eq!(json["usage"]["input_tokens"], 10);
    assert_eq!(json["stop_reason"], "end_turn");
}

#[test]
fn claude_usage_with_cache() {
    let usage = abp_shim_claude::Usage {
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: Some(20),
        cache_read_input_tokens: Some(80),
    };
    let json = serde_json::to_value(&usage).unwrap();
    assert_eq!(json["cache_creation_input_tokens"], 20);
    assert_eq!(json["cache_read_input_tokens"], 80);

    let back: abp_shim_claude::Usage = serde_json::from_value(json).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn claude_usage_without_cache_omits_fields() {
    let usage = abp_shim_claude::Usage {
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let json = serde_json::to_value(&usage).unwrap();
    assert!(json.get("cache_creation_input_tokens").is_none());
    assert!(json.get("cache_read_input_tokens").is_none());
}

#[test]
fn claude_error_types() {
    let e1 = abp_shim_claude::ShimError::InvalidRequest("empty messages".into());
    assert!(e1.to_string().contains("empty messages"));

    let e2 = abp_shim_claude::ShimError::ApiError {
        error_type: "invalid_request_error".into(),
        message: "max_tokens too large".into(),
    };
    assert!(e2.to_string().contains("invalid_request_error"));
    assert!(e2.to_string().contains("max_tokens too large"));

    let e3 = abp_shim_claude::ShimError::Internal("conversion failed".into());
    assert!(e3.to_string().contains("conversion failed"));
}

#[test]
fn claude_api_error_serde() {
    let err = abp_shim_claude::ApiError {
        error_type: "overloaded_error".into(),
        message: "API overloaded".into(),
    };
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["type"], "overloaded_error");
    assert_eq!(json["message"], "API overloaded");
    assert!(
        json.get("error_type").is_none(),
        "should be renamed to 'type'"
    );
}

#[test]
fn claude_model_name_handling() {
    assert!(abp_claude_sdk::dialect::is_known_model(
        "claude-sonnet-4-20250514"
    ));
    assert!(abp_claude_sdk::dialect::is_known_model(
        "claude-opus-4-20250514"
    ));
    assert!(!abp_claude_sdk::dialect::is_known_model("gpt-4o"));

    assert_eq!(
        abp_claude_sdk::dialect::to_canonical_model("claude-sonnet-4-20250514"),
        "anthropic/claude-sonnet-4-20250514"
    );
    assert_eq!(
        abp_claude_sdk::dialect::from_canonical_model("anthropic/claude-sonnet-4-20250514"),
        "claude-sonnet-4-20250514"
    );
}

#[test]
fn claude_image_source_serde() {
    let b64 = abp_shim_claude::ImageSource::Base64 {
        media_type: "image/png".into(),
        data: "iVBOR...".into(),
    };
    let json = serde_json::to_value(&b64).unwrap();
    assert_eq!(json["type"], "base64");
    assert_eq!(json["media_type"], "image/png");

    let url = abp_shim_claude::ImageSource::Url {
        url: "https://example.com/img.png".into(),
    };
    let json = serde_json::to_value(&url).unwrap();
    assert_eq!(json["type"], "url");
    assert_eq!(json["url"], "https://example.com/img.png");
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Gemini API surface (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_generate_content_request_builder() {
    let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash").add_content(
        abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("Hello")]),
    );
    assert_eq!(req.model, "gemini-2.5-flash");
    assert_eq!(req.contents.len(), 1);
    assert!(req.system_instruction.is_none());
    assert!(req.generation_config.is_none());
    assert!(req.safety_settings.is_none());
    assert!(req.tools.is_none());
}

#[test]
fn gemini_request_full_builder() {
    let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-pro")
        .add_content(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text("Hi"),
        ]))
        .system_instruction(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text("Be brief"),
        ]))
        .generation_config(abp_shim_gemini::GenerationConfig {
            max_output_tokens: Some(1024),
            temperature: Some(0.5),
            top_p: Some(0.9),
            top_k: Some(40),
            stop_sequences: Some(vec!["STOP".into()]),
            response_mime_type: None,
            response_schema: None,
        })
        .safety_settings(vec![abp_shim_gemini::SafetySetting {
            category: abp_shim_gemini::HarmCategory::HarmCategoryHarassment,
            threshold: abp_shim_gemini::HarmBlockThreshold::BlockMediumAndAbove,
        }])
        .tools(vec![abp_shim_gemini::ToolDeclaration {
            function_declarations: vec![abp_shim_gemini::FunctionDeclaration {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: json!({"type": "object"}),
            }],
        }])
        .tool_config(abp_shim_gemini::ToolConfig {
            function_calling_config: abp_shim_gemini::FunctionCallingConfig {
                mode: abp_shim_gemini::FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        });

    assert_eq!(req.model, "gemini-2.5-pro");
    assert!(req.system_instruction.is_some());
    assert!(req.generation_config.is_some());
    assert_eq!(req.safety_settings.as_ref().unwrap().len(), 1);
    assert_eq!(req.tools.as_ref().unwrap().len(), 1);
    assert!(req.tool_config.is_some());
}

#[test]
fn gemini_parts_serde() {
    let text = abp_shim_gemini::Part::text("hello");
    let json = serde_json::to_value(&text).unwrap();
    assert_eq!(json["text"], "hello");

    let data = abp_shim_gemini::Part::inline_data("image/png", "base64data");
    let json = serde_json::to_value(&data).unwrap();
    assert_eq!(json["inlineData"]["mime_type"], "image/png");
    assert_eq!(json["inlineData"]["data"], "base64data");

    let fc = abp_shim_gemini::Part::function_call("search", json!({"q": "test"}));
    let json = serde_json::to_value(&fc).unwrap();
    assert_eq!(json["functionCall"]["name"], "search");
    assert_eq!(json["functionCall"]["args"]["q"], "test");

    let fr = abp_shim_gemini::Part::function_response("search", json!({"results": []}));
    let json = serde_json::to_value(&fr).unwrap();
    assert_eq!(json["functionResponse"]["name"], "search");
}

#[test]
fn gemini_content_roles() {
    let user = abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("hi")]);
    assert_eq!(user.role, "user");

    let model = abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::text("hello")]);
    assert_eq!(model.role, "model");
}

#[test]
fn gemini_function_declaration_format() {
    let decl = abp_shim_gemini::FunctionDeclaration {
        name: "get_weather".into(),
        description: "Get current weather".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "location": {"type": "string"}
            },
            "required": ["location"]
        }),
    };
    let tool = abp_shim_gemini::ToolDeclaration {
        function_declarations: vec![decl],
    };
    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["functionDeclarations"][0]["name"], "get_weather");
    assert_eq!(
        json["functionDeclarations"][0]["description"],
        "Get current weather"
    );
}

#[test]
fn gemini_safety_settings_serde() {
    let setting = abp_shim_gemini::SafetySetting {
        category: abp_shim_gemini::HarmCategory::HarmCategoryHateSpeech,
        threshold: abp_shim_gemini::HarmBlockThreshold::BlockOnlyHigh,
    };
    let json = serde_json::to_value(&setting).unwrap();
    assert_eq!(json["category"], "HARM_CATEGORY_HATE_SPEECH");
    assert_eq!(json["threshold"], "BLOCK_ONLY_HIGH");

    let back: abp_shim_gemini::SafetySetting = serde_json::from_value(json).unwrap();
    assert_eq!(back, setting);
}

#[test]
fn gemini_generation_config_camel_case() {
    let config = abp_shim_gemini::GenerationConfig {
        max_output_tokens: Some(2048),
        temperature: Some(0.8),
        top_p: Some(0.95),
        top_k: Some(50),
        stop_sequences: Some(vec!["END".into()]),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(json!({"type": "object"})),
    };
    let json = serde_json::to_value(&config).unwrap();
    assert_eq!(json["maxOutputTokens"], 2048);
    assert_eq!(json["temperature"], 0.8);
    assert_eq!(json["topP"], 0.95);
    assert_eq!(json["topK"], 50);
    assert_eq!(json["stopSequences"], json!(["END"]));
    assert_eq!(json["responseMimeType"], "application/json");
    assert!(json["responseSchema"].is_object());
}

#[test]
fn gemini_generation_config_optional_fields_omitted() {
    let config = abp_shim_gemini::GenerationConfig {
        max_output_tokens: None,
        temperature: Some(0.5),
        top_p: None,
        top_k: None,
        stop_sequences: None,
        response_mime_type: None,
        response_schema: None,
    };
    let json = serde_json::to_value(&config).unwrap();
    assert!(json.get("maxOutputTokens").is_none());
    assert_eq!(json["temperature"], 0.5);
    assert!(json.get("topP").is_none());
    assert!(json.get("topK").is_none());
}

#[test]
fn gemini_usage_metadata_format() {
    let usage = abp_shim_gemini::UsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 50,
        total_token_count: 150,
    };
    let json = serde_json::to_value(&usage).unwrap();
    assert_eq!(json["promptTokenCount"], 100);
    assert_eq!(json["candidatesTokenCount"], 50);
    assert_eq!(json["totalTokenCount"], 150);

    let back: abp_shim_gemini::UsageMetadata = serde_json::from_value(json).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn gemini_response_text_extraction() {
    let resp = abp_shim_gemini::GenerateContentResponse {
        candidates: vec![abp_shim_gemini::Candidate {
            content: abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::Text(
                "The answer is 42".into(),
            )]),
            finish_reason: Some("STOP".into()),
        }],
        usage_metadata: None,
    };
    assert_eq!(resp.text(), Some("The answer is 42"));
}

#[test]
fn gemini_response_function_calls_extraction() {
    let resp = abp_shim_gemini::GenerateContentResponse {
        candidates: vec![abp_shim_gemini::Candidate {
            content: abp_shim_gemini::Content::model(vec![
                abp_shim_gemini::Part::function_call("search", json!({"q": "rust"})),
                abp_shim_gemini::Part::function_call("calc", json!({"expr": "1+1"})),
            ]),
            finish_reason: Some("STOP".into()),
        }],
        usage_metadata: None,
    };
    let calls = resp.function_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "search");
    assert_eq!(calls[1].0, "calc");
}

#[test]
fn gemini_model_name_handling() {
    assert!(abp_gemini_sdk::dialect::is_known_model("gemini-2.5-flash"));
    assert!(abp_gemini_sdk::dialect::is_known_model("gemini-2.5-pro"));
    assert!(!abp_gemini_sdk::dialect::is_known_model("gpt-4o"));

    assert_eq!(
        abp_gemini_sdk::dialect::to_canonical_model("gemini-2.5-flash"),
        "google/gemini-2.5-flash"
    );
    assert_eq!(
        abp_gemini_sdk::dialect::from_canonical_model("google/gemini-2.5-flash"),
        "gemini-2.5-flash"
    );
}

#[test]
fn gemini_error_variants() {
    let e1 = abp_shim_gemini::GeminiError::RequestConversion("invalid parts".into());
    assert!(e1.to_string().contains("invalid parts"));

    let e2 = abp_shim_gemini::GeminiError::ResponseConversion("bad candidate".into());
    assert!(e2.to_string().contains("bad candidate"));

    let e3 = abp_shim_gemini::GeminiError::BackendError("timeout".into());
    assert!(e3.to_string().contains("timeout"));

    let e4: abp_shim_gemini::GeminiError = serde_json::from_str::<i32>("x").unwrap_err().into();
    assert!(e4.to_string().contains("serde"));
}

#[test]
fn gemini_tool_config_serde() {
    let tc = abp_shim_gemini::ToolConfig {
        function_calling_config: abp_shim_gemini::FunctionCallingConfig {
            mode: abp_shim_gemini::FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["search".into(), "read".into()]),
        },
    };
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json["functionCallingConfig"]["mode"], "ANY");
    assert_eq!(
        json["functionCallingConfig"]["allowedFunctionNames"],
        json!(["search", "read"])
    );
}

#[test]
fn gemini_stream_event_text_extraction() {
    let ev = abp_shim_gemini::StreamEvent {
        candidates: vec![abp_shim_gemini::Candidate {
            content: abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::Text(
                "delta text".into(),
            )]),
            finish_reason: None,
        }],
        usage_metadata: None,
    };
    assert_eq!(ev.text(), Some("delta text"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Cross-SDK API compatibility (15+ tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cross_sdk_same_prompt_different_request_formats() {
    let prompt = "What is the capital of France?";

    // OpenAI format
    let openai_req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![abp_shim_openai::Message::user(prompt)])
        .build();
    let openai_json = serde_json::to_value(&openai_req).unwrap();
    assert_eq!(openai_json["messages"][0]["role"], "user");
    assert_eq!(openai_json["messages"][0]["content"], prompt);

    // Claude format
    let claude_req = abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text {
                text: prompt.into(),
            }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let claude_json = serde_json::to_value(&claude_req).unwrap();
    assert_eq!(claude_json["messages"][0]["role"], "user");
    assert_eq!(claude_json["messages"][0]["content"][0]["text"], prompt);

    // Gemini format
    let gemini_req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash").add_content(
        abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text(prompt)]),
    );
    let gemini_json = serde_json::to_value(&gemini_req).unwrap();
    assert_eq!(gemini_json["contents"][0]["role"], "user");
    assert_eq!(gemini_json["contents"][0]["parts"][0]["text"], prompt);
}

#[test]
fn cross_sdk_same_tool_different_formats() {
    let tool_name = "get_weather";
    let tool_desc = "Get current weather for a location";
    let params = json!({
        "type": "object",
        "properties": {
            "location": {"type": "string", "description": "City name"}
        },
        "required": ["location"]
    });

    // OpenAI
    let openai_tool = abp_shim_openai::Tool::function(tool_name, tool_desc, params.clone());
    let openai_json = serde_json::to_value(&openai_tool).unwrap();
    assert_eq!(openai_json["type"], "function");
    assert_eq!(openai_json["function"]["name"], tool_name);
    assert_eq!(openai_json["function"]["description"], tool_desc);

    // Gemini
    let gemini_tool = abp_shim_gemini::ToolDeclaration {
        function_declarations: vec![abp_shim_gemini::FunctionDeclaration {
            name: tool_name.into(),
            description: tool_desc.into(),
            parameters: params.clone(),
        }],
    };
    let gemini_json = serde_json::to_value(&gemini_tool).unwrap();
    assert_eq!(gemini_json["functionDeclarations"][0]["name"], tool_name);
    assert_eq!(
        gemini_json["functionDeclarations"][0]["description"],
        tool_desc
    );

    // Both carry the same parameter schema
    assert_eq!(openai_json["function"]["parameters"], params);
    assert_eq!(gemini_json["functionDeclarations"][0]["parameters"], params);
}

#[test]
fn cross_sdk_tool_call_response_formats() {
    let tool_name = "calculator";
    let tool_args = json!({"expression": "2+2"});

    // OpenAI tool call
    let openai_tc = abp_shim_openai::ToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: abp_shim_openai::FunctionCall {
            name: tool_name.into(),
            arguments: serde_json::to_string(&tool_args).unwrap(),
        },
    };
    let openai_json = serde_json::to_value(&openai_tc).unwrap();
    assert_eq!(openai_json["function"]["name"], tool_name);

    // Claude tool use
    let claude_tu = abp_shim_claude::ContentBlock::ToolUse {
        id: "tu_1".into(),
        name: tool_name.into(),
        input: tool_args.clone(),
    };
    let claude_json = serde_json::to_value(&claude_tu).unwrap();
    assert_eq!(claude_json["name"], tool_name);

    // Gemini function call
    let gemini_fc = abp_shim_gemini::Part::function_call(tool_name, tool_args.clone());
    let gemini_json = serde_json::to_value(&gemini_fc).unwrap();
    assert_eq!(gemini_json["functionCall"]["name"], tool_name);

    // OpenAI passes arguments as string, Claude as object, Gemini as object
    assert!(openai_json["function"]["arguments"].is_string());
    assert!(claude_json["input"].is_object());
    assert!(gemini_json["functionCall"]["args"].is_object());
}

#[test]
fn cross_sdk_response_text_extraction() {
    let text = "The capital of France is Paris.";

    // OpenAI
    let openai_resp = abp_shim_openai::ChatCompletionResponse {
        id: "chatcmpl-1".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![abp_shim_openai::Choice {
            index: 0,
            message: abp_shim_openai::Message::assistant(text),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    };
    assert_eq!(
        openai_resp.choices[0].message.content.as_deref(),
        Some(text)
    );

    // Claude
    let claude_resp = abp_shim_claude::MessageResponse {
        id: "msg_1".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![abp_shim_claude::ContentBlock::Text { text: text.into() }],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: Some("end_turn".into()),
        stop_sequence: None,
        usage: abp_shim_claude::Usage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    };
    let claude_text = match &claude_resp.content[0] {
        abp_shim_claude::ContentBlock::Text { text } => text.as_str(),
        _ => panic!("expected text block"),
    };
    assert_eq!(claude_text, text);

    // Gemini
    let gemini_resp = abp_shim_gemini::GenerateContentResponse {
        candidates: vec![abp_shim_gemini::Candidate {
            content: abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::Text(
                text.into(),
            )]),
            finish_reason: Some("STOP".into()),
        }],
        usage_metadata: None,
    };
    assert_eq!(gemini_resp.text(), Some(text));
}

#[test]
fn cross_sdk_usage_token_formats() {
    // OpenAI: prompt_tokens, completion_tokens, total_tokens
    let openai_usage = abp_shim_openai::Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let oj = serde_json::to_value(&openai_usage).unwrap();
    assert!(oj.get("prompt_tokens").is_some());
    assert!(oj.get("completion_tokens").is_some());

    // Claude: input_tokens, output_tokens
    let claude_usage = abp_shim_claude::Usage {
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let cj = serde_json::to_value(&claude_usage).unwrap();
    assert!(cj.get("input_tokens").is_some());
    assert!(cj.get("output_tokens").is_some());

    // Gemini: promptTokenCount, candidatesTokenCount, totalTokenCount (camelCase)
    let gemini_usage = abp_shim_gemini::UsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 50,
        total_token_count: 150,
    };
    let gj = serde_json::to_value(&gemini_usage).unwrap();
    assert!(gj.get("promptTokenCount").is_some());
    assert!(gj.get("candidatesTokenCount").is_some());
}

#[test]
fn cross_sdk_system_prompt_patterns() {
    // OpenAI: system message in messages array
    let openai_req = abp_shim_openai::ChatCompletionRequest::builder()
        .messages(vec![
            abp_shim_openai::Message::system("Be helpful"),
            abp_shim_openai::Message::user("Hi"),
        ])
        .build();
    let oj = serde_json::to_value(&openai_req).unwrap();
    assert_eq!(oj["messages"][0]["role"], "system");
    assert_eq!(oj["messages"][0]["content"], "Be helpful");

    // Claude: separate system field
    let claude_req = abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text { text: "Hi".into() }],
        }],
        system: Some("Be helpful".into()),
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let cj = serde_json::to_value(&claude_req).unwrap();
    assert_eq!(cj["system"], "Be helpful");
    assert_eq!(cj["messages"][0]["role"], "user");

    // Gemini: system_instruction field
    let gemini_req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text("Hi"),
        ]))
        .system_instruction(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text("Be helpful"),
        ]));
    let gj = serde_json::to_value(&gemini_req).unwrap();
    assert!(gj.get("system_instruction").is_some() || gj.get("systemInstruction").is_some());
}

#[test]
fn cross_sdk_stop_reason_formats() {
    // OpenAI: "stop", "tool_calls", "length", "content_filter"
    let openai_choice = abp_shim_openai::Choice {
        index: 0,
        message: abp_shim_openai::Message::assistant("done"),
        finish_reason: Some("stop".into()),
    };
    assert_eq!(openai_choice.finish_reason.as_deref(), Some("stop"));

    // Claude: "end_turn", "tool_use", "max_tokens", "stop_sequence"
    let claude_resp = abp_shim_claude::MessageResponse {
        id: "msg_1".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![abp_shim_claude::ContentBlock::Text {
            text: "done".into(),
        }],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: Some("end_turn".into()),
        stop_sequence: None,
        usage: abp_shim_claude::Usage {
            input_tokens: 5,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    };
    assert_eq!(claude_resp.stop_reason.as_deref(), Some("end_turn"));

    // Gemini: "STOP"
    let gemini_candidate = abp_shim_gemini::Candidate {
        content: abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::text("done")]),
        finish_reason: Some("STOP".into()),
    };
    assert_eq!(gemini_candidate.finish_reason.as_deref(), Some("STOP"));
}

#[test]
fn cross_sdk_role_naming() {
    // OpenAI roles
    let roles_openai = [
        abp_shim_openai::Role::System,
        abp_shim_openai::Role::User,
        abp_shim_openai::Role::Assistant,
        abp_shim_openai::Role::Tool,
    ];
    let serialized: Vec<String> = roles_openai
        .iter()
        .map(|r| {
            serde_json::to_value(r)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(serialized, ["system", "user", "assistant", "tool"]);

    // Claude roles
    let roles_claude = [
        abp_shim_claude::Role::User,
        abp_shim_claude::Role::Assistant,
    ];
    let serialized: Vec<String> = roles_claude
        .iter()
        .map(|r| {
            serde_json::to_value(r)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(serialized, ["user", "assistant"]);

    // Gemini uses string roles
    let user_content = abp_shim_gemini::Content::user(vec![]);
    assert_eq!(user_content.role, "user");
    let model_content = abp_shim_gemini::Content::model(vec![]);
    assert_eq!(model_content.role, "model");
}

#[test]
fn cross_sdk_model_name_canonical_roundtrip() {
    // OpenAI
    let openai_canonical = abp_openai_sdk::dialect::to_canonical_model("gpt-4o");
    let openai_back = abp_openai_sdk::dialect::from_canonical_model(&openai_canonical);
    assert_eq!(openai_back, "gpt-4o");

    // Claude
    let claude_canonical = abp_claude_sdk::dialect::to_canonical_model("claude-sonnet-4-20250514");
    let claude_back = abp_claude_sdk::dialect::from_canonical_model(&claude_canonical);
    assert_eq!(claude_back, "claude-sonnet-4-20250514");

    // Gemini
    let gemini_canonical = abp_gemini_sdk::dialect::to_canonical_model("gemini-2.5-flash");
    let gemini_back = abp_gemini_sdk::dialect::from_canonical_model(&gemini_canonical);
    assert_eq!(gemini_back, "gemini-2.5-flash");

    // All canonical forms use different prefixes
    assert!(openai_canonical.starts_with("openai/"));
    assert!(claude_canonical.starts_with("anthropic/"));
    assert!(gemini_canonical.starts_with("google/"));
}

#[test]
fn cross_sdk_streaming_object_types() {
    // OpenAI streaming uses "chat.completion.chunk"
    let openai_stream = abp_shim_openai::StreamEvent {
        id: "x".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
    assert_eq!(openai_stream.object, "chat.completion.chunk");

    // Claude streaming uses typed enum variants
    let claude_ping = abp_shim_claude::StreamEvent::Ping {};
    let json = serde_json::to_value(&claude_ping).unwrap();
    assert_eq!(json["type"], "ping");

    // Gemini streaming has candidates array
    let gemini_stream = abp_shim_gemini::StreamEvent {
        candidates: vec![],
        usage_metadata: None,
    };
    assert!(gemini_stream.candidates.is_empty());
}

#[test]
fn cross_sdk_error_types_coverage() {
    // OpenAI: InvalidRequest, Internal, Serde
    let _: abp_shim_openai::ShimError = abp_shim_openai::ShimError::InvalidRequest("test".into());
    let _: abp_shim_openai::ShimError = abp_shim_openai::ShimError::Internal("test".into());

    // Claude: InvalidRequest, ApiError, Internal
    let _: abp_shim_claude::ShimError = abp_shim_claude::ShimError::InvalidRequest("test".into());
    let _: abp_shim_claude::ShimError = abp_shim_claude::ShimError::ApiError {
        error_type: "invalid_request_error".into(),
        message: "test".into(),
    };
    let _: abp_shim_claude::ShimError = abp_shim_claude::ShimError::Internal("test".into());

    // Gemini: RequestConversion, ResponseConversion, BackendError, Serde
    let _: abp_shim_gemini::GeminiError =
        abp_shim_gemini::GeminiError::RequestConversion("test".into());
    let _: abp_shim_gemini::GeminiError =
        abp_shim_gemini::GeminiError::ResponseConversion("test".into());
    let _: abp_shim_gemini::GeminiError = abp_shim_gemini::GeminiError::BackendError("test".into());
}

#[test]
fn cross_sdk_default_models() {
    assert_eq!(abp_openai_sdk::dialect::DEFAULT_MODEL, "gpt-4o");
    assert_eq!(
        abp_claude_sdk::dialect::DEFAULT_MODEL,
        "claude-sonnet-4-20250514"
    );
    assert_eq!(abp_gemini_sdk::dialect::DEFAULT_MODEL, "gemini-2.5-flash");
}

#[test]
fn cross_sdk_dialect_versions() {
    assert_eq!(abp_openai_sdk::dialect::DIALECT_VERSION, "openai/v0.1");
    assert_eq!(abp_claude_sdk::dialect::DIALECT_VERSION, "claude/v0.1");
    assert_eq!(abp_gemini_sdk::dialect::DIALECT_VERSION, "gemini/v0.1");
}

#[test]
fn cross_sdk_request_serde_roundtrip() {
    // Verify all three request types survive a full JSON roundtrip.
    let openai_req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![abp_shim_openai::Message::user("test")])
        .temperature(0.5)
        .build();
    let json = serde_json::to_string(&openai_req).unwrap();
    let _: abp_shim_openai::ChatCompletionRequest = serde_json::from_str(&json).unwrap();

    let claude_req = abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text {
                text: "test".into(),
            }],
        }],
        system: None,
        temperature: Some(0.5),
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let json = serde_json::to_string(&claude_req).unwrap();
    let _: abp_shim_claude::MessageRequest = serde_json::from_str(&json).unwrap();

    let gemini_req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash").add_content(
        abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("test")]),
    );
    let json = serde_json::to_string(&gemini_req).unwrap();
    let _: abp_shim_gemini::GenerateContentRequest = serde_json::from_str(&json).unwrap();
}

#[test]
fn cross_sdk_capability_manifests() {
    let openai_caps = abp_openai_sdk::dialect::capability_manifest();
    let claude_caps = abp_claude_sdk::dialect::capability_manifest();
    let gemini_caps = abp_gemini_sdk::dialect::capability_manifest();

    // All SDKs should support streaming
    use abp_core::Capability;
    assert!(
        openai_caps.contains_key(&Capability::Streaming),
        "OpenAI should have streaming capability"
    );
    assert!(
        claude_caps.contains_key(&Capability::Streaming),
        "Claude should have streaming capability"
    );
    assert!(
        gemini_caps.contains_key(&Capability::Streaming),
        "Gemini should have streaming capability"
    );

    // All SDKs should have at least some tool-related capabilities
    assert!(
        openai_caps.contains_key(&Capability::ToolRead),
        "OpenAI should have tool capabilities"
    );
    assert!(
        claude_caps.contains_key(&Capability::ToolRead),
        "Claude should have tool capabilities"
    );
    assert!(
        gemini_caps.contains_key(&Capability::ToolRead),
        "Gemini should have tool capabilities"
    );
}
