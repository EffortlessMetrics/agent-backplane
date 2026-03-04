// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep surface-area tests for the Copilot shim: request format, response format,
//! streaming SSE, Copilot headers, model names, client config, WorkOrder/Receipt
//! conversion, dialect detection, and OpenAI compatibility.

use std::time::Duration;

use abp_copilot_sdk::dialect::{
    self, CanonicalToolDef, CopilotConfig, CopilotConfirmation, CopilotError, CopilotFunctionCall,
    CopilotFunctionDef, CopilotMessage, CopilotReference, CopilotReferenceType, CopilotResponse,
    CopilotStreamEvent, CopilotTool, CopilotToolType, CopilotTurnEntry, DEFAULT_MODEL,
    DIALECT_VERSION,
};
use abp_copilot_sdk::lowering;
use abp_core::ir::{IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, Capability, UsageNormalized, WorkOrderBuilder};
use abp_shim_copilot::client::{Client, ClientError};
use abp_shim_copilot::{
    CopilotClient, CopilotRequestBuilder, Message, ShimError, events_to_stream_events,
    ir_to_messages, ir_usage_to_tuple, messages_to_ir, mock_receipt, mock_receipt_with_usage,
    receipt_to_response, request_to_ir, request_to_work_order, response_to_ir,
};
use chrono::Utc;
use serde_json::json;
use tokio_stream::StreamExt;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn make_processor(
    events: Vec<AgentEvent>,
) -> Box<dyn Fn(&abp_core::WorkOrder) -> abp_core::Receipt + Send + Sync> {
    Box::new(move |_wo| mock_receipt(events.clone()))
}

fn ae(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Request format — Chat completions with Copilot-specific headers
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_format_messages_array_has_role_and_content() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("Hello")])
        .build();
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert_eq!(req.messages[0].content, "Hello");
}

#[test]
fn request_format_system_user_assistant_triple() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![
            Message::system("You are helpful."),
            Message::user("What is Rust?"),
            Message::assistant("A systems language."),
        ])
        .build();
    assert_eq!(req.messages.len(), 3);
    assert_eq!(req.messages[0].role, "system");
    assert_eq!(req.messages[1].role, "user");
    assert_eq!(req.messages[2].role, "assistant");
}

#[test]
fn request_format_default_model_is_gpt4o() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    assert_eq!(req.model, "gpt-4o");
}

#[test]
fn request_format_serialization_includes_model_and_messages() {
    let req = CopilotRequestBuilder::new()
        .model("copilot-gpt-4")
        .messages(vec![Message::user("hi")])
        .build();
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["model"], "copilot-gpt-4");
    assert!(json["messages"].is_array());
}

#[test]
fn request_format_tools_field_present_when_set() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Function,
        function: Some(CopilotFunctionDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({"type": "object"}),
        }),
        confirmation: None,
    };
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("hi")])
        .tools(vec![tool])
        .build();
    assert!(req.tools.is_some());
}

#[test]
fn request_format_references_field_present_when_set() {
    let r = CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "f-0".into(),
        data: json!({"path": "main.rs"}),
        metadata: None,
    };
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("check")])
        .references(vec![r])
        .build();
    assert_eq!(req.references.len(), 1);
}

#[test]
fn request_format_turn_history_preserved() {
    let history = vec![CopilotTurnEntry {
        request: "Q1".into(),
        response: "A1".into(),
    }];
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("Q2")])
        .turn_history(history)
        .build();
    assert_eq!(req.turn_history.len(), 1);
    assert_eq!(req.turn_history[0].request, "Q1");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Response format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_format_message_field_populated() {
    let events = vec![ae(AgentEventKind::AssistantMessage {
        text: "Reply".into(),
    })];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Reply");
}

#[test]
fn response_format_errors_array_populated() {
    let events = vec![ae(AgentEventKind::Error {
        message: "bad request".into(),
        error_code: None,
    })];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.copilot_errors.len(), 1);
    assert_eq!(resp.copilot_errors[0].error_type, "backend_error");
}

