// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive snapshot tests for SDK dialect types, protocol envelopes,
//! and cross-dialect mapping.

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_claude_sdk::dialect::{
    ClaudeContentBlock, ClaudeMessage, ClaudeMessageDelta, ClaudeRequest, ClaudeResponse,
    ClaudeStreamDelta, ClaudeStreamEvent, ClaudeUsage, ThinkingConfig,
};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, RunMetadata, SupportLevel, UsageNormalized,
    VerificationReport, WorkOrderBuilder, error::MappingError,
};
use abp_gemini_sdk::dialect::{
    FunctionCallingMode, GeminiContent, GeminiFunctionCallingConfig, GeminiFunctionDeclaration,
    GeminiGenerationConfig, GeminiPart, GeminiRequest, GeminiSafetySetting, GeminiTool,
    GeminiToolConfig, HarmBlockThreshold, HarmCategory,
};
use abp_integrations::projection::{Dialect, ProjectionMatrix, translate};
use abp_openai_sdk::dialect::{
    OpenAIFunctionCall, OpenAIFunctionDef, OpenAIMessage, OpenAIRequest, OpenAIToolCall,
    OpenAIToolDef, ToolChoice, ToolChoiceFunctionRef, ToolChoiceMode,
};
use abp_openai_sdk::response_format::{JsonSchemaSpec, ResponseFormat};
use abp_openai_sdk::streaming::ChunkUsage;
use abp_openai_sdk::streaming::{
    ChatCompletionChunk, ChunkChoice, ChunkDelta, ChunkFunctionCall, ChunkToolCall,
};
use abp_openai_sdk::validation::{ExtendedRequestFields, UnmappableParam, ValidationErrors};
use abp_protocol::Envelope;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_uuid() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap()
}

fn fixed_uuid2() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap()
}

fn sample_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
    caps
}

fn sample_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts(),
            duration_ms: 1234,
        },
        backend: BackendIdentity {
            id: "sidecar:test".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: sample_capabilities(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"input_tokens": 100, "output_tokens": 50}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.001),
        },
        trace: vec![AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        }],
        artifacts: vec![ArtifactRef {
            kind: "file".into(),
            path: "output.txt".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff --git a/file.txt".into()),
            git_status: Some("M file.txt".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ===========================================================================
// 1. OpenAI dialect snapshots
// ===========================================================================

#[test]
fn openai_request_full() {
    let req = OpenAIRequest {
        model: "gpt-4".into(),
        messages: vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("You are a helpful assistant.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Write a unit test.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "call_001".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "write_file".into(),
                        arguments: r#"{"path":"test.rs","content":"fn it_works() {}"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            },
        ],
        tools: Some(vec![OpenAIToolDef {
            tool_type: "function".into(),
            function: OpenAIFunctionDef {
                name: "write_file".into(),
                description: "Write content to a file".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "content": {"type": "string"}
                    },
                    "required": ["path", "content"]
                }),
            },
        }]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        temperature: Some(0.7),
        max_tokens: Some(4096),
        response_format: None,
    };
    insta::assert_json_snapshot!("openai_request_full", req);
}

#[test]
fn openai_streaming_chunk() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-fixed-001".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: Some("assistant".into()),
                content: Some("Hello, ".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    insta::assert_json_snapshot!("openai_streaming_chunk", chunk);
}

#[test]
fn openai_streaming_chunk_with_tool_call() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-fixed-002".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: None,
                tool_calls: Some(vec![ChunkToolCall {
                    index: 0,
                    id: Some("call_tc_001".into()),
                    call_type: Some("function".into()),
                    function: Some(ChunkFunctionCall {
                        name: Some("read_file".into()),
                        arguments: Some(r#"{"path":"main.rs"}"#.into()),
                    }),
                }]),
            },
            finish_reason: None,
        }],
        usage: None,
    };
    insta::assert_json_snapshot!("openai_streaming_chunk_tool_call", chunk);
}

#[test]
fn openai_streaming_chunk_final_usage() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-fixed-003".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: Some(ChunkUsage {
            prompt_tokens: 50,
            completion_tokens: 120,
            total_tokens: 170,
        }),
    };
    insta::assert_json_snapshot!("openai_streaming_chunk_final_usage", chunk);
}

