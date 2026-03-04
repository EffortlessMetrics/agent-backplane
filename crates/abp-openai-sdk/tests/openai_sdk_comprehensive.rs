#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

//! Comprehensive test suite for the `abp-openai-sdk` crate.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest, ContextPacket,
    ContextSnippet, ExecutionMode, Outcome, Receipt, RunMetadata, SupportLevel, UsageNormalized,
    VerificationReport, WorkOrder, WorkOrderBuilder,
    ir::{IrContentBlock, IrConversation, IrMessage, IrRole},
};
use abp_openai_sdk::api::{
    AssistantMessage, ChatCompletionRequest, ChatCompletionResponse, Choice, Delta, FinishReason,
    FunctionCall, FunctionDefinition, Message, StreamChoice, StreamChunk, Tool, ToolCall, Usage,
};
use abp_openai_sdk::dialect::{
    self, CanonicalToolDef, OpenAIChoice, OpenAIConfig, OpenAIFunctionCall, OpenAIFunctionDef,
    OpenAIMessage, OpenAIRequest, OpenAIResponse, OpenAIToolCall, OpenAIToolDef, OpenAIUsage,
    ToolChoice, ToolChoiceFunctionRef, ToolChoiceMode,
};
use abp_openai_sdk::lowering;
use abp_openai_sdk::response_format::{JsonSchemaSpec, ResponseFormat};
use abp_openai_sdk::streaming::{
    ChatCompletionChunk, ChunkChoice, ChunkDelta, ChunkFunctionCall, ChunkToolCall, ChunkUsage,
    ToolCallAccumulator,
};
use abp_openai_sdk::validation::{ExtendedRequestFields, UnmappableParam, ValidationErrors};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Helper functions
// ═══════════════════════════════════════════════════════════════════════════

fn make_api_request(messages: Vec<Message>) -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages,
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        stream_options: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
        parallel_tool_calls: None,
        service_tier: None,
    }
}

