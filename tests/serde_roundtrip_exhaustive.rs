#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive serde roundtrip tests for ALL public serializable types
//! across the Agent Backplane workspace.

use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::PathBuf;
use uuid::Uuid;

// ── Crate imports ──────────────────────────────────────────────────────
use abp_core::aggregate::AggregationSummary;
use abp_core::error::ErrorCode;
use abp_core::{
    ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage},
    negotiate::{CapabilityReport, CapabilityReportEntry, DialectSupportLevel},
    verify::{ChainEntry, ChainError, ChainVerification, ReceiptChain},
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityRequirement,
    CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode,
    MinSupport, Outcome, PolicyProfile, Receipt, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkspaceMode, WorkspaceSpec, CONTRACT_VERSION,
};

use abp_protocol::{
    batch::{BatchItemStatus, BatchRequest, BatchResponse, BatchResult},
    compress::{CompressedMessage, CompressionAlgorithm},
    router::{MessageRoute, RouteTable},
    version::{ProtocolVersion, VersionRange},
    Envelope,
};

use abp_host::{
    health::{HealthCheck, HealthReport, HealthStatus},
    lifecycle::{LifecycleState, LifecycleTransition},
    pool::{PoolConfig, PoolStats},
    process::{ProcessConfig, ProcessInfo, ProcessStatus},
    registry::SidecarConfig,
    retry::{RetryAttempt, RetryConfig, RetryMetadata},
    SidecarHello, SidecarSpec,
};

use abp_policy::{
    audit::PolicyDecision as AuditPolicyDecision,
    audit::{AuditAction, AuditEntry, AuditLog, AuditRecord},
    compose::{PolicyDecision as ComposePolicyDecision, PolicyPrecedence},
    composed::{ComposedResult, CompositionStrategy},
    rate_limit::{RateLimitPolicy, RateLimitResult},
    rules::{Rule, RuleCondition, RuleEffect},
    Decision,
};

use abp_runtime::{
    budget::BudgetLimit,
    cancel::CancellationReason,
    execution::{ExecutionConfig, PipelineEvent, PipelineOutput},
    observe::{ObservabilitySummary, Span, SpanStatus},
    retry::{FallbackChain, RetryPolicy, TimeoutConfig},
    telemetry::MetricsSnapshot,
};

use abp_workspace::{
    diff::{
        ChangeType, DiffAnalysis, DiffChangeKind, DiffHunk, DiffLine, DiffLineKind, DiffPolicy,
        DiffReport, DiffSummary, FileBreakdown, FileCategory, FileChange as DiffFileChange,
        FileDiff, FileStats, FileType, PolicyResult, RiskLevel, WorkspaceDiff,
    },
    ops::{FileOperation, OperationSummary},
    snapshot::{FileSnapshot, SnapshotDiff, WorkspaceSnapshot},
    template::WorkspaceTemplate,
    tracker::{ChangeKind, ChangeSummary, FileChange as TrackerFileChange},
};

// ── Helpers ────────────────────────────────────────────────────────────

/// Roundtrip: serialize to JSON then deserialize back, assert equality.
fn roundtrip<T>(val: &T)
where
    T: Serialize + DeserializeOwned + Debug + PartialEq,
{
    let json = serde_json::to_string(val).expect("serialize");
    let back: T = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(*val, back, "roundtrip mismatch for {json}");
}

/// Roundtrip via serde_json::Value (loose equality for non-PartialEq types).
fn roundtrip_value<T>(val: &T)
where
    T: Serialize + DeserializeOwned + Debug,
{
    let json = serde_json::to_string(val).expect("serialize");
    let back: T = serde_json::from_str(&json).expect("deserialize");
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(json, json2, "value roundtrip mismatch");
}

/// Assert that a JSON string contains an expected key.
fn assert_json_has_key(json: &str, key: &str) {
    let v: Value = serde_json::from_str(json).unwrap();
    assert!(v.get(key).is_some(), "missing key '{key}' in {json}");
}

/// Assert deserialization fails for a given JSON string.
fn assert_deser_fails<T: DeserializeOwned + Debug>(json: &str) {
    let result = serde_json::from_str::<T>(json);
    assert!(
        result.is_err(),
        "expected deserialization failure for {json}"
    );
}

fn ts() -> DateTime<Utc> {
    Utc::now()
}

fn test_uuid() -> Uuid {
    Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
}

