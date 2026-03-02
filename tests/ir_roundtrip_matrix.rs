// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive IR roundtrip tests across all 6×6 dialect pairs.
//!
//! Tests verify that lowering to IR and lifting back preserves data for
//! same-dialect passthrough and handles lossy features for cross-dialect
//! mapping, using the `MappingRegistry` to validate fidelity.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_dialect::Dialect;
use abp_mapping::{MappingMatrix, features, known_rules, validate_mapping};

// SDK lowering modules
use abp_claude_sdk::dialect::ClaudeMessage;
use abp_claude_sdk::lowering as claude_lower;
use abp_codex_sdk::dialect::CodexInputItem;
use abp_codex_sdk::lowering as codex_lower;
use abp_copilot_sdk::dialect::CopilotMessage;
use abp_copilot_sdk::lowering as copilot_lower;
use abp_gemini_sdk::dialect::{GeminiContent, GeminiInlineData, GeminiPart};
use abp_gemini_sdk::lowering as gemini_lower;
use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall};
use abp_kimi_sdk::lowering as kimi_lower;
use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
use abp_openai_sdk::lowering as openai_lower;

// ── Helpers ─────────────────────────────────────────────────────────────

/// Build a simple text-only IR conversation with system, user, assistant.
fn simple_text_ir() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::text(IrRole::Assistant, "Hi there!"))
}

/// Build an IR conversation with tool use blocks.
fn tool_use_ir() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::User, "What is the weather?"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "get_weather".into(),
                input: serde_json::json!({"city": "London"}),
            }],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "Sunny, 22°C".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(
            IrRole::Assistant,
            "The weather in London is sunny, 22°C.",
        ))
}

/// Build an IR conversation with a thinking block.
fn thinking_ir() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Explain quantum mechanics"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think step by step...".into(),
                },
                IrContentBlock::Text {
                    text: "Quantum mechanics is a branch of physics.".into(),
                },
            ],
        ))
}

/// Build an IR conversation with an image block.
fn image_ir() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "Describe this image".into(),
                },
                IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "iVBORw0KGgo=".into(),
                },
            ],
        ))
        .push(IrMessage::text(IrRole::Assistant, "I see an image."))
}

/// Lower IR to a dialect's native format and re-raise it back to IR.
/// Returns the round-tripped IR conversation.
fn roundtrip_through_dialect(ir: &IrConversation, dialect: Dialect) -> IrConversation {
    match dialect {
        Dialect::OpenAi => {
            let native = openai_lower::from_ir(ir);
            openai_lower::to_ir(&native)
        }
        Dialect::Claude => {
            let sys = claude_lower::extract_system_prompt(ir);
            let native = claude_lower::from_ir(ir);
            claude_lower::to_ir(&native, sys.as_deref())
        }
        Dialect::Gemini => {
            let sys_instr = gemini_lower::extract_system_instruction(ir);
            let native = gemini_lower::from_ir(ir);
            gemini_lower::to_ir(&native, sys_instr.as_ref())
        }
        Dialect::Codex => {
            // Codex from_ir only emits assistant/tool items; re-raise them
            let items = codex_lower::from_ir(ir);
            codex_lower::to_ir(&items)
        }
        Dialect::Kimi => {
            let native = kimi_lower::from_ir(ir);
            kimi_lower::to_ir(&native)
        }
        Dialect::Copilot => {
            let native = copilot_lower::from_ir(ir);
            copilot_lower::to_ir(&native)
        }
    }
}

/// Lower IR through source dialect, then cross to target dialect via IR.
fn cross_dialect_roundtrip(
    ir: &IrConversation,
    source: Dialect,
    target: Dialect,
) -> IrConversation {
    // First: IR → source native → IR (source roundtrip)
    let after_source = roundtrip_through_dialect(ir, source);
    // Second: that IR → target native → IR (target roundtrip)
    roundtrip_through_dialect(&after_source, target)
}

// ── Module: Passthrough (same-dialect) tests ────────────────────────────

mod passthrough {
    use super::*;

    // Text passthrough for each dialect

