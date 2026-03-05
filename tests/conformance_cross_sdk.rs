#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Comprehensive conformance test suite for cross-SDK translation correctness.
//!
//! Validates:
//! - Round-trip preservation: SDK request → WorkOrder → Receipt → SDK response
//! - Cross-SDK translation: e.g. OpenAI request → WorkOrder → Claude response
//! - Feature parity matrix: basic chat, tool calling, streaming, system messages, multi-turn
//! - Error code mapping: each SDK error maps to correct ABP error classification
//! - Capability mapping: manifests match actual feature support
//! - Passthrough fidelity: passthrough mode preserves vendor-specific fields
//! - Mapped mode lossy signals: mapped mode properly signals lost information

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, ExecutionMode, Outcome, Receipt,
    ReceiptBuilder, SupportLevel, UsageNormalized, WorkOrder, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_error::{ErrorCategory, ErrorCode};
use abp_mapper::{MapError, default_ir_mapper, supported_ir_pairs};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;

// ── Scenario builders ───────────────────────────────────────────────────

fn simple_chat() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hello, how are you?")])
}

fn chat_with_system() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello!"),
    ])
}

fn tool_use_scenario() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is the weather in London?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "get_weather".into(),
                input: json!({"city": "London"}),
            }],
        ),
    ])
}

fn tool_result_scenario() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is the weather in London?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "get_weather".into(),
                input: json!({"city": "London"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "Sunny, 22°C".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "The weather in London is sunny at 22°C."),
    ])
}

fn multi_turn_scenario() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a coding assistant."),
        IrMessage::text(IrRole::User, "Write a hello world in Python."),
        IrMessage::text(IrRole::Assistant, "print('Hello, World!')"),
        IrMessage::text(IrRole::User, "Now make it a function."),
        IrMessage::text(
            IrRole::Assistant,
            "def hello():\n    print('Hello, World!')",
        ),
    ])
}

fn thinking_scenario() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Explain quantum computing."),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think about how to explain this simply...".into(),
                },
                IrContentBlock::Text {
                    text: "Quantum computing uses qubits instead of classical bits.".into(),
                },
            ],
        ),
    ])
}

fn image_scenario() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is in this image?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        ],
    )])
}

fn unicode_scenario() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "こんにちは世界！🌍🎉 Ñoño café résumé naïve"),
        IrMessage::text(IrRole::Assistant, "你好！这是Unicode测试 🚀✨"),
    ])
}

fn long_content_scenario() -> IrConversation {
    let long_text = "A".repeat(10_000);
    IrConversation::from_messages(vec![IrMessage::text(IrRole::User, &long_text)])
}

fn empty_scenario() -> IrConversation {
    IrConversation::new()
}

fn multi_tool_scenario() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Compare weather in London and Paris"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "London"}),
                },
                IrContentBlock::ToolUse {
                    id: "call_2".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "Paris"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "London: Sunny 22°C".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_2".into(),
                content: vec![IrContentBlock::Text {
                    text: "Paris: Cloudy 18°C".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(
            IrRole::Assistant,
            "London is sunny at 22°C while Paris is cloudy at 18°C.",
        ),
    ])
}

fn metadata_scenario() -> IrConversation {
    let mut metadata = BTreeMap::new();
    metadata.insert("temperature".into(), json!(0.7));
    metadata.insert("model".into(), json!("gpt-4"));
    IrConversation::from_messages(vec![IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text {
            text: "Hello with metadata".into(),
        }],
        metadata,
    }])
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn run_mapping(ir: IrConversation, from: Dialect, to: Dialect) -> IrConversation {
    let mapper =
        default_ir_mapper(from, to).unwrap_or_else(|| panic!("no mapper for {from:?} -> {to:?}"));
    mapper
        .map_request(from, to, &ir)
        .unwrap_or_else(|e| panic!("mapping {from:?} -> {to:?} failed: {e}"))
}

fn run_roundtrip(ir: &IrConversation, from: Dialect, to: Dialect) -> IrConversation {
    let fwd = default_ir_mapper(from, to).expect("forward mapper");
    let rev = default_ir_mapper(to, from).expect("reverse mapper");
    let intermediate = fwd.map_request(from, to, ir).expect("forward map");
    rev.map_request(to, from, &intermediate)
        .expect("reverse map")
}

fn collect_all_text(conv: &IrConversation) -> String {
    conv.messages.iter().map(|m| m.text_content()).collect()
}

/// Build a minimal Receipt suitable for SDK from_receipt / translate_from_receipt.
fn test_receipt(events: Vec<AgentEvent>, mode: ExecutionMode) -> Receipt {
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .mode(mode)
        .usage(UsageNormalized {
            input_tokens: Some(10),
            output_tokens: Some(20),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        })
        .add_trace_event(
            events
                .into_iter()
                .fold(ReceiptBuilder::new("x"), |b, e| b.add_trace_event(e))
                .build()
                .trace
                .into_iter()
                .next()
                .unwrap_or(AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::RunCompleted {
                        message: "done".into(),
                    },
                    ext: None,
                }),
        )
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .build()
}

/// Simpler receipt builder that just adds events directly.
fn make_receipt(text: &str) -> Receipt {
    let now = Utc::now();
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .mode(ExecutionMode::Mapped)
        .usage(UsageNormalized {
            input_tokens: Some(10),
            output_tokens: Some(20),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        })
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantMessage {
                text: text.to_string(),
            },
            ext: None,
        })
        .build()
}

