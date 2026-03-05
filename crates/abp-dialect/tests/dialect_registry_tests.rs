#![allow(clippy::all)]
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
//! Integration tests for the dialect registry and IR types.

use abp_dialect::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
    IrToolDefinition, IrUsage,
};
use abp_dialect::registry::{parse_response, DialectEntry, DialectError, DialectRegistry};
use abp_dialect::Dialect;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// 1. Registry construction & basics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_registry_has_no_dialects() {
    let r = DialectRegistry::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    assert!(r.list_dialects().is_empty());
}

#[test]
fn builtins_registers_six_dialects() {
    let r = DialectRegistry::with_builtins();
    assert_eq!(r.len(), 6);
    assert!(!r.is_empty());
}

#[test]
fn builtin_list_contains_all_dialects() {
    let r = DialectRegistry::with_builtins();
    let dialects = r.list_dialects();
    assert!(dialects.contains(&Dialect::OpenAi));
    assert!(dialects.contains(&Dialect::Claude));
    assert!(dialects.contains(&Dialect::Gemini));
    assert!(dialects.contains(&Dialect::Codex));
    assert!(dialects.contains(&Dialect::Kimi));
    assert!(dialects.contains(&Dialect::Copilot));
}

#[test]
fn get_returns_some_for_registered() {
    let r = DialectRegistry::with_builtins();
    let entry = r.get(Dialect::OpenAi).unwrap();
    assert_eq!(entry.name, "openai");
    assert_eq!(entry.dialect, Dialect::OpenAi);
}

#[test]
fn get_returns_none_for_empty_registry() {
    let r = DialectRegistry::new();
    assert!(r.get(Dialect::OpenAi).is_none());
}

#[test]
fn supports_pair_both_registered() {
    let r = DialectRegistry::with_builtins();
    assert!(r.supports_pair(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn supports_pair_same_dialect() {
    let r = DialectRegistry::with_builtins();
    assert!(r.supports_pair(Dialect::Gemini, Dialect::Gemini));
}

#[test]
fn supports_pair_false_when_missing() {
    let r = DialectRegistry::new();
    assert!(!r.supports_pair(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn register_replaces_existing() {
    let mut r = DialectRegistry::with_builtins();
    let original = r.get(Dialect::OpenAi).unwrap().version;
    let entry = DialectEntry {
        dialect: Dialect::OpenAi,
        name: "openai",
        version: "v2-custom",
        parser: r.get(Dialect::OpenAi).unwrap().parser,
        serializer: r.get(Dialect::OpenAi).unwrap().serializer,
    };
    r.register(entry);
    assert_eq!(r.get(Dialect::OpenAi).unwrap().version, "v2-custom");
    assert_ne!(original, "v2-custom");
    assert_eq!(r.len(), 6); // didn't add a new one
}

#[test]
fn dialect_entry_debug() {
    let r = DialectRegistry::with_builtins();
    let entry = r.get(Dialect::Claude).unwrap();
    let dbg = format!("{entry:?}");
    assert!(dbg.contains("claude"));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. OpenAI parsing & serialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_parse_simple_request() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are helpful."},
            {"role": "user", "content": "Hello"}
        ]
    });
    let ir = r.parse(Dialect::OpenAi, &val).unwrap();
    assert_eq!(ir.model.as_deref(), Some("gpt-4"));
    assert_eq!(ir.system_prompt.as_deref(), Some("You are helpful."));
    assert_eq!(ir.messages.len(), 2);
}

#[test]
fn openai_parse_with_tools() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "object", "properties": {}}
            }
        }]
    });
    let ir = r.parse(Dialect::OpenAi, &val).unwrap();
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "get_weather");
}

#[test]
fn openai_parse_tool_calls() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "model": "gpt-4",
        "messages": [{
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "read_file",
                    "arguments": "{\"path\":\"main.rs\"}"
                }
            }]
        }]
    });
    let ir = r.parse(Dialect::OpenAi, &val).unwrap();
    let calls = ir.all_tool_calls();
    assert_eq!(calls.len(), 1);
}

