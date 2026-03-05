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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! IR fidelity tests — verifies that the Intermediate Representation layer
//! faithfully normalizes, roundtrips, and tracks information loss across
//! all supported SDK dialects.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_dialect::Dialect;
use abp_mapping::{
    Fidelity, MappingRegistry, MappingRule, features, known_rules, validate_mapping,
};
use serde_json::json;

// ── SDK dialect imports ─────────────────────────────────────────────────
use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
use abp_openai_sdk::lowering as openai_low;

use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeImageSource, ClaudeMessage};
use abp_claude_sdk::lowering as claude_low;

use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
use abp_gemini_sdk::lowering as gemini_low;

use abp_codex_sdk::dialect::{
    CodexContentPart, CodexInputItem, CodexResponseItem, CodexUsage, ReasoningSummary,
};
use abp_codex_sdk::lowering as codex_low;

use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall, KimiUsage};
use abp_kimi_sdk::lowering as kimi_low;

use abp_copilot_sdk::dialect::{CopilotMessage, CopilotReference, CopilotReferenceType};
use abp_copilot_sdk::lowering as copilot_low;

// ═══════════════════════════════════════════════════════════════════════════
// §1  IR message type construction (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_construct_user_text() {
    let msg = IrMessage::text(IrRole::User, "ping");
    assert_eq!(msg.role, IrRole::User);
    assert_eq!(msg.text_content(), "ping");
    assert!(msg.is_text_only());
    assert!(msg.metadata.is_empty());
}

#[test]
fn fidelity_construct_system_text() {
    let msg = IrMessage::text(IrRole::System, "You are a helpful assistant.");
    assert_eq!(msg.role, IrRole::System);
    assert_eq!(msg.text_content(), "You are a helpful assistant.");
}

#[test]
fn fidelity_construct_assistant_text() {
    let msg = IrMessage::text(IrRole::Assistant, "Understood.");
    assert_eq!(msg.role, IrRole::Assistant);
    assert!(msg.is_text_only());
}

#[test]
fn fidelity_construct_tool_text() {
    let msg = IrMessage::text(IrRole::Tool, "42");
    assert_eq!(msg.role, IrRole::Tool);
    assert_eq!(msg.text_content(), "42");
}

#[test]
fn fidelity_construct_multiblock_message() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Searching…".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "grep".into(),
                input: json!({"pattern": "fn main"}),
            },
        ],
    );
    assert!(!msg.is_text_only());
    assert_eq!(msg.text_content(), "Searching…");
    assert_eq!(msg.tool_use_blocks().len(), 1);
}

#[test]
fn fidelity_construct_tool_result_block() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "t1".into(),
        content: vec![IrContentBlock::Text {
            text: "done".into(),
        }],
        is_error: false,
    };
    let msg = IrMessage::new(IrRole::Tool, vec![block]);
    assert!(!msg.is_text_only());
    assert!(msg.tool_use_blocks().is_empty());
}

#[test]
fn fidelity_construct_image_block() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "iVBOR…".into(),
        }],
    );
    assert!(!msg.is_text_only());
    assert!(msg.text_content().is_empty());
}

#[test]
fn fidelity_construct_thinking_block() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "Step 1: analyse input.".into(),
        }],
    );
    assert!(!msg.is_text_only());
}

#[test]
fn fidelity_construct_message_with_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("vendor".into(), json!("openai"));
    meta.insert("model".into(), json!("gpt-4o"));
    let msg = IrMessage {
        role: IrRole::Assistant,
        content: vec![IrContentBlock::Text { text: "ok".into() }],
        metadata: meta,
    };
    assert_eq!(msg.metadata.len(), 2);
    assert_eq!(msg.metadata["vendor"], json!("openai"));
}

