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
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive shim surface-area tests.
//!
//! Validates the public API of each shim crate (OpenAI, Claude, Gemini,
//! Kimi, Codex, Copilot): type existence, request/response conversion,
//! tool calling, streaming, system prompts, usage reporting, and error mapping.

use abp_core::ir::{IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, UsageNormalized};
use chrono::Utc;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;
use tokio_stream::StreamExt;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn serde_roundtrip<T: Serialize + DeserializeOwned + std::fmt::Debug>(val: &T) -> T {
    let json = serde_json::to_string(val).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

fn assistant_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: text.into() },
        ext: None,
    }
}

fn tool_call_event(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: Some(id.into()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn error_event(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.into(),
            error_code: None,
        },
        ext: None,
    }
}

fn usage_100_50() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..Default::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1 · OpenAI Shim
// ═══════════════════════════════════════════════════════════════════════════

mod openai {
    use super::*;
    use abp_shim_openai::*;

    // ── 1a. Type existence & serde ─────────────────────────────────────

    #[test]
    fn role_variants() {
        assert_eq!(serde_json::to_value(Role::System).unwrap(), "system");
        assert_eq!(serde_json::to_value(Role::User).unwrap(), "user");
        assert_eq!(serde_json::to_value(Role::Assistant).unwrap(), "assistant");
        assert_eq!(serde_json::to_value(Role::Tool).unwrap(), "tool");
    }

