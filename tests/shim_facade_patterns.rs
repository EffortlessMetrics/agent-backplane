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
//! Integration tests verifying the shim facade pattern for all 6 SDK shims.
//!
//! Each shim accepts SDK-native requests, converts to ABP IR, routes through
//! a backend, and converts receipts back to SDK-native responses.

use abp_core::ir::{IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, Outcome, Receipt, UsageNormalized};
use chrono::Utc;
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn default_usage() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..Default::default()
    }
}

fn build_receipt(events: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: abp_core::RunMetadata {
            run_id: uuid::Uuid::new_v4(),
            work_order_id: uuid::Uuid::new_v4(),
            contract_version: abp_core::CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 42,
        },
        backend: abp_core::BackendIdentity {
            id: "test-backend".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: abp_core::CapabilityManifest::new(),
        mode: abp_core::ExecutionMode::default(),
        usage_raw: json!({}),
        usage,
        trace: events,
        artifacts: vec![],
        verification: abp_core::VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Module 1: OpenAI Shim
// ═════════════════════════════════════════════════════════════════════════

mod openai {
    use super::*;
    use abp_shim_openai::*;

    fn sample_request() -> ChatCompletionRequest {
        ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![
                Message::system("You are helpful."),
                Message::user("What is 2+2?"),
            ])
            .temperature(0.7)
            .max_tokens(1024)
            .build()
    }

    // ── 1. Request builders ─────────────────────────────────────────────

    #[test]
    fn builder_defaults_model() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("hi")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn builder_sets_all_fields() {
        let req = sample_request();
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.max_tokens, Some(1024));
    }

    #[test]
    fn builder_stream_flag() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o-mini")
            .messages(vec![Message::user("test")])
            .stream(true)
            .build();
        assert_eq!(req.stream, Some(true));
    }

    #[test]
    fn builder_with_tools() {
        let tool = Tool::function("get_weather", "Get weather", json!({"type": "object"}));
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("weather?")])
            .tools(vec![tool])
            .build();
        assert!(req.tools.is_some());
        assert_eq!(req.tools.as_ref().unwrap().len(), 1);
    }

    // ── 2. Type conversions ─────────────────────────────────────────────

    #[test]
    fn request_to_ir_roundtrip() {
        let req = sample_request();
        let ir = request_to_ir(&req);
        assert!(!ir.messages.is_empty());
        let has_user = ir.messages.iter().any(|m| m.role == IrRole::User);
        assert!(has_user);
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = sample_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn request_to_work_order_preserves_temperature() {
        let req = sample_request();
        let wo = request_to_work_order(&req);
        let temp = wo.config.vendor.get("temperature");
        assert!(temp.is_some());
    }

    #[test]
    fn tools_to_ir_conversion() {
        let tools = vec![Tool::function(
            "read_file",
            "Read a file",
            json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        )];
        let ir_tools = tools_to_ir(&tools);
        assert_eq!(ir_tools.len(), 1);
        assert_eq!(ir_tools[0].name, "read_file");
        assert_eq!(ir_tools[0].description, "Read a file");
    }

    // ── 3. Response mapping ─────────────────────────────────────────────

    #[test]
    fn receipt_to_response_text() {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "The answer is 4.".into(),
        })];
        let receipt = build_receipt(events, default_usage());
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("The answer is 4.")
        );
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn receipt_to_response_tool_calls() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "get_weather".into(),
            tool_use_id: Some("call_123".into()),
            parent_tool_use_id: None,
            input: json!({"location": "NYC"}),
        })];
        let receipt = build_receipt(events, default_usage());
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let msg = &resp.choices[0].message;
        assert!(msg.tool_calls.is_some());
        let tc = &msg.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.function.name, "get_weather");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn receipt_to_response_usage_mapping() {
        let receipt = build_receipt(
            vec![make_event(AgentEventKind::AssistantMessage {
                text: "hi".into(),
            })],
            default_usage(),
        );
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let usage = resp.usage.as_ref().unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    // ── 4. Streaming ────────────────────────────────────────────────────

    #[test]
    fn events_to_stream_maps_delta() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            }),
            make_event(AgentEventKind::AssistantDelta {
                text: " world".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert_eq!(stream.len(), 3);
        assert_eq!(stream[0].object, "chat.completion.chunk");
    }

    #[test]
    fn stream_events_carry_model() {
        let events = vec![make_event(AgentEventKind::AssistantDelta {
            text: "hi".into(),
        })];
        let stream = events_to_stream_events(&events, "gpt-4o-mini");
        assert_eq!(stream[0].model, "gpt-4o-mini");
    }

    // ── 5. Error mapping ────────────────────────────────────────────────

    #[test]
    fn receipt_error_event_maps_to_content() {
        let events = vec![make_event(AgentEventKind::Error {
            message: "rate limited".into(),
            error_code: None,
        })];
        let receipt = build_receipt(events, default_usage());
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let content = resp.choices[0].message.content.as_deref().unwrap();
        assert!(content.contains("rate limited"));
    }

    #[test]
    fn shim_error_display() {
        let err = ShimError::InvalidRequest("bad param".into());
        assert!(err.to_string().contains("bad param"));
    }

    // ── 6. Feature detection ────────────────────────────────────────────

    #[test]
    fn message_constructors_cover_all_roles() {
        let sys = Message::system("sys");
        let usr = Message::user("usr");
        let asst = Message::assistant("asst");
        let tool = Message::tool("call_1", "result");
        assert_eq!(sys.role, Role::System);
        assert_eq!(usr.role, Role::User);
        assert_eq!(asst.role, Role::Assistant);
        assert_eq!(tool.role, Role::Tool);
    }

    #[test]
    fn tool_function_constructor() {
        let t = Tool::function("fn_name", "desc", json!({}));
        assert_eq!(t.tool_type, "function");
        assert_eq!(t.function.name, "fn_name");
    }

    // ── 7. Model selection ──────────────────────────────────────────────

    #[test]
    fn work_order_model_resolution() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("hi")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    // ── 8. Round-trip ───────────────────────────────────────────────────

    #[test]
    fn full_roundtrip_preserves_key_fields() {
        let req = sample_request();
        let wo = request_to_work_order(&req);
        assert!(wo.config.model.is_some());

        let receipt = build_receipt(
            vec![make_event(AgentEventKind::AssistantMessage {
                text: "4".into(),
            })],
            default_usage(),
        );
        let resp = receipt_to_response(&receipt, &req.model);
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("4"));
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Module 2: Claude Shim
// ═════════════════════════════════════════════════════════════════════════

mod claude {
    use super::*;
    use abp_shim_claude::*;

    fn sample_request() -> MessageRequest {
        MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "What is 2+2?".into(),
                }],
            }],
            system: Some("You are helpful.".into()),
            temperature: Some(0.5),
            stop_sequences: None,
            thinking: None,
            stream: None,
        }
    }

    // ── 1. Request builders ─────────────────────────────────────────────

    #[test]
    fn message_request_has_required_fields() {
        let req = sample_request();
        assert_eq!(req.model, "claude-sonnet-4-20250514");
        assert_eq!(req.max_tokens, 4096);
        assert!(!req.messages.is_empty());
    }

    #[test]
    fn message_request_system_prompt() {
        let req = sample_request();
        assert_eq!(req.system.as_deref(), Some("You are helpful."));
    }

    // ── 2. Type conversions ─────────────────────────────────────────────

    #[test]
    fn content_block_to_ir_text() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        let ir = content_block_to_ir(&block);
        match ir {
            abp_claude_sdk::dialect::ClaudeContentBlock::Text { text } => {
                assert_eq!(text, "hello");
            }
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn content_block_roundtrip() {
        let original = ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read_file".into(),
            input: json!({"path": "/tmp/f"}),
        };
        let ir = content_block_to_ir(&original);
        let back = content_block_from_ir(&ir);
        assert_eq!(original, back);
    }

    #[test]
    fn image_source_roundtrip() {
        let block = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "abc123".into(),
            },
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn thinking_block_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "Let me think...".into(),
            signature: Some("sig123".into()),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn message_to_ir_simple_text() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "hello".into(),
            }],
        };
        let ir_msg = message_to_ir(&msg);
        assert_eq!(ir_msg.role, "user");
        assert_eq!(ir_msg.content, "hello");
    }

    #[test]
    fn request_to_claude_preserves_model() {
        let req = sample_request();
        let claude_req = request_to_claude(&req);
        assert_eq!(claude_req.model, "claude-sonnet-4-20250514");
        assert_eq!(claude_req.max_tokens, 4096);
    }

    #[test]
    fn request_to_work_order_preserves_fields() {
        let req = sample_request();
        let wo = request_to_work_order(&req);
        assert!(wo.config.model.is_some());
    }

    // ── 3. Response mapping ─────────────────────────────────────────────

    #[test]
    fn response_from_events_text() {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "4".into(),
        })];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.role, "assistant");
        assert_eq!(resp.response_type, "message");
        assert!(!resp.content.is_empty());
        match &resp.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "4"),
            _ => panic!("expected text block"),
        }
    }

    #[test]
    fn response_from_events_tool_use() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu_42".into()),
            parent_tool_use_id: None,
            input: json!({"command": "ls"}),
        })];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        match &resp.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_42");
                assert_eq!(name, "bash");
                assert_eq!(input, &json!({"command": "ls"}));
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    // ── 4. Streaming ────────────────────────────────────────────────────

    #[test]
    fn stream_delta_types_exist() {
        let _text = StreamDelta::TextDelta { text: "hi".into() };
        let _json = StreamDelta::InputJsonDelta {
            partial_json: "{".into(),
        };
        let _thinking = StreamDelta::ThinkingDelta {
            thinking: "hmm".into(),
        };
        let _sig = StreamDelta::SignatureDelta {
            signature: "sig".into(),
        };
    }

    #[test]
    fn stream_event_variants_constructible() {
        let msg_resp = MessageResponse {
            id: "msg_1".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: None,
            stop_sequence: None,
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        let start = StreamEvent::MessageStart { message: msg_resp };
        let stop = StreamEvent::MessageStop {};
        let ping = StreamEvent::Ping {};
        // Just verifying construction
        let _ = (start, stop, ping);
    }

    // ── 5. Error mapping ────────────────────────────────────────────────

    #[test]
    fn shim_error_variants() {
        let e1 = ShimError::InvalidRequest("bad".into());
        assert!(e1.to_string().contains("bad"));
        let e2 = ShimError::ApiError {
            error_type: "overloaded_error".into(),
            message: "try later".into(),
        };
        assert!(e2.to_string().contains("try later"));
    }

    // ── 6. Feature detection ────────────────────────────────────────────

    #[test]
    fn claude_supports_thinking_blocks() {
        let block = ContentBlock::Thinking {
            thinking: "reasoning...".into(),
            signature: None,
        };
        // Thinking blocks roundtrip through IR
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn claude_supports_image_content() {
        let block = ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/img.png".into(),
            },
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    // ── 7. Model selection ──────────────────────────────────────────────

    #[test]
    fn work_order_uses_request_model() {
        let mut req = sample_request();
        req.model = "claude-opus-4-20250514".into();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-opus-4-20250514"));
    }

    // ── 8. Round-trip ───────────────────────────────────────────────────

    #[test]
    fn full_roundtrip() {
        let req = sample_request();
        let _wo = request_to_work_order(&req);
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "The answer is 4.".into(),
        })];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.model, "claude-sonnet-4-20250514");
        match &resp.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "The answer is 4."),
            _ => panic!("expected text"),
        }
    }

    // ── Client tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn client_create_returns_response() {
        let client = AnthropicClient::new();
        let req = sample_request();
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.role, "assistant");
        assert!(!resp.content.is_empty());
    }

    #[tokio::test]
    async fn client_rejects_empty_messages() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let result = client.create(req).await;
        assert!(result.is_err());
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Module 3: Gemini Shim
// ═════════════════════════════════════════════════════════════════════════