#[test]
fn fidelity_construct_empty_message() {
    let msg = IrMessage::new(IrRole::User, vec![]);
    assert!(msg.content.is_empty());
    assert!(msg.is_text_only()); // vacuously true
    assert!(msg.text_content().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  IR role mapping (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_role_user_serde() {
    let json = serde_json::to_string(&IrRole::User).unwrap();
    assert_eq!(json, r#""user""#);
    let back: IrRole = serde_json::from_str(&json).unwrap();
    assert_eq!(back, IrRole::User);
}

#[test]
fn fidelity_role_assistant_serde() {
    let json = serde_json::to_string(&IrRole::Assistant).unwrap();
    assert_eq!(json, r#""assistant""#);
}

#[test]
fn fidelity_role_system_serde() {
    let json = serde_json::to_string(&IrRole::System).unwrap();
    assert_eq!(json, r#""system""#);
}

#[test]
fn fidelity_role_tool_serde() {
    let json = serde_json::to_string(&IrRole::Tool).unwrap();
    assert_eq!(json, r#""tool""#);
}

#[test]
fn fidelity_role_all_variants_roundtrip() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let json = serde_json::to_string(&role).unwrap();
        let back: IrRole = serde_json::from_str(&json).unwrap();
        assert_eq!(role, back);
    }
}

#[test]
fn fidelity_openai_role_user_maps_correctly() {
    let msgs = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("x".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    assert_eq!(openai_low::to_ir(&msgs).messages[0].role, IrRole::User);
}

#[test]
fn fidelity_gemini_model_role_maps_to_assistant() {
    let c = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text("hi".into())],
    }];
    assert_eq!(
        gemini_low::to_ir(&c, None).messages[0].role,
        IrRole::Assistant
    );
}

#[test]
fn fidelity_openai_unknown_role_defaults_user() {
    let msgs = vec![OpenAIMessage {
        role: "developer".into(),
        content: Some("x".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    assert_eq!(openai_low::to_ir(&msgs).messages[0].role, IrRole::User);
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  IR content types serde (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_content_text_json_tag() {
    let b = IrContentBlock::Text { text: "hi".into() };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["type"], "text");
}

#[test]
fn fidelity_content_image_json_tag() {
    let b = IrContentBlock::Image {
        media_type: "image/jpeg".into(),
        data: "abc".into(),
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["type"], "image");
}

#[test]
fn fidelity_content_tool_use_json_tag() {
    let b = IrContentBlock::ToolUse {
        id: "t".into(),
        name: "f".into(),
        input: json!({}),
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["type"], "tool_use");
}

#[test]
fn fidelity_content_tool_result_json_tag() {
    let b = IrContentBlock::ToolResult {
        tool_use_id: "t".into(),
        content: vec![],
        is_error: false,
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["type"], "tool_result");
}

#[test]
fn fidelity_content_thinking_json_tag() {
    let b = IrContentBlock::Thinking { text: "hmm".into() };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["type"], "thinking");
}

#[test]
fn fidelity_content_text_roundtrip() {
    let b = IrContentBlock::Text {
        text: "hello world 🦀".into(),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: IrContentBlock = serde_json::from_str(&j).unwrap();
    assert_eq!(b, back);
}

#[test]
fn fidelity_content_tool_use_roundtrip_complex_input() {
    let input = json!({"cmd": "ls", "args": ["-la"], "opts": {"recursive": true}});
    let b = IrContentBlock::ToolUse {
        id: "c1".into(),
        name: "shell".into(),
        input: input.clone(),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: IrContentBlock = serde_json::from_str(&j).unwrap();
    assert_eq!(b, back);
}

#[test]
fn fidelity_content_tool_result_nested_blocks() {
    let b = IrContentBlock::ToolResult {
        tool_use_id: "c1".into(),
        content: vec![
            IrContentBlock::Text {
                text: "line1\n".into(),
            },
            IrContentBlock::Text {
                text: "line2\n".into(),
            },
        ],
        is_error: false,
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: IrContentBlock = serde_json::from_str(&j).unwrap();
    assert_eq!(b, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  IR tool/function representation (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_tool_def_basic() {
    let def = IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file by path".into(),
        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
    };
    assert_eq!(def.name, "read_file");
}

#[test]
fn fidelity_tool_def_serde_roundtrip() {
    let def = IrToolDefinition {
        name: "web_search".into(),
        description: "Search the web".into(),
        parameters: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
    };
    let j = serde_json::to_string(&def).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&j).unwrap();
    assert_eq!(def, back);
}

#[test]
fn fidelity_tool_def_empty_params() {
    let def = IrToolDefinition {
        name: "noop".into(),
        description: "No-op".into(),
        parameters: json!({"type": "object"}),
    };
    let j = serde_json::to_string(&def).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&j).unwrap();
    assert_eq!(def, back);
}

#[test]
fn fidelity_tool_def_complex_schema() {
    let def = IrToolDefinition {
        name: "edit".into(),
        description: "Edit file".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old_str": {"type": "string"},
                "new_str": {"type": "string"}
            },
            "required": ["path", "old_str", "new_str"],
            "additionalProperties": false
        }),
    };
    let j = serde_json::to_string(&def).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&j).unwrap();
    assert_eq!(def, back);
}

#[test]
fn fidelity_tool_use_null_input() {
    let b = IrContentBlock::ToolUse {
        id: "c1".into(),
        name: "noop".into(),
        input: serde_json::Value::Null,
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: IrContentBlock = serde_json::from_str(&j).unwrap();
    match back {
        IrContentBlock::ToolUse { input, .. } => assert!(input.is_null()),
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn fidelity_tool_use_deeply_nested_input() {
    let input = json!({"a": {"b": {"c": {"d": {"e": 42}}}}});
    let b = IrContentBlock::ToolUse {
        id: "c1".into(),
        name: "deep".into(),
        input: input.clone(),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: IrContentBlock = serde_json::from_str(&j).unwrap();
    assert_eq!(b, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  IR → SDK lowering for each dialect (7 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_lower_to_openai() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "q"),
        IrMessage::text(IrRole::Assistant, "a"),
    ]);
    let msgs = openai_low::from_ir(&conv);
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].role, "system");
    assert_eq!(msgs[1].role, "user");
    assert_eq!(msgs[2].role, "assistant");
}

#[test]
fn fidelity_lower_to_claude_skips_system() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "q"),
    ]);
    let msgs = claude_low::from_ir(&conv);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].role, "user");
    let sys = claude_low::extract_system_prompt(&conv);
    assert_eq!(sys.as_deref(), Some("sys"));
}

