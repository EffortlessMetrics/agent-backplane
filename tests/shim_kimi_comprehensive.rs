#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
//! Comprehensive tests for the `abp-shim-kimi` crate.
//!
//! Covers initialization/configuration, request/response translation,
//! Kimi-specific features (refs, search_plus), streaming, model mapping,
//! error translation, and edge cases.

use serde_json::json;
use tokio_stream::StreamExt;

use abp_shim_kimi::{
    KimiClient, KimiRequestBuilder, Message, ProcessFn, ShimError, Usage, events_to_stream_chunks,
    ir_to_messages, ir_usage_to_usage, messages_to_ir, mock_receipt, mock_receipt_with_usage,
    receipt_to_response, request_to_ir, request_to_work_order, response_to_ir,
};

use abp_kimi_sdk::dialect::{
    self, CanonicalToolDef, DEFAULT_MODEL, DIALECT_VERSION, KimiChoice, KimiChunk, KimiChunkChoice,
    KimiChunkDelta, KimiChunkFunctionCall, KimiChunkToolCall, KimiConfig, KimiFunctionCall,
    KimiMessage, KimiRef, KimiRequest, KimiResponse, KimiResponseMessage, KimiRole, KimiTool,
    KimiToolCall, KimiUsage, ToolCallAccumulator, builtin_browser, builtin_search_internet,
    capability_manifest, extract_usage, from_canonical_model, is_known_model, map_response,
    map_stream_event, map_work_order, to_canonical_model, tool_def_from_kimi, tool_def_to_kimi,
};
use abp_kimi_sdk::lowering;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, SupportLevel, UsageNormalized, WorkOrderBuilder,
};
use chrono::Utc;

// =========================================================================
// Helpers
// =========================================================================

fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
    Box::new(move |_wo| mock_receipt(events.clone()))
}

fn _make_processor_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> ProcessFn {
    Box::new(move |_wo| mock_receipt_with_usage(events.clone(), usage.clone()))
}

fn assistant_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: text.into() },
        ext: None,
    }
}

fn tool_call_event(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: Some(id.into()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn error_event(message: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: message.into(),
            error_code: None,
        },
        ext: None,
    }
}

fn simple_kimi_response(text: &str) -> KimiResponse {
    KimiResponse {
        id: "cmpl-test".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some(text.into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    }
}

fn make_tool_call(id: &str, name: &str, args: &str) -> KimiToolCall {
    KimiToolCall {
        id: id.into(),
        call_type: "function".into(),
        function: KimiFunctionCall {
            name: name.into(),
            arguments: args.into(),
        },
    }
}

// =========================================================================
// 1. Shim initialization and configuration
// =========================================================================

#[test]
fn client_new_stores_model() {
    let client = KimiClient::new("test-api-key");
    assert_eq!(client.model(), "moonshot-v1-8k");
}

#[test]
fn client_new_custom_model() {
    let client = KimiClient::with_model("moonshot-v1-128k");
    assert_eq!(client.model(), "moonshot-v1-128k");
}

#[test]
fn client_debug_includes_model() {
    let client = KimiClient::with_model("moonshot-v1-32k");
    let debug = format!("{client:?}");
    assert!(debug.contains("moonshot-v1-32k"));
}

#[test]
fn builder_defaults_model_to_8k() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    assert_eq!(req.model, "moonshot-v1-8k");
}

#[test]
fn builder_custom_model() {
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-128k")
        .messages(vec![Message::user("test")])
        .build();
    assert_eq!(req.model, "moonshot-v1-128k");
}

#[test]
fn builder_sets_temperature() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .temperature(0.5)
        .build();
    assert_eq!(req.temperature, Some(0.5));
}

#[test]
fn builder_sets_max_tokens() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .max_tokens(2048)
        .build();
    assert_eq!(req.max_tokens, Some(2048));
}

#[test]
fn builder_sets_stream() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .stream(true)
        .build();
    assert_eq!(req.stream, Some(true));
}

#[test]
fn builder_sets_use_search() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .use_search(true)
        .build();
    assert_eq!(req.use_search, Some(true));
}

