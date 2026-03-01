// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the OpenAI SDK dialect mapping.

use abp_core::{AgentEventKind, ContextPacket, ContextSnippet, WorkOrderBuilder};
use abp_openai_sdk::dialect::{
    OpenAIChoice, OpenAIConfig, OpenAIFunctionCall, OpenAIMessage, OpenAIResponse, OpenAIToolCall,
    map_response, map_work_order,
};

#[test]
fn work_order_maps_to_correct_openai_request_fields() {
    let wo = WorkOrderBuilder::new("Fix the login bug")
        .model("gpt-4-turbo")
        .build();
    let cfg = OpenAIConfig {
        max_tokens: Some(8192),
        ..OpenAIConfig::default()
    };
    let req = map_work_order(&wo, &cfg);

    assert_eq!(req.model, "gpt-4-turbo");
    assert_eq!(req.max_tokens, Some(8192));
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(
        req.messages[0]
            .content
            .as_deref()
            .unwrap()
            .contains("Fix the login bug")
    );
}

#[test]
fn work_order_falls_back_to_config_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = OpenAIConfig::default();
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
    let cfg = OpenAIConfig::default();
    let req = map_work_order(&wo, &cfg);

    let content = req.messages[0].content.as_deref().unwrap();
    assert!(content.contains("README"));
    assert!(content.contains("# My Project"));
}

#[test]
fn response_with_text_and_tool_calls_produces_multiple_events() {
    let resp = OpenAIResponse {
        id: "chatcmpl-multi".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: Some("Let me check.".into()),
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "read_file".into(),
                        arguments: r#"{"path":"src/lib.rs"}"#.into(),
                    },
                }]),
                tool_call_id: None,
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
    let cfg = OpenAIConfig::default();
    assert!(!cfg.base_url.is_empty());
    assert!(cfg.base_url.starts_with("https://"));
    assert!(!cfg.model.is_empty());
    assert!(cfg.max_tokens.unwrap_or(0) >= 1024);
    assert!(cfg.api_key.is_empty(), "default api_key should be empty");
    assert!(cfg.temperature.is_none());
}
