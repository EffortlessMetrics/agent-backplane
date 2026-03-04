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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! SDK Conformance Matrix: tests that all 6 SDK shims conform to a common
//! behavioral contract.
//!
//! Dimensions tested per shim:
//!  1. Client construction
//!  2. Error type existence and Display
//!  3. Request construction via builder
//!  4. Request → IR conversion
//!  5. Request → WorkOrder conversion
//!  6. Receipt → Response conversion
//!  7. Events → Stream events conversion
//!  8. IR ↔ message roundtrip
//!  9. IR usage conversion
//! 10. Model name preservation
//! 11. Token usage fidelity
//! 12. System / user message handling
//! 13. Serde roundtrip on request types
//! 14. Mock receipt helpers
//! 15. Re-exported SDK types exist
//! 16. Debug formatting on client types
//! 17. Empty conversation edge cases
//! 18. Multi-turn conversations

use abp_core::ir::IrUsage;
use abp_core::{AgentEvent, AgentEventKind, UsageNormalized};
use chrono::Utc;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn event_assistant(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn event_delta(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: text.into() },
        ext: None,
    }
}

fn event_run_started() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "started".into(),
        },
        ext: None,
    }
}

fn event_run_completed() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    }
}

fn event_tool_call() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "get_weather".into(),
            tool_use_id: Some("tc_1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"city": "London"}),
        },
        ext: None,
    }
}

fn test_usage() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..Default::default()
    }
}

fn ir_usage_fixture() -> IrUsage {
    IrUsage::from_io(100, 50)
}

// ===========================================================================
// OpenAI Shim
// ===========================================================================
mod openai {
    use super::*;
    use abp_shim_openai::*;

    fn simple_request() -> ChatCompletionRequest {
        ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Hello")])
            .build()
    }

