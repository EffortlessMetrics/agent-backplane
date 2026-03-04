#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! SDK surface fidelity tests — verify each shim preserves the exact vendor API surface.
//!
//! Each vendor block tests:
//! 1. Request builder API — all methods exist and return correct types
//! 2. Response structure — matches vendor's response format
//! 3. Streaming — event types match vendor's SSE event names
//! 4. Error types — error codes/messages match vendor conventions
//! 5. Auth — header names and format match exactly
//! 6. Model names — all supported model identifiers accepted
//! 7. Content types — text, image, tool_use, tool_result match vendor format
//! 8. Metadata — usage fields, token counts, finish reasons match

use serde_json::json;

// ==========================================================================
// OpenAI SDK surface
// ==========================================================================
mod openai {
    use super::*;
    use abp_shim_openai::{
        ChatCompletionRequest, ChatCompletionResponse, Choice, Delta, FunctionCall, Message,
        ResponseFormat, Role, StreamEvent, StreamFunctionCall, StreamToolCall, Tool, ToolCall,
        ToolChoice, ToolChoiceMode, Usage,
    };

    // ── 1. Request builder ──────────────────────────────────────────────

    #[test]
    fn builder_default_model_is_gpt4o() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("hello")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn builder_all_fields() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("hi")])
            .temperature(0.7)
            .max_tokens(100)
            .stop(vec!["END".into()])
            .stream(true)
            .tools(vec![Tool::function("test", "desc", json!({}))])
            .tool_choice(ToolChoice::Mode(ToolChoiceMode::Auto))
            .response_format(ResponseFormat::Text)
            .build();
        assert_eq!(req.model, "gpt-4-turbo");
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.max_tokens, Some(100));
        assert_eq!(req.stop.as_ref().unwrap()[0], "END");
        assert_eq!(req.stream, Some(true));
        assert!(req.tools.is_some());
        assert!(req.tool_choice.is_some());
        assert!(req.response_format.is_some());
    }

    // ── 2. Response structure ───────────────────────────────────────────

    #[test]
    fn response_object_is_chat_completion() {
        let resp: ChatCompletionResponse = serde_json::from_value(json!({
            "id": "chatcmpl-abc",
            "object": "chat.completion",
            "created": 1700000000u64,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "hi"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
        }))
        .unwrap();
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ── 3. Streaming ────────────────────────────────────────────────────

    #[test]
    fn stream_event_object_is_chunk() {
        let chunk: StreamEvent = serde_json::from_value(json!({
            "id": "chatcmpl-abc",
            "object": "chat.completion.chunk",
            "created": 1700000000u64,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "delta": {"content": "hi"},
                "finish_reason": null
            }]
        }))
        .unwrap();
        assert_eq!(chunk.object, "chat.completion.chunk");
    }

    #[test]
    fn stream_delta_role_and_content() {
        let delta = Delta {
            role: Some("assistant".into()),
            content: Some("Hello".into()),
            tool_calls: None,
        };
        let json = serde_json::to_value(&delta).unwrap();
        assert_eq!(json["role"], "assistant");
        assert_eq!(json["content"], "Hello");
    }

    #[test]
    fn stream_tool_call_fragment() {
        let tc = StreamToolCall {
            index: 0,
            id: Some("call_1".into()),
            call_type: Some("function".into()),
            function: Some(StreamFunctionCall {
                name: Some("get_weather".into()),
                arguments: Some(r#"{"loc":"NY"}"#.into()),
            }),
        };
        let json = serde_json::to_value(&tc).unwrap();
        assert_eq!(json["type"], "function");
    }

    // ── 4. Error types ──────────────────────────────────────────────────

    #[test]
    fn shim_error_variants() {
        let e = abp_shim_openai::ShimError::InvalidRequest("bad".into());
        assert!(e.to_string().contains("invalid request"));
        let e = abp_shim_openai::ShimError::Internal("oops".into());
        assert!(e.to_string().contains("internal error"));
    }

    #[test]
    fn client_error_api_variant() {
        let e = abp_shim_openai::client::ClientError::Api {
            status: 429,
            body: "rate limited".into(),
        };
        assert!(e.to_string().contains("429"));
    }

    // ── 5. Auth ─────────────────────────────────────────────────────────

    #[test]
    fn client_default_base_url() {
        let c = abp_shim_openai::client::Client::new("sk-test").unwrap();
        assert_eq!(c.base_url(), "https://api.openai.com/v1");
    }

    #[test]
    fn client_builder_custom_base_url() {
        let c = abp_shim_openai::client::Client::builder("sk-test")
            .base_url("https://custom.openai.com")
            .build()
            .unwrap();
        assert_eq!(c.base_url(), "https://custom.openai.com");
    }

    // ── 6. Model names ──────────────────────────────────────────────────

    #[test]
    fn accepts_all_openai_models() {
        for model in &[
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4-turbo",
            "gpt-4",
            "gpt-3.5-turbo",
            "o1",
            "o1-mini",
            "o1-preview",
        ] {
            let req = ChatCompletionRequest::builder()
                .model(*model)
                .messages(vec![Message::user("hi")])
                .build();
            assert_eq!(req.model, *model);
        }
    }

    // ── 7. Content types ────────────────────────────────────────────────

    #[test]
    fn message_roles_serialize_snake_case() {
        assert_eq!(serde_json::to_value(Role::System).unwrap(), "system");
        assert_eq!(serde_json::to_value(Role::User).unwrap(), "user");
        assert_eq!(serde_json::to_value(Role::Assistant).unwrap(), "assistant");
        assert_eq!(serde_json::to_value(Role::Tool).unwrap(), "tool");
    }

    #[test]
    fn message_constructors_set_correct_role() {
        assert_eq!(Message::system("x").role, Role::System);
        assert_eq!(Message::user("x").role, Role::User);
        assert_eq!(Message::assistant("x").role, Role::Assistant);
        assert_eq!(Message::tool("id", "x").role, Role::Tool);
    }

    #[test]
    fn tool_call_type_field_is_function() {
        let tc = ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "f".into(),
                arguments: "{}".into(),
            },
        };
        let json = serde_json::to_value(&tc).unwrap();
        assert_eq!(json["type"], "function");
    }

    #[test]
    fn tool_definition_type_field() {
        let t = Tool::function("search", "Search the web", json!({"type": "object"}));
        assert_eq!(t.tool_type, "function");
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(json["type"], "function");
    }

    // ── 8. Metadata ─────────────────────────────────────────────────────

    #[test]
    fn usage_fields_match_openai_convention() {
        let u = Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        };
        let json = serde_json::to_value(&u).unwrap();
        assert!(json.get("prompt_tokens").is_some());
        assert!(json.get("completion_tokens").is_some());
        assert!(json.get("total_tokens").is_some());
    }

    #[test]
    fn finish_reason_stop_and_tool_calls() {
        let choice_stop: Choice = serde_json::from_value(json!({
            "index": 0,
            "message": {"role": "assistant", "content": "done"},
            "finish_reason": "stop"
        }))
        .unwrap();
        assert_eq!(choice_stop.finish_reason.as_deref(), Some("stop"));

        let choice_tc: Choice = serde_json::from_value(json!({
            "index": 0,
            "message": {"role": "assistant", "tool_calls": [{
                "id": "call_1", "type": "function",
                "function": {"name": "f", "arguments": "{}"}
            }]},
            "finish_reason": "tool_calls"
        }))
        .unwrap();
        assert_eq!(choice_tc.finish_reason.as_deref(), Some("tool_calls"));
    }
}