#[test]
fn openai_response_format_variants() {
    let text = ResponseFormat::Text;
    let json_obj = ResponseFormat::JsonObject;
    let json_schema = ResponseFormat::JsonSchema {
        json_schema: JsonSchemaSpec {
            name: "my_schema".into(),
            description: Some("A test schema".into()),
            schema: json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            }),
            strict: Some(true),
        },
    };
    insta::assert_json_snapshot!("openai_response_format_text", text);
    insta::assert_json_snapshot!("openai_response_format_json_object", json_obj);
    insta::assert_json_snapshot!("openai_response_format_json_schema", json_schema);
}

#[test]
fn openai_tool_choice_variants() {
    let none = ToolChoice::Mode(ToolChoiceMode::None);
    let auto = ToolChoice::Mode(ToolChoiceMode::Auto);
    let required = ToolChoice::Mode(ToolChoiceMode::Required);
    let specific = ToolChoice::Function {
        tool_type: "function".into(),
        function: ToolChoiceFunctionRef {
            name: "read_file".into(),
        },
    };
    insta::assert_json_snapshot!("openai_tool_choice_none", none);
    insta::assert_json_snapshot!("openai_tool_choice_auto", auto);
    insta::assert_json_snapshot!("openai_tool_choice_required", required);
    insta::assert_json_snapshot!("openai_tool_choice_specific_function", specific);
}

#[test]
fn openai_validation_errors() {
    let errs = ValidationErrors {
        errors: vec![
            UnmappableParam {
                param: "logprobs".into(),
                reason: "Not supported in Claude dialect".into(),
            },
            UnmappableParam {
                param: "logit_bias".into(),
                reason: "No equivalent in Gemini".into(),
            },
        ],
    };
    insta::assert_json_snapshot!("openai_validation_errors", errs);
}

#[test]
fn openai_extended_request_fields() {
    let mut bias = BTreeMap::new();
    bias.insert("50256".into(), -100.0);
    bias.insert("198".into(), 5.0);
    let fields = ExtendedRequestFields {
        logprobs: Some(true),
        top_logprobs: Some(5),
        logit_bias: Some(bias),
        seed: Some(42),
    };
    insta::assert_json_snapshot!("openai_extended_request_fields", fields);
}

// ===========================================================================
// 2. Claude dialect snapshots
// ===========================================================================

#[test]
fn claude_request_with_thinking() {
    let req = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 16384,
        system: Some("You are an expert Rust developer.".into()),
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: "Explain ownership in Rust.".into(),
        }],
        thinking: Some(ThinkingConfig::new(10000)),
    };
    insta::assert_json_snapshot!("claude_request_with_thinking", req);
}

#[test]
fn claude_streaming_content_block_start() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::Text {
            text: String::new(),
        },
    };
    insta::assert_json_snapshot!("claude_stream_content_block_start", event);
}

#[test]
fn claude_streaming_content_block_delta() {
    let text_delta = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "Hello, world!".into(),
        },
    };
    let thinking_delta = ClaudeStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "Let me think about this...".into(),
        },
    };
    let input_json_delta = ClaudeStreamEvent::ContentBlockDelta {
        index: 2,
        delta: ClaudeStreamDelta::InputJsonDelta {
            partial_json: r#"{"path":"#.into(),
        },
    };
    insta::assert_json_snapshot!("claude_stream_text_delta", text_delta);
    insta::assert_json_snapshot!("claude_stream_thinking_delta", thinking_delta);
    insta::assert_json_snapshot!("claude_stream_input_json_delta", input_json_delta);
}

#[test]
fn claude_stop_reason_mapping() {
    let end_turn = ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
        },
        usage: Some(ClaudeUsage {
            input_tokens: 200,
            output_tokens: 150,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let max_tokens = ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("max_tokens".into()),
            stop_sequence: None,
        },
        usage: None,
    };
    let stop_seq = ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("stop_sequence".into()),
            stop_sequence: Some("\n```".into()),
        },
        usage: None,
    };
    insta::assert_json_snapshot!("claude_stop_reason_end_turn", end_turn);
    insta::assert_json_snapshot!("claude_stop_reason_max_tokens", max_tokens);
    insta::assert_json_snapshot!("claude_stop_reason_stop_sequence", stop_seq);
}