fn make_receipt_with_tool_call() -> Receipt {
    let now = Utc::now();
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .mode(ExecutionMode::Mapped)
        .usage(UsageNormalized {
            input_tokens: Some(15),
            output_tokens: Some(30),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        })
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolCall {
                tool_name: "get_weather".into(),
                tool_use_id: Some("call_1".into()),
                parent_tool_use_id: None,
                input: json!({"city": "London"}),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolResult {
                tool_name: "get_weather".into(),
                tool_use_id: Some("call_1".into()),
                output: "Sunny, 22°C".into(),
                is_error: false,
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantMessage {
                text: "The weather in London is sunny at 22°C.".into(),
            },
            ext: None,
        })
        .build()
}

// ── 1. SDK-level round-trip: request → WorkOrder → Receipt → response ───

mod sdk_roundtrip {
    use super::*;

    // -- OpenAI shim round-trip --
    #[test]
    fn openai_request_to_work_order_preserves_task() {
        use abp_shim_openai::types::*;
        let req = ChatCompletionRequest {
            model: "gpt-4".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("Hello world".into()),
            }],
            temperature: Some(0.7),
            top_p: None,
            max_tokens: Some(1024),
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = abp_shim_openai::convert::to_work_order(&req);
        assert!(!wo.task.is_empty(), "work order task should be non-empty");
    }

    #[test]
    fn openai_receipt_to_response_has_choices() {
        use abp_shim_openai::types::*;
        let req = ChatCompletionRequest {
            model: "gpt-4".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("Hi".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = abp_shim_openai::convert::to_work_order(&req);
        let receipt = make_receipt("Hello there!");
        let resp = abp_shim_openai::convert::from_receipt(&receipt, &wo);
        assert!(!resp.choices.is_empty(), "response should have choices");
    }

    #[test]
    fn openai_roundtrip_preserves_model() {
        use abp_shim_openai::types::*;
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("test".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = abp_shim_openai::convert::to_work_order(&req);
        let receipt = make_receipt("response text");
        let resp = abp_shim_openai::convert::from_receipt(&receipt, &wo);
        assert_eq!(resp.model, "gpt-4o");
    }

    // -- Claude shim round-trip --
    #[test]
    fn claude_request_to_work_order_preserves_task() {
        use abp_shim_claude::types::*;
        let req = MessagesRequest {
            model: "claude-3-5-sonnet-latest".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Hello world".into()),
            }],
            max_tokens: 1024,
            system: Some("Be helpful.".into()),
            temperature: Some(0.5),
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = abp_shim_claude::convert::to_work_order(&req);
        assert!(!wo.task.is_empty());
    }

    #[test]
    fn claude_receipt_to_response_has_content() {
        use abp_shim_claude::types::*;
        let req = MessagesRequest {
            model: "claude-3-5-sonnet-latest".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Hi".into()),
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
        let wo = abp_shim_claude::convert::to_work_order(&req);
        let receipt = make_receipt("Hello from Claude!");
        let resp = abp_shim_claude::convert::from_receipt(&receipt, &wo);
        assert!(!resp.content.is_empty(), "response should have content");
    }

    // -- Kimi shim round-trip --
    #[test]
    fn kimi_request_to_work_order() {
        use abp_shim_kimi::types::*;
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message::user("Hello")],
            temperature: Some(0.3),
            top_p: None,
            max_tokens: Some(512),
            stream: None,
            n: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: None,
            tool_choice: None,
            response_format: None,
            use_search: None,
            ref_file_ids: None,
            plugin_ids: None,
            plugins: None,
        };
        let wo = abp_shim_kimi::translate::translate_to_work_order(&req);
        assert!(!wo.task.is_empty());
    }

    #[test]
    fn kimi_receipt_to_response() {
        use abp_shim_kimi::types::*;
        let receipt = make_receipt("Kimi response");
        let resp = abp_shim_kimi::translate::translate_from_receipt(&receipt, "moonshot-v1-8k");
        assert!(!resp.choices.is_empty());
    }
}

// ── 2. Cross-SDK translation ────────────────────────────────────────────

mod cross_sdk_translation {
    use super::*;