// ==========================================================================
// Claude SDK surface
// ==========================================================================
mod claude {
    use super::*;
    use abp_shim_claude::{
        ApiError, ContentBlock, EventStream, ImageSource, Message, MessageRequest, MessageResponse,
        Role, ShimError, StreamDelta, StreamEvent, Usage,
    };

    // ── 1. Request builder ──────────────────────────────────────────────

    #[test]
    fn message_request_fields() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".into(),
                }],
            }],
            system: Some("Be helpful".into()),
            temperature: Some(0.5),
            stop_sequences: Some(vec!["STOP".into()]),
            thinking: None,
            stream: Some(false),
        };
        assert_eq!(req.model, "claude-sonnet-4-20250514");
        assert_eq!(req.max_tokens, 1024);
        assert!(req.system.is_some());
    }

    // ── 2. Response structure ───────────────────────────────────────────

    #[test]
    fn response_type_field_is_message() {
        let resp = MessageResponse {
            id: "msg_abc".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text { text: "hi".into() }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["type"], "message");
        assert_eq!(json["role"], "assistant");
    }

    // ── 3. Streaming ────────────────────────────────────────────────────

    #[test]
    fn stream_event_variants_serialize_correctly() {
        let ping = StreamEvent::Ping {};
        let json = serde_json::to_value(&ping).unwrap();
        assert_eq!(json["type"], "ping");

        let stop = StreamEvent::MessageStop {};
        let json = serde_json::to_value(&stop).unwrap();
        assert_eq!(json["type"], "message_stop");
    }

    #[test]
    fn stream_delta_text_delta() {
        let d = StreamDelta::TextDelta {
            text: "Hello".into(),
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["type"], "text_delta");
        assert_eq!(json["text"], "Hello");
    }

    #[test]
    fn stream_delta_input_json_delta() {
        let d = StreamDelta::InputJsonDelta {
            partial_json: r#"{"key"#.into(),
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["type"], "input_json_delta");
    }

    #[test]
    fn stream_delta_thinking_and_signature() {
        let d = StreamDelta::ThinkingDelta {
            thinking: "hmm".into(),
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["type"], "thinking_delta");

        let d = StreamDelta::SignatureDelta {
            signature: "sig".into(),
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["type"], "signature_delta");
    }

    #[tokio::test]
    async fn event_stream_collects_all() {
        let events = vec![StreamEvent::Ping {}, StreamEvent::MessageStop {}];
        let stream = EventStream::from_vec(events);
        let collected = stream.collect_all().await;
        assert_eq!(collected.len(), 2);
    }

    // ── 4. Error types ──────────────────────────────────────────────────

    #[test]
    fn shim_error_api_variant() {
        let e = ShimError::ApiError {
            error_type: "overloaded_error".into(),
            message: "Service busy".into(),
        };
        assert!(e.to_string().contains("overloaded_error"));
    }

    #[test]
    fn api_error_struct_fields() {
        let e = ApiError {
            error_type: "invalid_request_error".into(),
            message: "bad param".into(),
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["type"], "invalid_request_error");
        assert_eq!(json["message"], "bad param");
    }

    // ── 5. Auth ─────────────────────────────────────────────────────────

    #[test]
    fn client_default_base_url_is_anthropic() {
        let c = abp_shim_claude::client::Client::new("sk-ant-test").unwrap();
        assert_eq!(c.base_url(), "https://api.anthropic.com/v1");
    }

    // ── 6. Model names ──────────────────────────────────────────────────

    #[test]
    fn accepts_claude_model_identifiers() {
        for model in &[
            "claude-sonnet-4-20250514",
            "claude-3-5-sonnet-20241022",
            "claude-3-haiku-20240307",
            "claude-3-opus-20240229",
        ] {
            let req = MessageRequest {
                model: model.to_string(),
                max_tokens: 100,
                messages: vec![Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text { text: "hi".into() }],
                }],
                system: None,
                temperature: None,
                stop_sequences: None,
                thinking: None,
                stream: None,
            };
            assert_eq!(req.model, *model);
        }
    }

    // ── 7. Content types ────────────────────────────────────────────────

    #[test]
    fn content_block_text() {
        let b = ContentBlock::Text {
            text: "hello".into(),
        };
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(json["type"], "text");
    }

    #[test]
    fn content_block_tool_use() {
        let b = ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        };
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(json["type"], "tool_use");
        assert_eq!(json["name"], "search");
    }

    #[test]
    fn content_block_tool_result() {
        let b = ContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("found it".into()),
            is_error: Some(false),
        };
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(json["type"], "tool_result");
    }

    #[test]
    fn content_block_thinking() {
        let b = ContentBlock::Thinking {
            thinking: "let me think".into(),
            signature: Some("sig_abc".into()),
        };
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(json["type"], "thinking");
    }

    #[test]
    fn content_block_image_base64() {
        let b = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "iVBOR...".into(),
            },
        };
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(json["type"], "image");
        assert_eq!(json["source"]["type"], "base64");
    }

    #[test]
    fn content_block_image_url() {
        let b = ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/img.png".into(),
            },
        };
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(json["source"]["type"], "url");
    }

    // ── 8. Metadata ─────────────────────────────────────────────────────

    #[test]
    fn usage_fields_match_claude_convention() {
        let u = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: Some(20),
            cache_read_input_tokens: Some(10),
        };
        let json = serde_json::to_value(&u).unwrap();
        assert!(json.get("input_tokens").is_some());
        assert!(json.get("output_tokens").is_some());
        assert!(json.get("cache_creation_input_tokens").is_some());
        assert!(json.get("cache_read_input_tokens").is_some());
    }

    #[test]
    fn stop_reason_end_turn_and_tool_use() {
        let resp1 = MessageResponse {
            id: "msg_1".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        assert_eq!(resp1.stop_reason.as_deref(), Some("end_turn"));

        let resp2 = MessageResponse {
            stop_reason: Some("tool_use".into()),
            ..resp1
        };
        assert_eq!(resp2.stop_reason.as_deref(), Some("tool_use"));
    }
}

