// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive snapshot regression tests for all major ABP data structures.
//!
//! Uses `insta` to detect serialization regressions across core types,
//! protocol envelopes, error taxonomy, capability negotiation, and policy.

use std::collections::BTreeMap;
use std::path::Path;

use abp_capability::{
    CapabilityRegistry, NegotiationResult, generate_report, negotiate, negotiate_capabilities,
};
use abp_core::*;
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use abp_policy::PolicyEngine;
use abp_protocol::Envelope;
use chrono::{TimeZone, Utc};
use insta::{assert_json_snapshot, assert_snapshot};
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_ts() -> chrono::DateTime<chrono::Utc> {
    Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
}

fn fixed_uuid() -> Uuid {
    Uuid::nil()
}

fn fixed_uuid2() -> Uuid {
    Uuid::from_u128(1)
}

fn make_work_order() -> WorkOrder {
    WorkOrder {
        id: fixed_uuid(),
        task: "Refactor auth module".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["README.md".into(), "src/lib.rs".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "Use JWT for auth".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read".into(), "glob".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec![".env".into()],
            deny_write: vec!["Cargo.lock".into()],
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["evil.com".into()],
            require_approval_for: vec!["write".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Emulated,
                },
            ],
        },
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor: BTreeMap::from([("abp".into(), json!({"mode": "mapped"}))]),
            env: BTreeMap::from([("RUST_LOG".into(), "debug".into())]),
            max_budget_usd: Some(5.0),
            max_turns: Some(20),
        },
    }
}

fn make_receipt() -> Receipt {
    let ts = fixed_ts();
    Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 1500,
        },
        backend: BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.2.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: BTreeMap::from([
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"prompt_tokens": 200, "completion_tokens": 150}),
        usage: UsageNormalized {
            input_tokens: Some(200),
            output_tokens: Some(150),
            cache_read_tokens: Some(50),
            cache_write_tokens: Some(10),
            request_units: Some(1),
            estimated_cost_usd: Some(0.005),
        },
        trace: vec![
            AgentEvent {
                ts,
                kind: AgentEventKind::RunStarted {
                    message: "starting".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts,
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            },
        ],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "out.patch".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("+added line".into()),
            git_status: Some("M src/lib.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: fixed_ts(),
        kind,
        ext: None,
    }
}

// ===========================================================================
// 1. Core type snapshots (15 tests)
// ===========================================================================

#[test]
fn regr_work_order_full() {
    assert_json_snapshot!(make_work_order());
}

#[test]
fn regr_work_order_minimal() {
    let wo = WorkOrder {
        id: fixed_uuid(),
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

#[test]
fn regr_receipt_without_hash() {
    let v = serde_json::to_value(make_receipt()).unwrap();
    assert_json_snapshot!(v);
}

#[test]
fn regr_receipt_with_hash() {
    let receipt = make_receipt().with_hash().unwrap();
    let v = serde_json::to_value(receipt).unwrap();
    assert_json_snapshot!(v);
}

#[test]
fn regr_event_run_started() {
    assert_json_snapshot!(make_event(AgentEventKind::RunStarted {
        message: "initializing".into(),
    }));
}

#[test]
fn regr_event_run_completed() {
    assert_json_snapshot!(make_event(AgentEventKind::RunCompleted {
        message: "finished".into(),
    }));
}

#[test]
fn regr_event_assistant_delta() {
    assert_json_snapshot!(make_event(AgentEventKind::AssistantDelta {
        text: "Hello, ".into(),
    }));
}

#[test]
fn regr_event_assistant_message() {
    assert_json_snapshot!(make_event(AgentEventKind::AssistantMessage {
        text: "Hello, world!".into(),
    }));
}

#[test]
fn regr_event_tool_call() {
    assert_json_snapshot!(make_event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("tu_42".into()),
        parent_tool_use_id: Some("tu_parent".into()),
        input: json!({"path": "src/main.rs"}),
    }));
}

#[test]
fn regr_event_tool_result() {
    assert_json_snapshot!(make_event(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: Some("tu_42".into()),
        output: json!({"content": "fn main() {}"}),
        is_error: false,
    }));
}

#[test]
fn regr_event_file_changed() {
    assert_json_snapshot!(make_event(AgentEventKind::FileChanged {
        path: "src/auth.rs".into(),
        summary: "added JWT validation".into(),
    }));
}

