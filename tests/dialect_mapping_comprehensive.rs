#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the dialect mapping layer — conversion between
//! SDK-specific request/response formats and the ABP intermediate representation.

// ── 1. OpenAI dialect → ABP IR mapping ──────────────────────────────────

mod openai_to_ir {
    use abp_core::ir::{IrContentBlock, IrRole};
    use abp_openai_sdk::dialect::{
        self, CanonicalToolDef, OpenAIChoice, OpenAIFunctionCall, OpenAIMessage, OpenAIResponse,
        OpenAIToolCall, OpenAIUsage,
    };
    use abp_openai_sdk::lowering;
    use serde_json::json;

    #[test]
    fn simple_user_message_to_ir() {
        let msgs = vec![OpenAIMessage {
            role: "user".into(),
            content: Some("Hello, world!".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello, world!");
    }

    #[test]
    fn system_message_to_ir() {
        let msgs = vec![OpenAIMessage {
            role: "system".into(),
            content: Some("You are a helpful assistant.".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(
            conv.messages[0].text_content(),
            "You are a helpful assistant."
        );
    }

    #[test]
    fn assistant_message_to_ir() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: Some("I can help with that.".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        assert_eq!(conv.messages[0].text_content(), "I can help with that.");
    }

    #[test]
    fn tool_call_maps_to_ir_tool_use() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_abc123".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"src/main.rs"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_abc123");
                assert_eq!(name, "read_file");
                assert_eq!(input, &json!({"path": "src/main.rs"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn multiple_tool_calls_in_single_message() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![
                OpenAIToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "read_file".into(),
                        arguments: r#"{"path":"a.rs"}"#.into(),
                    },
                },
                OpenAIToolCall {
                    id: "call_2".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "write_file".into(),
                        arguments: r#"{"path":"b.rs","content":"fn main() {}"}"#.into(),
                    },
                },
            ]),
            tool_call_id: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].content.len(), 2);
        assert!(matches!(
            &conv.messages[0].content[0],
            IrContentBlock::ToolUse { name, .. } if name == "read_file"
        ));
        assert!(matches!(
            &conv.messages[0].content[1],
            IrContentBlock::ToolUse { name, .. } if name == "write_file"
        ));
    }

    #[test]
    fn tool_result_message_to_ir() {
        let msgs = vec![OpenAIMessage {
            role: "tool".into(),
            content: Some("fn main() { println!(\"hello\"); }".into()),
            tool_calls: None,
            tool_call_id: Some("call_abc123".into()),
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Tool);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "call_abc123");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn mixed_text_and_tool_calls() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: Some("Let me check that file.".into()),
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_mix".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"test.rs"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].content.len(), 2);
        assert!(matches!(
            &conv.messages[0].content[0],
            IrContentBlock::Text { text } if text == "Let me check that file."
        ));
        assert!(matches!(
            &conv.messages[0].content[1],
            IrContentBlock::ToolUse { .. }
        ));
    }

    #[test]
    fn malformed_tool_arguments_preserved_as_string() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_bad".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "foo".into(),
                    arguments: "invalid json {{".into(),
                },
            }]),
            tool_call_id: None,
        }];
        let conv = lowering::to_ir(&msgs);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { input, .. } => {
                assert!(input.is_string());
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn unknown_role_defaults_to_user() {
        let msgs = vec![OpenAIMessage {
            role: "developer".into(),
            content: Some("hi".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn multi_turn_conversation_preserves_order() {
        let msgs = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("Be concise.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Hi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Bye".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.len(), 4);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[2].role, IrRole::Assistant);
        assert_eq!(conv.messages[3].role, IrRole::User);
    }

    #[test]
    fn empty_content_produces_empty_blocks() {
        let msgs = vec![OpenAIMessage {
            role: "user".into(),
            content: Some(String::new()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert!(conv.messages[0].content.is_empty());
    }

    #[test]
    fn none_content_produces_empty_blocks() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: None,
            tool_call_id: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert!(conv.messages[0].content.is_empty());
    }

    #[test]
    fn tool_def_roundtrip() {
        let canonical = CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        };
        let openai = dialect::tool_def_to_openai(&canonical);
        let back = dialect::tool_def_from_openai(&openai);
        assert_eq!(canonical, back);
    }

    #[test]
    fn openai_response_maps_to_agent_events() {
        let resp = OpenAIResponse {
            id: "chatcmpl-test".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIMessage {
                    role: "assistant".into(),
                    content: Some("The answer is 42.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        };
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            abp_core::AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text, "The answer is 42.");
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        }
    }

    #[test]
    fn openai_response_with_tool_calls_maps_to_events() {
        let resp = OpenAIResponse {
            id: "chatcmpl-tc".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![OpenAIToolCall {
                        id: "call_evt".into(),
                        call_type: "function".into(),
                        function: OpenAIFunctionCall {
                            name: "search".into(),
                            arguments: r#"{"q":"rust"}"#.into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
        };
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            abp_core::AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "search");
                assert_eq!(tool_use_id.as_deref(), Some("call_evt"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn canonical_model_roundtrip() {
        let vendor = "gpt-4o";
        let canonical = dialect::to_canonical_model(vendor);
        assert_eq!(canonical, "openai/gpt-4o");
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, vendor);
    }
}

// ── 2. Claude dialect → ABP IR mapping ──────────────────────────────────

mod claude_to_ir {
    use abp_claude_sdk::dialect::{
        self, CanonicalToolDef, ClaudeContentBlock, ClaudeImageSource, ClaudeMessage,
        ClaudeResponse, ClaudeStreamDelta, ClaudeStreamEvent, ClaudeUsage,
    };
    use abp_claude_sdk::lowering;
    use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
    use serde_json::json;

    #[test]
    fn user_text_to_ir() {
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "Hello from Claude".into(),
        }];
        let conv = lowering::to_ir(&msgs, None);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello from Claude");
    }

    #[test]
    fn system_prompt_becomes_system_message() {
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "Hi".into(),
        }];
        let conv = lowering::to_ir(&msgs, Some("You are a helpful assistant."));
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(
            conv.messages[0].text_content(),
            "You are a helpful assistant."
        );
        assert_eq!(conv.messages[1].role, IrRole::User);
    }

    #[test]
    fn empty_system_prompt_is_skipped() {
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "Hi".into(),
        }];
        let conv = lowering::to_ir(&msgs, Some(""));
        assert_eq!(conv.len(), 1);
    }

    #[test]
    fn tool_use_content_block_to_ir() {
        let blocks = vec![ClaudeContentBlock::ToolUse {
            id: "tu_abc".into(),
            name: "write_file".into(),
            input: json!({"path": "out.txt", "content": "hello"}),
        }];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = lowering::to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_abc");
                assert_eq!(name, "write_file");
                assert_eq!(input["path"], "out.txt");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_content_block_to_ir() {
        let blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_abc".into(),
            content: Some("file written successfully".into()),
            is_error: None,
        }];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = lowering::to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tu_abc");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_with_error_flag() {
        let blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_err".into(),
            content: Some("file not found".into()),
            is_error: Some(true),
        }];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = lowering::to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn thinking_block_to_ir() {
        let blocks = vec![
            ClaudeContentBlock::Thinking {
                thinking: "Let me reason through this...".into(),
                signature: Some("sig_xyz".into()),
            },
            ClaudeContentBlock::Text {
                text: "The answer is 42.".into(),
            },
        ];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = lowering::to_ir(&msgs, None);
        assert_eq!(conv.messages[0].content.len(), 2);
        match &conv.messages[0].content[0] {
            IrContentBlock::Thinking { text } => {
                assert_eq!(text, "Let me reason through this...");
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
        match &conv.messages[0].content[1] {
            IrContentBlock::Text { text } => assert_eq!(text, "The answer is 42."),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn image_block_base64_to_ir() {
        let blocks = vec![ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        }];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = lowering::to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "iVBORw0KGgo=");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    #[test]
    fn image_url_becomes_text_placeholder() {
        let blocks = vec![ClaudeContentBlock::Image {
            source: ClaudeImageSource::Url {
                url: "https://example.com/img.png".into(),
            },
        }];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = lowering::to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::Text { text } => {
                assert!(text.contains("https://example.com/img.png"));
            }
            other => panic!("expected Text placeholder, got {other:?}"),
        }
    }

    #[test]
    fn system_messages_skipped_in_from_ir() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "instructions"),
            IrMessage::text(IrRole::User, "hello"),
        ]);
        let back = lowering::from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
    }

    #[test]
    fn extract_system_prompt_from_ir() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You are helpful."),
            IrMessage::text(IrRole::User, "hi"),
        ]);
        let sys = lowering::extract_system_prompt(&conv);
        assert_eq!(sys.as_deref(), Some("You are helpful."));
    }

    #[test]
    fn claude_response_maps_to_agent_events() {
        let resp = ClaudeResponse {
            id: "msg_test".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![
                ClaudeContentBlock::Text {
                    text: "Here you go.".into(),
                },
                ClaudeContentBlock::ToolUse {
                    id: "tu_resp".into(),
                    name: "bash".into(),
                    input: json!({"command": "ls"}),
                },
            ],
            stop_reason: Some("tool_use".into()),
            usage: Some(ClaudeUsage {
                input_tokens: 100,
                output_tokens: 50,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        };
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0].kind,
            abp_core::AgentEventKind::AssistantMessage { text } if text == "Here you go."
        ));
        assert!(matches!(
            &events[1].kind,
            abp_core::AgentEventKind::ToolCall { tool_name, .. } if tool_name == "bash"
        ));
    }

    #[test]
    fn stream_text_delta_maps_to_event() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "partial ".into(),
            },
        };
        let mapped = dialect::map_stream_event(&event);
        assert_eq!(mapped.len(), 1);
        assert!(matches!(
            &mapped[0].kind,
            abp_core::AgentEventKind::AssistantDelta { text } if text == "partial "
        ));
    }

    #[test]
    fn canonical_model_roundtrip() {
        let vendor = "claude-sonnet-4-20250514";
        let canonical = dialect::to_canonical_model(vendor);
        assert_eq!(canonical, "anthropic/claude-sonnet-4-20250514");
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, vendor);
    }

    #[test]
    fn tool_def_roundtrip() {
        let canonical = CanonicalToolDef {
            name: "search".into(),
            description: "Search the web".into(),
            parameters_schema: json!({"type": "object"}),
        };
        let claude = dialect::tool_def_to_claude(&canonical);
        assert_eq!(claude.name, "search");
        assert_eq!(claude.input_schema, json!({"type": "object"}));
        let back = dialect::tool_def_from_claude(&claude);
        assert_eq!(canonical, back);
    }
}

