// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conformance golden file test suite.
//!
//! Uses `insta::assert_json_snapshot!` to capture expected canonical JSON
//! for every significant type, envelope, event kind, error code, and
//! cross-crate workflow in the Agent Backplane.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_config::parse_toml;
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
    ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage},
    negotiate::{
        CapabilityNegotiator, CapabilityReport, CapabilityReportEntry, DialectSupportLevel,
        NegotiationRequest,
    },
};
use abp_error::ErrorCode;
use abp_integrations::projection::{Dialect, ProjectionMatrix, translate};
use abp_policy::PolicyEngine;
use abp_protocol::Envelope;

// ── Helpers ──────────────────────────────────────────────────────────────

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_ts_end() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 5, 0).unwrap()
}

const FIXED_UUID: Uuid = Uuid::from_bytes([
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
]);

const FIXED_WO_UUID: Uuid = Uuid::from_bytes([
    0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf, 0xb0,
]);

fn fixed_backend() -> BackendIdentity {
    BackendIdentity {
        id: "golden-test".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn fixed_capabilities() -> CapabilityManifest {
    BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Emulated),
        (Capability::ToolBash, SupportLevel::Unsupported),
    ])
}

fn fixed_receipt(outcome: Outcome, mode: ExecutionMode) -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: FIXED_UUID,
            work_order_id: FIXED_WO_UUID,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_ts(),
            finished_at: fixed_ts_end(),
            duration_ms: 300_000,
        },
        backend: fixed_backend(),
        capabilities: fixed_capabilities(),
        mode,
        usage_raw: json!({"prompt_tokens": 100, "completion_tokens": 50}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.0015),
        },
        trace: vec![AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "Golden test run".into(),
            },
            ext: None,
        }],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("+added line".into()),
            git_status: Some("M src/main.rs".into()),
            harness_ok: true,
        },
        outcome,
        receipt_sha256: None,
    }
}

fn fixed_work_order(mode: WorkspaceMode) -> WorkOrder {
    WorkOrder {
        id: FIXED_WO_UUID,
        task: "Refactor auth module".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/workspace".into(),
            mode,
            include: vec!["src/**/*.rs".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "instructions".into(),
                content: "Focus on error handling".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["Read".into(), "Write".into()],
            disallowed_tools: vec!["Bash".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec!["**/.git/**".into()],
            allow_network: vec![],
            deny_network: vec!["*.internal".into()],
            require_approval_for: vec!["Delete".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        },
        config: RuntimeConfig {
            model: Some("gpt-4o".into()),
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: Some(1.0),
            max_turns: Some(10),
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. WorkOrder → canonical JSON golden file for each execution mode
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_work_order_passthrough_mode() {
    let wo = fixed_work_order(WorkspaceMode::PassThrough);
    insta::assert_json_snapshot!(wo);
}

#[test]
fn golden_work_order_staged_mode() {
    let wo = fixed_work_order(WorkspaceMode::Staged);
    insta::assert_json_snapshot!(wo);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Receipt → canonical JSON golden file for each outcome type
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_receipt_complete() {
    let r = fixed_receipt(Outcome::Complete, ExecutionMode::Mapped);
    let v = serde_json::to_value(&r).unwrap();
    insta::assert_json_snapshot!(v);
}

#[test]
fn golden_receipt_partial() {
    let r = fixed_receipt(Outcome::Partial, ExecutionMode::Mapped);
    let v = serde_json::to_value(&r).unwrap();
    insta::assert_json_snapshot!(v);
}

#[test]
fn golden_receipt_failed() {
    let r = fixed_receipt(Outcome::Failed, ExecutionMode::Passthrough);
    let v = serde_json::to_value(&r).unwrap();
    insta::assert_json_snapshot!(v);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Full JSONL session transcript (hello→run→events→final)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_full_session_transcript() {
    let hello = Envelope::hello(fixed_backend(), fixed_capabilities());
    let wo = fixed_work_order(WorkspaceMode::Staged);
    let run = Envelope::Run {
        id: "run-001".into(),
        work_order: wo,
    };
    let event1 = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "Starting".into(),
            },
            ext: None,
        },
    };
    let event2 = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Done refactoring.".into(),
            },
            ext: None,
        },
    };
    let final_env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: fixed_receipt(Outcome::Complete, ExecutionMode::Mapped),
    };

    let transcript: Vec<serde_json::Value> = vec![
        serde_json::to_value(&hello).unwrap(),
        serde_json::to_value(&run).unwrap(),
        serde_json::to_value(&event1).unwrap(),
        serde_json::to_value(&event2).unwrap(),
        serde_json::to_value(&final_env).unwrap(),
    ];
    insta::assert_json_snapshot!(transcript);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Error JSONL transcript (hello→run→fatal)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_error_session_transcript() {
    let hello = Envelope::hello(fixed_backend(), fixed_capabilities());
    let wo = fixed_work_order(WorkspaceMode::Staged);
    let run = Envelope::Run {
        id: "run-002".into(),
        work_order: wo,
    };
    let fatal = Envelope::fatal_with_code(
        Some("run-002".into()),
        "backend crashed unexpectedly",
        ErrorCode::BackendCrashed,
    );

    let transcript: Vec<serde_json::Value> = vec![
        serde_json::to_value(&hello).unwrap(),
        serde_json::to_value(&run).unwrap(),
        serde_json::to_value(&fatal).unwrap(),
    ];
    insta::assert_json_snapshot!(transcript);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Capability negotiation result → golden JSON for each scenario
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_capability_negotiation_all_satisfied() {
    let request = NegotiationRequest {
        required: vec![Capability::Streaming, Capability::ToolRead],
        preferred: vec![Capability::ExtendedThinking],
        minimum_support: SupportLevel::Emulated,
    };
    let manifest = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
        (Capability::ExtendedThinking, SupportLevel::Native),
    ]);
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    let snapshot = json!({
        "satisfied": result.satisfied,
        "unsatisfied": result.unsatisfied,
        "bonus": result.bonus,
        "is_compatible": result.is_compatible,
    });
    insta::assert_json_snapshot!(snapshot);
}

