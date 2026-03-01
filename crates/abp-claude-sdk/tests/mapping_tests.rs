// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the Claude SDK dialect mapping.

use abp_claude_sdk::dialect::{
    ClaudeConfig, ClaudeContentBlock, ClaudeResponse, map_response, map_work_order,
};
use abp_core::{AgentEventKind, ContextPacket, ContextSnippet, WorkOrderBuilder};

#[test]
fn work_order_maps_to_correct_claude_request_fields() {
    let wo = WorkOrderBuilder::new("Fix the login bug")
        .model("claude-opus-4-20250514")
        .build();
    let cfg = ClaudeConfig {
        max_tokens: 8192,
        ..ClaudeConfig::default()
    };
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.model, "claude-opus-4-20250514");
    assert_eq!(req.max_tokens, 8192);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(req.messages[0].content.contains("Fix the login bug"));
}

#[test]
fn work_order_falls_back_to_config_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = ClaudeConfig::default();
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.model, cfg.model);
}

#[test]
fn context_snippets_are_included_in_user_message() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "README".into(),
            content: "# My Project".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("Summarize").context(ctx).build();
    let cfg = ClaudeConfig::default();
    let req = map_work_order(&wo, &cfg);

    assert!(req.messages[0].content.contains("README"));
    assert!(req.messages[0].content.contains("# My Project"));
}

#[test]
fn response_with_multiple_blocks_produces_multiple_events() {
    let resp = ClaudeResponse {
        id: "msg_multi".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![
            ClaudeContentBlock::Text {
                text: "Let me check.".into(),
            },
            ClaudeContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read_file".into(),
                input: serde_json::json!({"path": "src/lib.rs"}),
            },
        ],
        stop_reason: Some("tool_use".into()),
        usage: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn default_config_values_are_sensible() {
    let cfg = ClaudeConfig::default();
    assert!(!cfg.base_url.is_empty());
    assert!(cfg.base_url.starts_with("https://"));
    assert!(!cfg.model.is_empty());
    assert!(cfg.max_tokens >= 1024);
    assert!(cfg.api_key.is_empty(), "default api_key should be empty");
    assert!(cfg.system_prompt.is_none());
}