    #[test]
    fn openai_text_passthrough() {
        let ir = simple_text_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::OpenAi);
        assert_eq!(rt.len(), ir.len());
        assert_eq!(rt.messages[0].text_content(), "You are helpful.");
        assert_eq!(rt.messages[1].text_content(), "Hello");
        assert_eq!(rt.messages[2].text_content(), "Hi there!");
    }

    #[test]
    fn claude_text_passthrough() {
        let ir = simple_text_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::Claude);
        // Claude separates system prompt; roundtrip should preserve it
        assert!(rt.system_message().is_some());
        assert_eq!(
            rt.system_message().unwrap().text_content(),
            "You are helpful."
        );
        // User and assistant preserved
        let non_sys: Vec<_> = rt
            .messages
            .iter()
            .filter(|m| m.role != IrRole::System)
            .collect();
        assert_eq!(non_sys.len(), 2);
        assert_eq!(non_sys[0].text_content(), "Hello");
        assert_eq!(non_sys[1].text_content(), "Hi there!");
    }

    #[test]
    fn gemini_text_passthrough() {
        let ir = simple_text_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::Gemini);
        assert!(rt.system_message().is_some());
        let non_sys: Vec<_> = rt
            .messages
            .iter()
            .filter(|m| m.role != IrRole::System)
            .collect();
        assert_eq!(non_sys.len(), 2);
        assert_eq!(non_sys[0].text_content(), "Hello");
    }

    #[test]
    fn codex_text_passthrough() {
        // Codex from_ir only emits assistant/tool messages; system/user are lost
        let ir = simple_text_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::Codex);
        // Only assistant message survives
        assert!(!rt.is_empty());
        assert!(rt.messages.iter().any(|m| m.role == IrRole::Assistant));
    }

    #[test]
    fn kimi_text_passthrough() {
        let ir = simple_text_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::Kimi);
        assert_eq!(rt.len(), ir.len());
        assert_eq!(rt.messages[0].text_content(), "You are helpful.");
        assert_eq!(rt.messages[1].text_content(), "Hello");
        assert_eq!(rt.messages[2].text_content(), "Hi there!");
    }

    #[test]
    fn copilot_text_passthrough() {
        let ir = simple_text_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::Copilot);
        assert_eq!(rt.len(), ir.len());
        assert_eq!(rt.messages[0].text_content(), "You are helpful.");
        assert_eq!(rt.messages[1].text_content(), "Hello");
        assert_eq!(rt.messages[2].text_content(), "Hi there!");
    }

    // Tool use passthrough

    #[test]
    fn openai_tool_use_passthrough() {
        let ir = tool_use_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::OpenAi);
        let calls = rt.tool_calls();
        assert_eq!(calls.len(), 1);
        if let IrContentBlock::ToolUse { name, input, .. } = calls[0] {
            assert_eq!(name, "get_weather");
            assert_eq!(input["city"], "London");
        } else {
            panic!("expected ToolUse block");
        }
    }

    #[test]
    fn claude_tool_use_passthrough() {
        let ir = tool_use_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::Claude);
        let calls = rt.tool_calls();
        assert!(!calls.is_empty());
    }

    #[test]
    fn gemini_tool_use_passthrough() {
        let ir = tool_use_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::Gemini);
        let calls = rt.tool_calls();
        assert!(!calls.is_empty());
        if let IrContentBlock::ToolUse { name, .. } = calls[0] {
            assert_eq!(name, "get_weather");
        }
    }

    #[test]
    fn kimi_tool_use_passthrough() {
        let ir = tool_use_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::Kimi);
        let calls = rt.tool_calls();
        assert_eq!(calls.len(), 1);
        if let IrContentBlock::ToolUse { name, input, .. } = calls[0] {
            assert_eq!(name, "get_weather");
            assert_eq!(input["city"], "London");
        } else {
            panic!("expected ToolUse block");
        }
    }

    #[test]
    fn copilot_text_content_preserved() {
        // Copilot is text-only; tool use blocks may not survive
        let ir = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "ping"))
            .push(IrMessage::text(IrRole::Assistant, "pong"));
        let rt = roundtrip_through_dialect(&ir, Dialect::Copilot);
        assert_eq!(rt.messages[0].text_content(), "ping");
        assert_eq!(rt.messages[1].text_content(), "pong");
    }

    #[test]
    fn codex_tool_use_passthrough() {
        let ir = tool_use_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::Codex);
        // Codex should preserve tool calls through response items
        let calls = rt.tool_calls();
        assert!(!calls.is_empty());
    }
}

// ── Module: Cross-dialect mapping matrix ────────────────────────────────

mod cross_dialect {
    use super::*;

    // Text messages survive all cross-dialect roundtrips (at minimum
    // the user+assistant text). System handling varies.