#[test]
fn fidelity_lower_to_gemini_skips_system() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "q"),
    ]);
    let contents = gemini_low::from_ir(&conv);
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].role, "user");
    let si = gemini_low::extract_system_instruction(&conv);
    assert!(si.is_some());
}

#[test]
fn fidelity_lower_to_codex_only_assistant() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "q"),
        IrMessage::text(IrRole::Assistant, "a"),
    ]);
    let items = codex_low::from_ir(&conv);
    assert_eq!(items.len(), 1);
    match &items[0] {
        CodexResponseItem::Message { role, .. } => assert_eq!(role, "assistant"),
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn fidelity_lower_to_kimi_preserves_system() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "q"),
    ]);
    let msgs = kimi_low::from_ir(&conv);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "system");
}

#[test]
fn fidelity_lower_to_copilot_preserves_system() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "q"),
    ]);
    let msgs = copilot_low::from_ir(&conv);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "system");
}

#[test]
fn fidelity_lower_tool_call_to_openai() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        }],
    )]);
    let msgs = openai_low::from_ir(&conv);
    let tc = msgs[0].tool_calls.as_ref().unwrap();
    assert_eq!(tc[0].id, "call_1");
    assert_eq!(tc[0].function.name, "search");
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  SDK → IR lifting for each dialect (7 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_lift_openai_user() {
    let msgs = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("hi".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = openai_low::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "hi");
}

#[test]
fn fidelity_lift_claude_with_system() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "hey".into(),
    }];
    let conv = claude_low::to_ir(&msgs, Some("Be kind"));
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "Be kind");
    assert_eq!(conv.messages[1].text_content(), "hey");
}

#[test]
fn fidelity_lift_gemini_with_system_instruction() {
    let si = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Be brief.".into())],
    };
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hi".into())],
    }];
    let conv = gemini_low::to_ir(&contents, Some(&si));
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "Be brief.");
}

#[test]
fn fidelity_lift_codex_input_items() {
    let items = vec![
        CodexInputItem::Message {
            role: "system".into(),
            content: "inst".into(),
        },
        CodexInputItem::Message {
            role: "user".into(),
            content: "hello".into(),
        },
    ];
    let conv = codex_low::input_to_ir(&items);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn fidelity_lift_kimi_tool_call() {
    let msgs = vec![KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "kc1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "web_search".into(),
                arguments: r#"{"q":"test"}"#.into(),
            },
        }]),
    }];
    let conv = kimi_low::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { name, .. } => assert_eq!(name, "web_search"),
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn fidelity_lift_copilot_with_references() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "check".into(),
        name: None,
        copilot_references: vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "f1".into(),
            data: json!({"path": "main.rs"}),
            metadata: None,
        }],
    }];
    let conv = copilot_low::to_ir(&msgs);
    assert!(conv.messages[0].metadata.contains_key("copilot_references"));
}

#[test]
fn fidelity_lift_claude_image_base64() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/png".into(),
            data: "abc123".into(),
        },
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = claude_low::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "abc123");
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Lossless roundtrip: SDK → IR → same SDK (12 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_rt_openai_text() {
    let orig = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("sys".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("q".into()),
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
    let back = openai_low::from_ir(&openai_low::to_ir(&orig));
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].content.as_deref(), Some("sys"));
    assert_eq!(back[1].content.as_deref(), Some("q"));
    assert_eq!(back[2].content.as_deref(), Some("a"));
}

#[test]
fn fidelity_rt_openai_tool_call() {
    let orig = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_42".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "ls".into(),
                arguments: r#"{"dir":"/"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let back = openai_low::from_ir(&openai_low::to_ir(&orig));
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_42");
    assert_eq!(tc.function.name, "ls");
}

#[test]
fn fidelity_rt_openai_tool_result() {
    let orig = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("file data".into()),
        tool_calls: None,
        tool_call_id: Some("call_42".into()),
    }];
    let back = openai_low::from_ir(&openai_low::to_ir(&orig));
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_42"));
    assert_eq!(back[0].content.as_deref(), Some("file data"));
}

#[test]
fn fidelity_rt_claude_text() {
    let orig = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "q".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "a".into(),
        },
    ];
    let back = claude_low::from_ir(&claude_low::to_ir(&orig, None));
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].content, "q");
    assert_eq!(back[1].content, "a");
}

#[test]
fn fidelity_rt_gemini_text() {
    let orig = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("q".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("a".into())],
        },
    ];
    let back = gemini_low::from_ir(&gemini_low::to_ir(&orig, None));
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[1].role, "model");
}