    fn multi_turn_request() -> ChatCompletionRequest {
        ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![
                Message::system("You are helpful."),
                Message::user("What is 2+2?"),
                Message::assistant("4"),
                Message::user("Thanks!"),
            ])
            .build()
    }

    #[test]
    fn client_construction() {
        let client = OpenAiClient::new("gpt-4o");
        assert_eq!(client.model(), "gpt-4o");
    }

    #[test]
    fn client_debug() {
        let client = OpenAiClient::new("gpt-4o");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("gpt-4o"));
    }

    #[test]
    fn client_chat_api_chain() {
        let client = OpenAiClient::new("gpt-4o");
        let _completions = client.chat().completions();
    }

    #[test]
    fn error_display_invalid_request() {
        let err = ShimError::InvalidRequest("bad".into());
        assert!(err.to_string().contains("bad"));
    }

    #[test]
    fn error_display_internal() {
        let err = ShimError::Internal("oops".into());
        assert!(err.to_string().contains("oops"));
    }

    #[test]
    fn error_display_serde() {
        let raw = serde_json::from_str::<serde_json::Value>("not json");
        let err = ShimError::Serde(raw.unwrap_err());
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn request_builder_defaults() {
        let req = ChatCompletionRequest::builder().build();
        assert_eq!(req.model, "gpt-4o");
        assert!(req.messages.is_empty());
    }

    #[test]
    fn request_builder_full() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4")
            .messages(vec![Message::user("hi")])
            .temperature(0.5)
            .max_tokens(100)
            .stream(false)
            .stop(vec!["END".into()])
            .build();
        assert_eq!(req.model, "gpt-4");
        assert_eq!(req.temperature, Some(0.5));
        assert_eq!(req.max_tokens, Some(100));
        assert_eq!(req.stream, Some(false));
    }

    #[test]
    fn request_to_ir_preserves_messages() {
        let req = simple_request();
        let conv = request_to_ir(&req);
        assert!(!conv.messages.is_empty());
    }

    #[test]
    fn request_to_work_order_has_task() {
        let req = simple_request();
        let wo = request_to_work_order(&req);
        assert!(!wo.task.is_empty());
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = simple_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model, Some("gpt-4o".into()));
    }

    #[test]
    fn receipt_to_response_has_choices() {
        let receipt = mock_receipt(vec![event_assistant("Hello world")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.choices.is_empty());
        assert_eq!(resp.model, "gpt-4o");
    }

    #[test]
    fn receipt_to_response_content() {
        let receipt = mock_receipt(vec![event_assistant("test reply")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let text = resp.choices[0].message.content.as_deref().unwrap();
        assert_eq!(text, "test reply");
    }

    #[test]
    fn events_to_stream_events_non_empty() {
        let events = vec![event_assistant("chunk1"), event_delta("chunk2")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    #[test]
    fn ir_roundtrip_messages() {
        let msgs = vec![Message::user("Hello"), Message::assistant("Hi")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn ir_usage_conversion() {
        let ir = ir_usage_fixture();
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn mock_receipt_helper() {
        let receipt = mock_receipt(vec![event_assistant("x")]);
        assert!(!receipt.trace.is_empty());
    }

    #[test]
    fn mock_receipt_with_usage_helper() {
        let receipt = mock_receipt_with_usage(vec![event_assistant("x")], test_usage());
        assert_eq!(receipt.usage.input_tokens, Some(100));
    }

    #[test]
    fn serde_roundtrip_request() {
        let req = simple_request();
        let json = serde_json::to_string(&req).unwrap();
        let back: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, req.model);
    }

    #[test]
    fn multi_turn_work_order_task() {
        let req = multi_turn_request();
        let wo = request_to_work_order(&req);
        assert!(wo.task.contains("Thanks"));
    }

    #[test]
    fn tools_to_ir_conversion() {
        let tools = vec![Tool {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        }];
        let ir_tools = tools_to_ir(&tools);
        assert_eq!(ir_tools.len(), 1);
        assert_eq!(ir_tools[0].name, "search");
    }

    #[test]
    fn receipt_with_tool_calls() {
        let receipt = mock_receipt(vec![event_tool_call()]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let msg = &resp.choices[0].message;
        assert!(msg.tool_calls.is_some());
    }

    #[test]
    fn message_constructors() {
        let s = Message::system("sys");
        assert_eq!(s.role, Role::System);
        let u = Message::user("usr");
        assert_eq!(u.role, Role::User);
        let a = Message::assistant("asst");
        assert_eq!(a.role, Role::Assistant);
    }

    #[test]
    fn reexported_types_exist() {
        // Verify re-exports compile
        let _: Option<OpenAIFunctionCall> = None;
        let _: Option<OpenAIFunctionDef> = None;
        let _: Option<OpenAIToolCall> = None;
        let _: Option<OpenAIToolDef> = None;
        let _: Option<ToolChoice> = None;
        let _: Option<ResponseFormat> = None;
    }
}

// ===========================================================================
// Claude Shim
// ===========================================================================
mod claude {
    use super::*;
    use abp_shim_claude::*;

    fn simple_request() -> MessageRequest {
        MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".into(),
                }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        }
    }

    fn multi_turn_request() -> MessageRequest {
        MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 2048,
            messages: vec![
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "What is 2+2?".into(),
                    }],
                },
                Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text { text: "4".into() }],
                },
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "Thanks!".into(),
                    }],
                },
            ],
            system: Some("You are helpful.".into()),
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        }
    }

    #[test]
    fn client_construction_default() {
        let client = AnthropicClient::new();
        let dbg = format!("{client:?}");
        assert!(dbg.contains("AnthropicClient"));
    }

    #[test]
    fn client_construction_with_model() {
        let _client = AnthropicClient::with_model("claude-sonnet-4-20250514");
    }

    #[test]
    fn error_display_invalid_request() {
        let err = ShimError::InvalidRequest("bad".into());
        assert!(err.to_string().contains("bad"));
    }

    #[test]
    fn error_display_api_error() {
        let err = ShimError::ApiError {
            error_type: "rate_limit".into(),
            message: "slow down".into(),
        };
        assert!(err.to_string().contains("slow down"));
    }

    #[test]
    fn error_display_internal() {
        let err = ShimError::Internal("oops".into());
        assert!(err.to_string().contains("oops"));
    }

    #[test]
    fn request_to_work_order_has_task() {
        let req = simple_request();
        let wo = request_to_work_order(&req);
        assert!(wo.task.contains("Hello"));
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = simple_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model, Some("claude-sonnet-4-20250514".into()));
    }

    #[test]
    fn response_from_events_has_content() {
        let events = vec![event_assistant("Hello world")];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert!(!resp.content.is_empty());
    }

    #[test]
    fn response_from_events_model() {
        let events = vec![event_assistant("test")];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn response_from_events_text_content() {
        let events = vec![event_assistant("reply text")];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        match &resp.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "reply text"),
            _ => panic!("expected text block"),
        }
    }

    #[test]
    fn response_from_events_with_tool_call() {
        let events = vec![event_tool_call()];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert!(
            resp.content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
        );
    }

    #[test]
    fn content_block_ir_roundtrip_text() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    #[test]
    fn content_block_ir_roundtrip_tool_use() {
        let block = ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "search".into(),
            input: serde_json::json!({"q": "rust"}),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    #[test]
    fn message_to_ir_preserves_role() {
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
    fn request_to_claude_dialect() {
        let req = simple_request();
        let dialect_req = request_to_claude(&req);
        assert_eq!(dialect_req.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn multi_turn_work_order_task() {
        let req = multi_turn_request();
        let wo = request_to_work_order(&req);
        assert!(wo.task.contains("Thanks"));
    }

    #[test]
    fn serde_roundtrip_request() {
        let req = simple_request();
        let json = serde_json::to_string(&req).unwrap();
        let back: MessageRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, req.model);
    }

    #[test]
    fn serde_roundtrip_response() {
        let events = vec![event_assistant("test")];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        let json = serde_json::to_string(&resp).unwrap();
        let back: MessageResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, resp.model);
    }

    #[test]
    fn role_variants() {
        let u = Role::User;
        let a = Role::Assistant;
        assert_ne!(u, a);
    }
}

// ===========================================================================
// Gemini Shim
// ===========================================================================
mod gemini {
    use abp_shim_gemini::*;

    fn simple_request() -> GenerateContentRequest {
        GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]))
    }

    fn multi_turn_request() -> GenerateContentRequest {
        GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::user(vec![Part::text("Be helpful")]))
            .add_content(Content::user(vec![Part::text("What is 2+2?")]))
            .add_content(Content::model(vec![Part::text("4")]))
            .add_content(Content::user(vec![Part::text("Thanks!")]))
    }

    #[test]
    fn client_construction() {
        let client = GeminiClient::new("gemini-2.5-flash");
        assert_eq!(client.model(), "gemini-2.5-flash");
    }

    #[test]
    fn client_debug() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("gemini-2.5-flash"));
    }

    #[test]
    fn error_display_request_conversion() {
        let err = GeminiError::RequestConversion("bad input".into());
        assert!(err.to_string().contains("bad input"));
    }

    #[test]
    fn error_display_response_conversion() {
        let err = GeminiError::ResponseConversion("bad output".into());
        assert!(err.to_string().contains("bad output"));
    }

    #[test]
    fn error_display_backend() {
        let err = GeminiError::BackendError("fail".into());
        assert!(err.to_string().contains("fail"));
    }

    #[test]
    fn error_display_serde() {
        let raw = serde_json::from_str::<serde_json::Value>("not json");
        let err = GeminiError::Serde(raw.unwrap_err());
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn request_builder_chaining() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("hi")]))
            .generation_config(GenerationConfig {
                max_output_tokens: Some(100),
                temperature: Some(0.5),
                top_p: None,
                top_k: None,
                stop_sequences: None,
                response_mime_type: None,
                response_schema: None,
            });
        assert_eq!(req.model, "gemini-2.5-flash");
        assert!(req.generation_config.is_some());
    }

    #[test]
    fn to_dialect_request_preserves_model() {
        let req = simple_request();
        let dialect = to_dialect_request(&req);
        assert_eq!(dialect.model, "gemini-2.5-flash");
    }

    #[test]
    fn usage_ir_roundtrip() {
        let meta = UsageMetadata {
            prompt_token_count: 100,
            candidates_token_count: 50,
            total_token_count: 150,
        };
        let ir = usage_to_ir(&meta);
        let back = usage_from_ir(&ir);
        assert_eq!(back.prompt_token_count, 100);
        assert_eq!(back.candidates_token_count, 50);
        assert_eq!(back.total_token_count, 150);
    }

    #[test]
    fn part_constructors() {
        let t = Part::text("hi");
        assert!(matches!(t, Part::Text(_)));

        let d = Part::inline_data("image/png", "base64data");
        assert!(matches!(d, Part::InlineData { .. }));

        let fc = Part::function_call("search", serde_json::json!({"q": "rust"}));
        assert!(matches!(fc, Part::FunctionCall { .. }));

        let fr = Part::function_response("search", serde_json::json!({"result": "found"}));
        assert!(matches!(fr, Part::FunctionResponse { .. }));
    }

    #[test]
    fn content_user_and_model() {
        let u = Content::user(vec![Part::text("Hello")]);
        assert_eq!(u.role, "user");
        let m = Content::model(vec![Part::text("Hi")]);
        assert_eq!(m.role, "model");
    }

    #[test]
    fn serde_roundtrip_request() {
        let req = simple_request();
        let json = serde_json::to_string(&req).unwrap();
        let back: GenerateContentRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, req.model);
    }

    #[test]
    fn gen_config_from_dialect_roundtrip() {
        let cfg = abp_gemini_sdk::dialect::GeminiGenerationConfig {
            max_output_tokens: Some(200),
            temperature: Some(0.7),
            top_p: None,
            top_k: None,
            candidate_count: None,
            stop_sequences: None,
            response_mime_type: None,
            response_schema: None,
        };
        let shim_cfg = gen_config_from_dialect(&cfg);
        assert_eq!(shim_cfg.max_output_tokens, Some(200));
        assert_eq!(shim_cfg.temperature, Some(0.7));
    }

    #[test]
    fn from_dialect_response_converts() {
        let resp = abp_gemini_sdk::dialect::GeminiResponse {
            candidates: vec![abp_gemini_sdk::dialect::GeminiCandidate {
                content: abp_gemini_sdk::dialect::GeminiContent {
                    role: "model".into(),
                    parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("hi".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let shim_resp = from_dialect_response(&resp);
        assert_eq!(shim_resp.candidates.len(), 1);
    }

    #[test]
    fn reexported_types_exist() {
        let _: Option<HarmCategory> = None;
        let _: Option<HarmBlockThreshold> = None;
        let _: Option<FunctionCallingMode> = None;
    }

    #[tokio::test]
    async fn client_generate_produces_response() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = simple_request();
        let resp = client.generate(req).await.unwrap();
        assert!(!resp.candidates.is_empty());
    }

    #[test]
    fn multi_turn_request_has_contents() {
        let req = multi_turn_request();
        assert!(req.contents.len() >= 3);
    }
}

// ===========================================================================
// Codex Shim
// ===========================================================================
mod codex {
    use super::*;
    use abp_shim_codex::*;

    fn simple_request() -> CodexShimRequest {
        CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "Hello")])
            .build()
    }

    #[test]
    fn client_construction() {
        let client = CodexClient::new("codex-mini-latest");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("codex-mini-latest"));
    }

    #[test]
    fn error_display_invalid_request() {
        let err = ShimError::InvalidRequest("bad".into());
        assert!(err.to_string().contains("bad"));
    }

    #[test]
    fn error_display_internal() {
        let err = ShimError::Internal("oops".into());
        assert!(err.to_string().contains("oops"));
    }

    #[test]
    fn error_display_serde() {
        let raw = serde_json::from_str::<serde_json::Value>("not json");
        let err = ShimError::Serde(raw.unwrap_err());
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn request_builder_defaults() {
        let req = CodexRequestBuilder::new().build();
        assert_eq!(req.model, "codex-mini-latest");
    }

    #[test]
    fn request_builder_full() {
        let req = CodexRequestBuilder::new()
            .model("codex-large")
            .input(vec![codex_message("user", "hi")])
            .temperature(0.5)
            .max_output_tokens(100)
            .build();
        assert_eq!(req.model, "codex-large");
        assert_eq!(req.temperature, Some(0.5));
        assert_eq!(req.max_output_tokens, Some(100));
    }

    #[test]
    fn request_to_ir_non_empty() {
        let req = simple_request();
        let conv = request_to_ir(&req);
        assert!(!conv.messages.is_empty());
    }

    #[test]
    fn request_to_work_order_has_task() {
        let req = simple_request();
        let wo = request_to_work_order(&req);
        assert!(!wo.task.is_empty());
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = simple_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model, Some("codex-mini-latest".into()));
    }

    #[test]
    fn receipt_to_response_has_output() {
        let receipt = mock_receipt(vec![event_assistant("Hello world")]);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());
    }

    #[test]
    fn events_to_stream_events_non_empty() {
        let events = vec![event_assistant("chunk1"), event_delta("chunk2")];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        assert!(!stream.is_empty());
    }

    #[test]
    fn ir_usage_conversion() {
        let ir = ir_usage_fixture();
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn mock_receipt_helper() {
        let receipt = mock_receipt(vec![event_assistant("x")]);
        assert!(!receipt.trace.is_empty());
    }

    #[test]
    fn mock_receipt_with_usage_helper() {
        let receipt = mock_receipt_with_usage(vec![event_assistant("x")], test_usage());
        assert_eq!(receipt.usage.input_tokens, Some(100));
    }

    #[test]
    fn codex_message_helper() {
        let msg = codex_message("user", "Hello");
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "user");
    }

    #[test]
    fn reexported_types_exist() {
        let _: Option<CodexFunctionDef> = None;
        let _: Option<CodexTextFormat> = None;
        let _: Option<CodexTool> = None;
        let _: Option<CodexToolDef> = None;
        let _: Option<SandboxConfig> = None;
    }

    #[test]
    fn receipt_with_tool_call() {
        let receipt = mock_receipt(vec![event_tool_call()]);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());
    }
}

