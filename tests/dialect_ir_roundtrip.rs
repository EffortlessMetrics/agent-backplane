// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive dialect IR roundtrip tests.
//!
//! For each SDK dialect (Claude, OpenAI, Gemini, Codex, Kimi, Copilot):
//!   - Constructs a typical request in that dialect's native format
//!   - Converts to IR (to_ir)
//!   - Converts from IR to each other dialect (from_ir for target)
//!   - Verifies key fields survive: messages, system prompt, tools
//!   - Verifies lossy conversions produce appropriate degradation
//!
//! Also covers edge cases: empty conversations, system-only messages,
//! multi-turn conversations, tool use/function calling, image/multimodal
//! content, thinking blocks, and usage stat mapping.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use serde_json::json;

// Lowering modules.
use abp_claude_sdk::lowering as claude_ir;
use abp_codex_sdk::lowering as codex_ir;
use abp_copilot_sdk::lowering as copilot_ir;
use abp_gemini_sdk::lowering as gemini_ir;
use abp_kimi_sdk::lowering as kimi_ir;
use abp_openai_sdk::lowering as openai_ir;

// Dialect types.
use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeImageSource, ClaudeMessage};
use abp_codex_sdk::dialect::{
    CodexContentPart, CodexInputItem, CodexResponseItem, CodexUsage, ReasoningSummary,
};
use abp_copilot_sdk::dialect::CopilotMessage;
use abp_gemini_sdk::dialect::{GeminiContent, GeminiInlineData, GeminiPart};
use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall, KimiUsage};
use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn openai_msg(role: &str, content: &str) -> OpenAIMessage {
    OpenAIMessage {
        role: role.into(),
        content: Some(content.into()),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn kimi_msg(role: &str, content: &str) -> KimiMessage {
    KimiMessage {
        role: role.into(),
        content: Some(content.into()),
        tool_call_id: None,
        tool_calls: None,
    }
}

fn copilot_msg(role: &str, content: &str) -> CopilotMessage {
    CopilotMessage {
        role: role.into(),
        content: content.into(),
        name: None,
        copilot_references: vec![],
    }
}

fn claude_msg(role: &str, content: &str) -> ClaudeMessage {
    ClaudeMessage {
        role: role.into(),
        content: content.into(),
    }
}

fn gemini_text(role: &str, text: &str) -> GeminiContent {
    GeminiContent {
        role: role.into(),
        parts: vec![GeminiPart::Text(text.into())],
    }
}

/// Build a full OpenAI conversation with system + user + assistant + tool call + tool result.
fn openai_full_conversation() -> Vec<OpenAIMessage> {
    vec![
        openai_msg("system", "You are a coding assistant."),
        openai_msg("user", "Read main.rs"),
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Let me read that file.".into()),
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_abc".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"main.rs"}"#.into(),
                },
            }]),
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "tool".into(),
            content: Some("fn main() { println!(\"hello\"); }".into()),
            tool_calls: None,
            tool_call_id: Some("call_abc".into()),
        },
        openai_msg("assistant", "The file contains a hello world program."),
    ]
}

/// Build a Claude conversation with thinking + tool use blocks.
fn claude_full_conversation() -> (Option<&'static str>, Vec<ClaudeMessage>) {
    let system = Some("You are a helpful assistant.");
    let thinking_and_tool = vec![
        ClaudeContentBlock::Thinking {
            thinking: "I need to read the file first.".into(),
            signature: Some("sig_1".into()),
        },
        ClaudeContentBlock::Text {
            text: "Let me check.".into(),
        },
        ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read_file".into(),
            input: json!({"path": "lib.rs"}),
        },
    ];
    let tool_result = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("pub fn hello() {}".into()),
        is_error: None,
    }];
    let msgs = vec![
        claude_msg("user", "Show me lib.rs"),
        ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&thinking_and_tool).unwrap(),
        },
        ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&tool_result).unwrap(),
        },
        claude_msg("assistant", "The file defines a hello function."),
    ];
    (system, msgs)
}

/// Build a Gemini conversation with function call + response.
fn gemini_full_conversation() -> (Option<GeminiContent>, Vec<GeminiContent>) {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Be concise and helpful.".into())],
    };
    let contents = vec![
        gemini_text("user", "Search for Rust tutorials"),
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "web_search".into(),
                args: json!({"query": "Rust tutorials"}),
            }],
        },
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "web_search".into(),
                response: json!("Found 10 results for Rust tutorials"),
            }],
        },
        gemini_text("model", "Here are some Rust tutorials I found."),
    ];
    (Some(sys), contents)
}

// ═══════════════════════════════════════════════════════════════════════════
// §1  OpenAI native → IR → every other dialect
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_ir_to_claude_full_conversation() {
    let msgs = openai_full_conversation();
    let ir = openai_ir::to_ir(&msgs);

    assert_eq!(ir.len(), 5);
    assert_eq!(ir.messages[0].role, IrRole::System);

    // System extracted for Claude
    let sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("You are a coding assistant."));

    let claude_msgs = claude_ir::from_ir(&ir);
    // System is stripped; 4 messages remain
    assert_eq!(claude_msgs.len(), 4);
    assert_eq!(claude_msgs[0].role, "user");
    assert_eq!(claude_msgs[0].content, "Read main.rs");

    // Assistant with tool call becomes structured content blocks
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude_msgs[1].content).unwrap();
    assert!(
        blocks
            .iter()
            .any(|b| matches!(b, ClaudeContentBlock::ToolUse { name, .. } if name == "read_file"))
    );
    assert!(
        blocks.iter().any(
            |b| matches!(b, ClaudeContentBlock::Text { text } if text.contains("Let me read"))
        )
    );

    // Tool result becomes a user message with ToolResult blocks
    let result_blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude_msgs[2].content).unwrap();
    assert!(
        matches!(&result_blocks[0], ClaudeContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "call_abc")
    );

    // Final text
    assert_eq!(claude_msgs[3].role, "assistant");
    assert!(claude_msgs[3].content.contains("hello world program"));
}