#[test]
fn fidelity_rt_gemini_function_call() {
    let orig = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "search".into(),
            args: json!({"q": "rust"}),
        }],
    }];
    let back = gemini_low::from_ir(&gemini_low::to_ir(&orig, None));
    match &back[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"q": "rust"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn fidelity_rt_kimi_text() {
    let orig = vec![
        KimiMessage {
            role: "system".into(),
            content: Some("sys".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "user".into(),
            content: Some("q".into()),
            tool_call_id: None,
            tool_calls: None,
        },
    ];
    let back = kimi_low::from_ir(&kimi_low::to_ir(&orig));
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content.as_deref(), Some("sys"));
    assert_eq!(back[1].content.as_deref(), Some("q"));
}

#[test]
fn fidelity_rt_kimi_tool_call() {
    let orig = vec![KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "kc1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"x"}"#.into(),
            },
        }]),
    }];
    let back = kimi_low::from_ir(&kimi_low::to_ir(&orig));
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "kc1");
    assert_eq!(tc.function.name, "search");
}

#[test]
fn fidelity_rt_copilot_text() {
    let orig = vec![CopilotMessage {
        role: "user".into(),
        content: "hi".into(),
        name: Some("alice".into()),
        copilot_references: vec![],
    }];
    let back = copilot_low::from_ir(&copilot_low::to_ir(&orig));
    assert_eq!(back[0].content, "hi");
    assert_eq!(back[0].name.as_deref(), Some("alice"));
}

#[test]
fn fidelity_rt_copilot_references() {
    let orig = vec![CopilotMessage {
        role: "user".into(),
        content: "look".into(),
        name: None,
        copilot_references: vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "f0".into(),
            data: json!({"path": "main.rs"}),
            metadata: None,
        }],
    }];
    let back = copilot_low::from_ir(&copilot_low::to_ir(&orig));
    assert_eq!(back[0].copilot_references.len(), 1);
    assert_eq!(back[0].copilot_references[0].id, "f0");
}

#[test]
fn fidelity_rt_codex_response_items() {
    let orig = vec![
        CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "done".into(),
            }],
        },
        CodexResponseItem::FunctionCall {
            id: "fc1".into(),
            call_id: None,
            name: "shell".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
        },
        CodexResponseItem::FunctionCallOutput {
            call_id: "fc1".into(),
            output: "file.txt".into(),
        },
    ];
    let back = codex_low::from_ir(&codex_low::to_ir(&orig));
    assert_eq!(back.len(), 3);
    assert!(matches!(&back[0], CodexResponseItem::Message { .. }));
    assert!(matches!(&back[1], CodexResponseItem::FunctionCall { .. }));
    assert!(matches!(
        &back[2],
        CodexResponseItem::FunctionCallOutput { .. }
    ));
}

#[test]
fn fidelity_rt_codex_reasoning() {
    let orig = vec![CodexResponseItem::Reasoning {
        summary: vec![ReasoningSummary {
            text: "Step 1".into(),
        }],
    }];
    let back = codex_low::from_ir(&codex_low::to_ir(&orig));
    match &back[0] {
        CodexResponseItem::Reasoning { summary } => assert_eq!(summary[0].text, "Step 1"),
        other => panic!("expected Reasoning, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  Cross-dialect mapping: SDK_A → IR → SDK_B (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_cross_openai_to_claude() {
    let orig = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("sys".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("q".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_low::to_ir(&orig);
    let claude = claude_low::from_ir(&ir);
    assert_eq!(claude.len(), 1); // system extracted
    assert_eq!(claude[0].role, "user");
    assert_eq!(
        claude_low::extract_system_prompt(&ir).as_deref(),
        Some("sys")
    );
}

#[test]
fn fidelity_cross_claude_to_openai() {
    let orig = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "q".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "a".into(),
        },
    ];
    let ir = claude_low::to_ir(&orig, Some("Be nice"));
    let oai = openai_low::from_ir(&ir);
    assert_eq!(oai.len(), 3);
    assert_eq!(oai[0].role, "system");
    assert_eq!(oai[0].content.as_deref(), Some("Be nice"));
}

#[test]
fn fidelity_cross_openai_to_gemini() {
    let orig = vec![
        OpenAIMessage {
            role: "user".into(),
            content: Some("q".into()),
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
    let ir = openai_low::to_ir(&orig);
    let gemini = gemini_low::from_ir(&ir);
    assert_eq!(gemini.len(), 2);
    assert_eq!(gemini[0].role, "user");
    assert_eq!(gemini[1].role, "model");
}

#[test]
fn fidelity_cross_gemini_to_kimi() {
    let orig = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("hi".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("hello".into())],
        },
    ];
    let ir = gemini_low::to_ir(&orig, None);
    let kimi = kimi_low::from_ir(&ir);
    assert_eq!(kimi.len(), 2);
    assert_eq!(kimi[0].role, "user");
    assert_eq!(kimi[1].role, "assistant");
    assert_eq!(kimi[1].content.as_deref(), Some("hello"));
}

#[test]
fn fidelity_cross_kimi_to_copilot() {
    let orig = vec![
        KimiMessage {
            role: "system".into(),
            content: Some("sys".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "user".into(),
            content: Some("q".into()),
            tool_call_id: None,
            tool_calls: None,
        },
    ];
    let ir = kimi_low::to_ir(&orig);
    let copilot = copilot_low::from_ir(&ir);
    assert_eq!(copilot.len(), 2);
    assert_eq!(copilot[0].role, "system");
    assert_eq!(copilot[0].content, "sys");
}

#[test]
fn fidelity_cross_openai_tool_call_to_gemini() {
    let orig = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"test"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let ir = openai_low::to_ir(&orig);
    let gemini = gemini_low::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"q": "test"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn fidelity_cross_codex_to_kimi() {
    let orig = vec![CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText {
            text: "done".into(),
        }],
    }];
    let ir = codex_low::to_ir(&orig);
    let kimi = kimi_low::from_ir(&ir);
    assert_eq!(kimi[0].role, "assistant");
    assert_eq!(kimi[0].content.as_deref(), Some("done"));
}

#[test]
fn fidelity_cross_claude_image_to_gemini() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/jpeg".into(),
            data: "imgdata".into(),
        },
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_low::to_ir(&msgs, None);
    let gemini = gemini_low::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/jpeg");
            assert_eq!(d.data, "imgdata");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn fidelity_cross_thinking_claude_to_openai_becomes_text() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "reasoning step".into(),
        signature: None,
    }];
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_low::to_ir(&msgs, None);
    let oai = openai_low::from_ir(&ir);
    assert_eq!(oai[0].content.as_deref(), Some("reasoning step"));
}