fn make_backend_identity() -> BackendIdentity {
    BackendIdentity {
        id: "mock-backend".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn make_run_metadata() -> RunMetadata {
    RunMetadata {
        run_id: test_uuid(),
        work_order_id: test_uuid(),
        contract_version: CONTRACT_VERSION.into(),
        started_at: ts(),
        finished_at: ts(),
        duration_ms: 42,
    }
}

fn make_receipt() -> Receipt {
    Receipt {
        meta: make_run_metadata(),
        backend: make_backend_identity(),
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn make_work_order() -> WorkOrder {
    WorkOrder {
        id: test_uuid(),
        task: "Say hello".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["**/*.rs".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "be concise".into(),
            }],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

// =========================================================================
// abp-core: WorkOrder & friends
// =========================================================================

#[test]
fn test_work_order_roundtrip() {
    roundtrip_value(&make_work_order());
}

#[test]
fn test_execution_lane_roundtrip() {
    roundtrip_value(&ExecutionLane::PatchFirst);
    roundtrip_value(&ExecutionLane::WorkspaceFirst);
}

#[test]
fn test_execution_lane_snake_case() {
    assert_eq!(
        serde_json::to_string(&ExecutionLane::PatchFirst).unwrap(),
        "\"patch_first\""
    );
    assert_eq!(
        serde_json::to_string(&ExecutionLane::WorkspaceFirst).unwrap(),
        "\"workspace_first\""
    );
}

#[test]
fn test_workspace_mode_roundtrip() {
    roundtrip_value(&WorkspaceMode::PassThrough);
    roundtrip_value(&WorkspaceMode::Staged);
}

#[test]
fn test_workspace_mode_snake_case() {
    assert_eq!(
        serde_json::to_string(&WorkspaceMode::PassThrough).unwrap(),
        "\"pass_through\""
    );
    assert_eq!(
        serde_json::to_string(&WorkspaceMode::Staged).unwrap(),
        "\"staged\""
    );
}

#[test]
fn test_workspace_spec_roundtrip() {
    let spec = WorkspaceSpec {
        root: "/tmp".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into()],
        exclude: vec![],
    };
    roundtrip_value(&spec);
}

#[test]
fn test_context_packet_roundtrip() {
    let pkt = ContextPacket {
        files: vec!["a.rs".into()],
        snippets: vec![ContextSnippet {
            name: "s".into(),
            content: "c".into(),
        }],
    };
    roundtrip_value(&pkt);
}

#[test]
fn test_context_snippet_roundtrip() {
    let s = ContextSnippet {
        name: "test".into(),
        content: "body".into(),
    };
    roundtrip_value(&s);
}

#[test]
fn test_runtime_config_default_roundtrip() {
    let cfg = RuntimeConfig::default();
    roundtrip_value(&cfg);
}

#[test]
fn test_runtime_config_with_values() {
    let mut vendor = BTreeMap::new();
    vendor.insert("openai".into(), json!({"model": "gpt-4"}));
    let mut env = BTreeMap::new();
    env.insert("API_KEY".into(), "secret".into());
    let cfg = RuntimeConfig {
        model: Some("gpt-4".into()),
        vendor,
        env,
        max_budget_usd: Some(1.50),
        max_turns: Some(10),
    };
    roundtrip_value(&cfg);
}

#[test]
fn test_runtime_config_missing_optional_fields() {
    let json = r#"{"vendor":{},"env":{}}"#;
    let cfg: RuntimeConfig = serde_json::from_str(json).unwrap();
    assert!(cfg.model.is_none());
    assert!(cfg.max_budget_usd.is_none());
    assert!(cfg.max_turns.is_none());
}

#[test]
fn test_policy_profile_default_roundtrip() {
    let pp = PolicyProfile::default();
    roundtrip_value(&pp);
}

#[test]
fn test_policy_profile_with_values() {
    let pp = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["rm".into()],
        deny_read: vec!["secret/**".into()],
        deny_write: vec!["/etc/**".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["bash".into()],
    };
    roundtrip_value(&pp);
}

// ── Capabilities ───────────────────────────────────────────────────────

#[test]
fn test_capability_all_variants_roundtrip() {
    let variants = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
        Capability::FunctionCalling,
        Capability::Vision,
        Capability::Audio,
        Capability::JsonMode,
        Capability::SystemMessage,
        Capability::Temperature,
        Capability::TopP,
        Capability::TopK,
        Capability::MaxTokens,
        Capability::FrequencyPenalty,
        Capability::PresencePenalty,
        Capability::CacheControl,
        Capability::BatchMode,
        Capability::Embeddings,
        Capability::ImageGeneration,
    ];
    for cap in &variants {
        roundtrip(cap);
    }
}

#[test]
fn test_capability_snake_case_format() {
    assert_eq!(
        serde_json::to_string(&Capability::ToolWebSearch).unwrap(),
        "\"tool_web_search\""
    );
    assert_eq!(
        serde_json::to_string(&Capability::StructuredOutputJsonSchema).unwrap(),
        "\"structured_output_json_schema\""
    );
    assert_eq!(
        serde_json::to_string(&Capability::McpClient).unwrap(),
        "\"mcp_client\""
    );
}

#[test]
fn test_support_level_all_variants() {
    roundtrip_value(&SupportLevel::Native);
    roundtrip_value(&SupportLevel::Emulated);
    roundtrip_value(&SupportLevel::Unsupported);
    roundtrip_value(&SupportLevel::Restricted {
        reason: "needs key".into(),
    });
}

#[test]
fn test_support_level_restricted_format() {
    let json = serde_json::to_string(&SupportLevel::Restricted {
        reason: "key".into(),
    })
    .unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    // Externally tagged: {"restricted": {"reason": "key"}}
    assert_eq!(v["restricted"]["reason"], "key");
}

#[test]
fn test_min_support_roundtrip() {
    roundtrip_value(&MinSupport::Native);
    roundtrip_value(&MinSupport::Emulated);
}

#[test]
fn test_capability_requirement_roundtrip() {
    let cr = CapabilityRequirement {
        capability: Capability::Streaming,
        min_support: MinSupport::Native,
    };
    roundtrip_value(&cr);
}

#[test]
fn test_capability_requirements_default() {
    let cr = CapabilityRequirements::default();
    roundtrip_value(&cr);
}

#[test]
fn test_capability_manifest_btreemap_order() {
    let mut manifest: BTreeMap<Capability, SupportLevel> = BTreeMap::new();
    manifest.insert(Capability::Vision, SupportLevel::Native);
    manifest.insert(Capability::Audio, SupportLevel::Unsupported);
    manifest.insert(Capability::Streaming, SupportLevel::Emulated);

    let json = serde_json::to_string(&manifest).unwrap();
    // BTreeMap keys are sorted - verify ordering in serialized output
    let v: Value = serde_json::from_str(&json).unwrap();
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    assert_eq!(keys, sorted_keys, "BTreeMap keys should be sorted");
}

// ── Execution Mode ─────────────────────────────────────────────────────

#[test]
fn test_execution_mode_roundtrip() {
    roundtrip(&ExecutionMode::Passthrough);
    roundtrip(&ExecutionMode::Mapped);
}

#[test]
fn test_execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn test_execution_mode_snake_case() {
    assert_eq!(
        serde_json::to_string(&ExecutionMode::Passthrough).unwrap(),
        "\"passthrough\""
    );
    assert_eq!(
        serde_json::to_string(&ExecutionMode::Mapped).unwrap(),
        "\"mapped\""
    );
}

// ── Backend Identity ───────────────────────────────────────────────────

#[test]
fn test_backend_identity_roundtrip() {
    roundtrip_value(&make_backend_identity());
}

#[test]
fn test_backend_identity_optional_fields() {
    let json = r#"{"id":"test"}"#;
    let bi: BackendIdentity = serde_json::from_str(json).unwrap();
    assert_eq!(bi.id, "test");
    assert!(bi.backend_version.is_none());
    assert!(bi.adapter_version.is_none());
}

// ── Outcome ────────────────────────────────────────────────────────────

#[test]
fn test_outcome_all_variants() {
    roundtrip(&Outcome::Complete);
    roundtrip(&Outcome::Partial);
    roundtrip(&Outcome::Failed);
}

#[test]
fn test_outcome_snake_case() {
    assert_eq!(
        serde_json::to_string(&Outcome::Complete).unwrap(),
        "\"complete\""
    );
    assert_eq!(
        serde_json::to_string(&Outcome::Partial).unwrap(),
        "\"partial\""
    );
    assert_eq!(
        serde_json::to_string(&Outcome::Failed).unwrap(),
        "\"failed\""
    );
}

// ── Usage Normalized ───────────────────────────────────────────────────

#[test]
fn test_usage_normalized_default() {
    let u = UsageNormalized::default();
    roundtrip_value(&u);
}

#[test]
fn test_usage_normalized_with_values() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(50),
        cache_write_tokens: Some(25),
        request_units: Some(1),
        estimated_cost_usd: Some(0.003),
    };
    roundtrip_value(&u);
}

// ── ArtifactRef ────────────────────────────────────────────────────────

#[test]
fn test_artifact_ref_roundtrip() {
    let a = ArtifactRef {
        kind: "patch".into(),
        path: "output.diff".into(),
    };
    roundtrip_value(&a);
}

// ── VerificationReport ─────────────────────────────────────────────────

#[test]
fn test_verification_report_default() {
    let vr = VerificationReport::default();
    roundtrip_value(&vr);
}

#[test]
fn test_verification_report_with_data() {
    let vr = VerificationReport {
        git_diff: Some("diff --git a/f b/f\n".into()),
        git_status: Some("M f\n".into()),
        harness_ok: true,
    };
    roundtrip_value(&vr);
}

// ── RunMetadata ────────────────────────────────────────────────────────

#[test]
fn test_run_metadata_roundtrip() {
    roundtrip_value(&make_run_metadata());
}

// ── Receipt ────────────────────────────────────────────────────────────

#[test]
fn test_receipt_roundtrip() {
    roundtrip_value(&make_receipt());
}

#[test]
fn test_receipt_with_hash() {
    let mut r = make_receipt();
    r.receipt_sha256 = Some("abc123".into());
    roundtrip_value(&r);
}

// ── AgentEvent & AgentEventKind ────────────────────────────────────────

#[test]
fn test_agent_event_run_started() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    assert_json_has_key(&json, "type");
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "run_started");
    roundtrip_value(&e);
}

#[test]
fn test_agent_event_run_completed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    };
    roundtrip_value(&e);
}

#[test]
fn test_agent_event_assistant_delta() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "assistant_delta");
    roundtrip_value(&e);
}

#[test]
fn test_agent_event_assistant_message() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "Full msg".into(),
        },
        ext: None,
    };
    roundtrip_value(&e);
}

#[test]
fn test_agent_event_tool_call() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "main.rs"}),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "tool_call");
    roundtrip_value(&e);
}

#[test]
fn test_agent_event_tool_result() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            output: json!("file content"),
            is_error: false,
        },
        ext: None,
    };
    roundtrip_value(&e);
}

#[test]
fn test_agent_event_file_changed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added fn".into(),
        },
        ext: None,
    };
    roundtrip_value(&e);
}

#[test]
fn test_agent_event_command_executed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
        ext: None,
    };
    roundtrip_value(&e);
}

#[test]
fn test_agent_event_warning() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Warning {
            message: "slow".into(),
        },
        ext: None,
    };
    roundtrip_value(&e);
}

#[test]
fn test_agent_event_error() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Error {
            message: "boom".into(),
            error_code: Some(abp_error::ErrorCode::BackendTimeout),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["error_code"], "backend_timeout");
    roundtrip_value(&e);
}

#[test]
fn test_agent_event_error_without_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Error {
            message: "boom".into(),
            error_code: None,
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    // error_code should be skipped when None
    let v: Value = serde_json::from_str(&json).unwrap();
    assert!(v.get("error_code").is_none());
    roundtrip_value(&e);
}

#[test]
fn test_agent_event_with_ext() {
    let mut ext = BTreeMap::new();
    ext.insert("custom".into(), json!(42));
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Warning {
            message: "x".into(),
        },
        ext: Some(ext),
    };
    roundtrip_value(&e);
}

#[test]
fn test_agent_event_ext_omitted_when_none() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Warning {
            message: "x".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert!(v.get("ext").is_none(), "ext should be skipped when None");
}

#[test]
fn test_agent_event_kind_tagged_format() {
    let kind = AgentEventKind::RunStarted {
        message: "go".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "run_started");
    assert_eq!(v["message"], "go");
}

// ── abp-core::error::ErrorCode ────────────────────────────────────────

#[test]
fn test_error_code_all_contract_variants() {
    let codes = vec![
        ErrorCode::InvalidContractVersion,
        ErrorCode::MalformedWorkOrder,
        ErrorCode::MalformedReceipt,
        ErrorCode::InvalidHash,
        ErrorCode::MissingRequiredField,
        ErrorCode::InvalidWorkOrderId,
        ErrorCode::InvalidRunId,
        ErrorCode::DuplicateWorkOrderId,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::InvalidOutcome,
        ErrorCode::InvalidExecutionLane,
        ErrorCode::InvalidExecutionMode,
    ];
    for code in &codes {
        roundtrip(code);
    }
}

