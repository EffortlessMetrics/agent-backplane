#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Exhaustive IR roundtrip, normalization, lowering, and cross-SDK tests.

use abp_ir::normalize::{
    dedup_system, extract_system, merge_adjacent_text, normalize, normalize_role,
    normalize_tool_schemas, sort_tools, strip_empty, strip_metadata, trim_text,
};
use abp_ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};

use abp_ir::lower::{
    ir_role_to_dialect, lower_for_dialect, lower_to_claude, lower_to_codex, lower_to_copilot,
    lower_to_gemini, lower_to_kimi, lower_to_openai,
};
use abp_sdk_types::Dialect;

// SDK lowering modules
use abp_claude_sdk::lowering as claude_lowering;
use abp_codex_sdk::lowering as codex_lowering;
use abp_copilot_sdk::lowering as copilot_lowering;
use abp_gemini_sdk::lowering as gemini_lowering;
use abp_kimi_sdk::lowering as kimi_lowering;
use abp_openai_sdk::lowering as openai_lowering;

// SDK dialect types
use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
use abp_codex_sdk::dialect::{CodexContentPart, CodexInputItem, CodexResponseItem};
use abp_copilot_sdk::dialect::CopilotMessage;
use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall};
use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};

// ═══════════════════════════════════════════════════════════════════════════
// 1. BASIC SDK ROUNDTRIP TESTS (per SDK)
// ═══════════════════════════════════════════════════════════════════════════

// ── OpenAI roundtrip ────────────────────────────────────────────────────

#[test]
fn openai_simple_text_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("Hello, world!".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = openai_lowering::to_ir(&msgs);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content.as_deref(), Some("Hello, world!"));
}

#[test]
fn openai_multi_turn_roundtrip() {
    let msgs = vec![
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
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Hello!".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Bye".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 4);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back.len(), 4);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[1].role, "user");
    assert_eq!(back[2].role, "assistant");
    assert_eq!(back[3].content.as_deref(), Some("Bye"));
}

#[test]
fn openai_system_message_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "system".into(),
        content: Some("You are a coding assistant.".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::System);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back[0].role, "system");
    assert_eq!(
        back[0].content.as_deref(),
        Some("You are a coding assistant.")
    );
}

#[test]
fn openai_tool_call_roundtrip() {
    let msgs = vec![
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Let me check.".into()),
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
            content: Some("fn main() {}".into()),
            tool_calls: None,
            tool_call_id: Some("call_abc".into()),
        },
    ];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 2);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back[0].role, "assistant");
    assert_eq!(back[0].content.as_deref(), Some("Let me check."));
    assert!(back[0].tool_calls.is_some());
    assert_eq!(back[1].role, "tool");
    assert_eq!(back[1].tool_call_id.as_deref(), Some("call_abc"));
}

#[test]
fn openai_assistant_only_tool_calls_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"rust"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = openai_lowering::to_ir(&msgs);
    let back = openai_lowering::from_ir(&conv);
    assert!(back[0].content.is_none());
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_1");
    assert_eq!(tc.function.name, "search");
}

// ── Claude roundtrip ────────────────────────────────────────────────────

#[test]
fn claude_simple_text_roundtrip() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hello, world!".into(),
    }];
    let conv = claude_lowering::to_ir(&msgs, None);
    let back = claude_lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "Hello, world!");
}

#[test]
fn claude_multi_turn_roundtrip() {
    let msgs = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "Hi".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "Hello!".into(),
        },
        ClaudeMessage {
            role: "user".into(),
            content: "Bye".into(),
        },
    ];
    let conv = claude_lowering::to_ir(&msgs, Some("Be nice"));
    assert_eq!(conv.len(), 4); // system + 3 messages
    let back = claude_lowering::from_ir(&conv);
    // System messages are skipped in from_ir for Claude
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[2].content, "Bye");
}

#[test]
fn claude_system_prompt_roundtrip() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hi".into(),
    }];
    let conv = claude_lowering::to_ir(&msgs, Some("You are a poet."));
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "You are a poet.");
    let sys = claude_lowering::extract_system_prompt(&conv);
    assert_eq!(sys.as_deref(), Some("You are a poet."));
}

#[test]
fn claude_tool_use_roundtrip() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "grep".into(),
        input: serde_json::json!({"pattern": "fn main"}),
    }];
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = claude_lowering::to_ir(&msgs, None);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::ToolUse { name, .. } if name == "grep"
    ));
    let back = claude_lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    assert!(matches!(&parsed[0], ClaudeContentBlock::ToolUse { name, .. } if name == "grep"));
}

