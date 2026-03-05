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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
//! Deep integration tests for IR normalization, validation, and transformation.
//!
//! Exercises the public API of `abp_dialect::ir` (IrRequest, IrResponse,
//! IrMessage, IrContentBlock, etc.) and `abp_mapper` (IrMapper trait,
//! factory, identity mapper) for construction, serde round-trips, builder
//! patterns, edge cases, and cross-dialect identity mapping.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_dialect::ir::{
    IrContentBlock as DialectBlock, IrGenerationConfig, IrMessage as DialectMsg, IrRequest,
    IrResponse, IrRole as DialectRole, IrStopReason, IrToolDefinition as DialectTool,
    IrUsage as DialectUsage,
};
use abp_mapper::{IrIdentityMapper, IrMapper, default_ir_mapper};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// §1  IR Validation — type construction & field-level constraints (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn valid_ir_request_passes_construction() {
    let msg = DialectMsg::text(DialectRole::User, "Hello");
    let req = IrRequest::new(vec![msg])
        .with_model("gpt-4o")
        .with_system_prompt("You are helpful.");
    assert_eq!(req.model.as_deref(), Some("gpt-4o"));
    assert_eq!(req.system_prompt.as_deref(), Some("You are helpful."));
    assert_eq!(req.messages.len(), 1);
    assert!(req.tools.is_empty());
}

#[test]
fn empty_model_string_round_trips() {
    // An empty model string is representable — the IR layer does not reject it.
    let req = IrRequest::new(vec![]).with_model("");
    let json = serde_json::to_value(&req).unwrap();
    let back: IrRequest = serde_json::from_value(json).unwrap();
    assert_eq!(back.model.as_deref(), Some(""));
}

#[test]
fn empty_messages_array_is_valid() {
    let req = IrRequest::new(vec![]);
    let json = serde_json::to_string(&req).unwrap();
    let back: IrRequest = serde_json::from_str(&json).unwrap();
    assert!(back.messages.is_empty());
}

#[test]
fn max_tokens_large_value_preserved() {
    let cfg = IrGenerationConfig {
        max_tokens: Some(u64::MAX),
        ..Default::default()
    };
    let req = IrRequest::new(vec![]).with_config(cfg);
    let json = serde_json::to_value(&req).unwrap();
    let back: IrRequest = serde_json::from_value(json).unwrap();
    assert_eq!(back.config.max_tokens, Some(u64::MAX));
}

#[test]
fn temperature_extreme_values_preserved() {
    for &temp in &[0.0_f64, 2.0, -1.0, 100.0] {
        let cfg = IrGenerationConfig {
            temperature: Some(temp),
            ..Default::default()
        };
        let json = serde_json::to_value(&cfg).unwrap();
        let back: IrGenerationConfig = serde_json::from_value(json).unwrap();
        assert!(
            (back.temperature.unwrap() - temp).abs() < f64::EPSILON,
            "temperature {temp} not preserved"
        );
    }
}

#[test]
fn tool_definition_requires_all_fields() {
    // All three fields are required by the struct — deserialization of partial
    // JSON should fail.
    let partial = json!({"name": "bash"});
    let res = serde_json::from_value::<DialectTool>(partial);
    assert!(res.is_err(), "missing description & parameters should fail");
}

#[test]
fn duplicate_tool_names_are_representable() {
    // The IR layer does not enforce uniqueness — duplicates are preserved.
    let tool = DialectTool {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: json!({"type": "object"}),
    };
    let req = IrRequest::new(vec![])
        .with_tool(tool.clone())
        .with_tool(tool.clone());
    assert_eq!(req.tools.len(), 2);
    assert_eq!(req.tools[0].name, req.tools[1].name);
}

#[test]
fn message_role_serde_round_trip() {
    for role in [
        DialectRole::System,
        DialectRole::User,
        DialectRole::Assistant,
        DialectRole::Tool,
    ] {
        let msg = DialectMsg::text(role, "x");
        let json = serde_json::to_value(&msg).unwrap();
        let back: DialectMsg = serde_json::from_value(json).unwrap();
        assert_eq!(back.role, role);
    }
}

#[test]
fn content_block_tag_discriminator() {
    // Blocks use `"type"` as the serde tag.
    let block = DialectBlock::Text { text: "hi".into() };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "text");

    let tool = DialectBlock::ToolCall {
        id: "t1".into(),
        name: "bash".into(),
        input: json!({}),
    };
    let v = serde_json::to_value(&tool).unwrap();
    assert_eq!(v["type"], "tool_call");
}