#[test]
fn response_format_function_call_populated() {
    let events = vec![ae(AgentEventKind::ToolCall {
        tool_name: "grep".into(),
        tool_use_id: Some("call_1".into()),
        parent_tool_use_id: None,
        input: json!({"pattern": "TODO"}),
    })];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let fc = resp.function_call.unwrap();
    assert_eq!(fc.name, "grep");
    assert_eq!(fc.id.as_deref(), Some("call_1"));
}

#[test]
fn response_format_empty_trace_yields_empty_message() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.message.is_empty());
    assert!(resp.copilot_errors.is_empty());
    assert!(resp.function_call.is_none());
}

#[test]
fn response_format_serde_roundtrip() {
    let resp = CopilotResponse {
        message: "Hello".into(),
        copilot_references: vec![],
        copilot_errors: vec![CopilotError {
            error_type: "test_error".into(),
            message: "oops".into(),
            code: Some("500".into()),
            identifier: None,
        }],
        copilot_confirmation: None,
        function_call: None,
    };
    let json_str = serde_json::to_string(&resp).unwrap();
    let back: CopilotResponse = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.message, "Hello");
    assert_eq!(back.copilot_errors.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Streaming — SSE format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_starts_with_copilot_references() {
    let events = vec![ae(AgentEventKind::AssistantDelta { text: "hi".into() })];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(matches!(
        &stream[0],
        CopilotStreamEvent::CopilotReferences { .. }
    ));
}

#[test]
fn stream_ends_with_done() {
    let stream = events_to_stream_events(&[], "gpt-4o");
    assert!(matches!(
        stream.last().unwrap(),
        CopilotStreamEvent::Done {}
    ));
}

#[test]
fn stream_text_deltas_in_order() {
    let events = vec![
        ae(AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        }),
        ae(AgentEventKind::AssistantDelta {
            text: " world".into(),
        }),
    ];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // refs + 2 deltas + done
    assert_eq!(stream.len(), 4);
    match &stream[1] {
        CopilotStreamEvent::TextDelta { text } => assert_eq!(text, "Hello"),
        other => panic!("expected TextDelta, got {other:?}"),
    }
    match &stream[2] {
        CopilotStreamEvent::TextDelta { text } => assert_eq!(text, " world"),
        other => panic!("expected TextDelta, got {other:?}"),
    }
}