#[test]
fn builder_sets_tools() {
    let tool = KimiTool::Function {
        function: dialect::KimiFunctionDef {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .tools(vec![tool])
        .build();
    assert!(req.tools.is_some());
    assert_eq!(req.tools.unwrap().len(), 1);
}

#[test]
fn default_config_sensible() {
    let cfg = KimiConfig::default();
    assert!(cfg.base_url.contains("moonshot.cn"));
    assert_eq!(cfg.model, "moonshot-v1-8k");
    assert!(cfg.max_tokens.is_some());
    assert!(cfg.api_key.is_empty());
}

#[test]
fn dialect_version_is_set() {
    assert_eq!(DIALECT_VERSION, "kimi/v0.1");
}

#[test]
fn default_model_constant() {
    assert_eq!(DEFAULT_MODEL, "moonshot-v1-8k");
}

// =========================================================================
// 2. Request translation (Kimi → IR)
// =========================================================================

#[test]
fn request_to_ir_simple_user() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("Hello")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello");
}

#[test]
fn request_to_ir_system_and_user() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::system("Be concise."), Message::user("Hello")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "Be concise.");
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn request_to_ir_multi_turn() {
    let req = KimiRequestBuilder::new()
        .messages(vec![
            Message::user("What is 2+2?"),
            Message::assistant("4"),
            Message::user("And 3+3?"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[1].role, IrRole::Assistant);
    assert_eq!(conv.messages[2].role, IrRole::User);
}

#[test]
fn request_to_ir_with_tool_message() {
    let req = KimiRequestBuilder::new()
        .messages(vec![
            Message::user("Search for Rust"),
            Message::assistant_with_tool_calls(vec![make_tool_call(
                "call_1",
                "search",
                r#"{"q":"rust"}"#,
            )]),
            Message::tool("call_1", "results here"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[2].role, IrRole::Tool);
}

#[test]
fn request_to_ir_empty_messages() {
    let req = KimiRequestBuilder::new().messages(vec![]).build();
    let conv = request_to_ir(&req);
    assert!(conv.is_empty());
}

#[test]
fn request_to_work_order_extracts_task() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("Fix the bug")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Fix the bug");
}

#[test]
fn request_to_work_order_uses_last_user_message_as_task() {
    let req = KimiRequestBuilder::new()
        .messages(vec![
            Message::system("You are helpful"),
            Message::user("First question"),
            Message::assistant("Answer"),
            Message::user("Second question"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Second question");
}

#[test]
fn request_to_work_order_preserves_model() {
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-128k")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
}

#[test]
fn request_to_work_order_maps_temperature() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .temperature(0.7)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(0.7))
    );
}

#[test]
fn request_to_work_order_maps_max_tokens() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .max_tokens(1024)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("max_tokens"),
        Some(&serde_json::Value::from(1024))
    );
}

#[test]
fn request_to_work_order_no_vendor_flags_when_unset() {
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert!(wo.config.vendor.is_empty());
}

#[test]
fn messages_to_ir_basic() {
    let msgs = vec![
        Message::system("sys"),
        Message::user("hi"),
        Message::assistant("hello"),
    ];
    let conv = messages_to_ir(&msgs);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
}

#[test]
fn messages_to_ir_tool_result() {
    let msgs = vec![Message::tool("call_1", "output data")];
    let conv = messages_to_ir(&msgs);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::Tool);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "call_1");
            assert!(!is_error);
            assert_eq!(content.len(), 1);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn messages_to_ir_assistant_with_tool_calls() {
    let msgs = vec![Message::assistant_with_tool_calls(vec![make_tool_call(
        "call_1",
        "web_search",
        r#"{"q":"test"}"#,
    )])];
    let conv = messages_to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "web_search");
            assert_eq!(input, &json!({"q": "test"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// =========================================================================
// 3. Response translation (IR → Kimi)
// =========================================================================

#[test]
fn ir_to_messages_basic_roundtrip() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "System prompt"),
        IrMessage::text(IrRole::User, "User msg"),
        IrMessage::text(IrRole::Assistant, "Assistant reply"),
    ]);
    let msgs = ir_to_messages(&conv);
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].role, "system");
    assert_eq!(msgs[0].content.as_deref(), Some("System prompt"));
    assert_eq!(msgs[1].role, "user");
    assert_eq!(msgs[2].role, "assistant");
}

#[test]
fn ir_to_messages_tool_result() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "call_42".into(),
            content: vec![IrContentBlock::Text {
                text: "result data".into(),
            }],
            is_error: false,
        }],
    )]);
    let msgs = ir_to_messages(&conv);
    assert_eq!(msgs[0].role, "tool");
    assert_eq!(msgs[0].tool_call_id.as_deref(), Some("call_42"));
    assert_eq!(msgs[0].content.as_deref(), Some("result data"));
}