fn make_receipt(trace: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
    let now = Utc::now();
    let run_id = Uuid::new_v4();
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::new_v4(),
            contract_version: "abp/v0.1".into(),
            started_at: now,
            finished_at: now,
            duration_ms: 100,
        },
        backend: BackendIdentity {
            id: "openai/gpt-4o".into(),
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

fn make_openai_msg(role: &str, content: Option<&str>) -> OpenAIMessage {
    OpenAIMessage {
        role: role.into(),
        content: content.map(|s| s.into()),
        tool_calls: None,
        tool_call_id: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Module constants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lib_backend_name_constant() {
    assert_eq!(abp_openai_sdk::BACKEND_NAME, "sidecar:openai");
}

#[test]
fn lib_host_script_relative_constant() {
    assert_eq!(abp_openai_sdk::HOST_SCRIPT_RELATIVE, "hosts/openai/host.js");
}

#[test]
fn lib_default_node_command_constant() {
    assert_eq!(abp_openai_sdk::DEFAULT_NODE_COMMAND, "node");
}

#[test]
fn dialect_version_constant() {
    assert_eq!(dialect::DIALECT_VERSION, "openai/v0.1");
}

#[test]
fn dialect_default_model_constant() {
    assert_eq!(dialect::DEFAULT_MODEL, "gpt-4o");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Sidecar script path
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_script_joins_root_with_relative_path() {
    let root = std::path::Path::new("/workspace");
    let script = abp_openai_sdk::sidecar_script(root);
    assert_eq!(script, root.join("hosts/openai/host.js"));
}

#[test]
fn sidecar_script_with_empty_root() {
    let root = std::path::Path::new("");
    let script = abp_openai_sdk::sidecar_script(root);
    assert_eq!(script, std::path::PathBuf::from("hosts/openai/host.js"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Model mapping (dialect)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn to_canonical_model_prepends_openai_prefix() {
    assert_eq!(dialect::to_canonical_model("gpt-4o"), "openai/gpt-4o");
}

#[test]
fn to_canonical_model_with_arbitrary_name() {
    assert_eq!(
        dialect::to_canonical_model("custom-model"),
        "openai/custom-model"
    );
}

#[test]
fn from_canonical_model_strips_openai_prefix() {
    assert_eq!(dialect::from_canonical_model("openai/gpt-4o"), "gpt-4o");
}

#[test]
fn from_canonical_model_no_prefix_returns_unchanged() {
    assert_eq!(dialect::from_canonical_model("gpt-4o"), "gpt-4o");
}

#[test]
fn from_canonical_model_different_prefix_unchanged() {
    assert_eq!(
        dialect::from_canonical_model("anthropic/claude"),
        "anthropic/claude"
    );
}

#[test]
fn is_known_model_returns_true_for_known() {
    assert!(dialect::is_known_model("gpt-4o"));
    assert!(dialect::is_known_model("gpt-4o-mini"));
    assert!(dialect::is_known_model("gpt-4-turbo"));
    assert!(dialect::is_known_model("o1"));
    assert!(dialect::is_known_model("o1-mini"));
    assert!(dialect::is_known_model("o3-mini"));
    assert!(dialect::is_known_model("gpt-4.1"));
}

#[test]
fn is_known_model_returns_false_for_unknown() {
    assert!(!dialect::is_known_model("gpt-5"));
    assert!(!dialect::is_known_model("claude-3"));
    assert!(!dialect::is_known_model(""));
}

#[test]
fn canonical_model_roundtrip() {
    let model = "gpt-4o";
    let canonical = dialect::to_canonical_model(model);
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, model);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Capability manifest (dialect)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_manifest_has_streaming_native() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_structured_output_native() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_tools_emulated() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
    assert!(matches!(
        m.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
    assert!(matches!(
        m.get(&Capability::ToolEdit),
        Some(SupportLevel::Emulated)
    ));
    assert!(matches!(
        m.get(&Capability::ToolBash),
        Some(SupportLevel::Emulated)
    ));
    assert!(matches!(
        m.get(&Capability::ToolGlob),
        Some(SupportLevel::Emulated)
    ));
    assert!(matches!(
        m.get(&Capability::ToolGrep),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn capability_manifest_mcp_unsupported() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpClient),
        Some(SupportLevel::Unsupported)
    ));
    assert!(matches!(
        m.get(&Capability::McpServer),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn capability_manifest_hooks_emulated() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::HooksPreToolUse),
        Some(SupportLevel::Emulated)
    ));
    assert!(matches!(
        m.get(&Capability::HooksPostToolUse),
        Some(SupportLevel::Emulated)
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Tool definition conversions (dialect)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_def_to_openai_sets_function_type() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let openai = dialect::tool_def_to_openai(&canonical);
    assert_eq!(openai.tool_type, "function");
    assert_eq!(openai.function.name, "read_file");
    assert_eq!(openai.function.description, "Read a file");
    assert_eq!(openai.function.parameters, json!({"type": "object"}));
}

#[test]
fn tool_def_from_openai_extracts_fields() {
    let openai = OpenAIToolDef {
        tool_type: "function".into(),
        function: OpenAIFunctionDef {
            name: "write_file".into(),
            description: "Write a file".into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
    };
    let canonical = dialect::tool_def_from_openai(&openai);
    assert_eq!(canonical.name, "write_file");
    assert_eq!(canonical.description, "Write a file");
    assert_eq!(
        canonical.parameters_schema,
        json!({"type": "object", "properties": {}})
    );
}

#[test]
fn tool_def_roundtrip_canonical_to_openai_and_back() {
    let original = CanonicalToolDef {
        name: "bash".into(),
        description: "Run shell commands".into(),
        parameters_schema: json!({"type": "object", "properties": {"cmd": {"type": "string"}}}),
    };
    let openai = dialect::tool_def_to_openai(&original);
    let back = dialect::tool_def_from_openai(&openai);
    assert_eq!(back, original);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. ToolChoice serde (dialect)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_choice_mode_none_serde() {
    let choice = ToolChoice::Mode(ToolChoiceMode::None);
    let json = serde_json::to_string(&choice).unwrap();
    assert_eq!(json, r#""none""#);
    let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, choice);
}

#[test]
fn tool_choice_mode_auto_serde() {
    let choice = ToolChoice::Mode(ToolChoiceMode::Auto);
    let json = serde_json::to_string(&choice).unwrap();
    assert_eq!(json, r#""auto""#);
    let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, choice);
}

#[test]
fn tool_choice_mode_required_serde() {
    let choice = ToolChoice::Mode(ToolChoiceMode::Required);
    let json = serde_json::to_string(&choice).unwrap();
    assert_eq!(json, r#""required""#);
    let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, choice);
}

#[test]
fn tool_choice_function_serde() {
    let choice = ToolChoice::Function {
        tool_type: "function".into(),
        function: ToolChoiceFunctionRef {
            name: "read_file".into(),
        },
    };
    let json = serde_json::to_string(&choice).unwrap();
    assert!(json.contains(r#""type":"function""#));
    assert!(json.contains(r#""name":"read_file""#));
    let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, choice);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. OpenAIConfig (dialect)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_config_default_values() {
    let cfg = OpenAIConfig::default();
    assert_eq!(cfg.api_key, "");
    assert!(cfg.base_url.contains("openai.com"));
    assert_eq!(cfg.model, "gpt-4o");
    assert_eq!(cfg.max_tokens, Some(4096));
    assert!(cfg.temperature.is_none());
}

#[test]
fn openai_config_serde_roundtrip() {
    let cfg = OpenAIConfig {
        api_key: "sk-test".into(),
        base_url: "https://api.openai.com/v1".into(),
        model: "gpt-4-turbo".into(),
        max_tokens: Some(2048),
        temperature: Some(0.7),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: OpenAIConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.api_key, cfg.api_key);
    assert_eq!(parsed.model, cfg.model);
    assert_eq!(parsed.max_tokens, cfg.max_tokens);
    assert_eq!(parsed.temperature, cfg.temperature);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. OpenAIMessage serde (dialect)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_message_user_serde_roundtrip() {
    let msg = make_openai_msg("user", Some("Hello"));
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: OpenAIMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, "user");
    assert_eq!(parsed.content.as_deref(), Some("Hello"));
}

#[test]
fn openai_message_system_serde_roundtrip() {
    let msg = make_openai_msg("system", Some("Be helpful"));
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: OpenAIMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, "system");
}

#[test]
fn openai_message_assistant_with_tool_calls_serde() {
    let msg = OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"main.rs"}"#.into(),
            },
        }]),
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("tool_calls"));
    assert!(json.contains("read_file"));
}

#[test]
fn openai_message_tool_result_serde() {
    let msg = OpenAIMessage {
        role: "tool".into(),
        content: Some("result data".into()),
        tool_calls: None,
        tool_call_id: Some("call_1".into()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: OpenAIMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn openai_message_skips_none_fields_in_serialization() {
    let msg = make_openai_msg("user", Some("Hi"));
    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains("tool_calls"));
    assert!(!json.contains("tool_call_id"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. OpenAIRequest serde (dialect)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_request_minimal_serde_roundtrip() {
    let req = OpenAIRequest {
        model: "gpt-4o".into(),
        messages: vec![make_openai_msg("user", Some("Hello"))],
        tools: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: OpenAIRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "gpt-4o");
    assert_eq!(parsed.messages.len(), 1);
}

#[test]
fn openai_request_omits_none_fields() {
    let req = OpenAIRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        tools: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("tools"));
    assert!(!json.contains("tool_choice"));
    assert!(!json.contains("temperature"));
    assert!(!json.contains("max_tokens"));
    assert!(!json.contains("response_format"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. OpenAIResponse & OpenAIChoice serde (dialect)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_response_serde_roundtrip() {
    let resp = OpenAIResponse {
        id: "chatcmpl-123".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: make_openai_msg("assistant", Some("Hi!")),
            finish_reason: Some("stop".into()),
        }],
        usage: Some(OpenAIUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: OpenAIResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "chatcmpl-123");
    assert_eq!(parsed.choices.len(), 1);
}

#[test]
fn openai_response_no_usage_field() {
    let resp = OpenAIResponse {
        id: "chatcmpl-456".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(!json.contains("usage"));
}

#[test]
fn openai_usage_serde_roundtrip() {
    let usage = OpenAIUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: OpenAIUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.prompt_tokens, 100);
    assert_eq!(parsed.total_tokens, 150);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. map_work_order (dialect)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_work_order_creates_user_message() {
    let wo = WorkOrderBuilder::new("Fix the bug").build();
    let cfg = OpenAIConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(
        req.messages[0]
            .content
            .as_deref()
            .unwrap()
            .contains("Fix the bug")
    );
}

#[test]
fn map_work_order_uses_config_model_when_no_override() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = OpenAIConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4o");
}

#[test]
fn map_work_order_uses_work_order_model_override() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4-turbo").build();
    let cfg = OpenAIConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4-turbo");
}

#[test]
fn map_work_order_includes_snippets_in_content() {
    let wo = WorkOrderBuilder::new("task")
        .context(ContextPacket {
            files: vec![],
            snippets: vec![ContextSnippet {
                name: "readme".into(),
                content: "# Hello".into(),
            }],
        })
        .build();
    let cfg = OpenAIConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    let content = req.messages[0].content.as_deref().unwrap();
    assert!(content.contains("readme"));
    assert!(content.contains("# Hello"));
}

#[test]
fn map_work_order_propagates_temperature() {
    let wo = WorkOrderBuilder::new("task").build();
    let mut cfg = OpenAIConfig::default();
    cfg.temperature = Some(0.5);
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.temperature, Some(0.5));
}

#[test]
fn map_work_order_propagates_max_tokens() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = OpenAIConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.max_tokens, Some(4096));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. map_response (dialect)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_response_emits_assistant_message_event() {
    let resp = OpenAIResponse {
        id: "chatcmpl-1".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: make_openai_msg("assistant", Some("Hello!")),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hello!"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn map_response_emits_tool_call_events() {
    let resp = OpenAIResponse {
        id: "chatcmpl-2".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "ls".into(),
                        arguments: "{}".into(),
                    },
                }]),
                tool_call_id: None,
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { tool_name, .. } => assert_eq!(tool_name, "ls"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn map_response_skips_empty_content() {
    let resp = OpenAIResponse {
        id: "x".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: make_openai_msg("assistant", Some("")),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_empty_choices() {
    let resp = OpenAIResponse {
        id: "x".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_multiple_choices() {
    let resp = OpenAIResponse {
        id: "x".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![
            OpenAIChoice {
                index: 0,
                message: make_openai_msg("assistant", Some("A")),
                finish_reason: Some("stop".into()),
            },
            OpenAIChoice {
                index: 1,
                message: make_openai_msg("assistant", Some("B")),
                finish_reason: Some("stop".into()),
            },
        ],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 2);
}

#[test]
fn map_response_tool_call_with_malformed_arguments() {
    let resp = OpenAIResponse {
        id: "x".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "call_bad".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "foo".into(),
                        arguments: "not-json".into(),
                    },
                }]),
                tool_call_id: None,
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-json".into()));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. ResponseFormat
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_format_text_serde() {
    let rf = ResponseFormat::text();
    let json = serde_json::to_string(&rf).unwrap();
    assert!(json.contains(r#""type":"text""#));
    let parsed: ResponseFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, rf);
}

#[test]
fn response_format_json_object_serde() {
    let rf = ResponseFormat::json_object();
    let json = serde_json::to_string(&rf).unwrap();
    assert!(json.contains(r#""type":"json_object""#));
    let parsed: ResponseFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, rf);
}

#[test]
fn response_format_json_schema_serde() {
    let schema = json!({
        "type": "object",
        "properties": {
            "answer": {"type": "string"}
        },
        "required": ["answer"]
    });
    let rf = ResponseFormat::json_schema("my_schema", schema.clone());
    let json_str = serde_json::to_string(&rf).unwrap();
    assert!(json_str.contains(r#""type":"json_schema""#));
    assert!(json_str.contains("my_schema"));
    let parsed: ResponseFormat = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed, rf);
}

#[test]
fn response_format_json_schema_has_strict_true_by_default() {
    let rf = ResponseFormat::json_schema("test", json!({}));
    match rf {
        ResponseFormat::JsonSchema { json_schema } => {
            assert_eq!(json_schema.strict, Some(true));
            assert!(json_schema.description.is_none());
        }
        _ => panic!("expected JsonSchema variant"),
    }
}

#[test]
fn json_schema_spec_serde_roundtrip() {
    let spec = JsonSchemaSpec {
        name: "output".into(),
        description: Some("The output format".into()),
        schema: json!({"type": "object"}),
        strict: Some(false),
    };
    let json = serde_json::to_string(&spec).unwrap();
    let parsed: JsonSchemaSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, spec);
}

#[test]
fn json_schema_spec_omits_none_description() {
    let spec = JsonSchemaSpec {
        name: "x".into(),
        description: None,
        schema: json!({}),
        strict: None,
    };
    let json = serde_json::to_string(&spec).unwrap();
    assert!(!json.contains("description"));
    assert!(!json.contains("strict"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Lowering: to_ir and from_ir
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lowering_user_text_roundtrip() {
    let msgs = vec![make_openai_msg("user", Some("Hello"))];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello");
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content.as_deref(), Some("Hello"));
}

#[test]
fn lowering_system_text_roundtrip() {
    let msgs = vec![make_openai_msg("system", Some("Be helpful"))];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::System);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "system");
}

#[test]
fn lowering_assistant_text_roundtrip() {
    let msgs = vec![make_openai_msg("assistant", Some("OK"))];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "assistant");
    assert_eq!(back[0].content.as_deref(), Some("OK"));
}

#[test]
fn lowering_tool_call_to_ir() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "bash".into(),
                arguments: r#"{"cmd":"ls"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "c1");
            assert_eq!(name, "bash");
            assert_eq!(input, &json!({"cmd": "ls"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn lowering_tool_call_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "c42".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"test"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "assistant");
    assert!(back[0].content.is_none());
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "c42");
    assert_eq!(tc.function.name, "search");
}

#[test]
fn lowering_tool_result_to_ir() {
    let msgs = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("result text".into()),
        tool_calls: None,
        tool_call_id: Some("c1".into()),
    }];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::Tool);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "c1");
            assert!(!is_error);
            assert_eq!(content.len(), 1);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn lowering_tool_result_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("ok".into()),
        tool_calls: None,
        tool_call_id: Some("c99".into()),
    }];
    let conv = lowering::to_ir(&msgs);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "tool");
    assert_eq!(back[0].content.as_deref(), Some("ok"));
    assert_eq!(back[0].tool_call_id.as_deref(), Some("c99"));
}

#[test]
fn lowering_empty_messages() {
    let conv = lowering::to_ir(&[]);
    assert!(conv.is_empty());
    let back = lowering::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn lowering_empty_content_string() {
    let msgs = vec![make_openai_msg("user", Some(""))];
    let conv = lowering::to_ir(&msgs);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn lowering_none_content() {
    let msgs = vec![make_openai_msg("assistant", None)];
    let conv = lowering::to_ir(&msgs);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn lowering_unknown_role_defaults_to_user() {
    let msgs = vec![make_openai_msg("developer", Some("hi"))];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn lowering_multi_turn_conversation() {
    let msgs = vec![
        make_openai_msg("system", Some("Be concise")),
        make_openai_msg("user", Some("Hi")),
        make_openai_msg("assistant", Some("Hello!")),
        make_openai_msg("user", Some("Bye")),
    ];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 4);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[3].role, IrRole::User);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 4);
    assert_eq!(back[3].content.as_deref(), Some("Bye"));
}

#[test]
fn lowering_assistant_text_and_tool_call_combined() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: Some("Let me check.".into()),
        tool_calls: Some(vec![OpenAIToolCall {
            id: "c7".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "ls".into(),
                arguments: "{}".into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("Let me check."));
    assert!(back[0].tool_calls.is_some());
}

#[test]
fn lowering_multiple_tool_calls_in_one_message() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![
            OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "a".into(),
                    arguments: "{}".into(),
                },
            },
            OpenAIToolCall {
                id: "c2".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "b".into(),
                    arguments: "{}".into(),
                },
            },
        ]),
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 2);
}

#[test]
fn lowering_malformed_tool_arguments_kept_as_string() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_bad".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "foo".into(),
                arguments: "not-json".into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-json".into()));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn lowering_tool_result_without_content() {
    let msgs = vec![OpenAIMessage {
        role: "tool".into(),
        content: None,
        tool_calls: None,
        tool_call_id: Some("c1".into()),
    }];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => {
            assert!(content.is_empty());
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn lowering_system_message_accessor() {
    let msgs = vec![
        make_openai_msg("system", Some("instructions")),
        make_openai_msg("user", Some("hi")),
    ];
    let conv = lowering::to_ir(&msgs);
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.text_content(), "instructions");
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Streaming types serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn chat_completion_chunk_serde_roundtrip() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-stream-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: Some("assistant".into()),
                content: Some("Hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, chunk);
}

#[test]
fn chunk_delta_default_all_none() {
    let d = ChunkDelta::default();
    assert!(d.role.is_none());
    assert!(d.content.is_none());
    assert!(d.tool_calls.is_none());
}

#[test]
fn chunk_delta_serde_skips_none_fields() {
    let d = ChunkDelta::default();
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(json, "{}");
}

#[test]
fn chunk_usage_serde_roundtrip() {
    let u = ChunkUsage {
        prompt_tokens: 50,
        completion_tokens: 25,
        total_tokens: 75,
    };
    let json = serde_json::to_string(&u).unwrap();
    let parsed: ChunkUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, u);
}

#[test]
fn chunk_tool_call_serde_roundtrip() {
    let tc = ChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("read_file".into()),
            arguments: Some(r#"{"path":"#.into()),
        }),
    };
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: ChunkToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn chunk_tool_call_minimal_serde() {
    let tc = ChunkToolCall {
        index: 1,
        id: None,
        call_type: None,
        function: Some(ChunkFunctionCall {
            name: None,
            arguments: Some(r#"main.rs"}"#.into()),
        }),
    };
    let json = serde_json::to_string(&tc).unwrap();
    assert!(!json.contains("\"id\""));
    assert!(!json.contains("\"type\""));
}

#[test]
fn chunk_function_call_serde_roundtrip() {
    let fc = ChunkFunctionCall {
        name: Some("test".into()),
        arguments: Some("{}".into()),
    };
    let json = serde_json::to_string(&fc).unwrap();
    let parsed: ChunkFunctionCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, fc);
}

#[test]
fn chat_completion_chunk_with_final_usage() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-end".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000001,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: Some(ChunkUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        }),
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.usage.unwrap().total_tokens, 150);
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. map_chunk (streaming)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_chunk_emits_assistant_delta() {
    let chunk = ChatCompletionChunk {
        id: "x".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: Some("Hello".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let events = abp_openai_sdk::streaming::map_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn map_chunk_skips_empty_content() {
    let chunk = ChatCompletionChunk {
        id: "x".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: Some("".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let events = abp_openai_sdk::streaming::map_chunk(&chunk);
    assert!(events.is_empty());
}

#[test]
fn map_chunk_no_content_no_events() {
    let chunk = ChatCompletionChunk {
        id: "x".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    };
    let events = abp_openai_sdk::streaming::map_chunk(&chunk);
    assert!(events.is_empty());
}

#[test]
fn map_chunk_empty_choices() {
    let chunk = ChatCompletionChunk {
        id: "x".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
    let events = abp_openai_sdk::streaming::map_chunk(&chunk);
    assert!(events.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. ToolCallAccumulator (streaming)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_call_accumulator_new_is_empty() {
    let acc = ToolCallAccumulator::new();
    let events = acc.finish();
    assert!(events.is_empty());
}

#[test]
fn tool_call_accumulator_single_tool_call() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("read_file".into()),
            arguments: Some(r#"{"path":"#.into()),
        }),
    }]);
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(ChunkFunctionCall {
            name: None,
            arguments: Some(r#""main.rs"}"#.into()),
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
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("call_1"));
            assert_eq!(input, &json!({"path": "main.rs"}));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn tool_call_accumulator_multiple_tool_calls() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[
        ChunkToolCall {
            index: 0,
            id: Some("call_a".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("alpha".into()),
                arguments: Some("{}".into()),
            }),
        },
        ChunkToolCall {
            index: 1,
            id: Some("call_b".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("beta".into()),
                arguments: Some("{}".into()),
            }),
        },
    ]);
    let events = acc.finish();
    assert_eq!(events.len(), 2);
}

#[test]
fn tool_call_accumulator_finish_as_openai() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("test_fn".into()),
            arguments: Some(r#"{"a":1}"#.into()),
        }),
    }]);
    let pairs = acc.finish_as_openai();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].0, "call_1");
    assert_eq!(pairs[0].1.name, "test_fn");
    assert_eq!(pairs[0].1.arguments, r#"{"a":1}"#);
}