// ==========================================================================
// Gemini SDK surface
// ==========================================================================
mod gemini {
    use super::*;
    use abp_shim_gemini::{
        Candidate, Content, FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration,
        GeminiClient, GeminiError, GenerateContentRequest, GenerateContentResponse,
        GenerationConfig, HarmBlockThreshold, HarmCategory, Part, SafetySetting, StreamEvent,
        ToolConfig, ToolDeclaration, UsageMetadata,
    };

    // ── 1. Request builder ──────────────────────────────────────────────

    #[test]
    fn request_builder_chain() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("hello")]))
            .generation_config(GenerationConfig {
                max_output_tokens: Some(1024),
                temperature: Some(0.5),
                top_p: Some(0.9),
                top_k: Some(40),
                stop_sequences: None,
                response_mime_type: None,
                response_schema: None,
            })
            .safety_settings(vec![SafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockMediumAndAbove,
            }])
            .tools(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "search".into(),
                    description: "Search web".into(),
                    parameters: json!({}),
                }],
            }])
            .tool_config(ToolConfig {
                function_calling_config: FunctionCallingConfig {
                    mode: FunctionCallingMode::Auto,
                    allowed_function_names: None,
                },
            });
        assert_eq!(req.model, "gemini-2.5-flash");
        assert_eq!(req.contents.len(), 1);
        assert!(req.generation_config.is_some());
        assert!(req.safety_settings.is_some());
        assert!(req.tools.is_some());
        assert!(req.tool_config.is_some());
    }

    #[test]
    fn system_instruction_builder() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::user(vec![Part::text("Be a helpful assistant")]));
        assert!(req.system_instruction.is_some());
    }

    // ── 2. Response structure ───────────────────────────────────────────

    #[test]
    fn response_text_extraction() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("Hello!")]),
                finish_reason: Some("STOP".into()),
            }],
            usage_metadata: None,
        };
        assert_eq!(resp.text(), Some("Hello!"));
    }

    #[test]
    fn response_function_calls_extraction() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::function_call(
                    "get_weather",
                    json!({"city": "NYC"}),
                )]),
                finish_reason: Some("STOP".into()),
            }],
            usage_metadata: None,
        };
        let calls = resp.function_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "get_weather");
    }

    // ── 3. Streaming ────────────────────────────────────────────────────

    #[test]
    fn stream_event_text_extraction() {
        let se = StreamEvent {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("chunk")]),
                finish_reason: None,
            }],
            usage_metadata: None,
        };
        assert_eq!(se.text(), Some("chunk"));
    }

    // ── 4. Error types ──────────────────────────────────────────────────

    #[test]
    fn gemini_error_variants() {
        let e = GeminiError::RequestConversion("bad request".into());
        assert!(e.to_string().contains("request conversion"));
        let e = GeminiError::ResponseConversion("bad response".into());
        assert!(e.to_string().contains("response conversion"));
        let e = GeminiError::BackendError("backend fail".into());
        assert!(e.to_string().contains("backend error"));
    }

    #[test]
    fn client_error_api_variant() {
        let e = abp_shim_gemini::client::ClientError::Api {
            status: 403,
            body: "forbidden".into(),
        };
        assert!(e.to_string().contains("403"));
    }

    // ── 5. Auth ─────────────────────────────────────────────────────────

    #[test]
    fn client_default_base_url_is_google() {
        let c = abp_shim_gemini::client::Client::new("AIza-test").unwrap();
        assert_eq!(
            c.base_url(),
            "https://generativelanguage.googleapis.com/v1beta"
        );
    }

    // ── 6. Model names ──────────────────────────────────────────────────

    #[test]
    fn accepts_gemini_model_identifiers() {
        for model in &[
            "gemini-2.5-flash",
            "gemini-2.5-pro",
            "gemini-2.0-flash",
            "gemini-1.5-flash",
            "gemini-1.5-pro",
        ] {
            let client = GeminiClient::new(*model);
            assert_eq!(client.model(), *model);
        }
    }

    // ── 7. Content types ────────────────────────────────────────────────

    #[test]
    fn part_text() {
        let p = Part::text("hello");
        let json = serde_json::to_value(&p).unwrap();
        // camelCase enum: Text(String) serializes as {"text": "hello"}
        assert_eq!(json["text"], "hello");
    }

    #[test]
    fn part_inline_data() {
        let p = Part::inline_data("image/png", "base64data");
        let json = serde_json::to_value(&p).unwrap();
        let json_str = serde_json::to_string_pretty(&json).unwrap();
        // Check the outer structure has an inlineData key
        assert!(
            json.as_object().unwrap().keys().any(|k| k == "inlineData"),
            "expected inlineData key in: {json_str}"
        );
    }

    #[test]
    fn part_function_call_and_response() {
        let fc = Part::function_call("search", json!({"q": "rust"}));
        let json = serde_json::to_value(&fc).unwrap();
        assert_eq!(json["functionCall"]["name"], "search");

        let fr = Part::function_response("search", json!({"result": "found"}));
        let json = serde_json::to_value(&fr).unwrap();
        assert_eq!(json["functionResponse"]["name"], "search");
    }

    #[test]
    fn content_user_and_model_roles() {
        let u = Content::user(vec![Part::text("hi")]);
        assert_eq!(u.role, "user");
        let m = Content::model(vec![Part::text("hello")]);
        assert_eq!(m.role, "model");
    }

    // ── 8. Metadata ─────────────────────────────────────────────────────

    #[test]
    fn usage_metadata_camel_case() {
        let u = UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 20,
            total_token_count: 30,
        };
        let json = serde_json::to_value(&u).unwrap();
        assert!(json.get("promptTokenCount").is_some());
        assert!(json.get("candidatesTokenCount").is_some());
        assert!(json.get("totalTokenCount").is_some());
    }

    #[test]
    fn harm_categories_and_thresholds() {
        let s = SafetySetting {
            category: HarmCategory::HarmCategoryDangerousContent,
            threshold: HarmBlockThreshold::BlockOnlyHigh,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert!(json.get("category").is_some());
        assert!(json.get("threshold").is_some());
    }

    #[test]
    fn function_calling_modes() {
        let auto_json = serde_json::to_value(FunctionCallingMode::Auto).unwrap();
        let any_json = serde_json::to_value(FunctionCallingMode::Any).unwrap();
        let none_json = serde_json::to_value(FunctionCallingMode::None).unwrap();
        // All three modes should serialize to distinct values
        assert_ne!(auto_json, any_json);
        assert_ne!(auto_json, none_json);
        assert_ne!(any_json, none_json);
    }
}