#[test]
fn claude_passthrough_response_roundtrip() {
    let response = ClaudeResponse {
        id: "msg_fixed_001".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Text {
            text: "Here is the answer.".into(),
        }],
        stop_reason: Some("end_turn".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 100,
            output_tokens: 42,
            cache_creation_input_tokens: Some(80),
            cache_read_input_tokens: Some(20),
        }),
    };
    let json_str = serde_json::to_string(&response).unwrap();
    let roundtripped: ClaudeResponse = serde_json::from_str(&json_str).unwrap();
    insta::assert_json_snapshot!("claude_passthrough_roundtrip", roundtripped);
}

#[test]
fn claude_tool_use_block() {
    let tool_use = ClaudeContentBlock::ToolUse {
        id: "toolu_fixed_001".into(),
        name: "read_file".into(),
        input: json!({"path": "src/main.rs"}),
    };
    let tool_result = ClaudeContentBlock::ToolResult {
        tool_use_id: "toolu_fixed_001".into(),
        content: Some("fn main() { println!(\"Hello\"); }".into()),
        is_error: Some(false),
    };
    insta::assert_json_snapshot!("claude_tool_use_block", tool_use);
    insta::assert_json_snapshot!("claude_tool_result_block", tool_result);
}

#[test]
fn claude_message_start_event() {
    let event = ClaudeStreamEvent::MessageStart {
        message: ClaudeResponse {
            id: "msg_fixed_002".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![],
            stop_reason: None,
            usage: Some(ClaudeUsage {
                input_tokens: 50,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        },
    };
    insta::assert_json_snapshot!("claude_stream_message_start", event);
}

// ===========================================================================
// 3. Gemini dialect snapshots
// ===========================================================================

#[test]
fn gemini_generation_config_full() {
    let config = GeminiGenerationConfig {
        max_output_tokens: Some(8192),
        temperature: Some(0.9),
        top_p: Some(0.95),
        top_k: Some(40),
        stop_sequences: Some(vec!["END".into(), "STOP".into()]),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(json!({
            "type": "object",
            "properties": {"result": {"type": "string"}}
        })),
    };
    insta::assert_json_snapshot!("gemini_generation_config_full", config);
}

#[test]
fn gemini_safety_settings_all_categories() {
    let settings = vec![
        GeminiSafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        },
        GeminiSafetySetting {
            category: HarmCategory::HarmCategoryHateSpeech,
            threshold: HarmBlockThreshold::BlockLowAndAbove,
        },
        GeminiSafetySetting {
            category: HarmCategory::HarmCategorySexuallyExplicit,
            threshold: HarmBlockThreshold::BlockOnlyHigh,
        },
        GeminiSafetySetting {
            category: HarmCategory::HarmCategoryDangerousContent,
            threshold: HarmBlockThreshold::BlockNone,
        },
        GeminiSafetySetting {
            category: HarmCategory::HarmCategoryCivicIntegrity,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        },
    ];
    insta::assert_json_snapshot!("gemini_safety_settings_all", settings);
}

#[test]
fn gemini_function_call_part() {
    let part = GeminiPart::FunctionCall {
        name: "read_file".into(),
        args: json!({"path": "src/lib.rs"}),
    };
    insta::assert_json_snapshot!("gemini_function_call_part", part);
}

#[test]
fn gemini_function_response_part() {
    let part = GeminiPart::FunctionResponse {
        name: "read_file".into(),
        response: json!({"content": "pub fn main() {}", "size": 17}),
    };
    insta::assert_json_snapshot!("gemini_function_response_part", part);
}

#[test]
fn gemini_multi_part_content() {
    let content = GeminiContent {
        role: "user".into(),
        parts: vec![
            GeminiPart::Text("Analyze this code:".into()),
            GeminiPart::FunctionResponse {
                name: "read_file".into(),
                response: json!({"content": "fn main() {}"}),
            },
        ],
    };
    insta::assert_json_snapshot!("gemini_multi_part_content", content);
}

#[test]
fn gemini_request_full() {
    let req = GeminiRequest {
        model: "gemini-2.0-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hello!".into())],
        }],
        system_instruction: Some(GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("You are a coding assistant.".into())],
        }),
        generation_config: Some(GeminiGenerationConfig {
            max_output_tokens: Some(4096),
            temperature: Some(0.7),
            top_p: None,
            top_k: None,
            stop_sequences: None,
            response_mime_type: None,
            response_schema: None,
        }),
        safety_settings: Some(vec![GeminiSafetySetting {
            category: HarmCategory::HarmCategoryDangerousContent,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        }]),
        tools: Some(vec![GeminiTool {
            function_declarations: vec![GeminiFunctionDeclaration {
                name: "write_file".into(),
                description: "Write to a file".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            }],
        }]),
        tool_config: Some(GeminiToolConfig {
            function_calling_config: GeminiFunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        }),
    };
    insta::assert_json_snapshot!("gemini_request_full", req);
}

