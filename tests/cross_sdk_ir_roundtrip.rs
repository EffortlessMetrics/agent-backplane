// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-SDK IR roundtrip integration tests.
//!
//! Verifies that messages can be converted between OpenAI, Claude, and Gemini
//! dialect formats via the vendor-neutral IR layer without losing semantic
//! meaning.

use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
use abp_claude_sdk::lowering as claude_ir;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
use abp_gemini_sdk::lowering as gemini_ir;
use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
use abp_openai_sdk::lowering as openai_ir;
use serde_json::json;

// =========================================================================
// Helpers
// =========================================================================

fn openai_text(role: &str, text: &str) -> OpenAIMessage {
    OpenAIMessage {
        role: role.into(),
        content: Some(text.into()),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn openai_tool_call(id: &str, name: &str, args: &str) -> OpenAIMessage {
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

fn openai_tool_result(call_id: &str, text: &str) -> OpenAIMessage {
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

// =========================================================================
// 1. OpenAI → IR → Claude (5+ tests)
// =========================================================================

#[test]
fn openai_to_claude_simple_text() {
    let oai = vec![openai_text("user", "Hello world")];
    let ir = openai_ir::to_ir(&oai);
    let claude = claude_ir::from_ir(&ir);

    assert_eq!(claude.len(), 1);
    assert_eq!(claude[0].role, "user");
    assert_eq!(claude[0].content, "Hello world");
}

#[test]
fn openai_to_claude_system_message() {
    let oai = vec![
        openai_text("system", "You are helpful."),
        openai_text("user", "Hi"),
    ];
    let ir = openai_ir::to_ir(&oai);

    // Claude skips system messages in from_ir; extract separately
    let sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("You are helpful."));

    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude.len(), 1);
    assert_eq!(claude[0].role, "user");
    assert_eq!(claude[0].content, "Hi");
}

#[test]
fn openai_to_claude_tool_calls() {
    let oai = vec![openai_tool_call(
        "call_1",
        "read_file",
        r#"{"path":"main.rs"}"#,
    )];
    let ir = openai_ir::to_ir(&oai);
    let claude = claude_ir::from_ir(&ir);

    assert_eq!(claude.len(), 1);
    assert_eq!(claude[0].role, "assistant");
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude[0].content).unwrap();
    match &blocks[0] {
        ClaudeContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({"path": "main.rs"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn openai_to_claude_multi_turn() {
    let oai = vec![
        openai_text("system", "Be concise."),
        openai_text("user", "What is Rust?"),
        openai_text("assistant", "A systems programming language."),
        openai_text("user", "Thanks!"),
    ];
    let ir = openai_ir::to_ir(&oai);
    let claude = claude_ir::from_ir(&ir);

    // System is skipped → 3 messages
    assert_eq!(claude.len(), 3);
    assert_eq!(claude[0].role, "user");
    assert_eq!(claude[0].content, "What is Rust?");
    assert_eq!(claude[1].role, "assistant");
    assert_eq!(claude[1].content, "A systems programming language.");
    assert_eq!(claude[2].role, "user");
    assert_eq!(claude[2].content, "Thanks!");
}

#[test]
fn openai_to_claude_tool_result_mapping() {
    let oai = vec![
        openai_text("user", "Read the file"),
        openai_tool_call("c1", "read_file", r#"{"path":"a.rs"}"#),
        openai_tool_result("c1", "fn main() {}"),
        openai_text("assistant", "Done."),
    ];
    let ir = openai_ir::to_ir(&oai);
    let claude = claude_ir::from_ir(&ir);

    assert_eq!(claude.len(), 4);
    // Tool message becomes "user" in Claude
    assert_eq!(claude[2].role, "user");
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude[2].content).unwrap();
    match &blocks[0] {
        ClaudeContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => {
            assert_eq!(tool_use_id, "c1");
            assert_eq!(content.as_deref(), Some("fn main() {}"));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn openai_to_claude_missing_features_graceful() {
    // OpenAI message with None content and no tool calls → empty content
    let oai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&oai);
    let claude = claude_ir::from_ir(&ir);

    assert_eq!(claude.len(), 1);
    assert_eq!(claude[0].role, "assistant");
    // Empty content blocks → empty text_content
    assert_eq!(claude[0].content, "");
}

// =========================================================================
// 2. Claude → IR → OpenAI (5+ tests)
// =========================================================================

#[test]
fn claude_to_openai_thinking_blocks_become_text() {
    let blocks = vec![
        ClaudeContentBlock::Thinking {
            thinking: "Let me think...".into(),
            signature: Some("sig".into()),
        },
        ClaudeContentBlock::Text {
            text: "Answer.".into(),
        },
    ];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let ir = claude_ir::to_ir(&msgs, None);
    let oai = openai_ir::from_ir(&ir);

    assert_eq!(oai.len(), 1);
    assert_eq!(oai[0].role, "assistant");
    // Thinking text is concatenated with the answer text in OpenAI
    let text = oai[0].content.as_deref().unwrap();
    assert!(text.contains("Let me think..."));
    assert!(text.contains("Answer."));
}

#[test]
fn claude_to_openai_content_blocks_concatenated() {
    let blocks = vec![
        ClaudeContentBlock::Text {
            text: "First. ".into(),
        },
        ClaudeContentBlock::Text {
            text: "Second.".into(),
        },
    ];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let ir = claude_ir::to_ir(&msgs, None);
    let oai = openai_ir::from_ir(&ir);

    assert_eq!(oai[0].content.as_deref(), Some("First. Second."));
}

#[test]
fn claude_to_openai_tool_use_mapping() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "search".into(),
        input: json!({"q": "rust"}),
    }];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let ir = claude_ir::to_ir(&msgs, None);
    let oai = openai_ir::from_ir(&ir);

    assert_eq!(oai[0].role, "assistant");
    let tc = &oai[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "tu_1");
    assert_eq!(tc.function.name, "search");
    let args: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
    assert_eq!(args, json!({"q": "rust"}));
}

#[test]
fn claude_to_openai_tool_result_mapping() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("result data".into()),
        is_error: None,
    }];
    let msgs = vec![claude_blocks("user", &blocks)];
    let ir = claude_ir::to_ir(&msgs, None);

    // Claude puts tool results in "user" role messages, so the IR role is
    // User. The ToolResult content block is preserved but the OpenAI
    // from_ir only emits a tool-role message when the IR role is Tool.
    // Verify the IR captured the ToolResult block correctly.
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => {
            assert_eq!(tool_use_id, "tu_1");
            assert_eq!(content.len(), 1);
        }
        other => panic!("expected ToolResult in IR, got {other:?}"),
    }

    let oai = openai_ir::from_ir(&ir);
    // Claude "user" role maps to IR User → OpenAI "user"
    assert_eq!(oai[0].role, "user");
}

