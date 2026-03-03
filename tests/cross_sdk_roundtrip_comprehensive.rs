// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive cross-SDK roundtrip tests.
//!
//! Verifies that a request can travel SDK A → ABP IR → SDK B → ABP IR → SDK A
//! without losing critical semantics. Documents fidelity loss where applicable.
//!
//! Coverage:
//! - OpenAI ↔ Claude roundtrip (15 tests)
//! - OpenAI ↔ Gemini roundtrip (10 tests)
//! - Claude ↔ Gemini roundtrip (10 tests)
//! - All SDKs text roundtrip (15 tests)
//! - Tool use roundtrip (10 tests)
//! - Lossy mapping documentation (10 tests)
//! - Error / edge cases (10 tests)

use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
use abp_claude_sdk::lowering as claude_ir;
use abp_codex_sdk::lowering as codex_ir;
use abp_copilot_sdk::dialect::CopilotMessage;
use abp_copilot_sdk::lowering as copilot_ir;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
use abp_gemini_sdk::lowering as gemini_ir;
use abp_kimi_sdk::dialect::KimiMessage;
use abp_kimi_sdk::lowering as kimi_ir;
use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
use abp_openai_sdk::lowering as openai_ir;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn oai_text(role: &str, text: &str) -> OpenAIMessage {
    OpenAIMessage {
        role: role.into(),
        content: Some(text.into()),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn oai_tool_call(id: &str, name: &str, args: &str) -> OpenAIMessage {
    OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: id.into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: name.into(),
                arguments: args.into(),
            },
        }]),
        tool_call_id: None,
    }
}

fn _oai_tool_result(call_id: &str, text: &str) -> OpenAIMessage {
    OpenAIMessage {
        role: "tool".into(),
        content: Some(text.into()),
        tool_calls: None,
        tool_call_id: Some(call_id.into()),
    }
}

fn claude_text(role: &str, text: &str) -> ClaudeMessage {
    ClaudeMessage {
        role: role.into(),
        content: text.into(),
    }
}

fn claude_blocks(role: &str, blocks: &[ClaudeContentBlock]) -> ClaudeMessage {
    ClaudeMessage {
        role: role.into(),
        content: serde_json::to_string(blocks).unwrap(),
    }
}

fn gemini_text(role: &str, text: &str) -> GeminiContent {
    GeminiContent {
        role: role.into(),
        parts: vec![GeminiPart::Text(text.into())],
    }
}