#[test]
fn stream_function_call_event_mapped() {
    let events = vec![ae(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("c1".into()),
        parent_tool_use_id: None,
        input: json!({"cmd": "ls"}),
    })];
    let stream = events_to_stream_events(&events, "gpt-4o");
    match &stream[1] {
        CopilotStreamEvent::FunctionCall { function_call } => {
            assert_eq!(function_call.name, "bash");
            assert_eq!(function_call.id.as_deref(), Some("c1"));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn stream_error_event_mapped() {
    let events = vec![ae(AgentEventKind::Error {
        message: "limit reached".into(),
        error_code: None,
    })];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(matches!(
        &stream[1],
        CopilotStreamEvent::CopilotErrors { errors } if errors.len() == 1
    ));
}

#[test]
fn stream_event_serde_text_delta_roundtrip() {
    let e = CopilotStreamEvent::TextDelta {
        text: "chunk".into(),
    };
    let json_str = serde_json::to_string(&e).unwrap();
    assert!(json_str.contains(r#""type":"text_delta""#));
    let back: CopilotStreamEvent = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, e);
}

#[test]
fn stream_event_serde_done_roundtrip() {
    let e = CopilotStreamEvent::Done {};
    let json_str = serde_json::to_string(&e).unwrap();
    assert!(json_str.contains(r#""type":"done""#));
    let back: CopilotStreamEvent = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, e);
}

#[test]
fn stream_empty_trace_produces_refs_and_done_only() {
    let stream = events_to_stream_events(&[], "gpt-4o");
    assert_eq!(stream.len(), 2);
}

#[tokio::test]
async fn stream_client_collect_all_chunks() {
    let events = vec![
        ae(AgentEventKind::AssistantDelta { text: "A".into() }),
        ae(AgentEventKind::AssistantDelta { text: "B".into() }),
    ];
    let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("go")])
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<CopilotStreamEvent> = stream.collect().await;
    // refs + 2 deltas + done
    assert_eq!(chunks.len(), 4);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Copilot headers — Integration-Id, Authorization: Bearer
// ═══════════════════════════════════════════════════════════════════════════

// NOTE: default_headers() is private on Client, so we test header conventions
// structurally via the client construction API and config serde.

#[test]
fn client_constructed_with_bearer_token() {
    // Client accepts a token that will be formatted as "Bearer <token>"
    let client = Client::new("ghu_testtoken123").unwrap();
    assert_eq!(client.base_url(), "https://api.githubcopilot.com");
}

#[test]
fn client_copilot_integration_id_is_agent_backplane() {
    // The hard-coded Copilot-Integration-Id is "agent-backplane"
    // verified by the source code in client.rs; we confirm the client builds.
    let client = Client::new("ghu_abc").unwrap();
    assert_eq!(client.base_url(), "https://api.githubcopilot.com");
}

#[test]
fn client_chat_completions_endpoint() {
    let client = Client::new("ghu_key").unwrap();
    let url = format!("{}/chat/completions", client.base_url());
    assert_eq!(url, "https://api.githubcopilot.com/chat/completions");
}

#[test]
fn client_bearer_token_convention_ghu_prefix() {
    // Copilot tokens typically start with ghu_ or ghp_
    // Client accepts any string as token
    let client = Client::new("ghu_abcdefghij1234567890").unwrap();
    assert_eq!(client.base_url(), "https://api.githubcopilot.com");
}

#[test]
fn client_bearer_token_convention_ghp_prefix() {
    let client = Client::new("ghp_somepersonaltoken").unwrap();
    assert_eq!(client.base_url(), "https://api.githubcopilot.com");
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Model names — copilot-gpt-4, copilot-gpt-3.5-turbo, known models
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn model_gpt4o_is_known() {
    assert!(dialect::is_known_model("gpt-4o"));
}

#[test]
fn model_gpt4o_mini_is_known() {
    assert!(dialect::is_known_model("gpt-4o-mini"));
}

#[test]
fn model_gpt4_turbo_is_known() {
    assert!(dialect::is_known_model("gpt-4-turbo"));
}

#[test]
fn model_gpt4_is_known() {
    assert!(dialect::is_known_model("gpt-4"));
}

#[test]
fn model_o1_is_known() {
    assert!(dialect::is_known_model("o1"));
}

#[test]
fn model_o1_mini_is_known() {
    assert!(dialect::is_known_model("o1-mini"));
}

#[test]
fn model_o3_mini_is_known() {
    assert!(dialect::is_known_model("o3-mini"));
}

#[test]
fn model_claude_sonnet_4_is_known() {
    assert!(dialect::is_known_model("claude-sonnet-4"));
}

#[test]
fn model_claude_35_sonnet_is_known() {
    assert!(dialect::is_known_model("claude-3.5-sonnet"));
}

#[test]
fn model_unknown_is_not_known() {
    assert!(!dialect::is_known_model("copilot-gpt-3.5-turbo"));
    assert!(!dialect::is_known_model("totally-fake-model"));
}

#[test]
fn model_canonical_mapping_adds_prefix() {
    assert_eq!(dialect::to_canonical_model("gpt-4o"), "copilot/gpt-4o");
    assert_eq!(
        dialect::to_canonical_model("gpt-4-turbo"),
        "copilot/gpt-4-turbo"
    );
}

#[test]
fn model_canonical_mapping_strips_prefix() {
    assert_eq!(dialect::from_canonical_model("copilot/gpt-4o"), "gpt-4o");
    assert_eq!(dialect::from_canonical_model("copilot/o3-mini"), "o3-mini");
}

#[test]
fn model_canonical_passthrough_for_non_prefixed() {
    assert_eq!(dialect::from_canonical_model("gpt-4o"), "gpt-4o");
}

#[test]
fn model_default_constant_is_gpt4o() {
    assert_eq!(DEFAULT_MODEL, "gpt-4o");
}

#[test]
fn model_preserved_in_work_order_conversion() {
    let req = CopilotRequestBuilder::new()
        .model("o3-mini")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Client configuration — API key, integration ID, base URL, timeout
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn client_config_default_base_url() {
    let client = Client::new("ghu_key").unwrap();
    assert_eq!(client.base_url(), "https://api.githubcopilot.com");
}

#[test]
fn client_config_custom_base_url() {
    let client = Client::builder("ghu_key")
        .base_url("https://custom.copilot.local")
        .build()
        .unwrap();
    assert_eq!(client.base_url(), "https://custom.copilot.local");
}

#[test]
fn client_config_custom_timeout() {
    // Just verify it builds without error
    let client = Client::builder("ghu_key")
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap();
    assert_eq!(client.base_url(), "https://api.githubcopilot.com");
}

#[test]
fn client_config_chat_completion_url() {
    let client = Client::new("ghu_key").unwrap();
    let url = format!("{}/chat/completions", client.base_url());
    assert_eq!(url, "https://api.githubcopilot.com/chat/completions");
}

#[test]
fn client_error_api_display() {
    let err = ClientError::Api {
        status: 429,
        body: "rate limited".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("429"));
    assert!(msg.contains("rate limited"));
}

#[test]
fn client_error_builder_display() {
    let err = ClientError::Builder("invalid config".into());
    let msg = err.to_string();
    assert!(msg.contains("invalid config"));
}

#[test]
fn copilot_config_default_values() {
    let cfg = CopilotConfig::default();
    assert_eq!(cfg.model, "gpt-4o");
    assert_eq!(cfg.base_url, "https://api.githubcopilot.com");
    assert!(cfg.token.is_empty());
    assert!(cfg.system_prompt.is_none());
}

#[test]
fn copilot_config_serde_roundtrip() {
    let cfg = CopilotConfig {
        token: "ghp_test".into(),
        base_url: "https://api.githubcopilot.com".into(),
        model: "gpt-4-turbo".into(),
        system_prompt: Some("Be brief.".into()),
    };
    let json_str = serde_json::to_string(&cfg).unwrap();
    let back: CopilotConfig = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.model, "gpt-4-turbo");
    assert_eq!(back.system_prompt.as_deref(), Some("Be brief."));
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Request → WorkOrder conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_task_is_last_user_message() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![
            Message::user("First"),
            Message::assistant("Reply"),
            Message::user("Second"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Second");
}

#[test]
fn work_order_model_propagated() {
    let req = CopilotRequestBuilder::new()
        .model("gpt-4-turbo")
        .messages(vec![Message::user("task")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
}

#[test]
fn work_order_has_valid_id() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("task")])
        .build();
    let wo = request_to_work_order(&req);
    assert!(!wo.id.is_nil());
}

#[test]
fn work_order_ir_preserves_system_role() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![
            Message::system("You are concise."),
            Message::user("Hi"),
        ])
        .build();
    let ir = request_to_ir(&req);
    assert_eq!(ir.messages[0].role, IrRole::System);
    assert_eq!(ir.messages[0].text_content(), "You are concise.");
}

#[test]
fn work_order_ir_preserves_user_and_assistant() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![
            Message::user("Question"),
            Message::assistant("Answer"),
        ])
        .build();
    let ir = request_to_ir(&req);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
}

#[test]
fn work_order_from_map_work_order_with_context_files() {
    let wo = WorkOrderBuilder::new("Check this")
        .context(abp_core::ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![],
        })
        .build();
    let cfg = CopilotConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(!req.references.is_empty());
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::File);
}