#[test]
fn claude_tool_result_roundtrip() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("file contents".into()),
        is_error: None,
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = claude_lowering::to_ir(&msgs, None);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "tu_1"
    ));
    let back = claude_lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    assert!(matches!(
        &parsed[0],
        ClaudeContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "tu_1"
    ));
}

// ── Gemini roundtrip ────────────────────────────────────────────────────

#[test]
fn gemini_simple_text_roundtrip() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hello, world!".into())],
    }];
    let conv = gemini_lowering::to_ir(&contents, None);
    let back = gemini_lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert!(matches!(&back[0].parts[0], GeminiPart::Text(t) if t == "Hello, world!"));
}

#[test]
fn gemini_multi_turn_roundtrip() {
    let contents = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hi".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("Hello!".into())],
        },
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Bye".into())],
        },
    ];
    let conv = gemini_lowering::to_ir(&contents, None);
    assert_eq!(conv.len(), 3);
    let back = gemini_lowering::from_ir(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[1].role, "model");
}

#[test]
fn gemini_system_instruction_roundtrip() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Be concise.".into())],
    };
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hi".into())],
    }];
    let conv = gemini_lowering::to_ir(&contents, Some(&sys));
    assert_eq!(conv.messages[0].role, IrRole::System);
    let extracted = gemini_lowering::extract_system_instruction(&conv);
    assert!(extracted.is_some());
}

#[test]
fn gemini_function_call_roundtrip() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "search".into(),
            args: serde_json::json!({"q": "rust"}),
        }],
    }];
    let conv = gemini_lowering::to_ir(&contents, None);
    let back = gemini_lowering::from_ir(&conv);
    assert!(matches!(
        &back[0].parts[0],
        GeminiPart::FunctionCall { name, .. } if name == "search"
    ));
}

#[test]
fn gemini_function_response_roundtrip() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "search".into(),
            response: serde_json::json!("results"),
        }],
    }];
    let conv = gemini_lowering::to_ir(&contents, None);
    let back = gemini_lowering::from_ir(&conv);
    assert!(matches!(
        &back[0].parts[0],
        GeminiPart::FunctionResponse { name, .. } if name == "search"
    ));
}

// ── Codex roundtrip ─────────────────────────────────────────────────────

#[test]
fn codex_simple_text_roundtrip() {
    let items = vec![CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText {
            text: "Hello!".into(),
        }],
    }];
    let conv = codex_lowering::to_ir(&items);
    let back = codex_lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert!(matches!(
        &back[0],
        CodexResponseItem::Message { role, content }
            if role == "assistant" && !content.is_empty()
    ));
}

#[test]
fn codex_input_items_to_ir() {
    let items = vec![
        CodexInputItem::Message {
            role: "system".into(),
            content: "Be helpful.".into(),
        },
        CodexInputItem::Message {
            role: "user".into(),
            content: "Hi".into(),
        },
    ];
    let conv = codex_lowering::input_to_ir(&items);
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn codex_multi_turn_roundtrip() {
    let items = vec![
        CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Let me check.".into(),
            }],
        },
        CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "read_file".into(),
            arguments: r#"{"path":"a.rs"}"#.into(),
        },
        CodexResponseItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "fn main() {}".into(),
        },
        CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Done.".into(),
            }],
        },
    ];
    let conv = codex_lowering::to_ir(&items);
    assert_eq!(conv.len(), 4);
    let back = codex_lowering::from_ir(&conv);
    assert_eq!(back.len(), 4);
}

#[test]
fn codex_system_message_in_input() {
    let items = vec![CodexInputItem::Message {
        role: "system".into(),
        content: "Be brief.".into(),
    }];
    let conv = codex_lowering::input_to_ir(&items);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "Be brief.");
}

#[test]
fn codex_tool_call_roundtrip() {
    let items = vec![CodexResponseItem::FunctionCall {
        id: "fc_42".into(),
        call_id: None,
        name: "shell".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
    }];
    let conv = codex_lowering::to_ir(&items);
    let back = codex_lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert!(matches!(
        &back[0],
        CodexResponseItem::FunctionCall { id, name, .. } if id == "fc_42" && name == "shell"
    ));
}

// ── Copilot roundtrip ───────────────────────────────────────────────────

