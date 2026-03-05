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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Deep tests for IR normalization and transformation.
//!
//! Covers 12 test categories: message normalization, content block normalization,
//! role normalization, empty content handling, whitespace normalization, tool
//! definition normalization, usage normalization, conversation normalization,
//! metadata preservation, lossy transformation marking, round-trip consistency,
//! and cross-dialect consistency.

use abp_ir::lower::*;
use abp_ir::normalize::*;
use abp_ir::*;
use abp_sdk_types::Dialect;
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════
// 1. Message normalization — various input formats → canonical IR
// ═══════════════════════════════════════════════════════════════════════

mod message_normalization {
    use super::*;

    #[test]
    fn simple_text_message_preserved() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
        let n = normalize(&conv);
        assert_eq!(n.len(), 1);
        assert_eq!(n.messages[0].text_content(), "hello");
    }

    #[test]
    fn system_user_assistant_triple_normalizes() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys  "))
            .push(IrMessage::text(IrRole::User, " hi "))
            .push(IrMessage::text(IrRole::Assistant, " bye "));
        let n = normalize(&conv);
        assert_eq!(n.len(), 3);
        assert_eq!(n.messages[0].text_content(), "sys");
        assert_eq!(n.messages[1].text_content(), "hi");
        assert_eq!(n.messages[2].text_content(), "bye");
    }

    #[test]
    fn scattered_system_messages_merged_into_leading_message() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "rule1"))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::System, "rule2"));
        let n = normalize(&conv);
        assert_eq!(n.messages[0].role, IrRole::System);
        assert_eq!(n.messages[0].text_content(), "rule1\nrule2");
    }

    #[test]
    fn adjacent_text_blocks_merged_in_single_message() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "part1".into(),
                },
                IrContentBlock::Text {
                    text: "part2".into(),
                },
            ],
        ));
        let n = normalize(&conv);
        assert_eq!(n.messages[0].content.len(), 1);
        assert_eq!(n.messages[0].text_content(), "part1part2");
    }

    #[test]
    fn normalize_is_idempotent() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys  "))
            .push(IrMessage::text(IrRole::User, " hi "))
            .push(IrMessage::text(IrRole::System, " extra "));
        let once = normalize(&conv);
        let twice = normalize(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn message_with_tool_use_and_text_preserves_tool_use() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "  let me check  ".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "search".into(),
                    input: json!({"q": "test"}),
                },
            ],
        ));
        let n = normalize(&conv);
        assert_eq!(n.messages[0].text_content(), "let me check");
        assert_eq!(n.messages[0].tool_use_blocks().len(), 1);
    }

    #[test]
    fn normalize_empty_conversation_returns_empty() {
        let conv = IrConversation::new();
        let n = normalize(&conv);
        assert!(n.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Content block normalization — Text, tool calls, tool results
// ═══════════════════════════════════════════════════════════════════════

mod content_block_normalization {
    use super::*;

    #[test]
    fn text_block_trim_applied() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "  hello  ".into(),
            }],
        ));
        let n = trim_text(&conv);
        assert_eq!(n.messages[0].text_content(), "hello");
    }

    #[test]
    fn tool_use_block_input_unchanged_by_normalize() {
        let input = json!({"path": "/tmp/test.rs", "nested": {"deep": true}});
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read_file".into(),
                input: input.clone(),
            }],
        ));
        let n = normalize(&conv);
        if let IrContentBlock::ToolUse {
            input: actual, id, ..
        } = &n.messages[0].content[0]
        {
            assert_eq!(*actual, input);
            assert_eq!(id, "t1");
        } else {
            panic!("expected ToolUse block");
        }
    }

    #[test]
    fn tool_result_content_preserved_through_normalize() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "  result data  ".into(),
                }],
                is_error: false,
            }],
        ));
        let n = normalize(&conv);
        if let IrContentBlock::ToolResult { content, .. } = &n.messages[0].content[0] {
            if let IrContentBlock::Text { text } = &content[0] {
                // trim_text only operates on top-level content blocks, not nested
                assert_eq!(text, "  result data  ");
            }
        }
    }

    #[test]
    fn tool_result_error_flag_preserved() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "error: not found".into(),
                }],
                is_error: true,
            }],
        ));
        let n = normalize(&conv);
        if let IrContentBlock::ToolResult { is_error, .. } = &n.messages[0].content[0] {
            assert!(is_error);
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn image_block_passes_through_normalize() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBOR...".into(),
            }],
        ));
        let n = normalize(&conv);
        if let IrContentBlock::Image { media_type, data } = &n.messages[0].content[0] {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "iVBOR...");
        } else {
            panic!("expected Image block");
        }
    }

    #[test]
    fn thinking_block_passes_through_normalize() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "  let me think  ".into(),
                },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
            ],
        ));
        let n = normalize(&conv);
        // Thinking blocks are not text, not trimmed by trim_text
        if let IrContentBlock::Thinking { text } = &n.messages[0].content[0] {
            assert_eq!(text, "  let me think  ");
        }
    }

    #[test]
    fn mixed_content_blocks_ordering_preserved() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking { text: "hmm".into() },
                IrContentBlock::Text {
                    text: "response".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "tool".into(),
                    input: json!({}),
                },
            ],
        ));
        let n = normalize(&conv);
        assert!(matches!(
            n.messages[0].content[0],
            IrContentBlock::Thinking { .. }
        ));
        assert!(matches!(
            n.messages[0].content[1],
            IrContentBlock::Text { .. }
        ));
        assert!(matches!(
            n.messages[0].content[2],
            IrContentBlock::ToolUse { .. }
        ));
    }

    #[test]
    fn tool_result_with_multiple_content_blocks() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![
                    IrContentBlock::Text {
                        text: "line 1\n".into(),
                    },
                    IrContentBlock::Text {
                        text: "line 2".into(),
                    },
                ],
                is_error: false,
            }],
        ));
        let n = normalize(&conv);
        if let IrContentBlock::ToolResult { content, .. } = &n.messages[0].content[0] {
            assert_eq!(content.len(), 2);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Role normalization — vendor role strings → IR roles
// ═══════════════════════════════════════════════════════════════════════

mod role_normalization {
    use super::*;

    #[test]
    fn canonical_system_role() {
        assert_eq!(normalize_role("system"), Some(IrRole::System));
    }

    #[test]
    fn canonical_user_role() {
        assert_eq!(normalize_role("user"), Some(IrRole::User));
    }

    #[test]
    fn canonical_assistant_role() {
        assert_eq!(normalize_role("assistant"), Some(IrRole::Assistant));
    }

    #[test]
    fn canonical_tool_role() {
        assert_eq!(normalize_role("tool"), Some(IrRole::Tool));
    }

    #[test]
    fn gemini_model_maps_to_assistant() {
        assert_eq!(normalize_role("model"), Some(IrRole::Assistant));
    }

    #[test]
    fn openai_function_maps_to_tool() {
        assert_eq!(normalize_role("function"), Some(IrRole::Tool));
    }

    #[test]
    fn openai_developer_maps_to_system() {
        assert_eq!(normalize_role("developer"), Some(IrRole::System));
    }

    #[test]
    fn anthropic_human_maps_to_user() {
        assert_eq!(normalize_role("human"), Some(IrRole::User));
    }

    #[test]
    fn bot_alias_maps_to_assistant() {
        assert_eq!(normalize_role("bot"), Some(IrRole::Assistant));
    }

    #[test]
    fn unknown_role_returns_none() {
        assert_eq!(normalize_role("narrator"), None);
        assert_eq!(normalize_role("moderator"), None);
        assert_eq!(normalize_role("observer"), None);
    }

    #[test]
    fn empty_string_returns_none() {
        assert_eq!(normalize_role(""), None);
    }

    #[test]
    fn role_matching_is_case_sensitive() {
        assert_eq!(normalize_role("System"), None);
        assert_eq!(normalize_role("USER"), None);
        assert_eq!(normalize_role("Assistant"), None);
        assert_eq!(normalize_role("Model"), None);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Empty content handling — empty strings, null content
// ═══════════════════════════════════════════════════════════════════════

mod empty_content_handling {
    use super::*;

    #[test]
    fn strip_empty_removes_no_content_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::new(IrRole::Assistant, vec![]));
        let n = strip_empty(&conv);
        assert_eq!(n.len(), 1);
        assert_eq!(n.messages[0].role, IrRole::User);
    }

    #[test]
    fn strip_empty_keeps_non_empty_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let n = strip_empty(&conv);
        assert_eq!(n.len(), 2);
    }

    #[test]
    fn strip_empty_all_empty_yields_empty() {
        let conv = IrConversation::new()
            .push(IrMessage::new(IrRole::User, vec![]))
            .push(IrMessage::new(IrRole::Assistant, vec![]));
        let n = strip_empty(&conv);
        assert!(n.is_empty());
    }

    #[test]
    fn empty_text_block_is_not_stripped_by_strip_empty() {
        // strip_empty only removes messages with zero content blocks,
        // not messages with empty text blocks
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, ""));
        let n = strip_empty(&conv);
        assert_eq!(n.len(), 1);
    }

    #[test]
    fn normalize_pipeline_keeps_empty_text_messages() {
        // The full pipeline trims whitespace but does not remove empty-text messages
        // because they still have a content block
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "  "));
        let n = normalize(&conv);
        // After trim_text the text becomes "" but the message still has a content block
        assert_eq!(n.len(), 1);
    }

    #[test]
    fn conversation_operations_on_empty() {
        let conv = IrConversation::new();
        assert!(conv.system_message().is_none());
        assert!(conv.last_assistant().is_none());
        assert!(conv.last_message().is_none());
        assert!(conv.tool_calls().is_empty());
        assert_eq!(conv.messages_by_role(IrRole::User).len(), 0);
    }

    #[test]
    fn message_with_no_content_blocks_text_content_is_empty() {
        let msg = IrMessage::new(IrRole::User, vec![]);
        assert_eq!(msg.text_content(), "");
        assert!(msg.is_text_only()); // vacuously true
        assert!(msg.tool_use_blocks().is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Whitespace normalization — leading/trailing whitespace
// ═══════════════════════════════════════════════════════════════════════

mod whitespace_normalization {
    use super::*;

    #[test]
    fn trim_text_strips_leading_and_trailing_spaces() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "  hello  "));
        let n = trim_text(&conv);
        assert_eq!(n.messages[0].text_content(), "hello");
    }

    #[test]
    fn trim_text_handles_tabs_and_newlines() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "\t\nhello\n\t"));
        let n = trim_text(&conv);
        assert_eq!(n.messages[0].text_content(), "hello");
    }

    #[test]
    fn trim_text_preserves_interior_whitespace() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello   world"));
        let n = trim_text(&conv);
        assert_eq!(n.messages[0].text_content(), "hello   world");
    }

    #[test]
    fn trim_text_all_whitespace_becomes_empty() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "   \t\n   "));
        let n = trim_text(&conv);
        assert_eq!(n.messages[0].text_content(), "");
    }

    #[test]
    fn trim_text_leaves_tool_use_untouched() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "  c1  ".into(),
                name: "  search  ".into(),
                input: json!({"q": "  spaces  "}),
            }],
        ));
        let n = trim_text(&conv);
        if let IrContentBlock::ToolUse { id, name, input } = &n.messages[0].content[0] {
            assert_eq!(id, "  c1  ");
            assert_eq!(name, "  search  ");
            assert_eq!(input["q"], "  spaces  ");
        }
    }

    #[test]
    fn trim_text_leaves_image_data_untouched() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "  base64data  ".into(),
            }],
        ));
        let n = trim_text(&conv);
        if let IrContentBlock::Image { data, .. } = &n.messages[0].content[0] {
            assert_eq!(data, "  base64data  ");
        }
    }

    #[test]
    fn trim_text_leaves_thinking_untouched() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "  thinking  ".into(),
            }],
        ));
        let n = trim_text(&conv);
        if let IrContentBlock::Thinking { text } = &n.messages[0].content[0] {
            assert_eq!(text, "  thinking  ");
        }
    }

    #[test]
    fn trim_text_in_tool_result_nested_blocks() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "  inner  ".into(),
                }],
                is_error: false,
            }],
        ));
        let n = trim_text(&conv);
        // trim_text operates at top level only; nested ToolResult content is not trimmed
        if let IrContentBlock::ToolResult { content, .. } = &n.messages[0].content[0] {
            if let IrContentBlock::Text { text } = &content[0] {
                // ToolResult is not a Text block, so inner content is not trimmed
                assert_eq!(text, "  inner  ");
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Tool definition normalization — function schemas normalized
// ═══════════════════════════════════════════════════════════════════════

mod tool_definition_normalization {
    use super::*;

    #[test]
    fn normalize_schemas_adds_missing_type_object() {
        let tools = vec![IrToolDefinition {
            name: "search".into(),
            description: "Search".into(),
            parameters: json!({"properties": {"q": {"type": "string"}}}),
        }];
        let normalized = normalize_tool_schemas(&tools);
        assert_eq!(normalized[0].parameters["type"], "object");
    }

    #[test]
    fn normalize_schemas_preserves_existing_type() {
        let tools = vec![IrToolDefinition {
            name: "calc".into(),
            description: "Calc".into(),
            parameters: json!({"type": "object", "properties": {}}),
        }];
        let normalized = normalize_tool_schemas(&tools);
        assert_eq!(normalized[0].parameters["type"], "object");
    }

    #[test]
    fn normalize_schemas_preserves_all_properties() {
        let tools = vec![IrToolDefinition {
            name: "fetch".into(),
            description: "Fetch URL".into(),
            parameters: json!({
                "properties": {
                    "url": {"type": "string"},
                    "method": {"type": "string", "enum": ["GET", "POST"]}
                },
                "required": ["url"]
            }),
        }];
        let normalized = normalize_tool_schemas(&tools);
        assert_eq!(normalized[0].parameters["type"], "object");
        assert!(normalized[0].parameters["required"].is_array());
        assert!(normalized[0].parameters["properties"]["url"].is_object());
    }

    #[test]
    fn normalize_schemas_empty_list() {
        let tools: Vec<IrToolDefinition> = vec![];
        let normalized = normalize_tool_schemas(&tools);
        assert!(normalized.is_empty());
    }

    #[test]
    fn sort_tools_alphabetical_order() {
        let mut tools = vec![
            IrToolDefinition {
                name: "zebra".into(),
                description: "z".into(),
                parameters: json!({}),
            },
            IrToolDefinition {
                name: "apple".into(),
                description: "a".into(),
                parameters: json!({}),
            },
            IrToolDefinition {
                name: "mango".into(),
                description: "m".into(),
                parameters: json!({}),
            },
        ];
        sort_tools(&mut tools);
        assert_eq!(tools[0].name, "apple");
        assert_eq!(tools[1].name, "mango");
        assert_eq!(tools[2].name, "zebra");
    }

    #[test]
    fn sort_tools_already_sorted_is_noop() {
        let mut tools = vec![
            IrToolDefinition {
                name: "alpha".into(),
                description: "a".into(),
                parameters: json!({}),
            },
            IrToolDefinition {
                name: "beta".into(),
                description: "b".into(),
                parameters: json!({}),
            },
        ];
        let expected = tools.clone();
        sort_tools(&mut tools);
        assert_eq!(tools, expected);
    }

    #[test]
    fn sort_tools_single_tool() {
        let mut tools = vec![IrToolDefinition {
            name: "only".into(),
            description: "".into(),
            parameters: json!({}),
        }];
        sort_tools(&mut tools);
        assert_eq!(tools[0].name, "only");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Usage normalization — token counts, cost calculations
// ═══════════════════════════════════════════════════════════════════════

mod usage_normalization {
    use super::*;

    #[test]
    fn from_io_computes_total() {
        let usage = IrUsage::from_io(100, 200);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 200);
        assert_eq!(usage.total_tokens, 300);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_write_tokens, 0);
    }

    #[test]
    fn with_cache_computes_total() {
        let usage = IrUsage::with_cache(100, 200, 50, 30);
        assert_eq!(usage.total_tokens, 300);
        assert_eq!(usage.cache_read_tokens, 50);
        assert_eq!(usage.cache_write_tokens, 30);
    }

    #[test]
    fn merge_sums_all_fields() {
        let a = IrUsage::with_cache(10, 20, 5, 3);
        let b = IrUsage::with_cache(100, 200, 50, 30);
        let merged = a.merge(b);
        assert_eq!(merged.input_tokens, 110);
        assert_eq!(merged.output_tokens, 220);
        assert_eq!(merged.total_tokens, 330);
        assert_eq!(merged.cache_read_tokens, 55);
        assert_eq!(merged.cache_write_tokens, 33);
    }

    #[test]
    fn default_usage_is_zero() {
        let usage = IrUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    #[test]
    fn merge_with_zero_is_identity() {
        let usage = IrUsage::from_io(10, 20);
        let merged = usage.merge(IrUsage::default());
        assert_eq!(usage, merged);
    }

    #[test]
    fn usage_serialization_roundtrip() {
        let usage = IrUsage::with_cache(42, 58, 10, 5);
        let json_str = serde_json::to_string(&usage).unwrap();
        let deserialized: IrUsage = serde_json::from_str(&json_str).unwrap();
        assert_eq!(usage, deserialized);
    }

    #[test]
    fn merge_is_commutative() {
        let a = IrUsage::with_cache(10, 20, 5, 3);
        let b = IrUsage::with_cache(100, 200, 50, 30);
        assert_eq!(a.merge(b), b.merge(a));
    }

    #[test]
    fn merge_is_associative() {
        let a = IrUsage::from_io(1, 2);
        let b = IrUsage::from_io(3, 4);
        let c = IrUsage::from_io(5, 6);
        assert_eq!(a.merge(b).merge(c), a.merge(b.merge(c)));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Conversation normalization — multi-turn conversation structure
// ═══════════════════════════════════════════════════════════════════════

mod conversation_normalization {
    use super::*;

    #[test]
    fn dedup_system_merges_scattered_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "rule1"))
            .push(IrMessage::text(IrRole::User, "q1"))
            .push(IrMessage::text(IrRole::System, "rule2"))
            .push(IrMessage::text(IrRole::Assistant, "a1"));
        let n = dedup_system(&conv);
        assert_eq!(n.messages[0].role, IrRole::System);
        assert_eq!(n.messages[0].text_content(), "rule1\nrule2");
        assert_eq!(n.len(), 3);
    }

    #[test]
    fn dedup_system_preserves_non_system_order() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "first"))
            .push(IrMessage::text(IrRole::System, "sys2"))
            .push(IrMessage::text(IrRole::Assistant, "second"))
            .push(IrMessage::text(IrRole::User, "third"));
        let n = dedup_system(&conv);
        assert_eq!(n.messages[1].text_content(), "first");
        assert_eq!(n.messages[2].text_content(), "second");
        assert_eq!(n.messages[3].text_content(), "third");
    }

    #[test]
    fn dedup_system_no_system_messages_leaves_unchanged() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let n = dedup_system(&conv);
        assert_eq!(n.len(), 2);
        assert_eq!(n.messages[0].role, IrRole::User);
    }

    #[test]
    fn dedup_system_single_system_at_start_preserved() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "instructions"))
            .push(IrMessage::text(IrRole::User, "hi"));
        let n = dedup_system(&conv);
        assert_eq!(n.len(), 2);
        assert_eq!(n.messages[0].text_content(), "instructions");
    }

    #[test]
    fn merge_adjacent_text_coalesces_sequential_blocks() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text { text: "A".into() },
                IrContentBlock::Text { text: "B".into() },
                IrContentBlock::Text { text: "C".into() },
            ],
        ));
        let n = merge_adjacent_text(&conv);
        assert_eq!(n.messages[0].content.len(), 1);
        assert_eq!(n.messages[0].text_content(), "ABC");
    }

    #[test]
    fn merge_adjacent_text_does_not_merge_across_non_text() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "before".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "t".into(),
                    input: json!({}),
                },
                IrContentBlock::Text {
                    text: "after".into(),
                },
            ],
        ));
        let n = merge_adjacent_text(&conv);
        assert_eq!(n.messages[0].content.len(), 3);
    }

    #[test]
    fn full_pipeline_applies_all_passes() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  rule1  "))
            .push(IrMessage::new(IrRole::User, vec![]))
            .push(IrMessage::new(
                IrRole::User,
                vec![
                    IrContentBlock::Text { text: " A ".into() },
                    IrContentBlock::Text { text: " B ".into() },
                ],
            ))
            .push(IrMessage::text(IrRole::System, "  rule2  "));
        let n = normalize(&conv);
        // Systems merged (joined with \n) then outer whitespace trimmed
        assert_eq!(n.messages[0].role, IrRole::System);
        assert_eq!(n.messages[0].text_content(), "rule1  \n  rule2");
        // Empty message stripped
        // Adjacent text merged and trimmed
        assert_eq!(n.len(), 2);
        assert_eq!(n.messages[1].text_content(), "AB");
    }

    #[test]
    fn multi_turn_tool_use_conversation() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be helpful"))
            .push(IrMessage::text(IrRole::User, "search for rust"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                }],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "Rust is a language".into(),
                    }],
                    is_error: false,
                }],
            ))
            .push(IrMessage::text(IrRole::Assistant, "Found it!"));
        let n = normalize(&conv);
        assert_eq!(n.len(), 5);
        assert_eq!(n.tool_calls().len(), 1);
        assert_eq!(n.last_assistant().unwrap().text_content(), "Found it!");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Metadata preservation — vendor-specific metadata kept
