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
        }),
    };
    let events: Vec<_> = map_response(&resp)
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_json_snapshot!("claude_mapped_response_events", events);
}