// ── 3. Gemini dialect → ABP IR mapping ──────────────────────────────────

mod gemini_to_ir {
    use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
    use abp_gemini_sdk::dialect::{
        self, CanonicalToolDef, GeminiContent, GeminiInlineData, GeminiPart, GeminiSafetyRating,
        GeminiSafetySetting, HarmBlockThreshold, HarmCategory, HarmProbability,
    };
    use abp_gemini_sdk::lowering;
    use serde_json::json;

    #[test]
    fn user_text_to_ir() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hello Gemini".into())],
        }];
        let conv = lowering::to_ir(&contents, None);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello Gemini");
    }

    #[test]
    fn model_role_maps_to_assistant() {
        let contents = vec![GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("Response".into())],
        }];
        let conv = lowering::to_ir(&contents, None);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
    }

    #[test]
    fn system_instruction_to_ir() {
        let sys = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Be concise and helpful.".into())],
        };
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hi".into())],
        }];
        let conv = lowering::to_ir(&contents, Some(&sys));
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be concise and helpful.");
    }

    #[test]
    fn function_call_part_to_ir() {
        let contents = vec![GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "get_weather".into(),
                args: json!({"location": "London", "unit": "celsius"}),
            }],
        }];
        let conv = lowering::to_ir(&contents, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(name, "get_weather");
                assert_eq!(input["location"], "London");
                assert!(id.starts_with("gemini_"));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn function_response_part_to_ir() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "get_weather".into(),
                response: json!("Sunny, 22°C"),
            }],
        }];
        let conv = lowering::to_ir(&contents, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "gemini_get_weather");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn inline_data_to_ir_image() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/jpeg".into(),
                data: "base64_jpeg_data".into(),
            })],
        }];
        let conv = lowering::to_ir(&contents, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/jpeg");
                assert_eq!(data, "base64_jpeg_data");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    #[test]
    fn multiple_parts_in_one_content() {
        let contents = vec![GeminiContent {
            role: "model".into(),
            parts: vec![
                GeminiPart::Text("I'll search for that.".into()),
                GeminiPart::FunctionCall {
                    name: "search".into(),
                    args: json!({"query": "rust programming"}),
                },
            ],
        }];
        let conv = lowering::to_ir(&contents, None);
        assert_eq!(conv.messages[0].content.len(), 2);
        assert!(matches!(
            &conv.messages[0].content[0],
            IrContentBlock::Text { .. }
        ));
        assert!(matches!(
            &conv.messages[0].content[1],
            IrContentBlock::ToolUse { .. }
        ));
    }

    #[test]
    fn safety_settings_serde_roundtrip() {
        let setting = GeminiSafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        };
        let json = serde_json::to_string(&setting).unwrap();
        let decoded: GeminiSafetySetting = serde_json::from_str(&json).unwrap();
        assert_eq!(setting, decoded);
    }

    #[test]
    fn safety_rating_variants() {
        let rating = GeminiSafetyRating {
            category: HarmCategory::HarmCategoryDangerousContent,
            probability: HarmProbability::Negligible,
        };
        let json = serde_json::to_string(&rating).unwrap();
        let decoded: GeminiSafetyRating = serde_json::from_str(&json).unwrap();
        assert_eq!(rating, decoded);
    }

    #[test]
    fn system_messages_skipped_in_from_ir() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "instructions"),
            IrMessage::text(IrRole::User, "hello"),
        ]);
        let back = lowering::from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
    }

    #[test]
    fn extract_system_instruction_from_ir() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be concise"),
            IrMessage::text(IrRole::User, "hi"),
        ]);
        let sys = lowering::extract_system_instruction(&conv).unwrap();
        match &sys.parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Be concise"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn function_response_object_payload_serialized() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "api".into(),
                response: json!({"status": 200, "body": "ok"}),
            }],
        }];
        let conv = lowering::to_ir(&contents, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { content, .. } => {
                let text = match &content[0] {
                    IrContentBlock::Text { text } => text.as_str(),
                    _ => panic!("expected text block"),
                };
                assert!(text.contains("200"));
                assert!(text.contains("ok"));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn canonical_model_roundtrip() {
        let vendor = "gemini-2.5-flash";
        let canonical = dialect::to_canonical_model(vendor);
        assert_eq!(canonical, "google/gemini-2.5-flash");
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, vendor);
    }

    #[test]
    fn tool_def_roundtrip() {
        let canonical = CanonicalToolDef {
            name: "fetch".into(),
            description: "Fetch a URL".into(),
            parameters_schema: json!({"type": "object", "properties": {"url": {"type": "string"}}}),
        };
        let gemini = dialect::tool_def_to_gemini(&canonical);
        assert_eq!(gemini.name, "fetch");
        let back = dialect::tool_def_from_gemini(&gemini);
        assert_eq!(canonical, back);
    }
}