#[test]
fn ir_to_messages_tool_use() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "call_7".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        }],
    )]);
    let msgs = ir_to_messages(&conv);
    assert_eq!(msgs[0].role, "assistant");
    assert!(msgs[0].content.is_none());
    let tc = &msgs[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_7");
    assert_eq!(tc.function.name, "search");
}

#[test]
fn receipt_to_response_basic() {
    let events = vec![assistant_event("Hello!")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(resp.model, "moonshot-v1-8k");
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn receipt_to_response_tool_calls() {
    let events = vec![tool_call_event(
        "web_search",
        "call_abc",
        json!({"q": "rust"}),
    )];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].id, "call_abc");
    assert_eq!(tcs[0].function.name, "web_search");
}

#[test]
fn receipt_to_response_error_event() {
    let events = vec![error_event("rate limit exceeded")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    let content = resp.choices[0].message.content.as_deref().unwrap();
    assert!(content.contains("rate limit exceeded"));
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn receipt_to_response_with_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    };
    let events = vec![assistant_event("ok")];
    let receipt = mock_receipt_with_usage(events, usage);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

#[test]
fn receipt_to_response_delta_accumulates() {
    let events = vec![delta_event("Hel"), delta_event("lo!")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
}

#[test]
fn receipt_to_response_id_contains_run_id() {
    let events = vec![assistant_event("test")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert!(resp.id.starts_with("cmpl-"));
}

#[test]
fn receipt_to_response_no_refs_by_default() {
    let events = vec![assistant_event("test")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert!(resp.refs.is_none());
}

#[test]
fn receipt_to_response_multi_tool_calls() {
    let events = vec![
        tool_call_event("search", "call_1", json!({"q": "a"})),
        tool_call_event("search", "call_2", json!({"q": "b"})),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs.len(), 2);
    assert_eq!(tcs[0].id, "call_1");
    assert_eq!(tcs[1].id, "call_2");
}

#[test]
fn receipt_to_response_no_tool_calls_is_none() {
    let events = vec![assistant_event("hello")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert!(resp.choices[0].message.tool_calls.is_none());
}

#[test]
fn response_to_ir_simple() {
    let resp = simple_kimi_response("Hello world");
    let conv = response_to_ir(&resp);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    assert_eq!(conv.messages[0].text_content(), "Hello world");
}

#[test]
fn response_to_ir_with_tool_calls() {
    let resp = KimiResponse {
        id: "cmpl-1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![make_tool_call("c1", "web_search", r#"{"q":"test"}"#)]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
        refs: None,
    };
    let conv = response_to_ir(&resp);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { name, .. } => assert_eq!(name, "web_search"),
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// =========================================================================
// 4. Kimi-specific features (refs, search, builtins)
// =========================================================================

#[test]
fn kimi_ref_serde_roundtrip() {
    let r = KimiRef {
        index: 1,
        url: "https://example.com".into(),
        title: Some("Example".into()),
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: KimiRef = serde_json::from_str(&json).unwrap();
    assert_eq!(back.index, 1);
    assert_eq!(back.url, "https://example.com");
    assert_eq!(back.title.as_deref(), Some("Example"));
}

#[test]
fn kimi_ref_without_title() {
    let r = KimiRef {
        index: 2,
        url: "https://docs.rs".into(),
        title: None,
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("title"));
    let back: KimiRef = serde_json::from_str(&json).unwrap();
    assert!(back.title.is_none());
}

#[test]
fn map_response_with_refs_attaches_ext() {
    let resp = KimiResponse {
        id: "cmpl-1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Here is the info [1].".into()),
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
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    let ext = events[0].ext.as_ref().unwrap();
    assert!(ext.contains_key("kimi_refs"));
}

#[test]
fn map_response_without_refs_no_ext() {
    let resp = simple_kimi_response("Just text");
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(events[0].ext.is_none());
}

#[test]
fn builtin_search_internet_tool() {
    let tool = builtin_search_internet();
    assert_eq!(tool.tool_type, "builtin_function");
    assert_eq!(tool.function.name, "$web_search");
}

#[test]
fn builtin_browser_tool() {
    let tool = builtin_browser();
    assert_eq!(tool.tool_type, "builtin_function");
    assert_eq!(tool.function.name, "$browser");
}

#[test]
fn kimi_tool_enum_function_variant() {
    let tool = KimiTool::Function {
        function: dialect::KimiFunctionDef {
            name: "my_func".into(),
            description: "Does stuff".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains("\"type\":\"function\""));
}

#[test]
fn kimi_tool_enum_builtin_variant() {
    let tool = KimiTool::BuiltinFunction {
        function: dialect::KimiBuiltinFunction {
            name: "$web_search".into(),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains("\"type\":\"builtin_function\""));
}

#[test]
fn map_work_order_includes_user_task() {
    let wo = WorkOrderBuilder::new("Optimize DB").build();
    let cfg = KimiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(
        req.messages[0]
            .content
            .as_deref()
            .unwrap()
            .contains("Optimize DB")
    );
}

#[test]
fn map_work_order_respects_model_override() {
    let wo = WorkOrderBuilder::new("task")
        .model("moonshot-v1-128k")
        .build();
    let cfg = KimiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, "moonshot-v1-128k");
}

#[test]
fn map_work_order_falls_back_to_config_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = KimiConfig {
        model: "moonshot-v1-32k".into(),
        ..Default::default()
    };
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, "moonshot-v1-32k");
}

#[test]
fn map_work_order_with_context_snippets() {
    let ctx = abp_core::ContextPacket {
        files: vec![],
        snippets: vec![abp_core::ContextSnippet {
            name: "README".into(),
            content: "Project info".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("task").context(ctx).build();
    let cfg = KimiConfig::default();
    let req = map_work_order(&wo, &cfg);
    let content = req.messages[0].content.as_deref().unwrap();
    assert!(content.contains("README"));
    assert!(content.contains("Project info"));
}

#[test]
fn config_use_k1_enables_search() {
    let cfg = KimiConfig {
        use_k1_reasoning: Some(true),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.use_search, Some(true));
}

#[test]
fn config_use_k1_false_disables_search() {
    let cfg = KimiConfig {
        use_k1_reasoning: Some(false),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = map_work_order(&wo, &cfg);
    assert!(req.use_search.is_none());
}

#[test]
fn extract_usage_from_response() {
    let resp = KimiResponse {
        id: "cmpl-1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: Some(KimiUsage {
            prompt_tokens: 200,
            completion_tokens: 100,
            total_tokens: 300,
        }),
        refs: None,
    };
    let usage = extract_usage(&resp).unwrap();
    assert_eq!(usage["prompt_tokens"], json!(200));
    assert_eq!(usage["completion_tokens"], json!(100));
    assert_eq!(usage["total_tokens"], json!(300));
}

#[test]
fn extract_usage_none_when_absent() {
    let resp = simple_kimi_response("test");
    assert!(extract_usage(&resp).is_none());
}

// =========================================================================
// 5. Streaming handling
// =========================================================================

#[test]
fn events_to_stream_chunks_delta_only() {
    let events = vec![delta_event("hello")];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    assert_eq!(chunks.len(), 2); // 1 delta + 1 stop
    assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("hello"));
    assert_eq!(chunks[1].choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn events_to_stream_chunks_multi_delta() {
    let events = vec![delta_event("Hel"), delta_event("lo"), delta_event("!")];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    assert_eq!(chunks.len(), 4); // 3 deltas + 1 stop
    assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("Hel"));
    assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("lo"));
    assert_eq!(chunks[2].choices[0].delta.content.as_deref(), Some("!"));
}

#[test]
fn events_to_stream_chunks_assistant_message() {
    let events = vec![assistant_event("Complete message")];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    assert_eq!(chunks.len(), 2);
    assert_eq!(
        chunks[0].choices[0].delta.role.as_deref(),
        Some("assistant")
    );
    assert_eq!(
        chunks[0].choices[0].delta.content.as_deref(),
        Some("Complete message")
    );
}

#[test]
fn events_to_stream_chunks_empty_events() {
    let events: Vec<AgentEvent> = vec![];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    assert_eq!(chunks.len(), 1); // only stop chunk
    assert_eq!(chunks[0].choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn events_to_stream_chunks_non_text_events_ignored() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        },
        delta_event("text"),
    ];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    assert_eq!(chunks.len(), 2); // 1 delta + stop
}

#[test]
fn events_to_stream_chunks_all_share_id() {
    let events = vec![delta_event("a"), delta_event("b")];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    let id = &chunks[0].id;
    assert!(chunks.iter().all(|c| &c.id == id));
}

#[test]
fn events_to_stream_chunks_object_is_chunk() {
    let events = vec![delta_event("a")];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
    assert!(chunks.iter().all(|c| c.object == "chat.completion.chunk"));
}

#[test]
fn events_to_stream_chunks_model_preserved() {
    let events = vec![delta_event("a")];
    let chunks = events_to_stream_chunks(&events, "moonshot-v1-128k");
    assert!(chunks.iter().all(|c| c.model == "moonshot-v1-128k"));
}

#[tokio::test]
async fn client_create_stream_produces_chunks() {
    let events = vec![delta_event("Hel"), delta_event("lo!")];
    let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("hi")])
        .stream(true)
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<KimiChunk> = stream.collect().await;
    assert_eq!(chunks.len(), 3);
}

#[test]
fn map_stream_event_text_delta() {
    let chunk = KimiChunk {
        id: "cmpl-1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: None,
                content: Some("hello".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };
    let events = map_stream_event(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hello"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn map_stream_event_finish_reason() {
    let chunk = KimiChunk {
        id: "cmpl-1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = map_stream_event(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::RunCompleted { message } => {
            assert!(message.contains("stop"));
        }
        other => panic!("expected RunCompleted, got {other:?}"),
    }
}

#[test]
fn map_stream_event_with_refs() {
    let chunk = KimiChunk {
        id: "cmpl-1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: None,
                content: Some("text".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: Some(vec![KimiRef {
            index: 1,
            url: "https://example.com".into(),
            title: None,
        }]),
    };
    let events = map_stream_event(&chunk);
    assert!(events[0].ext.is_some());
    assert!(events[0].ext.as_ref().unwrap().contains_key("kimi_refs"));
}

#[test]
fn map_stream_event_empty_content_ignored() {
    let chunk = KimiChunk {
        id: "cmpl-1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: Some("assistant".into()),
                content: Some(String::new()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };
    let events = map_stream_event(&chunk);
    assert!(events.is_empty());
}

// =========================================================================
// 5b. Tool call accumulator
// =========================================================================

#[test]
fn accumulator_single_tool_call() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(KimiChunkFunctionCall {
            name: Some("search".into()),
            arguments: Some(r#"{"q":"test"}"#.into()),
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
            assert_eq!(input, &json!({"q": "test"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn accumulator_incremental_arguments() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(KimiChunkFunctionCall {
            name: Some("search".into()),
            arguments: Some(r#"{"q":"#.into()),
        }),
    }]);
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(KimiChunkFunctionCall {
            name: None,
            arguments: Some(r#""test"}"#.into()),
        }),
    }]);
    let events = acc.finish();
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &json!({"q": "test"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn accumulator_multiple_tool_calls() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[
        KimiChunkToolCall {
            index: 0,
            id: Some("call_a".into()),
            call_type: Some("function".into()),
            function: Some(KimiChunkFunctionCall {
                name: Some("func_a".into()),
                arguments: Some("{}".into()),
            }),
        },
        KimiChunkToolCall {
            index: 1,
            id: Some("call_b".into()),
            call_type: Some("function".into()),
            function: Some(KimiChunkFunctionCall {
                name: Some("func_b".into()),
                arguments: Some("{}".into()),
            }),
        },
    ]);
    let events = acc.finish();
    assert_eq!(events.len(), 2);
}

#[test]
fn accumulator_empty_produces_nothing() {
    let acc = ToolCallAccumulator::new();
    let events = acc.finish();
    assert!(events.is_empty());
}

#[test]
fn accumulator_entry_without_name_skipped() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: None,
        function: Some(KimiChunkFunctionCall {
            name: None,
            arguments: Some("partial".into()),
        }),
    }]);
    let events = acc.finish();
    assert!(events.is_empty());
}

// =========================================================================
// 6. Model mapping
// =========================================================================

#[test]
fn to_canonical_model_prefixes_moonshot() {
    assert_eq!(
        to_canonical_model("moonshot-v1-8k"),
        "moonshot/moonshot-v1-8k"
    );
}

#[test]
fn from_canonical_model_strips_prefix() {
    assert_eq!(
        from_canonical_model("moonshot/moonshot-v1-8k"),
        "moonshot-v1-8k"
    );
}

#[test]
fn from_canonical_model_passthrough_without_prefix() {
    assert_eq!(from_canonical_model("gpt-4"), "gpt-4");
}

#[test]
fn canonical_roundtrip() {
    let model = "moonshot-v1-128k";
    let canonical = to_canonical_model(model);
    let back = from_canonical_model(&canonical);
    assert_eq!(back, model);
}

#[test]
fn is_known_model_recognizes_all() {
    assert!(is_known_model("moonshot-v1-8k"));
    assert!(is_known_model("moonshot-v1-32k"));
    assert!(is_known_model("moonshot-v1-128k"));
    assert!(is_known_model("kimi-latest"));
    assert!(is_known_model("k1"));
}

#[test]
fn is_known_model_rejects_unknown() {
    assert!(!is_known_model("gpt-4"));
    assert!(!is_known_model("claude-3-opus"));
    assert!(!is_known_model("moonshot-v2-8k"));
}

#[test]
fn capability_manifest_has_streaming() {
    let manifest = capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_web_search() {
    let manifest = capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::ToolWebSearch),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_bash_unsupported() {
    let manifest = capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::ToolBash),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn capability_manifest_edit_unsupported() {
    let manifest = capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::ToolEdit),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn capability_manifest_tool_read_native() {
    let manifest = capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_tool_write_emulated() {
    let manifest = capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn capability_manifest_mcp_unsupported() {
    let manifest = capability_manifest();
    assert!(matches!(
        manifest.get(&Capability::McpClient),
        Some(SupportLevel::Unsupported)
    ));
    assert!(matches!(
        manifest.get(&Capability::McpServer),
        Some(SupportLevel::Unsupported)
    ));
}

// =========================================================================
// 7. Error translation
// =========================================================================

#[tokio::test]
async fn no_processor_create_returns_internal_error() {
    let client = KimiClient::new("moonshot-v1-8k");
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let err = client.create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
    assert!(err.to_string().contains("no processor configured"));
}

#[tokio::test]
async fn no_processor_stream_returns_internal_error() {
    let client = KimiClient::new("moonshot-v1-8k");
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let result = client.create_stream(req).await;
    assert!(result.is_err());
}

#[test]
fn shim_error_display_invalid_request() {
    let err = ShimError::InvalidRequest("bad input".into());
    assert!(err.to_string().contains("bad input"));
}

#[test]
fn shim_error_display_internal() {
    let err = ShimError::Internal("broken".into());
    assert!(err.to_string().contains("broken"));
}

#[test]
fn shim_error_from_serde() {
    let json_err = serde_json::from_str::<String>("not-json").unwrap_err();
    let err: ShimError = json_err.into();
    assert!(matches!(err, ShimError::Serde(_)));
}

#[test]
fn error_event_in_receipt_maps_to_content() {
    let events = vec![error_event("API timeout")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    let content = resp.choices[0].message.content.as_deref().unwrap();
    assert!(content.contains("API timeout"));
}

#[test]
fn error_after_text_overwrites_content() {
    let events = vec![
        assistant_event("partial response"),
        error_event("connection lost"),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    let content = resp.choices[0].message.content.as_deref().unwrap();
    assert!(content.contains("connection lost"));
}

// =========================================================================
// 8. Edge cases
// =========================================================================

#[test]
fn message_system_constructor() {
    let m = Message::system("You are helpful.");
    assert_eq!(m.role, "system");
    assert_eq!(m.content.as_deref(), Some("You are helpful."));
    assert!(m.tool_calls.is_none());
    assert!(m.tool_call_id.is_none());
}

#[test]
fn message_user_constructor() {
    let m = Message::user("Hello");
    assert_eq!(m.role, "user");
    assert_eq!(m.content.as_deref(), Some("Hello"));
}

#[test]
fn message_assistant_constructor() {
    let m = Message::assistant("World");
    assert_eq!(m.role, "assistant");
    assert_eq!(m.content.as_deref(), Some("World"));
}

#[test]
fn message_assistant_with_tool_calls_constructor() {
    let m = Message::assistant_with_tool_calls(vec![make_tool_call("c1", "search", "{}")]);
    assert_eq!(m.role, "assistant");
    assert!(m.content.is_none());
    assert_eq!(m.tool_calls.as_ref().unwrap().len(), 1);
}

#[test]
fn message_tool_constructor() {
    let m = Message::tool("call_1", "result");
    assert_eq!(m.role, "tool");
    assert_eq!(m.content.as_deref(), Some("result"));
    assert_eq!(m.tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn message_serde_roundtrip() {
    let m = Message::user("test");
    let json = serde_json::to_string(&m).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, "user");
    assert_eq!(back.content.as_deref(), Some("test"));
}

#[test]
fn usage_struct_equality() {
    let u1 = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let u2 = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    assert_eq!(u1, u2);
}

#[test]
fn usage_serde_roundtrip() {
    let u = Usage {
        prompt_tokens: 200,
        completion_tokens: 100,
        total_tokens: 300,
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, u);
}

#[test]
fn ir_usage_to_usage_basic() {
    let ir = IrUsage::from_io(500, 200);
    let u = ir_usage_to_usage(&ir);
    assert_eq!(u.prompt_tokens, 500);
    assert_eq!(u.completion_tokens, 200);
    assert_eq!(u.total_tokens, 700);
}

#[test]
fn ir_usage_to_usage_zero() {
    let ir = IrUsage::from_io(0, 0);
    let u = ir_usage_to_usage(&ir);
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.completion_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn lowering_usage_to_ir() {
    let usage = KimiUsage {
        prompt_tokens: 300,
        completion_tokens: 150,
        total_tokens: 450,
    };
    let ir = lowering::usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 300);
    assert_eq!(ir.output_tokens, 150);
    assert_eq!(ir.total_tokens, 450);
}

#[test]
fn tool_def_roundtrip() {
    let canonical = CanonicalToolDef {
        name: "search".into(),
        description: "Search the web".into(),
        parameters_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    let kimi = tool_def_to_kimi(&canonical);
    assert_eq!(kimi.tool_type, "function");
    assert_eq!(kimi.function.name, "search");

    let back = tool_def_from_kimi(&kimi);
    assert_eq!(back, canonical);
}

#[test]
fn kimi_role_display() {
    assert_eq!(KimiRole::System.to_string(), "system");
    assert_eq!(KimiRole::User.to_string(), "user");
    assert_eq!(KimiRole::Assistant.to_string(), "assistant");
    assert_eq!(KimiRole::Tool.to_string(), "tool");
}

#[test]
fn kimi_role_serde_roundtrip() {
    for role in [
        KimiRole::System,
        KimiRole::User,
        KimiRole::Assistant,
        KimiRole::Tool,
    ] {
        let json = serde_json::to_string(&role).unwrap();
        let back: KimiRole = serde_json::from_str(&json).unwrap();
        assert_eq!(back, role);
    }
}

#[test]
fn kimi_request_serde_roundtrip() {
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-8k")
        .messages(vec![Message::user("hello")])
        .temperature(0.5)
        .max_tokens(1024)
        .stream(true)
        .build();
    let json = serde_json::to_string(&req).unwrap();
    let back: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "moonshot-v1-8k");
    assert_eq!(back.temperature, Some(0.5));
    assert_eq!(back.max_tokens, Some(1024));
    assert_eq!(back.stream, Some(true));
}

#[test]
fn kimi_response_serde_roundtrip() {
    let resp = simple_kimi_response("hello");
    let json = serde_json::to_string(&resp).unwrap();
    let back: KimiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.choices[0].message.content.as_deref(), Some("hello"));
}

#[test]
fn empty_receipt_trace_produces_no_content() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert!(resp.choices[0].message.content.is_none());
    assert!(resp.choices[0].message.tool_calls.is_none());
}

#[test]
fn receipt_with_zero_usage() {
    let usage = UsageNormalized::default();
    let events = vec![assistant_event("ok")];
    let receipt = mock_receipt_with_usage(events, usage);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.completion_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn receipt_to_response_ignores_file_changed_events() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "Modified main".into(),
            },
            ext: None,
        },
        assistant_event("done"),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("done"));
}

#[test]
fn receipt_to_response_ignores_command_events() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: None,
            },
            ext: None,
        },
        assistant_event("all tests pass"),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("all tests pass")
    );
}

#[test]
fn receipt_to_response_ignores_warning_events() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "deprecated API".into(),
            },
            ext: None,
        },
        assistant_event("ok"),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("ok"));
}

