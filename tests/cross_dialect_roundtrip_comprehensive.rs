// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive cross-dialect roundtrip tests.
//!
//! 80+ tests covering same-dialect passthrough, 6×6 cross-dialect text mapping,
//! tool-use preservation, thinking block handling, image support validation,
//! multi-turn conversation mapping, fidelity loss detection, error handling for
//! unsupported mappings, and capability-based routing.

use abp_capability::{
    NegotiationResult, SupportLevel as CapSupportLevel, check_capability, generate_report,
    negotiate,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel,
};
use abp_dialect::Dialect;
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, MappingRegistry, MappingRule, MappingValidation,
    features, known_rules, validate_mapping,
};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn all_dialects() -> &'static [Dialect] {
    Dialect::all()
}

fn text_ir_message(role: IrRole, text: &str) -> IrMessage {
    IrMessage::text(role, text)
}

fn tool_use_ir_block(id: &str, name: &str) -> IrContentBlock {
    IrContentBlock::ToolUse {
        id: id.into(),
        name: name.into(),
        input: serde_json::json!({"path": "src/main.rs"}),
    }
}

fn tool_result_ir_block(tool_use_id: &str) -> IrContentBlock {
    IrContentBlock::ToolResult {
        tool_use_id: tool_use_id.into(),
        content: vec![IrContentBlock::Text { text: "OK".into() }],
        is_error: false,
    }
}

fn thinking_ir_block(text: &str) -> IrContentBlock {
    IrContentBlock::Thinking { text: text.into() }
}

fn image_ir_block() -> IrContentBlock {
    IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "iVBORw0KGgo=".into(),
    }
}

fn sample_tool_def() -> IrToolDefinition {
    IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file from the workspace".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"]
        }),
    }
}

fn registry() -> MappingRegistry {
    known_rules()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Same-dialect passthrough roundtrip (all 6 SDKs)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_openai_all_features_lossless() {
    let reg = registry();
    for &f in &[
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ] {
        let rule = reg.lookup(Dialect::OpenAi, Dialect::OpenAi, f);
        assert!(rule.is_some(), "missing self-rule for OpenAI:{f}");
        assert!(rule.unwrap().fidelity.is_lossless());
    }
}

#[test]
fn passthrough_claude_all_features_lossless() {
    let reg = registry();
    for &f in &[
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ] {
        let rule = reg.lookup(Dialect::Claude, Dialect::Claude, f);
        assert!(rule.is_some(), "missing self-rule for Claude:{f}");
        assert!(rule.unwrap().fidelity.is_lossless());
    }
}

#[test]
fn passthrough_gemini_all_features_lossless() {
    let reg = registry();
    for &f in &[
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ] {
        let rule = reg.lookup(Dialect::Gemini, Dialect::Gemini, f);
        assert!(rule.is_some(), "missing self-rule for Gemini:{f}");
        assert!(rule.unwrap().fidelity.is_lossless());
    }
}

#[test]
fn passthrough_codex_all_features_lossless() {
    let reg = registry();
    for &f in &[
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ] {
        let rule = reg.lookup(Dialect::Codex, Dialect::Codex, f);
        assert!(rule.is_some(), "missing self-rule for Codex:{f}");
        assert!(rule.unwrap().fidelity.is_lossless());
    }
}

#[test]
fn passthrough_kimi_all_features_lossless() {
    let reg = registry();
    for &f in &[
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ] {
        let rule = reg.lookup(Dialect::Kimi, Dialect::Kimi, f);
        assert!(rule.is_some(), "missing self-rule for Kimi:{f}");
        assert!(rule.unwrap().fidelity.is_lossless());
    }
}

#[test]
fn passthrough_copilot_all_features_lossless() {
    let reg = registry();
    for &f in &[
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ] {
        let rule = reg.lookup(Dialect::Copilot, Dialect::Copilot, f);
        assert!(rule.is_some(), "missing self-rule for Copilot:{f}");
        assert!(rule.unwrap().fidelity.is_lossless());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Cross-dialect text mapping (6×6 matrix)
// ═══════════════════════════════════════════════════════════════════════════

/// IR text roundtrip: lowering into IR and reading back preserves content.
#[test]
fn ir_text_roundtrip_all_roles() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let msg = text_ir_message(role, "hello world");
        let json = serde_json::to_string(&msg).unwrap();
        let recovered: IrMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, recovered);
    }
}