    #[test]
    fn openai_to_claude_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::OpenAi, Dialect::Claude);
        // User and assistant text must survive
        assert!(rt.messages.iter().any(|m| m.text_content() == "Hello"));
        assert!(rt.messages.iter().any(|m| m.text_content() == "Hi there!"));
    }

    #[test]
    fn openai_to_gemini_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::OpenAi, Dialect::Gemini);
        assert!(rt.messages.iter().any(|m| m.text_content() == "Hello"));
    }

    #[test]
    fn openai_to_codex_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::OpenAi, Dialect::Codex);
        // Codex only keeps assistant; should have at least one message
        assert!(!rt.is_empty());
    }

    #[test]
    fn openai_to_kimi_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::OpenAi, Dialect::Kimi);
        assert_eq!(rt.len(), 3);
        assert_eq!(rt.messages[1].text_content(), "Hello");
    }

    #[test]
    fn openai_to_copilot_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::OpenAi, Dialect::Copilot);
        assert!(rt.messages.iter().any(|m| m.text_content() == "Hello"));
    }

    #[test]
    fn claude_to_openai_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Claude, Dialect::OpenAi);
        assert!(rt.messages.iter().any(|m| m.text_content() == "Hi there!"));
    }

    #[test]
    fn claude_to_gemini_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Claude, Dialect::Gemini);
        assert!(!rt.is_empty());
    }

    #[test]
    fn claude_to_codex_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Claude, Dialect::Codex);
        assert!(!rt.is_empty());
    }

    #[test]
    fn claude_to_kimi_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Claude, Dialect::Kimi);
        assert!(rt.messages.iter().any(|m| m.text_content() == "Hello"));
    }

    #[test]
    fn claude_to_copilot_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Claude, Dialect::Copilot);
        assert!(rt.messages.iter().any(|m| m.text_content() == "Hello"));
    }

    #[test]
    fn gemini_to_openai_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Gemini, Dialect::OpenAi);
        assert!(rt.messages.iter().any(|m| m.text_content() == "Hello"));
    }

    #[test]
    fn gemini_to_claude_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Gemini, Dialect::Claude);
        assert!(!rt.is_empty());
    }

    #[test]
    fn gemini_to_codex_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Gemini, Dialect::Codex);
        assert!(!rt.is_empty());
    }

    #[test]
    fn gemini_to_kimi_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Gemini, Dialect::Kimi);
        assert!(!rt.is_empty());
    }

    #[test]
    fn gemini_to_copilot_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Gemini, Dialect::Copilot);
        assert!(!rt.is_empty());
    }

    #[test]
    fn codex_to_openai_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Codex, Dialect::OpenAi);
        // Codex drops system/user so only assistant survives into OpenAI
        assert!(!rt.is_empty());
    }

    #[test]
    fn codex_to_claude_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Codex, Dialect::Claude);
        assert!(!rt.is_empty());
    }

    #[test]
    fn codex_to_gemini_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Codex, Dialect::Gemini);
        assert!(!rt.is_empty());
    }

    #[test]
    fn codex_to_kimi_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Codex, Dialect::Kimi);
        assert!(!rt.is_empty());
    }

    #[test]
    fn codex_to_copilot_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Codex, Dialect::Copilot);
        assert!(!rt.is_empty());
    }

    #[test]
    fn kimi_to_openai_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Kimi, Dialect::OpenAi);
        assert_eq!(rt.len(), 3);
    }

    #[test]
    fn kimi_to_claude_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Kimi, Dialect::Claude);
        assert!(!rt.is_empty());
    }

    #[test]
    fn kimi_to_gemini_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Kimi, Dialect::Gemini);
        assert!(!rt.is_empty());
    }

    #[test]
    fn kimi_to_codex_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Kimi, Dialect::Codex);
        assert!(!rt.is_empty());
    }

    #[test]
    fn kimi_to_copilot_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Kimi, Dialect::Copilot);
        assert_eq!(rt.len(), 3);
    }

    #[test]
    fn copilot_to_openai_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Copilot, Dialect::OpenAi);
        assert_eq!(rt.len(), 3);
    }

    #[test]
    fn copilot_to_claude_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Copilot, Dialect::Claude);
        assert!(!rt.is_empty());
    }

    #[test]
    fn copilot_to_gemini_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Copilot, Dialect::Gemini);
        assert!(!rt.is_empty());
    }

    #[test]
    fn copilot_to_codex_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Copilot, Dialect::Codex);
        assert!(!rt.is_empty());
    }

    #[test]
    fn copilot_to_kimi_text() {
        let ir = simple_text_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Copilot, Dialect::Kimi);
        assert_eq!(rt.len(), 3);
    }
}

// ── Module: Tool use cross-dialect ──────────────────────────────────────

