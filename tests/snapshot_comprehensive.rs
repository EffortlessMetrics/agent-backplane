#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

//! Comprehensive snapshot tests that verify the exact JSON serialization
//! format of all key ABP types using `insta` snapshots. Any serialization
//! format change will be caught immediately.

use abp_config::{BackendEntry, BackplaneConfig};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_error::ErrorCode;
use abp_protocol::Envelope;
use chrono::{TimeZone, Utc};
use insta::{assert_json_snapshot, assert_snapshot};
use serde_json;
use std::collections::BTreeMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_uuid() -> Uuid {
    Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
}

fn fixed_uuid2() -> Uuid {
    Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap()
}

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap()
}

fn fixed_ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 10, 31, 0).unwrap()
}

fn make_receipt(outcome: Outcome) -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 60000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome,
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

/// Helper: serialize any Envelope to serde_json::Value since Envelope
/// may contain CapabilityManifest (BTreeMap with non-string keys).
fn envelope_to_value(env: &Envelope) -> serde_json::Value {
    serde_json::to_value(env).unwrap()
}

/// Helper: serialize a Receipt to serde_json::Value (same reason).
fn receipt_to_value(r: &Receipt) -> serde_json::Value {
    serde_json::to_value(r).unwrap()
}

// =========================================================================
// 1. Receipt shape snapshots for each outcome type
// =========================================================================

#[test]
fn snap_receipt_outcome_success() {
    let r = make_receipt(Outcome::Complete);
    assert_json_snapshot!(receipt_to_value(&r));
}

#[test]
fn snap_receipt_outcome_error() {
    let mut r = make_receipt(Outcome::Failed);
    r.trace = vec![make_event(AgentEventKind::Error {
        message: "backend crashed".into(),
        error_code: Some(ErrorCode::BackendCrashed),
    })];
    assert_json_snapshot!(receipt_to_value(&r));
}

#[test]
fn snap_receipt_outcome_timeout() {
    let mut r = make_receipt(Outcome::Failed);
    r.trace = vec![make_event(AgentEventKind::Error {
        message: "backend timed out after 30s".into(),
        error_code: Some(ErrorCode::BackendTimeout),
    })];
    assert_json_snapshot!(receipt_to_value(&r));
}

#[test]
fn snap_receipt_outcome_cancel() {
    let mut r = make_receipt(Outcome::Partial);
    r.trace = vec![make_event(AgentEventKind::Warning {
        message: "run cancelled by user".into(),
    })];
    assert_json_snapshot!(receipt_to_value(&r));
}

#[test]
fn snap_receipt_with_full_usage() {
    let mut r = make_receipt(Outcome::Complete);
    r.usage = UsageNormalized {
        input_tokens: Some(1500),
        output_tokens: Some(800),
        cache_read_tokens: Some(200),
        cache_write_tokens: Some(100),
        request_units: Some(5),
        estimated_cost_usd: Some(0.042),
    };
    r.usage_raw = serde_json::json!({"prompt_tokens": 1500, "completion_tokens": 800});
    assert_json_snapshot!(receipt_to_value(&r));
}

#[test]
fn snap_receipt_with_artifacts_and_verification() {
    let mut r = make_receipt(Outcome::Complete);
    r.artifacts = vec![
        ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        },
        ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        },
    ];
    r.verification = VerificationReport {
        git_diff: Some("diff --git a/main.rs b/main.rs\n+// fixed".into()),
        git_status: Some("M main.rs".into()),
        harness_ok: true,
    };
    assert_json_snapshot!(receipt_to_value(&r));
}

#[test]
fn snap_receipt_passthrough_mode() {
    let mut r = make_receipt(Outcome::Complete);
    r.mode = ExecutionMode::Passthrough;
    assert_json_snapshot!(receipt_to_value(&r));
}

// =========================================================================
// 2. WorkOrder shape snapshots with various configurations
// =========================================================================

#[test]
fn snap_work_order_minimal() {
    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "hello world".into(),
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
    assert_json_snapshot!(wo);
}

#[test]
fn snap_work_order_with_model_and_budget() {
    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "Refactor auth module".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/workspace".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec!["src/**/*.rs".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: Some(5.0),
            max_turns: Some(20),
        },
    };
    assert_json_snapshot!(wo);
}