#[test]
fn gemini_tool_config_any_restricted() {
    let config = GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["read_file".into(), "write_file".into()]),
        },
    };
    insta::assert_json_snapshot!("gemini_tool_config_any_restricted", config);
}

// ===========================================================================
// 4. Protocol envelope snapshots
// ===========================================================================

#[test]
fn envelope_hello() {
    let env = Envelope::Hello {
        contract_version: "abp/v0.1".into(),
        backend: BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.2.0".into()),
        },
        capabilities: sample_capabilities(),
        mode: ExecutionMode::Mapped,
    };
    let json = serde_json::to_string_pretty(&env).unwrap();
    insta::assert_snapshot!("envelope_hello", json);
}

#[test]
fn envelope_run_with_work_order() {
    let wo = WorkOrderBuilder::new("Fix the login bug in auth.rs")
        .model("gpt-4")
        .max_budget_usd(1.0)
        .max_turns(10)
        .build();
    let env = Envelope::Run {
        id: "run-fixed-001".into(),
        work_order: wo,
    };
    insta::assert_json_snapshot!("envelope_run_full",
        env,
        {
            ".work_order.id" => "[uuid]",
        }
    );
}

#[test]
fn envelope_event_all_kinds() {
    let kinds: Vec<(&str, AgentEvent)> = vec![
        (
            "run_started",
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunStarted {
                    message: "Initializing...".into(),
                },
                ext: None,
            },
        ),
        (
            "run_completed",
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunCompleted {
                    message: "Done.".into(),
                },
                ext: None,
            },
        ),
        (
            "assistant_delta",
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::AssistantDelta {
                    text: "Hello".into(),
                },
                ext: None,
            },
        ),
        (
            "assistant_message",
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Full message.".into(),
                },
                ext: None,
            },
        ),
        (
            "tool_call",
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("tc-001".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "src/main.rs"}),
                },
                ext: None,
            },
        ),
        (
            "tool_result",
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("tc-001".into()),
                    output: json!("fn main() {}"),
                    is_error: false,
                },
                ext: None,
            },
        ),
        (
            "file_changed",
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::FileChanged {
                    path: "src/lib.rs".into(),
                    summary: "Added new function".into(),
                },
                ext: None,
            },
        ),
        (
            "command_executed",
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::CommandExecuted {
                    command: "cargo test".into(),
                    exit_code: Some(0),
                    output_preview: Some("test result: ok".into()),
                },
                ext: None,
            },
        ),
        (
            "warning",
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::Warning {
                    message: "Rate limit approaching".into(),
                },
                ext: None,
            },
        ),
        (
            "error",
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::Error {
                    message: "Connection timeout".into(),
                },
                ext: None,
            },
        ),
    ];

    for (name, event) in &kinds {
        let env = Envelope::Event {
            ref_id: "run-fixed-001".into(),
            event: event.clone(),
        };
        insta::assert_json_snapshot!(format!("envelope_event_{name}"), env);
    }
}

#[test]
fn envelope_final_with_receipt() {
    let receipt = sample_receipt();
    let env = Envelope::Final {
        ref_id: "run-fixed-001".into(),
        receipt,
    };
    let json = serde_json::to_string_pretty(&env).unwrap();
    insta::assert_snapshot!("envelope_final_receipt", json);
}

#[test]
fn envelope_fatal_with_error() {
    let env_with_ref = Envelope::Fatal {
        ref_id: Some("run-fixed-001".into()),
        error: "Sidecar process crashed with exit code 137 (OOM killed)".into(),
    };
    let env_no_ref = Envelope::Fatal {
        ref_id: None,
        error: "Failed to parse hello envelope: invalid JSON at line 1".into(),
    };
    insta::assert_json_snapshot!("envelope_fatal_with_ref", env_with_ref);
    insta::assert_json_snapshot!("envelope_fatal_no_ref", env_no_ref);
}