#[test]
fn openai_to_ir_to_gemini_full_conversation() {
    let msgs = openai_full_conversation();
    let ir = openai_ir::to_ir(&msgs);

    let sys_instr = gemini_ir::extract_system_instruction(&ir);
    assert!(sys_instr.is_some());
    assert!(
        matches!(&sys_instr.unwrap().parts[0], GeminiPart::Text(t) if t.contains("coding assistant"))
    );

    let gemini = gemini_ir::from_ir(&ir);
    // System stripped → 4 messages
    assert_eq!(gemini.len(), 4);
    assert_eq!(gemini[0].role, "user");

    // Assistant with tool → FunctionCall part
    assert_eq!(gemini[1].role, "model");
    let has_fc = gemini[1]
        .parts
        .iter()
        .any(|p| matches!(p, GeminiPart::FunctionCall { name, .. } if name == "read_file"));
    assert!(has_fc);

    // Tool result → FunctionResponse part
    assert_eq!(gemini[2].role, "user");
    assert!(matches!(
        &gemini[2].parts[0],
        GeminiPart::FunctionResponse { .. }
    ));
}

#[test]
fn openai_to_ir_to_kimi_tool_calls_preserved() {
    let msgs = openai_full_conversation();
    let ir = openai_ir::to_ir(&msgs);
    let kimi_msgs = kimi_ir::from_ir(&ir);

    assert_eq!(kimi_msgs.len(), 5);
    assert_eq!(kimi_msgs[0].role, "system");
    assert_eq!(
        kimi_msgs[0].content.as_deref(),
        Some("You are a coding assistant.")
    );

    // Tool call
    let tc = &kimi_msgs[2].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "read_file");
    assert_eq!(tc.id, "call_abc");

    // Tool result
    assert_eq!(kimi_msgs[3].role, "tool");
    assert_eq!(kimi_msgs[3].tool_call_id.as_deref(), Some("call_abc"));
}

#[test]
fn openai_to_ir_to_copilot_text_preserved() {
    let msgs = openai_full_conversation();
    let ir = openai_ir::to_ir(&msgs);
    let copilot_msgs = copilot_ir::from_ir(&ir);

    assert_eq!(copilot_msgs.len(), 5);
    assert_eq!(copilot_msgs[0].role, "system");
    assert_eq!(copilot_msgs[0].content, "You are a coding assistant.");
    assert_eq!(copilot_msgs[1].role, "user");
    assert_eq!(copilot_msgs[1].content, "Read main.rs");
    // Copilot doesn't have tool role; tool result mapped to user
    assert_eq!(copilot_msgs[3].role, "user");
}