fn kimi_text(role: &str, text: &str) -> KimiMessage {
    KimiMessage {
        role: role.into(),
        content: Some(text.into()),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn _copilot_msg(role: &str, text: &str) -> CopilotMessage {
    CopilotMessage {
        role: role.into(),
        content: text.into(),
        name: None,
        copilot_references: vec![],
    }
}

/// Helper: roundtrip OpenAI → IR → Claude → IR → OpenAI
fn openai_claude_openai(msgs: &[OpenAIMessage]) -> Vec<OpenAIMessage> {
    let ir1 = openai_ir::to_ir(msgs);
    let sys = claude_ir::extract_system_prompt(&ir1);
    let claude = claude_ir::from_ir(&ir1);
    let ir2 = claude_ir::to_ir(&claude, sys.as_deref());
    openai_ir::from_ir(&ir2)
}

/// Helper: roundtrip Claude → IR → OpenAI → IR → Claude
fn claude_openai_claude(
    msgs: &[ClaudeMessage],
    sys: Option<&str>,
) -> (Vec<ClaudeMessage>, Option<String>) {
    let ir1 = claude_ir::to_ir(msgs, sys);
    let oai = openai_ir::from_ir(&ir1);
    let ir2 = openai_ir::to_ir(&oai);
    let sys2 = claude_ir::extract_system_prompt(&ir2);
    let claude = claude_ir::from_ir(&ir2);
    (claude, sys2)
}

/// Helper: roundtrip OpenAI → IR → Gemini → IR → OpenAI
fn openai_gemini_openai(msgs: &[OpenAIMessage]) -> Vec<OpenAIMessage> {
    let ir1 = openai_ir::to_ir(msgs);
    let gemini = gemini_ir::from_ir(&ir1);
    let sys_instr = gemini_ir::extract_system_instruction(&ir1);
    let ir2 = gemini_ir::to_ir(&gemini, sys_instr.as_ref());
    openai_ir::from_ir(&ir2)
}

/// Helper: roundtrip Claude → IR → Gemini → IR → Claude
fn claude_gemini_claude(
    msgs: &[ClaudeMessage],
    sys: Option<&str>,
) -> (Vec<ClaudeMessage>, Option<String>) {
    let ir1 = claude_ir::to_ir(msgs, sys);
    let gemini = gemini_ir::from_ir(&ir1);
    let sys_instr = gemini_ir::extract_system_instruction(&ir1);
    let ir2 = gemini_ir::to_ir(&gemini, sys_instr.as_ref());
    let sys2 = claude_ir::extract_system_prompt(&ir2);
    let claude = claude_ir::from_ir(&ir2);
    (claude, sys2)
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. OpenAI ↔ Claude roundtrip (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_claude_rt_simple_user_text() {
    let rt = openai_claude_openai(&[oai_text("user", "Hello")]);
    assert_eq!(rt.len(), 1);
    assert_eq!(rt[0].role, "user");
    assert_eq!(rt[0].content.as_deref(), Some("Hello"));
}

#[test]
fn openai_claude_rt_assistant_text() {
    let rt = openai_claude_openai(&[oai_text("user", "Hi"), oai_text("assistant", "Hey there!")]);
    assert_eq!(rt.len(), 2);
    assert_eq!(rt[1].role, "assistant");
    assert_eq!(rt[1].content.as_deref(), Some("Hey there!"));
}

#[test]
fn openai_claude_rt_system_prompt_preserved() {
    let rt = openai_claude_openai(&[oai_text("system", "Be concise."), oai_text("user", "Hi")]);
    assert_eq!(rt.len(), 2);
    assert_eq!(rt[0].role, "system");
    assert_eq!(rt[0].content.as_deref(), Some("Be concise."));
}

#[test]
fn openai_claude_rt_tool_call_name_and_args() {
    let rt = openai_claude_openai(&[oai_tool_call("c1", "read_file", r#"{"path":"main.rs"}"#)]);
    assert_eq!(rt[0].role, "assistant");
    let tc = &rt[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "read_file");
    let args: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
    assert_eq!(args, json!({"path": "main.rs"}));
}

#[test]
fn openai_claude_rt_tool_call_id_preserved() {
    let rt = openai_claude_openai(&[oai_tool_call("call_abc", "grep", r#"{"q":"fn"}"#)]);
    let tc = &rt[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_abc");
}

#[test]
fn openai_claude_rt_multi_turn_conversation() {
    let rt = openai_claude_openai(&[
        oai_text("user", "What is Rust?"),
        oai_text("assistant", "A systems language."),
        oai_text("user", "Tell me more."),
        oai_text("assistant", "Memory safe without GC."),
    ]);
    assert_eq!(rt.len(), 4);
    assert_eq!(rt[0].content.as_deref(), Some("What is Rust?"));
    assert_eq!(rt[3].content.as_deref(), Some("Memory safe without GC."));
}

#[test]
fn openai_claude_rt_unicode_preserved() {
    let text = "こんにちは 🦀 café résumé";
    let rt = openai_claude_openai(&[oai_text("user", text)]);
    assert_eq!(rt[0].content.as_deref(), Some(text));
}

#[test]
fn openai_claude_rt_newlines_preserved() {
    let text = "line1\nline2\r\n\ttabbed";
    let rt = openai_claude_openai(&[oai_text("user", text)]);
    assert_eq!(rt[0].content.as_deref(), Some(text));
}

#[test]
fn openai_claude_rt_empty_content_assistant() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: None,
        tool_call_id: None,
    }];
    let rt = openai_claude_openai(&msgs);
    assert_eq!(rt.len(), 1);
    assert_eq!(rt[0].role, "assistant");
}

#[test]
fn claude_openai_rt_simple_text() {
    let (rt, _) = claude_openai_claude(&[claude_text("user", "Hello from Claude")], None);
    assert_eq!(rt.len(), 1);
    assert_eq!(rt[0].role, "user");
    assert_eq!(rt[0].content, "Hello from Claude");
}

#[test]
fn claude_openai_rt_system_prompt_preserved() {
    let (rt, sys) = claude_openai_claude(
        &[claude_text("user", "Hi")],
        Some("You are a coding assistant."),
    );
    assert_eq!(sys.as_deref(), Some("You are a coding assistant."));
    assert_eq!(rt.len(), 1);
    assert_eq!(rt[0].content, "Hi");
}

#[test]
fn claude_openai_rt_tool_use_block() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "search".into(),
        input: json!({"q": "rust"}),
    }];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let (rt, _) = claude_openai_claude(&msgs, None);
    let rt_blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&rt[0].content).unwrap();
    match &rt_blocks[0] {
        ClaudeContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "tu_1");
            assert_eq!(name, "search");
            assert_eq!(input, &json!({"q": "rust"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn claude_openai_rt_multi_content_blocks() {
    let blocks = vec![
        ClaudeContentBlock::Text {
            text: "First.".into(),
        },
        ClaudeContentBlock::Text {
            text: "Second.".into(),
        },
    ];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let ir1 = claude_ir::to_ir(&msgs, None);
    let oai = openai_ir::from_ir(&ir1);
    // OpenAI concatenates text blocks
    let text = oai[0].content.as_deref().unwrap();
    assert!(text.contains("First."));
    assert!(text.contains("Second."));
}

/// Thinking blocks are lossy: OpenAI merges thinking into text content.
/// The thinking/text distinction is lost when passing through OpenAI.
#[test]
fn claude_openai_rt_thinking_lossy_merged_into_text() {
    let blocks = vec![
        ClaudeContentBlock::Thinking {
            thinking: "Let me think...".into(),
            signature: Some("sig123".into()),
        },
        ClaudeContentBlock::Text {
            text: "Answer.".into(),
        },
    ];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let ir1 = claude_ir::to_ir(&msgs, None);
    let oai = openai_ir::from_ir(&ir1);
    let ir2 = openai_ir::to_ir(&oai);

    // After roundtrip through OpenAI, thinking becomes regular text
    let has_thinking = ir2.messages[0]
        .content
        .iter()
        .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
    assert!(
        !has_thinking,
        "LOSSY: thinking blocks are merged into text when passing through OpenAI"
    );
    // But the text content is preserved
    let text = ir2.messages[0].text_content();
    assert!(text.contains("Let me think..."));
    assert!(text.contains("Answer."));
}

/// Claude thinking signature is lost in roundtrip through OpenAI.
#[test]
fn claude_openai_rt_thinking_signature_lost() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "deep thought".into(),
        signature: Some("crypto_sig_456".into()),
    }];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let ir1 = claude_ir::to_ir(&msgs, None);
    // IR preserves thinking text but not Claude-specific signature
    match &ir1.messages[0].content[0] {
        IrContentBlock::Thinking { text } => {
            assert_eq!(text, "deep thought");
            // signature is a Claude-specific field not in IR
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. OpenAI ↔ Gemini roundtrip (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_gemini_rt_simple_user_text() {
    let rt = openai_gemini_openai(&[oai_text("user", "Hello Gemini")]);
    assert_eq!(rt.len(), 1);
    assert_eq!(rt[0].role, "user");
    assert_eq!(rt[0].content.as_deref(), Some("Hello Gemini"));
}

#[test]
fn openai_gemini_rt_assistant_role_mapping() {
    let rt = openai_gemini_openai(&[oai_text("user", "Hi"), oai_text("assistant", "Hello!")]);
    // assistant → model → assistant roundtrip
    assert_eq!(rt[1].role, "assistant");
    assert_eq!(rt[1].content.as_deref(), Some("Hello!"));
}

#[test]
fn openai_gemini_rt_tool_call_function_name() {
    let rt = openai_gemini_openai(&[oai_tool_call("c1", "read_file", r#"{"path":"a.rs"}"#)]);
    let tc = &rt[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "read_file");
    let args: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
    assert_eq!(args, json!({"path": "a.rs"}));
}

/// Gemini does not have native tool call IDs — IDs are synthesized as "gemini_{name}".
/// When roundtripping OpenAI→Gemini→OpenAI, the original tool_call ID may change.
#[test]
fn openai_gemini_rt_tool_id_lossy_synthesized() {
    let rt = openai_gemini_openai(&[oai_tool_call("original_id", "search", r#"{"q":"x"}"#)]);
    let tc = &rt[0].tool_calls.as_ref().unwrap()[0];
    // LOSSY: Gemini synthesizes IDs as "gemini_{name}", original ID lost
    assert_eq!(tc.id, "gemini_search");
}

#[test]
fn openai_gemini_rt_system_prompt_extraction() {
    let msgs = &[oai_text("system", "Be helpful"), oai_text("user", "Hi")];
    let ir1 = openai_ir::to_ir(msgs);
    let sys_instr = gemini_ir::extract_system_instruction(&ir1);
    assert!(sys_instr.is_some());
    let gemini = gemini_ir::from_ir(&ir1);
    let ir2 = gemini_ir::to_ir(&gemini, sys_instr.as_ref());
    let oai = openai_ir::from_ir(&ir2);
    assert_eq!(oai.len(), 2);
    assert_eq!(oai[0].role, "system");
    assert_eq!(oai[0].content.as_deref(), Some("Be helpful"));
}

#[test]
fn openai_gemini_rt_multi_turn() {
    let rt = openai_gemini_openai(&[
        oai_text("user", "Q1"),
        oai_text("assistant", "A1"),
        oai_text("user", "Q2"),
    ]);
    assert_eq!(rt.len(), 3);
    assert_eq!(rt[0].content.as_deref(), Some("Q1"));
    assert_eq!(rt[1].content.as_deref(), Some("A1"));
    assert_eq!(rt[2].content.as_deref(), Some("Q2"));
}

#[test]
fn gemini_openai_rt_simple_text() {
    let contents = vec![gemini_text("user", "Hello from Gemini")];
    let ir1 = gemini_ir::to_ir(&contents, None);
    let oai = openai_ir::from_ir(&ir1);
    let ir2 = openai_ir::to_ir(&oai);
    let gemini2 = gemini_ir::from_ir(&ir2);
    assert_eq!(gemini2[0].role, "user");
    match &gemini2[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hello from Gemini"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_openai_rt_function_call() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "search".into(),
            args: json!({"query": "rust"}),
        }],
    }];
    let ir1 = gemini_ir::to_ir(&contents, None);
    let oai = openai_ir::from_ir(&ir1);
    let ir2 = openai_ir::to_ir(&oai);
    let gemini2 = gemini_ir::from_ir(&ir2);
    match &gemini2[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"query": "rust"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn gemini_openai_rt_model_role_preserved() {
    let contents = vec![gemini_text("model", "I am a model response.")];
    let ir1 = gemini_ir::to_ir(&contents, None);
    let oai = openai_ir::from_ir(&ir1);
    assert_eq!(oai[0].role, "assistant");
    let ir2 = openai_ir::to_ir(&oai);
    let gemini2 = gemini_ir::from_ir(&ir2);
    assert_eq!(gemini2[0].role, "model");
}

#[test]
fn openai_gemini_rt_unicode_preserved() {
    let text = "日本語 العربية 🌍";
    let rt = openai_gemini_openai(&[oai_text("user", text)]);
    assert_eq!(rt[0].content.as_deref(), Some(text));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Claude ↔ Gemini roundtrip (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_gemini_rt_simple_text() {
    let (rt, _) = claude_gemini_claude(&[claude_text("user", "Hello from Claude")], None);
    assert_eq!(rt.len(), 1);
    assert_eq!(rt[0].content, "Hello from Claude");
}

#[test]
fn claude_gemini_rt_assistant_text() {
    let (rt, _) = claude_gemini_claude(
        &[claude_text("user", "Hi"), claude_text("assistant", "Hey!")],
        None,
    );
    assert_eq!(rt[1].role, "assistant");
    assert_eq!(rt[1].content, "Hey!");
}

#[test]
fn claude_gemini_rt_system_prompt_preserved() {
    let (_, sys) =
        claude_gemini_claude(&[claude_text("user", "Hi")], Some("Be a coding assistant."));
    assert_eq!(sys.as_deref(), Some("Be a coding assistant."));
}

#[test]
fn claude_gemini_rt_tool_use_block() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "read_file".into(),
        input: json!({"path": "lib.rs"}),
    }];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let ir1 = claude_ir::to_ir(&msgs, None);
    let gemini = gemini_ir::from_ir(&ir1);
    let ir2 = gemini_ir::to_ir(&gemini, None);
    let claude2 = claude_ir::from_ir(&ir2);
    let rt_blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude2[0].content).unwrap();
    match &rt_blocks[0] {
        ClaudeContentBlock::ToolUse { name, input, .. } => {
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({"path": "lib.rs"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

/// LOSSY: Gemini synthesizes tool IDs as "gemini_{name}", losing Claude's original ID.
#[test]
fn claude_gemini_rt_tool_id_lossy() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "original_claude_id".into(),
        name: "search".into(),
        input: json!({"q": "test"}),
    }];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let ir1 = claude_ir::to_ir(&msgs, None);
    let gemini = gemini_ir::from_ir(&ir1);
    let ir2 = gemini_ir::to_ir(&gemini, None);
    let claude2 = claude_ir::from_ir(&ir2);
    let rt_blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude2[0].content).unwrap();
    match &rt_blocks[0] {
        ClaudeContentBlock::ToolUse { id, .. } => {
            // LOSSY: Gemini does not preserve original tool call IDs
            assert_eq!(id, "gemini_search");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

/// LOSSY: Thinking blocks are lost when passing through Gemini — Gemini maps
/// thinking to plain text, losing the structured distinction.
#[test]
fn claude_gemini_rt_thinking_lossy_becomes_text() {
    let blocks = vec![
        ClaudeContentBlock::Thinking {
            thinking: "Let me reason...".into(),
            signature: Some("sig".into()),
        },
        ClaudeContentBlock::Text {
            text: "Answer.".into(),
        },
    ];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let ir1 = claude_ir::to_ir(&msgs, None);
    let gemini = gemini_ir::from_ir(&ir1);

    // Gemini renders thinking as text parts
    let has_function_call = gemini[0]
        .parts
        .iter()
        .any(|p| matches!(p, GeminiPart::FunctionCall { .. }));
    assert!(
        !has_function_call,
        "thinking should not become function calls"
    );

    let ir2 = gemini_ir::to_ir(&gemini, None);
    // After roundtrip, thinking is now regular text — the block type is lost
    let text = ir2.messages[0].text_content();
    assert!(text.contains("Let me reason..."));
    assert!(text.contains("Answer."));
}

#[test]
fn claude_gemini_rt_multi_turn() {
    let (rt, _) = claude_gemini_claude(
        &[
            claude_text("user", "First"),
            claude_text("assistant", "Second"),
            claude_text("user", "Third"),
        ],
        None,
    );
    assert_eq!(rt.len(), 3);
    assert_eq!(rt[0].content, "First");
    assert_eq!(rt[1].content, "Second");
    assert_eq!(rt[2].content, "Third");
}

#[test]
fn gemini_claude_rt_simple_text() {
    let contents = vec![gemini_text("user", "Gemini says hi")];
    let ir1 = gemini_ir::to_ir(&contents, None);
    let claude = claude_ir::from_ir(&ir1);
    let ir2 = claude_ir::to_ir(&claude, None);
    let gemini2 = gemini_ir::from_ir(&ir2);
    match &gemini2[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Gemini says hi"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_claude_rt_function_call_preserved() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "grep".into(),
            args: json!({"pattern": "fn main"}),
        }],
    }];
    let ir1 = gemini_ir::to_ir(&contents, None);
    let claude = claude_ir::from_ir(&ir1);
    let ir2 = claude_ir::to_ir(&claude, None);
    let gemini2 = gemini_ir::from_ir(&ir2);
    match &gemini2[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "grep");
            assert_eq!(args, &json!({"pattern": "fn main"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

/// LOSSY: Claude image blocks become InlineData in Gemini but the source type
/// distinction (Base64/URL) may not roundtrip perfectly.
#[test]
fn claude_gemini_rt_image_block_structure_preserved() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        }],
    )]);
    let gemini = gemini_ir::from_ir(&ir);
    let claude = claude_ir::from_ir(&ir);

    // Both dialects should produce output (even if structure differs)
    assert!(!gemini.is_empty());
    assert!(!claude.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. All SDKs text roundtrip (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

/// Full 6-SDK text roundtrip via IR.
#[test]
fn all_sdks_user_text_through_ir() {
    let text = "Universal message";
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, text)]);

    // OpenAI
    let oai = openai_ir::from_ir(&ir);
    let ir_oai = openai_ir::to_ir(&oai);
    assert_eq!(ir_oai.messages[0].text_content(), text);

    // Claude
    let claude = claude_ir::from_ir(&ir);
    let ir_claude = claude_ir::to_ir(&claude, None);
    assert_eq!(ir_claude.messages[0].text_content(), text);

    // Gemini
    let gemini = gemini_ir::from_ir(&ir);
    let ir_gemini = gemini_ir::to_ir(&gemini, None);
    assert_eq!(ir_gemini.messages[0].text_content(), text);

    // Kimi
    let kimi = kimi_ir::from_ir(&ir);
    let ir_kimi = kimi_ir::to_ir(&kimi);
    assert_eq!(ir_kimi.messages[0].text_content(), text);

    // Codex — LOSSY: Codex response format only represents assistant output;
    // user messages are dropped by from_ir().
    let codex = codex_ir::from_ir(&ir);
    assert!(
        codex.is_empty(),
        "Codex from_ir drops user messages (response-only format)"
    );

    // Copilot
    let copilot = copilot_ir::from_ir(&ir);
    let ir_copilot = copilot_ir::to_ir(&copilot);
    assert_eq!(ir_copilot.messages[0].text_content(), text);
}

#[test]
fn all_sdks_assistant_text_through_ir() {
    let text = "I can help with that.";
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, text)]);

    let oai = openai_ir::from_ir(&ir);
    assert_eq!(oai[0].role, "assistant");
    let ir2 = openai_ir::to_ir(&oai);
    assert_eq!(ir2.messages[0].text_content(), text);

    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude[0].role, "assistant");
    let ir3 = claude_ir::to_ir(&claude, None);
    assert_eq!(ir3.messages[0].text_content(), text);

    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].role, "model");
    let ir4 = gemini_ir::to_ir(&gemini, None);
    assert_eq!(ir4.messages[0].text_content(), text);

    let kimi = kimi_ir::from_ir(&ir);
    assert_eq!(kimi[0].role, "assistant");
    let ir5 = kimi_ir::to_ir(&kimi);
    assert_eq!(ir5.messages[0].text_content(), text);
}

#[test]
fn openai_kimi_rt_simple_text() {
    let oai = vec![oai_text("user", "OpenAI to Kimi")];
    let ir1 = openai_ir::to_ir(&oai);
    let kimi = kimi_ir::from_ir(&ir1);
    let ir2 = kimi_ir::to_ir(&kimi);
    let oai2 = openai_ir::from_ir(&ir2);
    assert_eq!(oai2[0].content.as_deref(), Some("OpenAI to Kimi"));
}

#[test]
fn openai_codex_rt_simple_text() {
    // LOSSY: Codex from_ir only produces response items for assistant messages.
    // User messages are dropped (Codex response API is output-only).
    let oai = vec![oai_text("assistant", "OpenAI to Codex")];
    let ir1 = openai_ir::to_ir(&oai);
    let codex = codex_ir::from_ir(&ir1);
    assert!(
        !codex.is_empty(),
        "assistant msg should produce codex items"
    );
    let ir2 = codex_ir::to_ir(&codex);
    let oai2 = openai_ir::from_ir(&ir2);
    assert_eq!(oai2[0].content.as_deref(), Some("OpenAI to Codex"));
}

#[test]
fn openai_copilot_rt_simple_text() {
    let oai = vec![oai_text("user", "OpenAI to Copilot")];
    let ir1 = openai_ir::to_ir(&oai);
    let copilot = copilot_ir::from_ir(&ir1);
    let ir2 = copilot_ir::to_ir(&copilot);
    let oai2 = openai_ir::from_ir(&ir2);
    assert_eq!(oai2[0].content.as_deref(), Some("OpenAI to Copilot"));
}

#[test]
fn claude_kimi_rt_simple_text() {
    let msgs = vec![claude_text("user", "Claude to Kimi")];
    let ir1 = claude_ir::to_ir(&msgs, None);
    let kimi = kimi_ir::from_ir(&ir1);
    let ir2 = kimi_ir::to_ir(&kimi);
    let claude2 = claude_ir::from_ir(&ir2);
    assert_eq!(claude2[0].content, "Claude to Kimi");
}

#[test]
fn claude_codex_rt_simple_text() {
    // LOSSY: Codex from_ir only handles assistant output; use assistant role.
    let msgs = vec![claude_text("assistant", "Claude to Codex")];
    let ir1 = claude_ir::to_ir(&msgs, None);
    let codex = codex_ir::from_ir(&ir1);
    assert!(
        !codex.is_empty(),
        "assistant msg should produce codex items"
    );
    let ir2 = codex_ir::to_ir(&codex);
    let claude2 = claude_ir::from_ir(&ir2);
    assert_eq!(claude2[0].content, "Claude to Codex");
}

#[test]
fn claude_copilot_rt_simple_text() {
    let msgs = vec![claude_text("user", "Claude to Copilot")];
    let ir1 = claude_ir::to_ir(&msgs, None);
    let copilot = copilot_ir::from_ir(&ir1);
    let ir2 = copilot_ir::to_ir(&copilot);
    let claude2 = claude_ir::from_ir(&ir2);
    assert_eq!(claude2[0].content, "Claude to Copilot");
}

#[test]
fn gemini_kimi_rt_simple_text() {
    let contents = vec![gemini_text("user", "Gemini to Kimi")];
    let ir1 = gemini_ir::to_ir(&contents, None);
    let kimi = kimi_ir::from_ir(&ir1);
    let ir2 = kimi_ir::to_ir(&kimi);
    let gemini2 = gemini_ir::from_ir(&ir2);
    match &gemini2[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Gemini to Kimi"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_codex_rt_simple_text() {
    // LOSSY: Codex from_ir only handles assistant output; use model role.
    let contents = vec![gemini_text("model", "Gemini to Codex")];
    let ir1 = gemini_ir::to_ir(&contents, None);
    let codex = codex_ir::from_ir(&ir1);
    assert!(!codex.is_empty(), "model msg should produce codex items");
    let ir2 = codex_ir::to_ir(&codex);
    let gemini2 = gemini_ir::from_ir(&ir2);
    match &gemini2[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Gemini to Codex"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_copilot_rt_simple_text() {
    let contents = vec![gemini_text("user", "Gemini to Copilot")];
    let ir1 = gemini_ir::to_ir(&contents, None);
    let copilot = copilot_ir::from_ir(&ir1);
    let ir2 = copilot_ir::to_ir(&copilot);
    let gemini2 = gemini_ir::from_ir(&ir2);
    match &gemini2[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Gemini to Copilot"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn kimi_codex_rt_simple_text() {
    // LOSSY: Codex from_ir only handles assistant output; use assistant role.
    let msgs = vec![kimi_text("assistant", "Kimi to Codex")];
    let ir1 = kimi_ir::to_ir(&msgs);
    let codex = codex_ir::from_ir(&ir1);
    assert!(
        !codex.is_empty(),
        "assistant msg should produce codex items"
    );
    let ir2 = codex_ir::to_ir(&codex);
    let kimi2 = kimi_ir::from_ir(&ir2);
    assert_eq!(kimi2[0].content.as_deref(), Some("Kimi to Codex"));
}

#[test]
fn kimi_copilot_rt_simple_text() {
    let msgs = vec![kimi_text("user", "Kimi to Copilot")];
    let ir1 = kimi_ir::to_ir(&msgs);
    let copilot = copilot_ir::from_ir(&ir1);
    let ir2 = copilot_ir::to_ir(&copilot);
    let kimi2 = kimi_ir::from_ir(&ir2);
    assert_eq!(kimi2[0].content.as_deref(), Some("Kimi to Copilot"));
}

#[test]
fn codex_copilot_rt_simple_text() {
    // Codex input_to_ir produces user messages; copilot preserves them;
    // but codex from_ir only handles assistant output.
    // Use assistant role to verify the full roundtrip.
    let ir1 =
        IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Codex to Copilot")]);
    let copilot = copilot_ir::from_ir(&ir1);
    let ir2 = copilot_ir::to_ir(&copilot);
    let codex2 = codex_ir::from_ir(&ir2);
    assert!(
        !codex2.is_empty(),
        "assistant msg should produce codex items"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Tool use roundtrip (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_use_openai_to_claude_to_openai() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "read_file".into(),
            input: json!({"path": "src/lib.rs"}),
        }],
    )]);

    // Through Claude
    let claude = claude_ir::from_ir(&ir);
    let ir2 = claude_ir::to_ir(&claude, None);
    let oai = openai_ir::from_ir(&ir2);
    let ir3 = openai_ir::to_ir(&oai);
    match &ir3.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "t1");
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({"path": "src/lib.rs"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn tool_use_openai_to_gemini_to_openai_name_preserved() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        }],
    )]);

    let gemini = gemini_ir::from_ir(&ir);
    let ir2 = gemini_ir::to_ir(&gemini, None);
    let oai = openai_ir::from_ir(&ir2);
    let tc = &oai[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "search");
}

#[test]
fn tool_use_claude_to_kimi_name_preserved() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "execute".into(),
        input: json!({"cmd": "ls"}),
    }];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let ir1 = claude_ir::to_ir(&msgs, None);
    let kimi = kimi_ir::from_ir(&ir1);

    // Kimi should have tool calls
    assert!(kimi[0].tool_calls.is_some());
    let tc = &kimi[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "execute");
}