// ==========================================================================
// Codex SDK surface
// ==========================================================================
mod codex {
    use super::*;
    use abp_codex_sdk::dialect::CodexInputItem;
    use abp_shim_codex::{
        CodexClient, CodexFunctionDef, CodexRequestBuilder, CodexTextFormat, CodexToolDef,
        SandboxConfig, ShimError, Usage,
    };

    // ── 1. Request builder ──────────────────────────────────────────────

    #[test]
    fn builder_default_model_is_codex_mini_latest() {
        let req = CodexRequestBuilder::new()
            .input(vec![CodexInputItem::Message {
                role: "user".into(),
                content: "hello".into(),
            }])
            .build();
        assert_eq!(req.model, "codex-mini-latest");
    }

    #[test]
    fn builder_all_fields() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![CodexInputItem::Message {
                role: "user".into(),
                content: "hi".into(),
            }])
            .max_output_tokens(500)
            .temperature(0.3)
            .tools(vec![])
            .text(CodexTextFormat::Text {})
            .build();
        assert_eq!(req.model, "codex-mini-latest");
        assert_eq!(req.max_output_tokens, Some(500));
        assert_eq!(req.temperature, Some(0.3));
    }

    #[test]
    fn codex_message_helper() {
        let msg = abp_shim_codex::codex_message("user", "hello");
        match msg {
            CodexInputItem::Message { role, content } => {
                assert_eq!(role, "user");
                assert_eq!(content, "hello");
            }
        }
    }

    // ── 2. Response structure ───────────────────────────────────────────

    #[test]
    fn usage_fields_match_codex_convention() {
        let u = Usage {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
        };
        let json = serde_json::to_value(&u).unwrap();
        assert!(json.get("input_tokens").is_some());
        assert!(json.get("output_tokens").is_some());
        assert!(json.get("total_tokens").is_some());
    }

    // ── 3. Streaming ────────────────────────────────────────────────────

    #[test]
    fn codex_client_model_accessor() {
        let client = CodexClient::new("codex-mini-latest");
        assert_eq!(client.model(), "codex-mini-latest");
    }

    // ── 4. Error types ──────────────────────────────────────────────────

    #[test]
    fn shim_error_variants() {
        let e = ShimError::InvalidRequest("bad".into());
        assert!(e.to_string().contains("invalid request"));
        let e = ShimError::Internal("oops".into());
        assert!(e.to_string().contains("internal error"));
    }

    #[test]
    fn client_error_builder_variant() {
        let e = abp_shim_codex::client::ClientError::Builder("missing key".into());
        assert!(e.to_string().contains("builder error"));
    }

    // ── 5. Auth ─────────────────────────────────────────────────────────

    #[test]
    fn client_default_base_url_is_openai() {
        let c = abp_shim_codex::client::Client::new("sk-test").unwrap();
        assert_eq!(c.base_url(), "https://api.openai.com/v1");
    }

    // ── 6. Model names ──────────────────────────────────────────────────

    #[test]
    fn accepts_codex_model_identifiers() {
        for model in &["codex-mini-latest", "o4-mini", "o3"] {
            let client = CodexClient::new(*model);
            assert_eq!(client.model(), *model);
        }
    }

    // ── 7. Content types ────────────────────────────────────────────────

    #[test]
    fn codex_input_message_variant() {
        let item = CodexInputItem::Message {
            role: "user".into(),
            content: "hello".into(),
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["role"], "user");
    }

    #[test]
    fn codex_input_is_single_variant_message_enum() {
        // The dialect CodexInputItem has only Message variant
        let item = CodexInputItem::Message {
            role: "assistant".into(),
            content: "response".into(),
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["role"], "assistant");
        assert_eq!(json["content"], "response");
    }

    #[test]
    fn codex_text_format_variants() {
        let _text = CodexTextFormat::Text {};
        let _json_obj = CodexTextFormat::JsonObject {};
        let _json_schema = CodexTextFormat::JsonSchema {
            name: "my_schema".into(),
            schema: json!({}),
            strict: true,
        };
    }

    #[test]
    fn sandbox_config_fields() {
        let _config = SandboxConfig::default();
    }

    // ── 8. Metadata ─────────────────────────────────────────────────────

    #[test]
    fn codex_tool_def_structure() {
        let td = CodexToolDef {
            tool_type: "function".into(),
            function: CodexFunctionDef {
                name: "run_code".into(),
                description: "Execute code".into(),
                parameters: json!({}),
            },
        };
        let json = serde_json::to_value(&td).unwrap();
        assert_eq!(json["type"], "function");
    }
}