#[test]
fn openai_to_ir_to_codex_only_assistant_and_tool_survive() {
    let msgs = openai_full_conversation();
    let ir = openai_ir::to_ir(&msgs);
    let codex_items = codex_ir::from_ir(&ir);

    // Codex from_ir only emits assistant + tool items: system and user are dropped
    // Assistant text+tool → Message + FunctionCall, tool result → FunctionCallOutput, final assistant → Message
    assert!(codex_items.len() >= 3);

    // At least one FunctionCall
    assert!(codex_items.iter().any(|item| matches!(
        item,
        CodexResponseItem::FunctionCall { name, .. } if name == "read_file"
    )));

    // At least one FunctionCallOutput
    assert!(codex_items.iter().any(|item| matches!(
        item,
        CodexResponseItem::FunctionCallOutput { call_id, .. } if call_id == "call_abc"
    )));
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  Claude native → IR → every other dialect
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_to_ir_to_openai_thinking_becomes_text() {
    let (system, msgs) = claude_full_conversation();
    let ir = claude_ir::to_ir(&msgs, system);

    // System message present
    assert_eq!(ir.messages[0].role, IrRole::System);

    let openai_msgs = openai_ir::from_ir(&ir);
    assert_eq!(openai_msgs[0].role, "system");
    assert_eq!(
        openai_msgs[0].content.as_deref(),
        Some("You are a helpful assistant.")
    );

    // Assistant with Thinking+Text+ToolUse: thinking and text become text, tool stays as tool_call
    let asst = &openai_msgs[2];
    assert_eq!(asst.role, "assistant");
    let text = asst.content.as_deref().unwrap_or("");
    assert!(text.contains("I need to read the file first.")); // Thinking text
    assert!(text.contains("Let me check.")); // Regular text
    assert!(asst.tool_calls.is_some());
    assert_eq!(
        asst.tool_calls.as_ref().unwrap()[0].function.name,
        "read_file"
    );
}

#[test]
fn claude_to_ir_to_gemini_tool_use_becomes_function_call() {
    let (system, msgs) = claude_full_conversation();
    let ir = claude_ir::to_ir(&msgs, system);

    let sys_instr = gemini_ir::extract_system_instruction(&ir);
    assert!(sys_instr.is_some());

    let gemini = gemini_ir::from_ir(&ir);
    // System stripped → msgs remain

    // Find the assistant message with the tool use
    let model_msg = gemini.iter().find(|c| c.role == "model").unwrap();
    let has_fc = model_msg
        .parts
        .iter()
        .any(|p| matches!(p, GeminiPart::FunctionCall { name, .. } if name == "read_file"));
    assert!(has_fc);
}

#[test]
fn claude_to_ir_to_kimi_system_stays_as_message() {
    let (system, msgs) = claude_full_conversation();
    let ir = claude_ir::to_ir(&msgs, system);
    let kimi_msgs = kimi_ir::from_ir(&ir);

    assert_eq!(kimi_msgs[0].role, "system");
    assert_eq!(
        kimi_msgs[0].content.as_deref(),
        Some("You are a helpful assistant.")
    );
}

#[test]
fn claude_to_ir_to_codex_thinking_becomes_reasoning() {
    let (system, msgs) = claude_full_conversation();
    let ir = claude_ir::to_ir(&msgs, system);
    let codex_items = codex_ir::from_ir(&ir);

    // Should have at least one Reasoning item from the Thinking block
    assert!(codex_items.iter().any(|item| matches!(
        item,
        CodexResponseItem::Reasoning { summary } if summary.iter().any(|s| s.text.contains("read the file"))
    )));
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  Gemini native → IR → every other dialect
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_to_ir_to_openai_function_call_becomes_tool_call() {
    let (sys, contents) = gemini_full_conversation();
    let ir = gemini_ir::to_ir(&contents, sys.as_ref());

    let openai_msgs = openai_ir::from_ir(&ir);
    // System present
    assert_eq!(openai_msgs[0].role, "system");
    assert!(
        openai_msgs[0]
            .content
            .as_deref()
            .unwrap()
            .contains("concise")
    );

    // Function call → tool_call
    let asst = openai_msgs.iter().find(|m| m.tool_calls.is_some()).unwrap();
    assert_eq!(
        asst.tool_calls.as_ref().unwrap()[0].function.name,
        "web_search"
    );

    // Function response comes from a Gemini "user" role, which maps to IR User
    // (not Tool), so OpenAI won't emit it as a tool message — it becomes a user
    // message. The ToolResult block is not rendered as text by OpenAI from_ir.
    // This is a known lossy path: Gemini FunctionResponse → IR User+ToolResult
    // → OpenAI user (content dropped because ToolResult isn't Text/Thinking).
    let user_msgs: Vec<_> = openai_msgs.iter().filter(|m| m.role == "user").collect();
    assert!(!user_msgs.is_empty());
}

#[test]
fn gemini_to_ir_to_claude_system_extracted() {
    let (sys, contents) = gemini_full_conversation();
    let ir = gemini_ir::to_ir(&contents, sys.as_ref());

    let claude_sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(claude_sys.as_deref(), Some("Be concise and helpful."));

    let claude_msgs = claude_ir::from_ir(&ir);
    assert!(claude_msgs.iter().all(|m| m.role != "system"));
}

#[test]
fn gemini_to_ir_to_copilot_text_survives() {
    let (sys, contents) = gemini_full_conversation();
    let ir = gemini_ir::to_ir(&contents, sys.as_ref());
    let copilot_msgs = copilot_ir::from_ir(&ir);

    // System stays as system
    assert_eq!(copilot_msgs[0].role, "system");
    assert!(copilot_msgs[0].content.contains("concise"));

    // Final text message preserved
    let last = copilot_msgs.last().unwrap();
    assert!(last.content.contains("tutorials"));
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Codex native → IR → every other dialect
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_input_to_ir_to_openai() {
    let items = vec![
        CodexInputItem::Message {
            role: "system".into(),
            content: "You review code.".into(),
        },
        CodexInputItem::Message {
            role: "user".into(),
            content: "Check this PR".into(),
        },
    ];
    let ir = codex_ir::input_to_ir(&items);
    let openai_msgs = openai_ir::from_ir(&ir);

    assert_eq!(openai_msgs.len(), 2);
    assert_eq!(openai_msgs[0].role, "system");
    assert_eq!(openai_msgs[0].content.as_deref(), Some("You review code."));
    assert_eq!(openai_msgs[1].role, "user");
}

#[test]
fn codex_response_to_ir_to_claude() {
    let items = vec![
        CodexResponseItem::Reasoning {
            summary: vec![ReasoningSummary {
                text: "Analyzing the code structure.".into(),
            }],
        },
        CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "The code looks good.".into(),
            }],
        },
        CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "write_file".into(),
            arguments: r#"{"path":"fix.rs","content":"fixed"}"#.into(),
        },
        CodexResponseItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "ok".into(),
        },
    ];
    let ir = codex_ir::to_ir(&items);
    let claude_msgs = claude_ir::from_ir(&ir);

    // Reasoning → assistant with Thinking block
    let first_content: Result<Vec<ClaudeContentBlock>, _> =
        serde_json::from_str(&claude_msgs[0].content);
    if let Ok(blocks) = first_content {
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, ClaudeContentBlock::Thinking { .. }))
        );
    }

    // FunctionCall → assistant with ToolUse
    let fc_msg = claude_msgs.iter().find(|m| {
        serde_json::from_str::<Vec<ClaudeContentBlock>>(&m.content)
            .map(|blocks| {
                blocks
                    .iter()
                    .any(|b| matches!(b, ClaudeContentBlock::ToolUse { name, .. } if name == "write_file"))
            })
            .unwrap_or(false)
    });
    assert!(fc_msg.is_some());
}

#[test]
fn codex_response_to_ir_to_kimi_function_calls_mapped() {
    let items = vec![
        CodexResponseItem::FunctionCall {
            id: "fc_x".into(),
            call_id: None,
            name: "shell".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
        },
        CodexResponseItem::FunctionCallOutput {
            call_id: "fc_x".into(),
            output: "file1.rs\nfile2.rs".into(),
        },
    ];
    let ir = codex_ir::to_ir(&items);
    let kimi_msgs = kimi_ir::from_ir(&ir);

    // FunctionCall → assistant with tool_calls
    assert_eq!(kimi_msgs[0].role, "assistant");
    let tc = &kimi_msgs[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "shell");

    // FunctionCallOutput → tool with tool_call_id
    assert_eq!(kimi_msgs[1].role, "tool");
    assert_eq!(kimi_msgs[1].tool_call_id.as_deref(), Some("fc_x"));
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Kimi native → IR → every other dialect
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn kimi_to_ir_to_openai_full_conversation() {
    let msgs = vec![
        kimi_msg("system", "You are Kimi."),
        kimi_msg("user", "Search for Rust"),
        KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "kc_1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "web_search".into(),
                    arguments: r#"{"q":"Rust"}"#.into(),
                },
            }]),
        },
        KimiMessage {
            role: "tool".into(),
            content: Some("Rust is a systems language.".into()),
            tool_call_id: Some("kc_1".into()),
            tool_calls: None,
        },
        kimi_msg("assistant", "Here are the results."),
    ];

    let ir = kimi_ir::to_ir(&msgs);
    let openai_msgs = openai_ir::from_ir(&ir);

    assert_eq!(openai_msgs.len(), 5);
    assert_eq!(openai_msgs[0].role, "system");

    // Tool call
    let tc = &openai_msgs[2].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "web_search");

    // Tool result
    assert_eq!(openai_msgs[3].role, "tool");
    assert_eq!(openai_msgs[3].tool_call_id.as_deref(), Some("kc_1"));
}

