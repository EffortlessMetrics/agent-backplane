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
//! Cross-dialect fidelity tests verifying the full translation pipeline for each SDK dialect pair.
//!
//! For each of the 6 SDK dialects (OpenAI, Claude, Gemini, Codex, Kimi, Copilot) we test:
//! 1. Request construction (SDK-native → IR) preserves all fields
//! 2. IR → WorkOrder → IR roundtrip preserves structure
//! 3. Cross-dialect translation preserves semantics
//! 4. Lossy translations are properly labeled with FidelityLabel
//! 5. Feature-specific fidelity: tool definitions, system messages, model names,
//!    token usage, streaming events

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, CapabilityRequirement,
    CapabilityRequirements, ExecutionMode, MinSupport, Receipt, SupportLevel as CoreSupportLevel,
    UsageNormalized, WorkOrderBuilder,
};
use abp_emulation::{
    EmulationEngine, EmulationReport, EmulationStrategy, FidelityLabel, compute_fidelity,
    default_strategy,
};
use chrono::Utc;
use serde_json::json;

use abp_capability::negotiate::{NegotiationPolicy, apply_policy, pre_negotiate};
use abp_capability::{
    CapabilityRegistry, claude_35_sonnet_manifest, codex_manifest, copilot_manifest,
    gemini_15_pro_manifest, kimi_manifest, negotiate_capabilities, openai_gpt4o_manifest,
};
use abp_dialect::Dialect;
use abp_mapper::{MapError, default_ir_mapper, supported_ir_pairs};
use abp_mapping::{Fidelity, MappingError, MappingRegistry, MappingRule};

// ═══════════════════════════════════════════════════════════════════════════
// Helper: build a canonical IR conversation for cross-dialect tests
// ═══════════════════════════════════════════════════════════════════════════

fn _canonical_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello, world!"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ])
}

fn _canonical_ir_with_tool() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read the file"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tool_1".into(),
                name: "read_file".into(),
                input: json!({"path": "src/main.rs"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tool_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

fn mock_receipt(events: Vec<AgentEvent>) -> Receipt {
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
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..Default::default()
        },
        trace: events,
        artifacts: vec![],
        verification: Default::default(),
        outcome: abp_core::Outcome::Complete,
        receipt_sha256: None,
    }
}

fn assistant_event(text: &str) -> AgentEvent {
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

// ═══════════════════════════════════════════════════════════════════════════
// Module 1: OpenAI request → IR → WorkOrder roundtrip
// ═══════════════════════════════════════════════════════════════════════════

mod openai {
    use super::*;
    use abp_shim_openai::{ChatCompletionRequest, Message, Role, Tool, Usage};

    #[test]
    fn request_to_ir_preserves_roles() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![
                Message::system("Be concise."),
                Message::user("Hello"),
                Message::assistant("Hi"),
            ])
            .build();

        let ir = abp_shim_openai::request_to_ir(&req);
        assert_eq!(ir.len(), 3);
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[0].text_content(), "Be concise.");
        assert_eq!(ir.messages[1].role, IrRole::User);
        assert_eq!(ir.messages[1].text_content(), "Hello");
        assert_eq!(ir.messages[2].role, IrRole::Assistant);
        assert_eq!(ir.messages[2].text_content(), "Hi");
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("test")])
            .temperature(0.7)
            .max_tokens(1024)
            .build();

        let wo = abp_shim_openai::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
        assert_eq!(
            wo.config.vendor.get("temperature"),
            Some(&serde_json::Value::from(0.7))
        );
        assert_eq!(
            wo.config.vendor.get("max_tokens"),
            Some(&serde_json::Value::from(1024))
        );
    }

    #[test]
    fn ir_roundtrip_preserves_messages() {
        let messages = vec![
            Message::system("System prompt"),
            Message::user("User message"),
            Message::assistant("Assistant reply"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&messages);
        let back = abp_shim_openai::ir_to_messages(&ir);

        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, Role::System);
        assert_eq!(back[0].content.as_deref(), Some("System prompt"));
        assert_eq!(back[1].role, Role::User);
        assert_eq!(back[1].content.as_deref(), Some("User message"));
        assert_eq!(back[2].role, Role::Assistant);
        assert_eq!(back[2].content.as_deref(), Some("Assistant reply"));
    }

    #[test]
    fn tool_definitions_to_ir() {
        let tools = vec![Tool::function(
            "search",
            "Search the web",
            json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        )];
        let ir_tools = abp_shim_openai::tools_to_ir(&tools);
        assert_eq!(ir_tools.len(), 1);
        assert_eq!(ir_tools[0].name, "search");
        assert_eq!(ir_tools[0].description, "Search the web");
        assert!(ir_tools[0].parameters.get("properties").is_some());
    }

    #[test]
    fn receipt_to_response_preserves_content() {
        let receipt = mock_receipt(vec![assistant_event("Hello from OpenAI!")]);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");

        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello from OpenAI!")
        );
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn receipt_with_tool_call_sets_finish_reason() {
        let receipt = mock_receipt(vec![tool_call_event(
            "read_file",
            "call_abc",
            json!({"path": "main.rs"}),
        )]);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");

        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "read_file");
    }

    #[test]
    fn usage_ir_roundtrip() {
        let ir = IrUsage::from_io(200, 100);
        let usage: Usage = abp_shim_openai::ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 200);
        assert_eq!(usage.completion_tokens, 100);
        assert_eq!(usage.total_tokens, 300);
    }

    #[test]
    fn stream_events_contain_deltas_and_stop() {
        let events = vec![delta_event("Hel"), delta_event("lo!")];
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");

        // 2 deltas + 1 final stop
        assert_eq!(stream.len(), 3);
        assert_eq!(stream[0].choices[0].delta.content.as_deref(), Some("Hel"));
        assert_eq!(stream[1].choices[0].delta.content.as_deref(), Some("lo!"));
        assert_eq!(stream[2].choices[0].finish_reason.as_deref(), Some("stop"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 2: Claude request → IR → WorkOrder roundtrip
// ═══════════════════════════════════════════════════════════════════════════

mod claude {
    use super::*;
    use abp_shim_claude::{ContentBlock, Message, MessageRequest, Role};

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

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = simple_request("Hello");
        let wo = abp_shim_claude::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn system_message_preserved_in_request() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".into(),
                }],
            }],
            system: Some("You are a helpful assistant.".into()),
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };

        let claude_req = abp_shim_claude::request_to_claude(&req);
        assert_eq!(
            claude_req.system.as_deref(),
            Some("You are a helpful assistant.")
        );
    }

    #[test]
    fn content_block_roundtrip_text() {
        let block = ContentBlock::Text {
            text: "Hello".into(),
        };
        let ir = abp_shim_claude::content_block_to_ir(&block);
        let back = abp_shim_claude::content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn content_block_roundtrip_tool_use() {
        let block = ContentBlock::ToolUse {
            id: "tool_1".into(),
            name: "read_file".into(),
            input: json!({"path": "test.rs"}),
        };
        let ir = abp_shim_claude::content_block_to_ir(&block);
        let back = abp_shim_claude::content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn content_block_roundtrip_thinking() {
        let block = ContentBlock::Thinking {
            thinking: "Let me think...".into(),
            signature: Some("sig_abc".into()),
        };
        let ir = abp_shim_claude::content_block_to_ir(&block);
        let back = abp_shim_claude::content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn response_from_events_preserves_text() {
        let events = vec![assistant_event("Hello from Claude!")];
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);

        assert_eq!(resp.role, "assistant");
        assert!(!resp.content.is_empty());
        match &resp.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello from Claude!"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn response_from_events_tool_call() {
        let events = vec![tool_call_event("search", "tool_1", json!({"q": "rust"}))];
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);

        assert!(
            resp.content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolUse { name, .. } if name == "search"))
        );
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn usage_preserved_in_response() {
        use abp_claude_sdk::dialect::ClaudeUsage;
        let usage = ClaudeUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(20),
        };
        let events = vec![assistant_event("hi")];
        let resp = abp_shim_claude::response_from_events(
            &events,
            "claude-sonnet-4-20250514",
            Some(&usage),
        );
        assert_eq!(resp.usage.input_tokens, 100);
        assert_eq!(resp.usage.output_tokens, 50);
        assert_eq!(resp.usage.cache_creation_input_tokens, Some(10));
        assert_eq!(resp.usage.cache_read_input_tokens, Some(20));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 3: Gemini request → IR → WorkOrder roundtrip
// ═══════════════════════════════════════════════════════════════════════════

mod gemini {
    use super::*;
    use abp_shim_gemini::{Content, GenerateContentRequest, Part, UsageMetadata};