// ==========================================================================
// Copilot SDK surface
// ==========================================================================
mod copilot {
    use super::*;
    use abp_shim_copilot::{
        CopilotClient, CopilotFunctionDef, CopilotRequestBuilder, CopilotToolType, Message,
        ShimError,
    };

    // ── 1. Request builder ──────────────────────────────────────────────

    #[test]
    fn builder_default_model_is_gpt4o() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("hello")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn builder_all_fields() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::system("sys"), Message::user("hi")])
            .turn_history(vec![])
            .references(vec![])
            .build();
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 2);
    }

    // ── 2. Response structure ───────────────────────────────────────────

    #[test]
    fn message_constructors() {
        let sys = Message::system("be helpful");
        assert_eq!(sys.role, "system");
        assert_eq!(sys.content, "be helpful");

        let usr = Message::user("hi");
        assert_eq!(usr.role, "user");

        let asst = Message::assistant("hello");
        assert_eq!(asst.role, "assistant");
    }

    #[test]
    fn message_user_with_refs() {
        let msg = Message::user_with_refs("hi", vec![]);
        assert_eq!(msg.role, "user");
        assert!(msg.copilot_references.is_empty());
    }

    // ── 3. Streaming ────────────────────────────────────────────────────

    #[test]
    fn copilot_client_model_accessor() {
        let client = CopilotClient::new("gpt-4o");
        assert_eq!(client.model(), "gpt-4o");
    }

    // ── 4. Error types ──────────────────────────────────────────────────

    #[test]
    fn shim_error_variants() {
        let e = ShimError::InvalidRequest("bad".into());
        assert!(e.to_string().contains("invalid request"));
        let e = ShimError::Internal("oops".into());
        assert!(e.to_string().contains("internal error"));
    }

    #[test]
    fn client_error_api_variant() {
        let e = abp_shim_copilot::client::ClientError::Api {
            status: 401,
            body: "unauthorized".into(),
        };
        assert!(e.to_string().contains("401"));
    }

    // ── 5. Auth ─────────────────────────────────────────────────────────

    #[test]
    fn client_default_base_url_is_github_copilot() {
        let c = abp_shim_copilot::client::Client::new("ghu_test").unwrap();
        assert_eq!(c.base_url(), "https://api.githubcopilot.com");
    }

    #[test]
    fn client_builder_custom_url() {
        let c = abp_shim_copilot::client::Client::builder("ghu_test")
            .base_url("https://custom.copilot.com")
            .build()
            .unwrap();
        assert_eq!(c.base_url(), "https://custom.copilot.com");
    }

    // ── 6. Model names ──────────────────────────────────────────────────

    #[test]
    fn accepts_copilot_model_identifiers() {
        for model in &["gpt-4o", "gpt-4o-mini", "gpt-4", "gpt-3.5-turbo"] {
            let client = CopilotClient::new(*model);
            assert_eq!(client.model(), *model);
        }
    }

    // ── 7. Content types ────────────────────────────────────────────────

    #[test]
    fn copilot_tool_type_variants() {
        let _f = CopilotToolType::Function;
        let _c = CopilotToolType::Confirmation;
    }

    #[test]
    fn copilot_function_def_fields() {
        let fd = CopilotFunctionDef {
            name: "get_repo".into(),
            description: "Get repository info".into(),
            parameters: json!({}),
        };
        let json = serde_json::to_value(&fd).unwrap();
        assert_eq!(json["name"], "get_repo");
        assert_eq!(json["description"], "Get repository info");
    }

    // ── 8. Metadata ─────────────────────────────────────────────────────

    #[test]
    fn copilot_client_debug() {
        let client = CopilotClient::new("gpt-4o");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("CopilotClient"));
        assert!(dbg.contains("gpt-4o"));
    }
}