#[test]
fn kimi_to_ir_to_gemini() {
    let msgs = vec![
        kimi_msg("system", "Instructions"),
        kimi_msg("user", "Hello"),
        kimi_msg("assistant", "Hi there!"),
    ];
    let ir = kimi_ir::to_ir(&msgs);

    let sys_instr = gemini_ir::extract_system_instruction(&ir);
    assert!(sys_instr.is_some());

    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini.len(), 2); // system stripped
    assert_eq!(gemini[0].role, "user");
    assert_eq!(gemini[1].role, "model");
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  Copilot native → IR → every other dialect
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_to_ir_to_openai() {
    let msgs = vec![
        copilot_msg("system", "You are GitHub Copilot."),
        copilot_msg("user", "Explain closures in Rust"),
        copilot_msg("assistant", "Closures are anonymous functions."),
    ];
    let ir = copilot_ir::to_ir(&msgs);
    let openai_msgs = openai_ir::from_ir(&ir);

    assert_eq!(openai_msgs.len(), 3);
    assert_eq!(openai_msgs[0].role, "system");
    assert_eq!(
        openai_msgs[0].content.as_deref(),
        Some("You are GitHub Copilot.")
    );
    assert_eq!(openai_msgs[2].role, "assistant");
    assert!(
        openai_msgs[2]
            .content
            .as_deref()
            .unwrap()
            .contains("anonymous functions")
    );
}

#[test]
fn copilot_to_ir_to_claude_system_extracted() {
    let msgs = vec![
        copilot_msg("system", "Copilot system prompt"),
        copilot_msg("user", "Help me"),
    ];
    let ir = copilot_ir::to_ir(&msgs);

    let sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("Copilot system prompt"));

    let claude_msgs = claude_ir::from_ir(&ir);
    assert_eq!(claude_msgs.len(), 1);
    assert_eq!(claude_msgs[0].role, "user");
    assert_eq!(claude_msgs[0].content, "Help me");
}

#[test]
fn copilot_to_ir_to_gemini() {
    let msgs = vec![
        copilot_msg("system", "Instructions"),
        copilot_msg("user", "What is Rust?"),
        copilot_msg("assistant", "Rust is a programming language."),
    ];
    let ir = copilot_ir::to_ir(&msgs);

    let sys_instr = gemini_ir::extract_system_instruction(&ir);
    assert!(sys_instr.is_some());

    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini.len(), 2); // system stripped
    assert_eq!(gemini[0].role, "user");
    assert_eq!(gemini[1].role, "model");
    assert!(
        matches!(&gemini[1].parts[0], GeminiPart::Text(t) if t.contains("programming language"))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Edge cases: empty conversation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_conversation_survives_all_dialects() {
    let empty = IrConversation::new();

    assert!(openai_ir::from_ir(&empty).is_empty());
    assert!(claude_ir::from_ir(&empty).is_empty());
    assert!(gemini_ir::from_ir(&empty).is_empty());
    assert!(codex_ir::from_ir(&empty).is_empty());
    assert!(kimi_ir::from_ir(&empty).is_empty());
    assert!(copilot_ir::from_ir(&empty).is_empty());

    // to_ir from empty native inputs
    assert!(openai_ir::to_ir(&[]).is_empty());
    assert!(claude_ir::to_ir(&[], None).is_empty());
    assert!(gemini_ir::to_ir(&[], None).is_empty());
    assert!(codex_ir::to_ir(&[]).is_empty());
    assert!(codex_ir::input_to_ir(&[]).is_empty());
    assert!(kimi_ir::to_ir(&[]).is_empty());
    assert!(copilot_ir::to_ir(&[]).is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  Edge cases: system-only messages
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn system_only_openai_to_all_dialects() {
    let msgs = vec![openai_msg("system", "Only system instructions.")];
    let ir = openai_ir::to_ir(&msgs);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::System);

    // Claude: system extracted, message list empty
    let sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("Only system instructions."));
    assert!(claude_ir::from_ir(&ir).is_empty());

    // Gemini: system extracted, content list empty
    let instr = gemini_ir::extract_system_instruction(&ir);
    assert!(instr.is_some());
    assert!(gemini_ir::from_ir(&ir).is_empty());

    // OpenAI roundtrip: system stays
    let oai = openai_ir::from_ir(&ir);
    assert_eq!(oai.len(), 1);
    assert_eq!(oai[0].role, "system");

    // Kimi: system stays
    let km = kimi_ir::from_ir(&ir);
    assert_eq!(km.len(), 1);
    assert_eq!(km[0].role, "system");

    // Copilot: system stays
    let cp = copilot_ir::from_ir(&ir);
    assert_eq!(cp.len(), 1);
    assert_eq!(cp[0].role, "system");

    // Codex: system dropped
    assert!(codex_ir::from_ir(&ir).is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// §9  Edge cases: multi-turn conversations
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ten_turn_conversation_preserves_order_through_all_dialects() {
    let turns: Vec<(&str, &str)> = vec![
        ("user", "Turn 1"),
        ("assistant", "Turn 2"),
        ("user", "Turn 3"),
        ("assistant", "Turn 4"),
        ("user", "Turn 5"),
        ("assistant", "Turn 6"),
        ("user", "Turn 7"),
        ("assistant", "Turn 8"),
        ("user", "Turn 9"),
        ("assistant", "Turn 10"),
    ];

    let ir = IrConversation::from_messages(
        turns
            .iter()
            .map(|(role, text)| {
                let r = match *role {
                    "assistant" => IrRole::Assistant,
                    _ => IrRole::User,
                };
                IrMessage::text(r, *text)
            })
            .collect(),
    );

    // OpenAI
    let oai = openai_ir::from_ir(&ir);
    assert_eq!(oai.len(), 10);
    for (i, msg) in oai.iter().enumerate() {
        assert_eq!(msg.content.as_deref(), Some(turns[i].1));
    }

    // Claude
    let cl = claude_ir::from_ir(&ir);
    assert_eq!(cl.len(), 10);
    for (i, msg) in cl.iter().enumerate() {
        assert_eq!(msg.content, turns[i].1);
    }

    // Kimi
    let km = kimi_ir::from_ir(&ir);
    assert_eq!(km.len(), 10);
    for (i, msg) in km.iter().enumerate() {
        assert_eq!(msg.content.as_deref(), Some(turns[i].1));
    }

    // Copilot
    let cp = copilot_ir::from_ir(&ir);
    assert_eq!(cp.len(), 10);
    for (i, msg) in cp.iter().enumerate() {
        assert_eq!(msg.content, turns[i].1);
    }

    // Gemini
    let gm = gemini_ir::from_ir(&ir);
    assert_eq!(gm.len(), 10);
    for (i, content) in gm.iter().enumerate() {
        assert!(matches!(&content.parts[0], GeminiPart::Text(t) if t == turns[i].1));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §10  Tool use / function calling conversion between dialects
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_call_openai_to_gemini_and_back() {
    let msgs = vec![
        openai_msg("user", "Search for docs"),
        OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_99".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"docs"}"#.into(),
                },
            }]),
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "tool".into(),
            content: Some("Found documentation.".into()),
            tool_calls: None,
            tool_call_id: Some("call_99".into()),
        },
    ];

    // OpenAI → IR → Gemini
    let ir = openai_ir::to_ir(&msgs);
    let gemini = gemini_ir::from_ir(&ir);

    // Verify FunctionCall
    let model_msg = gemini.iter().find(|c| c.role == "model").unwrap();
    assert!(
        model_msg
            .parts
            .iter()
            .any(|p| { matches!(p, GeminiPart::FunctionCall { name, .. } if name == "search") })
    );

    // Verify FunctionResponse
    let user_msg = gemini.iter().find(|c| {
        c.parts
            .iter()
            .any(|p| matches!(p, GeminiPart::FunctionResponse { .. }))
    });
    assert!(user_msg.is_some());

    // Gemini → IR → OpenAI (round trip)
    let ir2 = gemini_ir::to_ir(&gemini, None);
    let openai_back = openai_ir::from_ir(&ir2);

    // Tool call survives
    let asst_back = openai_back.iter().find(|m| m.tool_calls.is_some()).unwrap();
    assert_eq!(
        asst_back.tool_calls.as_ref().unwrap()[0].function.name,
        "search"
    );
}