mod gemini {
    use super::*;
    use abp_shim_gemini::*;

    fn sample_request() -> GenerateContentRequest {
        GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("What is 2+2?")]))
    }

    // ── 1. Request builders ─────────────────────────────────────────────

    #[test]
    fn request_new_sets_model() {
        let req = GenerateContentRequest::new("gemini-2.5-pro");
        assert_eq!(req.model, "gemini-2.5-pro");
        assert!(req.contents.is_empty());
    }

    #[test]
    fn request_builder_chaining() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("hello")]))
            .generation_config(GenerationConfig {
                temperature: Some(0.5),
                max_output_tokens: Some(1000),
                ..Default::default()
            });
        assert_eq!(req.contents.len(), 1);
        assert!(req.generation_config.is_some());
    }

    #[test]
    fn request_with_system_instruction() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::user(vec![Part::text("Be concise.")]));
        assert!(req.system_instruction.is_some());
    }

    #[test]
    fn request_with_tools() {
        let req = GenerateContentRequest::new("gemini-2.5-flash").tools(vec![ToolDeclaration {
            function_declarations: vec![FunctionDeclaration {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: json!({"type": "object"}),
            }],
        }]);
        assert!(req.tools.is_some());
    }

    // ── 2. Type conversions ─────────────────────────────────────────────

    #[test]
    fn request_to_ir_produces_conversation() {
        let req = sample_request();
        let (ir, _cfg, _safety) = request_to_ir(&req).unwrap();
        assert!(!ir.conversation.messages.is_empty());
    }

    #[test]
    fn ir_to_work_order_sets_model() {
        let req = sample_request();
        let (ir, cfg, _safety) = request_to_ir(&req).unwrap();
        let wo = ir_to_work_order(&ir, &req.model, &cfg);
        let model = wo.config.model.as_deref().unwrap();
        assert!(model.contains("gemini-2.5-flash"));
    }

    #[test]
    fn part_text_constructor() {
        let p = Part::text("hello");
        assert_eq!(p, Part::Text("hello".into()));
    }

    #[test]
    fn part_function_call_constructor() {
        let p = Part::function_call("search", json!({"q": "test"}));
        match p {
            Part::FunctionCall { name, args } => {
                assert_eq!(name, "search");
                assert_eq!(args, json!({"q": "test"}));
            }
            _ => panic!("expected FunctionCall"),
        }
    }

    #[test]
    fn content_user_role() {
        let c = Content::user(vec![Part::text("hi")]);
        assert_eq!(c.role, "user");
    }

    #[test]
    fn content_model_role() {
        let c = Content::model(vec![Part::text("hello")]);
        assert_eq!(c.role, "model");
    }

    // ── 3. Response mapping ─────────────────────────────────────────────

    #[test]
    fn receipt_to_ir_produces_conversation() {
        let receipt = build_receipt(
            vec![make_event(AgentEventKind::AssistantMessage {
                text: "4".into(),
            })],
            default_usage(),
        );
        let ir = receipt_to_ir(&receipt);
        assert!(!ir.messages.is_empty());
    }

    #[test]
    fn make_usage_metadata_maps_tokens() {
        let usage = default_usage();
        let meta = make_usage_metadata(&usage).unwrap();
        assert_eq!(meta.prompt_token_count, 100);
        assert_eq!(meta.candidates_token_count, 50);
        assert_eq!(meta.total_token_count, 150);
    }

    // ── 4. Streaming ────────────────────────────────────────────────────

    #[test]
    fn receipt_to_stream_events_non_empty() {
        let receipt = build_receipt(
            vec![
                make_event(AgentEventKind::AssistantDelta {
                    text: "hello".into(),
                }),
                make_event(AgentEventKind::RunCompleted {
                    message: "done".into(),
                }),
            ],
            default_usage(),
        );
        let events = receipt_to_stream_events(&receipt);
        assert!(!events.is_empty());
    }

    // ── 5. Error mapping ────────────────────────────────────────────────

    #[test]
    fn gemini_error_display() {
        let e = GeminiError::RequestConversion("bad request".into());
        assert!(e.to_string().contains("bad request"));
        let e2 = GeminiError::BackendError("timeout".into());
        assert!(e2.to_string().contains("timeout"));
    }

    // ── 6. Feature detection ────────────────────────────────────────────

    #[test]
    fn inline_data_part() {
        let p = Part::inline_data("image/png", "base64data");
        match p {
            Part::InlineData { mime_type, data } => {
                assert_eq!(mime_type, "image/png");
                assert_eq!(data, "base64data");
            }
            _ => panic!("expected InlineData"),
        }
    }

    #[test]
    fn function_response_part() {
        let p = Part::function_response("search", json!({"results": []}));
        match p {
            Part::FunctionResponse { name, response } => {
                assert_eq!(name, "search");
                assert_eq!(response, json!({"results": []}));
            }
            _ => panic!("expected FunctionResponse"),
        }
    }

    #[test]
    fn usage_ir_roundtrip() {
        let usage = UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 20,
            total_token_count: 30,
        };
        let ir = usage_to_ir(&usage);
        let back = usage_from_ir(&ir);
        assert_eq!(usage, back);
    }

    // ── 7. Model selection ──────────────────────────────────────────────

    #[test]
    fn client_model_accessor() {
        let client = PipelineClient::new("gemini-2.5-pro");
        assert_eq!(client.model(), "gemini-2.5-pro");
    }

    // ── 8. Round-trip ───────────────────────────────────────────────────

    #[tokio::test]
    async fn full_roundtrip_via_client() {
        let client = PipelineClient::new("gemini-2.5-flash");
        let request = sample_request();
        let response = client.generate(request).await.unwrap();
        assert!(!response.candidates.is_empty());
        assert!(response.text().is_some());
    }

    #[test]
    fn response_text_accessor() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("hello world")]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        assert_eq!(resp.text(), Some("hello world"));
    }

    #[test]
    fn response_function_calls_accessor() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::function_call("search", json!({"q": "test"}))]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        let fcs = resp.function_calls();
        assert_eq!(fcs.len(), 1);
        assert_eq!(fcs[0].0, "search");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Module 4: Codex Shim
