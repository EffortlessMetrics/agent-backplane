#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the abp-kimi-sdk crate covering all public API.

use abp_kimi_sdk::api;
use abp_kimi_sdk::convert;
use abp_kimi_sdk::dialect;
use abp_kimi_sdk::lowering;
use abp_kimi_sdk::types;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest, ContextPacket,
    ContextSnippet, ExecutionMode, Outcome, Receipt, ReceiptBuilder, RunMetadata, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use uuid::Uuid;

// ============================================================================
// Module: lib.rs – Constants and registration helpers
// ============================================================================

#[test]
fn lib_backend_name_constant() {
    assert_eq!(abp_kimi_sdk::BACKEND_NAME, "sidecar:kimi");
}

#[test]
fn lib_host_script_relative_constant() {
    assert_eq!(abp_kimi_sdk::HOST_SCRIPT_RELATIVE, "hosts/kimi/host.js");
}

#[test]
fn lib_default_node_command_constant() {
    assert_eq!(abp_kimi_sdk::DEFAULT_NODE_COMMAND, "node");
}

#[test]
fn lib_sidecar_script_joins_path() {
    let root = Path::new("/my/project");
    let script = abp_kimi_sdk::sidecar_script(root);
    assert_eq!(script, root.join("hosts/kimi/host.js"));
}

#[test]
fn lib_sidecar_script_with_empty_root() {
    let root = Path::new("");
    let script = abp_kimi_sdk::sidecar_script(root);
    assert_eq!(script, Path::new("hosts/kimi/host.js"));
}

#[test]
fn lib_register_default_nonexistent_returns_false() {
    let mut runtime = abp_runtime::Runtime::new();
    let bogus = Path::new("/nonexistent/xyz123");
    let result = abp_kimi_sdk::register_default(&mut runtime, bogus, None).unwrap_or(false);
    assert!(!result);
}

#[test]
fn lib_register_backend_custom_name_nonexistent() {
    let mut runtime = abp_runtime::Runtime::new();
    let bogus = Path::new("/no/such/path");
    let result =
        abp_kimi_sdk::register_backend(&mut runtime, "custom:kimi", bogus, None).unwrap_or(false);
    assert!(!result);
}

// ============================================================================
// Module: types.rs – SearchOptions, SearchMode, ChatMessage, etc.
// ============================================================================

#[test]
fn types_search_mode_auto_serde() {
    let mode = types::SearchMode::Auto;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, r#""auto""#);
    let parsed: types::SearchMode = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, mode);
}

#[test]
fn types_search_mode_always_serde() {
    let mode = types::SearchMode::Always;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, r#""always""#);
    let parsed: types::SearchMode = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, mode);
}

#[test]
fn types_search_mode_never_serde() {
    let mode = types::SearchMode::Never;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, r#""never""#);
    let parsed: types::SearchMode = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, mode);
}

#[test]
fn types_search_options_serde_roundtrip() {
    let opts = types::SearchOptions {
        mode: types::SearchMode::Auto,
        result_count: Some(10),
    };
    let json = serde_json::to_string(&opts).unwrap();
    let parsed: types::SearchOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, opts);
}

#[test]
fn types_search_options_none_result_count_omitted() {
    let opts = types::SearchOptions {
        mode: types::SearchMode::Never,
        result_count: None,
    };
    let json = serde_json::to_string(&opts).unwrap();
    assert!(!json.contains("result_count"));
}