// ── 4. Kimi dialect → ABP IR mapping ────────────────────────────────────

mod kimi_to_ir {
    use abp_core::ir::{IrContentBlock, IrRole};
    use abp_kimi_sdk::dialect::{
        self, CanonicalToolDef, KimiBuiltinTool, KimiFunctionCall, KimiMessage, KimiRef,
        KimiToolCall, KimiUsage,
    };
    use abp_kimi_sdk::lowering;
    use serde_json::json;

    #[test]
    fn user_text_to_ir() {
        let msgs = vec![KimiMessage {
            role: "user".into(),
            content: Some("Hello Kimi".into()),
            tool_call_id: None,
            tool_calls: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello Kimi");
    }

    #[test]
    fn system_message_to_ir() {
        let msgs = vec![KimiMessage {
            role: "system".into(),
            content: Some("You are Kimi.".into()),
            tool_call_id: None,
            tool_calls: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    #[test]
    fn tool_call_to_ir() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "call_kimi_1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "web_search".into(),
                    arguments: r#"{"query":"rust async runtime"}"#.into(),
                },
            }]),
        }];
        let conv = lowering::to_ir(&msgs);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_kimi_1");
                assert_eq!(name, "web_search");
                assert_eq!(input["query"], "rust async runtime");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_to_ir() {
        let msgs = vec![KimiMessage {
            role: "tool".into(),
            content: Some("search results...".into()),
            tool_call_id: Some("call_kimi_1".into()),
            tool_calls: None,
        }];
        let conv = lowering::to_ir(&msgs);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "call_kimi_1");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn usage_to_ir_computes_total() {
        let usage = KimiUsage {
            prompt_tokens: 150,
            completion_tokens: 75,
            total_tokens: 225,
        };
        let ir = lowering::usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 150);
        assert_eq!(ir.output_tokens, 75);
        assert_eq!(ir.total_tokens, 225);
    }

    #[test]
    fn builtin_search_internet_serde() {
        let tool = dialect::builtin_search_internet();
        assert_eq!(tool.tool_type, "builtin_function");
        assert_eq!(tool.function.name, "$web_search");
        let json = serde_json::to_string(&tool).unwrap();
        let decoded: KimiBuiltinTool = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, decoded);
    }

    #[test]
    fn kimi_ref_serde() {
        let r = KimiRef {
            index: 1,
            url: "https://example.com".into(),
            title: Some("Example".into()),
        };
        let json = serde_json::to_string(&r).unwrap();
        let decoded: KimiRef = serde_json::from_str(&json).unwrap();
        assert_eq!(r, decoded);
    }

    #[test]
    fn malformed_tool_arguments_preserved() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "call_bad".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "foo".into(),
                    arguments: "not valid json".into(),
                },
            }]),
        }];
        let conv = lowering::to_ir(&msgs);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { input, .. } => {
                assert!(input.is_string());
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn canonical_model_roundtrip() {
        let vendor = "moonshot-v1-8k";
        let canonical = dialect::to_canonical_model(vendor);
        assert_eq!(canonical, "moonshot/moonshot-v1-8k");
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, vendor);
    }

    #[test]
    fn tool_def_roundtrip() {
        let canonical = CanonicalToolDef {
            name: "web_search".into(),
            description: "Search the web".into(),
            parameters_schema: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        };
        let kimi = dialect::tool_def_to_kimi(&canonical);
        assert_eq!(kimi.function.name, "web_search");
        let back = dialect::tool_def_from_kimi(&kimi);
        assert_eq!(canonical, back);
    }

    #[test]
    fn unknown_role_defaults_to_user() {
        let msgs = vec![KimiMessage {
            role: "developer".into(),
            content: Some("hi".into()),
            tool_call_id: None,
            tool_calls: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn none_content_produces_empty_blocks() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert!(conv.messages[0].content.is_empty());
    }
}

// ── 5. Round-trip fidelity: SDK → IR → SDK ──────────────────────────────

mod roundtrip_fidelity {
    use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
    use abp_claude_sdk::lowering as claude_lowering;
    use abp_gemini_sdk::dialect::{GeminiContent, GeminiInlineData, GeminiPart};
    use abp_gemini_sdk::lowering as gemini_lowering;
    use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall};
    use abp_kimi_sdk::lowering as kimi_lowering;
    use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
    use abp_openai_sdk::lowering as openai_lowering;
    use serde_json::json;

    #[test]
    fn openai_text_roundtrip() {
        let original = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("Be helpful.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: Some("Hi there!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let conv = openai_lowering::to_ir(&original);
        let recovered = openai_lowering::from_ir(&conv);

        assert_eq!(recovered.len(), original.len());
        for (orig, rec) in original.iter().zip(recovered.iter()) {
            assert_eq!(orig.role, rec.role);
            assert_eq!(orig.content, rec.content);
        }
    }

    #[test]
    fn openai_tool_call_roundtrip() {
        let original = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_rt".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }];
        let conv = openai_lowering::to_ir(&original);
        let recovered = openai_lowering::from_ir(&conv);

        assert!(recovered[0].content.is_none());
        let tc = &recovered[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id, "call_rt");
        assert_eq!(tc.function.name, "search");
    }

    #[test]
    fn openai_tool_result_roundtrip() {
        let original = vec![OpenAIMessage {
            role: "tool".into(),
            content: Some("result data".into()),
            tool_calls: None,
            tool_call_id: Some("call_rt".into()),
        }];
        let conv = openai_lowering::to_ir(&original);
        let recovered = openai_lowering::from_ir(&conv);

        assert_eq!(recovered[0].role, "tool");
        assert_eq!(recovered[0].content.as_deref(), Some("result data"));
        assert_eq!(recovered[0].tool_call_id.as_deref(), Some("call_rt"));
    }

    #[test]
    fn claude_text_roundtrip() {
        let original = vec![
            ClaudeMessage {
                role: "user".into(),
                content: "Hello Claude".into(),
            },
            ClaudeMessage {
                role: "assistant".into(),
                content: "Hello!".into(),
            },
        ];
        let conv = claude_lowering::to_ir(&original, Some("Be helpful"));
        let recovered = claude_lowering::from_ir(&conv);

        // System message is extracted separately (not in messages)
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].role, "user");
        assert_eq!(recovered[0].content, "Hello Claude");
        assert_eq!(recovered[1].role, "assistant");
    }

    #[test]
    fn claude_tool_use_roundtrip() {
        let blocks = vec![ClaudeContentBlock::ToolUse {
            id: "tu_rt".into(),
            name: "grep".into(),
            input: json!({"pattern": "fn main"}),
        }];
        let original = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = claude_lowering::to_ir(&original, None);
        let recovered = claude_lowering::from_ir(&conv);

        let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&recovered[0].content).unwrap();
        match &parsed[0] {
            ClaudeContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "tu_rt");
                assert_eq!(name, "grep");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn gemini_text_roundtrip() {
        let original = vec![
            GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hello".into())],
            },
            GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Hi!".into())],
            },
        ];
        let conv = gemini_lowering::to_ir(&original, None);
        let recovered = gemini_lowering::from_ir(&conv);

        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].role, "user");
        assert_eq!(recovered[1].role, "model");
        match &recovered[0].parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Hello"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn gemini_function_call_roundtrip() {
        let original = vec![GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "read".into(),
                args: json!({"file": "a.rs"}),
            }],
        }];
        let conv = gemini_lowering::to_ir(&original, None);
        let recovered = gemini_lowering::from_ir(&conv);

        match &recovered[0].parts[0] {
            GeminiPart::FunctionCall { name, args } => {
                assert_eq!(name, "read");
                assert_eq!(args, &json!({"file": "a.rs"}));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn gemini_inline_data_roundtrip() {
        let original = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/png".into(),
                data: "abc123".into(),
            })],
        }];
        let conv = gemini_lowering::to_ir(&original, None);
        let recovered = gemini_lowering::from_ir(&conv);

        match &recovered[0].parts[0] {
            GeminiPart::InlineData(d) => {
                assert_eq!(d.mime_type, "image/png");
                assert_eq!(d.data, "abc123");
            }
            other => panic!("expected InlineData, got {other:?}"),
        }
    }

    #[test]
    fn kimi_text_roundtrip() {
        let original = vec![
            KimiMessage {
                role: "system".into(),
                content: Some("Be concise.".into()),
                tool_call_id: None,
                tool_calls: None,
            },
            KimiMessage {
                role: "user".into(),
                content: Some("Hi".into()),
                tool_call_id: None,
                tool_calls: None,
            },
        ];
        let conv = kimi_lowering::to_ir(&original);
        let recovered = kimi_lowering::from_ir(&conv);

        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].role, "system");
        assert_eq!(recovered[0].content.as_deref(), Some("Be concise."));
        assert_eq!(recovered[1].role, "user");
        assert_eq!(recovered[1].content.as_deref(), Some("Hi"));
    }

    #[test]
    fn kimi_tool_call_roundtrip() {
        let original = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "call_krt".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"test"}"#.into(),
                },
            }]),
        }];
        let conv = kimi_lowering::to_ir(&original);
        let recovered = kimi_lowering::from_ir(&conv);

        assert!(recovered[0].content.is_none());
        let tc = &recovered[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id, "call_krt");
        assert_eq!(tc.function.name, "search");
    }

    #[test]
    fn empty_conversation_roundtrip_all_dialects() {
        // OpenAI
        let conv = openai_lowering::to_ir(&[]);
        assert!(conv.is_empty());
        assert!(openai_lowering::from_ir(&conv).is_empty());

        // Claude
        let conv = claude_lowering::to_ir(&[], None);
        assert!(conv.is_empty());
        assert!(claude_lowering::from_ir(&conv).is_empty());

        // Gemini
        let conv = gemini_lowering::to_ir(&[], None);
        assert!(conv.is_empty());
        assert!(gemini_lowering::from_ir(&conv).is_empty());

        // Kimi
        let conv = kimi_lowering::to_ir(&[]);
        assert!(conv.is_empty());
        assert!(kimi_lowering::from_ir(&conv).is_empty());
    }
}