#[test]
fn claude_to_openai_system_as_separate_message() {
    let msgs = vec![claude_text("user", "Hello")];
    let ir = claude_ir::to_ir(&msgs, Some("You are a coding assistant."));
    let oai = openai_ir::from_ir(&ir);

    assert_eq!(oai.len(), 2);
    assert_eq!(oai[0].role, "system");
    assert_eq!(
        oai[0].content.as_deref(),
        Some("You are a coding assistant.")
    );
    assert_eq!(oai[1].role, "user");
    assert_eq!(oai[1].content.as_deref(), Some("Hello"));
}

// =========================================================================
// 3. Gemini → IR → OpenAI (5+ tests)
// =========================================================================

#[test]
fn gemini_to_openai_text_parts() {
    let contents = vec![gemini_text("user", "Hello from Gemini")];
    let ir = gemini_ir::to_ir(&contents, None);
    let oai = openai_ir::from_ir(&ir);

    assert_eq!(oai.len(), 1);
    assert_eq!(oai[0].role, "user");
    assert_eq!(oai[0].content.as_deref(), Some("Hello from Gemini"));
}

#[test]
fn gemini_to_openai_function_call_to_tool_call() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "search".into(),
            args: json!({"query": "rust"}),
        }],
    }];
    let ir = gemini_ir::to_ir(&contents, None);
    let oai = openai_ir::from_ir(&ir);

    assert_eq!(oai[0].role, "assistant");
    let tc = &oai[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "search");
    // ID is synthesized as "gemini_search"
    assert_eq!(tc.id, "gemini_search");
}

