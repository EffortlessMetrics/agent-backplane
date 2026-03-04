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

//! Deep tests for the dialect mapping engine covering mapping rules validation,
//! early failure for unmappable requests, role mapping across dialects, tool
//! format translation, content type normalization, and validation pipeline.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapper::validation::{DefaultMappingValidator, MappingValidator, ValidationPipeline};
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, CodexClaudeIrMapper, DialectRequest,
    GeminiKimiIrMapper, IdentityMapper, IrMapper, MapError, Mapper, OpenAiClaudeIrMapper,
    OpenAiCodexIrMapper, OpenAiCopilotIrMapper, OpenAiGeminiIrMapper, OpenAiKimiIrMapper,
    default_ir_mapper, supported_ir_pairs,
};
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn simple_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ])
}

fn tool_call_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Check the weather"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "call_42".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "NYC"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_42".into(),
                content: vec![IrContentBlock::Text {
                    text: "72°F, sunny".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "It's 72°F and sunny in NYC."),
    ])
}

fn thinking_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Solve 2+2"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "2+2 is 4".into(),
                },
                IrContentBlock::Text {
                    text: "The answer is 4.".into(),
                },
            ],
        ),
    ])
}

// =========================================================================
// 1. Mapping rules validation
// =========================================================================

#[test]
fn supported_ir_pairs_is_not_empty() {
    let pairs = supported_ir_pairs();
    assert!(!pairs.is_empty());
}

#[test]
fn supported_ir_pairs_includes_all_identity() {
    let pairs = supported_ir_pairs();
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)), "missing identity pair ({d}, {d})");
    }
}

#[test]
fn factory_returns_correct_mapper_for_known_pairs() {
    // Each concrete mapper's supported_pairs() should agree with factory
    let mappers: Vec<Box<dyn IrMapper>> = vec![
        Box::new(OpenAiClaudeIrMapper),
        Box::new(OpenAiGeminiIrMapper),
        Box::new(ClaudeGeminiIrMapper),
        Box::new(OpenAiCodexIrMapper),
        Box::new(OpenAiKimiIrMapper),
        Box::new(ClaudeKimiIrMapper),
        Box::new(OpenAiCopilotIrMapper),
        Box::new(GeminiKimiIrMapper),
        Box::new(CodexClaudeIrMapper),
    ];

    for mapper in &mappers {
        for (from, to) in mapper.supported_pairs() {
            let factory_mapper = default_ir_mapper(from, to);
            assert!(
                factory_mapper.is_some(),
                "factory should resolve ({from}, {to})"
            );
        }
    }
}

#[test]
fn each_mapper_has_at_least_two_supported_pairs() {
    let mappers: Vec<Box<dyn IrMapper>> = vec![
        Box::new(OpenAiClaudeIrMapper),
        Box::new(OpenAiGeminiIrMapper),
        Box::new(ClaudeGeminiIrMapper),
        Box::new(OpenAiCodexIrMapper),
        Box::new(OpenAiKimiIrMapper),
        Box::new(ClaudeKimiIrMapper),
        Box::new(OpenAiCopilotIrMapper),
        Box::new(GeminiKimiIrMapper),
        Box::new(CodexClaudeIrMapper),
    ];

    for mapper in &mappers {
        let pairs = mapper.supported_pairs();
        assert!(
            pairs.len() >= 2,
            "mapper should support at least 2 pairs (bidirectional), got {}",
            pairs.len()
        );
    }
}

// =========================================================================
// 2. Early failure for unmappable requests
// =========================================================================

#[test]
fn unsupported_pair_returns_error_on_map_request() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = simple_ir();
    let result = mapper.map_request(Dialect::Gemini, Dialect::Kimi, &ir);
    assert!(matches!(result, Err(MapError::UnsupportedPair { .. })));
}

#[test]
fn unsupported_pair_returns_error_on_map_response() {
    let mapper = OpenAiGeminiIrMapper;
    let ir = simple_ir();
    let result = mapper.map_response(Dialect::Claude, Dialect::Kimi, &ir);
    assert!(matches!(result, Err(MapError::UnsupportedPair { .. })));
}