// ── 6. Error cases: unmappable features → typed errors ──────────────────

mod error_cases {
    use abp_dialect::Dialect;
    use abp_mapping::{Fidelity, MappingError, features, known_rules, validate_mapping};

    #[test]
    fn feature_unsupported_error_for_unknown_feature() {
        let registry = known_rules();
        let results = validate_mapping(
            &registry,
            Dialect::OpenAi,
            Dialect::Claude,
            &["teleportation".into()],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
        assert!(!results[0].errors.is_empty());
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FeatureUnsupported { feature, .. } if feature == "teleportation"
        ));
    }

    #[test]
    fn empty_feature_name_is_invalid_input() {
        let registry = known_rules();
        let results = validate_mapping(&registry, Dialect::OpenAi, Dialect::Claude, &["".into()]);
        assert_eq!(results.len(), 1);
        assert!(matches!(
            &results[0].errors[0],
            MappingError::InvalidInput { reason } if reason.contains("empty")
        ));
    }

    #[test]
    fn image_input_unsupported_openai_to_codex() {
        let registry = known_rules();
        let results = validate_mapping(
            &registry,
            Dialect::OpenAi,
            Dialect::Codex,
            &[features::IMAGE_INPUT.into()],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn code_exec_unsupported_from_kimi() {
        let registry = known_rules();
        let results = validate_mapping(
            &registry,
            Dialect::Kimi,
            Dialect::OpenAi,
            &[features::CODE_EXEC.into()],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn fidelity_loss_produces_warning_in_validation() {
        let registry = known_rules();
        let results = validate_mapping(
            &registry,
            Dialect::Claude,
            Dialect::OpenAi,
            &[features::THINKING.into()],
        );
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].fidelity, Fidelity::LossyLabeled { .. }));
        assert!(!results[0].errors.is_empty());
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FidelityLoss { feature, .. } if feature == "thinking"
        ));
    }

    #[test]
    fn mapping_error_serde_roundtrip() {
        let errors = vec![
            MappingError::FeatureUnsupported {
                feature: "logprobs".into(),
                from: Dialect::Claude,
                to: Dialect::Gemini,
            },
            MappingError::FidelityLoss {
                feature: "thinking".into(),
                warning: "lossy mapping".into(),
            },
            MappingError::DialectMismatch {
                from: Dialect::OpenAi,
                to: Dialect::Kimi,
            },
            MappingError::InvalidInput {
                reason: "bad input".into(),
            },
        ];
        for err in &errors {
            let json = serde_json::to_string(err).unwrap();
            let decoded: MappingError = serde_json::from_str(&json).unwrap();
            assert_eq!(*err, decoded);
        }
    }

    #[test]
    fn multiple_features_validated_at_once() {
        let registry = known_rules();
        let results = validate_mapping(
            &registry,
            Dialect::OpenAi,
            Dialect::Codex,
            &[
                features::TOOL_USE.into(),
                features::STREAMING.into(),
                features::IMAGE_INPUT.into(),
            ],
        );
        assert_eq!(results.len(), 3);

        // tool_use: lossy (OpenAI -> Codex)
        assert!(matches!(results[0].fidelity, Fidelity::LossyLabeled { .. }));
        // streaming: lossless
        assert!(results[1].fidelity.is_lossless());
        // image_input: unsupported
        assert!(results[2].fidelity.is_unsupported());
    }

    #[test]
    fn mapping_error_display_is_informative() {
        let err = MappingError::FeatureUnsupported {
            feature: "logprobs".into(),
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        let display = err.to_string();
        assert!(display.contains("logprobs"));
        assert!(display.contains("unsupported"));
    }
}

