// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec)]
//! Golden-master snapshot test suite for ABP contract types.
//!
//! Categories:
//!  1. WorkOrder snapshots
//!  2. Receipt snapshots (success, failure, with events)
//!  3. AgentEvent snapshots (every variant)
//!  4. Envelope snapshots (every protocol variant)
//!  5. CapabilityManifest snapshots
//!  6. ErrorCode / error-response snapshots
//!  7. PolicyProfile snapshots
//!  8. BackplaneConfig snapshots
//!  9. IR type snapshots (IrMessage, IrContent, IrUsage)
//! 10. Cross-format snapshots (JSON vs TOML)

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_config::{BackendEntry, BackplaneConfig};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_error::{AbpError, AbpErrorDto, ErrorCode};
use abp_protocol::Envelope;

// ===========================================================================
// Helpers
// ===========================================================================

fn ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 8, 1, 8, 0, 0).unwrap()
}

fn ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 8, 1, 8, 5, 0).unwrap()
}

fn uid1() -> Uuid {
    Uuid::parse_str("cccccccc-dddd-4eee-8fff-aaaaaaaaaaaa").unwrap()
}

fn uid2() -> Uuid {
    Uuid::parse_str("22222222-3333-4444-8555-666666666666").unwrap()
}

fn backend_full() -> BackendIdentity {
    BackendIdentity {
        id: "sidecar:claude".into(),
        backend_version: Some("4.0.0".into()),
        adapter_version: Some("1.0.0".into()),
    }
}

fn backend_minimal() -> BackendIdentity {
    BackendIdentity {
        id: "mock".into(),
        backend_version: None,
        adapter_version: None,
    }
}

fn caps_streaming_only() -> CapabilityManifest {
    BTreeMap::from([(Capability::Streaming, SupportLevel::Native)])
}

fn caps_rich() -> CapabilityManifest {
    BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::ToolEdit, SupportLevel::Emulated),
        (Capability::ToolBash, SupportLevel::Emulated),
        (Capability::ToolUse, SupportLevel::Native),
        (Capability::ExtendedThinking, SupportLevel::Unsupported),
        (
            Capability::McpClient,
            SupportLevel::Restricted {
                reason: "beta feature".into(),
            },
        ),
    ])
}

fn make_receipt(outcome: Outcome, mode: ExecutionMode) -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: uid1(),
            work_order_id: uid2(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts(),
            finished_at: ts2(),
            duration_ms: 300_000,
        },
        backend: backend_full(),
        capabilities: caps_streaming_only(),
        mode,
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome,
        receipt_sha256: None,
    }
}

fn snap_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap()
}

// ===========================================================================
// 1. WorkOrder snapshots
// ===========================================================================

#[test]
fn gm_work_order_minimal() {
    let wo = WorkOrderBuilder::new("ping").build();
    insta::assert_json_snapshot!("gm_work_order_minimal", wo, {
        ".id" => "[uuid]"
    });
}