#[test]
fn map_error_unsupported_pair_contains_dialects() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let msg = err.to_string();
    assert!(msg.contains("Kimi"));
    assert!(msg.contains("Copilot"));
}

#[test]
fn map_error_unmappable_tool_contains_name() {
    let err = MapError::UnmappableTool {
        name: "computer_use".into(),
        reason: "not supported in target".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("computer_use"));
}

// =========================================================================
// 3. Role mapping across dialects
// =========================================================================

#[test]
fn system_role_preserved_openai_to_claude() {
    let ir = simple_ir();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    // System message should be present (possibly extracted)
    let sys = mapped.system_message();
    assert!(sys.is_some(), "system message lost in OpenAI→Claude");
    assert!(sys.unwrap().text_content().contains("helpful"));
}

#[test]
fn user_role_preserved_across_all_supported_pairs() {
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "test input")]);

    for (from, to) in supported_ir_pairs() {
        if from == to {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let mapped = mapper.map_request(from, to, &ir).unwrap();
        let user_msgs = mapped.messages_by_role(IrRole::User);
        assert!(!user_msgs.is_empty(), "user role lost in {from}→{to}");
    }
}

#[test]
fn assistant_role_preserved_openai_to_claude() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "hi"),
        IrMessage::text(IrRole::Assistant, "hello"),
    ]);
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert!(mapped.last_assistant().is_some());
    assert!(
        mapped
            .last_assistant()
            .unwrap()
            .text_content()
            .contains("hello")
    );
}

#[test]
fn tool_role_remapped_to_user_in_openai_to_claude() {
    // OpenAI Tool-role messages become User-role with ToolResult blocks in Claude.
    let ir = tool_call_ir();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let tool_msgs = mapped.messages_by_role(IrRole::Tool);
    assert!(
        tool_msgs.is_empty(),
        "Tool role should be absent in Claude dialect"
    );
    // The ToolResult blocks should now live inside User-role messages.
    let user_msgs = mapped.messages_by_role(IrRole::User);
    let has_tool_result = user_msgs.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
    });
    assert!(
        has_tool_result,
        "ToolResult should appear in a User message"
    );
}

// =========================================================================
// 4. Tool format translation
// =========================================================================

#[test]
fn tool_use_block_survives_openai_to_claude() {
    let ir = tool_call_ir();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let tool_calls = mapped.tool_calls();
    assert!(!tool_calls.is_empty(), "tool_use blocks lost");
    // Verify the tool name is preserved
    if let IrContentBlock::ToolUse { name, .. } = tool_calls[0] {
        assert_eq!(name, "get_weather");
    } else {
        panic!("expected ToolUse block");
    }
}

#[test]
fn tool_result_block_survives_openai_to_claude() {
    // After mapping, ToolResult blocks move from Tool-role to User-role.
    let ir = tool_call_ir();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let user_msgs = mapped.messages_by_role(IrRole::User);
    assert!(!user_msgs.is_empty());
    let has_tool_result = user_msgs.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
    });
    assert!(has_tool_result, "ToolResult block lost");
}

#[test]
fn tool_use_roundtrip_openai_claude_openai() {
    let ir = tool_call_ir();
    let fwd = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let bwd = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();

    let intermediate = fwd
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let roundtripped = bwd
        .map_request(Dialect::Claude, Dialect::OpenAi, &intermediate)
        .unwrap();

    let original_tools = ir.tool_calls();
    let rt_tools = roundtripped.tool_calls();
    assert_eq!(
        original_tools.len(),
        rt_tools.len(),
        "tool call count mismatch after round-trip"
    );
}

// =========================================================================
// 5. Content type normalization
// =========================================================================

