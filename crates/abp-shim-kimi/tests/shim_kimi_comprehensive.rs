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
//! Comprehensive tests for the Kimi shim crate — validates that ABP can act
//! as a Kimi SDK drop-in replacement.

use abp_core::ir::{IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, UsageNormalized};
use abp_kimi_sdk::dialect::{
    self, CanonicalToolDef, KimiBuiltinFunction, KimiBuiltinTool, KimiChoice, KimiChunk,
    KimiChunkChoice, KimiChunkDelta, KimiChunkFunctionCall, KimiChunkToolCall, KimiConfig,
    KimiFunctionCall, KimiFunctionDef, KimiMessage, KimiRef, KimiRequest, KimiResponse,
    KimiResponseMessage, KimiRole, KimiTool, KimiToolCall, KimiUsage, ToolCallAccumulator,
};
use abp_shim_kimi::{
    events_to_stream_chunks, ir_to_messages, ir_usage_to_usage, messages_to_ir, mock_receipt,
    mock_receipt_with_usage, receipt_to_response, request_to_ir, request_to_work_order, KimiClient,
    KimiRequestBuilder, Message, ProcessFn, ShimError,
};
use chrono::Utc;
use serde_json::json;
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

// ═══════════════════════════════════════════════════════════════════════════
// 1. Kimi SDK types fidelity (~15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t01_kimi_request_serde_roundtrip() {
    let req = KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![KimiMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_call_id: None,
            tool_calls: None,
        }],
        max_tokens: Some(4096),
        temperature: Some(0.7),
        stream: None,
        tools: None,
        use_search: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "moonshot-v1-8k");
    assert_eq!(parsed.messages.len(), 1);
    assert_eq!(parsed.max_tokens, Some(4096));
}

#[test]
fn t02_kimi_response_serde_roundtrip() {
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
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
        refs: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: KimiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "cmpl-abc");
    assert_eq!(parsed.choices[0].message.content.as_deref(), Some("Hello!"));
    assert_eq!(parsed.usage.unwrap().total_tokens, 15);
}