#[test]
fn tool_call_claude_to_kimi_and_back() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_10".into(),
        name: "grep".into(),
        input: json!({"pattern": "fn main"}),
    }];
    let msgs = vec![
        claude_msg("user", "Find main function"),
        ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        },
    ];

    let ir = claude_ir::to_ir(&msgs, None);
    let kimi_msgs = kimi_ir::from_ir(&ir);

    // Tool call in Kimi
    let asst = kimi_msgs.iter().find(|m| m.tool_calls.is_some()).unwrap();
    let tc = &asst.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "grep");
    assert_eq!(tc.id, "tu_10");

    // Kimi → IR → Claude
    let ir2 = kimi_ir::to_ir(&kimi_msgs);
    let claude_back = claude_ir::from_ir(&ir2);
    let back_blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude_back.last().unwrap().content).unwrap();
    assert!(
        back_blocks
            .iter()
            .any(|b| matches!(b, ClaudeContentBlock::ToolUse { name, .. } if name == "grep"))
    );
}

#[test]
fn multiple_tool_calls_in_single_message_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![
            OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"a.rs"}"#.into(),
                },
            },
            OpenAIToolCall {
                id: "c2".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"b.rs"}"#.into(),
                },
            },
            OpenAIToolCall {
                id: "c3".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "write_file".into(),
                    arguments: r#"{"path":"c.rs","content":"// new"}"#.into(),
                },
            },
        ]),
        tool_call_id: None,
    }];

    let ir = openai_ir::to_ir(&msgs);
    assert_eq!(ir.messages[0].content.len(), 3);

    // All tool names survive through Kimi
    let kimi_msgs = kimi_ir::from_ir(&ir);
    let tcs = kimi_msgs[0].tool_calls.as_ref().unwrap();
    assert_eq!(tcs.len(), 3);
    assert_eq!(tcs[0].function.name, "read_file");
    assert_eq!(tcs[2].function.name, "write_file");

    // All tool names survive through Gemini
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].parts.len(), 3);
    for part in &gemini[0].parts {
        assert!(matches!(part, GeminiPart::FunctionCall { .. }));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §11  Image / multimodal content conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn image_ir_to_gemini_becomes_inline_data() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
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
    )]);

    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini.len(), 1);
    assert_eq!(gemini[0].parts.len(), 2);
    assert!(matches!(&gemini[0].parts[0], GeminiPart::Text(t) if t.contains("image")));
    match &gemini[0].parts[1] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/png");
            assert_eq!(d.data, "iVBORw0KGgo=");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn image_ir_to_claude_becomes_image_block() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/jpeg".into(),
            data: "base64jpeg".into(),
        }],
    )]);

    let claude_msgs = claude_ir::from_ir(&ir);
    assert_eq!(claude_msgs.len(), 1);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude_msgs[0].content).unwrap();
    assert!(matches!(
        &blocks[0],
        ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 { media_type, data }
        } if media_type == "image/jpeg" && data == "base64jpeg"
    ));
}

#[test]
fn image_ir_to_openai_is_silently_dropped() {
    // OpenAI lowering doesn't handle Image blocks; they are dropped
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "data".into(),
        }],
    )]);

    let openai_msgs = openai_ir::from_ir(&ir);
    assert_eq!(openai_msgs.len(), 1);
    // Image is not a Text or Thinking, so content will be None
    assert!(openai_msgs[0].content.is_none());
}

#[test]
fn image_ir_to_copilot_is_silently_dropped() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "data".into(),
        }],
    )]);

    let copilot_msgs = copilot_ir::from_ir(&ir);
    assert_eq!(copilot_msgs.len(), 1);
    assert!(copilot_msgs[0].content.is_empty());
}

#[test]
fn image_ir_to_kimi_is_silently_dropped() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "data".into(),
        }],
    )]);

    let kimi_msgs = kimi_ir::from_ir(&ir);
    assert_eq!(kimi_msgs.len(), 1);
    assert!(kimi_msgs[0].content.is_none());
}