#[test]
fn ir_text_content_preserved_through_serde() {
    let msg = IrMessage::text(IrRole::User, "special chars: <>&\"'\n\ttab");
    let json = serde_json::to_value(&msg).unwrap();
    let back: IrMessage = serde_json::from_value(json).unwrap();
    assert_eq!(msg.text_content(), back.text_content());
}

#[test]
fn streaming_lossless_openai_claude() {
    let reg = registry();
    let r = reg.lookup(Dialect::OpenAi, Dialect::Claude, features::STREAMING);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn streaming_lossless_claude_gemini() {
    let reg = registry();
    let r = reg.lookup(Dialect::Claude, Dialect::Gemini, features::STREAMING);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn streaming_lossless_gemini_codex() {
    let reg = registry();
    let r = reg.lookup(Dialect::Gemini, Dialect::Codex, features::STREAMING);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn streaming_lossless_kimi_copilot() {
    let reg = registry();
    let r = reg.lookup(Dialect::Kimi, Dialect::Copilot, features::STREAMING);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn streaming_lossless_copilot_openai() {
    let reg = registry();
    let r = reg.lookup(Dialect::Copilot, Dialect::OpenAi, features::STREAMING);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn streaming_all_cross_pairs_have_rules() {
    let reg = registry();
    for &src in all_dialects() {
        for &tgt in all_dialects() {
            let r = reg.lookup(src, tgt, features::STREAMING);
            assert!(r.is_some(), "missing streaming rule {src}->{tgt}");
        }
    }
}

#[test]
fn cross_dialect_text_mapping_matrix_streaming() {
    let reg = registry();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Gemini,
        &["streaming".into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_lossless());
}

#[test]
fn validate_mapping_openai_to_claude_tool_use() {
    let reg = registry();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["tool_use".into()]);
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_lossless());
    assert!(results[0].errors.is_empty());
}

#[test]
fn validate_mapping_claude_to_openai_tool_use() {
    let reg = registry();
    let results = validate_mapping(&reg, Dialect::Claude, Dialect::OpenAi, &["tool_use".into()]);
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_lossless());
}

#[test]
fn validate_mapping_gemini_to_claude_tool_use() {
    let reg = registry();
    let results = validate_mapping(&reg, Dialect::Gemini, Dialect::Claude, &["tool_use".into()]);
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_lossless());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Tool use preservation across dialects
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_use_ir_block_roundtrip() {
    let block = tool_use_ir_block("tu-1", "read_file");
    let json = serde_json::to_value(&block).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn tool_result_ir_block_roundtrip() {
    let block = tool_result_ir_block("tu-1");
    let json = serde_json::to_value(&block).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn tool_definition_roundtrip() {
    let def = sample_tool_def();
    let json = serde_json::to_value(&def).unwrap();
    let back: IrToolDefinition = serde_json::from_value(json).unwrap();
    assert_eq!(def, back);
}

#[test]
fn tool_use_openai_to_codex_lossy() {
    let reg = registry();
    let r = reg.lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE);
    assert!(r.is_some());
    assert!(matches!(r.unwrap().fidelity, Fidelity::LossyLabeled { .. }));
}

#[test]
fn tool_use_claude_to_codex_lossy() {
    let reg = registry();
    let r = reg.lookup(Dialect::Claude, Dialect::Codex, features::TOOL_USE);
    assert!(r.is_some());
    assert!(matches!(r.unwrap().fidelity, Fidelity::LossyLabeled { .. }));
}

#[test]
fn tool_use_kimi_to_openai_lossless() {
    let reg = registry();
    let r = reg.lookup(Dialect::Kimi, Dialect::OpenAi, features::TOOL_USE);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn tool_use_copilot_to_claude_lossless() {
    let reg = registry();
    let r = reg.lookup(Dialect::Copilot, Dialect::Claude, features::TOOL_USE);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn tool_use_message_with_mixed_blocks_roundtrip() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me read the file.".into(),
            },
            tool_use_ir_block("tu-42", "read_file"),
        ],
    );
    let json = serde_json::to_value(&msg).unwrap();
    let back: IrMessage = serde_json::from_value(json).unwrap();
    assert_eq!(msg, back);
    assert!(!back.is_text_only());
    assert_eq!(back.tool_use_blocks().len(), 1);
}

#[test]
fn tool_use_codex_to_gemini_lossy() {
    let reg = registry();
    let r = reg.lookup(Dialect::Codex, Dialect::Gemini, features::TOOL_USE);
    assert!(r.is_some());
    assert!(matches!(r.unwrap().fidelity, Fidelity::LossyLabeled { .. }));
}

#[test]
fn tool_use_kimi_copilot_lossless() {
    let reg = registry();
    let r = reg.lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_lossless());
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Thinking block handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_ir_block_roundtrip() {
    let block = thinking_ir_block("step 1: analyze the code");
    let json = serde_json::to_value(&block).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn thinking_claude_to_openai_lossy() {
    let reg = registry();
    let r = reg.lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING);
    assert!(r.is_some());
    assert!(matches!(r.unwrap().fidelity, Fidelity::LossyLabeled { .. }));
}

#[test]
fn thinking_claude_to_gemini_lossy() {
    let reg = registry();
    let r = reg.lookup(Dialect::Claude, Dialect::Gemini, features::THINKING);
    assert!(r.is_some());
    assert!(matches!(r.unwrap().fidelity, Fidelity::LossyLabeled { .. }));
}

#[test]
fn thinking_openai_to_claude_lossy() {
    let reg = registry();
    let r = reg.lookup(Dialect::OpenAi, Dialect::Claude, features::THINKING);
    assert!(r.is_some());
    assert!(matches!(r.unwrap().fidelity, Fidelity::LossyLabeled { .. }));
}

#[test]
fn thinking_kimi_to_claude_lossy() {
    let reg = registry();
    let r = reg.lookup(Dialect::Kimi, Dialect::Claude, features::THINKING);
    assert!(r.is_some());
    assert!(matches!(r.unwrap().fidelity, Fidelity::LossyLabeled { .. }));
}

#[test]
fn thinking_copilot_to_openai_lossy() {
    let reg = registry();
    let r = reg.lookup(Dialect::Copilot, Dialect::OpenAi, features::THINKING);
    assert!(r.is_some());
    assert!(matches!(r.unwrap().fidelity, Fidelity::LossyLabeled { .. }));
}

#[test]
fn thinking_message_with_text_and_thinking_roundtrip() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            thinking_ir_block("I should check the imports"),
            IrContentBlock::Text {
                text: "The imports look correct.".into(),
            },
        ],
    );
    let json = serde_json::to_value(&msg).unwrap();
    let back: IrMessage = serde_json::from_value(json).unwrap();
    assert_eq!(msg, back);
    assert!(!back.is_text_only());
}

