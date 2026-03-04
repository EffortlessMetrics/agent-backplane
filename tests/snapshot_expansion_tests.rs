#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Expanded snapshot tests for ABP contract types, protocol envelopes, error
//! taxonomy, capability reports, policy profiles, and all SDK shim formats.

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use insta::{assert_json_snapshot, assert_snapshot};
use serde_json::json;
use uuid::Uuid;

use abp_core::negotiate::{CapabilityReport, CapabilityReportEntry, DialectSupportLevel};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_error::{ErrorCode, ErrorInfo};
use abp_protocol::Envelope;

// SDK types
use abp_claude_sdk::messages::{
    ContentBlock, Message as ClaudeMessage, MessageContent, MessagesRequest, MessagesResponse,
    Metadata, Role, SystemMessage, Usage as ClaudeUsage,
};
use abp_codex_sdk::types::{
    CodexChoice, CodexChoiceMessage, CodexCommand, CodexFileChange, CodexFunctionCall,
    CodexFunctionDef, CodexMessage, CodexRequest, CodexResponse, CodexTool, CodexToolCall,
    CodexUsage, FileOperation,
};
use abp_copilot_sdk::types::{
    CopilotChatChoice, CopilotChatChoiceMessage, CopilotChatMessage, CopilotChatRequest,
    CopilotChatResponse, CopilotFunctionCall, CopilotTool, CopilotToolCall, CopilotToolFunction,
    CopilotUsage, Reference, ReferenceType,
};
use abp_gemini_sdk::types::{
    Candidate, Content, FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration,
    GeminiTool, GenerateContentRequest, GenerateContentResponse, GenerationConfig,
    HarmBlockThreshold, HarmCategory, HarmProbability, Part, SafetyRating, SafetySetting,
    ToolConfig, UsageMetadata,
};
use abp_kimi_sdk::types::{
    ChatMessage as KimiMessage, Choice as KimiChoice, ChoiceMessage as KimiChoiceMessage,
    FunctionCall as KimiFunctionCall, FunctionDef as KimiFunctionDef, KimiChatRequest,
    KimiChatResponse, KimiUsage, SearchMode, SearchOptions, Tool as KimiTool,
    ToolCall as KimiToolCall,
};
use abp_openai_sdk::api::{
    AssistantMessage, ChatCompletionRequest, ChatCompletionResponse, Choice, FinishReason,
    FunctionCall, FunctionDefinition, Message as OaiMessage, StreamOptions, Tool as OaiTool,
    ToolCall as OaiToolCall, Usage as OaiUsage,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 1, 10, 0, 0).unwrap()
}

fn ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 1, 10, 5, 0).unwrap()
}

fn uid1() -> Uuid {
    Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap()
}

fn uid2() -> Uuid {
    Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: ts(),
        kind,
        ext: None,
    }
}

// ===========================================================================
// 1. Receipt snapshots
// ===========================================================================