#[test]
fn gm_work_order_full() {
    let wo = WorkOrder {
        id: uid1(),
        task: "Implement authentication module".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/projects/app".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into(), "tests/**".into()],
            exclude: vec!["node_modules/**".into(), "target/**".into()],
        },
        context: ContextPacket {
            files: vec!["README.md".into(), "CONTRIBUTING.md".into()],
            snippets: vec![
                ContextSnippet {
                    name: "spec".into(),
                    content: "Use JWT-based auth".into(),
                },
                ContextSnippet {
                    name: "error_log".into(),
                    content: "auth module missing".into(),
                },
            ],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read".into(), "write".into(), "edit".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec![".env".into(), "secrets/**".into()],
            deny_write: vec!["Cargo.lock".into()],
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["*.evil.com".into()],
            require_approval_for: vec!["delete".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Emulated,
                },
            ],
        },
        config: RuntimeConfig {
            model: Some("claude-4-opus".into()),
            vendor: {
                let mut v = BTreeMap::new();
                v.insert("temperature".into(), json!(0.3));
                v.insert("max_tokens".into(), json!(8192));
                v
            },
            env: {
                let mut e = BTreeMap::new();
                e.insert("RUST_LOG".into(), "debug".into());
                e
            },
            max_budget_usd: Some(5.0),
            max_turns: Some(50),
        },
    };
    insta::assert_json_snapshot!("gm_work_order_full", wo);
}

#[test]
fn gm_work_order_workspace_first_passthrough() {
    let wo = WorkOrder {
        id: uid1(),
        task: "Observe only".into(),
        lane: ExecutionLane::WorkspaceFirst,
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
    insta::assert_json_snapshot!("gm_work_order_workspace_first_passthrough", wo);
}

#[test]
fn gm_work_order_builder_with_options() {
    let wo = WorkOrderBuilder::new("build task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/home/dev/project")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("gpt-4o")
        .max_turns(30)
        .max_budget_usd(2.5)
        .build();
    insta::assert_json_snapshot!("gm_work_order_builder_with_options", wo, {
        ".id" => "[uuid]"
    });
}

#[test]
fn gm_work_order_empty_context() {
    let wo = WorkOrder {
        id: uid1(),
        task: "No context".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket {
            files: vec![],
            snippets: vec![],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: None,
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: None,
            max_turns: None,
        },
    };
    insta::assert_json_snapshot!("gm_work_order_empty_context", wo);
}

#[test]
fn gm_work_order_many_requirements() {
    let wo = WorkOrder {
        id: uid1(),
        task: "demanding task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::ToolBash,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ExtendedThinking,
                    min_support: MinSupport::Emulated,
                },
                CapabilityRequirement {
                    capability: Capability::ImageInput,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::McpClient,
                    min_support: MinSupport::Emulated,
                },
            ],
        },
        config: RuntimeConfig::default(),
    };
    insta::assert_json_snapshot!("gm_work_order_many_requirements", wo);
}

// ===========================================================================
// 2. Receipt snapshots
// ===========================================================================

#[test]
fn gm_receipt_success_minimal() {
    let r = make_receipt(Outcome::Complete, ExecutionMode::Mapped);
    insta::assert_snapshot!("gm_receipt_success_minimal", snap_json(&r));
}

#[test]
fn gm_receipt_failed() {
    let r = make_receipt(Outcome::Failed, ExecutionMode::Mapped);
    insta::assert_snapshot!("gm_receipt_failed", snap_json(&r));
}

#[test]
fn gm_receipt_partial() {
    let r = make_receipt(Outcome::Partial, ExecutionMode::Mapped);
    insta::assert_snapshot!("gm_receipt_partial", snap_json(&r));
}

#[test]
fn gm_receipt_passthrough_mode() {
    let r = make_receipt(Outcome::Complete, ExecutionMode::Passthrough);
    insta::assert_snapshot!("gm_receipt_passthrough_mode", snap_json(&r));
}

#[test]
fn gm_receipt_with_trace_events() {
    let mut r = make_receipt(Outcome::Complete, ExecutionMode::Mapped);
    r.trace = vec![
        AgentEvent {
            ts: ts(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Working on it".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: ts2(),
            kind: AgentEventKind::RunCompleted {
                message: "finished".into(),
            },
            ext: None,
        },
    ];
    insta::assert_snapshot!("gm_receipt_with_trace_events", snap_json(&r));
}

#[test]
fn gm_receipt_with_artifacts() {
    let mut r = make_receipt(Outcome::Complete, ExecutionMode::Mapped);
    r.artifacts = vec![
        ArtifactRef {
            kind: "patch".into(),
            path: "changes.patch".into(),
        },
        ArtifactRef {
            kind: "log".into(),
            path: "execution.log".into(),
        },
        ArtifactRef {
            kind: "diff".into(),
            path: "staged.diff".into(),
        },
    ];
    insta::assert_snapshot!("gm_receipt_with_artifacts", snap_json(&r));
}

#[test]
fn gm_receipt_full_usage() {
    let mut r = make_receipt(Outcome::Complete, ExecutionMode::Mapped);
    r.usage = UsageNormalized {
        input_tokens: Some(2500),
        output_tokens: Some(1200),
        cache_read_tokens: Some(300),
        cache_write_tokens: Some(100),
        request_units: Some(4),
        estimated_cost_usd: Some(0.035),
    };
    r.usage_raw = json!({
        "prompt_tokens": 2500,
        "completion_tokens": 1200,
        "cached_tokens": 300,
    });
    insta::assert_snapshot!("gm_receipt_full_usage", snap_json(&r));
}

#[test]
fn gm_receipt_with_verification() {
    let mut r = make_receipt(Outcome::Complete, ExecutionMode::Mapped);
    r.verification = VerificationReport {
        git_diff: Some("diff --git a/src/auth.rs b/src/auth.rs\n+pub fn login()".into()),
        git_status: Some("M src/auth.rs\nA src/jwt.rs".into()),
        harness_ok: true,
    };
    insta::assert_snapshot!("gm_receipt_with_verification", snap_json(&r));
}

#[test]
fn gm_receipt_with_hash() {
    let r = make_receipt(Outcome::Complete, ExecutionMode::Mapped)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    insta::assert_snapshot!("gm_receipt_with_hash", snap_json(&r));
}

#[test]
fn gm_receipt_rich_capabilities() {
    let mut r = make_receipt(Outcome::Complete, ExecutionMode::Mapped);
    r.capabilities = caps_rich();
    insta::assert_snapshot!("gm_receipt_rich_capabilities", snap_json(&r));
}

// ===========================================================================
// 3. AgentEvent snapshots — every variant
// ===========================================================================

#[test]
fn gm_event_run_started() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunStarted {
            message: "execution begin".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_run_started", e);
}

#[test]
fn gm_event_run_completed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunCompleted {
            message: "execution end".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_run_completed", e);
}

#[test]
fn gm_event_assistant_delta() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantDelta { text: "Hel".into() },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_assistant_delta", e);
}

#[test]
fn gm_event_assistant_message() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "I have completed the requested changes.".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_assistant_message", e);
}

#[test]
fn gm_event_tool_call_with_ids() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_gm_001".into()),
            parent_tool_use_id: Some("tu_gm_000".into()),
            input: json!({"path": "src/lib.rs"}),
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_tool_call_with_ids", e);
}

#[test]
fn gm_event_tool_call_no_parent() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu_gm_010".into()),
            parent_tool_use_id: None,
            input: json!({"command": "cargo test"}),
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_tool_call_no_parent", e);
}

#[test]
fn gm_event_tool_result_success() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_gm_001".into()),
            output: json!({"content": "fn main() { println!(\"hello\"); }"}),
            is_error: false,
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_tool_result_success", e);
}

#[test]
fn gm_event_tool_result_error() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "write_file".into(),
            tool_use_id: Some("tu_gm_002".into()),
            output: json!({"error": "permission denied: /etc/passwd"}),
            is_error: true,
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_tool_result_error", e);
}

#[test]
fn gm_event_file_changed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::FileChanged {
            path: "src/auth.rs".into(),
            summary: "Added JWT validation helper".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_file_changed", e);
}

#[test]
fn gm_event_command_executed_ok() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test --lib".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok. 15 passed; 0 failed".into()),
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_command_executed_ok", e);
}

#[test]
fn gm_event_command_executed_failed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(101),
            output_preview: Some("error[E0308]: mismatched types".into()),
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_command_executed_failed", e);
}

#[test]
fn gm_event_command_executed_no_output() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "sleep 10".into(),
            exit_code: None,
            output_preview: None,
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_command_executed_no_output", e);
}

#[test]
fn gm_event_warning() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Warning {
            message: "approaching budget limit".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_warning", e);
}

#[test]
fn gm_event_error_no_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Error {
            message: "unexpected internal error".into(),
            error_code: None,
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_error_no_code", e);
}

#[test]
fn gm_event_error_with_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Error {
            message: "backend timed out after 120s".into(),
            error_code: Some(ErrorCode::BackendTimeout),
        },
        ext: None,
    };
    insta::assert_json_snapshot!("gm_event_error_with_code", e);
}

#[test]
fn gm_event_with_ext_data() {
    let mut ext = BTreeMap::new();
    ext.insert("vendor_msg_id".into(), json!("msg_abc_123"));
    ext.insert(
        "raw_payload".into(),
        json!({"role": "assistant", "stop_reason": "end_turn"}),
    );
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "passthrough response".into(),
        },
        ext: Some(ext),
    };
    insta::assert_json_snapshot!("gm_event_with_ext_data", e);
}

// ===========================================================================
// 4. Envelope snapshots — all protocol variants
// ===========================================================================

#[test]
fn gm_envelope_hello_mapped() {
    let env = Envelope::hello(backend_full(), caps_streaming_only());
    insta::assert_snapshot!("gm_envelope_hello_mapped", snap_json(&env));
}

#[test]
fn gm_envelope_hello_passthrough() {
    let env = Envelope::hello_with_mode(backend_full(), caps_rich(), ExecutionMode::Passthrough);
    insta::assert_snapshot!("gm_envelope_hello_passthrough", snap_json(&env));
}

#[test]
fn gm_envelope_hello_empty_caps() {
    let env = Envelope::hello(backend_minimal(), BTreeMap::new());
    insta::assert_snapshot!("gm_envelope_hello_empty_caps", snap_json(&env));
}

#[test]
fn gm_envelope_run() {
    let wo = WorkOrder {
        id: uid1(),
        task: "envelope run test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    let env = Envelope::Run {
        id: "run-gm-001".into(),
        work_order: wo,
    };
    insta::assert_json_snapshot!("gm_envelope_run", env);
}

#[test]
fn gm_envelope_event() {
    let env = Envelope::Event {
        ref_id: "run-gm-001".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "streaming token".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!("gm_envelope_event", env);
}

#[test]
fn gm_envelope_final() {
    let env = Envelope::Final {
        ref_id: "run-gm-001".into(),
        receipt: make_receipt(Outcome::Complete, ExecutionMode::Mapped),
    };
    insta::assert_snapshot!("gm_envelope_final", snap_json(&env));
}

#[test]
fn gm_envelope_fatal_with_ref() {
    let env = Envelope::Fatal {
        ref_id: Some("run-gm-001".into()),
        error: "sidecar exited unexpectedly".into(),
        error_code: None,
    };
    insta::assert_json_snapshot!("gm_envelope_fatal_with_ref", env);
}

#[test]
fn gm_envelope_fatal_no_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "handshake timeout".into(),
        error_code: Some(ErrorCode::ProtocolHandshakeFailed),
    };
    insta::assert_json_snapshot!("gm_envelope_fatal_no_ref", env);
}

#[test]
fn gm_envelope_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-gm-002".into()),
        "model not found",
        ErrorCode::BackendModelNotFound,
    );
    insta::assert_json_snapshot!("gm_envelope_fatal_with_error_code", env);
}

// ===========================================================================
// 5. CapabilityManifest snapshots
// ===========================================================================

#[test]
fn gm_capability_manifest_empty() {
    let m: CapabilityManifest = BTreeMap::new();
    insta::assert_snapshot!("gm_capability_manifest_empty", snap_json(&m));
}

#[test]
fn gm_capability_manifest_streaming_only() {
    insta::assert_snapshot!(
        "gm_capability_manifest_streaming_only",
        snap_json(&caps_streaming_only())
    );
}

#[test]
fn gm_capability_manifest_rich() {
    insta::assert_snapshot!("gm_capability_manifest_rich", snap_json(&caps_rich()));
}

#[test]
fn gm_support_level_native() {
    insta::assert_json_snapshot!("gm_support_level_native", SupportLevel::Native);
}

#[test]
fn gm_support_level_emulated() {
    insta::assert_json_snapshot!("gm_support_level_emulated", SupportLevel::Emulated);
}

#[test]
fn gm_support_level_unsupported() {
    insta::assert_json_snapshot!("gm_support_level_unsupported", SupportLevel::Unsupported);
}

#[test]
fn gm_support_level_restricted() {
    insta::assert_json_snapshot!(
        "gm_support_level_restricted",
        SupportLevel::Restricted {
            reason: "experimental preview".into()
        }
    );
}

// ===========================================================================
// 6. ErrorCode / error-response snapshots
// ===========================================================================

#[test]
fn gm_error_code_backend_timeout() {
    insta::assert_json_snapshot!("gm_error_code_backend_timeout", ErrorCode::BackendTimeout);
}

#[test]
fn gm_error_code_policy_denied() {
    insta::assert_json_snapshot!("gm_error_code_policy_denied", ErrorCode::PolicyDenied);
}

#[test]
fn gm_error_code_protocol_invalid_envelope() {
    insta::assert_json_snapshot!(
        "gm_error_code_protocol_invalid_envelope",
        ErrorCode::ProtocolInvalidEnvelope
    );
}

#[test]
fn gm_error_code_internal() {
    insta::assert_json_snapshot!("gm_error_code_internal", ErrorCode::Internal);
}

#[test]
fn gm_error_code_receipt_hash_mismatch() {
    insta::assert_json_snapshot!(
        "gm_error_code_receipt_hash_mismatch",
        ErrorCode::ReceiptHashMismatch
    );
}

#[test]
fn gm_error_dto_minimal() {
    let dto = AbpErrorDto {
        code: ErrorCode::BackendTimeout,
        message: "timed out after 30s".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    insta::assert_json_snapshot!("gm_error_dto_minimal", dto);
}

#[test]
fn gm_error_dto_with_context() {
    let mut ctx = BTreeMap::new();
    ctx.insert("backend".into(), json!("sidecar:claude"));
    ctx.insert("timeout_ms".into(), json!(30000));
    let dto = AbpErrorDto {
        code: ErrorCode::BackendTimeout,
        message: "backend did not respond".into(),
        context: ctx,
        source_message: Some("connection reset by peer".into()),
    };
    insta::assert_json_snapshot!("gm_error_dto_with_context", dto);
}

#[test]
fn gm_error_dto_from_abp_error() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "tool bash is disallowed")
        .with_context("tool", "bash");
    let dto = AbpErrorDto::from(&err);
    insta::assert_json_snapshot!("gm_error_dto_from_abp_error", dto);
}

// ===========================================================================
// 7. PolicyProfile snapshots
// ===========================================================================

#[test]
fn gm_policy_profile_default() {
    insta::assert_json_snapshot!("gm_policy_profile_default", PolicyProfile::default());
}

#[test]
fn gm_policy_profile_full() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into(), "edit".into(), "glob".into()],
        disallowed_tools: vec!["bash".into(), "exec".into()],
        deny_read: vec![".env".into(), "secrets/**".into(), "*.pem".into()],
        deny_write: vec!["Cargo.lock".into(), "package-lock.json".into()],
        allow_network: vec!["api.github.com".into(), "registry.npmjs.org".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["delete".into(), "exec".into()],
    };
    insta::assert_json_snapshot!("gm_policy_profile_full", p);
}

#[test]
fn gm_policy_profile_deny_all_network() {
    let p = PolicyProfile {
        deny_network: vec!["*".into()],
        ..PolicyProfile::default()
    };
    insta::assert_json_snapshot!("gm_policy_profile_deny_all_network", p);
}

#[test]
fn gm_policy_profile_tools_only() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into(), "grep".into()],
        disallowed_tools: vec!["bash".into(), "write".into()],
        ..PolicyProfile::default()
    };
    insta::assert_json_snapshot!("gm_policy_profile_tools_only", p);
}

// ===========================================================================
// 8. BackplaneConfig snapshots
// ===========================================================================

#[test]
fn gm_config_default() {
    insta::assert_json_snapshot!("gm_config_default", BackplaneConfig::default());
}

#[test]
fn gm_config_full() {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    backends.insert(
        "claude".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["hosts/claude/index.js".into()],
            timeout_secs: Some(180),
        },
    );
    backends.insert(
        "openai".into(),
        BackendEntry::Sidecar {
            command: "python3".into(),
            args: vec!["hosts/python/main.py".into(), "--backend=openai".into()],
            timeout_secs: Some(120),
        },
    );
    let cfg = BackplaneConfig {
        default_backend: Some("claude".into()),
        workspace_dir: Some("/tmp/abp-workspaces".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/tmp/abp-receipts".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        policy_profiles: vec!["policies/default.toml".into()],
        backends,
    };
    insta::assert_json_snapshot!("gm_config_full", cfg);
}

#[test]
fn gm_config_backend_entry_mock() {
    insta::assert_json_snapshot!("gm_config_backend_entry_mock", BackendEntry::Mock {});
}

#[test]
fn gm_config_backend_entry_sidecar() {
    insta::assert_json_snapshot!(
        "gm_config_backend_entry_sidecar",
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["hosts/node/index.js".into(), "--debug".into()],
            timeout_secs: Some(90),
        }
    );
}

#[test]
fn gm_config_backend_entry_sidecar_no_timeout() {
    insta::assert_json_snapshot!(
        "gm_config_backend_entry_sidecar_no_timeout",
        BackendEntry::Sidecar {
            command: "python3".into(),
            args: vec![],
            timeout_secs: None,
        }
    );
}

// ===========================================================================
// 9. IR type snapshots
// ===========================================================================

#[test]
fn gm_ir_message_user_text() {
    let msg = IrMessage::text(IrRole::User, "Explain closures in Rust.");
    insta::assert_json_snapshot!("gm_ir_message_user_text", msg);
}

#[test]
fn gm_ir_message_system() {
    let msg = IrMessage::text(IrRole::System, "You are a Rust expert.");
    insta::assert_json_snapshot!("gm_ir_message_system", msg);
}

#[test]
fn gm_ir_message_assistant() {
    let msg = IrMessage::text(IrRole::Assistant, "Closures capture their environment.");
    insta::assert_json_snapshot!("gm_ir_message_assistant", msg);
}

#[test]
fn gm_ir_message_tool_role() {
    let msg = IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "tu_gm_100".into(),
            content: vec![IrContentBlock::Text {
                text: "file contents here".into(),
            }],
            is_error: false,
        }],
    );
    insta::assert_json_snapshot!("gm_ir_message_tool_role", msg);
}

#[test]
fn gm_ir_message_with_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("vendor_id".into(), json!("msg_xyz789"));
    meta.insert("model".into(), json!("claude-4-opus"));
    let msg = IrMessage {
        role: IrRole::Assistant,
        content: vec![IrContentBlock::Text {
            text: "response with metadata".into(),
        }],
        metadata: meta,
    };
    insta::assert_json_snapshot!("gm_ir_message_with_metadata", msg);
}

#[test]
fn gm_ir_message_multi_block() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "Let me analyze the code...".into(),
            },
            IrContentBlock::Text {
                text: "Found the issue in line 42.".into(),
            },
            IrContentBlock::ToolUse {
                id: "tu_gm_200".into(),
                name: "edit_file".into(),
                input: json!({"path": "src/main.rs", "new_content": "fixed code"}),
            },
        ],
    );
    insta::assert_json_snapshot!("gm_ir_message_multi_block", msg);
}

#[test]
fn gm_ir_content_text() {
    insta::assert_json_snapshot!(
        "gm_ir_content_text",
        IrContentBlock::Text {
            text: "plain text block".into()
        }
    );
}

#[test]
fn gm_ir_content_image() {
    insta::assert_json_snapshot!(
        "gm_ir_content_image",
        IrContentBlock::Image {
            media_type: "image/jpeg".into(),
            data: "L2ltYWdlLWRhdGE=".into(),
        }
    );
}

#[test]
fn gm_ir_content_tool_use() {
    insta::assert_json_snapshot!(
        "gm_ir_content_tool_use",
        IrContentBlock::ToolUse {
            id: "tu_gm_300".into(),
            name: "grep".into(),
            input: json!({"pattern": "TODO", "path": "src/"}),
        }
    );
}

#[test]
fn gm_ir_content_tool_result_ok() {
    insta::assert_json_snapshot!(
        "gm_ir_content_tool_result_ok",
        IrContentBlock::ToolResult {
            tool_use_id: "tu_gm_300".into(),
            content: vec![IrContentBlock::Text {
                text: "src/main.rs:10: // TODO: refactor".into(),
            }],
            is_error: false,
        }
    );
}

#[test]
fn gm_ir_content_tool_result_err() {
    insta::assert_json_snapshot!(
        "gm_ir_content_tool_result_err",
        IrContentBlock::ToolResult {
            tool_use_id: "tu_gm_301".into(),
            content: vec![IrContentBlock::Text {
                text: "directory not found".into(),
            }],
            is_error: true,
        }
    );
}

#[test]
fn gm_ir_content_thinking() {
    insta::assert_json_snapshot!(
        "gm_ir_content_thinking",
        IrContentBlock::Thinking {
            text: "Step 1: Read the file. Step 2: Find the bug.".into(),
        }
    );
}

#[test]
fn gm_ir_tool_definition() {
    let td = IrToolDefinition {
        name: "write_file".into(),
        description: "Write content to a file".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Destination path"},
                "content": {"type": "string", "description": "File content"},
            },
            "required": ["path", "content"],
        }),
    };
    insta::assert_json_snapshot!("gm_ir_tool_definition", td);
}

#[test]
fn gm_ir_tool_definition_empty_params() {
    let td = IrToolDefinition {
        name: "list_files".into(),
        description: "List all workspace files".into(),
        parameters: json!({"type": "object", "properties": {}}),
    };
    insta::assert_json_snapshot!("gm_ir_tool_definition_empty_params", td);
}

#[test]
fn gm_ir_conversation_empty() {
    insta::assert_json_snapshot!("gm_ir_conversation_empty", IrConversation::new());
}

#[test]
fn gm_ir_conversation_multi_turn() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are an expert coder."))
        .push(IrMessage::text(IrRole::User, "Fix the login bug."))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me look at the code.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tu_gm_400".into(),
                    name: "read_file".into(),
                    input: json!({"path": "src/auth.rs"}),
                },
            ],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_gm_400".into(),
                content: vec![IrContentBlock::Text {
                    text: "pub fn login() { todo!() }".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(
            IrRole::Assistant,
            "I see the issue. The login function is unimplemented.",
        ));
    insta::assert_json_snapshot!("gm_ir_conversation_multi_turn", conv);
}

#[test]
fn gm_ir_usage_default() {
    insta::assert_json_snapshot!("gm_ir_usage_default", IrUsage::default());
}

#[test]
fn gm_ir_usage_from_io() {
    insta::assert_json_snapshot!("gm_ir_usage_from_io", IrUsage::from_io(500, 250));
}

#[test]
fn gm_ir_usage_with_cache() {
    insta::assert_json_snapshot!(
        "gm_ir_usage_with_cache",
        IrUsage::with_cache(1000, 500, 200, 50)
    );
}

// ===========================================================================
// 10. Cross-format snapshots (JSON vs TOML)
// ===========================================================================

#[test]
fn gm_cross_format_policy_json() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec![".env".into()],
        deny_write: vec![],
        allow_network: vec!["*.github.com".into()],
        deny_network: vec![],
        require_approval_for: vec![],
    };
    insta::assert_snapshot!("gm_cross_format_policy_json", snap_json(&p));
}

#[test]
fn gm_cross_format_policy_toml() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec![".env".into()],
        deny_write: vec![],
        allow_network: vec!["*.github.com".into()],
        deny_network: vec![],
        require_approval_for: vec![],
    };
    let toml_str = toml::to_string_pretty(&p).unwrap();
    insta::assert_snapshot!("gm_cross_format_policy_toml", toml_str);
}

#[test]
fn gm_cross_format_config_json() {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("info".into()),
        backends,
        ..Default::default()
    };
    insta::assert_snapshot!("gm_cross_format_config_json", snap_json(&cfg));
}

#[test]
fn gm_cross_format_config_toml() {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("info".into()),
        backends,
        ..Default::default()
    };
    let toml_str = toml::to_string_pretty(&cfg).unwrap();
    insta::assert_snapshot!("gm_cross_format_config_toml", toml_str);
}

#[test]
fn gm_cross_format_work_order_json() {
    let wo = WorkOrder {
        id: uid1(),
        task: "cross-format test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: Some("gpt-4o".into()),
            ..RuntimeConfig::default()
        },
    };
    insta::assert_snapshot!("gm_cross_format_work_order_json", snap_json(&wo));
}

#[test]
fn gm_cross_format_work_order_toml() {
    let wo = WorkOrder {
        id: uid1(),
        task: "cross-format test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: Some("gpt-4o".into()),
            ..RuntimeConfig::default()
        },
    };
    let toml_str = toml::to_string_pretty(&wo).unwrap();
    insta::assert_snapshot!("gm_cross_format_work_order_toml", toml_str);
}

#[test]
fn gm_cross_format_runtime_config_json() {
    let cfg = RuntimeConfig {
        model: Some("claude-4-opus".into()),
        vendor: BTreeMap::from([("temperature".into(), json!(0.5))]),
        env: BTreeMap::from([("LANG".into(), "en_US".into())]),
        max_budget_usd: Some(10.0),
        max_turns: Some(100),
    };
    insta::assert_snapshot!("gm_cross_format_runtime_config_json", snap_json(&cfg));
}

#[test]
fn gm_cross_format_runtime_config_toml() {
    let cfg = RuntimeConfig {
        model: Some("claude-4-opus".into()),
        vendor: BTreeMap::from([("temperature".into(), json!(0.5))]),
        env: BTreeMap::from([("LANG".into(), "en_US".into())]),
        max_budget_usd: Some(10.0),
        max_turns: Some(100),
    };
    let toml_str = toml::to_string_pretty(&cfg).unwrap();
    insta::assert_snapshot!("gm_cross_format_runtime_config_toml", toml_str);
}