#[test]
fn thinking_all_cross_pairs_have_rules() {
    let reg = registry();
    for &src in all_dialects() {
        for &tgt in all_dialects() {
            let r = reg.lookup(src, tgt, features::THINKING);
            assert!(r.is_some(), "missing thinking rule {src}->{tgt}");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Image support validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn image_ir_block_roundtrip() {
    let block = image_ir_block();
    let json = serde_json::to_value(&block).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn image_openai_to_claude_lossless() {
    let reg = registry();
    let r = reg.lookup(Dialect::OpenAi, Dialect::Claude, features::IMAGE_INPUT);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn image_openai_to_gemini_lossless() {
    let reg = registry();
    let r = reg.lookup(Dialect::OpenAi, Dialect::Gemini, features::IMAGE_INPUT);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn image_openai_to_codex_unsupported() {
    let reg = registry();
    let r = reg.lookup(Dialect::OpenAi, Dialect::Codex, features::IMAGE_INPUT);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_unsupported());
}

#[test]
fn image_claude_to_codex_unsupported() {
    let reg = registry();
    let r = reg.lookup(Dialect::Claude, Dialect::Codex, features::IMAGE_INPUT);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_unsupported());
}

#[test]
fn image_gemini_to_codex_unsupported() {
    let reg = registry();
    let r = reg.lookup(Dialect::Gemini, Dialect::Codex, features::IMAGE_INPUT);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_unsupported());
}

#[test]
fn image_kimi_to_any_unsupported() {
    let reg = registry();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let r = reg.lookup(Dialect::Kimi, tgt, features::IMAGE_INPUT);
        assert!(r.is_some(), "missing image_input rule Kimi->{tgt}");
        assert!(
            r.unwrap().fidelity.is_unsupported(),
            "Kimi->{tgt} image should be unsupported"
        );
    }
}

#[test]
fn image_copilot_to_any_unsupported() {
    let reg = registry();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let r = reg.lookup(Dialect::Copilot, tgt, features::IMAGE_INPUT);
        assert!(r.is_some(), "missing image_input rule Copilot->{tgt}");
        assert!(
            r.unwrap().fidelity.is_unsupported(),
            "Copilot->{tgt} image should be unsupported"
        );
    }
}

#[test]
fn image_message_with_text_roundtrip() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What's in this image?".into(),
            },
            image_ir_block(),
        ],
    );
    let json = serde_json::to_value(&msg).unwrap();
    let back: IrMessage = serde_json::from_value(json).unwrap();
    assert_eq!(msg, back);
    assert!(!back.is_text_only());
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Multi-turn conversation mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn conversation_roundtrip_simple() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::text(IrRole::Assistant, "Hi there!"));
    let json = serde_json::to_value(&conv).unwrap();
    let back: IrConversation = serde_json::from_value(json).unwrap();
    assert_eq!(conv, back);
    assert_eq!(back.len(), 3);
}

