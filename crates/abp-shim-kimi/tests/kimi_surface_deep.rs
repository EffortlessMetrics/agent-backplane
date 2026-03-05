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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Deep surface-area tests for the Kimi shim — validates that ABP faithfully
//! mirrors the Moonshot Kimi Chat Completions API wire format, conversions,
//! streaming, function calling, model names, client configuration, dialect
//! detection, context window sizes, and serialization fidelity.
//!
//! Kimi uses an OpenAI-compatible format with extensions for web search,
//! citation references, and the k1 reasoning mode.

use abp_core::ir::{IrContentBlock, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, UsageNormalized};
use abp_kimi_sdk::dialect::{
    self, CanonicalToolDef, KimiBuiltinFunction, KimiBuiltinTool, KimiChoice, KimiChunk,
    KimiChunkChoice, KimiChunkDelta, KimiChunkFunctionCall, KimiChunkToolCall, KimiConfig,
    KimiFunctionCall, KimiFunctionDef, KimiMessage, KimiRef, KimiRequest, KimiResponse,
    KimiResponseMessage, KimiRole, KimiTool, KimiToolCall, KimiUsage, ToolCallAccumulator,
};
use abp_shim_kimi::client::Client;
use abp_shim_kimi::{
    events_to_stream_chunks, ir_to_messages, ir_usage_to_usage, messages_to_ir, mock_receipt,
    mock_receipt_with_usage, receipt_to_response, request_to_ir, request_to_work_order,
    response_to_ir, KimiClient, KimiRequestBuilder, Message, ProcessFn, ShimError, Usage,
};
use chrono::Utc;
use serde_json::json;
use std::time::Duration;
use tokio_stream::StreamExt;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
    Box::new(move |_wo| mock_receipt(events.clone()))
}

fn make_processor_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> ProcessFn {
    Box::new(move |_wo| mock_receipt_with_usage(events.clone(), usage.clone()))
}

fn assistant_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn tool_call_event(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.to_string(),
            tool_use_id: Some(id.to_string()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn error_event(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.to_string(),
            error_code: None,
        },
        ext: None,
    }
}

fn sample_usage() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(150),
        output_tokens: Some(75),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Request format — OpenAI-compatible chat completions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t01_request_serializes_model_and_messages() {
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-8k")
        .messages(vec![Message::system("Be helpful."), Message::user("Hello")])
        .build();

    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["model"], "moonshot-v1-8k");
    assert!(v["messages"].is_array());
    assert_eq!(v["messages"].as_array().unwrap().len(), 2);
}

#[test]
fn t02_request_serializes_optional_params() {
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-32k")
        .messages(vec![Message::user("test")])
        .temperature(0.7)
        .max_tokens(2048)
        .stream(true)
        .use_search(true)
        .build();

    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["temperature"], 0.7);
    assert_eq!(v["max_tokens"], 2048);
    assert_eq!(v["stream"], true);
    assert_eq!(v["use_search"], true);
}

#[test]
fn t03_request_omits_none_fields() {
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-8k")
        .messages(vec![Message::user("Hi")])
        .build();

    let v = serde_json::to_value(&req).unwrap();
    assert!(v.get("temperature").is_none());
    assert!(v.get("max_tokens").is_none());
    assert!(v.get("stream").is_none());
    assert!(v.get("tools").is_none());
    assert!(v.get("use_search").is_none());
}

#[test]
fn t04_request_json_roundtrip() {
    let req = KimiRequest {
        model: "moonshot-v1-128k".into(),
        messages: vec![KimiMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_call_id: None,
            tool_calls: None,
        }],
        max_tokens: Some(4096),
        temperature: Some(0.5),
        stream: Some(true),
        tools: None,
        use_search: Some(true),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "moonshot-v1-128k");
    assert_eq!(parsed.max_tokens, Some(4096));
    assert_eq!(parsed.stream, Some(true));
    assert_eq!(parsed.use_search, Some(true));
}