#[test]
fn gemini_to_openai_model_role_maps_to_assistant() {
    let contents = vec![gemini_text("model", "I am a model response.")];
    let ir = gemini_ir::to_ir(&contents, None);
    let oai = openai_ir::from_ir(&ir);

    assert_eq!(oai[0].role, "assistant");
    assert_eq!(oai[0].content.as_deref(), Some("I am a model response."));
}

#[test]
fn gemini_to_openai_function_response_to_tool_result() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "search".into(),
            response: json!("found results"),
        }],
    }];
    let ir = gemini_ir::to_ir(&contents, None);

    // Gemini function responses live in "user" role → IR role is User.
    // The ToolResult content block is preserved in IR.
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => {
            assert_eq!(tool_use_id, "gemini_search");
            assert_eq!(content.len(), 1);
        }
        other => panic!("expected ToolResult in IR, got {other:?}"),
    }

    let oai = openai_ir::from_ir(&ir);
    // Gemini "user" → IR User → OpenAI "user"
    assert_eq!(oai[0].role, "user");
}

#[test]
fn gemini_to_openai_system_instruction() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Be helpful".into())],
    };
    let contents = vec![gemini_text("user", "Hi")];
    let ir = gemini_ir::to_ir(&contents, Some(&sys));
    let oai = openai_ir::from_ir(&ir);

    assert_eq!(oai.len(), 2);
    assert_eq!(oai[0].role, "system");
    assert_eq!(oai[0].content.as_deref(), Some("Be helpful"));
    assert_eq!(oai[1].role, "user");
}

#[test]
fn gemini_to_openai_multi_part_content() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![
            GeminiPart::Text("Let me search.".into()),
            GeminiPart::FunctionCall {
                name: "grep".into(),
                args: json!({"pattern": "fn"}),
            },
        ],
    }];
    let ir = gemini_ir::to_ir(&contents, None);
    let oai = openai_ir::from_ir(&ir);

    assert_eq!(oai[0].role, "assistant");
    assert_eq!(oai[0].content.as_deref(), Some("Let me search."));
    assert!(oai[0].tool_calls.is_some());
    assert_eq!(oai[0].tool_calls.as_ref().unwrap()[0].function.name, "grep");
}

// =========================================================================
// 4. OpenAI → IR → Gemini (5+ tests)
// =========================================================================

#[test]
fn openai_to_gemini_role_mapping() {
    let oai = vec![
        openai_text("user", "Hi"),
        openai_text("assistant", "Hello!"),
    ];
    let ir = openai_ir::to_ir(&oai);
    let gemini = gemini_ir::from_ir(&ir);

    assert_eq!(gemini.len(), 2);
    assert_eq!(gemini[0].role, "user");
    assert_eq!(gemini[1].role, "model");
}