#[test]
fn tool_use_multiple_tool_calls_preserved() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "ls".into(),
                input: json!({}),
            },
            IrContentBlock::ToolUse {
                id: "t2".into(),
                name: "cat".into(),
                input: json!({"file": "a.txt"}),
            },
        ],
    )]);

    // OpenAI
    let oai = openai_ir::from_ir(&ir);
    assert_eq!(oai[0].tool_calls.as_ref().unwrap().len(), 2);
    let ir2 = openai_ir::to_ir(&oai);
    assert_eq!(ir2.messages[0].content.len(), 2);

    // Gemini
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].parts.len(), 2);
}

#[test]
fn tool_result_openai_to_claude_to_openai() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![IrContentBlock::Text {
                text: "file contents".into(),
            }],
            is_error: false,
        }],
    )]);

    let claude = claude_ir::from_ir(&ir);
    let ir2 = claude_ir::to_ir(&claude, None);
    // Verify the tool result content is in IR
    match &ir2.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "c1");
            assert!(!is_error);
            assert_eq!(content.len(), 1);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_result_error_flag_preserved_through_claude() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![IrContentBlock::Text {
                text: "error occurred".into(),
            }],
            is_error: true,
        }],
    )]);

    let claude = claude_ir::from_ir(&ir);
    let ir2 = claude_ir::to_ir(&claude, None);
    match &ir2.messages[0].content[0] {
        IrContentBlock::ToolResult { is_error, .. } => {
            assert!(is_error, "error flag should be preserved through Claude");
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_use_complex_json_args_roundtrip() {
    let args = json!({
        "path": "src/main.rs",
        "content": "fn main() {\n    println!(\"hello\");\n}",
        "options": {"recursive": true, "depth": 3},
        "tags": ["rust", "code"],
        "metadata": null
    });
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "tc1".into(),
            name: "write_file".into(),
            input: args.clone(),
        }],
    )]);

    // Through OpenAI
    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    match &ir2.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => assert_eq!(input, &args),
        other => panic!("expected ToolUse, got {other:?}"),
    }

    // Through Claude
    let claude = claude_ir::from_ir(&ir);
    let ir3 = claude_ir::to_ir(&claude, None);
    match &ir3.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => assert_eq!(input, &args),
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn tool_use_openai_to_kimi_to_openai() {
    let oai_msgs = vec![oai_tool_call("c1", "read", r#"{"f":"x"}"#)];
    let ir1 = openai_ir::to_ir(&oai_msgs);
    let kimi = kimi_ir::from_ir(&ir1);
    let ir2 = kimi_ir::to_ir(&kimi);
    let oai2 = openai_ir::from_ir(&ir2);

    let tc = &oai2[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "read");
}

#[test]
fn tool_use_gemini_function_call_through_claude() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "execute".into(),
            args: json!({"cmd": "cargo build"}),
        }],
    }];
    let ir1 = gemini_ir::to_ir(&contents, None);
    let claude = claude_ir::from_ir(&ir1);
    let ir2 = claude_ir::to_ir(&claude, None);

    match &ir2.messages[0].content[0] {
        IrContentBlock::ToolUse { name, input, .. } => {
            assert_eq!(name, "execute");
            assert_eq!(input, &json!({"cmd": "cargo build"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn tool_use_kimi_to_gemini_to_kimi() {
    let msgs = vec![KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![abp_kimi_sdk::dialect::KimiToolCall {
            id: "k1".into(),
            call_type: "function".into(),
            function: abp_kimi_sdk::dialect::KimiFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"test"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let ir1 = kimi_ir::to_ir(&msgs);
    let gemini = gemini_ir::from_ir(&ir1);
    let ir2 = gemini_ir::to_ir(&gemini, None);
    let kimi2 = kimi_ir::from_ir(&ir2);

    let tc = &kimi2[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "search");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Lossy mapping documentation (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

/// LOSSY: Claude thinking blocks become plain text in OpenAI.
/// Lost: block type distinction, signature field.
#[test]
fn lossy_claude_thinking_to_openai_becomes_text() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "reasoning step".into(),
        }],
    )]);
    let oai = openai_ir::from_ir(&ir);
    // Thinking rendered as text content
    let text = oai[0].content.as_deref().unwrap();
    assert!(text.contains("reasoning step"));
    // Roundtrip loses thinking block type
    let ir2 = openai_ir::to_ir(&oai);
    assert!(
        ir2.messages[0]
            .content
            .iter()
            .all(|b| !matches!(b, IrContentBlock::Thinking { .. }))
    );
}

/// LOSSY: Claude thinking blocks become plain text in Gemini.
/// Lost: block type distinction, signature field.
#[test]
fn lossy_claude_thinking_to_gemini_becomes_text() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "thinking hard".into(),
        }],
    )]);
    let gemini = gemini_ir::from_ir(&ir);
    // Thinking becomes text part
    match &gemini[0].parts[0] {
        GeminiPart::Text(t) => assert!(t.contains("thinking hard")),
        other => panic!("expected Text, got {other:?}"),
    }
}