#[test]
fn copilot_simple_text_roundtrip() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "Hello!".into(),
        name: None,
        copilot_references: vec![],
    }];
    let conv = copilot_lowering::to_ir(&msgs);
    let back = copilot_lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "Hello!");
}

#[test]
fn copilot_multi_turn_roundtrip() {
    let msgs = vec![
        CopilotMessage {
            role: "system".into(),
            content: "Be helpful.".into(),
            name: None,
            copilot_references: vec![],
        },
        CopilotMessage {
            role: "user".into(),
            content: "Hi".into(),
            name: None,
            copilot_references: vec![],
        },
        CopilotMessage {
            role: "assistant".into(),
            content: "Hello!".into(),
            name: None,
            copilot_references: vec![],
        },
    ];
    let conv = copilot_lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 3);
    let back = copilot_lowering::from_ir(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[2].content, "Hello!");
}

#[test]
fn copilot_system_message_roundtrip() {
    let msgs = vec![CopilotMessage {
        role: "system".into(),
        content: "You are Copilot.".into(),
        name: None,
        copilot_references: vec![],
    }];
    let conv = copilot_lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::System);
    let back = copilot_lowering::from_ir(&conv);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content, "You are Copilot.");
}

#[test]
fn copilot_name_preserved_roundtrip() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "Hi".into(),
        name: Some("alice".into()),
        copilot_references: vec![],
    }];
    let conv = copilot_lowering::to_ir(&msgs);
    let back = copilot_lowering::from_ir(&conv);
    assert_eq!(back[0].name.as_deref(), Some("alice"));
}

#[test]
fn copilot_tool_role_maps_to_user() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Tool, "result data")]);
    let back = copilot_lowering::from_ir(&conv);
    assert_eq!(back[0].role, "user");
}

// ── Kimi roundtrip ──────────────────────────────────────────────────────

#[test]
fn kimi_simple_text_roundtrip() {
    let msgs = vec![KimiMessage {
        role: "user".into(),
        content: Some("Hello!".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let conv = kimi_lowering::to_ir(&msgs);
    let back = kimi_lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content.as_deref(), Some("Hello!"));
}

#[test]
fn kimi_multi_turn_roundtrip() {
    let msgs = vec![
        KimiMessage {
            role: "system".into(),
            content: Some("Be concise.".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "assistant".into(),
            content: Some("Hello!".into()),
            tool_call_id: None,
            tool_calls: None,
        },
    ];
    let conv = kimi_lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 3);
    let back = kimi_lowering::from_ir(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[2].content.as_deref(), Some("Hello!"));
}

#[test]
fn kimi_system_message_roundtrip() {
    let msgs = vec![KimiMessage {
        role: "system".into(),
        content: Some("You are Kimi.".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let conv = kimi_lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::System);
    let back = kimi_lowering::from_ir(&conv);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content.as_deref(), Some("You are Kimi."));
}

#[test]
fn kimi_tool_call_roundtrip() {
    let msgs = vec![
        KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "web_search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
        },
        KimiMessage {
            role: "tool".into(),
            content: Some("search results".into()),
            tool_call_id: Some("call_1".into()),
            tool_calls: None,
        },
    ];
    let conv = kimi_lowering::to_ir(&msgs);
    let back = kimi_lowering::from_ir(&conv);
    assert_eq!(back[0].role, "assistant");
    assert!(back[0].tool_calls.is_some());
    assert_eq!(back[1].role, "tool");
    assert_eq!(back[1].tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn kimi_text_and_tool_call_roundtrip() {
    let msgs = vec![KimiMessage {
        role: "assistant".into(),
        content: Some("Let me search.".into()),
        tool_call_id: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "call_7".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "search".into(),
                arguments: "{}".into(),
            },
        }]),
    }];
    let conv = kimi_lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = kimi_lowering::from_ir(&conv);
    assert!(back[0].content.is_some());
    assert!(back[0].tool_calls.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. IR NORMALIZATION TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn normalize_role_human_to_user() {
    assert_eq!(normalize_role("human"), Some(IrRole::User));
}

#[test]
fn normalize_role_bot_to_assistant() {
    assert_eq!(normalize_role("bot"), Some(IrRole::Assistant));
}

#[test]
fn normalize_role_model_to_assistant() {
    assert_eq!(normalize_role("model"), Some(IrRole::Assistant));
}

#[test]
fn normalize_role_function_to_tool() {
    assert_eq!(normalize_role("function"), Some(IrRole::Tool));
}

#[test]
fn normalize_role_developer_to_system() {
    assert_eq!(normalize_role("developer"), Some(IrRole::System));
}

#[test]
fn normalize_role_unknown_returns_none() {
    assert_eq!(normalize_role("narrator"), None);
    assert_eq!(normalize_role("ai"), None);
    assert_eq!(normalize_role(""), None);
}

#[test]
fn dedup_system_merges_scattered_system_messages() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "First instruction."))
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::text(IrRole::System, "Second instruction."));
    let result = dedup_system(&conv);
    let sys_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::System)
        .collect();
    assert_eq!(sys_msgs.len(), 1);
    assert_eq!(
        sys_msgs[0].text_content(),
        "First instruction.\nSecond instruction."
    );
    // System should be first
    assert_eq!(result.messages[0].role, IrRole::System);
}