#[test]
fn gemini_inline_data_roundtrip_through_ir() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![
            GeminiPart::Text("Describe this:".into()),
            GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/webp".into(),
                data: "webpdata123".into(),
            }),
        ],
    }];

    let ir = gemini_ir::to_ir(&contents, None);
    assert_eq!(ir.messages[0].content.len(), 2);
    assert!(
        matches!(&ir.messages[0].content[1], IrContentBlock::Image { media_type, data } if media_type == "image/webp" && data == "webpdata123")
    );

    // Roundtrip back to Gemini
    let back = gemini_ir::from_ir(&ir);
    match &back[0].parts[1] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/webp");
            assert_eq!(d.data, "webpdata123");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §12  Thinking / chain-of-thought block handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_block_survives_claude_to_claude() {
    let blocks = vec![
        ClaudeContentBlock::Thinking {
            thinking: "Let me reason step by step.".into(),
            signature: Some("sig_abc".into()),
        },
        ClaudeContentBlock::Text {
            text: "The answer is 42.".into(),
        },
    ];
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];

    let ir = claude_ir::to_ir(&msgs, None);
    assert!(
        matches!(&ir.messages[0].content[0], IrContentBlock::Thinking { text } if text == "Let me reason step by step.")
    );

    let back = claude_ir::from_ir(&ir);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    assert!(
        matches!(&parsed[0], ClaudeContentBlock::Thinking { thinking, .. } if thinking == "Let me reason step by step.")
    );
    assert!(matches!(&parsed[1], ClaudeContentBlock::Text { text } if text == "The answer is 42."));
}

#[test]
fn thinking_ir_to_codex_becomes_reasoning() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "Step 1: analyze. Step 2: implement.".into(),
            },
            IrContentBlock::Text {
                text: "Done.".into(),
            },
        ],
    )]);

    let codex_items = codex_ir::from_ir(&ir);
    assert!(codex_items.len() >= 2);
    assert!(matches!(
        &codex_items[0],
        CodexResponseItem::Reasoning { summary } if summary[0].text.contains("Step 1")
    ));
    assert!(matches!(
        &codex_items[1],
        CodexResponseItem::Message { content, .. } if !content.is_empty()
    ));
}

#[test]
fn thinking_ir_to_openai_becomes_text() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "Internal reasoning".into(),
        }],
    )]);

    let openai_msgs = openai_ir::from_ir(&ir);
    assert_eq!(openai_msgs.len(), 1);
    assert_eq!(
        openai_msgs[0].content.as_deref(),
        Some("Internal reasoning")
    );
}

#[test]
fn thinking_ir_to_gemini_becomes_text_part() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "Reasoning...".into(),
        }],
    )]);

    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini.len(), 1);
    assert!(matches!(&gemini[0].parts[0], GeminiPart::Text(t) if t == "Reasoning..."));
}

#[test]
fn thinking_ir_to_copilot_becomes_text() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "Deep thought".into(),
        }],
    )]);

    let copilot_msgs = copilot_ir::from_ir(&ir);
    assert_eq!(copilot_msgs.len(), 1);
    assert_eq!(copilot_msgs[0].content, "Deep thought");
}

// ═══════════════════════════════════════════════════════════════════════════
// §13  Usage stats mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_usage_to_ir_and_merge() {
    let u1 = codex_ir::usage_to_ir(&CodexUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
    });
    assert_eq!(u1.input_tokens, 100);
    assert_eq!(u1.output_tokens, 50);
    assert_eq!(u1.total_tokens, 150);
    assert_eq!(u1.cache_read_tokens, 0);
    assert_eq!(u1.cache_write_tokens, 0);

    let u2 = kimi_ir::usage_to_ir(&KimiUsage {
        prompt_tokens: 200,
        completion_tokens: 80,
        total_tokens: 280,
    });
    assert_eq!(u2.input_tokens, 200);
    assert_eq!(u2.output_tokens, 80);
    assert_eq!(u2.total_tokens, 280);

    let merged = u1.merge(u2);
    assert_eq!(merged.input_tokens, 300);
    assert_eq!(merged.output_tokens, 130);
    assert_eq!(merged.total_tokens, 430);
}

#[test]
fn default_usage_is_zero() {
    let usage = IrUsage::default();
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
    assert_eq!(usage.total_tokens, 0);
    assert_eq!(usage.cache_read_tokens, 0);
    assert_eq!(usage.cache_write_tokens, 0);
}

#[test]
fn usage_with_cache_fields() {
    let usage = IrUsage::with_cache(500, 200, 100, 50);
    assert_eq!(usage.input_tokens, 500);
    assert_eq!(usage.output_tokens, 200);
    assert_eq!(usage.total_tokens, 700);
    assert_eq!(usage.cache_read_tokens, 100);
    assert_eq!(usage.cache_write_tokens, 50);
}

// ═══════════════════════════════════════════════════════════════════════════
// §14  Lossy conversion paths
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_from_ir_drops_system_and_user() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful"),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there"),
        IrMessage::text(IrRole::User, "More questions"),
    ]);

    let codex_items = codex_ir::from_ir(&ir);
    // Only the assistant message survives
    assert_eq!(codex_items.len(), 1);
    assert!(matches!(
        &codex_items[0],
        CodexResponseItem::Message { role, content } if role == "assistant" && !content.is_empty()
    ));
}

#[test]
fn copilot_tool_role_mapped_to_user() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![IrContentBlock::Text {
                text: "result data".into(),
            }],
            is_error: false,
        }],
    )]);

    let copilot_msgs = copilot_ir::from_ir(&ir);
    assert_eq!(copilot_msgs.len(), 1);
    // Copilot has no tool role, mapped to "user"
    assert_eq!(copilot_msgs[0].role, "user");
}

#[test]
fn claude_from_ir_strips_system_messages() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "System instructions"),
        IrMessage::text(IrRole::System, "More system instructions"),
        IrMessage::text(IrRole::User, "Hello"),
    ]);

    let claude_msgs = claude_ir::from_ir(&ir);
    // Both system messages stripped
    assert_eq!(claude_msgs.len(), 1);
    assert_eq!(claude_msgs[0].role, "user");
    assert_eq!(claude_msgs[0].content, "Hello");
}