    #[test]
    fn message_constructors() {
        let sys = Message::system("You are helpful");
        assert_eq!(sys.role, Role::System);
        assert_eq!(sys.content.as_deref(), Some("You are helpful"));

        let user = Message::user("Hello");
        assert_eq!(user.role, Role::User);

        let asst = Message::assistant("Hi");
        assert_eq!(asst.role, Role::Assistant);

        let tc = vec![ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "get_time".into(),
                arguments: "{}".into(),
            },
        }];
        let asst_tc = Message::assistant_with_tool_calls(tc);
        assert_eq!(asst_tc.role, Role::Assistant);
        assert!(asst_tc.tool_calls.is_some());

        let tool = Message::tool("call_1", "12:00");
        assert_eq!(tool.role, Role::Tool);
        assert_eq!(tool.tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn tool_definition_serde() {
        let t = Tool::function("search", "Search the web", json!({"type": "object"}));
        assert_eq!(t.tool_type, "function");
        assert_eq!(t.function.name, "search");
        let back = serde_roundtrip(&t);
        assert_eq!(back.function.description, "Search the web");
    }

    #[test]
    fn builder_defaults() {
        let req = ChatCompletionRequest::builder().build();
        assert_eq!(req.model, "gpt-4o");
        assert!(req.messages.is_empty());
        assert!(req.tools.is_none());
    }

    #[test]
    fn builder_all_fields() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("Hi")])
            .tools(vec![Tool::function("f", "d", json!({}))])
            .temperature(0.7)
            .max_tokens(1024)
            .stop(vec!["END".into()])
            .stream(true)
            .build();
        assert_eq!(req.model, "gpt-4-turbo");
        assert_eq!(req.messages.len(), 1);
        assert!(req.tools.is_some());
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.max_tokens, Some(1024));
        assert_eq!(req.stop.as_ref().unwrap()[0], "END");
        assert_eq!(req.stream, Some(true));
    }

    // ── 1b. Request → IR → WorkOrder ───────────────────────────────────

    #[test]
    fn request_to_ir_preserves_roles() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![
                Message::system("Be brief"),
                Message::user("Hello"),
                Message::assistant("Hi there"),
            ])
            .build();
        let ir = request_to_ir(&req);
        assert!(ir.messages.iter().any(|m| m.role == IrRole::System));
        assert!(ir.messages.iter().any(|m| m.role == IrRole::User));
        assert!(ir.messages.iter().any(|m| m.role == IrRole::Assistant));
    }

    #[test]
    fn request_to_work_order_sets_model() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Hello")])
            .build();
        let wo = request_to_work_order(&req);
        assert!(wo.config.model.as_deref() == Some("gpt-4o"));
    }

    // ── 1c. Receipt → Response (tool calling) ──────────────────────────

    #[test]
    fn receipt_maps_tool_calls() {
        let events = vec![tool_call_event(
            "get_weather",
            "call_abc",
            json!({"city": "NYC"}),
        )];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.object, "chat.completion");
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].id, "call_abc");
        assert_eq!(tc[0].function.name, "get_weather");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn receipt_maps_text_response() {
        let events = vec![assistant_event("Hi!")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hi!"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn receipt_maps_error() {
        let events = vec![error_event("rate limit")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.choices[0]
            .message
            .content
            .as_deref()
            .unwrap()
            .contains("rate limit"));
    }

    // ── 1d. Streaming ──────────────────────────────────────────────────

    #[test]
    fn events_to_stream_produces_chunks() {
        let events = vec![delta_event("He"), delta_event("llo")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        // 2 deltas + 1 stop
        assert_eq!(stream.len(), 3);
        assert_eq!(stream[0].object, "chat.completion.chunk");
        assert_eq!(stream[0].choices[0].delta.content.as_deref(), Some("He"));
        assert_eq!(stream[2].choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn stream_tool_call_event() {
        let events = vec![tool_call_event("search", "call_1", json!({"q": "rust"}))];
        let stream = events_to_stream_events(&events, "gpt-4o");
        let tc = &stream[0].choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(
            tc.function.as_ref().unwrap().name.as_deref(),
            Some("search")
        );
    }

    // ── 1e. System prompt ──────────────────────────────────────────────

    #[test]
    fn system_prompt_in_messages() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::system("You are a cat"), Message::user("Hi")])
            .build();
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[0].content.as_deref(), Some("You are a cat"));
    }

    // ── 1f. Token usage ────────────────────────────────────────────────

    #[test]
    fn usage_reporting() {
        let events = vec![assistant_event("Ok")];
        let receipt = mock_receipt_with_usage(events, usage_100_50());
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[test]
    fn ir_usage_conversion() {
        let ir = IrUsage::from_io(42, 18);
        let u = ir_usage_to_usage(&ir);
        assert_eq!(u.prompt_tokens, 42);
        assert_eq!(u.completion_tokens, 18);
        assert_eq!(u.total_tokens, 60);
    }

    // ── 1g. Error mapping ──────────────────────────────────────────────

    #[test]
    fn shim_error_variants() {
        let e1 = ShimError::InvalidRequest("bad".into());
        assert!(e1.to_string().contains("bad"));
        let e2 = ShimError::Internal("oops".into());
        assert!(e2.to_string().contains("oops"));
    }

    // ── 1h. IR roundtrip ───────────────────────────────────────────────

    #[test]
    fn messages_ir_roundtrip() {
        let msgs = vec![Message::user("Hello"), Message::assistant("Hi")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert!(back.iter().any(|m| m.role == Role::User));
        assert!(back.iter().any(|m| m.role == Role::Assistant));
    }

    #[test]
    fn tools_to_ir_conversion() {
        let tools = vec![Tool::function(
            "read_file",
            "Read a file",
            json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        )];
        let ir = tools_to_ir(&tools);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir[0].name, "read_file");
        assert_eq!(ir[0].description, "Read a file");
    }

    // ── 1i. Client API surface ─────────────────────────────────────────

    #[tokio::test]
    async fn client_chat_completions_create() {
        let client = OpenAiClient::new("gpt-4o")
            .with_processor(Box::new(|_| mock_receipt(vec![assistant_event("done")])));
        assert_eq!(client.model(), "gpt-4o");

        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hi")])
            .build();
        let resp = client.chat().completions().create(req).await.unwrap();
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("done"));
    }

    #[tokio::test]
    async fn client_streaming() {
        let client = OpenAiClient::new("gpt-4o").with_processor(Box::new(|_| {
            mock_receipt(vec![delta_event("a"), delta_event("b")])
        }));
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hi")])
            .build();
        let stream = client
            .chat()
            .completions()
            .create_stream(req)
            .await
            .unwrap();
        let chunks: Vec<StreamEvent> = stream.collect().await;
        assert_eq!(chunks.len(), 3); // 2 deltas + stop
    }

    #[tokio::test]
    async fn client_no_processor_errors() {
        let client = OpenAiClient::new("gpt-4o");
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hi")])
            .build();
        let result = client.chat().completions().create(req).await;
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2 · Claude Shim
// ═══════════════════════════════════════════════════════════════════════════

mod claude {
    use super::*;
    use abp_shim_claude::*;

    // ── 2a. Type existence & serde ─────────────────────────────────────

    #[test]
    fn role_variants() {
        assert_eq!(serde_json::to_value(Role::User).unwrap(), "user");
        assert_eq!(serde_json::to_value(Role::Assistant).unwrap(), "assistant");
    }

    #[test]
    fn content_block_text_serde() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        let back: ContentBlock = serde_roundtrip(&block);
        assert_eq!(back, block);
    }

    #[test]
    fn content_block_tool_use_serde() {
        let block = ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        };
        let back: ContentBlock = serde_roundtrip(&block);
        assert_eq!(back, block);
    }

    #[test]
    fn content_block_tool_result_serde() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("found it".into()),
            is_error: Some(false),
        };
        let back: ContentBlock = serde_roundtrip(&block);
        assert_eq!(back, block);
    }

    #[test]
    fn content_block_thinking_serde() {
        let block = ContentBlock::Thinking {
            thinking: "Let me think...".into(),
            signature: Some("sig_abc".into()),
        };
        let back: ContentBlock = serde_roundtrip(&block);
        assert_eq!(back, block);
    }

    #[test]
    fn content_block_image_serde() {
        let block = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "aGVsbG8=".into(),
            },
        };
        let back: ContentBlock = serde_roundtrip(&block);
        assert_eq!(back, block);
    }

    #[test]
    fn image_source_url_variant() {
        let src = ImageSource::Url {
            url: "https://example.com/img.png".into(),
        };
        let back: ImageSource = serde_roundtrip(&src);
        assert_eq!(back, src);
    }

    // ── 2b. Message request construction ───────────────────────────────

    #[test]
    fn message_request_fields() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".into(),
                }],
            }],
            system: Some("Be concise".into()),
            temperature: Some(0.5),
            stop_sequences: Some(vec!["END".into()]),
            thinking: None,
            stream: None,
        };
        assert_eq!(req.model, "claude-sonnet-4-20250514");
        assert_eq!(req.max_tokens, 4096);
        assert_eq!(req.system.as_deref(), Some("Be concise"));
    }

    // ── 2c. Content block IR roundtrip ─────────────────────────────────

    #[test]
    fn content_block_ir_roundtrip() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    #[test]
    fn content_block_tool_use_ir_roundtrip() {
        let block = ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "search".into(),
            input: json!({"q": "test"}),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    #[test]
    fn image_ir_roundtrip() {
        let block = ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/img.png".into(),
            },
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    // ── 2d. Request → Claude SDK → Response ────────────────────────────

    #[test]
    fn request_to_claude_conversion() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "Hi".into() }],
            }],
            system: Some("Be helpful".into()),
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let claude_req = request_to_claude(&req);
        assert_eq!(claude_req.model, "claude-sonnet-4-20250514");
        assert_eq!(claude_req.max_tokens, 1024);
        assert_eq!(claude_req.system.as_deref(), Some("Be helpful"));
        assert_eq!(claude_req.messages.len(), 1);
    }

    // ── 2e. Tool calling format ────────────────────────────────────────

    #[test]
    fn response_from_events_tool_use() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_123".into()),
                parent_tool_use_id: None,
                input: json!({"path": "/tmp/x"}),
            },
            ext: None,
        }];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert!(resp
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { name, .. } if name == "read_file")));
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    }

    // ── 2f. Streaming ──────────────────────────────────────────────────

    #[test]
    fn stream_event_variants_exist() {
        // Verify all stream event variants compile
        let _start = StreamEvent::MessageStart {
            message: MessageResponse {
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
            },
        };
        let _delta = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta { text: "hi".into() },
        };
        let _stop = StreamEvent::MessageStop {};
        let _ping = StreamEvent::Ping {};
    }

    #[test]
    fn stream_delta_variants() {
        let _text = StreamDelta::TextDelta { text: "hi".into() };
        let _json = StreamDelta::InputJsonDelta {
            partial_json: "{".into(),
        };
        let _think = StreamDelta::ThinkingDelta {
            thinking: "hmm".into(),
        };
        let _sig = StreamDelta::SignatureDelta {
            signature: "sig".into(),
        };
    }

    // ── 2g. System prompt handling ─────────────────────────────────────

    #[test]
    fn system_prompt_is_separate_field() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "Hi".into() }],
            }],
            system: Some("You are a cat".into()),
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        // Claude uses a separate system field, not a system role message
        assert_eq!(req.system.as_deref(), Some("You are a cat"));
        let claude_req = request_to_claude(&req);
        assert_eq!(claude_req.system.as_deref(), Some("You are a cat"));
    }

    // ── 2h. Token usage with cache ─────────────────────────────────────

    #[test]
    fn usage_with_cache_tokens() {
        let u = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: Some(20),
            cache_read_input_tokens: Some(30),
        };
        let back: Usage = serde_roundtrip(&u);
        assert_eq!(back, u);
        assert_eq!(back.cache_creation_input_tokens, Some(20));
        assert_eq!(back.cache_read_input_tokens, Some(30));
    }

    // ── 2i. Error mapping ──────────────────────────────────────────────

    #[test]
    fn shim_error_variants() {
        let e1 = ShimError::InvalidRequest("bad".into());
        assert!(e1.to_string().contains("bad"));
        let e2 = ShimError::ApiError {
            error_type: "overloaded_error".into(),
            message: "too busy".into(),
        };
        assert!(e2.to_string().contains("too busy"));
        let e3 = ShimError::Internal("whoops".into());
        assert!(e3.to_string().contains("whoops"));
    }

    #[test]
    fn api_error_type_serde() {
        let err = ApiError {
            error_type: "not_found_error".into(),
            message: "model not found".into(),
        };
        let back: ApiError = serde_roundtrip(&err);
        assert_eq!(back, err);
    }

    // ── 2j. WorkOrder conversion ───────────────────────────────────────

    #[test]
    fn request_to_work_order_extracts_task() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Explain Rust".into(),
                }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let wo = request_to_work_order(&req);
        assert!(wo.task.contains("Explain Rust"));
    }

    // ── 2k. Client ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn client_creates_response() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
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
        };
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.response_type, "message");
        assert_eq!(resp.role, "assistant");
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

    #[tokio::test]
    async fn client_streaming() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "Hi".into() }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: Some(true),
        };
        let stream = client.create_stream(req).await.unwrap();
        let events = stream.collect_all().await;
        assert!(!events.is_empty());
        assert!(events
            .iter()
            .any(|e| matches!(e, StreamEvent::MessageStart { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, StreamEvent::MessageStop { .. })));
    }

    #[test]
    fn event_stream_from_vec() {
        use tokio_stream::Stream;
        let stream = EventStream::from_vec(vec![StreamEvent::Ping {}, StreamEvent::MessageStop {}]);
        assert_eq!(stream.size_hint(), (2, Some(2)));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3 · Gemini Shim
// ═══════════════════════════════════════════════════════════════════════════

mod gemini {
    use super::*;
    use abp_shim_gemini::*;

    // ── 3a. Part constructors & serde ──────────────────────────────────

    #[test]
    fn part_text() {
        let p = Part::text("Hello");
        assert!(matches!(p, Part::Text(t) if t == "Hello"));
    }

    #[test]
    fn part_inline_data() {
        let p = Part::inline_data("image/png", "base64data");
        assert!(matches!(
            p,
            Part::InlineData { mime_type, .. } if mime_type == "image/png"
        ));
    }

    #[test]
    fn part_function_call() {
        let p = Part::function_call("search", json!({"q": "rust"}));
        assert!(matches!(
            p,
            Part::FunctionCall { name, .. } if name == "search"
        ));
    }

    #[test]
    fn part_function_response() {
        let p = Part::function_response("search", json!({"results": []}));
        assert!(matches!(
            p,
            Part::FunctionResponse { name, .. } if name == "search"
        ));
    }

    // ── 3b. Content constructors ───────────────────────────────────────

    #[test]
    fn content_user_and_model() {
        let user = Content::user(vec![Part::text("Hi")]);
        assert_eq!(user.role, "user");

        let model = Content::model(vec![Part::text("Hello")]);
        assert_eq!(model.role, "model");
    }

    // ── 3c. Request builder ────────────────────────────────────────────

    #[test]
    fn request_builder_chaining() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hi")]))
            .generation_config(GenerationConfig {
                temperature: Some(0.8),
                max_output_tokens: Some(2048),
                ..Default::default()
            })
            .tools(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "search".into(),
                    description: "Search the web".into(),
                    parameters: json!({"type": "object"}),
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
        assert!(req.tools.is_some());
        assert!(req.tool_config.is_some());
    }

    // ── 3d. System prompt ──────────────────────────────────────────────

    #[test]
    fn system_instruction_content() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content {
                role: "user".into(),
                parts: vec![Part::text("You are a cat")],
            })
            .add_content(Content::user(vec![Part::text("Meow?")]));
        assert!(req.system_instruction.is_some());
        let si = req.system_instruction.unwrap();
        assert!(matches!(&si.parts[0], Part::Text(t) if t == "You are a cat"));
    }

    // ── 3e. Dialect conversion ─────────────────────────────────────────

    #[test]
    fn to_dialect_request_roundtrip() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]));
        let dialect = to_dialect_request(&req);
        assert_eq!(dialect.model, "gemini-2.5-flash");
        assert_eq!(dialect.contents.len(), 1);
    }

    #[test]
    fn from_dialect_response_conversion() {
        use abp_gemini_sdk::dialect::{GeminiCandidate, GeminiResponse, GeminiUsageMetadata};
        let dialect_resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: abp_gemini_sdk::dialect::GeminiContent {
                    role: "model".into(),
                    parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("Hi!".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 5,
                total_token_count: 15,
            }),
        };
        let resp = from_dialect_response(&dialect_resp);
        assert_eq!(resp.text(), Some("Hi!"));
        let u = resp.usage_metadata.unwrap();
        assert_eq!(u.prompt_token_count, 10);
        assert_eq!(u.candidates_token_count, 5);
        assert_eq!(u.total_token_count, 15);
    }

    // ── 3f. Function calls in response ─────────────────────────────────

    #[test]
    fn response_function_calls_extraction() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::function_call(
                    "get_weather",
                    json!({"city": "NYC"}),
                )]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        let calls = resp.function_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "get_weather");
    }

    // ── 3g. Safety settings ────────────────────────────────────────────

    #[test]
    fn safety_setting_serde() {
        let s = SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        };
        let back: SafetySetting = serde_roundtrip(&s);
        assert_eq!(back, s);
    }

    // ── 3h. Usage conversion ───────────────────────────────────────────

    #[test]
    fn usage_to_ir_and_back() {
        let u = UsageMetadata {
            prompt_token_count: 100,
            candidates_token_count: 50,
            total_token_count: 150,
        };
        let ir = usage_to_ir(&u);
        assert_eq!(ir.input_tokens, 100);
        assert_eq!(ir.output_tokens, 50);
        assert_eq!(ir.total_tokens, 150);

        let back = usage_from_ir(&ir);
        assert_eq!(back, u);
    }

    // ── 3i. Generation config from dialect ─────────────────────────────

    #[test]
    fn gen_config_dialect_roundtrip() {
        let cfg = GenerationConfig {
            max_output_tokens: Some(1024),
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
            stop_sequences: Some(vec!["END".into()]),
            response_mime_type: Some("application/json".into()),
            response_schema: Some(json!({"type": "object"})),
            candidate_count: None,
        };
        // Convert to dialect and back
        let dialect = abp_gemini_sdk::dialect::GeminiGenerationConfig {
            max_output_tokens: cfg.max_output_tokens,
            temperature: cfg.temperature,
            top_p: cfg.top_p,
            top_k: cfg.top_k,
            candidate_count: None,
            stop_sequences: cfg.stop_sequences.clone(),
            response_mime_type: cfg.response_mime_type.clone(),
            response_schema: cfg.response_schema.clone(),
        };
        let back = gen_config_from_dialect(&dialect);
        assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
        assert_eq!(back.temperature, cfg.temperature);
    }

    // ── 3j. Error variants ─────────────────────────────────────────────

    #[test]
    fn gemini_error_variants() {
        let e1 = GeminiError::RequestConversion("bad request".into());
        assert!(e1.to_string().contains("bad request"));
        let e2 = GeminiError::ResponseConversion("bad response".into());
        assert!(e2.to_string().contains("bad response"));
        let e3 = GeminiError::BackendError("fail".into());
        assert!(e3.to_string().contains("fail"));
    }

    // ── 3k. Client surface ─────────────────────────────────────────────

    #[tokio::test]
    async fn client_generate() {
        let client = PipelineClient::new("gemini-2.5-flash");
        assert_eq!(client.model(), "gemini-2.5-flash");

        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]));
        let resp = client.generate(req).await.unwrap();
        assert!(!resp.candidates.is_empty());
    }

    #[tokio::test]
    async fn client_generate_stream() {
        let client = PipelineClient::new("gemini-2.5-flash");
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]));
        let mut stream = client.generate_stream(req).await.unwrap();
        let mut count = 0;
        while let Some(_event) = stream.next().await {
            count += 1;
        }
        assert!(count > 0);
    }

    // ── 3l. Stream event text extraction ───────────────────────────────

    #[test]
    fn stream_event_text_extraction() {
        let event = StreamEvent {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("chunk")]),
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
        };
        assert_eq!(event.text(), Some("chunk"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4 · Kimi Shim
// ═══════════════════════════════════════════════════════════════════════════

mod kimi {
    use super::*;
    use abp_shim_kimi::*;

    // ── 4a. Message constructors ───────────────────────────────────────

    #[test]
    fn message_constructors() {
        let sys = Message::system("Be helpful");
        assert_eq!(sys.role, "system");
        assert_eq!(sys.content.as_deref(), Some("Be helpful"));

        let user = Message::user("Hello");
        assert_eq!(user.role, "user");

        let asst = Message::assistant("Hi");
        assert_eq!(asst.role, "assistant");

        let tool = Message::tool("call_1", "result");
        assert_eq!(tool.role, "tool");
        assert_eq!(tool.tool_call_id.as_deref(), Some("call_1"));
    }

    // ── 4b. Builder defaults ───────────────────────────────────────────

    #[test]
    fn builder_defaults() {
        let req = KimiRequestBuilder::new().build();
        assert_eq!(req.model, "moonshot-v1-8k");
    }

    #[test]
    fn builder_all_fields() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::user("Hi")])
            .max_tokens(4096)
            .temperature(0.8)
            .stream(true)
            .use_search(true)
            .build();
        assert_eq!(req.model, "moonshot-v1-128k");
        assert!(req.stream == Some(true));
        assert!(req.use_search == Some(true));
    }

    // ── 4c. Request → IR → WorkOrder ───────────────────────────────────

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("Hello")])
            .build();
        let wo = request_to_work_order(&req);
        assert!(wo.config.model.as_deref() == Some("moonshot-v1-8k"));
    }

    #[test]
    fn request_to_ir_has_user_message() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("What is Rust?")])
            .build();
        let ir = request_to_ir(&req);
        assert!(ir.messages.iter().any(|m| m.role == IrRole::User));
    }

    // ── 4d. Receipt → Response (tool calls) ────────────────────────────

    #[test]
    fn receipt_maps_tool_calls() {
        let events = vec![tool_call_event(
            "web_search",
            "call_1",
            json!({"query": "test"}),
        )];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "web_search");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn receipt_maps_text() {
        let events = vec![assistant_event("Reply")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Reply"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ── 4e. Streaming ──────────────────────────────────────────────────

    #[test]
    fn events_to_stream_chunks_structure() {
        let events = vec![delta_event("He"), delta_event("llo")];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        // 2 deltas + 1 stop
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].object, "chat.completion.chunk");
        assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("He"));
        assert_eq!(chunks[2].choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ── 4f. System prompt ──────────────────────────────────────────────

    #[test]
    fn system_prompt_in_messages() {
        let req = KimiRequestBuilder::new()
            .messages(vec![
                Message::system("You are a translator"),
                Message::user("Translate: hello"),
            ])
            .build();
        assert_eq!(req.messages[0].role, "system");
    }

    // ── 4g. Token usage ────────────────────────────────────────────────

    #[test]
    fn usage_reporting() {
        let events = vec![assistant_event("Ok")];
        let receipt = mock_receipt_with_usage(events, usage_100_50());
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[test]
    fn ir_usage_conversion() {
        let ir = IrUsage::from_io(42, 18);
        let u = ir_usage_to_usage(&ir);
        assert_eq!(u.prompt_tokens, 42);
        assert_eq!(u.completion_tokens, 18);
        assert_eq!(u.total_tokens, 60);
    }

    // ── 4h. Error mapping ──────────────────────────────────────────────

    #[test]
    fn shim_error_variants() {
        let e1 = ShimError::InvalidRequest("bad".into());
        assert!(e1.to_string().contains("bad"));
        let e2 = ShimError::Internal("oops".into());
        assert!(e2.to_string().contains("oops"));
    }

    // ── 4i. IR roundtrip ───────────────────────────────────────────────

    #[test]
    fn messages_ir_roundtrip() {
        let msgs = vec![Message::user("Hello"), Message::assistant("Hi")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert!(back.iter().any(|m| m.role == "user"));
        assert!(back.iter().any(|m| m.role == "assistant"));
    }

    #[test]
    fn response_to_ir_conversion() {
        let events = vec![assistant_event("Hi there")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        let ir = response_to_ir(&resp);
        assert!(!ir.messages.is_empty());
    }

    // ── 4j. Client surface ─────────────────────────────────────────────

    #[tokio::test]
    async fn client_create() {
        let client = KimiClient::new("moonshot-v1-8k")
            .with_processor(Box::new(|_| mock_receipt(vec![assistant_event("done")])));
        assert_eq!(client.model(), "moonshot-v1-8k");

        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .build();
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("done"));
    }

    #[tokio::test]
    async fn client_streaming() {
        let client = KimiClient::new("moonshot-v1-8k").with_processor(Box::new(|_| {
            mock_receipt(vec![delta_event("a"), delta_event("b")])
        }));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .build();
        let stream = client.create_stream(req).await.unwrap();
        let chunks: Vec<_> = stream.collect().await;
        assert_eq!(chunks.len(), 3); // 2 deltas + stop
    }

    #[tokio::test]
    async fn client_no_processor_errors() {
        let client = KimiClient::new("moonshot-v1-8k");
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .build();
        let result = client.create(req).await;
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5 · Codex Shim
// ═══════════════════════════════════════════════════════════════════════════

mod codex {
    use super::*;
    use abp_shim_codex::*;

    // ── 5a. Input constructors ─────────────────────────────────────────

    #[test]
    fn codex_message_constructor() {
        let item = codex_message("user", "Hello");
        assert!(matches!(
            item,
            abp_codex_sdk::dialect::CodexInputItem::Message { role, content }
            if role == "user" && content == "Hello"
        ));
    }

    // ── 5b. Builder defaults ───────────────────────────────────────────

    #[test]
    fn builder_defaults() {
        let req = CodexRequestBuilder::new().build();
        assert_eq!(req.model, "codex-mini-latest");
        assert!(req.input.is_empty());
    }

    #[test]
    fn builder_all_fields() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-2025-01")
            .input(vec![codex_message("user", "Hi")])
            .max_output_tokens(2048)
            .temperature(0.3)
            .build();
        assert_eq!(req.model, "codex-mini-2025-01");
        assert_eq!(req.input.len(), 1);
        assert_eq!(req.max_output_tokens, Some(2048));
        assert_eq!(req.temperature, Some(0.3));
    }

    // ── 5c. Request → WorkOrder ────────────────────────────────────────

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "Hello")])
            .build();
        let wo = request_to_work_order(&req);
        assert!(wo.config.model.as_deref() == Some("codex-mini-latest"));
    }

    #[test]
    fn request_to_ir_has_messages() {
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "What is Rust?")])
            .build();
        let ir = request_to_ir(&req);
        assert!(!ir.messages.is_empty());
    }

    // ── 5d. Receipt → Response (tool calls) ────────────────────────────

    #[test]
    fn receipt_maps_tool_calls() {
        let events = vec![tool_call_event("run_cmd", "fc_1", json!({"cmd": "ls"}))];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(resp.output.iter().any(|item| {
            matches!(
                item,
                abp_codex_sdk::dialect::CodexResponseItem::FunctionCall { name, .. }
                if name == "run_cmd"
            )
        }));
    }

    #[test]
    fn receipt_maps_text() {
        let events = vec![assistant_event("Done!")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert_eq!(resp.model, "codex-mini-latest");
        assert_eq!(resp.output.len(), 1);
    }

    #[test]
    fn receipt_maps_error() {
        let events = vec![error_event("sandbox failure")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(resp.output.iter().any(|item| {
            matches!(
                item,
                abp_codex_sdk::dialect::CodexResponseItem::Message { content, .. }
                if content.iter().any(|p| matches!(
                    p,
                    abp_codex_sdk::dialect::CodexContentPart::OutputText { text }
                    if text.contains("sandbox failure")
                ))
            )
        }));
    }

    // ── 5e. Streaming ──────────────────────────────────────────────────

    #[test]
    fn events_to_stream_structure() {
        let events = vec![delta_event("He"), delta_event("llo")];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        // 1 created + 2 deltas + 1 completed
        assert_eq!(stream.len(), 4);
        assert!(matches!(
            &stream[0],
            abp_codex_sdk::dialect::CodexStreamEvent::ResponseCreated { .. }
        ));
        assert!(matches!(
            &stream[3],
            abp_codex_sdk::dialect::CodexStreamEvent::ResponseCompleted { .. }
        ));
    }

    // ── 5f. Token usage ────────────────────────────────────────────────

    #[test]
    fn usage_reporting() {
        let events = vec![assistant_event("Ok")];
        let receipt = mock_receipt_with_usage(events, usage_100_50());
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        let u = resp.usage.unwrap();
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[test]
    fn ir_usage_conversion() {
        let ir = IrUsage::from_io(42, 18);
        let u = ir_usage_to_usage(&ir);
        assert_eq!(u.input_tokens, 42);
        assert_eq!(u.output_tokens, 18);
        assert_eq!(u.total_tokens, 60);
    }

    // ── 5g. Error mapping ──────────────────────────────────────────────

    #[test]
    fn shim_error_variants() {
        let e1 = ShimError::InvalidRequest("bad".into());
        assert!(e1.to_string().contains("bad"));
        let e2 = ShimError::Internal("oops".into());
        assert!(e2.to_string().contains("oops"));
    }

    // ── 5h. IR roundtrip ───────────────────────────────────────────────

    #[test]
    fn response_to_ir_conversion() {
        let events = vec![assistant_event("Hello")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        let ir = response_to_ir(&resp);
        assert!(!ir.messages.is_empty());
    }

    #[test]
    fn ir_to_response_items_roundtrip() {
        let events = vec![assistant_event("Hello")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        let ir = response_to_ir(&resp);
        let items = ir_to_response_items(&ir);
        assert!(!items.is_empty());
    }

    // ── 5i. Client surface ─────────────────────────────────────────────

    #[tokio::test]
    async fn client_create() {
        let client = CodexClient::new("codex-mini-latest")
            .with_processor(Box::new(|_| mock_receipt(vec![assistant_event("done")])));
        assert_eq!(client.model(), "codex-mini-latest");

        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "Hi")])
            .build();
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.status.as_deref(), Some("completed"));
    }

    #[tokio::test]
    async fn client_streaming() {
        let client = CodexClient::new("codex-mini-latest").with_processor(Box::new(|_| {
            mock_receipt(vec![delta_event("a"), delta_event("b")])
        }));
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "Hi")])
            .build();
        let stream = client.create_stream(req).await.unwrap();
        let chunks: Vec<_> = stream.collect().await;
        assert_eq!(chunks.len(), 4); // created + 2 deltas + completed
    }

    #[tokio::test]
    async fn client_no_processor_errors() {
        let client = CodexClient::new("codex-mini-latest");
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "Hi")])
            .build();
        let result = client.create(req).await;
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6 · Copilot Shim
// ═══════════════════════════════════════════════════════════════════════════