#[test]
fn tool_call_accumulator_skips_entries_with_no_name() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("call_empty".into()),
        call_type: Some("function".into()),
        function: None,
    }]);
    let events = acc.finish();
    assert!(events.is_empty());
}

#[test]
fn tool_call_accumulator_empty_id_becomes_none() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: None,
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("foo".into()),
            arguments: Some("{}".into()),
        }),
    }]);
    let events = acc.finish();
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { tool_use_id, .. } => {
            assert!(tool_use_id.is_none());
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn tool_call_accumulator_malformed_json_arguments() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("broken".into()),
            arguments: Some("not-json".into()),
        }),
    }]);
    let events = acc.finish();
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-json".into()));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validate_empty_fields_passes() {
    let fields = ExtendedRequestFields::default();
    assert!(abp_openai_sdk::validation::validate_for_mapped_mode(&fields).is_ok());
}

#[test]
fn validate_logprobs_true_fails() {
    let fields = ExtendedRequestFields {
        logprobs: Some(true),
        ..Default::default()
    };
    let err = abp_openai_sdk::validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert_eq!(err.errors.len(), 1);
    assert_eq!(err.errors[0].param, "logprobs");
}

#[test]
fn validate_logprobs_false_passes() {
    let fields = ExtendedRequestFields {
        logprobs: Some(false),
        ..Default::default()
    };
    assert!(abp_openai_sdk::validation::validate_for_mapped_mode(&fields).is_ok());
}