#[test]
fn test_error_code_all_protocol_variants() {
    let codes = vec![
        ErrorCode::InvalidEnvelope,
        ErrorCode::HandshakeFailed,
        ErrorCode::UnexpectedMessage,
        ErrorCode::VersionMismatch,
        ErrorCode::MalformedJsonl,
        ErrorCode::InvalidRefId,
        ErrorCode::EnvelopeTooLarge,
        ErrorCode::MissingEnvelopeField,
        ErrorCode::InvalidEnvelopeTag,
        ErrorCode::ProtocolTimeout,
        ErrorCode::DuplicateHello,
        ErrorCode::UnexpectedFinal,
    ];
    for code in &codes {
        roundtrip(code);
    }
}

#[test]
fn test_error_code_all_policy_variants() {
    let codes = vec![
        ErrorCode::ToolDenied,
        ErrorCode::ReadDenied,
        ErrorCode::WriteDenied,
        ErrorCode::PolicyCompilationFailed,
        ErrorCode::CapabilityNotSupported,
        ErrorCode::NetworkDenied,
        ErrorCode::ApprovalRequired,
        ErrorCode::PolicyViolation,
        ErrorCode::InvalidGlobPattern,
        ErrorCode::ToolNotRegistered,
        ErrorCode::PathTraversal,
    ];
    for code in &codes {
        roundtrip(code);
    }
}

#[test]
fn test_error_code_all_runtime_variants() {
    let codes = vec![
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::EventStreamClosed,
        ErrorCode::RunCancelled,
        ErrorCode::SidecarCrashed,
        ErrorCode::SidecarSpawnFailed,
        ErrorCode::WorkspaceCleanupFailed,
        ErrorCode::MaxTurnsExceeded,
        ErrorCode::BudgetExceeded,
        ErrorCode::BackendMismatch,
        ErrorCode::RunAlreadyCompleted,
        ErrorCode::NoBackendRegistered,
    ];
    for code in &codes {
        roundtrip(code);
    }
}

#[test]
fn test_error_code_all_system_variants() {
    let codes = vec![
        ErrorCode::IoError,
        ErrorCode::SerializationError,
        ErrorCode::InternalError,
        ErrorCode::ConfigurationError,
        ErrorCode::ResourceExhausted,
        ErrorCode::Utf8Error,
        ErrorCode::TaskJoinError,
        ErrorCode::ChannelClosed,
        ErrorCode::InvalidArgument,
        ErrorCode::PermissionDenied,
        ErrorCode::NotImplemented,
    ];
    for code in &codes {
        roundtrip(code);
    }
}

#[test]
fn test_error_code_snake_case_format() {
    assert_eq!(
        serde_json::to_string(&ErrorCode::InvalidContractVersion).unwrap(),
        "\"invalid_contract_version\""
    );
    assert_eq!(
        serde_json::to_string(&ErrorCode::BackendUnavailable).unwrap(),
        "\"backend_unavailable\""
    );
    assert_eq!(
        serde_json::to_string(&ErrorCode::IoError).unwrap(),
        "\"io_error\""
    );
}

// ── abp-error::ErrorCode (used in AgentEventKind) ─────────────────────

#[test]
fn test_abp_error_code_roundtrip() {
    let codes = vec![
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
        abp_error::ErrorCode::BackendNotFound,
        abp_error::ErrorCode::BackendTimeout,
        abp_error::ErrorCode::PolicyDenied,
        abp_error::ErrorCode::Internal,
        abp_error::ErrorCode::CapabilityUnsupported,
        abp_error::ErrorCode::IrLoweringFailed,
        abp_error::ErrorCode::ReceiptHashMismatch,
        abp_error::ErrorCode::DialectUnknown,
        abp_error::ErrorCode::ConfigInvalid,
    ];
    for code in &codes {
        roundtrip(code);
    }
}

#[test]
fn test_abp_error_code_as_str_snake_case() {
    assert_eq!(
        abp_error::ErrorCode::BackendTimeout.as_str(),
        "backend_timeout"
    );
    assert_eq!(
        abp_error::ErrorCode::ProtocolInvalidEnvelope.as_str(),
        "protocol_invalid_envelope"
    );
}

#[test]
fn test_abp_error_category_roundtrip() {
    let cats = vec![
        abp_error::ErrorCategory::Protocol,
        abp_error::ErrorCategory::Backend,
        abp_error::ErrorCategory::Capability,
        abp_error::ErrorCategory::Policy,
        abp_error::ErrorCategory::Workspace,
        abp_error::ErrorCategory::Ir,
        abp_error::ErrorCategory::Receipt,
        abp_error::ErrorCategory::Dialect,
        abp_error::ErrorCategory::Config,
        abp_error::ErrorCategory::Mapping,
        abp_error::ErrorCategory::Execution,
        abp_error::ErrorCategory::Contract,
        abp_error::ErrorCategory::Internal,
    ];
    for cat in &cats {
        roundtrip(cat);
    }
}

// ── IR types ───────────────────────────────────────────────────────────

#[test]
fn test_ir_role_all_variants() {
    roundtrip(&IrRole::System);
    roundtrip(&IrRole::User);
    roundtrip(&IrRole::Assistant);
    roundtrip(&IrRole::Tool);
}

#[test]
fn test_ir_role_snake_case() {
    assert_eq!(
        serde_json::to_string(&IrRole::System).unwrap(),
        "\"system\""
    );
    assert_eq!(
        serde_json::to_string(&IrRole::Assistant).unwrap(),
        "\"assistant\""
    );
}

#[test]
fn test_ir_content_block_text() {
    let b = IrContentBlock::Text {
        text: "hello".into(),
    };
    roundtrip(&b);
    let json = serde_json::to_string(&b).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "text");
}

#[test]
fn test_ir_content_block_image() {
    let b = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "base64data".into(),
    };
    roundtrip(&b);
}

#[test]
fn test_ir_content_block_tool_use() {
    let b = IrContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "read".into(),
        input: json!({"path": "f.rs"}),
    };
    roundtrip(&b);
    let json = serde_json::to_string(&b).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "tool_use");
}

#[test]
fn test_ir_content_block_tool_result() {
    let b = IrContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: vec![IrContentBlock::Text {
            text: "result".into(),
        }],
        is_error: false,
    };
    roundtrip(&b);
}

#[test]
fn test_ir_content_block_thinking() {
    let b = IrContentBlock::Thinking {
        text: "thinking...".into(),
    };
    roundtrip(&b);
    let json = serde_json::to_string(&b).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "thinking");
}

#[test]
fn test_ir_message_roundtrip() {
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata: BTreeMap::new(),
    };
    roundtrip(&msg);
}

#[test]
fn test_ir_message_metadata_btreemap_order() {
    let mut meta = BTreeMap::new();
    meta.insert("z_key".into(), json!("z"));
    meta.insert("a_key".into(), json!("a"));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![],
        metadata: meta,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let a_pos = json.find("a_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(a_pos < z_pos, "BTreeMap keys should be sorted");
}

#[test]
fn test_ir_message_empty_metadata_skipped() {
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![],
        metadata: BTreeMap::new(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert!(
        v.get("metadata").is_none(),
        "empty metadata should be skipped"
    );
}

#[test]
fn test_ir_conversation_roundtrip() {
    let conv = IrConversation {
        messages: vec![IrMessage {
            role: IrRole::System,
            content: vec![IrContentBlock::Text { text: "sys".into() }],
            metadata: BTreeMap::new(),
        }],
    };
    roundtrip_value(&conv);
}

#[test]
fn test_ir_tool_definition_roundtrip() {
    let td = IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    roundtrip_value(&td);
}

#[test]
fn test_ir_usage_roundtrip() {
    let u = IrUsage {
        input_tokens: 100,
        output_tokens: 200,
        total_tokens: 300,
        cache_read_tokens: 10,
        cache_write_tokens: 5,
    };
    roundtrip(&u);
}

#[test]
fn test_ir_usage_default() {
    let u = IrUsage::default();
    assert_eq!(u.input_tokens, 0);
    roundtrip(&u);
}

// ── Negotiate types ────────────────────────────────────────────────────

#[test]
fn test_dialect_support_level_all_variants() {
    let native = DialectSupportLevel::Native;
    roundtrip(&native);
    let json = serde_json::to_string(&native).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["level"], "native");

    let emu = DialectSupportLevel::Emulated {
        detail: "polyfill".into(),
    };
    roundtrip(&emu);
    let json = serde_json::to_string(&emu).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["level"], "emulated");

    let unsup = DialectSupportLevel::Unsupported {
        reason: "not available".into(),
    };
    roundtrip(&unsup);
    let json = serde_json::to_string(&unsup).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["level"], "unsupported");
}

#[test]
fn test_capability_report_entry_roundtrip() {
    let entry = CapabilityReportEntry {
        capability: Capability::Streaming,
        support: DialectSupportLevel::Native,
    };
    roundtrip_value(&entry);
}