// ── 7. Capability downgrade paths and emulation labeling ────────────────

mod capability_downgrade {
    use abp_core::Capability;
    use abp_core::ir::{IrConversation, IrMessage, IrRole};
    use abp_emulation::{
        EmulationConfig, EmulationEngine, EmulationStrategy, FidelityLabel, apply_emulation,
        can_emulate, compute_fidelity, default_strategy,
    };

    #[test]
    fn extended_thinking_emulated_via_system_prompt() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Base system prompt"))
            .push(IrMessage::text(IrRole::User, "Think about this carefully"));

        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

        assert_eq!(report.applied.len(), 1);
        assert!(report.warnings.is_empty());

        let sys = conv.system_message().unwrap();
        let text = sys.text_content();
        assert!(text.contains("Think step by step"));
        assert!(text.contains("Base system prompt"));
    }

    #[test]
    fn code_execution_disabled_generates_warning() {
        let engine = EmulationEngine::with_defaults();
        let mut conv =
            IrConversation::new().push(IrMessage::text(IrRole::User, "Execute this code"));

        let report = engine.apply(&[Capability::CodeExecution], &mut conv);

        assert!(report.applied.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("not emulated"));
    }

    #[test]
    fn structured_output_emulated_via_post_processing() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Return JSON"));

        let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

        assert_eq!(report.applied.len(), 1);
        assert!(matches!(
            report.applied[0].strategy,
            EmulationStrategy::PostProcessing { .. }
        ));
    }

    #[test]
    fn post_processing_does_not_mutate_conversation() {
        let original = IrConversation::new().push(IrMessage::text(IrRole::User, "Return JSON"));
        let mut conv = original.clone();

        let engine = EmulationEngine::with_defaults();
        engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

        assert_eq!(conv, original);
    }

    #[test]
    fn config_override_replaces_default() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::CodeExecution,
            EmulationStrategy::SystemPromptInjection {
                prompt: "Simulate code execution carefully.".into(),
            },
        );

        let engine = EmulationEngine::new(config);
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Run this code"));

        let report = engine.apply(&[Capability::CodeExecution], &mut conv);

        assert_eq!(report.applied.len(), 1);
        assert!(report.warnings.is_empty());
        assert!(conv.system_message().is_some());
    }

    #[test]
    fn multiple_capabilities_emulated_at_once() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Original."))
            .push(IrMessage::text(IrRole::User, "Complex request"));

        let report = engine.apply(
            &[
                Capability::ExtendedThinking,
                Capability::StructuredOutputJsonSchema,
                Capability::CodeExecution,
            ],
            &mut conv,
        );

        assert_eq!(report.applied.len(), 2); // thinking + structured output
        assert_eq!(report.warnings.len(), 1); // code execution disabled
    }

    #[test]
    fn compute_fidelity_labels_native_and_emulated() {
        let engine = EmulationEngine::with_defaults();
        let native = vec![Capability::Streaming, Capability::ToolRead];
        let report = engine.check_missing(&[Capability::ExtendedThinking]);

        let labels = compute_fidelity(&native, &report);

        assert!(matches!(
            labels.get(&Capability::Streaming),
            Some(FidelityLabel::Native)
        ));
        assert!(matches!(
            labels.get(&Capability::ToolRead),
            Some(FidelityLabel::Native)
        ));
        assert!(matches!(
            labels.get(&Capability::ExtendedThinking),
            Some(FidelityLabel::Emulated { .. })
        ));
    }

    #[test]
    fn can_emulate_predicate() {
        assert!(can_emulate(&Capability::ExtendedThinking));
        assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
        assert!(can_emulate(&Capability::ImageInput));
        assert!(can_emulate(&Capability::StopSequences));
        assert!(!can_emulate(&Capability::CodeExecution));
        assert!(!can_emulate(&Capability::Streaming));
        assert!(!can_emulate(&Capability::ToolUse));
    }

    #[test]
    fn default_strategy_variants() {
        assert!(matches!(
            default_strategy(&Capability::ExtendedThinking),
            EmulationStrategy::SystemPromptInjection { .. }
        ));
        assert!(matches!(
            default_strategy(&Capability::StructuredOutputJsonSchema),
            EmulationStrategy::PostProcessing { .. }
        ));
        assert!(matches!(
            default_strategy(&Capability::CodeExecution),
            EmulationStrategy::Disabled { .. }
        ));
        assert!(matches!(
            default_strategy(&Capability::Streaming),
            EmulationStrategy::Disabled { .. }
        ));
    }

    #[test]
    fn emulation_report_serde_roundtrip() {
        use abp_emulation::EmulationEntry;

        let report = abp_emulation::EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "Think carefully.".into(),
                },
            }],
            warnings: vec!["CodeExecution not available".into()],
        };

        let json = serde_json::to_string(&report).unwrap();
        let decoded: abp_emulation::EmulationReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, decoded);
    }

    #[test]
    fn system_prompt_injection_creates_system_when_missing() {
        let engine = EmulationEngine::with_defaults();
        let mut conv =
            IrConversation::new().push(IrMessage::text(IrRole::User, "No system message"));

        engine.apply(&[Capability::ExtendedThinking], &mut conv);

        assert_eq!(conv.messages[0].role, IrRole::System);
        assert!(
            conv.messages[0]
                .text_content()
                .contains("Think step by step")
        );
    }

    #[test]
    fn free_function_apply_emulation_works() {
        let config = EmulationConfig::new();
        let mut conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "base"))
            .push(IrMessage::text(IrRole::User, "hi"));

        let report = apply_emulation(&config, &[Capability::ExtendedThinking], &mut conv);
        assert_eq!(report.applied.len(), 1);
    }

    #[test]
    fn empty_capabilities_produces_empty_report() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = IrConversation::new();
        let report = engine.apply(&[], &mut conv);
        assert!(report.is_empty());
    }
}