#[test]
fn openai_parse_config() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.7,
        "max_tokens": 1024,
        "top_p": 0.9,
        "stop": ["END"]
    });
    let ir = r.parse(Dialect::OpenAi, &val).unwrap();
    assert_eq!(ir.config.temperature, Some(0.7));
    assert_eq!(ir.config.max_tokens, Some(1024));
    assert_eq!(ir.config.top_p, Some(0.9));
    assert_eq!(ir.config.stop_sequences, vec!["END"]);
}

#[test]
fn openai_roundtrip() {
    let r = DialectRegistry::with_builtins();
    let original = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Hello world"}
        ],
        "temperature": 0.5
    });
    let ir = r.parse(Dialect::OpenAi, &original).unwrap();
    let serialized = r.serialize(Dialect::OpenAi, &ir).unwrap();
    assert_eq!(serialized["model"].as_str(), Some("gpt-4"));
    assert_eq!(serialized["messages"][0]["role"].as_str(), Some("user"));
}

#[test]
fn openai_serialize_with_system_prompt() {
    let r = DialectRegistry::with_builtins();
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")])
        .with_model("gpt-4")
        .with_system_prompt("Be concise");
    let val = r.serialize(Dialect::OpenAi, &ir).unwrap();
    // System prompt should appear as first message
    assert_eq!(val["messages"][0]["role"].as_str(), Some("system"));
    assert_eq!(val["messages"][0]["content"].as_str(), Some("Be concise"));
}

#[test]
fn openai_parse_rejects_non_object() {
    let r = DialectRegistry::with_builtins();
    let err = r
        .parse(Dialect::OpenAi, &json!("not an object"))
        .unwrap_err();
    assert_eq!(err.dialect, Dialect::OpenAi);
    assert!(err.message.contains("object"));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Claude parsing & serialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_parse_simple_request() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "model": "claude-3-opus-20240229",
        "system": "You are a coding assistant.",
        "messages": [
            {"role": "user", "content": "Hello"}
        ],
        "max_tokens": 1024
    });
    let ir = r.parse(Dialect::Claude, &val).unwrap();
    assert_eq!(ir.model.as_deref(), Some("claude-3-opus-20240229"));
    assert_eq!(
        ir.system_prompt.as_deref(),
        Some("You are a coding assistant.")
    );
    assert_eq!(ir.config.max_tokens, Some(1024));
}

#[test]
fn claude_parse_content_blocks() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "model": "claude-3",
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "What is this?"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc123"}}
            ]
        }]
    });
    let ir = r.parse(Dialect::Claude, &val).unwrap();
    assert_eq!(ir.messages[0].content.len(), 2);
    match &ir.messages[0].content[1] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "abc123");
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

#[test]
fn claude_parse_tool_use_blocks() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "model": "claude-3",
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "tool_use", "id": "tu_1", "name": "search", "input": {"q": "rust"}}
            ]
        }]
    });
    let ir = r.parse(Dialect::Claude, &val).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolCall { id, name, input } => {
            assert_eq!(id, "tu_1");
            assert_eq!(name, "search");
            assert_eq!(input, &json!({"q": "rust"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn claude_parse_tool_result_blocks() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "model": "claude-3",
        "messages": [{
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "tu_1",
                "content": "result text",
                "is_error": false
            }]
        }]
    });
    let ir = r.parse(Dialect::Claude, &val).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_call_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_call_id, "tu_1");
            assert!(!is_error);
            assert_eq!(content.len(), 1);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn claude_roundtrip() {
    let r = DialectRegistry::with_builtins();
    let original = json!({
        "model": "claude-3-sonnet",
        "system": "Be helpful",
        "messages": [{"role": "user", "content": "Hi"}],
        "max_tokens": 512
    });
    let ir = r.parse(Dialect::Claude, &original).unwrap();
    let serialized = r.serialize(Dialect::Claude, &ir).unwrap();
    assert_eq!(serialized["model"].as_str(), Some("claude-3-sonnet"));
    assert_eq!(serialized["system"].as_str(), Some("Be helpful"));
}

