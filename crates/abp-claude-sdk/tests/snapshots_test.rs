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
use abp_claude_sdk::dialect::*;
use abp_core::WorkOrderBuilder;
use insta::assert_json_snapshot;

#[test]
fn snapshot_default_config() {
    assert_json_snapshot!("claude_default_config", ClaudeConfig::default());
}

#[test]
fn snapshot_mapped_request() {
    let wo = WorkOrderBuilder::new("Refactor auth module").build();
    let cfg = ClaudeConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_json_snapshot!("claude_mapped_request", req);
}

#[test]
fn snapshot_mapped_response_events() {
    let resp = ClaudeResponse {
        id: "msg_snapshot".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![
            ClaudeContentBlock::Text {
                text: "I'll refactor the auth module.".into(),
            },
            ClaudeContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read_file".into(),
                input: serde_json::json!({"path": "src/auth.rs"}),
            },
        ],
        stop_reason: Some("tool_use".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 120,
            output_tokens: 35,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let events: Vec<_> = map_response(&resp).into_iter().map(|e| e.kind).collect();
    assert_json_snapshot!("claude_mapped_response_events", events);
}
