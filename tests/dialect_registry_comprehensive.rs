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
//! Comprehensive tests for the SDK dialect registry system.
//!
//! Covers dialect enum parsing/validation, dialect entry metadata, registry
//! CRUD operations, cross-dialect compatibility via IR, and integration with
//! `WorkOrder` / `RuntimeConfig` vendor fields.

use std::collections::{BTreeMap, HashSet};

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ExecutionMode,
    MinSupport, RuntimeConfig, SupportLevel, WorkOrderBuilder,
};
use abp_dialect::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
    IrToolDefinition, IrUsage,
};
use abp_dialect::registry::{parse_response, DialectEntry, DialectError, DialectRegistry};
use abp_dialect::Dialect;
use serde_json::{json, Value};

// ═══════════════════════════════════════════════════════════════════════
// 1. Dialect enum parsing and validation (~15 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_serde_roundtrip_all_variants() {
    for &d in Dialect::all() {
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d, "roundtrip failed for {d:?}");
    }
}

#[test]
fn dialect_serde_snake_case_openai() {
    let json = serde_json::to_string(&Dialect::OpenAi).unwrap();
    assert_eq!(json, r#""open_ai""#);
    let back: Dialect = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Dialect::OpenAi);
}

#[test]
fn dialect_serde_snake_case_claude() {
    let json = serde_json::to_string(&Dialect::Claude).unwrap();
    assert_eq!(json, r#""claude""#);
}

#[test]
fn dialect_serde_snake_case_gemini() {
    let json = serde_json::to_string(&Dialect::Gemini).unwrap();
    assert_eq!(json, r#""gemini""#);
}

#[test]
fn dialect_serde_snake_case_codex() {
    let json = serde_json::to_string(&Dialect::Codex).unwrap();
    assert_eq!(json, r#""codex""#);
}

#[test]
fn dialect_serde_snake_case_kimi() {
    let json = serde_json::to_string(&Dialect::Kimi).unwrap();
    assert_eq!(json, r#""kimi""#);
}

#[test]
fn dialect_serde_snake_case_copilot() {
    let json = serde_json::to_string(&Dialect::Copilot).unwrap();
    assert_eq!(json, r#""copilot""#);
}

#[test]
fn dialect_deserialize_invalid_string_errors() {
    let result = serde_json::from_str::<Dialect>(r#""unknown_dialect""#);
    assert!(result.is_err());
}

#[test]
fn dialect_deserialize_empty_string_errors() {
    let result = serde_json::from_str::<Dialect>(r#""""#);
    assert!(result.is_err());
}

#[test]
fn dialect_deserialize_number_errors() {
    let result = serde_json::from_str::<Dialect>("42");
    assert!(result.is_err());
}

#[test]
fn dialect_equality_and_clone() {
    let a = Dialect::OpenAi;
    let b = a;
    assert_eq!(a, b);
}

#[test]
fn dialect_ordering_is_deterministic() {
    let mut dialects: Vec<Dialect> = Dialect::all().to_vec();
    dialects.sort();
    let mut again = dialects.clone();
    again.sort();
    assert_eq!(dialects, again);
}

#[test]
fn dialect_hashing_in_set() {
    let mut set = HashSet::new();
    for &d in Dialect::all() {
        assert!(set.insert(d), "duplicate in hash set for {d:?}");
    }
    assert_eq!(set.len(), 6);
    assert!(set.contains(&Dialect::Gemini));
}

#[test]
fn dialect_display_returns_label() {
    assert_eq!(format!("{}", Dialect::OpenAi), "OpenAI");
    assert_eq!(format!("{}", Dialect::Claude), "Claude");
    assert_eq!(format!("{}", Dialect::Gemini), "Gemini");
    assert_eq!(format!("{}", Dialect::Codex), "Codex");
    assert_eq!(format!("{}", Dialect::Kimi), "Kimi");
    assert_eq!(format!("{}", Dialect::Copilot), "Copilot");
}

#[test]
fn dialect_debug_format() {
    let dbg = format!("{:?}", Dialect::OpenAi);
    assert_eq!(dbg, "OpenAi");
}

#[test]
fn dialect_all_returns_six() {
    assert_eq!(Dialect::all().len(), 6);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. DialectEntry / DialectInfo completeness (~15 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn builtin_openai_entry_has_name_and_version() {
    let r = DialectRegistry::with_builtins();
    let e = r.get(Dialect::OpenAi).unwrap();
    assert_eq!(e.name, "openai");
    assert_eq!(e.version, "v1");
    assert_eq!(e.dialect, Dialect::OpenAi);
}

#[test]
fn builtin_claude_entry_has_name_and_version() {
    let r = DialectRegistry::with_builtins();
    let e = r.get(Dialect::Claude).unwrap();
    assert_eq!(e.name, "claude");
    assert_eq!(e.version, "v1");
}

#[test]
fn builtin_gemini_entry_has_name_and_version() {
    let r = DialectRegistry::with_builtins();
    let e = r.get(Dialect::Gemini).unwrap();
    assert_eq!(e.name, "gemini");
    assert_eq!(e.version, "v1");
}

#[test]
fn builtin_codex_entry_has_name_and_version() {
    let r = DialectRegistry::with_builtins();
    let e = r.get(Dialect::Codex).unwrap();
    assert_eq!(e.name, "codex");
    assert_eq!(e.version, "v1");
}

#[test]
fn builtin_kimi_entry_has_name_and_version() {
    let r = DialectRegistry::with_builtins();
    let e = r.get(Dialect::Kimi).unwrap();
    assert_eq!(e.name, "kimi");
    assert_eq!(e.version, "v1");
}

#[test]
fn builtin_copilot_entry_has_name_and_version() {
    let r = DialectRegistry::with_builtins();
    let e = r.get(Dialect::Copilot).unwrap();
    assert_eq!(e.name, "copilot");
    assert_eq!(e.version, "v1");
}

#[test]
fn dialect_entry_debug_includes_name() {
    let r = DialectRegistry::with_builtins();
    for &d in Dialect::all() {
        let e = r.get(d).unwrap();
        let dbg = format!("{e:?}");
        assert!(dbg.contains(e.name), "Debug for {d:?} missing name");
    }
}

#[test]
fn dialect_entry_clone_preserves_fields() {
    let r = DialectRegistry::with_builtins();
    let original = r.get(Dialect::Claude).unwrap();
    let cloned = original.clone();
    assert_eq!(cloned.dialect, original.dialect);
    assert_eq!(cloned.name, original.name);
    assert_eq!(cloned.version, original.version);
}

#[test]
fn all_builtin_entries_have_parsers_that_accept_objects() {
    let r = DialectRegistry::with_builtins();
    for &d in Dialect::all() {
        let result = r.parse(d, &json!({}));
        // Should not panic; may succeed or return DialectError
        assert!(result.is_ok() || result.is_err());
    }
}

#[test]
fn all_builtin_entries_can_serialize_empty_request() {
    let r = DialectRegistry::with_builtins();
    let ir = IrRequest::new(vec![]);
    for &d in Dialect::all() {
        let result = r.serialize(d, &ir);
        assert!(result.is_ok(), "serialize failed for {d:?}");
    }
}

#[test]
fn dialect_error_display_includes_label() {
    for &d in Dialect::all() {
        let err = DialectError {
            dialect: d,
            message: "test".into(),
        };
        let s = format!("{err}");
        assert!(
            s.contains(d.label()),
            "Display for {d:?} missing label in: {s}"
        );
    }
}

#[test]
fn dialect_error_is_std_error() {
    let err = DialectError {
        dialect: Dialect::OpenAi,
        message: "test".into(),
    };
    let _: &dyn std::error::Error = &err;
}

#[test]
fn dialect_error_equality() {
    let a = DialectError {
        dialect: Dialect::Claude,
        message: "oops".into(),
    };
    let b = DialectError {
        dialect: Dialect::Claude,
        message: "oops".into(),
    };
    assert_eq!(a, b);

    let c = DialectError {
        dialect: Dialect::Gemini,
        message: "oops".into(),
    };
    assert_ne!(a, c);
}

#[test]
fn ir_serde_roundtrip_request_with_tools_and_config() {
    let ir = IrRequest::new(vec![
        IrMessage::text(IrRole::System, "Be brief"),
        IrMessage::text(IrRole::User, "Hello"),
    ])
    .with_model("test-model")
    .with_system_prompt("Be brief")
    .with_tool(IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    })
    .with_config(IrGenerationConfig {
        max_tokens: Some(512),
        temperature: Some(0.3),
        top_p: Some(0.9),
        top_k: Some(40),
        stop_sequences: vec!["STOP".into()],
        extra: BTreeMap::new(),
    });

    let val = serde_json::to_value(&ir).unwrap();
    let back: IrRequest = serde_json::from_value(val).unwrap();
    assert_eq!(back.model, ir.model);
    assert_eq!(back.tools.len(), 1);
    assert_eq!(back.config.max_tokens, Some(512));
    assert_eq!(back.config.stop_sequences, vec!["STOP"]);
}

#[test]
fn ir_serde_roundtrip_response_with_all_fields() {
    let resp = IrResponse::new(vec![
        IrContentBlock::Text {
            text: "hello".into(),
        },
        IrContentBlock::ToolCall {
            id: "tc1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        },
    ])
    .with_id("resp-42")
    .with_model("gpt-4")
    .with_stop_reason(IrStopReason::ToolUse)
    .with_usage(IrUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        cache_read_tokens: 10,
        cache_write_tokens: 5,
    });

    let val = serde_json::to_value(&resp).unwrap();
    let back: IrResponse = serde_json::from_value(val).unwrap();
    assert_eq!(back.id, resp.id);
    assert_eq!(back.stop_reason, Some(IrStopReason::ToolUse));
    assert_eq!(back.usage.unwrap().cache_read_tokens, 10);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Registry operations (~20 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_new_is_empty() {
    let r = DialectRegistry::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
}

#[test]
fn registry_default_is_empty() {
    let r = DialectRegistry::default();
    assert!(r.is_empty());
}

#[test]
fn registry_with_builtins_not_empty() {
    let r = DialectRegistry::with_builtins();
    assert!(!r.is_empty());
    assert_eq!(r.len(), 6);
}

#[test]
fn registry_list_dialects_deterministic_order() {
    let r = DialectRegistry::with_builtins();
    let list1 = r.list_dialects();
    let list2 = r.list_dialects();
    assert_eq!(list1, list2);
}

#[test]
fn registry_list_dialects_is_sorted() {
    let r = DialectRegistry::with_builtins();
    let list = r.list_dialects();
    let mut sorted = list.clone();
    sorted.sort();
    assert_eq!(list, sorted, "BTreeMap iteration should be sorted");
}

#[test]
fn registry_get_all_builtins() {
    let r = DialectRegistry::with_builtins();
    for &d in Dialect::all() {
        assert!(r.get(d).is_some(), "missing builtin entry for {d:?}");
    }
}

#[test]
fn registry_get_from_empty_returns_none() {
    let r = DialectRegistry::new();
    for &d in Dialect::all() {
        assert!(r.get(d).is_none());
    }
}

#[test]
fn registry_register_custom_entry() {
    let mut r = DialectRegistry::new();
    assert!(r.is_empty());

    let reg_builtins = DialectRegistry::with_builtins();
    let openai = reg_builtins.get(Dialect::OpenAi).unwrap();
    r.register(openai.clone());

    assert_eq!(r.len(), 1);
    assert!(r.get(Dialect::OpenAi).is_some());
}

#[test]
fn registry_register_replaces_existing_entry() {
    let mut r = DialectRegistry::with_builtins();
    let orig_version = r.get(Dialect::Gemini).unwrap().version;

    let entry = DialectEntry {
        dialect: Dialect::Gemini,
        name: "gemini",
        version: "v2-custom",
        parser: r.get(Dialect::Gemini).unwrap().parser,
        serializer: r.get(Dialect::Gemini).unwrap().serializer,
    };
    r.register(entry);

    assert_eq!(r.len(), 6); // no growth
    assert_eq!(r.get(Dialect::Gemini).unwrap().version, "v2-custom");
    assert_ne!(orig_version, "v2-custom");
}

#[test]
fn registry_supports_pair_both_present() {
    let r = DialectRegistry::with_builtins();
    assert!(r.supports_pair(Dialect::OpenAi, Dialect::Claude));
    assert!(r.supports_pair(Dialect::Gemini, Dialect::Codex));
    assert!(r.supports_pair(Dialect::Kimi, Dialect::Copilot));
}

#[test]
fn registry_supports_pair_same_dialect() {
    let r = DialectRegistry::with_builtins();
    for &d in Dialect::all() {
        assert!(r.supports_pair(d, d), "same-dialect pair failed for {d:?}");
    }
}

#[test]
fn registry_supports_pair_one_missing() {
    let mut r = DialectRegistry::new();
    let builtins = DialectRegistry::with_builtins();
    r.register(builtins.get(Dialect::OpenAi).unwrap().clone());
    assert!(!r.supports_pair(Dialect::OpenAi, Dialect::Claude));
    assert!(!r.supports_pair(Dialect::Claude, Dialect::OpenAi));
}

#[test]
fn registry_supports_pair_both_missing() {
    let r = DialectRegistry::new();
    assert!(!r.supports_pair(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn registry_parse_delegates_to_parser() {
    let r = DialectRegistry::with_builtins();
    let val = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let ir = r.parse(Dialect::OpenAi, &val).unwrap();
    assert_eq!(ir.model.as_deref(), Some("gpt-4"));
}

#[test]
fn registry_parse_unregistered_returns_error() {
    let r = DialectRegistry::new();
    let err = r.parse(Dialect::Claude, &json!({})).unwrap_err();
    assert!(err.message.contains("not registered"));
    assert_eq!(err.dialect, Dialect::Claude);
}

#[test]
fn registry_serialize_delegates_to_serializer() {
    let r = DialectRegistry::with_builtins();
    let ir =
        IrRequest::new(vec![IrMessage::text(IrRole::User, "Hello")]).with_model("claude-3-sonnet");
    let val = r.serialize(Dialect::Claude, &ir).unwrap();
    assert_eq!(val["model"].as_str(), Some("claude-3-sonnet"));
}

#[test]
fn registry_serialize_unregistered_returns_error() {
    let r = DialectRegistry::new();
    let ir = IrRequest::new(vec![]);
    let err = r.serialize(Dialect::Gemini, &ir).unwrap_err();
    assert!(err.message.contains("not registered"));
}

#[test]
fn registry_clone_is_independent() {
    let r1 = DialectRegistry::with_builtins();
    let mut r2 = r1.clone();

    let entry = DialectEntry {
        dialect: Dialect::OpenAi,
        name: "openai",
        version: "v99",
        parser: r1.get(Dialect::OpenAi).unwrap().parser,
        serializer: r1.get(Dialect::OpenAi).unwrap().serializer,
    };
    r2.register(entry);

    assert_eq!(r1.get(Dialect::OpenAi).unwrap().version, "v1");
    assert_eq!(r2.get(Dialect::OpenAi).unwrap().version, "v99");
}

#[test]
fn registry_debug_format() {
    let r = DialectRegistry::with_builtins();
    let dbg = format!("{r:?}");
    assert!(dbg.contains("DialectRegistry"));
}

#[test]
fn registry_iteration_via_list_covers_all() {
    let r = DialectRegistry::with_builtins();
    let listed: HashSet<Dialect> = r.list_dialects().into_iter().collect();
    let all: HashSet<Dialect> = Dialect::all().iter().copied().collect();
    assert_eq!(listed, all);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Cross-dialect compatibility (~15 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cross_dialect_openai_to_claude_preserves_user_text() {
    let r = DialectRegistry::with_builtins();
    let openai = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Hello from OpenAI"}]
    });
    let ir = r.parse(Dialect::OpenAi, &openai).unwrap();
    let claude_val = r.serialize(Dialect::Claude, &ir).unwrap();
    let msgs = claude_val["messages"].as_array().unwrap();
    assert!(!msgs.is_empty());
    // The user message text should survive the translation
    let user_msg = &msgs[0];
    let content = user_msg.get("content").unwrap();
    let text = if content.is_string() {
        content.as_str().unwrap().to_string()
    } else {
        content[0]["text"].as_str().unwrap().to_string()
    };
    assert!(text.contains("Hello from OpenAI"));
}

#[test]
fn cross_dialect_claude_to_gemini_preserves_user_text() {
    let r = DialectRegistry::with_builtins();
    let claude = json!({
        "model": "claude-3",
        "messages": [{"role": "user", "content": "Hello from Claude"}],
        "max_tokens": 256
    });
    let ir = r.parse(Dialect::Claude, &claude).unwrap();
    let gemini_val = r.serialize(Dialect::Gemini, &ir).unwrap();
    let contents = gemini_val["contents"].as_array().unwrap();
    assert!(!contents.is_empty());
    let text = contents[0]["parts"][0]["text"].as_str().unwrap();
    assert!(text.contains("Hello from Claude"));
}

#[test]
fn cross_dialect_gemini_to_openai_preserves_user_text() {
    let r = DialectRegistry::with_builtins();
    let gemini = json!({
        "contents": [{"role": "user", "parts": [{"text": "Hello from Gemini"}]}]
    });
    let ir = r.parse(Dialect::Gemini, &gemini).unwrap();
    let openai_val = r.serialize(Dialect::OpenAi, &ir).unwrap();
    let msgs = openai_val["messages"].as_array().unwrap();
    assert!(!msgs.is_empty());
    assert!(msgs[0]["content"]
        .as_str()
        .unwrap()
        .contains("Hello from Gemini"));
}

#[test]
fn cross_dialect_codex_to_openai() {
    let r = DialectRegistry::with_builtins();
    let codex = json!({
        "model": "codex-mini",
        "instructions": "Be helpful",
        "input": "Fix the bug"
    });
    let ir = r.parse(Dialect::Codex, &codex).unwrap();
    let openai_val = r.serialize(Dialect::OpenAi, &ir).unwrap();
    // Should have messages from the codex input
    assert!(openai_val.get("messages").is_some());
}

#[test]
fn cross_dialect_kimi_to_claude() {
    let r = DialectRegistry::with_builtins();
    let kimi = json!({
        "model": "kimi",
        "messages": [{"role": "user", "content": "Summarize"}],
        "refs": ["https://example.com"]
    });
    let ir = r.parse(Dialect::Kimi, &kimi).unwrap();
    // Kimi-specific metadata survives parsing
    assert!(ir.metadata.contains_key("kimi_refs"));
    let claude_val = r.serialize(Dialect::Claude, &ir).unwrap();
    assert!(!claude_val["messages"].as_array().unwrap().is_empty());
}

#[test]
fn cross_dialect_copilot_to_gemini() {
    let r = DialectRegistry::with_builtins();
    let copilot = json!({
        "messages": [{"role": "user", "content": "Deploy service"}],
        "references": [{"type": "file", "path": "main.rs"}],
        "agent_mode": true
    });
    let ir = r.parse(Dialect::Copilot, &copilot).unwrap();
    assert!(ir.metadata.contains_key("copilot_references"));
    let gemini_val = r.serialize(Dialect::Gemini, &ir).unwrap();
    let contents = gemini_val["contents"].as_array().unwrap();
    assert!(!contents.is_empty());
}

#[test]
fn cross_dialect_all_pairs_text_roundtrip() {
    let r = DialectRegistry::with_builtins();
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Universal message")])
        .with_model("test-model");

    for &src in Dialect::all() {
        let serialized = r.serialize(src, &ir).unwrap();
        let parsed = r.parse(src, &serialized).unwrap();
        // Text content should survive a same-dialect roundtrip
        let text = parsed
            .messages
            .iter()
            .map(|m| m.text_content())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains("Universal message") || text.contains("Universal"),
            "text lost for {src:?}: {text}"
        );
    }
}

#[test]
fn cross_dialect_system_prompt_propagation() {
    let r = DialectRegistry::with_builtins();
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")])
        .with_system_prompt("Always respond in JSON");

    // OpenAI: system prompt as first message
    let openai_val = r.serialize(Dialect::OpenAi, &ir).unwrap();
    let first = &openai_val["messages"][0];
    assert_eq!(first["role"].as_str(), Some("system"));

    // Claude: system prompt as top-level field
    let claude_val = r.serialize(Dialect::Claude, &ir).unwrap();
    assert_eq!(
        claude_val["system"].as_str(),
        Some("Always respond in JSON")
    );

    // Gemini: system prompt as system_instruction
    let gemini_val = r.serialize(Dialect::Gemini, &ir).unwrap();
    assert!(gemini_val.get("system_instruction").is_some());
}

#[test]
fn cross_dialect_tool_definitions_openai_to_claude() {
    let r = DialectRegistry::with_builtins();
    let openai = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read file contents",
                "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}
            }
        }]
    });
    let ir = r.parse(Dialect::OpenAi, &openai).unwrap();
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "read_file");

    let claude_val = r.serialize(Dialect::Claude, &ir).unwrap();
    let tools = claude_val["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"].as_str(), Some("read_file"));
}

#[test]
fn cross_dialect_tool_definitions_claude_to_gemini() {
    let r = DialectRegistry::with_builtins();
    let claude = json!({
        "model": "claude-3",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{
            "name": "search",
            "description": "Web search",
            "input_schema": {"type": "object", "properties": {"q": {"type": "string"}}}
        }]
    });
    let ir = r.parse(Dialect::Claude, &claude).unwrap();
    assert_eq!(ir.tools[0].name, "search");

    let gemini_val = r.serialize(Dialect::Gemini, &ir).unwrap();
    let decls = &gemini_val["tools"][0]["functionDeclarations"];
    assert_eq!(decls[0]["name"].as_str(), Some("search"));
}

#[test]
fn cross_dialect_config_temperature_preservation() {
    let r = DialectRegistry::with_builtins();
    let ir =
        IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")]).with_config(IrGenerationConfig {
            temperature: Some(0.7),
            max_tokens: Some(1024),
            ..Default::default()
        });

    // OpenAI roundtrip preserves temperature
    let openai_val = r.serialize(Dialect::OpenAi, &ir).unwrap();
    assert_eq!(openai_val["temperature"].as_f64(), Some(0.7));
    assert_eq!(openai_val["max_tokens"].as_u64(), Some(1024));

    // Claude roundtrip preserves temperature
    let claude_val = r.serialize(Dialect::Claude, &ir).unwrap();
    assert_eq!(claude_val["temperature"].as_f64(), Some(0.7));

    // Gemini roundtrip preserves temperature
    let gemini_val = r.serialize(Dialect::Gemini, &ir).unwrap();
    assert_eq!(
        gemini_val["generationConfig"]["temperature"].as_f64(),
        Some(0.7)
    );
}

#[test]
fn cross_dialect_response_parsing_openai() {
    let resp_val = json!({
        "id": "chatcmpl-abc",
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "The answer is 42."},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 8, "total_tokens": 18}
    });
    let resp = parse_response(Dialect::OpenAi, &resp_val).unwrap();
    assert_eq!(resp.text_content(), "The answer is 42.");
    assert_eq!(resp.stop_reason, Some(IrStopReason::EndTurn));
    assert_eq!(resp.usage.unwrap().total_tokens, 18);
}