// ── 8. Cross-dialect mapping rules and registry ─────────────────────────

mod mapping_registry {
    use abp_dialect::Dialect;
    use abp_mapping::{
        Fidelity, MappingMatrix, MappingRegistry, MappingRule, features, known_rules,
    };

    #[test]
    fn known_rules_is_non_empty() {
        let registry = known_rules();
        assert!(!registry.is_empty());
    }

    #[test]
    fn same_dialect_is_always_lossless() {
        let registry = known_rules();
        for &d in Dialect::all() {
            for &f in &[
                features::TOOL_USE,
                features::STREAMING,
                features::THINKING,
                features::IMAGE_INPUT,
                features::CODE_EXEC,
            ] {
                let rule = registry.lookup(d, d, f);
                assert!(rule.is_some(), "missing same-dialect rule for {d:?}/{f}");
                assert!(
                    rule.unwrap().fidelity.is_lossless(),
                    "same-dialect rule for {d:?}/{f} should be lossless"
                );
            }
        }
    }

    #[test]
    fn openai_claude_tool_use_is_lossless() {
        let registry = known_rules();
        let rule = registry
            .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn openai_claude_streaming_is_lossless() {
        let registry = known_rules();
        let rule = registry
            .lookup(Dialect::OpenAi, Dialect::Claude, features::STREAMING)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn claude_openai_thinking_is_lossy() {
        let registry = known_rules();
        let rule = registry
            .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
            .unwrap();
        assert!(matches!(rule.fidelity, Fidelity::LossyLabeled { .. }));
    }

    #[test]
    fn codex_tool_use_is_lossy_bidirectional() {
        let registry = known_rules();

        let to_codex = registry
            .lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE)
            .unwrap();
        assert!(matches!(to_codex.fidelity, Fidelity::LossyLabeled { .. }));

        let from_codex = registry
            .lookup(Dialect::Codex, Dialect::OpenAi, features::TOOL_USE)
            .unwrap();
        assert!(matches!(from_codex.fidelity, Fidelity::LossyLabeled { .. }));
    }

    #[test]
    fn image_input_unsupported_to_codex() {
        let registry = known_rules();
        for &source in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            let rule = registry
                .lookup(source, Dialect::Codex, features::IMAGE_INPUT)
                .unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "image_input {source:?}->Codex should be unsupported"
            );
        }
    }

    #[test]
    fn kimi_copilot_tool_use_lossless() {
        let registry = known_rules();
        let rule = registry
            .lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn kimi_code_exec_unsupported() {
        let registry = known_rules();
        let rule = registry.lookup(Dialect::Kimi, Dialect::OpenAi, features::CODE_EXEC);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_unsupported());
    }

    #[test]
    fn mapping_matrix_from_registry() {
        let registry = known_rules();
        let matrix = MappingMatrix::from_registry(&registry);

        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert!(matrix.is_supported(Dialect::Claude, Dialect::Gemini));
        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Gemini));
    }

    #[test]
    fn rank_targets_returns_best_matches() {
        let registry = known_rules();
        let ranked =
            registry.rank_targets(Dialect::OpenAi, &[features::TOOL_USE, features::STREAMING]);
        assert!(!ranked.is_empty());
        // Claude should be in the results with lossless for both
        let claude_rank = ranked.iter().find(|(d, _)| *d == Dialect::Claude);
        assert!(claude_rank.is_some());
        assert_eq!(claude_rank.unwrap().1, 2); // both lossless
    }

    #[test]
    fn registry_insert_replaces_existing() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "test".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "test".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "changed".into(),
            },
        });
        assert_eq!(reg.len(), 1);
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, "test")
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn fidelity_serde_roundtrip() {
        let variants = vec![
            Fidelity::Lossless,
            Fidelity::LossyLabeled {
                warning: "some loss".into(),
            },
            Fidelity::Unsupported {
                reason: "not possible".into(),
            },
        ];
        for f in &variants {
            let json = serde_json::to_string(f).unwrap();
            let decoded: Fidelity = serde_json::from_str(&json).unwrap();
            assert_eq!(*f, decoded);
        }
    }
}

// ── 9. Dialect detection ────────────────────────────────────────────────

mod dialect_detection {
    use abp_dialect::{Dialect, DialectDetector};
    use serde_json::json;

    #[test]
    fn detect_openai_format() {
        let detector = DialectDetector::new();
        let payload = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello"}],
            "temperature": 0.7,
            "max_tokens": 1000
        });
        let result = detector.detect(&payload).unwrap();
        assert_eq!(result.dialect, Dialect::OpenAi);
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn detect_claude_format() {
        let detector = DialectDetector::new();
        let payload = json!({
            "type": "message",
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}],
            "stop_reason": "end_turn"
        });
        let result = detector.detect(&payload).unwrap();
        assert_eq!(result.dialect, Dialect::Claude);
    }

    #[test]
    fn detect_gemini_format() {
        let detector = DialectDetector::new();
        let payload = json!({
            "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
            "generationConfig": {"maxOutputTokens": 1000}
        });
        let result = detector.detect(&payload).unwrap();
        assert_eq!(result.dialect, Dialect::Gemini);
    }

    #[test]
    fn non_object_returns_none() {
        let detector = DialectDetector::new();
        assert!(detector.detect(&json!("string")).is_none());
        assert!(detector.detect(&json!(42)).is_none());
        assert!(detector.detect(&json!([1, 2, 3])).is_none());
    }

    #[test]
    fn detect_all_returns_multiple_candidates() {
        let detector = DialectDetector::new();
        let payload = json!({
            "model": "test",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.5
        });
        let results = detector.detect_all(&payload);
        assert!(!results.is_empty());
        // Results should be sorted by descending confidence
        for w in results.windows(2) {
            assert!(w[0].confidence >= w[1].confidence);
        }
    }
}

