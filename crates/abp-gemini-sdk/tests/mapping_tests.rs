// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the Gemini SDK dialect mapping.

use abp_gemini_sdk::dialect::{
    map_response, map_work_order, GeminiCandidate, GeminiConfig, GeminiContent, GeminiPart,
    GeminiResponse,
};
use abp_core::{AgentEventKind, ContextPacket, ContextSnippet, WorkOrderBuilder};

#[test]
fn work_order_maps_to_correct_gemini_request_fields() {
    let wo = WorkOrderBuilder::new("Migrate to async")
        .model("gemini-2.5-pro")
        .build();
    let cfg = GeminiConfig {
        max_output_tokens: Some(2048),
        ..GeminiConfig::default()
    };
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.model, "gemini-2.5-pro");
    assert_eq!(req.contents.len(), 1);
    assert_eq!(req.contents[0].role, "user");
    match &req.contents[0].parts[0] {
        GeminiPart::Text(t) => assert!(t.contains("Migrate to async")),
        other => panic!("expected Text, got {other:?}"),
    }
    assert!(req.generation_config.is_some());
    assert_eq!(req.generation_config.unwrap().max_output_tokens, Some(2048));
}

#[test]
fn work_order_falls_back_to_config_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = GeminiConfig::default();
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.model, cfg.model);
}

#[test]
fn context_snippets_are_included_in_user_message() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "config.yaml".into(),
            content: "port: 8080".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("Update config").context(ctx).build();
    let cfg = GeminiConfig::default();
    let req = map_work_order(&wo, &cfg);

    match &req.contents[0].parts[0] {
        GeminiPart::Text(t) => {
            assert!(t.contains("config.yaml"));
            assert!(t.contains("port: 8080"));
        }
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn response_with_text_and_function_call_produces_events() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![
                    GeminiPart::Text("Let me search.".into()),
                    GeminiPart::FunctionCall {
                        name: "web_search".into(),
                        args: serde_json::json!({"query": "rust"}),
                    },
                ],
            },
            finish_reason: None,
        }],
        usage_metadata: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0].kind, AgentEventKind::AssistantMessage { .. }));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn default_config_values_are_sensible() {
    let cfg = GeminiConfig::default();
    assert!(!cfg.base_url.is_empty());
    assert!(cfg.base_url.starts_with("https://"));
    assert!(!cfg.model.is_empty());
    assert!(cfg.max_output_tokens.unwrap_or(0) >= 1024);
    assert!(cfg.api_key.is_empty(), "default api_key should be empty");
    assert!(cfg.temperature.is_none());
}