mod copilot {
    use super::*;
    use abp_shim_copilot::*;

    // ── 6a. Message constructors ───────────────────────────────────────

    #[test]
    fn message_constructors() {
        let sys = Message::system("Be helpful");
        assert_eq!(sys.role, "system");
        assert_eq!(sys.content, "Be helpful");

        let user = Message::user("Hello");
        assert_eq!(user.role, "user");

        let asst = Message::assistant("Hi");
        assert_eq!(asst.role, "assistant");
        assert!(asst.name.is_none());
        assert!(asst.copilot_references.is_empty());
    }

    // ── 6b. Builder defaults ───────────────────────────────────────────

    #[test]
    fn builder_defaults() {
        let req = CopilotRequestBuilder::new().build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn builder_all_fields() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("Hi")])
            .build();
        assert_eq!(req.model, "gpt-4-turbo");
        assert_eq!(req.messages.len(), 1);
    }

    // ── 6c. Request → IR → WorkOrder ───────────────────────────────────

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("Hello")])
            .build();
        let wo = request_to_work_order(&req);
        assert!(wo.config.model.as_deref() == Some("gpt-4o"));
    }

    #[test]
    fn request_to_ir_has_user_message() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("What is Rust?")])
            .build();
        let ir = request_to_ir(&req);
        assert!(ir.messages.iter().any(|m| m.role == IrRole::User));
    }

    // ── 6d. Receipt → Response (tool calls) ────────────────────────────

    #[test]
    fn receipt_maps_tool_call() {
        let events = vec![tool_call_event(
            "get_code",
            "fc_1",
            json!({"path": "src/main.rs"}),
        )];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.function_call.is_some());
        assert_eq!(resp.function_call.as_ref().unwrap().name, "get_code");
    }

    #[test]
    fn receipt_maps_text() {
        let events = vec![assistant_event("Here you go!")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Here you go!");
    }

    #[test]
    fn receipt_maps_error_to_copilot_errors() {
        let events = vec![error_event("api failure")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.copilot_errors.is_empty());
        assert!(resp.copilot_errors[0].message.contains("api failure"));
    }

    // ── 6e. Streaming ──────────────────────────────────────────────────

    #[test]
    fn events_to_stream_structure() {
        let events = vec![delta_event("He"), delta_event("llo")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        // 1 references + 2 text deltas + 1 done
        assert_eq!(stream.len(), 4);
        assert!(matches!(
            &stream[0],
            abp_copilot_sdk::dialect::CopilotStreamEvent::CopilotReferences { .. }
        ));
        assert!(matches!(
            &stream[3],
            abp_copilot_sdk::dialect::CopilotStreamEvent::Done { .. }
        ));
    }

    #[test]
    fn stream_error_event() {
        let events = vec![error_event("timeout")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(stream.iter().any(|e| matches!(
            e,
            abp_copilot_sdk::dialect::CopilotStreamEvent::CopilotErrors { .. }
        )));
    }

    // ── 6f. System prompt ──────────────────────────────────────────────

    #[test]
    fn system_prompt_in_messages() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![
                Message::system("You are a code reviewer"),
                Message::user("Review this"),
            ])
            .build();
        assert_eq!(req.messages[0].role, "system");
    }

    // ── 6g. Token usage ────────────────────────────────────────────────

    #[test]
    fn ir_usage_to_tuple_conversion() {
        let ir = IrUsage::from_io(100, 50);
        let (input, output, total) = ir_usage_to_tuple(&ir);
        assert_eq!(input, 100);
        assert_eq!(output, 50);
        assert_eq!(total, 150);
    }

    // ── 6h. Error mapping ──────────────────────────────────────────────

    #[test]
    fn shim_error_variants() {
        let e1 = ShimError::InvalidRequest("bad".into());
        assert!(e1.to_string().contains("bad"));
        let e2 = ShimError::Internal("oops".into());
        assert!(e2.to_string().contains("oops"));
    }

    // ── 6i. IR roundtrip ───────────────────────────────────────────────

    #[test]
    fn messages_ir_roundtrip() {
        let msgs = vec![Message::user("Hello"), Message::assistant("Hi")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert!(back.iter().any(|m| m.role == "user"));
        assert!(back.iter().any(|m| m.role == "assistant"));
    }

    #[test]
    fn response_to_ir_conversion() {
        let events = vec![assistant_event("Hi there")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let ir = response_to_ir(&resp);
        assert!(!ir.messages.is_empty());
    }

    // ── 6j. Client surface ─────────────────────────────────────────────

    #[tokio::test]
    async fn client_create() {
        let client = CopilotClient::new("gpt-4o")
            .with_processor(Box::new(|_| mock_receipt(vec![assistant_event("done")])));
        assert_eq!(client.model(), "gpt-4o");

        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .build();
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.message, "done");
    }

    #[tokio::test]
    async fn client_streaming() {
        let client = CopilotClient::new("gpt-4o").with_processor(Box::new(|_| {
            mock_receipt(vec![delta_event("a"), delta_event("b")])
        }));
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .build();
        let stream = client.create_stream(req).await.unwrap();
        let chunks: Vec<_> = stream.collect().await;
        assert!(!chunks.is_empty());
    }

    #[tokio::test]
    async fn client_no_processor_errors() {
        let client = CopilotClient::new("gpt-4o");
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .build();
        let result = client.create(req).await;
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7 · Cross-shim consistency checks
// ═══════════════════════════════════════════════════════════════════════════

mod cross_shim {
    use super::*;

    /// All shims should produce a response from the same set of agent events.
    #[test]
    fn all_shims_handle_assistant_event() {
        let events = vec![assistant_event("Hello from ABP")];

        // OpenAI
        let r = abp_shim_openai::mock_receipt(events.clone());
        let oai = abp_shim_openai::receipt_to_response(&r, "gpt-4o");
        assert_eq!(
            oai.choices[0].message.content.as_deref(),
            Some("Hello from ABP")
        );

        // Kimi
        let r = abp_shim_kimi::mock_receipt(events.clone());
        let kimi = abp_shim_kimi::receipt_to_response(&r, "moonshot-v1-8k");
        assert_eq!(
            kimi.choices[0].message.content.as_deref(),
            Some("Hello from ABP")
        );

        // Codex
        let r = abp_shim_codex::mock_receipt(events.clone());
        let codex = abp_shim_codex::receipt_to_response(&r, "codex-mini-latest");
        assert!(!codex.output.is_empty());

        // Copilot
        let r = abp_shim_copilot::mock_receipt(events);
        let cop = abp_shim_copilot::receipt_to_response(&r, "gpt-4o");
        assert_eq!(cop.message, "Hello from ABP");
    }

    /// All shims should map tool call events into their SDK-specific format.
    #[test]
    fn all_shims_handle_tool_call_event() {
        let events = vec![tool_call_event(
            "get_info",
            "call_x",
            json!({"key": "value"}),
        )];

        // OpenAI: tool_calls array
        let r = abp_shim_openai::mock_receipt(events.clone());
        let oai = abp_shim_openai::receipt_to_response(&r, "gpt-4o");
        assert!(oai.choices[0].message.tool_calls.is_some());

        // Kimi: tool_calls array (OpenAI-compatible)
        let r = abp_shim_kimi::mock_receipt(events.clone());
        let kimi = abp_shim_kimi::receipt_to_response(&r, "moonshot-v1-8k");
        assert!(kimi.choices[0].message.tool_calls.is_some());

        // Codex: FunctionCall response item
        let r = abp_shim_codex::mock_receipt(events.clone());
        let codex = abp_shim_codex::receipt_to_response(&r, "codex-mini-latest");
        assert!(codex.output.iter().any(|item| {
            matches!(
                item,
                abp_codex_sdk::dialect::CodexResponseItem::FunctionCall { .. }
            )
        }));

        // Copilot: single function_call field
        let r = abp_shim_copilot::mock_receipt(events);
        let cop = abp_shim_copilot::receipt_to_response(&r, "gpt-4o");
        assert!(cop.function_call.is_some());
    }

    /// Usage tokens should be consistent across all shims from the same receipt.
    #[test]
    fn usage_consistency_across_shims() {
        let events = vec![assistant_event("Ok")];
        let usage = usage_100_50();

        let r = abp_shim_openai::mock_receipt_with_usage(events.clone(), usage.clone());
        let oai = abp_shim_openai::receipt_to_response(&r, "gpt-4o");
        let oai_u = oai.usage.unwrap();

        let r = abp_shim_kimi::mock_receipt_with_usage(events.clone(), usage.clone());
        let kimi = abp_shim_kimi::receipt_to_response(&r, "moonshot-v1-8k");
        let kimi_u = kimi.usage.unwrap();

        let r = abp_shim_codex::mock_receipt_with_usage(events.clone(), usage.clone());
        let codex = abp_shim_codex::receipt_to_response(&r, "codex-mini-latest");
        let codex_u = codex.usage.unwrap();

        // OpenAI and Kimi use the same field names
        assert_eq!(oai_u.prompt_tokens, kimi_u.prompt_tokens);
        assert_eq!(oai_u.completion_tokens, kimi_u.completion_tokens);
        assert_eq!(oai_u.total_tokens, kimi_u.total_tokens);

        // Codex uses input_tokens/output_tokens
        assert_eq!(codex_u.input_tokens, oai_u.prompt_tokens);
        assert_eq!(codex_u.output_tokens, oai_u.completion_tokens);
        assert_eq!(codex_u.total_tokens, oai_u.total_tokens);
    }

    /// All shims that have error events should surface them in their response.
    #[test]
    fn error_handling_across_shims() {
        let events = vec![error_event("something went wrong")];

        // OpenAI: appears in content
        let r = abp_shim_openai::mock_receipt(events.clone());
        let oai = abp_shim_openai::receipt_to_response(&r, "gpt-4o");
        assert!(oai.choices[0]
            .message
            .content
            .as_deref()
            .unwrap()
            .contains("something went wrong"));

        // Kimi: appears in content
        let r = abp_shim_kimi::mock_receipt(events.clone());
        let kimi = abp_shim_kimi::receipt_to_response(&r, "moonshot-v1-8k");
        assert!(kimi.choices[0]
            .message
            .content
            .as_deref()
            .unwrap()
            .contains("something went wrong"));

        // Codex: appears in output text
        let r = abp_shim_codex::mock_receipt(events.clone());
        let codex = abp_shim_codex::receipt_to_response(&r, "codex-mini-latest");
        assert!(codex.output.iter().any(|item| {
            matches!(
                item,
                abp_codex_sdk::dialect::CodexResponseItem::Message { content, .. }
                if content.iter().any(|p| matches!(
                    p,
                    abp_codex_sdk::dialect::CodexContentPart::OutputText { text }
                    if text.contains("something went wrong")
                ))
            )
        }));

        // Copilot: appears in copilot_errors
        let r = abp_shim_copilot::mock_receipt(events);
        let cop = abp_shim_copilot::receipt_to_response(&r, "gpt-4o");
        assert!(cop
            .copilot_errors
            .iter()
            .any(|e| e.message.contains("something went wrong")));
    }
}