#[test]
fn conversation_system_message_accessor() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Be concise."))
        .push(IrMessage::text(IrRole::User, "Hi"));
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.text_content(), "Be concise.");
}

#[test]
fn conversation_last_assistant_accessor() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "1"))
        .push(IrMessage::text(IrRole::Assistant, "first"))
        .push(IrMessage::text(IrRole::User, "2"))
        .push(IrMessage::text(IrRole::Assistant, "second"));
    assert_eq!(conv.last_assistant().unwrap().text_content(), "second");
}

#[test]
fn conversation_tool_calls_collected() {
    let conv = IrConversation::new()
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_ir_block("t1", "read_file")],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![tool_result_ir_block("t1")],
        ))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_ir_block("t2", "write_file")],
        ));
    assert_eq!(conv.tool_calls().len(), 2);
}

#[test]
fn conversation_messages_by_role() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "a"))
        .push(IrMessage::text(IrRole::Assistant, "b"))
        .push(IrMessage::text(IrRole::User, "c"));
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 1);
}

#[test]
fn conversation_empty() {
    let conv = IrConversation::new();
    assert!(conv.is_empty());
    assert_eq!(conv.len(), 0);
    assert!(conv.system_message().is_none());
    assert!(conv.last_assistant().is_none());
    assert!(conv.last_message().is_none());
}

#[test]
fn conversation_with_tool_turn_roundtrip() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Read main.rs"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Reading...".into(),
                },
                tool_use_ir_block("tu-1", "read_file"),
            ],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![tool_result_ir_block("tu-1")],
        ))
        .push(IrMessage::text(IrRole::Assistant, "Done."));
    let json = serde_json::to_value(&conv).unwrap();
    let back: IrConversation = serde_json::from_value(json).unwrap();
    assert_eq!(conv, back);
    assert_eq!(back.len(), 4);
}

#[test]
fn conversation_from_messages() {
    let msgs = vec![
        IrMessage::text(IrRole::User, "a"),
        IrMessage::text(IrRole::Assistant, "b"),
    ];
    let conv = IrConversation::from_messages(msgs.clone());
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages, msgs);
}

#[test]
fn conversation_last_message() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hello"))
        .push(IrMessage::text(IrRole::Assistant, "world"));
    assert_eq!(conv.last_message().unwrap().text_content(), "world");
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Fidelity loss detection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_loss_codex_tool_use_from_openai() {
    let reg = registry();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Codex, &["tool_use".into()]);
    assert_eq!(results.len(), 1);
    assert!(matches!(results[0].fidelity, Fidelity::LossyLabeled { .. }));
    assert!(!results[0].errors.is_empty());
}