#[test]
fn gemini_from_ir_strips_system_messages() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "System instructions"),
        IrMessage::text(IrRole::User, "Hello"),
    ]);

    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini.len(), 1);
    assert_eq!(gemini[0].role, "user");
}

// ═══════════════════════════════════════════════════════════════════════════
// §15  Text fidelity: Unicode, emoji, special characters
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unicode_and_emoji_survive_full_roundtrip() {
    let text = "Hello 🦀 世界 مرحبا мир 🚀✨ <script>alert('xss')</script>";
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, text)]);

    // Through every dialect
    let oai = openai_ir::from_ir(&ir);
    assert_eq!(oai[0].content.as_deref(), Some(text));

    let cl = claude_ir::from_ir(&ir);
    assert_eq!(cl[0].content, text);

    let gm = gemini_ir::from_ir(&ir);
    assert!(matches!(&gm[0].parts[0], GeminiPart::Text(t) if t == text));

    let km = kimi_ir::from_ir(&ir);
    assert_eq!(km[0].content.as_deref(), Some(text));

    let cp = copilot_ir::from_ir(&ir);
    assert_eq!(cp[0].content, text);
}

#[test]
fn multiline_text_preserved() {
    let text = "Line 1\nLine 2\n\nLine 4\n\ttabbed";
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, text)]);

    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    assert_eq!(ir2.messages[0].text_content(), text);

    let cl = claude_ir::from_ir(&ir);
    let ir3 = claude_ir::to_ir(&cl, None);
    assert_eq!(ir3.messages[0].text_content(), text);
}

// ═══════════════════════════════════════════════════════════════════════════
// §16  Full chain: A → IR → B → IR → C
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn chain_openai_to_claude_to_gemini() {
    let openai_msgs = vec![
        openai_msg("system", "Be brief."),
        openai_msg("user", "Hello chain test"),
        openai_msg("assistant", "Chain reply"),
    ];

    // OpenAI → IR
    let ir1 = openai_ir::to_ir(&openai_msgs);

    // IR → Claude
    let sys = claude_ir::extract_system_prompt(&ir1);
    let claude_msgs = claude_ir::from_ir(&ir1);
    assert_eq!(sys.as_deref(), Some("Be brief."));
    assert_eq!(claude_msgs.len(), 2);

    // Claude → IR
    let ir2 = claude_ir::to_ir(&claude_msgs, sys.as_deref());
    assert_eq!(ir2.messages[0].text_content(), "Be brief.");
    assert_eq!(ir2.messages[1].text_content(), "Hello chain test");
    assert_eq!(ir2.messages[2].text_content(), "Chain reply");

    // IR → Gemini
    let gem_sys = gemini_ir::extract_system_instruction(&ir2);
    let gemini = gemini_ir::from_ir(&ir2);
    assert!(gem_sys.is_some());
    assert_eq!(gemini.len(), 2);
    assert!(matches!(&gemini[0].parts[0], GeminiPart::Text(t) if t == "Hello chain test"));
    assert!(matches!(&gemini[1].parts[0], GeminiPart::Text(t) if t == "Chain reply"));
}

#[test]
fn chain_kimi_to_copilot_to_openai() {
    let kimi_msgs = vec![
        kimi_msg("system", "Kimi instructions"),
        kimi_msg("user", "First message"),
        kimi_msg("assistant", "First reply"),
    ];

    // Kimi → IR
    let ir1 = kimi_ir::to_ir(&kimi_msgs);

    // IR → Copilot
    let copilot_msgs = copilot_ir::from_ir(&ir1);
    assert_eq!(copilot_msgs.len(), 3);
    assert_eq!(copilot_msgs[0].role, "system");

    // Copilot → IR
    let ir2 = copilot_ir::to_ir(&copilot_msgs);
    assert_eq!(ir2.len(), 3);

    // IR → OpenAI
    let openai_msgs = openai_ir::from_ir(&ir2);
    assert_eq!(openai_msgs.len(), 3);
    assert_eq!(openai_msgs[0].role, "system");
    assert_eq!(openai_msgs[0].content.as_deref(), Some("Kimi instructions"));
    assert_eq!(openai_msgs[2].content.as_deref(), Some("First reply"));
}

#[test]
fn chain_gemini_to_codex_to_kimi() {
    let contents = vec![
        gemini_text("model", "Analyzing the code."),
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "lint".into(),
                args: json!({"file": "main.rs"}),
            }],
        },
    ];

    // Gemini → IR
    let ir1 = gemini_ir::to_ir(&contents, None);

    // IR → Codex (only assistant items survive)
    let codex_items = codex_ir::from_ir(&ir1);
    assert!(!codex_items.is_empty());

    // Codex → IR
    let ir2 = codex_ir::to_ir(&codex_items);

    // IR → Kimi
    let kimi_msgs = kimi_ir::from_ir(&ir2);
    assert!(!kimi_msgs.is_empty());

    // Verify the function call name survived
    let has_lint = kimi_msgs.iter().any(|m| {
        m.tool_calls
            .as_ref()
            .is_some_and(|tcs| tcs.iter().any(|tc| tc.function.name == "lint"))
    });
    assert!(has_lint);
}

// ═══════════════════════════════════════════════════════════════════════════
// §17  IrConversation helpers
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_conversation_accessors() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "user"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "tool1".into(),
                input: json!({}),
            }],
        ),
        IrMessage::text(IrRole::Assistant, "final"),
    ]);

    assert_eq!(conv.len(), 4);
    assert!(!conv.is_empty());

    // system_message
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.text_content(), "sys");

    // last_assistant
    let last_asst = conv.last_assistant().unwrap();
    assert_eq!(last_asst.text_content(), "final");

    // tool_calls
    let tcs = conv.tool_calls();
    assert_eq!(tcs.len(), 1);
    assert!(matches!(tcs[0], IrContentBlock::ToolUse { name, .. } if name == "tool1"));

    // messages_by_role
    let asst_msgs = conv.messages_by_role(IrRole::Assistant);
    assert_eq!(asst_msgs.len(), 2);

    // last_message
    let last = conv.last_message().unwrap();
    assert_eq!(last.text_content(), "final");

    // is_text_only
    assert!(conv.messages[0].is_text_only());
    assert!(!conv.messages[2].is_text_only());
}