#[test]
fn exp_receipt_canonical_minimal() {
    let r = Receipt {
        meta: RunMetadata {
            run_id: uid1(),
            work_order_id: uid2(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts(),
            finished_at: ts2(),
            duration_ms: 300_000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    assert_json_snapshot!(r);
}

#[test]
fn exp_receipt_canonical_full() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(
        Capability::ToolWrite,
        SupportLevel::Restricted {
            reason: "read-only mode".into(),
        },
    );

    let r = Receipt {
        meta: RunMetadata {
            run_id: uid1(),
            work_order_id: uid2(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts(),
            finished_at: ts2(),
            duration_ms: 300_000,
        },
        backend: BackendIdentity {
            id: "sidecar:claude".into(),
            backend_version: Some("2.0.0".into()),
            adapter_version: Some("0.3.0".into()),
        },
        capabilities: caps,
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"prompt_tokens": 2000, "completion_tokens": 1200}),
        usage: UsageNormalized {
            input_tokens: Some(2000),
            output_tokens: Some(1200),
            cache_read_tokens: Some(300),
            cache_write_tokens: Some(80),
            request_units: Some(1),
            estimated_cost_usd: Some(0.018),
        },
        trace: vec![
            make_event(AgentEventKind::RunStarted {
                message: "begin".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "end".into(),
            }),
        ],
        artifacts: vec![
            ArtifactRef {
                kind: "patch".into(),
                path: "output/changes.patch".into(),
            },
            ArtifactRef {
                kind: "log".into(),
                path: "output/run.log".into(),
            },
        ],
        verification: VerificationReport {
            git_diff: Some("+new line\n-old line".into()),
            git_status: Some("M src/lib.rs\nA src/new.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    // Receipt with CapabilityManifest has non-string map keys.
    let pretty = serde_json::to_string_pretty(&r).unwrap();
    assert_snapshot!(pretty);
}

#[test]
fn exp_receipt_with_hash() {
    let r = Receipt {
        meta: RunMetadata {
            run_id: uid1(),
            work_order_id: uid2(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts(),
            finished_at: ts2(),
            duration_ms: 1000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Passthrough,
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
    .with_hash()
    .unwrap();
    assert_json_snapshot!(r);
}

#[test]
fn exp_receipt_outcome_partial() {
    let r = Receipt {
        meta: RunMetadata {
            run_id: uid1(),
            work_order_id: uid2(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts(),
            finished_at: ts2(),
            duration_ms: 500,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Partial,
        receipt_sha256: None,
    };
    assert_json_snapshot!(r);
}

#[test]
fn exp_receipt_outcome_failed() {
    let r = Receipt {
        meta: RunMetadata {
            run_id: uid1(),
            work_order_id: uid2(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts(),
            finished_at: ts2(),
            duration_ms: 100,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Failed,
        receipt_sha256: None,
    };
    assert_json_snapshot!(r);
}

// ===========================================================================
// 2. WorkOrder snapshots
// ===========================================================================

#[test]
fn exp_work_order_all_fields() {
    let wo = WorkOrder {
        id: uid1(),
        task: "Implement caching layer with TTL support".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/home/dev/project".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**/*.rs".into(), "Cargo.toml".into()],
            exclude: vec!["target/**".into(), ".git/**".into()],
        },
        context: ContextPacket {
            files: vec!["src/cache.rs".into(), "README.md".into()],
            snippets: vec![
                ContextSnippet {
                    name: "architecture".into(),
                    content: "Use LRU eviction".into(),
                },
                ContextSnippet {
                    name: "constraints".into(),
                    content: "Max 100MB memory".into(),
                },
            ],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["Read".into(), "Write".into(), "Glob".into()],
            disallowed_tools: vec!["WebSearch".into()],
            deny_read: vec!["**/.env".into(), "**/secrets/**".into()],
            deny_write: vec!["Cargo.lock".into()],
            allow_network: vec!["crates.io".into()],
            deny_network: vec!["*.evil.com".into()],
            require_approval_for: vec!["Bash".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolEdit,
                    min_support: MinSupport::Emulated,
                },
            ],
        },
        config: RuntimeConfig {
            model: Some("claude-sonnet-4-20250514".into()),
            vendor: BTreeMap::from([
                ("temperature".into(), json!(0.7)),
                ("abp".into(), json!({"mode": "mapped"})),
            ]),
            env: BTreeMap::from([("RUST_LOG".into(), "debug".into())]),
            max_budget_usd: Some(10.0),
            max_turns: Some(50),
        },
    };
    assert_json_snapshot!(wo);
}

#[test]
fn exp_work_order_minimal() {
    let wo = WorkOrder {
        id: uid1(),
        task: "hello".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    assert_json_snapshot!(wo);
}

// ===========================================================================
// 3. AgentEvent snapshots (each variant)
// ===========================================================================

#[test]
fn exp_event_run_started() {
    assert_json_snapshot!(make_event(AgentEventKind::RunStarted {
        message: "Initializing workspace".into(),
    }));
}

#[test]
fn exp_event_run_completed() {
    assert_json_snapshot!(make_event(AgentEventKind::RunCompleted {
        message: "All tasks finished".into(),
    }));
}

#[test]
fn exp_event_assistant_delta() {
    assert_json_snapshot!(make_event(AgentEventKind::AssistantDelta {
        text: "I'll start by ".into(),
    }));
}

#[test]
fn exp_event_assistant_message() {
    assert_json_snapshot!(make_event(AgentEventKind::AssistantMessage {
        text: "I've completed the refactoring of the cache module.".into(),
    }));
}

#[test]
fn exp_event_tool_call() {
    assert_json_snapshot!(make_event(AgentEventKind::ToolCall {
        tool_name: "write_file".into(),
        tool_use_id: Some("tu_abc123".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/cache.rs", "content": "fn new() {}"}),
    }));
}

#[test]
fn exp_event_tool_call_nested() {
    assert_json_snapshot!(make_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("tu_child".into()),
        parent_tool_use_id: Some("tu_parent".into()),
        input: json!({"command": "cargo test"}),
    }));
}

#[test]
fn exp_event_tool_result_success() {
    assert_json_snapshot!(make_event(AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu_read1".into()),
        output: json!({"content": "file contents here"}),
        is_error: false,
    }));
}

#[test]
fn exp_event_tool_result_error() {
    assert_json_snapshot!(make_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("tu_bash1".into()),
        output: json!({"stderr": "permission denied"}),
        is_error: true,
    }));
}

#[test]
fn exp_event_file_changed() {
    assert_json_snapshot!(make_event(AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "Added caching module import".into(),
    }));
}

#[test]
fn exp_event_command_executed() {
    assert_json_snapshot!(make_event(AgentEventKind::CommandExecuted {
        command: "cargo test --lib".into(),
        exit_code: Some(0),
        output_preview: Some("test result: ok. 42 passed".into()),
    }));
}

#[test]
fn exp_event_command_executed_failed() {
    assert_json_snapshot!(make_event(AgentEventKind::CommandExecuted {
        command: "cargo build".into(),
        exit_code: Some(101),
        output_preview: Some("error[E0308]: mismatched types".into()),
    }));
}

#[test]
fn exp_event_warning() {
    assert_json_snapshot!(make_event(AgentEventKind::Warning {
        message: "Approaching budget limit".into(),
    }));
}

#[test]
fn exp_event_error() {
    assert_json_snapshot!(make_event(AgentEventKind::Error {
        message: "Backend timed out".into(),
        error_code: Some(ErrorCode::BackendTimeout),
    }));
}

#[test]
fn exp_event_error_no_code() {
    assert_json_snapshot!(make_event(AgentEventKind::Error {
        message: "Unknown failure".into(),
        error_code: None,
    }));
}

#[test]
fn exp_event_with_ext() {
    let evt = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "done".into(),
        },
        ext: Some(BTreeMap::from([
            ("latency_ms".into(), json!(42)),
            ("model".into(), json!("gpt-4o")),
        ])),
    };
    assert_json_snapshot!(evt);
}

// ===========================================================================
// 4. Envelope JSONL snapshots (each variant)
// ===========================================================================

#[test]
fn exp_envelope_hello() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.2.0".into()),
        },
        capabilities: caps,
        mode: ExecutionMode::Mapped,
    };
    // Envelope with CapabilityManifest has non-string map keys.
    let pretty = serde_json::to_string_pretty(&env).unwrap();
    assert_snapshot!(pretty);
}