#[test]
fn trim_text_removes_whitespace() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "  hello  "))
        .push(IrMessage::text(IrRole::Assistant, "\n  world\t  "));
    let result = trim_text(&conv);
    assert_eq!(result.messages[0].text_content(), "hello");
    assert_eq!(result.messages[1].text_content(), "world");
}

#[test]
fn trim_text_leaves_non_text_blocks_untouched() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "  padded  ".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: serde_json::json!({}),
            },
        ],
    )]);
    let result = trim_text(&conv);
    assert_eq!(result.messages[0].text_content(), "padded");
    assert!(matches!(
        &result.messages[0].content[1],
        IrContentBlock::ToolUse { .. }
    ));
}

#[test]
fn strip_empty_removes_content_free_messages() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::new(IrRole::Assistant, vec![]))
        .push(IrMessage::text(IrRole::Assistant, "World"));
    let result = strip_empty(&conv);
    assert_eq!(result.len(), 2);
    assert_eq!(result.messages[0].text_content(), "Hello");
    assert_eq!(result.messages[1].text_content(), "World");
}

#[test]
fn merge_adjacent_text_combines_consecutive_text_blocks() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "Hello ".into(),
            },
            IrContentBlock::Text {
                text: "World".into(),
            },
        ],
    )]);
    let result = merge_adjacent_text(&conv);
    assert_eq!(result.messages[0].content.len(), 1);
    assert_eq!(result.messages[0].text_content(), "Hello World");
}

#[test]
fn merge_adjacent_text_preserves_non_text_boundary() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Part1".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "f".into(),
                input: serde_json::json!({}),
            },
            IrContentBlock::Text {
                text: "Part2".into(),
            },
        ],
    )]);
    let result = merge_adjacent_text(&conv);
    // 3 blocks remain: text, tool_use, text — no merging across tool_use
    assert_eq!(result.messages[0].content.len(), 3);
}

#[test]
fn full_normalize_pipeline_applies_all_passes() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "  First  "))
        .push(IrMessage::text(IrRole::User, " hi "))
        .push(IrMessage::new(IrRole::Assistant, vec![]))
        .push(IrMessage::text(IrRole::System, " Second "));
    let result = normalize(&conv);
    // System messages deduped, empty stripped, text trimmed
    assert_eq!(
        result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::System)
            .count(),
        1
    );
    // dedup_system joins with "\n" preserving inner whitespace; trim only strips outer edges
    assert_eq!(result.messages[0].text_content(), "First  \n Second");
    assert!(result
        .messages
        .iter()
        .all(|m| !m.content.is_empty() || m.role == IrRole::System));
}

#[test]
fn normalize_pipeline_is_idempotent() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "  sys  "))
        .push(IrMessage::text(IrRole::User, " hi "))
        .push(IrMessage::text(IrRole::System, " extra "));
    let once = normalize(&conv);
    let twice = normalize(&once);
    assert_eq!(once, twice);
}

#[test]
fn extract_system_separates_system_messages() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Be nice."))
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::System, "Be brief."));
    let (sys, rest) = extract_system(&conv);
    assert_eq!(sys.unwrap(), "Be nice.\nBe brief.");
    assert_eq!(rest.len(), 1);
    assert!(rest.messages.iter().all(|m| m.role != IrRole::System));
}

#[test]
fn strip_metadata_removes_all_with_empty_keep() {
    let mut meta = std::collections::BTreeMap::new();
    meta.insert("key".to_string(), serde_json::json!("val"));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata: meta,
    };
    let conv = IrConversation::from_messages(vec![msg]);
    let result = strip_metadata(&conv, &[]);
    assert!(result.messages[0].metadata.is_empty());
}