mod tool_use_cross {
    use super::*;

    #[test]
    fn openai_to_claude_tool_use() {
        let ir = tool_use_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::OpenAi, Dialect::Claude);
        let calls = rt.tool_calls();
        assert!(!calls.is_empty(), "tool calls should survive OpenAI→Claude");
    }

    #[test]
    fn openai_to_gemini_tool_use() {
        let ir = tool_use_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::OpenAi, Dialect::Gemini);
        let calls = rt.tool_calls();
        assert!(!calls.is_empty(), "tool calls should survive OpenAI→Gemini");
    }

    #[test]
    fn openai_to_kimi_tool_use() {
        let ir = tool_use_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::OpenAi, Dialect::Kimi);
        let calls = rt.tool_calls();
        assert!(!calls.is_empty(), "tool calls should survive OpenAI→Kimi");
    }

    #[test]
    fn claude_to_openai_tool_use() {
        let ir = tool_use_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Claude, Dialect::OpenAi);
        let calls = rt.tool_calls();
        assert!(!calls.is_empty(), "tool calls should survive Claude→OpenAI");
    }

    #[test]
    fn claude_to_gemini_tool_use() {
        let ir = tool_use_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Claude, Dialect::Gemini);
        let calls = rt.tool_calls();
        assert!(!calls.is_empty(), "tool calls should survive Claude→Gemini");
    }

    #[test]
    fn gemini_to_openai_tool_use() {
        let ir = tool_use_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Gemini, Dialect::OpenAi);
        let calls = rt.tool_calls();
        assert!(!calls.is_empty(), "tool calls should survive Gemini→OpenAI");
    }

    #[test]
    fn gemini_to_claude_tool_use() {
        let ir = tool_use_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Gemini, Dialect::Claude);
        let calls = rt.tool_calls();
        assert!(!calls.is_empty(), "tool calls should survive Gemini→Claude");
    }

    #[test]
    fn kimi_to_openai_tool_use() {
        let ir = tool_use_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Kimi, Dialect::OpenAi);
        let calls = rt.tool_calls();
        assert!(!calls.is_empty(), "tool calls should survive Kimi→OpenAI");
    }

    #[test]
    fn codex_to_openai_tool_use() {
        let ir = tool_use_ir();
        let rt = cross_dialect_roundtrip(&ir, Dialect::Codex, Dialect::OpenAi);
        let calls = rt.tool_calls();
        assert!(!calls.is_empty(), "tool calls should survive Codex→OpenAI");
    }

    #[test]
    fn copilot_to_openai_tool_use_text() {
        // Copilot is text-only; tool calls lowered through Copilot lose
        // structure, so we verify the text-only path doesn't panic.
        let ir = tool_use_ir();
        let _rt = cross_dialect_roundtrip(&ir, Dialect::Copilot, Dialect::OpenAi);
        // No panic = pass; copilot may lose tool structure
    }
}

// ── Module: Thinking block cross-dialect ────────────────────────────────

mod thinking_cross {
    use super::*;

    #[test]
    fn thinking_survives_openai_roundtrip() {
        let ir = thinking_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::OpenAi);
        // OpenAI doesn't have native thinking; may lose it
        // The text portion should survive
        assert!(rt.messages.iter().any(|m| m.role == IrRole::Assistant));
    }

    #[test]
    fn thinking_survives_claude_roundtrip() {
        let ir = thinking_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::Claude);
        assert!(rt.messages.iter().any(|m| m.role == IrRole::Assistant));
    }

    #[test]
    fn claude_to_openai_thinking_lossy() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Claude,
            Dialect::OpenAi,
            &[features::THINKING.into()],
        );
        assert_eq!(results.len(), 1);
        assert!(
            !results[0].fidelity.is_lossless(),
            "thinking should be lossy Claude→OpenAI"
        );
    }

    #[test]
    fn claude_to_gemini_thinking_lossy() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Claude,
            Dialect::Gemini,
            &[features::THINKING.into()],
        );
        assert!(!results[0].fidelity.is_lossless());
    }

    #[test]
    fn thinking_text_preserved_openai() {
        let ir = thinking_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::OpenAi);
        let asst = rt.last_assistant().expect("assistant message");
        let text = asst.text_content();
        assert!(
            text.contains("Quantum mechanics"),
            "text portion should survive"
        );
    }

    #[test]
    fn thinking_text_preserved_kimi() {
        let ir = thinking_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::Kimi);
        let asst = rt.last_assistant().expect("assistant message");
        let text = asst.text_content();
        assert!(text.contains("Quantum mechanics"));
    }

    #[test]
    fn kimi_copilot_thinking_lossy() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Kimi,
            Dialect::Copilot,
            &[features::THINKING.into()],
        );
        assert!(!results[0].fidelity.is_lossless());
    }
}

