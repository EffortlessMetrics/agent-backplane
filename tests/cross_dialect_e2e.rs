// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests exercising cross-dialect mapping through the IR pipeline.
//!
//! Each test builds native SDK messages, lowers them to IR via `to_ir`,
//! then raises back into a (possibly different) dialect via `from_ir`.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use serde_json::json;

// Re-import lowering modules from each SDK crate.
use abp_claude_sdk::lowering as claude_ir;
use abp_codex_sdk::lowering as codex_ir;
use abp_copilot_sdk::lowering as copilot_ir;
use abp_gemini_sdk::lowering as gemini_ir;
use abp_kimi_sdk::lowering as kimi_ir;
use abp_openai_sdk::lowering as openai_ir;

// Re-import dialect types.
use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
use abp_codex_sdk::dialect::{CodexContentPart, CodexInputItem, CodexResponseItem, CodexUsage};
use abp_copilot_sdk::dialect::CopilotMessage;
use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
use abp_kimi_sdk::dialect::{KimiMessage, KimiUsage};
use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};

// ═══════════════════════════════════════════════════════════════════════════
// §1  Same-dialect passthrough (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_openai_message_preserved_exactly() {
    let msgs = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("You are a coding assistant.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Write hello world in Rust".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("fn main() { println!(\"Hello, world!\"); }".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    let ir = openai_ir::to_ir(&msgs);
    let back = openai_ir::from_ir(&ir);

    assert_eq!(back.len(), msgs.len());
    for (orig, round) in msgs.iter().zip(back.iter()) {
        assert_eq!(orig.role, round.role);
        assert_eq!(orig.content, round.content);
    }
}

#[test]
fn claude_to_claude_content_blocks_preserved() {
    let blocks = vec![
        ClaudeContentBlock::Text {
            text: "Let me check.".into(),
        },
        ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read_file".into(),
            input: json!({"path": "main.rs"}),
        },
    ];
    let msgs = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "Show me main.rs".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        },
    ];

    let ir = claude_ir::to_ir(&msgs, Some("Be helpful"));
    let back = claude_ir::from_ir(&ir);

    // System is extracted separately, so from_ir skips it
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "Show me main.rs");
    // Re-parse the structured blocks to verify fidelity
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[1].content).unwrap();
    assert_eq!(parsed.len(), 2);
    assert!(matches!(&parsed[0], ClaudeContentBlock::Text { text } if text == "Let me check."));
    assert!(matches!(&parsed[1], ClaudeContentBlock::ToolUse { name, .. } if name == "read_file"));
}

#[test]
fn gemini_to_gemini_parts_preserved() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Be concise".into())],
    };
    let contents = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hello".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("Hi there!".into())],
        },
    ];

    let ir = gemini_ir::to_ir(&contents, Some(&sys));
    let back = gemini_ir::from_ir(&ir);

    // System message skipped in from_ir
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].role, "user");
    assert!(matches!(&back[0].parts[0], GeminiPart::Text(t) if t == "Hello"));
    assert_eq!(back[1].role, "model");
    assert!(matches!(&back[1].parts[0], GeminiPart::Text(t) if t == "Hi there!"));
}

#[test]
fn codex_to_codex_passthrough() {
    let items = vec![
        CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Here is the code.".into(),
            }],
        },
        CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "write_file".into(),
            arguments: r#"{"path":"a.rs","content":"fn main() {}"}"#.into(),
        },
        CodexResponseItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "ok".into(),
        },
    ];

    let ir = codex_ir::to_ir(&items);
    let back = codex_ir::from_ir(&ir);

    assert_eq!(back.len(), 3);
    assert!(matches!(&back[0], CodexResponseItem::Message { role, .. } if role == "assistant"));
    assert!(
        matches!(&back[1], CodexResponseItem::FunctionCall { name, .. } if name == "write_file")
    );
    assert!(
        matches!(&back[2], CodexResponseItem::FunctionCallOutput { call_id, .. } if call_id == "fc_1")
    );
}