#[test]
fn validate_top_logprobs_fails() {
    let fields = ExtendedRequestFields {
        top_logprobs: Some(5),
        ..Default::default()
    };
    let err = abp_openai_sdk::validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert!(err.errors.iter().any(|e| e.param == "logprobs"));
}

#[test]
fn validate_logit_bias_nonempty_fails() {
    let mut bias = std::collections::BTreeMap::new();
    bias.insert("100".into(), 1.0);
    let fields = ExtendedRequestFields {
        logit_bias: Some(bias),
        ..Default::default()
    };
    let err = abp_openai_sdk::validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert!(err.errors.iter().any(|e| e.param == "logit_bias"));
}

#[test]
fn validate_logit_bias_empty_passes() {
    let fields = ExtendedRequestFields {
        logit_bias: Some(std::collections::BTreeMap::new()),
        ..Default::default()
    };
    assert!(abp_openai_sdk::validation::validate_for_mapped_mode(&fields).is_ok());
}

#[test]
fn validate_seed_fails() {
    let fields = ExtendedRequestFields {
        seed: Some(42),
        ..Default::default()
    };
    let err = abp_openai_sdk::validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert!(err.errors.iter().any(|e| e.param == "seed"));
}

#[test]
fn validate_multiple_unmappable_params() {
    let mut bias = std::collections::BTreeMap::new();
    bias.insert("50".into(), -2.0);
    let fields = ExtendedRequestFields {
        logprobs: Some(true),
        top_logprobs: None,
        logit_bias: Some(bias),
        seed: Some(123),
    };
    let err = abp_openai_sdk::validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert_eq!(err.errors.len(), 3);
}

