#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]

//! Comprehensive cross-SDK end-to-end tests for the Agent Backplane.
//!
//! Tests translation fidelity, capability negotiation, tool mapping, streaming
//! event mapping, error code translation, receipt generation, and work order
//! creation across different SDK dialects.

use std::collections::BTreeMap;

use abp_capability::{
    SupportLevel as CapSupportLevel, check_capability, generate_report, negotiate,
};
use abp_claude_sdk::dialect::{
    self as claude_dialect, CanonicalToolDef as ClaudeCanonical, ClaudeConfig, ClaudeContentBlock,
    ClaudeMessage, ClaudeResponse, ClaudeStopReason, ClaudeStreamDelta, ClaudeStreamEvent,
    ClaudeUsage,
};
use abp_codex_sdk::dialect::{
    self as codex_dialect, CanonicalToolDef as CodexCanonical, CodexConfig,
};
use abp_copilot_sdk::dialect::{
    self as copilot_dialect, CanonicalToolDef as CopilotCanonical, CopilotConfig,
    CopilotConfirmation, CopilotError, CopilotFunctionCall, CopilotMessage, CopilotReference,
    CopilotReferenceType, CopilotResponse, CopilotStreamEvent, CopilotTool, CopilotToolType,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{
    AgentEventKind, CONTRACT_VERSION, Capability, CapabilityRequirement, CapabilityRequirements,
    MinSupport, Outcome, Receipt, SupportLevel, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_emulation::{
    EmulationConfig, EmulationEngine, EmulationStrategy, FidelityLabel, can_emulate,
    compute_fidelity, default_strategy,
};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_gemini_sdk::dialect::{
    self as gemini_dialect, CanonicalToolDef as GeminiCanonical, GeminiCandidate, GeminiConfig,
    GeminiContent, GeminiPart, GeminiResponse, GeminiStreamChunk, GeminiUsageMetadata,
};
use abp_kimi_sdk::dialect::{
    self as kimi_dialect, CanonicalToolDef as KimiCanonical, KimiChoice, KimiChunk,
    KimiChunkChoice, KimiChunkDelta, KimiConfig, KimiMessage, KimiResponse, KimiResponseMessage,
    KimiUsage,
};
use abp_mapping::{Fidelity, MappingMatrix, features, known_rules, validate_mapping};
use abp_openai_sdk::dialect::{
    self as openai_dialect, CanonicalToolDef as OpenAICanonical, OpenAIChoice, OpenAIConfig,
    OpenAIFunctionCall, OpenAIMessage, OpenAIResponse, OpenAIToolCall, OpenAIUsage,
};
use abp_receipt::{ReceiptBuilder, canonicalize, compute_hash, diff_receipts, verify_hash};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn _make_canonical_tool(name: &str, desc: &str) -> IrToolDefinition {
    IrToolDefinition {
        name: name.to_string(),
        description: desc.to_string(),
        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    }
}

fn make_openai_canonical(name: &str, desc: &str) -> OpenAICanonical {
    OpenAICanonical {
        name: name.into(),
        description: desc.into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    }
}

fn make_claude_canonical(name: &str, desc: &str) -> ClaudeCanonical {
    ClaudeCanonical {
        name: name.into(),
        description: desc.into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    }
}

fn make_gemini_canonical(name: &str, desc: &str) -> GeminiCanonical {
    GeminiCanonical {
        name: name.into(),
        description: desc.into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    }
}

fn make_kimi_canonical(name: &str, desc: &str) -> KimiCanonical {
    KimiCanonical {
        name: name.into(),
        description: desc.into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    }
}

fn make_copilot_canonical(name: &str, desc: &str) -> CopilotCanonical {
    CopilotCanonical {
        name: name.into(),
        description: desc.into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    }
}

fn make_codex_canonical(name: &str, desc: &str) -> CodexCanonical {
    CodexCanonical {
        name: name.into(),
        description: desc.into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    }
}

fn build_simple_ir_conversation(task: &str) -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::text(IrRole::User, task)])
}

fn build_multi_turn_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
        IrMessage::text(IrRole::User, "Goodbye"),
    ])
}

