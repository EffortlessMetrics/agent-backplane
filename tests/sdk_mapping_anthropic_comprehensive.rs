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
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive tests for Anthropic/Claude SDK dialect mapping.
//!
//! Validates the full mapping pipeline between Anthropic Messages API semantics
//! and ABP canonical types: WorkOrder, AgentEvent, Receipt, and capabilities.

use abp_claude_sdk::dialect::{
    self, CanonicalToolDef, ClaudeApiError, ClaudeConfig, ClaudeContentBlock, ClaudeImageSource,
    ClaudeMessage, ClaudeMessageDelta, ClaudeRequest, ClaudeResponse, ClaudeStopReason,
    ClaudeStreamDelta, ClaudeStreamEvent, ClaudeSystemBlock, ClaudeToolDef, ClaudeUsage,
    ThinkingConfig,
};
use abp_claude_sdk::lowering;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, ReceiptBuilder, SupportLevel, WorkOrderBuilder,
    CONTRACT_VERSION,
};
use abp_shim_claude::{
    content_block_from_ir, content_block_to_ir, request_to_claude, request_to_work_order,
    response_from_claude, response_from_events, stream_event_from_claude, AnthropicClient,
    ApiError, ContentBlock, EventStream, ImageSource, Message, MessageRequest, MessageResponse,
    Role, ShimError, StreamDelta, StreamEvent, Usage,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn simple_request(text: &str) -> MessageRequest {
    MessageRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        max_tokens: 4096,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    }
}

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

#[allow(dead_code)]
fn make_agent_event_with_ext(
    kind: AgentEventKind,
    ext: BTreeMap<String, serde_json::Value>,
) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: Some(ext),
    }
}

fn claude_msg(role: &str, content: &str) -> ClaudeMessage {
    ClaudeMessage {
        role: role.into(),
        content: content.into(),
    }
}

fn claude_blocks_msg(role: &str, blocks: &[ClaudeContentBlock]) -> ClaudeMessage {
    ClaudeMessage {
        role: role.into(),
        content: serde_json::to_string(blocks).unwrap(),
    }
}

fn make_claude_response(content: Vec<ClaudeContentBlock>) -> ClaudeResponse {
    ClaudeResponse {
        id: "msg_test".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content,
        stop_reason: Some("end_turn".into()),
        usage: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Anthropic messages API format → WorkOrder
// ═══════════════════════════════════════════════════════════════════════════

mod messages_api_to_work_order {
    use super::*;

    #[test]
    fn simple_text_request_maps_to_work_order() {
        let req = simple_request("Write a function");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Write a function");
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn model_preserved_in_work_order() {
        let mut req = simple_request("task");
        req.model = "claude-opus-4-20250514".into();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-opus-4-20250514"));
    }

    #[test]
    fn temperature_propagated_to_vendor_config() {
        let mut req = simple_request("task");
        req.temperature = Some(0.5);
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.vendor.get("temperature").unwrap(), &json!(0.5));
    }

    #[test]
    fn max_tokens_propagated_to_vendor_config() {
        let mut req = simple_request("task");
        req.temperature = Some(0.5);
        req.max_tokens = 8192;
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.vendor.get("max_tokens").unwrap(), &json!(8192));
    }

    #[test]
    fn stop_sequences_propagated_to_vendor_config() {
        let mut req = simple_request("task");
        req.temperature = Some(0.5);
        req.stop_sequences = Some(vec!["STOP".into(), "END".into()]);
        let wo = request_to_work_order(&req);
        let stop = wo.config.vendor.get("stop_sequences").unwrap();
        assert_eq!(stop, &json!(["STOP", "END"]));
    }

    #[test]
    fn multi_message_request_extracts_last_user_text() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            messages: vec![
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "Hello".into(),
                    }],
                },
                Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text {
                        text: "Hi there".into(),
                    }],
                },
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "Write tests".into(),
                    }],
                },
            ],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Write tests");
    }

    #[test]
    fn work_order_has_valid_uuid() {
        let req = simple_request("test");
        let wo = request_to_work_order(&req);
        assert!(!wo.id.is_nil());
    }

    #[test]
    fn work_order_contract_version_exists() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn request_to_claude_simple() {
        let req = simple_request("Hello world");
        let claude_req = request_to_claude(&req);
        assert_eq!(claude_req.model, "claude-sonnet-4-20250514");
        assert_eq!(claude_req.max_tokens, 4096);
        assert_eq!(claude_req.messages.len(), 1);
        assert_eq!(claude_req.messages[0].role, "user");
    }

    #[test]
    fn request_to_claude_preserves_all_messages() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            messages: vec![
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "First".into(),
                    }],
                },
                Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text {
                        text: "Response".into(),
                    }],
                },
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "Follow-up".into(),
                    }],
                },
            ],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let claude_req = request_to_claude(&req);
        assert_eq!(claude_req.messages.len(), 3);
        assert_eq!(claude_req.messages[0].role, "user");
        assert_eq!(claude_req.messages[1].role, "assistant");
        assert_eq!(claude_req.messages[2].role, "user");
    }

    #[test]
    fn map_work_order_uses_task_as_user_message() {
        let wo = WorkOrderBuilder::new("Summarize this code").build();
        let cfg = ClaudeConfig::default();
        let req = dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert!(req.messages[0].content.contains("Summarize this code"));
    }

    #[test]
    fn map_work_order_model_from_work_order() {
        let wo = WorkOrderBuilder::new("task")
            .model("claude-opus-4-20250514")
            .build();
        let cfg = ClaudeConfig::default();
        let req = dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.model, "claude-opus-4-20250514");
    }

    #[test]
    fn map_work_order_defaults_to_config_model() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = ClaudeConfig {
            model: "claude-haiku-3-5-20241022".into(),
            ..ClaudeConfig::default()
        };
        let req = dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.model, "claude-haiku-3-5-20241022");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Anthropic tool_use content blocks → ABP tool_use events
// ═══════════════════════════════════════════════════════════════════════════

mod tool_use_mapping {
    use super::*;

    #[test]
    fn tool_use_block_maps_to_tool_call_event() {
        let resp = make_claude_response(vec![ClaudeContentBlock::ToolUse {
            id: "toolu_01A".into(),
            name: "read_file".into(),
            input: json!({"path": "src/main.rs"}),
        }]);
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("toolu_01A"));
                assert_eq!(input["path"], "src/main.rs");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn multiple_tool_use_blocks_map_to_multiple_events() {
        let resp = make_claude_response(vec![
            ClaudeContentBlock::ToolUse {
                id: "toolu_01".into(),
                name: "read_file".into(),
                input: json!({"path": "a.rs"}),
            },
            ClaudeContentBlock::ToolUse {
                id: "toolu_02".into(),
                name: "write_file".into(),
                input: json!({"path": "b.rs", "content": "fn main(){}"}),
            },
        ]);
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
        assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
    }