#[test]
fn fidelity_loss_thinking_claude_to_openai() {
    let reg = registry();
    let results = validate_mapping(&reg, Dialect::Claude, Dialect::OpenAi, &["thinking".into()]);
    assert!(matches!(results[0].fidelity, Fidelity::LossyLabeled { .. }));
    assert_eq!(results[0].errors.len(), 1);
    assert!(matches!(
        results[0].errors[0],
        MappingError::FidelityLoss { .. }
    ));
}

#[test]
fn fidelity_lossless_streaming_openai_codex() {
    let reg = registry();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Codex, &["streaming".into()]);
    assert!(results[0].fidelity.is_lossless());
    assert!(results[0].errors.is_empty());
}

#[test]
fn fidelity_matrix_from_registry_marks_supported_pairs() {
    let reg = registry();
    let matrix = MappingMatrix::from_registry(&reg);
    // OpenAI→Claude has multiple supported features
    assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(matrix.is_supported(Dialect::Claude, Dialect::OpenAi));
    assert!(matrix.is_supported(Dialect::Gemini, Dialect::Claude));
}

#[test]
fn fidelity_matrix_unsupported_pairs_default_false() {
    let matrix = MappingMatrix::new();
    assert!(!matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn fidelity_code_exec_kimi_to_any_unsupported() {
    let reg = registry();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ] {
        let r = reg.lookup(Dialect::Kimi, tgt, features::CODE_EXEC);
        assert!(r.is_some(), "missing code_exec rule Kimi->{tgt}");
        assert!(
            r.unwrap().fidelity.is_unsupported(),
            "Kimi->{tgt} code_exec should be unsupported"
        );
    }
}

#[test]
fn fidelity_code_exec_cross_dialect_lossy() {
    let reg = registry();
    let r = reg.lookup(Dialect::OpenAi, Dialect::Claude, features::CODE_EXEC);
    assert!(r.is_some());
    assert!(matches!(r.unwrap().fidelity, Fidelity::LossyLabeled { .. }));
}

#[test]
fn fidelity_rank_targets_streaming() {
    let reg = registry();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::STREAMING]);
    assert!(!ranked.is_empty());
    // All streaming mappings are lossless, so every target should have lossless_count = 1
    for &(_, lossless_count) in &ranked {
        assert_eq!(lossless_count, 1);
    }
}

#[test]
fn fidelity_rank_targets_excludes_source() {
    let reg = registry();
    let ranked = reg.rank_targets(Dialect::Claude, &[features::TOOL_USE]);
    for &(d, _) in &ranked {
        assert_ne!(d, Dialect::Claude, "source dialect should not appear");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Error handling for unsupported mappings
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_feature_unsupported_image_to_codex() {
    let reg = registry();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &["image_input".into()],
    );
    assert!(results[0].fidelity.is_unsupported());
    assert!(matches!(
        results[0].errors[0],
        MappingError::FeatureUnsupported { .. }
    ));
}

#[test]
fn error_unknown_feature_produces_unsupported() {
    let reg = registry();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["nonexistent_feature".into()],
    );
    assert!(results[0].fidelity.is_unsupported());
    assert!(matches!(
        results[0].errors[0],
        MappingError::FeatureUnsupported { .. }
    ));
}

#[test]
fn error_empty_feature_name() {
    let reg = registry();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert!(results[0].fidelity.is_unsupported());
    assert!(matches!(
        results[0].errors[0],
        MappingError::InvalidInput { .. }
    ));
}

#[test]
fn error_multiple_features_mixed_results() {
    let reg = registry();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &["streaming".into(), "image_input".into(), "tool_use".into()],
    );
    assert_eq!(results.len(), 3);
    assert!(results[0].fidelity.is_lossless()); // streaming
    assert!(results[1].fidelity.is_unsupported()); // image_input
    assert!(matches!(results[2].fidelity, Fidelity::LossyLabeled { .. })); // tool_use
}

#[test]
fn error_mapping_error_display() {
    let err = MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    let s = err.to_string();
    assert!(s.contains("logprobs"));
    assert!(s.contains("Claude"));
    assert!(s.contains("Gemini"));
}