#[tokio::test]
async fn full_roundtrip_create() {
    let events = vec![assistant_event("Computed answer: 42")];
    let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-8k")
        .messages(vec![
            Message::system("You are a calculator."),
            Message::user("What is 6*7?"),
        ])
        .temperature(0.0)
        .max_tokens(100)
        .build();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.model, "moonshot-v1-8k");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Computed answer: 42")
    );
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    assert_eq!(resp.choices[0].message.role, "assistant");
}

#[tokio::test]
async fn full_roundtrip_stream() {
    let events = vec![delta_event("4"), delta_event("2")];
    let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("6*7?")])
        .stream(true)
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<KimiChunk> = stream.collect().await;
    assert_eq!(chunks.len(), 3);
    let full_text: String = chunks
        .iter()
        .filter_map(|c| c.choices.first()?.delta.content.as_deref())
        .collect();
    assert_eq!(full_text, "42");
}

#[tokio::test]
async fn full_roundtrip_tool_use() {
    let events = vec![tool_call_event(
        "web_search",
        "call_99",
        json!({"query": "rust async"}),
    )];
    let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("Search for rust async")])
        .build();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].function.name, "web_search");
    assert_eq!(tcs[0].call_type, "function");
}

#[test]
fn ir_conversation_accessors_work_with_kimi_data() {
    let req = KimiRequestBuilder::new()
        .messages(vec![
            Message::system("sys"),
            Message::user("u1"),
            Message::assistant("a1"),
            Message::user("u2"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.system_message().unwrap().text_content(), "sys");
    assert_eq!(conv.last_assistant().unwrap().text_content(), "a1");
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    assert_eq!(conv.last_message().unwrap().text_content(), "u2");
}

#[test]
fn tool_call_without_explicit_id_gets_generated() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "search".into(),
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
fn kimi_chunk_serde_roundtrip() {
    let chunk = KimiChunk {
        id: "cmpl-test".into(),
        object: "chat.completion.chunk".into(),
        created: 1234567890,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: Some("assistant".into()),
                content: Some("hello".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: KimiChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "cmpl-test");
    assert_eq!(back.choices[0].delta.content.as_deref(), Some("hello"));
}

#[test]
fn kimi_config_serde_roundtrip() {
    let cfg = KimiConfig {
        api_key: "sk-test".into(),
        base_url: "https://api.moonshot.cn/v1".into(),
        model: "moonshot-v1-8k".into(),
        max_tokens: Some(4096),
        temperature: Some(0.7),
        use_k1_reasoning: Some(true),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: KimiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.api_key, "sk-test");
    assert_eq!(back.use_k1_reasoning, Some(true));
}

#[test]
fn map_response_empty_content_ignored() {
    let resp = KimiResponse {
        id: "cmpl-1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some(String::new()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_multiple_choices() {
    let resp = KimiResponse {
        id: "cmpl-1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![
            KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("choice 0".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            },
            KimiChoice {
                index: 1,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("choice 1".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            },
        ],
        usage: None,
        refs: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 2);
}

#[test]
fn map_response_text_and_tool_calls() {
    let resp = KimiResponse {
        id: "cmpl-1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Let me search.".into()),
                tool_calls: Some(vec![make_tool_call("c1", "search", "{}")]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
        refs: None,
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
fn malformed_tool_args_kept_as_string_value() {
    let resp = KimiResponse {
        id: "cmpl-1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![make_tool_call("c1", "func", "not-valid-json")]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-valid-json".into()));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn kimi_usage_serde_roundtrip() {
    let u = KimiUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: KimiUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, u);
}

#[test]
fn lowering_to_ir_unknown_role_defaults_user() {
    let msgs = vec![KimiMessage {
        role: "developer".into(),
        content: Some("hi".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn lowering_roundtrip_preserves_content() {
    let msgs = vec![
        KimiMessage {
            role: "system".into(),
            content: Some("Be helpful.".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_call_id: None,
            tool_calls: None,
        },
    ];
    let conv = lowering::to_ir(&msgs);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].content.as_deref(), Some("Be helpful."));
    assert_eq!(back[1].content.as_deref(), Some("Hello"));
}

#[test]
fn kimi_message_with_none_content() {
    let msg = KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: None,
    };
    let conv = lowering::to_ir(&[msg]);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn mock_receipt_has_correct_contract_version() {
    let receipt = mock_receipt(vec![]);
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
}

#[test]
fn mock_receipt_outcome_is_complete() {
    let receipt = mock_receipt(vec![]);
    assert_eq!(receipt.outcome, abp_core::Outcome::Complete);
}

#[test]
fn mock_receipt_backend_is_mock() {
    let receipt = mock_receipt(vec![]);
    assert_eq!(receipt.backend.id, "mock");
}

#[test]
fn client_with_processor_returns_ok() {
    let client = KimiClient::new("moonshot-v1-8k")
        .with_processor(make_processor(vec![assistant_event("hi")]));
    assert_eq!(client.model(), "moonshot-v1-8k");
}