#[test]
fn exp_envelope_run() {
    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: WorkOrder {
            id: uid1(),
            task: "test task".into(),
            lane: ExecutionLane::PatchFirst,
            workspace: WorkspaceSpec {
                root: ".".into(),
                mode: WorkspaceMode::PassThrough,
                include: vec![],
                exclude: vec![],
            },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig::default(),
        },
    };
    assert_json_snapshot!(env);
}

#[test]
fn exp_envelope_event() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: make_event(AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        }),
    };
    assert_json_snapshot!(env);
}

#[test]
fn exp_envelope_final() {
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: Receipt {
            meta: RunMetadata {
                run_id: uid1(),
                work_order_id: uid2(),
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: ts(),
                finished_at: ts2(),
                duration_ms: 1000,
            },
            backend: BackendIdentity {
                id: "mock".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        },
    };
    assert_json_snapshot!(env);
}

#[test]
fn exp_envelope_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "Sidecar process crashed".into(),
        error_code: Some(ErrorCode::BackendCrashed),
    };
    assert_json_snapshot!(env);
}

#[test]
fn exp_envelope_fatal_no_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "Handshake failed".into(),
        error_code: Some(ErrorCode::ProtocolHandshakeFailed),
    };
    assert_json_snapshot!(env);
}

// ===========================================================================
// 5. ErrorInfo snapshots
// ===========================================================================

#[test]
fn exp_error_info_basic() {
    let e = ErrorInfo {
        code: ErrorCode::BackendTimeout,
        message: "Request timed out after 30s".into(),
        details: BTreeMap::new(),
        is_retryable: true,
    };
    assert_json_snapshot!(e);
}

#[test]
fn exp_error_info_with_details() {
    let e = ErrorInfo {
        code: ErrorCode::PolicyDenied,
        message: "Tool 'bash' is not allowed".into(),
        details: BTreeMap::from([
            ("tool".into(), json!("bash")),
            ("policy_rule".into(), json!("disallowed_tools")),
        ]),
        is_retryable: false,
    };
    assert_json_snapshot!(e);
}