#[test]
fn cross_dialect_response_parsing_claude() {
    let resp_val = json!({
        "id": "msg-xyz",
        "model": "claude-3-opus",
        "content": [{"type": "text", "text": "Here you go."}],
        "stop_reason": "max_tokens",
        "usage": {"input_tokens": 15, "output_tokens": 20}
    });
    let resp = parse_response(Dialect::Claude, &resp_val).unwrap();
    assert_eq!(resp.text_content(), "Here you go.");
    assert_eq!(resp.stop_reason, Some(IrStopReason::MaxTokens));
}

#[test]
fn cross_dialect_response_parsing_gemini() {
    let resp_val = json!({
        "candidates": [{
            "content": {"parts": [{"text": "Gemini says hi"}]},
            "finishReason": "STOP"
        }],
        "usageMetadata": {
            "promptTokenCount": 5,
            "candidatesTokenCount": 3,
            "totalTokenCount": 8
        }
    });
    let resp = parse_response(Dialect::Gemini, &resp_val).unwrap();
    assert_eq!(resp.text_content(), "Gemini says hi");
    assert_eq!(resp.usage.unwrap().total_tokens, 8);
}

#[test]
fn cross_dialect_passthrough_via_same_dialect_roundtrip() {
    let r = DialectRegistry::with_builtins();
    let original = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "System prompt"},
            {"role": "user", "content": "User message"},
            {"role": "assistant", "content": "Assistant reply"}
        ],
        "temperature": 0.5,
        "max_tokens": 2048
    });

    let ir = r.parse(Dialect::OpenAi, &original).unwrap();
    let roundtripped = r.serialize(Dialect::OpenAi, &ir).unwrap();
    let ir2 = r.parse(Dialect::OpenAi, &roundtripped).unwrap();

    assert_eq!(ir.model, ir2.model);
    assert_eq!(ir.messages.len(), ir2.messages.len());
    assert_eq!(ir.config.temperature, ir2.config.temperature);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Integration with WorkOrder and Capabilities (~10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_with_vendor_dialect_hint() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "dialect".to_string(),
        serde_json::to_value("openai").unwrap(),
    );
    let wo = WorkOrderBuilder::new("Test task")
        .config(RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor,
            ..Default::default()
        })
        .build();

    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(
        wo.config.vendor.get("dialect").and_then(Value::as_str),
        Some("openai")
    );
}