    #[test]
    fn openai_request_to_claude_response() {
        use abp_shim_openai::types::*;
        let req = ChatCompletionRequest {
            model: "gpt-4".into(),
            messages: vec![
                ChatMessage::System {
                    content: "You are helpful.".into(),
                },
                ChatMessage::User {
                    content: MessageContent::Text("Explain Rust.".into()),
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let _wo = abp_shim_openai::convert::to_work_order(&req);
        assert!(!_wo.task.is_empty());
        let receipt = make_receipt("Rust is a systems programming language.");
        let claude_resp = abp_shim_claude::convert::from_receipt(&receipt, &_wo);
        assert!(!claude_resp.content.is_empty());
    }

    #[test]
    fn claude_request_to_openai_response() {
        use abp_shim_claude::types::*;
        let req = MessagesRequest {
            model: "claude-3-5-sonnet-latest".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Explain Python.".into()),
            }],
            max_tokens: 1024,
            system: Some("Be concise.".into()),
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = abp_shim_claude::convert::to_work_order(&req);
        let receipt = make_receipt("Python is a versatile language.");
        let openai_resp = abp_shim_openai::convert::from_receipt(&receipt, &wo);
        assert!(!openai_resp.choices.is_empty());
    }

    #[test]
    fn openai_request_to_kimi_response() {
        use abp_shim_openai::types::*;
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("What is AI?".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let _wo = abp_shim_openai::convert::to_work_order(&req);
        let receipt = make_receipt("AI stands for artificial intelligence.");
        let kimi_resp =
            abp_shim_kimi::translate::translate_from_receipt(&receipt, "moonshot-v1-8k");
        assert!(!kimi_resp.choices.is_empty());
    }

    #[test]
    fn kimi_request_to_openai_response() {
        use abp_shim_kimi::types::*;
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message::user("Hello")],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            n: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: None,
            tool_choice: None,
            response_format: None,
            use_search: None,
            ref_file_ids: None,
            plugin_ids: None,
            plugins: None,
        };
        let wo = abp_shim_kimi::translate::translate_to_work_order(&req);
        let receipt = make_receipt("Hi from OpenAI.");
        let openai_resp = abp_shim_openai::convert::from_receipt(&receipt, &wo);
        assert!(!openai_resp.choices.is_empty());
    }

    #[test]
    fn claude_request_to_kimi_response() {
        use abp_shim_claude::types::*;
        let req = MessagesRequest {
            model: "claude-3-5-sonnet-latest".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("What is ML?".into()),
            }],
            max_tokens: 256,
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
        let _wo = abp_shim_claude::convert::to_work_order(&req);
        let receipt = make_receipt("ML is machine learning.");
        let kimi_resp =
            abp_shim_kimi::translate::translate_from_receipt(&receipt, "moonshot-v1-8k");
        assert!(!kimi_resp.choices.is_empty());
    }

    #[test]
    fn work_order_interop_openai_to_claude_receipt() {
        let receipt = make_receipt_with_tool_call();
        let wo = WorkOrderBuilder::new("test tool call").build();
        let openai_resp = abp_shim_openai::convert::from_receipt(&receipt, &wo);
        let claude_resp = abp_shim_claude::convert::from_receipt(&receipt, &wo);
        // Both should produce non-empty responses from the same receipt
        assert!(!openai_resp.choices.is_empty());
        assert!(!claude_resp.content.is_empty());
    }
}

// ── 3. Feature parity matrix ────────────────────────────────────────────

mod feature_parity_matrix {
    use super::*;

    // Basic chat: all pairs
    #[test]
    fn basic_chat_all_supported_pairs() {
        let ir = simple_chat();
        for (from, to) in supported_ir_pairs() {
            let mapper = default_ir_mapper(from, to).unwrap();
            let result = mapper.map_request(from, to, &ir);
            assert!(
                result.is_ok(),
                "basic chat should work for {from:?} -> {to:?}"
            );
        }
    }

    // System messages
    #[test]
    fn system_messages_supported_pairs() {
        let ir = chat_with_system();
        let pairs_supporting_system = [
            (Dialect::OpenAi, Dialect::Claude),
            (Dialect::OpenAi, Dialect::Gemini),
            (Dialect::OpenAi, Dialect::Kimi),
            (Dialect::Claude, Dialect::OpenAi),
            (Dialect::Claude, Dialect::Gemini),
            (Dialect::Claude, Dialect::Kimi),
        ];
        for (from, to) in pairs_supporting_system {
            let mapped = run_mapping(ir.clone(), from, to);
            assert!(
                mapped.system_message().is_some(),
                "system message should be preserved for {from:?}->{to:?}"
            );
        }
    }

    // Tool calling
    #[test]
    fn tool_calling_preserved_major_pairs() {
        let ir = tool_use_scenario();
        let tool_pairs = [
            (Dialect::OpenAi, Dialect::Claude),
            (Dialect::Claude, Dialect::OpenAi),
            (Dialect::OpenAi, Dialect::Gemini),
            (Dialect::Gemini, Dialect::OpenAi),
            (Dialect::OpenAi, Dialect::Kimi),
            (Dialect::Kimi, Dialect::OpenAi),
        ];
        for (from, to) in tool_pairs {
            let mapped = run_mapping(ir.clone(), from, to);
            assert!(
                !mapped.tool_calls().is_empty(),
                "tool calls should be preserved for {from:?}->{to:?}"
            );
        }
    }

    // Multi-turn
    #[test]
    fn multi_turn_message_count_preserved() {
        let ir = multi_turn_scenario();
        let msg_count = ir.len();
        for (from, to) in supported_ir_pairs() {
            let mapper = default_ir_mapper(from, to).unwrap();
            let result = mapper.map_request(from, to, &ir);
            if let Ok(mapped) = result {
                assert!(
                    mapped.len() > 0,
                    "multi-turn should produce output for {from:?}->{to:?}"
                );
                // Identity should preserve exact count
                if from == to {
                    assert_eq!(
                        mapped.len(),
                        msg_count,
                        "identity should preserve message count for {from:?}"
                    );
                }
            }
        }
    }

    // Streaming simulation (IR-level)
    #[test]
    fn streaming_text_chunks_preserved_all_pairs() {
        let chunk = IrConversation::from_messages(vec![IrMessage::text(
            IrRole::Assistant,
            "streaming text",
        )]);
        for (from, to) in supported_ir_pairs() {
            let mapper = default_ir_mapper(from, to).unwrap();
            let result = mapper.map_request(from, to, &chunk);
            if let Ok(mapped) = result {
                let text = collect_all_text(&mapped);
                assert!(
                    text.contains("streaming text"),
                    "streaming chunk text lost for {from:?}->{to:?}"
                );
            }
        }
    }

