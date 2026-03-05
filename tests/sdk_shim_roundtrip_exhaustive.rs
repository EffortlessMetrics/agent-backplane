#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive SDK shim round-trip tests validating that every SDK shim can
//! faithfully convert to and from ABP IR types.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, Outcome, Receipt, ReceiptBuilder, RuntimeConfig, UsageNormalized,
    WorkOrder, WorkOrderBuilder,
};
use chrono::Utc;
use serde_json::json;

// ── Shared test helpers ─────────────────────────────────────────────────

fn make_receipt_with_events(events: Vec<AgentEvent>) -> Receipt {
    make_receipt_with_events_and_usage(events, UsageNormalized::default())
}

fn make_receipt_with_events_and_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: abp_core::RunMetadata {
            run_id: uuid::Uuid::new_v4(),
            work_order_id: uuid::Uuid::new_v4(),
            contract_version: abp_core::CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: abp_core::BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: Default::default(),
        mode: abp_core::ExecutionMode::Mapped,
        usage_raw: serde_json::Value::Null,
        usage,
        trace: events,
        artifacts: vec![],
        verification: Default::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn text_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn tool_call_event(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.to_string(),
            tool_use_id: Some(id.to_string()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn error_event(message: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: message.to_string(),
            error_code: None,
        },
        ext: None,
    }
}

fn run_completed_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".to_string(),
        },
        ext: None,
    }
}

fn sample_usage() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 1: OpenAI shim tests
// ═══════════════════════════════════════════════════════════════════════════
mod openai {
    use super::*;
    use abp_shim_openai::*;

    fn simple_request() -> ChatCompletionRequest {
        ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Hello")])
            .build()
    }

    // ── SDK → IR ────────────────────────────────────────────────────────

    #[test]
    fn request_to_ir_basic() {
        let req = simple_request();
        let conv = request_to_ir(&req);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn request_to_ir_with_system() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::system("Be concise"), Message::user("Hello")])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be concise");
    }

    #[test]
    fn request_to_ir_multi_turn() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![
                Message::user("Hi"),
                Message::assistant("Hello!"),
                Message::user("How are you?"),
            ])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.messages.len(), 3);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[1].role, IrRole::Assistant);
        assert_eq!(conv.messages[2].role, IrRole::User);
    }

    // ── IR → SDK ────────────────────────────────────────────────────────

    #[test]
    fn ir_to_messages_basic() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Hello"),
            IrMessage::text(IrRole::Assistant, "Hi there"),
        ]);
        let msgs = ir_to_messages(&conv);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, Role::User);
        assert_eq!(msgs[0].content.as_deref(), Some("Hello"));
        assert_eq!(msgs[1].role, Role::Assistant);
        assert_eq!(msgs[1].content.as_deref(), Some("Hi there"));
    }

    #[test]
    fn ir_to_messages_system_role() {
        let conv =
            IrConversation::from_messages(vec![IrMessage::text(IrRole::System, "System prompt")]);
        let msgs = ir_to_messages(&conv);
        assert_eq!(msgs[0].role, Role::System);
    }

    #[test]
    fn ir_to_messages_tool_role() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tc_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        )]);
        let msgs = ir_to_messages(&conv);
        assert_eq!(msgs[0].role, Role::Tool);
    }

    // ── Roundtrip: SDK → IR → SDK ───────────────────────────────────────

    #[test]
    fn messages_roundtrip_text() {
        let original = vec![
            Message::system("System prompt"),
            Message::user("User message"),
            Message::assistant("Assistant reply"),
        ];
        let conv = messages_to_ir(&original);
        let back = ir_to_messages(&conv);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, Role::System);
        assert_eq!(back[0].content.as_deref(), Some("System prompt"));
        assert_eq!(back[1].role, Role::User);
        assert_eq!(back[1].content.as_deref(), Some("User message"));
        assert_eq!(back[2].role, Role::Assistant);
        assert_eq!(back[2].content.as_deref(), Some("Assistant reply"));
    }

    // ── Field preservation ──────────────────────────────────────────────

    #[test]
    fn work_order_preserves_model() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn work_order_preserves_temperature() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("test")])
            .temperature(0.7)
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("temperature"),
            Some(&serde_json::Value::from(0.7))
        );
    }

    #[test]
    fn work_order_preserves_max_tokens() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("test")])
            .max_tokens(2048)
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("max_tokens"),
            Some(&serde_json::Value::from(2048))
        );
    }

    // ── Usage conversion ────────────────────────────────────────────────

    #[test]
    fn ir_usage_to_openai_usage() {
        let ir = IrUsage::from_io(200, 100);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 200);
        assert_eq!(usage.completion_tokens, 100);
        assert_eq!(usage.total_tokens, 300);
    }

    #[test]
    fn ir_usage_zero_values() {
        let ir = IrUsage::from_io(0, 0);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    // ── Receipt → Response ──────────────────────────────────────────────

    #[test]
    fn receipt_to_response_text_only() {
        let events = vec![text_event("Hello!")];
        let receipt = make_receipt_with_events(events);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = abp_shim_openai::convert::from_receipt(&receipt, &wo);
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn receipt_to_response_with_tool_calls() {
        let events = vec![tool_call_event("search", "tc_1", json!({"q": "rust"}))];
        let receipt = make_receipt_with_events(events);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = abp_shim_openai::convert::from_receipt(&receipt, &wo);
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].function.name, "search");
    }

    #[test]
    fn receipt_to_response_usage() {
        let events = vec![text_event("ok")];
        let receipt = make_receipt_with_events_and_usage(events, sample_usage());
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = abp_shim_openai::convert::from_receipt(&receipt, &wo);
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    // ── Streaming ───────────────────────────────────────────────────────

    #[test]
    fn agent_event_to_stream_chunk_delta() {
        let event = delta_event("Hello");
        let chunk = abp_shim_openai::convert::from_agent_event(&event, "gpt-4o", "chunk_1");
        assert!(chunk.is_some());
        let chunk = chunk.unwrap();
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello"));
    }

    #[test]
    fn agent_event_to_stream_chunk_stop() {
        let event = run_completed_event();
        let chunk = abp_shim_openai::convert::from_agent_event(&event, "gpt-4o", "chunk_1");
        assert!(chunk.is_some());
        let chunk = chunk.unwrap();
        assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn agent_event_unsupported_returns_none() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "test.rs".into(),
                summary: "modified".into(),
            },
            ext: None,
        };
        let chunk = abp_shim_openai::convert::from_agent_event(&event, "gpt-4o", "chunk_1");
        assert!(chunk.is_none());
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn empty_messages_request() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![])
            .build();
        let conv = request_to_ir(&req);
        assert!(conv.messages.is_empty());
    }

    #[test]
    fn unicode_content_preserved() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("こんにちは 🌍 مرحبا")])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.messages[0].text_content(), "こんにちは 🌍 مرحبا");
    }

    #[test]
    fn large_payload_preserved() {
        let large = "x".repeat(100_000);
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user(large.clone())])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.messages[0].text_content(), large);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 2: Claude shim tests
// ═══════════════════════════════════════════════════════════════════════════
mod claude {
    use super::*;
    use abp_shim_claude::convert;
    use abp_shim_claude::types::*;