// ═══════════════════════════════════════════════════════════════════════

mod metadata_preservation {
    use super::*;

    #[test]
    fn metadata_survives_normalize_pipeline() {
        let mut meta = BTreeMap::new();
        meta.insert("vendor_id".to_string(), json!("abc-123"));
        meta.insert("model".to_string(), json!("gpt-4"));
        let msg = IrMessage {
            role: IrRole::Assistant,
            content: vec![IrContentBlock::Text {
                text: " hello ".into(),
            }],
            metadata: meta,
        };
        let conv = IrConversation::from_messages(vec![msg]);
        let n = normalize(&conv);
        assert_eq!(n.messages[0].metadata["vendor_id"], "abc-123");
        assert_eq!(n.messages[0].metadata["model"], "gpt-4");
    }

    #[test]
    fn strip_metadata_selective_keep() {
        let mut meta = BTreeMap::new();
        meta.insert("source".to_string(), json!("test"));
        meta.insert("vendor_id".to_string(), json!("abc"));
        meta.insert("trace_id".to_string(), json!("xyz"));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        };
        let conv = IrConversation::from_messages(vec![msg]);
        let stripped = strip_metadata(&conv, &["source", "trace_id"]);
        assert_eq!(stripped.messages[0].metadata.len(), 2);
        assert!(stripped.messages[0].metadata.contains_key("source"));
        assert!(stripped.messages[0].metadata.contains_key("trace_id"));
        assert!(!stripped.messages[0].metadata.contains_key("vendor_id"));
    }

    #[test]
    fn strip_metadata_removes_all_when_empty_keep() {
        let mut meta = BTreeMap::new();
        meta.insert("x".to_string(), json!(1));
        meta.insert("y".to_string(), json!(2));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        };
        let conv = IrConversation::from_messages(vec![msg]);
        let stripped = strip_metadata(&conv, &[]);
        assert!(stripped.messages[0].metadata.is_empty());
    }

    #[test]
    fn strip_metadata_no_metadata_is_noop() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let stripped = strip_metadata(&conv, &["anything"]);
        assert!(stripped.messages[0].metadata.is_empty());
    }

    #[test]
    fn metadata_preserved_through_dedup_system() {
        let mut meta = BTreeMap::new();
        meta.insert("tag".to_string(), json!("important"));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        };
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(msg);
        let n = dedup_system(&conv);
        assert_eq!(n.messages[1].metadata["tag"], "important");
    }

    #[test]
    fn metadata_btreemap_deterministic_serialization() {
        let mut meta = BTreeMap::new();
        meta.insert("z_key".to_string(), json!("last"));
        meta.insert("a_key".to_string(), json!("first"));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        };
        let json_str = serde_json::to_string(&msg).unwrap();
        // BTreeMap ensures a_key comes before z_key
        let a_pos = json_str.find("a_key").unwrap();
        let z_pos = json_str.find("z_key").unwrap();
        assert!(a_pos < z_pos);
    }

    #[test]
    fn metadata_with_complex_json_values() {
        let mut meta = BTreeMap::new();
        meta.insert(
            "config".to_string(),
            json!({"nested": {"deep": true}, "list": [1, 2, 3]}),
        );
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        };
        let conv = IrConversation::from_messages(vec![msg]);
        let n = normalize(&conv);
        assert_eq!(n.messages[0].metadata["config"]["nested"]["deep"], true);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Lossy transformation marking — track what was lost