    // Feature coverage: thinking blocks
    #[test]
    fn thinking_blocks_only_native_for_claude() {
        let manifest = abp_claude_sdk::dialect::capability_manifest();
        let thinking_support = manifest.get(&Capability::ExtendedThinking);
        assert!(
            matches!(thinking_support, Some(SupportLevel::Native)),
            "Claude should natively support extended thinking"
        );
    }
}

// ── 4. Error code mapping ───────────────────────────────────────────────

mod error_code_mapping {
    use super::*;

    #[test]
    fn backend_error_codes_have_correct_category() {
        let backend_codes = [
            ErrorCode::BackendNotFound,
            ErrorCode::BackendUnavailable,
            ErrorCode::BackendTimeout,
            ErrorCode::BackendRateLimited,
            ErrorCode::BackendAuthFailed,
            ErrorCode::BackendModelNotFound,
            ErrorCode::BackendCrashed,
        ];
        for code in &backend_codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Backend,
                "{:?} should be Backend category",
                code
            );
        }
    }

    #[test]
    fn mapping_error_codes_have_correct_category() {
        let mapping_codes = [
            ErrorCode::MappingUnsupportedCapability,
            ErrorCode::MappingDialectMismatch,
            ErrorCode::MappingLossyConversion,
            ErrorCode::MappingUnmappableTool,
        ];
        for code in &mapping_codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Mapping,
                "{:?} should be Mapping category",
                code
            );
        }
    }

    #[test]
    fn protocol_error_codes_have_correct_category() {
        let protocol_codes = [
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::ProtocolHandshakeFailed,
            ErrorCode::ProtocolMissingRefId,
            ErrorCode::ProtocolUnexpectedMessage,
            ErrorCode::ProtocolVersionMismatch,
        ];
        for code in &protocol_codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Protocol,
                "{:?} should be Protocol category",
                code
            );
        }
    }

    #[test]
    fn retryable_error_codes_include_rate_limit() {
        assert!(ErrorCode::BackendRateLimited.is_retryable());
        assert!(ErrorCode::BackendTimeout.is_retryable());
        assert!(ErrorCode::BackendUnavailable.is_retryable());
    }

    #[test]
    fn non_retryable_error_codes() {
        assert!(!ErrorCode::BackendAuthFailed.is_retryable());
        assert!(!ErrorCode::PolicyDenied.is_retryable());
        assert!(!ErrorCode::ContractSchemaViolation.is_retryable());
    }

    #[test]
    fn all_error_codes_have_non_empty_message() {
        let codes = [
            ErrorCode::BackendNotFound,
            ErrorCode::BackendUnavailable,
            ErrorCode::BackendTimeout,
            ErrorCode::BackendRateLimited,
            ErrorCode::BackendAuthFailed,
            ErrorCode::MappingLossyConversion,
            ErrorCode::MappingUnmappableTool,
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::CapabilityUnsupported,
            ErrorCode::PolicyDenied,
        ];
        for code in &codes {
            assert!(
                !code.message().is_empty(),
                "{:?} should have a non-empty message",
                code
            );
        }
    }

    #[test]
    fn error_code_as_str_is_snake_case() {
        let codes = [
            ErrorCode::BackendNotFound,
            ErrorCode::MappingLossyConversion,
            ErrorCode::ProtocolInvalidEnvelope,
        ];
        for code in &codes {
            let s = code.as_str();
            assert!(
                !s.contains(char::is_uppercase),
                "{:?}.as_str() = '{}' should be snake_case",
                code,
                s
            );
        }
    }
}

// ── 5. Capability mapping ───────────────────────────────────────────────

mod capability_mapping {
    use super::*;

    fn assert_manifest_has(manifest: &CapabilityManifest, cap: Capability, label: &str) {
        assert!(
            manifest.contains_key(&cap),
            "{label} manifest should declare {cap:?}"
        );
    }

    #[test]
    fn openai_manifest_declares_core_capabilities() {
        let m = abp_openai_sdk::dialect::capability_manifest();
        assert_manifest_has(&m, Capability::Streaming, "OpenAI");
        assert_manifest_has(&m, Capability::FunctionCalling, "OpenAI");
        assert_manifest_has(&m, Capability::SystemMessage, "OpenAI");
        assert_manifest_has(&m, Capability::Temperature, "OpenAI");
    }

    #[test]
    fn claude_manifest_declares_core_capabilities() {
        let m = abp_claude_sdk::dialect::capability_manifest();
        assert_manifest_has(&m, Capability::Streaming, "Claude");
        assert_manifest_has(&m, Capability::FunctionCalling, "Claude");
        assert_manifest_has(&m, Capability::SystemMessage, "Claude");
        assert_manifest_has(&m, Capability::ExtendedThinking, "Claude");
    }

    #[test]
    fn gemini_manifest_declares_core_capabilities() {
        let m = abp_gemini_sdk::dialect::capability_manifest();
        assert_manifest_has(&m, Capability::Streaming, "Gemini");
        assert_manifest_has(&m, Capability::FunctionCalling, "Gemini");
        assert_manifest_has(&m, Capability::SystemMessage, "Gemini");
    }