#[test]
fn sort_tools_alphabetically() {
    let mut tools = vec![
        IrToolDefinition {
            name: "zebra".into(),
            description: "z".into(),
            parameters: serde_json::json!({}),
        },
        IrToolDefinition {
            name: "apple".into(),
            description: "a".into(),
            parameters: serde_json::json!({}),
        },
    ];
    sort_tools(&mut tools);
    assert_eq!(tools[0].name, "apple");
    assert_eq!(tools[1].name, "zebra");
}

#[test]
fn normalize_tool_schemas_adds_missing_type() {
    let tools = vec![IrToolDefinition {
        name: "search".into(),
        description: "Search things".into(),
        parameters: serde_json::json!({"properties": {"q": {"type": "string"}}}),
    }];
    let normalized = normalize_tool_schemas(&tools);
    assert_eq!(normalized[0].parameters["type"], "object");
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. IR LOWERING TESTS
// ═══════════════════════════════════════════════════════════════════════════

fn sample_ir_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hi"))
        .push(IrMessage::text(IrRole::Assistant, "Hello!"))
}

fn sample_ir_tools() -> Vec<IrToolDefinition> {
    vec![IrToolDefinition {
        name: "calc".into(),
        description: "Math evaluator".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"expr": {"type": "string"}}
        }),
    }]
}

#[test]
fn lower_to_openai_produces_messages_array() {
    let lowered = lower_to_openai(&sample_ir_conv(), &[]);
    let msgs = lowered["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[2]["role"], "assistant");
}

#[test]
fn lower_to_openai_includes_tools_field() {
    let lowered = lower_to_openai(&sample_ir_conv(), &sample_ir_tools());
    let tools = lowered["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["function"]["name"], "calc");
}

#[test]
fn lower_to_openai_omits_tools_when_empty() {
    let lowered = lower_to_openai(&sample_ir_conv(), &[]);
    assert!(lowered.get("tools").is_none());
}

#[test]
fn lower_to_claude_extracts_system() {
    let lowered = lower_to_claude(&sample_ir_conv(), &[]);
    assert_eq!(lowered["system"], "You are helpful.");
    let msgs = lowered["messages"].as_array().unwrap();
    assert!(msgs.iter().all(|m| m["role"] != "system"));
}

#[test]
fn lower_to_claude_uses_input_schema_for_tools() {
    let lowered = lower_to_claude(&sample_ir_conv(), &sample_ir_tools());
    let tool = &lowered["tools"][0];
    assert!(tool.get("input_schema").is_some());
    assert!(tool.get("parameters").is_none());
}

#[test]
fn lower_to_claude_content_blocks_are_typed() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text { text: "hi".into() },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "search".into(),
                input: serde_json::json!({}),
            },
        ],
    ));
    let lowered = lower_to_claude(&conv, &[]);
    let content = lowered["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "tool_use");
}

#[test]
fn lower_to_gemini_uses_system_instruction() {
    let lowered = lower_to_gemini(&sample_ir_conv(), &[]);
    assert_eq!(
        lowered["system_instruction"]["parts"][0]["text"],
        "You are helpful."
    );
}

#[test]
fn lower_to_gemini_uses_model_role() {
    let lowered = lower_to_gemini(&sample_ir_conv(), &[]);
    let contents = lowered["contents"].as_array().unwrap();
    assert_eq!(contents[1]["role"], "model");
}

#[test]
fn lower_to_gemini_uses_function_declarations() {
    let lowered = lower_to_gemini(&sample_ir_conv(), &sample_ir_tools());
    let decls = lowered["tools"][0]["function_declarations"]
        .as_array()
        .unwrap();
    assert_eq!(decls[0]["name"], "calc");
}

#[test]
fn lower_to_gemini_skips_thinking_blocks() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking { text: "hmm".into() },
            IrContentBlock::Text {
                text: "answer".into(),
            },
        ],
    ));
    let lowered = lower_to_gemini(&conv, &[]);
    let parts = lowered["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0]["text"], "answer");
}

#[test]
fn lower_kimi_codex_copilot_match_openai() {
    let conv = sample_ir_conv();
    let tools = sample_ir_tools();
    let openai = lower_to_openai(&conv, &tools);
    assert_eq!(lower_to_kimi(&conv, &tools), openai);
    assert_eq!(lower_to_codex(&conv, &tools), openai);
    assert_eq!(lower_to_copilot(&conv, &tools), openai);
}