// ===========================================================================
// Kimi Shim
// ===========================================================================
mod kimi {
    use super::*;
    use abp_shim_kimi::*;

    fn simple_request() -> abp_kimi_sdk::dialect::KimiRequest {
        KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("Hello")])
            .build()
    }

    fn multi_turn_request() -> abp_kimi_sdk::dialect::KimiRequest {
        KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![
                Message::system("Be helpful."),
                Message::user("What is 2+2?"),
                Message::assistant("4"),
                Message::user("Thanks!"),
            ])
            .build()
    }

    #[test]
    fn client_construction() {
        let client = KimiClient::new("moonshot-v1-8k");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("moonshot-v1-8k"));
    }

    #[test]
    fn error_display_invalid_request() {
        let err = ShimError::InvalidRequest("bad".into());
        assert!(err.to_string().contains("bad"));
    }

    #[test]
    fn error_display_internal() {
        let err = ShimError::Internal("oops".into());
        assert!(err.to_string().contains("oops"));
    }

    #[test]
    fn error_display_serde() {
        let raw = serde_json::from_str::<serde_json::Value>("not json");
        let err = ShimError::Serde(raw.unwrap_err());
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn request_builder_defaults() {
        let req = KimiRequestBuilder::new().build();
        assert_eq!(req.model, "moonshot-v1-8k");
    }

    #[test]
    fn request_builder_full() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-32k")
            .messages(vec![Message::user("hi")])
            .temperature(0.5)
            .max_tokens(100)
            .stream(false)
            .use_search(true)
            .build();
        assert_eq!(req.model, "moonshot-v1-32k");
        assert_eq!(req.temperature, Some(0.5));
    }

    #[test]
    fn request_to_ir_non_empty() {
        let req = simple_request();
        let conv = request_to_ir(&req);
        assert!(!conv.messages.is_empty());
    }

    #[test]
    fn request_to_work_order_has_task() {
        let req = simple_request();
        let wo = request_to_work_order(&req);
        assert!(!wo.task.is_empty());
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = simple_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model, Some("moonshot-v1-8k".into()));
    }

    #[test]
    fn receipt_to_response_has_choices() {
        let receipt = mock_receipt(vec![event_assistant("Hello world")]);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(!resp.choices.is_empty());
    }

    #[test]
    fn receipt_to_response_content() {
        let receipt = mock_receipt(vec![event_assistant("test reply")]);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        let text = resp.choices[0].message.content.as_deref().unwrap();
        assert_eq!(text, "test reply");
    }

    #[test]
    fn events_to_stream_chunks_non_empty() {
        let events = vec![event_assistant("chunk1"), event_delta("chunk2")];
        let stream = events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert!(!stream.is_empty());
    }

    #[test]
    fn ir_roundtrip_messages() {
        let msgs = vec![Message::user("Hello"), Message::assistant("Hi")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn ir_usage_conversion() {
        let ir = ir_usage_fixture();
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn mock_receipt_helper() {
        let receipt = mock_receipt(vec![event_assistant("x")]);
        assert!(!receipt.trace.is_empty());
    }

    #[test]
    fn mock_receipt_with_usage_helper() {
        let receipt = mock_receipt_with_usage(vec![event_assistant("x")], test_usage());
        assert_eq!(receipt.usage.input_tokens, Some(100));
    }

    #[test]
    fn message_constructors() {
        let s = Message::system("sys");
        assert_eq!(s.role, "system");
        let u = Message::user("usr");
        assert_eq!(u.role, "user");
        let a = Message::assistant("asst");
        assert_eq!(a.role, "assistant");
        let t = Message::tool("tc_1", "result");
        assert_eq!(t.role, "tool");
    }

    #[test]
    fn multi_turn_work_order_task() {
        let req = multi_turn_request();
        let wo = request_to_work_order(&req);
        assert!(wo.task.contains("Thanks"));
    }

    #[test]
    fn serde_roundtrip_message() {
        let msg = Message::user("hello");
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, "user");
    }

    #[test]
    fn reexported_types_exist() {
        let _: Option<KimiFunctionDef> = None;
        let _: Option<KimiTool> = None;
        let _: Option<KimiToolDef> = None;
        let _: Option<KimiRole> = None;
    }
}

// ===========================================================================
// Copilot Shim
// ===========================================================================
mod copilot {
    use super::*;
    use abp_shim_copilot::*;

    fn simple_request() -> abp_copilot_sdk::dialect::CopilotRequest {
        CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("Hello")])
            .build()
    }

    fn multi_turn_request() -> abp_copilot_sdk::dialect::CopilotRequest {
        CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![
                Message::system("Be helpful."),
                Message::user("What is 2+2?"),
                Message::assistant("4"),
                Message::user("Thanks!"),
            ])
            .build()
    }

    #[test]
    fn client_construction() {
        let client = CopilotClient::new("gpt-4o");
        assert_eq!(client.model(), "gpt-4o");
    }

    #[test]
    fn client_debug() {
        let client = CopilotClient::new("gpt-4o");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("gpt-4o"));
    }

    #[test]
    fn error_display_invalid_request() {
        let err = ShimError::InvalidRequest("bad".into());
        assert!(err.to_string().contains("bad"));
    }

    #[test]
    fn error_display_internal() {
        let err = ShimError::Internal("oops".into());
        assert!(err.to_string().contains("oops"));
    }

    #[test]
    fn error_display_serde() {
        let raw = serde_json::from_str::<serde_json::Value>("not json");
        let err = ShimError::Serde(raw.unwrap_err());
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn request_builder_defaults() {
        let req = CopilotRequestBuilder::new().build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn request_builder_full() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4")
            .messages(vec![Message::user("hi")])
            .build();
        assert_eq!(req.model, "gpt-4");
    }

    #[test]
    fn request_to_ir_non_empty() {
        let req = simple_request();
        let conv = request_to_ir(&req);
        assert!(!conv.messages.is_empty());
    }

    #[test]
    fn request_to_work_order_has_task() {
        let req = simple_request();
        let wo = request_to_work_order(&req);
        assert!(!wo.task.is_empty());
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = simple_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model, Some("gpt-4o".into()));
    }

    #[test]
    fn receipt_to_response_has_content() {
        let receipt = mock_receipt(vec![event_assistant("Hello world")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let json = serde_json::to_value(&resp).unwrap();
        // CopilotResponse should be non-null
        assert!(!json.is_null());
    }

    #[test]
    fn events_to_stream_events_non_empty() {
        let events = vec![event_assistant("chunk1"), event_delta("chunk2")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    #[test]
    fn ir_roundtrip_messages() {
        let msgs = vec![Message::user("Hello"), Message::assistant("Hi")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn ir_usage_to_tuple_conversion() {
        let ir = ir_usage_fixture();
        let (input, output, total) = ir_usage_to_tuple(&ir);
        assert_eq!(input, 100);
        assert_eq!(output, 50);
        assert_eq!(total, 150);
    }

    #[test]
    fn mock_receipt_helper() {
        let receipt = mock_receipt(vec![event_assistant("x")]);
        assert!(!receipt.trace.is_empty());
    }

    #[test]
    fn mock_receipt_with_usage_helper() {
        let receipt = mock_receipt_with_usage(vec![event_assistant("x")], test_usage());
        assert_eq!(receipt.usage.input_tokens, Some(100));
    }

    #[test]
    fn message_constructors() {
        let s = Message::system("sys");
        assert_eq!(s.role, "system");
        let u = Message::user("usr");
        assert_eq!(u.role, "user");
        let a = Message::assistant("asst");
        assert_eq!(a.role, "assistant");
    }

    #[test]
    fn message_user_with_refs() {
        let msg = Message::user_with_refs("hello", vec![]);
        assert_eq!(msg.role, "user");
        assert!(msg.copilot_references.is_empty());
    }

    #[test]
    fn multi_turn_work_order_task() {
        let req = multi_turn_request();
        let wo = request_to_work_order(&req);
        assert!(wo.task.contains("Thanks"));
    }

    #[test]
    fn serde_roundtrip_message() {
        let msg = Message::user("hello");
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, "user");
    }

    #[test]
    fn reexported_types_exist() {
        let _: Option<CopilotFunctionDef> = None;
        let _: Option<CopilotToolType> = None;
    }
}

// ===========================================================================
// Cross-shim structural conformance
// ===========================================================================
mod cross_shim {
    use super::*;

    /// All 6 shims must produce a WorkOrder with a non-empty task from a
    /// simple user message.
    #[test]
    fn all_shims_produce_non_empty_work_order_task() {
        // OpenAI
        let wo = abp_shim_openai::request_to_work_order(
            &abp_shim_openai::ChatCompletionRequest::builder()
                .messages(vec![abp_shim_openai::Message::user("Hello")])
                .build(),
        );
        assert!(!wo.task.is_empty(), "openai task empty");

        // Claude
        let wo = abp_shim_claude::request_to_work_order(&abp_shim_claude::MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![abp_shim_claude::Message {
                role: abp_shim_claude::Role::User,
                content: vec![abp_shim_claude::ContentBlock::Text {
                    text: "Hello".into(),
                }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        });
        assert!(!wo.task.is_empty(), "claude task empty");

        // Gemini (uses internal pipeline, test via client)
        let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash").add_content(
            abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("Hello")]),
        );
        let dialect = abp_shim_gemini::to_dialect_request(&req);
        assert_eq!(dialect.model, "gemini-2.5-flash");

        // Codex
        let wo = abp_shim_codex::request_to_work_order(
            &abp_shim_codex::CodexRequestBuilder::new()
                .input(vec![abp_shim_codex::codex_message("user", "Hello")])
                .build(),
        );
        assert!(!wo.task.is_empty(), "codex task empty");

        // Kimi
        let wo = abp_shim_kimi::request_to_work_order(
            &abp_shim_kimi::KimiRequestBuilder::new()
                .messages(vec![abp_shim_kimi::Message::user("Hello")])
                .build(),
        );
        assert!(!wo.task.is_empty(), "kimi task empty");

        // Copilot
        let wo = abp_shim_copilot::request_to_work_order(
            &abp_shim_copilot::CopilotRequestBuilder::new()
                .messages(vec![abp_shim_copilot::Message::user("Hello")])
                .build(),
        );
        assert!(!wo.task.is_empty(), "copilot task empty");
    }

    /// All 6 shims must preserve the model name in the WorkOrder config.
    #[test]
    fn all_shims_preserve_model_in_work_order() {
        let openai_wo = abp_shim_openai::request_to_work_order(
            &abp_shim_openai::ChatCompletionRequest::builder()
                .model("gpt-4o")
                .messages(vec![abp_shim_openai::Message::user("Hi")])
                .build(),
        );
        assert_eq!(openai_wo.config.model, Some("gpt-4o".into()));

        let claude_wo = abp_shim_claude::request_to_work_order(&abp_shim_claude::MessageRequest {
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
        });
        assert_eq!(
            claude_wo.config.model,
            Some("claude-sonnet-4-20250514".into())
        );

        let codex_wo = abp_shim_codex::request_to_work_order(
            &abp_shim_codex::CodexRequestBuilder::new()
                .model("codex-mini-latest")
                .input(vec![abp_shim_codex::codex_message("user", "Hi")])
                .build(),
        );
        assert_eq!(codex_wo.config.model, Some("codex-mini-latest".into()));

        let kimi_wo = abp_shim_kimi::request_to_work_order(
            &abp_shim_kimi::KimiRequestBuilder::new()
                .model("moonshot-v1-8k")
                .messages(vec![abp_shim_kimi::Message::user("Hi")])
                .build(),
        );
        assert_eq!(kimi_wo.config.model, Some("moonshot-v1-8k".into()));

        let copilot_wo = abp_shim_copilot::request_to_work_order(
            &abp_shim_copilot::CopilotRequestBuilder::new()
                .model("gpt-4o")
                .messages(vec![abp_shim_copilot::Message::user("Hi")])
                .build(),
        );
        assert_eq!(copilot_wo.config.model, Some("gpt-4o".into()));
    }

    /// All shims that have receipt-to-response must produce non-empty output
    /// from a receipt with an assistant message event.
    #[test]
    fn all_receipt_conversions_produce_output() {
        let events = vec![event_assistant("Hello world")];

        // OpenAI
        let receipt = abp_shim_openai::mock_receipt(events.clone());
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.choices.is_empty(), "openai no choices");

        // Claude
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert!(!resp.content.is_empty(), "claude no content");

        // Codex
        let receipt = abp_shim_codex::mock_receipt(events.clone());
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty(), "codex no output");

        // Kimi
        let receipt = abp_shim_kimi::mock_receipt(events.clone());
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(!resp.choices.is_empty(), "kimi no choices");

        // Copilot
        let receipt = abp_shim_copilot::mock_receipt(events);
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        let json = serde_json::to_value(&resp).unwrap();
        assert!(!json.is_null(), "copilot null response");
    }

    /// All shims that support streaming must produce events from assistant messages.
    #[test]
    fn all_stream_conversions_produce_events() {
        let events = vec![event_assistant("chunk"), event_delta("delta")];

        let openai_stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
        assert!(!openai_stream.is_empty(), "openai stream empty");

        let codex_stream = abp_shim_codex::events_to_stream_events(&events, "codex-mini-latest");
        assert!(!codex_stream.is_empty(), "codex stream empty");

        let kimi_stream = abp_shim_kimi::events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert!(!kimi_stream.is_empty(), "kimi stream empty");

        let copilot_stream = abp_shim_copilot::events_to_stream_events(&events, "gpt-4o");
        assert!(!copilot_stream.is_empty(), "copilot stream empty");
    }

    /// All shims with IR roundtrip must preserve message count.
    #[test]
    fn all_ir_roundtrips_preserve_count() {
        // OpenAI
        let msgs = vec![
            abp_shim_openai::Message::user("Hello"),
            abp_shim_openai::Message::assistant("Hi"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);
        let back = abp_shim_openai::ir_to_messages(&ir);
        assert_eq!(back.len(), 2, "openai roundtrip count");

        // Kimi
        let msgs = vec![
            abp_shim_kimi::Message::user("Hello"),
            abp_shim_kimi::Message::assistant("Hi"),
        ];
        let ir = abp_shim_kimi::messages_to_ir(&msgs);
        let back = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(back.len(), 2, "kimi roundtrip count");

        // Copilot
        let msgs = vec![
            abp_shim_copilot::Message::user("Hello"),
            abp_shim_copilot::Message::assistant("Hi"),
        ];
        let ir = abp_shim_copilot::messages_to_ir(&msgs);
        let back = abp_shim_copilot::ir_to_messages(&ir);
        assert_eq!(back.len(), 2, "copilot roundtrip count");
    }

    /// All shims with mock_receipt must return a receipt with trace events.
    #[test]
    fn all_mock_receipts_have_trace() {
        let events = vec![event_assistant("x")];

        let r = abp_shim_openai::mock_receipt(events.clone());
        assert!(!r.trace.is_empty(), "openai");

        let r = abp_shim_codex::mock_receipt(events.clone());
        assert!(!r.trace.is_empty(), "codex");

        let r = abp_shim_kimi::mock_receipt(events.clone());
        assert!(!r.trace.is_empty(), "kimi");

        let r = abp_shim_copilot::mock_receipt(events);
        assert!(!r.trace.is_empty(), "copilot");
    }

    /// All shims with mock_receipt_with_usage must honor the usage fields.
    #[test]
    fn all_mock_receipts_honor_usage() {
        let events = vec![event_assistant("x")];
        let usage = test_usage();

        let r = abp_shim_openai::mock_receipt_with_usage(events.clone(), usage.clone());
        assert_eq!(r.usage.input_tokens, Some(100));

        let r = abp_shim_codex::mock_receipt_with_usage(events.clone(), usage.clone());
        assert_eq!(r.usage.input_tokens, Some(100));

        let r = abp_shim_kimi::mock_receipt_with_usage(events.clone(), usage.clone());
        assert_eq!(r.usage.input_tokens, Some(100));

        let r = abp_shim_copilot::mock_receipt_with_usage(events, usage);
        assert_eq!(r.usage.input_tokens, Some(100));
    }

    /// All error types implement std::error::Error (via Debug + Display).
    #[test]
    fn all_error_types_implement_error_trait() {
        fn assert_is_error<E: std::error::Error>() {}

        assert_is_error::<abp_shim_openai::ShimError>();
        assert_is_error::<abp_shim_claude::ShimError>();
        assert_is_error::<abp_shim_gemini::GeminiError>();
        assert_is_error::<abp_shim_codex::ShimError>();
        assert_is_error::<abp_shim_kimi::ShimError>();
        assert_is_error::<abp_shim_copilot::ShimError>();
    }

    /// All client types implement Debug.
    #[test]
    fn all_client_types_implement_debug() {
        fn assert_debug<T: std::fmt::Debug>() {}

        assert_debug::<abp_shim_openai::OpenAiClient>();
        assert_debug::<abp_shim_claude::AnthropicClient>();
        assert_debug::<abp_shim_gemini::GeminiClient>();
        assert_debug::<abp_shim_codex::CodexClient>();
        assert_debug::<abp_shim_kimi::KimiClient>();
        assert_debug::<abp_shim_copilot::CopilotClient>();
    }

    /// WorkOrder from all shims must have a valid UUID id.
    #[test]
    fn all_work_orders_have_valid_uuid() {
        let openai_wo = abp_shim_openai::request_to_work_order(
            &abp_shim_openai::ChatCompletionRequest::builder()
                .messages(vec![abp_shim_openai::Message::user("x")])
                .build(),
        );
        assert!(!openai_wo.id.is_nil());

        let codex_wo = abp_shim_codex::request_to_work_order(
            &abp_shim_codex::CodexRequestBuilder::new()
                .input(vec![abp_shim_codex::codex_message("user", "x")])
                .build(),
        );
        assert!(!codex_wo.id.is_nil());

        let kimi_wo = abp_shim_kimi::request_to_work_order(
            &abp_shim_kimi::KimiRequestBuilder::new()
                .messages(vec![abp_shim_kimi::Message::user("x")])
                .build(),
        );
        assert!(!kimi_wo.id.is_nil());

        let copilot_wo = abp_shim_copilot::request_to_work_order(
            &abp_shim_copilot::CopilotRequestBuilder::new()
                .messages(vec![abp_shim_copilot::Message::user("x")])
                .build(),
        );
        assert!(!copilot_wo.id.is_nil());
    }

    /// Mock receipts with run lifecycle events produce valid receipts.
    #[test]
    fn mock_receipts_with_lifecycle_events() {
        let events = vec![
            event_run_started(),
            event_assistant("output"),
            event_run_completed(),
        ];

        let r = abp_shim_openai::mock_receipt(events.clone());
        assert_eq!(r.trace.len(), 3);

        let r = abp_shim_codex::mock_receipt(events.clone());
        assert_eq!(r.trace.len(), 3);

        let r = abp_shim_kimi::mock_receipt(events.clone());
        assert_eq!(r.trace.len(), 3);

        let r = abp_shim_copilot::mock_receipt(events);
        assert_eq!(r.trace.len(), 3);
    }
}