#[test]
fn exp_error_info_protocol() {
    let e = ErrorInfo {
        code: ErrorCode::ProtocolInvalidEnvelope,
        message: "Missing 't' discriminator field".into(),
        details: BTreeMap::from([("raw_line".into(), json!("{\"invalid\": true}"))]),
        is_retryable: false,
    };
    assert_json_snapshot!(e);
}

#[test]
fn exp_error_info_contract_mismatch() {
    let e = ErrorInfo {
        code: ErrorCode::ContractVersionMismatch,
        message: "Expected abp/v0.1, got abp/v0.2".into(),
        details: BTreeMap::from([
            ("expected".into(), json!("abp/v0.1")),
            ("actual".into(), json!("abp/v0.2")),
        ]),
        is_retryable: false,
    };
    assert_json_snapshot!(e);
}

#[test]
fn exp_error_info_capability_unsupported() {
    let e = ErrorInfo {
        code: ErrorCode::CapabilityUnsupported,
        message: "Vision capability not available".into(),
        details: BTreeMap::from([("capability".into(), json!("vision"))]),
        is_retryable: false,
    };
    assert_json_snapshot!(e);
}

// ===========================================================================
// 6. PolicyProfile snapshots
// ===========================================================================

#[test]
fn exp_policy_profile_default() {
    assert_json_snapshot!(PolicyProfile::default());
}

#[test]
fn exp_policy_profile_full() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Glob".into(), "Grep".into()],
        disallowed_tools: vec!["WebSearch".into(), "Bash".into()],
        deny_read: vec!["**/.env".into(), "**/secrets/**".into()],
        deny_write: vec!["Cargo.lock".into(), "**/.git/**".into()],
        allow_network: vec!["crates.io".into(), "api.github.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["Bash".into(), "Write".into()],
    };
    assert_json_snapshot!(p);
}

// ===========================================================================
// 7. CapabilityReport snapshots
// ===========================================================================

#[test]
fn exp_capability_report_full() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![
            CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            },
            CapabilityReportEntry {
                capability: Capability::ExtendedThinking,
                support: DialectSupportLevel::Unsupported {
                    reason: "OpenAI does not support extended thinking".into(),
                },
            },
            CapabilityReportEntry {
                capability: Capability::ToolBash,
                support: DialectSupportLevel::Emulated {
                    detail: "Mapped to function_call with bash wrapper".into(),
                },
            },
            CapabilityReportEntry {
                capability: Capability::CacheControl,
                support: DialectSupportLevel::Unsupported {
                    reason: "No prompt caching in OpenAI API".into(),
                },
            },
        ],
    };
    assert_json_snapshot!(report);
}

#[test]
fn exp_capability_report_empty() {
    let report = CapabilityReport {
        source_dialect: "openai".into(),
        target_dialect: "openai".into(),
        entries: vec![],
    };
    assert_json_snapshot!(report);
}

// ===========================================================================
// 8. OpenAI SDK request/response
// ===========================================================================

#[test]
fn exp_openai_request_minimal() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![OaiMessage::User {
            content: "Hello".into(),
        }],
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
    };
    assert_json_snapshot!(req);
}

#[test]
fn exp_openai_request_with_tools() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![
            OaiMessage::System {
                content: "You are a coding assistant.".into(),
            },
            OaiMessage::User {
                content: "Read the file src/lib.rs".into(),
            },
        ],
        temperature: Some(0.7),
        max_tokens: Some(4096),
        tools: Some(vec![OaiTool {
            tool_type: "function".into(),
            function: FunctionDefinition {
                name: "read_file".into(),
                description: Some("Read a file from disk".into()),
                parameters: Some(
                    json!({"type": "object", "properties": {"path": {"type": "string"}}}),
                ),
                strict: Some(true),
            },
        }]),
        tool_choice: None,
        stream: Some(true),
        stream_options: Some(StreamOptions {
            include_usage: Some(true),
        }),
        top_p: Some(0.9),
        frequency_penalty: Some(0.1),
        presence_penalty: Some(0.1),
        stop: Some(vec!["###".into()]),
        n: Some(1),
        seed: Some(42),
        response_format: None,
        user: Some("user-123".into()),
        parallel_tool_calls: Some(true),
        service_tier: Some("auto".into()),
    };
    assert_json_snapshot!(req);
}