#[test]
fn text_content_preserved_across_identity_mapping() {
    let ir = simple_ir();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(ir.messages.len(), mapped.messages.len());
    for (orig, m) in ir.messages.iter().zip(mapped.messages.iter()) {
        assert_eq!(orig.text_content(), m.text_content());
    }
}

#[test]
fn thinking_block_handled_claude_to_openai() {
    let ir = thinking_ir();
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
    let mapped = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    // The text answer should survive
    let assistant = mapped.last_assistant();
    assert!(assistant.is_some(), "assistant message lost");
    let text = assistant.unwrap().text_content();
    assert!(text.contains("4"), "answer text lost: {text}");
}

#[test]
fn empty_conversation_maps_to_empty() {
    let ir = IrConversation::new();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let mapped = mapper.map_request(from, to, &ir).unwrap();
        assert!(
            mapped.is_empty(),
            "empty conv became non-empty for {from}→{to}"
        );
    }
}

#[test]
fn multi_message_conversation_preserves_order() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "first"),
        IrMessage::text(IrRole::Assistant, "second"),
        IrMessage::text(IrRole::User, "third"),
        IrMessage::text(IrRole::Assistant, "fourth"),
    ]);
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    // Verify message order is preserved
    let texts: Vec<String> = mapped.messages.iter().map(|m| m.text_content()).collect();
    let first_idx = texts.iter().position(|t| t.contains("first"));
    let third_idx = texts.iter().position(|t| t.contains("third"));
    assert!(first_idx.is_some(), "first message lost");
    assert!(third_idx.is_some(), "third message lost");
    assert!(
        first_idx.unwrap() < third_idx.unwrap(),
        "message order not preserved"
    );
}

// =========================================================================
// 6. Validation pipeline integration
// =========================================================================

#[test]
fn validation_pipeline_passes_valid_openai_request() {
    let pipe = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::OpenAi,
    );
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let result = pipe.run(&req, |v| Ok(v.clone()));
    assert!(result.pre.is_valid());
    assert!(result.mapped.is_some());
    assert!(result.post.as_ref().unwrap().is_valid());
}

#[test]
fn validation_pipeline_rejects_invalid_request() {
    let pipe = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::OpenAi,
    );
    let req = json!({"not_a_valid_field": true});
    let result = pipe.run(&req, |_| panic!("should not map"));
    assert!(!result.pre.is_valid());
    assert!(result.mapped.is_none());
}

#[test]
fn validation_detects_missing_claude_max_tokens() {
    let v = DefaultMappingValidator::new();
    let req = json!({"model": "claude-3", "messages": [{"role": "user", "content": "hi"}]});
    let result = v.validate_pre_mapping(Dialect::Claude, &req);
    assert!(!result.is_valid());
    assert!(result.error_count() > 0);
}

#[test]
fn validation_detects_missing_gemini_contents() {
    let v = DefaultMappingValidator::new();
    let req = json!({"model": "gemini-pro"});
    let result = v.validate_pre_mapping(Dialect::Gemini, &req);
    assert!(!result.is_valid());
}

#[test]
fn validation_roundtrip_detects_lost_field() {
    let v = DefaultMappingValidator::new();
    let original = json!({"model": "gpt-4", "temperature": 0.7, "top_p": 0.9});
    let roundtripped = json!({"model": "gpt-4"});
    let result = v.validate_roundtrip(&original, &roundtripped);
    assert!(!result.is_lossless());
    assert!(!result.lost_fields.is_empty());
}

// =========================================================================
// 7. Identity mapper preserves all dialects
// =========================================================================

#[test]
fn identity_mapper_map_request_preserves_body() {
    let mapper = IdentityMapper;
    for &d in Dialect::all() {
        let req = DialectRequest {
            dialect: d,
            body: json!({"test": true, "dialect": d.label()}),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result, req.body);
    }
}

#[test]
fn identity_mapper_source_and_target_are_openai() {
    let mapper = IdentityMapper;
    assert_eq!(mapper.source_dialect(), Dialect::OpenAi);
    assert_eq!(mapper.target_dialect(), Dialect::OpenAi);
}