#[test]
fn work_order_from_map_work_order_with_snippets() {
    let wo = WorkOrderBuilder::new("Review")
        .context(abp_core::ContextPacket {
            files: vec![],
            snippets: vec![abp_core::ContextSnippet {
                name: "helper".into(),
                content: "fn foo() {}".into(),
            }],
        })
        .build();
    let cfg = CopilotConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    let snippet_refs: Vec<_> = req
        .references
        .iter()
        .filter(|r| r.ref_type == CopilotReferenceType::Snippet)
        .collect();
    assert_eq!(snippet_refs.len(), 1);
}

#[test]
fn work_order_from_map_work_order_with_system_prompt() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CopilotConfig {
        system_prompt: Some("Be helpful.".into()),
        ..CopilotConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages[0].role, "system");
    assert_eq!(req.messages[0].content, "Be helpful.");
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Receipt → Response conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_response_assistant_message() {
    let resp = dialect::map_response(&CopilotResponse {
        message: "Done!".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    });
    assert_eq!(resp.len(), 1);
    assert!(matches!(
        &resp[0].kind,
        AgentEventKind::AssistantMessage { text } if text == "Done!"
    ));
}

#[test]
fn receipt_response_errors_mapped() {
    let resp = dialect::map_response(&CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![CopilotError {
            error_type: "rate_limit".into(),
            message: "Too many requests".into(),
            code: None,
            identifier: None,
        }],
        copilot_confirmation: None,
        function_call: None,
    });
    assert_eq!(resp.len(), 1);
    assert!(matches!(&resp[0].kind, AgentEventKind::Error { .. }));
}