#[test]
fn test_capability_report_roundtrip() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![CapabilityReportEntry {
            capability: Capability::ToolUse,
            support: DialectSupportLevel::Emulated {
                detail: "mapped".into(),
            },
        }],
    };
    roundtrip_value(&report);
}

// ── AggregationSummary ─────────────────────────────────────────────────

#[test]
fn test_aggregation_summary_roundtrip() {
    let mut by_kind = BTreeMap::new();
    by_kind.insert("tool_call".into(), 5);
    by_kind.insert("assistant_delta".into(), 10);
    let summary = AggregationSummary {
        total_events: 15,
        by_kind,
        tool_calls: 5,
        unique_tools: 2,
        errors: 0,
        total_text_chars: 1234,
        duration_ms: Some(500),
    };
    roundtrip(&summary);
}

#[test]
fn test_aggregation_summary_btreemap_order() {
    let mut by_kind = BTreeMap::new();
    by_kind.insert("z_kind".into(), 1);
    by_kind.insert("a_kind".into(), 2);
    let summary = AggregationSummary {
        total_events: 3,
        by_kind,
        tool_calls: 0,
        unique_tools: 0,
        errors: 0,
        total_text_chars: 0,
        duration_ms: None,
    };
    let json = serde_json::to_string(&summary).unwrap();
    let a_pos = json.find("a_kind").unwrap();
    let z_pos = json.find("z_kind").unwrap();
    assert!(a_pos < z_pos);
}

// ── Chain / Verify types ───────────────────────────────────────────────

#[test]
fn test_chain_entry_roundtrip() {
    let entry = ChainEntry {
        receipt: make_receipt(),
        parent_id: Some(test_uuid()),
    };
    roundtrip_value(&entry);
}

#[test]
fn test_chain_entry_no_parent() {
    let entry = ChainEntry {
        receipt: make_receipt(),
        parent_id: None,
    };
    roundtrip_value(&entry);
}

#[test]
fn test_chain_error_all_variants() {
    let errors = vec![
        ChainError::BrokenHash {
            index: 0,
            run_id: test_uuid(),
        },
        ChainError::MissingParent {
            index: 1,
            parent_id: test_uuid(),
        },
        ChainError::OutOfOrder { index: 2 },
        ChainError::DuplicateId { id: test_uuid() },
        ChainError::ContractVersionMismatch {
            index: 0,
            expected: "abp/v0.1".into(),
            actual: "abp/v0.2".into(),
        },
    ];
    for err in &errors {
        roundtrip_value(err);
    }
}

#[test]
fn test_chain_error_tagged_format() {
    let err = ChainError::BrokenHash {
        index: 0,
        run_id: test_uuid(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "broken_hash");
}

#[test]
fn test_chain_verification_roundtrip() {
    let cv = ChainVerification {
        valid: true,
        errors: vec![],
        chain_length: 3,
        total_events: 42,
        total_duration_ms: 1000,
    };
    roundtrip_value(&cv);
}

#[test]
fn test_receipt_chain_roundtrip() {
    let chain = ReceiptChain::default();
    roundtrip_value(&chain);
}

// ── canonical_json ─────────────────────────────────────────────────────

#[test]
fn test_canonical_json_simple_types() {
    let val = json!({"b": 2, "a": 1});
    let canon = abp_core::canonical_json(&val).unwrap();
    let normal = serde_json::to_string(&val).unwrap();
    // Both should produce same output since serde_json::Value uses BTreeMap
    assert_eq!(canon, normal);
}

#[test]
fn test_canonical_json_struct() {
    let bi = make_backend_identity();
    let canon = abp_core::canonical_json(&bi).unwrap();
    let normal = serde_json::to_string(&serde_json::to_value(&bi).unwrap()).unwrap();
    assert_eq!(canon, normal);
}

// =========================================================================
// abp-protocol
// =========================================================================

#[test]
fn test_envelope_hello_roundtrip() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: make_backend_identity(),
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
    };
    roundtrip_value(&env);
}

#[test]
fn test_envelope_hello_tag() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: make_backend_identity(),
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
    };
    let json = serde_json::to_string(&env).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["t"], "hello", "Envelope tag field must be 't'");
}

#[test]
fn test_envelope_run_roundtrip() {
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: make_work_order(),
    };
    roundtrip_value(&env);
    let json = serde_json::to_string(&env).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["t"], "run");
}

#[test]
fn test_envelope_event_roundtrip() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: None,
        },
    };
    roundtrip_value(&env);
    let json = serde_json::to_string(&env).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["t"], "event");
}

#[test]
fn test_envelope_final_roundtrip() {
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: make_receipt(),
    };
    roundtrip_value(&env);
    let json = serde_json::to_string(&env).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["t"], "final");
}

#[test]
fn test_envelope_fatal_roundtrip() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "something bad".into(),
        error_code: Some(abp_error::ErrorCode::BackendCrashed),
    };
    roundtrip_value(&env);
    let json = serde_json::to_string(&env).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["t"], "fatal");
    assert_eq!(v["error_code"], "backend_crashed");
}

#[test]
fn test_envelope_fatal_no_ref_id() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "early failure".into(),
        error_code: None,
    };
    roundtrip_value(&env);
}

#[test]
fn test_envelope_reject_missing_tag() {
    let json = r#"{"contract_version":"abp/v0.1","backend":{"id":"x"},"capabilities":{}}"#;
    assert_deser_fails::<Envelope>(json);
}

// ── Protocol batch types ───────────────────────────────────────────────

#[test]
fn test_batch_item_status_all_variants() {
    roundtrip(&BatchItemStatus::Success);
    roundtrip(&BatchItemStatus::Failed {
        error: "oops".into(),
    });
    roundtrip(&BatchItemStatus::Skipped {
        reason: "n/a".into(),
    });
}

#[test]
fn test_batch_item_status_tagged_format() {
    let s = BatchItemStatus::Failed {
        error: "err".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "failed");
}

#[test]
fn test_batch_request_roundtrip() {
    let req = BatchRequest {
        id: "batch-1".into(),
        envelopes: vec![Envelope::Fatal {
            ref_id: None,
            error: "test".into(),
            error_code: None,
        }],
        created_at: "2024-01-01T00:00:00Z".into(),
    };
    roundtrip_value(&req);
}

#[test]
fn test_batch_result_roundtrip() {
    let r = BatchResult {
        index: 0,
        status: BatchItemStatus::Success,
        envelope: None,
    };
    roundtrip_value(&r);
}

#[test]
fn test_batch_response_roundtrip() {
    let resp = BatchResponse {
        request_id: "batch-1".into(),
        results: vec![BatchResult {
            index: 0,
            status: BatchItemStatus::Success,
            envelope: None,
        }],
        total_duration_ms: 100,
    };
    roundtrip_value(&resp);
}

// ── Protocol compression ───────────────────────────────────────────────

#[test]
fn test_compression_algorithm_all_variants() {
    roundtrip(&CompressionAlgorithm::None);
    roundtrip(&CompressionAlgorithm::Gzip);
    roundtrip(&CompressionAlgorithm::Zstd);
}

#[test]
fn test_compression_algorithm_snake_case() {
    assert_eq!(
        serde_json::to_string(&CompressionAlgorithm::None).unwrap(),
        "\"none\""
    );
    assert_eq!(
        serde_json::to_string(&CompressionAlgorithm::Gzip).unwrap(),
        "\"gzip\""
    );
    assert_eq!(
        serde_json::to_string(&CompressionAlgorithm::Zstd).unwrap(),
        "\"zstd\""
    );
}

#[test]
fn test_compressed_message_roundtrip() {
    let msg = CompressedMessage {
        algorithm: CompressionAlgorithm::Gzip,
        original_size: 1000,
        compressed_size: 500,
        data: vec![1, 2, 3],
    };
    roundtrip_value(&msg);
}

// ── Protocol routing ───────────────────────────────────────────────────

#[test]
fn test_message_route_roundtrip() {
    let route = MessageRoute {
        pattern: "event.*".into(),
        destination: "logger".into(),
        priority: 10,
    };
    roundtrip_value(&route);
}

#[test]
fn test_route_table_roundtrip() {
    let rt = RouteTable::default();
    roundtrip_value(&rt);
}

// ── Protocol version ───────────────────────────────────────────────────

#[test]
fn test_protocol_version_roundtrip() {
    let v = ProtocolVersion { major: 0, minor: 1 };
    roundtrip(&v);
}

#[test]
fn test_version_range_roundtrip() {
    let vr = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 1, minor: 0 },
    };
    roundtrip(&vr);
}

// =========================================================================
// abp-host
// =========================================================================

#[test]
fn test_sidecar_spec_roundtrip() {
    let spec = SidecarSpec {
        command: "node".into(),
        args: vec!["index.js".into()],
        env: BTreeMap::new(),
        cwd: Some("/tmp".into()),
    };
    roundtrip_value(&spec);
}