    #[test]
    fn codex_manifest_declares_core_capabilities() {
        let m = abp_codex_sdk::dialect::capability_manifest();
        assert_manifest_has(&m, Capability::Streaming, "Codex");
        assert_manifest_has(&m, Capability::FunctionCalling, "Codex");
    }

    #[test]
    fn copilot_manifest_declares_core_capabilities() {
        let m = abp_copilot_sdk::dialect::capability_manifest();
        assert_manifest_has(&m, Capability::Streaming, "Copilot");
        assert_manifest_has(&m, Capability::FunctionCalling, "Copilot");
    }

    #[test]
    fn kimi_manifest_declares_core_capabilities() {
        let m = abp_kimi_sdk::dialect::capability_manifest();
        assert_manifest_has(&m, Capability::Streaming, "Kimi");
        assert_manifest_has(&m, Capability::FunctionCalling, "Kimi");
        assert_manifest_has(&m, Capability::SystemMessage, "Kimi");
    }

    #[test]
    fn all_manifests_have_streaming() {
        let manifests: Vec<(&str, CapabilityManifest)> = vec![
            ("OpenAI", abp_openai_sdk::dialect::capability_manifest()),
            ("Claude", abp_claude_sdk::dialect::capability_manifest()),
            ("Gemini", abp_gemini_sdk::dialect::capability_manifest()),
            ("Codex", abp_codex_sdk::dialect::capability_manifest()),
            ("Copilot", abp_copilot_sdk::dialect::capability_manifest()),
            ("Kimi", abp_kimi_sdk::dialect::capability_manifest()),
        ];
        for (name, m) in &manifests {
            assert!(
                m.contains_key(&Capability::Streaming),
                "{name} should declare streaming capability"
            );
        }
    }

    #[test]
    fn all_manifests_have_function_calling() {
        let manifests: Vec<(&str, CapabilityManifest)> = vec![
            ("OpenAI", abp_openai_sdk::dialect::capability_manifest()),
            ("Claude", abp_claude_sdk::dialect::capability_manifest()),
            ("Gemini", abp_gemini_sdk::dialect::capability_manifest()),
            ("Codex", abp_codex_sdk::dialect::capability_manifest()),
            ("Copilot", abp_copilot_sdk::dialect::capability_manifest()),
            ("Kimi", abp_kimi_sdk::dialect::capability_manifest()),
        ];
        for (name, m) in &manifests {
            assert!(
                m.contains_key(&Capability::FunctionCalling),
                "{name} should declare function calling capability"
            );
        }
    }

    #[test]
    fn extended_thinking_only_native_in_claude() {
        let non_claude = [
            ("OpenAI", abp_openai_sdk::dialect::capability_manifest()),
            ("Gemini", abp_gemini_sdk::dialect::capability_manifest()),
            ("Kimi", abp_kimi_sdk::dialect::capability_manifest()),
        ];
        for (name, m) in &non_claude {
            if let Some(level) = m.get(&Capability::ExtendedThinking) {
                assert!(
                    !matches!(level, SupportLevel::Native),
                    "{name} should not natively support ExtendedThinking"
                );
            }
        }
    }

    #[test]
    fn manifests_are_non_empty() {
        let manifests = [
            abp_openai_sdk::dialect::capability_manifest(),
            abp_claude_sdk::dialect::capability_manifest(),
            abp_gemini_sdk::dialect::capability_manifest(),
            abp_codex_sdk::dialect::capability_manifest(),
            abp_copilot_sdk::dialect::capability_manifest(),
            abp_kimi_sdk::dialect::capability_manifest(),
        ];
        for (i, m) in manifests.iter().enumerate() {
            assert!(!m.is_empty(), "manifest {i} should be non-empty");
        }
    }
}

// ── 6. Passthrough fidelity ─────────────────────────────────────────────

mod passthrough_fidelity {
    use super::*;

    // Identity mapping preserves exact content (passthrough semantics)
    macro_rules! identity_exact {
        ($name:ident, $dialect:expr, $scenario_fn:ident) => {
            #[test]
            fn $name() {
                let original = $scenario_fn();
                let mapped = run_mapping(original.clone(), $dialect, $dialect);
                assert_eq!(original, mapped, "identity mapping must be exact");
            }
        };
    }

    identity_exact!(simple_openai, Dialect::OpenAi, simple_chat);
    identity_exact!(simple_claude, Dialect::Claude, simple_chat);
    identity_exact!(simple_gemini, Dialect::Gemini, simple_chat);
    identity_exact!(simple_codex, Dialect::Codex, simple_chat);
    identity_exact!(simple_kimi, Dialect::Kimi, simple_chat);
    identity_exact!(simple_copilot, Dialect::Copilot, simple_chat);

    identity_exact!(multi_turn_openai, Dialect::OpenAi, multi_turn_scenario);
    identity_exact!(multi_turn_claude, Dialect::Claude, multi_turn_scenario);
    identity_exact!(multi_turn_gemini, Dialect::Gemini, multi_turn_scenario);

    identity_exact!(tool_result_openai, Dialect::OpenAi, tool_result_scenario);
    identity_exact!(tool_result_claude, Dialect::Claude, tool_result_scenario);
    identity_exact!(tool_result_gemini, Dialect::Gemini, tool_result_scenario);