#[test]
fn fidelity_cross_thinking_ir_to_copilot_becomes_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking { text: "hmm".into() }],
    )]);
    let copilot = copilot_low::from_ir(&conv);
    assert_eq!(copilot[0].content, "hmm");
}

// ═══════════════════════════════════════════════════════════════════════════
// §9  IR fidelity tracking via MappingRegistry (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_tracking_lossless() {
    let f = Fidelity::Lossless;
    assert!(f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_tracking_lossy_labeled() {
    let f = Fidelity::LossyLabeled {
        warning: "thinking blocks stripped".into(),
    };
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_tracking_unsupported() {
    let f = Fidelity::Unsupported {
        reason: "no image support".into(),
    };
    assert!(f.is_unsupported());
    assert!(!f.is_lossless());
}

#[test]
fn fidelity_tracking_serde_roundtrip() {
    for f in [
        Fidelity::Lossless,
        Fidelity::LossyLabeled {
            warning: "partial".into(),
        },
        Fidelity::Unsupported {
            reason: "N/A".into(),
        },
    ] {
        let j = serde_json::to_string(&f).unwrap();
        let back: Fidelity = serde_json::from_str(&j).unwrap();
        assert_eq!(f, back);
    }
}

#[test]
fn fidelity_tracking_registry_lookup() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: "thinking".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "flattened to text".into(),
        },
    });
    let rule = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, "thinking")
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
    assert!(
        reg.lookup(Dialect::Claude, Dialect::Gemini, "thinking")
            .is_none()
    );
}

#[test]
fn fidelity_tracking_known_rules_same_dialect_lossless() {
    let reg = known_rules();
    for &d in Dialect::all() {
        let rule = reg.lookup(d, d, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }
}

#[test]
fn fidelity_tracking_rank_targets() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "streaming".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Gemini,
        feature: "tool_use".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "no call IDs".into(),
        },
    });
    let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use", "streaming"]);
    assert!(!ranked.is_empty());
    assert_eq!(ranked[0].0, Dialect::Claude);
}

#[test]
fn fidelity_tracking_validate_mapping_reports_loss() {
    let reg = known_rules();
    let features: Vec<String> = vec!["tool_use".into(), "thinking".into()];
    let validations = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &features);
    assert_eq!(validations.len(), 2);
}

#[test]
fn fidelity_tracking_validate_empty_feature_error() {
    let reg = known_rules();
    let features = vec!["".into()];
    let validations = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &features);
    assert!(!validations[0].errors.is_empty());
}

#[test]
fn fidelity_tracking_copilot_tool_role_maps_to_user() {
    // Copilot has no tool role; IR Tool → "user" (lossy)
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![IrContentBlock::Text {
                text: "result".into(),
            }],
            is_error: false,
        }],
    )]);
    let copilot = copilot_low::from_ir(&conv);
    assert_eq!(copilot[0].role, "user");
}

// ═══════════════════════════════════════════════════════════════════════════
// §10 Streaming events via IR (4 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_streaming_incremental_text_aggregation() {
    let chunks = vec!["Hello", " ", "world", "!"];
    let mut acc = IrConversation::new();
    for chunk in &chunks {
        acc = acc.push(IrMessage::text(IrRole::Assistant, *chunk));
    }
    assert_eq!(acc.len(), 4);
    let full: String = acc.messages.iter().map(|m| m.text_content()).collect();
    assert_eq!(full, "Hello world!");
}