#[test]
fn t05_request_with_tools() {
    let tools = vec![KimiTool::Function {
        function: KimiFunctionDef {
            name: "get_weather".into(),
            description: "Get weather for a city".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string"}
                },
                "required": ["city"]
            }),
        },
    }];
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("Weather in Tokyo?")])
        .tools(tools)
        .build();

    assert!(req.tools.is_some());
    let v = serde_json::to_value(&req).unwrap();
    assert!(v["tools"][0]["function"]["name"] == "get_weather");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Response format — OpenAI-compatible
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t06_response_has_required_fields() {
    let resp = KimiResponse {
        id: "cmpl-test123".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
        refs: None,
    };

    let v = serde_json::to_value(&resp).unwrap();
    for key in ["id", "model", "choices"] {
        assert!(v.get(key).is_some(), "missing key: {key}");
    }
}

#[test]
fn t07_response_json_roundtrip() {
    let resp = KimiResponse {
        id: "cmpl-abc".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 20,
            completion_tokens: 10,
            total_tokens: 30,
        }),
        refs: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: KimiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "cmpl-abc");
    assert_eq!(parsed.choices[0].message.content.as_deref(), Some("Hello!"));
    assert_eq!(parsed.usage.unwrap().total_tokens, 30);
}

#[test]
fn t08_response_id_starts_with_cmpl() {
    let events = vec![assistant_event("ok")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert!(resp.id.starts_with("cmpl-"));
}

#[test]
fn t09_response_choices_has_one_entry() {
    let events = vec![assistant_event("Hello")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].index, 0);
}