    // Passthrough mode receipt preserves vendor-specific fields
    #[test]
    fn passthrough_receipt_preserves_execution_mode() {
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .mode(ExecutionMode::Passthrough)
            .build();
        assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn passthrough_receipt_vendor_usage_raw_preserved() {
        let raw_usage = json!({
            "prompt_tokens": 42,
            "completion_tokens": 99,
            "vendor_specific_field": "preserved"
        });
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .mode(ExecutionMode::Passthrough)
            .usage_raw(raw_usage.clone())
            .build();
        assert_eq!(receipt.usage_raw, raw_usage);
    }

    #[test]
    fn passthrough_receipt_ext_fields_in_events() {
        let mut ext = BTreeMap::new();
        ext.insert("vendor_field".to_string(), json!("preserved_data"));
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "test".into(),
            },
            ext: Some(ext),
        };
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .mode(ExecutionMode::Passthrough)
            .add_trace_event(event)
            .build();
        let evt = &receipt.trace[0];
        assert!(evt.ext.is_some());
        assert_eq!(
            evt.ext.as_ref().unwrap()["vendor_field"],
            json!("preserved_data")
        );
    }

    #[test]
    fn metadata_preserved_in_identity_mapping() {
        let orig = metadata_scenario();
        for &d in Dialect::all() {
            let mapped = run_mapping(orig.clone(), d, d);
            assert_eq!(
                orig.messages[0].metadata, mapped.messages[0].metadata,
                "metadata should be preserved in identity mapping for {d:?}"
            );
        }
    }
}

// ── 7. Mapped mode lossy signals ────────────────────────────────────────

mod mapped_mode_lossy_signals {
    use super::*;

    #[test]
    fn thinking_dropped_openai_signals_loss() {
        let ir = thinking_scenario();
        let mapped = run_mapping(ir, Dialect::Claude, Dialect::OpenAi);
        let has_thinking = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
        assert!(!has_thinking, "OpenAI should drop thinking blocks (lossy)");
    }

    #[test]
    fn thinking_dropped_gemini_signals_loss() {
        let ir = thinking_scenario();
        let mapped = run_mapping(ir, Dialect::Claude, Dialect::Gemini);
        let has_thinking = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
        assert!(!has_thinking, "Gemini should drop thinking blocks (lossy)");
    }

    #[test]
    fn thinking_dropped_kimi_signals_loss() {
        let ir = thinking_scenario();
        let mapped = run_mapping(ir, Dialect::Claude, Dialect::Kimi);
        let has_thinking = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
        assert!(!has_thinking, "Kimi should drop thinking blocks (lossy)");
    }

    #[test]
    fn codex_drops_system_message_lossy() {
        let ir = chat_with_system();
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Codex);
        assert!(
            mapped.system_message().is_none(),
            "Codex should drop system messages (lossy)"
        );
    }

    #[test]
    fn codex_drops_images_lossy() {
        let ir = image_scenario();
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Codex);
        let has_image = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::Image { .. }));
        assert!(!has_image, "Codex should drop images (lossy)");
    }

    #[test]
    fn codex_drops_tool_results_lossy() {
        let ir = tool_result_scenario();
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Codex);
        let has_tool_result = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::ToolResult { .. }));
        assert!(!has_tool_result, "Codex should drop tool results (lossy)");
    }

    #[test]
    fn unmappable_tool_error_signals_loss() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "apply_patch".into(),
                input: json!({}),
            }],
        )]);
        let mapper = default_ir_mapper(Dialect::Codex, Dialect::Claude).unwrap();
        let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &ir);
        assert!(
            matches!(result, Err(MapError::UnmappableTool { .. })),
            "unmappable tool should signal loss via error"
        );
    }

    #[test]
    fn mapped_mode_receipt_is_mapped() {
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .mode(ExecutionMode::Mapped)
            .build();
        assert_eq!(receipt.mode, ExecutionMode::Mapped);
    }

    #[test]
    fn thinking_text_preserved_even_when_blocks_dropped() {
        let ir = thinking_scenario();
        let mapped = run_mapping(ir, Dialect::Claude, Dialect::OpenAi);
        let text = collect_all_text(&mapped);
        assert!(
            text.contains("Quantum computing uses qubits"),
            "text content should survive even when thinking blocks are dropped"
        );
    }

    #[test]
    fn roundtrip_stability_after_lossy_drop() {
        let ir = thinking_scenario();
        let first = run_roundtrip(&ir, Dialect::Claude, Dialect::OpenAi);
        let second = run_roundtrip(&first, Dialect::Claude, Dialect::OpenAi);
        assert_eq!(
            first, second,
            "after initial lossy drop, further round-trips should be stable"
        );
    }
}

// ── Cross-dialect mapping pair tests ────────────────────────────────────