#[test]
fn regr_event_command_executed() {
    assert_json_snapshot!(make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("test result: ok".into()),
    }));
}

#[test]
fn regr_event_warning() {
    assert_json_snapshot!(make_event(AgentEventKind::Warning {
        message: "approaching budget limit".into(),
    }));
}

#[test]
fn regr_event_error_with_code() {
    assert_json_snapshot!(make_event(AgentEventKind::Error {
        message: "backend returned 500".into(),
        error_code: Some(ErrorCode::BackendUnavailable),
    }));
}

#[test]
fn regr_event_with_ext() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "passthrough".into(),
        },
        ext: Some(BTreeMap::from([(
            "raw_message".into(),
            json!({"role": "assistant", "content": "passthrough"}),
        )])),
    };
    assert_json_snapshot!(event);
}

// ===========================================================================
// 2. Protocol envelope snapshots (10 tests)
// ===========================================================================

#[test]
fn regr_envelope_hello_default() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert_json_snapshot!(env);
}

#[test]
fn regr_envelope_hello_passthrough() {
    let env = Envelope::hello_with_mode(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    assert_json_snapshot!(env);
}

#[test]
fn regr_envelope_hello_full_caps() {
    let caps = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Emulated),
        (
            Capability::McpClient,
            SupportLevel::Restricted {
                reason: "disabled".into(),
            },
        ),
    ]);
    let env = Envelope::hello(
        BackendIdentity {
            id: "full-sidecar".into(),
            backend_version: Some("2.0.0".into()),
            adapter_version: Some("0.5.0".into()),
        },
        caps,
    );
    let v = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!(v);
}

#[test]
fn regr_envelope_run() {
    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: make_work_order(),
    };
    assert_json_snapshot!(env);
}

#[test]
fn regr_envelope_run_minimal() {
    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "simple task".into(),
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
    let env = Envelope::Run {
        id: "run-002".into(),
        work_order: wo,
    };
    assert_json_snapshot!(env);
}

#[test]
fn regr_envelope_event() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: make_event(AgentEventKind::AssistantDelta {
            text: "streaming...".into(),
        }),
    };
    assert_json_snapshot!(env);
}

#[test]
fn regr_envelope_final() {
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: make_receipt(),
    };
    let v = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!(v);
}

#[test]
fn regr_envelope_fatal_with_ref() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "backend crashed".into(),
        error_code: None,
    };
    assert_json_snapshot!(env);
}

#[test]
fn regr_envelope_fatal_without_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "handshake failed".into(),
        error_code: None,
    };
    assert_json_snapshot!(env);
}

#[test]
fn regr_envelope_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-003".into()),
        "timed out",
        ErrorCode::BackendTimeout,
    );
    assert_json_snapshot!(env);
}

// ===========================================================================
// 3. Error type snapshots (10 tests)
// ===========================================================================

#[test]
fn regr_error_code_protocol_variants() {
    let codes: Vec<serde_json::Value> = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ]
    .iter()
    .map(|c| json!({"code": c, "str": c.as_str(), "category": c.category().to_string()}))
    .collect();
    assert_json_snapshot!(codes);
}

#[test]
fn regr_error_code_backend_variants() {
    let codes: Vec<serde_json::Value> = [
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
    ]
    .iter()
    .map(|c| json!({"code": c, "str": c.as_str(), "retryable": c.is_retryable()}))
    .collect();
    assert_json_snapshot!(codes);
}

#[test]
fn regr_error_code_remaining_variants() {
    let codes: Vec<serde_json::Value> = [
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ]
    .iter()
    .map(|c| json!({"code": c, "str": c.as_str(), "category": c.category().to_string()}))
    .collect();
    assert_json_snapshot!(codes);
}

#[test]
fn regr_error_code_mapping_variants() {
    let codes: Vec<serde_json::Value> = [
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
    ]
    .iter()
    .map(|c| json!({"code": c, "str": c.as_str(), "message": c.message()}))
    .collect();
    assert_json_snapshot!(codes);
}

#[test]
fn regr_abp_error_display_simple() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s");
    assert_snapshot!(err.to_string());
}

#[test]
fn regr_abp_error_display_with_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);
    assert_snapshot!(err.to_string());
}