#[test]
fn golden_capability_negotiation_unsatisfied() {
    let request = NegotiationRequest {
        required: vec![Capability::Streaming, Capability::Logprobs],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let manifest = BTreeMap::from([(Capability::Streaming, SupportLevel::Native)]);
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    let snapshot = json!({
        "satisfied": result.satisfied,
        "unsatisfied": result.unsatisfied,
        "bonus": result.bonus,
        "is_compatible": result.is_compatible,
    });
    insta::assert_json_snapshot!(snapshot);
}

#[test]
fn golden_capability_negotiation_emulated_meets_emulated() {
    let request = NegotiationRequest {
        required: vec![Capability::ToolWrite],
        preferred: vec![Capability::ToolBash],
        minimum_support: SupportLevel::Emulated,
    };
    let manifest = BTreeMap::from([
        (Capability::ToolWrite, SupportLevel::Emulated),
        (
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandboxed".into(),
            },
        ),
    ]);
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    let snapshot = json!({
        "satisfied": result.satisfied,
        "unsatisfied": result.unsatisfied,
        "bonus": result.bonus,
        "is_compatible": result.is_compatible,
    });
    insta::assert_json_snapshot!(snapshot);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Mapping validation result → golden JSON for each dialect pair
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_mapping_fidelity_identity() {
    let matrix = ProjectionMatrix::new();
    let fidelity = matrix.can_translate(Dialect::Claude, Dialect::Claude);
    insta::assert_json_snapshot!(json!({
        "from": "claude",
        "to": "claude",
        "fidelity": fidelity,
    }));
}

#[test]
fn golden_mapping_fidelity_abp_to_openai() {
    let matrix = ProjectionMatrix::new();
    let fidelity = matrix.can_translate(Dialect::Abp, Dialect::OpenAi);
    insta::assert_json_snapshot!(json!({
        "from": "abp",
        "to": "openai",
        "fidelity": fidelity,
    }));
}

#[test]
fn golden_mapping_fidelity_cross_vendor() {
    let matrix = ProjectionMatrix::new();
    let fidelity = matrix.can_translate(Dialect::Claude, Dialect::OpenAi);
    insta::assert_json_snapshot!(json!({
        "from": "claude",
        "to": "openai",
        "fidelity": fidelity,
    }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Policy decision → golden JSON for allow/deny scenarios
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_policy_tool_allowed() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Read");
    insta::assert_json_snapshot!(decision);
}

#[test]
fn golden_policy_tool_denied_by_denylist() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Bash");
    insta::assert_json_snapshot!(decision);
}

#[test]
fn golden_policy_tool_denied_by_missing_allowlist() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Write");
    insta::assert_json_snapshot!(decision);
}

#[test]
fn golden_policy_write_path_denied() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_write_path(Path::new(".git/config"));
    insta::assert_json_snapshot!(decision);
}