    fn simple_request() -> MessagesRequest {
        MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Hello".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        }
    }

    // ── SDK → WorkOrder ─────────────────────────────────────────────────

    #[test]
    fn request_to_work_order_basic() {
        let req = simple_request();
        let wo = convert::to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert!(!wo.task.is_empty());
    }

    #[test]
    fn request_to_work_order_preserves_system() {
        let mut req = simple_request();
        req.system = Some("Be helpful".into());
        let wo = convert::to_work_order(&req);
        assert_eq!(wo.config.vendor.get("system"), Some(&json!("Be helpful")));
    }

    #[test]
    fn request_to_work_order_preserves_temperature() {
        let mut req = simple_request();
        req.temperature = Some(0.5);
        let wo = convert::to_work_order(&req);
        assert_eq!(wo.config.vendor.get("temperature"), Some(&json!(0.5)));
    }

    #[test]
    fn request_to_work_order_preserves_max_tokens() {
        let req = simple_request();
        let wo = convert::to_work_order(&req);
        assert_eq!(wo.config.vendor.get("max_tokens"), Some(&json!(1024)));
    }

    // ── Receipt → Response ──────────────────────────────────────────────

    #[test]
    fn receipt_to_response_text() {
        let events = vec![text_event("Hello!"), run_completed_event()];
        let receipt = make_receipt_with_events(events);
        let wo = WorkOrderBuilder::new("test")
            .model("claude-sonnet-4-20250514")
            .build();
        let resp = convert::from_receipt(&receipt, &wo);
        assert_eq!(resp.model, "claude-sonnet-4-20250514");
        assert_eq!(resp.role, "assistant");
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello!"),
            _ => panic!("expected Text block"),
        }
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn receipt_to_response_tool_use() {
        let events = vec![tool_call_event("search", "tu_1", json!({"q": "rust"}))];
        let receipt = make_receipt_with_events(events);
        let wo = WorkOrderBuilder::new("test")
            .model("claude-sonnet-4-20250514")
            .build();
        let resp = convert::from_receipt(&receipt, &wo);
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        match &resp.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_1");
                assert_eq!(name, "search");
                assert_eq!(input, &json!({"q": "rust"}));
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    // ── Streaming ───────────────────────────────────────────────────────

    #[test]
    fn agent_event_to_stream_delta() {
        let event = delta_event("Hi");
        let stream_ev = convert::from_agent_event(&event);
        assert!(stream_ev.is_some());
        match stream_ev.unwrap() {
            StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                StreamDelta::TextDelta { text } => assert_eq!(text, "Hi"),
                _ => panic!("expected TextDelta"),
            },
            _ => panic!("expected ContentBlockDelta"),
        }
    }

    #[test]
    fn agent_event_to_stream_run_completed() {
        let event = run_completed_event();
        let stream_ev = convert::from_agent_event(&event);
        assert!(stream_ev.is_some());
        match stream_ev.unwrap() {
            StreamEvent::MessageDelta { delta, .. } => {
                assert_eq!(delta.stop_reason.as_deref(), Some("end_turn"));
            }
            _ => panic!("expected MessageDelta"),
        }
    }

    // ── Role mapping ────────────────────────────────────────────────────

    #[test]
    fn role_to_abp_user() {
        assert_eq!(convert::map_role_to_abp("user"), "user");
    }

    #[test]
    fn role_to_abp_assistant() {
        assert_eq!(convert::map_role_to_abp("assistant"), "assistant");
    }

    #[test]
    fn role_from_abp_system_maps_to_user() {
        assert_eq!(convert::map_role_from_abp("system"), "user");
    }