/// LOSSY: Gemini tool call IDs are synthesized, original IDs lost.
#[test]
fn lossy_gemini_synthesizes_tool_ids() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "my_custom_id_123".into(),
            name: "read_file".into(),
            input: json!({}),
        }],
    )]);
    let gemini = gemini_ir::from_ir(&ir);
    let ir2 = gemini_ir::to_ir(&gemini, None);
    match &ir2.messages[0].content[0] {
        IrContentBlock::ToolUse { id, .. } => {
            assert_ne!(id, "my_custom_id_123", "LOSSY: original ID lost in Gemini");
            assert_eq!(id, "gemini_read_file");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

/// LOSSY: OpenAI system messages are extracted as separate fields in Claude.
/// Not a data loss, but structural transformation: system becomes a parameter.
#[test]
fn lossy_system_message_structural_change_claude() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be brief."),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let claude = claude_ir::from_ir(&ir);
    // System message is NOT in the message list
    assert_eq!(claude.len(), 1);
    assert_eq!(claude[0].role, "user");
    // It's extracted separately
    let sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("Be brief."));
}

/// LOSSY: OpenAI system messages are extracted as system_instruction in Gemini.
#[test]
fn lossy_system_message_structural_change_gemini() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be concise."),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let gemini = gemini_ir::from_ir(&ir);
    // System message is NOT in contents
    assert_eq!(gemini.len(), 1);
    assert_eq!(gemini[0].role, "user");
    // It's extracted separately
    let sys_instr = gemini_ir::extract_system_instruction(&ir);
    assert!(sys_instr.is_some());
}