#[test]
fn fidelity_streaming_tool_call_assembled() {
    // Simulate a streamed tool call arriving in a single final message
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "call_s1".into(),
            name: "read_file".into(),
            input: json!({"path": "src/main.rs"}),
        }],
    );
    let conv = IrConversation::new().push(msg);
    assert_eq!(conv.tool_calls().len(), 1);
}

#[test]
fn fidelity_streaming_mixed_text_and_tool() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::Assistant, "Let me check."),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "s1".into(),
                name: "search".into(),
                input: json!({}),
            }],
        ),
    ]);
    assert_eq!(conv.tool_calls().len(), 1);
    assert_eq!(conv.messages[0].text_content(), "Let me check.");
}

#[test]
fn fidelity_streaming_empty_chunks_ignored() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::Assistant, ""),
        IrMessage::text(IrRole::Assistant, "real content"),
        IrMessage::text(IrRole::Assistant, ""),
    ]);
    let non_empty: Vec<_> = conv
        .messages
        .iter()
        .filter(|m| !m.text_content().is_empty())
        .collect();
    assert_eq!(non_empty.len(), 1);
    assert_eq!(non_empty[0].text_content(), "real content");
}

// ═══════════════════════════════════════════════════════════════════════════
// §11 Multi-turn conversations (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_multi_turn_basic() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "What is Rust?"))
        .push(IrMessage::text(IrRole::Assistant, "A systems language."))
        .push(IrMessage::text(IrRole::User, "More details?"))
        .push(IrMessage::text(IrRole::Assistant, "Memory-safe and fast."));
    assert_eq!(conv.len(), 5);
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    assert_eq!(
        conv.last_assistant().unwrap().text_content(),
        "Memory-safe and fast."
    );
}

#[test]
fn fidelity_multi_turn_with_tool_calls() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read main.rs"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "read_file".into(),
                input: json!({"path": "main.rs"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "The file contains a main function."),
    ]);
    assert_eq!(conv.len(), 4);
    assert_eq!(conv.tool_calls().len(), 1);
}

#[test]
fn fidelity_multi_turn_roundtrip_openai() {
    let orig = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("sys".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("q1".into()),
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
            content: Some("q2".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("a2".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let back = openai_low::from_ir(&openai_low::to_ir(&orig));
    assert_eq!(back.len(), 5);
    assert_eq!(back[4].content.as_deref(), Some("a2"));
}

#[test]
fn fidelity_multi_turn_cross_openai_to_claude() {
    let orig = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("sys".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("q".into()),
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
    let ir = openai_low::to_ir(&orig);
    let claude = claude_low::from_ir(&ir);
    assert_eq!(claude.len(), 2); // system extracted
    assert_eq!(claude[0].role, "user");
    assert_eq!(claude[1].role, "assistant");
}

#[test]
fn fidelity_multi_turn_large_conversation() {
    let msgs: Vec<IrMessage> = (0..100)
        .map(|i| {
            let role = if i % 2 == 0 {
                IrRole::User
            } else {
                IrRole::Assistant
            };
            IrMessage::text(role, format!("Turn {i}"))
        })
        .collect();
    let conv = IrConversation::from_messages(msgs);
    assert_eq!(conv.len(), 100);
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 50);
    let j = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.len(), 100);
}

#[test]
fn fidelity_multi_turn_preserves_message_order() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "first"),
        IrMessage::text(IrRole::Assistant, "second"),
        IrMessage::text(IrRole::User, "third"),
    ]);
    assert_eq!(conv.messages[0].text_content(), "first");
    assert_eq!(conv.messages[1].text_content(), "second");
    assert_eq!(conv.messages[2].text_content(), "third");
}

// ═══════════════════════════════════════════════════════════════════════════
// §12 System prompt handling (7 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_system_openai_inline() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be brief"),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let msgs = openai_low::from_ir(&conv);
    assert_eq!(msgs[0].role, "system");
    assert_eq!(msgs[0].content.as_deref(), Some("Be brief"));
}

#[test]
fn fidelity_system_claude_extracted() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be brief"),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let sys = claude_low::extract_system_prompt(&conv);
    assert_eq!(sys.as_deref(), Some("Be brief"));
    let msgs = claude_low::from_ir(&conv);
    assert_eq!(msgs.len(), 1);
}

#[test]
fn fidelity_system_gemini_instruction() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be brief"),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let instruction = gemini_low::extract_system_instruction(&conv);
    assert!(instruction.is_some());
    let contents = gemini_low::from_ir(&conv);
    assert_eq!(contents.len(), 1);
}

#[test]
fn fidelity_system_kimi_inline() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be brief"),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let msgs = kimi_low::from_ir(&conv);
    assert_eq!(msgs[0].role, "system");
}