#[test]
fn exp_openai_response() {
    let resp = ChatCompletionResponse {
        id: "chatcmpl-test123".into(),
        object: "chat.completion".into(),
        created: 1717200000,
        model: "gpt-4o-2024-05-13".into(),
        choices: vec![Choice {
            index: 0,
            message: AssistantMessage {
                role: "assistant".into(),
                content: Some("Hello! How can I help?".into()),
                tool_calls: None,
            },
            finish_reason: FinishReason::Stop,
        }],
        usage: Some(OaiUsage {
            prompt_tokens: 10,
            completion_tokens: 8,
            total_tokens: 18,
            completion_tokens_details: None,
            prompt_tokens_details: None,
        }),
        system_fingerprint: Some("fp_abc123".into()),
    };
    assert_json_snapshot!(resp);
}

#[test]
fn exp_openai_response_tool_calls() {
    let resp = ChatCompletionResponse {
        id: "chatcmpl-tools".into(),
        object: "chat.completion".into(),
        created: 1717200000,
        model: "gpt-4o".into(),
        choices: vec![Choice {
            index: 0,
            message: AssistantMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OaiToolCall {
                    id: "call_abc".into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: "read_file".into(),
                        arguments: r#"{"path":"src/lib.rs"}"#.into(),
                    },
                }]),
            },
            finish_reason: FinishReason::ToolCalls,
        }],
        usage: Some(OaiUsage {
            prompt_tokens: 50,
            completion_tokens: 20,
            total_tokens: 70,
            completion_tokens_details: None,
            prompt_tokens_details: None,
        }),
        system_fingerprint: None,
    };
    assert_json_snapshot!(resp);
}

#[test]
fn exp_openai_message_variants() {
    let msgs: Vec<OaiMessage> = vec![
        OaiMessage::System {
            content: "You are helpful.".into(),
        },
        OaiMessage::User {
            content: "Hi".into(),
        },
        OaiMessage::Assistant {
            content: Some("Hello!".into()),
            tool_calls: None,
        },
        OaiMessage::Tool {
            tool_call_id: "call_1".into(),
            content: "file contents".into(),
        },
    ];
    assert_json_snapshot!(msgs);
}

// ===========================================================================
// 9. Claude SDK request/response
// ===========================================================================

#[test]
fn exp_claude_request_minimal() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: Role::User,
            content: MessageContent::Text("Hello Claude".into()),
        }],
        max_tokens: 1024,
        system: None,
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        tool_choice: None,
        thinking: None,
    };
    assert_json_snapshot!(req);
}

#[test]
fn exp_claude_request_full() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![
            ClaudeMessage {
                role: Role::User,
                content: MessageContent::Text("Refactor auth module".into()),
            },
            ClaudeMessage {
                role: Role::Assistant,
                content: MessageContent::Blocks(vec![ContentBlock::Text {
                    text: "I'll help with that.".into(),
                }]),
            },
        ],
        max_tokens: 8192,
        system: Some(SystemMessage::Text("You are a Rust expert.".into())),
        tools: None,
        metadata: Some(Metadata {
            user_id: Some("user-456".into()),
        }),
        stream: Some(true),
        stop_sequences: Some(vec!["---".into()]),
        temperature: Some(0.5),
        top_p: Some(0.95),
        top_k: Some(40),
        tool_choice: None,
        thinking: None,
    };
    assert_json_snapshot!(req);
}

#[test]
fn exp_claude_response() {
    let resp = MessagesResponse {
        id: "msg_01test".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![ContentBlock::Text {
            text: "Here's the refactored code.".into(),
        }],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: Some("end_turn".into()),
        stop_sequence: None,
        usage: ClaudeUsage {
            input_tokens: 500,
            output_tokens: 200,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    };
    assert_json_snapshot!(resp);
}

#[test]
fn exp_claude_response_tool_use() {
    let resp = MessagesResponse {
        id: "msg_02tooluse".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![ContentBlock::ToolUse {
            id: "toolu_01abc".into(),
            name: "read_file".into(),
            input: json!({"path": "src/lib.rs"}),
        }],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: Some("tool_use".into()),
        stop_sequence: None,
        usage: ClaudeUsage {
            input_tokens: 300,
            output_tokens: 50,
            cache_creation_input_tokens: Some(100),
            cache_read_input_tokens: Some(200),
        },
    };
    assert_json_snapshot!(resp);
}

// ===========================================================================
// 10. Gemini SDK request/response
// ===========================================================================

#[test]
fn exp_gemini_request_minimal() {
    let req = GenerateContentRequest {
        contents: vec![Content {
            role: Some("user".into()),
            parts: vec![Part::Text("Hello Gemini".into())],
        }],
        system_instruction: None,
        tools: None,
        tool_config: None,
        generation_config: None,
        safety_settings: None,
    };
    assert_json_snapshot!(req);
}

#[test]
fn exp_gemini_request_full() {
    let req = GenerateContentRequest {
        contents: vec![Content {
            role: Some("user".into()),
            parts: vec![Part::Text("Analyze this image".into())],
        }],
        system_instruction: Some(Content {
            role: None,
            parts: vec![Part::Text("You are an image analyst.".into())],
        }),
        tools: Some(vec![GeminiTool {
            function_declarations: vec![FunctionDeclaration {
                name: "get_weather".into(),
                description: "Get current weather".into(),
                parameters: json!({"type": "object", "properties": {"location": {"type": "string"}}}),
            }],
        }]),
        tool_config: Some(ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        }),
        generation_config: Some(GenerationConfig {
            temperature: Some(0.8),
            top_p: Some(0.95),
            top_k: Some(40),
            max_output_tokens: Some(2048),
            candidate_count: Some(1),
            stop_sequences: None,
        }),
        safety_settings: Some(vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        }]),
    };
    assert_json_snapshot!(req);
}