#[test]
fn lower_for_dialect_dispatches_correctly() {
    let conv = sample_ir_conv();
    let tools = sample_ir_tools();
    for dialect in Dialect::all() {
        let lowered = lower_for_dialect(*dialect, &conv, &tools);
        assert!(lowered.is_object(), "{dialect}: should produce JSON object");
    }
}

#[test]
fn ir_role_openai_mappings() {
    assert_eq!(
        ir_role_to_dialect(IrRole::System, Dialect::OpenAi),
        "system"
    );
    assert_eq!(ir_role_to_dialect(IrRole::User, Dialect::OpenAi), "user");
    assert_eq!(
        ir_role_to_dialect(IrRole::Assistant, Dialect::OpenAi),
        "assistant"
    );
    assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::OpenAi), "tool");
}

#[test]
fn ir_role_claude_tool_becomes_user() {
    assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Claude), "user");
}

#[test]
fn ir_role_gemini_assistant_becomes_model() {
    assert_eq!(
        ir_role_to_dialect(IrRole::Assistant, Dialect::Gemini),
        "model"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. CROSS-SDK VIA IR TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cross_openai_to_claude_text() {
    let openai_msgs = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("Be helpful.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Hi there!".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_lowering::to_ir(&openai_msgs);
    let claude_msgs = claude_lowering::from_ir(&ir);
    // Claude skips system messages in from_ir
    assert_eq!(claude_msgs.len(), 2);
    assert_eq!(claude_msgs[0].role, "user");
    assert_eq!(claude_msgs[0].content, "Hello");
    assert_eq!(claude_msgs[1].role, "assistant");
    assert_eq!(claude_msgs[1].content, "Hi there!");
    // System is extracted separately
    let sys = claude_lowering::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("Be helpful."));
}

#[test]
fn cross_claude_to_openai_text() {
    let claude_msgs = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "Hello".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "Hi!".into(),
        },
    ];
    let ir = claude_lowering::to_ir(&claude_msgs, Some("Be brief."));
    let openai_msgs = openai_lowering::from_ir(&ir);
    assert_eq!(openai_msgs.len(), 3); // system + user + assistant
    assert_eq!(openai_msgs[0].role, "system");
    assert_eq!(openai_msgs[0].content.as_deref(), Some("Be brief."));
    assert_eq!(openai_msgs[1].role, "user");
    assert_eq!(openai_msgs[2].role, "assistant");
}

#[test]
fn cross_openai_to_gemini_text() {
    let openai_msgs = vec![
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Hi!".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_lowering::to_ir(&openai_msgs);
    let gemini_contents = gemini_lowering::from_ir(&ir);
    assert_eq!(gemini_contents.len(), 2);
    assert_eq!(gemini_contents[0].role, "user");
    assert_eq!(gemini_contents[1].role, "model");
    assert!(matches!(
        &gemini_contents[1].parts[0],
        GeminiPart::Text(t) if t == "Hi!"
    ));
}

#[test]
fn cross_gemini_to_openai_text() {
    let gemini_contents = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hello".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("Hi!".into())],
        },
    ];
    let ir = gemini_lowering::to_ir(&gemini_contents, None);
    let openai_msgs = openai_lowering::from_ir(&ir);
    assert_eq!(openai_msgs.len(), 2);
    assert_eq!(openai_msgs[0].role, "user");
    assert_eq!(openai_msgs[1].role, "assistant");
    assert_eq!(openai_msgs[1].content.as_deref(), Some("Hi!"));
}

#[test]
fn cross_kimi_to_claude_text() {
    let kimi_msgs = vec![
        KimiMessage {
            role: "system".into(),
            content: Some("Be smart.".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "user".into(),
            content: Some("Question?".into()),
            tool_call_id: None,
            tool_calls: None,
        },
    ];
    let ir = kimi_lowering::to_ir(&kimi_msgs);
    let sys = claude_lowering::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("Be smart."));
    let claude_msgs = claude_lowering::from_ir(&ir);
    assert_eq!(claude_msgs.len(), 1);
    assert_eq!(claude_msgs[0].content, "Question?");
}

#[test]
fn cross_copilot_to_openai_text() {
    let copilot_msgs = vec![
        CopilotMessage {
            role: "system".into(),
            content: "You are Copilot.".into(),
            name: None,
            copilot_references: vec![],
        },
        CopilotMessage {
            role: "user".into(),
            content: "Help me".into(),
            name: None,
            copilot_references: vec![],
        },
    ];
    let ir = copilot_lowering::to_ir(&copilot_msgs);
    let openai_msgs = openai_lowering::from_ir(&ir);
    assert_eq!(openai_msgs.len(), 2);
    assert_eq!(openai_msgs[0].role, "system");
    assert_eq!(openai_msgs[0].content.as_deref(), Some("You are Copilot."));
    assert_eq!(openai_msgs[1].role, "user");
}