#[test]
fn openai_to_gemini_tool_calls_to_function_call() {
    let oai = vec![openai_tool_call("c1", "read_file", r#"{"path":"x.rs"}"#)];
    let ir = openai_ir::to_ir(&oai);
    let gemini = gemini_ir::from_ir(&ir);

    assert_eq!(gemini[0].role, "model");
    match &gemini[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "read_file");
            assert_eq!(args, &json!({"path": "x.rs"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn openai_to_gemini_content_preservation() {
    let oai = vec![openai_text("user", "What is 2+2?")];
    let ir = openai_ir::to_ir(&oai);
    let gemini = gemini_ir::from_ir(&ir);

    match &gemini[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "What is 2+2?"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn openai_to_gemini_tool_result_to_function_response() {
    let oai = vec![openai_tool_result("c1", "file contents here")];
    let ir = openai_ir::to_ir(&oai);
    let gemini = gemini_ir::from_ir(&ir);

    assert_eq!(gemini[0].role, "user");
    match &gemini[0].parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            // tool_use_id "c1" → strip "gemini_" prefix (which doesn't apply) → "c1"
            assert_eq!(name, "c1");
            assert_eq!(response, &json!("file contents here"));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn openai_to_gemini_system_skipped() {
    let oai = vec![
        openai_text("system", "Instructions here"),
        openai_text("user", "Hello"),
    ];
    let ir = openai_ir::to_ir(&oai);
    let gemini = gemini_ir::from_ir(&ir);

    // Gemini from_ir skips system messages
    assert_eq!(gemini.len(), 1);
    assert_eq!(gemini[0].role, "user");

    // But system instruction can be extracted
    let sys = gemini_ir::extract_system_instruction(&ir).unwrap();
    match &sys.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Instructions here"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn openai_to_gemini_multi_turn() {
    let oai = vec![
        openai_text("user", "First"),
        openai_text("assistant", "Second"),
        openai_text("user", "Third"),
    ];
    let ir = openai_ir::to_ir(&oai);
    let gemini = gemini_ir::from_ir(&ir);

    assert_eq!(gemini.len(), 3);
    assert_eq!(gemini[0].role, "user");
    assert_eq!(gemini[1].role, "model");
    assert_eq!(gemini[2].role, "user");
}

// =========================================================================
// 5. All-dialect roundtrip (5+ tests)
// =========================================================================

/// Sends a simple text message through all three dialects via IR.
#[test]
fn all_dialects_simple_text_roundtrip() {
    let text = "Hello from the universal IR layer!";

    // Start from IR
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, text)]);

    // IR → OpenAI → IR
    let oai = openai_ir::from_ir(&ir);
    let ir_from_oai = openai_ir::to_ir(&oai);
    assert_eq!(ir_from_oai.messages[0].text_content(), text);

    // IR → Claude → IR
    let claude = claude_ir::from_ir(&ir);
    let ir_from_claude = claude_ir::to_ir(&claude, None);
    assert_eq!(ir_from_claude.messages[0].text_content(), text);

    // IR → Gemini → IR
    let gemini = gemini_ir::from_ir(&ir);
    let ir_from_gemini = gemini_ir::to_ir(&gemini, None);
    assert_eq!(ir_from_gemini.messages[0].text_content(), text);
}

#[test]
fn all_dialects_assistant_text_roundtrip() {
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
}

#[test]
fn all_dialects_tool_use_content_fidelity() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "tool_42".into(),
            name: "read_file".into(),
            input: json!({"path": "src/lib.rs"}),
        }],
    )]);

    // OpenAI roundtrip
    let oai = openai_ir::from_ir(&ir);
    let ir_oai = openai_ir::to_ir(&oai);
    match &ir_oai.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "tool_42");
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({"path": "src/lib.rs"}));
        }
        other => panic!("expected ToolUse from OpenAI roundtrip, got {other:?}"),
    }

    // Claude roundtrip
    let claude = claude_ir::from_ir(&ir);
    let ir_claude = claude_ir::to_ir(&claude, None);
    match &ir_claude.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "tool_42");
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({"path": "src/lib.rs"}));
        }
        other => panic!("expected ToolUse from Claude roundtrip, got {other:?}"),
    }

    // Gemini roundtrip — note: Gemini doesn't preserve tool IDs (strips prefix)
    let gemini = gemini_ir::from_ir(&ir);
    let ir_gemini = gemini_ir::to_ir(&gemini, None);
    match &ir_gemini.messages[0].content[0] {
        IrContentBlock::ToolUse { name, input, .. } => {
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({"path": "src/lib.rs"}));
        }
        other => panic!("expected ToolUse from Gemini roundtrip, got {other:?}"),
    }
}

#[test]
fn all_dialects_system_message_preservation() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be helpful and concise."),
        IrMessage::text(IrRole::User, "Hi"),
    ]);

    // OpenAI preserves system as a message
    let oai = openai_ir::from_ir(&ir);
    assert_eq!(oai.len(), 2);
    assert_eq!(oai[0].role, "system");

    // Claude: system is extracted, not in messages
    let sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("Be helpful and concise."));
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude.len(), 1); // system skipped

    // Gemini: system extracted, not in contents
    let sys_g = gemini_ir::extract_system_instruction(&ir);
    assert!(sys_g.is_some());
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini.len(), 1); // system skipped
}