    #[test]
    fn simple_request_to_ir_via_client() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]));

        let dialect_req = abp_shim_gemini::to_dialect_request(&req);
        let ir = abp_gemini_sdk::lowering::to_ir(
            &dialect_req.contents,
            dialect_req.system_instruction.as_ref(),
        );
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Hello");
    }

    #[test]
    fn system_instruction_preserved() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content {
                role: "user".into(),
                parts: vec![Part::text("You are helpful.")],
            })
            .add_content(Content::user(vec![Part::text("Hi")]));

        let dialect_req = abp_shim_gemini::to_dialect_request(&req);
        let ir = abp_gemini_sdk::lowering::to_ir(
            &dialect_req.contents,
            dialect_req.system_instruction.as_ref(),
        );
        // System instruction should appear as a system message
        assert!(ir.messages.iter().any(|m| m.role == IrRole::System));
    }

    #[test]
    fn multi_turn_ir_roundtrip() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Turn 1")]))
            .add_content(Content::model(vec![Part::text("Reply 1")]))
            .add_content(Content::user(vec![Part::text("Turn 2")]));

        let dialect_req = abp_shim_gemini::to_dialect_request(&req);
        let ir = abp_gemini_sdk::lowering::to_ir(
            &dialect_req.contents,
            dialect_req.system_instruction.as_ref(),
        );
        assert_eq!(ir.len(), 3);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[1].role, IrRole::Assistant);
        assert_eq!(ir.messages[2].role, IrRole::User);
    }

    #[test]
    fn function_call_in_ir() {
        let content = Content::model(vec![Part::function_call(
            "get_weather",
            json!({"location": "NYC"}),
        )]);

        let dialect = abp_shim_gemini::to_dialect_request(
            &GenerateContentRequest::new("gemini-2.5-flash").add_content(content),
        );
        let ir = abp_gemini_sdk::lowering::to_ir(&dialect.contents, None);
        assert_eq!(ir.len(), 1);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolUse { name, input, .. } => {
                assert_eq!(name, "get_weather");
                assert_eq!(input["location"], "NYC");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn usage_ir_roundtrip() {
        let usage = UsageMetadata {
            prompt_token_count: 50,
            candidates_token_count: 25,
            total_token_count: 75,
        };
        let ir = abp_shim_gemini::usage_to_ir(&usage);
        let back = abp_shim_gemini::usage_from_ir(&ir);
        assert_eq!(back, usage);
    }

    #[test]
    fn generation_config_roundtrip_via_dialect() {
        let dialect_cfg = abp_gemini_sdk::dialect::GeminiGenerationConfig {
            max_output_tokens: Some(2048),
            temperature: Some(0.5),
            top_p: Some(0.9),
            top_k: Some(40),
            candidate_count: None,
            stop_sequences: Some(vec!["END".into()]),
            response_mime_type: None,
            response_schema: None,
        };
        let shim_cfg = abp_shim_gemini::gen_config_from_dialect(&dialect_cfg);
        assert_eq!(shim_cfg.max_output_tokens, Some(2048));
        assert_eq!(shim_cfg.temperature, Some(0.5));
        assert_eq!(shim_cfg.stop_sequences, Some(vec!["END".into()]));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 4: Codex request → IR → WorkOrder roundtrip
// ═══════════════════════════════════════════════════════════════════════════

mod codex {
    use super::*;
    use abp_shim_codex::{CodexRequestBuilder, Usage, codex_message};

    #[test]
    fn request_to_ir_preserves_messages() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![
                codex_message("user", "Write hello world"),
                codex_message("assistant", "print('hello world')"),
            ])
            .build();

        let ir = abp_shim_codex::request_to_ir(&req);
        assert_eq!(ir.len(), 2);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Write hello world");
        assert_eq!(ir.messages[1].role, IrRole::Assistant);
    }

    #[test]
    fn request_to_work_order_preserves_model_and_config() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "test")])
            .temperature(0.3)
            .max_output_tokens(500)
            .build();

        let wo = abp_shim_codex::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
        assert_eq!(
            wo.config.vendor.get("temperature"),
            Some(&serde_json::Value::from(0.3))
        );
        assert_eq!(
            wo.config.vendor.get("max_output_tokens"),
            Some(&serde_json::Value::from(500))
        );
    }

    #[test]
    fn receipt_to_response_preserves_content() {
        let receipt = mock_receipt(vec![assistant_event("Hello from Codex!")]);
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");

        assert_eq!(resp.model, "codex-mini-latest");
        assert!(!resp.output.is_empty());
        assert_eq!(resp.status.as_deref(), Some("completed"));
    }

    #[test]
    fn receipt_with_tool_call() {
        let receipt = mock_receipt(vec![tool_call_event(
            "bash",
            "fc_1",
            json!({"command": "ls"}),
        )]);
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");

        let has_function_call = resp.output.iter().any(|item| {
            matches!(item, abp_codex_sdk::dialect::CodexResponseItem::FunctionCall { name, .. } if name == "bash")
        });
        assert!(has_function_call);
    }

    #[test]
    fn usage_ir_roundtrip() {
        let ir = IrUsage::from_io(150, 75);
        let usage: Usage = abp_shim_codex::ir_usage_to_usage(&ir);
        assert_eq!(usage.input_tokens, 150);
        assert_eq!(usage.output_tokens, 75);
        assert_eq!(usage.total_tokens, 225);
    }

    #[test]
    fn stream_events_bookend() {
        let events = vec![delta_event("chunk1")];
        let stream = abp_shim_codex::events_to_stream_events(&events, "codex-mini-latest");

        // created + delta + completed
        assert!(stream.len() >= 3);
        assert!(matches!(
            &stream[0],
            abp_codex_sdk::dialect::CodexStreamEvent::ResponseCreated { .. }
        ));
        assert!(matches!(
            stream.last().unwrap(),
            abp_codex_sdk::dialect::CodexStreamEvent::ResponseCompleted { .. }
        ));
    }

    #[test]
    fn response_to_ir_and_back() {
        let receipt = mock_receipt(vec![assistant_event("Hello")]);
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        let ir = abp_shim_codex::response_to_ir(&resp);

        assert!(!ir.is_empty());
        let items = abp_shim_codex::ir_to_response_items(&ir);
        assert!(!items.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 5: Kimi request → IR → WorkOrder roundtrip
// ═══════════════════════════════════════════════════════════════════════════

mod kimi {
    use super::*;
    use abp_shim_kimi::{KimiRequestBuilder, Message, Usage};

    #[test]
    fn request_to_ir_preserves_roles() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![
                Message::system("Be helpful."),
                Message::user("Hello"),
                Message::assistant("Hi!"),
            ])
            .build();

        let ir = abp_shim_kimi::request_to_ir(&req);
        assert_eq!(ir.len(), 3);
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[0].text_content(), "Be helpful.");
        assert_eq!(ir.messages[1].role, IrRole::User);
        assert_eq!(ir.messages[2].role, IrRole::Assistant);
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-32k")
            .messages(vec![Message::user("test")])
            .temperature(0.5)
            .max_tokens(2000)
            .build();

        let wo = abp_shim_kimi::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-32k"));
    }

    #[test]
    fn ir_roundtrip_preserves_messages() {
        let messages = vec![
            Message::system("System"),
            Message::user("User msg"),
            Message::assistant("Reply"),
        ];
        let ir = abp_shim_kimi::messages_to_ir(&messages);
        let back = abp_shim_kimi::ir_to_messages(&ir);

        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content.as_deref(), Some("System"));
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
    }

    #[test]
    fn receipt_to_response_preserves_content() {
        let receipt = mock_receipt(vec![assistant_event("Kimi says hello!")]);
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");

        assert_eq!(resp.model, "moonshot-v1-8k");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Kimi says hello!")
        );
    }

    #[test]
    fn usage_ir_roundtrip() {
        let ir = IrUsage::from_io(300, 150);
        let usage: Usage = abp_shim_kimi::ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 300);
        assert_eq!(usage.completion_tokens, 150);
        assert_eq!(usage.total_tokens, 450);
    }

    #[test]
    fn stream_chunks_end_with_stop() {
        let events = vec![delta_event("hi")];
        let chunks = abp_shim_kimi::events_to_stream_chunks(&events, "moonshot-v1-8k");

        assert!(chunks.len() >= 2);
        let last = chunks.last().unwrap();
        assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn response_to_ir_and_back() {
        let receipt = mock_receipt(vec![assistant_event("Response")]);
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        let ir = abp_shim_kimi::response_to_ir(&resp);

        assert!(!ir.is_empty());
        let msgs = abp_shim_kimi::ir_to_messages(&ir);
        assert!(!msgs.is_empty());
        assert_eq!(msgs[0].role, "assistant");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 6: Copilot request → IR → WorkOrder roundtrip
// ═══════════════════════════════════════════════════════════════════════════

mod copilot {
    use super::*;
    use abp_shim_copilot::{CopilotRequestBuilder, Message};

    #[test]
    fn request_to_ir_preserves_roles() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![
                Message::system("Be concise."),
                Message::user("Hello"),
                Message::assistant("Hi!"),
            ])
            .build();

        let ir = abp_shim_copilot::request_to_ir(&req);
        assert_eq!(ir.len(), 3);
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[0].text_content(), "Be concise.");
        assert_eq!(ir.messages[1].role, IrRole::User);
        assert_eq!(ir.messages[2].role, IrRole::Assistant);
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("test")])
            .build();

        let wo = abp_shim_copilot::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn ir_roundtrip_preserves_messages() {
        let messages = vec![
            Message::system("System prompt"),
            Message::user("User message"),
            Message::assistant("Assistant reply"),
        ];
        let ir = abp_shim_copilot::messages_to_ir(&messages);
        let back = abp_shim_copilot::ir_to_messages(&ir);

        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content, "System prompt");
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
    }

    #[test]
    fn receipt_to_response_preserves_content() {
        let receipt = mock_receipt(vec![assistant_event("Copilot reply!")]);
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");

        assert_eq!(resp.message, "Copilot reply!");
        assert!(resp.copilot_errors.is_empty());
    }

    #[test]
    fn receipt_with_tool_call() {
        let receipt = mock_receipt(vec![tool_call_event(
            "read_file",
            "call_xyz",
            json!({"path": "lib.rs"}),
        )]);
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");

        let fc = resp.function_call.unwrap();
        assert_eq!(fc.name, "read_file");
        assert_eq!(fc.id.as_deref(), Some("call_xyz"));
    }

    #[test]
    fn usage_ir_tuple() {
        let ir = IrUsage::from_io(200, 100);
        let (input, output, total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
        assert_eq!(input, 200);
        assert_eq!(output, 100);
        assert_eq!(total, 300);
    }

    #[test]
    fn response_to_ir_and_back() {
        let resp = abp_copilot_sdk::dialect::CopilotResponse {
            message: "Hello!".into(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: None,
        };
        let ir = abp_shim_copilot::response_to_ir(&resp);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::Assistant);

        let msgs = abp_shim_copilot::ir_to_messages(&ir);
        assert_eq!(msgs[0].content, "Hello!");
    }

    #[test]
    fn stream_events_include_done() {
        let events = vec![delta_event("hi")];
        let stream = abp_shim_copilot::events_to_stream_events(&events, "gpt-4o");

        assert!(matches!(
            stream.last().unwrap(),
            abp_copilot_sdk::dialect::CopilotStreamEvent::Done {}
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 7: Cross-dialect translation (A → IR → B → IR → A)
// ═══════════════════════════════════════════════════════════════════════════

mod cross_dialect {
    use super::*;

    #[test]
    fn openai_to_kimi_roundtrip() {
        let openai_msgs = vec![
            abp_shim_openai::Message::system("Be concise."),
            abp_shim_openai::Message::user("What is Rust?"),
            abp_shim_openai::Message::assistant("A systems language."),
        ];

        // OpenAI → IR
        let ir = abp_shim_openai::messages_to_ir(&openai_msgs);
        assert_eq!(ir.len(), 3);

        // IR → Kimi
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(kimi_msgs.len(), 3);
        assert_eq!(kimi_msgs[0].role, "system");
        assert_eq!(kimi_msgs[1].role, "user");
        assert_eq!(kimi_msgs[2].role, "assistant");

        // Kimi → IR
        let ir2 = abp_shim_kimi::messages_to_ir(&kimi_msgs);
        assert_eq!(ir2.len(), 3);

        // IR → OpenAI
        let openai_back = abp_shim_openai::ir_to_messages(&ir2);
        assert_eq!(openai_back.len(), 3);
        assert_eq!(openai_back[0].content.as_deref(), Some("Be concise."));
        assert_eq!(openai_back[1].content.as_deref(), Some("What is Rust?"));
        assert_eq!(
            openai_back[2].content.as_deref(),
            Some("A systems language.")
        );
    }

    #[test]
    fn openai_to_copilot_roundtrip() {
        let openai_msgs = vec![
            abp_shim_openai::Message::user("Hello"),
            abp_shim_openai::Message::assistant("Hi!"),
        ];

        // OpenAI → IR → Copilot
        let ir = abp_shim_openai::messages_to_ir(&openai_msgs);
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&ir);
        assert_eq!(copilot_msgs.len(), 2);
        assert_eq!(copilot_msgs[0].role, "user");
        assert_eq!(copilot_msgs[0].content, "Hello");
        assert_eq!(copilot_msgs[1].role, "assistant");
        assert_eq!(copilot_msgs[1].content, "Hi!");

        // Copilot → IR → OpenAI
        let ir2 = abp_shim_copilot::messages_to_ir(&copilot_msgs);
        let openai_back = abp_shim_openai::ir_to_messages(&ir2);
        assert_eq!(openai_back.len(), 2);
        assert_eq!(openai_back[0].content.as_deref(), Some("Hello"));
        assert_eq!(openai_back[1].content.as_deref(), Some("Hi!"));
    }

    #[test]
    fn kimi_to_copilot_roundtrip() {
        let kimi_msgs = vec![
            abp_shim_kimi::Message::system("System"),
            abp_shim_kimi::Message::user("Msg"),
        ];

        // Kimi → IR → Copilot
        let ir = abp_shim_kimi::messages_to_ir(&kimi_msgs);
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&ir);
        assert_eq!(copilot_msgs.len(), 2);
        assert_eq!(copilot_msgs[0].content, "System");
        assert_eq!(copilot_msgs[1].content, "Msg");

        // Copilot → IR → Kimi
        let ir2 = abp_shim_copilot::messages_to_ir(&copilot_msgs);
        let kimi_back = abp_shim_kimi::ir_to_messages(&ir2);
        assert_eq!(kimi_back.len(), 2);
        assert_eq!(kimi_back[0].content.as_deref(), Some("System"));
        assert_eq!(kimi_back[1].content.as_deref(), Some("Msg"));
    }

    #[test]
    fn openai_to_codex_via_ir() {
        // OpenAI messages → IR (include assistant message since Codex output
        // items only represent assistant/tool roles)
        let openai_msgs = vec![
            abp_shim_openai::Message::user("Write hello world in Python"),
            abp_shim_openai::Message::assistant("print('hello world')"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&openai_msgs);
        assert_eq!(ir.len(), 2);
        assert_eq!(ir.messages[0].text_content(), "Write hello world in Python");

        // IR → Codex response items (only assistant messages produce items)
        let items = abp_shim_codex::ir_to_response_items(&ir);
        assert!(!items.is_empty());
    }

    #[test]
    fn gemini_to_openai_via_ir() {
        let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(abp_shim_gemini::Content::user(vec![
                abp_shim_gemini::Part::text("Hi there"),
            ]))
            .add_content(abp_shim_gemini::Content::model(vec![
                abp_shim_gemini::Part::text("Hello!"),
            ]));

        let dialect_req = abp_shim_gemini::to_dialect_request(&req);
        let ir = abp_gemini_sdk::lowering::to_ir(
            &dialect_req.contents,
            dialect_req.system_instruction.as_ref(),
        );
        assert_eq!(ir.len(), 2);

        // IR → OpenAI
        let openai_msgs = abp_shim_openai::ir_to_messages(&ir);
        assert_eq!(openai_msgs.len(), 2);
        assert_eq!(openai_msgs[0].role, abp_shim_openai::Role::User);
        assert_eq!(openai_msgs[0].content.as_deref(), Some("Hi there"));
        assert_eq!(openai_msgs[1].role, abp_shim_openai::Role::Assistant);
        assert_eq!(openai_msgs[1].content.as_deref(), Some("Hello!"));
    }

    #[test]
    fn all_text_dialects_agree_on_user_content() {
        let text = "What is the meaning of life?";

        // Build IR from each dialect, verify they all produce the same user text
        let openai_ir = abp_shim_openai::messages_to_ir(&[abp_shim_openai::Message::user(text)]);
        let kimi_ir = abp_shim_kimi::messages_to_ir(&[abp_shim_kimi::Message::user(text)]);
        let copilot_ir = abp_shim_copilot::messages_to_ir(&[abp_shim_copilot::Message::user(text)]);

        // All should produce same IR
        assert_eq!(openai_ir.messages[0].text_content(), text);
        assert_eq!(kimi_ir.messages[0].text_content(), text);
        assert_eq!(copilot_ir.messages[0].text_content(), text);
        assert_eq!(openai_ir.messages[0].role, IrRole::User);
        assert_eq!(kimi_ir.messages[0].role, IrRole::User);
        assert_eq!(copilot_ir.messages[0].role, IrRole::User);
    }

    #[test]
    fn system_message_roundtrips_through_openai_kimi_copilot() {
        let system_text = "You are a helpful coding assistant.";

        // OpenAI → IR
        let ir = abp_shim_openai::messages_to_ir(&[
            abp_shim_openai::Message::system(system_text),
            abp_shim_openai::Message::user("Hi"),
        ]);
        assert_eq!(ir.messages[0].role, IrRole::System);

        // IR → Kimi → IR
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(kimi_msgs[0].role, "system");
        assert_eq!(kimi_msgs[0].content.as_deref(), Some(system_text));

        let ir2 = abp_shim_kimi::messages_to_ir(&kimi_msgs);
        assert_eq!(ir2.messages[0].role, IrRole::System);

        // IR → Copilot → IR
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&ir);
        assert_eq!(copilot_msgs[0].role, "system");
        assert_eq!(copilot_msgs[0].content, system_text);

        let ir3 = abp_shim_copilot::messages_to_ir(&copilot_msgs);
        assert_eq!(ir3.messages[0].role, IrRole::System);
        assert_eq!(ir3.messages[0].text_content(), system_text);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 8: FidelityLabel and lossy translation labeling
// ═══════════════════════════════════════════════════════════════════════════

mod fidelity {
    use super::*;

    #[test]
    fn native_capabilities_labeled_native() {
        let native = vec![Capability::Streaming, Capability::ToolRead];
        let report = EmulationReport {
            applied: vec![],
            warnings: vec![],
        };
        let labels = compute_fidelity(&native, &report);
        assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
        assert_eq!(labels[&Capability::ToolRead], FidelityLabel::Native);
    }

    #[test]
    fn emulated_capabilities_labeled_emulated() {
        let native = vec![Capability::Streaming];
        let report = EmulationReport {
            applied: vec![abp_emulation::EmulationEntry {
                capability: Capability::StructuredOutputJsonSchema,
                strategy: abp_emulation::emulate_structured_output(),
            }],
            warnings: vec![],
        };
        let labels = compute_fidelity(&native, &report);

        assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
        match &labels[&Capability::StructuredOutputJsonSchema] {
            FidelityLabel::Emulated { strategy } => {
                assert!(matches!(
                    strategy,
                    EmulationStrategy::SystemPromptInjection { .. }
                ));
            }
            other => panic!("expected Emulated, got {other:?}"),
        }
    }

    #[test]
    fn warnings_not_included_in_fidelity() {
        let native = vec![];
        let report = EmulationReport {
            applied: vec![],
            warnings: vec!["Cannot emulate checkpointing".into()],
        };
        let labels = compute_fidelity(&native, &report);
        // Warnings don't produce fidelity labels
        assert!(labels.is_empty());
    }

    #[test]
    fn emulation_engine_resolves_defaults() {
        let engine = EmulationEngine::with_defaults();

        // StructuredOutputJsonSchema should be emulatable
        let report = engine.check_missing(&[Capability::StructuredOutputJsonSchema]);
        // Should either apply or warn, but not panic
        assert!(report.applied.len() + report.warnings.len() > 0);
    }

    #[test]
    fn emulation_modifies_conversation_for_structured_output() {
        let engine = EmulationEngine::with_defaults();
        let mut conv =
            IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Give me JSON")]);

        let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
        // StructuredOutputJsonSchema uses PostProcessing strategy (not system
        // prompt injection), so the conversation is unchanged but the action
        // is recorded in the report.
        assert!(
            !report.applied.is_empty(),
            "Engine should record post-processing emulation"
        );
        assert!(matches!(
            report.applied[0].strategy,
            EmulationStrategy::PostProcessing { .. }
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 9: Feature-specific fidelity across dialects
// ═══════════════════════════════════════════════════════════════════════════

mod feature_fidelity {
    use super::*;

    #[test]
    fn tool_definition_ir_structure() {
        let ir_tool = IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"}
                },
                "required": ["path"]
            }),
        };

        // Verify all fields present
        assert_eq!(ir_tool.name, "read_file");
        assert_eq!(ir_tool.description, "Read a file from disk");
        assert!(ir_tool.parameters.get("properties").is_some());
        assert!(ir_tool.parameters["required"].as_array().unwrap().len() == 1);
    }

    #[test]
    fn openai_tool_to_ir_preserves_all_fields() {
        let tools = vec![abp_shim_openai::Tool::function(
            "get_weather",
            "Get weather for a location",
            json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"},
                    "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
                },
                "required": ["location"]
            }),
        )];

        let ir_tools = abp_shim_openai::tools_to_ir(&tools);
        assert_eq!(ir_tools[0].name, "get_weather");
        assert_eq!(ir_tools[0].description, "Get weather for a location");
        assert_eq!(
            ir_tools[0].parameters["properties"]["location"]["type"],
            "string"
        );
        assert_eq!(
            ir_tools[0].parameters["properties"]["unit"]["enum"][0],
            "celsius"
        );
    }

    #[test]
    fn model_name_fidelity_across_work_orders() {
        let models = [
            ("gpt-4o", "openai"),
            ("claude-sonnet-4-20250514", "claude"),
            ("gemini-2.5-flash", "gemini"),
            ("codex-mini-latest", "codex"),
            ("moonshot-v1-8k", "kimi"),
            ("gpt-4-turbo", "copilot"),
        ];

        for (model, dialect_name) in &models {
            let wo = WorkOrderBuilder::new("test task").model(*model).build();
            assert_eq!(
                wo.config.model.as_deref(),
                Some(*model),
                "Model name should be preserved for {dialect_name} dialect"
            );
        }
    }

    #[test]
    fn ir_usage_preserves_cache_fields() {
        let ir = IrUsage::with_cache(100, 50, 20, 10);
        assert_eq!(ir.input_tokens, 100);
        assert_eq!(ir.output_tokens, 50);
        assert_eq!(ir.total_tokens, 150);
        assert_eq!(ir.cache_read_tokens, 20);
        assert_eq!(ir.cache_write_tokens, 10);
    }

    #[test]
    fn receipt_usage_maps_to_all_dialects() {
        let receipt = mock_receipt(vec![assistant_event("ok")]);

        // OpenAI
        let openai_resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let openai_usage = openai_resp.usage.unwrap();
        assert_eq!(openai_usage.prompt_tokens, 100);
        assert_eq!(openai_usage.completion_tokens, 50);
        assert_eq!(openai_usage.total_tokens, 150);

        // Codex
        let codex_resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        let codex_usage = codex_resp.usage.unwrap();
        assert_eq!(codex_usage.input_tokens, 100);
        assert_eq!(codex_usage.output_tokens, 50);
        assert_eq!(codex_usage.total_tokens, 150);

        // Kimi
        let kimi_resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        let kimi_usage = kimi_resp.usage.unwrap();
        assert_eq!(kimi_usage.prompt_tokens, 100);
        assert_eq!(kimi_usage.completion_tokens, 50);
        assert_eq!(kimi_usage.total_tokens, 150);
    }

    #[test]
    fn streaming_event_structure_openai() {
        let events = vec![
            assistant_event("Hello"),
            tool_call_event("search", "tc_1", json!({"q": "rust"})),
        ];
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");

        // Should have: 1 text + 1 tool_call + 1 stop
        assert_eq!(stream.len(), 3);
        assert!(stream[0].choices[0].delta.content.is_some());
        assert!(stream[1].choices[0].delta.tool_calls.is_some());
        assert_eq!(stream[2].choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn streaming_event_structure_copilot() {
        let events = vec![
            delta_event("Hi"),
            tool_call_event("bash", "tc_1", json!({"cmd": "ls"})),
        ];
        let stream = abp_shim_copilot::events_to_stream_events(&events, "gpt-4o");

        // references + delta + function_call + done
        assert_eq!(stream.len(), 4);
        assert!(matches!(
            &stream[0],
            abp_copilot_sdk::dialect::CopilotStreamEvent::CopilotReferences { .. }
        ));
        assert!(matches!(
            &stream[1],
            abp_copilot_sdk::dialect::CopilotStreamEvent::TextDelta { .. }
        ));
        assert!(matches!(
            &stream[2],
            abp_copilot_sdk::dialect::CopilotStreamEvent::FunctionCall { .. }
        ));
        assert!(matches!(
            &stream[3],
            abp_copilot_sdk::dialect::CopilotStreamEvent::Done {}
        ));
    }

    #[test]
    fn streaming_event_structure_codex() {
        let events = vec![assistant_event("Code output")];
        let stream = abp_shim_codex::events_to_stream_events(&events, "codex-mini-latest");

        // created + item_done + completed
        assert!(stream.len() >= 3);
        assert!(matches!(
            &stream[0],
            abp_codex_sdk::dialect::CodexStreamEvent::ResponseCreated { .. }
        ));
        assert!(matches!(
            stream.last().unwrap(),
            abp_codex_sdk::dialect::CodexStreamEvent::ResponseCompleted { .. }
        ));
    }

    #[test]
    fn streaming_event_structure_kimi() {
        let events = vec![delta_event("Chunk1"), delta_event("Chunk2")];
        let chunks = abp_shim_kimi::events_to_stream_chunks(&events, "moonshot-v1-8k");

        // 2 deltas + 1 stop
        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks[0].choices[0].delta.content.as_deref(),
            Some("Chunk1")
        );
        assert_eq!(
            chunks[1].choices[0].delta.content.as_deref(),
            Some("Chunk2")
        );
        assert_eq!(chunks[2].choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn error_event_maps_across_dialects() {
        let err_event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "rate limit exceeded".into(),
                error_code: None,
            },
            ext: None,
        };

        // OpenAI: error becomes content
        let receipt = mock_receipt(vec![err_event.clone()]);
        let openai_resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert!(
            openai_resp.choices[0]
                .message
                .content
                .as_deref()
                .unwrap()
                .contains("rate limit")
        );

        // Copilot: error becomes copilot_errors
        let copilot_resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(copilot_resp.copilot_errors.len(), 1);
        assert!(
            copilot_resp.copilot_errors[0]
                .message
                .contains("rate limit")
        );

        // Codex: error becomes error message in output
        let codex_resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        let has_error_text = codex_resp.output.iter().any(|item| match item {
            abp_codex_sdk::dialect::CodexResponseItem::Message { content, .. } => {
                content.iter().any(|p| {
                    let abp_codex_sdk::dialect::CodexContentPart::OutputText { text } = p;
                    text.contains("rate limit")
                })
            }
            _ => false,
        });
        assert!(has_error_text);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 10: Multi-turn conversation fidelity
// ═══════════════════════════════════════════════════════════════════════════

mod multi_turn_fidelity {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn openai_multi_turn_roundtrip() {
        let msgs = vec![
            abp_shim_openai::Message::system("You are helpful."),
            abp_shim_openai::Message::user("What is 2+2?"),
            abp_shim_openai::Message::assistant("4"),
            abp_shim_openai::Message::user("And 3+3?"),
            abp_shim_openai::Message::assistant("6"),
        ];

        let ir = abp_shim_openai::messages_to_ir(&msgs);
        assert_eq!(ir.len(), 5);

        let back = abp_shim_openai::ir_to_messages(&ir);
        assert_eq!(back.len(), 5);
        assert_eq!(back[2].content.as_deref(), Some("4"));
        assert_eq!(back[4].content.as_deref(), Some("6"));
    }

    #[test]
    fn kimi_multi_turn_roundtrip() {
        let msgs = vec![
            abp_shim_kimi::Message::user("Question 1"),
            abp_shim_kimi::Message::assistant("Answer 1"),
            abp_shim_kimi::Message::user("Question 2"),
            abp_shim_kimi::Message::assistant("Answer 2"),
        ];

        let ir = abp_shim_kimi::messages_to_ir(&msgs);
        assert_eq!(ir.len(), 4);

        let back = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(back.len(), 4);
        assert_eq!(back[0].content.as_deref(), Some("Question 1"));
        assert_eq!(back[3].content.as_deref(), Some("Answer 2"));
    }

    #[test]
    fn copilot_multi_turn_roundtrip() {
        let msgs = vec![
            abp_shim_copilot::Message::user("Hi"),
            abp_shim_copilot::Message::assistant("Hello!"),
            abp_shim_copilot::Message::user("Bye"),
        ];

        let ir = abp_shim_copilot::messages_to_ir(&msgs);
        assert_eq!(ir.len(), 3);

        let back = abp_shim_copilot::ir_to_messages(&ir);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].content, "Hi");
        assert_eq!(back[2].content, "Bye");
    }

    #[test]
    fn multi_turn_cross_dialect_openai_kimi_copilot() {
        // Build a conversation in OpenAI format
        let openai_msgs = vec![
            abp_shim_openai::Message::system("Helpful bot"),
            abp_shim_openai::Message::user("Turn 1"),
            abp_shim_openai::Message::assistant("Reply 1"),
            abp_shim_openai::Message::user("Turn 2"),
        ];

        // OpenAI → IR
        let ir = abp_shim_openai::messages_to_ir(&openai_msgs);
        assert_eq!(ir.len(), 4);

        // IR → Kimi → IR
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir);
        let ir_via_kimi = abp_shim_kimi::messages_to_ir(&kimi_msgs);

        // IR → Copilot → IR
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&ir);
        let ir_via_copilot = abp_shim_copilot::messages_to_ir(&copilot_msgs);

        // All IRs should agree on message count and roles
        assert_eq!(ir_via_kimi.len(), 4);
        assert_eq!(ir_via_copilot.len(), 4);

        for i in 0..4 {
            assert_eq!(ir.messages[i].role, ir_via_kimi.messages[i].role);
            assert_eq!(ir.messages[i].role, ir_via_copilot.messages[i].role);
            assert_eq!(
                ir.messages[i].text_content(),
                ir_via_kimi.messages[i].text_content()
            );
            assert_eq!(
                ir.messages[i].text_content(),
                ir_via_copilot.messages[i].text_content()
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Helpers for mapped-mode / negotiation / IR tests
// ═══════════════════════════════════════════════════════════════════════════

fn manifest_for(dialect: Dialect) -> CapabilityManifest {
    match dialect {
        Dialect::OpenAi => openai_gpt4o_manifest(),
        Dialect::Claude => claude_35_sonnet_manifest(),
        Dialect::Gemini => gemini_15_pro_manifest(),
        Dialect::Codex => codex_manifest(),
        Dialect::Kimi => kimi_manifest(),
        Dialect::Copilot => copilot_manifest(),
    }
}

fn registry_key(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::OpenAi => "openai/gpt-4o",
        Dialect::Claude => "anthropic/claude-3.5-sonnet",
        Dialect::Gemini => "google/gemini-1.5-pro",
        Dialect::Codex => "openai/codex",
        Dialect::Kimi => "moonshot/kimi",
        Dialect::Copilot => "github/copilot",
    }
}

fn require_native(caps: &[Capability]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Native,
            })
            .collect(),
    }
}