// ==========================================================================
// Kimi SDK surface
// ==========================================================================
mod kimi {
    use super::*;
    use abp_shim_kimi::{
        KimiBuiltinFunction, KimiBuiltinTool, KimiClient, KimiFunctionDef, KimiRequestBuilder,
        KimiRole, KimiTool, KimiToolDef, Message, ShimError, Usage,
    };

    // ── 1. Request builder ──────────────────────────────────────────────

    #[test]
    fn builder_default_model_is_moonshot_v1_8k() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("hello")])
            .build();
        assert_eq!(req.model, "moonshot-v1-8k");
    }

    #[test]
    fn builder_all_fields() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-32k")
            .messages(vec![Message::user("hi")])
            .max_tokens(1024)
            .temperature(0.7)
            .stream(true)
            .tools(vec![])
            .use_search(true)
            .build();
        assert_eq!(req.model, "moonshot-v1-32k");
        assert_eq!(req.max_tokens, Some(1024));
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.stream, Some(true));
        assert_eq!(req.use_search, Some(true));
    }

    // ── 2. Response structure ───────────────────────────────────────────

    #[test]
    fn message_constructors() {
        let sys = Message::system("be helpful");
        assert_eq!(sys.role, "system");
        assert_eq!(sys.content, Some("be helpful".into()));

        let usr = Message::user("hi");
        assert_eq!(usr.role, "user");

        let asst = Message::assistant("hello");
        assert_eq!(asst.role, "assistant");
        assert_eq!(asst.content, Some("hello".into()));
    }

    #[test]
    fn message_tool_result() {
        let msg = Message::tool("call_1", "result");
        assert_eq!(msg.role, "tool");
        assert_eq!(msg.tool_call_id, Some("call_1".into()));
    }

    // ── 3. Streaming ────────────────────────────────────────────────────

    #[test]
    fn kimi_client_model_accessor() {
        let client = KimiClient::new("moonshot-v1-8k");
        assert_eq!(client.model(), "moonshot-v1-8k");
    }

    // ── 4. Error types ──────────────────────────────────────────────────

    #[test]
    fn shim_error_variants() {
        let e = ShimError::InvalidRequest("bad".into());
        assert!(e.to_string().contains("invalid request"));
        let e = ShimError::Internal("oops".into());
        assert!(e.to_string().contains("internal error"));
    }

    #[test]
    fn client_error_api_variant() {
        let e = abp_shim_kimi::client::ClientError::Api {
            status: 500,
            body: "server error".into(),
        };
        assert!(e.to_string().contains("500"));
    }

    // ── 5. Auth ─────────────────────────────────────────────────────────

    #[test]
    fn client_default_base_url_is_moonshot() {
        let c = abp_shim_kimi::client::Client::new("sk-test").unwrap();
        assert_eq!(c.base_url(), "https://api.moonshot.cn/v1");
    }

    #[test]
    fn client_builder_custom_url() {
        let c = abp_shim_kimi::client::Client::builder("sk-test")
            .base_url("https://custom.moonshot.cn")
            .build()
            .unwrap();
        assert_eq!(c.base_url(), "https://custom.moonshot.cn");
    }

    // ── 6. Model names ──────────────────────────────────────────────────

    #[test]
    fn accepts_kimi_model_identifiers() {
        for model in &[
            "moonshot-v1-8k",
            "moonshot-v1-32k",
            "moonshot-v1-128k",
            "kimi-latest",
        ] {
            let client = KimiClient::new(*model);
            assert_eq!(client.model(), *model);
        }
    }

    // ── 7. Content types ────────────────────────────────────────────────

    #[test]
    fn kimi_role_enum() {
        let _system = KimiRole::System;
        let _user = KimiRole::User;
        let _assistant = KimiRole::Assistant;
        let _tool = KimiRole::Tool;
    }

    #[test]
    fn kimi_tool_def_type_field() {
        let td = KimiToolDef {
            tool_type: "function".into(),
            function: KimiFunctionDef {
                name: "search".into(),
                description: "Search web".into(),
                parameters: json!({}),
            },
        };
        let json = serde_json::to_value(&td).unwrap();
        assert_eq!(json["type"], "function");
    }

    #[test]
    fn kimi_builtin_tool_fields() {
        let bt = KimiBuiltinTool {
            tool_type: "builtin_function".into(),
            function: KimiBuiltinFunction {
                name: "web_search".into(),
            },
        };
        let json = serde_json::to_value(&bt).unwrap();
        assert_eq!(json["type"], "builtin_function");
        assert_eq!(json["function"]["name"], "web_search");
    }

    #[test]
    fn kimi_tool_builtin_function_variant() {
        let t = KimiTool::BuiltinFunction {
            function: KimiBuiltinFunction {
                name: "web_search".into(),
            },
        };
        let _json = serde_json::to_value(&t).unwrap();
    }

    #[test]
    fn kimi_tool_function_variant() {
        let t = KimiTool::Function {
            function: KimiFunctionDef {
                name: "search".into(),
                description: "Search".into(),
                parameters: json!({}),
            },
        };
        let _json = serde_json::to_value(&t).unwrap();
    }

    // ── 8. Metadata ─────────────────────────────────────────────────────

    #[test]
    fn usage_fields_match_kimi_convention() {
        let u = Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        };
        let json = serde_json::to_value(&u).unwrap();
        assert!(json.get("prompt_tokens").is_some());
        assert!(json.get("completion_tokens").is_some());
        assert!(json.get("total_tokens").is_some());
    }

    #[test]
    fn kimi_client_debug() {
        let client = KimiClient::new("moonshot-v1-8k");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("KimiClient"));
        assert!(dbg.contains("moonshot-v1-8k"));
    }
}