#[test]
fn error_fidelity_loss_display() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "mapped to system message".into(),
    };
    let s = err.to_string();
    assert!(s.contains("thinking"));
    assert!(s.contains("mapped to system message"));
}

#[test]
fn error_dialect_mismatch_display() {
    let err = MappingError::DialectMismatch {
        from: Dialect::Kimi,
        to: Dialect::Codex,
    };
    let s = err.to_string();
    assert!(s.contains("Kimi"));
    assert!(s.contains("Codex"));
}

#[test]
fn error_invalid_input_display() {
    let err = MappingError::InvalidInput {
        reason: "bad json".into(),
    };
    assert!(err.to_string().contains("bad json"));
}

#[test]
fn error_kimi_image_to_copilot_unsupported() {
    let reg = registry();
    let r = reg.lookup(Dialect::Kimi, Dialect::Copilot, features::IMAGE_INPUT);
    assert!(r.is_some());
    assert!(r.unwrap().fidelity.is_unsupported());
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Capability-based routing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_negotiate_native_streaming() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);

    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native, vec![Capability::Streaming]);
}

#[test]
fn capability_negotiate_emulated_tool_read() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Emulated);

    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.emulated_caps(), vec![Capability::ToolRead]);
}

#[test]
fn capability_negotiate_unsupported() {
    let manifest = CapabilityManifest::new();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::CodeExecution,
            min_support: MinSupport::Native,
        }],
    };
    let result = negotiate(&manifest, &reqs);
    assert!(!result.is_compatible());
    assert_eq!(result.unsupported_caps(), vec![Capability::CodeExecution]);
}

#[test]
fn capability_check_native() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolUse, SupportLevel::Native);
    assert_eq!(
        check_capability(&manifest, &Capability::ToolUse),
        CapSupportLevel::Native
    );
}

#[test]
fn capability_check_emulated() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolWrite, SupportLevel::Emulated);
    assert!(matches!(
        check_capability(&manifest, &Capability::ToolWrite),
        CapSupportLevel::Emulated { .. }
    ));
}

#[test]
fn capability_check_missing_is_unsupported() {
    let manifest = CapabilityManifest::new();
    assert_eq!(
        check_capability(&manifest, &Capability::Logprobs),
        CapSupportLevel::Unsupported {
            reason: "not declared in manifest".into()
        }
    );
}

#[test]
fn capability_report_compatible() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolUse],
        vec![],
        vec![],
    );
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 2);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn capability_report_incompatible() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![Capability::ImageInput]);
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert_eq!(report.unsupported_count, 1);
}

#[test]
fn capability_routing_rank_by_lossless_count() {
    let reg = registry();
    let ranked = reg.rank_targets(
        Dialect::OpenAi,
        &[
            features::TOOL_USE,
            features::STREAMING,
            features::IMAGE_INPUT,
        ],
    );
    // Claude and Gemini should score higher than Codex (which has unsupported image)
    assert!(!ranked.is_empty());
    let codex_pos = ranked.iter().position(|&(d, _)| d == Dialect::Codex);
    let claude_pos = ranked.iter().position(|&(d, _)| d == Dialect::Claude);
    if let (Some(cp), Some(cdp)) = (claude_pos, codex_pos) {
        assert!(cp < cdp, "Claude should rank above Codex");
    }
}

#[test]
fn capability_restricted_treated_as_emulated() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    assert!(matches!(
        check_capability(&manifest, &Capability::ToolBash),
        CapSupportLevel::Restricted { .. }
    ));
}