macro_rules! all_pair_tests {
    ($mod_name:ident, $scenario_fn:ident) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn identity_openai() {
                run_mapping($scenario_fn(), Dialect::OpenAi, Dialect::OpenAi);
            }
            #[test]
            fn identity_claude() {
                run_mapping($scenario_fn(), Dialect::Claude, Dialect::Claude);
            }
            #[test]
            fn identity_gemini() {
                run_mapping($scenario_fn(), Dialect::Gemini, Dialect::Gemini);
            }
            #[test]
            fn identity_codex() {
                run_mapping($scenario_fn(), Dialect::Codex, Dialect::Codex);
            }
            #[test]
            fn identity_kimi() {
                run_mapping($scenario_fn(), Dialect::Kimi, Dialect::Kimi);
            }
            #[test]
            fn identity_copilot() {
                run_mapping($scenario_fn(), Dialect::Copilot, Dialect::Copilot);
            }
            #[test]
            fn openai_to_claude() {
                run_mapping($scenario_fn(), Dialect::OpenAi, Dialect::Claude);
            }
            #[test]
            fn claude_to_openai() {
                run_mapping($scenario_fn(), Dialect::Claude, Dialect::OpenAi);
            }
            #[test]
            fn openai_to_gemini() {
                run_mapping($scenario_fn(), Dialect::OpenAi, Dialect::Gemini);
            }
            #[test]
            fn gemini_to_openai() {
                run_mapping($scenario_fn(), Dialect::Gemini, Dialect::OpenAi);
            }
            #[test]
            fn claude_to_gemini() {
                run_mapping($scenario_fn(), Dialect::Claude, Dialect::Gemini);
            }
            #[test]
            fn gemini_to_claude() {
                run_mapping($scenario_fn(), Dialect::Gemini, Dialect::Claude);
            }
            #[test]
            fn openai_to_codex() {
                run_mapping($scenario_fn(), Dialect::OpenAi, Dialect::Codex);
            }
            #[test]
            fn codex_to_openai() {
                run_mapping($scenario_fn(), Dialect::Codex, Dialect::OpenAi);
            }
            #[test]
            fn openai_to_kimi() {
                run_mapping($scenario_fn(), Dialect::OpenAi, Dialect::Kimi);
            }
            #[test]
            fn kimi_to_openai() {
                run_mapping($scenario_fn(), Dialect::Kimi, Dialect::OpenAi);
            }
            #[test]
            fn claude_to_kimi() {
                run_mapping($scenario_fn(), Dialect::Claude, Dialect::Kimi);
            }
            #[test]
            fn kimi_to_claude() {
                run_mapping($scenario_fn(), Dialect::Kimi, Dialect::Claude);
            }
            #[test]
            fn openai_to_copilot() {
                run_mapping($scenario_fn(), Dialect::OpenAi, Dialect::Copilot);
            }
            #[test]
            fn copilot_to_openai() {
                run_mapping($scenario_fn(), Dialect::Copilot, Dialect::OpenAi);
            }
            #[test]
            fn gemini_to_kimi() {
                run_mapping($scenario_fn(), Dialect::Gemini, Dialect::Kimi);
            }
            #[test]
            fn kimi_to_gemini() {
                run_mapping($scenario_fn(), Dialect::Kimi, Dialect::Gemini);
            }
            #[test]
            fn codex_to_claude() {
                run_mapping($scenario_fn(), Dialect::Codex, Dialect::Claude);
            }
            #[test]
            fn claude_to_codex() {
                run_mapping($scenario_fn(), Dialect::Claude, Dialect::Codex);
            }
        }
    };
}

all_pair_tests!(simple_chat_pairs, simple_chat);
all_pair_tests!(system_prompt_pairs, chat_with_system);
all_pair_tests!(tool_use_pairs, tool_use_scenario);
all_pair_tests!(tool_result_pairs, tool_result_scenario);
all_pair_tests!(multi_turn_pairs, multi_turn_scenario);
all_pair_tests!(thinking_pairs, thinking_scenario);

// ── Round-trip text preservation ────────────────────────────────────────

mod roundtrip_text_preservation {
    use super::*;

    #[test]
    fn simple_chat_openai_claude() {
        let orig = simple_chat();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Claude);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn simple_chat_openai_gemini() {
        let orig = simple_chat();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Gemini);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn simple_chat_openai_kimi() {
        let orig = simple_chat();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Kimi);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn simple_chat_openai_copilot() {
        let orig = simple_chat();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Copilot);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn unicode_openai_claude() {
        let orig = unicode_scenario();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Claude);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn multi_turn_openai_claude() {
        let orig = multi_turn_scenario();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Claude);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn long_content_openai_claude() {
        let orig = long_content_scenario();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Claude);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }
}

// ── Round-trip stability ────────────────────────────────────────────────

mod roundtrip_stability {
    use super::*;

    fn assert_roundtrip_stable(scenario: IrConversation, a: Dialect, b: Dialect) {
        let first = run_roundtrip(&scenario, a, b);
        let second = run_roundtrip(&first, a, b);
        assert_eq!(
            first, second,
            "round-trip {a:?}<->{b:?} should stabilize after one pass"
        );
    }

    #[test]
    fn stable_openai_claude() {
        assert_roundtrip_stable(simple_chat(), Dialect::OpenAi, Dialect::Claude);
    }

    #[test]
    fn stable_openai_gemini() {
        assert_roundtrip_stable(simple_chat(), Dialect::OpenAi, Dialect::Gemini);
    }

    #[test]
    fn stable_claude_gemini() {
        assert_roundtrip_stable(simple_chat(), Dialect::Claude, Dialect::Gemini);
    }

    #[test]
    fn stable_openai_kimi() {
        assert_roundtrip_stable(simple_chat(), Dialect::OpenAi, Dialect::Kimi);
    }

    #[test]
    fn stable_openai_copilot() {
        assert_roundtrip_stable(simple_chat(), Dialect::OpenAi, Dialect::Copilot);
    }
}

// ── Unsupported pair tests ──────────────────────────────────────────────

mod unsupported_pairs {
    use super::*;