// ── 10. IR type fundamentals ────────────────────────────────────────────

mod ir_fundamentals {
    use abp_core::ir::{
        IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage,
    };
    use serde_json::json;

    #[test]
    fn ir_message_text_helper() {
        let msg = IrMessage::text(IrRole::User, "Hello");
        assert_eq!(msg.role, IrRole::User);
        assert_eq!(msg.text_content(), "Hello");
        assert!(msg.is_text_only());
    }

    #[test]
    fn ir_message_tool_use_blocks() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "read".into(),
                    input: json!({}),
                },
                IrContentBlock::ToolUse {
                    id: "t2".into(),
                    name: "write".into(),
                    input: json!({}),
                },
            ],
        );
        let tools = msg.tool_use_blocks();
        assert_eq!(tools.len(), 2);
        assert!(!msg.is_text_only());
    }

    #[test]
    fn ir_conversation_chaining() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be helpful"))
            .push(IrMessage::text(IrRole::User, "Hi"))
            .push(IrMessage::text(IrRole::Assistant, "Hello!"));

        assert_eq!(conv.len(), 3);
        assert!(!conv.is_empty());
        assert_eq!(conv.system_message().unwrap().text_content(), "Be helpful");
        assert_eq!(conv.last_assistant().unwrap().text_content(), "Hello!");
        assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
    }

    #[test]
    fn ir_conversation_tool_calls_across_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "Do stuff"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "a".into(),
                        input: json!({}),
                    },
                    IrContentBlock::ToolUse {
                        id: "t2".into(),
                        name: "b".into(),
                        input: json!({}),
                    },
                ],
            ));
        let all_tools = conv.tool_calls();
        assert_eq!(all_tools.len(), 2);
    }

    #[test]
    fn ir_usage_from_io() {
        let usage = IrUsage::from_io(100, 50);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_write_tokens, 0);
    }

    #[test]
    fn ir_usage_with_cache() {
        let usage = IrUsage::with_cache(100, 50, 20, 10);
        assert_eq!(usage.total_tokens, 150);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_write_tokens, 10);
    }

    #[test]
    fn ir_usage_merge() {
        let a = IrUsage::from_io(100, 50);
        let b = IrUsage::with_cache(200, 100, 30, 20);
        let merged = a.merge(b);
        assert_eq!(merged.input_tokens, 300);
        assert_eq!(merged.output_tokens, 150);
        assert_eq!(merged.total_tokens, 450);
        assert_eq!(merged.cache_read_tokens, 30);
        assert_eq!(merged.cache_write_tokens, 20);
    }

    #[test]
    fn ir_tool_definition_serde() {
        let def = IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }),
        };
        let json = serde_json::to_string(&def).unwrap();
        let decoded: IrToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(def, decoded);
    }

    #[test]
    fn ir_conversation_serde_roundtrip() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be helpful"))
            .push(IrMessage::text(IrRole::User, "Hello"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "Let me check.".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "search".into(),
                        input: json!({"q": "rust"}),
                    },
                ],
            ));

        let json = serde_json::to_string(&conv).unwrap();
        let decoded: IrConversation = serde_json::from_str(&json).unwrap();
        assert_eq!(conv, decoded);
    }

    #[test]
    fn ir_content_block_all_variants_serde() {
        let blocks = vec![
            IrContentBlock::Text {
                text: "hello".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: json!({"path": "a.rs"}),
            },
            IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            },
            IrContentBlock::Thinking {
                text: "reasoning...".into(),
            },
        ];
        for block in &blocks {
            let json = serde_json::to_string(block).unwrap();
            let decoded: IrContentBlock = serde_json::from_str(&json).unwrap();
            assert_eq!(*block, decoded);
        }
    }
}

// ── 11. Cross-dialect lowering (one dialect → IR → another) ─────────────

mod cross_dialect_lowering {
    use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
    use abp_claude_sdk::lowering as claude_lowering;
    use abp_core::ir::IrContentBlock;
    use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
    use abp_gemini_sdk::lowering as gemini_lowering;
    use abp_kimi_sdk::dialect::KimiMessage;
    use abp_kimi_sdk::lowering as kimi_lowering;
    use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
    use abp_openai_sdk::lowering as openai_lowering;
    use serde_json::json;

    #[test]
    fn openai_to_ir_to_claude() {
        let openai_msgs = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("Be helpful.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let ir = openai_lowering::to_ir(&openai_msgs);

        // Extract system prompt for Claude
        let sys = claude_lowering::extract_system_prompt(&ir);
        assert_eq!(sys.as_deref(), Some("Be helpful."));

        let claude_msgs = claude_lowering::from_ir(&ir);
        assert_eq!(claude_msgs.len(), 1); // System filtered out
        assert_eq!(claude_msgs[0].role, "user");
        assert_eq!(claude_msgs[0].content, "Hello");
    }

    #[test]
    fn openai_to_ir_to_gemini() {
        let openai_msgs = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("Be concise.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let ir = openai_lowering::to_ir(&openai_msgs);

        let sys_instr = gemini_lowering::extract_system_instruction(&ir);
        assert!(sys_instr.is_some());

        let gemini_contents = gemini_lowering::from_ir(&ir);
        assert_eq!(gemini_contents.len(), 1); // System filtered
        assert_eq!(gemini_contents[0].role, "user");
    }

    #[test]
    fn openai_tool_call_to_ir_to_kimi() {
        let openai_msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_cross".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"test"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }];
        let ir = openai_lowering::to_ir(&openai_msgs);
        let kimi_msgs = kimi_lowering::from_ir(&ir);

        assert_eq!(kimi_msgs[0].role, "assistant");
        let tc = &kimi_msgs[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id, "call_cross");
        assert_eq!(tc.function.name, "search");
    }

    #[test]
    fn claude_thinking_to_ir_to_openai() {
        let blocks = vec![
            ClaudeContentBlock::Thinking {
                thinking: "Let me reason...".into(),
                signature: None,
            },
            ClaudeContentBlock::Text {
                text: "The answer is 42.".into(),
            },
        ];
        let claude_msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let ir = claude_lowering::to_ir(&claude_msgs, None);
        let openai_msgs = openai_lowering::from_ir(&ir);

        assert_eq!(openai_msgs.len(), 1);
        assert_eq!(openai_msgs[0].role, "assistant");
        // Thinking blocks become text content in OpenAI (lossy but preserved)
        let content = openai_msgs[0].content.as_deref().unwrap();
        assert!(content.contains("Let me reason..."));
        assert!(content.contains("The answer is 42."));
    }

    #[test]
    fn gemini_function_call_to_ir_to_openai() {
        let gemini_contents = vec![GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "read_file".into(),
                args: json!({"path": "main.rs"}),
            }],
        }];
        let ir = gemini_lowering::to_ir(&gemini_contents, None);
        let openai_msgs = openai_lowering::from_ir(&ir);