#[test]
fn test_sidecar_hello_roundtrip() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: make_backend_identity(),
        capabilities: BTreeMap::new(),
    };
    roundtrip_value(&hello);
}

#[test]
fn test_health_status_all_variants() {
    let variants: Vec<HealthStatus> = vec![
        HealthStatus::Healthy,
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        HealthStatus::Unknown,
    ];
    for v in &variants {
        roundtrip(v);
    }
}

#[test]
fn test_health_status_tagged_format() {
    let h = HealthStatus::Degraded {
        reason: "lag".into(),
    };
    let json = serde_json::to_string(&h).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["status"], "degraded");
    assert_eq!(v["reason"], "lag");
}

#[test]
fn test_health_check_roundtrip() {
    let hc = HealthCheck {
        name: "sidecar-1".into(),
        status: HealthStatus::Healthy,
        last_checked: ts(),
        response_time: Some(std::time::Duration::from_millis(50)),
        consecutive_failures: 0,
    };
    roundtrip_value(&hc);
}

#[test]
fn test_health_check_duration_as_millis() {
    let hc = HealthCheck {
        name: "test".into(),
        status: HealthStatus::Healthy,
        last_checked: ts(),
        response_time: Some(std::time::Duration::from_millis(123)),
        consecutive_failures: 0,
    };
    let json = serde_json::to_string(&hc).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["response_time"], 123);
}

#[test]
fn test_health_report_roundtrip() {
    let report = HealthReport {
        overall: HealthStatus::Healthy,
        checks: vec![],
        generated_at: ts(),
    };
    roundtrip_value(&report);
}

#[test]
fn test_lifecycle_state_all_variants() {
    let variants = vec![
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
        LifecycleState::Stopped,
        LifecycleState::Failed,
    ];
    for v in &variants {
        roundtrip(v);
    }
}

#[test]
fn test_lifecycle_state_snake_case() {
    assert_eq!(
        serde_json::to_string(&LifecycleState::Uninitialized).unwrap(),
        "\"uninitialized\""
    );
    assert_eq!(
        serde_json::to_string(&LifecycleState::Running).unwrap(),
        "\"running\""
    );
}

#[test]
fn test_lifecycle_transition_roundtrip() {
    let t = LifecycleTransition {
        from: LifecycleState::Starting,
        to: LifecycleState::Ready,
        timestamp: "2024-01-01T00:00:00Z".into(),
        reason: Some("init complete".into()),
    };
    roundtrip_value(&t);
}

#[test]
fn test_pool_config_roundtrip() {
    let pc = PoolConfig {
        min_size: 1,
        max_size: 10,
        idle_timeout: std::time::Duration::from_secs(30),
        health_check_interval: std::time::Duration::from_secs(5),
    };
    roundtrip_value(&pc);
}

#[test]
fn test_pool_config_duration_millis() {
    let pc = PoolConfig {
        min_size: 1,
        max_size: 5,
        idle_timeout: std::time::Duration::from_secs(60),
        health_check_interval: std::time::Duration::from_secs(10),
    };
    let json = serde_json::to_string(&pc).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["idle_timeout"], 60000);
    assert_eq!(v["health_check_interval"], 10000);
}

#[test]
fn test_pool_stats_roundtrip() {
    let ps = PoolStats {
        total: 5,
        idle: 2,
        busy: 3,
        draining: 0,
        failed: 0,
    };
    roundtrip(&ps);
}

#[test]
fn test_process_status_all_variants() {
    let variants: Vec<ProcessStatus> = vec![
        ProcessStatus::NotStarted,
        ProcessStatus::Running { pid: 12345 },
        ProcessStatus::Exited { code: 0 },
        ProcessStatus::Killed,
        ProcessStatus::TimedOut,
    ];
    for v in &variants {
        roundtrip(v);
    }
}

#[test]
fn test_process_status_tagged_format() {
    let s = ProcessStatus::Running { pid: 42 };
    let json = serde_json::to_string(&s).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["state"], "running");
    assert_eq!(v["pid"], 42);
}

#[test]
fn test_process_config_roundtrip() {
    let pc = ProcessConfig {
        working_dir: Some(PathBuf::from("/tmp")),
        env_vars: BTreeMap::new(),
        timeout: Some(std::time::Duration::from_secs(30)),
        inherit_env: true,
    };
    roundtrip_value(&pc);
}

#[test]
fn test_process_info_roundtrip() {
    let pi = ProcessInfo {
        spec: SidecarSpec {
            command: "python".into(),
            args: vec![],
            env: BTreeMap::new(),
            cwd: None,
        },
        config: ProcessConfig {
            working_dir: None,
            env_vars: BTreeMap::new(),
            timeout: None,
            inherit_env: true,
        },
        status: ProcessStatus::NotStarted,
        started_at: None,
        ended_at: None,
    };
    roundtrip_value(&pi);
}

#[test]
fn test_sidecar_config_roundtrip() {
    let sc = SidecarConfig {
        name: "node-sidecar".into(),
        command: "node".into(),
        args: vec!["index.js".into()],
        env: BTreeMap::new(),
        working_dir: None,
    };
    roundtrip_value(&sc);
}

#[test]
fn test_retry_config_roundtrip() {
    let rc = RetryConfig {
        max_retries: 3,
        base_delay: std::time::Duration::from_millis(100),
        max_delay: std::time::Duration::from_secs(10),
        overall_timeout: std::time::Duration::from_secs(60),
        jitter_factor: 0.5,
    };
    roundtrip_value(&rc);
}

#[test]
fn test_retry_attempt_roundtrip() {
    let ra = RetryAttempt {
        attempt: 1,
        error: "timeout".into(),
        delay: std::time::Duration::from_millis(200),
    };
    roundtrip_value(&ra);
}

#[test]
fn test_retry_metadata_roundtrip() {
    let rm = RetryMetadata {
        total_attempts: 2,
        failed_attempts: vec![RetryAttempt {
            attempt: 1,
            error: "err".into(),
            delay: std::time::Duration::from_millis(100),
        }],
        total_duration: std::time::Duration::from_millis(500),
    };
    roundtrip_value(&rm);
}

// =========================================================================
// abp-policy
// =========================================================================

#[test]
fn test_decision_roundtrip() {
    let d = Decision {
        allowed: true,
        reason: Some("ok".into()),
    };
    roundtrip_value(&d);
}

#[test]
fn test_decision_no_reason() {
    let d = Decision {
        allowed: false,
        reason: None,
    };
    roundtrip_value(&d);
}

#[test]
fn test_rate_limit_result_all_variants() {
    roundtrip(&RateLimitResult::Allowed);
    roundtrip(&RateLimitResult::Throttled {
        retry_after_ms: 1000,
    });
    roundtrip(&RateLimitResult::Denied {
        reason: "quota".into(),
    });
}

#[test]
fn test_rate_limit_result_tagged_format() {
    let r = RateLimitResult::Throttled {
        retry_after_ms: 500,
    };
    let json = serde_json::to_string(&r).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "throttled");
}

#[test]
fn test_rate_limit_policy_roundtrip() {
    let p = RateLimitPolicy {
        max_requests_per_minute: Some(60),
        max_tokens_per_minute: Some(100000),
        max_concurrent: Some(5),
    };
    roundtrip_value(&p);
}

#[test]
fn test_rate_limit_policy_default() {
    let p = RateLimitPolicy::default();
    roundtrip_value(&p);
}

#[test]
fn test_rule_condition_all_variants() {
    let variants: Vec<RuleCondition> = vec![
        RuleCondition::Always,
        RuleCondition::Never,
        RuleCondition::Pattern("*.rs".into()),
        RuleCondition::And(vec![RuleCondition::Always, RuleCondition::Never]),
        RuleCondition::Or(vec![RuleCondition::Always]),
        RuleCondition::Not(Box::new(RuleCondition::Always)),
    ];
    for v in &variants {
        roundtrip_value(v);
    }
}

#[test]
fn test_rule_effect_all_variants() {
    roundtrip(&RuleEffect::Allow);
    roundtrip(&RuleEffect::Deny);
    roundtrip(&RuleEffect::Log);
    roundtrip(&RuleEffect::Throttle { max: 10 });
}

#[test]
fn test_rule_effect_tagged_format() {
    let e = RuleEffect::Throttle { max: 5 };
    let json = serde_json::to_string(&e).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "throttle");
    assert_eq!(v["max"], 5);
}

#[test]
fn test_rule_roundtrip() {
    let rule = Rule {
        id: "r1".into(),
        description: "Block writes to /etc".into(),
        condition: RuleCondition::Pattern("/etc/**".into()),
        effect: RuleEffect::Deny,
        priority: 100,
    };
    roundtrip_value(&rule);
}

#[test]
fn test_composition_strategy_all_variants() {
    roundtrip(&CompositionStrategy::AllMustAllow);
    roundtrip(&CompositionStrategy::AnyMustAllow);
    roundtrip(&CompositionStrategy::FirstMatch);
}