// ═══════════════════════════════════════════════════════════════════════

mod lossy_transformation_marking {
    use super::*;

    #[test]
    fn gemini_lowering_drops_thinking_blocks() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "reasoning here".into(),
                },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
            ],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let parts = lowered["contents"][0]["parts"].as_array().unwrap();
        // Thinking blocks are lost in Gemini format
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "answer");
    }

    #[test]
    fn gemini_thinking_only_message_produces_empty_parts() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "just thinking".into(),
            }],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let parts = lowered["contents"][0]["parts"].as_array().unwrap();
        assert!(parts.is_empty());
    }

    #[test]
    fn claude_lowering_maps_tool_role_to_user() {
        // Claude doesn't have a "tool" role — tool results go under "user"
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        assert_eq!(lowered["messages"][0]["role"], "user");
    }

    #[test]
    fn openai_tool_result_flattens_content_to_string() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![
                    IrContentBlock::Text {
                        text: "part1".into(),
                    },
                    IrContentBlock::Text {
                        text: "part2".into(),
                    },
                ],
                is_error: false,
            }],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        // OpenAI flattens multiple text blocks into a single string
        assert_eq!(lowered["messages"][0]["content"], "part1part2");
    }

    #[test]
    fn openai_tool_result_loses_is_error_flag() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "error".into(),
                }],
                is_error: true,
            }],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        // OpenAI format has no is_error field
        assert!(lowered["messages"][0].get("is_error").is_none());
    }

    #[test]
    fn extract_system_separates_system_from_conversation() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be concise"))
            .push(IrMessage::text(IrRole::User, "hi"));
        let (sys, rest) = extract_system(&conv);
        assert_eq!(sys.unwrap(), "Be concise");
        // System message is lost from the conversation
        assert!(rest.messages.iter().all(|m| m.role != IrRole::System));
    }

    #[test]
    fn extract_system_skips_empty_system_text() {
        let conv = IrConversation::new()
            .push(IrMessage::new(IrRole::System, vec![]))
            .push(IrMessage::text(IrRole::User, "hi"));
        let (sys, _rest) = extract_system(&conv);
        // Empty system message is treated as absent
        assert!(sys.is_none());
    }

    #[test]
    fn strip_metadata_is_lossy() {
        let mut meta = BTreeMap::new();
        meta.insert("internal_id".to_string(), json!("secret-123"));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        };
        let conv = IrConversation::from_messages(vec![msg]);
        let stripped = strip_metadata(&conv, &[]);
        assert!(stripped.messages[0].metadata.is_empty());
        // Original data is lost
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Round-trip consistency — normalize(denormalize(x)) == normalize(x)
// ═══════════════════════════════════════════════════════════════════════