fn _build_tool_call_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read the file"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "read_file".into(),
                input: json!({"path": "src/main.rs"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

fn build_receipt_for_backend(backend_id: &str) -> Receipt {
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .build()
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 1: OpenAI → IR → Claude backend mapping
// ═══════════════════════════════════════════════════════════════════════════

mod openai_to_claude {
    use super::*;

    #[test]
    fn openai_messages_to_ir_basic() {
        let msgs = vec![OpenAIMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let conv = abp_openai_sdk::lowering::to_ir(&msgs);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn ir_to_claude_messages_basic() {
        let conv = build_simple_ir_conversation("Hello");
        let claude_msgs = abp_claude_sdk::lowering::from_ir(&conv);
        assert_eq!(claude_msgs.len(), 1);
        assert_eq!(claude_msgs[0].role, "user");
    }

    #[test]
    fn openai_to_ir_to_claude_roundtrip_text() {
        let openai_msgs = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("Be helpful.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Hi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let ir = abp_openai_sdk::lowering::to_ir(&openai_msgs);
        assert_eq!(ir.len(), 2);
        let claude_msgs = abp_claude_sdk::lowering::from_ir(&ir);
        assert!(claude_msgs.iter().any(|m| m.role == "user"));
    }

    #[test]
    fn openai_to_ir_to_claude_tool_call() {
        let openai_msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"main.rs"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }];
        let ir = abp_openai_sdk::lowering::to_ir(&openai_msgs);
        assert_eq!(ir.messages[0].role, IrRole::Assistant);
        let tool_uses: Vec<_> = ir.messages[0]
            .content
            .iter()
            .filter(|b| matches!(b, IrContentBlock::ToolUse { .. }))
            .collect();
        assert_eq!(tool_uses.len(), 1);
    }

    #[test]
    fn openai_to_ir_to_claude_tool_result() {
        let openai_msgs = vec![OpenAIMessage {
            role: "tool".into(),
            content: Some("result data".into()),
            tool_calls: None,
            tool_call_id: Some("call_1".into()),
        }];
        let ir = abp_openai_sdk::lowering::to_ir(&openai_msgs);
        assert_eq!(ir.messages[0].role, IrRole::Tool);
    }

    #[test]
    fn openai_model_to_canonical_format() {
        let canonical = openai_dialect::to_canonical_model("gpt-4o");
        assert_eq!(canonical, "openai/gpt-4o");
    }

    #[test]
    fn claude_model_from_canonical_format() {
        let vendor = claude_dialect::from_canonical_model("anthropic/claude-sonnet-4-20250514");
        assert_eq!(vendor, "claude-sonnet-4-20250514");
    }

    #[test]
    fn openai_work_order_maps_to_openai_request() {
        let wo = WorkOrderBuilder::new("Refactor code").build();
        let config = OpenAIConfig::default();
        let req = openai_dialect::map_work_order(&wo, &config);
        assert_eq!(req.model, "gpt-4o");
        assert!(
            req.messages
                .iter()
                .any(|m| m.content.as_deref() == Some("Refactor code"))
        );
    }

    #[test]
    fn claude_work_order_maps_to_claude_request() {
        let wo = WorkOrderBuilder::new("Fix bug").build();
        let config = ClaudeConfig::default();
        let req = claude_dialect::map_work_order(&wo, &config);
        assert!(req.messages.iter().any(|m| m.role == "user"));
    }

    #[test]
    fn openai_response_maps_to_agent_events() {
        let resp = OpenAIResponse {
            id: "cmpl_1".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIMessage {
                    role: "assistant".into(),
                    content: Some("Hello!".into()),
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
        let events = openai_dialect::map_response(&resp);
        assert!(!events.is_empty());
        assert!(events.iter().any(
            |e| matches!(&e.kind, AgentEventKind::AssistantMessage { text } if text == "Hello!")
        ));
    }

    #[test]
    fn claude_response_maps_to_agent_events() {
        let resp = ClaudeResponse {
            id: "msg_1".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![ClaudeContentBlock::Text { text: "Hi!".into() }],
            stop_reason: Some("end_turn".into()),
            usage: Some(ClaudeUsage {
                input_tokens: 10,
                output_tokens: 5,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        };
        let events = claude_dialect::map_response(&resp);
        assert!(!events.is_empty());
    }

    #[test]
    fn openai_multi_turn_to_ir_preserves_order() {
        let msgs = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("sys".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("u1".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: Some("a1".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("u2".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let ir = abp_openai_sdk::lowering::to_ir(&msgs);
        assert_eq!(ir.len(), 4);
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[3].role, IrRole::User);
    }

    #[test]
    fn ir_to_claude_preserves_multi_turn() {
        let ir = build_multi_turn_ir();
        let claude_msgs = abp_claude_sdk::lowering::from_ir(&ir);
        // Claude from_ir skips system messages
        assert_eq!(claude_msgs.len(), 3);
    }

    #[test]
    fn openai_empty_content_message() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: None,
            tool_call_id: None,
        }];
        let ir = abp_openai_sdk::lowering::to_ir(&msgs);
        assert!(ir.messages[0].content.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 2: Claude → IR → Gemini backend mapping
// ═══════════════════════════════════════════════════════════════════════════

mod claude_to_gemini {
    use super::*;

    #[test]
    fn claude_messages_to_ir() {
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "Hello".into(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&msgs, None);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
    }

    #[test]
    fn ir_to_gemini_contents() {
        let conv = build_simple_ir_conversation("Explain Rust");
        let contents = abp_gemini_sdk::lowering::from_ir(&conv);
        assert!(!contents.is_empty());
    }

    #[test]
    fn claude_to_ir_to_gemini_text_roundtrip() {
        let claude_msgs = vec![
            ClaudeMessage {
                role: "user".into(),
                content: "Hi".into(),
            },
            ClaudeMessage {
                role: "assistant".into(),
                content: "Hello!".into(),
            },
        ];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        assert_eq!(ir.len(), 2);
        let gemini_contents = abp_gemini_sdk::lowering::from_ir(&ir);
        assert!(!gemini_contents.is_empty());
    }

    #[test]
    fn claude_tool_use_to_ir() {
        let blocks = vec![ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "search".into(),
            input: json!({"query": "rust"}),
        }];
        let claude_msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        assert!(
            ir.messages[0]
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolUse { .. }))
        );
    }

    #[test]
    fn claude_system_prompt_extraction() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You are a coder."),
            IrMessage::text(IrRole::User, "Hi"),
        ]);
        let system = abp_claude_sdk::lowering::extract_system_prompt(&ir);
        assert!(system.is_some());
        assert!(system.unwrap().contains("coder"));
    }

    #[test]
    fn gemini_system_instruction_extraction() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be concise."),
            IrMessage::text(IrRole::User, "Explain"),
        ]);
        let sys = abp_gemini_sdk::lowering::extract_system_instruction(&ir);
        assert!(sys.is_some());
    }

    #[test]
    fn gemini_work_order_maps_to_request() {
        let wo = WorkOrderBuilder::new("Optimize").build();
        let config = GeminiConfig::default();
        let req = gemini_dialect::map_work_order(&wo, &config);
        assert!(!req.contents.is_empty());
    }

    #[test]
    fn gemini_response_maps_to_events() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("result".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 5,
                total_token_count: 15,
            }),
        };
        let events = gemini_dialect::map_response(&resp);
        assert!(!events.is_empty());
    }

    #[test]
    fn claude_to_gemini_model_mapping() {
        let canonical = claude_dialect::to_canonical_model("claude-sonnet-4-20250514");
        assert!(canonical.starts_with("anthropic/"));
        let gemini_model = gemini_dialect::from_canonical_model(&canonical);
        // Gemini strips "google/" prefix only; anthropic/ is not stripped
        assert_eq!(gemini_model, canonical);
    }

    #[test]
    fn claude_thinking_block_to_ir() {
        let blocks = vec![ClaudeContentBlock::Thinking {
            thinking: "Let me think...".into(),
            signature: Some("sig".into()),
        }];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&msgs, None);
        assert!(
            ir.messages[0]
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        );
    }

    #[test]
    fn claude_empty_conversation_to_ir() {
        let ir = abp_claude_sdk::lowering::to_ir(&[], None);
        assert!(ir.is_empty());
    }

    #[test]
    fn gemini_stream_chunk_maps_to_events() {
        let chunk = GeminiStreamChunk {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("partial".into())],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        let events = gemini_dialect::map_stream_chunk(&chunk);
        assert!(!events.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 3: Cross-SDK tool definition translation
// ═══════════════════════════════════════════════════════════════════════════

mod tool_translation {
    use super::*;

    #[test]
    fn openai_canonical_to_openai_tool_def() {
        let canonical = make_openai_canonical("read_file", "Read a file");
        let tool = openai_dialect::tool_def_to_openai(&canonical);
        assert_eq!(tool.tool_type, "function");
        assert_eq!(tool.function.name, "read_file");
    }

    #[test]
    fn openai_tool_def_roundtrip() {
        let canonical = make_openai_canonical("write_file", "Write a file");
        let tool = openai_dialect::tool_def_to_openai(&canonical);
        let back = openai_dialect::tool_def_from_openai(&tool);
        assert_eq!(back.name, canonical.name);
        assert_eq!(back.description, canonical.description);
    }

    #[test]
    fn claude_canonical_to_claude_tool_def() {
        let canonical = make_claude_canonical("bash", "Execute bash");
        let tool = claude_dialect::tool_def_to_claude(&canonical);
        assert_eq!(tool.name, "bash");
    }

    #[test]
    fn claude_tool_def_roundtrip() {
        let canonical = make_claude_canonical("grep", "Search code");
        let tool = claude_dialect::tool_def_to_claude(&canonical);
        let back = claude_dialect::tool_def_from_claude(&tool);
        assert_eq!(back.name, canonical.name);
    }

    #[test]
    fn gemini_canonical_to_gemini_function_declaration() {
        let canonical = make_gemini_canonical("search", "Web search");
        let decl = gemini_dialect::tool_def_to_gemini(&canonical);
        assert_eq!(decl.name, "search");
    }

    #[test]
    fn gemini_function_declaration_roundtrip() {
        let canonical = make_gemini_canonical("edit_file", "Edit a file");
        let decl = gemini_dialect::tool_def_to_gemini(&canonical);
        let back = gemini_dialect::tool_def_from_gemini(&decl);
        assert_eq!(back.name, canonical.name);
    }

    #[test]
    fn kimi_canonical_to_kimi_tool_def() {
        let canonical = make_kimi_canonical("web_search", "Search internet");
        let tool = kimi_dialect::tool_def_to_kimi(&canonical);
        assert_eq!(tool.tool_type, "function");
        assert_eq!(tool.function.name, "web_search");
    }

    #[test]
    fn kimi_tool_def_roundtrip() {
        let canonical = make_kimi_canonical("read_file", "Read a file");
        let tool = kimi_dialect::tool_def_to_kimi(&canonical);
        let back = kimi_dialect::tool_def_from_kimi(&tool);
        assert_eq!(back.name, canonical.name);
    }

    #[test]
    fn copilot_canonical_to_copilot_tool() {
        let canonical = make_copilot_canonical("run_tests", "Run tests");
        let tool = copilot_dialect::tool_def_to_copilot(&canonical);
        assert_eq!(tool.tool_type, CopilotToolType::Function);
        assert!(tool.function.is_some());
    }

    #[test]
    fn copilot_tool_roundtrip() {
        let canonical = make_copilot_canonical("deploy", "Deploy app");
        let tool = copilot_dialect::tool_def_to_copilot(&canonical);
        let back = copilot_dialect::tool_def_from_copilot(&tool);
        assert!(back.is_some());
        assert_eq!(back.unwrap().name, canonical.name);
    }

    #[test]
    fn codex_canonical_to_codex_tool_def() {
        let canonical = make_codex_canonical("execute", "Run code");
        let tool = codex_dialect::tool_def_to_codex(&canonical);
        assert_eq!(tool.tool_type, "function");
    }

    #[test]
    fn codex_tool_def_roundtrip() {
        let canonical = make_codex_canonical("search", "Search");
        let tool = codex_dialect::tool_def_to_codex(&canonical);
        let back = codex_dialect::tool_def_from_codex(&tool);
        assert_eq!(back.name, canonical.name);
    }

    #[test]
    fn openai_to_claude_tool_translation() {
        let openai_canonical = make_openai_canonical("tool_a", "Tool A");
        let openai_tool = openai_dialect::tool_def_to_openai(&openai_canonical);
        let back = openai_dialect::tool_def_from_openai(&openai_tool);
        let claude_canonical = ClaudeCanonical {
            name: back.name,
            description: back.description,
            parameters_schema: back.parameters_schema,
        };
        let claude_tool = claude_dialect::tool_def_to_claude(&claude_canonical);
        assert_eq!(claude_tool.name, "tool_a");
    }

    #[test]
    fn claude_to_gemini_tool_translation() {
        let claude_canonical = make_claude_canonical("tool_b", "Tool B");
        let claude_tool = claude_dialect::tool_def_to_claude(&claude_canonical);
        let back = claude_dialect::tool_def_from_claude(&claude_tool);
        let gemini_canonical = GeminiCanonical {
            name: back.name,
            description: back.description,
            parameters_schema: back.parameters_schema,
        };
        let gemini_decl = gemini_dialect::tool_def_to_gemini(&gemini_canonical);
        assert_eq!(gemini_decl.name, "tool_b");
    }

    #[test]
    fn gemini_to_kimi_tool_translation() {
        let gemini_canonical = make_gemini_canonical("tool_c", "Tool C");
        let gemini_decl = gemini_dialect::tool_def_to_gemini(&gemini_canonical);
        let back = gemini_dialect::tool_def_from_gemini(&gemini_decl);
        let kimi_canonical = KimiCanonical {
            name: back.name,
            description: back.description,
            parameters_schema: back.parameters_schema,
        };
        let kimi_tool = kimi_dialect::tool_def_to_kimi(&kimi_canonical);
        assert_eq!(kimi_tool.function.name, "tool_c");
    }

    #[test]
    fn kimi_to_copilot_tool_translation() {
        let kimi_canonical = make_kimi_canonical("tool_d", "Tool D");
        let kimi_tool = kimi_dialect::tool_def_to_kimi(&kimi_canonical);
        let back = kimi_dialect::tool_def_from_kimi(&kimi_tool);
        let copilot_canonical = CopilotCanonical {
            name: back.name,
            description: back.description,
            parameters_schema: back.parameters_schema,
        };
        let copilot_tool = copilot_dialect::tool_def_to_copilot(&copilot_canonical);
        assert_eq!(copilot_tool.function.unwrap().name, "tool_d");
    }

    #[test]
    fn all_sdks_preserve_parameter_schema() {
        let _schema = json!({"type": "object", "properties": {"x": {"type": "integer"}}});
        let oai = make_openai_canonical("fn", "d");
        let oai_tool = openai_dialect::tool_def_to_openai(&oai);
        let oai_back = openai_dialect::tool_def_from_openai(&oai_tool);
        assert_eq!(oai_back.parameters_schema, oai.parameters_schema);

        let cl = make_claude_canonical("fn", "d");
        let cl_tool = claude_dialect::tool_def_to_claude(&cl);
        let cl_back = claude_dialect::tool_def_from_claude(&cl_tool);
        assert_eq!(cl_back.parameters_schema, cl.parameters_schema);
    }

    #[test]
    fn copilot_confirmation_tool_has_no_function() {
        let tool = CopilotTool {
            tool_type: CopilotToolType::Confirmation,
            function: None,
            confirmation: Some(CopilotConfirmation {
                id: "c1".into(),
                title: "Confirm".into(),
                message: "Do it?".into(),
                accepted: None,
            }),
        };
        let canonical = copilot_dialect::tool_def_from_copilot(&tool);
        assert!(canonical.is_none());
    }

    #[test]
    fn tool_names_preserved_across_all_six_sdks() {
        let name = "my_complex_tool";
        let desc = "A complex tool";

        let o = openai_dialect::tool_def_from_openai(&openai_dialect::tool_def_to_openai(
            &make_openai_canonical(name, desc),
        ));
        assert_eq!(o.name, name);

        let c = claude_dialect::tool_def_from_claude(&claude_dialect::tool_def_to_claude(
            &make_claude_canonical(name, desc),
        ));
        assert_eq!(c.name, name);

        let g = gemini_dialect::tool_def_from_gemini(&gemini_dialect::tool_def_to_gemini(
            &make_gemini_canonical(name, desc),
        ));
        assert_eq!(g.name, name);

        let k = kimi_dialect::tool_def_from_kimi(&kimi_dialect::tool_def_to_kimi(
            &make_kimi_canonical(name, desc),
        ));
        assert_eq!(k.name, name);

        let co = copilot_dialect::tool_def_from_copilot(&copilot_dialect::tool_def_to_copilot(
            &make_copilot_canonical(name, desc),
        ));
        assert_eq!(co.unwrap().name, name);

        let cd = codex_dialect::tool_def_from_codex(&codex_dialect::tool_def_to_codex(
            &make_codex_canonical(name, desc),
        ));
        assert_eq!(cd.name, name);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 4: Cross-SDK streaming event mapping
// ═══════════════════════════════════════════════════════════════════════════

mod streaming_events {
    use super::*;

    #[test]
    fn claude_stream_text_delta_maps_to_event() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "Hello".into(),
            },
        };
        let events = claude_dialect::map_stream_event(&event);
        assert!(!events.is_empty());
        assert!(events.iter().any(
            |e| matches!(&e.kind, AgentEventKind::AssistantDelta { text } if text == "Hello")
        ));
    }

    #[test]
    fn claude_stream_message_stop() {
        let event = ClaudeStreamEvent::MessageStop {};
        let events = claude_dialect::map_stream_event(&event);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
        );
    }

    #[test]
    fn gemini_stream_chunk_text() {
        let chunk = GeminiStreamChunk {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("streaming text".into())],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        let events = gemini_dialect::map_stream_event(&chunk);
        assert!(!events.is_empty());
    }

    #[test]
    fn kimi_stream_chunk_text() {
        let chunk = KimiChunk {
            id: "chunk_1".into(),
            object: "chat.completion.chunk".into(),
            created: 1234567890,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChunkChoice {
                index: 0,
                delta: KimiChunkDelta {
                    role: None,
                    content: Some("hi".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        };
        let events = kimi_dialect::map_stream_event(&chunk);
        assert!(
            events.iter().any(
                |e| matches!(&e.kind, AgentEventKind::AssistantDelta { text } if text == "hi")
            )
        );
    }

    #[test]
    fn kimi_stream_finish_reason_maps_to_run_completed() {
        let chunk = KimiChunk {
            id: "chunk_2".into(),
            object: "chat.completion.chunk".into(),
            created: 1234567890,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChunkChoice {
                index: 0,
                delta: KimiChunkDelta::default(),
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            refs: None,
        };
        let events = kimi_dialect::map_stream_event(&chunk);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
        );
    }

    #[test]
    fn copilot_stream_text_delta() {
        let event = CopilotStreamEvent::TextDelta {
            text: "streaming".into(),
        };
        let events = copilot_dialect::map_stream_event(&event);
        assert!(events.iter().any(
            |e| matches!(&e.kind, AgentEventKind::AssistantDelta { text } if text == "streaming")
        ));
    }

    #[test]
    fn copilot_stream_done() {
        let event = CopilotStreamEvent::Done {};
        let events = copilot_dialect::map_stream_event(&event);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
        );
    }

    #[test]
    fn copilot_stream_errors_map_to_error_events() {
        let event = CopilotStreamEvent::CopilotErrors {
            errors: vec![CopilotError {
                error_type: "rate_limit".into(),
                message: "Too many requests".into(),
                code: Some("429".into()),
                identifier: None,
            }],
        };
        let events = copilot_dialect::map_stream_event(&event);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::Error { .. }))
        );
    }

    #[test]
    fn copilot_stream_function_call() {
        let event = CopilotStreamEvent::FunctionCall {
            function_call: CopilotFunctionCall {
                name: "run_test".into(),
                arguments: r#"{"suite":"all"}"#.into(),
                id: Some("fc_1".into()),
            },
        };
        let events = copilot_dialect::map_stream_event(&event);
        assert!(events.iter().any(|e| matches!(&e.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "run_test")));
    }

    #[test]
    fn copilot_stream_references() {
        let event = CopilotStreamEvent::CopilotReferences {
            references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "f1".into(),
                data: json!({"path": "src/main.rs"}),
                metadata: None,
            }],
        };
        let events = copilot_dialect::map_stream_event(&event);
        assert!(!events.is_empty());
    }

    #[test]
    fn claude_passthrough_roundtrip() {
        let original = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "test".into(),
            },
        };
        let wrapped = claude_dialect::to_passthrough_event(&original);
        let recovered = claude_dialect::from_passthrough_event(&wrapped);
        assert_eq!(recovered, Some(original));
    }

    #[test]
    fn copilot_passthrough_roundtrip() {
        let original = CopilotStreamEvent::TextDelta {
            text: "test".into(),
        };
        let wrapped = copilot_dialect::to_passthrough_event(&original);
        let recovered = copilot_dialect::from_passthrough_event(&wrapped);
        assert_eq!(recovered, Some(original));
    }

    #[test]
    fn claude_verify_passthrough_fidelity() {
        let events = vec![
            ClaudeStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ClaudeStreamDelta::TextDelta { text: "a".into() },
            },
            ClaudeStreamEvent::MessageStop {},
        ];
        assert!(claude_dialect::verify_passthrough_fidelity(&events));
    }

    #[test]
    fn copilot_verify_passthrough_fidelity() {
        let events = vec![
            CopilotStreamEvent::TextDelta { text: "x".into() },
            CopilotStreamEvent::Done {},
        ];
        assert!(copilot_dialect::verify_passthrough_fidelity(&events));
    }

    #[test]
    fn copilot_empty_references_stream_produces_no_events() {
        let event = CopilotStreamEvent::CopilotReferences { references: vec![] };
        let events = copilot_dialect::map_stream_event(&event);
        assert!(events.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 5: Cross-SDK error code translation
// ═══════════════════════════════════════════════════════════════════════════

mod error_translation {
    use super::*;

    #[test]
    fn backend_not_found_error() {
        let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend: openai");
        assert_eq!(err.code, ErrorCode::BackendNotFound);
        assert_eq!(err.category(), ErrorCategory::Backend);
    }

    #[test]
    fn backend_timeout_with_context() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
            .with_context("backend", "claude")
            .with_context("timeout_ms", 30000);
        assert_eq!(err.context.len(), 2);
        assert_eq!(err.context["backend"], json!("claude"));
    }

    #[test]
    fn capability_unsupported_error() {
        let err = AbpError::new(
            ErrorCode::CapabilityUnsupported,
            "extended_thinking not available",
        );
        assert_eq!(err.category(), ErrorCategory::Capability);
    }

    #[test]
    fn dialect_unknown_error() {
        let err = AbpError::new(ErrorCode::DialectUnknown, "unknown dialect: foo");
        assert_eq!(err.category(), ErrorCategory::Dialect);
        assert_eq!(err.code.as_str(), "DIALECT_UNKNOWN");
    }

    #[test]
    fn dialect_mapping_failed_error() {
        let err = AbpError::new(
            ErrorCode::DialectMappingFailed,
            "cannot map thinking to openai",
        );
        assert_eq!(err.code, ErrorCode::DialectMappingFailed);
    }

    #[test]
    fn error_dto_roundtrip() {
        let err = AbpError::new(ErrorCode::IrLoweringFailed, "bad IR")
            .with_context("source", "openai")
            .with_context("target", "claude");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back);
    }

    #[test]
    fn error_dto_to_abp_error_conversion() {
        let dto = AbpErrorDto {
            code: ErrorCode::ReceiptHashMismatch,
            message: "hash mismatch".into(),
            context: BTreeMap::new(),
            source_message: None,
        };
        let err: AbpError = dto.into();
        assert_eq!(err.code, ErrorCode::ReceiptHashMismatch);
    }

    #[test]
    fn all_error_categories_have_codes() {
        let categories = vec![
            ErrorCategory::Protocol,
            ErrorCategory::Backend,
            ErrorCategory::Capability,
            ErrorCategory::Policy,
            ErrorCategory::Workspace,
            ErrorCategory::Ir,
            ErrorCategory::Receipt,
            ErrorCategory::Dialect,
            ErrorCategory::Config,
            ErrorCategory::Internal,
        ];
        for cat in &categories {
            let s = cat.to_string();
            assert!(!s.is_empty());
        }
    }

    #[test]
    fn protocol_error_codes() {
        assert_eq!(
            ErrorCode::ProtocolInvalidEnvelope.category(),
            ErrorCategory::Protocol
        );
        assert_eq!(
            ErrorCode::ProtocolUnexpectedMessage.category(),
            ErrorCategory::Protocol
        );
        assert_eq!(
            ErrorCode::ProtocolVersionMismatch.category(),
            ErrorCategory::Protocol
        );
    }

    #[test]
    fn policy_error_codes() {
        assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
        assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
    }

    #[test]
    fn workspace_error_codes() {
        assert_eq!(
            ErrorCode::WorkspaceInitFailed.category(),
            ErrorCategory::Workspace
        );
        assert_eq!(
            ErrorCode::WorkspaceStagingFailed.category(),
            ErrorCategory::Workspace
        );
    }

    #[test]
    fn receipt_error_codes() {
        assert_eq!(
            ErrorCode::ReceiptHashMismatch.category(),
            ErrorCategory::Receipt
        );
        assert_eq!(
            ErrorCode::ReceiptChainBroken.category(),
            ErrorCategory::Receipt
        );
    }

    #[test]
    fn error_display_includes_code() {
        let err = AbpError::new(ErrorCode::ConfigInvalid, "bad config");
        let display = err.to_string();
        assert!(display.contains("CONFIG_INVALID"));
        assert!(display.contains("bad config"));
    }

    #[test]
    fn error_code_serde_roundtrip() {
        let code = ErrorCode::BackendCrashed;
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(code, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 6: Receipt generation across SDK combinations
// ═══════════════════════════════════════════════════════════════════════════

mod receipt_generation {
    use super::*;

    #[test]
    fn receipt_builder_for_mock_backend() {
        let r = build_receipt_for_backend("mock");
        assert_eq!(r.backend.id, "mock");
        assert_eq!(r.outcome, Outcome::Complete);
    }

    #[test]
    fn receipt_builder_for_openai_backend() {
        let r = ReceiptBuilder::new("sidecar:openai")
            .outcome(Outcome::Complete)
            .backend_version("0.1")
            .adapter_version("0.1")
            .build();
        assert_eq!(r.backend.id, "sidecar:openai");
    }

    #[test]
    fn receipt_builder_for_claude_backend() {
        let r = ReceiptBuilder::new("sidecar:claude")
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(r.backend.id, "sidecar:claude");
    }

    #[test]
    fn receipt_builder_for_gemini_backend() {
        let r = ReceiptBuilder::new("sidecar:gemini")
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(r.backend.id, "sidecar:gemini");
    }

    #[test]
    fn receipt_builder_for_kimi_backend() {
        let r = ReceiptBuilder::new("sidecar:kimi")
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(r.backend.id, "sidecar:kimi");
    }

    #[test]
    fn receipt_builder_for_copilot_backend() {
        let r = ReceiptBuilder::new("sidecar:copilot")
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(r.backend.id, "sidecar:copilot");
    }

    #[test]
    fn receipt_builder_for_codex_backend() {
        let r = ReceiptBuilder::new("sidecar:codex")
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(r.backend.id, "sidecar:codex");
    }

    #[test]
    fn receipt_hash_is_deterministic() {
        let r = build_receipt_for_backend("mock");
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn receipt_hash_verification() {
        let mut r = build_receipt_for_backend("openai");
        r.receipt_sha256 = Some(compute_hash(&r).unwrap());
        assert!(verify_hash(&r));
    }

    #[test]
    fn receipt_tampered_hash_fails_verification() {
        let mut r = build_receipt_for_backend("claude");
        r.receipt_sha256 = Some("tampered".into());
        assert!(!verify_hash(&r));
    }

    #[test]
    fn receipt_canonicalization_is_stable() {
        let r = build_receipt_for_backend("mock");
        let j1 = canonicalize(&r).unwrap();
        let j2 = canonicalize(&r).unwrap();
        assert_eq!(j1, j2);
    }

    #[test]
    fn different_backends_produce_different_hashes() {
        let r1 = build_receipt_for_backend("mock");
        let r2 = build_receipt_for_backend("sidecar:openai");
        let h1 = compute_hash(&r1).unwrap();
        let h2 = compute_hash(&r2).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn receipt_diff_detects_changes() {
        let r1 = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let r2 = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
        let diff = diff_receipts(&r1, &r2);
        assert!(!diff.changes.is_empty());
    }

    #[test]
    fn receipt_diff_identical_is_empty() {
        let r1 = build_receipt_for_backend("mock");
        let r2 = r1.clone();
        let diff = diff_receipts(&r1, &r2);
        assert!(diff.changes.is_empty());
    }

    #[test]
    fn receipt_with_hash_method() {
        let r = build_receipt_for_backend("mock");
        let hashed = r.with_hash().unwrap();
        assert!(hashed.receipt_sha256.is_some());
        assert!(verify_hash(&hashed));
    }

    #[test]
    fn receipt_no_hash_passes_verification() {
        let r = build_receipt_for_backend("mock");
        assert!(r.receipt_sha256.is_none());
        assert!(verify_hash(&r));
    }

    #[test]
    fn receipt_contract_version() {
        let r = build_receipt_for_backend("mock");
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn receipt_partial_outcome() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Partial)
            .build();
        assert_eq!(r.outcome, Outcome::Partial);
    }

    #[test]
    fn receipt_failed_outcome() {
        let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
        assert_eq!(r.outcome, Outcome::Failed);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 7: WorkOrder creation with different SDK dialects
// ═══════════════════════════════════════════════════════════════════════════

mod work_order_creation {
    use super::*;

    #[test]
    fn work_order_basic_creation() {
        let wo = WorkOrderBuilder::new("Fix the bug").build();
        assert_eq!(wo.task, "Fix the bug");
    }

    #[test]
    fn work_order_with_model() {
        let wo = WorkOrderBuilder::new("task").model("gpt-4o").build();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn work_order_maps_to_openai_request() {
        let wo = WorkOrderBuilder::new("Refactor").model("gpt-4o").build();
        let config = OpenAIConfig::default();
        let req = openai_dialect::map_work_order(&wo, &config);
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn work_order_maps_to_claude_request() {
        let wo = WorkOrderBuilder::new("Debug").build();
        let config = ClaudeConfig::default();
        let req = claude_dialect::map_work_order(&wo, &config);
        assert_eq!(req.model, claude_dialect::DEFAULT_MODEL);
    }

    #[test]
    fn work_order_maps_to_gemini_request() {
        let wo = WorkOrderBuilder::new("Optimize").build();
        let config = GeminiConfig::default();
        let req = gemini_dialect::map_work_order(&wo, &config);
        assert!(!req.contents.is_empty());
    }

    #[test]
    fn work_order_maps_to_kimi_request() {
        let wo = WorkOrderBuilder::new("Search web").build();
        let config = KimiConfig::default();
        let req = kimi_dialect::map_work_order(&wo, &config);
        assert_eq!(req.model, kimi_dialect::DEFAULT_MODEL);
    }

    #[test]
    fn work_order_maps_to_copilot_request() {
        let wo = WorkOrderBuilder::new("Review code").build();
        let config = CopilotConfig::default();
        let req = copilot_dialect::map_work_order(&wo, &config);
        assert!(!req.messages.is_empty());
    }

    #[test]
    fn work_order_maps_to_codex_request() {
        let wo = WorkOrderBuilder::new("Execute").build();
        let config = CodexConfig::default();
        let req = codex_dialect::map_work_order(&wo, &config);
        assert_eq!(req.model, codex_dialect::DEFAULT_MODEL);
    }

    #[test]
    fn work_order_model_override_respected_by_openai() {
        let wo = WorkOrderBuilder::new("task").model("o1").build();
        let config = OpenAIConfig::default();
        let req = openai_dialect::map_work_order(&wo, &config);
        assert_eq!(req.model, "o1");
    }

    #[test]
    fn work_order_model_override_respected_by_claude() {
        let wo = WorkOrderBuilder::new("task")
            .model("claude-sonnet-4-20250514")
            .build();
        let config = ClaudeConfig::default();
        let req = claude_dialect::map_work_order(&wo, &config);
        assert_eq!(req.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn work_order_model_override_respected_by_kimi() {
        let wo = WorkOrderBuilder::new("task")
            .model("moonshot-v1-128k")
            .build();
        let config = KimiConfig::default();
        let req = kimi_dialect::map_work_order(&wo, &config);
        assert_eq!(req.model, "moonshot-v1-128k");
    }

    #[test]
    fn work_order_with_context_snippets_for_kimi() {
        let wo = WorkOrderBuilder::new("task")
            .context(abp_core::ContextPacket {
                files: vec![],
                snippets: vec![abp_core::ContextSnippet {
                    name: "helper.rs".into(),
                    content: "fn help() {}".into(),
                }],
            })
            .build();
        let config = KimiConfig::default();
        let req = kimi_dialect::map_work_order(&wo, &config);
        let content = req.messages[0].content.as_deref().unwrap_or("");
        assert!(content.contains("helper.rs"));
    }

    #[test]
    fn work_order_with_context_files_for_copilot() {
        let wo = WorkOrderBuilder::new("task")
            .context(abp_core::ContextPacket {
                files: vec!["src/main.rs".into()],
                snippets: vec![],
            })
            .build();
        let config = CopilotConfig::default();
        let req = copilot_dialect::map_work_order(&wo, &config);
        assert!(!req.references.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 8: Multi-SDK capability comparison
// ═══════════════════════════════════════════════════════════════════════════

mod capability_comparison {
    use super::*;

    #[test]
    fn openai_has_streaming_native() {
        let m = openai_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn claude_has_streaming_native() {
        let m = claude_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn gemini_has_streaming_native() {
        let m = gemini_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn kimi_has_streaming_native() {
        let m = kimi_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn copilot_has_streaming_native() {
        let m = copilot_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn openai_structured_output_is_native() {
        let m = openai_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::StructuredOutputJsonSchema),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn claude_structured_output_is_native() {
        let m = claude_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::StructuredOutputJsonSchema),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn copilot_mcp_is_unsupported() {
        let m = copilot_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::McpClient),
            Some(SupportLevel::Unsupported)
        ));
        assert!(matches!(
            m.get(&Capability::McpServer),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn kimi_mcp_is_unsupported() {
        let m = kimi_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::McpClient),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn negotiate_openai_streaming_requirement() {
        let m = openai_dialect::capability_manifest();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        let result = negotiate(&m, &reqs);
        assert!(result.is_compatible());
        assert_eq!(result.native.len(), 1);
    }

    #[test]
    fn negotiate_claude_tool_requirement() {
        let m = claude_dialect::capability_manifest();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            }],
        };
        let result = negotiate(&m, &reqs);
        assert!(result.is_compatible());
    }

    #[test]
    fn negotiate_kimi_unsupported_capability() {
        let m = kimi_dialect::capability_manifest();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        };
        let result = negotiate(&m, &reqs);
        assert!(!result.is_compatible());
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn generate_compatibility_report_for_openai() {
        let m = openai_dialect::capability_manifest();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        let neg = negotiate(&m, &reqs);
        let report = generate_report(&neg);
        assert!(report.compatible);
        assert_eq!(report.native_count, 1);
    }

    #[test]
    fn check_capability_native_for_openai_streaming() {
        let m = openai_dialect::capability_manifest();
        let level = check_capability(&m, &Capability::Streaming);
        assert!(matches!(level, CapSupportLevel::Native));
    }

    #[test]
    fn check_capability_unsupported_for_missing() {
        let m = kimi_dialect::capability_manifest();
        let level = check_capability(&m, &Capability::SessionResume);
        assert!(matches!(level, CapSupportLevel::Unsupported));
    }

    #[test]
    fn all_manifests_are_nonempty() {
        assert!(!openai_dialect::capability_manifest().is_empty());
        assert!(!claude_dialect::capability_manifest().is_empty());
        assert!(!gemini_dialect::capability_manifest().is_empty());
        assert!(!kimi_dialect::capability_manifest().is_empty());
        assert!(!copilot_dialect::capability_manifest().is_empty());
        assert!(!codex_dialect::capability_manifest().is_empty());
    }

    #[test]
    fn kimi_web_search_is_native() {
        let m = kimi_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolWebSearch),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn copilot_web_search_is_native() {
        let m = copilot_dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolWebSearch),
            Some(SupportLevel::Native)
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 9: Dialect detection and mapping rules
// ═══════════════════════════════════════════════════════════════════════════

mod dialect_and_mapping {
    use super::*;

    #[test]
    fn dialect_all_returns_six_dialects() {
        let all = Dialect::all();
        assert_eq!(all.len(), 6);
    }

    #[test]
    fn dialect_labels_are_nonempty() {
        for &d in Dialect::all() {
            assert!(!d.label().is_empty());
        }
    }

    #[test]
    fn dialect_display() {
        assert_eq!(Dialect::OpenAi.to_string(), "OpenAI");
        assert_eq!(Dialect::Claude.to_string(), "Claude");
        assert_eq!(Dialect::Gemini.to_string(), "Gemini");
        assert_eq!(Dialect::Codex.to_string(), "Codex");
        assert_eq!(Dialect::Kimi.to_string(), "Kimi");
        assert_eq!(Dialect::Copilot.to_string(), "Copilot");
    }

    #[test]
    fn dialect_serde_roundtrip() {
        for &d in Dialect::all() {
            let json = serde_json::to_string(&d).unwrap();
            let back: Dialect = serde_json::from_str(&json).unwrap();
            assert_eq!(d, back);
        }
    }

    #[test]
    fn known_rules_is_nonempty() {
        let reg = known_rules();
        assert!(!reg.is_empty());
    }

    #[test]
    fn known_rules_has_openai_to_claude_tool_use() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn known_rules_same_dialect_is_always_lossless() {
        let reg = known_rules();
        for &d in Dialect::all() {
            let rule = reg.lookup(d, d, features::TOOL_USE);
            assert!(rule.is_some(), "missing self-rule for {d:?}");
            assert!(rule.unwrap().fidelity.is_lossless());
        }
    }

    #[test]
    fn validate_mapping_openai_to_claude() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::TOOL_USE.into(), features::STREAMING.into()],
        );
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn mapping_matrix_from_registry() {
        let reg = known_rules();
        let matrix = MappingMatrix::from_registry(&reg);
        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn mapping_matrix_self_dialect_supported() {
        let reg = known_rules();
        let matrix = MappingMatrix::from_registry(&reg);
        for &d in Dialect::all() {
            assert!(matrix.is_supported(d, d), "self-mapping missing for {d:?}");
        }
    }

    #[test]
    fn mapping_registry_rank_targets() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE, features::STREAMING]);
        assert!(!ranked.is_empty());
    }

    #[test]
    fn fidelity_lossless_check() {
        assert!(Fidelity::Lossless.is_lossless());
        assert!(!Fidelity::Lossless.is_unsupported());
    }

    #[test]
    fn fidelity_lossy_check() {
        let f = Fidelity::LossyLabeled {
            warning: "some loss".into(),
        };
        assert!(!f.is_lossless());
        assert!(!f.is_unsupported());
    }

    #[test]
    fn fidelity_unsupported_check() {
        let f = Fidelity::Unsupported {
            reason: "no mapping".into(),
        };
        assert!(!f.is_lossless());
        assert!(f.is_unsupported());
    }

    #[test]
    fn validate_mapping_with_empty_feature() {
        let reg = known_rules();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].errors.is_empty());
    }

    #[test]
    fn validate_mapping_unknown_feature() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["nonexistent_feature".into()],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 10: Model name mapping across SDKs
// ═══════════════════════════════════════════════════════════════════════════

mod model_name_mapping {
    use super::*;

    #[test]
    fn openai_model_canonical_roundtrip() {
        let canonical = openai_dialect::to_canonical_model("gpt-4o");
        let vendor = openai_dialect::from_canonical_model(&canonical);
        assert_eq!(vendor, "gpt-4o");
    }

    #[test]
    fn claude_model_canonical_roundtrip() {
        let canonical = claude_dialect::to_canonical_model("claude-sonnet-4-20250514");
        let vendor = claude_dialect::from_canonical_model(&canonical);
        assert_eq!(vendor, "claude-sonnet-4-20250514");
    }

    #[test]
    fn gemini_model_canonical_roundtrip() {
        let canonical = gemini_dialect::to_canonical_model("gemini-2.5-flash");
        let vendor = gemini_dialect::from_canonical_model(&canonical);
        assert_eq!(vendor, "gemini-2.5-flash");
    }

    #[test]
    fn kimi_model_canonical_roundtrip() {
        let canonical = kimi_dialect::to_canonical_model("moonshot-v1-8k");
        let vendor = kimi_dialect::from_canonical_model(&canonical);
        assert_eq!(vendor, "moonshot-v1-8k");
    }

    #[test]
    fn copilot_model_canonical_roundtrip() {
        let canonical = copilot_dialect::to_canonical_model("gpt-4o");
        let vendor = copilot_dialect::from_canonical_model(&canonical);
        assert_eq!(vendor, "gpt-4o");
    }

    #[test]
    fn codex_model_canonical_roundtrip() {
        let canonical = codex_dialect::to_canonical_model("codex-mini-latest");
        let vendor = codex_dialect::from_canonical_model(&canonical);
        assert_eq!(vendor, "codex-mini-latest");
    }

    #[test]
    fn openai_is_known_model() {
        assert!(openai_dialect::is_known_model("gpt-4o"));
        assert!(!openai_dialect::is_known_model("nonexistent"));
    }

    #[test]
    fn claude_is_known_model() {
        assert!(claude_dialect::is_known_model("claude-sonnet-4-20250514"));
        assert!(!claude_dialect::is_known_model("nonexistent"));
    }

    #[test]
    fn gemini_is_known_model() {
        assert!(gemini_dialect::is_known_model("gemini-2.5-flash"));
        assert!(!gemini_dialect::is_known_model("nonexistent"));
    }

    #[test]
    fn kimi_is_known_model() {
        assert!(kimi_dialect::is_known_model("moonshot-v1-8k"));
        assert!(!kimi_dialect::is_known_model("nonexistent"));
    }

    #[test]
    fn copilot_is_known_model() {
        assert!(copilot_dialect::is_known_model("gpt-4o"));
        assert!(!copilot_dialect::is_known_model("nonexistent"));
    }

    #[test]
    fn codex_is_known_model() {
        assert!(codex_dialect::is_known_model("codex-mini-latest"));
        assert!(!codex_dialect::is_known_model("nonexistent"));
    }

    #[test]
    fn canonical_model_prefixes_are_unique() {
        let openai = openai_dialect::to_canonical_model("gpt-4o");
        let claude = claude_dialect::to_canonical_model("gpt-4o");
        let gemini = gemini_dialect::to_canonical_model("gpt-4o");
        let kimi = kimi_dialect::to_canonical_model("gpt-4o");
        let copilot = copilot_dialect::to_canonical_model("gpt-4o");
        let codex = codex_dialect::to_canonical_model("gpt-4o");

        // OpenAI and Codex share the `openai/` prefix
        assert_eq!(openai, codex);
        let set: std::collections::HashSet<_> = [openai, claude, gemini, kimi, copilot, codex]
            .into_iter()
            .collect();
        assert_eq!(set.len(), 5);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 11: Emulation across SDKs
// ═══════════════════════════════════════════════════════════════════════════

mod cross_sdk_emulation {
    use super::*;

    #[test]
    fn can_emulate_extended_thinking() {
        assert!(can_emulate(&Capability::ExtendedThinking));
    }

    #[test]
    fn cannot_emulate_code_execution() {
        assert!(!can_emulate(&Capability::CodeExecution));
    }

    #[test]
    fn cannot_emulate_streaming() {
        assert!(!can_emulate(&Capability::Streaming));
    }

    #[test]
    fn emulation_engine_injects_system_prompt() {
        let mut conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "base"))
            .push(IrMessage::text(IrRole::User, "hi"));
        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
        assert_eq!(report.applied.len(), 1);
        let sys = conv.system_message().unwrap();
        assert!(sys.text_content().contains("Think step by step"));
    }

    #[test]
    fn emulation_config_override() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::CodeExecution,
            EmulationStrategy::SystemPromptInjection {
                prompt: "Simulate code.".into(),
            },
        );
        let engine = EmulationEngine::new(config);
        let strategy = engine.resolve_strategy(&Capability::CodeExecution);
        assert!(matches!(
            strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn compute_fidelity_labels() {
        let native = vec![Capability::Streaming, Capability::ToolRead];
        let report = abp_emulation::EmulationReport {
            applied: vec![abp_emulation::EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "think".into(),
                },
            }],
            warnings: vec![],
        };
        let labels = compute_fidelity(&native, &report);
        assert_eq!(
            labels.get(&Capability::Streaming),
            Some(&FidelityLabel::Native)
        );
        assert!(matches!(
            labels.get(&Capability::ExtendedThinking),
            Some(FidelityLabel::Emulated { .. })
        ));
    }

    #[test]
    fn emulation_report_empty() {
        let report = abp_emulation::EmulationReport::default();
        assert!(report.is_empty());
        assert!(!report.has_unemulatable());
    }

    #[test]
    fn emulation_report_with_warnings() {
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[Capability::CodeExecution], &mut conv);
        assert!(report.has_unemulatable());
    }

    #[test]
    fn default_strategy_for_image_input() {
        let s = default_strategy(&Capability::ImageInput);
        assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    }

    #[test]
    fn default_strategy_for_stop_sequences() {
        let s = default_strategy(&Capability::StopSequences);
        assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 12: IR roundtrip across all SDKs
// ═══════════════════════════════════════════════════════════════════════════

mod ir_roundtrip {
    use super::*;

    #[test]
    fn openai_text_ir_roundtrip() {
        let msgs = vec![OpenAIMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let ir = abp_openai_sdk::lowering::to_ir(&msgs);
        let back = abp_openai_sdk::lowering::from_ir(&ir);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content.as_deref(), Some("Hello"));
    }

    #[test]
    fn claude_text_ir_roundtrip() {
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "Hello".into(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&msgs, None);
        let back = abp_claude_sdk::lowering::from_ir(&ir);
        assert_eq!(back[0].role, "user");
    }

    #[test]
    fn kimi_text_ir_roundtrip() {
        let msgs = vec![KimiMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_call_id: None,
            tool_calls: None,
        }];
        let ir = abp_kimi_sdk::lowering::to_ir(&msgs);
        let back = abp_kimi_sdk::lowering::from_ir(&ir);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content.as_deref(), Some("Hello"));
    }

    #[test]
    fn copilot_text_ir_roundtrip() {
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Hello".into(),
            name: None,
            copilot_references: vec![],
        }];
        let ir = abp_copilot_sdk::lowering::to_ir(&msgs);
        let back = abp_copilot_sdk::lowering::from_ir(&ir);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content, "Hello");
    }

    #[test]
    fn openai_tool_call_ir_roundtrip() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }];
        let ir = abp_openai_sdk::lowering::to_ir(&msgs);
        let back = abp_openai_sdk::lowering::from_ir(&ir);
        assert!(back[0].tool_calls.is_some());
        assert_eq!(back[0].tool_calls.as_ref().unwrap()[0].id, "c1");
    }

    #[test]
    fn kimi_tool_call_ir_roundtrip() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![abp_kimi_sdk::dialect::KimiToolCall {
                id: "c2".into(),
                call_type: "function".into(),
                function: abp_kimi_sdk::dialect::KimiFunctionCall {
                    name: "read".into(),
                    arguments: "{}".into(),
                },
            }]),
        }];
        let ir = abp_kimi_sdk::lowering::to_ir(&msgs);
        let back = abp_kimi_sdk::lowering::from_ir(&ir);
        assert!(back[0].tool_calls.is_some());
    }

    #[test]
    fn kimi_usage_to_ir() {
        let usage = KimiUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let ir = abp_kimi_sdk::lowering::usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 100);
        assert_eq!(ir.output_tokens, 50);
        assert_eq!(ir.total_tokens, 150);
    }

    #[test]
    fn openai_to_kimi_via_ir() {
        let openai_msgs = vec![OpenAIMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let ir = abp_openai_sdk::lowering::to_ir(&openai_msgs);
        let kimi_msgs = abp_kimi_sdk::lowering::from_ir(&ir);
        assert_eq!(kimi_msgs[0].role, "user");
        assert_eq!(kimi_msgs[0].content.as_deref(), Some("Hi"));
    }

    #[test]
    fn claude_to_copilot_via_ir() {
        let claude_msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "Hello".into(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        let copilot_msgs = abp_copilot_sdk::lowering::from_ir(&ir);
        assert_eq!(copilot_msgs[0].role, "user");
        assert_eq!(copilot_msgs[0].content, "Hello");
    }

    #[test]
    fn openai_to_copilot_via_ir() {
        let openai_msgs = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("Be helpful.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Hi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let ir = abp_openai_sdk::lowering::to_ir(&openai_msgs);
        let copilot_msgs = abp_copilot_sdk::lowering::from_ir(&ir);
        assert_eq!(copilot_msgs.len(), 2);
        assert_eq!(copilot_msgs[0].role, "system");
    }

    #[test]
    fn kimi_to_claude_via_ir() {
        let kimi_msgs = vec![KimiMessage {
            role: "user".into(),
            content: Some("Search for info".into()),
            tool_call_id: None,
            tool_calls: None,
        }];
        let ir = abp_kimi_sdk::lowering::to_ir(&kimi_msgs);
        let claude_msgs = abp_claude_sdk::lowering::from_ir(&ir);
        assert_eq!(claude_msgs[0].role, "user");
    }

    #[test]
    fn copilot_to_openai_via_ir() {
        let copilot_msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Help me".into(),
            name: None,
            copilot_references: vec![],
        }];
        let ir = abp_copilot_sdk::lowering::to_ir(&copilot_msgs);
        let openai_msgs = abp_openai_sdk::lowering::from_ir(&ir);
        assert_eq!(openai_msgs[0].role, "user");
        assert_eq!(openai_msgs[0].content.as_deref(), Some("Help me"));
    }

    #[test]
    fn multi_turn_openai_to_gemini_via_ir() {
        let openai_msgs = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("sys".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("u".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: Some("a".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let ir = abp_openai_sdk::lowering::to_ir(&openai_msgs);
        let gemini_contents = abp_gemini_sdk::lowering::from_ir(&ir);
        assert!(!gemini_contents.is_empty());
    }

    #[test]
    fn copilot_references_survive_ir_roundtrip() {
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Check file".into(),
            name: None,
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "f1".into(),
                data: json!({"path": "main.rs"}),
                metadata: None,
            }],
        }];
        let ir = abp_copilot_sdk::lowering::to_ir(&msgs);
        let back = abp_copilot_sdk::lowering::from_ir(&ir);
        assert_eq!(back[0].copilot_references.len(), 1);
        assert_eq!(back[0].copilot_references[0].id, "f1");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 13: SDK constant and config validation
// ═══════════════════════════════════════════════════════════════════════════

mod sdk_constants {
    use super::*;

    #[test]
    fn openai_backend_name() {
        assert_eq!(abp_openai_sdk::BACKEND_NAME, "sidecar:openai");
    }

    #[test]
    fn claude_backend_name() {
        assert_eq!(abp_claude_sdk::BACKEND_NAME, "sidecar:claude");
    }

    #[test]
    fn gemini_backend_name() {
        assert_eq!(abp_gemini_sdk::BACKEND_NAME, "sidecar:gemini");
    }

    #[test]
    fn kimi_backend_name() {
        assert_eq!(abp_kimi_sdk::BACKEND_NAME, "sidecar:kimi");
    }

    #[test]
    fn copilot_backend_name() {
        assert_eq!(abp_copilot_sdk::BACKEND_NAME, "sidecar:copilot");
    }

    #[test]
    fn codex_backend_name() {
        assert_eq!(abp_codex_sdk::BACKEND_NAME, "sidecar:codex");
    }

    #[test]
    fn openai_dialect_version() {
        assert_eq!(openai_dialect::DIALECT_VERSION, "openai/v0.1");
    }

    #[test]
    fn claude_dialect_version() {
        assert_eq!(claude_dialect::DIALECT_VERSION, "claude/v0.1");
    }

    #[test]
    fn gemini_dialect_version() {
        assert_eq!(gemini_dialect::DIALECT_VERSION, "gemini/v0.1");
    }

    #[test]
    fn kimi_dialect_version() {
        assert_eq!(kimi_dialect::DIALECT_VERSION, "kimi/v0.1");
    }

    #[test]
    fn copilot_dialect_version() {
        assert_eq!(copilot_dialect::DIALECT_VERSION, "copilot/v0.1");
    }

    #[test]
    fn codex_dialect_version() {
        assert_eq!(codex_dialect::DIALECT_VERSION, "codex/v0.1");
    }

    #[test]
    fn openai_default_model() {
        assert_eq!(openai_dialect::DEFAULT_MODEL, "gpt-4o");
    }

    #[test]
    fn claude_default_model() {
        assert_eq!(claude_dialect::DEFAULT_MODEL, "claude-sonnet-4-20250514");
    }

    #[test]
    fn gemini_default_model() {
        assert_eq!(gemini_dialect::DEFAULT_MODEL, "gemini-2.5-flash");
    }

    #[test]
    fn kimi_default_model() {
        assert_eq!(kimi_dialect::DEFAULT_MODEL, "moonshot-v1-8k");
    }

    #[test]
    fn copilot_default_model() {
        assert_eq!(copilot_dialect::DEFAULT_MODEL, "gpt-4o");
    }

    #[test]
    fn codex_default_model() {
        assert_eq!(codex_dialect::DEFAULT_MODEL, "codex-mini-latest");
    }

    #[test]
    fn openai_config_defaults() {
        let c = OpenAIConfig::default();
        assert!(c.base_url.contains("openai.com"));
        assert_eq!(c.model, "gpt-4o");
    }

    #[test]
    fn claude_config_defaults() {
        let c = ClaudeConfig::default();
        assert!(c.base_url.contains("anthropic"));
        assert_eq!(c.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn gemini_config_defaults() {
        let c = GeminiConfig::default();
        assert!(c.base_url.contains("googleapis"));
    }

    #[test]
    fn kimi_config_defaults() {
        let c = KimiConfig::default();
        assert!(c.base_url.contains("moonshot.cn"));
    }

    #[test]
    fn copilot_config_defaults() {
        let c = CopilotConfig::default();
        assert!(c.base_url.contains("githubcopilot"));
    }

    #[test]
    fn codex_config_defaults() {
        let c = CodexConfig::default();
        assert!(c.base_url.contains("openai.com"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 14: Response mapping across backends
// ═══════════════════════════════════════════════════════════════════════════

mod response_mapping {
    use super::*;

    #[test]
    fn kimi_response_with_tool_calls() {
        let resp = KimiResponse {
            id: "resp_1".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![abp_kimi_sdk::dialect::KimiToolCall {
                        id: "tc_1".into(),
                        call_type: "function".into(),
                        function: abp_kimi_sdk::dialect::KimiFunctionCall {
                            name: "web_search".into(),
                            arguments: r#"{"query":"test"}"#.into(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: Some(KimiUsage {
                prompt_tokens: 50,
                completion_tokens: 20,
                total_tokens: 70,
            }),
            refs: None,
        };
        let events = kimi_dialect::map_response(&resp);
        assert!(events.iter().any(|e| matches!(&e.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "web_search")));
    }

    #[test]
    fn copilot_response_with_errors() {
        let resp = CopilotResponse {
            message: String::new(),
            copilot_references: vec![],
            copilot_errors: vec![CopilotError {
                error_type: "auth_error".into(),
                message: "Token expired".into(),
                code: Some("401".into()),
                identifier: None,
            }],
            copilot_confirmation: None,
            function_call: None,
        };
        let events = copilot_dialect::map_response(&resp);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::Error { .. }))
        );
    }

    #[test]
    fn copilot_response_with_confirmation() {
        let resp = CopilotResponse {
            message: String::new(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: Some(CopilotConfirmation {
                id: "conf_1".into(),
                title: "Delete file?".into(),
                message: "Are you sure?".into(),
                accepted: None,
            }),
            function_call: None,
        };
        let events = copilot_dialect::map_response(&resp);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::Warning { .. }))
        );
    }

    #[test]
    fn copilot_response_with_function_call() {
        let resp = CopilotResponse {
            message: String::new(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: Some(CopilotFunctionCall {
                name: "deploy".into(),
                arguments: r#"{"env":"prod"}"#.into(),
                id: Some("fc_1".into()),
            }),
        };
        let events = copilot_dialect::map_response(&resp);
        assert!(events.iter().any(|e| matches!(&e.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "deploy")));
    }

    #[test]
    fn claude_stop_reason_parsing() {
        assert_eq!(
            claude_dialect::parse_stop_reason("end_turn"),
            Some(ClaudeStopReason::EndTurn)
        );
        assert_eq!(
            claude_dialect::parse_stop_reason("tool_use"),
            Some(ClaudeStopReason::ToolUse)
        );
        assert_eq!(
            claude_dialect::parse_stop_reason("max_tokens"),
            Some(ClaudeStopReason::MaxTokens)
        );
    }

    #[test]
    fn claude_stop_reason_mapping() {
        assert_eq!(
            claude_dialect::map_stop_reason(ClaudeStopReason::EndTurn),
            "end_turn"
        );
        assert_eq!(
            claude_dialect::map_stop_reason(ClaudeStopReason::ToolUse),
            "tool_use"
        );
    }

    #[test]
    fn gemini_function_call_maps_to_tool_call_event() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::FunctionCall {
                        name: "search".into(),
                        args: json!({"query": "rust"}),
                    }],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        let events = gemini_dialect::map_response(&resp);
        assert!(events.iter().any(|e| matches!(&e.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "search")));
    }

    #[test]
    fn kimi_response_with_citations() {
        let resp = KimiResponse {
            id: "resp_2".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("Rust is great.".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            refs: Some(vec![abp_kimi_sdk::dialect::KimiRef {
                index: 1,
                url: "https://rust-lang.org".into(),
                title: Some("Rust".into()),
            }]),
        };
        let events = kimi_dialect::map_response(&resp);
        assert!(!events.is_empty());
        let has_ext = events.iter().any(|e| e.ext.is_some());
        assert!(has_ext);
    }

    #[test]
    fn kimi_tool_call_accumulator() {
        let mut acc = kimi_dialect::ToolCallAccumulator::new();
        acc.feed(&[abp_kimi_sdk::dialect::KimiChunkToolCall {
            index: 0,
            id: Some("tc_1".into()),
            call_type: Some("function".into()),
            function: Some(abp_kimi_sdk::dialect::KimiChunkFunctionCall {
                name: Some("search".into()),
                arguments: Some(r#"{"q":"#.into()),
            }),
        }]);
        acc.feed(&[abp_kimi_sdk::dialect::KimiChunkToolCall {
            index: 0,
            id: None,
            call_type: None,
            function: Some(abp_kimi_sdk::dialect::KimiChunkFunctionCall {
                name: None,
                arguments: Some(r#""test"}"#.into()),
            }),
        }]);
        let events = acc.finish();
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0].kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "search")
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 15: Kimi-specific features
// ═══════════════════════════════════════════════════════════════════════════

mod kimi_specific {
    use super::*;

    #[test]
    fn kimi_builtin_search_internet() {
        let tool = kimi_dialect::builtin_search_internet();
        assert_eq!(tool.tool_type, "builtin_function");
        assert_eq!(tool.function.name, "$web_search");
    }

    #[test]
    fn kimi_builtin_browser() {
        let tool = kimi_dialect::builtin_browser();
        assert_eq!(tool.tool_type, "builtin_function");
        assert_eq!(tool.function.name, "$browser");
    }

    #[test]
    fn kimi_extract_usage() {
        let resp = KimiResponse {
            id: "r".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![],
            usage: Some(KimiUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
            }),
            refs: None,
        };
        let usage = kimi_dialect::extract_usage(&resp);
        assert!(usage.is_some());
        let u = usage.unwrap();
        assert_eq!(u["prompt_tokens"], json!(100));
        assert_eq!(u["completion_tokens"], json!(50));
    }

    #[test]
    fn kimi_extract_usage_none() {
        let resp = KimiResponse {
            id: "r".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![],
            usage: None,
            refs: None,
        };
        assert!(kimi_dialect::extract_usage(&resp).is_none());
    }

    #[test]
    fn kimi_config_k1_reasoning() {
        let mut config = KimiConfig::default();
        config.use_k1_reasoning = Some(true);
        let wo = WorkOrderBuilder::new("think hard").build();
        let req = kimi_dialect::map_work_order(&wo, &config);
        assert!(req.use_search.is_some());
    }
}
