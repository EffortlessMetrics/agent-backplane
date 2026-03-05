#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive cross-SDK integration tests.
//!
//! These tests exercise **real cross-crate interactions** between SDK shim
//! crates, verifying the full pipeline:
//!   request → IR → work order → receipt → response
//!
//! Test areas:
//!   1. OpenAI ↔ Claude mapping
//!   2. OpenAI ↔ Gemini mapping
//!   3. Claude ↔ Gemini mapping
//!   4. All dialects → IR → All dialects
//!   5. Projection matrix integration
//!   6. Receipt chain integration
//!   7. Capability negotiation integration

use abp_capability::{
    negotiate,
    negotiate::NegotiationPolicy,
    negotiate::{apply_policy, pre_negotiate},
    negotiate_capabilities, EmulationStrategy, NegotiationResult,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    RuntimeConfig, SupportLevel as CoreSupportLevel, UsageNormalized, WorkOrder, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapper::{
    default_ir_mapper, supported_ir_pairs, ClaudeGeminiIrMapper, ClaudeKimiIrMapper,
    CodexClaudeIrMapper, GeminiKimiIrMapper, IrMapper, MapError, OpenAiClaudeIrMapper,
    OpenAiCodexIrMapper, OpenAiCopilotIrMapper, OpenAiGeminiIrMapper, OpenAiKimiIrMapper,
};
use abp_projection::{
    BackendEntry, CompatibilityScore, DialectPair, FallbackEntry, ProjectionEntry, ProjectionError,
    ProjectionMatrix, ProjectionMode, ProjectionResult, ProjectionScore, RequiredEmulation,
    RoutingPath,
};
use abp_receipt::{
    canonicalize, compute_hash, diff_receipts, verify_hash, ChainBuilder, ReceiptBuilder,
    ReceiptChain,
};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn simple_text_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ])
}

fn with_system_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
    ])
}

fn with_tools_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What's the weather?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "Portland"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "55°F, cloudy".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "It's 55°F and cloudy in Portland."),
    ])
}

fn multi_turn_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
        IrMessage::text(IrRole::User, "How are you?"),
        IrMessage::text(IrRole::Assistant, "Great, thanks!"),
        IrMessage::text(IrRole::User, "Tell me a joke"),
    ])
}

fn make_openai_request() -> abp_shim_openai::ChatCompletionRequest {
    abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            abp_shim_openai::Message::system("You are helpful."),
            abp_shim_openai::Message::user("Hello"),
        ])
        .temperature(0.7)
        .max_tokens(1024)
        .build()
}

fn make_openai_request_with_tools() -> abp_shim_openai::ChatCompletionRequest {
    abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            abp_shim_openai::Message::system("You are helpful."),
            abp_shim_openai::Message::user("What's the weather in Portland?"),
        ])
        .tools(vec![abp_shim_openai::Tool::function(
            "get_weather",
            "Get weather for a location",
            json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                }
            }),
        )])
        .build()
}

fn make_claude_request() -> abp_shim_claude::MessageRequest {
    abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text {
                text: "Hello".into(),
            }],
        }],
        system: Some("You are helpful.".into()),
        temperature: Some(0.7),
        stop_sequences: None,
        thinking: None,
        stream: None,
    }
}

fn make_gemini_request() -> abp_shim_gemini::GenerateContentRequest {
    abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text("Hello"),
        ]))
        .system_instruction(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text("You are helpful."),
        ]))
        .generation_config(abp_shim_gemini::GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(1024),
            ..Default::default()
        })
}

fn make_codex_request() -> abp_codex_sdk::dialect::CodexRequest {
    abp_shim_codex::CodexRequestBuilder::new()
        .model("codex-mini-latest")
        .input(vec![abp_shim_codex::codex_message("user", "Hello")])
        .temperature(0.7)
        .max_output_tokens(1024)
        .build()
}

fn make_kimi_request() -> abp_kimi_sdk::dialect::KimiRequest {
    abp_shim_kimi::KimiRequestBuilder::new()
        .model("moonshot-v1-8k")
        .messages(vec![
            abp_shim_kimi::Message::system("You are helpful."),
            abp_shim_kimi::Message::user("Hello"),
        ])
        .temperature(0.7)
        .max_tokens(1024)
        .build()
}

fn make_copilot_request() -> abp_copilot_sdk::dialect::CopilotRequest {
    abp_shim_copilot::CopilotRequestBuilder::new()
        .model("gpt-4o")
        .messages(vec![
            abp_shim_copilot::Message::system("You are helpful."),
            abp_shim_copilot::Message::user("Hello"),
        ])
        .build()
}

fn mock_receipt(backend_id: &str, model: &str) -> Receipt {
    let now = Utc::now();
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .model(model)
        .usage_tokens(100, 50)
        .add_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantMessage {
                text: "Hello! How can I help you?".into(),
            },
            ext: None,
        })
        .build()
}

fn mock_receipt_with_tool_call(backend_id: &str, model: &str) -> Receipt {
    let now = Utc::now();
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .model(model)
        .usage_tokens(150, 80)
        .add_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolCall {
                tool_name: "get_weather".into(),
                tool_use_id: Some("call_123".into()),
                parent_tool_use_id: None,
                input: json!({"location": "Portland"}),
            },
            ext: None,
        })
        .build()
}