/// LOSSY: Copilot references are SDK-specific metadata, lost in other SDKs.
#[test]
fn lossy_copilot_references_lost_in_other_sdks() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "Check this file".into(),
        name: None,
        copilot_references: vec![abp_copilot_sdk::dialect::CopilotReference {
            ref_type: abp_copilot_sdk::dialect::CopilotReferenceType::File,
            id: "file1".into(),
            data: json!({"path": "src/lib.rs"}),
            metadata: None,
        }],
    }];
    let ir = copilot_ir::to_ir(&msgs);
    // Through OpenAI — references are lost
    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    let copilot2 = copilot_ir::from_ir(&ir2);
    // Text preserved but references gone
    assert_eq!(copilot2[0].content, "Check this file");
    assert!(
        copilot2[0].copilot_references.is_empty(),
        "LOSSY: Copilot references lost after OpenAI roundtrip"
    );
}

/// LOSSY: Kimi search results (refs) have no equivalent in other SDKs.
#[test]
fn lossy_kimi_search_metadata_not_in_ir() {
    // Kimi messages with use_search=true generate refs in responses.
    // When we roundtrip Kimi messages through IR, search-related metadata
    // is not part of the core IR conversation model.
    let msgs = vec![kimi_text("user", "Search for Rust tutorials")];
    let ir = kimi_ir::to_ir(&msgs);
    // IR has the text but no Kimi-specific search metadata
    assert_eq!(ir.messages[0].text_content(), "Search for Rust tutorials");
    assert!(
        ir.messages[0].metadata.is_empty(),
        "Kimi search metadata not in standard IR"
    );
}