#[test]
fn golden_policy_read_path_allowed() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_read_path(Path::new("src/main.rs"));
    insta::assert_json_snapshot!(decision);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Config parse → golden JSON for example TOML
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_config_parse_full() {
    let toml_str = r#"
default_backend = "mock"
log_level = "debug"
receipts_dir = "./data/receipts"
workspace_dir = "/tmp/abp"

[backends.mock]
type = "mock"

[backends.openai]
type = "sidecar"
command = "node"
args = ["openai-sidecar.js"]
timeout_secs = 300
"#;
    let config = parse_toml(toml_str).unwrap();
    insta::assert_json_snapshot!(config);
}

#[test]
fn golden_config_parse_minimal() {
    let toml_str = r#"
[backends.mock]
type = "mock"
"#;
    let config = parse_toml(toml_str).unwrap();
    insta::assert_json_snapshot!(config);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Each envelope type → golden JSON
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_envelope_hello() {
    let env = Envelope::hello(fixed_backend(), fixed_capabilities());
    let v = serde_json::to_value(&env).unwrap();
    insta::assert_json_snapshot!(v);
}

#[test]
fn golden_envelope_hello_passthrough() {
    let env = Envelope::hello_with_mode(
        fixed_backend(),
        fixed_capabilities(),
        ExecutionMode::Passthrough,
    );
    let v = serde_json::to_value(&env).unwrap();
    insta::assert_json_snapshot!(v);
}

#[test]
fn golden_envelope_run() {
    let env = Envelope::Run {
        id: "run-golden".into(),
        work_order: fixed_work_order(WorkspaceMode::Staged),
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn golden_envelope_event() {
    let env = Envelope::Event {
        ref_id: "run-golden".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello from golden test".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn golden_envelope_final() {
    let env = Envelope::Final {
        ref_id: "run-golden".into(),
        receipt: fixed_receipt(Outcome::Complete, ExecutionMode::Mapped),
    };
    let v = serde_json::to_value(&env).unwrap();
    insta::assert_json_snapshot!(v);
}

#[test]
fn golden_envelope_fatal_with_code() {
    let env = Envelope::fatal_with_code(
        Some("run-golden".into()),
        "sidecar process exited",
        ErrorCode::BackendCrashed,
    );
    insta::assert_json_snapshot!(env);
}

#[test]
fn golden_envelope_fatal_without_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "startup failure".into(),
        error_code: None,
    };
    insta::assert_json_snapshot!(env);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Each event kind → golden JSON
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_event_run_started() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "Starting golden run".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn golden_event_run_completed() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunCompleted {
            message: "Run finished successfully".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn golden_event_assistant_delta() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn golden_event_assistant_message() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "The refactoring is complete.".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn golden_event_tool_call() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tool-001".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/main.rs"}),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn golden_event_tool_result() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tool-001".into()),
            output: json!({"content": "fn main() {}"}),
            is_error: false,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn golden_event_file_changed() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::FileChanged {
            path: "src/auth.rs".into(),
            summary: "Added error handling".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn golden_event_command_executed() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok".into()),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn golden_event_warning() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Warning {
            message: "Approaching budget limit".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn golden_event_error() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Error {
            message: "Tool execution failed".into(),
            error_code: Some(ErrorCode::BackendCrashed),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Each error code → golden JSON (code + category + message)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_error_code_protocol_invalid_envelope() {
    let code = ErrorCode::ProtocolInvalidEnvelope;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_protocol_unexpected_message() {
    let code = ErrorCode::ProtocolUnexpectedMessage;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_protocol_version_mismatch() {
    let code = ErrorCode::ProtocolVersionMismatch;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_backend_not_found() {
    let code = ErrorCode::BackendNotFound;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_backend_timeout() {
    let code = ErrorCode::BackendTimeout;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_backend_crashed() {
    let code = ErrorCode::BackendCrashed;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_capability_unsupported() {
    let code = ErrorCode::CapabilityUnsupported;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_capability_emulation_failed() {
    let code = ErrorCode::CapabilityEmulationFailed;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_policy_denied() {
    let code = ErrorCode::PolicyDenied;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_policy_invalid() {
    let code = ErrorCode::PolicyInvalid;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_workspace_init_failed() {
    let code = ErrorCode::WorkspaceInitFailed;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_workspace_staging_failed() {
    let code = ErrorCode::WorkspaceStagingFailed;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_ir_lowering_failed() {
    let code = ErrorCode::IrLoweringFailed;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_ir_invalid() {
    let code = ErrorCode::IrInvalid;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_receipt_hash_mismatch() {
    let code = ErrorCode::ReceiptHashMismatch;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_receipt_chain_broken() {
    let code = ErrorCode::ReceiptChainBroken;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_dialect_unknown() {
    let code = ErrorCode::DialectUnknown;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_dialect_mapping_failed() {
    let code = ErrorCode::DialectMappingFailed;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_config_invalid() {
    let code = ErrorCode::ConfigInvalid;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

#[test]
fn golden_error_code_internal() {
    let code = ErrorCode::Internal;
    insta::assert_json_snapshot!(json!({
        "code": code,
        "category": code.category(),
        "display": code.as_str(),
    }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. IR conversion → golden JSON for each dialect's representative request
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_ir_conversation() {
    let conv = IrConversation::new()
        .push(IrMessage::text(
            IrRole::System,
            "You are a helpful assistant.",
        ))
        .push(IrMessage::text(IrRole::User, "Refactor auth module"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "I'll read the file first.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tu-001".into(),
                    name: "read_file".into(),
                    input: json!({"path": "src/auth.rs"}),
                },
            ],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu-001".into(),
                content: vec![IrContentBlock::Text {
                    text: "pub fn login() {}".into(),
                }],
                is_error: false,
            }],
        ));
    insta::assert_json_snapshot!(conv);
}

#[test]
fn golden_ir_tool_definition() {
    let tool = IrToolDefinition {
        name: "read_file".into(),
        description: "Read contents of a file".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"}
            },
            "required": ["path"]
        }),
    };
    insta::assert_json_snapshot!(tool);
}

#[test]
fn golden_ir_usage() {
    let usage = IrUsage::with_cache(1000, 500, 200, 100);
    insta::assert_json_snapshot!(usage);
}

#[test]
fn golden_translate_abp_to_claude() {
    let wo = fixed_work_order(WorkspaceMode::Staged);
    let result = translate(Dialect::Abp, Dialect::Claude, &wo).unwrap();
    insta::assert_json_snapshot!(result);
}

#[test]
fn golden_translate_abp_to_openai() {
    let wo = fixed_work_order(WorkspaceMode::Staged);
    let result = translate(Dialect::Abp, Dialect::OpenAi, &wo).unwrap();
    insta::assert_json_snapshot!(result);
}

#[test]
fn golden_translate_abp_to_gemini() {
    let wo = fixed_work_order(WorkspaceMode::Staged);
    let result = translate(Dialect::Abp, Dialect::Gemini, &wo).unwrap();
    insta::assert_json_snapshot!(result);
}

#[test]
fn golden_translate_abp_to_codex() {
    let wo = fixed_work_order(WorkspaceMode::Staged);
    let result = translate(Dialect::Abp, Dialect::Codex, &wo).unwrap();
    insta::assert_json_snapshot!(result);
}

#[test]
fn golden_translate_abp_to_kimi() {
    let wo = fixed_work_order(WorkspaceMode::Staged);
    let result = translate(Dialect::Abp, Dialect::Kimi, &wo).unwrap();
    insta::assert_json_snapshot!(result);
}

#[test]
fn golden_translate_abp_to_mock() {
    let wo = fixed_work_order(WorkspaceMode::Staged);
    let result = translate(Dialect::Abp, Dialect::Mock, &wo).unwrap();
    insta::assert_json_snapshot!(result);
}

// ═══════════════════════════════════════════════════════════════════════════
// Extra: Capability report (dialect-aware negotiation)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn golden_capability_report_claude() {
    let report = CapabilityReport {
        source_dialect: "abp".into(),
        target_dialect: "claude".into(),
        entries: vec![
            CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            },
            CapabilityReportEntry {
                capability: Capability::StructuredOutputJsonSchema,
                support: DialectSupportLevel::Emulated {
                    detail: "tool_use with JSON schema".into(),
                },
            },
            CapabilityReportEntry {
                capability: Capability::Logprobs,
                support: DialectSupportLevel::Unsupported {
                    reason: "Claude API does not expose logprobs".into(),
                },
            },
        ],
    };
    insta::assert_json_snapshot!(report);
}