#[test]
fn snap_work_order_with_context_snippets() {
    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "Fix bug".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket {
            files: vec!["src/main.rs".into(), "Cargo.toml".into()],
            snippets: vec![ContextSnippet {
                name: "error_log".into(),
                content: "thread 'main' panicked at 'index out of bounds'".into(),
            }],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    assert_json_snapshot!(wo);
}

#[test]
fn snap_work_order_with_policy_and_requirements() {
    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "Secure refactor".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile {
            allowed_tools: vec!["Read".into(), "Grep".into()],
            disallowed_tools: vec!["Bash".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec!["**/.git/**".into()],
            allow_network: vec!["*.example.com".into()],
            deny_network: vec!["evil.example.com".into()],
            require_approval_for: vec!["DeleteFile".into()],
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
        config: RuntimeConfig::default(),
    };
    assert_json_snapshot!(wo);
}

#[test]
fn snap_work_order_with_vendor_config() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "openai".into(),
        serde_json::json!({"temperature": 0.7, "top_p": 0.9}),
    );
    let mut env = BTreeMap::new();
    env.insert("OPENAI_API_KEY".into(), "sk-test".into());

    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "Generate code".into(),
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
        config: RuntimeConfig {
            model: Some("claude-3-opus".into()),
            vendor,
            env,
            max_budget_usd: None,
            max_turns: Some(5),
        },
    };
    assert_json_snapshot!(wo);
}

// =========================================================================
// 3. AgentEvent snapshots for each kind
// =========================================================================

#[test]
fn snap_event_text_delta() {
    let e = make_event(AgentEventKind::AssistantDelta {
        text: "Hello, ".into(),
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_assistant_message() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "I've completed the refactoring.".into(),
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_tool_call() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "Read".into(),
        tool_use_id: Some("tu_001".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "src/main.rs"}),
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_tool_call_nested() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "Grep".into(),
        tool_use_id: Some("tu_002".into()),
        parent_tool_use_id: Some("tu_001".into()),
        input: serde_json::json!({"pattern": "fn main", "path": "src/"}),
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_tool_result_success() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "Read".into(),
        tool_use_id: Some("tu_001".into()),
        output: serde_json::json!({"content": "fn main() {}"}),
        is_error: false,
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_tool_result_error() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "Read".into(),
        tool_use_id: Some("tu_001".into()),
        output: serde_json::json!({"error": "file not found"}),
        is_error: true,
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_error_with_code() {
    let e = make_event(AgentEventKind::Error {
        message: "backend crashed unexpectedly".into(),
        error_code: Some(ErrorCode::BackendCrashed),
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_error_without_code() {
    let e = make_event(AgentEventKind::Error {
        message: "unknown failure".into(),
        error_code: None,
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_run_started() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "Starting execution".into(),
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_run_completed() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "Execution finished successfully".into(),
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_file_changed() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "Added error handling".into(),
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_command_executed() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("test result: ok. 42 passed".into()),
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_warning() {
    let e = make_event(AgentEventKind::Warning {
        message: "approaching budget limit".into(),
    });
    assert_json_snapshot!(e);
}

#[test]
fn snap_event_with_ext_passthrough() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        serde_json::json!({"vendor_field": "value"}),
    );
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantDelta {
            text: "token".into(),
        },
        ext: Some(ext),
    };
    assert_json_snapshot!(e);
}

// =========================================================================
// 4. Envelope snapshots for each variant
// =========================================================================

#[test]
fn snap_envelope_hello_default() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        {
            let mut caps = CapabilityManifest::new();
            caps.insert(Capability::Streaming, SupportLevel::Native);
            caps.insert(Capability::ToolRead, SupportLevel::Native);
            caps
        },
    );
    assert_json_snapshot!(serde_json::to_value(&env).unwrap());
}

#[test]
fn snap_envelope_hello_passthrough() {
    let env = Envelope::hello_with_mode(
        BackendIdentity {
            id: "sidecar:claude".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    assert_json_snapshot!(envelope_to_value(&env));
}

#[test]
fn snap_envelope_hello_empty_caps() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert_json_snapshot!(envelope_to_value(&env));
}

#[test]
fn snap_envelope_run() {
    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "Do something".into(),
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
        id: "run-001".into(),
        work_order: wo,
    };
    assert_json_snapshot!(envelope_to_value(&env));
}

#[test]
fn snap_envelope_event() {
    let event = make_event(AgentEventKind::AssistantDelta {
        text: "Hello".into(),
    });
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event,
    };
    assert_json_snapshot!(envelope_to_value(&env));
}

#[test]
fn snap_envelope_final() {
    let receipt = make_receipt(Outcome::Complete);
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt,
    };
    assert_json_snapshot!(envelope_to_value(&env));
}

#[test]
fn snap_envelope_fatal_with_ref() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    assert_json_snapshot!(envelope_to_value(&env));
}