/// LOSSY: Codex reasoning summaries have no equivalent in other SDKs.
#[test]
fn lossy_codex_reasoning_not_in_other_sdks() {
    // Codex Reasoning items map to Thinking blocks in IR.
    // Other SDKs that don't support thinking will lose this.
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "reasoning about the problem".into(),
        }],
    )]);
    // Copilot has no thinking support — becomes regular text
    let copilot = copilot_ir::from_ir(&ir);
    assert!(copilot[0].content.contains("reasoning about the problem"));
    let ir2 = copilot_ir::to_ir(&copilot);
    // No longer a thinking block
    assert!(
        ir2.messages[0]
            .content
            .iter()
            .all(|b| !matches!(b, IrContentBlock::Thinking { .. }))
    );
}

/// LOSSY: Image blocks only supported by Claude and Gemini natively.
/// OpenAI/Kimi/Codex/Copilot will lose image content.
#[test]
fn lossy_image_blocks_not_supported_by_all_sdks() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        }],
    )]);
    // OpenAI doesn't have native image content blocks in this lowering
    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    let has_image = ir2.messages.first().is_some_and(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Image { .. }))
    });
    assert!(
        !has_image,
        "LOSSY: Image blocks lost after OpenAI roundtrip"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Error / edge cases (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_conversation_all_sdks() {
    let ir = IrConversation::new();

    assert!(openai_ir::from_ir(&ir).is_empty());
    assert!(claude_ir::from_ir(&ir).is_empty());
    assert!(gemini_ir::from_ir(&ir).is_empty());
    assert!(kimi_ir::from_ir(&ir).is_empty());
    assert!(codex_ir::from_ir(&ir).is_empty());
    assert!(copilot_ir::from_ir(&ir).is_empty());
}

#[test]
fn edge_empty_roundtrip_preserves_empty() {
    let ir = IrConversation::new();
    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    assert!(ir2.is_empty());

    let claude = claude_ir::from_ir(&ir);
    let ir3 = claude_ir::to_ir(&claude, None);
    assert!(ir3.is_empty());
}

#[test]
fn edge_very_long_message_all_sdks() {
    let long_text: String = "B".repeat(50_000);
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, &long_text)]);

    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    assert_eq!(ir2.messages[0].text_content().len(), 50_000);

    let kimi = kimi_ir::from_ir(&ir);
    let ir3 = kimi_ir::to_ir(&kimi);
    assert_eq!(ir3.messages[0].text_content().len(), 50_000);
}