    #[test]
    fn tool_use_with_empty_input() {
        let resp = make_claude_response(vec![ClaudeContentBlock::ToolUse {
            id: "toolu_empty".into(),
            name: "get_time".into(),
            input: json!({}),
        }]);
        let events = dialect::map_response(&resp);
        match &events[0].kind {
            AgentEventKind::ToolCall { input, .. } => assert_eq!(input, &json!({})),
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_use_with_nested_input() {
        let resp = make_claude_response(vec![ClaudeContentBlock::ToolUse {
            id: "toolu_nested".into(),
            name: "execute".into(),
            input: json!({
                "command": "ls -la",
                "env": {"PATH": "/usr/bin"},
                "options": [1, 2, 3]
            }),
        }]);
        let events = dialect::map_response(&resp);
        match &events[0].kind {
            AgentEventKind::ToolCall { input, .. } => {
                assert_eq!(input["command"], "ls -la");
                assert_eq!(input["env"]["PATH"], "/usr/bin");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_use_block_roundtrip_via_shim() {
        let block = ContentBlock::ToolUse {
            id: "toolu_round".into(),
            name: "bash".into(),
            input: json!({"cmd": "echo hello"}),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn tool_def_canonical_to_claude_and_back() {
        let canonical = CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        };
        let claude = dialect::tool_def_to_claude(&canonical);
        assert_eq!(claude.name, "read_file");
        assert_eq!(claude.description, "Read a file from disk");
        assert_eq!(claude.input_schema, canonical.parameters_schema);

        let back = dialect::tool_def_from_claude(&claude);
        assert_eq!(back, canonical);
    }

    #[test]
    fn tool_def_serde_roundtrip() {
        let def = ClaudeToolDef {
            name: "grep".into(),
            description: "Search files".into(),
            input_schema: json!({"type": "object", "properties": {"pattern": {"type": "string"}}}),
        };
        let json_str = serde_json::to_string(&def).unwrap();
        let back: ClaudeToolDef = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back, def);
    }

    #[test]
    fn text_and_tool_use_mixed_response() {
        let resp = make_claude_response(vec![
            ClaudeContentBlock::Text {
                text: "Let me read that file.".into(),
            },
            ClaudeContentBlock::ToolUse {
                id: "toolu_mix".into(),
                name: "read_file".into(),
                input: json!({"path": "main.rs"}),
            },
        ]);
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantMessage { .. }
        ));
        assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
    }

    #[test]
    fn tool_use_parent_id_is_none() {
        let resp = make_claude_response(vec![ClaudeContentBlock::ToolUse {
            id: "toolu_p".into(),
            name: "tool".into(),
            input: json!({}),
        }]);
        let events = dialect::map_response(&resp);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                parent_tool_use_id, ..
            } => assert!(parent_tool_use_id.is_none()),
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Anthropic streaming (SSE with content_block_delta) mapping
// ═══════════════════════════════════════════════════════════════════════════

mod streaming_mapping {
    use super::*;

    #[test]
    fn message_start_maps_to_run_started() {
        let event = ClaudeStreamEvent::MessageStart {
            message: make_claude_response(vec![]),
        };
        let events = dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
    }

    #[test]
    fn message_stop_maps_to_run_completed() {
        let event = ClaudeStreamEvent::MessageStop {};
        let events = dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::RunCompleted { .. }
        ));
    }

    #[test]
    fn text_delta_maps_to_assistant_delta() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "Hello ".into(),
            },
        };
        let events = dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello "),
            other => panic!("expected AssistantDelta, got {other:?}"),
        }
    }

    #[test]
    fn thinking_delta_has_ext_marker() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::ThinkingDelta {
                thinking: "reasoning...".into(),
            },
        };
        let events = dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        let ext = events[0].ext.as_ref().unwrap();
        assert_eq!(ext.get("thinking"), Some(&json!(true)));
    }

    #[test]
    fn input_json_delta_produces_no_events() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::InputJsonDelta {
                partial_json: r#"{"pa"#.into(),
            },
        };
        let events = dialect::map_stream_event(&event);
        assert!(events.is_empty());
    }

    #[test]
    fn signature_delta_produces_no_events() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::SignatureDelta {
                signature: "sig_partial".into(),
            },
        };
        let events = dialect::map_stream_event(&event);
        assert!(events.is_empty());
    }

    #[test]
    fn content_block_start_tool_use_maps_to_tool_call() {
        let event = ClaudeStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ClaudeContentBlock::ToolUse {
                id: "toolu_stream".into(),
                name: "bash".into(),
                input: json!({}),
            },
        };
        let events = dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "bash");
                assert_eq!(tool_use_id.as_deref(), Some("toolu_stream"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn content_block_start_text_produces_no_events() {
        let event = ClaudeStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ClaudeContentBlock::Text {
                text: String::new(),
            },
        };
        let events = dialect::map_stream_event(&event);
        assert!(events.is_empty());
    }

    #[test]
    fn content_block_stop_produces_no_events() {
        let event = ClaudeStreamEvent::ContentBlockStop { index: 0 };
        let events = dialect::map_stream_event(&event);
        assert!(events.is_empty());
    }

    #[test]
    fn ping_produces_no_events() {
        let event = ClaudeStreamEvent::Ping {};
        let events = dialect::map_stream_event(&event);
        assert!(events.is_empty());
    }

    #[test]
    fn message_delta_produces_no_events() {
        let event = ClaudeStreamEvent::MessageDelta {
            delta: ClaudeMessageDelta {
                stop_reason: Some("end_turn".into()),
                stop_sequence: None,
            },
            usage: None,
        };
        let events = dialect::map_stream_event(&event);
        assert!(events.is_empty());
    }

    #[test]
    fn error_stream_event_maps_to_error() {
        let event = ClaudeStreamEvent::Error {
            error: ClaudeApiError {
                error_type: "overloaded_error".into(),
                message: "Server overloaded".into(),
            },
        };
        let events = dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::Error { message, .. } => {
                assert!(message.contains("overloaded_error"));
                assert!(message.contains("Server overloaded"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn stream_event_serde_roundtrip_all_variants() {
        let variants: Vec<ClaudeStreamEvent> = vec![
            ClaudeStreamEvent::MessageStart {
                message: make_claude_response(vec![]),
            },
            ClaudeStreamEvent::ContentBlockStart {
                index: 0,
                content_block: ClaudeContentBlock::Text {
                    text: String::new(),
                },
            },
            ClaudeStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ClaudeStreamDelta::TextDelta { text: "hi".into() },
            },
            ClaudeStreamEvent::ContentBlockStop { index: 0 },
            ClaudeStreamEvent::MessageDelta {
                delta: ClaudeMessageDelta {
                    stop_reason: Some("end_turn".into()),
                    stop_sequence: None,
                },
                usage: None,
            },
            ClaudeStreamEvent::MessageStop {},
            ClaudeStreamEvent::Ping {},
        ];
        for event in &variants {
            let json = serde_json::to_string(event).unwrap();
            let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, event);
        }
    }

    #[test]
    fn shim_stream_event_from_claude_text_delta() {
        let claude_event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "word".into(),
            },
        };
        let shim_event = stream_event_from_claude(&claude_event);
        match shim_event {
            StreamEvent::ContentBlockDelta {
                delta: StreamDelta::TextDelta { text },
                ..
            } => assert_eq!(text, "word"),
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn streaming_produces_full_event_sequence() {
        let client = AnthropicClient::new();
        let stream = client.create_stream(simple_request("Hello")).await.unwrap();
        let events = stream.collect_all().await;
        assert!(events.len() >= 5);
        assert!(matches!(&events[0], StreamEvent::MessageStart { .. }));
        assert!(matches!(
            events.last().unwrap(),
            StreamEvent::MessageStop {}
        ));
    }

    #[tokio::test]
    async fn event_stream_from_vec() {
        let stream = EventStream::from_vec(vec![StreamEvent::Ping {}, StreamEvent::MessageStop {}]);
        let events = stream.collect_all().await;
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn event_stream_empty() {
        let stream = EventStream::from_vec(vec![]);
        let events = stream.collect_all().await;
        assert!(events.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Anthropic model names capability mapping
// ═══════════════════════════════════════════════════════════════════════════

mod model_capability_mapping {
    use super::*;

    #[test]
    fn sonnet_is_known_model() {
        assert!(dialect::is_known_model("claude-sonnet-4-20250514"));
    }

    #[test]
    fn opus_is_known_model() {
        assert!(dialect::is_known_model("claude-opus-4-20250514"));
    }

    #[test]
    fn haiku_is_known_model() {
        assert!(dialect::is_known_model("claude-haiku-3-5-20241022"));
    }

    #[test]
    fn sonnet_latest_is_known() {
        assert!(dialect::is_known_model("claude-sonnet-4-latest"));
    }

    #[test]
    fn opus_latest_is_known() {
        assert!(dialect::is_known_model("claude-opus-4-latest"));
    }

    #[test]
    fn haiku_latest_is_known() {
        assert!(dialect::is_known_model("claude-3-5-haiku-latest"));
    }

    #[test]
    fn unknown_model_is_not_known() {
        assert!(!dialect::is_known_model("gpt-4"));
        assert!(!dialect::is_known_model("gemini-pro"));
        assert!(!dialect::is_known_model("llama-3"));
    }

    #[test]
    fn empty_model_is_not_known() {
        assert!(!dialect::is_known_model(""));
    }

    #[test]
    fn default_model_is_known() {
        assert!(dialect::is_known_model(dialect::DEFAULT_MODEL));
    }

    #[test]
    fn canonical_model_adds_anthropic_prefix() {
        let canonical = dialect::to_canonical_model("claude-sonnet-4-20250514");
        assert_eq!(canonical, "anthropic/claude-sonnet-4-20250514");
    }

    #[test]
    fn from_canonical_strips_prefix() {
        let vendor = dialect::from_canonical_model("anthropic/claude-opus-4-20250514");
        assert_eq!(vendor, "claude-opus-4-20250514");
    }

    #[test]
    fn from_canonical_no_prefix_passthrough() {
        let vendor = dialect::from_canonical_model("claude-sonnet-4-20250514");
        assert_eq!(vendor, "claude-sonnet-4-20250514");
    }

    #[test]
    fn canonical_model_roundtrip() {
        let models = [
            "claude-sonnet-4-20250514",
            "claude-opus-4-20250514",
            "claude-haiku-3-5-20241022",
        ];
        for model in &models {
            let canonical = dialect::to_canonical_model(model);
            let back = dialect::from_canonical_model(&canonical);
            assert_eq!(&back, model);
        }
    }

    #[test]
    fn capability_manifest_includes_streaming() {
        let manifest = dialect::capability_manifest();
        assert!(matches!(
            manifest.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn capability_manifest_includes_tool_capabilities() {
        let manifest = dialect::capability_manifest();
        assert!(matches!(
            manifest.get(&Capability::ToolRead),
            Some(SupportLevel::Native)
        ));
        assert!(matches!(
            manifest.get(&Capability::ToolWrite),
            Some(SupportLevel::Native)
        ));
        assert!(matches!(
            manifest.get(&Capability::ToolEdit),
            Some(SupportLevel::Native)
        ));
        assert!(matches!(
            manifest.get(&Capability::ToolBash),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn capability_manifest_includes_hooks() {
        let manifest = dialect::capability_manifest();
        assert!(matches!(
            manifest.get(&Capability::HooksPreToolUse),
            Some(SupportLevel::Native)
        ));
        assert!(matches!(
            manifest.get(&Capability::HooksPostToolUse),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn capability_manifest_mcp_server_unsupported() {
        let manifest = dialect::capability_manifest();
        assert!(matches!(
            manifest.get(&Capability::McpServer),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn capability_manifest_checkpointing_emulated() {
        let manifest = dialect::capability_manifest();
        assert!(matches!(
            manifest.get(&Capability::Checkpointing),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn capability_manifest_mcp_client_native() {
        let manifest = dialect::capability_manifest();
        assert!(matches!(
            manifest.get(&Capability::McpClient),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn capability_manifest_is_deterministic() {
        let m1 = dialect::capability_manifest();
        let m2 = dialect::capability_manifest();
        let j1 = serde_json::to_string(&m1).unwrap();
        let j2 = serde_json::to_string(&m2).unwrap();
        assert_eq!(j1, j2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Anthropic system prompt handling
// ═══════════════════════════════════════════════════════════════════════════

mod system_prompt_handling {
    use super::*;

    #[test]
    fn system_prompt_mapped_to_claude_request() {
        let mut req = simple_request("Hi");
        req.system = Some("You are a helpful coding assistant.".into());
        let claude_req = request_to_claude(&req);
        assert_eq!(
            claude_req.system.as_deref(),
            Some("You are a helpful coding assistant.")
        );
    }

    #[test]
    fn no_system_prompt_maps_to_none() {
        let req = simple_request("Hi");
        let claude_req = request_to_claude(&req);
        assert!(claude_req.system.is_none());
    }

    #[test]
    fn system_prompt_from_config() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = ClaudeConfig {
            system_prompt: Some("Be concise and direct.".into()),
            ..ClaudeConfig::default()
        };
        let req = dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.system, Some("Be concise and direct.".into()));
    }

    #[test]
    fn system_prompt_none_when_not_configured() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = ClaudeConfig::default();
        let req = dialect::map_work_order(&wo, &cfg);
        assert!(req.system.is_none());
    }

    #[test]
    fn system_prompt_separate_from_messages() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".into(),
                }],
            }],
            system: Some("System instructions".into()),
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let claude_req = request_to_claude(&req);
        assert_eq!(claude_req.system.as_deref(), Some("System instructions"));
        assert_eq!(claude_req.messages.len(), 1);
        assert_eq!(claude_req.messages[0].role, "user");
    }

    #[test]
    fn system_prompt_ir_extraction() {
        let msgs = vec![claude_msg("user", "hi")];
        let conv = lowering::to_ir(&msgs, Some("Be helpful"));
        let extracted = lowering::extract_system_prompt(&conv);
        assert_eq!(extracted.as_deref(), Some("Be helpful"));
    }

    #[test]
    fn system_prompt_ir_none_when_absent() {
        let msgs = vec![claude_msg("user", "hi")];
        let conv = lowering::to_ir(&msgs, None);
        let extracted = lowering::extract_system_prompt(&conv);
        assert!(extracted.is_none());
    }

    #[test]
    fn ir_lift_skips_system_messages() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "instructions"),
            IrMessage::text(IrRole::User, "hello"),
        ]);
        let back = lowering::from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
    }

    #[test]
    fn claude_system_block_serde() {
        let block = ClaudeSystemBlock::Text {
            text: "System instructions".into(),
            cache_control: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: ClaudeSystemBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(back, block);
    }

    #[test]
    fn claude_system_block_with_cache_control() {
        let block = ClaudeSystemBlock::Text {
            text: "Cached instructions".into(),
            cache_control: Some(abp_claude_sdk::dialect::ClaudeCacheControl::ephemeral()),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("cache_control"));
        let back: ClaudeSystemBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(back, block);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Anthropic stop_reason mapping
// ═══════════════════════════════════════════════════════════════════════════

mod stop_reason_mapping {
    use super::*;

    #[test]
    fn end_turn_parse_roundtrip() {
        let parsed = dialect::parse_stop_reason("end_turn").unwrap();
        assert_eq!(parsed, ClaudeStopReason::EndTurn);
        assert_eq!(dialect::map_stop_reason(parsed), "end_turn");
    }

    #[test]
    fn tool_use_parse_roundtrip() {
        let parsed = dialect::parse_stop_reason("tool_use").unwrap();
        assert_eq!(parsed, ClaudeStopReason::ToolUse);
        assert_eq!(dialect::map_stop_reason(parsed), "tool_use");
    }

    #[test]
    fn max_tokens_parse_roundtrip() {
        let parsed = dialect::parse_stop_reason("max_tokens").unwrap();
        assert_eq!(parsed, ClaudeStopReason::MaxTokens);
        assert_eq!(dialect::map_stop_reason(parsed), "max_tokens");
    }

    #[test]
    fn stop_sequence_parse_roundtrip() {
        let parsed = dialect::parse_stop_reason("stop_sequence").unwrap();
        assert_eq!(parsed, ClaudeStopReason::StopSequence);
        assert_eq!(dialect::map_stop_reason(parsed), "stop_sequence");
    }

    #[test]
    fn unknown_stop_reason_returns_none() {
        assert!(dialect::parse_stop_reason("unknown").is_none());
        assert!(dialect::parse_stop_reason("cancelled").is_none());
        assert!(dialect::parse_stop_reason("").is_none());
    }

    #[test]
    fn stop_reason_serde_roundtrip_all() {
        let reasons = [
            ClaudeStopReason::EndTurn,
            ClaudeStopReason::ToolUse,
            ClaudeStopReason::MaxTokens,
            ClaudeStopReason::StopSequence,
        ];
        for reason in &reasons {
            let json = serde_json::to_string(reason).unwrap();
            let back: ClaudeStopReason = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, reason);
        }
    }

    #[test]
    fn stop_reason_json_format() {
        let pairs = [
            (ClaudeStopReason::EndTurn, "\"end_turn\""),
            (ClaudeStopReason::ToolUse, "\"tool_use\""),
            (ClaudeStopReason::MaxTokens, "\"max_tokens\""),
            (ClaudeStopReason::StopSequence, "\"stop_sequence\""),
        ];
        for (reason, expected) in &pairs {
            let json = serde_json::to_string(reason).unwrap();
            assert_eq!(&json, expected);
        }
    }

    #[test]
    fn response_from_events_assistant_text_implies_end_turn() {
        let events = vec![make_agent_event(AgentEventKind::AssistantMessage {
            text: "Done.".into(),
        })];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn response_from_events_tool_call_implies_tool_use() {
        let events = vec![make_agent_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("toolu_01".into()),
            parent_tool_use_id: None,
            input: json!({"path": "a.rs"}),
        })];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn response_from_events_tool_then_completed_is_tool_use() {
        let events = vec![
            make_agent_event(AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("toolu_02".into()),
                parent_tool_use_id: None,
                input: json!({}),
            }),
            make_agent_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ];
        let resp = response_from_events(&events, "test", None);
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn response_from_events_empty_has_no_stop_reason() {
        let resp = response_from_events(&[], "test", None);
        assert!(resp.stop_reason.is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Anthropic multi-turn with tool_result content blocks
// ═══════════════════════════════════════════════════════════════════════════

mod multi_turn_tool_result {
    use super::*;

    #[test]
    fn tool_result_block_maps_to_tool_result_event() {
        let resp = make_claude_response(vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "toolu_01".into(),
            content: Some("file contents here".into()),
            is_error: None,
        }]);
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolResult {
                tool_use_id,
                output,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id.as_deref(), Some("toolu_01"));
                assert_eq!(output, &json!("file contents here"));
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_with_error() {
        let resp = make_claude_response(vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "toolu_err".into(),
            content: Some("Permission denied".into()),
            is_error: Some(true),
        }]);
        let events = dialect::map_response(&resp);
        match &events[0].kind {
            AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_no_content() {
        let resp = make_claude_response(vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "toolu_nil".into(),
            content: None,
            is_error: None,
        }]);
        let events = dialect::map_response(&resp);
        match &events[0].kind {
            AgentEventKind::ToolResult {
                output, is_error, ..
            } => {
                assert_eq!(output, &json!(""));
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_block_roundtrip_via_shim() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "toolu_round".into(),
            content: Some("output data".into()),
            is_error: Some(false),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn tool_result_error_roundtrip_via_shim() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "toolu_err".into(),
            content: Some("error: not found".into()),
            is_error: Some(true),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn multi_turn_tool_conversation_lowering() {
        let tool_use = vec![ClaudeContentBlock::ToolUse {
            id: "toolu_conv".into(),
            name: "search".into(),
            input: json!({"query": "test"}),
        }];
        let tool_result = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "toolu_conv".into(),
            content: Some("found 3 matches".into()),
            is_error: None,
        }];
        let msgs = vec![
            claude_msg("user", "Find tests"),
            claude_blocks_msg("assistant", &tool_use),
            claude_blocks_msg("user", &tool_result),
            claude_msg("assistant", "I found 3 matches."),
        ];
        let conv = lowering::to_ir(&msgs, None);
        assert_eq!(conv.len(), 4);
        let back = lowering::from_ir(&conv);
        assert_eq!(back.len(), 4);
        assert_eq!(back[0].content, "Find tests");
        assert_eq!(back[3].content, "I found 3 matches.");
    }

    #[test]
    fn map_tool_result_helper_success() {
        let msg = dialect::map_tool_result("toolu_01", "output text", false);
        assert_eq!(msg.role, "user");
        let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
        match &parsed[0] {
            ClaudeContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "toolu_01");
                assert_eq!(content.as_deref(), Some("output text"));
                assert!(is_error.is_none());
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn map_tool_result_helper_error() {
        let msg = dialect::map_tool_result("toolu_err", "failed", true);
        let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
        match &parsed[0] {
            ClaudeContentBlock::ToolResult { is_error, .. } => {
                assert_eq!(*is_error, Some(true));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_ir_lowering_content() {
        let blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "toolu_ir".into(),
            content: Some("data".into()),
            is_error: None,
        }];
        let msgs = vec![claude_blocks_msg("user", &blocks)];
        let conv = lowering::to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "toolu_ir");
                assert_eq!(content.len(), 1);
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_ir_lowering_is_error_true() {
        let blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "toolu_err2".into(),
            content: Some("fail".into()),
            is_error: Some(true),
        }];
        let msgs = vec![claude_blocks_msg("user", &blocks)];
        let conv = lowering::to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tool_use_in_response_via_handler() {
        let mut client = AnthropicClient::new();
        client.set_handler(Box::new(|req| {
            let events = vec![make_agent_event(AgentEventKind::ToolCall {
                tool_name: "write_file".into(),
                tool_use_id: Some("toolu_999".into()),
                parent_tool_use_id: None,
                input: json!({"path": "out.txt", "content": "data"}),
            })];
            Ok(response_from_events(&events, &req.model, None))
        }));
        let resp = client.create(simple_request("Write it")).await.unwrap();
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        assert!(
            matches!(&resp.content[0], ContentBlock::ToolUse { name, .. } if name == "write_file")
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Anthropic token usage mapping to receipt
// ═══════════════════════════════════════════════════════════════════════════

mod token_usage_mapping {
    use super::*;

    #[test]
    fn usage_from_claude_response() {
        let resp = ClaudeResponse {
            id: "msg_usage".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![ClaudeContentBlock::Text { text: "ok".into() }],
            stop_reason: Some("end_turn".into()),
            usage: Some(ClaudeUsage {
                input_tokens: 150,
                output_tokens: 75,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        };
        let shim_resp = response_from_claude(&resp);
        assert_eq!(shim_resp.usage.input_tokens, 150);
        assert_eq!(shim_resp.usage.output_tokens, 75);
    }

    #[test]
    fn usage_with_cache_tokens() {
        let resp = ClaudeResponse {
            id: "msg_cache".into(),
            model: "test".into(),
            role: "assistant".into(),
            content: vec![],
            stop_reason: None,
            usage: Some(ClaudeUsage {
                input_tokens: 1000,
                output_tokens: 500,
                cache_creation_input_tokens: Some(200),
                cache_read_input_tokens: Some(100),
            }),
        };
        let shim_resp = response_from_claude(&resp);
        assert_eq!(shim_resp.usage.input_tokens, 1000);
        assert_eq!(shim_resp.usage.output_tokens, 500);
        assert_eq!(shim_resp.usage.cache_creation_input_tokens, Some(200));
        assert_eq!(shim_resp.usage.cache_read_input_tokens, Some(100));
    }

    #[test]
    fn usage_none_defaults_to_zero() {
        let resp = ClaudeResponse {
            id: "msg_nousage".into(),
            model: "test".into(),
            role: "assistant".into(),
            content: vec![],
            stop_reason: None,
            usage: None,
        };
        let shim_resp = response_from_claude(&resp);
        assert_eq!(shim_resp.usage.input_tokens, 0);
        assert_eq!(shim_resp.usage.output_tokens, 0);
    }

    #[test]
    fn usage_serde_roundtrip() {
        let usage = ClaudeUsage {
            input_tokens: 100,
            output_tokens: 200,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(5),
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: ClaudeUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, usage);
    }

    #[test]
    fn usage_no_cache_fields_skipped_in_json() {
        let usage = ClaudeUsage {
            input_tokens: 50,
            output_tokens: 25,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let json = serde_json::to_string(&usage).unwrap();
        assert!(!json.contains("cache_creation"));
        assert!(!json.contains("cache_read"));
    }

    #[test]
    fn usage_in_stream_message_delta() {
        let usage = ClaudeUsage {
            input_tokens: 300,
            output_tokens: 150,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let event = ClaudeStreamEvent::MessageDelta {
            delta: ClaudeMessageDelta {
                stop_reason: Some("end_turn".into()),
                stop_sequence: None,
            },
            usage: Some(usage.clone()),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
        match back {
            ClaudeStreamEvent::MessageDelta { usage: u, .. } => {
                let u = u.unwrap();
                assert_eq!(u.input_tokens, 300);
                assert_eq!(u.output_tokens, 150);
            }
            other => panic!("expected MessageDelta, got {other:?}"),
        }
    }

    #[test]
    fn receipt_builder_accepts_usage_raw() {
        let usage_raw = json!({
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_creation_input_tokens": 10
        });
        let receipt = ReceiptBuilder::new("claude-backend")
            .usage_raw(usage_raw.clone())
            .build();
        assert_eq!(receipt.usage_raw, usage_raw);
    }

    #[test]
    fn usage_maps_to_shim_usage_type() {
        let usage = Usage {
            input_tokens: 500,
            output_tokens: 250,
            cache_creation_input_tokens: Some(50),
            cache_read_input_tokens: Some(25),
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.input_tokens, 500);
        assert_eq!(back.output_tokens, 250);
        assert_eq!(back.cache_creation_input_tokens, Some(50));
        assert_eq!(back.cache_read_input_tokens, Some(25));
    }

    #[test]
    fn response_from_events_with_usage() {
        let events = vec![make_agent_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        })];
        let usage = ClaudeUsage {
            input_tokens: 42,
            output_tokens: 17,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let resp = response_from_events(&events, "test", Some(&usage));
        assert_eq!(resp.usage.input_tokens, 42);
        assert_eq!(resp.usage.output_tokens, 17);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Anthropic error type mapping
// ═══════════════════════════════════════════════════════════════════════════

mod error_type_mapping {
    use super::*;

    #[test]
    fn rate_limit_error_display() {
        let err = ShimError::ApiError {
            error_type: "rate_limit_error".into(),
            message: "Too many requests".into(),
        };
        assert_eq!(
            err.to_string(),
            "api error (rate_limit_error): Too many requests"
        );
    }

    #[test]
    fn overloaded_error_display() {
        let err = ShimError::ApiError {
            error_type: "overloaded_error".into(),
            message: "Server is overloaded".into(),
        };
        assert_eq!(
            err.to_string(),
            "api error (overloaded_error): Server is overloaded"
        );
    }

    #[test]
    fn invalid_request_error_display() {
        let err = ShimError::InvalidRequest("missing model field".into());
        assert_eq!(err.to_string(), "invalid request: missing model field");
    }

    #[test]
    fn internal_error_display() {
        let err = ShimError::Internal("unexpected panic".into());
        assert_eq!(err.to_string(), "internal: unexpected panic");
    }

    #[test]
    fn api_error_serde_roundtrip() {
        let err = ApiError {
            error_type: "invalid_request_error".into(),
            message: "Invalid parameter".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn claude_api_error_serde_roundtrip() {
        let err = ClaudeApiError {
            error_type: "authentication_error".into(),
            message: "Invalid API key".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ClaudeApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(back, err);
    }

    #[test]
    fn stream_error_event_serde() {
        let event = StreamEvent::Error {
            error: ApiError {
                error_type: "overloaded_error".into(),
                message: "Busy".into(),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn claude_stream_error_maps_to_agent_error() {
        let event = ClaudeStreamEvent::Error {
            error: ClaudeApiError {
                error_type: "rate_limit_error".into(),
                message: "Rate limited".into(),
            },
        };
        let events = dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::Error { message, .. } => {
                assert!(message.contains("rate_limit_error"));
                assert!(message.contains("Rate limited"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn claude_api_error_stream_conversion_via_shim() {
        let claude_event = ClaudeStreamEvent::Error {
            error: ClaudeApiError {
                error_type: "authentication_error".into(),
                message: "Invalid API key".into(),
            },
        };
        let shim_event = stream_event_from_claude(&claude_event);
        match shim_event {
            StreamEvent::Error { error } => {
                assert_eq!(error.error_type, "authentication_error");
                assert_eq!(error.message, "Invalid API key");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn empty_messages_returns_invalid_request() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "test".into(),
            max_tokens: 4096,
            messages: vec![],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn custom_handler_api_error() {
        let mut client = AnthropicClient::new();
        client.set_handler(Box::new(|_| {
            Err(ShimError::ApiError {
                error_type: "overloaded_error".into(),
                message: "Server is overloaded".into(),
            })
        }));
        let err = client.create(simple_request("test")).await.unwrap_err();
        match err {
            ShimError::ApiError {
                error_type,
                message,
            } => {
                assert_eq!(error_type, "overloaded_error");
                assert_eq!(message, "Server is overloaded");
            }
            other => panic!("expected ApiError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn custom_handler_internal_error() {
        let mut client = AnthropicClient::new();
        client.set_handler(Box::new(|_| {
            Err(ShimError::Internal("pipeline failed".into()))
        }));
        let err = client.create(simple_request("test")).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    #[tokio::test]
    async fn custom_stream_handler_error() {
        let mut client = AnthropicClient::new();
        client.set_stream_handler(Box::new(|_| {
            Err(ShimError::InvalidRequest("bad request".into()))
        }));
        let err = client
            .create_stream(simple_request("test"))
            .await
            .unwrap_err();
        assert!(matches!(err, ShimError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn empty_messages_stream_returns_invalid_request() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "test".into(),
            max_tokens: 4096,
            messages: vec![],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let err = client.create_stream(req).await.unwrap_err();
        assert!(matches!(err, ShimError::InvalidRequest(_)));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Anthropic metadata and client tracking
// ═══════════════════════════════════════════════════════════════════════════

mod metadata_and_tracking {
    use super::*;

    #[test]
    fn response_id_has_msg_prefix() {
        let events = vec![make_agent_event(AgentEventKind::AssistantMessage {
            text: "test".into(),
        })];
        let resp = response_from_events(&events, "model", None);
        assert!(resp.id.starts_with("msg_"));
    }

    #[test]
    fn response_role_is_always_assistant() {
        let resp = response_from_events(&[], "m", None);
        assert_eq!(resp.role, "assistant");
    }

    #[test]
    fn response_type_is_message() {
        let resp = response_from_events(&[], "m", None);
        assert_eq!(resp.response_type, "message");
    }

    #[test]
    fn response_model_preserved() {
        let events = vec![make_agent_event(AgentEventKind::AssistantMessage {
            text: "hi".into(),
        })];
        let resp = response_from_events(&events, "claude-opus-4-20250514", None);
        assert_eq!(resp.model, "claude-opus-4-20250514");
    }

    #[test]
    fn claude_config_default_values() {
        let cfg = ClaudeConfig::default();
        assert_eq!(cfg.base_url, "https://api.anthropic.com/v1");
        assert_eq!(cfg.model, "claude-sonnet-4-20250514");
        assert_eq!(cfg.max_tokens, 4096);
        assert!(cfg.system_prompt.is_none());
        assert!(cfg.thinking.is_none());
    }

    #[test]
    fn thinking_config_new() {
        let tc = ThinkingConfig::new(8192);
        assert_eq!(tc.thinking_type, "enabled");
        assert_eq!(tc.budget_tokens, 8192);
    }

    #[test]
    fn thinking_config_serde_roundtrip() {
        let tc = ThinkingConfig::new(4096);
        let json = serde_json::to_string(&tc).unwrap();
        let back: ThinkingConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(tc, back);
    }

    #[test]
    fn request_with_thinking_maps_to_claude() {
        let mut req = simple_request("Think hard");
        req.thinking = Some(ThinkingConfig::new(4096));
        let claude_req = request_to_claude(&req);
        assert!(claude_req.thinking.is_some());
        assert_eq!(claude_req.thinking.unwrap().budget_tokens, 4096);
    }

    #[test]
    fn client_default_creates_with_defaults() {
        let client = AnthropicClient::new();
        let dbg = format!("{client:?}");
        assert!(dbg.contains("AnthropicClient"));
    }

    #[test]
    fn client_with_model() {
        let client = AnthropicClient::with_model("claude-opus-4-20250514");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("claude-opus-4-20250514"));
    }

    #[test]
    fn client_default_eq_new() {
        let c1 = AnthropicClient::new();
        let c2 = AnthropicClient::default();
        let d1 = format!("{c1:?}");
        let d2 = format!("{c2:?}");
        assert_eq!(d1, d2);
    }

    #[test]
    fn message_request_optional_fields_skip_serialization() {
        let req = simple_request("Hi");
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("system").is_none());
        assert!(json.get("temperature").is_none());
        assert!(json.get("stop_sequences").is_none());
        assert!(json.get("thinking").is_none());
        assert!(json.get("stream").is_none());
    }

    #[test]
    fn message_request_full_serde_roundtrip() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 8192,
            messages: vec![
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "Hello".into(),
                    }],
                },
                Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text { text: "Hi".into() }],
                },
            ],
            system: Some("Be helpful.".into()),
            temperature: Some(0.7),
            stop_sequences: Some(vec!["STOP".into()]),
            thinking: Some(ThinkingConfig::new(2048)),
            stream: Some(false),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: MessageRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, req.model);
        assert_eq!(back.max_tokens, req.max_tokens);
        assert_eq!(back.system, req.system);
        assert_eq!(back.temperature, req.temperature);
        assert_eq!(back.messages.len(), req.messages.len());
    }

    #[test]
    fn message_response_serde_roundtrip() {
        let resp = MessageResponse {
            id: "msg_roundtrip".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "Hello".into(),
            }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 20,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: MessageResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, resp.id);
        assert_eq!(back.model, resp.model);
        assert_eq!(back.stop_reason, resp.stop_reason);
    }

    #[test]
    fn response_from_claude_preserves_all_fields() {
        let claude_resp = ClaudeResponse {
            id: "msg_full".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![ClaudeContentBlock::Text {
                text: "Hello!".into(),
            }],
            stop_reason: Some("end_turn".into()),
            usage: Some(ClaudeUsage {
                input_tokens: 10,
                output_tokens: 20,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        };
        let resp = response_from_claude(&claude_resp);
        assert_eq!(resp.id, "msg_full");
        assert_eq!(resp.response_type, "message");
        assert_eq!(resp.role, "assistant");
        assert_eq!(resp.model, "claude-sonnet-4-20250514");
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 20);
    }

    #[test]
    fn dialect_version_constant() {
        assert_eq!(dialect::DIALECT_VERSION, "claude/v0.1");
    }

    #[test]
    fn backend_name_constant() {
        assert_eq!(abp_claude_sdk::BACKEND_NAME, "sidecar:claude");
    }

    #[test]
    fn btreemap_deterministic_json() {
        let mut ext1 = BTreeMap::new();
        ext1.insert("a".to_string(), json!(1));
        ext1.insert("b".to_string(), json!(2));
        ext1.insert("c".to_string(), json!(3));

        let mut ext2 = BTreeMap::new();
        ext2.insert("c".to_string(), json!(3));
        ext2.insert("a".to_string(), json!(1));
        ext2.insert("b".to_string(), json!(2));

        let json1 = serde_json::to_string(&ext1).unwrap();
        let json2 = serde_json::to_string(&ext2).unwrap();
        assert_eq!(json1, json2);
    }

    #[tokio::test]
    async fn model_preserved_in_response() {
        let client = AnthropicClient::new();
        let mut req = simple_request("Test");
        req.model = "claude-opus-4-20250514".into();
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.model, "claude-opus-4-20250514");
    }

    #[test]
    fn ext_field_serialization() {
        let mut ext = BTreeMap::new();
        ext.insert("thinking".into(), serde_json::Value::Bool(true));
        ext.insert(
            "signature".into(),
            serde_json::Value::String("sig_abc".into()),
        );
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "reasoning".into(),
            },
            ext: Some(ext),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"thinking\":true"));
        assert!(json.contains("\"signature\":\"sig_abc\""));
    }

    #[test]
    fn response_from_events_ignores_non_content_events() {
        let events = vec![
            make_agent_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }),
            make_agent_event(AgentEventKind::Warning {
                message: "warn".into(),
            }),
            make_agent_event(AgentEventKind::FileChanged {
                path: "f.txt".into(),
                summary: "changed".into(),
            }),
        ];
        let resp = response_from_events(&events, "test", None);
        assert!(resp.content.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Passthrough mode and fidelity
// ═══════════════════════════════════════════════════════════════════════════

mod passthrough_fidelity {
    use super::*;

    #[test]
    fn to_passthrough_event_preserves_raw() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "hello".into(),
            },
        };
        let agent_event = dialect::to_passthrough_event(&event);
        let ext = agent_event.ext.as_ref().unwrap();
        assert!(ext.contains_key("raw_message"));
    }

    #[test]
    fn from_passthrough_event_round_trip() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "hello".into(),
            },
        };
        let agent_event = dialect::to_passthrough_event(&event);
        let back = dialect::from_passthrough_event(&agent_event).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn verify_passthrough_fidelity_valid_events() {
        let events = vec![
            ClaudeStreamEvent::MessageStart {
                message: make_claude_response(vec![]),
            },
            ClaudeStreamEvent::ContentBlockStart {
                index: 0,
                content_block: ClaudeContentBlock::Text {
                    text: String::new(),
                },
            },
            ClaudeStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ClaudeStreamDelta::TextDelta { text: "hi".into() },
            },
            ClaudeStreamEvent::ContentBlockStop { index: 0 },
            ClaudeStreamEvent::MessageStop {},
        ];
        assert!(dialect::verify_passthrough_fidelity(&events));
    }

    #[test]
    fn passthrough_message_start_roundtrip() {
        let event = ClaudeStreamEvent::MessageStart {
            message: make_claude_response(vec![ClaudeContentBlock::Text {
                text: "init".into(),
            }]),
        };
        let agent = dialect::to_passthrough_event(&event);
        let back = dialect::from_passthrough_event(&agent).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn passthrough_ping_roundtrip() {
        let event = ClaudeStreamEvent::Ping {};
        let agent = dialect::to_passthrough_event(&event);
        let back = dialect::from_passthrough_event(&agent).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn non_passthrough_event_returns_none() {
        let agent = make_agent_event(AgentEventKind::AssistantMessage {
            text: "not passthrough".into(),
        });
        assert!(dialect::from_passthrough_event(&agent).is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Content block serde and edge cases
// ═══════════════════════════════════════════════════════════════════════════

mod content_block_serde {
    use super::*;

    #[test]
    fn text_block_serde_has_type_field() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"text\""));
    }

    #[test]
    fn tool_use_block_serde_has_type_field() {
        let block = ContentBlock::ToolUse {
            id: "toolu_01".into(),
            name: "bash".into(),
            input: json!({}),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"tool_use\""));
    }

    #[test]
    fn tool_result_block_serde_has_type_field() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "toolu_01".into(),
            content: Some("output".into()),
            is_error: Some(false),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"tool_result\""));
    }

    #[test]
    fn thinking_block_serde_has_type_field() {
        let block = ContentBlock::Thinking {
            thinking: "hmm".into(),
            signature: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"thinking\""));
    }

    #[test]
    fn image_block_serde_has_type_field() {
        let block = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "abc".into(),
            },
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"image\""));
    }

    #[test]
    fn text_block_roundtrip() {
        let block = ContentBlock::Text {
            text: "Hello world".into(),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn text_block_unicode_roundtrip() {
        let block = ContentBlock::Text {
            text: "こんにちは 🌍 مرحبا".into(),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn text_block_empty_roundtrip() {
        let block = ContentBlock::Text {
            text: String::new(),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn image_base64_roundtrip() {
        let block = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn image_url_roundtrip() {
        let block = ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/image.png".into(),
            },
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn thinking_block_with_signature_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "reasoning step by step".into(),
            signature: Some("sig_xyz".into()),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn thinking_block_no_signature_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "just thinking".into(),
            signature: None,
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn claude_request_serde_roundtrip() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            system: Some("system prompt".into()),
            messages: vec![claude_msg("user", "hi")],
            thinking: Some(ThinkingConfig::new(5000)),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ClaudeRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, req.model);
        assert_eq!(back.max_tokens, req.max_tokens);
        assert_eq!(back.system, req.system);
        assert!(back.thinking.is_some());
    }

    #[test]
    fn claude_response_serde_roundtrip() {
        let resp = ClaudeResponse {
            id: "msg_42".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![
                ClaudeContentBlock::Text {
                    text: "Sure".into(),
                },
                ClaudeContentBlock::ToolUse {
                    id: "toolu_1".into(),
                    name: "read".into(),
                    input: json!({"path": "a.rs"}),
                },
            ],
            stop_reason: Some("tool_use".into()),
            usage: Some(ClaudeUsage {
                input_tokens: 200,
                output_tokens: 100,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ClaudeResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, resp);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. IR lowering roundtrips
// ═══════════════════════════════════════════════════════════════════════════

mod ir_lowering_roundtrips {
    use super::*;

    #[test]
    fn roundtrip_user_text() {
        let msgs = vec![claude_msg("user", "Hello world")];
        let conv = lowering::to_ir(&msgs, None);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content, "Hello world");
    }

    #[test]
    fn roundtrip_assistant_text() {
        let msgs = vec![claude_msg("assistant", "I will help you")];
        let conv = lowering::to_ir(&msgs, None);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "assistant");
        assert_eq!(back[0].content, "I will help you");
    }

    #[test]
    fn roundtrip_multi_turn() {
        let msgs = vec![
            claude_msg("user", "Hello"),
            claude_msg("assistant", "Hi there"),
            claude_msg("user", "Do something"),
            claude_msg("assistant", "Done"),
        ];
        let conv = lowering::to_ir(&msgs, None);
        let back = lowering::from_ir(&conv);
        assert_eq!(back.len(), 4);
        assert_eq!(back[0].content, "Hello");
        assert_eq!(back[3].content, "Done");
    }

    #[test]
    fn roundtrip_tool_use_block() {
        let blocks = vec![ClaudeContentBlock::ToolUse {
            id: "toolu_rt".into(),
            name: "glob".into(),
            input: json!({"pattern": "*.rs"}),
        }];
        let msgs = vec![claude_blocks_msg("assistant", &blocks)];
        let conv = lowering::to_ir(&msgs, None);
        let back = lowering::from_ir(&conv);
        let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
        match &parsed[0] {
            ClaudeContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_rt");
                assert_eq!(name, "glob");
                assert_eq!(input, &json!({"pattern": "*.rs"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_thinking_block() {
        let blocks = vec![ClaudeContentBlock::Thinking {
            thinking: "Let me reason step by step".into(),
            signature: None,
        }];
        let msgs = vec![claude_blocks_msg("assistant", &blocks)];
        let conv = lowering::to_ir(&msgs, None);
        let back = lowering::from_ir(&conv);
        let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
        match &parsed[0] {
            ClaudeContentBlock::Thinking { thinking, .. } => {
                assert_eq!(thinking, "Let me reason step by step");
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_image_base64() {
        let blocks = vec![ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 {
                media_type: "image/png".into(),
                data: "abcd".into(),
            },
        }];
        let msgs = vec![claude_blocks_msg("user", &blocks)];
        let conv = lowering::to_ir(&msgs, None);
        let back = lowering::from_ir(&conv);
        let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
        match &parsed[0] {
            ClaudeContentBlock::Image {
                source: ClaudeImageSource::Base64 { media_type, data },
            } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "abcd");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    #[test]
    fn plain_text_not_parsed_as_json() {
        let msgs = vec![claude_msg("assistant", "just plain text")];
        let conv = lowering::to_ir(&msgs, None);
        assert_eq!(conv.messages[0].text_content(), "just plain text");
    }

    #[test]
    fn invalid_json_treated_as_plain_text() {
        let msgs = vec![claude_msg("assistant", "[not valid json")];
        let conv = lowering::to_ir(&msgs, None);
        assert_eq!(conv.messages[0].text_content(), "[not valid json");
    }

    #[test]
    fn unknown_role_defaults_to_user() {
        let msgs = vec![claude_msg("tool", "result")];
        let conv = lowering::to_ir(&msgs, None);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn ir_tool_role_maps_to_claude_user() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Tool, "output")]);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "user");
    }
}