#[test]
fn all_dialects_multi_turn_content_fidelity() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is Rust?"),
        IrMessage::text(IrRole::Assistant, "A systems language."),
        IrMessage::text(IrRole::User, "Tell me more."),
        IrMessage::text(IrRole::Assistant, "It has ownership and borrowing."),
    ]);

    // Through OpenAI
    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    for (orig, rt) in ir.messages.iter().zip(ir2.messages.iter()) {
        assert_eq!(orig.text_content(), rt.text_content());
        assert_eq!(orig.role, rt.role);
    }

    // Through Claude
    let claude = claude_ir::from_ir(&ir);
    let ir3 = claude_ir::to_ir(&claude, None);
    for (orig, rt) in ir.messages.iter().zip(ir3.messages.iter()) {
        assert_eq!(orig.text_content(), rt.text_content());
    }

    // Through Gemini
    let gemini = gemini_ir::from_ir(&ir);
    let ir4 = gemini_ir::to_ir(&gemini, None);
    for (orig, rt) in ir.messages.iter().zip(ir4.messages.iter()) {
        assert_eq!(orig.text_content(), rt.text_content());
    }
}

/// Cross-dialect chain: OpenAI → IR → Claude → IR → Gemini → IR, content preserved.
#[test]
fn chain_openai_claude_gemini() {
    let oai = vec![
        openai_text("user", "Write a poem about Rust."),
        openai_text("assistant", "Rust is safe and fast."),
    ];

    // OpenAI → IR
    let ir1 = openai_ir::to_ir(&oai);
    assert_eq!(ir1.messages[0].text_content(), "Write a poem about Rust.");

    // IR → Claude → IR
    let claude = claude_ir::from_ir(&ir1);
    let ir2 = claude_ir::to_ir(&claude, None);
    assert_eq!(ir2.messages[0].text_content(), "Write a poem about Rust.");
    assert_eq!(ir2.messages[1].text_content(), "Rust is safe and fast.");

    // IR → Gemini → IR
    let gemini = gemini_ir::from_ir(&ir2);
    let ir3 = gemini_ir::to_ir(&gemini, None);
    assert_eq!(ir3.messages[0].text_content(), "Write a poem about Rust.");
    assert_eq!(ir3.messages[1].text_content(), "Rust is safe and fast.");
}

// =========================================================================
// 6. Edge cases (5+ tests)
// =========================================================================

#[test]
fn edge_empty_conversation_all_dialects() {
    let ir = IrConversation::new();

    let oai = openai_ir::from_ir(&ir);
    assert!(oai.is_empty());
    let ir2 = openai_ir::to_ir(&oai);
    assert!(ir2.is_empty());

    let claude = claude_ir::from_ir(&ir);
    assert!(claude.is_empty());
    let ir3 = claude_ir::to_ir(&claude, None);
    assert!(ir3.is_empty());

    let gemini = gemini_ir::from_ir(&ir);
    assert!(gemini.is_empty());
    let ir4 = gemini_ir::to_ir(&gemini, None);
    assert!(ir4.is_empty());
}

#[test]
fn edge_unicode_content_roundtrip() {
    let text = "こんにちは世界 🌍 Ñoño café résumé 中文 العربية";
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, text)]);

    // Through OpenAI
    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    assert_eq!(ir2.messages[0].text_content(), text);

    // Through Claude
    let claude = claude_ir::from_ir(&ir);
    let ir3 = claude_ir::to_ir(&claude, None);
    assert_eq!(ir3.messages[0].text_content(), text);

    // Through Gemini
    let gemini = gemini_ir::from_ir(&ir);
    let ir4 = gemini_ir::to_ir(&gemini, None);
    assert_eq!(ir4.messages[0].text_content(), text);
}

#[test]
fn edge_very_long_message() {
    let long_text: String = "A".repeat(100_000);
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, &long_text)]);

    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    assert_eq!(ir2.messages[0].text_content().len(), 100_000);

    let claude = claude_ir::from_ir(&ir);
    let ir3 = claude_ir::to_ir(&claude, None);
    assert_eq!(ir3.messages[0].text_content().len(), 100_000);

    let gemini = gemini_ir::from_ir(&ir);
    let ir4 = gemini_ir::to_ir(&gemini, None);
    assert_eq!(ir4.messages[0].text_content().len(), 100_000);
}