// ===========================================================================
// 5. Cross-dialect mapping snapshots
// ===========================================================================

#[test]
fn cross_dialect_to_openai() {
    let wo = WorkOrderBuilder::new("Refactor the auth module")
        .model("gpt-4")
        .build();
    let result = translate(Dialect::Abp, Dialect::OpenAi, &wo);
    match result {
        Ok(val) => insta::assert_json_snapshot!("cross_dialect_abp_to_openai",
            val,
            {
                ".id" => "[uuid]",
                ".work_order_id" => "[uuid]",
            }
        ),
        Err(e) => insta::assert_snapshot!("cross_dialect_abp_to_openai_err", e.to_string()),
    }
}

#[test]
fn cross_dialect_to_claude() {
    let wo = WorkOrderBuilder::new("Refactor the auth module")
        .model("claude-sonnet-4-20250514")
        .build();
    let result = translate(Dialect::Abp, Dialect::Claude, &wo);
    match result {
        Ok(val) => insta::assert_json_snapshot!("cross_dialect_abp_to_claude",
            val,
            {
                ".id" => "[uuid]",
                ".work_order_id" => "[uuid]",
            }
        ),
        Err(e) => insta::assert_snapshot!("cross_dialect_abp_to_claude_err", e.to_string()),
    }
}

#[test]
fn cross_dialect_to_gemini() {
    let wo = WorkOrderBuilder::new("Refactor the auth module")
        .model("gemini-2.0-flash")
        .build();
    let result = translate(Dialect::Abp, Dialect::Gemini, &wo);
    match result {
        Ok(val) => insta::assert_json_snapshot!("cross_dialect_abp_to_gemini",
            val,
            {
                ".id" => "[uuid]",
                ".work_order_id" => "[uuid]",
            }
        ),
        Err(e) => insta::assert_snapshot!("cross_dialect_abp_to_gemini_err", e.to_string()),
    }
}

#[test]
fn projection_matrix_fidelity_report() {
    let matrix = ProjectionMatrix::new();
    let translations = matrix.supported_translations();
    let mut report: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (from, to) in &translations {
        report
            .entry(format!("{from:?}"))
            .or_default()
            .push(format!("{to:?}"));
    }
    insta::assert_json_snapshot!("projection_fidelity_report", report);
}

#[test]
fn error_taxonomy_display_strings() {
    let errors: Vec<(String, String, String)> = vec![
        MappingError::FidelityLoss {
            field: "temperature".into(),
            source_dialect: "openai".into(),
            target_dialect: "gemini".into(),
            detail: "Gemini uses different scaling".into(),
        },
        MappingError::UnsupportedCapability {
            capability: "logprobs".into(),
            dialect: "claude".into(),
        },
        MappingError::EmulationRequired {
            feature: "json_mode".into(),
            detail: "Emulated via system prompt instruction".into(),
        },
        MappingError::IncompatibleModel {
            requested: "gpt-4-turbo".into(),
            dialect: "claude".into(),
            suggestion: Some("claude-sonnet-4-20250514".into()),
        },
        MappingError::ParameterNotMappable {
            parameter: "seed".into(),
            value: "42".into(),
            dialect: "gemini".into(),
        },
        MappingError::StreamingUnsupported {
            dialect: "mock".into(),
        },
    ]
    .into_iter()
    .map(|e| {
        (
            e.code().to_string(),
            e.to_string(),
            format!("{:?}", e.kind()),
        )
    })
    .collect();

    insta::assert_json_snapshot!("error_taxonomy_display", errors);
}

#[test]
fn mapping_error_json_roundtrip() {
    let err = MappingError::FidelityLoss {
        field: "response_format".into(),
        source_dialect: "openai".into(),
        target_dialect: "claude".into(),
        detail: "Claude does not support json_schema response format natively".into(),
    };
    let json_str = serde_json::to_string_pretty(&err).unwrap();
    let roundtripped: MappingError = serde_json::from_str(&json_str).unwrap();
    assert_eq!(err, roundtripped);
    insta::assert_snapshot!("mapping_error_fidelity_loss_json", json_str);
}