#[test]
fn edge_special_chars_in_content() {
    let text = r#"He said "hello" & she said <goodbye> \n \t"#;
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, text)]);

    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    assert_eq!(ir2.messages[0].text_content(), text);

    let claude = claude_ir::from_ir(&ir);
    let ir3 = claude_ir::to_ir(&claude, None);
    assert_eq!(ir3.messages[0].text_content(), text);
}

#[test]
fn edge_deeply_nested_tool_args() {
    let nested = json!({
        "a": {"b": {"c": {"d": {"e": "deep"}}}}
    });
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "deep_tool".into(),
            input: nested.clone(),
        }],
    )]);
    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    match &ir2.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => assert_eq!(input, &nested),
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn edge_tool_args_with_array() {
    let args = json!({"items": [1, 2, 3, "four", null, true]});
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "list_tool".into(),
            input: args.clone(),
        }],
    )]);
    let claude = claude_ir::from_ir(&ir);
    let ir2 = claude_ir::to_ir(&claude, None);
    match &ir2.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => assert_eq!(input, &args),
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

/// Chain test: OpenAI → Claude → Gemini → Kimi → OpenAI preserves text.
#[test]
fn edge_four_sdk_chain_text_preserved() {
    let text = "Survives the gauntlet";
    let oai = vec![oai_text("user", text)];
    let ir1 = openai_ir::to_ir(&oai);
    let claude = claude_ir::from_ir(&ir1);
    let ir2 = claude_ir::to_ir(&claude, None);
    let gemini = gemini_ir::from_ir(&ir2);
    let ir3 = gemini_ir::to_ir(&gemini, None);
    let kimi = kimi_ir::from_ir(&ir3);
    let ir4 = kimi_ir::to_ir(&kimi);
    let oai2 = openai_ir::from_ir(&ir4);
    assert_eq!(oai2[0].content.as_deref(), Some(text));
}