#[test]
fn unmappable_param_display() {
    let p = UnmappableParam {
        param: "seed".into(),
        reason: "not supported".into(),
    };
    let display = format!("{p}");
    assert!(display.contains("seed"));
    assert!(display.contains("not supported"));
}

#[test]
fn validation_errors_display() {
    let errs = ValidationErrors {
        errors: vec![
            UnmappableParam {
                param: "a".into(),
                reason: "r1".into(),
            },
            UnmappableParam {
                param: "b".into(),
                reason: "r2".into(),
            },
        ],
    };
    let display = format!("{errs}");
    assert!(display.contains("2 unmappable parameter(s)"));
    assert!(display.contains("a"));
    assert!(display.contains("b"));
}

#[test]
fn unmappable_param_serde_roundtrip() {
    let p = UnmappableParam {
        param: "logprobs".into(),
        reason: "not supported".into(),
    };
    let json = serde_json::to_string(&p).unwrap();
    let parsed: UnmappableParam = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, p);
}

#[test]
fn validation_errors_serde_roundtrip() {
    let errs = ValidationErrors {
        errors: vec![UnmappableParam {
            param: "seed".into(),
            reason: "unmappable".into(),
        }],
    };
    let json = serde_json::to_string(&errs).unwrap();
    let parsed: ValidationErrors = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, errs);
}

