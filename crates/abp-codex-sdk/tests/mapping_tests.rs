// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the Codex SDK dialect mapping.

use abp_codex_sdk::dialect::{
    map_response, map_work_order, CodexConfig, CodexContentPart, CodexInputItem, CodexOutputItem,
    CodexResponse,
};
use abp_core::{AgentEventKind, ContextPacket, ContextSnippet, WorkOrderBuilder};

#[test]
fn work_order_maps_to_correct_codex_request_fields() {
    let wo = WorkOrderBuilder::new("Write unit tests")
        .model("o3-mini")
        .build();
    let cfg = CodexConfig {
        max_output_tokens: Some(2048),
        ..CodexConfig::default()
    };
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.model, "o3-mini");
    assert_eq!(req.max_output_tokens, Some(2048));
    assert_eq!(req.input.len(), 1);
    match &req.input[0] {
        CodexInputItem::Message { role, content } => {
            assert_eq!(role, "user");
            assert!(content.contains("Write unit tests"));
        }
    }
}

#[test]
fn work_order_falls_back_to_config_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CodexConfig::default();
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.model, cfg.model);
}

#[test]
fn context_snippets_are_included_in_user_message() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "schema.sql".into(),
            content: "CREATE TABLE users (id INT);".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("Add migration").context(ctx).build();
    let cfg = CodexConfig::default();
    let req = map_work_order(&wo, &cfg);

    match &req.input[0] {
        CodexInputItem::Message { content, .. } => {
            assert!(content.contains("schema.sql"));
            assert!(content.contains("CREATE TABLE"));
        }
    }
}

#[test]
fn response_with_message_and_function_call_produces_events() {
    let resp = CodexResponse {
        id: "resp_multi".into(),
        model: "codex-mini-latest".into(),
        output: vec![
            CodexOutputItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Running command.".into(),
                }],
            },
            CodexOutputItem::FunctionCall {
                id: "fc_1".into(),
                name: "shell".into(),
                arguments: r#"{"command":"cargo test"}"#.into(),
            },
        ],
        usage: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0].kind, AgentEventKind::AssistantMessage { .. }));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn default_config_values_are_sensible() {
    let cfg = CodexConfig::default();
    assert!(!cfg.base_url.is_empty());
    assert!(cfg.base_url.starts_with("https://"));
    assert!(!cfg.model.is_empty());
    assert!(cfg.max_output_tokens.unwrap_or(0) >= 1024);
    assert!(cfg.api_key.is_empty(), "default api_key should be empty");
    assert!(cfg.temperature.is_none());
}