#[test]
fn t03_kimi_message_roles_serde() {
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
fn t04_kimi_role_display() {
    assert_eq!(KimiRole::System.to_string(), "system");
    assert_eq!(KimiRole::User.to_string(), "user");
    assert_eq!(KimiRole::Assistant.to_string(), "assistant");
    assert_eq!(KimiRole::Tool.to_string(), "tool");
}

#[test]
fn t05_kimi_usage_serde_roundtrip() {
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
fn t06_kimi_tool_call_serde_roundtrip() {
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
fn t07_kimi_chunk_serde_roundtrip() {
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
fn t08_kimi_ref_serde_roundtrip() {
    let r = KimiRef {
        index: 1,
        url: "https://example.com".into(),
        title: Some("Example".into()),
    };
    let json = serde_json::to_string(&r).unwrap();
    let parsed: KimiRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, r);
}

#[test]
fn t09_kimi_ref_without_title() {
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

#[test]
fn t10_kimi_config_default_values() {
    let cfg = KimiConfig::default();
    assert!(cfg.base_url.contains("moonshot.cn"));
    assert_eq!(cfg.model, "moonshot-v1-8k");
    assert_eq!(cfg.max_tokens, Some(4096));
    assert!(cfg.api_key.is_empty());
    assert!(cfg.temperature.is_none());
    assert!(cfg.use_k1_reasoning.is_none());
}

#[test]
fn t11_kimi_config_serde_roundtrip() {
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

#[test]
fn t12_kimi_tool_enum_function_serde() {
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
fn t13_kimi_tool_enum_builtin_serde() {
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
fn t14_kimi_chunk_delta_default_is_empty() {
    let delta = KimiChunkDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

#[test]
fn t15_kimi_request_omits_none_fields() {
    let req = KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: None,
        use_search: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("max_tokens"));
    assert!(!json.contains("temperature"));
    assert!(!json.contains("stream"));
    assert!(!json.contains("tools"));
    assert!(!json.contains("use_search"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Request translation (~15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t16_request_to_work_order_basic() {
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-8k")
        .messages(vec![Message::user("Explain async")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Explain async");
    assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
}

#[test]
fn t17_request_to_work_order_with_system_message() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::system("Be concise."), Message::user("Hello")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Hello");
}

#[test]
fn t18_request_to_work_order_temperature_mapped() {
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
fn t19_request_to_work_order_max_tokens_mapped() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .max_tokens(2048)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("max_tokens"),
        Some(&serde_json::Value::from(2048))
    );
}

#[test]
fn t20_request_to_work_order_multi_turn_uses_last_user() {
    let req = KimiRequestBuilder::new()
        .messages(vec![
            Message::user("First question"),
            Message::assistant("First answer"),
            Message::user("Second question"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Second question");
}

#[test]
fn t21_request_to_work_order_model_default() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    assert_eq!(req.model, "moonshot-v1-8k");
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
}

#[test]
fn t22_request_to_ir_basic() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::system("Be helpful."), Message::user("Hi")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "Be helpful.");
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn t23_request_to_ir_with_tool_messages() {
    let req = KimiRequestBuilder::new()
        .messages(vec![
            Message::user("Search for rust"),
            Message::assistant_with_tool_calls(vec![KimiToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
            Message::tool("call_1", "search results"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[2].role, IrRole::Tool);
}

#[test]
fn t24_request_to_work_order_model_128k() {
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-128k")
        .messages(vec![Message::user("analyze long doc")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
}

#[test]
fn t25_request_to_work_order_no_user_message() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::system("System only")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "kimi completion");
}

#[test]
fn t26_request_builder_stream_flag() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .stream(true)
        .build();
    assert_eq!(req.stream, Some(true));
}

#[test]
fn t27_request_builder_use_search_flag() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .use_search(true)
        .build();
    assert_eq!(req.use_search, Some(true));
}

#[test]
fn t28_request_builder_tools() {
    let tools = vec![KimiTool::Function {
        function: KimiFunctionDef {
            name: "bash".into(),
            description: "Run commands".into(),
            parameters: json!({"type": "object"}),
        },
    }];
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .tools(tools)
        .build();
    assert!(req.tools.is_some());
    assert_eq!(req.tools.unwrap().len(), 1);
}

#[test]
fn t29_messages_to_ir_roundtrip() {
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
fn t30_request_to_ir_empty_messages() {
    let req = KimiRequestBuilder::new().messages(vec![]).build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Response translation (~15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t31_receipt_to_response_assistant_text() {
    let events = vec![assistant_event("Hello!")];
    let client = KimiClient::with_model("moonshot-v1-8k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("Hi")])
        .build();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[tokio::test]
async fn t32_receipt_to_response_tool_calls() {
    let events = vec![tool_call_event(
        "web_search",
        "call_1",
        json!({"query": "rust"}),
    )];
    let client = KimiClient::with_model("moonshot-v1-8k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("Search rust")])
        .build();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].id, "call_1");
    assert_eq!(tcs[0].function.name, "web_search");
}

#[tokio::test]
async fn t33_receipt_to_response_error_event() {
    let events = vec![error_event("rate limit exceeded")];
    let client = KimiClient::with_model("moonshot-v1-8k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let resp = client.create(req).await.unwrap();
    let content = resp.choices[0].message.content.as_deref().unwrap();
    assert!(content.contains("rate limit exceeded"));
}

#[tokio::test]
async fn t34_receipt_to_response_usage_tracking() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    };
    let events = vec![assistant_event("response")];
    let client = KimiClient::with_model("moonshot-v1-8k")
        .with_processor(make_processor_with_usage(events, usage));
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let resp = client.create(req).await.unwrap();
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

#[tokio::test]
async fn t35_receipt_to_response_model_preserved() {
    let events = vec![assistant_event("ok")];
    let client = KimiClient::with_model("moonshot-v1-128k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-128k")
        .messages(vec![Message::user("test")])
        .build();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.model, "moonshot-v1-128k");
}

#[tokio::test]
async fn t36_receipt_to_response_multi_tool_calls() {
    let events = vec![
        tool_call_event("search", "call_1", json!({"q": "a"})),
        tool_call_event("search", "call_2", json!({"q": "b"})),
    ];
    let client = KimiClient::with_model("moonshot-v1-8k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("search")])
        .build();
    let resp = client.create(req).await.unwrap();
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs.len(), 2);
}

#[test]
fn t37_receipt_to_response_direct() {
    let events = vec![assistant_event("direct response")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("direct response")
    );
    assert!(resp.id.starts_with("cmpl-"));
}

#[test]
fn t38_receipt_to_response_delta_concatenation() {
    let events = vec![delta_event("Part 1 "), delta_event("Part 2")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Part 1 Part 2")
    );
}

#[test]
fn t39_receipt_to_response_mixed_text_and_tool_calls() {
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

#[test]
fn t40_receipt_to_response_empty_trace() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert!(resp.choices[0].message.content.is_none());
    assert!(resp.choices[0].message.tool_calls.is_none());
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t41_response_to_ir_basic() {
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
    let conv = abp_shim_kimi::response_to_ir(&resp);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    assert_eq!(conv.messages[0].text_content(), "Hello");
}

#[test]
fn t42_ir_usage_conversion() {
    let ir = IrUsage::from_io(200, 100);
    let usage = ir_usage_to_usage(&ir);
    assert_eq!(usage.prompt_tokens, 200);
    assert_eq!(usage.completion_tokens, 100);
    assert_eq!(usage.total_tokens, 300);
}

#[tokio::test]
async fn t43_no_processor_returns_error() {
    let client = KimiClient::with_model("moonshot-v1-8k");
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let err = client.create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
}

#[test]
fn t44_receipt_to_response_zero_usage() {
    let usage = UsageNormalized::default();
    let receipt = mock_receipt_with_usage(vec![assistant_event("ok")], usage);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.completion_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn t45_receipt_to_response_tool_call_without_id() {
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
    // Should generate a UUID-based id
    assert!(tcs[0].id.starts_with("call_"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Streaming (~10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t46_streaming_basic() {
    let events = vec![delta_event("Hel"), delta_event("lo!")];
    let client = KimiClient::with_model("moonshot-v1-8k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("Hi")])
        .stream(true)
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<KimiChunk> = stream.collect().await;
    // 2 deltas + 1 final stop chunk
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("Hel"));
    assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("lo!"));
}

#[tokio::test]
async fn t47_streaming_ends_with_stop() {
    let events = vec![delta_event("hi")];
    let client = KimiClient::with_model("moonshot-v1-8k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<KimiChunk> = stream.collect().await;
    let last = chunks.last().unwrap();
    assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t48_events_to_stream_chunks_delta_events() {
    let events = vec![delta_event("a"), delta_event("b"), delta_event("c")];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    // 3 deltas + 1 stop
    assert_eq!(chunks.len(), 4);
    assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("a"));
    assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("b"));
    assert_eq!(chunks[2].choices[0].delta.content.as_deref(), Some("c"));
    assert!(chunks[3].choices[0].finish_reason.is_some());
}

#[test]
fn t49_events_to_stream_chunks_assistant_message() {
    let events = vec![assistant_event("full message")];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    // 1 message chunk + 1 stop
    assert_eq!(chunks.len(), 2);
    assert_eq!(
        chunks[0].choices[0].delta.content.as_deref(),
        Some("full message")
    );
    assert_eq!(
        chunks[0].choices[0].delta.role.as_deref(),
        Some("assistant")
    );
}

#[test]
fn t50_events_to_stream_chunks_empty() {
    let chunks = events_to_stream_chunks(&[], "moonshot-v1-8k");
    // Just the stop chunk
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t51_events_to_stream_chunks_model_preserved() {
    let chunks = events_to_stream_chunks(&[delta_event("x")], "kimi-latest");
    for chunk in &chunks {
        assert_eq!(chunk.model, "kimi-latest");
    }
}

#[test]
fn t52_events_to_stream_chunks_consistent_id() {
    let events = vec![delta_event("a"), delta_event("b")];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    let id = &chunks[0].id;
    for chunk in &chunks {
        assert_eq!(&chunk.id, id);
    }
}

#[test]
fn t53_events_to_stream_chunks_object_type() {
    let chunks = events_to_stream_chunks(&[delta_event("x")], "moonshot-v1-8k");
    for chunk in &chunks {
        assert_eq!(chunk.object, "chat.completion.chunk");
    }
}

#[tokio::test]
async fn t54_streaming_no_processor_returns_error() {
    let client = KimiClient::with_model("moonshot-v1-8k");
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let result = client.create_stream(req).await;
    assert!(result.is_err());
}

#[test]
fn t55_events_to_stream_chunks_skips_non_text_events() {
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
    // 1 delta + 1 stop (RunStarted is skipped)
    assert_eq!(chunks.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Tool use (~10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t56_tool_def_to_kimi_and_back() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let kimi = dialect::tool_def_to_kimi(&canonical);
    assert_eq!(kimi.tool_type, "function");
    assert_eq!(kimi.function.name, "read_file");
    let back = dialect::tool_def_from_kimi(&kimi);
    assert_eq!(back, canonical);
}

#[test]
fn t57_builtin_search_internet() {
    let tool = dialect::builtin_search_internet();
    assert_eq!(tool.tool_type, "builtin_function");
    assert_eq!(tool.function.name, "$web_search");
}

#[test]
fn t58_builtin_browser() {
    let tool = dialect::builtin_browser();
    assert_eq!(tool.tool_type, "builtin_function");
    assert_eq!(tool.function.name, "$browser");
}

#[test]
fn t59_builtin_tool_serde_roundtrip() {
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
fn t60_tool_call_accumulator_single() {
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
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "search");
            assert_eq!(tool_use_id.as_deref(), Some("call_1"));
            assert_eq!(input, &json!({"q": "rust"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn t61_tool_call_accumulator_incremental() {
    let mut acc = ToolCallAccumulator::new();
    // First fragment: id + name
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(KimiChunkFunctionCall {
            name: Some("search".into()),
            arguments: Some(r#"{"q""#.into()),
        }),
    }]);
    // Second fragment: more arguments
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
fn t62_tool_call_accumulator_multiple_tools() {
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
fn t63_tool_call_accumulator_empty() {
    let acc = ToolCallAccumulator::new();
    let events = acc.finish();
    assert!(events.is_empty());
}

#[test]
fn t64_tool_call_accumulator_no_name_skipped() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: None,
    }]);
    let events = acc.finish();
    // Entry without a name is filtered out
    assert!(events.is_empty());
}

#[test]
fn t65_tool_def_kimi_roundtrip_complex_schema() {
    let schema = json!({
        "type": "object",
        "properties": {
            "location": {"type": "string", "description": "City name"},
            "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
        },
        "required": ["location"]
    });
    let canonical = CanonicalToolDef {
        name: "get_weather".into(),
        description: "Get weather for a location".into(),
        parameters_schema: schema.clone(),
    };
    let kimi = dialect::tool_def_to_kimi(&canonical);
    assert_eq!(kimi.function.parameters, schema);
    let back = dialect::tool_def_from_kimi(&kimi);
    assert_eq!(back.parameters_schema, schema);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Edge cases (~5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t66_empty_messages_roundtrip() {
    let messages: Vec<Message> = vec![];
    let conv = messages_to_ir(&messages);
    assert!(conv.is_empty());
    let back = ir_to_messages(&conv);
    assert!(back.is_empty());
}

#[test]
fn t67_model_name_mapping() {
    assert_eq!(
        dialect::to_canonical_model("moonshot-v1-8k"),
        "moonshot/moonshot-v1-8k"
    );
    assert_eq!(
        dialect::from_canonical_model("moonshot/moonshot-v1-8k"),
        "moonshot-v1-8k"
    );
    assert_eq!(
        dialect::from_canonical_model("moonshot-v1-8k"),
        "moonshot-v1-8k"
    );
}

#[test]
fn t68_known_models() {
    assert!(dialect::is_known_model("moonshot-v1-8k"));
    assert!(dialect::is_known_model("moonshot-v1-32k"));
    assert!(dialect::is_known_model("moonshot-v1-128k"));
    assert!(dialect::is_known_model("kimi-latest"));
    assert!(dialect::is_known_model("k1"));
    assert!(!dialect::is_known_model("gpt-4"));
    assert!(!dialect::is_known_model("unknown-model"));
}

#[test]
fn t69_capability_manifest() {
    use abp_core::Capability;
    let manifest = dialect::capability_manifest();
    // Verify capabilities are present by checking they're in the manifest
    assert!(manifest.contains_key(&Capability::Streaming));
    assert!(manifest.contains_key(&Capability::ToolWebSearch));
    assert!(manifest.contains_key(&Capability::ToolEdit));
    assert!(manifest.contains_key(&Capability::ToolRead));
    // Verify the manifest has the expected number of entries
    let json = serde_json::to_string(&manifest).unwrap();
    assert!(json.contains("streaming"));
    assert!(json.contains("tool_web_search"));
}

#[test]
fn t70_extract_usage_helper() {
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
        usage_map.get("completion_tokens"),
        Some(&serde_json::Value::from(25))
    );
    assert_eq!(
        usage_map.get("total_tokens"),
        Some(&serde_json::Value::from(75))
    );

    // No usage returns None
    let resp_no_usage = KimiResponse {
        id: "cmpl-2".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: None,
        refs: None,
    };
    assert!(dialect::extract_usage(&resp_no_usage).is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// §11 — Additional coverage (t71–t85)
// ═══════════════════════════════════════════════════════════════════════════

// ── 71. ShimError Display variants ──────────────────────────────────────

#[test]
fn t71_shim_error_display_variants() {
    let inv = ShimError::InvalidRequest("bad input".into());
    assert!(inv.to_string().contains("bad input"));

    let int = ShimError::Internal("boom".into());
    assert!(int.to_string().contains("boom"));

    let serde_err: std::result::Result<serde_json::Value, _> = serde_json::from_str("{{bad");
    let se = ShimError::Serde(serde_err.unwrap_err());
    assert!(se.to_string().contains("serde error"));
}

// ── 72. Message::tool constructor ───────────────────────────────────────

#[test]
fn t72_message_tool_constructor() {
    let msg = Message::tool("call_123", "result text");
    assert_eq!(msg.role, "tool");
    assert_eq!(msg.content.as_deref(), Some("result text"));
    assert_eq!(msg.tool_call_id.as_deref(), Some("call_123"));
    assert!(msg.tool_calls.is_none());
}

// ── 73. Message::assistant_with_tool_calls constructor ──────────────────

#[test]
fn t73_message_assistant_with_tool_calls() {
    let tc = KimiToolCall {
        id: "call_x".into(),
        call_type: "function".into(),
        function: KimiFunctionCall {
            name: "search".into(),
            arguments: "{}".into(),
        },
    };
    let msg = Message::assistant_with_tool_calls(vec![tc.clone()]);
    assert_eq!(msg.role, "assistant");
    assert!(msg.content.is_none());
    let tcs = msg.tool_calls.unwrap();
    assert_eq!(tcs.len(), 1);
    assert_eq!(tcs[0].id, "call_x");
}

// ── 74. Message serde roundtrip ─────────────────────────────────────────

#[test]
fn t74_message_serde_roundtrip() {
    let msg = Message::user("hello world");
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, "user");
    assert_eq!(back.content.as_deref(), Some("hello world"));
    assert!(back.tool_calls.is_none());
    assert!(back.tool_call_id.is_none());
}

// ── 75. Usage serde roundtrip ───────────────────────────────────────────

#[test]
fn t75_usage_serde_roundtrip() {
    use abp_shim_kimi::Usage;
    let usage = Usage {
        prompt_tokens: 42,
        completion_tokens: 17,
        total_tokens: 59,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, usage);
}

// ── 76. to_canonical_model / from_canonical_model roundtrip ─────────────

#[test]
fn t76_canonical_model_roundtrip() {
    let vendor = "moonshot-v1-128k";
    let canonical = dialect::to_canonical_model(vendor);
    assert_eq!(canonical, "moonshot/moonshot-v1-128k");
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

// ── 77. from_canonical_model without prefix passes through ──────────────

#[test]
fn t77_from_canonical_model_no_prefix() {
    let result = dialect::from_canonical_model("gpt-4");
    assert_eq!(result, "gpt-4");
}

// ── 78. response_to_ir with tool calls ──────────────────────────────────

#[test]
fn t78_response_to_ir_with_tool_calls() {
    use abp_shim_kimi::response_to_ir;
    let resp = KimiResponse {
        id: "cmpl-1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![KimiToolCall {
                    id: "call_a".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "web_search".into(),
                        arguments: r#"{"q":"test"}"#.into(),
                    },
                }]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
        refs: None,
    };
    let conv = response_to_ir(&resp);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
}

// ── 79. map_work_order from dialect ─────────────────────────────────────

#[test]
fn t79_dialect_map_work_order() {
    use abp_core::WorkOrderBuilder;
    let wo = WorkOrderBuilder::new("Summarize this article").build();
    let cfg = KimiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "moonshot-v1-8k");
    assert!(req.messages[0]
        .content
        .as_deref()
        .unwrap()
        .contains("Summarize this article"));
}

// ── 80. map_response from dialect ───────────────────────────────────────

#[test]
fn t80_dialect_map_response_text() {
    let resp = KimiResponse {
        id: "cmpl-x".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Answer here".into()),
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
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Answer here"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

// ── 81. map_stream_event from dialect ───────────────────────────────────

#[test]
fn t81_dialect_map_stream_event_delta() {
    let chunk = KimiChunk {
        id: "cmpl-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: None,
                content: Some("partial".into()),
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
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "partial"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

// ── 82. map_stream_event finish_reason emits RunCompleted ───────────────

#[test]
fn t82_dialect_map_stream_event_finish() {
    let chunk = KimiChunk {
        id: "cmpl-1".into(),
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

// ── 83. Client builder default base_url ─────────────────────────────────

#[test]
fn t83_client_builder_defaults() {
    use abp_shim_kimi::client::Client;
    let client = Client::new("sk-test-key").unwrap();
    assert_eq!(client.base_url(), "https://api.moonshot.cn/v1");
}

// ── 84. ClientError display ─────────────────────────────────────────────

#[test]
fn t84_client_error_display() {
    use abp_shim_kimi::client::ClientError;
    let err = ClientError::Api {
        status: 500,
        body: "internal server error".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("500"));
    assert!(msg.contains("internal server error"));

    let builder_err = ClientError::Builder("cannot build".into());
    assert!(builder_err.to_string().contains("cannot build"));
}

// ── 85. KimiClient Debug impl ───────────────────────────────────────────

#[test]
fn t85_kimi_client_debug() {
    let client = KimiClient::with_model("moonshot-v1-8k");
    let dbg = format!("{:?}", client);
    assert!(dbg.contains("moonshot-v1-8k"));
    assert!(dbg.contains("KimiClient"));
}

// ── 86. map_response with refs attaches ext metadata ────────────────────

#[test]
fn t86_dialect_map_response_with_refs() {
    let resp = KimiResponse {
        id: "cmpl-r".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("See [1].".into()),
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
    let ext = events[0].ext.as_ref().expect("ext should be present");
    assert!(ext.contains_key("kimi_refs"));
}

// ── 87. DIALECT_VERSION constant ────────────────────────────────────────

#[test]
fn t87_dialect_version() {
    assert_eq!(dialect::DIALECT_VERSION, "kimi/v0.1");
}

// ── 88. DEFAULT_MODEL constant ──────────────────────────────────────────

#[test]
fn t88_default_model() {
    assert_eq!(dialect::DEFAULT_MODEL, "moonshot-v1-8k");
}

// ── 89. KimiChunkToolCall serde roundtrip ───────────────────────────────

#[test]
fn t89_chunk_tool_call_serde_roundtrip() {
    let ctc = KimiChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(KimiChunkFunctionCall {
            name: Some("search".into()),
            arguments: Some(r#"{"q":"test"}"#.into()),
        }),
    };
    let json = serde_json::to_string(&ctc).unwrap();
    let back: KimiChunkToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ctc);
}

// ── 90. map_response empty content skipped ──────────────────────────────

#[test]
fn t90_dialect_map_response_empty_content_skipped() {
    let resp = KimiResponse {
        id: "cmpl-e".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty(), "empty string content should be skipped");
}