#[test]
fn exp_gemini_response() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content {
                role: Some("model".into()),
                parts: vec![Part::Text("Hello! I'm Gemini.".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: Some(vec![SafetyRating {
                category: HarmCategory::HarmCategoryHarassment,
                probability: HarmProbability::Negligible,
            }]),
        }],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 8,
            total_token_count: 18,
        }),
        prompt_feedback: None,
    };
    assert_json_snapshot!(resp);
}

#[test]
fn exp_gemini_function_call_response() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content {
                role: Some("model".into()),
                parts: vec![Part::FunctionCall {
                    name: "get_weather".into(),
                    args: json!({"location": "Seattle"}),
                }],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
        }],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 20,
            candidates_token_count: 15,
            total_token_count: 35,
        }),
        prompt_feedback: None,
    };
    assert_json_snapshot!(resp);
}

#[test]
fn exp_gemini_part_variants() {
    let parts: Vec<Part> = vec![
        Part::Text("plain text".into()),
        Part::FunctionCall {
            name: "search".into(),
            args: json!({"query": "rust"}),
        },
        Part::FunctionResponse {
            name: "search".into(),
            response: json!({"results": ["crate1", "crate2"]}),
        },
    ];
    assert_json_snapshot!(parts);
}

// ===========================================================================
// 11. Copilot SDK request/response
// ===========================================================================