#[test]
fn t10_response_finish_reason_stop_for_text() {
    let events = vec![assistant_event("done")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t11_response_finish_reason_tool_calls() {
    let events = vec![tool_call_event("search", "call_1", json!({"q": "rust"}))];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
}

#[test]
fn t12_response_with_refs() {
    let resp = KimiResponse {
        id: "cmpl-ref".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("According to [1]...".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: Some(vec![KimiRef {
            index: 1,
            url: "https://example.com".into(),
            title: Some("Example".into()),
        }]),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: KimiResponse = serde_json::from_str(&json).unwrap();
    assert!(parsed.refs.is_some());
    assert_eq!(parsed.refs.unwrap()[0].url, "https://example.com");
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Streaming — SSE format matching OpenAI
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t13_stream_chunk_object_is_chat_completion_chunk() {
    let chunks = events_to_stream_chunks(&[delta_event("hi")], "moonshot-v1-8k");
    for chunk in &chunks {
        assert_eq!(chunk.object, "chat.completion.chunk");
    }
}

#[test]
fn t14_stream_chunks_delta_content() {
    let events = vec![delta_event("a"), delta_event("b"), delta_event("c")];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    assert_eq!(chunks.len(), 4); // 3 deltas + 1 stop
    assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("a"));
    assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("b"));
    assert_eq!(chunks[2].choices[0].delta.content.as_deref(), Some("c"));
}

#[test]
fn t15_stream_chunks_end_with_stop() {
    let chunks = events_to_stream_chunks(&[delta_event("hi")], "moonshot-v1-8k");
    let last = chunks.last().unwrap();
    assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t16_stream_chunks_consistent_id() {
    let events = vec![delta_event("a"), delta_event("b")];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    let id = &chunks[0].id;
    for chunk in &chunks {
        assert_eq!(&chunk.id, id);
    }
}

#[test]
fn t17_stream_chunks_model_preserved() {
    let chunks = events_to_stream_chunks(&[delta_event("x")], "moonshot-v1-128k");
    for chunk in &chunks {
        assert_eq!(chunk.model, "moonshot-v1-128k");
    }
}

#[test]
fn t18_stream_chunks_empty_events_produces_stop_only() {
    let chunks = events_to_stream_chunks(&[], "moonshot-v1-8k");
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t19_stream_chunk_serde_roundtrip() {
    let chunk = KimiChunk {
        id: "cmpl-stream-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: Some("assistant".into()),
                content: Some("Hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: KimiChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, chunk);
}

#[test]
fn t20_stream_assistant_message_includes_role() {
    let events = vec![assistant_event("full message")];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    assert_eq!(
        chunks[0].choices[0].delta.role.as_deref(),
        Some("assistant")
    );
}

#[tokio::test]
async fn t21_stream_client_roundtrip() {
    let events = vec![delta_event("Hel"), delta_event("lo!")];
    let client = KimiClient::with_model("moonshot-v1-8k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("Hi")])
        .stream(true)
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<KimiChunk> = stream.collect().await;
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("Hel"));
    assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("lo!"));
    assert_eq!(chunks[2].choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t22_stream_skips_non_text_events() {
    let events = vec![
        delta_event("text"),
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        },
    ];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    assert_eq!(chunks.len(), 2); // 1 delta + 1 stop
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Model names — moonshot-v1-8k, moonshot-v1-32k, moonshot-v1-128k
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t23_known_models_detected() {
    assert!(dialect::is_known_model("moonshot-v1-8k"));
    assert!(dialect::is_known_model("moonshot-v1-32k"));
    assert!(dialect::is_known_model("moonshot-v1-128k"));
    assert!(dialect::is_known_model("kimi-latest"));
    assert!(dialect::is_known_model("k1"));
}

#[test]
fn t24_unknown_models_rejected() {
    assert!(!dialect::is_known_model("gpt-4o"));
    assert!(!dialect::is_known_model("claude-3-opus"));
    assert!(!dialect::is_known_model("moonshot-v2-8k"));
    assert!(!dialect::is_known_model(""));
}

#[test]
fn t25_canonical_model_mapping() {
    assert_eq!(
        dialect::to_canonical_model("moonshot-v1-8k"),
        "moonshot/moonshot-v1-8k"
    );
    assert_eq!(
        dialect::to_canonical_model("kimi-latest"),
        "moonshot/kimi-latest"
    );
    assert_eq!(dialect::to_canonical_model("k1"), "moonshot/k1");
}

#[test]
fn t26_canonical_model_roundtrip() {
    for model in &[
        "moonshot-v1-8k",
        "moonshot-v1-32k",
        "moonshot-v1-128k",
        "kimi-latest",
        "k1",
    ] {
        let canonical = dialect::to_canonical_model(model);
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(&back, model);
    }
}

#[test]
fn t27_from_canonical_model_strips_prefix() {
    assert_eq!(
        dialect::from_canonical_model("moonshot/moonshot-v1-8k"),
        "moonshot-v1-8k"
    );
}

#[test]
fn t28_from_canonical_model_passthrough() {
    assert_eq!(
        dialect::from_canonical_model("moonshot-v1-8k"),
        "moonshot-v1-8k"
    );
}

#[test]
fn t29_default_model_is_moonshot_v1_8k() {
    assert_eq!(dialect::DEFAULT_MODEL, "moonshot-v1-8k");
}

#[test]
fn t30_builder_defaults_model_to_8k() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    assert_eq!(req.model, "moonshot-v1-8k");
}

#[test]
fn t31_model_preserved_in_work_order_all_variants() {
    for model in &["moonshot-v1-8k", "moonshot-v1-32k", "moonshot-v1-128k"] {
        let req = KimiRequestBuilder::new()
            .model(*model)
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some(*model));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Base URL — api.moonshot.cn/v1
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t32_default_base_url() {
    let client = Client::new("sk-test").unwrap();
    assert_eq!(client.base_url(), "https://api.moonshot.cn/v1");
}

#[test]
fn t33_chat_completions_url() {
    let client = Client::new("sk-test").unwrap();
    let url = format!("{}/chat/completions", client.base_url());
    assert_eq!(url, "https://api.moonshot.cn/v1/chat/completions");
}

#[test]
fn t34_config_default_base_url() {
    let cfg = KimiConfig::default();
    assert_eq!(cfg.base_url, "https://api.moonshot.cn/v1");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Client configuration — API key, base URL
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t35_client_accepts_api_key() {
    // Client construction with API key succeeds and preserves default base URL
    let client = Client::new("sk-kimi-abc123").unwrap();
    assert_eq!(client.base_url(), "https://api.moonshot.cn/v1");
}

#[test]
fn t36_client_construction_with_different_keys() {
    let c1 = Client::new("sk-key-1").unwrap();
    let c2 = Client::new("sk-key-2").unwrap();
    // Both clients should independently construct
    assert_eq!(c1.base_url(), c2.base_url());
}

#[test]
fn t37_client_builder_base_url_override() {
    let client = Client::builder("sk-key")
        .base_url("https://custom.moonshot.example/v1")
        .build()
        .unwrap();
    assert_eq!(client.base_url(), "https://custom.moonshot.example/v1");
}

#[test]
fn t38_client_builder_timeout() {
    let client = Client::builder("sk-key")
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();
    // Client constructed successfully with custom timeout
    assert_eq!(client.base_url(), "https://api.moonshot.cn/v1");
}

#[test]
fn t39_client_error_display() {
    use abp_shim_kimi::client::ClientError;
    let err = ClientError::Api {
        status: 429,
        body: "rate limit exceeded".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("429"));
    assert!(msg.contains("rate limit exceeded"));
}

#[test]
fn t40_config_serde_roundtrip() {
    let cfg = KimiConfig {
        api_key: "sk-test".into(),
        base_url: "https://api.moonshot.cn/v1".into(),
        model: "moonshot-v1-32k".into(),
        max_tokens: Some(8192),
        temperature: Some(0.5),
        use_k1_reasoning: Some(true),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: KimiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "moonshot-v1-32k");
    assert_eq!(parsed.use_k1_reasoning, Some(true));
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Request → WorkOrder conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t41_request_to_work_order_extracts_task() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("Explain async in Rust")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Explain async in Rust");
}

#[test]
fn t42_request_to_work_order_multi_turn_uses_last_user() {
    let req = KimiRequestBuilder::new()
        .messages(vec![
            Message::user("First question"),
            Message::assistant("Answer"),
            Message::user("Follow-up"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Follow-up");
}

#[test]
fn t43_request_to_work_order_system_only_fallback() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::system("System only")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "kimi completion");
}

#[test]
fn t44_request_to_work_order_temperature_in_vendor() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .temperature(0.9)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(0.9))
    );
}

#[test]
fn t45_request_to_work_order_max_tokens_in_vendor() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .max_tokens(4096)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("max_tokens"),
        Some(&serde_json::Value::from(4096))
    );
}

#[test]
fn t46_request_to_ir_roles_preserved() {
    let req = KimiRequestBuilder::new()
        .messages(vec![
            Message::system("Be concise."),
            Message::user("Hi"),
            Message::assistant("Hello!"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
}

#[test]
fn t47_request_to_ir_tool_messages() {
    let req = KimiRequestBuilder::new()
        .messages(vec![
            Message::user("Search"),
            Message::assistant_with_tool_calls(vec![KimiToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
            Message::tool("call_1", "results"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[2].role, IrRole::Tool);
    match &conv.messages[2].content[0] {
        IrContentBlock::ToolResult { tool_use_id, .. } => {
            assert_eq!(tool_use_id, "call_1");
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Receipt → Response conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t48_receipt_to_response_assistant_text() {
    let events = vec![assistant_event("Hello, world!")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello, world!")
    );
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t49_receipt_to_response_tool_calls() {
    let events = vec![tool_call_event(
        "web_search",
        "call_1",
        json!({"query": "rust"}),
    )];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].id, "call_1");
    assert_eq!(tcs[0].function.name, "web_search");
}

#[test]
fn t50_receipt_to_response_error_event() {
    let events = vec![error_event("rate limit exceeded")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    let content = resp.choices[0].message.content.as_deref().unwrap();
    assert!(content.contains("rate limit exceeded"));
}

#[tokio::test]
async fn t51_receipt_to_response_usage() {
    let usage = sample_usage();
    let events = vec![assistant_event("ok")];
    let client = KimiClient::with_model("moonshot-v1-8k")
        .with_processor(make_processor_with_usage(events, usage));
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let resp = client.create(req).await.unwrap();
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 150);
    assert_eq!(u.completion_tokens, 75);
    assert_eq!(u.total_tokens, 225);
}

#[test]
fn t52_receipt_to_response_delta_concatenation() {
    let events = vec![delta_event("Part 1 "), delta_event("Part 2")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Part 1 Part 2")
    );
}

#[test]
fn t53_receipt_to_response_empty_trace() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert!(resp.choices[0].message.content.is_none());
    assert!(resp.choices[0].message.tool_calls.is_none());
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t54_receipt_to_response_tool_call_without_id_generates_one() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "test".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        ext: None,
    }];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert!(tcs[0].id.starts_with("call_"));
}

#[test]
fn t55_receipt_to_response_mixed_text_and_tools() {
    let events = vec![
        assistant_event("Let me search."),
        tool_call_event("bash", "call_1", json!({"cmd": "ls"})),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Let me search.")
    );
    assert!(resp.choices[0].message.tool_calls.is_some());
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Dialect detection — Kimi identification
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t56_dialect_version() {
    assert_eq!(dialect::DIALECT_VERSION, "kimi/v0.1");
}

#[test]
fn t57_kimi_role_serde_roundtrip() {
    for (role_str, role_enum) in [
        ("system", KimiRole::System),
        ("user", KimiRole::User),
        ("assistant", KimiRole::Assistant),
        ("tool", KimiRole::Tool),
    ] {
        let json = format!("\"{role_str}\"");
        let parsed: KimiRole = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, role_enum);
        let back = serde_json::to_string(&role_enum).unwrap();
        assert_eq!(back, json);
    }
}

#[test]
fn t58_kimi_role_display() {
    assert_eq!(KimiRole::System.to_string(), "system");
    assert_eq!(KimiRole::User.to_string(), "user");
    assert_eq!(KimiRole::Assistant.to_string(), "assistant");
    assert_eq!(KimiRole::Tool.to_string(), "tool");
}

#[test]
fn t59_capability_manifest_has_expected_entries() {
    use abp_core::Capability;
    let manifest = dialect::capability_manifest();
    assert!(manifest.contains_key(&Capability::Streaming));
    assert!(manifest.contains_key(&Capability::ToolWebSearch));
    assert!(manifest.contains_key(&Capability::ToolEdit));
    assert!(manifest.contains_key(&Capability::ToolRead));
    let json = serde_json::to_string(&manifest).unwrap();
    assert!(json.contains("streaming"));
    assert!(json.contains("tool_web_search"));
}

#[test]
fn t60_response_to_ir_basic() {
    let resp = KimiResponse {
        id: "cmpl-1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Hello".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    };
    let conv = response_to_ir(&resp);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    assert_eq!(conv.messages[0].text_content(), "Hello");
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Context window sizes — 8k, 32k, 128k model variants
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t61_8k_model_roundtrip() {
    let events = vec![assistant_event("8k response")];
    let client = KimiClient::with_model("moonshot-v1-8k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-8k")
        .messages(vec![Message::user("test")])
        .build();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.model, "moonshot-v1-8k");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("8k response")
    );
}

#[tokio::test]
async fn t62_32k_model_roundtrip() {
    let events = vec![assistant_event("32k response")];
    let client = KimiClient::with_model("moonshot-v1-32k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-32k")
        .messages(vec![Message::user("test")])
        .build();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.model, "moonshot-v1-32k");
}

#[tokio::test]
async fn t63_128k_model_roundtrip() {
    let events = vec![assistant_event("128k response")];
    let client = KimiClient::with_model("moonshot-v1-128k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-128k")
        .messages(vec![Message::user("analyze this very long document")])
        .build();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.model, "moonshot-v1-128k");
}

#[test]
fn t64_model_names_work_order_all_variants() {
    let models = [
        "moonshot-v1-8k",
        "moonshot-v1-32k",
        "moonshot-v1-128k",
        "kimi-latest",
        "k1",
    ];
    for model in &models {
        let req = KimiRequestBuilder::new()
            .model(*model)
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some(*model));
    }
}

#[test]
fn t65_config_default_model_is_8k() {
    let cfg = KimiConfig::default();
    assert_eq!(cfg.model, "moonshot-v1-8k");
    assert_eq!(cfg.max_tokens, Some(4096));
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. IR roundtrip and message conversions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t66_messages_to_ir_roundtrip() {
    let messages = vec![
        Message::system("System prompt"),
        Message::user("User message"),
        Message::assistant("Reply"),
    ];
    let conv = messages_to_ir(&messages);
    assert_eq!(conv.len(), 3);
    let back = ir_to_messages(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content.as_deref(), Some("System prompt"));
    assert_eq!(back[1].role, "user");
    assert_eq!(back[2].role, "assistant");
}

#[test]
fn t67_ir_usage_conversion() {
    let ir = IrUsage::from_io(200, 100);
    let usage = ir_usage_to_usage(&ir);
    assert_eq!(usage.prompt_tokens, 200);
    assert_eq!(usage.completion_tokens, 100);
    assert_eq!(usage.total_tokens, 300);
}

#[test]
fn t68_usage_serde_roundtrip() {
    let usage = Usage {
        prompt_tokens: 500,
        completion_tokens: 250,
        total_tokens: 750,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Tool use and built-in functions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t69_tool_def_to_kimi_and_back() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({
            "type": "object",
            "properties": {"path": {"type": "string"}}
        }),
    };
    let kimi = dialect::tool_def_to_kimi(&canonical);
    assert_eq!(kimi.tool_type, "function");
    assert_eq!(kimi.function.name, "read_file");
    let back = dialect::tool_def_from_kimi(&kimi);
    assert_eq!(back, canonical);
}

#[test]
fn t70_builtin_search_internet() {
    let tool = dialect::builtin_search_internet();
    assert_eq!(tool.tool_type, "builtin_function");
    assert_eq!(tool.function.name, "$web_search");
}

#[test]
fn t71_builtin_browser() {
    let tool = dialect::builtin_browser();
    assert_eq!(tool.tool_type, "builtin_function");
    assert_eq!(tool.function.name, "$browser");
}

#[test]
fn t72_tool_call_accumulator_single() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(KimiChunkFunctionCall {
            name: Some("search".into()),
            arguments: Some(r#"{"q":"rust"}"#.into()),
        }),
    }]);
    let events = acc.finish();
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "search");
            assert_eq!(input, &json!({"q": "rust"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn t73_tool_call_accumulator_incremental_fragments() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(KimiChunkFunctionCall {
            name: Some("search".into()),
            arguments: Some(r#"{"q""#.into()),
        }),
    }]);
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(KimiChunkFunctionCall {
            name: None,
            arguments: Some(r#":"rust"}"#.into()),
        }),
    }]);
    let events = acc.finish();
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &json!({"q": "rust"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn t74_tool_call_accumulator_multiple_tools() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[
        KimiChunkToolCall {
            index: 0,
            id: Some("call_1".into()),
            call_type: Some("function".into()),
            function: Some(KimiChunkFunctionCall {
                name: Some("search".into()),
                arguments: Some(r#"{"q":"a"}"#.into()),
            }),
        },
        KimiChunkToolCall {
            index: 1,
            id: Some("call_2".into()),
            call_type: Some("function".into()),
            function: Some(KimiChunkFunctionCall {
                name: Some("read".into()),
                arguments: Some(r#"{"path":"x"}"#.into()),
            }),
        },
    ]);
    let events = acc.finish();
    assert_eq!(events.len(), 2);
}

#[test]
fn t75_tool_call_accumulator_empty() {
    let acc = ToolCallAccumulator::new();
    let events = acc.finish();
    assert!(events.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Error handling and edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t76_no_processor_returns_error() {
    let client = KimiClient::with_model("moonshot-v1-8k");
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let err = client.create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
}

#[tokio::test]
async fn t77_no_processor_stream_returns_error() {
    let client = KimiClient::with_model("moonshot-v1-8k");
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let result = client.create_stream(req).await;
    assert!(result.is_err());
}

#[test]
fn t78_client_debug_impl() {
    let client = KimiClient::with_model("moonshot-v1-8k");
    let debug = format!("{client:?}");
    assert!(debug.contains("moonshot-v1-8k"));
}

#[test]
fn t79_client_model_accessor() {
    let client = KimiClient::with_model("moonshot-v1-128k");
    assert_eq!(client.model(), "moonshot-v1-128k");
}

#[test]
fn t80_empty_messages_roundtrip() {
    let messages: Vec<Message> = vec![];
    let conv = messages_to_ir(&messages);
    assert_eq!(conv.len(), 0);
    let back = ir_to_messages(&conv);
    assert!(back.is_empty());
}

#[test]
fn t81_kimi_tool_enum_function_serde() {
    let tool = KimiTool::Function {
        function: KimiFunctionDef {
            name: "get_weather".into(),
            description: "Get weather".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains(r#""type":"function""#));
    let parsed: KimiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn t82_kimi_tool_enum_builtin_serde() {
    let tool = KimiTool::BuiltinFunction {
        function: KimiBuiltinFunction {
            name: "$web_search".into(),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains(r#""type":"builtin_function""#));
    let parsed: KimiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn t83_chunk_delta_default_is_empty() {
    let delta = KimiChunkDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

#[test]
fn t84_kimi_ref_without_title_skips_field() {
    let r = KimiRef {
        index: 2,
        url: "https://rust-lang.org".into(),
        title: None,
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("title"));
    let parsed: KimiRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.title, None);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. WorkOrder ↔ KimiRequest dialect mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t85_map_work_order_uses_task_as_user_message() {
    use abp_core::WorkOrderBuilder;
    let wo = WorkOrderBuilder::new("Optimize queries").build();
    let cfg = KimiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(req.messages[0]
        .content
        .as_deref()
        .unwrap()
        .contains("Optimize queries"));
}

#[test]
fn t86_map_work_order_respects_model_override() {
    use abp_core::WorkOrderBuilder;
    let wo = WorkOrderBuilder::new("task")
        .model("moonshot-v1-128k")
        .build();
    let cfg = KimiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "moonshot-v1-128k");
}

#[test]
fn t87_map_response_produces_agent_events() {
    let resp = KimiResponse {
        id: "cmpl_123".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("The answer.".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => {
            assert_eq!(text, "The answer.");
        }
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn t88_map_response_with_tool_calls() {
    let resp = KimiResponse {
        id: "cmpl_456".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![KimiToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "web_search".into(),
                        arguments: r#"{"query":"rust"}"#.into(),
                    },
                }]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            ..
        } => {
            assert_eq!(tool_name, "web_search");
            assert_eq!(tool_use_id.as_deref(), Some("call_1"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn t89_map_stream_event_text_delta() {
    let chunk = KimiChunk {
        id: "cmpl-s1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: None,
                content: Some("Hello".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };
    let events = dialect::map_stream_event(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn t90_map_stream_event_finish_reason() {
    let chunk = KimiChunk {
        id: "cmpl-s1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = dialect::map_stream_event(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::RunCompleted { message } => {
            assert!(message.contains("stop"));
        }
        other => panic!("expected RunCompleted, got {other:?}"),
    }
}

#[test]
fn t91_extract_usage_helper() {
    let resp = KimiResponse {
        id: "cmpl-1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: Some(KimiUsage {
            prompt_tokens: 50,
            completion_tokens: 25,
            total_tokens: 75,
        }),
        refs: None,
    };
    let usage_map = dialect::extract_usage(&resp).unwrap();
    assert_eq!(
        usage_map.get("prompt_tokens"),
        Some(&serde_json::Value::from(50))
    );
    assert_eq!(
        usage_map.get("total_tokens"),
        Some(&serde_json::Value::from(75))
    );
}

#[test]
fn t92_extract_usage_none_when_absent() {
    let resp = KimiResponse {
        id: "cmpl-2".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: None,
        refs: None,
    };
    assert!(dialect::extract_usage(&resp).is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Kimi-specific extensions (search, refs, k1)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t93_config_k1_reasoning_mode() {
    let cfg = KimiConfig {
        api_key: "sk-test".into(),
        base_url: "https://api.moonshot.cn/v1".into(),
        model: "k1".into(),
        max_tokens: Some(4096),
        temperature: None,
        use_k1_reasoning: Some(true),
    };
    assert_eq!(cfg.use_k1_reasoning, Some(true));
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("use_k1_reasoning"));
}

#[test]
fn t94_builtin_tool_serde_roundtrip() {
    let tool = KimiBuiltinTool {
        tool_type: "builtin_function".into(),
        function: KimiBuiltinFunction {
            name: "$web_search".into(),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let parsed: KimiBuiltinTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn t95_map_response_preserves_refs_in_ext() {
    let resp = KimiResponse {
        id: "cmpl-ref".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("See [1]".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: Some(vec![KimiRef {
            index: 1,
            url: "https://example.com".into(),
            title: Some("Example".into()),
        }]),
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    let ext = events[0].ext.as_ref().unwrap();
    assert!(ext.contains_key("kimi_refs"));
}

#[test]
fn t96_kimi_tool_call_serde_roundtrip() {
    let tc = KimiToolCall {
        id: "call_abc".into(),
        call_type: "function".into(),
        function: KimiFunctionCall {
            name: "web_search".into(),
            arguments: r#"{"query":"rust async"}"#.into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: KimiToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn t97_kimi_usage_serde_roundtrip() {
    let usage = KimiUsage {
        prompt_tokens: 200,
        completion_tokens: 100,
        total_tokens: 300,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: KimiUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}

#[test]
fn t98_message_constructors() {
    let sys = Message::system("prompt");
    assert_eq!(sys.role, "system");
    assert_eq!(sys.content.as_deref(), Some("prompt"));

    let usr = Message::user("hi");
    assert_eq!(usr.role, "user");

    let asst = Message::assistant("reply");
    assert_eq!(asst.role, "assistant");

    let tool = Message::tool("call_1", "result");
    assert_eq!(tool.role, "tool");
    assert_eq!(tool.tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn t99_assistant_with_tool_calls_constructor() {
    let msg = Message::assistant_with_tool_calls(vec![KimiToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: KimiFunctionCall {
            name: "search".into(),
            arguments: "{}".into(),
        },
    }]);
    assert_eq!(msg.role, "assistant");
    assert!(msg.content.is_none());
    assert_eq!(msg.tool_calls.as_ref().unwrap().len(), 1);
}

#[test]
fn t100_backend_name_constant() {
    assert_eq!(abp_kimi_sdk::BACKEND_NAME, "sidecar:kimi");
}