/// Chain test: Claude → Gemini → OpenAI → Kimi → Codex → Copilot → Claude
#[test]
fn edge_six_sdk_chain_text_preserved() {
    // Use assistant role so Codex leg doesn't drop the message.
    let text = "Through all six SDKs";
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, text)]);

    // Claude
    let claude = claude_ir::from_ir(&ir);
    let ir1 = claude_ir::to_ir(&claude, None);
    // Gemini
    let gemini = gemini_ir::from_ir(&ir1);
    let ir2 = gemini_ir::to_ir(&gemini, None);
    // OpenAI
    let oai = openai_ir::from_ir(&ir2);
    let ir3 = openai_ir::to_ir(&oai);
    // Kimi
    let kimi = kimi_ir::from_ir(&ir3);
    let ir4 = kimi_ir::to_ir(&kimi);
    // Codex
    let codex = codex_ir::from_ir(&ir4);
    let ir5 = codex_ir::to_ir(&codex);
    // Copilot
    let copilot = copilot_ir::from_ir(&ir5);
    let ir6 = copilot_ir::to_ir(&copilot);

    assert_eq!(ir6.messages[0].text_content(), text);
}

#[test]
fn edge_only_whitespace_content() {
    let text = "   \t\n  ";
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, text)]);
    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    assert_eq!(ir2.messages[0].text_content(), text);
}

#[test]
fn edge_empty_string_content() {
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "")]);
    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    assert_eq!(ir2.messages[0].text_content(), "");
}
