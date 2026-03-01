// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the Kimi SDK dialect mapping.

use abp_core::{AgentEventKind, ContextPacket, ContextSnippet, WorkOrderBuilder};
use abp_kimi_sdk::dialect::{
    KimiChoice, KimiConfig, KimiFunctionCall, KimiResponse, KimiResponseMessage, KimiToolCall,
    map_response, map_work_order,
};

#[test]
fn work_order_maps_to_correct_kimi_request_fields() {
    let wo = WorkOrderBuilder::new("Optimize database queries")
        .model("moonshot-v1-128k")
        .build();
    let cfg = KimiConfig {
        max_tokens: Some(2048),
        ..KimiConfig::default()
    };
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.model, "moonshot-v1-128k");
    assert_eq!(req.max_tokens, Some(2048));
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(
        req.messages[0]
            .content
            .contains("Optimize database queries")
    );
}

#[test]
fn work_order_falls_back_to_config_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = KimiConfig::default();
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.model, cfg.model);
}

#[test]
fn context_snippets_are_included_in_user_message() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "notes.md".into(),
            content: "Important context here.".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("Summarize notes")
        .context(ctx)
        .build();
    let cfg = KimiConfig::default();
    let req = map_work_order(&wo, &cfg);

    assert!(req.messages[0].content.contains("notes.md"));
    assert!(req.messages[0].content.contains("Important context here."));
}

#[test]
fn response_with_content_and_tool_calls_produces_events() {
    let resp = KimiResponse {
        id: "cmpl_multi".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Let me look that up.".into()),
                tool_calls: Some(vec![KimiToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "web_search".into(),
                        arguments: r#"{"query":"rust async"}"#.into(),
                    },
                }]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
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
    let cfg = KimiConfig::default();
    assert!(!cfg.base_url.is_empty());
    assert!(cfg.base_url.starts_with("https://"));
    assert!(!cfg.model.is_empty());
    assert!(cfg.max_tokens.unwrap_or(0) >= 1024);
    assert!(cfg.api_key.is_empty(), "default api_key should be empty");
    assert!(cfg.temperature.is_none());
}