#[test]
fn mock_passthrough_via_ir() {
    // "Mock" here means a plain IR roundtrip — verifying the IR itself is a passthrough.
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a test bot."),
        IrMessage::text(IrRole::User, "ping"),
        IrMessage::text(IrRole::Assistant, "pong"),
    ]);

    let serialized = serde_json::to_string(&conv).unwrap();
    let deserialized: IrConversation = serde_json::from_str(&serialized).unwrap();
    assert_eq!(conv, deserialized);
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  Cross-dialect mapping (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_claude_messages_mapped_roles_preserved() {
    let msgs = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("Be helpful.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("What is Rust?".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Rust is a systems language.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    let ir = openai_ir::to_ir(&msgs);
    let system_prompt = claude_ir::extract_system_prompt(&ir);
    let claude_msgs = claude_ir::from_ir(&ir);

    assert_eq!(system_prompt.as_deref(), Some("Be helpful."));
    // Claude from_ir skips system messages
    assert_eq!(claude_msgs.len(), 2);
    assert_eq!(claude_msgs[0].role, "user");
    assert_eq!(claude_msgs[0].content, "What is Rust?");
    assert_eq!(claude_msgs[1].role, "assistant");
    assert_eq!(claude_msgs[1].content, "Rust is a systems language.");
}

#[test]
fn claude_to_openai_content_blocks_become_string() {
    let blocks = vec![
        ClaudeContentBlock::Text {
            text: "Here is the result.".into(),
        },
        ClaudeContentBlock::Thinking {
            thinking: "Let me reason...".into(),
            signature: None,
        },
    ];
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];

    let ir = claude_ir::to_ir(&msgs, None);
    let openai_msgs = openai_ir::from_ir(&ir);

    assert_eq!(openai_msgs.len(), 1);
    assert_eq!(openai_msgs[0].role, "assistant");
    // Text + Thinking both become text in OpenAI
    let text = openai_msgs[0].content.as_deref().unwrap();
    assert!(text.contains("Here is the result."));
    assert!(text.contains("Let me reason..."));
}

#[test]
fn openai_to_gemini_messages_become_parts() {
    let msgs = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("Instructions here".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hello Gemini".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    let ir = openai_ir::to_ir(&msgs);
    let sys_instr = gemini_ir::extract_system_instruction(&ir);
    let gemini_contents = gemini_ir::from_ir(&ir);

    assert!(sys_instr.is_some());
    assert!(
        matches!(&sys_instr.unwrap().parts[0], GeminiPart::Text(t) if t == "Instructions here")
    );
    // from_ir skips system messages
    assert_eq!(gemini_contents.len(), 1);
    assert_eq!(gemini_contents[0].role, "user");
    assert!(matches!(&gemini_contents[0].parts[0], GeminiPart::Text(t) if t == "Hello Gemini"));
}

#[test]
fn gemini_to_claude_parts_become_content_blocks() {
    let contents = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Do something".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"q": "rust"}),
            }],
        },
    ];

    let ir = gemini_ir::to_ir(&contents, None);
    let claude_msgs = claude_ir::from_ir(&ir);

    assert_eq!(claude_msgs.len(), 2);
    assert_eq!(claude_msgs[0].role, "user");
    assert_eq!(claude_msgs[0].content, "Do something");
    assert_eq!(claude_msgs[1].role, "assistant");
    // Should contain a ToolUse block serialised as JSON
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&claude_msgs[1].content).unwrap();
    assert!(matches!(&parsed[0], ClaudeContentBlock::ToolUse { name, .. } if name == "search"));
}

#[test]
fn claude_to_codex_thinking_blocks_handled() {
    let blocks = vec![
        ClaudeContentBlock::Thinking {
            thinking: "Step 1: parse. Step 2: compile.".into(),
            signature: Some("sig".into()),
        },
        ClaudeContentBlock::Text {
            text: "Done compiling.".into(),
        },
    ];
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];

    let ir = claude_ir::to_ir(&msgs, None);
    let codex_items = codex_ir::from_ir(&ir);

    // Thinking → Reasoning, Text → Message
    assert_eq!(codex_items.len(), 2);
    assert!(
        matches!(&codex_items[0], CodexResponseItem::Reasoning { summary } if !summary.is_empty())
    );
    assert!(
        matches!(&codex_items[1], CodexResponseItem::Message { content, .. } if !content.is_empty())
    );
}