#[test]
fn cross_openai_to_kimi_text() {
    let openai_msgs = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("Be helpful.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_lowering::to_ir(&openai_msgs);
    let kimi_msgs = kimi_lowering::from_ir(&ir);
    assert_eq!(kimi_msgs.len(), 2);
    assert_eq!(kimi_msgs[0].role, "system");
    assert_eq!(kimi_msgs[0].content.as_deref(), Some("Be helpful."));
    assert_eq!(kimi_msgs[1].role, "user");
}

#[test]
fn cross_claude_to_gemini_text() {
    let claude_msgs = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "Hello".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "Hi!".into(),
        },
    ];
    let ir = claude_lowering::to_ir(&claude_msgs, None);
    let gemini_contents = gemini_lowering::from_ir(&ir);
    assert_eq!(gemini_contents.len(), 2);
    assert_eq!(gemini_contents[0].role, "user");
    assert_eq!(gemini_contents[1].role, "model");
}

#[test]
fn cross_gemini_to_kimi_text() {
    let gemini_contents = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hello".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("Hi!".into())],
        },
    ];
    let ir = gemini_lowering::to_ir(&gemini_contents, None);
    let kimi_msgs = kimi_lowering::from_ir(&ir);
    assert_eq!(kimi_msgs.len(), 2);
    assert_eq!(kimi_msgs[0].role, "user");
    assert_eq!(kimi_msgs[1].role, "assistant");
    assert_eq!(kimi_msgs[1].content.as_deref(), Some("Hi!"));
}

#[test]
fn cross_openai_tool_call_to_claude_via_ir() {
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
    let ir = openai_lowering::to_ir(&openai_msgs);
    // Verify IR has ToolUse
    assert!(matches!(
        &ir.messages[0].content[0],
        IrContentBlock::ToolUse { name, .. } if name == "read_file"
    ));
    // Convert to Claude
    let claude_msgs = claude_lowering::from_ir(&ir);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&claude_msgs[0].content).unwrap();
    assert!(matches!(
        &parsed[0],
        ClaudeContentBlock::ToolUse { name, .. } if name == "read_file"
    ));
}

#[test]
fn cross_codex_to_openai_via_ir() {
    let items = vec![CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText {
            text: "Done!".into(),
        }],
    }];
    let ir = codex_lowering::to_ir(&items);
    let openai_msgs = openai_lowering::from_ir(&ir);
    assert_eq!(openai_msgs.len(), 1);
    assert_eq!(openai_msgs[0].role, "assistant");
    assert_eq!(openai_msgs[0].content.as_deref(), Some("Done!"));
}

#[test]
fn cross_openai_to_copilot_via_ir() {
    let openai_msgs = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("Be helpful.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_lowering::to_ir(&openai_msgs);
    let copilot_msgs = copilot_lowering::from_ir(&ir);
    assert_eq!(copilot_msgs.len(), 2);
    assert_eq!(copilot_msgs[0].role, "system");
    assert_eq!(copilot_msgs[0].content, "Be helpful.");
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. ADDITIONAL EDGE CASES AND COMPREHENSIVE COVERAGE
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_conversation_empty_roundtrip_all_sdks() {
    let empty_ir = IrConversation::new();
    assert!(openai_lowering::from_ir(&empty_ir).is_empty());
    assert!(claude_lowering::from_ir(&empty_ir).is_empty());
    assert!(gemini_lowering::from_ir(&empty_ir).is_empty());
    assert!(codex_lowering::from_ir(&empty_ir).is_empty());
    assert!(kimi_lowering::from_ir(&empty_ir).is_empty());
    assert!(copilot_lowering::from_ir(&empty_ir).is_empty());
}

#[test]
fn lower_all_dialects_no_tools_omits_tools() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    for dialect in Dialect::all() {
        let lowered = lower_for_dialect(*dialect, &conv, &[]);
        assert!(
            lowered.get("tools").is_none(),
            "{dialect}: tools should be absent when empty"
        );
    }
}

#[test]
fn lower_openai_tool_result_produces_tool_message() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "call_1".into(),
            content: vec![IrContentBlock::Text {
                text: "output".into(),
            }],
            is_error: false,
        }],
    ));
    let lowered = lower_to_openai(&conv, &[]);
    let msg = &lowered["messages"][0];
    assert_eq!(msg["role"], "tool");
    assert_eq!(msg["tool_call_id"], "call_1");
    assert_eq!(msg["content"], "output");
}