#[test]
fn nested_tool_result_content_blocks() {
    let inner = DialectBlock::Text {
        text: "result data".into(),
    };
    let result = DialectBlock::ToolResult {
        tool_call_id: "tc_1".into(),
        content: vec![inner],
        is_error: false,
    };
    let json = serde_json::to_value(&result).unwrap();
    let back: DialectBlock = serde_json::from_value(json).unwrap();
    if let DialectBlock::ToolResult { content, .. } = &back {
        assert_eq!(content.len(), 1);
        assert_eq!(content[0].as_text(), Some("result data"));
    } else {
        panic!("expected ToolResult");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  IR Normalization — idempotency, defaults, metadata (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn whitespace_in_content_is_preserved_by_ir() {
    // The IR layer does not trim whitespace — it preserves it exactly.
    let msg = DialectMsg::text(DialectRole::User, "  hello  ");
    assert_eq!(msg.text_content(), "  hello  ");
}

#[test]
fn empty_system_message_preserved() {
    // An empty system message is a valid IrMessage.
    let msg = DialectMsg::text(DialectRole::System, "");
    assert_eq!(msg.text_content(), "");
    assert_eq!(msg.role, DialectRole::System);
}

#[test]
fn consecutive_same_role_messages_stay_separate() {
    // The IR layer does not merge consecutive same-role messages.
    let msgs = vec![
        DialectMsg::text(DialectRole::User, "a"),
        DialectMsg::text(DialectRole::User, "b"),
    ];
    let req = IrRequest::new(msgs);
    assert_eq!(req.messages.len(), 2);
}

#[test]
fn tool_call_id_preserved_through_serde() {
    let block = DialectBlock::ToolCall {
        id: "call_abc123".into(),
        name: "bash".into(),
        input: json!({"cmd": "ls"}),
    };
    let json = serde_json::to_value(&block).unwrap();
    let back: DialectBlock = serde_json::from_value(json).unwrap();
    if let DialectBlock::ToolCall { id, .. } = &back {
        assert_eq!(id, "call_abc123");
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn model_name_preserved_exactly() {
    let req = IrRequest::new(vec![]).with_model("claude-sonnet-4-20250514");
    let json = serde_json::to_value(&req).unwrap();
    let back: IrRequest = serde_json::from_value(json).unwrap();
    assert_eq!(back.model.as_deref(), Some("claude-sonnet-4-20250514"));
}

#[test]
fn generation_config_defaults() {
    let cfg = IrGenerationConfig::default();
    assert!(cfg.max_tokens.is_none());
    assert!(cfg.temperature.is_none());
    assert!(cfg.top_p.is_none());
    assert!(cfg.top_k.is_none());
    assert!(cfg.stop_sequences.is_empty());
    assert!(cfg.extra.is_empty());
}

#[test]
fn response_format_metadata_preserved() {
    let mut meta = BTreeMap::new();
    meta.insert("response_format".into(), json!({"type": "json_object"}));
    let req = IrRequest {
        model: None,
        system_prompt: None,
        messages: vec![],
        tools: vec![],
        config: IrGenerationConfig::default(),
        metadata: meta.clone(),
    };
    let json = serde_json::to_value(&req).unwrap();
    let back: IrRequest = serde_json::from_value(json).unwrap();
    assert_eq!(back.metadata, meta);
}

#[test]
fn metadata_key_ordering_deterministic() {
    // BTreeMap guarantees deterministic key ordering.
    let mut meta = BTreeMap::new();
    meta.insert("zebra".into(), json!(1));
    meta.insert("alpha".into(), json!(2));
    meta.insert("middle".into(), json!(3));

    let msg = DialectMsg {
        role: DialectRole::User,
        content: vec![],
        metadata: meta,
    };
    let json_str = serde_json::to_string(&msg).unwrap();
    let alpha_pos = json_str.find("alpha").unwrap();
    let middle_pos = json_str.find("middle").unwrap();
    let zebra_pos = json_str.find("zebra").unwrap();
    assert!(alpha_pos < middle_pos && middle_pos < zebra_pos);
}

#[test]
fn idempotent_serde_round_trip() {
    // Serializing → deserializing twice yields identical JSON.
    let req = IrRequest::new(vec![
        DialectMsg::text(DialectRole::System, "You are a bot."),
        DialectMsg::text(DialectRole::User, "Hi"),
    ])
    .with_model("gpt-4o")
    .with_config(IrGenerationConfig {
        max_tokens: Some(1024),
        temperature: Some(0.7),
        ..Default::default()
    });

    let json1 = serde_json::to_string(&req).unwrap();
    let rt1: IrRequest = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string(&rt1).unwrap();
    assert_eq!(
        json1, json2,
        "normalize(normalize(x)) should equal normalize(x)"
    );
}

#[test]
fn identity_mapper_is_idempotent() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::text(IrRole::Assistant, "Hi there!"));

    let mapper = IrIdentityMapper;
    let once = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();
    let twice = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &once)
        .unwrap();
    assert_eq!(once, twice);
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  IR Transformation — serde, conversion, aggregation (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_request_json_round_trip() {
    let tool = DialectTool {
        name: "read_file".into(),
        description: "Read a file from disk".into(),
        parameters: json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        }),
    };
    let req = IrRequest::new(vec![DialectMsg::text(DialectRole::User, "read foo.txt")])
        .with_model("gpt-4o")
        .with_tool(tool);

    let json = serde_json::to_value(&req).unwrap();
    let back: IrRequest = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(back.model, req.model);
    assert_eq!(back.tools.len(), 1);
    assert_eq!(back.tools[0].name, "read_file");
}

#[test]
fn ir_response_json_round_trip() {
    let resp = IrResponse::text("The answer is 42.")
        .with_id("resp_abc")
        .with_model("gpt-4o")
        .with_stop_reason(IrStopReason::EndTurn)
        .with_usage(DialectUsage::from_io(100, 50));

    let json = serde_json::to_value(&resp).unwrap();
    let back: IrResponse = serde_json::from_value(json).unwrap();
    assert_eq!(back.id.as_deref(), Some("resp_abc"));
    assert_eq!(back.text_content(), "The answer is 42.");
    assert_eq!(back.stop_reason, Some(IrStopReason::EndTurn));
    assert_eq!(back.usage.unwrap().total_tokens, 150);
}

#[test]
fn unknown_fields_in_extra_metadata_preserved() {
    let mut extra = BTreeMap::new();
    extra.insert("vendor_specific_flag".into(), json!(true));
    extra.insert("custom_counter".into(), json!(42));

    let cfg = IrGenerationConfig {
        extra: extra.clone(),
        ..Default::default()
    };
    let json = serde_json::to_value(&cfg).unwrap();
    let back: IrGenerationConfig = serde_json::from_value(json).unwrap();
    assert_eq!(back.extra, extra);
}

#[test]
fn tool_call_argument_serialization() {
    let complex_args = json!({
        "path": "/tmp/test.rs",
        "content": "fn main() {}",
        "options": {"overwrite": true, "mode": 0o644}
    });
    let block = DialectBlock::ToolCall {
        id: "tc_1".into(),
        name: "write_file".into(),
        input: complex_args.clone(),
    };
    let json = serde_json::to_value(&block).unwrap();
    let back: DialectBlock = serde_json::from_value(json).unwrap();
    if let DialectBlock::ToolCall { input, .. } = back {
        assert_eq!(input, complex_args);
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn usage_statistics_aggregation() {
    let u1 = DialectUsage::from_io(100, 50);
    let u2 = DialectUsage::from_io(200, 80);
    let merged = u1.merge(u2);
    assert_eq!(merged.input_tokens, 300);
    assert_eq!(merged.output_tokens, 130);
    assert_eq!(merged.total_tokens, 430);
}

#[test]
fn multi_block_content_assembly() {
    let msg = DialectMsg {
        role: DialectRole::Assistant,
        content: vec![
            DialectBlock::Text {
                text: "Here is ".into(),
            },
            DialectBlock::Text {
                text: "the answer.".into(),
            },
        ],
        metadata: BTreeMap::new(),
    };
    assert_eq!(msg.text_content(), "Here is the answer.");
}

#[test]
fn thinking_block_preservation() {
    let block = DialectBlock::Thinking {
        text: "Let me reason about this...".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "thinking");
    assert_eq!(json["text"], "Let me reason about this...");

    let back: DialectBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back.as_text(), None); // Thinking.as_text() returns None
    if let DialectBlock::Thinking { text } = back {
        assert_eq!(text, "Let me reason about this...");
    } else {
        panic!("expected Thinking");
    }
}

#[test]
fn custom_content_block_handling() {
    let block = DialectBlock::Custom {
        custom_type: "citation".into(),
        data: json!({"source": "arxiv:2301.00001", "text": "Relevant finding"}),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "custom");
    assert_eq!(json["custom_type"], "citation");

    let back: DialectBlock = serde_json::from_value(json).unwrap();
    if let DialectBlock::Custom { custom_type, data } = back {
        assert_eq!(custom_type, "citation");
        assert_eq!(data["source"], "arxiv:2301.00001");
    } else {
        panic!("expected Custom");
    }
}

#[test]
fn stop_reason_all_variants_serde() {
    let variants = vec![
        IrStopReason::EndTurn,
        IrStopReason::StopSequence,
        IrStopReason::MaxTokens,
        IrStopReason::ToolUse,
        IrStopReason::ContentFilter,
        IrStopReason::Other("custom_reason".into()),
    ];
    for reason in &variants {
        let resp = IrResponse::text("x").with_stop_reason(reason.clone());
        let json = serde_json::to_value(&resp).unwrap();
        let back: IrResponse = serde_json::from_value(json).unwrap();
        assert_eq!(back.stop_reason.as_ref(), Some(reason));
    }
}

#[test]
fn error_response_representation() {
    // A ToolResult with is_error=true represents an error response.
    let error_block = DialectBlock::ToolResult {
        tool_call_id: "tc_err".into(),
        content: vec![DialectBlock::Text {
            text: "Command failed: permission denied".into(),
        }],
        is_error: true,
    };
    let json = serde_json::to_value(&error_block).unwrap();
    assert_eq!(json["is_error"], true);
    let back: DialectBlock = serde_json::from_value(json).unwrap();
    if let DialectBlock::ToolResult {
        is_error, content, ..
    } = back
    {
        assert!(is_error);
        assert_eq!(
            content[0].as_text(),
            Some("Command failed: permission denied")
        );
    } else {
        panic!("expected ToolResult");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Bonus: mapper factory & cross-dialect structural tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn default_mapper_returns_identity_for_same_dialect() {
    for &dialect in Dialect::all() {
        let mapper = default_ir_mapper(dialect, dialect);
        assert!(
            mapper.is_some(),
            "identity mapper should exist for {dialect:?}"
        );
    }
}

#[test]
fn unsupported_pair_returns_none() {
    // Copilot ↔ Codex is not directly supported.
    let mapper = default_ir_mapper(Dialect::Copilot, Dialect::Codex);
    assert!(mapper.is_none());
}

#[test]
fn identity_mapper_preserves_tool_calls() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "bash".into(),
            input: json!({"command": "echo hi"}),
        }],
    )]);

    let mapper = IrIdentityMapper;
    let mapped = mapper
        .map_request(Dialect::Claude, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(mapped.tool_calls().len(), 1);
}