#[test]
fn codex_to_openai_instructions_become_system_message() {
    let input_items = vec![
        CodexInputItem::Message {
            role: "system".into(),
            content: "You are a code reviewer.".into(),
        },
        CodexInputItem::Message {
            role: "user".into(),
            content: "Review this PR".into(),
        },
    ];

    let ir = codex_ir::input_to_ir(&input_items);
    let openai_msgs = openai_ir::from_ir(&ir);

    assert_eq!(openai_msgs.len(), 2);
    assert_eq!(openai_msgs[0].role, "system");
    assert_eq!(
        openai_msgs[0].content.as_deref(),
        Some("You are a code reviewer.")
    );
    assert_eq!(openai_msgs[1].role, "user");
    assert_eq!(openai_msgs[1].content.as_deref(), Some("Review this PR"));
}

#[test]
fn kimi_to_claude_multi_turn_preserved() {
    let msgs = vec![
        KimiMessage {
            role: "system".into(),
            content: Some("Be concise.".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "assistant".into(),
            content: Some("Hi!".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "user".into(),
            content: Some("Bye".into()),
            tool_call_id: None,
            tool_calls: None,
        },
    ];

    let ir = kimi_ir::to_ir(&msgs);
    let system_prompt = claude_ir::extract_system_prompt(&ir);
    let claude_msgs = claude_ir::from_ir(&ir);

    assert_eq!(system_prompt.as_deref(), Some("Be concise."));
    // 4 messages minus the system = 3 Claude messages
    assert_eq!(claude_msgs.len(), 3);
    assert_eq!(claude_msgs[0].role, "user");
    assert_eq!(claude_msgs[0].content, "Hello");
    assert_eq!(claude_msgs[1].role, "assistant");
    assert_eq!(claude_msgs[1].content, "Hi!");
    assert_eq!(claude_msgs[2].role, "user");
    assert_eq!(claude_msgs[2].content, "Bye");
}

#[test]
fn copilot_to_openai_messages_mapped() {
    let msgs = vec![
        CopilotMessage {
            role: "system".into(),
            content: "Be helpful.".into(),
            name: None,
            copilot_references: vec![],
        },
        CopilotMessage {
            role: "user".into(),
            content: "Explain async/await".into(),
            name: Some("alice".into()),
            copilot_references: vec![],
        },
        CopilotMessage {
            role: "assistant".into(),
            content: "async/await is for concurrency.".into(),
            name: None,
            copilot_references: vec![],
        },
    ];

    let ir = copilot_ir::to_ir(&msgs);
    let openai_msgs = openai_ir::from_ir(&ir);

    assert_eq!(openai_msgs.len(), 3);
    assert_eq!(openai_msgs[0].role, "system");
    assert_eq!(openai_msgs[0].content.as_deref(), Some("Be helpful."));
    assert_eq!(openai_msgs[1].role, "user");
    assert_eq!(
        openai_msgs[1].content.as_deref(),
        Some("Explain async/await")
    );
    assert_eq!(openai_msgs[2].role, "assistant");
    assert_eq!(
        openai_msgs[2].content.as_deref(),
        Some("async/await is for concurrency.")
    );
}

#[test]
fn openai_to_kimi_tool_calls_preserved() {
    let msgs = vec![
        OpenAIMessage {
            role: "user".into(),
            content: Some("Search for Rust".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "web_search".into(),
                    arguments: r#"{"query":"Rust lang"}"#.into(),
                },
            }]),
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "tool".into(),
            content: Some("Rust is a systems language...".into()),
            tool_calls: None,
            tool_call_id: Some("call_1".into()),
        },
    ];

    let ir = openai_ir::to_ir(&msgs);
    let kimi_msgs = kimi_ir::from_ir(&ir);

    assert_eq!(kimi_msgs.len(), 3);
    assert_eq!(kimi_msgs[0].role, "user");
    // Assistant with tool call
    assert_eq!(kimi_msgs[1].role, "assistant");
    let tc = &kimi_msgs[1].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "web_search");
    assert_eq!(tc.id, "call_1");
    // Tool result
    assert_eq!(kimi_msgs[2].role, "tool");
    assert_eq!(kimi_msgs[2].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(
        kimi_msgs[2].content.as_deref(),
        Some("Rust is a systems language...")
    );
}