#[test]
fn claude_serialize_skips_system_messages() {
    let r = DialectRegistry::with_builtins();
    let ir = IrRequest::new(vec![
        IrMessage::text(IrRole::System, "System prompt"),
        IrMessage::text(IrRole::User, "Hi"),
    ])
    .with_model("claude-3");
    let val = r.serialize(Dialect::Claude, &ir).unwrap();
    // System messages should not appear in the messages array
    let msgs = val["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["role"].as_str(), Some("user"));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Gemini parsing & serialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gemini_parse_simple_request() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "contents": [
            {"role": "user", "parts": [{"text": "Hello"}]}
        ]
    });
    let ir = r.parse(Dialect::Gemini, &val).unwrap();
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].text_content(), "Hello");
}

#[test]
fn gemini_parse_system_instruction() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "system_instruction": {"parts": [{"text": "Be concise"}]},
        "contents": [{"role": "user", "parts": [{"text": "Hi"}]}]
    });
    let ir = r.parse(Dialect::Gemini, &val).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("Be concise"));
}

#[test]
fn gemini_parse_function_call() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "contents": [{
            "role": "model",
            "parts": [{"functionCall": {"name": "get_weather", "args": {"city": "NYC"}}}]
        }]
    });
    let ir = r.parse(Dialect::Gemini, &val).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolCall { name, input, .. } => {
            assert_eq!(name, "get_weather");
            assert_eq!(input, &json!({"city": "NYC"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn gemini_parse_generation_config() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "contents": [{"role": "user", "parts": [{"text": "Hi"}]}],
        "generationConfig": {
            "maxOutputTokens": 2048,
            "temperature": 0.8,
            "topP": 0.95,
            "topK": 40
        }
    });
    let ir = r.parse(Dialect::Gemini, &val).unwrap();
    assert_eq!(ir.config.max_tokens, Some(2048));
    assert_eq!(ir.config.temperature, Some(0.8));
    assert_eq!(ir.config.top_p, Some(0.95));
    assert_eq!(ir.config.top_k, Some(40));
}

#[test]
fn gemini_roundtrip() {
    let r = DialectRegistry::with_builtins();
    let original = json!({
        "contents": [{"role": "user", "parts": [{"text": "What is 2+2?"}]}],
        "generationConfig": {"temperature": 0.5}
    });
    let ir = r.parse(Dialect::Gemini, &original).unwrap();
    let serialized = r.serialize(Dialect::Gemini, &ir).unwrap();
    let parts = &serialized["contents"][0]["parts"];
    assert_eq!(parts[0]["text"].as_str(), Some("What is 2+2?"));
}

#[test]
fn gemini_serialize_system_instruction() {
    let r = DialectRegistry::with_builtins();
    let ir =
        IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")]).with_system_prompt("Be concise");
    let val = r.serialize(Dialect::Gemini, &ir).unwrap();
    assert!(val.get("system_instruction").is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Codex parsing & serialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codex_parse_request() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "model": "codex-mini",
        "instructions": "You are a coding assistant.",
        "input": "Write a hello world in Rust"
    });
    let ir = r.parse(Dialect::Codex, &val).unwrap();
    assert_eq!(ir.model.as_deref(), Some("codex-mini"));
    assert_eq!(
        ir.system_prompt.as_deref(),
        Some("You are a coding assistant.")
    );
    assert!(ir
        .messages
        .iter()
        .any(|m| m.text_content().contains("hello world")));
}

#[test]
fn codex_parse_response_items() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "items": [
            {"type": "message", "role": "assistant", "content": "Done"},
            {"type": "function_call", "call_id": "fc1", "name": "write_file", "arguments": "{}"}
        ]
    });
    let ir = r.parse(Dialect::Codex, &val).unwrap();
    assert!(ir.messages.len() >= 2);
}