#[test]
fn ir_conversation_accessors() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::text(IrRole::Assistant, "Hi!"));

    assert_eq!(conv.len(), 3);
    assert!(!conv.is_empty());
    assert_eq!(
        conv.system_message().unwrap().text_content(),
        "You are helpful."
    );
    assert_eq!(conv.last_assistant().unwrap().text_content(), "Hi!");
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
    assert_eq!(conv.last_message().unwrap().text_content(), "Hi!");
}

#[test]
fn ir_request_all_tool_calls() {
    let req = IrRequest::new(vec![
        DialectMsg::new(
            DialectRole::Assistant,
            vec![DialectBlock::ToolCall {
                id: "tc_1".into(),
                name: "bash".into(),
                input: json!({}),
            }],
        ),
        DialectMsg::new(
            DialectRole::Assistant,
            vec![DialectBlock::ToolCall {
                id: "tc_2".into(),
                name: "read".into(),
                input: json!({}),
            }],
        ),
    ]);
    assert_eq!(req.all_tool_calls().len(), 2);
}

#[test]
fn ir_response_has_tool_calls() {
    let resp = IrResponse::new(vec![
        DialectBlock::Text {
            text: "Let me check".into(),
        },
        DialectBlock::ToolCall {
            id: "tc_1".into(),
            name: "search".into(),
            input: json!({"query": "rust"}),
        },
    ]);
    assert!(resp.has_tool_calls());
    assert_eq!(resp.tool_calls().len(), 1);
    assert_eq!(resp.text_content(), "Let me check");
}