    macro_rules! unsupported {
        ($name:ident, $from:expr, $to:expr) => {
            #[test]
            fn $name() {
                assert!(
                    default_ir_mapper($from, $to).is_none(),
                    "pair {:?}->{:?} should be unsupported",
                    $from,
                    $to
                );
            }
        };
    }

    unsupported!(codex_gemini, Dialect::Codex, Dialect::Gemini);
    unsupported!(gemini_codex, Dialect::Gemini, Dialect::Codex);
    unsupported!(codex_kimi, Dialect::Codex, Dialect::Kimi);
    unsupported!(kimi_codex, Dialect::Kimi, Dialect::Codex);
    unsupported!(codex_copilot, Dialect::Codex, Dialect::Copilot);
    unsupported!(copilot_codex, Dialect::Copilot, Dialect::Codex);
    unsupported!(copilot_claude, Dialect::Copilot, Dialect::Claude);
    unsupported!(claude_copilot, Dialect::Claude, Dialect::Copilot);
    unsupported!(copilot_gemini, Dialect::Copilot, Dialect::Gemini);
    unsupported!(gemini_copilot, Dialect::Gemini, Dialect::Copilot);
    unsupported!(copilot_kimi, Dialect::Copilot, Dialect::Kimi);
    unsupported!(kimi_copilot, Dialect::Kimi, Dialect::Copilot);
}

// ── Edge case tests ─────────────────────────────────────────────────────

mod edge_cases {
    use super::*;

    #[test]
    fn single_empty_message() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(IrRole::User, vec![])]);
        for (from, to) in supported_ir_pairs() {
            let mapper = default_ir_mapper(from, to).unwrap();
            let _ = mapper.map_request(from, to, &ir);
        }
    }

    #[test]
    fn tool_error_result() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "run this"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "execute".into(),
                    input: json!({"cmd": "fail"}),
                }],
            ),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "Error: command failed".into(),
                    }],
                    is_error: true,
                }],
            ),
        ]);
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Claude);
        let has_error_result = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::ToolResult { is_error: true, .. }));
        assert!(has_error_result, "error flag should be preserved");
    }

    #[test]
    fn complex_tool_input() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "query".into(),
                input: json!({
                    "filters": [{"field": "name", "op": "eq", "value": "test"}],
                    "nested": {"deep": {"key": [1, 2, 3]}},
                    "unicode": "日本語",
                    "empty_array": [],
                    "null_val": null
                }),
            }],
        )]);
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Claude);
        if let IrContentBlock::ToolUse { input, .. } = &mapped.tool_calls()[0] {
            assert!(input.get("nested").is_some(), "nested input preserved");
            assert_eq!(input["unicode"], "日本語");
        }
    }

    #[test]
    fn codex_to_claude_rejects_apply_patch() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "apply_patch".into(),
                input: json!({}),
            }],
        )]);
        let mapper = default_ir_mapper(Dialect::Codex, Dialect::Claude).unwrap();
        let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &ir);
        assert!(matches!(result, Err(MapError::UnmappableTool { .. })));
    }

    #[test]
    fn empty_conversation_stays_empty() {
        for &to in Dialect::all() {
            let mapped = run_mapping(empty_scenario(), Dialect::OpenAi, to);
            assert!(mapped.is_empty(), "empty should stay empty for {to:?}");
        }
    }
}

// ── Factory & meta tests ────────────────────────────────────────────────

mod factory_meta {
    use super::*;

    #[test]
    fn supported_pairs_includes_all_identity() {
        let pairs = supported_ir_pairs();
        for &d in Dialect::all() {
            assert!(pairs.contains(&(d, d)), "identity pair missing for {d:?}");
        }
    }

    #[test]
    fn supported_pairs_count() {
        let pairs = supported_ir_pairs();
        assert_eq!(pairs.len(), 24);
    }

    #[test]
    fn all_supported_pairs_have_mappers() {
        for (from, to) in supported_ir_pairs() {
            assert!(
                default_ir_mapper(from, to).is_some(),
                "no mapper for {from:?} -> {to:?}"
            );
        }
    }

    #[test]
    fn both_map_request_and_map_response_succeed() {
        let ir = simple_chat();
        for (from, to) in supported_ir_pairs() {
            let mapper = default_ir_mapper(from, to).unwrap();
            mapper
                .map_request(from, to, &ir)
                .unwrap_or_else(|e| panic!("map_request {from:?}->{to:?}: {e}"));
            mapper
                .map_response(from, to, &ir)
                .unwrap_or_else(|e| panic!("map_response {from:?}->{to:?}: {e}"));
        }
    }

    #[test]
    fn ir_tool_definition_roundtrip() {
        let tool = IrToolDefinition {
            name: "get_weather".into(),
            description: "Get weather for a city".into(),
            parameters: json!({
                "type": "object",
                "properties": { "city": {"type": "string"} },
                "required": ["city"]
            }),
        };
        let json = serde_json::to_value(&tool).unwrap();
        let back: IrToolDefinition = serde_json::from_value(json).unwrap();
        assert_eq!(tool, back);
    }

    #[test]
    fn ir_conversation_serde_roundtrip() {
        let conv = tool_result_scenario();
        let json = serde_json::to_value(&conv).unwrap();
        let back: IrConversation = serde_json::from_value(json).unwrap();
        assert_eq!(conv, back);
    }
}