// ── Module: Image cross-dialect ─────────────────────────────────────────

mod image_cross {
    use super::*;

    #[test]
    fn image_survives_gemini_roundtrip() {
        let ir = image_ir();
        let rt = roundtrip_through_dialect(&ir, Dialect::Gemini);
        assert!(
            rt.messages.iter().any(|m| m
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Image { .. }))),
            "image should survive Gemini roundtrip"
        );
    }

    #[test]
    fn image_codex_unsupported() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &[features::IMAGE_INPUT.into()],
        );
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn image_kimi_unsupported() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Kimi,
            &[features::IMAGE_INPUT.into()],
        );
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn image_copilot_unsupported() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Copilot,
            &[features::IMAGE_INPUT.into()],
        );
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn openai_claude_image_lossless() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::IMAGE_INPUT.into()],
        );
        assert!(results[0].fidelity.is_lossless());
    }

    #[test]
    fn openai_gemini_image_lossless() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Gemini,
            &[features::IMAGE_INPUT.into()],
        );
        assert!(results[0].fidelity.is_lossless());
    }

    #[test]
    fn claude_gemini_image_lossless() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Claude,
            Dialect::Gemini,
            &[features::IMAGE_INPUT.into()],
        );
        assert!(results[0].fidelity.is_lossless());
    }
}

// ── Module: Mapping registry validation ─────────────────────────────────

mod mapping_registry {
    use super::*;

    #[test]
    fn known_rules_non_empty() {
        let reg = known_rules();
        assert!(!reg.is_empty());
    }

    #[test]
    fn same_dialect_always_lossless() {
        let reg = known_rules();
        let feats = [
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ];
        for &d in Dialect::all() {
            for &f in &feats {
                let rule = reg.lookup(d, d, f);
                assert!(rule.is_some(), "missing same-dialect rule for {d} / {f}");
                assert!(
                    rule.unwrap().fidelity.is_lossless(),
                    "same-dialect rule for {d} / {f} should be lossless"
                );
            }
        }
    }

    #[test]
    fn streaming_all_cross_dialect_lossless() {
        let reg = known_rules();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                if src == tgt {
                    continue;
                }
                if let Some(rule) = reg.lookup(src, tgt, features::STREAMING) {
                    assert!(
                        rule.fidelity.is_lossless(),
                        "streaming {src}→{tgt} should be lossless"
                    );
                }
            }
        }
    }

    #[test]
    fn matrix_from_registry_same_dialect_not_marked() {
        let reg = known_rules();
        let matrix = MappingMatrix::from_registry(&reg);
        // Same-dialect entries are set because there are lossless rules
        for &d in Dialect::all() {
            assert!(
                matrix.is_supported(d, d),
                "same-dialect {d} should be supported"
            );
        }
    }

    #[test]
    fn tool_use_openai_claude_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
            .expect("rule should exist");
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn tool_use_openai_codex_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE)
            .expect("rule should exist");
        assert!(
            !rule.fidelity.is_lossless(),
            "OpenAI→Codex tool_use should be lossy"
        );
    }

    #[test]
    fn code_exec_kimi_unsupported() {
        let reg = known_rules();
        for &tgt in Dialect::all() {
            if tgt == Dialect::Kimi {
                continue;
            }
            let rule = reg.lookup(Dialect::Kimi, tgt, features::CODE_EXEC);
            if let Some(r) = rule {
                assert!(
                    r.fidelity.is_unsupported(),
                    "Kimi→{tgt} code_exec should be unsupported"
                );
            }
        }
    }

    #[test]
    fn validate_multiple_features() {
        let reg = known_rules();
        let features_list: Vec<String> = vec![
            features::TOOL_USE.into(),
            features::STREAMING.into(),
            features::THINKING.into(),
        ];
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &features_list);
        assert_eq!(results.len(), 3);
        // tool_use: lossless, streaming: lossless, thinking: lossy
        assert!(results[0].fidelity.is_lossless()); // tool_use
        assert!(results[1].fidelity.is_lossless()); // streaming
        assert!(!results[2].fidelity.is_lossless()); // thinking
    }

    #[test]
    fn validate_empty_feature_name() {
        let reg = known_rules();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
        assert!(!results[0].errors.is_empty());
    }

    #[test]
    fn rank_targets_returns_results() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE, features::STREAMING]);
        assert!(!ranked.is_empty());
        // First result should have the highest lossless count
        let (_, first_count) = ranked[0];
        for &(_, count) in &ranked[1..] {
            assert!(count <= first_count);
        }
    }
}