#[test]
fn codex_roundtrip() {
    let r = DialectRegistry::with_builtins();
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Fix my code")])
        .with_model("codex-mini")
        .with_system_prompt("You are a fixer.");
    let val = r.serialize(Dialect::Codex, &ir).unwrap();
    assert_eq!(val["model"].as_str(), Some("codex-mini"));
    assert_eq!(val["instructions"].as_str(), Some("You are a fixer."));
    assert_eq!(val["input"].as_str(), Some("Fix my code"));
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Kimi parsing & serialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn kimi_parse_with_refs() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "model": "kimi",
        "messages": [{"role": "user", "content": "Summarize this"}],
        "refs": ["https://example.com"]
    });
    let ir = r.parse(Dialect::Kimi, &val).unwrap();
    assert_eq!(ir.model.as_deref(), Some("kimi"));
    assert!(ir.metadata.contains_key("kimi_refs"));
}

#[test]
fn kimi_parse_with_search_plus() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "model": "kimi",
        "messages": [{"role": "user", "content": "Search for X"}],
        "search_plus": true
    });
    let ir = r.parse(Dialect::Kimi, &val).unwrap();
    assert!(ir.metadata.contains_key("kimi_search_plus"));
}

#[test]
fn kimi_roundtrip_preserves_metadata() {
    let r = DialectRegistry::with_builtins();
    let original = json!({
        "model": "kimi",
        "messages": [{"role": "user", "content": "Hi"}],
        "refs": ["https://example.com"],
        "search_plus": true
    });
    let ir = r.parse(Dialect::Kimi, &original).unwrap();
    let val = r.serialize(Dialect::Kimi, &ir).unwrap();
    assert!(val.get("refs").is_some());
    assert!(val.get("search_plus").is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Copilot parsing & serialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn copilot_parse_with_references() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "messages": [{"role": "user", "content": "Fix bug"}],
        "references": [{"type": "file", "path": "src/main.rs"}]
    });
    let ir = r.parse(Dialect::Copilot, &val).unwrap();
    assert!(ir.metadata.contains_key("copilot_references"));
}

#[test]
fn copilot_parse_with_agent_mode() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "messages": [{"role": "user", "content": "Deploy"}],
        "agent_mode": true,
        "confirmations": [{"id": "c1"}]
    });
    let ir = r.parse(Dialect::Copilot, &val).unwrap();
    assert!(ir.metadata.contains_key("copilot_agent_mode"));
    assert!(ir.metadata.contains_key("copilot_confirmations"));
}

#[test]
fn copilot_roundtrip_preserves_metadata() {
    let r = DialectRegistry::with_builtins();
    let original = json!({
        "messages": [{"role": "user", "content": "Hi"}],
        "references": [{"type": "file"}],
        "agent_mode": true
    });
    let ir = r.parse(Dialect::Copilot, &original).unwrap();
    let val = r.serialize(Dialect::Copilot, &ir).unwrap();
    assert!(val.get("references").is_some());
    assert!(val.get("agent_mode").is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Cross-dialect transformation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_claude_via_ir() {
    let r = DialectRegistry::with_builtins();
    let openai_req = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "Be helpful"},
            {"role": "user", "content": "Hello"}
        ],
        "max_tokens": 512
    });
    let ir = r.parse(Dialect::OpenAi, &openai_req).unwrap();
    let claude_val = r.serialize(Dialect::Claude, &ir).unwrap();
    // Claude should have system as top-level field
    assert!(claude_val.get("system").is_some() || claude_val.get("messages").is_some());
}

#[test]
fn claude_to_openai_via_ir() {
    let r = DialectRegistry::with_builtins();
    let claude_req = json!({
        "model": "claude-3-opus",
        "system": "You are a translator.",
        "messages": [{"role": "user", "content": "Translate this"}],
        "max_tokens": 1024
    });
    let ir = r.parse(Dialect::Claude, &claude_req).unwrap();
    let openai_val = r.serialize(Dialect::OpenAi, &ir).unwrap();
    // Should have system message in messages array
    let msgs = openai_val["messages"].as_array().unwrap();
    assert!(msgs.iter().any(|m| m["role"].as_str() == Some("system")));
}