        assert_eq!(openai_msgs[0].role, "assistant");
        let tc = &openai_msgs[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.function.name, "read_file");
        assert!(tc.id.starts_with("gemini_")); // Synthetic ID preserved
    }

    #[test]
    fn kimi_to_ir_to_gemini() {
        let kimi_msgs = vec![
            KimiMessage {
                role: "user".into(),
                content: Some("Search for rust".into()),
                tool_call_id: None,
                tool_calls: None,
            },
            KimiMessage {
                role: "assistant".into(),
                content: Some("Here are the results.".into()),
                tool_call_id: None,
                tool_calls: None,
            },
        ];
        let ir = kimi_lowering::to_ir(&kimi_msgs);
        let gemini = gemini_lowering::from_ir(&ir);

        assert_eq!(gemini.len(), 2);
        assert_eq!(gemini[0].role, "user");
        assert_eq!(gemini[1].role, "model"); // assistant → model
    }

    #[test]
    fn full_tool_cycle_openai_to_claude_and_back() {
        // OpenAI assistant requests a tool call
        let openai_msgs = vec![
            OpenAIMessage {
                role: "user".into(),
                content: Some("Read main.rs".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "call_full".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "read_file".into(),
                        arguments: r#"{"path":"main.rs"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "tool".into(),
                content: Some("fn main() {}".into()),
                tool_calls: None,
                tool_call_id: Some("call_full".into()),
            },
        ];

        // Convert to IR
        let ir = openai_lowering::to_ir(&openai_msgs);
        assert_eq!(ir.len(), 3);

        // Verify IR contains the tool cycle
        assert!(matches!(
            &ir.messages[1].content[0],
            IrContentBlock::ToolUse { name, .. } if name == "read_file"
        ));
        assert!(matches!(
            &ir.messages[2].content[0],
            IrContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "call_full"
        ));

        // Convert back to OpenAI
        let recovered = openai_lowering::from_ir(&ir);
        assert_eq!(recovered.len(), 3);
        assert_eq!(recovered[2].tool_call_id.as_deref(), Some("call_full"));
    }
}

// ── 12. Capability manifest coverage ────────────────────────────────────

mod capability_manifests {
    use abp_claude_sdk::dialect as claude_dialect;
    use abp_core::{Capability, SupportLevel};
    use abp_gemini_sdk::dialect as gemini_dialect;
    use abp_kimi_sdk::dialect as kimi_dialect;
    use abp_openai_sdk::dialect as openai_dialect;

    #[test]
    fn openai_manifest_has_streaming() {
        let m = openai_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn claude_manifest_has_native_tool_read() {
        let m = claude_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolRead),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn gemini_manifest_glob_unsupported() {
        let m = gemini_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolGlob),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn kimi_manifest_tool_edit_unsupported() {
        let m = kimi_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolEdit),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn kimi_has_native_web_search() {
        let m = kimi_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolWebSearch),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn claude_has_native_web_search() {
        let m = claude_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolWebSearch),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn all_manifests_have_streaming() {
        let manifests = vec![
            ("OpenAI", openai_dialect::capability_manifest()),
            ("Claude", claude_dialect::capability_manifest()),
            ("Gemini", gemini_dialect::capability_manifest()),
            ("Kimi", kimi_dialect::capability_manifest()),
        ];
        for (name, m) in manifests {
            assert!(
                matches!(m.get(&Capability::Streaming), Some(SupportLevel::Native)),
                "{name} should support streaming natively"
            );
        }
    }
}

// ── 13. Claude passthrough fidelity ─────────────────────────────────────

mod claude_passthrough {
    use abp_claude_sdk::dialect::{
        self, ClaudeApiError, ClaudeMessageDelta, ClaudeResponse, ClaudeStreamDelta,
        ClaudeStreamEvent, ClaudeUsage,
    };

    #[test]
    fn passthrough_roundtrip_text_delta() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "hello ".into(),
            },
        };
        let wrapped = dialect::to_passthrough_event(&event);
        let recovered = dialect::from_passthrough_event(&wrapped);
        assert_eq!(recovered.as_ref(), Some(&event));
    }

    #[test]
    fn passthrough_roundtrip_message_start() {
        let event = ClaudeStreamEvent::MessageStart {
            message: ClaudeResponse {
                id: "msg_test".into(),
                model: "claude-sonnet-4-20250514".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: None,
            },
        };
        let wrapped = dialect::to_passthrough_event(&event);
        let recovered = dialect::from_passthrough_event(&wrapped);
        assert_eq!(recovered.as_ref(), Some(&event));
    }

    #[test]
    fn passthrough_roundtrip_error_event() {
        let event = ClaudeStreamEvent::Error {
            error: ClaudeApiError {
                error_type: "overloaded_error".into(),
                message: "Overloaded".into(),
            },
        };
        let wrapped = dialect::to_passthrough_event(&event);
        let recovered = dialect::from_passthrough_event(&wrapped);
        assert_eq!(recovered.as_ref(), Some(&event));
    }

    #[test]
    fn verify_passthrough_fidelity_sequence() {
        let events = vec![
            ClaudeStreamEvent::MessageStart {
                message: ClaudeResponse {
                    id: "msg_seq".into(),
                    model: "claude-sonnet-4-20250514".into(),
                    role: "assistant".into(),
                    content: vec![],
                    stop_reason: None,
                    usage: None,
                },
            },
            ClaudeStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ClaudeStreamDelta::TextDelta {
                    text: "hello".into(),
                },
            },
            ClaudeStreamEvent::ContentBlockStop { index: 0 },
            ClaudeStreamEvent::MessageDelta {
                delta: ClaudeMessageDelta {
                    stop_reason: Some("end_turn".into()),
                    stop_sequence: None,
                },
                usage: Some(ClaudeUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
            },
            ClaudeStreamEvent::MessageStop {},
        ];
        assert!(dialect::verify_passthrough_fidelity(&events));
    }

    #[test]
    fn passthrough_event_has_dialect_marker() {
        let event = ClaudeStreamEvent::Ping {};
        let wrapped = dialect::to_passthrough_event(&event);
        let ext = wrapped.ext.as_ref().unwrap();
        assert_eq!(ext.get("dialect").and_then(|v| v.as_str()), Some("claude"));
    }
}