mod round_trip_consistency {
    use super::*;

    #[test]
    fn normalize_is_idempotent_simple() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "  hello  "));
        let once = normalize(&conv);
        let twice = normalize(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn normalize_is_idempotent_with_system_dedup() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  a  "))
            .push(IrMessage::text(IrRole::User, " q "))
            .push(IrMessage::text(IrRole::System, "  b  "));
        let once = normalize(&conv);
        let twice = normalize(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn normalize_is_idempotent_with_merged_text() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text { text: " A ".into() },
                IrContentBlock::Text { text: " B ".into() },
            ],
        ));
        let once = normalize(&conv);
        let twice = normalize(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn normalize_is_idempotent_with_empty_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::new(IrRole::User, vec![]))
            .push(IrMessage::text(IrRole::User, "hi"));
        let once = normalize(&conv);
        let twice = normalize(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn serde_roundtrip_conversation() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "response".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "c1".into(),
                        name: "search".into(),
                        input: json!({"q": "test"}),
                    },
                ],
            ));
        let json_str = serde_json::to_string(&conv).unwrap();
        let deserialized: IrConversation = serde_json::from_str(&json_str).unwrap();
        assert_eq!(conv, deserialized);
    }

    #[test]
    fn serde_roundtrip_preserves_metadata() {
        let mut meta = BTreeMap::new();
        meta.insert("key".to_string(), json!({"nested": true}));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        };
        let json_str = serde_json::to_string(&msg).unwrap();
        let deserialized: IrMessage = serde_json::from_str(&json_str).unwrap();
        assert_eq!(msg, deserialized);
    }

    #[test]
    fn normalize_then_serialize_then_deserialize_then_normalize_stable() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys  "))
            .push(IrMessage::text(IrRole::User, " hi "))
            .push(IrMessage::text(IrRole::System, " extra "));
        let normalized = normalize(&conv);
        let json_str = serde_json::to_string(&normalized).unwrap();
        let deserialized: IrConversation = serde_json::from_str(&json_str).unwrap();
        let re_normalized = normalize(&deserialized);
        assert_eq!(normalized, re_normalized);
    }

    #[test]
    fn serde_roundtrip_all_content_block_types() {
        let blocks = vec![
            IrContentBlock::Text {
                text: "hello".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "search".into(),
                input: json!({"q": "test"}),
            },
            IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            },
            IrContentBlock::Thinking { text: "hmm".into() },
        ];
        for block in &blocks {
            let json_str = serde_json::to_string(block).unwrap();
            let deserialized: IrContentBlock = serde_json::from_str(&json_str).unwrap();
            assert_eq!(*block, deserialized);
        }
    }

    #[test]
    fn normalize_idempotent_complex_conversation() {
        let mut meta = BTreeMap::new();
        meta.insert("key".to_string(), json!("value"));
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "  instructions  "),
            IrMessage {
                role: IrRole::User,
                content: vec![
                    IrContentBlock::Text {
                        text: "  hello  ".into(),
                    },
                    IrContentBlock::Text {
                        text: "  world  ".into(),
                    },
                ],
                metadata: meta,
            },
            IrMessage::new(IrRole::Assistant, vec![]),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "  thought  ".into(),
                    },
                    IrContentBlock::Text {
                        text: "  answer  ".into(),
                    },
                ],
            ),
            IrMessage::text(IrRole::System, "  more instructions  "),
        ]);
        let once = normalize(&conv);
        let twice = normalize(&once);
        assert_eq!(once, twice);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Cross-dialect consistency — same logical message → same IR