// ── Module: Native format construction tests ────────────────────────────

mod native_construction {
    use super::*;

    #[test]
    fn openai_native_to_ir_and_back() {
        let messages = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("Be helpful".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Hi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let ir = openai_lower::to_ir(&messages);
        assert_eq!(ir.len(), 3);
        let roundtripped = openai_lower::from_ir(&ir);
        assert_eq!(roundtripped.len(), 3);
        assert_eq!(roundtripped[0].role, "system");
        assert_eq!(roundtripped[1].content.as_deref(), Some("Hi"));
    }

    #[test]
    fn openai_tool_call_native_roundtrip() {
        let messages = vec![
            OpenAIMessage {
                role: "user".into(),
                content: Some("Get weather".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "tc_1".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "weather".into(),
                        arguments: r#"{"city":"NYC"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "tool".into(),
                content: Some("Rainy".into()),
                tool_calls: None,
                tool_call_id: Some("tc_1".into()),
            },
        ];
        let ir = openai_lower::to_ir(&messages);
        let calls = ir.tool_calls();
        assert_eq!(calls.len(), 1);
        let rt = openai_lower::from_ir(&ir);
        assert_eq!(rt.len(), 3);
    }

    #[test]
    fn claude_native_to_ir_and_back() {
        let messages = vec![
            ClaudeMessage {
                role: "user".into(),
                content: "Hello Claude".into(),
            },
            ClaudeMessage {
                role: "assistant".into(),
                content: "Hello!".into(),
            },
        ];
        let ir = claude_lower::to_ir(&messages, Some("Be concise"));
        assert_eq!(ir.len(), 3); // system + user + assistant
        let sys = claude_lower::extract_system_prompt(&ir);
        assert_eq!(sys.as_deref(), Some("Be concise"));
        let rt = claude_lower::from_ir(&ir);
        // System is stripped; user + assistant
        assert_eq!(rt.len(), 2);
    }

    #[test]
    fn gemini_native_to_ir_and_back() {
        let contents = vec![
            GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hello Gemini".into())],
            },
            GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Hi!".into())],
            },
        ];
        let sys = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("System instructions".into())],
        };
        let ir = gemini_lower::to_ir(&contents, Some(&sys));
        assert_eq!(ir.len(), 3);
        let rt = gemini_lower::from_ir(&ir);
        // System is stripped
        assert_eq!(rt.len(), 2);
        assert_eq!(rt[0].role, "user");
    }

    #[test]
    fn gemini_image_native_roundtrip() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![
                GeminiPart::Text("Describe this".into()),
                GeminiPart::InlineData(GeminiInlineData {
                    mime_type: "image/jpeg".into(),
                    data: "base64data".into(),
                }),
            ],
        }];
        let ir = gemini_lower::to_ir(&contents, None);
        assert!(ir.messages[0].content.len() >= 2);
        let rt = gemini_lower::from_ir(&ir);
        assert_eq!(rt[0].parts.len(), 2);
    }

    #[test]
    fn codex_input_to_ir() {
        let items = vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Write hello world".into(),
        }];
        let ir = codex_lower::input_to_ir(&items);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].text_content(), "Write hello world");
    }

    #[test]
    fn kimi_native_to_ir_and_back() {
        let messages = vec![
            KimiMessage {
                role: "system".into(),
                content: Some("Be helpful".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            KimiMessage {
                role: "user".into(),
                content: Some("Hello Kimi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            KimiMessage {
                role: "assistant".into(),
                content: Some("Hi!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let ir = kimi_lower::to_ir(&messages);
        assert_eq!(ir.len(), 3);
        let rt = kimi_lower::from_ir(&ir);
        assert_eq!(rt.len(), 3);
        assert_eq!(rt[0].role, "system");
    }

    #[test]
    fn kimi_tool_call_native_roundtrip() {
        let messages = vec![
            KimiMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![KimiToolCall {
                    id: "kimi_tc1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "search".into(),
                        arguments: r#"{"q":"rust"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            },
            KimiMessage {
                role: "tool".into(),
                content: Some("Results found".into()),
                tool_calls: None,
                tool_call_id: Some("kimi_tc1".into()),
            },
        ];
        let ir = kimi_lower::to_ir(&messages);
        let calls = ir.tool_calls();
        assert_eq!(calls.len(), 1);
        let rt = kimi_lower::from_ir(&ir);
        assert_eq!(rt.len(), 2);
    }

    #[test]
    fn copilot_native_to_ir_and_back() {
        let messages = vec![
            CopilotMessage {
                role: "system".into(),
                content: "You are copilot".into(),
                name: None,
                copilot_references: vec![],
            },
            CopilotMessage {
                role: "user".into(),
                content: "Help me code".into(),
                name: None,
                copilot_references: vec![],
            },
            CopilotMessage {
                role: "assistant".into(),
                content: "Sure!".into(),
                name: None,
                copilot_references: vec![],
            },
        ];
        let ir = copilot_lower::to_ir(&messages);
        assert_eq!(ir.len(), 3);
        let rt = copilot_lower::from_ir(&ir);
        assert_eq!(rt.len(), 3);
        assert_eq!(rt[0].role, "system");
        assert_eq!(rt[1].content, "Help me code");
    }
}

// ── Module: IR conversation helpers ─────────────────────────────────────

mod ir_helpers {
    use super::*;

    #[test]
    fn empty_conversation() {
        let conv = IrConversation::new();
        assert!(conv.is_empty());
        assert_eq!(conv.len(), 0);
        assert!(conv.system_message().is_none());
        assert!(conv.last_assistant().is_none());
        assert!(conv.tool_calls().is_empty());
    }

    #[test]
    fn text_message_is_text_only() {
        let msg = IrMessage::text(IrRole::User, "Hello");
        assert!(msg.is_text_only());
        assert_eq!(msg.text_content(), "Hello");
    }

    #[test]
    fn tool_use_message_is_not_text_only() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "1".into(),
                name: "test".into(),
                input: serde_json::json!({}),
            }],
        );
        assert!(!msg.is_text_only());
    }

    #[test]
    fn messages_by_role() {
        let conv = simple_text_ir();
        let users = conv.messages_by_role(IrRole::User);
        assert_eq!(users.len(), 1);
        let systems = conv.messages_by_role(IrRole::System);
        assert_eq!(systems.len(), 1);
    }

    #[test]
    fn last_message() {
        let conv = simple_text_ir();
        let last = conv.last_message().expect("should have last message");
        assert_eq!(last.role, IrRole::Assistant);
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
        let a = IrUsage::from_io(100, 50);
        let b = IrUsage::from_io(200, 100);
        let merged = a.merge(b);
        assert_eq!(merged.input_tokens, 300);
        assert_eq!(merged.output_tokens, 150);
        assert_eq!(merged.total_tokens, 450);
    }

    #[test]
    fn ir_usage_with_cache() {
        let usage = IrUsage::with_cache(100, 50, 20, 10);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_write_tokens, 10);
        assert_eq!(usage.total_tokens, 150);
    }
}

// ── Module: Serde roundtrip ─────────────────────────────────────────────

mod serde_roundtrip {
    use super::*;

    #[test]
    fn ir_conversation_json_roundtrip() {
        let conv = simple_text_ir();
        let json = serde_json::to_string(&conv).expect("serialize");
        let deser: IrConversation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(conv, deser);
    }

    #[test]
    fn ir_tool_use_json_roundtrip() {
        let conv = tool_use_ir();
        let json = serde_json::to_string(&conv).expect("serialize");
        let deser: IrConversation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(conv, deser);
    }

    #[test]
    fn ir_thinking_json_roundtrip() {
        let conv = thinking_ir();
        let json = serde_json::to_string(&conv).expect("serialize");
        let deser: IrConversation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(conv, deser);
    }

    #[test]
    fn ir_image_json_roundtrip() {
        let conv = image_ir();
        let json = serde_json::to_string(&conv).expect("serialize");
        let deser: IrConversation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(conv, deser);
    }

    #[test]
    fn ir_message_metadata_preserved() {
        let mut msg = IrMessage::text(IrRole::User, "test");
        msg.metadata
            .insert("key".into(), serde_json::json!("value"));
        let json = serde_json::to_string(&msg).expect("serialize");
        let deser: IrMessage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deser.metadata["key"], "value");
    }
}

// ── Module: Edge cases ──────────────────────────────────────────────────

mod edge_cases {
    use super::*;

    #[test]
    fn empty_ir_roundtrip_all_dialects() {
        let ir = IrConversation::new();
        for &d in Dialect::all() {
            let rt = roundtrip_through_dialect(&ir, d);
            assert!(rt.is_empty(), "empty IR through {d} should remain empty");
        }
    }

    #[test]
    fn single_user_message_all_dialects() {
        let ir = IrConversation::new().push(IrMessage::text(IrRole::User, "ping"));
        for &d in Dialect::all() {
            let rt = roundtrip_through_dialect(&ir, d);
            match d {
                // Codex from_ir drops user messages
                Dialect::Codex => assert!(rt.is_empty()),
                _ => {
                    assert!(
                        !rt.is_empty(),
                        "single user msg through {d} should not be empty"
                    );
                }
            }
        }
    }

    #[test]
    fn empty_content_message() {
        let ir = IrConversation::new().push(IrMessage::new(IrRole::User, vec![]));
        for &d in Dialect::all() {
            // Should not panic
            let _rt = roundtrip_through_dialect(&ir, d);
        }
    }

    #[test]
    fn unicode_content_preserved() {
        let ir = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "こんにちは 🌍 مرحبا"))
            .push(IrMessage::text(IrRole::Assistant, "Héllo wörld 你好"));
        for &d in &[Dialect::OpenAi, Dialect::Kimi, Dialect::Copilot] {
            let rt = roundtrip_through_dialect(&ir, d);
            assert_eq!(rt.messages[0].text_content(), "こんにちは 🌍 مرحبا");
            assert_eq!(rt.messages[1].text_content(), "Héllo wörld 你好");
        }
    }

    #[test]
    fn large_conversation() {
        let mut msgs = Vec::new();
        for i in 0..50 {
            msgs.push(IrMessage::text(IrRole::User, format!("Message {i}")));
            msgs.push(IrMessage::text(IrRole::Assistant, format!("Response {i}")));
        }
        let ir = IrConversation::from_messages(msgs);
        assert_eq!(ir.len(), 100);
        let rt = roundtrip_through_dialect(&ir, Dialect::OpenAi);
        assert_eq!(rt.len(), 100);
    }

    #[test]
    fn mixed_content_blocks() {
        let ir = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Here's the result:".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tc1".into(),
                    name: "compute".into(),
                    input: serde_json::json!({"x": 42}),
                },
            ],
        ));
        let rt = roundtrip_through_dialect(&ir, Dialect::OpenAi);
        let calls = rt.tool_calls();
        assert!(!calls.is_empty());
    }

    #[test]
    fn tool_result_error_flag() {
        let ir = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tc1".into(),
                content: vec![IrContentBlock::Text {
                    text: "Error occurred".into(),
                }],
                is_error: true,
            }],
        ));
        let json = serde_json::to_string(&ir).unwrap();
        assert!(json.contains("true"));
    }
}