    #[test]
    fn role_from_abp_tool_maps_to_user() {
        assert_eq!(convert::map_role_from_abp("tool"), "user");
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn empty_content_extraction() {
        let content = ClaudeContent::Text(String::new());
        assert!(convert::content_to_text(&content).is_none());
    }

    #[test]
    fn blocks_content_extraction() {
        let content = ClaudeContent::Blocks(vec![
            ContentBlock::Text {
                text: "Hello".into(),
            },
            ContentBlock::Text {
                text: " World".into(),
            },
        ]);
        assert_eq!(
            convert::content_to_text(&content),
            Some("Hello World".into())
        );
    }

    #[test]
    fn unicode_content_preserved() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("こんにちは 🌍 مرحبا".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = convert::to_work_order(&req);
        assert!(wo.task.contains("こんにちは"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 3: Gemini shim tests
// ═══════════════════════════════════════════════════════════════════════════
mod gemini {
    use super::*;
    use abp_shim_gemini::*;

    // ── Part ↔ Dialect roundtrip ────────────────────────────────────────

    #[test]
    fn text_part_roundtrip() {
        let part = Part::text("Hello");
        let dialect = part_to_dialect(&part);
        let back = part_from_dialect(&dialect);
        assert_eq!(part, back);
    }

    #[test]
    fn inline_data_part_roundtrip() {
        let part = Part::inline_data("image/png", "base64data");
        let dialect = part_to_dialect(&part);
        let back = part_from_dialect(&dialect);
        assert_eq!(part, back);
    }

    #[test]
    fn function_call_part_roundtrip() {
        let part = Part::function_call("search", json!({"query": "rust"}));
        let dialect = part_to_dialect(&part);
        let back = part_from_dialect(&dialect);
        assert_eq!(part, back);
    }

    #[test]
    fn function_response_part_roundtrip() {
        let part = Part::function_response("search", json!({"results": ["a", "b"]}));
        let dialect = part_to_dialect(&part);
        let back = part_from_dialect(&dialect);
        assert_eq!(part, back);
    }

    // ── Content ↔ Dialect roundtrip ─────────────────────────────────────

    #[test]
    fn content_user_roundtrip() {
        let content = Content::user(vec![Part::text("Hi")]);
        let dialect = content_to_dialect(&content);
        let back = content_from_dialect(&dialect);
        assert_eq!(back.role, "user");
        assert_eq!(back.parts.len(), 1);
    }

    #[test]
    fn content_model_roundtrip() {
        let content = Content::model(vec![Part::text("Hello"), Part::text("World")]);
        let dialect = content_to_dialect(&content);
        let back = content_from_dialect(&dialect);
        assert_eq!(back.role, "model");
        assert_eq!(back.parts.len(), 2);
    }

    // ── GenerationConfig roundtrip ──────────────────────────────────────

    #[test]
    fn gen_config_roundtrip_all_fields() {
        let cfg = GenerationConfig {
            max_output_tokens: Some(1024),
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
            candidate_count: None,
            stop_sequences: Some(vec!["END".into()]),
            response_mime_type: Some("application/json".into()),
            response_schema: Some(json!({"type": "object"})),
        };
        let dialect = gen_config_to_dialect(&cfg);
        let back = gen_config_from_dialect(&dialect);
        assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
        assert_eq!(back.temperature, cfg.temperature);
        assert_eq!(back.top_p, cfg.top_p);
        assert_eq!(back.top_k, cfg.top_k);
        assert_eq!(back.stop_sequences, cfg.stop_sequences);
        assert_eq!(back.response_mime_type, cfg.response_mime_type);
        assert_eq!(back.response_schema, cfg.response_schema);
    }

    #[test]
    fn gen_config_default_roundtrip() {
        let cfg = GenerationConfig::default();
        let dialect = gen_config_to_dialect(&cfg);
        let back = gen_config_from_dialect(&dialect);
        assert!(back.max_output_tokens.is_none());
        assert!(back.temperature.is_none());
    }

    // ── Usage ↔ IR roundtrip ────────────────────────────────────────────

    #[test]
    fn usage_to_ir_and_back() {
        let usage = UsageMetadata {
            prompt_token_count: 100,
            candidates_token_count: 50,
            total_token_count: 150,
        };
        let ir = usage_to_ir(&usage);
        let back = usage_from_ir(&ir);
        assert_eq!(back.prompt_token_count, 100);
        assert_eq!(back.candidates_token_count, 50);
        assert_eq!(back.total_token_count, 150);
    }

    #[test]
    fn usage_zero_values() {
        let ir = IrUsage::from_io(0, 0);
        let usage = usage_from_ir(&ir);
        assert_eq!(usage.prompt_token_count, 0);
        assert_eq!(usage.candidates_token_count, 0);
        assert_eq!(usage.total_token_count, 0);
    }

    // ── Request → IR → WorkOrder ────────────────────────────────────────

    #[test]
    fn request_to_ir_basic() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]));
        let (ir, _, _) = request_to_ir(&req).unwrap();
        assert!(!ir.conversation.messages.is_empty());
        assert_eq!(ir.conversation.messages[0].role, IrRole::User);
    }

    #[test]
    fn request_to_ir_with_system_instruction() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::user(vec![Part::text("Be concise")]))
            .add_content(Content::user(vec![Part::text("Hello")]));
        let (ir, _, _) = request_to_ir(&req).unwrap();
        assert_eq!(ir.conversation.messages[0].role, IrRole::System);
    }

    // ── Receipt → IR → Response ─────────────────────────────────────────

    #[test]
    fn receipt_to_ir_text_message() {
        let events = vec![text_event("Response text")];
        let receipt = make_receipt_with_events(events);
        let ir = receipt_to_ir(&receipt);
        assert_eq!(ir.messages.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::Assistant);
        assert_eq!(ir.messages[0].text_content(), "Response text");
    }

    #[test]
    fn receipt_to_ir_tool_call() {
        let events = vec![tool_call_event("search", "tc_1", json!({"q": "test"}))];
        let receipt = make_receipt_with_events(events);
        let ir = receipt_to_ir(&receipt);
        assert_eq!(ir.messages.len(), 1);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolUse { name, id, input } => {
                assert_eq!(name, "search");
                assert_eq!(id, "tc_1");
                assert_eq!(input, &json!({"q": "test"}));
            }
            _ => panic!("expected ToolUse"),
        }
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn empty_request_to_ir() {
        let req = GenerateContentRequest::new("gemini-2.5-flash");
        let (ir, _, _) = request_to_ir(&req).unwrap();
        assert!(ir.conversation.messages.is_empty());
    }

    #[test]
    fn unicode_content_preserved() {
        let content = Content::user(vec![Part::text("こんにちは 🌍")]);
        let dialect = content_to_dialect(&content);
        let back = content_from_dialect(&dialect);
        match &back.parts[0] {
            Part::Text(t) => assert_eq!(t, "こんにちは 🌍"),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn large_payload_preserved() {
        let large = "x".repeat(100_000);
        let content = Content::user(vec![Part::text(large.clone())]);
        let dialect = content_to_dialect(&content);
        let back = content_from_dialect(&dialect);
        match &back.parts[0] {
            Part::Text(t) => assert_eq!(t.len(), 100_000),
            _ => panic!("expected text"),
        }
    }

    // ── ToolDeclaration roundtrip ───────────────────────────────────────

    #[test]
    fn tool_declaration_to_dialect_preserves_fields() {
        let tool = ToolDeclaration {
            function_declarations: vec![FunctionDeclaration {
                name: "get_weather".into(),
                description: "Gets weather info".into(),
                parameters: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
            }],
        };
        let dialect = tool_decl_to_dialect(&tool);
        assert_eq!(dialect.function_declarations.len(), 1);
        assert_eq!(dialect.function_declarations[0].name, "get_weather");
        assert_eq!(
            dialect.function_declarations[0].description,
            "Gets weather info"
        );
    }

    // ── Stream events ───────────────────────────────────────────────────

    #[test]
    fn receipt_to_stream_events_produces_events() {
        let events = vec![text_event("Hello"), run_completed_event()];
        let receipt = make_receipt_with_events_and_usage(events, sample_usage());
        let stream = receipt_to_stream_events(&receipt);
        assert!(stream.len() >= 1);
        assert!(stream[0].candidates.len() >= 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 4: Codex shim tests
// ═══════════════════════════════════════════════════════════════════════════
mod codex {
    use super::*;
    use abp_codex_sdk::dialect::{
        CodexContentPart, CodexInputItem, CodexResponse, CodexResponseItem,
    };
    use abp_shim_codex::*;

    // ── SDK → IR ────────────────────────────────────────────────────────

    #[test]
    fn request_to_ir_basic() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "Hello")])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn request_to_ir_with_system() {
        let req = CodexRequestBuilder::new()
            .input(vec![
                codex_message("system", "Be concise"),
                codex_message("user", "Hello"),
            ])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    // ── IR roundtrip ────────────────────────────────────────────────────

    #[test]
    fn response_to_ir_and_back() {
        let resp = CodexResponse {
            id: "resp_1".into(),
            model: "codex-mini-latest".into(),
            output: vec![CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Done".into(),
                }],
            }],
            usage: None,
            status: None,
        };
        let conv = response_to_ir(&resp);
        let back = ir_to_response_items(&conv);
        assert_eq!(back.len(), 1);
        match &back[0] {
            CodexResponseItem::Message { content, .. } => match &content[0] {
                CodexContentPart::OutputText { text } => assert_eq!(text, "Done"),
            },
            _ => panic!("expected Message"),
        }
    }

    // ── Usage ────────────────────────────────────────────────────────────

    #[test]
    fn ir_usage_to_codex_usage() {
        let ir = IrUsage::from_io(200, 100);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.total_tokens, 300);
    }

    // ── WorkOrder ───────────────────────────────────────────────────────

    #[test]
    fn work_order_preserves_model() {
        let req = CodexRequestBuilder::new()
            .model("o3-mini")
            .input(vec![codex_message("user", "test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
    }

    #[test]
    fn work_order_preserves_temperature() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "test")])
            .temperature(0.7)
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.vendor.get("temperature"), Some(&json!(0.7)));
    }

    #[test]
    fn work_order_preserves_max_output_tokens() {
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "test")])
            .max_output_tokens(2048)
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("max_output_tokens"),
            Some(&json!(2048))
        );
    }

    // ── Receipt → Response ──────────────────────────────────────────────

    #[test]
    fn receipt_to_response_text() {
        let events = vec![text_event("Result text")];
        let receipt = make_receipt_with_events_and_usage(events, sample_usage());
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert_eq!(resp.model, "codex-mini-latest");
        assert_eq!(resp.output.len(), 1);
        assert_eq!(resp.status.as_deref(), Some("completed"));
        let usage = resp.usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
    }

    #[test]
    fn receipt_to_response_tool_call() {
        let events = vec![tool_call_event("shell", "fc_1", json!({"cmd": "ls"}))];
        let receipt = make_receipt_with_events(events);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        match &resp.output[0] {
            CodexResponseItem::FunctionCall { id, name, .. } => {
                assert_eq!(id, "fc_1");
                assert_eq!(name, "shell");
            }
            _ => panic!("expected FunctionCall"),
        }
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn unicode_content_preserved() {
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "日本語テスト 🚀")])
            .build();
        let conv = request_to_ir(&req);
        assert!(conv.messages[0].text_content().contains("日本語テスト"));
    }

    #[test]
    fn empty_input_request() {
        let req = CodexRequestBuilder::new().input(vec![]).build();
        let conv = request_to_ir(&req);
        assert!(conv.messages.is_empty());
    }

    #[test]
    fn builder_defaults_model_to_codex_mini() {
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "test")])
            .build();
        assert_eq!(req.model, "codex-mini-latest");
    }

    // ── Stream events ───────────────────────────────────────────────────

    #[test]
    fn stream_events_bookends() {
        let events = vec![delta_event("hi")];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        assert!(stream.len() >= 3); // created + delta + completed
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 5: Kimi shim tests
// ═══════════════════════════════════════════════════════════════════════════
mod kimi {
    use super::*;
    use abp_shim_kimi::*;

    // ── SDK → IR ────────────────────────────────────────────────────────

    #[test]
    fn request_to_ir_basic() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Hello")])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn request_to_ir_with_system() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::system("Be concise"), Message::user("Hello")])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    // ── IR → SDK ────────────────────────────────────────────────────────

    #[test]
    fn ir_to_messages_basic() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Hello"),
            IrMessage::text(IrRole::Assistant, "Hi"),
        ]);
        let msgs = ir_to_messages(&conv);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content.as_deref(), Some("Hello"));
        assert_eq!(msgs[1].role, "assistant");
    }

    // ── Roundtrip: SDK → IR → SDK ───────────────────────────────────────

    #[test]
    fn messages_roundtrip() {
        let original = vec![
            Message::system("System prompt"),
            Message::user("User message"),
            Message::assistant("Assistant reply"),
        ];
        let conv = messages_to_ir(&original);
        let back = ir_to_messages(&conv);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content.as_deref(), Some("System prompt"));
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
    }

    // ── Usage ────────────────────────────────────────────────────────────

    #[test]
    fn ir_usage_to_kimi_usage() {
        let ir = IrUsage::from_io(200, 100);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 200);
        assert_eq!(usage.completion_tokens, 100);
        assert_eq!(usage.total_tokens, 300);
    }

    // ── WorkOrder ───────────────────────────────────────────────────────

    #[test]
    fn work_order_preserves_model() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
    }

    #[test]
    fn work_order_preserves_temperature() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .temperature(0.7)
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.vendor.get("temperature"), Some(&json!(0.7)));
    }

    #[test]
    fn work_order_preserves_max_tokens() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .max_tokens(1024)
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.vendor.get("max_tokens"), Some(&json!(1024)));
    }

    // ── Receipt → Response ──────────────────────────────────────────────

    #[test]
    fn receipt_to_response_text() {
        let events = vec![text_event("Hello!")];
        let receipt = make_receipt_with_events_and_usage(events, sample_usage());
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.model, "moonshot-v1-8k");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
    }

    #[test]
    fn receipt_to_response_tool_calls() {
        let events = vec![tool_call_event("search", "call_1", json!({"q": "rust"}))];
        let receipt = make_receipt_with_events(events);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_1");
        assert_eq!(tcs[0].function.name, "search");
    }

    // ── Response → IR ───────────────────────────────────────────────────

    #[test]
    fn response_to_ir_text() {
        use abp_kimi_sdk::dialect::{KimiChoice, KimiResponse, KimiResponseMessage};
        let resp = KimiResponse {
            id: "cmpl-1".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            refs: None,
        };
        let conv = response_to_ir(&resp);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn unicode_content_preserved() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("中文测试 🚀")])
            .build();
        let conv = request_to_ir(&req);
        assert!(conv.messages[0].text_content().contains("中文测试"));
    }

    #[test]
    fn builder_defaults_model() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        assert_eq!(req.model, "moonshot-v1-8k");
    }

    // ── Stream chunks ───────────────────────────────────────────────────

    #[test]
    fn stream_chunks_end_with_stop() {
        let events = vec![delta_event("hi")];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert_eq!(chunks.len(), 2);
        assert_eq!(
            chunks.last().unwrap().choices[0].finish_reason.as_deref(),
            Some("stop")
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 6: Copilot shim tests
// ═══════════════════════════════════════════════════════════════════════════
mod copilot {
    use super::*;
    use abp_copilot_sdk::dialect::{CopilotResponse, CopilotStreamEvent};
    use abp_shim_copilot::*;

    // ── SDK → IR ────────────────────────────────────────────────────────

    #[test]
    fn request_to_ir_basic() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("Hello")])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn request_to_ir_with_system() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::system("Be concise"), Message::user("Hello")])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    // ── IR → SDK ────────────────────────────────────────────────────────

    #[test]
    fn ir_to_messages_basic() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Hello"),
            IrMessage::text(IrRole::Assistant, "Hi"),
        ]);
        let msgs = ir_to_messages(&conv);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "Hello");
        assert_eq!(msgs[1].role, "assistant");
    }

    // ── Roundtrip: SDK → IR → SDK ───────────────────────────────────────

    #[test]
    fn messages_roundtrip() {
        let original = vec![
            Message::system("System prompt"),
            Message::user("User message"),
            Message::assistant("Assistant reply"),
        ];
        let conv = messages_to_ir(&original);
        let back = ir_to_messages(&conv);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content, "System prompt");
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
    }

    // ── Usage ────────────────────────────────────────────────────────────

    #[test]
    fn ir_usage_to_tuple_conversion() {
        let ir = IrUsage::from_io(200, 100);
        let (input, output, total) = ir_usage_to_tuple(&ir);
        assert_eq!(input, 200);
        assert_eq!(output, 100);
        assert_eq!(total, 300);
    }

    // ── WorkOrder ───────────────────────────────────────────────────────

    #[test]
    fn work_order_preserves_model() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    // ── Receipt → Response ──────────────────────────────────────────────

    #[test]
    fn receipt_to_response_text() {
        let events = vec![text_event("Hello!")];
        let receipt = make_receipt_with_events(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Hello!");
        assert!(resp.copilot_errors.is_empty());
    }

    #[test]
    fn receipt_to_response_error() {
        let events = vec![error_event("rate limit exceeded")];
        let receipt = make_receipt_with_events(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.copilot_errors.len(), 1);
        assert!(resp.copilot_errors[0].message.contains("rate limit"));
    }

    #[test]
    fn receipt_to_response_tool_call() {
        let events = vec![tool_call_event(
            "read_file",
            "call_1",
            json!({"path": "a.rs"}),
        )];
        let receipt = make_receipt_with_events(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let fc = resp.function_call.unwrap();
        assert_eq!(fc.name, "read_file");
        assert_eq!(fc.id.as_deref(), Some("call_1"));
    }

    // ── Response → IR ───────────────────────────────────────────────────

    #[test]
    fn response_to_ir_text() {
        let resp = CopilotResponse {
            message: "Hello!".into(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: None,
        };
        let conv = response_to_ir(&resp);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        assert_eq!(conv.messages[0].text_content(), "Hello!");
    }

    #[test]
    fn response_to_ir_empty() {
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

    // ── Stream events ───────────────────────────────────────────────────

    #[test]
    fn stream_events_structure() {
        let events = vec![delta_event("hi")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        // references + delta + done
        assert_eq!(stream.len(), 3);
        assert!(matches!(
            &stream[0],
            CopilotStreamEvent::CopilotReferences { .. }
        ));
        assert!(matches!(&stream[1], CopilotStreamEvent::TextDelta { .. }));
        assert!(matches!(&stream[2], CopilotStreamEvent::Done {}));
    }

    #[test]
    fn stream_error_event() {
        let events = vec![error_event("boom")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(matches!(
            &stream[1],
            CopilotStreamEvent::CopilotErrors { .. }
        ));
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn unicode_content_preserved() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("한국어 🎉")])
            .build();
        let conv = request_to_ir(&req);
        assert!(conv.messages[0].text_content().contains("한국어"));
    }

    #[test]
    fn builder_defaults_model() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 7: Cross-SDK tests (SDK A → IR → SDK B)
// ═══════════════════════════════════════════════════════════════════════════
mod cross_sdk {
    use super::*;

    // ── OpenAI → IR → Kimi ──────────────────────────────────────────────

    #[test]
    fn openai_to_ir_to_kimi() {
        let openai_msgs = vec![
            abp_shim_openai::Message::system("You are helpful"),
            abp_shim_openai::Message::user("Hello"),
            abp_shim_openai::Message::assistant("Hi there"),
        ];
        let conv = abp_shim_openai::messages_to_ir(&openai_msgs);
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&conv);

        assert_eq!(kimi_msgs.len(), 3);
        assert_eq!(kimi_msgs[0].role, "system");
        assert_eq!(kimi_msgs[0].content.as_deref(), Some("You are helpful"));
        assert_eq!(kimi_msgs[1].role, "user");
        assert_eq!(kimi_msgs[1].content.as_deref(), Some("Hello"));
        assert_eq!(kimi_msgs[2].role, "assistant");
        assert_eq!(kimi_msgs[2].content.as_deref(), Some("Hi there"));
    }

    // ── Kimi → IR → OpenAI ──────────────────────────────────────────────

    #[test]
    fn kimi_to_ir_to_openai() {
        let kimi_msgs = vec![
            abp_shim_kimi::Message::system("Be concise"),
            abp_shim_kimi::Message::user("Hello"),
            abp_shim_kimi::Message::assistant("Hi"),
        ];
        let conv = abp_shim_kimi::messages_to_ir(&kimi_msgs);
        let openai_msgs = abp_shim_openai::ir_to_messages(&conv);

        assert_eq!(openai_msgs.len(), 3);
        assert_eq!(openai_msgs[0].role, abp_shim_openai::Role::System);
        assert_eq!(openai_msgs[1].role, abp_shim_openai::Role::User);
        assert_eq!(openai_msgs[2].role, abp_shim_openai::Role::Assistant);
    }

    // ── OpenAI → IR → Copilot ───────────────────────────────────────────

    #[test]
    fn openai_to_ir_to_copilot() {
        let openai_msgs = vec![
            abp_shim_openai::Message::user("What is Rust?"),
            abp_shim_openai::Message::assistant("A systems programming language."),
        ];
        let conv = abp_shim_openai::messages_to_ir(&openai_msgs);
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&conv);

        assert_eq!(copilot_msgs.len(), 2);
        assert_eq!(copilot_msgs[0].role, "user");
        assert_eq!(copilot_msgs[0].content, "What is Rust?");
        assert_eq!(copilot_msgs[1].role, "assistant");
        assert_eq!(copilot_msgs[1].content, "A systems programming language.");
    }

    // ── Copilot → IR → Kimi ────────────────────────────────────────────

    #[test]
    fn copilot_to_ir_to_kimi() {
        let copilot_msgs = vec![
            abp_shim_copilot::Message::system("System prompt"),
            abp_shim_copilot::Message::user("Hello from Copilot"),
        ];
        let conv = abp_shim_copilot::messages_to_ir(&copilot_msgs);
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&conv);

        assert_eq!(kimi_msgs.len(), 2);
        assert_eq!(kimi_msgs[0].role, "system");
        assert_eq!(kimi_msgs[0].content.as_deref(), Some("System prompt"));
        assert_eq!(kimi_msgs[1].role, "user");
        assert_eq!(kimi_msgs[1].content.as_deref(), Some("Hello from Copilot"));
    }

    // ── Kimi → IR → Copilot ────────────────────────────────────────────

    #[test]
    fn kimi_to_ir_to_copilot() {
        let kimi_msgs = vec![
            abp_shim_kimi::Message::user("Kimi says hi"),
            abp_shim_kimi::Message::assistant("Hi from Kimi"),
        ];
        let conv = abp_shim_kimi::messages_to_ir(&kimi_msgs);
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&conv);

        assert_eq!(copilot_msgs.len(), 2);
        assert_eq!(copilot_msgs[0].role, "user");
        assert_eq!(copilot_msgs[0].content, "Kimi says hi");
        assert_eq!(copilot_msgs[1].role, "assistant");
        assert_eq!(copilot_msgs[1].content, "Hi from Kimi");
    }

    // ── Copilot → IR → OpenAI ──────────────────────────────────────────

    #[test]
    fn copilot_to_ir_to_openai() {
        let copilot_msgs = vec![
            abp_shim_copilot::Message::user("Test"),
            abp_shim_copilot::Message::assistant("Response"),
        ];
        let conv = abp_shim_copilot::messages_to_ir(&copilot_msgs);
        let openai_msgs = abp_shim_openai::ir_to_messages(&conv);

        assert_eq!(openai_msgs.len(), 2);
        assert_eq!(openai_msgs[0].role, abp_shim_openai::Role::User);
        assert_eq!(openai_msgs[0].content.as_deref(), Some("Test"));
    }

    // ── Cross-SDK usage IR roundtrip ────────────────────────────────────

    #[test]
    fn usage_openai_ir_kimi() {
        let ir = IrUsage::from_io(100, 50);
        let openai_usage = abp_shim_openai::ir_usage_to_usage(&ir);
        let kimi_usage = abp_shim_kimi::ir_usage_to_usage(&ir);
        // Validate both map from the same IR
        assert_eq!(openai_usage.prompt_tokens, kimi_usage.prompt_tokens);
        assert_eq!(openai_usage.completion_tokens, kimi_usage.completion_tokens);
        assert_eq!(openai_usage.total_tokens, kimi_usage.total_tokens);
    }

    #[test]
    fn usage_codex_ir_gemini() {
        let ir = IrUsage::from_io(200, 100);
        let codex_usage = abp_shim_codex::ir_usage_to_usage(&ir);
        let gemini_usage = abp_shim_gemini::usage_from_ir(&ir);
        assert_eq!(codex_usage.input_tokens, gemini_usage.prompt_token_count);
        assert_eq!(
            codex_usage.output_tokens,
            gemini_usage.candidates_token_count
        );
        assert_eq!(codex_usage.total_tokens, gemini_usage.total_token_count);
    }

    #[test]
    fn usage_copilot_ir_tuple() {
        let ir = IrUsage::from_io(300, 200);
        let (input, output, total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
        assert_eq!(input, 300);
        assert_eq!(output, 200);
        assert_eq!(total, 500);
    }

    // ── Multi-turn cross-SDK ────────────────────────────────────────────

    #[test]
    fn multi_turn_openai_to_kimi_and_back() {
        let openai_msgs = vec![
            abp_shim_openai::Message::system("System"),
            abp_shim_openai::Message::user("Turn 1"),
            abp_shim_openai::Message::assistant("Reply 1"),
            abp_shim_openai::Message::user("Turn 2"),
        ];
        let conv = abp_shim_openai::messages_to_ir(&openai_msgs);
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&conv);
        assert_eq!(kimi_msgs.len(), 4);

        // Round-trip back
        let conv2 = abp_shim_kimi::messages_to_ir(&kimi_msgs);
        let openai_back = abp_shim_openai::ir_to_messages(&conv2);
        assert_eq!(openai_back.len(), 4);
        assert_eq!(openai_back[0].role, abp_shim_openai::Role::System);
        assert_eq!(openai_back[3].role, abp_shim_openai::Role::User);
    }

    // ── Unicode cross-SDK ───────────────────────────────────────────────

    #[test]
    fn unicode_cross_sdk_openai_to_copilot() {
        let msg = abp_shim_openai::Message::user("こんにちは 🌏 مرحبا");
        let conv = abp_shim_openai::messages_to_ir(&[msg]);
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&conv);
        assert_eq!(copilot_msgs[0].content, "こんにちは 🌏 مرحبا");
    }

    #[test]
    fn unicode_cross_sdk_kimi_to_openai() {
        let msg = abp_shim_kimi::Message::user("Привет мир 🌍");
        let conv = abp_shim_kimi::messages_to_ir(&[msg]);
        let openai_msgs = abp_shim_openai::ir_to_messages(&conv);
        assert_eq!(openai_msgs[0].content.as_deref(), Some("Привет мир 🌍"));
    }

    // ── Large payloads cross-SDK ────────────────────────────────────────

    #[test]
    fn large_payload_cross_sdk() {
        let large = "x".repeat(100_000);
        let msg = abp_shim_openai::Message::user(large.clone());
        let conv = abp_shim_openai::messages_to_ir(&[msg]);
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&conv);
        assert_eq!(copilot_msgs[0].content.len(), 100_000);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 8: IR edge cases and invariants