#[test]
fn snap_envelope_fatal_without_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "process crashed".into(),
        error_code: None,
    };
    assert_json_snapshot!(envelope_to_value(&env));
}

#[test]
fn snap_envelope_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-002".into()),
        "backend timed out",
        ErrorCode::BackendTimeout,
    );
    assert_json_snapshot!(envelope_to_value(&env));
}

// =========================================================================
// 5. PolicyProfile snapshots for various policies
// =========================================================================

#[test]
fn snap_policy_empty() {
    let p = PolicyProfile::default();
    assert_json_snapshot!(p);
}

#[test]
fn snap_policy_tools_only() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Grep".into()],
        disallowed_tools: vec!["Bash".into(), "BashExec".into()],
        ..PolicyProfile::default()
    };
    assert_json_snapshot!(p);
}

#[test]
fn snap_policy_paths_only() {
    let p = PolicyProfile {
        deny_read: vec!["**/.env".into(), "**/.env.*".into(), "**/id_rsa".into()],
        deny_write: vec!["**/.git/**".into(), "**/node_modules/**".into()],
        ..PolicyProfile::default()
    };
    assert_json_snapshot!(p);
}

#[test]
fn snap_policy_network_rules() {
    let p = PolicyProfile {
        allow_network: vec!["*.example.com".into(), "api.github.com".into()],
        deny_network: vec!["evil.example.com".into(), "*.malware.net".into()],
        ..PolicyProfile::default()
    };
    assert_json_snapshot!(p);
}

#[test]
fn snap_policy_approval_required() {
    let p = PolicyProfile {
        require_approval_for: vec!["Bash".into(), "DeleteFile".into(), "WriteFile".into()],
        ..PolicyProfile::default()
    };
    assert_json_snapshot!(p);
}

#[test]
fn snap_policy_full_lockdown() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash*".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/*".into()],
        allow_network: vec![],
        deny_network: vec!["*".into()],
        require_approval_for: vec!["Read".into()],
    };
    assert_json_snapshot!(p);
}

// =========================================================================
// 6. BackplaneConfig snapshots for various configs
// =========================================================================

#[test]
fn snap_config_default() {
    let cfg = BackplaneConfig::default();
    assert_json_snapshot!(cfg);
}

#[test]
fn snap_config_with_mock_backend() {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        backends,
        ..BackplaneConfig::default()
    };
    assert_json_snapshot!(cfg);
}

#[test]
fn snap_config_with_sidecar_backend() {
    let mut backends = BTreeMap::new();
    backends.insert(
        "node".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["hosts/node/index.js".into()],
            timeout_secs: Some(120),
        },
    );
    let cfg = BackplaneConfig {
        default_backend: Some("node".into()),
        workspace_dir: Some("/tmp/abp-workspaces".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/var/log/abp/receipts".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        policy_profiles: vec!["policies/default.toml".into()],
        backends,
    };
    assert_json_snapshot!(cfg);
}

#[test]
fn snap_config_multi_backend() {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    backends.insert(
        "claude".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["hosts/claude/index.js".into()],
            timeout_secs: Some(300),
        },
    );
    backends.insert(
        "openai".into(),
        BackendEntry::Sidecar {
            command: "python".into(),
            args: vec!["hosts/openai/main.py".into()],
            timeout_secs: None,
        },
    );
    let cfg = BackplaneConfig {
        default_backend: Some("claude".into()),
        backends,
        ..BackplaneConfig::default()
    };
    assert_json_snapshot!(cfg);
}

// =========================================================================
// 7. ErrorCode display snapshots for all error codes
// =========================================================================

#[test]
fn snap_error_code_display_all() {
    let codes = vec![
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
        ErrorCode::BackendContentFiltered,
        ErrorCode::BackendContextLength,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
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
        ErrorCode::RateLimitExceeded,
        ErrorCode::CircuitBreakerOpen,
        ErrorCode::StreamClosed,
        ErrorCode::ReceiptStoreFailed,
        ErrorCode::ValidationFailed,
        ErrorCode::SidecarSpawnFailed,
        ErrorCode::Internal,
    ];
    let display_map: BTreeMap<String, String> = codes
        .iter()
        .map(|c| (c.as_str().to_string(), c.to_string()))
        .collect();
    assert_json_snapshot!(display_map);
}

#[test]
fn snap_error_code_serde_roundtrip() {
    let codes = vec![
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendTimeout,
        ErrorCode::PolicyDenied,
        ErrorCode::Internal,
        ErrorCode::ReceiptHashMismatch,
    ];
    let json_codes: Vec<serde_json::Value> = codes
        .iter()
        .map(|c| serde_json::to_value(c).unwrap())
        .collect();
    assert_json_snapshot!(json_codes);
}