#[test]
fn exp_copilot_request() {
    let req = CopilotChatRequest {
        model: "gpt-4o".into(),
        messages: vec![
            CopilotChatMessage {
                role: "system".into(),
                content: Some("You are GitHub Copilot.".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            CopilotChatMessage {
                role: "user".into(),
                content: Some("Fix the bug in auth.rs".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        temperature: Some(0.3),
        top_p: None,
        max_tokens: Some(4096),
        stream: Some(true),
        tools: Some(vec![CopilotTool {
            tool_type: "function".into(),
            function: CopilotToolFunction {
                name: "read_file".into(),
                description: "Read file contents".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            },
        }]),
        tool_choice: None,
        intent: Some("code-review".into()),
        references: Some(vec![Reference {
            ref_type: ReferenceType::File,
            id: "ref-1".into(),
            uri: Some("file:///src/auth.rs".into()),
            content: Some("fn login() {}".into()),
            metadata: None,
        }]),
    };
    assert_json_snapshot!(req);
}

#[test]
fn exp_copilot_response() {
    let resp = CopilotChatResponse {
        id: "chatcmpl-copilot1".into(),
        object: "chat.completion".into(),
        created: 1717200000,
        model: "gpt-4o".into(),
        choices: vec![CopilotChatChoice {
            index: 0,
            message: CopilotChatChoiceMessage {
                role: "assistant".into(),
                content: Some("I found the bug in auth.rs.".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(CopilotUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            copilot_tokens: Some(25),
        }),
    };
    assert_json_snapshot!(resp);
}

#[test]
fn exp_copilot_reference_variants() {
    let refs = vec![
        Reference {
            ref_type: ReferenceType::File,
            id: "f1".into(),
            uri: Some("file:///main.rs".into()),
            content: None,
            metadata: None,
        },
        Reference {
            ref_type: ReferenceType::Selection,
            id: "s1".into(),
            uri: Some("file:///lib.rs".into()),
            content: Some("fn process()".into()),
            metadata: Some(BTreeMap::from([
                ("startLine".into(), json!(10)),
                ("endLine".into(), json!(20)),
            ])),
        },
        Reference {
            ref_type: ReferenceType::Terminal,
            id: "t1".into(),
            uri: None,
            content: Some("$ cargo build\nerror[E0308]".into()),
            metadata: None,
        },
        Reference {
            ref_type: ReferenceType::GitDiff,
            id: "d1".into(),
            uri: None,
            content: Some("+new line\n-old line".into()),
            metadata: None,
        },
    ];
    assert_json_snapshot!(refs);
}

// ===========================================================================
// 12. Codex SDK request/response
// ===========================================================================

#[test]
fn exp_codex_request() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        messages: vec![
            CodexMessage::System {
                content: "You are Codex.".into(),
            },
            CodexMessage::User {
                content: "Add error handling".into(),
            },
        ],
        instructions: Some("Follow Rust conventions".into()),
        temperature: Some(0.2),
        top_p: None,
        max_tokens: Some(8192),
        stream: Some(false),
        tools: Some(vec![CodexTool {
            tool_type: "function".into(),
            function: CodexFunctionDef {
                name: "write_file".into(),
                description: "Write file to disk".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
            },
        }]),
        tool_choice: None,
    };
    assert_json_snapshot!(req);
}

#[test]
fn exp_codex_response() {
    let resp = CodexResponse {
        id: "codex-resp-1".into(),
        object: "chat.completion".into(),
        created: 1717200000,
        model: "codex-mini-latest".into(),
        choices: vec![CodexChoice {
            index: 0,
            message: CodexChoiceMessage {
                role: "assistant".into(),
                content: Some("I've added error handling.".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(CodexUsage {
            prompt_tokens: 80,
            completion_tokens: 40,
            total_tokens: 120,
        }),
    };
    assert_json_snapshot!(resp);
}

#[test]
fn exp_codex_file_change_create() {
    let fc = CodexFileChange {
        path: "src/error.rs".into(),
        operation: FileOperation::Create,
        content: Some("pub struct AppError;".into()),
        diff: None,
    };
    assert_json_snapshot!(fc);
}

#[test]
fn exp_codex_file_change_patch() {
    let fc = CodexFileChange {
        path: "src/lib.rs".into(),
        operation: FileOperation::Patch,
        content: None,
        diff: Some("@@ -1,3 +1,4 @@\n+use crate::error::AppError;\n fn main() {}".into()),
    };
    assert_json_snapshot!(fc);
}

#[test]
fn exp_codex_command() {
    let cmd = CodexCommand {
        command: "cargo test --lib".into(),
        cwd: Some("src".into()),
        timeout_seconds: Some(60),
        stdout: Some("test result: ok. 5 passed".into()),
        stderr: None,
        exit_code: Some(0),
    };
    assert_json_snapshot!(cmd);
}

#[test]
fn exp_codex_message_variants() {
    let msgs: Vec<CodexMessage> = vec![
        CodexMessage::System {
            content: "system prompt".into(),
        },
        CodexMessage::User {
            content: "user message".into(),
        },
        CodexMessage::Assistant {
            content: Some("response".into()),
            tool_calls: Some(vec![CodexToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: CodexFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"main.rs"}"#.into(),
                },
            }]),
        },
        CodexMessage::Tool {
            content: "file contents".into(),
            tool_call_id: "call_1".into(),
        },
    ];
    assert_json_snapshot!(msgs);
}

// ===========================================================================
// 13. Kimi SDK request/response
// ===========================================================================

#[test]
fn exp_kimi_request() {
    let req = KimiChatRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![
            KimiMessage::System {
                content: "You are Kimi.".into(),
            },
            KimiMessage::User {
                content: "Search for Rust async patterns".into(),
            },
        ],
        temperature: Some(0.7),
        top_p: Some(0.9),
        max_tokens: Some(4096),
        stream: Some(true),
        tools: Some(vec![KimiTool {
            tool_type: "function".into(),
            function: KimiFunctionDef {
                name: "web_search".into(),
                description: "Search the web".into(),
                parameters: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
            },
        }]),
        tool_choice: None,
        use_search: Some(true),
        search_options: Some(SearchOptions {
            mode: SearchMode::Auto,
            result_count: Some(5),
        }),
    };
    assert_json_snapshot!(req);
}

#[test]
fn exp_kimi_response() {
    let resp = KimiChatResponse {
        id: "kimi-resp-1".into(),
        object: "chat.completion".into(),
        created: 1717200000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiChoiceMessage {
                role: "assistant".into(),
                content: Some("Here are async patterns in Rust.".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 60,
            completion_tokens: 100,
            total_tokens: 160,
            search_tokens: Some(30),
        }),
    };
    assert_json_snapshot!(resp);
}

#[test]
fn exp_kimi_message_variants() {
    let msgs: Vec<KimiMessage> = vec![
        KimiMessage::System {
            content: "system".into(),
        },
        KimiMessage::User {
            content: "user query".into(),
        },
        KimiMessage::Assistant {
            content: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "tc_1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "web_search".into(),
                    arguments: r#"{"query":"rust"}"#.into(),
                },
            }]),
        },
        KimiMessage::Tool {
            content: "search results".into(),
            tool_call_id: "tc_1".into(),
        },
    ];
    assert_json_snapshot!(msgs);
}

#[test]
fn exp_kimi_search_modes() {
    assert_json_snapshot!("search_auto", SearchMode::Auto);
    assert_json_snapshot!("search_always", SearchMode::Always);
    assert_json_snapshot!("search_never", SearchMode::Never);
}

// ===========================================================================
// 14. Capability & SupportLevel enums
// ===========================================================================

#[test]
fn exp_capability_manifest() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m.insert(Capability::Vision, SupportLevel::Unsupported);
    m.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    // CapabilityManifest has non-string keys, so use text snapshot via serde_json.
    let pretty = serde_json::to_string_pretty(&m).unwrap();
    assert_snapshot!(pretty);
}

#[test]
fn exp_execution_modes() {
    assert_json_snapshot!("exec_passthrough", ExecutionMode::Passthrough);
    assert_json_snapshot!("exec_mapped", ExecutionMode::Mapped);
}

#[test]
fn exp_outcome_variants() {
    assert_json_snapshot!("outcome_complete", Outcome::Complete);
    assert_json_snapshot!("outcome_partial", Outcome::Partial);
    assert_json_snapshot!("outcome_failed", Outcome::Failed);
}

// ===========================================================================
// 15. JSONL codec text output
// ===========================================================================

#[test]
fn exp_jsonl_codec_encode_hello() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: caps,
        mode: ExecutionMode::Mapped,
    };
    let line = serde_json::to_string(&env).unwrap();
    assert_snapshot!(line);
}