// ═══════════════════════════════════════════════════════════════════════

mod cross_dialect_consistency {
    use super::*;

    fn sample_conv() -> IrConversation {
        IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are helpful."))
            .push(IrMessage::text(IrRole::User, "Hi"))
            .push(IrMessage::text(IrRole::Assistant, "Hello!"))
    }

    fn sample_tools() -> Vec<IrToolDefinition> {
        vec![IrToolDefinition {
            name: "calc".into(),
            description: "Math evaluator".into(),
            parameters: json!({
                "type": "object",
                "properties": {"expr": {"type": "string"}}
            }),
        }]
    }

    #[test]
    fn kimi_matches_openai_output() {
        let conv = sample_conv();
        let tools = sample_tools();
        assert_eq!(lower_to_openai(&conv, &tools), lower_to_kimi(&conv, &tools));
    }

    #[test]
    fn codex_matches_openai_output() {
        let conv = sample_conv();
        let tools = sample_tools();
        assert_eq!(
            lower_to_openai(&conv, &tools),
            lower_to_codex(&conv, &tools)
        );
    }

    #[test]
    fn copilot_matches_openai_output() {
        let conv = sample_conv();
        let tools = sample_tools();
        assert_eq!(
            lower_to_openai(&conv, &tools),
            lower_to_copilot(&conv, &tools)
        );
    }