#[test]
fn edge_message_with_only_tool_calls() {
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

    // OpenAI: should have no content, two tool_calls
    let oai = openai_ir::from_ir(&ir);
    assert!(oai[0].content.is_none());
    assert_eq!(oai[0].tool_calls.as_ref().unwrap().len(), 2);

    // Claude: should serialize as JSON blocks
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude[0].content).unwrap();
    assert_eq!(blocks.len(), 2);

    // Gemini: two FunctionCall parts
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].parts.len(), 2);
    assert!(matches!(
        gemini[0].parts[0],
        GeminiPart::FunctionCall { .. }
    ));
    assert!(matches!(
        gemini[0].parts[1],
        GeminiPart::FunctionCall { .. }
    ));
}

#[test]
fn edge_mixed_content_block_types() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me help.".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "test"}),
            },
            IrContentBlock::Thinking {
                text: "I should search first.".into(),
            },
        ],
    )]);

    // OpenAI: text + thinking concatenated, tool call preserved
    let oai = openai_ir::from_ir(&ir);
    let text = oai[0].content.as_deref().unwrap();
    assert!(text.contains("Let me help."));
    assert!(text.contains("I should search first."));
    assert!(oai[0].tool_calls.is_some());

    // Claude: structured blocks
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude[0].content).unwrap();
    assert_eq!(blocks.len(), 3);

    // Gemini: thinking becomes text part
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].parts.len(), 3);
}

#[test]
fn edge_special_characters_in_tool_args() {
    let args = json!({
        "path": "src/main.rs",
        "content": "fn main() {\n    println!(\"hello \\\"world\\\"\");\n}",
        "unicode": "🦀",
        "null_val": null,
        "nested": {"a": [1, 2, 3]}
    });

    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "tc1".into(),
            name: "write_file".into(),
            input: args.clone(),
        }],
    )]);

    // OpenAI roundtrip preserves complex args
    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    match &ir2.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &args);
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }

    // Claude roundtrip preserves complex args
    let claude = claude_ir::from_ir(&ir);
    let ir3 = claude_ir::to_ir(&claude, None);
    match &ir3.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &args);
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn edge_newlines_and_whitespace_preserved() {
    let text = "  line1\n\tline2\r\n  line3  ";
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, text)]);

    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    assert_eq!(ir2.messages[0].text_content(), text);

    let claude = claude_ir::from_ir(&ir);
    let ir3 = claude_ir::to_ir(&claude, None);
    assert_eq!(ir3.messages[0].text_content(), text);

    let gemini = gemini_ir::from_ir(&ir);
    let ir4 = gemini_ir::to_ir(&gemini, None);
    assert_eq!(ir4.messages[0].text_content(), text);
}

#[test]
fn edge_image_block_cross_dialect() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        }],
    )]);

    // Claude roundtrip
    let claude = claude_ir::from_ir(&ir);
    let ir2 = claude_ir::to_ir(&claude, None);
    match &ir2.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "iVBORw0KGgo=");
        }
        other => panic!("expected Image from Claude roundtrip, got {other:?}"),
    }

    // Gemini roundtrip
    let gemini = gemini_ir::from_ir(&ir);
    let ir3 = gemini_ir::to_ir(&gemini, None);
    match &ir3.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "iVBORw0KGgo=");
        }
        other => panic!("expected Image from Gemini roundtrip, got {other:?}"),
    }
}

#[test]
fn edge_empty_string_content() {
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "")]);

    let oai = openai_ir::from_ir(&ir);
    let claude = claude_ir::from_ir(&ir);
    let gemini = gemini_ir::from_ir(&ir);

    // Each format should handle empty string without panicking
    assert_eq!(oai.len(), 1);
    assert_eq!(claude.len(), 1);
    assert_eq!(gemini.len(), 1);
}

// =========================================================================
// Additional cross-dialect tests
// =========================================================================