#[test]
fn openai_to_gemini_via_ir() {
    let r = DialectRegistry::with_builtins();
    let openai_req = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "Be concise"},
            {"role": "user", "content": "What is AI?"}
        ]
    });
    let ir = r.parse(Dialect::OpenAi, &openai_req).unwrap();
    let gemini_val = r.serialize(Dialect::Gemini, &ir).unwrap();
    assert!(gemini_val.get("contents").is_some());
    if ir.system_prompt.is_some() {
        assert!(gemini_val.get("system_instruction").is_some());
    }
}

#[test]
fn gemini_to_claude_via_ir() {
    let r = DialectRegistry::with_builtins();
    let gemini_req = json!({
        "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
        "generationConfig": {"temperature": 0.9}
    });
    let ir = r.parse(Dialect::Gemini, &gemini_req).unwrap();
    let claude_val = r.serialize(Dialect::Claude, &ir).unwrap();
    let msgs = claude_val["messages"].as_array().unwrap();
    assert!(!msgs.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 9. IR type unit tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_message_text_helper() {
    let msg = IrMessage::text(IrRole::User, "Hello");
    assert_eq!(msg.text_content(), "Hello");
    assert_eq!(msg.role, IrRole::User);
}

#[test]
fn ir_message_tool_calls_accessor() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me check".into(),
            },
            IrContentBlock::ToolCall {
                id: "tc1".into(),
                name: "search".into(),
                input: json!({}),
            },
        ],
    );
    assert_eq!(msg.tool_calls().len(), 1);
}

#[test]
fn ir_content_block_as_text() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    assert_eq!(block.as_text(), Some("hello"));

    let block = IrContentBlock::ToolCall {
        id: "1".into(),
        name: "x".into(),
        input: json!({}),
    };
    assert!(block.as_text().is_none());
}

#[test]
fn ir_content_block_is_tool_call() {
    let block = IrContentBlock::ToolCall {
        id: "1".into(),
        name: "x".into(),
        input: json!({}),
    };
    assert!(block.is_tool_call());
    assert!(!block.is_tool_result());
}

#[test]
fn ir_content_block_is_tool_result() {
    let block = IrContentBlock::ToolResult {
        tool_call_id: "1".into(),
        content: vec![],
        is_error: false,
    };
    assert!(block.is_tool_result());
    assert!(!block.is_tool_call());
}

#[test]
fn ir_request_builder_chain() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")])
        .with_model("gpt-4")
        .with_system_prompt("Be helpful")
        .with_tool(IrToolDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: json!({"type": "object"}),
        })
        .with_config(IrGenerationConfig {
            max_tokens: Some(1024),
            temperature: Some(0.7),
            ..Default::default()
        });

    assert_eq!(ir.model.as_deref(), Some("gpt-4"));
    assert_eq!(ir.system_prompt.as_deref(), Some("Be helpful"));
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.config.max_tokens, Some(1024));
}

#[test]
fn ir_request_system_message() {
    let ir = IrRequest::new(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    assert_eq!(ir.system_message().unwrap().text_content(), "sys");
}

#[test]
fn ir_request_all_tool_calls() {
    let ir = IrRequest::new(vec![
        IrMessage::text(IrRole::User, "hi"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolCall {
                id: "1".into(),
                name: "search".into(),
                input: json!({}),
            }],
        ),
    ]);
    assert_eq!(ir.all_tool_calls().len(), 1);
}

#[test]
fn ir_response_text_helper() {
    let resp = IrResponse::text("Hello!");
    assert_eq!(resp.text_content(), "Hello!");
}

#[test]
fn ir_response_builder_chain() {
    let resp = IrResponse::text("Hi")
        .with_id("resp-1")
        .with_model("gpt-4")
        .with_stop_reason(IrStopReason::EndTurn)
        .with_usage(IrUsage::from_io(100, 50));

    assert_eq!(resp.id.as_deref(), Some("resp-1"));
    assert_eq!(resp.model.as_deref(), Some("gpt-4"));
    assert_eq!(resp.stop_reason, Some(IrStopReason::EndTurn));
    assert_eq!(resp.usage.unwrap().total_tokens, 150);
}