#[test]
fn test_composition_strategy_default() {
    assert_eq!(
        CompositionStrategy::default(),
        CompositionStrategy::AllMustAllow
    );
}

#[test]
fn test_composed_result_all_variants() {
    roundtrip(&ComposedResult::Allowed {
        by: "policy-a".into(),
    });
    roundtrip(&ComposedResult::Denied {
        by: "policy-b".into(),
        reason: "blocked".into(),
    });
}

#[test]
fn test_composed_result_tagged_format() {
    let r = ComposedResult::Denied {
        by: "p".into(),
        reason: "r".into(),
    };
    let json = serde_json::to_string(&r).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "denied");
}

#[test]
fn test_compose_policy_decision_all_variants() {
    roundtrip(&ComposePolicyDecision::Allow {
        reason: "ok".into(),
    });
    roundtrip(&ComposePolicyDecision::Deny {
        reason: "no".into(),
    });
    roundtrip(&ComposePolicyDecision::Abstain);
}

#[test]
fn test_policy_precedence_all_variants() {
    roundtrip(&PolicyPrecedence::DenyOverrides);
    roundtrip(&PolicyPrecedence::AllowOverrides);
    roundtrip(&PolicyPrecedence::FirstApplicable);
}

#[test]
fn test_policy_precedence_default() {
    assert_eq!(PolicyPrecedence::default(), PolicyPrecedence::DenyOverrides);
}

// ── Audit ──────────────────────────────────────────────────────────────

#[test]
fn test_audit_policy_decision_all_variants() {
    roundtrip(&AuditPolicyDecision::Allow);
    roundtrip(&AuditPolicyDecision::Deny {
        reason: "no".into(),
    });
    roundtrip(&AuditPolicyDecision::Warn {
        reason: "maybe".into(),
    });
}

#[test]
fn test_audit_policy_decision_tagged() {
    let d = AuditPolicyDecision::Deny {
        reason: "blocked".into(),
    };
    let json = serde_json::to_string(&d).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "deny");
}

#[test]
fn test_audit_action_all_variants() {
    let actions = vec![
        AuditAction::ToolAllowed,
        AuditAction::ToolDenied,
        AuditAction::ReadAllowed,
        AuditAction::ReadDenied,
        AuditAction::WriteAllowed,
        AuditAction::WriteDenied,
        AuditAction::RateLimited,
    ];
    for a in &actions {
        roundtrip(a);
    }
}

#[test]
fn test_audit_action_snake_case() {
    assert_eq!(
        serde_json::to_string(&AuditAction::ToolAllowed).unwrap(),
        "\"tool_allowed\""
    );
    assert_eq!(
        serde_json::to_string(&AuditAction::RateLimited).unwrap(),
        "\"rate_limited\""
    );
}

#[test]
fn test_audit_entry_roundtrip() {
    let entry = AuditEntry {
        timestamp: ts(),
        action: "tool_use".into(),
        resource: "read_file".into(),
        decision: AuditPolicyDecision::Allow,
    };
    roundtrip_value(&entry);
}

#[test]
fn test_audit_record_roundtrip() {
    let record = AuditRecord {
        timestamp: "2024-01-01T00:00:00Z".into(),
        action: AuditAction::ToolAllowed,
        resource: "bash".into(),
        policy_name: Some("default".into()),
        reason: None,
    };
    roundtrip_value(&record);
}

#[test]
fn test_audit_log_roundtrip() {
    let log = AuditLog::default();
    roundtrip_value(&log);
}

// =========================================================================
// abp-runtime
// =========================================================================

#[test]
fn test_execution_config_default() {
    let ec = ExecutionConfig::default();
    roundtrip_value(&ec);
}

#[test]
fn test_execution_config_with_retry() {
    let ec = ExecutionConfig {
        retry_policy: Some(RetryPolicy {
            max_retries: 3,
            initial_backoff: std::time::Duration::from_millis(100),
            max_backoff: std::time::Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }),
        fallback_chain: None,
    };
    roundtrip_value(&ec);
}

#[test]
fn test_pipeline_event_all_variants() {
    let events: Vec<PipelineEvent> = vec![
        PipelineEvent::Retry {
            attempt: 1,
            backend: "mock".into(),
            delay_ms: 100,
            reason: "timeout".into(),
        },
        PipelineEvent::Fallback {
            from_backend: "a".into(),
            to_backend: "b".into(),
            reason: "unavailable".into(),
        },
        PipelineEvent::Success {
            backend: "mock".into(),
            attempts: 1,
        },
    ];
    for e in &events {
        roundtrip(e);
    }
}

#[test]
fn test_pipeline_event_tagged_format() {
    let e = PipelineEvent::Retry {
        attempt: 1,
        backend: "x".into(),
        delay_ms: 50,
        reason: "r".into(),
    };
    let json = serde_json::to_string(&e).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "retry");
}

#[test]
fn test_pipeline_output_roundtrip() {
    let po = PipelineOutput {
        receipt: make_receipt(),
        backend: "mock".into(),
        events: vec![PipelineEvent::Success {
            backend: "mock".into(),
            attempts: 1,
        }],
    };
    roundtrip_value(&po);
}

#[test]
fn test_retry_policy_roundtrip() {
    let rp = RetryPolicy {
        max_retries: 5,
        initial_backoff: std::time::Duration::from_millis(200),
        max_backoff: std::time::Duration::from_secs(60),
        backoff_multiplier: 1.5,
    };
    roundtrip(&rp);
}

#[test]
fn test_retry_policy_duration_millis() {
    let rp = RetryPolicy {
        max_retries: 1,
        initial_backoff: std::time::Duration::from_millis(500),
        max_backoff: std::time::Duration::from_secs(10),
        backoff_multiplier: 2.0,
    };
    let json = serde_json::to_string(&rp).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["initial_backoff"], 500);
    assert_eq!(v["max_backoff"], 10000);
}

#[test]
fn test_timeout_config_default() {
    let tc = TimeoutConfig::default();
    roundtrip(&tc);
}

#[test]
fn test_timeout_config_with_values() {
    let tc = TimeoutConfig {
        run_timeout: Some(std::time::Duration::from_secs(300)),
        event_timeout: Some(std::time::Duration::from_secs(30)),
    };
    roundtrip(&tc);
}

#[test]
fn test_fallback_chain_roundtrip() {
    let json = r#"{"backends":["openai","anthropic","gemini"]}"#;
    let fc: FallbackChain = serde_json::from_str(json).unwrap();
    roundtrip_value(&fc);
}

#[test]
fn test_budget_limit_default() {
    let bl = BudgetLimit::default();
    roundtrip_value(&bl);
}

#[test]
fn test_budget_limit_with_values() {
    let bl = BudgetLimit {
        max_tokens: Some(100000),
        max_cost_usd: Some(5.0),
        max_turns: Some(50),
        max_duration: Some(std::time::Duration::from_secs(600)),
    };
    roundtrip_value(&bl);
}

#[test]
fn test_span_status_all_variants() {
    roundtrip(&SpanStatus::Ok);
    roundtrip(&SpanStatus::Error {
        message: "fail".into(),
    });
    roundtrip(&SpanStatus::Unset);
}

#[test]
fn test_span_status_tagged_format() {
    let s = SpanStatus::Error {
        message: "bad".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "error");
}

#[test]
fn test_span_roundtrip() {
    let span = Span {
        id: "span-1".into(),
        name: "process_event".into(),
        parent_id: None,
        start_time: "2024-01-01T00:00:00Z".into(),
        end_time: Some("2024-01-01T00:00:01Z".into()),
        attributes: BTreeMap::new(),
        status: SpanStatus::Ok,
    };
    roundtrip_value(&span);
}

#[test]
fn test_observability_summary_roundtrip() {
    let s = ObservabilitySummary {
        total_spans: 10,
        active_spans: 2,
        error_spans: 1,
        metrics_count: 5,
    };
    roundtrip(&s);
}

#[test]
fn test_cancellation_reason_all_variants() {
    let reasons = vec![
        CancellationReason::UserRequested,
        CancellationReason::Timeout,
        CancellationReason::BudgetExhausted,
        CancellationReason::PolicyViolation,
        CancellationReason::SystemShutdown,
    ];
    for r in &reasons {
        roundtrip(r);
    }
}

#[test]
fn test_cancellation_reason_snake_case() {
    assert_eq!(
        serde_json::to_string(&CancellationReason::UserRequested).unwrap(),
        "\"user_requested\""
    );
    assert_eq!(
        serde_json::to_string(&CancellationReason::BudgetExhausted).unwrap(),
        "\"budget_exhausted\""
    );
}

