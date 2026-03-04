#![allow(clippy::all)]
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
//! Shim API Surface Tests
//!
//! Verifies the public API surface of each of the 6 shim crates matches
//! expected SDK patterns: required fields, serde traits, builder defaults,
//! IR roundtrips, streaming vs non-streaming, and cross-shim compatibility.

use abp_core::ir::{IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind};
use chrono::Utc;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn assert_serialize_deserialize<T: Serialize + DeserializeOwned + std::fmt::Debug>(val: &T) {
    let json = serde_json::to_string(val).expect("serialize");
    let _back: T = serde_json::from_str(&json).expect("deserialize");
}

fn make_assistant_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn make_delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: text.into() },
        ext: None,
    }
}

fn make_tool_call_event(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
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

fn make_error_event(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.into(),
            error_code: None,
        },
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 1: OpenAI shim — request fields, response shape, builder, serde
// ═══════════════════════════════════════════════════════════════════════

mod openai_shim {
    use super::*;
    use abp_shim_openai::*;

    // ── 1. Request type has required SDK fields ────────────────────────
    #[test]
    fn request_has_model_messages_tools_temperature_stream() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("hi")])
            .temperature(0.7)
            .stream(true)
            .tools(vec![Tool::function("f", "desc", json!({}))])
            .build();

        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 1);
        assert!(req.tools.is_some());
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.stream, Some(true));
    }

    // ── 2. Response type matches SDK shape ─────────────────────────────
    #[test]
    fn response_shape_has_id_object_model_choices_usage() {
        let events = vec![make_assistant_event("hello")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");

        assert!(resp.id.starts_with("chatcmpl-"));
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.choices.len(), 1);
        assert!(resp.usage.is_some());
    }

    // ── 3. Converts to/from IR ─────────────────────────────────────────
    #[test]
    fn request_to_ir_roundtrip() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::system("Be concise"), Message::user("Hello")])
            .build();

        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[1].role, IrRole::User);
    }

    // ── 4. Serialize + Deserialize ─────────────────────────────────────
    #[test]
    fn request_serde_roundtrip() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("test")])
            .build();
        assert_serialize_deserialize(&req);
    }

    #[test]
    fn response_serde_roundtrip() {
        let events = vec![make_assistant_event("hi")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_serialize_deserialize(&resp);
    }

    // ── 5. Clone + Debug ───────────────────────────────────────────────
    #[test]
    fn request_clone_debug() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("test")])
            .build();
        let _cloned = req.clone();
        let debug = format!("{:?}", req);
        assert!(debug.contains("gpt-4o"));
    }

    #[test]
    fn message_clone_debug() {
        let msg = Message::user("hi");
        let _cloned = msg.clone();
        let debug = format!("{:?}", msg);
        assert!(debug.contains("User"));
    }

    // ── 6. Builder default model ───────────────────────────────────────
    #[test]
    fn builder_defaults_model_to_gpt4o() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("hi")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    // ── 7. Builder patterns chain correctly ────────────────────────────
    #[test]
    fn builder_chains_all_fields() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("hi")])
            .temperature(0.5)
            .max_tokens(100)
            .stop(vec!["END".into()])
            .stream(false)
            .build();

        assert_eq!(req.model, "gpt-4-turbo");
        assert_eq!(req.temperature, Some(0.5));
        assert_eq!(req.max_tokens, Some(100));
        assert_eq!(req.stop.as_ref().unwrap().len(), 1);
        assert_eq!(req.stream, Some(false));
    }

    // ── 8. Streaming events ────────────────────────────────────────────
    #[test]
    fn stream_events_end_with_stop() {
        let events = vec![make_delta_event("hi")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(stream.len() >= 2); // at least delta + stop
        let last = stream.last().unwrap();
        assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ── 9. Non-streaming response ──────────────────────────────────────
    #[test]
    fn non_streaming_response_has_finish_reason() {
        let events = vec![make_assistant_event("done")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ── 10. Messages to IR and back ────────────────────────────────────
    #[test]
    fn messages_ir_roundtrip() {
        let msgs = vec![
            Message::system("sys"),
            Message::user("usr"),
            Message::assistant("asst"),
        ];
        let conv = messages_to_ir(&msgs);
        let back = ir_to_messages(&conv);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, Role::System);
        assert_eq!(back[1].role, Role::User);
        assert_eq!(back[2].role, Role::Assistant);
    }

    // ── 11. IR usage conversion ────────────────────────────────────────
    #[test]
    fn ir_usage_converts() {
        let ir = IrUsage::from_io(100, 50);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    // ── 12. Tool type serde ────────────────────────────────────────────
    #[test]
    fn tool_serde_roundtrip() {
        let tool = Tool::function("read_file", "Read a file", json!({"type": "object"}));
        assert_serialize_deserialize(&tool);
        assert_eq!(tool.tool_type, "function");
    }

    // ── 13. Tool call in response ──────────────────────────────────────
    #[test]
    fn tool_call_mapped_to_response() {
        let events = vec![make_tool_call_event(
            "search",
            "call_1",
            json!({"q": "rust"}),
        )];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "search");
        assert_eq!(tc[0].call_type, "function");
    }

    // ── 14. Work order preserves model ─────────────────────────────────
    #[test]
    fn work_order_preserves_model() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    // ── 15. Client Debug impl ──────────────────────────────────────────
    #[test]
    fn client_debug() {
        let client = OpenAiClient::new("gpt-4o");
        let debug = format!("{:?}", client);
        assert!(debug.contains("OpenAiClient"));
        assert_eq!(client.model(), "gpt-4o");
    }

    // ── 16. Stream events for tool calls ───────────────────────────────
    #[test]
    fn stream_events_include_tool_calls() {
        let events = vec![make_tool_call_event("f", "id1", json!({}))];
        let stream = events_to_stream_events(&events, "gpt-4o");
        let has_tool = stream
            .iter()
            .any(|e| e.choices.iter().any(|c| c.delta.tool_calls.is_some()));
        assert!(has_tool);
    }

    // ── 17. Usage type serde ───────────────────────────────────────────
    #[test]
    fn usage_serde() {
        let u = Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        };
        assert_serialize_deserialize(&u);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 2: Claude shim — request fields, response shape, builder, serde
// ═══════════════════════════════════════════════════════════════════════

mod claude_shim {
    use super::*;
    use abp_shim_claude::*;

    fn simple_request(text: &str) -> MessageRequest {
        MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: text.into() }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        }
    }

    // ── 1. Request has model, messages, max_tokens, system fields ──────
    #[test]
    fn request_has_required_fields() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 8192,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }],
            system: Some("Be helpful".into()),
            temperature: Some(0.5),
            stop_sequences: None,
            thinking: None,
            stream: Some(false),
        };

        assert_eq!(req.model, "claude-sonnet-4-20250514");
        assert_eq!(req.max_tokens, 8192);
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.system.as_deref(), Some("Be helpful"));
    }

    // ── 2. Response shape matches SDK ──────────────────────────────────
    #[tokio::test]
    async fn response_shape() {
        let client = AnthropicClient::new();
        let resp = client.create(simple_request("Hello")).await.unwrap();

        assert_eq!(resp.response_type, "message");
        assert_eq!(resp.role, "assistant");
        assert!(resp.id.starts_with("msg_"));
        assert!(!resp.content.is_empty());
    }

    // ── 3. Serialize + Deserialize ─────────────────────────────────────
    #[test]
    fn request_serde_roundtrip() {
        let req = simple_request("test");
        assert_serialize_deserialize(&req);
    }

    #[test]
    fn content_block_serde_roundtrip() {
        let blocks = vec![
            ContentBlock::Text { text: "hi".into() },
            ContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: json!({}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: Some("ok".into()),
                is_error: Some(false),
            },
            ContentBlock::Thinking {
                thinking: "hmm".into(),
                signature: Some("sig".into()),
            },
        ];
        for block in &blocks {
            assert_serialize_deserialize(block);
        }
    }

    // ── 4. Clone + Debug ───────────────────────────────────────────────
    #[test]
    fn message_clone_debug() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        };
        let _cloned = msg.clone();
        let debug = format!("{:?}", msg);
        assert!(debug.contains("User"));
    }

    #[test]
    fn role_clone_debug_eq() {
        let r = Role::User;
        assert_eq!(r.clone(), Role::User);
        assert_ne!(r, Role::Assistant);
        let _ = format!("{:?}", r);
    }

    // ── 5. Client defaults ─────────────────────────────────────────────
    #[test]
    fn client_default_model() {
        let client = AnthropicClient::new();
        let debug = format!("{:?}", client);
        assert!(debug.contains("AnthropicClient"));
    }

    #[test]
    fn client_with_model() {
        let client = AnthropicClient::with_model("claude-3-haiku");
        let debug = format!("{:?}", client);
        assert!(debug.contains("claude-3-haiku"));
    }

    // ── 6. Content block conversions ───────────────────────────────────
    #[test]
    fn content_block_to_ir_and_back() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn tool_use_block_roundtrip() {
        let block = ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "bash".into(),
            input: json!({"cmd": "ls"}),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    // ── 7. Message to IR conversion ────────────────────────────────────
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

    // ── 8. Request to work order ───────────────────────────────────────
    #[test]
    fn request_to_work_order_preserves_model() {
        let req = simple_request("test");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    // ── 9. Stream events ───────────────────────────────────────────────
    #[test]
    fn stream_delta_serde() {
        let delta = StreamDelta::TextDelta { text: "hi".into() };
        assert_serialize_deserialize(&delta);
    }

    #[test]
    fn stream_event_variants_serde() {
        let events = vec![
            StreamEvent::Ping {},
            StreamEvent::MessageStop {},
            StreamEvent::ContentBlockStop { index: 0 },
        ];
        for event in &events {
            assert_serialize_deserialize(event);
        }
    }

    // ── 10. Usage type ─────────────────────────────────────────────────
    #[test]
    fn usage_serde() {
        let u = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(5),
        };
        assert_serialize_deserialize(&u);
    }

    // ── 11. Empty messages rejected ────────────────────────────────────
    #[tokio::test]
    async fn empty_messages_rejected() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
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

    // ── 12. Thinking block support ─────────────────────────────────────
    #[test]
    fn thinking_block_serde() {
        let block = ContentBlock::Thinking {
            thinking: "reasoning...".into(),
            signature: None,
        };
        assert_serialize_deserialize(&block);
    }

    // ── 13. Image source serde ─────────────────────────────────────────
    #[test]
    fn image_source_serde() {
        let src = ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "abc123".into(),
        };
        assert_serialize_deserialize(&src);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 3: Gemini shim — request fields, response shape, builder, serde
// ═══════════════════════════════════════════════════════════════════════

mod gemini_shim {
    use super::*;
    use abp_shim_gemini::*;

    // ── 1. Request has model, contents, generationConfig ───────────────
    #[test]
    fn request_has_model_contents_generation_config() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("hi")]))
            .generation_config(GenerationConfig {
                max_output_tokens: Some(1024),
                temperature: Some(0.7),
                ..Default::default()
            });

        assert_eq!(req.model, "gemini-2.5-flash");
        assert_eq!(req.contents.len(), 1);
        assert!(req.generation_config.is_some());
        assert_eq!(
            req.generation_config.as_ref().unwrap().temperature,
            Some(0.7)
        );
    }

    // ── 2. Response shape ──────────────────────────────────────────────
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

    // ── 3. Serialize + Deserialize ─────────────────────────────────────
    #[test]
    fn request_serde_roundtrip() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("hi")]));
        assert_serialize_deserialize(&req);
    }

    #[test]
    fn response_serde_roundtrip() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("hey")]),
                finish_reason: Some("STOP".into()),
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 20,
                total_token_count: 30,
            }),
        };
        assert_serialize_deserialize(&resp);
    }

    // ── 4. Clone + Debug ───────────────────────────────────────────────
    #[test]
    fn content_clone_debug() {
        let c = Content::user(vec![Part::text("hi")]);
        let _cloned = c.clone();
        let debug = format!("{:?}", c);
        assert!(debug.contains("user"));
    }

    #[test]
    fn part_clone_debug() {
        let p = Part::text("hi");
        let _cloned = p.clone();
        let _ = format!("{:?}", p);
    }

    // ── 5. Builder patterns ────────────────────────────────────────────
    #[test]
    fn request_builder_chain() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("hi")]))
            .system_instruction(Content::user(vec![Part::text("be concise")]))
            .generation_config(GenerationConfig {
                temperature: Some(0.5),
                ..Default::default()
            });

        assert!(req.system_instruction.is_some());
        assert_eq!(req.contents.len(), 1);
    }

    // ── 6. Part constructors ───────────────────────────────────────────
    #[test]
    fn part_constructors() {
        let _text = Part::text("hi");
        let _data = Part::inline_data("image/png", "abc");
        let _fc = Part::function_call("fn1", json!({}));
        let _fr = Part::function_response("fn1", json!({"result": "ok"}));
    }

    // ── 7. Content constructors ────────────────────────────────────────
    #[test]
    fn content_user_and_model() {
        let u = Content::user(vec![Part::text("hi")]);
        assert_eq!(u.role, "user");
        let m = Content::model(vec![Part::text("hello")]);
        assert_eq!(m.role, "model");
    }

    // ── 8. Function calls extraction ───────────────────────────────────
    #[test]
    fn response_function_calls() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::function_call("search", json!({"q": "rust"}))]),
                finish_reason: Some("STOP".into()),
            }],
            usage_metadata: None,
        };
        let calls = resp.function_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "search");
    }

    // ── 9. GenerationConfig defaults ───────────────────────────────────
    #[test]
    fn generation_config_default() {
        let cfg = GenerationConfig::default();
        assert!(cfg.max_output_tokens.is_none());
        assert!(cfg.temperature.is_none());
        assert!(cfg.top_p.is_none());
        assert!(cfg.top_k.is_none());
        assert!(cfg.stop_sequences.is_none());
    }

    // ── 10. Usage metadata serde ───────────────────────────────────────
    #[test]
    fn usage_metadata_serde() {
        let u = UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 20,
            total_token_count: 30,
        };
        assert_serialize_deserialize(&u);
    }

    // ── 11. Tool declarations serde ────────────────────────────────────
    #[test]
    fn tool_declaration_serde() {
        let td = ToolDeclaration {
            function_declarations: vec![FunctionDeclaration {
                name: "read".into(),
                description: "Read file".into(),
                parameters: json!({"type": "object"}),
            }],
        };
        assert_serialize_deserialize(&td);
    }

    // ── 12. Safety setting serde ───────────────────────────────────────
    #[test]
    fn safety_setting_serde() {
        let ss = SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        };
        assert_serialize_deserialize(&ss);
    }

    // ── 13. Stream event serde ─────────────────────────────────────────
    #[test]
    fn stream_event_serde() {
        let se = StreamEvent {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("hi")]),
                finish_reason: None,
            }],
            usage_metadata: None,
        };
        assert_serialize_deserialize(&se);
    }

    // ── 14. Dialect round-trip ─────────────────────────────────────────
    #[test]
    fn to_dialect_request_roundtrip() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("hello")]));
        let dialect_req = to_dialect_request(&req);
        assert_eq!(dialect_req.model, "gemini-2.5-flash");
        assert_eq!(dialect_req.contents.len(), 1);
    }

    // ── 15. Client constructor ─────────────────────────────────────────
    #[test]
    fn client_new_and_model() {
        let client = GeminiClient::new("gemini-2.5-flash");
        assert_eq!(client.model(), "gemini-2.5-flash");
        let debug = format!("{:?}", client);
        assert!(debug.contains("GeminiClient"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 4: Codex shim — request fields, response shape, builder, serde
// ═══════════════════════════════════════════════════════════════════════

mod codex_shim {
    use super::*;
    use abp_shim_codex::*;

    // ── 1. Request has model, input, max_output_tokens, temperature ────
    #[test]
    fn request_has_required_fields() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "hi")])
            .max_output_tokens(1024)
            .temperature(0.5)
            .build();

        assert_eq!(req.model, "codex-mini-latest");
        assert!(!req.input.is_empty());
        assert_eq!(req.max_output_tokens, Some(1024));
        assert_eq!(req.temperature, Some(0.5));
    }

    // ── 2. Response shape ──────────────────────────────────────────────
    #[test]
    fn response_has_id_model_output_usage() {
        let events = vec![make_assistant_event("hello")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");

        assert!(resp.id.starts_with("resp_"));
        assert_eq!(resp.model, "codex-mini-latest");
        assert!(!resp.output.is_empty());
        assert!(resp.usage.is_some());
    }

    // ── 3. Converts to/from IR ─────────────────────────────────────────
    #[test]
    fn request_to_ir_works() {
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "hello")])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    // ── 4. Serialize + Deserialize ─────────────────────────────────────
    #[test]
    fn usage_serde() {
        let u = Usage {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
        };
        assert_serialize_deserialize(&u);
    }

    // ── 5. Clone + Debug ───────────────────────────────────────────────
    #[test]
    fn usage_clone_debug() {
        let u = Usage {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
        };
        let _cloned = u.clone();
        let _ = format!("{:?}", u);
    }

    // ── 6. Builder default model ───────────────────────────────────────
    #[test]
    fn builder_defaults_model_to_codex_mini() {
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "hi")])
            .build();
        assert_eq!(req.model, "codex-mini-latest");
    }

    // ── 7. Work order preserves model ──────────────────────────────────
    #[test]
    fn work_order_preserves_model() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    // ── 8. Streaming events ────────────────────────────────────────────
    #[test]
    fn stream_events_bookend() {
        let events = vec![make_delta_event("hi")];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        assert!(stream.len() >= 3); // created + delta + completed
    }

    // ── 9. Tool call in response ───────────────────────────────────────
    #[test]
    fn tool_call_mapped_to_response() {
        let events = vec![make_tool_call_event("shell", "fc_1", json!({"cmd": "ls"}))];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());
    }

    // ── 10. Response to IR ─────────────────────────────────────────────
    #[test]
    fn response_to_ir_works() {
        let events = vec![make_assistant_event("hello")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        let conv = response_to_ir(&resp);
        assert!(!conv.is_empty());
    }

    // ── 11. IR usage conversion ────────────────────────────────────────
    #[test]
    fn ir_usage_converts() {
        let ir = IrUsage::from_io(200, 100);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.total_tokens, 300);
    }

    // ── 12. Client debug ───────────────────────────────────────────────
    #[test]
    fn client_debug() {
        let client = CodexClient::new("codex-mini-latest");
        let debug = format!("{:?}", client);
        assert!(debug.contains("CodexClient"));
        assert_eq!(client.model(), "codex-mini-latest");
    }

    // ── 13. Error events ───────────────────────────────────────────────
    #[test]
    fn error_event_in_response() {
        let events = vec![make_error_event("rate limited")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 5: Kimi shim — request fields, response shape, builder, serde
// ═══════════════════════════════════════════════════════════════════════

mod kimi_shim {
    use super::*;
    use abp_shim_kimi::*;

    // ── 1. Request has model, messages, max_tokens, temperature, stream ─
    #[test]
    fn request_has_required_fields() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("hi")])
            .max_tokens(1024)
            .temperature(0.7)
            .stream(true)
            .build();

        assert_eq!(req.model, "moonshot-v1-8k");
        assert!(!req.messages.is_empty());
        assert_eq!(req.max_tokens, Some(1024));
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.stream, Some(true));
    }

    // ── 2. Response shape ──────────────────────────────────────────────
    #[test]
    fn response_shape() {
        let events = vec![make_assistant_event("hello")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");

        assert!(resp.id.starts_with("cmpl-"));
        assert_eq!(resp.model, "moonshot-v1-8k");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.role, "assistant");
    }

    // ── 3. Converts to/from IR ─────────────────────────────────────────
    #[test]
    fn request_to_ir_works() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("hello")])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    // ── 4. Serialize + Deserialize ─────────────────────────────────────
    #[test]
    fn message_serde() {
        let msg = Message::user("hi");
        assert_serialize_deserialize(&msg);
    }

    #[test]
    fn usage_serde() {
        let u = Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        };
        assert_serialize_deserialize(&u);
    }

    // ── 5. Clone + Debug ───────────────────────────────────────────────
    #[test]
    fn message_clone_debug() {
        let msg = Message::user("hi");
        let _cloned = msg.clone();
        let debug = format!("{:?}", msg);
        assert!(debug.contains("user"));
    }

    // ── 6. Builder default model ───────────────────────────────────────
    #[test]
    fn builder_defaults_model_to_moonshot() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("hi")])
            .build();
        assert_eq!(req.model, "moonshot-v1-8k");
    }

    // ── 7. Builder chains ──────────────────────────────────────────────
    #[test]
    fn builder_chains_all_fields() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::user("hi")])
            .max_tokens(2048)
            .temperature(0.3)
            .stream(false)
            .use_search(true)
            .build();

        assert_eq!(req.model, "moonshot-v1-128k");
        assert_eq!(req.max_tokens, Some(2048));
        assert_eq!(req.temperature, Some(0.3));
        assert_eq!(req.stream, Some(false));
        assert_eq!(req.use_search, Some(true));
    }

    // ── 8. Work order preserves model ──────────────────────────────────
    #[test]
    fn work_order_preserves_model() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
    }

    // ── 9. Streaming chunks ────────────────────────────────────────────
    #[test]
    fn stream_chunks_end_with_stop() {
        let events = vec![make_delta_event("hi")];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert!(chunks.len() >= 2); // delta + stop
        let last = chunks.last().unwrap();
        assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ── 10. Message constructors ───────────────────────────────────────
    #[test]
    fn message_constructors() {
        let sys = Message::system("sys");
        assert_eq!(sys.role, "system");
        let usr = Message::user("usr");
        assert_eq!(usr.role, "user");
        let asst = Message::assistant("asst");
        assert_eq!(asst.role, "assistant");
        let tool = Message::tool("tid", "result");
        assert_eq!(tool.role, "tool");
        assert_eq!(tool.tool_call_id.as_deref(), Some("tid"));
    }

    // ── 11. Messages to IR and back ────────────────────────────────────
    #[test]
    fn messages_ir_roundtrip() {
        let msgs = vec![
            Message::system("sys"),
            Message::user("usr"),
            Message::assistant("asst"),
        ];
        let conv = messages_to_ir(&msgs);
        let back = ir_to_messages(&conv);
        assert_eq!(back.len(), 3);
    }

    // ── 12. IR usage conversion ────────────────────────────────────────
    #[test]
    fn ir_usage_converts() {
        let ir = IrUsage::from_io(100, 50);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    // ── 13. Response to IR ─────────────────────────────────────────────
    #[test]
    fn response_to_ir_works() {
        let events = vec![make_assistant_event("hello")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        let conv = response_to_ir(&resp);
        assert!(!conv.is_empty());
    }

    // ── 14. Client debug ───────────────────────────────────────────────
    #[test]
    fn client_debug() {
        let client = KimiClient::new("moonshot-v1-8k");
        let debug = format!("{:?}", client);
        assert!(debug.contains("KimiClient"));
        assert_eq!(client.model(), "moonshot-v1-8k");
    }

    // ── 15. Tool call in response ──────────────────────────────────────
    #[test]
    fn tool_call_in_response() {
        let events = vec![make_tool_call_event(
            "search",
            "call_1",
            json!({"q": "rust"}),
        )];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "search");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 6: Copilot shim — request fields, response shape, builder, serde
// ═══════════════════════════════════════════════════════════════════════

mod copilot_shim {
    use super::*;
    use abp_shim_copilot::*;

    // ── 1. Request has model, messages, tools ──────────────────────────
    #[test]
    fn request_has_required_fields() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("hi")])
            .build();

        assert_eq!(req.model, "gpt-4o");
        assert!(!req.messages.is_empty());
    }

    // ── 2. Response from receipt ───────────────────────────────────────
    #[test]
    fn response_from_receipt() {
        let events = vec![make_assistant_event("hello")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");

        assert_eq!(resp.message, "hello");
        assert!(resp.copilot_errors.is_empty());
    }

    // ── 3. Converts to/from IR ─────────────────────────────────────────
    #[test]
    fn request_to_ir_works() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("hello")])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    // ── 4. Serialize + Deserialize ─────────────────────────────────────
    #[test]
    fn message_serde() {
        let msg = Message::user("hi");
        assert_serialize_deserialize(&msg);
    }

    // ── 5. Clone + Debug ───────────────────────────────────────────────
    #[test]
    fn message_clone_debug() {
        let msg = Message::user("hi");
        let _cloned = msg.clone();
        let debug = format!("{:?}", msg);
        assert!(debug.contains("user"));
    }

    // ── 6. Builder default model ───────────────────────────────────────
    #[test]
    fn builder_defaults_model_to_gpt4o() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("hi")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    // ── 7. Work order preserves model ──────────────────────────────────
    #[test]
    fn work_order_preserves_model() {
        let req = CopilotRequestBuilder::new()
            .model("o3-mini")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
    }

    // ── 8. Stream events ───────────────────────────────────────────────
    #[test]
    fn stream_events_end_with_done() {
        let events = vec![make_delta_event("hi")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    // ── 9. Message constructors ────────────────────────────────────────
    #[test]
    fn message_constructors() {
        let sys = Message::system("sys");
        assert_eq!(sys.role, "system");
        let usr = Message::user("usr");
        assert_eq!(usr.role, "user");
        let asst = Message::assistant("asst");
        assert_eq!(asst.role, "assistant");
    }

    // ── 10. Messages to IR and back ────────────────────────────────────
    #[test]
    fn messages_ir_roundtrip() {
        let msgs = vec![
            Message::system("sys"),
            Message::user("usr"),
            Message::assistant("asst"),
        ];
        let conv = messages_to_ir(&msgs);
        let back = ir_to_messages(&conv);
        assert_eq!(back.len(), 3);
    }

    // ── 11. IR usage conversion ────────────────────────────────────────
    #[test]
    fn ir_usage_converts() {
        let ir = IrUsage::from_io(200, 100);
        let (input, output, total) = ir_usage_to_tuple(&ir);
        assert_eq!(input, 200);
        assert_eq!(output, 100);
        assert_eq!(total, 300);
    }

    // ── 12. Response to IR ─────────────────────────────────────────────
    #[test]
    fn response_to_ir_works() {
        use abp_copilot_sdk::dialect::CopilotResponse;
        let resp = CopilotResponse {
            message: "hello".into(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: None,
        };
        let conv = response_to_ir(&resp);
        assert!(!conv.is_empty());
    }

    // ── 13. Client debug ───────────────────────────────────────────────
    #[test]
    fn client_debug() {
        let client = CopilotClient::new("gpt-4o");
        let debug = format!("{:?}", client);
        assert!(debug.contains("CopilotClient"));
        assert_eq!(client.model(), "gpt-4o");
    }

    // ── 14. Error events in response ───────────────────────────────────
    #[test]
    fn error_events_in_response() {
        let events = vec![make_error_event("rate limited")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.copilot_errors.len(), 1);
        assert!(resp.copilot_errors[0].message.contains("rate limited"));
    }

    // ── 15. Empty response to IR ───────────────────────────────────────
    #[test]
    fn empty_response_to_ir() {
        use abp_copilot_sdk::dialect::CopilotResponse;
        let resp = CopilotResponse {
            message: String::new(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: None,
        };
        let conv = response_to_ir(&resp);
        assert!(conv.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 7: Cross-shim type compatibility via IR
// ═══════════════════════════════════════════════════════════════════════

mod cross_shim_ir {
    use super::*;

    // ── 1. OpenAI → IR → Kimi roundtrip ────────────────────────────────
    #[test]
    fn openai_to_kimi_via_ir() {
        let openai_msgs = vec![
            abp_shim_openai::Message::system("Be concise"),
            abp_shim_openai::Message::user("Hello"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&openai_msgs);

        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(kimi_msgs.len(), 2);
        assert_eq!(kimi_msgs[0].role, "system");
        assert_eq!(kimi_msgs[1].role, "user");
    }

    // ── 2. Kimi → IR → OpenAI roundtrip ────────────────────────────────
    #[test]
    fn kimi_to_openai_via_ir() {
        let kimi_msgs = vec![
            abp_shim_kimi::Message::system("Be helpful"),
            abp_shim_kimi::Message::user("Hi"),
        ];
        let ir = abp_shim_kimi::messages_to_ir(&kimi_msgs);

        let openai_msgs = abp_shim_openai::ir_to_messages(&ir);
        assert_eq!(openai_msgs.len(), 2);
        assert_eq!(openai_msgs[0].role, abp_shim_openai::Role::System);
        assert_eq!(openai_msgs[1].role, abp_shim_openai::Role::User);
    }

    // ── 3. OpenAI → IR → Copilot roundtrip ─────────────────────────────
    #[test]
    fn openai_to_copilot_via_ir() {
        let openai_msgs = vec![
            abp_shim_openai::Message::user("Hello"),
            abp_shim_openai::Message::assistant("Hi there"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&openai_msgs);

        let copilot_msgs = abp_shim_copilot::ir_to_messages(&ir);
        assert_eq!(copilot_msgs.len(), 2);
        assert_eq!(copilot_msgs[0].role, "user");
        assert_eq!(copilot_msgs[1].role, "assistant");
    }

    // ── 4. Copilot → IR → Kimi roundtrip ───────────────────────────────
    #[test]
    fn copilot_to_kimi_via_ir() {
        let copilot_msgs = vec![
            abp_shim_copilot::Message::system("System prompt"),
            abp_shim_copilot::Message::user("Question"),
        ];
        let ir = abp_shim_copilot::messages_to_ir(&copilot_msgs);

        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(kimi_msgs.len(), 2);
        assert_eq!(kimi_msgs[0].role, "system");
        assert_eq!(kimi_msgs[1].role, "user");
    }

    // ── 5. All shims produce IrRole::User for user messages ────────────
    #[test]
    fn all_shims_user_role_consistent() {
        // OpenAI
        let openai_ir = abp_shim_openai::messages_to_ir(&[abp_shim_openai::Message::user("hi")]);
        assert_eq!(openai_ir.messages[0].role, IrRole::User);

        // Kimi
        let kimi_ir = abp_shim_kimi::messages_to_ir(&[abp_shim_kimi::Message::user("hi")]);
        assert_eq!(kimi_ir.messages[0].role, IrRole::User);

        // Copilot
        let copilot_ir = abp_shim_copilot::messages_to_ir(&[abp_shim_copilot::Message::user("hi")]);
        assert_eq!(copilot_ir.messages[0].role, IrRole::User);

        // Codex
        let codex_req = abp_shim_codex::CodexRequestBuilder::new()
            .input(vec![abp_shim_codex::codex_message("user", "hi")])
            .build();
        let codex_ir = abp_shim_codex::request_to_ir(&codex_req);
        assert_eq!(codex_ir.messages[0].role, IrRole::User);
    }

    // ── 6. All shims produce IrRole::Assistant for assistant messages ───
    #[test]
    fn all_shims_assistant_role_consistent() {
        // OpenAI
        let openai_ir =
            abp_shim_openai::messages_to_ir(&[abp_shim_openai::Message::assistant("ok")]);
        assert_eq!(openai_ir.messages[0].role, IrRole::Assistant);

        // Kimi
        let kimi_ir = abp_shim_kimi::messages_to_ir(&[abp_shim_kimi::Message::assistant("ok")]);
        assert_eq!(kimi_ir.messages[0].role, IrRole::Assistant);

        // Copilot
        let copilot_ir =
            abp_shim_copilot::messages_to_ir(&[abp_shim_copilot::Message::assistant("ok")]);
        assert_eq!(copilot_ir.messages[0].role, IrRole::Assistant);
    }

    // ── 7. All shims produce IrRole::System for system messages ────────
    #[test]
    fn all_shims_system_role_consistent() {
        // OpenAI
        let openai_ir = abp_shim_openai::messages_to_ir(&[abp_shim_openai::Message::system("sys")]);
        assert_eq!(openai_ir.messages[0].role, IrRole::System);

        // Kimi
        let kimi_ir = abp_shim_kimi::messages_to_ir(&[abp_shim_kimi::Message::system("sys")]);
        assert_eq!(kimi_ir.messages[0].role, IrRole::System);

        // Copilot
        let copilot_ir =
            abp_shim_copilot::messages_to_ir(&[abp_shim_copilot::Message::system("sys")]);
        assert_eq!(copilot_ir.messages[0].role, IrRole::System);
    }

    // ── 8. Multi-turn cross-shim ───────────────────────────────────────
    #[test]
    fn multi_turn_openai_to_kimi() {
        let openai_msgs = vec![
            abp_shim_openai::Message::system("context"),
            abp_shim_openai::Message::user("q1"),
            abp_shim_openai::Message::assistant("a1"),
            abp_shim_openai::Message::user("q2"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&openai_msgs);
        assert_eq!(ir.len(), 4);

        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(kimi_msgs.len(), 4);
        assert_eq!(kimi_msgs[3].role, "user");
    }

    // ── 9. IR usage types are interoperable ────────────────────────────
    #[test]
    fn ir_usage_interop() {
        let ir = IrUsage::from_io(100, 50);

        let openai_usage = abp_shim_openai::ir_usage_to_usage(&ir);
        assert_eq!(openai_usage.prompt_tokens, 100);

        let kimi_usage = abp_shim_kimi::ir_usage_to_usage(&ir);
        assert_eq!(kimi_usage.prompt_tokens, 100);

        let codex_usage = abp_shim_codex::ir_usage_to_usage(&ir);
        assert_eq!(codex_usage.input_tokens, 100);

        let (copilot_in, copilot_out, copilot_total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
        assert_eq!(copilot_in, 100);
        assert_eq!(copilot_out, 50);
        assert_eq!(copilot_total, 150);
    }

    // ── 10. Text content preserved across shim boundaries ──────────────
    #[test]
    fn text_content_preserved_across_shims() {
        let text = "The quick brown fox";

        // OpenAI → IR → Kimi
        let openai_ir = abp_shim_openai::messages_to_ir(&[abp_shim_openai::Message::user(text)]);
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&openai_ir);
        assert_eq!(kimi_msgs[0].content.as_deref(), Some(text));

        // Kimi → IR → Copilot
        let kimi_ir = abp_shim_kimi::messages_to_ir(&[abp_shim_kimi::Message::user(text)]);
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&kimi_ir);
        assert_eq!(copilot_msgs[0].content, text);

        // Copilot → IR → OpenAI
        let copilot_ir = abp_shim_copilot::messages_to_ir(&[abp_shim_copilot::Message::user(text)]);
        let openai_msgs = abp_shim_openai::ir_to_messages(&copilot_ir);
        assert_eq!(openai_msgs[0].content.as_deref(), Some(text));
    }
}