// ═════════════════════════════════════════════════════════════════════════

mod codex {
    use super::*;
    use abp_shim_codex::*;

    fn sample_request() -> abp_codex_sdk::dialect::CodexRequest {
        CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "Write hello world")])
            .max_output_tokens(2048)
            .temperature(0.3)
            .build()
    }

    // ── 1. Request builders ─────────────────────────────────────────────

    #[test]
    fn builder_defaults_model() {
        let req = CodexRequestBuilder::new().build();
        assert_eq!(req.model, "codex-mini-latest");
    }

    #[test]
    fn builder_sets_fields() {
        let req = sample_request();
        assert_eq!(req.model, "codex-mini-latest");
        assert_eq!(req.max_output_tokens, Some(2048));
        assert_eq!(req.temperature, Some(0.3));
    }

    #[test]
    fn codex_message_constructor() {
        let item = codex_message("user", "hello");
        match item {
            abp_codex_sdk::dialect::CodexInputItem::Message { role, content } => {
                assert_eq!(role, "user");
                assert_eq!(content, "hello");
            }
        }
    }

    // ── 2. Type conversions ─────────────────────────────────────────────

    #[test]
    fn request_to_ir_produces_messages() {
        let req = sample_request();
        let ir = request_to_ir(&req);
        assert!(!ir.messages.is_empty());
    }

    #[test]
    fn request_to_work_order_sets_model() {
        let req = sample_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    // ── 3. Response mapping ─────────────────────────────────────────────

    #[test]
    fn receipt_to_response_has_output() {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "print('hello')".into(),
        })];
        let receipt = build_receipt(events, default_usage());
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());
    }

    #[test]
    fn receipt_to_response_usage() {
        let receipt = build_receipt(
            vec![make_event(AgentEventKind::AssistantMessage {
                text: "code".into(),
            })],
            default_usage(),
        );
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(resp.usage.is_some());
    }

    // ── 4. Streaming ────────────────────────────────────────────────────

    #[test]
    fn events_to_stream_events_non_empty() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta {
                text: "print".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        assert!(!stream.is_empty());
    }

    // ── 5. Error mapping ────────────────────────────────────────────────

    #[test]
    fn shim_error_display() {
        let e = ShimError::InvalidRequest("missing input".into());
        assert!(e.to_string().contains("missing input"));
        let e2 = ShimError::Internal("crash".into());
        assert!(e2.to_string().contains("crash"));
    }

    // ── 6. IR roundtrip helpers ─────────────────────────────────────────

    #[test]
    fn ir_usage_conversion() {
        let ir = IrUsage::from_io(10, 20);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    // ── 7. Model selection ──────────────────────────────────────────────

    #[test]
    fn client_model_accessor() {
        let client = CodexClient::new("codex-mini-latest");
        assert_eq!(client.model(), "codex-mini-latest");
    }

    // ── 8. Round-trip with processor ────────────────────────────────────

    #[tokio::test]
    async fn client_with_processor_roundtrip() {
        let client = CodexClient::new("codex-mini-latest").with_processor(Box::new(|_wo| {
            build_receipt(
                vec![make_event(AgentEventKind::AssistantMessage {
                    text: "hello world".into(),
                })],
                default_usage(),
            )
        }));
        let req = sample_request();
        let resp = client.create(req).await.unwrap();
        assert!(!resp.output.is_empty());
    }

    #[tokio::test]
    async fn client_without_processor_errors() {
        let client = CodexClient::new("codex-mini-latest");
        let req = sample_request();
        let result = client.create(req).await;
        assert!(result.is_err());
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Module 5: Copilot Shim
// ═════════════════════════════════════════════════════════════════════════

mod copilot {
    use super::*;
    use abp_shim_copilot::*;

    fn sample_request() -> abp_copilot_sdk::dialect::CopilotRequest {
        CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![
                Message::system("You are a coding assistant."),
                Message::user("Explain ownership in Rust"),
            ])
            .build()
    }

    // ── 1. Request builders ─────────────────────────────────────────────

    #[test]
    fn builder_defaults_model() {
        let req = CopilotRequestBuilder::new().build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn builder_sets_messages() {
        let req = sample_request();
        assert_eq!(req.messages.len(), 2);
    }

    // ── 2. Type conversions ─────────────────────────────────────────────

    #[test]
    fn request_to_ir_produces_conversation() {
        let req = sample_request();
        let ir = request_to_ir(&req);
        assert!(!ir.messages.is_empty());
    }

    #[test]
    fn request_to_work_order_sets_model() {
        let req = sample_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn messages_to_ir_roundtrip() {
        let msgs = vec![
            Message::system("sys"),
            Message::user("usr"),
            Message::assistant("asst"),
        ];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.messages.len(), 3);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 3);
        assert_eq!(back[1].content, "usr");
    }

    // ── 3. Response mapping ─────────────────────────────────────────────

    #[test]
    fn receipt_to_response_has_message() {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "Ownership means...".into(),
        })];
        let receipt = build_receipt(events, default_usage());
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.message.is_empty());
    }

    #[test]
    fn receipt_to_response_with_function_call() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "search_code".into(),
            tool_use_id: Some("fc_1".into()),
            parent_tool_use_id: None,
            input: json!({"query": "ownership"}),
        })];
        let receipt = build_receipt(events, default_usage());
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.function_call.is_some());
    }

    // ── 4. Streaming ────────────────────────────────────────────────────

    #[test]
    fn events_to_stream_events_non_empty() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta {
                text: "Ownership ".into(),
            }),
            make_event(AgentEventKind::AssistantDelta {
                text: "is...".into(),
            }),
        ];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    // ── 5. Error mapping ────────────────────────────────────────────────

    #[test]
    fn shim_error_variants() {
        let e1 = ShimError::InvalidRequest("bad".into());
        assert!(e1.to_string().contains("bad"));
        let e2 = ShimError::Internal("oops".into());
        assert!(e2.to_string().contains("oops"));
    }

    // ── 6. Feature detection ────────────────────────────────────────────

    #[test]
    fn message_constructors() {
        let sys = Message::system("sys");
        assert_eq!(sys.role, "system");
        let usr = Message::user("usr");
        assert_eq!(usr.role, "user");
        let asst = Message::assistant("asst");
        assert_eq!(asst.role, "assistant");
    }

    #[test]
    fn ir_usage_to_tuple_works() {
        let ir = IrUsage::from_io(10, 20);
        let (inp, out, total) = ir_usage_to_tuple(&ir);
        assert_eq!(inp, 10);
        assert_eq!(out, 20);
        assert_eq!(total, 30);
    }

    // ── 7. Model selection ──────────────────────────────────────────────

    #[test]
    fn client_model_accessor() {
        let client = CopilotClient::new("gpt-4o");
        assert_eq!(client.model(), "gpt-4o");
    }

    // ── 8. Round-trip with processor ────────────────────────────────────

    #[tokio::test]
    async fn client_with_processor_roundtrip() {
        let client = CopilotClient::new("gpt-4o").with_processor(Box::new(|_wo| {
            build_receipt(
                vec![make_event(AgentEventKind::AssistantMessage {
                    text: "Ownership is a memory management concept.".into(),
                })],
                default_usage(),
            )
        }));
        let req = sample_request();
        let resp = client.create(req).await.unwrap();
        assert!(!resp.message.is_empty());
    }

    #[tokio::test]
    async fn client_without_processor_errors() {
        let client = CopilotClient::new("gpt-4o");
        let req = sample_request();
        let result = client.create(req).await;
        assert!(result.is_err());
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Module 6: Kimi Shim
// ═════════════════════════════════════════════════════════════════════════

mod kimi {
    use super::*;
    use abp_shim_kimi::*;

    fn sample_request() -> abp_kimi_sdk::dialect::KimiRequest {
        KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![
                Message::system("You are helpful."),
                Message::user("Tell me a joke."),
            ])
            .max_tokens(1024)
            .temperature(0.8)
            .build()
    }

    // ── 1. Request builders ─────────────────────────────────────────────

    #[test]
    fn builder_defaults_model() {
        let req = KimiRequestBuilder::new().build();
        assert_eq!(req.model, "moonshot-v1-8k");
    }

    #[test]
    fn builder_sets_fields() {
        let req = sample_request();
        assert_eq!(req.model, "moonshot-v1-8k");
        assert_eq!(req.max_tokens, Some(1024));
        assert_eq!(req.temperature, Some(0.8));
    }

    #[test]
    fn builder_stream_flag() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-32k")
            .messages(vec![Message::user("hi")])
            .stream(true)
            .build();
        assert_eq!(req.stream, Some(true));
    }

    #[test]
    fn builder_use_search() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("search for Rust news")])
            .use_search(true)
            .build();
        assert_eq!(req.use_search, Some(true));
    }

    // ── 2. Type conversions ─────────────────────────────────────────────

    #[test]
    fn request_to_ir_produces_conversation() {
        let req = sample_request();
        let ir = request_to_ir(&req);
        assert!(!ir.messages.is_empty());
    }

    #[test]
    fn request_to_work_order_sets_model() {
        let req = sample_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
    }

    #[test]
    fn messages_to_ir_roundtrip() {
        let msgs = vec![
            Message::system("sys"),
            Message::user("usr"),
            Message::assistant("asst"),
        ];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.messages.len(), 3);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 3);
        assert_eq!(back[1].content, Some("usr".into()));
    }

    // ── 3. Response mapping ─────────────────────────────────────────────

    #[test]
    fn receipt_to_response_text() {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "Why did the crab never share?".into(),
        })];
        let receipt = build_receipt(events, default_usage());
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.model, "moonshot-v1-8k");
        assert!(!resp.choices.is_empty());
    }

    #[test]
    fn receipt_to_response_with_tool_calls() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "web_search".into(),
            tool_use_id: Some("tc_1".into()),
            parent_tool_use_id: None,
            input: json!({"query": "joke"}),
        })];
        let receipt = build_receipt(events, default_usage());
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(!resp.choices.is_empty());
    }

    // ── 4. Streaming ────────────────────────────────────────────────────

    #[test]
    fn events_to_stream_chunks_non_empty() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta { text: "Why".into() }),
            make_event(AgentEventKind::AssistantDelta {
                text: " did".into(),
            }),
        ];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert!(!chunks.is_empty());
    }

    // ── 5. Error mapping ────────────────────────────────────────────────

    #[test]
    fn shim_error_display() {
        let e = ShimError::InvalidRequest("bad".into());
        assert!(e.to_string().contains("bad"));
    }

    // ── 6. Feature detection ────────────────────────────────────────────

    #[test]
    fn message_constructors_cover_roles() {
        let sys = Message::system("sys");
        assert_eq!(sys.role, "system");
        let usr = Message::user("usr");
        assert_eq!(usr.role, "user");
        let asst = Message::assistant("asst");
        assert_eq!(asst.role, "assistant");
        let tool = Message::tool("tc_1", "result");
        assert_eq!(tool.role, "tool");
    }

    #[test]
    fn ir_usage_conversion() {
        let ir = IrUsage::from_io(15, 25);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 15);
        assert_eq!(usage.completion_tokens, 25);
        assert_eq!(usage.total_tokens, 40);
    }

    // ── 7. Model selection ──────────────────────────────────────────────

    #[test]
    fn client_model_accessor() {
        let client = KimiClient::with_model("moonshot-v1-128k");
        assert_eq!(client.model(), "moonshot-v1-128k");
    }

    #[test]
    fn work_order_different_model() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::user("hi")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
    }

    // ── 8. Round-trip with processor ────────────────────────────────────

    #[tokio::test]
    async fn client_with_processor_roundtrip() {
        let client = KimiClient::new("moonshot-v1-8k").with_processor(Box::new(|_wo| {
            build_receipt(
                vec![make_event(AgentEventKind::AssistantMessage {
                    text: "Because it was a little shellfish!".into(),
                })],
                default_usage(),
            )
        }));
        let req = sample_request();
        let resp = client.create(req).await.unwrap();
        assert!(!resp.choices.is_empty());
    }

    #[tokio::test]
    async fn client_without_processor_errors() {
        let client = KimiClient::new("moonshot-v1-8k");
        let req = sample_request();
        let result = client.create(req).await;
        assert!(result.is_err());
    }
}