    #[test]
    fn all_dialects_produce_json_objects() {
        let conv = sample_conv();
        let tools = sample_tools();
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &tools);
            assert!(
                lowered.is_object(),
                "{dialect}: should produce a JSON object"
            );
        }
    }

    #[test]
    fn no_tools_omits_tools_key_in_all_dialects() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            assert!(
                lowered.get("tools").is_none(),
                "{dialect}: tools should be omitted when empty"
            );
        }
    }

    #[test]
    fn all_openai_compatible_dialects_have_same_structure() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hello"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "search".into(),
                    input: json!({"q": "test"}),
                }],
            ));
        let openai = lower_to_openai(&conv, &[]);
        let kimi = lower_to_kimi(&conv, &[]);
        let codex = lower_to_codex(&conv, &[]);
        let copilot = lower_to_copilot(&conv, &[]);
        // All OpenAI-compatible dialects produce identical output
        assert_eq!(openai, kimi);
        assert_eq!(openai, codex);
        assert_eq!(openai, copilot);
    }

    #[test]
    fn claude_has_system_as_top_level_field() {
        let conv = sample_conv();
        let lowered = lower_to_claude(&conv, &[]);
        assert_eq!(lowered["system"], "You are helpful.");
        let msgs = lowered["messages"].as_array().unwrap();
        assert!(msgs.iter().all(|m| m["role"] != "system"));
    }

    #[test]
    fn gemini_has_system_instruction_field() {
        let conv = sample_conv();
        let lowered = lower_to_gemini(&conv, &[]);
        assert_eq!(
            lowered["system_instruction"]["parts"][0]["text"],
            "You are helpful."
        );
    }

    #[test]
    fn openai_has_system_as_inline_message() {
        let conv = sample_conv();
        let lowered = lower_to_openai(&conv, &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are helpful.");
    }

    #[test]
    fn claude_uses_input_schema_not_parameters() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let tools = sample_tools();
        let lowered = lower_to_claude(&conv, &tools);
        let tool = &lowered["tools"][0];
        assert!(tool.get("input_schema").is_some());
        assert!(tool.get("parameters").is_none());
    }

    #[test]
    fn gemini_uses_function_declarations() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let tools = sample_tools();
        let lowered = lower_to_gemini(&conv, &tools);
        assert!(lowered["tools"][0].get("function_declarations").is_some());
    }

    #[test]
    fn role_mapping_differs_by_dialect() {
        // Assistant → "model" in Gemini, "assistant" elsewhere
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::Gemini),
            "model"
        );
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::OpenAi),
            "assistant"
        );
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::Claude),
            "assistant"
        );
    }

    #[test]
    fn tool_role_mapping_differs_by_dialect() {
        // Tool → "tool" in OpenAI, "user" in Claude/Gemini
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::OpenAi), "tool");
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Claude), "user");
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Gemini), "user");
    }

    #[test]
    fn user_role_consistent_across_all_dialects() {
        for dialect in Dialect::all() {
            assert_eq!(
                ir_role_to_dialect(IrRole::User, *dialect),
                "user",
                "{dialect}: User should always map to 'user'"
            );
        }
    }
}