#[test]
fn full_chain_openai_ir_claude_ir_gemini() {
    let openai_msgs = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("Be brief.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hello chain".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Chain reply".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    // OpenAI → IR
    let ir1 = openai_ir::to_ir(&openai_msgs);
    assert_eq!(ir1.len(), 3);

    // IR → Claude
    let claude_sys = claude_ir::extract_system_prompt(&ir1);
    let claude_msgs = claude_ir::from_ir(&ir1);
    assert_eq!(claude_sys.as_deref(), Some("Be brief."));
    assert_eq!(claude_msgs.len(), 2); // system stripped

    // Claude → IR (round 2)
    let ir2 = claude_ir::to_ir(&claude_msgs, claude_sys.as_deref());
    assert_eq!(ir2.len(), 3);
    // Verify text survived
    assert_eq!(ir2.messages[0].text_content(), "Be brief.");
    assert_eq!(ir2.messages[1].text_content(), "Hello chain");
    assert_eq!(ir2.messages[2].text_content(), "Chain reply");

    // IR → Gemini
    let gemini_sys = gemini_ir::extract_system_instruction(&ir2);
    let gemini_contents = gemini_ir::from_ir(&ir2);
    assert!(gemini_sys.is_some());
    assert_eq!(gemini_contents.len(), 2); // system stripped
    assert!(matches!(&gemini_contents[0].parts[0], GeminiPart::Text(t) if t == "Hello chain"));
    assert!(matches!(&gemini_contents[1].parts[0], GeminiPart::Text(t) if t == "Chain reply"));
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  Error / edge cases (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unsupported_feature_image_in_copilot_drops_gracefully() {
    // Copilot is text-only. An Image block from Claude→IR→Copilot should not
    // appear as image data — it gets silently dropped or rendered as empty text.
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "base64data".into(),
        }],
    )]);

    let copilot_msgs = copilot_ir::from_ir(&ir);
    assert_eq!(copilot_msgs.len(), 1);
    // Image block is not a Text or Thinking, so it won't contribute text
    assert!(copilot_msgs[0].content.is_empty());
}

#[test]
fn invalid_ir_no_crash_on_empty_content() {
    // An IrMessage with zero content blocks should not panic anywhere.
    let ir = IrConversation::from_messages(vec![IrMessage::new(IrRole::User, vec![])]);

    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai.len(), 1);
    assert!(openai[0].content.is_none());

    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude.len(), 1);
    assert!(claude[0].content.is_empty());

    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini.len(), 1);
    assert!(gemini[0].parts.is_empty());

    let kimi = kimi_ir::from_ir(&ir);
    assert_eq!(kimi.len(), 1);
    assert!(kimi[0].content.is_none());

    let copilot = copilot_ir::from_ir(&ir);
    assert_eq!(copilot.len(), 1);
    assert!(copilot[0].content.is_empty());

    // Codex skips user messages in from_ir, so we test with assistant role
    let ir_asst = IrConversation::from_messages(vec![IrMessage::new(IrRole::Assistant, vec![])]);
    let codex = codex_ir::from_ir(&ir_asst);
    // Empty assistant produces an empty message item list
    assert!(codex.is_empty());
}

#[test]
fn empty_conversation_mapping() {
    let empty_ir = IrConversation::new();

    assert!(openai_ir::from_ir(&empty_ir).is_empty());
    assert!(claude_ir::from_ir(&empty_ir).is_empty());
    assert!(gemini_ir::from_ir(&empty_ir).is_empty());
    assert!(codex_ir::from_ir(&empty_ir).is_empty());
    assert!(kimi_ir::from_ir(&empty_ir).is_empty());
    assert!(copilot_ir::from_ir(&empty_ir).is_empty());

    // Also verify to_ir from empty inputs
    assert!(openai_ir::to_ir(&[]).is_empty());
    assert!(claude_ir::to_ir(&[], None).is_empty());
    assert!(gemini_ir::to_ir(&[], None).is_empty());
    assert!(codex_ir::to_ir(&[]).is_empty());
    assert!(codex_ir::input_to_ir(&[]).is_empty());
    assert!(kimi_ir::to_ir(&[]).is_empty());
    assert!(copilot_ir::to_ir(&[]).is_empty());
}