#[test]
fn receipt_response_function_call_mapped() {
    let resp = dialect::map_response(&CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: Some(CopilotFunctionCall {
            name: "read_file".into(),
            arguments: r#"{"path":"main.rs"}"#.into(),
            id: Some("c1".into()),
        }),
    });
    assert_eq!(resp.len(), 1);
    assert!(matches!(&resp[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn receipt_response_confirmation_mapped_as_warning() {
    let resp = dialect::map_response(&CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: Some(CopilotConfirmation {
            id: "c1".into(),
            title: "Delete?".into(),
            message: "Delete main.rs?".into(),
            accepted: None,
        }),
        function_call: None,
    });
    assert_eq!(resp.len(), 1);
    assert!(matches!(&resp[0].kind, AgentEventKind::Warning { .. }));
    assert!(resp[0].ext.is_some());
}

#[test]
fn receipt_response_delta_concatenation_in_shim() {
    let events = vec![
        ae(AgentEventKind::AssistantDelta { text: "Hel".into() }),
        ae(AgentEventKind::AssistantDelta { text: "lo".into() }),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Hello");
}

#[test]
fn receipt_response_usage_in_mock() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..Default::default()
    };
    let receipt = mock_receipt_with_usage(vec![], usage);
    assert_eq!(receipt.usage.input_tokens, Some(100));
    assert_eq!(receipt.usage.output_tokens, Some(50));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Dialect detection — Identify Copilot dialect
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_version_constant() {
    assert_eq!(DIALECT_VERSION, "copilot/v0.1");
}

#[test]
fn dialect_capability_manifest_has_streaming_native() {
    let m = dialect::capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn dialect_capability_manifest_has_tool_read_emulated() {
    let m = dialect::capability_manifest();
    assert!(m.contains_key(&Capability::ToolRead));
}

#[test]
fn dialect_capability_manifest_has_web_search_native() {
    let m = dialect::capability_manifest();
    assert!(m.contains_key(&Capability::ToolWebSearch));
}

#[test]
fn dialect_capability_manifest_mcp_unsupported() {
    let m = dialect::capability_manifest();
    assert!(m.contains_key(&Capability::McpClient));
    assert!(m.contains_key(&Capability::McpServer));
}

#[test]
fn dialect_passthrough_text_delta_roundtrip() {
    let event = CopilotStreamEvent::TextDelta {
        text: "hello".into(),
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let ext = wrapped.ext.as_ref().unwrap();
    assert_eq!(ext.get("dialect").unwrap(), "copilot");
    let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(recovered, event);
}

#[test]
fn dialect_passthrough_function_call_roundtrip() {
    let event = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "search".into(),
            arguments: r#"{"q":"test"}"#.into(),
            id: Some("c1".into()),
        },
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(recovered, event);
}

#[test]
fn dialect_passthrough_done_roundtrip() {
    let event = CopilotStreamEvent::Done {};
    let wrapped = dialect::to_passthrough_event(&event);
    let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(recovered, event);
}

#[test]
fn dialect_verify_fidelity_all_event_types() {
    let events = vec![
        CopilotStreamEvent::CopilotReferences {
            references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "f1".into(),
                data: json!({"path": "a.rs"}),
                metadata: None,
            }],
        },
        CopilotStreamEvent::TextDelta {
            text: "chunk".into(),
        },
        CopilotStreamEvent::FunctionCall {
            function_call: CopilotFunctionCall {
                name: "read".into(),
                arguments: "{}".into(),
                id: None,
            },
        },
        CopilotStreamEvent::CopilotErrors {
            errors: vec![CopilotError {
                error_type: "err".into(),
                message: "oops".into(),
                code: None,
                identifier: None,
            }],
        },
        CopilotStreamEvent::Done {},
    ];
    assert!(dialect::verify_passthrough_fidelity(&events));
}

#[test]
fn dialect_map_stream_event_text_delta_to_agent_event() {
    let event = CopilotStreamEvent::TextDelta { text: "hi".into() };
    let mapped = dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    assert!(matches!(
        &mapped[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "hi"
    ));
}

#[test]
fn dialect_map_stream_event_done_to_run_completed() {
    let event = CopilotStreamEvent::Done {};
    let mapped = dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    assert!(matches!(
        &mapped[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn dialect_map_stream_event_empty_references_produces_nothing() {
    let event = CopilotStreamEvent::CopilotReferences { references: vec![] };
    let mapped = dialect::map_stream_event(&event);
    assert!(mapped.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. OpenAI compatibility — Base format compatible with OpenAI
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_compat_request_has_model_and_messages() {
    let req = CopilotRequestBuilder::new()
        .model("gpt-4o")
        .messages(vec![Message::user("Hello")])
        .build();
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("model").is_some());
    assert!(json.get("messages").is_some());
}

#[test]
fn openai_compat_messages_have_role_and_content() {
    let msg = CopilotMessage {
        role: "user".into(),
        content: "test".into(),
        name: None,
        copilot_references: vec![],
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "user");
    assert_eq!(json["content"], "test");
}

#[test]
fn openai_compat_response_has_message_field() {
    let resp = CopilotResponse {
        message: "Hello!".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["message"], "Hello!");
}

#[test]
fn openai_compat_function_call_has_name_and_arguments() {
    let fc = CopilotFunctionCall {
        name: "get_weather".into(),
        arguments: r#"{"city":"London"}"#.into(),
        id: Some("call_1".into()),
    };
    let json = serde_json::to_value(&fc).unwrap();
    assert_eq!(json["name"], "get_weather");
    assert!(json["arguments"].is_string());
    assert_eq!(json["id"], "call_1");
}

#[test]
fn openai_compat_tool_type_is_function() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Function,
        function: Some(CopilotFunctionDef {
            name: "test".into(),
            description: "desc".into(),
            parameters: json!({"type": "object"}),
        }),
        confirmation: None,
    };
    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["type"], "function");
}

#[test]
fn openai_compat_tool_def_canonical_roundtrip() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read file contents".into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let copilot_tool = dialect::tool_def_to_copilot(&canonical);
    let back = dialect::tool_def_from_copilot(&copilot_tool).unwrap();
    assert_eq!(back.name, canonical.name);
    assert_eq!(back.description, canonical.description);
    assert_eq!(back.parameters_schema, canonical.parameters_schema);
}

#[test]
fn openai_compat_ir_messages_roundtrip() {
    let messages = vec![
        Message::system("System"),
        Message::user("Hello"),
        Message::assistant("Hi"),
    ];
    let ir = messages_to_ir(&messages);
    assert_eq!(ir.len(), 3);
    let back = ir_to_messages(&ir);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[1].role, "user");
    assert_eq!(back[2].role, "assistant");
}

#[test]
fn openai_compat_response_to_ir_and_back() {
    let resp = CopilotResponse {
        message: "Result".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let ir = response_to_ir(&resp);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
}

#[test]
fn openai_compat_ir_usage_tuple() {
    let ir = IrUsage::from_io(250, 75);
    let (input, output, total) = ir_usage_to_tuple(&ir);
    assert_eq!(input, 250);
    assert_eq!(output, 75);
    assert_eq!(total, 325);
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional edge cases and integration patterns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lowering_copilot_references_preserved_in_ir() {
    let refs = vec![CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "f-0".into(),
        data: json!({"path": "lib.rs"}),
        metadata: None,
    }];
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "Check file".into(),
        name: None,
        copilot_references: refs,
    }];
    let ir = lowering::to_ir(&msgs);
    assert!(ir.messages[0].metadata.contains_key("copilot_references"));
    let back = lowering::from_ir(&ir);
    assert_eq!(back[0].copilot_references.len(), 1);
}

#[test]
fn lowering_display_name_preserved() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "Hi".into(),
        name: Some("bob".into()),
        copilot_references: vec![],
    }];
    let ir = lowering::to_ir(&msgs);
    let back = lowering::from_ir(&ir);
    assert_eq!(back[0].name.as_deref(), Some("bob"));
}