#[test]
fn ir_conversation_push_chaining() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "first"))
        .push(IrMessage::text(IrRole::Assistant, "second"));

    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].text_content(), "first");
    assert_eq!(conv.messages[1].text_content(), "second");
}

#[test]
fn ir_conversation_serde_roundtrip() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys prompt"),
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "thinking...".into(),
                },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "search".into(),
                    input: json!({"q": "test"}),
                },
            ],
        ),
    ]);

    let json = serde_json::to_string(&conv).unwrap();
    let deserialized: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(conv, deserialized);
}

// ═══════════════════════════════════════════════════════════════════════════
// §18  Empty content blocks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_content_blocks_handled_by_all_dialects() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(IrRole::User, vec![])]);

    let oai = openai_ir::from_ir(&ir);
    assert_eq!(oai.len(), 1);
    assert!(oai[0].content.is_none());

    let cl = claude_ir::from_ir(&ir);
    assert_eq!(cl.len(), 1);
    assert!(cl[0].content.is_empty());

    let gm = gemini_ir::from_ir(&ir);
    assert_eq!(gm.len(), 1);
    assert!(gm[0].parts.is_empty());

    let km = kimi_ir::from_ir(&ir);
    assert_eq!(km.len(), 1);
    assert!(km[0].content.is_none());

    let cp = copilot_ir::from_ir(&ir);
    assert_eq!(cp.len(), 1);
    assert!(cp[0].content.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// §19  ToolResult (error flag) cross-dialect
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_result_error_flag_survives_claude_roundtrip() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "t_err".into(),
            content: vec![IrContentBlock::Text {
                text: "command failed".into(),
            }],
            is_error: true,
        }],
    )]);

    let claude_msgs = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude_msgs[0].content).unwrap();
    match &blocks[0] {
        ClaudeContentBlock::ToolResult { is_error, .. } => {
            assert_eq!(*is_error, Some(true));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }

    // Claude → IR → verify is_error survives
    let ir2 = claude_ir::to_ir(&claude_msgs, None);
    match &ir2.messages[0].content[0] {
        IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_result_openai_roundtrip() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c_42".into(),
            content: vec![IrContentBlock::Text {
                text: "result output".into(),
            }],
            is_error: false,
        }],
    )]);

    let oai = openai_ir::from_ir(&ir);
    assert_eq!(oai[0].role, "tool");
    assert_eq!(oai[0].tool_call_id.as_deref(), Some("c_42"));
    assert_eq!(oai[0].content.as_deref(), Some("result output"));

    // Roundtrip
    let ir2 = openai_ir::to_ir(&oai);
    match &ir2.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => {
            assert_eq!(tool_use_id, "c_42");
            assert_eq!(content.len(), 1);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §20  Mixed content blocks in a single message
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mixed_text_and_tool_use_in_single_assistant_message() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me search for that.".into(),
            },
            IrContentBlock::ToolUse {
                id: "call_mix".into(),
                name: "search".into(),
                input: json!({"q": "Rust"}),
            },
        ],
    )]);

    // OpenAI: text + tool_call
    let oai = openai_ir::from_ir(&ir);
    assert_eq!(oai[0].content.as_deref(), Some("Let me search for that."));
    assert_eq!(
        oai[0].tool_calls.as_ref().unwrap()[0].function.name,
        "search"
    );

    // Claude: structured blocks
    let cl = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&cl[0].content).unwrap();
    assert_eq!(blocks.len(), 2);
    assert!(matches!(&blocks[0], ClaudeContentBlock::Text { .. }));
    assert!(matches!(&blocks[1], ClaudeContentBlock::ToolUse { .. }));

    // Gemini: text + function_call parts
    let gm = gemini_ir::from_ir(&ir);
    assert_eq!(gm[0].parts.len(), 2);
    assert!(matches!(&gm[0].parts[0], GeminiPart::Text(_)));
    assert!(matches!(&gm[0].parts[1], GeminiPart::FunctionCall { .. }));

    // Kimi: text + tool_calls
    let km = kimi_ir::from_ir(&ir);
    assert_eq!(km[0].content.as_deref(), Some("Let me search for that."));
    assert!(km[0].tool_calls.is_some());

    // Codex: splits into Message + FunctionCall
    let cdx = codex_ir::from_ir(&ir);
    assert!(cdx.len() >= 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// §21  Copilot-specific metadata preservation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_references_survive_roundtrip() {
    use abp_copilot_sdk::dialect::{CopilotReference, CopilotReferenceType};

    let refs = vec![CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "file-main".into(),
        data: json!({"path": "src/main.rs"}),
        metadata: None,
    }];
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "Explain this file".into(),
        name: Some("dev".into()),
        copilot_references: refs,
    }];

    let ir = copilot_ir::to_ir(&msgs);
    let back = copilot_ir::from_ir(&ir);

    assert_eq!(back[0].content, "Explain this file");
    assert_eq!(back[0].name.as_deref(), Some("dev"));
    assert_eq!(back[0].copilot_references.len(), 1);
    assert_eq!(back[0].copilot_references[0].id, "file-main");
}

#[test]
fn copilot_references_lost_when_crossing_to_other_dialect() {
    use abp_copilot_sdk::dialect::{CopilotReference, CopilotReferenceType};

    let refs = vec![CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "f1".into(),
        data: json!({}),
        metadata: None,
    }];
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "Check this".into(),
        name: None,
        copilot_references: refs,
    }];

    let ir = copilot_ir::to_ir(&msgs);

    // OpenAI doesn't know about copilot_references
    let oai = openai_ir::from_ir(&ir);
    assert_eq!(oai[0].content.as_deref(), Some("Check this"));
    // References are in IR metadata but not in OpenAI's schema
    // The text content is preserved, references are metadata-only

    // But going back through Copilot from the same IR preserves them
    let back = copilot_ir::from_ir(&ir);
    assert_eq!(back[0].copilot_references.len(), 1);
}