#[test]
fn fidelity_system_copilot_inline() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be brief"),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let msgs = copilot_low::from_ir(&conv);
    assert_eq!(msgs[0].role, "system");
}

#[test]
fn fidelity_system_codex_skipped() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let items = codex_low::from_ir(&conv);
    assert!(items.is_empty());
}

#[test]
fn fidelity_system_absent_returns_none() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
    assert!(conv.system_message().is_none());
    assert!(claude_low::extract_system_prompt(&conv).is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// §13 Tool definitions through IR (4 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_tool_definition_json_roundtrip() {
    let def = IrToolDefinition {
        name: "grep".into(),
        description: "Search code".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string"}
            },
            "required": ["pattern"]
        }),
    };
    let j = serde_json::to_string(&def).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&j).unwrap();
    assert_eq!(def, back);
}

#[test]
fn fidelity_tool_definition_various_schemas() {
    let schemas = vec![
        json!({"type": "object"}),
        json!({"type": "object", "properties": {"x": {"type": "number"}}}),
        json!({"type": "object", "properties": {}, "additionalProperties": false}),
    ];
    for schema in schemas {
        let def = IrToolDefinition {
            name: "tool".into(),
            description: "desc".into(),
            parameters: schema.clone(),
        };
        let j = serde_json::to_string(&def).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&j).unwrap();
        assert_eq!(def, back, "failed for schema: {schema:?}");
    }
}

#[test]
fn fidelity_multiple_tool_defs() {
    let defs = vec![
        IrToolDefinition {
            name: "read".into(),
            description: "read".into(),
            parameters: json!({"type": "object"}),
        },
        IrToolDefinition {
            name: "write".into(),
            description: "write".into(),
            parameters: json!({"type": "object"}),
        },
        IrToolDefinition {
            name: "search".into(),
            description: "search".into(),
            parameters: json!({"type": "object"}),
        },
    ];
    let j = serde_json::to_string(&defs).unwrap();
    let back: Vec<IrToolDefinition> = serde_json::from_str(&j).unwrap();
    assert_eq!(defs, back);
}

#[test]
fn fidelity_tool_def_with_nested_schema() {
    let def = IrToolDefinition {
        name: "complex".into(),
        description: "Complex tool".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "config": {
                    "type": "object",
                    "properties": {
                        "nested": {"type": "boolean"},
                        "items": {"type": "array", "items": {"type": "string"}}
                    }
                }
            }
        }),
    };
    let j = serde_json::to_string(&def).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&j).unwrap();
    assert_eq!(def, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// §14 Token usage normalization (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_usage_from_io() {
    let u = IrUsage::from_io(100, 50);
    assert_eq!(u.total_tokens, 150);
    assert_eq!(u.cache_read_tokens, 0);
}

#[test]
fn fidelity_usage_with_cache() {
    let u = IrUsage::with_cache(200, 80, 30, 10);
    assert_eq!(u.total_tokens, 280);
    assert_eq!(u.cache_read_tokens, 30);
    assert_eq!(u.cache_write_tokens, 10);
}

#[test]
fn fidelity_usage_merge() {
    let a = IrUsage::from_io(100, 50);
    let b = IrUsage::with_cache(200, 80, 20, 10);
    let m = a.merge(b);
    assert_eq!(m.input_tokens, 300);
    assert_eq!(m.output_tokens, 130);
    assert_eq!(m.total_tokens, 430);
    assert_eq!(m.cache_read_tokens, 20);
}

#[test]
fn fidelity_usage_merge_identity() {
    let u = IrUsage::from_io(42, 58);
    let m = u.merge(IrUsage::default());
    assert_eq!(m, u);
}

#[test]
fn fidelity_usage_merge_associative() {
    let a = IrUsage::from_io(10, 20);
    let b = IrUsage::from_io(30, 40);
    let c = IrUsage::from_io(50, 60);
    assert_eq!(a.merge(b).merge(c), a.merge(b.merge(c)));
}

#[test]
fn fidelity_usage_serde_roundtrip() {
    let u = IrUsage::with_cache(500, 200, 50, 25);
    let j = serde_json::to_string(&u).unwrap();
    let back: IrUsage = serde_json::from_str(&j).unwrap();
    assert_eq!(u, back);
}

#[test]
fn fidelity_usage_codex_to_ir() {
    let cu = CodexUsage {
        input_tokens: 150,
        output_tokens: 75,
        total_tokens: 225,
    };
    let ir = codex_low::usage_to_ir(&cu);
    assert_eq!(ir.input_tokens, 150);
    assert_eq!(ir.output_tokens, 75);
    assert_eq!(ir.total_tokens, 225);
}

#[test]
fn fidelity_usage_kimi_to_ir() {
    let ku = KimiUsage {
        prompt_tokens: 300,
        completion_tokens: 120,
        total_tokens: 420,
    };
    let ir = kimi_low::usage_to_ir(&ku);
    assert_eq!(ir.input_tokens, 300);
    assert_eq!(ir.output_tokens, 120);
    assert_eq!(ir.total_tokens, 420);
}