#[test]
fn regr_abp_error_dto() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "tool 'Bash' is disallowed")
        .with_context("tool", "Bash")
        .with_context("policy_rule", "disallowed_tools");
    let dto: AbpErrorDto = (&err).into();
    assert_json_snapshot!(dto);
}

#[test]
fn regr_error_info_simple() {
    let info = ErrorInfo::new(ErrorCode::BackendNotFound, "backend 'foo' not found");
    assert_json_snapshot!(info);
}

#[test]
fn regr_error_info_with_details() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out after 30s")
        .with_detail("backend", "openai")
        .with_detail("timeout_ms", 30_000);
    assert_json_snapshot!(info);
}

#[test]
fn regr_error_info_display() {
    let info = ErrorInfo::new(ErrorCode::ReceiptHashMismatch, "hash does not match")
        .with_detail("expected", "abc123")
        .with_detail("actual", "def456");
    assert_snapshot!(info.to_string());
}

// ===========================================================================
// 4. Cross-type interaction snapshots (15 tests)
// ===========================================================================

#[test]
fn regr_work_order_to_receipt_pipeline() {
    let wo = make_work_order();
    let ts = fixed_ts();
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: wo.id,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
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
        trace: vec![
            make_event(AgentEventKind::RunStarted {
                message: "starting".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    let hashed = receipt.with_hash().unwrap();
    assert_json_snapshot!(json!({
        "work_order_id": wo.id,
        "receipt_work_order_id": hashed.meta.work_order_id,
        "outcome": hashed.outcome,
        "has_hash": hashed.receipt_sha256.is_some(),
        "contract_version": hashed.meta.contract_version,
    }));
}

#[test]
fn regr_negotiation_all_native() {
    let manifest = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
    ]);
    let result = negotiate_capabilities(
        &[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ],
        &manifest,
    );
    assert_json_snapshot!(result);
}

#[test]
fn regr_negotiation_mixed() {
    let manifest = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
    ]);
    let result = negotiate_capabilities(
        &[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::McpClient,
        ],
        &manifest,
    );
    assert_json_snapshot!(result);
}

#[test]
fn regr_negotiation_all_unsupported() {
    let manifest = CapabilityManifest::new();
    let result = negotiate_capabilities(&[Capability::Streaming, Capability::ToolBash], &manifest);
    assert_json_snapshot!(result);
}

#[test]
fn regr_negotiation_display() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::McpClient],
    );
    assert_snapshot!(result.to_string());
}

#[test]
fn regr_negotiate_with_requirements() {
    let manifest = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
    ]);
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let result = negotiate(&manifest, &reqs);
    assert_json_snapshot!(result);
}

#[test]
fn regr_compatibility_report_viable() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolRead],
        vec![Capability::ToolWrite],
        vec![],
    );
    let report = generate_report(&result);
    assert_json_snapshot!(report);
}

#[test]
fn regr_compatibility_report_not_viable() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![],
        vec![Capability::McpClient, Capability::ExtendedThinking],
    );
    let report = generate_report(&result);
    assert_json_snapshot!(report);
}

#[test]
fn regr_registry_openai_vs_claude() {
    let registry = CapabilityRegistry::with_defaults();
    let result = registry
        .compare("openai/gpt-4o", "anthropic/claude-3.5-sonnet")
        .unwrap();
    assert_json_snapshot!(json!({
        "native_count": result.native.len(),
        "emulated_count": result.emulated.len(),
        "unsupported_count": result.unsupported.len(),
        "is_viable": result.is_viable(),
        "unsupported_caps": result.unsupported_caps(),
    }));
}

#[test]
fn regr_registry_claude_vs_gemini() {
    let registry = CapabilityRegistry::with_defaults();
    let result = registry
        .compare("anthropic/claude-3.5-sonnet", "google/gemini-1.5-pro")
        .unwrap();
    assert_json_snapshot!(json!({
        "native_count": result.native.len(),
        "emulated_count": result.emulated.len(),
        "unsupported_count": result.unsupported.len(),
        "is_viable": result.is_viable(),
        "unsupported_caps": result.unsupported_caps(),
    }));
}

#[test]
fn regr_registry_codex_vs_copilot() {
    let registry = CapabilityRegistry::with_defaults();
    let result = registry.compare("openai/codex", "github/copilot").unwrap();
    assert_json_snapshot!(json!({
        "native_count": result.native.len(),
        "emulated_count": result.emulated.len(),
        "unsupported_count": result.unsupported.len(),
        "is_viable": result.is_viable(),
    }));
}