#[test]
fn test_metrics_snapshot_serialize() {
    // MetricsSnapshot only implements Serialize, not Deserialize
    let ms = MetricsSnapshot {
        total_runs: 100,
        successful_runs: 90,
        failed_runs: 10,
        total_events: 500,
        average_run_duration_ms: 1500,
    };
    let json = serde_json::to_string(&ms).unwrap();
    assert_json_has_key(&json, "total_runs");
    assert_json_has_key(&json, "successful_runs");
    assert_json_has_key(&json, "average_run_duration_ms");
}

// =========================================================================
// abp-workspace
// =========================================================================

// ── tracker ────────────────────────────────────────────────────────────

#[test]
fn test_change_kind_all_variants() {
    let variants = vec![
        ChangeKind::Created,
        ChangeKind::Modified,
        ChangeKind::Deleted,
        ChangeKind::Renamed {
            from: "old.rs".into(),
        },
    ];
    for v in &variants {
        roundtrip(v);
    }
}

#[test]
fn test_change_kind_tagged_format() {
    let k = ChangeKind::Renamed {
        from: "old.rs".into(),
    };
    let json = serde_json::to_string(&k).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "renamed");
    assert_eq!(v["from"], "old.rs");
}

#[test]
fn test_tracker_file_change_roundtrip() {
    let fc = TrackerFileChange {
        path: "src/lib.rs".into(),
        kind: ChangeKind::Modified,
        size_before: Some(100),
        size_after: Some(150),
        content_hash: Some("abc123".into()),
    };
    roundtrip(&fc);
}

#[test]
fn test_change_summary_roundtrip() {
    let cs = ChangeSummary {
        created: 2,
        modified: 3,
        deleted: 1,
        renamed: 0,
        total_size_delta: 500,
    };
    roundtrip(&cs);
}

#[test]
fn test_change_summary_default() {
    let cs = ChangeSummary::default();
    roundtrip(&cs);
}

// ── snapshot ───────────────────────────────────────────────────────────

#[test]
fn test_file_snapshot_roundtrip() {
    let fs = FileSnapshot {
        size: 1024,
        sha256: "deadbeef".into(),
        is_binary: false,
    };
    roundtrip_value(&fs);
}

#[test]
fn test_workspace_snapshot_roundtrip() {
    let mut files = BTreeMap::new();
    files.insert(
        PathBuf::from("src/main.rs"),
        FileSnapshot {
            size: 100,
            sha256: "abc".into(),
            is_binary: false,
        },
    );
    let ws = WorkspaceSnapshot {
        files,
        created_at: ts(),
        root: PathBuf::from("/tmp/ws"),
    };
    roundtrip_value(&ws);
}

#[test]
fn test_snapshot_diff_default() {
    let sd = SnapshotDiff::default();
    roundtrip_value(&sd);
}

#[test]
fn test_snapshot_diff_with_data() {
    let sd = SnapshotDiff {
        added: vec![PathBuf::from("new.rs")],
        removed: vec![PathBuf::from("old.rs")],
        modified: vec![PathBuf::from("changed.rs")],
        unchanged: vec![PathBuf::from("same.rs")],
    };
    roundtrip_value(&sd);
}

// ── template ───────────────────────────────────────────────────────────

#[test]
fn test_workspace_template_roundtrip() {
    let mut files = BTreeMap::new();
    files.insert(PathBuf::from("main.rs"), "fn main() {}".into());
    let tmpl = WorkspaceTemplate {
        name: "rust-hello".into(),
        description: "A hello world template".into(),
        files,
        globs: None,
    };
    roundtrip_value(&tmpl);
}

// ── ops ────────────────────────────────────────────────────────────────

#[test]
fn test_file_operation_all_variants() {
    let ops = vec![
        FileOperation::Read {
            path: "f.rs".into(),
        },
        FileOperation::Write {
            path: "f.rs".into(),
            size: 100,
        },
        FileOperation::Delete {
            path: "f.rs".into(),
        },
        FileOperation::Move {
            from: "a.rs".into(),
            to: "b.rs".into(),
        },
        FileOperation::Copy {
            from: "a.rs".into(),
            to: "b.rs".into(),
        },
        FileOperation::CreateDir {
            path: "src/".into(),
        },
    ];
    for op in &ops {
        roundtrip_value(op);
    }
}

#[test]
fn test_file_operation_tagged_format() {
    let op = FileOperation::Write {
        path: "f.rs".into(),
        size: 42,
    };
    let json = serde_json::to_string(&op).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "write");
}

#[test]
fn test_operation_summary_roundtrip() {
    let os = OperationSummary {
        reads: 5,
        writes: 3,
        deletes: 1,
        moves: 0,
        copies: 0,
        create_dirs: 2,
        total_writes_bytes: 1000,
    };
    roundtrip(&os);
}

#[test]
fn test_operation_summary_default() {
    let os = OperationSummary::default();
    roundtrip(&os);
}

// ── diff ───────────────────────────────────────────────────────────────

#[test]
fn test_diff_change_kind_all_variants() {
    let kinds = vec![
        DiffChangeKind::Added,
        DiffChangeKind::Modified,
        DiffChangeKind::Deleted,
        DiffChangeKind::Renamed,
    ];
    for k in &kinds {
        roundtrip(k);
    }
}

#[test]
fn test_change_type_all_variants() {
    roundtrip(&ChangeType::Added);
    roundtrip(&ChangeType::Modified);
    roundtrip(&ChangeType::Deleted);
}

#[test]
fn test_diff_line_kind_all_variants() {
    roundtrip(&DiffLineKind::Context);
    roundtrip(&DiffLineKind::Added);
    roundtrip(&DiffLineKind::Removed);
    roundtrip(&DiffLineKind::NoNewlineMarker);
}

#[test]
fn test_diff_line_roundtrip() {
    let dl = DiffLine {
        kind: DiffLineKind::Added,
        content: "+fn main() {}".into(),
    };
    roundtrip(&dl);
}

#[test]
fn test_diff_hunk_roundtrip() {
    let hunk = DiffHunk {
        old_start: 1,
        old_count: 3,
        new_start: 1,
        new_count: 4,
        header: "@@ -1,3 +1,4 @@".into(),
        lines: vec![DiffLine {
            kind: DiffLineKind::Added,
            content: "+new line".into(),
        }],
    };
    roundtrip(&hunk);
}

#[test]
fn test_file_type_roundtrip_sample() {
    let types = vec![
        FileType::Rust,
        FileType::JavaScript,
        FileType::TypeScript,
        FileType::Python,
        FileType::Json,
        FileType::Toml,
        FileType::Markdown,
        FileType::Binary,
        FileType::Other,
    ];
    for ft in &types {
        roundtrip(ft);
    }
}

#[test]
fn test_file_category_all_variants() {
    let cats = vec![
        FileCategory::SourceCode,
        FileCategory::Config,
        FileCategory::Documentation,
        FileCategory::Tests,
        FileCategory::Assets,
        FileCategory::Build,
        FileCategory::CiCd,
        FileCategory::Other,
    ];
    for c in &cats {
        roundtrip(c);
    }
}

#[test]
fn test_risk_level_all_variants() {
    roundtrip(&RiskLevel::Low);
    roundtrip(&RiskLevel::Medium);
    roundtrip(&RiskLevel::High);
}

#[test]
fn test_file_diff_roundtrip() {
    let fd = FileDiff {
        path: "src/lib.rs".into(),
        change_kind: DiffChangeKind::Modified,
        is_binary: false,
        hunks: vec![],
        additions: 5,
        deletions: 2,
        old_mode: None,
        new_mode: None,
        file_type: FileType::Rust,
        renamed_from: None,
    };
    roundtrip(&fd);
}

#[test]
fn test_file_stats_roundtrip() {
    let fs = FileStats {
        path: "main.rs".into(),
        additions: 10,
        deletions: 3,
        is_binary: false,
        file_type: FileType::Rust,
        change_kind: DiffChangeKind::Modified,
    };
    roundtrip(&fs);
}

#[test]
fn test_diff_analysis_default() {
    let da = DiffAnalysis::default();
    roundtrip(&da);
}

#[test]
fn test_diff_summary_roundtrip() {
    let ds = DiffSummary {
        added: vec![PathBuf::from("new.rs")],
        modified: vec![],
        deleted: vec![],
        total_additions: 10,
        total_deletions: 0,
    };
    roundtrip(&ds);
}

#[test]
fn test_workspace_diff_roundtrip() {
    let wd = WorkspaceDiff::default();
    roundtrip(&wd);
}

#[test]
fn test_diff_policy_roundtrip() {
    let dp = DiffPolicy {
        max_files: Some(100),
        max_additions: Some(5000),
        denied_paths: vec!["secret/**".into()],
    };
    roundtrip_value(&dp);
}

#[test]
fn test_policy_result_all_variants() {
    roundtrip(&PolicyResult::Pass);
    roundtrip(&PolicyResult::Fail {
        violations: vec!["too many files".into()],
    });
}