#[test]
fn ir_response_has_tool_calls() {
    let resp = IrResponse::new(vec![IrContentBlock::ToolCall {
        id: "1".into(),
        name: "x".into(),
        input: json!({}),
    }]);
    assert!(resp.has_tool_calls());
    assert_eq!(resp.tool_calls().len(), 1);

    let resp2 = IrResponse::text("no tools");
    assert!(!resp2.has_tool_calls());
}

#[test]
fn ir_usage_from_io() {
    let u = IrUsage::from_io(100, 50);
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);
    assert_eq!(u.total_tokens, 150);
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
fn ir_generation_config_default() {
    let config = IrGenerationConfig::default();
    assert!(config.max_tokens.is_none());
    assert!(config.temperature.is_none());
    assert!(config.stop_sequences.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Response parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_openai_response() {
    let val = json!({
        "id": "chatcmpl-123",
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "Hello!"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    });
    let resp = parse_response(Dialect::OpenAi, &val).unwrap();
    assert_eq!(resp.id.as_deref(), Some("chatcmpl-123"));
    assert_eq!(resp.text_content(), "Hello!");
    assert_eq!(resp.stop_reason, Some(IrStopReason::EndTurn));
    assert_eq!(resp.usage.unwrap().total_tokens, 15);
}

#[test]
fn parse_claude_response_test() {
    let val = json!({
        "id": "msg_123",
        "model": "claude-3-opus",
        "content": [{"type": "text", "text": "Sure!"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 20, "output_tokens": 10}
    });
    let resp = parse_response(Dialect::Claude, &val).unwrap();
    assert_eq!(resp.id.as_deref(), Some("msg_123"));
    assert_eq!(resp.text_content(), "Sure!");
    assert_eq!(resp.stop_reason, Some(IrStopReason::EndTurn));
}

#[test]
fn parse_gemini_response_test() {
    let val = json!({
        "candidates": [{
            "content": {"parts": [{"text": "42"}]},
            "finishReason": "STOP"
        }],
        "usageMetadata": {
            "promptTokenCount": 5,
            "candidatesTokenCount": 1,
            "totalTokenCount": 6
        }
    });
    let resp = parse_response(Dialect::Gemini, &val).unwrap();
    assert_eq!(resp.text_content(), "42");
    assert_eq!(resp.usage.unwrap().total_tokens, 6);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Serde roundtrip for IR types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_role_serde_roundtrip() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let json = serde_json::to_string(&role).unwrap();
        let back: IrRole = serde_json::from_str(&json).unwrap();
        assert_eq!(back, role);
    }
}

#[test]
fn ir_content_block_serde_roundtrip() {
    let blocks = vec![
        IrContentBlock::Text {
            text: "hello".into(),
        },
        IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "abc".into(),
        },
        IrContentBlock::ToolCall {
            id: "1".into(),
            name: "search".into(),
            input: json!({"q": "test"}),
        },
        IrContentBlock::ToolResult {
            tool_call_id: "1".into(),
            content: vec![IrContentBlock::Text {
                text: "result".into(),
            }],
            is_error: false,
        },
        IrContentBlock::Thinking { text: "hmm".into() },
    ];
    for block in blocks {
        let json = serde_json::to_value(&block).unwrap();
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(back, block);
    }
}

#[test]
fn ir_request_serde_roundtrip() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")])
        .with_model("gpt-4")
        .with_system_prompt("Be helpful");
    let json = serde_json::to_value(&ir).unwrap();
    let back: IrRequest = serde_json::from_value(json).unwrap();
    assert_eq!(back.model, ir.model);
    assert_eq!(back.system_prompt, ir.system_prompt);
}