// ── Module: Full matrix validation ──────────────────────────────────────

mod full_matrix {
    use super::*;

    #[test]
    fn all_dialect_pairs_no_panic() {
        let ir = simple_text_ir();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                let _rt = cross_dialect_roundtrip(&ir, src, tgt);
            }
        }
    }

    #[test]
    fn all_dialect_pairs_tool_use_no_panic() {
        let ir = tool_use_ir();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                let _rt = cross_dialect_roundtrip(&ir, src, tgt);
            }
        }
    }

    #[test]
    fn all_dialect_pairs_thinking_no_panic() {
        let ir = thinking_ir();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                let _rt = cross_dialect_roundtrip(&ir, src, tgt);
            }
        }
    }

    #[test]
    fn all_dialect_pairs_image_no_panic() {
        let ir = image_ir();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                let _rt = cross_dialect_roundtrip(&ir, src, tgt);
            }
        }
    }

    #[test]
    fn matrix_symmetry_for_known_features() {
        let reg = known_rules();
        // For each feature, if A→B has a rule, check that rule exists
        for rule in reg.iter() {
            let reverse = reg.lookup(rule.target_dialect, rule.source_dialect, &rule.feature);
            // Same-dialect reverse is always itself, skip
            if rule.source_dialect == rule.target_dialect {
                continue;
            }
            assert!(
                reverse.is_some(),
                "missing reverse rule for {} {} → {}",
                rule.feature,
                rule.target_dialect,
                rule.source_dialect,
            );
        }
    }

    #[test]
    fn mapping_matrix_all_same_dialect_supported() {
        let reg = known_rules();
        let matrix = MappingMatrix::from_registry(&reg);
        for &d in Dialect::all() {
            assert!(
                matrix.is_supported(d, d),
                "{d} → {d} should be supported in matrix"
            );
        }
    }
}