#[test]
fn claude_to_gemini_tool_use_mapping() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_99".into(),
        name: "write_file".into(),
        input: json!({"path": "test.rs", "content": "// test"}),
    }];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let ir = claude_ir::to_ir(&msgs, None);
    let gemini = gemini_ir::from_ir(&ir);

    assert_eq!(gemini[0].role, "model");
    match &gemini[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "write_file");
            assert_eq!(args, &json!({"path": "test.rs", "content": "// test"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn gemini_to_claude_function_call_mapping() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "grep".into(),
            args: json!({"pattern": "TODO"}),
        }],
    }];
    let ir = gemini_ir::to_ir(&contents, None);
    let claude = claude_ir::from_ir(&ir);

    assert_eq!(claude[0].role, "assistant");
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude[0].content).unwrap();
    match &blocks[0] {
        ClaudeContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "gemini_grep");
            assert_eq!(name, "grep");
            assert_eq!(input, &json!({"pattern": "TODO"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn full_tool_cycle_openai_to_claude_to_gemini() {
    // OpenAI: user asks → assistant calls tool → tool returns result → assistant responds
    let oai = vec![
        openai_text("user", "Read main.rs"),
        openai_tool_call("c1", "read_file", r#"{"path":"main.rs"}"#),
        openai_tool_result("c1", "fn main() {}"),
        openai_text("assistant", "The file contains a main function."),
    ];

    // OpenAI → IR
    let ir = openai_ir::to_ir(&oai);
    assert_eq!(ir.len(), 4);

    // IR → Claude
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude.len(), 4);
    assert_eq!(claude[0].role, "user");
    assert_eq!(claude[3].role, "assistant");

    // Claude → IR → Gemini
    let ir2 = claude_ir::to_ir(&claude, None);
    let gemini = gemini_ir::from_ir(&ir2);
    assert_eq!(gemini.len(), 4);
    assert_eq!(gemini[0].role, "user");
    assert_eq!(gemini[1].role, "model"); // assistant with tool call
    // Tool result becomes user with FunctionResponse
    assert_eq!(gemini[2].role, "user");
    assert_eq!(gemini[3].role, "model");
}

#[test]
fn thinking_block_openai_to_gemini_via_ir() {
    // Create IR with thinking block
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "Let me reason about this...".into(),
            },
            IrContentBlock::Text {
                text: "Here is my answer.".into(),
            },
        ],
    )]);

    // IR → OpenAI: thinking becomes text
    let oai = openai_ir::from_ir(&ir);
    let text = oai[0].content.as_deref().unwrap();
    assert!(text.contains("Let me reason about this..."));
    assert!(text.contains("Here is my answer."));

    // IR → Gemini: thinking becomes text part
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].parts.len(), 2);
    match &gemini[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Let me reason about this..."),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn claude_thinking_to_openai_and_gemini() {
    let blocks = vec![
        ClaudeContentBlock::Thinking {
            thinking: "Step 1: analyze. Step 2: synthesize.".into(),
            signature: Some("sig_abc".into()),
        },
        ClaudeContentBlock::Text {
            text: "Based on my analysis...".into(),
        },
    ];
    let msgs = vec![claude_blocks("assistant", &blocks)];
    let ir = claude_ir::to_ir(&msgs, None);

    // To OpenAI: thinking text included
    let oai = openai_ir::from_ir(&ir);
    let text = oai[0].content.as_deref().unwrap();
    assert!(text.contains("Step 1: analyze"));
    assert!(text.contains("Based on my analysis"));

    // To Gemini: thinking becomes text part
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].parts.len(), 2);
}

#[test]
fn tool_error_flag_preserved_across_dialects() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: vec![IrContentBlock::Text {
                text: "permission denied".into(),
            }],
            is_error: true,
        }],
    )]);

    // Through Claude
    let claude = claude_ir::from_ir(&ir);
    let ir2 = claude_ir::to_ir(&claude, None);
    match &ir2.messages[0].content[0] {
        IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
        other => panic!("expected ToolResult, got {other:?}"),
    }

    // Through OpenAI (OpenAI doesn't have is_error, so it's lost)
    // But check it doesn't crash
    let oai = openai_ir::from_ir(&ir);
    assert_eq!(oai.len(), 1);
}