#[test]
fn capability_negotiation_total() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    assert_eq!(result.total(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional cross-cutting roundtrip tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_usage_roundtrip() {
    let usage = IrUsage::from_io(100, 50);
    let json = serde_json::to_value(usage).unwrap();
    let back: IrUsage = serde_json::from_value(json).unwrap();
    assert_eq!(usage, back);
    assert_eq!(back.total_tokens, 150);
}

#[test]
fn ir_usage_with_cache_roundtrip() {
    let usage = IrUsage::with_cache(200, 100, 50, 25);
    let json = serde_json::to_value(usage).unwrap();
    let back: IrUsage = serde_json::from_value(json).unwrap();
    assert_eq!(usage, back);
    assert_eq!(back.cache_read_tokens, 50);
}

#[test]
fn ir_usage_merge() {
    let a = IrUsage::from_io(100, 50);
    let b = IrUsage::from_io(200, 75);
    let merged = a.merge(b);
    assert_eq!(merged.input_tokens, 300);
    assert_eq!(merged.output_tokens, 125);
    assert_eq!(merged.total_tokens, 425);
}

#[test]
fn ir_message_metadata_preserved() {
    let mut msg = IrMessage::text(IrRole::User, "test");
    msg.metadata
        .insert("vendor_id".into(), serde_json::json!("abc-123"));
    let json = serde_json::to_value(&msg).unwrap();
    let back: IrMessage = serde_json::from_value(json).unwrap();
    assert_eq!(
        back.metadata.get("vendor_id"),
        Some(&serde_json::json!("abc-123"))
    );
}

#[test]
fn ir_tool_result_error_flag_roundtrip() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "err-1".into(),
        content: vec![IrContentBlock::Text {
            text: "permission denied".into(),
        }],
        is_error: true,
    };
    let json = serde_json::to_value(&block).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn mapping_rule_serde_roundtrip() {
    let rule = MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    };
    let json = serde_json::to_value(&rule).unwrap();
    let back: MappingRule = serde_json::from_value(json).unwrap();
    assert_eq!(rule, back);
}

#[test]
fn mapping_rule_lossy_serde_roundtrip() {
    let rule = MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::Codex,
        feature: "thinking".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "budget differs".into(),
        },
    };
    let json = serde_json::to_value(&rule).unwrap();
    let back: MappingRule = serde_json::from_value(json).unwrap();
    assert_eq!(rule, back);
}

#[test]
fn fidelity_serde_roundtrip_all_variants() {
    for fidelity in [
        Fidelity::Lossless,
        Fidelity::LossyLabeled {
            warning: "test".into(),
        },
        Fidelity::Unsupported {
            reason: "nope".into(),
        },
    ] {
        let json = serde_json::to_value(&fidelity).unwrap();
        let back: Fidelity = serde_json::from_value(json).unwrap();
        assert_eq!(fidelity, back);
    }
}

#[test]
fn dialect_serde_roundtrip_all() {
    for &d in all_dialects() {
        let json = serde_json::to_value(d).unwrap();
        let back: Dialect = serde_json::from_value(json).unwrap();
        assert_eq!(d, back);
    }
}

#[test]
fn dialect_display_all() {
    let labels: Vec<&str> = all_dialects().iter().map(|d| d.label()).collect();
    assert_eq!(
        labels,
        vec!["OpenAI", "Claude", "Gemini", "Codex", "Kimi", "Copilot"]
    );
}

#[test]
fn known_rules_non_empty() {
    let reg = registry();
    assert!(!reg.is_empty());
    // At minimum: 6 dialects × 5 features self-rules = 30
    assert!(reg.len() >= 30);
}

#[test]
fn mapping_matrix_from_registry_symmetric_for_lossless() {
    let reg = registry();
    let matrix = MappingMatrix::from_registry(&reg);
    // OpenAI↔Claude tool_use is lossless in both directions
    assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(matrix.is_supported(Dialect::Claude, Dialect::OpenAi));
}

#[test]
fn mapping_registry_iter() {
    let reg = registry();
    let count = reg.iter().count();
    assert_eq!(count, reg.len());
}

#[test]
fn mapping_error_serde_roundtrip() {
    let err = MappingError::FeatureUnsupported {
        feature: "image_input".into(),
        from: Dialect::Kimi,
        to: Dialect::Codex,
    };
    let json = serde_json::to_value(&err).unwrap();
    let back: MappingError = serde_json::from_value(json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn mapping_validation_serde_roundtrip() {
    let v = MappingValidation {
        feature: "streaming".into(),
        fidelity: Fidelity::Lossless,
        errors: vec![],
    };
    let json = serde_json::to_value(&v).unwrap();
    let back: MappingValidation = serde_json::from_value(json).unwrap();
    assert_eq!(v, back);
}