#[test]
fn ir_usage_from_io_computes_total() {
    let u = DialectUsage::from_io(500, 200);
    assert_eq!(u.total_tokens, 700);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
}

#[test]
fn ir_usage_default_is_zero() {
    let u = DialectUsage::default();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn generation_config_stop_sequences_round_trip() {
    let cfg = IrGenerationConfig {
        stop_sequences: vec!["</s>".into(), "\n\n".into()],
        ..Default::default()
    };
    let json = serde_json::to_value(&cfg).unwrap();
    let back: IrGenerationConfig = serde_json::from_value(json).unwrap();
    assert_eq!(back.stop_sequences, vec!["</s>", "\n\n"]);
}

#[test]
fn audio_content_block_round_trip() {
    let block = DialectBlock::Audio {
        media_type: "audio/wav".into(),
        data: "UklGRiQAAABXQVZF".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "audio");
    let back: DialectBlock = serde_json::from_value(json).unwrap();
    if let DialectBlock::Audio { media_type, data } = back {
        assert_eq!(media_type, "audio/wav");
        assert_eq!(data, "UklGRiQAAABXQVZF");
    } else {
        panic!("expected Audio");
    }
}

#[test]
fn image_content_block_round_trip() {
    let block = DialectBlock::Image {
        media_type: "image/png".into(),
        data: "iVBORw0KGgo=".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "image");
    let back: DialectBlock = serde_json::from_value(json).unwrap();
    if let DialectBlock::Image { media_type, data } = back {
        assert_eq!(media_type, "image/png");
        assert_eq!(data, "iVBORw0KGgo=");
    } else {
        panic!("expected Image");
    }
}