#[test]
fn regr_policy_empty_allows_all() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    let tool = engine.can_use_tool("Bash");
    let read = engine.can_read_path(Path::new("any/file.txt"));
    let write = engine.can_write_path(Path::new("any/file.txt"));
    assert_json_snapshot!(json!({
        "tool_allowed": tool.allowed,
        "read_allowed": read.allowed,
        "write_allowed": write.allowed,
    }));
}

#[test]
fn regr_policy_deny_tool() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let bash = engine.can_use_tool("Bash");
    let read_tool = engine.can_use_tool("Read");
    assert_json_snapshot!(json!({
        "bash_allowed": bash.allowed,
        "bash_reason": bash.reason,
        "read_allowed": read_tool.allowed,
    }));
}

#[test]
fn regr_policy_deny_paths() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into(), "**/secret*".into()],
        deny_write: vec!["**/.git/**".into(), "**/Cargo.lock".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert_json_snapshot!(json!({
        "read_env": engine.can_read_path(Path::new(".env")).allowed,
        "read_secret": engine.can_read_path(Path::new("secret.key")).allowed,
        "read_src": engine.can_read_path(Path::new("src/lib.rs")).allowed,
        "write_git": engine.can_write_path(Path::new(".git/config")).allowed,
        "write_lock": engine.can_write_path(Path::new("Cargo.lock")).allowed,
        "write_src": engine.can_write_path(Path::new("src/lib.rs")).allowed,
    }));
}

#[test]
fn regr_policy_complex_combination() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Grep".into()],
        disallowed_tools: vec!["Write".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/locked/**".into()],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["evil.example.com".into()],
        require_approval_for: vec!["Bash".into()],
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert_json_snapshot!(json!({
        "write_denied_by_denylist": !engine.can_use_tool("Write").allowed,
        "read_allowed": engine.can_use_tool("Read").allowed,
        "bash_not_in_allowlist": !engine.can_use_tool("Bash").allowed,
        "env_read_denied": !engine.can_read_path(Path::new(".env")).allowed,
        "src_read_allowed": engine.can_read_path(Path::new("src/lib.rs")).allowed,
        "locked_write_denied": !engine.can_write_path(Path::new("locked/data.txt")).allowed,
        "src_write_allowed": engine.can_write_path(Path::new("src/lib.rs")).allowed,
    }));
}

// ===========================================================================
// 5. Additional regression coverage
// ===========================================================================

#[test]
fn regr_error_category_display_all() {
    let categories: Vec<String> = [
        ErrorCategory::Protocol,
        ErrorCategory::Backend,
        ErrorCategory::Capability,
        ErrorCategory::Policy,
        ErrorCategory::Workspace,
        ErrorCategory::Ir,
        ErrorCategory::Receipt,
        ErrorCategory::Dialect,
        ErrorCategory::Config,
        ErrorCategory::Mapping,
        ErrorCategory::Execution,
        ErrorCategory::Contract,
        ErrorCategory::Internal,
    ]
    .iter()
    .map(|c| c.to_string())
    .collect();
    assert_json_snapshot!(categories);
}

#[test]
fn regr_execution_mode_variants() {
    assert_json_snapshot!(json!({
        "mapped": ExecutionMode::Mapped,
        "passthrough": ExecutionMode::Passthrough,
        "default_is_mapped": ExecutionMode::default() == ExecutionMode::Mapped,
    }));
}

#[test]
fn regr_outcome_variants() {
    assert_json_snapshot!(json!({
        "complete": Outcome::Complete,
        "partial": Outcome::Partial,
        "failed": Outcome::Failed,
    }));
}

#[test]
fn regr_contract_version() {
    assert_snapshot!(CONTRACT_VERSION);
}

#[test]
fn regr_receipt_hash_determinism() {
    let r1 = make_receipt().with_hash().unwrap();
    let r2 = make_receipt().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    assert_snapshot!(r1.receipt_sha256.unwrap());
}

#[test]
fn regr_envelope_fatal_from_abp_error() {
    let err = AbpError::new(ErrorCode::BackendCrashed, "process exited with code 1");
    let env = Envelope::fatal_from_abp_error(Some("run-999".into()), &err);
    assert_json_snapshot!(env);
}