#[test]
fn mapping_with_unknown_role_defaults_to_user() {
    // All SDKs should handle an unknown role string gracefully by mapping to User.
    let openai = openai_ir::to_ir(&[OpenAIMessage {
        role: "developer".into(),
        content: Some("hi".into()),
        tool_calls: None,
        tool_call_id: None,
    }]);
    assert_eq!(openai.messages[0].role, IrRole::User);

    let kimi = kimi_ir::to_ir(&[KimiMessage {
        role: "custom_role".into(),
        content: Some("hello".into()),
        tool_call_id: None,
        tool_calls: None,
    }]);
    assert_eq!(kimi.messages[0].role, IrRole::User);

    let copilot = copilot_ir::to_ir(&[CopilotMessage {
        role: "tool".into(),
        content: "data".into(),
        name: None,
        copilot_references: vec![],
    }]);
    assert_eq!(copilot.messages[0].role, IrRole::User);
}

#[test]
fn lossy_mapping_codex_drops_system_user_in_from_ir() {
    // Codex from_ir only emits assistant and tool items.
    // System/user messages are intentionally dropped — this is a known lossy path.
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "instructions"),
        IrMessage::text(IrRole::User, "request"),
        IrMessage::text(IrRole::Assistant, "response"),
    ]);

    let codex_items = codex_ir::from_ir(&ir);
    // Only the assistant message survives
    assert_eq!(codex_items.len(), 1);
    assert!(
        matches!(&codex_items[0], CodexResponseItem::Message { role, .. } if role == "assistant")
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Fidelity verification (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn text_content_survives_any_mapping_path() {
    let text = "The quick brown fox jumps over the lazy dog. 🦀";
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, text)]);

    // IR → OpenAI → IR
    let oai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&oai);
    assert_eq!(ir2.messages[0].text_content(), text);

    // IR → Claude → IR
    let cl = claude_ir::from_ir(&ir);
    let ir3 = claude_ir::to_ir(&cl, None);
    assert_eq!(ir3.messages[0].text_content(), text);

    // IR → Gemini → IR
    let gm = gemini_ir::from_ir(&ir);
    let ir4 = gemini_ir::to_ir(&gm, None);
    assert_eq!(ir4.messages[0].text_content(), text);

    // IR → Kimi → IR
    let km = kimi_ir::from_ir(&ir);
    let ir5 = kimi_ir::to_ir(&km);
    assert_eq!(ir5.messages[0].text_content(), text);

    // IR → Copilot → IR
    let cp = copilot_ir::from_ir(&ir);
    let ir6 = copilot_ir::to_ir(&cp);
    assert_eq!(ir6.messages[0].text_content(), text);
}

#[test]
fn tool_names_survive_any_mapping_path() {
    let tool_name = "complex_tool_name_v2";
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: tool_name.into(),
            input: json!({"arg": "value"}),
        }],
    )]);

    // OpenAI roundtrip
    let oai = openai_ir::from_ir(&ir);
    let tc = &oai[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, tool_name);
    let ir2 = openai_ir::to_ir(&oai);
    assert!(matches!(
        &ir2.messages[0].content[0],
        IrContentBlock::ToolUse { name, .. } if name == tool_name
    ));

    // Claude roundtrip
    let cl = claude_ir::from_ir(&ir);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&cl[0].content).unwrap();
    assert!(matches!(&parsed[0], ClaudeContentBlock::ToolUse { name, .. } if name == tool_name));

    // Gemini roundtrip (tool id is synthesized, but name preserved)
    let gm = gemini_ir::from_ir(&ir);
    assert!(matches!(&gm[0].parts[0], GeminiPart::FunctionCall { name, .. } if name == tool_name));

    // Kimi roundtrip
    let km = kimi_ir::from_ir(&ir);
    let ktc = &km[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(ktc.function.name, tool_name);
}