#[test]
fn extended_request_fields_default_all_none() {
    let fields = ExtendedRequestFields::default();
    assert!(fields.logprobs.is_none());
    assert!(fields.top_logprobs.is_none());
    assert!(fields.logit_bias.is_none());
    assert!(fields.seed.is_none());
}

#[test]
fn extended_request_fields_serde_roundtrip() {
    let fields = ExtendedRequestFields {
        logprobs: Some(true),
        top_logprobs: Some(3),
        logit_bias: None,
        seed: Some(42),
    };
    let json = serde_json::to_string(&fields).unwrap();
    let parsed: ExtendedRequestFields = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.logprobs, Some(true));
    assert_eq!(parsed.seed, Some(42));
}

#[test]
fn unmappable_param_is_std_error() {
    let p = UnmappableParam {
        param: "test".into(),
        reason: "reason".into(),
    };
    let _: &dyn std::error::Error = &p;
}

#[test]
fn validation_errors_is_std_error() {
    let errs = ValidationErrors { errors: vec![] };
    let _: &dyn std::error::Error = &errs;
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. API types: Message enum serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn api_message_system_serde() {
    let msg = Message::System {
        content: "Be helpful".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"system""#));
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn api_message_user_serde() {
    let msg = Message::User {
        content: "Hello".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"user""#));
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn api_message_assistant_with_content_serde() {
    let msg = Message::Assistant {
        content: Some("Sure!".into()),
        tool_calls: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"assistant""#));
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn api_message_assistant_with_tool_calls_serde() {
    let msg = Message::Assistant {
        content: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"main.rs"}"#.into(),
            },
        }]),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn api_message_tool_serde() {
    let msg = Message::Tool {
        tool_call_id: "call_1".into(),
        content: "file data".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"tool""#));
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. API types: FinishReason serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn finish_reason_stop_serde() {
    let r = FinishReason::Stop;
    assert_eq!(serde_json::to_string(&r).unwrap(), r#""stop""#);
}

#[test]
fn finish_reason_length_serde() {
    let r = FinishReason::Length;
    assert_eq!(serde_json::to_string(&r).unwrap(), r#""length""#);
}

#[test]
fn finish_reason_tool_calls_serde() {
    let r = FinishReason::ToolCalls;
    assert_eq!(serde_json::to_string(&r).unwrap(), r#""tool_calls""#);
}

#[test]
fn finish_reason_content_filter_serde() {
    let r = FinishReason::ContentFilter;
    assert_eq!(serde_json::to_string(&r).unwrap(), r#""content_filter""#);
}

#[test]
fn finish_reason_all_variants_roundtrip() {
    for reason in [
        FinishReason::Stop,
        FinishReason::Length,
        FinishReason::ToolCalls,
        FinishReason::ContentFilter,
    ] {
        let json = serde_json::to_string(&reason).unwrap();
        let parsed: FinishReason = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, reason);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 21. API types: Tool, FunctionDefinition, ToolCall, FunctionCall
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn api_tool_serde_roundtrip() {
    let tool = Tool {
        tool_type: "function".into(),
        function: FunctionDefinition {
            name: "get_weather".into(),
            description: Some("Get weather".into()),
            parameters: Some(json!({"type": "object", "properties": {"city": {"type": "string"}}})),
            strict: Some(true),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let parsed: Tool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn api_function_definition_minimal_serde() {
    let fd = FunctionDefinition {
        name: "noop".into(),
        description: None,
        parameters: None,
        strict: None,
    };
    let json = serde_json::to_string(&fd).unwrap();
    assert!(!json.contains("description"));
    assert!(!json.contains("parameters"));
    assert!(!json.contains("strict"));
    let parsed: FunctionDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, fd);
}

#[test]
fn api_tool_call_serde_roundtrip() {
    let tc = ToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "bash".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: ToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

// ═══════════════════════════════════════════════════════════════════════════
// 22. API types: Usage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn api_usage_serde_roundtrip() {
    let u = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        completion_tokens_details: None,
        prompt_tokens_details: None,
    };
    let json = serde_json::to_string(&u).unwrap();
    let parsed: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, u);
}

#[test]
fn api_usage_zero_values() {
    let u = Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        completion_tokens_details: None,
        prompt_tokens_details: None,
    };
    let json = serde_json::to_string(&u).unwrap();
    let parsed: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.total_tokens, 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 23. API types: ChatCompletionRequest serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn api_request_full_serde_roundtrip() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![
            Message::System {
                content: "Be helpful.".into(),
            },
            Message::User {
                content: "Hello".into(),
            },
        ],
        temperature: Some(0.7),
        max_tokens: Some(4096),
        tools: None,
        tool_choice: None,
        stream: Some(true),
        stream_options: None,
        top_p: Some(0.9),
        frequency_penalty: Some(0.1),
        presence_penalty: Some(0.2),
        stop: Some(vec!["END".into()]),
        n: Some(2),
        seed: Some(42),
        response_format: Some(ResponseFormat::json_object()),
        user: Some("user-123".into()),
        parallel_tool_calls: None,
        service_tier: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn api_request_omits_none_fields() {
    let req = make_api_request(vec![]);
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("temperature"));
    assert!(!json.contains("max_tokens"));
    assert!(!json.contains("tools"));
    assert!(!json.contains("stream"));
    assert!(!json.contains("top_p"));
    assert!(!json.contains("stop"));
    assert!(!json.contains("user"));
    assert!(!json.contains("seed"));
    assert!(!json.contains("response_format"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 24. API types: ChatCompletionResponse serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn api_response_serde_roundtrip() {
    let resp = ChatCompletionResponse {
        id: "chatcmpl-abc".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![Choice {
            index: 0,
            message: AssistantMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
            },
            finish_reason: FinishReason::Stop,
        }],
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            completion_tokens_details: None,
            prompt_tokens_details: None,
        }),
        system_fingerprint: Some("fp_abc123".into()),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn api_response_no_usage_no_fingerprint() {
    let resp = ChatCompletionResponse {
        id: "x".into(),
        object: "chat.completion".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
        system_fingerprint: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(!json.contains("usage"));
    assert!(!json.contains("system_fingerprint"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 25. API types: StreamChunk, StreamChoice, Delta
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn api_stream_chunk_serde_roundtrip() {
    let chunk = StreamChunk {
        id: "chatcmpl-stream-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: Delta {
                role: Some("assistant".into()),
                content: Some("Hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: StreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, chunk);
}

#[test]
fn api_delta_default_all_none() {
    let d = Delta::default();
    assert!(d.role.is_none());
    assert!(d.content.is_none());
    assert!(d.tool_calls.is_none());
}

#[test]
fn api_stream_choice_with_finish_reason() {
    let sc = StreamChoice {
        index: 0,
        delta: Delta::default(),
        finish_reason: Some(FinishReason::Stop),
    };
    let json = serde_json::to_string(&sc).unwrap();
    let parsed: StreamChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, sc);
}

// ═══════════════════════════════════════════════════════════════════════════
// 26. API conversions: From<ChatCompletionRequest> for WorkOrder
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn api_request_to_work_order_last_user_message() {
    let req = make_api_request(vec![
        Message::User {
            content: "First".into(),
        },
        Message::User {
            content: "Second".into(),
        },
    ]);
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Second");
}

#[test]
fn api_request_to_work_order_preserves_model() {
    let mut req = make_api_request(vec![Message::User {
        content: "Hello".into(),
    }]);
    req.model = "gpt-4-turbo".into();
    let wo: WorkOrder = req.into();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
}

#[test]
fn api_request_to_work_order_system_to_snippets() {
    let req = make_api_request(vec![
        Message::System {
            content: "Be concise.".into(),
        },
        Message::User {
            content: "Hi".into(),
        },
    ]);
    let wo: WorkOrder = req.into();
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].content, "Be concise.");
}

#[test]
fn api_request_to_work_order_empty_messages() {
    let req = make_api_request(vec![]);
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "");
}

#[test]
fn api_request_to_work_order_only_system_messages() {
    let req = make_api_request(vec![
        Message::System {
            content: "System 1".into(),
        },
        Message::System {
            content: "System 2".into(),
        },
    ]);
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "");
    assert_eq!(wo.context.snippets.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 27. API conversions: From<Receipt> for ChatCompletionResponse
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_to_api_response_maps_text() {
    let trace = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        },
        ext: None,
    }];
    let receipt = make_receipt(trace, UsageNormalized::default());
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    assert_eq!(resp.choices[0].finish_reason, FinishReason::Stop);
}

#[test]
fn receipt_to_api_response_maps_tool_calls() {
    let trace = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("call_abc".into()),
            parent_tool_use_id: None,
            input: json!({"path": "main.rs"}),
        },
        ext: None,
    }];
    let receipt = make_receipt(trace, UsageNormalized::default());
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].finish_reason, FinishReason::ToolCalls);
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].id, "call_abc");
}

#[test]
fn receipt_to_api_response_maps_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..UsageNormalized::default()
    };
    let receipt = make_receipt(vec![], usage);
    let resp: ChatCompletionResponse = receipt.into();
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

#[test]
fn receipt_to_api_response_no_usage() {
    let receipt = make_receipt(vec![], UsageNormalized::default());
    let resp: ChatCompletionResponse = receipt.into();
    assert!(resp.usage.is_none());
}

#[test]
fn receipt_to_api_response_failed_outcome() {
    let mut receipt = make_receipt(vec![], UsageNormalized::default());
    receipt.outcome = Outcome::Failed;
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].finish_reason, FinishReason::Stop);
}

#[test]
fn receipt_to_api_response_id_format() {
    let receipt = make_receipt(vec![], UsageNormalized::default());
    let run_id = receipt.meta.run_id;
    let resp: ChatCompletionResponse = receipt.into();
    assert!(resp.id.starts_with("chatcmpl-"));
    assert!(resp.id.contains(&run_id.to_string()));
}

#[test]
fn receipt_to_api_response_object_field() {
    let receipt = make_receipt(vec![], UsageNormalized::default());
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.object, "chat.completion");
}

// ═══════════════════════════════════════════════════════════════════════════
// 28. API helper functions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn api_tool_calls_to_events_maps_correctly() {
    let tcs = vec![ToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "read_file".into(),
            arguments: r#"{"path":"a.rs"}"#.into(),
        },
    }];
    let events = abp_openai_sdk::api::tool_calls_to_events(&tcs);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("call_1"));
            assert_eq!(input, &json!({"path": "a.rs"}));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn api_tool_calls_to_events_empty() {
    let events = abp_openai_sdk::api::tool_calls_to_events(&[]);
    assert!(events.is_empty());
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
    let tc = abp_openai_sdk::api::event_to_tool_call(&event).unwrap();
    assert_eq!(tc.id, "call_x");
    assert_eq!(tc.function.name, "bash");
}