#[test]
fn snap_error_code_categories() {
    let codes = vec![
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendTimeout,
        ErrorCode::PolicyDenied,
        ErrorCode::IrLoweringFailed,
        ErrorCode::Internal,
    ];
    let category_map: BTreeMap<String, String> = codes
        .iter()
        .map(|c| (c.as_str().to_string(), c.category().to_string()))
        .collect();
    assert_json_snapshot!(category_map);
}

#[test]
fn snap_error_code_retryable() {
    let retryable: Vec<&str> = vec![
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
        ErrorCode::RateLimitExceeded,
        ErrorCode::CircuitBreakerOpen,
        ErrorCode::StreamClosed,
    ]
    .into_iter()
    .filter(|c| c.is_retryable())
    .map(|c| c.as_str())
    .collect();
    assert_json_snapshot!(retryable);
}

// =========================================================================
// 8. CapabilityManifest snapshots with various capability sets
// =========================================================================

/// Helper: serialize a CapabilityManifest to a serde_json::Value so insta
/// can snapshot it (BTreeMap<Capability,_> keys aren't plain strings).
fn manifest_to_value(m: &CapabilityManifest) -> serde_json::Value {
    serde_json::to_value(m).unwrap()
}

#[test]
fn snap_capability_manifest_empty() {
    let manifest = CapabilityManifest::new();
    assert_json_snapshot!(manifest_to_value(&manifest));
}

#[test]
fn snap_capability_manifest_streaming_only() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    assert_json_snapshot!(manifest_to_value(&manifest));
}

#[test]
fn snap_capability_manifest_tool_suite() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::ToolWrite, SupportLevel::Native);
    manifest.insert(Capability::ToolEdit, SupportLevel::Native);
    manifest.insert(Capability::ToolBash, SupportLevel::Native);
    manifest.insert(Capability::ToolGlob, SupportLevel::Emulated);
    manifest.insert(Capability::ToolGrep, SupportLevel::Emulated);
    assert_json_snapshot!(manifest_to_value(&manifest));
}

#[test]
fn snap_capability_manifest_mixed_support() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
    manifest.insert(Capability::McpClient, SupportLevel::Unsupported);
    manifest.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed environment".into(),
        },
    );
    assert_json_snapshot!(manifest_to_value(&manifest));
}

#[test]
fn snap_capability_manifest_full() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::ToolWrite, SupportLevel::Native);
    manifest.insert(Capability::ToolEdit, SupportLevel::Native);
    manifest.insert(Capability::ToolBash, SupportLevel::Native);
    manifest.insert(Capability::ToolUse, SupportLevel::Native);
    manifest.insert(Capability::ExtendedThinking, SupportLevel::Native);
    manifest.insert(Capability::ImageInput, SupportLevel::Native);
    manifest.insert(Capability::McpClient, SupportLevel::Native);
    manifest.insert(Capability::McpServer, SupportLevel::Emulated);
    manifest.insert(Capability::SessionResume, SupportLevel::Unsupported);
    assert_json_snapshot!(manifest_to_value(&manifest));
}

// =========================================================================
// Additional variant snapshots
// =========================================================================

#[test]
fn snap_support_level_all_variants() {
    let variants: Vec<SupportLevel> = vec![
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "policy restriction".into(),
        },
    ];
    assert_json_snapshot!(variants);
}

#[test]
fn snap_outcome_all_variants() {
    let variants: Vec<Outcome> = vec![Outcome::Complete, Outcome::Partial, Outcome::Failed];
    assert_json_snapshot!(variants);
}

#[test]
fn snap_execution_mode_variants() {
    let variants: Vec<ExecutionMode> = vec![ExecutionMode::Mapped, ExecutionMode::Passthrough];
    assert_json_snapshot!(variants);
}

#[test]
fn snap_execution_lane_variants() {
    let variants: Vec<ExecutionLane> =
        vec![ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst];
    assert_json_snapshot!(variants);
}

#[test]
fn snap_workspace_mode_variants() {
    let variants: Vec<WorkspaceMode> = vec![WorkspaceMode::Staged, WorkspaceMode::PassThrough];
    assert_json_snapshot!(variants);
}

#[test]
fn snap_min_support_variants() {
    let variants: Vec<MinSupport> = vec![MinSupport::Native, MinSupport::Emulated, MinSupport::Any];
    assert_json_snapshot!(variants);
}

#[test]
fn snap_contract_version() {
    assert_snapshot!(CONTRACT_VERSION, @"abp/v0.1");
}