// ═══════════════════════════════════════════════════════════════════════════
// §15 Model name normalization / dialect detection (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_dialect_all_six_variants() {
    let all = Dialect::all();
    assert_eq!(all.len(), 6);
    assert!(all.contains(&Dialect::OpenAi));
    assert!(all.contains(&Dialect::Claude));
    assert!(all.contains(&Dialect::Gemini));
    assert!(all.contains(&Dialect::Codex));
    assert!(all.contains(&Dialect::Kimi));
    assert!(all.contains(&Dialect::Copilot));
}

#[test]
fn fidelity_dialect_labels() {
    assert_eq!(Dialect::OpenAi.label(), "OpenAI");
    assert_eq!(Dialect::Claude.label(), "Claude");
    assert_eq!(Dialect::Gemini.label(), "Gemini");
    assert_eq!(Dialect::Codex.label(), "Codex");
    assert_eq!(Dialect::Kimi.label(), "Kimi");
    assert_eq!(Dialect::Copilot.label(), "Copilot");
}

#[test]
fn fidelity_dialect_serde_roundtrip() {
    for d in Dialect::all() {
        let j = serde_json::to_string(d).unwrap();
        let back: Dialect = serde_json::from_str(&j).unwrap();
        assert_eq!(*d, back);
    }
}

#[test]
fn fidelity_dialect_display() {
    assert_eq!(format!("{}", Dialect::OpenAi), "OpenAI");
    assert_eq!(format!("{}", Dialect::Copilot), "Copilot");
}

#[test]
fn fidelity_ir_metadata_can_store_model_name() {
    let mut msg = IrMessage::text(IrRole::Assistant, "response");
    msg.metadata.insert("model".into(), json!("gpt-4o"));
    msg.metadata.insert("dialect".into(), json!("open_ai"));
    let j = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["model"], json!("gpt-4o"));
}

#[test]
fn fidelity_ir_metadata_model_across_dialects() {
    let mut msg = IrMessage::text(IrRole::Assistant, "resp");
    msg.metadata.insert("model".into(), json!("claude-3-opus"));
    let conv = IrConversation::new().push(msg);
    let oai = openai_low::from_ir(&conv);
    // Metadata is not carried to dialect-specific messages, but IR preserves it
    let j = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.messages[0].metadata["model"], json!("claude-3-opus"));
    assert_eq!(oai[0].role, "assistant");
}

// ═══════════════════════════════════════════════════════════════════════════
// §bonus  Edge cases, unicode, determinism (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_unicode_roundtrip() {
    let text = "Hello 🌍! Привет мир. こんにちは世界. مرحبا";
    let msg = IrMessage::text(IrRole::User, text);
    let j = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.text_content(), text);
}

#[test]
fn fidelity_special_characters_roundtrip() {
    let text = "Line 1\nLine 2\tTabbed\r\nCRLF\\ backslash \"quoted\"";
    let msg = IrMessage::text(IrRole::User, text);
    let j = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.text_content(), text);
}

#[test]
fn fidelity_very_long_content() {
    let long = "x".repeat(100_000);
    let msg = IrMessage::text(IrRole::User, &long);
    let j = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&j).unwrap();
    assert_eq!(back.text_content().len(), 100_000);
}

#[test]
fn fidelity_metadata_deterministic_order() {
    let mut msg = IrMessage::text(IrRole::User, "test");
    msg.metadata.insert("z".into(), json!("z"));
    msg.metadata.insert("a".into(), json!("a"));
    msg.metadata.insert("m".into(), json!("m"));
    let j = serde_json::to_string(&msg).unwrap();
    let a_pos = j.find("\"a\"").unwrap();
    let m_pos = j.find("\"m\"").unwrap();
    let z_pos = j.find("\"z\"").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

#[test]
fn fidelity_metadata_omitted_when_empty() {
    let msg = IrMessage::text(IrRole::User, "hi");
    let j = serde_json::to_string(&msg).unwrap();
    assert!(!j.contains("metadata"));
}

#[test]
fn fidelity_empty_conversation_serde() {
    let conv = IrConversation::new();
    let j = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&j).unwrap();
    assert!(back.is_empty());
}

#[test]
fn fidelity_empty_dialect_roundtrips() {
    assert!(openai_low::to_ir(&[]).is_empty());
    assert!(claude_low::to_ir(&[], None).is_empty());
    assert!(gemini_low::to_ir(&[], None).is_empty());
    assert!(codex_low::to_ir(&[]).is_empty());
    assert!(kimi_low::to_ir(&[]).is_empty());
    assert!(copilot_low::to_ir(&[]).is_empty());
}

#[test]
fn fidelity_conversation_default() {
    let conv = IrConversation::default();
    assert!(conv.is_empty());
    assert_eq!(conv.len(), 0);
    assert!(conv.system_message().is_none());
    assert!(conv.last_assistant().is_none());
    assert!(conv.last_message().is_none());
    assert!(conv.tool_calls().is_empty());
}