// ═══════════════════════════════════════════════════════════════════════════
mod ir_invariants {
    use super::*;

    #[test]
    fn ir_usage_total_is_sum() {
        let ir = IrUsage::from_io(123, 456);
        assert_eq!(ir.total_tokens, 123 + 456);
    }

    #[test]
    fn ir_usage_with_cache() {
        let ir = IrUsage::with_cache(100, 50, 10, 5);
        assert_eq!(ir.input_tokens, 100);
        assert_eq!(ir.output_tokens, 50);
        assert_eq!(ir.total_tokens, 150);
        assert_eq!(ir.cache_read_tokens, 10);
        assert_eq!(ir.cache_write_tokens, 5);
    }

    #[test]
    fn ir_conversation_empty() {
        let conv = IrConversation::new();
        assert!(conv.messages.is_empty());
        assert!(conv.system_message().is_none());
        assert!(conv.last_assistant().is_none());
    }

    #[test]
    fn ir_conversation_system_accessor() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "System prompt"),
            IrMessage::text(IrRole::User, "Hi"),
        ]);
        let sys = conv.system_message().unwrap();
        assert_eq!(sys.text_content(), "System prompt");
    }

    #[test]
    fn ir_conversation_last_assistant() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Hi"),
            IrMessage::text(IrRole::Assistant, "Hello"),
            IrMessage::text(IrRole::User, "How?"),
            IrMessage::text(IrRole::Assistant, "Fine"),
        ]);
        let last = conv.last_assistant().unwrap();
        assert_eq!(last.text_content(), "Fine");
    }

    #[test]
    fn ir_message_text_only() {
        let msg = IrMessage::text(IrRole::User, "Hello");
        assert!(msg.is_text_only());
    }

    #[test]
    fn ir_message_not_text_only_with_tool_use() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tc_1".into(),
                name: "search".into(),
                input: json!({}),
            }],
        );
        assert!(!msg.is_text_only());
    }

    #[test]
    fn ir_message_tool_use_blocks() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me search".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tc_1".into(),
                    name: "search".into(),
                    input: json!({}),
                },
                IrContentBlock::ToolUse {
                    id: "tc_2".into(),
                    name: "read".into(),
                    input: json!({}),
                },
            ],
        );
        assert_eq!(msg.tool_use_blocks().len(), 2);
    }

    #[test]
    fn ir_content_block_serde_roundtrip_text() {
        let block = IrContentBlock::Text {
            text: "Hello".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn ir_content_block_serde_roundtrip_image() {
        let block = IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "base64data".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn ir_content_block_serde_roundtrip_tool_use() {
        let block = IrContentBlock::ToolUse {
            id: "tc_1".into(),
            name: "search".into(),
            input: json!({"query": "test"}),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn ir_content_block_serde_roundtrip_tool_result() {
        let block = IrContentBlock::ToolResult {
            tool_use_id: "tc_1".into(),
            content: vec![IrContentBlock::Text {
                text: "result".into(),
            }],
            is_error: false,
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn ir_content_block_serde_roundtrip_thinking() {
        let block = IrContentBlock::Thinking {
            text: "Let me think...".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn ir_role_serde_roundtrip() {
        for role in [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
        ] {
            let json = serde_json::to_string(&role).unwrap();
            let back: IrRole = serde_json::from_str(&json).unwrap();
            assert_eq!(role, back);
        }
    }

    #[test]
    fn ir_conversation_serde_roundtrip() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "System"),
            IrMessage::text(IrRole::User, "Hello"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "Let me search".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "tc_1".into(),
                        name: "search".into(),
                        input: json!({"q": "test"}),
                    },
                ],
            ),
        ]);
        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        assert_eq!(conv, back);
    }

    #[test]
    fn ir_usage_serde_roundtrip() {
        let usage = IrUsage::with_cache(100, 50, 10, 5);
        let json = serde_json::to_string(&usage).unwrap();
        let back: IrUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, back);
    }

    #[test]
    fn ir_tool_definition_serde_roundtrip() {
        use abp_core::ir::IrToolDefinition;
        let tool = IrToolDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 9: Error handling tests
// ═══════════════════════════════════════════════════════════════════════════
mod error_handling {
    use super::*;

    #[test]
    fn openai_error_event_in_receipt() {
        let events = vec![error_event("model overloaded")];
        let receipt = make_receipt_with_events(events);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = abp_shim_openai::convert::from_receipt(&receipt, &wo);
        assert!(
            resp.choices[0]
                .message
                .content
                .as_ref()
                .unwrap()
                .contains("model overloaded")
        );
    }

    #[test]
    fn claude_error_event_skipped_in_receipt() {
        let events = vec![error_event("timeout")];
        let receipt = make_receipt_with_events(events);
        let wo = WorkOrderBuilder::new("test")
            .model("claude-sonnet-4-20250514")
            .build();
        let resp = abp_shim_claude::convert::from_receipt(&receipt, &wo);
        // Claude shim ignores error events in receipt trace
        assert!(resp.content.is_empty() || resp.stop_reason.is_some());
    }

    #[test]
    fn kimi_error_in_receipt() {
        let events = vec![error_event("connection failed")];
        let receipt = make_receipt_with_events(events);
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(
            resp.choices[0]
                .message
                .content
                .as_ref()
                .unwrap()
                .contains("connection failed")
        );
    }

    #[test]
    fn copilot_error_in_receipt() {
        let events = vec![error_event("internal error")];
        let receipt = make_receipt_with_events(events);
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.copilot_errors.len(), 1);
        assert!(resp.copilot_errors[0].message.contains("internal error"));
    }

    #[test]
    fn codex_error_in_receipt() {
        use abp_codex_sdk::dialect::{CodexContentPart, CodexResponseItem};
        let events = vec![error_event("quota exceeded")];
        let receipt = make_receipt_with_events(events);
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        match &resp.output[0] {
            CodexResponseItem::Message { content, .. } => match &content[0] {
                CodexContentPart::OutputText { text } => {
                    assert!(text.contains("quota exceeded"));
                }
            },
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn openai_stream_error_event() {
        let event = error_event("rate limited");
        let chunk = abp_shim_openai::convert::from_agent_event(&event, "gpt-4o", "chunk_1");
        let chunk = chunk.unwrap();
        assert!(
            chunk.choices[0]
                .delta
                .content
                .as_ref()
                .unwrap()
                .contains("rate limited")
        );
        assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn copilot_stream_error_event() {
        use abp_copilot_sdk::dialect::CopilotStreamEvent;
        let events = vec![error_event("boom")];
        let stream = abp_shim_copilot::events_to_stream_events(&events, "gpt-4o");
        let found = stream
            .iter()
            .any(|e| matches!(e, CopilotStreamEvent::CopilotErrors { .. }));
        assert!(found);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 10: Field preservation stress tests
// ═══════════════════════════════════════════════════════════════════════════
mod field_preservation {
    use super::*;

    #[test]
    fn openai_all_optional_fields_work_order() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .temperature(0.5)
            .max_tokens(4096)
            .stop(vec!["END".into()])
            .stream(true)
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
        assert_eq!(wo.config.vendor.get("temperature"), Some(&json!(0.5)));
        assert_eq!(wo.config.vendor.get("max_tokens"), Some(&json!(4096)));
    }

    #[test]
    fn gemini_all_generation_config_fields() {
        let cfg = abp_shim_gemini::GenerationConfig {
            max_output_tokens: Some(2048),
            temperature: Some(0.8),
            top_p: Some(0.95),
            top_k: Some(50),
            candidate_count: None,
            stop_sequences: Some(vec!["STOP".into(), "END".into()]),
            response_mime_type: Some("text/plain".into()),
            response_schema: Some(json!({"type": "string"})),
        };
        let dialect = abp_shim_gemini::gen_config_to_dialect(&cfg);
        let back = abp_shim_gemini::gen_config_from_dialect(&dialect);
        assert_eq!(back.max_output_tokens, Some(2048));
        assert_eq!(back.temperature, Some(0.8));
        assert_eq!(back.top_p, Some(0.95));
        assert_eq!(back.top_k, Some(50));
        assert_eq!(back.stop_sequences, Some(vec!["STOP".into(), "END".into()]));
        assert_eq!(back.response_mime_type, Some("text/plain".into()));
        assert_eq!(back.response_schema, Some(json!({"type": "string"})));
    }

    #[test]
    fn claude_tools_in_work_order() {
        use abp_shim_claude::types::*;
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("test".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: Some(vec![ClaudeTool {
                name: "calculator".into(),
                description: Some("Does math".into()),
                input_schema: json!({"type": "object"}),
            }]),
            tool_choice: None,
            thinking: None,
        };
        let wo = abp_shim_claude::convert::to_work_order(&req);
        assert!(wo.config.vendor.contains_key("tools"));
    }

    #[test]
    fn codex_temperature_and_max_output_in_work_order() {
        let req = abp_shim_codex::CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![abp_shim_codex::codex_message("user", "test")])
            .temperature(0.9)
            .max_output_tokens(4096)
            .build();
        let wo = abp_shim_codex::request_to_work_order(&req);
        assert_eq!(wo.config.vendor.get("temperature"), Some(&json!(0.9)));
        assert_eq!(
            wo.config.vendor.get("max_output_tokens"),
            Some(&json!(4096))
        );
    }

    #[test]
    fn receipt_with_deltas_accumulates_in_openai() {
        let events = vec![delta_event("Hello"), delta_event(" "), delta_event("World")];
        let receipt = make_receipt_with_events(events);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = abp_shim_openai::convert::from_receipt(&receipt, &wo);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello World")
        );
    }

    #[test]
    fn receipt_with_deltas_accumulates_in_kimi() {
        let events = vec![delta_event("Hello"), delta_event(" "), delta_event("World")];
        let receipt = make_receipt_with_events(events);
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello World")
        );
    }

    #[test]
    fn copilot_receipt_with_deltas_accumulates() {
        let events = vec![delta_event("Hello"), delta_event(" "), delta_event("World")];
        let receipt = make_receipt_with_events(events);
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Hello World");
    }
}