fn require_emulated(caps: &[Capability]) -> CapabilityRequirements {
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

fn ir_simple() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello"),
    ])
}

fn ir_with_tools() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a coding assistant."),
        IrMessage::text(IrRole::User, "Read my file"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read_file".into(),
                input: json!({"path": "src/main.rs"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

fn ir_with_thinking() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is 2+2?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think... 2+2=4".into(),
                },
                IrContentBlock::Text {
                    text: "The answer is 4.".into(),
                },
            ],
        ),
    ])
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 11: Identity pair (passthrough) tests
// ═══════════════════════════════════════════════════════════════════════════

mod identity_passthrough {
    use super::*;

    #[test]
    fn identity_mapper_exists_for_all_dialects() {
        for &d in Dialect::all() {
            assert!(
                default_ir_mapper(d, d).is_some(),
                "identity mapper missing for {d}"
            );
        }
    }

    #[test]
    fn identity_preserves_simple_conversation() {
        for &d in Dialect::all() {
            let mapper = default_ir_mapper(d, d).unwrap();
            let conv = ir_simple();
            let mapped = mapper.map_request(d, d, &conv).unwrap();
            assert_eq!(conv, mapped, "identity altered conv for {d}");
        }
    }

    #[test]
    fn identity_preserves_tool_conversation() {
        for &d in Dialect::all() {
            let mapper = default_ir_mapper(d, d).unwrap();
            let conv = ir_with_tools();
            let mapped = mapper.map_request(d, d, &conv).unwrap();
            assert_eq!(conv, mapped, "identity altered tool conv for {d}");
        }
    }

