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
//! Comprehensive API fidelity tests for all 6 SDK shims.
//!
//! Verifies each shim's API matches its SDK's surface area:
//! 1. Request builder API
//! 2. Type serialization (serde roundtrip + expected JSON field names)
//! 3. Conversion completeness
//! 4. Error type consistency

use serde_json::{Value, json};

// ═══════════════════════════════════════════════════════════════════════
// OpenAI Shim
// ═══════════════════════════════════════════════════════════════════════

mod openai {
    use super::*;
    use abp_shim_openai::*;

    // ── Request builder API ─────────────────────────────────────────

    #[test]
    fn t01_openai_request_builder_basic() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("hello")])
            .build();
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 1);
    }

    #[test]
    fn t02_openai_request_builder_all_fields() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::system("sys"), Message::user("hi")])
            .temperature(0.7)
            .max_tokens(1024)
            .stream(true)
            .stop(vec!["END".into()])
            .tools(vec![Tool::function("fn1", "desc", json!({}))])
            .tool_choice(ToolChoice::Mode(ToolChoiceMode::Auto))
            .build();
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.max_tokens, Some(1024));
        assert_eq!(req.stream, Some(true));
        assert_eq!(req.stop.as_ref().unwrap().len(), 1);
        assert_eq!(req.tools.as_ref().unwrap().len(), 1);
        assert!(req.tool_choice.is_some());
    }

    #[test]
    fn t03_openai_request_builder_defaults_model() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("test")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    // ── Message constructors ────────────────────────────────────────

    #[test]
    fn t04_openai_message_constructors() {
        let sys = Message::system("You are helpful");
        assert_eq!(sys.role, Role::System);
        assert_eq!(sys.content.as_deref(), Some("You are helpful"));

        let user = Message::user("Hi");
        assert_eq!(user.role, Role::User);

        let asst = Message::assistant("Hello!");
        assert_eq!(asst.role, Role::Assistant);
        assert!(asst.tool_calls.is_none());

        let tool = Message::tool("call_1", "result");
        assert_eq!(tool.role, Role::Tool);
        assert_eq!(tool.tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn t05_openai_message_assistant_with_tool_calls() {
        let msg = Message::assistant_with_tool_calls(vec![ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"rust"}"#.into(),
            },
        }]);
        assert_eq!(msg.role, Role::Assistant);
        assert!(msg.content.is_none());
        assert_eq!(msg.tool_calls.as_ref().unwrap().len(), 1);
    }

    // ── Type serialization ──────────────────────────────────────────

    #[test]
    fn t06_openai_request_field_names() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("hi")])
            .temperature(0.5)
            .max_tokens(100)
            .stream(true)
            .tools(vec![Tool::function("f", "d", json!({}))])
            .build();
        let val: Value = serde_json::to_value(&req).unwrap();
        assert!(val.get("model").is_some(), "missing 'model'");
        assert!(val.get("messages").is_some(), "missing 'messages'");
        assert!(val.get("tools").is_some(), "missing 'tools'");
        assert!(val.get("temperature").is_some(), "missing 'temperature'");
        assert!(val.get("max_tokens").is_some(), "missing 'max_tokens'");
        assert!(val.get("stream").is_some(), "missing 'stream'");
    }

    #[test]
    fn t07_openai_tool_type_renamed() {
        let tool = Tool::function("read", "Read a file", json!({}));
        let val: Value = serde_json::to_value(&tool).unwrap();
        assert_eq!(
            val["type"], "function",
            "Tool.tool_type should serialize as 'type'"
        );
        assert!(
            val.get("tool_type").is_none(),
            "tool_type should not appear"
        );
    }

    #[test]
    fn t08_openai_tool_call_type_renamed() {
        let tc = ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "search".into(),
                arguments: "{}".into(),
            },
        };
        let val: Value = serde_json::to_value(&tc).unwrap();
        assert_eq!(val["type"], "function");
        assert!(val.get("call_type").is_none());
    }

    #[test]
    fn t09_openai_role_serde_roundtrip() {
        let role = Role::Assistant;
        let s = serde_json::to_string(&role).unwrap();
        assert_eq!(s, r#""assistant""#);
        let back: Role = serde_json::from_str(&s).unwrap();
        assert_eq!(back, role);
    }

    #[test]
    fn t10_openai_usage_serde_roundtrip() {
        let usage = Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, usage);
    }

    // ── Conversion completeness ─────────────────────────────────────

    #[test]
    fn t11_openai_request_to_ir() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::system("sys"), Message::user("hello")])
            .build();
        let ir = request_to_ir(&req);
        assert_eq!(ir.len(), 2);
    }

    #[test]
    fn t12_openai_request_to_work_order() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o-mini")
            .messages(vec![Message::user("test")])
            .temperature(0.5)
            .max_tokens(512)
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(wo.config.vendor.get("temperature"), Some(&json!(0.5)));
        assert_eq!(wo.config.vendor.get("max_tokens"), Some(&json!(512)));
    }

    #[test]
    fn t13_openai_tools_to_ir() {
        let tools = vec![
            Tool::function("fn_a", "desc_a", json!({"type": "object"})),
            Tool::function("fn_b", "desc_b", json!({})),
        ];
        let ir_tools = tools_to_ir(&tools);
        assert_eq!(ir_tools.len(), 2);
        assert_eq!(ir_tools[0].name, "fn_a");
        assert_eq!(ir_tools[1].name, "fn_b");
    }

    // ── Error types ─────────────────────────────────────────────────

    #[test]
    fn t14_openai_shim_error_variants() {
        let e1 = ShimError::InvalidRequest("bad".into());
        assert!(e1.to_string().contains("bad"));

        let e2 = ShimError::Internal("oops".into());
        assert!(e2.to_string().contains("oops"));

        let e3: ShimError = serde_json::from_str::<Value>("not json")
            .unwrap_err()
            .into();
        assert!(e3.to_string().contains("serde"));
    }

    // ── Stream types serialization ──────────────────────────────────

    #[test]
    fn t15_openai_stream_tool_call_type_renamed() {
        let stc = StreamToolCall {
            index: 0,
            id: Some("call_1".into()),
            call_type: Some("function".into()),
            function: Some(StreamFunctionCall {
                name: Some("search".into()),
                arguments: Some("{}".into()),
            }),
        };
        let val: Value = serde_json::to_value(&stc).unwrap();
        assert_eq!(val["type"], "function");
        assert!(val.get("call_type").is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Claude Shim
// ═══════════════════════════════════════════════════════════════════════

mod claude {
    use super::*;
    use abp_shim_claude::*;

    // ── Request construction ────────────────────────────────────────

    #[test]
    fn t01_claude_request_construction() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }],
            system: Some("Be helpful".into()),
            temperature: Some(0.7),
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        assert_eq!(req.model, "claude-sonnet-4-20250514");
        assert_eq!(req.max_tokens, 4096);
        assert_eq!(req.system.as_deref(), Some("Be helpful"));
    }

    // ── Type serialization ──────────────────────────────────────────

    #[test]
    fn t02_claude_request_field_names() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![],
            system: Some("sys".into()),
            temperature: Some(0.5),
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let val: Value = serde_json::to_value(&req).unwrap();
        assert!(val.get("model").is_some(), "missing 'model'");
        assert!(val.get("messages").is_some(), "missing 'messages'");
        assert!(val.get("system").is_some(), "missing 'system'");
        assert!(val.get("max_tokens").is_some(), "missing 'max_tokens'");
        assert!(val.get("temperature").is_some(), "missing 'temperature'");
    }

    #[test]
    fn t03_claude_content_block_tagged_serde() {
        let text = ContentBlock::Text {
            text: "hello".into(),
        };
        let val: Value = serde_json::to_value(&text).unwrap();
        assert_eq!(val["type"], "text");
        assert_eq!(val["text"], "hello");

        let tool_use = ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read_file".into(),
            input: json!({"path": "a.rs"}),
        };
        let val2: Value = serde_json::to_value(&tool_use).unwrap();
        assert_eq!(val2["type"], "tool_use");
        assert_eq!(val2["name"], "read_file");
    }

    #[test]
    fn t04_claude_content_block_roundtrip() {
        let blocks = vec![
            ContentBlock::Text { text: "hi".into() },
            ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "fn".into(),
                input: json!({}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: Some("ok".into()),
                is_error: None,
            },
            ContentBlock::Thinking {
                thinking: "Let me think...".into(),
                signature: Some("sig_abc".into()),
            },
        ];
        for block in &blocks {
            let json = serde_json::to_string(block).unwrap();
            let back: ContentBlock = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, block);
        }
    }

    #[test]
    fn t05_claude_response_type_renamed() {
        let resp = MessageResponse {
            id: "msg_1".into(),
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
        let val: Value = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            val["type"], "message",
            "response_type should serialize as 'type'"
        );
        assert!(val.get("response_type").is_none());
    }

    #[test]
    fn t06_claude_usage_serde_roundtrip() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(20),
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, usage);
    }

    #[test]
    fn t07_claude_role_serde() {
        let user = Role::User;
        let s = serde_json::to_string(&user).unwrap();
        assert_eq!(s, r#""user""#);
        let asst = Role::Assistant;
        let s2 = serde_json::to_string(&asst).unwrap();
        assert_eq!(s2, r#""assistant""#);
    }

    // ── Stream types ────────────────────────────────────────────────

    #[test]
    fn t08_claude_stream_event_tagged() {
        let ping = StreamEvent::Ping {};
        let val: Value = serde_json::to_value(&ping).unwrap();
        assert_eq!(val["type"], "ping");

        let msg_stop = StreamEvent::MessageStop {};
        let val2: Value = serde_json::to_value(&msg_stop).unwrap();
        assert_eq!(val2["type"], "message_stop");
    }

    #[test]
    fn t09_claude_stream_delta_tagged() {
        let td = StreamDelta::TextDelta {
            text: "hello".into(),
        };
        let val: Value = serde_json::to_value(&td).unwrap();
        assert_eq!(val["type"], "text_delta");
        assert_eq!(val["text"], "hello");

        let ijd = StreamDelta::InputJsonDelta {
            partial_json: r#"{"x":"#.into(),
        };
        let val2: Value = serde_json::to_value(&ijd).unwrap();
        assert_eq!(val2["type"], "input_json_delta");
    }

    // ── Conversion completeness ─────────────────────────────────────

    #[test]
    fn t10_claude_content_block_to_ir_roundtrip() {
        let block = ContentBlock::Text { text: "hi".into() };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    #[test]
    fn t11_claude_image_source_roundtrip() {
        let b64 = ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "abc123".into(),
        };
        let block = ContentBlock::Image {
            source: b64.clone(),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);

        let url_src = ImageSource::Url {
            url: "https://example.com/img.png".into(),
        };
        let block2 = ContentBlock::Image { source: url_src };
        let ir2 = content_block_to_ir(&block2);
        let back2 = content_block_from_ir(&ir2);
        assert_eq!(back2, block2);
    }

    #[test]
    fn t12_claude_message_to_ir() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "hello".into(),
            }],
        };
        let ir = message_to_ir(&msg);
        assert_eq!(ir.role, "user");
    }

    #[test]
    fn t13_claude_request_to_claude_sdk() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }],
            system: Some("Be concise".into()),
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let sdk_req = request_to_claude(&req);
        assert_eq!(sdk_req.model, "claude-sonnet-4-20250514");
        assert_eq!(sdk_req.max_tokens, 1024);
        assert_eq!(sdk_req.system.as_deref(), Some("Be concise"));
        assert_eq!(sdk_req.messages.len(), 1);
    }

    // ── Error types ─────────────────────────────────────────────────

    #[test]
    fn t14_claude_shim_error_variants() {
        let e1 = ShimError::InvalidRequest("bad field".into());
        assert!(e1.to_string().contains("bad field"));

        let e2 = ShimError::ApiError {
            error_type: "rate_limit".into(),
            message: "slow down".into(),
        };
        assert!(e2.to_string().contains("rate_limit"));
        assert!(e2.to_string().contains("slow down"));

        let e3 = ShimError::Internal("boom".into());
        assert!(e3.to_string().contains("boom"));
    }

    #[test]
    fn t15_claude_api_error_type_renamed() {
        let err = ApiError {
            error_type: "invalid_request".into(),
            message: "bad".into(),
        };
        let val: Value = serde_json::to_value(&err).unwrap();
        assert_eq!(val["type"], "invalid_request");
        assert!(val.get("error_type").is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Gemini Shim
// ═══════════════════════════════════════════════════════════════════════

mod gemini {
    use super::*;
    use abp_shim_gemini::*;

    // ── Request builder API ─────────────────────────────────────────

    #[test]
    fn t01_gemini_request_builder_basic() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("hi")]));
        assert_eq!(req.model, "gemini-2.5-flash");
        assert_eq!(req.contents.len(), 1);
    }

    #[test]
    fn t02_gemini_request_builder_all_fields() {
        let req = GenerateContentRequest::new("gemini-2.5-pro")
            .add_content(Content::user(vec![Part::text("hi")]))
            .system_instruction(Content::user(vec![Part::text("Be helpful")]))
            .generation_config(GenerationConfig {
                temperature: Some(0.7),
                max_output_tokens: Some(1024),
                top_p: Some(0.9),
                top_k: Some(40),
                stop_sequences: Some(vec!["END".into()]),
                response_mime_type: None,
                response_schema: None,
            })
            .safety_settings(vec![SafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockNone,
            }])
            .tools(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "fn1".into(),
                    description: "desc".into(),
                    parameters: json!({}),
                }],
            }])
            .tool_config(ToolConfig {
                function_calling_config: FunctionCallingConfig {
                    mode: FunctionCallingMode::Auto,
                    allowed_function_names: None,
                },
            });
        assert!(req.system_instruction.is_some());
        assert!(req.generation_config.is_some());
        assert!(req.safety_settings.is_some());
        assert!(req.tools.is_some());
        assert!(req.tool_config.is_some());
    }

    // ── Type serialization ──────────────────────────────────────────

    #[test]
    fn t03_gemini_request_field_names() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("hi")]))
            .generation_config(GenerationConfig {
                temperature: Some(0.5),
                ..Default::default()
            })
            .safety_settings(vec![SafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockNone,
            }])
            .tools(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "f".into(),
                    description: "d".into(),
                    parameters: json!({}),
                }],
            }]);
        let val: Value = serde_json::to_value(&req).unwrap();
        assert!(val.get("contents").is_some(), "missing 'contents'");
        assert!(
            val.get("generation_config").is_some(),
            "missing 'generation_config'"
        );
        assert!(
            val.get("safety_settings").is_some(),
            "missing 'safety_settings'"
        );
        assert!(val.get("tools").is_some(), "missing 'tools'");
    }

    #[test]
    fn t04_gemini_generation_config_camel_case() {
        let cfg = GenerationConfig {
            max_output_tokens: Some(1024),
            temperature: Some(0.5),
            top_p: Some(0.9),
            top_k: Some(40),
            stop_sequences: Some(vec!["END".into()]),
            response_mime_type: Some("application/json".into()),
            response_schema: Some(json!({"type": "object"})),
        };
        let val: Value = serde_json::to_value(&cfg).unwrap();
        assert!(val.get("maxOutputTokens").is_some());
        assert!(val.get("topP").is_some());
        assert!(val.get("topK").is_some());
        assert!(val.get("stopSequences").is_some());
        assert!(val.get("responseMimeType").is_some());
        assert!(val.get("responseSchema").is_some());
    }

    #[test]
    fn t05_gemini_usage_metadata_camel_case() {
        let usage = UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 20,
            total_token_count: 30,
        };
        let val: Value = serde_json::to_value(&usage).unwrap();
        assert!(val.get("promptTokenCount").is_some());
        assert!(val.get("candidatesTokenCount").is_some());
        assert!(val.get("totalTokenCount").is_some());
        let back: UsageMetadata = serde_json::from_value(val).unwrap();
        assert_eq!(back, usage);
    }

    #[test]
    fn t06_gemini_part_camel_case_serde() {
        let fc = Part::function_call("search", json!({"q": "rust"}));
        let val: Value = serde_json::to_value(&fc).unwrap();
        assert!(val.get("functionCall").is_some());

        let fr = Part::function_response("search", json!("ok"));
        let val2: Value = serde_json::to_value(&fr).unwrap();
        assert!(val2.get("functionResponse").is_some());

        let id = Part::inline_data("image/png", "abc");
        let val3: Value = serde_json::to_value(&id).unwrap();
        assert!(val3.get("inlineData").is_some());
    }

    // ── Part constructors ───────────────────────────────────────────

    #[test]
    fn t07_gemini_part_constructors() {
        let t = Part::text("hi");
        assert!(matches!(t, Part::Text(ref s) if s == "hi"));

        let fc = Part::function_call("fn1", json!({}));
        assert!(matches!(fc, Part::FunctionCall { ref name, .. } if name == "fn1"));

        let fr = Part::function_response("fn1", json!("ok"));
        assert!(matches!(fr, Part::FunctionResponse { ref name, .. } if name == "fn1"));

        let id = Part::inline_data("image/jpeg", "data");
        assert!(matches!(id, Part::InlineData { ref mime_type, .. } if mime_type == "image/jpeg"));
    }

    // ── Content constructors ────────────────────────────────────────

    #[test]
    fn t08_gemini_content_constructors() {
        let user = Content::user(vec![Part::text("hi")]);
        assert_eq!(user.role, "user");
        let model = Content::model(vec![Part::text("hello")]);
        assert_eq!(model.role, "model");
    }

    // ── Conversion completeness ─────────────────────────────────────

    #[test]
    fn t09_gemini_part_dialect_roundtrip() {
        let parts = vec![
            Part::text("hi"),
            Part::inline_data("image/png", "data"),
            Part::function_call("fn", json!({})),
            Part::function_response("fn", json!("ok")),
        ];
        for p in &parts {
            let dialect = part_to_dialect(p);
            let back = part_from_dialect(&dialect);
            let json_orig = serde_json::to_value(p).unwrap();
            let json_back = serde_json::to_value(&back).unwrap();
            assert_eq!(json_orig, json_back);
        }
    }

    #[test]
    fn t10_gemini_content_dialect_roundtrip() {
        let content = Content::user(vec![Part::text("hi"), Part::text("there")]);
        let dialect = content_to_dialect(&content);
        let back = content_from_dialect(&dialect);
        assert_eq!(back.role, "user");
        assert_eq!(back.parts.len(), 2);
    }

    #[test]
    fn t11_gemini_gen_config_dialect_roundtrip() {
        let cfg = GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(1024),
            top_p: Some(0.9),
            top_k: Some(40),
            stop_sequences: Some(vec!["STOP".into()]),
            response_mime_type: None,
            response_schema: None,
        };
        let dialect = gen_config_to_dialect(&cfg);
        let back = gen_config_from_dialect(&dialect);
        assert_eq!(back.temperature, cfg.temperature);
        assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
        assert_eq!(back.top_p, cfg.top_p);
        assert_eq!(back.top_k, cfg.top_k);
    }

    #[test]
    fn t12_gemini_to_dialect_request() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("hi")]));
        let dialect = to_dialect_request(&req);
        assert_eq!(dialect.model, "gemini-2.5-flash");
        assert_eq!(dialect.contents.len(), 1);
    }

    // ── Response accessors ──────────────────────────────────────────

    #[test]
    fn t13_gemini_response_text_accessor() {
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
    fn t14_gemini_response_function_calls_accessor() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![
                    Part::function_call("fn_a", json!({"x": 1})),
                    Part::function_call("fn_b", json!({"y": 2})),
                ]),
                finish_reason: None,
            }],
            usage_metadata: None,
        };
        let calls = resp.function_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "fn_a");
        assert_eq!(calls[1].0, "fn_b");
    }

    // ── Error types ─────────────────────────────────────────────────

    #[test]
    fn t15_gemini_error_variants() {
        let e1 = GeminiError::RequestConversion("bad".into());
        assert!(e1.to_string().contains("bad"));

        let e2 = GeminiError::ResponseConversion("fail".into());
        assert!(e2.to_string().contains("fail"));

        let e3 = GeminiError::BackendError("timeout".into());
        assert!(e3.to_string().contains("timeout"));

        let e4: GeminiError = serde_json::from_str::<Value>("bad").unwrap_err().into();
        assert!(e4.to_string().contains("serde"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Codex Shim
// ═══════════════════════════════════════════════════════════════════════

mod codex {
    use super::*;
    use abp_codex_sdk::dialect::{CodexInputItem, CodexTextFormat, CodexTool};
    use abp_shim_codex::*;

    // ── Request builder API ─────────────────────────────────────────

    #[test]
    fn t01_codex_request_builder_basic() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "hello")])
            .build();
        assert_eq!(req.model, "codex-mini-latest");
        assert_eq!(req.input.len(), 1);
    }

    #[test]
    fn t02_codex_request_builder_all_fields() {
        let req = CodexRequestBuilder::new()
            .model("o3-mini")
            .input(vec![codex_message("user", "hi")])
            .max_output_tokens(2048)
            .temperature(0.7)
            .tools(vec![CodexTool::Function {
                function: CodexFunctionDef {
                    name: "shell".into(),
                    description: "Run command".into(),
                    parameters: json!({"type": "object"}),
                },
            }])
            .text(CodexTextFormat::Text {})
            .build();
        assert_eq!(req.model, "o3-mini");
        assert_eq!(req.max_output_tokens, Some(2048));
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.tools.len(), 1);
        assert!(req.text.is_some());
    }

    #[test]
    fn t03_codex_request_builder_defaults_model() {
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "test")])
            .build();
        assert_eq!(req.model, "codex-mini-latest");
    }

    #[test]
    fn t04_codex_message_helper() {
        let msg = codex_message("user", "Hello there");
        match msg {
            CodexInputItem::Message { role, content } => {
                assert_eq!(role, "user");
                assert_eq!(content, "Hello there");
            }
        }
    }

    // ── Type serialization ──────────────────────────────────────────

    #[test]
    fn t05_codex_request_field_names() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "hi")])
            .max_output_tokens(1024)
            .temperature(0.5)
            .build();
        let val: Value = serde_json::to_value(&req).unwrap();
        assert!(val.get("model").is_some(), "missing 'model'");
        assert!(val.get("input").is_some(), "missing 'input'");
        assert!(
            val.get("max_output_tokens").is_some(),
            "missing 'max_output_tokens'"
        );
        assert!(val.get("temperature").is_some(), "missing 'temperature'");
    }

    #[test]
    fn t06_codex_input_item_tagged() {
        let item = codex_message("user", "hi");
        let val: Value = serde_json::to_value(&item).unwrap();
        assert_eq!(val["type"], "message");
        assert_eq!(val["role"], "user");
        assert_eq!(val["content"], "hi");
    }

    #[test]
    fn t07_codex_tool_tagged() {
        let tool = CodexTool::Function {
            function: CodexFunctionDef {
                name: "shell".into(),
                description: "Run a command".into(),
                parameters: json!({}),
            },
        };
        let val: Value = serde_json::to_value(&tool).unwrap();
        assert_eq!(val["type"], "function");
    }

    #[test]
    fn t08_codex_usage_serde() {
        let usage = Usage {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, usage);
    }

    // ── Conversion completeness ─────────────────────────────────────

    #[test]
    fn t09_codex_request_to_ir() {
        let req = CodexRequestBuilder::new()
            .input(vec![
                codex_message("system", "Be concise"),
                codex_message("user", "Hello"),
            ])
            .build();
        let ir = request_to_ir(&req);
        assert_eq!(ir.len(), 2);
    }

    #[test]
    fn t10_codex_request_to_work_order() {
        let req = CodexRequestBuilder::new()
            .model("o3-mini")
            .input(vec![codex_message("user", "test")])
            .temperature(0.7)
            .max_output_tokens(2048)
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
        assert_eq!(wo.config.vendor.get("temperature"), Some(&json!(0.7)));
        assert_eq!(
            wo.config.vendor.get("max_output_tokens"),
            Some(&json!(2048))
        );
    }

    #[test]
    fn t11_codex_ir_usage_to_usage() {
        let ir = abp_core::ir::IrUsage::from_io(100, 50);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    // ── Error types ─────────────────────────────────────────────────

    #[test]
    fn t12_codex_shim_error_variants() {
        let e1 = ShimError::InvalidRequest("bad".into());
        assert!(e1.to_string().contains("bad"));

        let e2 = ShimError::Internal("oops".into());
        assert!(e2.to_string().contains("oops"));

        let e3: ShimError = serde_json::from_str::<Value>("bad").unwrap_err().into();
        assert!(e3.to_string().contains("serde"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Copilot Shim
// ═══════════════════════════════════════════════════════════════════════

mod copilot {
    use super::*;
    use abp_copilot_sdk::dialect::CopilotReference;
    use abp_shim_copilot::*;

    // ── Request builder API ─────────────────────────────────────────

    #[test]
    fn t01_copilot_request_builder_basic() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("hello")])
            .build();
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 1);
    }

    #[test]
    fn t02_copilot_request_builder_all_fields() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4-turbo")
            .messages(vec![Message::system("sys"), Message::user("hi")])
            .tools(vec![])
            .turn_history(vec![])
            .references(vec![])
            .build();
        assert_eq!(req.model, "gpt-4-turbo");
        assert_eq!(req.messages.len(), 2);
        assert!(req.tools.is_some());
    }

    #[test]
    fn t03_copilot_request_builder_defaults_model() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    // ── Message constructors ────────────────────────────────────────

    #[test]
    fn t04_copilot_message_constructors() {
        let sys = Message::system("sys prompt");
        assert_eq!(sys.role, "system");
        assert_eq!(sys.content, "sys prompt");
        assert!(sys.copilot_references.is_empty());

        let user = Message::user("hello");
        assert_eq!(user.role, "user");

        let asst = Message::assistant("hi");
        assert_eq!(asst.role, "assistant");
    }

    #[test]
    fn t05_copilot_message_user_with_refs() {
        let refs = vec![CopilotReference {
            ref_type: abp_copilot_sdk::dialect::CopilotReferenceType::File,
            id: "file_1".into(),
            data: json!({"path": "src/main.rs"}),
            metadata: None,
        }];
        let msg = Message::user_with_refs("check this file", refs);
        assert_eq!(msg.role, "user");
        assert_eq!(msg.copilot_references.len(), 1);
    }

    // ── Type serialization ──────────────────────────────────────────

    #[test]
    fn t06_copilot_request_field_names() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("hi")])
            .build();
        let val: Value = serde_json::to_value(&req).unwrap();
        assert!(val.get("messages").is_some(), "missing 'messages'");
        assert!(val.get("model").is_some(), "missing 'model'");
    }

    #[test]
    fn t07_copilot_message_serde() {
        let msg = Message::user("hello");
        let val: Value = serde_json::to_value(&msg).unwrap();
        assert_eq!(val["role"], "user");
        assert_eq!(val["content"], "hello");
        // copilot_references should be skipped if empty
        assert!(
            val.get("copilot_references").is_none(),
            "empty copilot_references should be skipped"
        );
    }

    #[test]
    fn t08_copilot_message_with_refs_serde() {
        let refs = vec![CopilotReference {
            ref_type: abp_copilot_sdk::dialect::CopilotReferenceType::Repository,
            id: "repo_1".into(),
            data: json!({}),
            metadata: None,
        }];
        let msg = Message::user_with_refs("check repo", refs);
        let val: Value = serde_json::to_value(&msg).unwrap();
        assert!(val.get("copilot_references").is_some());
        assert_eq!(val["copilot_references"].as_array().unwrap().len(), 1);
    }

    // ── Conversion completeness ─────────────────────────────────────

    #[test]
    fn t09_copilot_request_to_ir() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::system("sys"), Message::user("hello")])
            .build();
        let ir = request_to_ir(&req);
        assert_eq!(ir.len(), 2);
    }

    #[test]
    fn t10_copilot_request_to_work_order() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn t11_copilot_messages_ir_roundtrip() {
        let messages = vec![
            Message::system("System"),
            Message::user("User"),
            Message::assistant("Assistant"),
        ];
        let ir = messages_to_ir(&messages);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content, "System");
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
    }

    #[test]
    fn t12_copilot_ir_usage_to_tuple() {
        let ir = abp_core::ir::IrUsage::from_io(200, 100);
        let (input, output, total) = ir_usage_to_tuple(&ir);
        assert_eq!(input, 200);
        assert_eq!(output, 100);
        assert_eq!(total, 300);
    }

    // ── Error types ─────────────────────────────────────────────────

    #[test]
    fn t13_copilot_shim_error_variants() {
        let e1 = ShimError::InvalidRequest("invalid".into());
        assert!(e1.to_string().contains("invalid"));

        let e2 = ShimError::Internal("internal".into());
        assert!(e2.to_string().contains("internal"));

        let e3: ShimError = serde_json::from_str::<Value>("bad").unwrap_err().into();
        assert!(e3.to_string().contains("serde"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Kimi Shim
// ═══════════════════════════════════════════════════════════════════════

mod kimi {
    use super::*;
    use abp_shim_kimi::*;

    // ── Request builder API ─────────────────────────────────────────

    #[test]
    fn t01_kimi_request_builder_basic() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("hello")])
            .build();
        assert_eq!(req.model, "moonshot-v1-8k");
        assert_eq!(req.messages.len(), 1);
    }

    #[test]
    fn t02_kimi_request_builder_all_fields() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::system("sys"), Message::user("hi")])
            .max_tokens(2048)
            .temperature(0.7)
            .stream(true)
            .tools(vec![])
            .use_search(true)
            .build();
        assert_eq!(req.model, "moonshot-v1-128k");
        assert_eq!(req.max_tokens, Some(2048));
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.stream, Some(true));
        assert!(req.tools.is_some());
        assert_eq!(req.use_search, Some(true));
    }

    #[test]
    fn t03_kimi_request_builder_defaults_model() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        assert_eq!(req.model, "moonshot-v1-8k");
    }

    // ── Message constructors ────────────────────────────────────────

    #[test]
    fn t04_kimi_message_constructors() {
        let sys = Message::system("prompt");
        assert_eq!(sys.role, "system");
        assert_eq!(sys.content.as_deref(), Some("prompt"));
        assert!(sys.tool_calls.is_none());

        let user = Message::user("hi");
        assert_eq!(user.role, "user");

        let asst = Message::assistant("hello");
        assert_eq!(asst.role, "assistant");

        let tool = Message::tool("call_1", "result");
        assert_eq!(tool.role, "tool");
        assert_eq!(tool.tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn t05_kimi_message_assistant_with_tool_calls() {
        use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiToolCall};
        let msg = Message::assistant_with_tool_calls(vec![KimiToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "search".into(),
                arguments: "{}".into(),
            },
        }]);
        assert_eq!(msg.role, "assistant");
        assert!(msg.content.is_none());
        assert_eq!(msg.tool_calls.as_ref().unwrap().len(), 1);
    }

    // ── Type serialization ──────────────────────────────────────────

    #[test]
    fn t06_kimi_request_field_names() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("hi")])
            .temperature(0.5)
            .max_tokens(1024)
            .tools(vec![])
            .build();
        let val: Value = serde_json::to_value(&req).unwrap();
        assert!(val.get("messages").is_some(), "missing 'messages'");
        assert!(val.get("model").is_some(), "missing 'model'");
        assert!(val.get("tools").is_some(), "missing 'tools'");
        assert!(val.get("temperature").is_some(), "missing 'temperature'");
        assert!(val.get("max_tokens").is_some(), "missing 'max_tokens'");
    }

    #[test]
    fn t07_kimi_message_serde() {
        let msg = Message::user("hello");
        let val: Value = serde_json::to_value(&msg).unwrap();
        assert_eq!(val["role"], "user");
        assert_eq!(val["content"], "hello");
        // tool_calls should be skipped when None
        assert!(val.get("tool_calls").is_none());
    }

    #[test]
    fn t08_kimi_usage_serde() {
        let usage = Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, usage);
    }

    // ── Conversion completeness ─────────────────────────────────────

    #[test]
    fn t09_kimi_request_to_ir() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::system("sys"), Message::user("hello")])
            .build();
        let ir = request_to_ir(&req);
        assert_eq!(ir.len(), 2);
    }

    #[test]
    fn t10_kimi_request_to_work_order() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::user("test")])
            .temperature(0.7)
            .max_tokens(1024)
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
        assert_eq!(wo.config.vendor.get("temperature"), Some(&json!(0.7)));
        assert_eq!(wo.config.vendor.get("max_tokens"), Some(&json!(1024)));
    }

    #[test]
    fn t11_kimi_messages_ir_roundtrip() {
        let messages = vec![
            Message::system("System"),
            Message::user("User"),
            Message::assistant("Assistant"),
        ];
        let ir = messages_to_ir(&messages);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content.as_deref(), Some("System"));
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
    }

    #[test]
    fn t12_kimi_ir_usage_to_usage() {
        let ir = abp_core::ir::IrUsage::from_io(200, 100);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 200);
        assert_eq!(usage.completion_tokens, 100);
        assert_eq!(usage.total_tokens, 300);
    }

    // ── Error types ─────────────────────────────────────────────────

    #[test]
    fn t13_kimi_shim_error_variants() {
        let e1 = ShimError::InvalidRequest("invalid".into());
        assert!(e1.to_string().contains("invalid"));

        let e2 = ShimError::Internal("internal".into());
        assert!(e2.to_string().contains("internal"));

        let e3: ShimError = serde_json::from_str::<Value>("bad").unwrap_err().into();
        assert!(e3.to_string().contains("serde"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Cross-shim consistency checks
// ═══════════════════════════════════════════════════════════════════════

mod cross_shim {
    #[test]
    fn t01_all_shims_have_consistent_error_variant_names() {
        // OpenAI
        let _ = abp_shim_openai::ShimError::InvalidRequest("x".into());
        let _ = abp_shim_openai::ShimError::Internal("x".into());

        // Codex
        let _ = abp_shim_codex::ShimError::InvalidRequest("x".into());
        let _ = abp_shim_codex::ShimError::Internal("x".into());

        // Copilot
        let _ = abp_shim_copilot::ShimError::InvalidRequest("x".into());
        let _ = abp_shim_copilot::ShimError::Internal("x".into());

        // Kimi
        let _ = abp_shim_kimi::ShimError::InvalidRequest("x".into());
        let _ = abp_shim_kimi::ShimError::Internal("x".into());

        // Claude has ApiError instead of Internal — that's fine but verify it exists
        let _ = abp_shim_claude::ShimError::InvalidRequest("x".into());
        let _ = abp_shim_claude::ShimError::Internal("x".into());
        let _ = abp_shim_claude::ShimError::ApiError {
            error_type: "t".into(),
            message: "m".into(),
        };

        // Gemini uses GeminiError
        let _ = abp_shim_gemini::GeminiError::RequestConversion("x".into());
        let _ = abp_shim_gemini::GeminiError::BackendError("x".into());
    }

    #[test]
    fn t02_all_shims_produce_work_orders_with_model() {
        // OpenAI
        let oai = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .build();
        let wo_oai = abp_shim_openai::request_to_work_order(&oai);
        assert!(wo_oai.config.model.is_some());

        // Codex
        let cdx = abp_shim_codex::CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![abp_shim_codex::codex_message("user", "test")])
            .build();
        let wo_cdx = abp_shim_codex::request_to_work_order(&cdx);
        assert!(wo_cdx.config.model.is_some());

        // Copilot
        let cop = abp_shim_copilot::CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![abp_shim_copilot::Message::user("test")])
            .build();
        let wo_cop = abp_shim_copilot::request_to_work_order(&cop);
        assert!(wo_cop.config.model.is_some());

        // Kimi
        let kimi = abp_shim_kimi::KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![abp_shim_kimi::Message::user("test")])
            .build();
        let wo_kimi = abp_shim_kimi::request_to_work_order(&kimi);
        assert!(wo_kimi.config.model.is_some());
    }
}