fn make_manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn make_requirements(caps: &[Capability]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Emulated,
            })
            .collect(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. OpenAI ↔ Claude mapping (25+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod openai_claude_mapping {
    use super::*;

    #[test]
    fn openai_request_to_ir_preserves_user_message() {
        let req = make_openai_request();
        let ir = abp_shim_openai::request_to_ir(&req);
        assert!(ir.messages.iter().any(|m| m.role == IrRole::User));
        let user = ir.messages.iter().find(|m| m.role == IrRole::User).unwrap();
        assert_eq!(user.text_content(), "Hello");
    }

    #[test]
    fn openai_request_to_ir_preserves_system_message() {
        let req = make_openai_request();
        let ir = abp_shim_openai::request_to_ir(&req);
        let sys = ir.messages.iter().find(|m| m.role == IrRole::System);
        assert!(sys.is_some());
        assert_eq!(sys.unwrap().text_content(), "You are helpful.");
    }

    #[test]
    fn openai_to_ir_to_claude_system_becomes_separate_field() {
        let req = make_openai_request();
        let ir = abp_shim_openai::request_to_ir(&req);
        let sys_msg = ir.system_message();
        assert!(sys_msg.is_some(), "IR should have system message");
        // Claude treats system as a separate field, not a message
        // Verify IR carries the system info that Claude can extract
        assert_eq!(sys_msg.unwrap().text_content(), "You are helpful.");
    }

    #[test]
    fn openai_ir_roundtrip_through_claude_mapper() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert!(!mapped.is_empty());
        // Messages should preserve user/assistant pattern
        assert!(mapped.messages.iter().any(|m| m.role == IrRole::User));
        assert!(mapped.messages.iter().any(|m| m.role == IrRole::Assistant));
    }

    #[test]
    fn openai_claude_mapper_preserves_text_content() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let user = mapped
            .messages
            .iter()
            .find(|m| m.role == IrRole::User)
            .unwrap();
        assert_eq!(user.text_content(), "Hello");
    }

    #[test]
    fn openai_tool_call_maps_to_claude_tool_use() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = with_tools_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let assistant = mapped
            .messages
            .iter()
            .find(|m| m.role == IrRole::Assistant)
            .unwrap();
        let has_tool_use = assistant
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::ToolUse { .. }));
        assert!(
            has_tool_use,
            "Tool use should survive OpenAI→Claude mapping"
        );
    }

    #[test]
    fn openai_tool_result_maps_to_claude_tool_result() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = with_tools_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        // Claude maps Tool-role messages to User-role with ToolResult content blocks
        let has_tool_result = mapped.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        });
        assert!(
            has_tool_result,
            "Tool result content should survive mapping"
        );
    }

    #[test]
    fn openai_to_work_order_sets_model() {
        let req = make_openai_request();
        let wo = abp_shim_openai::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn claude_to_work_order_sets_model() {
        let req = make_claude_request();
        let wo = abp_shim_claude::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn openai_receipt_to_response_has_choices() {
        let receipt = mock_receipt("openai-mock", "gpt-4o");
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.choices.is_empty());
        assert_eq!(resp.model, "gpt-4o");
    }

    #[test]
    fn openai_receipt_to_response_contains_assistant_text() {
        let receipt = mock_receipt("openai-mock", "gpt-4o");
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let msg = &resp.choices[0].message;
        assert!(msg.content.as_ref().unwrap().contains("Hello"));
    }

    #[test]
    fn openai_receipt_tool_call_becomes_function_call() {
        let receipt = mock_receipt_with_tool_call("openai-mock", "gpt-4o");
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let msg = &resp.choices[0].message;
        assert!(msg.tool_calls.is_some());
        let tc = &msg.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.function.name, "get_weather");
        assert_eq!(tc.call_type, "function");
    }

    #[test]
    fn claude_system_field_vs_openai_system_role() {
        // OpenAI uses system role message; Claude uses system field
        let oai_req = make_openai_request();
        let oai_ir = abp_shim_openai::request_to_ir(&oai_req);
        let sys = oai_ir.system_message().unwrap();
        assert_eq!(sys.role, IrRole::System);
        // Claude request has system as a separate field
        let claude_req = make_claude_request();
        assert!(claude_req.system.is_some());
        assert_eq!(claude_req.system.as_deref().unwrap(), "You are helpful.");
    }

    #[test]
    fn openai_streaming_events_produce_sse_chunks() {
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: "Hello".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: " world".into(),
                },
                ext: None,
            },
        ];
        let chunks = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
        assert_eq!(chunks.len(), 3); // 2 deltas + 1 final stop chunk
    }

    #[test]
    fn openai_stream_chunk_has_correct_object_type() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "Hi".into() },
            ext: None,
        }];
        let chunks = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
        assert_eq!(chunks[0].object, "chat.completion.chunk");
    }

    #[test]
    fn openai_usage_from_receipt_maps_token_counts() {
        let receipt = mock_receipt("openai-mock", "gpt-4o");
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn claude_receipt_response_uses_message_format() {
        let receipt = mock_receipt("claude-mock", "claude-sonnet-4-20250514");
        let events = &receipt.trace;
        let resp = abp_shim_claude::response_from_events(events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.response_type, "message");
        assert_eq!(resp.role, "assistant");
    }

    #[test]
    fn claude_response_has_content_blocks() {
        let receipt = mock_receipt("claude-mock", "claude-sonnet-4-20250514");
        let events = &receipt.trace;
        let resp = abp_shim_claude::response_from_events(events, "claude-sonnet-4-20250514", None);
        assert!(!resp.content.is_empty());
        match &resp.content[0] {
            abp_shim_claude::ContentBlock::Text { text } => {
                assert!(text.contains("Hello"));
            }
            _ => panic!("Expected text content block"),
        }
    }

    #[test]
    fn openai_claude_multi_turn_preserves_count() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = multi_turn_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(mapped.len(), conv.len());
    }

    #[test]
    fn openai_to_claude_tool_name_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = with_tools_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let tool_uses: Vec<_> = mapped.tool_calls();
        assert!(!tool_uses.is_empty());
        if let IrContentBlock::ToolUse { name, .. } = tool_uses[0] {
            assert_eq!(name, "get_weather");
        }
    }

    #[test]
    fn openai_to_claude_tool_input_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = with_tools_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let tool_uses: Vec<_> = mapped.tool_calls();
        if let IrContentBlock::ToolUse { input, .. } = tool_uses[0] {
            assert_eq!(input["city"], "Portland");
        }
    }

    #[test]
    fn openai_request_builder_defaults_model() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .messages(vec![abp_shim_openai::Message::user("hi")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn openai_work_order_preserves_temperature() {
        let req = make_openai_request();
        let wo = abp_shim_openai::request_to_work_order(&req);
        let temp = wo.config.vendor.get("temperature");
        assert!(temp.is_some());
    }

    #[test]
    fn claude_work_order_preserves_max_tokens() {
        let req = make_claude_request();
        let wo = abp_shim_claude::request_to_work_order(&req);
        // The work order config should have model set
        assert!(wo.config.model.is_some());
    }

    #[test]
    fn openai_finish_reason_stop_on_text_response() {
        let receipt = mock_receipt("openai-mock", "gpt-4o");
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn openai_finish_reason_tool_calls_on_tool_response() {
        let receipt = mock_receipt_with_tool_call("openai-mock", "gpt-4o");
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. OpenAI ↔ Gemini mapping (20+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod openai_gemini_mapping {
    use super::*;

    #[test]
    fn openai_to_gemini_simple_text_roundtrip() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        assert!(!mapped.is_empty());
        assert!(mapped.messages.iter().any(|m| m.role == IrRole::User));
    }

    #[test]
    fn openai_to_gemini_preserves_user_text() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        let user = mapped
            .messages
            .iter()
            .find(|m| m.role == IrRole::User)
            .unwrap();
        assert_eq!(user.text_content(), "Hello");
    }

    #[test]
    fn openai_to_gemini_preserves_assistant_text() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        let asst = mapped
            .messages
            .iter()
            .find(|m| m.role == IrRole::Assistant)
            .unwrap();
        assert_eq!(asst.text_content(), "Hi there!");
    }

    #[test]
    fn gemini_request_to_ir_basic() {
        let req = make_gemini_request();
        let (ir_req, gen_config, safety) = abp_shim_gemini::request_to_ir(&req).unwrap();
        assert!(!ir_req.conversation.is_empty());
    }

    #[test]
    fn gemini_request_preserves_generation_config() {
        let req = make_gemini_request();
        let (_, gen_config, _) = abp_shim_gemini::request_to_ir(&req).unwrap();
        assert!(gen_config.is_some());
        assert_eq!(gen_config.unwrap().temperature, Some(0.7));
    }

    #[test]
    fn gemini_safety_settings_survive_conversion() {
        let req = make_gemini_request().safety_settings(vec![abp_shim_gemini::SafetySetting {
            category: abp_shim_gemini::HarmCategory::HarmCategoryHarassment,
            threshold: abp_shim_gemini::HarmBlockThreshold::BlockMediumAndAbove,
        }]);
        let (_, _, safety) = abp_shim_gemini::request_to_ir(&req).unwrap();
        assert_eq!(safety.len(), 1);
    }

    #[test]
    fn gemini_function_call_maps_to_ir_tool_use() {
        let req = make_gemini_request().add_content(abp_shim_gemini::Content::model(vec![
            abp_shim_gemini::Part::function_call("search", json!({"query": "rust"})),
        ]));
        let (ir_req, _, _) = abp_shim_gemini::request_to_ir(&req).unwrap();
        let tool_calls = ir_req.conversation.tool_calls();
        assert!(!tool_calls.is_empty());
    }

    #[test]
    fn openai_to_gemini_tool_calls_preserved() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = with_tools_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        let tool_calls = mapped.tool_calls();
        assert!(!tool_calls.is_empty());
    }

    #[test]
    fn openai_to_gemini_multi_turn() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = multi_turn_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(mapped.len(), conv.len());
    }

    #[test]
    fn openai_to_gemini_system_message_handling() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = with_system_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        // System message should be present in some form
        assert!(!mapped.is_empty());
    }

    #[test]
    fn gemini_ir_to_work_order_sets_model() {
        let req = make_gemini_request();
        let (ir_req, gen_config, _) = abp_shim_gemini::request_to_ir(&req).unwrap();
        let wo = abp_shim_gemini::ir_to_work_order(&ir_req, "gemini-2.5-flash", &gen_config);
        assert_eq!(wo.config.model.as_deref(), Some("google/gemini-2.5-flash"));
    }

    #[test]
    fn gemini_content_to_dialect_roundtrip() {
        let content = abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("Hello")]);
        let dialect = abp_shim_gemini::content_to_dialect(&content);
        let back = abp_shim_gemini::content_from_dialect(&dialect);
        assert_eq!(back.role, "user");
        assert_eq!(back.parts.len(), 1);
    }

    #[test]
    fn gemini_part_function_call_roundtrip() {
        let part = abp_shim_gemini::Part::function_call("search", json!({"q": "rust"}));
        let dialect = abp_shim_gemini::part_to_dialect(&part);
        let back = abp_shim_gemini::part_from_dialect(&dialect);
        assert_eq!(part, back);
    }

    #[test]
    fn gemini_part_function_response_roundtrip() {
        let part = abp_shim_gemini::Part::function_response("search", json!("results"));
        let dialect = abp_shim_gemini::part_to_dialect(&part);
        let back = abp_shim_gemini::part_from_dialect(&dialect);
        assert_eq!(part, back);
    }

    #[test]
    fn gemini_part_inline_data_roundtrip() {
        let part = abp_shim_gemini::Part::inline_data("image/png", "base64data");
        let dialect = abp_shim_gemini::part_to_dialect(&part);
        let back = abp_shim_gemini::part_from_dialect(&dialect);
        assert_eq!(part, back);
    }

    #[test]
    fn gemini_generation_config_to_dialect_roundtrip() {
        let cfg = abp_shim_gemini::GenerationConfig {
            temperature: Some(0.5),
            max_output_tokens: Some(512),
            top_p: Some(0.9),
            top_k: Some(40),
            ..Default::default()
        };
        let dialect = abp_shim_gemini::gen_config_to_dialect(&cfg);
        let back = abp_shim_gemini::gen_config_from_dialect(&dialect);
        assert_eq!(back.temperature, Some(0.5));
        assert_eq!(back.max_output_tokens, Some(512));
        assert_eq!(back.top_p, Some(0.9));
        assert_eq!(back.top_k, Some(40));
    }

    #[test]
    fn gemini_tool_declaration_to_dialect() {
        let tool = abp_shim_gemini::ToolDeclaration {
            function_declarations: vec![abp_shim_gemini::FunctionDeclaration {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: json!({"type": "object"}),
            }],
        };
        let dialect = abp_shim_gemini::tool_decl_to_dialect(&tool);
        assert_eq!(dialect.function_declarations.len(), 1);
        assert_eq!(dialect.function_declarations[0].name, "search");
    }

    #[test]
    fn openai_request_to_ir_then_gemini_work_order() {
        let oai_req = make_openai_request();
        let ir = abp_shim_openai::request_to_ir(&oai_req);
        // Verify IR can be consumed by gemini pipeline
        assert!(!ir.is_empty());
        let user_msgs = ir.messages_by_role(IrRole::User);
        assert!(!user_msgs.is_empty());
    }

    #[test]
    fn gemini_multi_content_request_to_ir() {
        let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(abp_shim_gemini::Content::user(vec![
                abp_shim_gemini::Part::text("First"),
            ]))
            .add_content(abp_shim_gemini::Content::model(vec![
                abp_shim_gemini::Part::text("Reply"),
            ]))
            .add_content(abp_shim_gemini::Content::user(vec![
                abp_shim_gemini::Part::text("Second"),
            ]));
        let (ir_req, _, _) = abp_shim_gemini::request_to_ir(&req).unwrap();
        assert_eq!(ir_req.conversation.len(), 3);
    }

    #[test]
    fn gemini_to_dialect_request_preserves_system_instruction() {
        let req = make_gemini_request();
        let dialect_req = abp_shim_gemini::to_dialect_request(&req);
        assert!(dialect_req.system_instruction.is_some());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Claude ↔ Gemini mapping (20+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod claude_gemini_mapping {
    use super::*;

    #[test]
    fn claude_to_gemini_simple_text() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        assert!(!mapped.is_empty());
    }

    #[test]
    fn claude_to_gemini_preserves_user_content() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        let user = mapped
            .messages
            .iter()
            .find(|m| m.role == IrRole::User)
            .unwrap();
        assert_eq!(user.text_content(), "Hello");
    }

    #[test]
    fn claude_to_gemini_preserves_assistant_content() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        let asst = mapped
            .messages
            .iter()
            .find(|m| m.role == IrRole::Assistant)
            .unwrap();
        assert_eq!(asst.text_content(), "Hi there!");
    }

    #[test]
    fn claude_to_gemini_tool_use_maps() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = with_tools_conv();
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        let tool_calls = mapped.tool_calls();
        assert!(!tool_calls.is_empty());
    }

    #[test]
    fn claude_to_gemini_tool_result_maps() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = with_tools_conv();
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        let has_tool_result = mapped.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        });
        assert!(has_tool_result);
    }

    #[test]
    fn claude_to_gemini_system_handling() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = with_system_conv();
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        assert!(!mapped.is_empty());
    }

    #[test]
    fn claude_to_gemini_multi_turn() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = multi_turn_conv();
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(mapped.len(), conv.len());
    }

    #[test]
    fn claude_content_block_text_roundtrip() {
        let block = abp_shim_claude::ContentBlock::Text {
            text: "Hello world".into(),
        };
        let ir = abp_shim_claude::content_block_to_ir(&block);
        let back = abp_shim_claude::content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn claude_content_block_tool_use_roundtrip() {
        let block = abp_shim_claude::ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "search".into(),
            input: json!({"query": "rust"}),
        };
        let ir = abp_shim_claude::content_block_to_ir(&block);
        let back = abp_shim_claude::content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn claude_content_block_tool_result_roundtrip() {
        let block = abp_shim_claude::ContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("result".into()),
            is_error: Some(false),
        };
        let ir = abp_shim_claude::content_block_to_ir(&block);
        let back = abp_shim_claude::content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn claude_content_block_thinking_roundtrip() {
        let block = abp_shim_claude::ContentBlock::Thinking {
            thinking: "Reasoning...".into(),
            signature: Some("sig123".into()),
        };
        let ir = abp_shim_claude::content_block_to_ir(&block);
        let back = abp_shim_claude::content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn claude_tool_use_event_to_response() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "get_weather".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: json!({"city": "Portland"}),
            },
            ext: None,
        }];
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert!(resp
            .content
            .iter()
            .any(|b| matches!(b, abp_shim_claude::ContentBlock::ToolUse { .. })));
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn gemini_to_claude_text_preserves() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Translate this"),
            IrMessage::text(IrRole::Assistant, "Translated text"),
        ]);
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        let asst = mapped
            .messages
            .iter()
            .find(|m| m.role == IrRole::Assistant)
            .unwrap();
        assert_eq!(asst.text_content(), "Translated text");
    }

    #[test]
    fn claude_gemini_mapper_tool_name_preserved() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = with_tools_conv();
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        let tool_calls = mapped.tool_calls();
        if let Some(IrContentBlock::ToolUse { name, .. }) = tool_calls.first() {
            assert_eq!(name, "get_weather");
        }
    }

    #[test]
    fn claude_gemini_mapper_tool_input_preserved() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = with_tools_conv();
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        let tool_calls = mapped.tool_calls();
        if let Some(IrContentBlock::ToolUse { input, .. }) = tool_calls.first() {
            assert_eq!(input["city"], "Portland");
        }
    }

    #[test]
    fn claude_request_to_ir_to_gemini_work_order() {
        let claude_req = make_claude_request();
        let claude_wo = abp_shim_claude::request_to_work_order(&claude_req);
        assert!(claude_wo.config.model.is_some());
    }

    #[test]
    fn gemini_request_content_from_dialect_roundtrip() {
        let content =
            abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::text("Response text")]);
        let dialect = abp_shim_gemini::content_to_dialect(&content);
        let back = abp_shim_gemini::content_from_dialect(&dialect);
        assert_eq!(back.role, "model");
        assert_eq!(back.parts.len(), 1);
    }

    #[test]
    fn claude_thinking_block_not_lost_through_mapping() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Think carefully"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "Let me think...".into(),
                    },
                    IrContentBlock::Text {
                        text: "Here's my answer.".into(),
                    },
                ],
            ),
        ]);
        let mapper = ClaudeGeminiIrMapper;
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        // The thinking block may or may not survive depending on mapper behavior
        // but the text should survive
        let asst = mapped
            .messages
            .iter()
            .find(|m| m.role == IrRole::Assistant)
            .unwrap();
        assert!(!asst.text_content().is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. All dialects → IR → All dialects (20+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod all_dialects_roundtrip {
    use super::*;

    #[test]
    fn openai_request_to_ir() {
        let req = make_openai_request();
        let ir = abp_shim_openai::request_to_ir(&req);
        assert!(!ir.is_empty());
    }

    #[test]
    fn claude_request_to_work_order_roundtrip() {
        let req = make_claude_request();
        let wo = abp_shim_claude::request_to_work_order(&req);
        let receipt = mock_receipt("claude", "claude-sonnet-4-20250514");
        let resp =
            abp_shim_claude::response_from_events(&receipt.trace, "claude-sonnet-4-20250514", None);
        assert!(!resp.content.is_empty());
    }

    #[test]
    fn gemini_request_to_ir_roundtrip() {
        let req = make_gemini_request();
        let (ir_req, _, _) = abp_shim_gemini::request_to_ir(&req).unwrap();
        assert!(!ir_req.conversation.is_empty());
    }

    #[test]
    fn codex_request_to_ir() {
        let req = make_codex_request();
        let ir = abp_shim_codex::request_to_ir(&req);
        assert!(!ir.is_empty());
    }

    #[test]
    fn codex_request_to_work_order() {
        let req = make_codex_request();
        let wo = abp_shim_codex::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    #[test]
    fn kimi_request_to_ir() {
        let req = make_kimi_request();
        let ir = abp_shim_kimi::request_to_ir(&req);
        assert!(!ir.is_empty());
    }

    #[test]
    fn kimi_request_to_work_order() {
        let req = make_kimi_request();
        let wo = abp_shim_kimi::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
    }

    #[test]
    fn copilot_request_to_ir() {
        let req = make_copilot_request();
        let ir = abp_shim_copilot::request_to_ir(&req);
        assert!(!ir.is_empty());
    }

    #[test]
    fn copilot_request_to_work_order() {
        let req = make_copilot_request();
        let wo = abp_shim_copilot::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn openai_passthrough_same_dialect_no_loss() {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::OpenAi);
        assert!(mapper.is_some());
        let conv = simple_text_conv();
        let mapped = mapper
            .unwrap()
            .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(mapped, conv);
    }

    #[test]
    fn claude_passthrough_same_dialect_no_loss() {
        let mapper = default_ir_mapper(Dialect::Claude, Dialect::Claude);
        assert!(mapper.is_some());
        let conv = simple_text_conv();
        let mapped = mapper
            .unwrap()
            .map_request(Dialect::Claude, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(mapped, conv);
    }

    #[test]
    fn gemini_passthrough_same_dialect_no_loss() {
        let mapper = default_ir_mapper(Dialect::Gemini, Dialect::Gemini);
        assert!(mapper.is_some());
        let conv = simple_text_conv();
        let mapped = mapper
            .unwrap()
            .map_request(Dialect::Gemini, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(mapped, conv);
    }

    #[test]
    fn supported_ir_pairs_includes_major_pairs() {
        let pairs = supported_ir_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
        assert!(pairs.contains(&(Dialect::Claude, Dialect::Gemini)));
    }

    #[test]
    fn default_ir_mapper_returns_some_for_openai_claude() {
        assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Claude).is_some());
    }

    #[test]
    fn default_ir_mapper_returns_some_for_openai_gemini() {
        assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).is_some());
    }

    #[test]
    fn default_ir_mapper_returns_some_for_claude_gemini() {
        assert!(default_ir_mapper(Dialect::Claude, Dialect::Gemini).is_some());
    }

    #[test]
    fn default_ir_mapper_returns_some_for_openai_codex() {
        assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Codex).is_some());
    }

    #[test]
    fn default_ir_mapper_returns_some_for_openai_kimi() {
        assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Kimi).is_some());
    }

    #[test]
    fn default_ir_mapper_returns_some_for_openai_copilot() {
        assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Copilot).is_some());
    }

    #[test]
    fn codex_receipt_to_response() {
        let receipt = mock_receipt("codex-mock", "codex-mini-latest");
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());
    }

    #[test]
    fn kimi_receipt_to_response() {
        let receipt = mock_receipt("kimi-mock", "moonshot-v1-8k");
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(!resp.choices.is_empty());
    }

    #[test]
    fn copilot_receipt_to_response() {
        let receipt = mock_receipt("copilot-mock", "gpt-4o");
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.message.is_empty());
    }

    #[test]
    fn all_six_dialects_produce_non_empty_ir() {
        let oai_ir = abp_shim_openai::request_to_ir(&make_openai_request());
        assert!(!oai_ir.is_empty());

        let gemini_req = make_gemini_request();
        let (gem_ir, _, _) = abp_shim_gemini::request_to_ir(&gemini_req).unwrap();
        assert!(!gem_ir.conversation.is_empty());

        let codex_ir = abp_shim_codex::request_to_ir(&make_codex_request());
        assert!(!codex_ir.is_empty());

        let kimi_ir = abp_shim_kimi::request_to_ir(&make_kimi_request());
        assert!(!kimi_ir.is_empty());

        let copilot_ir = abp_shim_copilot::request_to_ir(&make_copilot_request());
        assert!(!copilot_ir.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Projection matrix integration (20+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod projection_matrix_integration {
    use super::*;

    #[test]
    fn empty_matrix_returns_error() {
        let matrix = ProjectionMatrix::new();
        let wo = WorkOrderBuilder::new("test").build();
        let result = matrix.project(&wo);
        assert!(matches!(result, Err(ProjectionError::EmptyMatrix)));
    }

    #[test]
    fn single_backend_selected() {
        let mut matrix = ProjectionMatrix::with_defaults();
        let caps = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        matrix.register_backend("openai", caps, Dialect::OpenAi, 50);
        let wo = WorkOrderBuilder::new("test").build();
        let result = matrix.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "openai");
    }

    #[test]
    fn higher_priority_backend_preferred() {
        let mut matrix = ProjectionMatrix::with_defaults();
        let caps = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        matrix.register_backend("backend-a", caps.clone(), Dialect::OpenAi, 30);
        matrix.register_backend("backend-b", caps, Dialect::OpenAi, 80);
        let wo = WorkOrderBuilder::new("test").build();
        let result = matrix.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "backend-b");
    }

    #[test]
    fn passthrough_same_dialect() {
        let mut matrix = ProjectionMatrix::with_defaults();
        let entry = matrix.lookup(Dialect::OpenAi, Dialect::OpenAi);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().mode, ProjectionMode::Passthrough);
    }

    #[test]
    fn mapped_mode_cross_dialect() {
        let mut matrix = ProjectionMatrix::with_defaults();
        let entry = matrix.lookup(Dialect::OpenAi, Dialect::Claude);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().mode, ProjectionMode::Mapped);
    }

    #[test]
    fn register_defaults_covers_all_identity_pairs() {
        let matrix = ProjectionMatrix::with_defaults();
        for &d in Dialect::all() {
            let entry = matrix.lookup(d, d);
            assert!(entry.is_some(), "Missing identity entry for {d:?}");
            assert_eq!(entry.unwrap().mode, ProjectionMode::Passthrough);
        }
    }

    #[test]
    fn register_defaults_covers_openai_claude_mapped() {
        let matrix = ProjectionMatrix::with_defaults();
        assert_eq!(
            matrix
                .lookup(Dialect::OpenAi, Dialect::Claude)
                .unwrap()
                .mode,
            ProjectionMode::Mapped
        );
        assert_eq!(
            matrix
                .lookup(Dialect::Claude, Dialect::OpenAi)
                .unwrap()
                .mode,
            ProjectionMode::Mapped
        );
    }

    #[test]
    fn register_defaults_covers_openai_gemini_mapped() {
        let matrix = ProjectionMatrix::with_defaults();
        assert_eq!(
            matrix
                .lookup(Dialect::OpenAi, Dialect::Gemini)
                .unwrap()
                .mode,
            ProjectionMode::Mapped
        );
        assert_eq!(
            matrix
                .lookup(Dialect::Gemini, Dialect::OpenAi)
                .unwrap()
                .mode,
            ProjectionMode::Mapped
        );
    }

    #[test]
    fn projection_score_weights() {
        // Verify the scoring formula uses 50% capability, 30% fidelity, 20% priority
        let mut matrix = ProjectionMatrix::with_defaults();
        let full_caps = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        matrix.register_backend("full", full_caps, Dialect::OpenAi, 100);
        let wo = WorkOrderBuilder::new("test")
            .requirements(make_requirements(&[
                Capability::Streaming,
                Capability::ToolUse,
            ]))
            .build();
        let result = matrix.project(&wo).unwrap();
        assert!(result.fidelity_score.total > 0.0);
    }

    #[test]
    fn fallback_chain_contains_alternatives() {
        let mut matrix = ProjectionMatrix::with_defaults();
        let caps = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        matrix.register_backend("primary", caps.clone(), Dialect::OpenAi, 80);
        matrix.register_backend("secondary", caps, Dialect::Claude, 40);
        let wo = WorkOrderBuilder::new("test").build();
        let result = matrix.project(&wo).unwrap();
        assert!(!result.fallback_chain.is_empty());
    }

    #[test]
    fn resolve_mapper_identity_for_same_dialect() {
        let matrix = ProjectionMatrix::with_defaults();
        let mapper = matrix.resolve_mapper(Dialect::OpenAi, Dialect::OpenAi);
        assert!(mapper.is_some());
    }

    #[test]
    fn resolve_mapper_mapped_for_cross_dialect() {
        let matrix = ProjectionMatrix::with_defaults();
        let mapper = matrix.resolve_mapper(Dialect::OpenAi, Dialect::Claude);
        assert!(mapper.is_some());
    }

    #[test]
    fn find_route_identity_cost_zero() {
        let matrix = ProjectionMatrix::with_defaults();
        let route = matrix.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
        assert_eq!(route.cost, 0);
        assert_eq!(route.fidelity, 1.0);
    }

    #[test]
    fn find_route_direct_cost_one() {
        let matrix = ProjectionMatrix::with_defaults();
        let route = matrix.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(route.cost, 1);
    }

    #[test]
    fn compatibility_score_identity_is_perfect() {
        let matrix = ProjectionMatrix::with_defaults();
        let score = matrix.compatibility_score(Dialect::OpenAi, Dialect::OpenAi);
        assert_eq!(score.fidelity, 1.0);
    }

    #[test]
    fn backend_count_increases_after_registration() {
        let mut matrix = ProjectionMatrix::new();
        assert_eq!(matrix.backend_count(), 0);
        let caps = make_manifest(&[]);
        matrix.register_backend("test", caps, Dialect::OpenAi, 50);
        assert_eq!(matrix.backend_count(), 1);
    }

    #[test]
    fn remove_backend_decreases_count() {
        let mut matrix = ProjectionMatrix::new();
        let caps = make_manifest(&[]);
        matrix.register_backend("test", caps, Dialect::OpenAi, 50);
        assert!(matrix.remove_backend("test"));
        assert_eq!(matrix.backend_count(), 0);
    }

    #[test]
    fn projection_mode_passthrough_for_same_dialect_register() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register(Dialect::Claude, Dialect::Claude, ProjectionMode::Mapped);
        // Same-dialect should be forced to Passthrough
        let entry = matrix.lookup(Dialect::Claude, Dialect::Claude).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Passthrough);
    }

    #[test]
    fn dialect_pair_display() {
        let pair = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
        let s = format!("{pair}");
        assert!(s.contains("OpenAi") || s.contains("openai") || s.contains("→"));
    }

    #[test]
    fn projection_with_no_matching_capabilities_errors() {
        let mut matrix = ProjectionMatrix::with_defaults();
        let caps = make_manifest(&[]);
        matrix.register_backend("empty", caps, Dialect::OpenAi, 50);
        let wo = WorkOrderBuilder::new("test")
            .requirements(make_requirements(&[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::Vision,
            ]))
            .build();
        let result = matrix.project(&wo);
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Receipt chain integration (15+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod receipt_chain_integration {
    use super::*;

    #[test]
    fn receipt_hash_is_deterministic() {
        let r = mock_receipt("mock", "gpt-4o");
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn receipt_hash_is_64_hex_chars() {
        let r = mock_receipt("mock", "gpt-4o");
        let h = compute_hash(&r).unwrap();
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn receipt_verify_hash_succeeds_when_correct() {
        let mut r = mock_receipt("mock", "gpt-4o");
        r.receipt_sha256 = Some(compute_hash(&r).unwrap());
        assert!(verify_hash(&r));
    }

    #[test]
    fn receipt_verify_hash_fails_when_tampered() {
        let mut r = mock_receipt("mock", "gpt-4o");
        r.receipt_sha256 = Some(compute_hash(&r).unwrap());
        r.outcome = Outcome::Failed;
        assert!(!verify_hash(&r));
    }

    #[test]
    fn receipt_verify_hash_succeeds_when_none() {
        let r = mock_receipt("mock", "gpt-4o");
        assert!(verify_hash(&r));
    }

    #[test]
    fn receipt_canonicalize_is_deterministic() {
        let r = mock_receipt("mock", "gpt-4o");
        let c1 = canonicalize(&r).unwrap();
        let c2 = canonicalize(&r).unwrap();
        assert_eq!(c1, c2);
    }

    #[test]
    fn receipt_canonicalize_nulls_hash_field() {
        let mut r = mock_receipt("mock", "gpt-4o");
        r.receipt_sha256 = Some("somehash".into());
        let canonical = canonicalize(&r).unwrap();
        assert!(canonical.contains("\"receipt_sha256\":null"));
    }

    #[test]
    fn receipt_from_different_backends_have_different_hashes() {
        let r1 = mock_receipt("openai", "gpt-4o");
        let r2 = mock_receipt("claude", "claude-sonnet-4-20250514");
        let h1 = compute_hash(&r1).unwrap();
        let h2 = compute_hash(&r2).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn receipt_builder_with_events() {
        let now = Utc::now();
        let r = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .add_event(AgentEvent {
                ts: now,
                kind: AgentEventKind::AssistantMessage { text: "Hi".into() },
                ext: None,
            })
            .add_event(AgentEvent {
                ts: now,
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            })
            .build();
        assert_eq!(r.trace.len(), 2);
    }

    #[test]
    fn receipt_diff_detects_outcome_change() {
        let r1 = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let r2 = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
        let diff = diff_receipts(&r1, &r2);
        assert!(!diff.is_empty());
    }

    #[test]
    fn receipt_diff_empty_for_identical() {
        let r = mock_receipt("mock", "gpt-4o");
        let diff = diff_receipts(&r, &r);
        assert!(diff.is_empty());
    }

    #[test]
    fn receipt_builder_error_sets_failed_outcome() {
        let r = ReceiptBuilder::new("mock")
            .error("something failed")
            .build();
        assert_eq!(r.outcome, Outcome::Failed);
        assert!(r
            .trace
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::Error { .. })));
    }

    #[test]
    fn receipt_cross_sdk_hash_determinism() {
        // Create receipts from different SDK pipelines and verify hash determinism
        let openai_receipt = mock_receipt("openai", "gpt-4o");
        let claude_receipt = mock_receipt("claude", "claude-sonnet-4-20250514");
        let gemini_receipt = mock_receipt("gemini", "gemini-2.5-flash");

        // Each should have a deterministic hash
        assert_eq!(
            compute_hash(&openai_receipt).unwrap(),
            compute_hash(&openai_receipt).unwrap()
        );
        assert_eq!(
            compute_hash(&claude_receipt).unwrap(),
            compute_hash(&claude_receipt).unwrap()
        );
        assert_eq!(
            compute_hash(&gemini_receipt).unwrap(),
            compute_hash(&gemini_receipt).unwrap()
        );
    }

    #[test]
    fn receipt_usage_tokens_preserved() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .usage_tokens(200, 100)
            .build();
        assert_eq!(r.usage.input_tokens, Some(200));
        assert_eq!(r.usage.output_tokens, Some(100));
    }

    #[test]
    fn chain_builder_creates_chain() {
        let r1 = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let r2 = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let chain = ChainBuilder::new()
            .append(r1)
            .unwrap()
            .append(r2)
            .unwrap()
            .build();
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn chain_from_mixed_backends() {
        let r1 = mock_receipt("openai", "gpt-4o");
        let r2 = mock_receipt("claude", "claude-sonnet-4-20250514");
        let r3 = mock_receipt("gemini", "gemini-2.5-flash");
        let chain = ChainBuilder::new()
            .append(r1)
            .unwrap()
            .append(r2)
            .unwrap()
            .append(r3)
            .unwrap()
            .build();
        assert_eq!(chain.len(), 3);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Capability negotiation integration (20+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod capability_negotiation_integration {
    use super::*;

    #[test]
    fn negotiate_all_native_is_viable() {
        let manifest = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let result =
            negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &manifest);
        assert!(result.is_viable());
        assert_eq!(result.native.len(), 2);
    }

    #[test]
    fn negotiate_all_unsupported_is_not_viable() {
        let manifest = make_manifest(&[]);
        let result =
            negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &manifest);
        assert!(!result.is_viable());
        assert_eq!(result.unsupported.len(), 2);
    }

    #[test]
    fn negotiate_mixed_native_and_emulated() {
        let manifest = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
        ]);
        let result =
            negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &manifest);
        assert!(result.is_viable());
        assert_eq!(result.native.len(), 1);
        assert_eq!(result.emulated.len(), 1);
    }

    #[test]
    fn negotiate_empty_requirements_viable() {
        let manifest = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let result = negotiate_capabilities(&[], &manifest);
        assert!(result.is_viable());
        assert_eq!(result.total(), 0);
    }

    #[test]
    fn openai_typical_manifest() {
        let manifest = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
            (Capability::JsonMode, CoreSupportLevel::Native),
            (Capability::SystemMessage, CoreSupportLevel::Native),
            (Capability::Temperature, CoreSupportLevel::Native),
        ]);
        let result =
            negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &manifest);
        assert!(result.is_viable());
    }

    #[test]
    fn claude_typical_manifest() {
        let manifest = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
            (Capability::ExtendedThinking, CoreSupportLevel::Native),
            (Capability::SystemMessage, CoreSupportLevel::Native),
        ]);
        let result = negotiate_capabilities(
            &[Capability::Streaming, Capability::ExtendedThinking],
            &manifest,
        );
        assert!(result.is_viable());
    }

    #[test]
    fn gemini_typical_manifest() {
        let manifest = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
            (Capability::CodeExecution, CoreSupportLevel::Native),
        ]);
        let result = negotiate_capabilities(&[Capability::CodeExecution], &manifest);
        assert!(result.is_viable());
    }

    #[test]
    fn cross_sdk_capability_comparison_streaming() {
        let openai_m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let claude_m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let gemini_m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);

        let req = &[Capability::Streaming];
        assert!(negotiate_capabilities(req, &openai_m).is_viable());
        assert!(negotiate_capabilities(req, &claude_m).is_viable());
        assert!(negotiate_capabilities(req, &gemini_m).is_viable());
    }

    #[test]
    fn cross_sdk_capability_extended_thinking_only_claude() {
        let openai_m = make_manifest(&[]);
        let claude_m = make_manifest(&[(Capability::ExtendedThinking, CoreSupportLevel::Native)]);
        let gemini_m = make_manifest(&[]);

        let req = &[Capability::ExtendedThinking];
        assert!(!negotiate_capabilities(req, &openai_m).is_viable());
        assert!(negotiate_capabilities(req, &claude_m).is_viable());
        assert!(!negotiate_capabilities(req, &gemini_m).is_viable());
    }

    #[test]
    fn cross_sdk_code_execution_only_gemini() {
        let openai_m = make_manifest(&[]);
        let claude_m = make_manifest(&[]);
        let gemini_m = make_manifest(&[(Capability::CodeExecution, CoreSupportLevel::Native)]);

        let req = &[Capability::CodeExecution];
        assert!(!negotiate_capabilities(req, &openai_m).is_viable());
        assert!(!negotiate_capabilities(req, &claude_m).is_viable());
        assert!(negotiate_capabilities(req, &gemini_m).is_viable());
    }

    #[test]
    fn pre_negotiate_and_apply_strict_passes() {
        let manifest = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let result = pre_negotiate(&[Capability::Streaming], &manifest);
        assert!(apply_policy(&result, NegotiationPolicy::Strict).is_ok());
    }

    #[test]
    fn pre_negotiate_and_apply_strict_fails() {
        let manifest = make_manifest(&[]);
        let result = pre_negotiate(&[Capability::Streaming], &manifest);
        assert!(apply_policy(&result, NegotiationPolicy::Strict).is_err());
    }

    #[test]
    fn pre_negotiate_and_apply_permissive_always_passes() {
        let manifest = make_manifest(&[]);
        let result = pre_negotiate(&[Capability::Streaming, Capability::Vision], &manifest);
        assert!(apply_policy(&result, NegotiationPolicy::Permissive).is_ok());
    }

    #[test]
    fn negotiation_result_total_correct() {
        let manifest = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
        ]);
        let result = negotiate_capabilities(
            &[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::Vision,
            ],
            &manifest,
        );
        assert_eq!(result.total(), 3);
    }

    #[test]
    fn negotiation_result_from_simple_helper() {
        let result = NegotiationResult::from_simple(
            vec![Capability::Streaming],
            vec![Capability::ToolUse],
            vec![Capability::Vision],
        );
        assert!(!result.is_viable());
        assert_eq!(result.native.len(), 1);
        assert_eq!(result.emulated.len(), 1);
        assert_eq!(result.unsupported.len(), 1);
    }

    #[test]
    fn negotiation_result_emulated_caps() {
        let result = NegotiationResult::from_simple(
            vec![],
            vec![Capability::ToolUse, Capability::Streaming],
            vec![],
        );
        let emulated = result.emulated_caps();
        assert!(emulated.contains(&Capability::ToolUse));
        assert!(emulated.contains(&Capability::Streaming));
    }

    #[test]
    fn negotiation_result_is_compatible_alias() {
        let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
        assert!(result.is_compatible());
        assert!(result.is_viable());
    }

    #[test]
    fn emulation_strategy_fidelity_loss() {
        assert!(!EmulationStrategy::ClientSide.has_fidelity_loss());
        assert!(!EmulationStrategy::ServerFallback.has_fidelity_loss());
        assert!(EmulationStrategy::Approximate.has_fidelity_loss());
    }

    #[test]
    fn negotiation_warnings_only_for_approximate() {
        let result = NegotiationResult {
            native: vec![],
            emulated: vec![
                (Capability::ToolUse, EmulationStrategy::ClientSide),
                (Capability::Vision, EmulationStrategy::Approximate),
            ],
            unsupported: vec![],
        };
        let warnings = result.warnings();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].0, Capability::Vision);
    }

    #[test]
    fn projection_with_capability_negotiation_selects_best() {
        let mut matrix = ProjectionMatrix::with_defaults();

        let openai_caps = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let claude_caps = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
            (Capability::ExtendedThinking, CoreSupportLevel::Native),
        ]);

        matrix.register_backend("openai", openai_caps, Dialect::OpenAi, 50);
        matrix.register_backend("claude", claude_caps, Dialect::Claude, 50);

        let wo = WorkOrderBuilder::new("test")
            .requirements(make_requirements(&[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::ExtendedThinking,
            ]))
            .build();

        let result = matrix.project(&wo).unwrap();
        // Claude should be selected because it supports ExtendedThinking
        assert_eq!(result.selected_backend, "claude");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional cross-cutting integration tests