#[test]
fn test_policy_result_tagged_format() {
    let r = PolicyResult::Fail {
        violations: vec!["x".into()],
    };
    let json = serde_json::to_string(&r).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["result"], "fail");
}

#[test]
fn test_file_breakdown_roundtrip() {
    let fb = FileBreakdown {
        path: "src/lib.rs".into(),
        change_kind: DiffChangeKind::Modified,
        category: FileCategory::SourceCode,
        additions: 10,
        deletions: 5,
        is_binary: false,
        is_security_sensitive: false,
        is_large: false,
        risk: RiskLevel::Low,
    };
    roundtrip(&fb);
}

#[test]
fn test_diff_report_roundtrip() {
    let mut categories = BTreeMap::new();
    categories.insert(FileCategory::SourceCode, 3);
    categories.insert(FileCategory::Config, 1);
    let report = DiffReport {
        files: vec![],
        total_additions: 20,
        total_deletions: 5,
        total_files: 4,
        risk_level: RiskLevel::Low,
        summary_text: "4 files changed".into(),
        categories,
        has_security_sensitive_changes: false,
        large_change_count: 0,
    };
    roundtrip_value(&report);
}

#[test]
fn test_diff_report_categories_btreemap_order() {
    let mut categories = BTreeMap::new();
    categories.insert(FileCategory::Tests, 2);
    categories.insert(FileCategory::Assets, 1);
    categories.insert(FileCategory::SourceCode, 5);
    let report = DiffReport {
        files: vec![],
        total_additions: 0,
        total_deletions: 0,
        total_files: 8,
        risk_level: RiskLevel::Low,
        summary_text: "".into(),
        categories,
        has_security_sensitive_changes: false,
        large_change_count: 0,
    };
    let json = serde_json::to_string(&report).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let keys: Vec<String> = v["categories"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "BTreeMap keys should be sorted");
}

// =========================================================================
// Deserialization rejection tests (missing required fields)
// =========================================================================

#[test]
fn test_reject_work_order_missing_task() {
    let json = json!({
        "id": "00000000-0000-0000-0000-000000000001",
        "lane": "patch_first",
        "workspace": {"root": "/", "mode": "staged", "include": [], "exclude": []},
        "context": {"files": [], "snippets": []},
        "policy": {"allowed_tools": [], "disallowed_tools": [], "deny_read": [], "deny_write": [], "allow_network": [], "deny_network": [], "require_approval_for": []},
        "requirements": {"required": []},
        "config": {"vendor": {}, "env": {}}
    });
    assert_deser_fails::<WorkOrder>(&json.to_string());
}

#[test]
fn test_reject_backend_identity_missing_id() {
    let json = r#"{"backend_version": "1.0"}"#;
    assert_deser_fails::<BackendIdentity>(json);
}

#[test]
fn test_reject_run_metadata_missing_fields() {
    let json = r#"{"run_id": "00000000-0000-0000-0000-000000000001"}"#;
    assert_deser_fails::<RunMetadata>(json);
}

#[test]
fn test_reject_agent_event_kind_unknown_type() {
    let json = r#"{"type": "unknown_kind", "data": "x"}"#;
    assert_deser_fails::<AgentEventKind>(json);
}

#[test]
fn test_reject_envelope_invalid_tag() {
    let json = r#"{"t": "invalid_tag"}"#;
    assert_deser_fails::<Envelope>(json);
}

#[test]
fn test_reject_ir_content_block_unknown_type() {
    let json = r#"{"type": "video", "url": "x"}"#;
    assert_deser_fails::<IrContentBlock>(json);
}

#[test]
fn test_reject_health_status_unknown_tag() {
    let json = r#"{"status": "exploded"}"#;
    assert_deser_fails::<HealthStatus>(json);
}

#[test]
fn test_reject_process_status_unknown_state() {
    let json = r#"{"state": "zombie"}"#;
    assert_deser_fails::<ProcessStatus>(json);
}

// =========================================================================
// Backwards compatibility (old format → new type)
// =========================================================================

#[test]
fn test_backwards_compat_runtime_config_no_model() {
    // Old format may not include model field
    let json = r#"{"vendor": {}, "env": {}}"#;
    let cfg: RuntimeConfig = serde_json::from_str(json).unwrap();
    assert!(cfg.model.is_none());
}

#[test]
fn test_backwards_compat_verification_report_defaults() {
    // Old format may have fewer fields
    let json = r#"{"harness_ok": true}"#;
    let vr: VerificationReport = serde_json::from_str(json).unwrap();
    assert!(vr.harness_ok);
    assert!(vr.git_diff.is_none());
}

#[test]
fn test_backwards_compat_agent_event_no_ext() {
    // Older events have no ext field
    let json = json!({
        "ts": "2024-01-01T00:00:00Z",
        "type": "warning",
        "message": "slow"
    });
    let e: AgentEvent = serde_json::from_str(&json.to_string()).unwrap();
    assert!(e.ext.is_none());
}

#[test]
fn test_backwards_compat_execution_mode_default() {
    // If mode is missing, should default to Mapped
    let json = json!({
        "contract_version": "abp/v0.1",
        "backend": {"id": "test"},
        "capabilities": {},
        "t": "hello"
    });
    let env: Envelope = serde_json::from_str(&json.to_string()).unwrap();
    match env {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn test_backwards_compat_usage_normalized_all_none() {
    let json = r#"{}"#;
    let u: UsageNormalized = serde_json::from_str(json).unwrap();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
}

// =========================================================================
// Nested type roundtrips
// =========================================================================

#[test]
fn test_nested_envelope_with_receipt_and_events() {
    let receipt = Receipt {
        meta: make_run_metadata(),
        backend: make_backend_identity(),
        capabilities: {
            let mut m = BTreeMap::new();
            m.insert(Capability::Streaming, SupportLevel::Native);
            m.insert(Capability::ToolUse, SupportLevel::Emulated);
            m
        },
        mode: ExecutionMode::Passthrough,
        usage_raw: json!({"total_tokens": 300}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(200),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.01),
        },
        trace: vec![
            AgentEvent {
                ts: ts(),
                kind: AgentEventKind::RunStarted {
                    message: "go".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: ts(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "bash".into(),
                    tool_use_id: Some("tu1".into()),
                    parent_tool_use_id: None,
                    input: json!({"command": "ls"}),
                },
                ext: None,
            },
        ],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "out.diff".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff data".into()),
            git_status: Some("M file".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: Some("sha256hash".into()),
    };
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    roundtrip_value(&env);
}

#[test]
fn test_nested_work_order_full() {
    let wo = WorkOrder {
        id: test_uuid(),
        task: "Complex task".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/home/user/project".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into(), "tests/**".into()],
            exclude: vec!["target/**".into(), "node_modules/**".into()],
        },
        context: ContextPacket {
            files: vec!["README.md".into(), "Cargo.toml".into()],
            snippets: vec![
                ContextSnippet {
                    name: "instructions".into(),
                    content: "Fix the bug in parser".into(),
                },
                ContextSnippet {
                    name: "error_log".into(),
                    content: "thread 'main' panicked".into(),
                },
            ],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read_file".into(), "write_file".into()],
            disallowed_tools: vec!["execute_command".into()],
            deny_read: vec![".env".into()],
            deny_write: vec!["Cargo.lock".into()],
            allow_network: vec![],
            deny_network: vec!["*".into()],
            require_approval_for: vec!["write_file".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolWrite,
                    min_support: MinSupport::Emulated,
                },
            ],
        },
        config: RuntimeConfig {
            model: Some("claude-3-opus".into()),
            vendor: {
                let mut m = BTreeMap::new();
                m.insert("anthropic".into(), json!({"max_tokens": 4096}));
                m
            },
            env: {
                let mut m = BTreeMap::new();
                m.insert("RUST_LOG".into(), "debug".into());
                m
            },
            max_budget_usd: Some(2.0),
            max_turns: Some(20),
        },
    };
    roundtrip_value(&wo);
}

#[test]
fn test_nested_ir_conversation_with_tool_cycle() {
    let conv = IrConversation {
        messages: vec![
            IrMessage {
                role: IrRole::System,
                content: vec![IrContentBlock::Text {
                    text: "You are helpful.".into(),
                }],
                metadata: BTreeMap::new(),
            },
            IrMessage {
                role: IrRole::User,
                content: vec![IrContentBlock::Text {
                    text: "Read main.rs".into(),
                }],
                metadata: BTreeMap::new(),
            },
            IrMessage {
                role: IrRole::Assistant,
                content: vec![IrContentBlock::ToolUse {
                    id: "tu_1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "main.rs"}),
                }],
                metadata: BTreeMap::new(),
            },
            IrMessage {
                role: IrRole::Tool,
                content: vec![IrContentBlock::ToolResult {
                    tool_use_id: "tu_1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "fn main() {}".into(),
                    }],
                    is_error: false,
                }],
                metadata: BTreeMap::new(),
            },
        ],
    };
    roundtrip_value(&conv);
}