#[test]
fn system_messages_survive_or_fold_correctly() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hi"),
    ]);

    // OpenAI: system message stays as a message
    let oai = openai_ir::from_ir(&ir);
    assert_eq!(oai[0].role, "system");
    assert_eq!(
        oai[0].content.as_deref(),
        Some("You are a helpful assistant.")
    );

    // Claude: system extracted separately, not in message list
    let sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("You are a helpful assistant."));
    let cl = claude_ir::from_ir(&ir);
    assert!(cl.iter().all(|m| m.role != "system"));

    // Gemini: system extracted as instruction, not in content list
    let instr = gemini_ir::extract_system_instruction(&ir);
    assert!(instr.is_some());
    let gm = gemini_ir::from_ir(&ir);
    assert!(gm.iter().all(|c| c.role != "system"));

    // Kimi: system stays as a message (like OpenAI)
    let km = kimi_ir::from_ir(&ir);
    assert_eq!(km[0].role, "system");

    // Copilot: system stays as a message
    let cp = copilot_ir::from_ir(&ir);
    assert_eq!(cp[0].role, "system");
}

#[test]
fn multiple_messages_maintain_order() {
    let messages: Vec<&str> = vec!["first", "second", "third", "fourth", "fifth"];
    let ir = IrConversation::from_messages(
        messages
            .iter()
            .enumerate()
            .map(|(i, text)| {
                let role = if i % 2 == 0 {
                    IrRole::User
                } else {
                    IrRole::Assistant
                };
                IrMessage::text(role, *text)
            })
            .collect(),
    );

    // OpenAI preserves order
    let oai = openai_ir::from_ir(&ir);
    for (i, msg) in oai.iter().enumerate() {
        assert_eq!(msg.content.as_deref(), Some(messages[i]));
    }

    // Kimi preserves order
    let km = kimi_ir::from_ir(&ir);
    for (i, msg) in km.iter().enumerate() {
        assert_eq!(msg.content.as_deref(), Some(messages[i]));
    }

    // Copilot preserves order
    let cp = copilot_ir::from_ir(&ir);
    for (i, msg) in cp.iter().enumerate() {
        assert_eq!(msg.content, messages[i]);
    }

    // Claude preserves order (no system here)
    let cl = claude_ir::from_ir(&ir);
    for (i, msg) in cl.iter().enumerate() {
        assert_eq!(msg.content, messages[i]);
    }

    // Gemini preserves order
    let gm = gemini_ir::from_ir(&ir);
    for (i, content) in gm.iter().enumerate() {
        assert!(matches!(&content.parts[0], GeminiPart::Text(t) if t == messages[i]));
    }
}

#[test]
fn usage_stats_preserved_or_zeroed() {
    // Codex usage
    let codex_usage = CodexUsage {
        input_tokens: 150,
        output_tokens: 75,
        total_tokens: 225,
    };
    let ir_usage = codex_ir::usage_to_ir(&codex_usage);
    assert_eq!(ir_usage.input_tokens, 150);
    assert_eq!(ir_usage.output_tokens, 75);
    assert_eq!(ir_usage.total_tokens, 225);
    assert_eq!(ir_usage.cache_read_tokens, 0);
    assert_eq!(ir_usage.cache_write_tokens, 0);

    // Kimi usage
    let kimi_usage = KimiUsage {
        prompt_tokens: 200,
        completion_tokens: 100,
        total_tokens: 300,
    };
    let ir_usage2 = kimi_ir::usage_to_ir(&kimi_usage);
    assert_eq!(ir_usage2.input_tokens, 200);
    assert_eq!(ir_usage2.output_tokens, 100);
    assert_eq!(ir_usage2.total_tokens, 300);

    // IrUsage merge
    let merged = ir_usage.merge(ir_usage2);
    assert_eq!(merged.input_tokens, 350);
    assert_eq!(merged.output_tokens, 175);
    assert_eq!(merged.total_tokens, 525);

    // Default usage is all zeros
    let default_usage = IrUsage::default();
    assert_eq!(default_usage.input_tokens, 0);
    assert_eq!(default_usage.output_tokens, 0);
    assert_eq!(default_usage.total_tokens, 0);
}