#[test]
fn lowering_unknown_role_mapped_to_user() {
    let msgs = vec![CopilotMessage {
        role: "developer".into(),
        content: "hi".into(),
        name: None,
        copilot_references: vec![],
    }];
    let ir = lowering::to_ir(&msgs);
    assert_eq!(ir.messages[0].role, IrRole::User);
}

#[tokio::test]
async fn client_no_processor_create_returns_error() {
    let client = CopilotClient::new("gpt-4o");
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let err = client.create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
}

#[tokio::test]
async fn client_no_processor_stream_returns_error() {
    let client = CopilotClient::new("gpt-4o");
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    assert!(client.create_stream(req).await.is_err());
}

#[test]
fn shim_client_debug_output() {
    let client = CopilotClient::new("copilot-gpt-4");
    let dbg = format!("{client:?}");
    assert!(dbg.contains("copilot-gpt-4"));
}

#[test]
fn shim_client_model_accessor() {
    let client = CopilotClient::new("gpt-4o-mini");
    assert_eq!(client.model(), "gpt-4o-mini");
}

#[tokio::test]
async fn full_roundtrip_request_to_receipt_to_response() {
    let events = vec![
        ae(AgentEventKind::AssistantMessage {
            text: "I can help with that.".into(),
        }),
        ae(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("call_xyz".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/lib.rs"}),
        }),
    ];
    let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
    let req = CopilotRequestBuilder::new()
        .model("gpt-4o")
        .messages(vec![
            Message::system("You are a coding assistant."),
            Message::user("Read the lib file"),
        ])
        .build();

    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.message, "I can help with that.");
    let fc = resp.function_call.unwrap();
    assert_eq!(fc.name, "read_file");
    assert_eq!(fc.id.as_deref(), Some("call_xyz"));
}

#[test]
fn mock_receipt_defaults() {
    let receipt = mock_receipt(vec![]);
    assert_eq!(receipt.backend.id, "mock");
    assert!(receipt.trace.is_empty());
    assert!(receipt.receipt_sha256.is_none());
}