    #[test]
    fn identity_response_preserves_conversation() {
        for &d in Dialect::all() {
            let mapper = default_ir_mapper(d, d).unwrap();
            let conv = ir_simple();
            let mapped = mapper.map_response(d, d, &conv).unwrap();
            assert_eq!(conv, mapped, "identity response altered conv for {d}");
        }
    }

    #[test]
    fn passthrough_preserves_thinking_blocks() {
        let conv = ir_with_thinking();
        for &d in Dialect::all() {
            let mapper = default_ir_mapper(d, d).unwrap();
            let mapped = mapper.map_request(d, d, &conv).unwrap();
            let has_thinking = mapped.messages.iter().any(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
            });
            assert!(has_thinking, "passthrough lost thinking for {d}");
        }
    }

    #[test]
    fn passthrough_preserves_metadata() {
        let mut conv = ir_simple();
        conv.messages[0]
            .metadata
            .insert("custom_key".into(), json!("custom_value"));
        for &d in Dialect::all() {
            let mapper = default_ir_mapper(d, d).unwrap();
            let mapped = mapper.map_request(d, d, &conv).unwrap();
            assert_eq!(
                mapped.messages[0].metadata.get("custom_key"),
                Some(&json!("custom_value")),
                "passthrough lost metadata for {d}"
            );
        }
    }

    #[test]
    fn execution_mode_default_is_mapped() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    }

    #[test]
    fn execution_mode_serde_roundtrip() {
        for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
            let j = serde_json::to_string(&mode).unwrap();
            let back: ExecutionMode = serde_json::from_str(&j).unwrap();
            assert_eq!(mode, back);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 12: Supported IR pairs matrix
// ═══════════════════════════════════════════════════════════════════════════

mod ir_pair_matrix {
    use super::*;

    #[test]
    fn supported_pairs_include_all_identity() {
        let pairs = supported_ir_pairs();
        for &d in Dialect::all() {
            assert!(pairs.contains(&(d, d)), "identity pair ({d},{d}) missing");
        }
    }

    #[test]
    fn supported_pairs_are_bidirectional() {
        let pairs = supported_ir_pairs();
        for &(from, to) in &pairs {
            if from != to {
                assert!(
                    pairs.contains(&(to, from)),
                    "({from},{to}) exists but ({to},{from}) missing"
                );
            }
        }
    }

    #[test]
    fn all_supported_pairs_have_mapper() {
        for &(from, to) in &supported_ir_pairs() {
            assert!(
                default_ir_mapper(from, to).is_some(),
                "no mapper for ({from},{to})"
            );
        }
    }

    #[test]
    fn at_least_18_non_identity_pairs() {
        let pairs = supported_ir_pairs();
        let non_id: Vec<_> = pairs.iter().filter(|(f, t)| f != t).collect();
        assert!(
            non_id.len() >= 18,
            "expected ≥18 non-identity, got {}",
            non_id.len()
        );
    }

    #[test]
    fn all_supported_pairs_map_simple_request() {
        let conv = ir_simple();
        for &(from, to) in &supported_ir_pairs() {
            let mapper = default_ir_mapper(from, to).unwrap();
            let result = mapper.map_request(from, to, &conv);
            assert!(result.is_ok(), "request mapping failed: {from}→{to}");
        }
    }

    #[test]
    fn all_supported_pairs_map_response() {
        let conv =
            IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Response.")]);
        for &(from, to) in &supported_ir_pairs() {
            let mapper = default_ir_mapper(from, to).unwrap();
            let result = mapper.map_response(from, to, &conv);
            assert!(
                result.is_ok(),
                "response mapping failed: {from}→{to}: {:?}",
                result.err()
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 13: Feature fidelity per dialect pair (capability negotiation)
// ═══════════════════════════════════════════════════════════════════════════

mod cap_fidelity_per_pair {
    use super::*;

    #[test]
    fn openai_to_claude_native_shared() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::OpenAi), registry_key(Dialect::Claude))
            .unwrap();
        assert!(r.native.contains(&Capability::Streaming));
        assert!(r.native.contains(&Capability::ToolUse));
        assert!(r.native.contains(&Capability::SystemMessage));
    }

    #[test]
    fn openai_to_claude_logprobs_unsupported() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::OpenAi), registry_key(Dialect::Claude))
            .unwrap();
        assert!(r.unsupported_caps().contains(&Capability::Logprobs));
    }

    #[test]
    fn openai_to_claude_frequency_penalty_unsupported() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::OpenAi), registry_key(Dialect::Claude))
            .unwrap();
        assert!(r.unsupported_caps().contains(&Capability::FrequencyPenalty));
    }

    #[test]
    fn openai_to_claude_audio_unsupported() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::OpenAi), registry_key(Dialect::Claude))
            .unwrap();
        assert!(r.unsupported_caps().contains(&Capability::Audio));
    }

    #[test]
    fn claude_to_openai_extended_thinking_unsupported() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::Claude), registry_key(Dialect::OpenAi))
            .unwrap();
        assert!(r.unsupported_caps().contains(&Capability::ExtendedThinking));
    }

    #[test]
    fn claude_to_openai_cache_control_unsupported() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::Claude), registry_key(Dialect::OpenAi))
            .unwrap();
        assert!(r.unsupported_caps().contains(&Capability::CacheControl));
    }

    #[test]
    fn claude_to_openai_native_streaming() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::Claude), registry_key(Dialect::OpenAi))
            .unwrap();
        assert!(r.native.contains(&Capability::Streaming));
    }

    #[test]
    fn gemini_to_openai_native_shared() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::Gemini), registry_key(Dialect::OpenAi))
            .unwrap();
        assert!(r.native.contains(&Capability::Streaming));
        assert!(r.native.contains(&Capability::ToolUse));
        assert!(r.native.contains(&Capability::FunctionCalling));
    }

    #[test]
    fn openai_to_gemini_logprobs_unsupported() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::OpenAi), registry_key(Dialect::Gemini))
            .unwrap();
        assert!(r.unsupported_caps().contains(&Capability::Logprobs));
    }

    #[test]
    fn openai_to_gemini_native_tool_use() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::OpenAi), registry_key(Dialect::Gemini))
            .unwrap();
        assert!(r.native.contains(&Capability::ToolUse));
        assert!(r.native.contains(&Capability::FunctionCalling));
    }

    #[test]
    fn codex_to_claude_native_streaming() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::Codex), registry_key(Dialect::Claude))
            .unwrap();
        assert!(r.native.contains(&Capability::Streaming));
        assert!(r.native.contains(&Capability::ToolUse));
    }

    #[test]
    fn codex_to_claude_logprobs_unsupported() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::Codex), registry_key(Dialect::Claude))
            .unwrap();
        assert!(r.unsupported_caps().contains(&Capability::Logprobs));
    }

    #[test]
    fn kimi_to_openai_native_shared() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::Kimi), registry_key(Dialect::OpenAi))
            .unwrap();
        assert!(r.native.contains(&Capability::Streaming));
        assert!(r.native.contains(&Capability::ToolUse));
        assert!(r.native.contains(&Capability::FrequencyPenalty));
        assert!(r.native.contains(&Capability::Temperature));
    }

    #[test]
    fn copilot_to_openai_native_features() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(
                registry_key(Dialect::Copilot),
                registry_key(Dialect::OpenAi),
            )
            .unwrap();
        assert!(r.native.contains(&Capability::Streaming));
        assert!(r.native.contains(&Capability::ToolUse));
    }

    #[test]
    fn openai_to_copilot_logprobs_unsupported() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(
                registry_key(Dialect::OpenAi),
                registry_key(Dialect::Copilot),
            )
            .unwrap();
        assert!(r.unsupported_caps().contains(&Capability::Logprobs));
    }

    #[test]
    fn gemini_to_claude_code_execution_emulated() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::Gemini), registry_key(Dialect::Claude))
            .unwrap();
        assert!(r.emulated_caps().contains(&Capability::CodeExecution));
    }

    #[test]
    fn codex_to_gemini_code_execution_native() {
        let reg = CapabilityRegistry::with_defaults();
        let r = reg
            .compare(registry_key(Dialect::Codex), registry_key(Dialect::Gemini))
            .unwrap();
        assert!(r.native.contains(&Capability::CodeExecution));
    }

    #[test]
    fn all_pairs_negotiable() {
        let reg = CapabilityRegistry::with_defaults();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                if src == tgt {
                    continue;
                }
                assert!(
                    reg.compare(registry_key(src), registry_key(tgt)).is_some(),
                    "negotiation failed for {src}→{tgt}"
                );
            }
        }
    }

    #[test]
    fn self_negotiation_no_unsupported() {
        let reg = CapabilityRegistry::with_defaults();
        for &d in Dialect::all() {
            let key = registry_key(d);
            let r = reg.compare(key, key).unwrap();
            assert!(
                r.unsupported.is_empty(),
                "self-negotiation for {d} has unsupported"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 14: Specific IR mapping tests
// ═══════════════════════════════════════════════════════════════════════════

mod ir_mapping_specific {
    use super::*;

    #[test]
    fn openai_to_claude_tool_conv() {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
        let result = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &ir_with_tools());
        assert!(result.is_ok());
    }

    #[test]
    fn claude_to_openai_thinking_block() {
        let mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
        let result = mapper.map_request(Dialect::Claude, Dialect::OpenAi, &ir_with_thinking());
        assert!(result.is_ok());
    }

    #[test]
    fn openai_to_gemini_tool_conv() {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).unwrap();
        let result = mapper.map_request(Dialect::OpenAi, Dialect::Gemini, &ir_with_tools());
        assert!(result.is_ok());
    }

    #[test]
    fn codex_to_claude_simple() {
        let mapper = default_ir_mapper(Dialect::Codex, Dialect::Claude).unwrap();
        assert!(
            mapper
                .map_request(Dialect::Codex, Dialect::Claude, &ir_simple())
                .is_ok()
        );
    }

    #[test]
    fn codex_to_openai_simple() {
        let mapper = default_ir_mapper(Dialect::Codex, Dialect::OpenAi).unwrap();
        assert!(
            mapper
                .map_request(Dialect::Codex, Dialect::OpenAi, &ir_simple())
                .is_ok()
        );
    }

    #[test]
    fn kimi_to_openai_simple() {
        let mapper = default_ir_mapper(Dialect::Kimi, Dialect::OpenAi).unwrap();
        assert!(
            mapper
                .map_request(Dialect::Kimi, Dialect::OpenAi, &ir_simple())
                .is_ok()
        );
    }

    #[test]
    fn openai_to_kimi_simple() {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Kimi).unwrap();
        assert!(
            mapper
                .map_request(Dialect::OpenAi, Dialect::Kimi, &ir_simple())
                .is_ok()
        );
    }

    #[test]
    fn copilot_to_openai_simple() {
        let mapper = default_ir_mapper(Dialect::Copilot, Dialect::OpenAi).unwrap();
        assert!(
            mapper
                .map_request(Dialect::Copilot, Dialect::OpenAi, &ir_simple())
                .is_ok()
        );
    }

    #[test]
    fn openai_to_copilot_simple() {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Copilot).unwrap();
        assert!(
            mapper
                .map_request(Dialect::OpenAi, Dialect::Copilot, &ir_simple())
                .is_ok()
        );
    }

    #[test]
    fn claude_to_gemini_simple() {
        let mapper = default_ir_mapper(Dialect::Claude, Dialect::Gemini).unwrap();
        assert!(
            mapper
                .map_request(Dialect::Claude, Dialect::Gemini, &ir_simple())
                .is_ok()
        );
    }

    #[test]
    fn gemini_to_claude_simple() {
        let mapper = default_ir_mapper(Dialect::Gemini, Dialect::Claude).unwrap();
        assert!(
            mapper
                .map_request(Dialect::Gemini, Dialect::Claude, &ir_simple())
                .is_ok()
        );
    }

    #[test]
    fn claude_to_kimi_simple() {
        let mapper = default_ir_mapper(Dialect::Claude, Dialect::Kimi).unwrap();
        assert!(
            mapper
                .map_request(Dialect::Claude, Dialect::Kimi, &ir_simple())
                .is_ok()
        );
    }

    #[test]
    fn kimi_to_claude_simple() {
        let mapper = default_ir_mapper(Dialect::Kimi, Dialect::Claude).unwrap();
        assert!(
            mapper
                .map_request(Dialect::Kimi, Dialect::Claude, &ir_simple())
                .is_ok()
        );
    }

    #[test]
    fn gemini_to_kimi_simple() {
        let mapper = default_ir_mapper(Dialect::Gemini, Dialect::Kimi).unwrap();
        assert!(
            mapper
                .map_request(Dialect::Gemini, Dialect::Kimi, &ir_simple())
                .is_ok()
        );
    }

    #[test]
    fn kimi_to_gemini_simple() {
        let mapper = default_ir_mapper(Dialect::Kimi, Dialect::Gemini).unwrap();
        assert!(
            mapper
                .map_request(Dialect::Kimi, Dialect::Gemini, &ir_simple())
                .is_ok()
        );
    }

    #[test]
    fn roundtrip_openai_claude() {
        let fwd = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
        let rev = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
        let conv = ir_simple();
        let mapped = fwd
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let rt = rev
            .map_request(Dialect::Claude, Dialect::OpenAi, &mapped)
            .unwrap();
        assert_eq!(conv.messages.len(), rt.messages.len());
    }

    #[test]
    fn roundtrip_openai_gemini() {
        let fwd = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).unwrap();
        let rev = default_ir_mapper(Dialect::Gemini, Dialect::OpenAi).unwrap();
        let conv = ir_simple();
        let mapped = fwd
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        let rt = rev
            .map_request(Dialect::Gemini, Dialect::OpenAi, &mapped)
            .unwrap();
        assert_eq!(conv.messages.len(), rt.messages.len());
    }

    #[test]
    fn roundtrip_openai_kimi() {
        let fwd = default_ir_mapper(Dialect::OpenAi, Dialect::Kimi).unwrap();
        let rev = default_ir_mapper(Dialect::Kimi, Dialect::OpenAi).unwrap();
        let conv = ir_simple();
        let mapped = fwd
            .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
            .unwrap();
        let rt = rev
            .map_request(Dialect::Kimi, Dialect::OpenAi, &mapped)
            .unwrap();
        assert_eq!(conv.messages.len(), rt.messages.len());
    }

    #[test]
    fn roundtrip_claude_gemini() {
        let fwd = default_ir_mapper(Dialect::Claude, Dialect::Gemini).unwrap();
        let rev = default_ir_mapper(Dialect::Gemini, Dialect::Claude).unwrap();
        let conv = ir_simple();
        let mapped = fwd
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        let rt = rev
            .map_request(Dialect::Gemini, Dialect::Claude, &mapped)
            .unwrap();
        assert_eq!(conv.messages.len(), rt.messages.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 15: Emulation labeling
// ═══════════════════════════════════════════════════════════════════════════

mod emulation_labeling {
    use super::*;

    #[test]
    fn extended_thinking_uses_system_prompt_injection() {
        let strategy = default_strategy(&Capability::ExtendedThinking);
        assert!(matches!(
            strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn structured_output_uses_post_processing() {
        let strategy = default_strategy(&Capability::StructuredOutputJsonSchema);
        assert!(matches!(strategy, EmulationStrategy::PostProcessing { .. }));
    }

    #[test]
    fn code_execution_is_disabled() {
        let strategy = default_strategy(&Capability::CodeExecution);
        assert!(matches!(strategy, EmulationStrategy::Disabled { .. }));
    }

    #[test]
    fn image_input_uses_system_prompt_injection() {
        let strategy = default_strategy(&Capability::ImageInput);
        assert!(matches!(
            strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn stop_sequences_uses_post_processing() {
        let strategy = default_strategy(&Capability::StopSequences);
        assert!(matches!(strategy, EmulationStrategy::PostProcessing { .. }));
    }

    #[test]
    fn engine_labels_applied_capabilities() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = ir_simple();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
        assert!(!report.applied.is_empty());
        assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
        assert!(matches!(
            report.applied[0].strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn engine_warns_on_disabled() {
        let engine = EmulationEngine::with_defaults();
        let report = engine.check_missing(&[Capability::CodeExecution]);
        assert!(report.has_unemulatable());
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn compute_fidelity_native_and_emulated() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = ir_simple();
        let report = engine.apply(
            &[Capability::ExtendedThinking, Capability::StopSequences],
            &mut conv,
        );
        let labels = compute_fidelity(&[Capability::Streaming], &report);
        assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
        assert!(matches!(
            labels.get(&Capability::ExtendedThinking),
            Some(FidelityLabel::Emulated { .. })
        ));
    }

    #[test]
    fn emulation_report_includes_method_string() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = ir_simple();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
        match &report.applied[0].strategy {
            EmulationStrategy::SystemPromptInjection { prompt } => {
                assert!(!prompt.is_empty());
            }
            other => panic!("expected SystemPromptInjection, got {other:?}"),
        }
    }

    #[test]
    fn engine_applies_multiple_strategies() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = ir_simple();
        let caps = vec![
            Capability::ExtendedThinking,
            Capability::ImageInput,
            Capability::StopSequences,
        ];
        let report = engine.apply(&caps, &mut conv);
        assert_eq!(report.applied.len(), 3);
    }

    #[test]
    fn emulation_modifies_system_prompt() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = ir_simple();
        let original = conv.system_message().unwrap().text_content();
        engine.apply(&[Capability::ExtendedThinking], &mut conv);
        let modified = conv.system_message().unwrap().text_content();
        assert!(modified.len() > original.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 16: Early failure
// ═══════════════════════════════════════════════════════════════════════════

mod early_failure {
    use super::*;

    #[test]
    fn native_logprobs_on_claude_unsupported() {
        let manifest = claude_35_sonnet_manifest();
        let caps: Vec<_> = require_native(&[Capability::Logprobs])
            .required
            .iter()
            .map(|r| r.capability.clone())
            .collect();
        let r = negotiate_capabilities(&caps, &manifest);
        assert!(r.unsupported_caps().contains(&Capability::Logprobs));
    }

    #[test]
    fn native_extended_thinking_on_openai_unsupported() {
        let manifest = openai_gpt4o_manifest();
        let caps: Vec<_> = require_native(&[Capability::ExtendedThinking])
            .required
            .iter()
            .map(|r| r.capability.clone())
            .collect();
        let r = negotiate_capabilities(&caps, &manifest);
        assert!(r.unsupported_caps().contains(&Capability::ExtendedThinking));
    }

    #[test]
    fn strict_policy_rejects_unsupported() {
        let manifest = claude_35_sonnet_manifest();
        let r = pre_negotiate(&[Capability::Logprobs], &manifest);
        let pr = apply_policy(&r, NegotiationPolicy::Strict);
        assert!(pr.is_err());
        assert!(!pr.unwrap_err().unsupported.is_empty());
    }

    #[test]
    fn permissive_policy_allows_unsupported() {
        let manifest = claude_35_sonnet_manifest();
        let r = pre_negotiate(&[Capability::Logprobs], &manifest);
        assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
    }

    #[test]
    fn best_effort_rejects_unsupported() {
        let manifest = claude_35_sonnet_manifest();
        let r = pre_negotiate(&[Capability::Logprobs], &manifest);
        assert!(apply_policy(&r, NegotiationPolicy::BestEffort).is_err());
    }

    #[test]
    fn error_identifies_capability() {
        let manifest = openai_gpt4o_manifest();
        let r = pre_negotiate(&[Capability::ExtendedThinking], &manifest);
        let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
        let cap_names: Vec<_> = err.unsupported.iter().map(|(c, _)| c.clone()).collect();
        assert!(cap_names.contains(&Capability::ExtendedThinking));
    }

    #[test]
    fn multiple_unsupported_all_listed() {
        let manifest = claude_35_sonnet_manifest();
        let r = pre_negotiate(
            &[Capability::Logprobs, Capability::FrequencyPenalty],
            &manifest,
        );
        let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
        assert!(err.unsupported.len() >= 2);
    }

    #[test]
    fn native_min_rejects_emulated() {
        let manifest = claude_35_sonnet_manifest();
        let reqs = require_native(&[Capability::FunctionCalling]);
        let r = abp_capability::negotiate(&manifest, &reqs);
        assert!(!r.is_viable());
    }

    #[test]
    fn emulated_min_accepts_emulated() {
        let manifest = claude_35_sonnet_manifest();
        let reqs = require_emulated(&[Capability::FunctionCalling]);
        let r = abp_capability::negotiate(&manifest, &reqs);
        assert!(r.is_viable());
    }

    #[test]
    fn native_on_gemini_logprobs_unsupported() {
        let manifest = gemini_15_pro_manifest();
        let r = pre_negotiate(&[Capability::Logprobs], &manifest);
        assert!(r.unsupported_caps().contains(&Capability::Logprobs));
    }

    #[test]
    fn native_on_copilot_logprobs_unsupported() {
        let manifest = copilot_manifest();
        let r = pre_negotiate(&[Capability::Logprobs], &manifest);
        assert!(r.unsupported_caps().contains(&Capability::Logprobs));
    }

    #[test]
    fn native_on_kimi_extended_thinking_unsupported() {
        let manifest = kimi_manifest();
        let r = pre_negotiate(&[Capability::ExtendedThinking], &manifest);
        assert!(r.unsupported_caps().contains(&Capability::ExtendedThinking));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 17: Mapping registry / fidelity matrix
// ═══════════════════════════════════════════════════════════════════════════

mod mapping_registry_tests {
    use super::*;

    #[test]
    fn lossless_tool_use_rule() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn unsupported_logprobs_rule() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "logprobs".into(),
            fidelity: Fidelity::Unsupported {
                reason: "Claude does not support logprobs".into(),
            },
        });
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, "logprobs")
            .unwrap();
        assert!(rule.fidelity.is_unsupported());
    }

    #[test]
    fn lossy_labeled_rule() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            feature: "extended_thinking".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "thinking blocks stripped".into(),
            },
        });
        let rule = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, "extended_thinking")
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
        assert!(!rule.fidelity.is_unsupported());
    }

    #[test]
    fn rank_targets_prefers_lossless() {
        let mut reg = MappingRegistry::new();
        for &target in Dialect::all() {
            if target == Dialect::OpenAi {
                continue;
            }
            reg.insert(MappingRule {
                source_dialect: Dialect::OpenAi,
                target_dialect: target,
                feature: "streaming".into(),
                fidelity: Fidelity::Lossless,
            });
        }
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        let ranked = reg.rank_targets(Dialect::OpenAi, &["streaming", "tool_use"]);
        assert!(!ranked.is_empty());
        assert_eq!(ranked[0].0, Dialect::Claude);
        assert_eq!(ranked[0].1, 2);
    }

    #[test]
    fn fidelity_serde_roundtrip() {
        let variants = vec![
            Fidelity::Lossless,
            Fidelity::LossyLabeled {
                warning: "degraded".into(),
            },
            Fidelity::Unsupported {
                reason: "n/a".into(),
            },
        ];
        for f in &variants {
            let j = serde_json::to_string(f).unwrap();
            let back: Fidelity = serde_json::from_str(&j).unwrap();
            assert_eq!(f, &back);
        }
    }

    #[test]
    fn mapping_error_feature_unsupported_display() {
        let err = MappingError::FeatureUnsupported {
            feature: "logprobs".into(),
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        };
        assert!(err.to_string().contains("logprobs"));
    }

    #[test]
    fn mapping_error_fidelity_loss_display() {
        let err = MappingError::FidelityLoss {
            feature: "extended_thinking".into(),
            warning: "thinking blocks stripped".into(),
        };
        assert!(err.to_string().contains("extended_thinking"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 18: MapError handling
// ═══════════════════════════════════════════════════════════════════════════

mod map_error_handling {
    use super::*;

    #[test]
    fn unsupported_pair_display() {
        let err = MapError::UnsupportedPair {
            from: Dialect::Kimi,
            to: Dialect::Copilot,
        };
        assert!(err.to_string().contains("Kimi"));
        assert!(err.to_string().contains("Copilot"));
    }

    #[test]
    fn lossy_conversion_display() {
        let err = MapError::LossyConversion {
            field: "thinking".into(),
            reason: "target has no thinking block".into(),
        };
        assert!(err.to_string().contains("thinking"));
    }

    #[test]
    fn incompatible_capability_display() {
        let err = MapError::IncompatibleCapability {
            capability: "logprobs".into(),
            reason: "target dialect does not support logprobs".into(),
        };
        assert!(err.to_string().contains("logprobs"));
    }

    #[test]
    fn unmappable_content_display() {
        let err = MapError::UnmappableContent {
            field: "system".into(),
            reason: "image blocks in system prompt".into(),
        };
        assert!(err.to_string().contains("system"));
    }

    #[test]
    fn map_error_serde_roundtrip() {
        let errors = vec![
            MapError::UnsupportedPair {
                from: Dialect::OpenAi,
                to: Dialect::Claude,
            },
            MapError::LossyConversion {
                field: "thinking".into(),
                reason: "stripped".into(),
            },
            MapError::UnmappableTool {
                name: "bash".into(),
                reason: "restricted".into(),
            },
            MapError::IncompatibleCapability {
                capability: "vision".into(),
                reason: "no image support".into(),
            },
            MapError::UnmappableContent {
                field: "system".into(),
                reason: "images in system".into(),
            },
        ];
        for err in &errors {
            let j = serde_json::to_string(err).unwrap();
            let back: MapError = serde_json::from_str(&j).unwrap();
            assert_eq!(err, &back);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 19: MinSupport / SupportLevel correctness
// ═══════════════════════════════════════════════════════════════════════════

mod support_level_tests {
    use super::*;

    #[test]
    fn native_satisfies_both() {
        let n = CoreSupportLevel::Native;
        assert!(n.satisfies(&MinSupport::Native));
        assert!(n.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn emulated_satisfies_emulated_only() {
        let e = CoreSupportLevel::Emulated;
        assert!(!e.satisfies(&MinSupport::Native));
        assert!(e.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn unsupported_satisfies_neither() {
        let u = CoreSupportLevel::Unsupported;
        assert!(!u.satisfies(&MinSupport::Native));
        assert!(!u.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn restricted_satisfies_emulated() {
        let r = CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        };
        assert!(!r.satisfies(&MinSupport::Native));
        assert!(r.satisfies(&MinSupport::Emulated));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 20: Manifest assertions
// ═══════════════════════════════════════════════════════════════════════════

mod manifest_assertions {
    use super::*;

    #[test]
    fn all_manifests_have_streaming_native() {
        for &d in Dialect::all() {
            let m = manifest_for(d);
            assert!(
                matches!(
                    m.get(&Capability::Streaming),
                    Some(CoreSupportLevel::Native)
                ),
                "{d} missing native streaming"
            );
        }
    }

    #[test]
    fn all_manifests_have_tool_use_native() {
        for &d in Dialect::all() {
            let m = manifest_for(d);
            assert!(
                matches!(m.get(&Capability::ToolUse), Some(CoreSupportLevel::Native)),
                "{d} missing native tool_use"
            );
        }
    }

    #[test]
    fn all_manifests_have_system_message_native() {
        for &d in Dialect::all() {
            let m = manifest_for(d);
            assert!(
                matches!(
                    m.get(&Capability::SystemMessage),
                    Some(CoreSupportLevel::Native)
                ),
                "{d} missing native system_message"
            );
        }
    }

    #[test]
    fn registry_with_defaults_has_six_backends() {
        let reg = CapabilityRegistry::with_defaults();
        assert_eq!(reg.len(), 6);
    }

    #[test]
    fn gemini_extended_thinking_unsupported() {
        let m = gemini_15_pro_manifest();
        assert!(matches!(
            m.get(&Capability::ExtendedThinking),
            Some(CoreSupportLevel::Unsupported)
        ));
    }

    #[test]
    fn claude_logprobs_unsupported() {
        let m = claude_35_sonnet_manifest();
        assert!(matches!(
            m.get(&Capability::Logprobs),
            Some(CoreSupportLevel::Unsupported)
        ));
    }

    #[test]
    fn openai_extended_thinking_unsupported() {
        let m = openai_gpt4o_manifest();
        assert!(matches!(
            m.get(&Capability::ExtendedThinking),
            Some(CoreSupportLevel::Unsupported)
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 21: Dialect detection metadata
// ═══════════════════════════════════════════════════════════════════════════

mod dialect_metadata {
    use super::*;

    #[test]
    fn dialect_all_returns_six() {
        assert_eq!(Dialect::all().len(), 6);
    }

    #[test]
    fn dialect_labels_unique() {
        let labels: Vec<&str> = Dialect::all().iter().map(|d| d.label()).collect();
        let mut deduped = labels.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(labels.len(), deduped.len());
    }

    #[test]
    fn dialect_display_matches_label() {
        for &d in Dialect::all() {
            assert_eq!(d.to_string(), d.label());
        }
    }
}