#[test]
fn types_chat_message_system_serde() {
    let msg = types::ChatMessage::System {
        content: "Be helpful.".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"system""#));
    let parsed: types::ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn types_chat_message_user_serde() {
    let msg = types::ChatMessage::User {
        content: "Hello".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"user""#));
    let parsed: types::ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn types_chat_message_assistant_text_serde() {
    let msg = types::ChatMessage::Assistant {
        content: Some("Reply".into()),
        tool_calls: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"assistant""#));
    let parsed: types::ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn types_chat_message_assistant_with_tool_calls() {
    let msg = types::ChatMessage::Assistant {
        content: None,
        tool_calls: Some(vec![types::ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: types::FunctionCall {
                name: "search".into(),
                arguments: "{}".into(),
            },
        }]),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: types::ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn types_chat_message_tool_serde() {
    let msg = types::ChatMessage::Tool {
        content: "result".into(),
        tool_call_id: "call_1".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"tool""#));
    let parsed: types::ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn types_kimi_chat_request_minimal_serde() {
    let req = types::KimiChatRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
        use_search: None,
        search_options: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("temperature"));
    assert!(!json.contains("use_search"));
    let parsed: types::KimiChatRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn types_kimi_chat_request_full_serde() {
    let req = types::KimiChatRequest {
        model: "moonshot-v1-32k".into(),
        messages: vec![types::ChatMessage::User {
            content: "test".into(),
        }],
        temperature: Some(0.7),
        top_p: Some(0.9),
        max_tokens: Some(4096),
        stream: Some(true),
        tools: Some(vec![types::Tool {
            tool_type: "function".into(),
            function: types::FunctionDef {
                name: "search".into(),
                description: "Search".into(),
                parameters: json!({"type": "object"}),
            },
        }]),
        tool_choice: Some(types::ToolChoice::Mode(types::ToolChoiceMode::Auto)),
        use_search: Some(true),
        search_options: Some(types::SearchOptions {
            mode: types::SearchMode::Always,
            result_count: Some(5),
        }),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: types::KimiChatRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn types_kimi_chat_response_serde() {
    let resp = types::KimiChatResponse {
        id: "resp_1".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![types::Choice {
            index: 0,
            message: types::ChoiceMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(types::KimiUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            search_tokens: None,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: types::KimiChatResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn types_kimi_usage_with_search_tokens() {
    let usage = types::KimiUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        search_tokens: Some(20),
    };
    let json = serde_json::to_string(&usage).unwrap();
    assert!(json.contains("search_tokens"));
    let parsed: types::KimiUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}

#[test]
fn types_kimi_usage_without_search_tokens() {
    let usage = types::KimiUsage {
        prompt_tokens: 50,
        completion_tokens: 25,
        total_tokens: 75,
        search_tokens: None,
    };
    let json = serde_json::to_string(&usage).unwrap();
    assert!(!json.contains("search_tokens"));
}

#[test]
fn types_tool_choice_mode_none() {
    let tc = types::ToolChoice::Mode(types::ToolChoiceMode::None);
    let json = serde_json::to_string(&tc).unwrap();
    assert_eq!(json, r#""none""#);
    let parsed: types::ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn types_tool_choice_mode_auto() {
    let tc = types::ToolChoice::Mode(types::ToolChoiceMode::Auto);
    let json = serde_json::to_string(&tc).unwrap();
    assert_eq!(json, r#""auto""#);
}

#[test]
fn types_tool_choice_mode_required() {
    let tc = types::ToolChoice::Mode(types::ToolChoiceMode::Required);
    let json = serde_json::to_string(&tc).unwrap();
    assert_eq!(json, r#""required""#);
}

#[test]
fn types_tool_choice_function_serde() {
    let tc = types::ToolChoice::Function {
        tool_type: "function".into(),
        function: types::ToolChoiceFunctionRef {
            name: "my_func".into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    assert!(json.contains("my_func"));
    let parsed: types::ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn types_stream_delta_default() {
    let delta = types::StreamDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

#[test]
fn types_kimi_stream_chunk_serde() {
    let chunk = types::KimiStreamChunk {
        id: "chunk_1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![types::StreamChoice {
            index: 0,
            delta: types::StreamDelta {
                role: Some("assistant".into()),
                content: Some("Hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: types::KimiStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, chunk);
}

#[test]
fn types_stream_tool_call_serde() {
    let stc = types::StreamToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(types::StreamFunctionCall {
            name: Some("search".into()),
            arguments: Some(r#"{"q":"rust"}"#.into()),
        }),
    };
    let json = serde_json::to_string(&stc).unwrap();
    let parsed: types::StreamToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, stc);
}

#[test]
fn types_stream_function_call_partial() {
    let sfc = types::StreamFunctionCall {
        name: None,
        arguments: Some(r#"{"partial"#.into()),
    };
    let json = serde_json::to_string(&sfc).unwrap();
    assert!(!json.contains("name"));
    let parsed: types::StreamFunctionCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, sfc);
}

#[test]
fn types_tool_serde_roundtrip() {
    let tool = types::Tool {
        tool_type: "function".into(),
        function: types::FunctionDef {
            name: "get_weather".into(),
            description: "Get weather info".into(),
            parameters: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let parsed: types::Tool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn types_function_call_serde() {
    let fc = types::FunctionCall {
        name: "do_thing".into(),
        arguments: r#"{"key":"value"}"#.into(),
    };
    let json = serde_json::to_string(&fc).unwrap();
    let parsed: types::FunctionCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, fc);
}

#[test]
fn types_tool_call_serde() {
    let tc = types::ToolCall {
        id: "call_99".into(),
        call_type: "function".into(),
        function: types::FunctionCall {
            name: "bash".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    assert!(json.contains(r#""type":"function""#));
    let parsed: types::ToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

// ============================================================================
// Module: convert.rs – to_work_order, from_receipt, search metadata helpers
// ============================================================================

fn sample_convert_request() -> types::KimiChatRequest {
    types::KimiChatRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![types::ChatMessage::User {
            content: "What is Rust?".into(),
        }],
        temperature: Some(0.5),
        top_p: Some(0.9),
        max_tokens: Some(4096),
        stream: Some(true),
        tools: None,
        tool_choice: None,
        use_search: Some(true),
        search_options: Some(types::SearchOptions {
            mode: types::SearchMode::Auto,
            result_count: Some(5),
        }),
    }
}

fn sample_receipt_for_convert(wo: &WorkOrder) -> Receipt {
    ReceiptBuilder::new("kimi")
        .work_order_id(wo.id)
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(200),
            output_tokens: Some(80),
            ..UsageNormalized::default()
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Rust is a systems programming language.".into(),
            },
            ext: None,
        })
        .build()
}

#[test]
fn convert_to_work_order_extracts_task() {
    let req = sample_convert_request();
    let wo = convert::to_work_order(&req);
    assert_eq!(wo.task, "What is Rust?");
}

#[test]
fn convert_to_work_order_sets_model() {
    let req = sample_convert_request();
    let wo = convert::to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
}

#[test]
fn convert_to_work_order_stores_use_search() {
    let req = sample_convert_request();
    let wo = convert::to_work_order(&req);
    let kimi = wo.config.vendor.get("kimi").unwrap();
    assert_eq!(kimi["use_search"], true);
}

#[test]
fn convert_to_work_order_stores_search_options() {
    let req = sample_convert_request();
    let wo = convert::to_work_order(&req);
    let kimi = wo.config.vendor.get("kimi").unwrap();
    assert!(kimi.get("search_options").is_some());
    assert_eq!(kimi["search_options"]["result_count"], 5);
}

#[test]
fn convert_to_work_order_stores_temperature() {
    let req = sample_convert_request();
    let wo = convert::to_work_order(&req);
    let kimi = wo.config.vendor.get("kimi").unwrap();
    let temp = kimi["temperature"].as_f64().unwrap();
    assert!((temp - 0.5).abs() < f64::EPSILON);
}

#[test]
fn convert_to_work_order_stores_top_p() {
    let req = sample_convert_request();
    let wo = convert::to_work_order(&req);
    let kimi = wo.config.vendor.get("kimi").unwrap();
    let top_p = kimi["top_p"].as_f64().unwrap();
    assert!((top_p - 0.9).abs() < f64::EPSILON);
}

#[test]
fn convert_to_work_order_stores_stream() {
    let req = sample_convert_request();
    let wo = convert::to_work_order(&req);
    let kimi = wo.config.vendor.get("kimi").unwrap();
    assert_eq!(kimi["stream"], true);
}

#[test]
fn convert_to_work_order_stores_dialect() {
    let req = sample_convert_request();
    let wo = convert::to_work_order(&req);
    let kimi = wo.config.vendor.get("kimi").unwrap();
    assert!(kimi.get("dialect").is_some());
}

#[test]
fn convert_to_work_order_no_user_message_fallback() {
    let req = types::KimiChatRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![types::ChatMessage::System {
            content: "system".into(),
        }],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
        use_search: None,
        search_options: None,
    };
    let wo = convert::to_work_order(&req);
    assert_eq!(wo.task, "(empty)");
}

#[test]
fn convert_to_work_order_multiple_user_messages_concatenated() {
    let req = types::KimiChatRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![
            types::ChatMessage::User {
                content: "First".into(),
            },
            types::ChatMessage::User {
                content: "Second".into(),
            },
        ],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
        use_search: None,
        search_options: None,
    };
    let wo = convert::to_work_order(&req);
    assert!(wo.task.contains("First"));
    assert!(wo.task.contains("Second"));
}

#[test]
fn convert_to_work_order_without_search_omits_keys() {
    let mut req = sample_convert_request();
    req.use_search = None;
    req.search_options = None;
    let wo = convert::to_work_order(&req);
    let kimi = wo.config.vendor.get("kimi").unwrap();
    assert!(kimi.get("use_search").is_none());
    assert!(kimi.get("search_options").is_none());
}

#[test]
fn convert_from_receipt_produces_valid_response() {
    let wo = WorkOrderBuilder::new("task")
        .model("moonshot-v1-8k")
        .build();
    let receipt = sample_receipt_for_convert(&wo);
    let resp = convert::from_receipt(&receipt, &wo);
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.model, "moonshot-v1-8k");
}

#[test]
fn convert_from_receipt_includes_assistant_text() {
    let wo = WorkOrderBuilder::new("task")
        .model("moonshot-v1-8k")
        .build();
    let receipt = sample_receipt_for_convert(&wo);
    let resp = convert::from_receipt(&receipt, &wo);
    let text = resp.choices[0].message.content.as_deref().unwrap();
    assert!(text.contains("Rust is a systems programming language"));
}

#[test]
fn convert_from_receipt_sets_stop_on_complete() {
    let wo = WorkOrderBuilder::new("task").build();
    let receipt = sample_receipt_for_convert(&wo);
    let resp = convert::from_receipt(&receipt, &wo);
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn convert_from_receipt_sets_length_on_partial() {
    let wo = WorkOrderBuilder::new("task").build();
    let receipt = ReceiptBuilder::new("kimi")
        .outcome(Outcome::Partial)
        .build();
    let resp = convert::from_receipt(&receipt, &wo);
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("length"));
}

#[test]
fn convert_from_receipt_includes_usage() {
    let wo = WorkOrderBuilder::new("task").build();
    let receipt = sample_receipt_for_convert(&wo);
    let resp = convert::from_receipt(&receipt, &wo);
    let usage = resp.usage.as_ref().unwrap();
    assert_eq!(usage.prompt_tokens, 200);
    assert_eq!(usage.completion_tokens, 80);
    assert_eq!(usage.total_tokens, 280);
    assert!(usage.search_tokens.is_none());
}

#[test]
fn convert_from_receipt_no_usage_when_zero() {
    let wo = WorkOrderBuilder::new("task").build();
    let receipt = ReceiptBuilder::new("kimi")
        .outcome(Outcome::Complete)
        .build();
    let resp = convert::from_receipt(&receipt, &wo);
    assert!(resp.usage.is_none());
}

#[test]
fn convert_from_receipt_default_model_fallback() {
    let wo = WorkOrderBuilder::new("task").build();
    let receipt = ReceiptBuilder::new("kimi")
        .outcome(Outcome::Complete)
        .build();
    let resp = convert::from_receipt(&receipt, &wo);
    assert_eq!(resp.model, "moonshot-v1-8k");
}

#[test]
fn convert_extract_search_metadata_present() {
    let req = sample_convert_request();
    let wo = convert::to_work_order(&req);
    let (use_search, opts) = convert::extract_search_metadata(&wo);
    assert_eq!(use_search, Some(true));
    assert!(opts.is_some());
    assert_eq!(opts.unwrap().result_count, Some(5));
}

#[test]
fn convert_extract_search_metadata_absent() {
    let wo = WorkOrderBuilder::new("task").build();
    let (use_search, opts) = convert::extract_search_metadata(&wo);
    assert!(use_search.is_none());
    assert!(opts.is_none());
}

#[test]
fn convert_build_search_vendor_config_with_values() {
    let opts = types::SearchOptions {
        mode: types::SearchMode::Always,
        result_count: Some(10),
    };
    let vendor = convert::build_search_vendor_config(Some(true), Some(&opts));
    let kimi = vendor.get("kimi").unwrap();
    assert_eq!(kimi["use_search"], true);
    let decoded: types::SearchOptions =
        serde_json::from_value(kimi["search_options"].clone()).unwrap();
    assert_eq!(decoded.mode, types::SearchMode::Always);
    assert_eq!(decoded.result_count, Some(10));
}

#[test]
fn convert_build_search_vendor_config_empty() {
    let vendor = convert::build_search_vendor_config(None, None);
    assert!(vendor.is_empty());
}

#[test]
fn convert_build_search_vendor_config_only_use_search() {
    let vendor = convert::build_search_vendor_config(Some(false), None);
    let kimi = vendor.get("kimi").unwrap();
    assert_eq!(kimi["use_search"], false);
    assert!(kimi.get("search_options").is_none());
}

// ============================================================================
// Module: dialect.rs – Config, model mapping, capabilities, tool translation
// ============================================================================

#[test]
fn dialect_version_constant() {
    assert_eq!(dialect::DIALECT_VERSION, "kimi/v0.1");
}

#[test]
fn dialect_default_model_constant() {
    assert_eq!(dialect::DEFAULT_MODEL, "moonshot-v1-8k");
}

#[test]
fn dialect_to_canonical_model() {
    assert_eq!(
        dialect::to_canonical_model("moonshot-v1-8k"),
        "moonshot/moonshot-v1-8k"
    );
}

#[test]
fn dialect_from_canonical_model_with_prefix() {
    assert_eq!(
        dialect::from_canonical_model("moonshot/moonshot-v1-128k"),
        "moonshot-v1-128k"
    );
}

#[test]
fn dialect_from_canonical_model_without_prefix() {
    assert_eq!(dialect::from_canonical_model("kimi-latest"), "kimi-latest");
}

#[test]
fn dialect_is_known_model_true() {
    assert!(dialect::is_known_model("moonshot-v1-8k"));
    assert!(dialect::is_known_model("moonshot-v1-32k"));
    assert!(dialect::is_known_model("moonshot-v1-128k"));
    assert!(dialect::is_known_model("kimi-latest"));
    assert!(dialect::is_known_model("k1"));
}

#[test]
fn dialect_is_known_model_false() {
    assert!(!dialect::is_known_model("gpt-4"));
    assert!(!dialect::is_known_model("unknown-model"));
}

#[test]
fn dialect_capability_manifest_has_streaming() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn dialect_capability_manifest_has_tool_read() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn dialect_capability_manifest_has_tool_write_emulated() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn dialect_capability_manifest_has_web_search_native() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolWebSearch),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn dialect_capability_manifest_tool_edit_unsupported() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolEdit),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn dialect_tool_def_to_kimi() {
    let canonical = dialect::CanonicalToolDef {
        name: "search".into(),
        description: "Search the web".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let kimi = dialect::tool_def_to_kimi(&canonical);
    assert_eq!(kimi.tool_type, "function");
    assert_eq!(kimi.function.name, "search");
    assert_eq!(kimi.function.description, "Search the web");
}

#[test]
fn dialect_tool_def_from_kimi() {
    let kimi = dialect::KimiToolDef {
        tool_type: "function".into(),
        function: dialect::KimiFunctionDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let canonical = dialect::tool_def_from_kimi(&kimi);
    assert_eq!(canonical.name, "read_file");
    assert_eq!(canonical.description, "Read a file");
}

#[test]
fn dialect_tool_def_roundtrip() {
    let original = dialect::CanonicalToolDef {
        name: "bash".into(),
        description: "Run a command".into(),
        parameters_schema: json!({"type": "object", "properties": {"cmd": {"type": "string"}}}),
    };
    let kimi = dialect::tool_def_to_kimi(&original);
    let back = dialect::tool_def_from_kimi(&kimi);
    assert_eq!(back, original);
}

#[test]
fn dialect_builtin_search_internet() {
    let builtin = dialect::builtin_search_internet();
    assert_eq!(builtin.tool_type, "builtin_function");
    assert_eq!(builtin.function.name, "$web_search");
}

#[test]
fn dialect_builtin_browser() {
    let builtin = dialect::builtin_browser();
    assert_eq!(builtin.tool_type, "builtin_function");
    assert_eq!(builtin.function.name, "$browser");
}

#[test]
fn dialect_kimi_ref_serde() {
    let r = dialect::KimiRef {
        index: 1,
        url: "https://example.com".into(),
        title: Some("Example".into()),
    };
    let json = serde_json::to_string(&r).unwrap();
    let parsed: dialect::KimiRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, r);
}

#[test]
fn dialect_kimi_ref_without_title() {
    let r = dialect::KimiRef {
        index: 2,
        url: "https://example.com".into(),
        title: None,
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("title"));
}

#[test]
fn dialect_kimi_config_default() {
    let cfg = dialect::KimiConfig::default();
    assert!(cfg.base_url.contains("moonshot.cn"));
    assert_eq!(cfg.model, "moonshot-v1-8k");
    assert_eq!(cfg.max_tokens, Some(4096));
    assert!(cfg.api_key.is_empty());
    assert!(cfg.temperature.is_none());
    assert!(cfg.use_k1_reasoning.is_none());
}

#[test]
fn dialect_kimi_role_display() {
    assert_eq!(dialect::KimiRole::System.to_string(), "system");
    assert_eq!(dialect::KimiRole::User.to_string(), "user");
    assert_eq!(dialect::KimiRole::Assistant.to_string(), "assistant");
    assert_eq!(dialect::KimiRole::Tool.to_string(), "tool");
}

#[test]
fn dialect_kimi_role_serde() {
    let role = dialect::KimiRole::Assistant;
    let json = serde_json::to_string(&role).unwrap();
    assert_eq!(json, r#""assistant""#);
    let parsed: dialect::KimiRole = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, role);
}

#[test]
fn dialect_kimi_tool_enum_function_serde() {
    let tool = dialect::KimiTool::Function {
        function: dialect::KimiFunctionDef {
            name: "test".into(),
            description: "desc".into(),
            parameters: json!({}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains(r#""type":"function""#));
    let parsed: dialect::KimiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn dialect_kimi_tool_enum_builtin_serde() {
    let tool = dialect::KimiTool::BuiltinFunction {
        function: dialect::KimiBuiltinFunction {
            name: "$web_search".into(),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains(r#""type":"builtin_function""#));
    let parsed: dialect::KimiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn dialect_map_work_order_basic() {
    let wo = WorkOrderBuilder::new("Optimize queries").build();
    let cfg = dialect::KimiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(
        req.messages[0]
            .content
            .as_deref()
            .unwrap()
            .contains("Optimize queries")
    );
}

#[test]
fn dialect_map_work_order_respects_model_override() {
    let wo = WorkOrderBuilder::new("task")
        .model("moonshot-v1-128k")
        .build();
    let cfg = dialect::KimiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "moonshot-v1-128k");
}

#[test]
fn dialect_map_work_order_uses_config_model_when_no_override() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = dialect::KimiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "moonshot-v1-8k");
}

#[test]
fn dialect_map_response_produces_assistant_message() {
    let resp = dialect::KimiResponse {
        id: "cmpl_1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![dialect::KimiChoice {
            index: 0,
            message: dialect::KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Answer here.".into()),
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
            assert_eq!(text, "Answer here.");
        }
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn dialect_map_response_with_tool_calls() {
    let resp = dialect::KimiResponse {
        id: "cmpl_2".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![dialect::KimiChoice {
            index: 0,
            message: dialect::KimiResponseMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![dialect::KimiToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: dialect::KimiFunctionCall {
                        name: "search".into(),
                        arguments: r#"{"q":"test"}"#.into(),
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
            assert_eq!(tool_name, "search");
            assert_eq!(tool_use_id.as_deref(), Some("call_1"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn dialect_map_response_with_refs() {
    let resp = dialect::KimiResponse {
        id: "cmpl_3".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![dialect::KimiChoice {
            index: 0,
            message: dialect::KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Text with refs".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: Some(vec![dialect::KimiRef {
            index: 1,
            url: "https://example.com".into(),
            title: Some("Example".into()),
        }]),
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(events[0].ext.is_some());
    let ext = events[0].ext.as_ref().unwrap();
    assert!(ext.contains_key("kimi_refs"));
}

#[test]
fn dialect_map_stream_event_text_delta() {
    let chunk = dialect::KimiChunk {
        id: "chunk_1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![dialect::KimiChunkChoice {
            index: 0,
            delta: dialect::KimiChunkDelta {
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
fn dialect_map_stream_event_finish_reason() {
    let chunk = dialect::KimiChunk {
        id: "chunk_2".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![dialect::KimiChunkChoice {
            index: 0,
            delta: dialect::KimiChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = dialect::map_stream_event(&chunk);
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
    );
}

#[test]
fn dialect_map_stream_event_empty_content_no_event() {
    let chunk = dialect::KimiChunk {
        id: "chunk_3".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![dialect::KimiChunkChoice {
            index: 0,
            delta: dialect::KimiChunkDelta {
                role: Some("assistant".into()),
                content: Some("".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };
    let events = dialect::map_stream_event(&chunk);
    assert!(events.is_empty());
}

#[test]
fn dialect_tool_call_accumulator_basic() {
    let mut acc = dialect::ToolCallAccumulator::new();
    acc.feed(&[dialect::KimiChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(dialect::KimiChunkFunctionCall {
            name: Some("search".into()),
            arguments: Some(r#"{"q":"#.into()),
        }),
    }]);
    acc.feed(&[dialect::KimiChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(dialect::KimiChunkFunctionCall {
            name: None,
            arguments: Some(r#""rust"}"#.into()),
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
fn dialect_tool_call_accumulator_empty() {
    let acc = dialect::ToolCallAccumulator::new();
    let events = acc.finish();
    assert!(events.is_empty());
}

#[test]
fn dialect_tool_call_accumulator_multiple_tools() {
    let mut acc = dialect::ToolCallAccumulator::new();
    acc.feed(&[
        dialect::KimiChunkToolCall {
            index: 0,
            id: Some("c1".into()),
            call_type: Some("function".into()),
            function: Some(dialect::KimiChunkFunctionCall {
                name: Some("a".into()),
                arguments: Some("{}".into()),
            }),
        },
        dialect::KimiChunkToolCall {
            index: 1,
            id: Some("c2".into()),
            call_type: Some("function".into()),
            function: Some(dialect::KimiChunkFunctionCall {
                name: Some("b".into()),
                arguments: Some("{}".into()),
            }),
        },
    ]);
    let events = acc.finish();
    assert_eq!(events.len(), 2);
}

#[test]
fn dialect_extract_usage() {
    let resp = dialect::KimiResponse {
        id: "r1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: Some(dialect::KimiUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        }),
        refs: None,
    };
    let usage_map = dialect::extract_usage(&resp).unwrap();
    assert_eq!(usage_map["prompt_tokens"], json!(100));
    assert_eq!(usage_map["completion_tokens"], json!(50));
    assert_eq!(usage_map["total_tokens"], json!(150));
}

#[test]
fn dialect_extract_usage_none() {
    let resp = dialect::KimiResponse {
        id: "r2".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: None,
        refs: None,
    };
    assert!(dialect::extract_usage(&resp).is_none());
}

// ============================================================================
// Module: api.rs – KimiChatRequest, Response, From impls, tool helpers
// ============================================================================

#[test]
fn api_message_system_serde() {
    let msg = api::KimiMessage::System {
        content: "You are a helper.".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"system""#));
    let parsed: api::KimiMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn api_message_user_serde() {
    let msg = api::KimiMessage::User {
        content: "Hi".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: api::KimiMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn api_message_tool_serde() {
    let msg = api::KimiMessage::Tool {
        tool_call_id: "call_1".into(),
        content: "output".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: api::KimiMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn api_finish_reason_variants_serde() {
    for (reason, expected) in [
        (api::KimiFinishReason::Stop, "\"stop\""),
        (api::KimiFinishReason::Length, "\"length\""),
        (api::KimiFinishReason::ToolCalls, "\"tool_calls\""),
        (api::KimiFinishReason::ContentFilter, "\"content_filter\""),
    ] {
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, expected);
    }
}

#[test]
fn api_request_to_work_order_via_from() {
    let req = api::KimiChatRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![api::KimiMessage::User {
            content: "Hello world".into(),
        }],
        temperature: None,
        top_p: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        n: None,
        stop: None,
        presence_penalty: None,
        frequency_penalty: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Hello world");
    assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
}

#[test]
fn api_request_to_work_order_system_as_snippets() {
    let req = api::KimiChatRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![
            api::KimiMessage::System {
                content: "Be concise.".into(),
            },
            api::KimiMessage::User {
                content: "Hi".into(),
            },
        ],
        temperature: None,
        top_p: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        n: None,
        stop: None,
        presence_penalty: None,
        frequency_penalty: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].content, "Be concise.");
}

#[test]
fn api_request_empty_messages_empty_task() {
    let req = api::KimiChatRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![],
        temperature: None,
        top_p: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        n: None,
        stop: None,
        presence_penalty: None,
        frequency_penalty: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "");
}

fn make_api_receipt(trace: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::new_v4(),
            contract_version: "abp/v0.1".into(),
            started_at: now,
            finished_at: now,
            duration_ms: 100,
        },
        backend: BackendIdentity {
            id: "moonshot/moonshot-v1-8k".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage,
        trace,
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

#[test]
fn api_receipt_to_response_via_from() {
    let trace = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        },
        ext: None,
    }];
    let receipt = make_api_receipt(trace, UsageNormalized::default());
    let resp: api::KimiChatResponse = receipt.into();
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    assert_eq!(resp.choices[0].finish_reason, api::KimiFinishReason::Stop);
}

#[test]
fn api_receipt_to_response_with_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..UsageNormalized::default()
    };
    let receipt = make_api_receipt(vec![], usage);
    let resp: api::KimiChatResponse = receipt.into();
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

#[test]
fn api_receipt_to_response_no_usage_when_none() {
    let receipt = make_api_receipt(vec![], UsageNormalized::default());
    let resp: api::KimiChatResponse = receipt.into();
    assert!(resp.usage.is_none());
}

#[test]
fn api_tool_calls_to_events() {
    let tcs = vec![api::KimiToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: api::KimiFunctionCall {
            name: "read_file".into(),
            arguments: r#"{"path":"a.rs"}"#.into(),
        },
    }];
    let events = api::tool_calls_to_events(&tcs);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            ..
        } => {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("call_1"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn api_tool_calls_to_events_malformed_args() {
    let tcs = vec![api::KimiToolCall {
        id: "call_bad".into(),
        call_type: "function".into(),
        function: api::KimiFunctionCall {
            name: "foo".into(),
            arguments: "not-json".into(),
        },
    }];
    let events = api::tool_calls_to_events(&tcs);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-json".into()));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn api_event_to_tool_call_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("call_x".into()),
            parent_tool_use_id: None,
            input: json!({"cmd": "ls"}),
        },
        ext: None,
    };
    let tc = api::event_to_tool_call(&event).unwrap();
    assert_eq!(tc.id, "call_x");
    assert_eq!(tc.function.name, "bash");
}

#[test]
fn api_event_to_tool_call_non_tool_returns_none() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    assert!(api::event_to_tool_call(&event).is_none());
}

#[test]
fn api_event_to_tool_call_no_id_uses_default() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "test".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        ext: None,
    };
    let tc = api::event_to_tool_call(&event).unwrap();
    assert_eq!(tc.id, "call_0");
}

#[test]
fn api_stream_chunk_serde() {
    let chunk = api::KimiStreamChunk {
        id: "sc_1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![api::KimiStreamChoice {
            index: 0,
            delta: api::KimiDelta {
                role: Some("assistant".into()),
                content: Some("text".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: api::KimiStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, chunk);
}

#[test]
fn api_delta_default_all_none() {
    let delta = api::KimiDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

// ============================================================================
// Module: lowering.rs – IR conversion roundtrips
// ============================================================================

#[test]
fn lowering_user_text_roundtrip() {
    let msgs = vec![dialect::KimiMessage {
        role: "user".into(),
        content: Some("Hello".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello");
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content.as_deref(), Some("Hello"));
}

#[test]
fn lowering_system_text_roundtrip() {
    let msgs = vec![dialect::KimiMessage {
        role: "system".into(),
        content: Some("Be helpful.".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::System);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "system");
}

#[test]
fn lowering_assistant_text_roundtrip() {
    let msgs = vec![dialect::KimiMessage {
        role: "assistant".into(),
        content: Some("Sure!".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("Sure!"));
}

#[test]
fn lowering_tool_call_to_ir() {
    let msgs = vec![dialect::KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![dialect::KimiToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: dialect::KimiFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"rust"}"#.into(),
            },
        }]),
    }];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "search");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn lowering_tool_result_roundtrip() {
    let msgs = vec![dialect::KimiMessage {
        role: "tool".into(),
        content: Some("result data".into()),
        tool_call_id: Some("call_1".into()),
        tool_calls: None,
    }];
    let conv = lowering::to_ir(&msgs);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "tool");
    assert_eq!(back[0].content.as_deref(), Some("result data"));
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn lowering_empty_messages() {
    let conv = lowering::to_ir(&[]);
    assert!(conv.is_empty());
    let back = lowering::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn lowering_unknown_role_defaults_to_user() {
    let msgs = vec![dialect::KimiMessage {
        role: "developer".into(),
        content: Some("hi".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn lowering_usage_to_ir() {
    let usage = dialect::KimiUsage {
        prompt_tokens: 200,
        completion_tokens: 80,
        total_tokens: 280,
    };
    let ir = lowering::usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 200);
    assert_eq!(ir.output_tokens, 80);
    assert_eq!(ir.total_tokens, 280);
}

#[test]
fn lowering_multi_turn_conversation() {
    let msgs = vec![
        dialect::KimiMessage {
            role: "system".into(),
            content: Some("Concise.".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        dialect::KimiMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        dialect::KimiMessage {
            role: "assistant".into(),
            content: Some("Hello!".into()),
            tool_call_id: None,
            tool_calls: None,
        },
    ];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 3);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[2].content.as_deref(), Some("Hello!"));
}

#[test]
fn lowering_none_content_produces_empty_blocks() {
    let msgs = vec![dialect::KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn lowering_malformed_tool_arguments_kept_as_string() {
    let msgs = vec![dialect::KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![dialect::KimiToolCall {
            id: "call_bad".into(),
            call_type: "function".into(),
            function: dialect::KimiFunctionCall {
                name: "foo".into(),
                arguments: "not-json".into(),
            },
        }]),
    }];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-json".into()));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}