#[test]
fn exp_jsonl_codec_encode_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("run-x".into()),
        error: "boom".into(),
        error_code: Some(ErrorCode::Internal),
    };
    let line = serde_json::to_string(&env).unwrap();
    assert_snapshot!(line);
}

// ===========================================================================
// 16. Additional edge cases
// ===========================================================================

#[test]
fn exp_error_code_variants_sample() {
    let codes = vec![
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendTimeout,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::PolicyDenied,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::Internal,
        ErrorCode::DialectUnknown,
        ErrorCode::IrLoweringFailed,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ConfigInvalid,
    ];
    assert_json_snapshot!(codes);
}

#[test]
fn exp_workspace_spec_variants() {
    let staged = WorkspaceSpec {
        root: "/project".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["**/*.rs".into()],
        exclude: vec!["target/**".into()],
    };
    let passthrough = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    assert_json_snapshot!("ws_staged", staged);
    assert_json_snapshot!("ws_passthrough", passthrough);
}

#[test]
fn exp_context_packet_with_snippets() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![
            ContextSnippet {
                name: "instructions".into(),
                content: "Use idiomatic Rust".into(),
            },
            ContextSnippet {
                name: "constraints".into(),
                content: "No unsafe code".into(),
            },
        ],
    };
    assert_json_snapshot!(ctx);
}

#[test]
fn exp_usage_normalized_all_fields() {
    let u = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: Some(200),
        cache_write_tokens: Some(50),
        request_units: Some(2),
        estimated_cost_usd: Some(0.025),
    };
    assert_json_snapshot!(u);
}

#[test]
fn exp_verification_report_full() {
    let v = VerificationReport {
        git_diff: Some("+new\n-old".into()),
        git_status: Some("M file.rs\nA new.rs\nD old.rs".into()),
        harness_ok: true,
    };
    assert_json_snapshot!(v);
}

#[test]
fn exp_backend_identity_full() {
    let b = BackendIdentity {
        id: "sidecar:gemini".into(),
        backend_version: Some("1.5-pro".into()),
        adapter_version: Some("0.4.0".into()),
    };
    assert_json_snapshot!(b);
}