#[test]
fn api_event_to_tool_call_returns_none_for_non_tool() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    assert!(abp_openai_sdk::api::event_to_tool_call(&event).is_none());
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
    let tc = abp_openai_sdk::api::event_to_tool_call(&event).unwrap();
    assert_eq!(tc.id, "call_0");
}

#[test]
fn api_tool_calls_to_events_malformed_arguments() {
    let tcs = vec![ToolCall {
        id: "call_bad".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "foo".into(),
            arguments: "not-json".into(),
        },
    }];
    let events = abp_openai_sdk::api::tool_calls_to_events(&tcs);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-json".into()));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 29. OpenAI dialect struct cloning and equality
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn canonical_tool_def_clone_eq() {
    let a = CanonicalToolDef {
        name: "x".into(),
        description: "y".into(),
        parameters_schema: json!({}),
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn openai_tool_def_clone_eq() {
    let a = OpenAIToolDef {
        tool_type: "function".into(),
        function: OpenAIFunctionDef {
            name: "f".into(),
            description: "d".into(),
            parameters: json!({}),
        },
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn openai_tool_call_clone_eq() {
    let a = OpenAIToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: OpenAIFunctionCall {
            name: "f".into(),
            arguments: "{}".into(),
        },
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn tool_choice_function_ref_clone_eq() {
    let a = ToolChoiceFunctionRef { name: "x".into() };
    let b = a.clone();
    assert_eq!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════════
// 30. Edge cases and special scenarios
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_message_with_unicode_content() {
    let msg = make_openai_msg("user", Some("日本語テスト 🎉"));
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: OpenAIMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.content.as_deref(), Some("日本語テスト 🎉"));
}

#[test]
fn api_message_with_unicode_content() {
    let msg = Message::User {
        content: "Ünîcödé ✨".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

#[test]
fn openai_message_with_very_long_content() {
    let long = "x".repeat(100_000);
    let msg = make_openai_msg("user", Some(&long));
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: OpenAIMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.content.as_deref().unwrap().len(), 100_000);
}

#[test]
fn openai_message_with_special_json_characters() {
    let msg = make_openai_msg("user", Some(r#"He said "hello" and \n\t"#));
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: OpenAIMessage = serde_json::from_str(&json).unwrap();
    assert!(parsed.content.as_deref().unwrap().contains("hello"));
}

#[test]
fn api_request_with_multiple_stop_sequences() {
    let mut req = make_api_request(vec![Message::User {
        content: "test".into(),
    }]);
    req.stop = Some(vec!["END".into(), "STOP".into(), "\n\n".into()]);
    let json = serde_json::to_string(&req).unwrap();
    let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.stop.as_ref().unwrap().len(), 3);
}

#[test]
fn api_assistant_message_default_role() {
    let json = r#"{"content":"Hello!"}"#;
    let parsed: AssistantMessage = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.role, "assistant");
}

#[test]
fn api_choice_serde_roundtrip() {
    let choice = Choice {
        index: 0,
        message: AssistantMessage {
            role: "assistant".into(),
            content: Some("ok".into()),
            tool_calls: None,
        },
        finish_reason: FinishReason::Stop,
    };
    let json = serde_json::to_string(&choice).unwrap();
    let parsed: Choice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, choice);
}

#[test]
fn openai_function_call_with_complex_arguments() {
    let args = json!({
        "path": "src/main.rs",
        "content": "fn main() {\n    println!(\"hello\");\n}",
        "nested": {"key": [1, 2, 3]}
    });
    let fc = OpenAIFunctionCall {
        name: "write_file".into(),
        arguments: serde_json::to_string(&args).unwrap(),
    };
    let json = serde_json::to_string(&fc).unwrap();
    let parsed: OpenAIFunctionCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, fc);
}

#[test]
fn openai_tool_def_serde_roundtrip() {
    let td = OpenAIToolDef {
        tool_type: "function".into(),
        function: OpenAIFunctionDef {
            name: "test".into(),
            description: "A test function".into(),
            parameters: json!({"type": "object", "properties": {"x": {"type": "number"}}}),
        },
    };
    let json = serde_json::to_string(&td).unwrap();
    let parsed: OpenAIToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, td);
}

#[test]
fn openai_request_with_tools_and_tool_choice() {
    let req = OpenAIRequest {
        model: "gpt-4o".into(),
        messages: vec![make_openai_msg("user", Some("test"))],
        tools: Some(vec![OpenAIToolDef {
            tool_type: "function".into(),
            function: OpenAIFunctionDef {
                name: "bash".into(),
                description: "Run commands".into(),
                parameters: json!({"type": "object"}),
            },
        }]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        temperature: Some(0.5),
        max_tokens: Some(2048),
        response_format: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: OpenAIRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.tools.as_ref().unwrap().len(), 1);
}

#[test]
fn receipt_to_response_concatenates_multiple_messages() {
    let trace = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Part 1. ".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Part 2.".into(),
            },
            ext: None,
        },
    ];
    let receipt = make_receipt(trace, UsageNormalized::default());
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Part 1. Part 2.")
    );
}

#[test]
fn openai_tool_call_debug_format() {
    let tc = OpenAIToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: OpenAIFunctionCall {
            name: "test".into(),
            arguments: "{}".into(),
        },
    };
    let debug = format!("{tc:?}");
    assert!(debug.contains("call_1"));
    assert!(debug.contains("test"));
}

#[test]
fn tool_call_accumulator_default_is_new() {
    let acc = ToolCallAccumulator::default();
    let events = acc.finish();
    assert!(events.is_empty());
}