#[test]
fn lower_claude_thinking_block() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "Let me think...".into(),
            },
            IrContentBlock::Text {
                text: "The answer is 42.".into(),
            },
        ],
    ));
    let lowered = lower_to_claude(&conv, &[]);
    let content = lowered["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "thinking");
    assert_eq!(content[1]["type"], "text");
}

#[test]
fn lower_gemini_image_block() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "base64data".into(),
        }],
    ));
    let lowered = lower_to_gemini(&conv, &[]);
    let part = &lowered["contents"][0]["parts"][0];
    assert_eq!(part["inline_data"]["mime_type"], "image/png");
    assert_eq!(part["inline_data"]["data"], "base64data");
}

#[test]
fn openai_multiple_tool_calls_in_single_message_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![
            OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read".into(),
                    arguments: "{}".into(),
                },
            },
            OpenAIToolCall {
                id: "c2".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "write".into(),
                    arguments: "{}".into(),
                },
            },
        ]),
        tool_call_id: None,
    }];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 2);
}

#[test]
fn claude_image_block_roundtrip_via_ir() {
    use abp_claude_sdk::dialect::ClaudeImageSource;
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/jpeg".into(),
            data: "abc123".into(),
        },
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = claude_lowering::to_ir(&msgs, None);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::Image { media_type, .. } if media_type == "image/jpeg"
    ));
}

#[test]
fn gemini_inline_data_roundtrip() {
    use abp_gemini_sdk::dialect::GeminiInlineData;
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "xyz".into(),
        })],
    }];
    let conv = gemini_lowering::to_ir(&contents, None);
    let back = gemini_lowering::from_ir(&conv);
    assert!(matches!(
        &back[0].parts[0],
        GeminiPart::InlineData(d) if d.mime_type == "image/png" && d.data == "xyz"
    ));
}

#[test]
fn codex_reasoning_roundtrip() {
    use abp_codex_sdk::dialect::ReasoningSummary;
    let items = vec![CodexResponseItem::Reasoning {
        summary: vec![ReasoningSummary {
            text: "thinking step".into(),
        }],
    }];
    let conv = codex_lowering::to_ir(&items);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::Thinking { text } if text == "thinking step"
    ));
    let back = codex_lowering::from_ir(&conv);
    assert!(matches!(
        &back[0],
        CodexResponseItem::Reasoning { summary } if summary[0].text == "thinking step"
    ));
}

#[test]
fn normalize_then_lower_preserves_semantics() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "  Be helpful.  "))
        .push(IrMessage::text(IrRole::User, " Hi "))
        .push(IrMessage::text(IrRole::System, " Extra. "))
        .push(IrMessage::text(IrRole::Assistant, " Hello! "));
    let normalized = normalize(&conv);
    let lowered = lower_to_openai(&normalized, &[]);
    let msgs = lowered["messages"].as_array().unwrap();
    // System should be deduped and trimmed
    // dedup_system joins with "\n" preserving inner whitespace; trim only strips outer edges
    assert_eq!(msgs[0]["content"], "Be helpful.  \n Extra.");
    assert_eq!(msgs[1]["content"], "Hi");
    assert_eq!(msgs[2]["content"], "Hello!");
}

#[test]
fn cross_sdk_tool_flow_openai_to_kimi() {
    let openai_msgs = vec![
        OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_x".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"test"}"#.into(),
                },
            }]),
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "tool".into(),
            content: Some("result data".into()),
            tool_calls: None,
            tool_call_id: Some("call_x".into()),
        },
    ];
    let ir = openai_lowering::to_ir(&openai_msgs);
    let kimi_msgs = kimi_lowering::from_ir(&ir);
    assert_eq!(kimi_msgs[0].role, "assistant");
    assert!(kimi_msgs[0].tool_calls.is_some());
    assert_eq!(kimi_msgs[1].role, "tool");
    assert_eq!(kimi_msgs[1].tool_call_id.as_deref(), Some("call_x"));
}

#[test]
fn all_user_roles_map_to_user_across_dialects() {
    for dialect in Dialect::all() {
        assert_eq!(
            ir_role_to_dialect(IrRole::User, *dialect),
            "user",
            "{dialect}: User should always map to 'user'"
        );
    }
}