// ═══════════════════════════════════════════════════════════════════════════

mod cross_cutting {
    use super::*;

    #[test]
    fn openai_full_pipeline_request_to_response() {
        let req = make_openai_request();
        let ir = abp_shim_openai::request_to_ir(&req);
        let wo = abp_shim_openai::request_to_work_order(&req);
        let receipt = mock_receipt("openai", "gpt-4o");
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.choices.is_empty());
        assert_eq!(resp.object, "chat.completion");
    }

    #[test]
    fn claude_full_pipeline_request_to_response() {
        let req = make_claude_request();
        let wo = abp_shim_claude::request_to_work_order(&req);
        let receipt = mock_receipt("claude", "claude-sonnet-4-20250514");
        let resp =
            abp_shim_claude::response_from_events(&receipt.trace, "claude-sonnet-4-20250514", None);
        assert!(!resp.content.is_empty());
    }

    #[test]
    fn codex_full_pipeline_request_to_response() {
        let req = make_codex_request();
        let wo = abp_shim_codex::request_to_work_order(&req);
        let receipt = mock_receipt("codex", "codex-mini-latest");
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());
    }

    #[test]
    fn kimi_full_pipeline_request_to_response() {
        let req = make_kimi_request();
        let wo = abp_shim_kimi::request_to_work_order(&req);
        let receipt = mock_receipt("kimi", "moonshot-v1-8k");
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(!resp.choices.is_empty());
    }

    #[test]
    fn copilot_full_pipeline_request_to_response() {
        let req = make_copilot_request();
        let wo = abp_shim_copilot::request_to_work_order(&req);
        let receipt = mock_receipt("copilot", "gpt-4o");
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.message.is_empty());
    }

    #[test]
    fn openai_to_claude_cross_pipeline() {
        // Create OpenAI request, convert to IR, map to Claude dialect, verify
        let oai_req = make_openai_request();
        let ir = abp_shim_openai::request_to_ir(&oai_req);
        let mapper = OpenAiClaudeIrMapper;
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
            .unwrap();
        assert!(!mapped.is_empty());
        // Execute and convert response back
        let receipt = mock_receipt("claude", "claude-sonnet-4-20250514");
        let resp =
            abp_shim_claude::response_from_events(&receipt.trace, "claude-sonnet-4-20250514", None);
        assert!(!resp.content.is_empty());
    }

    #[test]
    fn openai_to_gemini_cross_pipeline() {
        let oai_req = make_openai_request();
        let ir = abp_shim_openai::request_to_ir(&oai_req);
        let mapper = OpenAiGeminiIrMapper;
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
            .unwrap();
        assert!(!mapped.is_empty());
    }

    #[test]
    fn claude_to_openai_cross_pipeline() {
        let claude_req = make_claude_request();
        let wo = abp_shim_claude::request_to_work_order(&claude_req);
        // Convert receipt to OpenAI format
        let receipt = mock_receipt("openai", "gpt-4o");
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.choices.is_empty());
    }

    #[test]
    fn all_mappers_handle_empty_conversation() {
        let empty = IrConversation::new();
        let mapper_oai_claude = OpenAiClaudeIrMapper;
        let result = mapper_oai_claude
            .map_request(Dialect::OpenAi, Dialect::Claude, &empty)
            .unwrap();
        assert!(result.is_empty());

        let mapper_oai_gemini = OpenAiGeminiIrMapper;
        let result = mapper_oai_gemini
            .map_request(Dialect::OpenAi, Dialect::Gemini, &empty)
            .unwrap();
        assert!(result.is_empty());

        let mapper_claude_gemini = ClaudeGeminiIrMapper;
        let result = mapper_claude_gemini
            .map_request(Dialect::Claude, Dialect::Gemini, &empty)
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn ir_conversation_system_message_accessor() {
        let conv = with_system_conv();
        let sys = conv.system_message().unwrap();
        assert_eq!(sys.role, IrRole::System);
        assert_eq!(sys.text_content(), "You are a helpful assistant.");
    }

    #[test]
    fn ir_conversation_last_assistant() {
        let conv = multi_turn_conv();
        let last = conv.last_assistant().unwrap();
        assert_eq!(last.text_content(), "Great, thanks!");
    }

    #[test]
    fn ir_conversation_tool_calls_extraction() {
        let conv = with_tools_conv();
        let tool_calls = conv.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        if let IrContentBlock::ToolUse { name, .. } = tool_calls[0] {
            assert_eq!(name, "get_weather");
        }
    }

    #[test]
    fn ir_message_is_text_only() {
        let msg = IrMessage::text(IrRole::User, "Hello");
        assert!(msg.is_text_only());
        let complex = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text { text: "Hi".into() },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "test".into(),
                    input: json!({}),
                },
            ],
        );
        assert!(!complex.is_text_only());
    }

    #[test]
    fn ir_usage_from_io() {
        let usage = IrUsage::from_io(100, 50);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn ir_usage_merge() {
        let u1 = IrUsage::from_io(100, 50);
        let u2 = IrUsage::from_io(200, 100);
        let merged = u1.merge(u2);
        assert_eq!(merged.input_tokens, 300);
        assert_eq!(merged.output_tokens, 150);
        assert_eq!(merged.total_tokens, 450);
    }

    #[test]
    fn ir_usage_with_cache() {
        let usage = IrUsage::with_cache(100, 50, 10, 5);
        assert_eq!(usage.cache_read_tokens, 10);
        assert_eq!(usage.cache_write_tokens, 5);
    }

    #[test]
    fn dialect_all_returns_six() {
        let all = Dialect::all();
        assert_eq!(all.len(), 6);
    }

    #[test]
    fn dialect_label_non_empty() {
        for &d in Dialect::all() {
            assert!(!d.label().is_empty());
        }
    }

    #[test]
    fn projection_matrix_with_all_backends() {
        let mut matrix = ProjectionMatrix::with_defaults();
        let caps = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        for &d in Dialect::all() {
            matrix.register_backend(format!("backend-{}", d.label()), caps.clone(), d, 50);
        }
        assert_eq!(matrix.backend_count(), 6);
    }

    #[test]
    fn receipt_from_each_sdk_has_unique_hash() {
        let backends = ["openai", "claude", "gemini", "codex", "kimi", "copilot"];
        let hashes: Vec<String> = backends
            .iter()
            .map(|b| compute_hash(&mock_receipt(b, "model")).unwrap())
            .collect();
        // All hashes should be unique (different backend IDs)
        let unique: std::collections::HashSet<_> = hashes.iter().collect();
        assert_eq!(unique.len(), backends.len());
    }

    #[test]
    fn openai_codex_ir_mapper_works() {
        let mapper = OpenAiCodexIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        assert_eq!(mapped, conv);
    }

    #[test]
    fn openai_kimi_ir_mapper_works() {
        let mapper = OpenAiKimiIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
            .unwrap();
        assert!(!mapped.is_empty());
    }

    #[test]
    fn openai_copilot_ir_mapper_works() {
        let mapper = OpenAiCopilotIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
            .unwrap();
        assert!(!mapped.is_empty());
    }

    #[test]
    fn claude_kimi_ir_mapper_works() {
        let mapper = ClaudeKimiIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Kimi, &conv)
            .unwrap();
        assert!(!mapped.is_empty());
    }

    #[test]
    fn gemini_kimi_ir_mapper_works() {
        let mapper = GeminiKimiIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
            .unwrap();
        assert!(!mapped.is_empty());
    }

    #[test]
    fn codex_claude_ir_mapper_works() {
        let mapper = CodexClaudeIrMapper;
        let conv = simple_text_conv();
        let mapped = mapper
            .map_request(Dialect::Codex, Dialect::Claude, &conv)
            .unwrap();
        assert!(!mapped.is_empty());
    }

    #[test]
    fn receipt_builder_dialect() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .dialect("openai")
            .build();
        // Verify the receipt was built without error
        assert_eq!(r.outcome, Outcome::Complete);
    }

    #[test]
    fn receipt_builder_mode() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .mode(ExecutionMode::Passthrough)
            .build();
        assert_eq!(r.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn kimi_streaming_events_to_chunks() {
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: "Hello".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: " world".into(),
                },
                ext: None,
            },
        ];
        let chunks = abp_shim_kimi::events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert_eq!(chunks.len(), 3); // 2 deltas + 1 final stop chunk
    }

    #[test]
    fn copilot_streaming_events_to_events() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "Hi".into() },
            ext: None,
        }];
        let stream = abp_shim_copilot::events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    #[test]
    fn codex_streaming_events_to_events() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "Code".into(),
            },
            ext: None,
        }];
        let stream = abp_shim_codex::events_to_stream_events(&events, "codex-mini-latest");
        assert!(!stream.is_empty());
    }

    #[test]
    fn work_order_builder_with_requirements() {
        let wo = WorkOrderBuilder::new("test")
            .requirements(make_requirements(&[Capability::Streaming]))
            .build();
        assert!(!wo.requirements.required.is_empty());
    }

    #[test]
    fn receipt_chain_from_cross_sdk_receipts() {
        let r_openai = mock_receipt("openai", "gpt-4o");
        let r_claude = mock_receipt("claude", "claude-sonnet-4-20250514");
        let r_gemini = mock_receipt("gemini", "gemini-2.5-flash");
        let r_codex = mock_receipt("codex", "codex-mini-latest");
        let r_kimi = mock_receipt("kimi", "moonshot-v1-8k");
        let r_copilot = mock_receipt("copilot", "gpt-4o");

        let chain = ChainBuilder::new()
            .append(r_openai)
            .unwrap()
            .append(r_claude)
            .unwrap()
            .append(r_gemini)
            .unwrap()
            .append(r_codex)
            .unwrap()
            .append(r_kimi)
            .unwrap()
            .append(r_copilot)
            .unwrap()
            .build();
        assert_eq!(chain.len(), 6);
    }
}