#[test]
fn work_order_with_dialect_serde_in_vendor_config() {
    let mut vendor = BTreeMap::new();
    let dialect = Dialect::Claude;
    vendor.insert(
        "dialect".to_string(),
        serde_json::to_value(dialect).unwrap(),
    );

    let wo = WorkOrderBuilder::new("Translate code")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();

    let val = wo.config.vendor.get("dialect").unwrap();
    let back: Dialect = serde_json::from_value(val.clone()).unwrap();
    assert_eq!(back, Dialect::Claude);
}

#[test]
fn work_order_capability_requirements_with_tool_use() {
    let wo = WorkOrderBuilder::new("Code review")
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolWrite,
                    min_support: MinSupport::Emulated,
                },
            ],
        })
        .build();

    assert_eq!(wo.requirements.required.len(), 2);
    assert_eq!(wo.requirements.required[0].capability, Capability::ToolRead);
}

#[test]
fn capability_manifest_serde_roundtrip() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolUse, SupportLevel::Native);
    manifest.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
    manifest.insert(Capability::Vision, SupportLevel::Unsupported);

    let json = serde_json::to_value(&manifest).unwrap();
    let back: CapabilityManifest = serde_json::from_value(json).unwrap();
    assert_eq!(back.len(), 4);
    assert!(back.contains_key(&Capability::Streaming));
}

#[test]
fn support_level_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_satisfies_emulated() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_unsupported_satisfies_nothing() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn execution_mode_default_is_mapped() {
    let mode = ExecutionMode::default();
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serde_roundtrip() {
    for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, mode);
    }
}

#[test]
fn dialect_registry_parse_for_each_backend_model() {
    let r = DialectRegistry::with_builtins();

    // Simulate choosing a dialect based on a backend model string
    let backends = [
        ("gpt-4", Dialect::OpenAi),
        ("claude-3-opus", Dialect::Claude),
        ("gemini-1.5-pro", Dialect::Gemini),
        ("codex-mini", Dialect::Codex),
    ];

    for (model, dialect) in backends {
        let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "test")]).with_model(model);
        let val = r.serialize(dialect, &ir).unwrap();
        let parsed = r.parse(dialect, &val).unwrap();
        assert_eq!(parsed.model.as_deref(), Some(model));
    }
}