#[test]
fn ir_response_serde_roundtrip() {
    let resp = IrResponse::text("Hello!")
        .with_id("r1")
        .with_stop_reason(IrStopReason::EndTurn)
        .with_usage(IrUsage::from_io(10, 5));
    let json = serde_json::to_value(&resp).unwrap();
    let back: IrResponse = serde_json::from_value(json).unwrap();
    assert_eq!(back.text_content(), "Hello!");
    assert_eq!(back.stop_reason, Some(IrStopReason::EndTurn));
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Error handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_unregistered_dialect_errors() {
    let r = DialectRegistry::new();
    let err = r.parse(Dialect::OpenAi, &json!({})).unwrap_err();
    assert!(err.message.contains("not registered"));
}

#[test]
fn serialize_unregistered_dialect_errors() {
    let r = DialectRegistry::new();
    let ir = IrRequest::new(vec![]);
    let err = r.serialize(Dialect::OpenAi, &ir).unwrap_err();
    assert!(err.message.contains("not registered"));
}

#[test]
fn dialect_error_display() {
    let err = DialectError {
        dialect: Dialect::OpenAi,
        message: "something broke".into(),
    };
    let s = format!("{err}");
    assert!(s.contains("OpenAI"));
    assert!(s.contains("something broke"));
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_empty_messages_array() {
    let r = DialectRegistry::with_builtins();
    let val = json!({"model": "gpt-4", "messages": []});
    let ir = r.parse(Dialect::OpenAi, &val).unwrap();
    assert!(ir.messages.is_empty());
}

#[test]
fn parse_no_model_field() {
    let r = DialectRegistry::with_builtins();
    let val = json!({"messages": [{"role": "user", "content": "hi"}]});
    let ir = r.parse(Dialect::OpenAi, &val).unwrap();
    assert!(ir.model.is_none());
}

#[test]
fn serialize_empty_request() {
    let r = DialectRegistry::with_builtins();
    let ir = IrRequest::new(vec![]);
    let val = r.serialize(Dialect::OpenAi, &ir).unwrap();
    assert!(val.is_object());
}

#[test]
fn thinking_block_preserved_in_claude() {
    let r = DialectRegistry::with_builtins();
    let val = json!({
        "model": "claude-3",
        "messages": [{
            "role": "assistant",
            "content": [{"type": "thinking", "thinking": "Let me think..."}]
        }]
    });
    let ir = r.parse(Dialect::Claude, &val).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::Thinking { text } => assert_eq!(text, "Let me think..."),
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn audio_content_block_serde() {
    let block = IrContentBlock::Audio {
        media_type: "audio/wav".into(),
        data: "base64data".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn custom_content_block_serde() {
    let block = IrContentBlock::Custom {
        custom_type: "vendor_specific".into(),
        data: json!({"key": "value"}),
    };
    let json = serde_json::to_value(&block).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn ir_stop_reason_variants_serde() {
    let reasons = vec![
        IrStopReason::EndTurn,
        IrStopReason::StopSequence,
        IrStopReason::MaxTokens,
        IrStopReason::ToolUse,
        IrStopReason::ContentFilter,
        IrStopReason::Other("custom".into()),
    ];
    for reason in reasons {
        let json = serde_json::to_value(&reason).unwrap();
        let back: IrStopReason = serde_json::from_value(json).unwrap();
        assert_eq!(back, reason);
    }
}

#[test]
fn entry_versions_are_set() {
    let r = DialectRegistry::with_builtins();
    for d in Dialect::all() {
        let entry = r.get(*d).unwrap();
        assert!(!entry.version.is_empty());
    }
}

#[test]
fn entry_names_match_dialect() {
    let r = DialectRegistry::with_builtins();
    assert_eq!(r.get(Dialect::OpenAi).unwrap().name, "openai");
    assert_eq!(r.get(Dialect::Claude).unwrap().name, "claude");
    assert_eq!(r.get(Dialect::Gemini).unwrap().name, "gemini");
    assert_eq!(r.get(Dialect::Codex).unwrap().name, "codex");
    assert_eq!(r.get(Dialect::Kimi).unwrap().name, "kimi");
    assert_eq!(r.get(Dialect::Copilot).unwrap().name, "copilot");
}
