// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive golden-file / snapshot tests for JSON serialization stability.
//!
//! Tests cover: WorkOrder, Receipt, AgentEvent, Envelope, PolicyProfile,
//! CapabilityManifest, Dialect, and full pipeline output snapshots.
//! Every test uses deterministic data (fixed UUIDs, timestamps) and
//! `insta::assert_json_snapshot!` for regression detection.

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet,
    ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec, CONTRACT_VERSION,
};
use abp_dialect::Dialect;
use abp_protocol::{Envelope, JsonlCodec};

// ===========================================================================
// Helpers
// ===========================================================================

fn ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 7, 1, 12, 0, 0).unwrap()
}

fn ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 7, 1, 12, 5, 0).unwrap()
}

fn uid1() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap()
}

fn uid2() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap()
}

fn uid3() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000003").unwrap()
}

fn backend_id() -> BackendIdentity {
    BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("3.0.0".into()),
        adapter_version: Some("1.2.0".into()),
    }
}

fn minimal_backend() -> BackendIdentity {
    BackendIdentity {
        id: "mock".into(),
        backend_version: None,
        adapter_version: None,
    }
}

fn small_caps() -> CapabilityManifest {
    let mut m = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolUse, SupportLevel::Emulated);
    m
}

fn full_caps() -> CapabilityManifest {
    let mut m = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Native);
    m.insert(Capability::ToolEdit, SupportLevel::Native);
    m.insert(Capability::ToolBash, SupportLevel::Native);
    m.insert(Capability::ToolGlob, SupportLevel::Native);
    m.insert(Capability::ToolGrep, SupportLevel::Native);
    m.insert(Capability::ToolUse, SupportLevel::Emulated);
    m.insert(Capability::ExtendedThinking, SupportLevel::Unsupported);
    m.insert(
        Capability::McpClient,
        SupportLevel::Restricted {
            reason: "experimental feature".into(),
        },
    );
    m
}

fn sample_policy() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into(), "glob".into()],
        disallowed_tools: vec!["bash".into(), "delete".into()],
        deny_read: vec![".env".into(), "secrets/**".into()],
        deny_write: vec!["Cargo.lock".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["execute".into()],
    }
}

fn sample_runtime_config() -> RuntimeConfig {
    RuntimeConfig {
        model: Some("gpt-4o".into()),
        vendor: {
            let mut m = BTreeMap::new();
            m.insert("temperature".into(), json!(0.7));
            m.insert("top_p".into(), json!(0.95));
            m
        },
        env: {
            let mut m = BTreeMap::new();
            m.insert("RUST_LOG".into(), "debug".into());
            m
        },
        max_budget_usd: Some(1.50),
        max_turns: Some(25),
    }
}

fn sample_context() -> ContextPacket {
    ContextPacket {
        files: vec!["src/main.rs".into(), "README.md".into()],
        snippets: vec![
            ContextSnippet {
                name: "hint".into(),
                content: "Look at the auth module".into(),
            },
            ContextSnippet {
                name: "constraint".into(),
                content: "Do not modify tests".into(),
            },
        ],
    }
}

fn sample_work_order() -> WorkOrder {
    WorkOrder {
        id: uid1(),
        task: "Refactor authentication module".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/workspace".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["node_modules/**".into(), "target/**".into()],
        },
        context: sample_context(),
        policy: sample_policy(),
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
        config: sample_runtime_config(),
    }
}

fn minimal_work_order() -> WorkOrder {
    WorkOrder {
        id: uid2(),
        task: "Hello world".into(),
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
    }
}

fn full_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: uid1(),
            work_order_id: uid2(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts(),
            finished_at: ts2(),
            duration_ms: 300_000,
        },
        backend: backend_id(),
        capabilities: full_caps(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"prompt_tokens": 1200, "completion_tokens": 800}),
        usage: UsageNormalized {
            input_tokens: Some(1200),
            output_tokens: Some(800),
            cache_read_tokens: Some(100),
            cache_write_tokens: Some(50),
            request_units: Some(2),
            estimated_cost_usd: Some(0.015),
        },
        trace: vec![
            AgentEvent {
                ts: ts(),
                kind: AgentEventKind::RunStarted {
                    message: "run started".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: ts2(),
                kind: AgentEventKind::RunCompleted {
                    message: "run completed".into(),
                },
                ext: None,
            },
        ],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff --git a/src/auth.rs b/src/auth.rs".into()),
            git_status: Some("M src/auth.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn minimal_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: uid3(),
            work_order_id: uid1(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts(),
            finished_at: ts(),
            duration_ms: 0,
        },
        backend: minimal_backend(),
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

// ===========================================================================
// 1. WorkOrder JSON structure stability
// ===========================================================================

#[test]
fn wo_full_structure() {
    insta::assert_json_snapshot!(sample_work_order());
}

#[test]
fn wo_minimal_structure() {
    insta::assert_json_snapshot!(minimal_work_order());
}

#[test]
fn wo_workspace_first_lane() {
    let mut wo = minimal_work_order();
    wo.lane = ExecutionLane::WorkspaceFirst;
    insta::assert_json_snapshot!(wo);
}

#[test]
fn wo_patch_first_lane() {
    let wo = minimal_work_order();
    insta::assert_json_snapshot!(wo.lane);
}

#[test]
fn wo_staged_workspace_mode() {
    insta::assert_json_snapshot!(WorkspaceMode::Staged);
}

#[test]
fn wo_passthrough_workspace_mode() {
    insta::assert_json_snapshot!(WorkspaceMode::PassThrough);
}

#[test]
fn wo_workspace_spec_full() {
    let spec = WorkspaceSpec {
        root: "/home/user/project".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into(), "lib/**".into()],
        exclude: vec!["target/**".into(), ".git/**".into()],
    };
    insta::assert_json_snapshot!(spec);
}

#[test]
fn wo_context_packet_empty() {
    insta::assert_json_snapshot!(ContextPacket::default());
}

#[test]
fn wo_context_packet_with_snippets() {
    insta::assert_json_snapshot!(sample_context());
}

#[test]
fn wo_runtime_config_default() {
    insta::assert_json_snapshot!(RuntimeConfig::default());
}

#[test]
fn wo_runtime_config_full() {
    insta::assert_json_snapshot!(sample_runtime_config());
}

#[test]
fn wo_builder_defaults() {
    let wo = WorkOrderBuilder::new("test task").build();
    insta::assert_json_snapshot!(wo, {
        ".id" => "[uuid]",
    });
}

#[test]
fn wo_builder_customized() {
    let wo = WorkOrderBuilder::new("custom task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/custom/root")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("claude-3-opus")
        .max_turns(50)
        .max_budget_usd(10.0)
        .build();
    insta::assert_json_snapshot!(wo, {
        ".id" => "[uuid]",
    });
}

#[test]
fn wo_capability_requirements_empty() {
    insta::assert_json_snapshot!(CapabilityRequirements::default());
}

#[test]
fn wo_capability_requirements_multiple() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Native,
            },
        ],
    };
    insta::assert_json_snapshot!(reqs);
}

// ===========================================================================
// 2. Receipt JSON structure stability
// ===========================================================================

#[test]
fn receipt_full_structure() {
    let v = serde_json::to_value(full_receipt()).unwrap();
    insta::assert_json_snapshot!(v);
}

#[test]
fn receipt_minimal_structure() {
    insta::assert_json_snapshot!(minimal_receipt());
}

#[test]
fn receipt_outcome_complete() {
    insta::assert_json_snapshot!(Outcome::Complete);
}

#[test]
fn receipt_outcome_partial() {
    insta::assert_json_snapshot!(Outcome::Partial);
}

#[test]
fn receipt_outcome_failed() {
    insta::assert_json_snapshot!(Outcome::Failed);
}

#[test]
fn receipt_with_hash_determinism() {
    let r1 = full_receipt().with_hash().unwrap();
    let r2 = full_receipt().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    insta::assert_json_snapshot!(r1.receipt_sha256);
}

#[test]
fn receipt_hash_is_64_hex_chars() {
    let r = full_receipt().with_hash().unwrap();
    let hash = r.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_passthrough_mode() {
    let mut r = minimal_receipt();
    r.mode = ExecutionMode::Passthrough;
    insta::assert_json_snapshot!(r);
}

#[test]
fn receipt_with_artifacts() {
    let mut r = minimal_receipt();
    r.artifacts = vec![
        ArtifactRef {
            kind: "patch".into(),
            path: "fix.patch".into(),
        },
        ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        },
        ArtifactRef {
            kind: "diff".into(),
            path: "changes.diff".into(),
        },
    ];
    insta::assert_json_snapshot!(r);
}

#[test]
fn receipt_with_verification() {
    let mut r = minimal_receipt();
    r.verification = VerificationReport {
        git_diff: Some("diff --git a/file.rs b/file.rs\n+new line".into()),
        git_status: Some("M file.rs\nA new_file.rs".into()),
        harness_ok: true,
    };
    insta::assert_json_snapshot!(r);
}

#[test]
fn receipt_with_full_usage() {
    let mut r = minimal_receipt();
    r.usage = UsageNormalized {
        input_tokens: Some(5000),
        output_tokens: Some(2000),
        cache_read_tokens: Some(500),
        cache_write_tokens: Some(100),
        request_units: Some(10),
        estimated_cost_usd: Some(0.123),
    };
    r.usage_raw = json!({
        "prompt_tokens": 5000,
        "completion_tokens": 2000,
        "total_tokens": 7000
    });
    insta::assert_json_snapshot!(r);
}

#[test]
fn receipt_builder_minimal() {
    let r = ReceiptBuilder::new("mock").build();
    insta::assert_json_snapshot!(r, {
        ".meta.run_id" => "[uuid]",
        ".meta.started_at" => "[timestamp]",
        ".meta.finished_at" => "[timestamp]",
    });
}

#[test]
fn receipt_builder_with_outcome() {
    let r = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Failed)
        .build();
    insta::assert_json_snapshot!(r, {
        ".meta.run_id" => "[uuid]",
        ".meta.started_at" => "[timestamp]",
        ".meta.finished_at" => "[timestamp]",
    });
}

#[test]
fn receipt_run_metadata() {
    let meta = RunMetadata {
        run_id: uid1(),
        work_order_id: uid2(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: ts(),
        finished_at: ts2(),
        duration_ms: 300_000,
    };
    insta::assert_json_snapshot!(meta);
}

#[test]
fn receipt_usage_normalized_default() {
    insta::assert_json_snapshot!(UsageNormalized::default());
}

#[test]
fn receipt_verification_default() {
    insta::assert_json_snapshot!(VerificationReport::default());
}

#[test]
fn receipt_artifact_ref() {
    insta::assert_json_snapshot!(ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    });
}

// ===========================================================================
// 3. AgentEvent variants JSON
// ===========================================================================

#[test]
fn event_run_started() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunStarted {
            message: "Starting execution".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_run_completed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunCompleted {
            message: "Execution finished successfully".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_assistant_delta() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantDelta {
            text: "Hello ".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_assistant_message() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "I'll help you refactor the auth module.".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_tool_call_basic() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_001".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/auth.rs"}),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_tool_call_nested() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("tu_002".into()),
            parent_tool_use_id: Some("tu_001".into()),
            input: json!({"path": "src/new.rs", "content": "fn main() {}"}),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_tool_call_no_ids() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({"command": "cargo test"}),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_tool_result_success() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_001".into()),
            output: json!({"content": "fn main() { println!(\"hello\"); }"}),
            is_error: false,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_tool_result_error() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_003".into()),
            output: json!({"error": "file not found"}),
            is_error: true,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_file_changed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::FileChanged {
            path: "src/auth.rs".into(),
            summary: "Added JWT validation function".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_command_executed_success() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok. 42 passed".into()),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_command_executed_failure() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(1),
            output_preview: Some("error[E0308]: mismatched types".into()),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_command_executed_no_exit_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "sleep 60".into(),
            exit_code: None,
            output_preview: None,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_warning() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Warning {
            message: "Approaching token budget limit".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_error_without_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Error {
            message: "Unexpected failure".into(),
            error_code: None,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_error_with_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Error {
            message: "Backend timed out".into(),
            error_code: Some(abp_error::ErrorCode::BackendTimeout),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_with_ext_passthrough() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"role": "assistant", "content": "hello"}));
    ext.insert("vendor_id".into(), json!("msg_abc123"));
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: Some(ext),
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn event_with_empty_ext() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    // Ensure ext is not serialized when None
    let json_str = serde_json::to_string(&e).unwrap();
    assert!(!json_str.contains("\"ext\""));
    insta::assert_json_snapshot!(e);
}

// ===========================================================================
// 4. Envelope variants JSON (with tag "t")
// ===========================================================================

#[test]
fn envelope_hello_default_mode() {
    let env = Envelope::hello(
        backend_id(),
        small_caps(),
    );
    let v = serde_json::to_value(env).unwrap();
    insta::assert_json_snapshot!(v);
}

#[test]
fn envelope_hello_passthrough_mode() {
    let env = Envelope::hello_with_mode(
        backend_id(),
        small_caps(),
        ExecutionMode::Passthrough,
    );
    let v = serde_json::to_value(env).unwrap();
    insta::assert_json_snapshot!(v);
}

#[test]
fn envelope_hello_empty_caps() {
    let env = Envelope::hello(
        minimal_backend(),
        BTreeMap::new(),
    );
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_hello_full_caps() {
    let env = Envelope::hello(
        backend_id(),
        full_caps(),
    );
    let v = serde_json::to_value(env).unwrap();
    insta::assert_json_snapshot!(v);
}

#[test]
fn envelope_run() {
    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: sample_work_order(),
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_run_minimal() {
    let env = Envelope::Run {
        id: "run-002".into(),
        work_order: minimal_work_order(),
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_event_assistant_message() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Working on it...".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_event_tool_call() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_100".into()),
                parent_tool_use_id: None,
                input: json!({"path": "main.rs"}),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_event_delta() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "token ".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_final_receipt() {
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: full_receipt(),
    };
    let v = serde_json::to_value(env).unwrap();
    insta::assert_json_snapshot!(v);
}

#[test]
fn envelope_final_minimal_receipt() {
    let env = Envelope::Final {
        ref_id: "run-002".into(),
        receipt: minimal_receipt(),
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_fatal_with_ref() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "Backend process crashed".into(),
        error_code: None,
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_fatal_without_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "Handshake timeout".into(),
        error_code: None,
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-001".into()),
        "Backend not found",
        abp_error::ErrorCode::BackendNotFound,
    );
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_fatal_from_abp_error() {
    let err = abp_error::AbpError::new(
        abp_error::ErrorCode::ProtocolVersionMismatch,
        "version mismatch: expected abp/v0.1",
    );
    let env = Envelope::fatal_from_abp_error(Some("run-003".into()), &err);
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_tag_is_t_not_type() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains("\"t\":"));
    assert!(!json.contains("\"type\":\"fatal\""));
}

#[test]
fn envelope_hello_tag_value() {
    let env = Envelope::hello(minimal_backend(), BTreeMap::new());
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains("\"t\":\"hello\""));
}

#[test]
fn envelope_run_tag_value() {
    let env = Envelope::Run {
        id: "r".into(),
        work_order: minimal_work_order(),
    };
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains("\"t\":\"run\""));
}

#[test]
fn envelope_event_tag_value() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::Warning {
                message: "warn".into(),
            },
            ext: None,
        },
    };
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains("\"t\":\"event\""));
}

#[test]
fn envelope_final_tag_value() {
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt: minimal_receipt(),
    };
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains("\"t\":\"final\""));
}

#[test]
fn envelope_fatal_tag_value() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains("\"t\":\"fatal\""));
}

// ===========================================================================
// 5. PolicyProfile JSON
// ===========================================================================

#[test]
fn policy_profile_empty() {
    insta::assert_json_snapshot!(PolicyProfile::default());
}

#[test]
fn policy_profile_full() {
    insta::assert_json_snapshot!(sample_policy());
}

#[test]
fn policy_profile_tools_only() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn policy_profile_paths_only() {
    let p = PolicyProfile {
        deny_read: vec![".env".into(), "secrets/**".into()],
        deny_write: vec!["Cargo.lock".into(), "*.toml".into()],
        ..Default::default()
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn policy_profile_network_only() {
    let p = PolicyProfile {
        allow_network: vec!["api.example.com".into(), "cdn.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        ..Default::default()
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn policy_profile_approval_only() {
    let p = PolicyProfile {
        require_approval_for: vec!["execute".into(), "delete".into(), "bash".into()],
        ..Default::default()
    };
    insta::assert_json_snapshot!(p);
}

// ===========================================================================
// 6. CapabilitySet JSON
// ===========================================================================

#[test]
fn capability_manifest_empty() {
    let m: CapabilityManifest = BTreeMap::new();
    insta::assert_json_snapshot!(m);
}

#[test]
fn capability_manifest_small() {
    // CapabilityManifest has enum keys; use assert_snapshot with pretty JSON
    let v = serde_json::to_value(small_caps()).unwrap();
    insta::assert_json_snapshot!(v);
}

#[test]
fn capability_manifest_full() {
    let v = serde_json::to_value(full_caps()).unwrap();
    insta::assert_json_snapshot!(v);
}

#[test]
fn capability_streaming() {
    insta::assert_json_snapshot!(Capability::Streaming);
}

#[test]
fn capability_tool_read() {
    insta::assert_json_snapshot!(Capability::ToolRead);
}

#[test]
fn capability_tool_write() {
    insta::assert_json_snapshot!(Capability::ToolWrite);
}

#[test]
fn capability_tool_edit() {
    insta::assert_json_snapshot!(Capability::ToolEdit);
}

#[test]
fn capability_tool_bash() {
    insta::assert_json_snapshot!(Capability::ToolBash);
}

#[test]
fn capability_extended_thinking() {
    insta::assert_json_snapshot!(Capability::ExtendedThinking);
}

#[test]
fn capability_mcp_client() {
    insta::assert_json_snapshot!(Capability::McpClient);
}

#[test]
fn capability_mcp_server() {
    insta::assert_json_snapshot!(Capability::McpServer);
}

#[test]
fn support_level_native() {
    insta::assert_json_snapshot!(SupportLevel::Native);
}

#[test]
fn support_level_emulated() {
    insta::assert_json_snapshot!(SupportLevel::Emulated);
}

#[test]
fn support_level_unsupported() {
    insta::assert_json_snapshot!(SupportLevel::Unsupported);
}

#[test]
fn support_level_restricted() {
    insta::assert_json_snapshot!(SupportLevel::Restricted {
        reason: "disabled by policy".into(),
    });
}

#[test]
fn min_support_native() {
    insta::assert_json_snapshot!(MinSupport::Native);
}

#[test]
fn min_support_emulated() {
    insta::assert_json_snapshot!(MinSupport::Emulated);
}

#[test]
fn backend_identity_full() {
    insta::assert_json_snapshot!(backend_id());
}

#[test]
fn backend_identity_minimal() {
    insta::assert_json_snapshot!(minimal_backend());
}

// ===========================================================================
// 7. Dialect enum serialization
// ===========================================================================

#[test]
fn dialect_openai() {
    insta::assert_json_snapshot!(Dialect::OpenAi);
}

#[test]
fn dialect_claude() {
    insta::assert_json_snapshot!(Dialect::Claude);
}

#[test]
fn dialect_gemini() {
    insta::assert_json_snapshot!(Dialect::Gemini);
}

#[test]
fn dialect_codex() {
    insta::assert_json_snapshot!(Dialect::Codex);
}

#[test]
fn dialect_kimi() {
    insta::assert_json_snapshot!(Dialect::Kimi);
}

#[test]
fn dialect_copilot() {
    insta::assert_json_snapshot!(Dialect::Copilot);
}

#[test]
fn dialect_all_variants() {
    insta::assert_json_snapshot!(Dialect::all());
}

#[test]
fn dialect_roundtrip_all() {
    for dialect in Dialect::all() {
        let json = serde_json::to_string(dialect).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(*dialect, back);
    }
}

// ===========================================================================
// 8. Full pipeline output snapshots
// ===========================================================================

#[test]
fn pipeline_hello_jsonl_encode() {
    let env = Envelope::hello(backend_id(), small_caps());
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.ends_with('\n'));
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn pipeline_run_jsonl_roundtrip() {
    let env = Envelope::Run {
        id: "run-pipeline-001".into(),
        work_order: sample_work_order(),
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Run { id, work_order } = decoded {
        assert_eq!(id, "run-pipeline-001");
        assert_eq!(work_order.task, "Refactor authentication module");
    } else {
        panic!("Expected Envelope::Run");
    }
}

#[test]
fn pipeline_event_stream_jsonl() {
    let events = vec![
        Envelope::Event {
            ref_id: "run-001".into(),
            event: AgentEvent {
                ts: ts(),
                kind: AgentEventKind::RunStarted {
                    message: "starting".into(),
                },
                ext: None,
            },
        },
        Envelope::Event {
            ref_id: "run-001".into(),
            event: AgentEvent {
                ts: ts(),
                kind: AgentEventKind::AssistantDelta {
                    text: "Hello ".into(),
                },
                ext: None,
            },
        },
        Envelope::Event {
            ref_id: "run-001".into(),
            event: AgentEvent {
                ts: ts(),
                kind: AgentEventKind::AssistantDelta {
                    text: "world!".into(),
                },
                ext: None,
            },
        },
        Envelope::Event {
            ref_id: "run-001".into(),
            event: AgentEvent {
                ts: ts2(),
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            },
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &events).unwrap();
    let output = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines.len(), 4);
    insta::assert_snapshot!(output);
}

#[test]
fn pipeline_full_session_transcript() {
    let hello = Envelope::hello(backend_id(), small_caps());
    let run = Envelope::Run {
        id: "session-001".into(),
        work_order: minimal_work_order(),
    };
    let event1 = Envelope::Event {
        ref_id: "session-001".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        },
    };
    let event2 = Envelope::Event {
        ref_id: "session-001".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Done".into(),
            },
            ext: None,
        },
    };
    let event3 = Envelope::Event {
        ref_id: "session-001".into(),
        event: AgentEvent {
            ts: ts2(),
            kind: AgentEventKind::RunCompleted {
                message: "complete".into(),
            },
            ext: None,
        },
    };
    let final_env = Envelope::Final {
        ref_id: "session-001".into(),
        receipt: minimal_receipt(),
    };

    let all = vec![hello, run, event1, event2, event3, final_env];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &all).unwrap();
    let output = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines.len(), 6);
    insta::assert_snapshot!(output);
}

#[test]
fn pipeline_fatal_session() {
    let hello = Envelope::hello(minimal_backend(), BTreeMap::new());
    let run = Envelope::Run {
        id: "fail-001".into(),
        work_order: minimal_work_order(),
    };
    let fatal = Envelope::Fatal {
        ref_id: Some("fail-001".into()),
        error: "Out of memory".into(),
        error_code: Some(abp_error::ErrorCode::BackendCrashed),
    };

    let all = vec![hello, run, fatal];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &all).unwrap();
    let output = String::from_utf8(buf).unwrap();
    insta::assert_snapshot!(output);
}

#[test]
fn pipeline_receipt_canonical_json_stability() {
    let r = full_receipt();
    let json1 = abp_core::canonical_json(&r).unwrap();
    let json2 = abp_core::canonical_json(&r).unwrap();
    assert_eq!(json1, json2, "canonical_json must be deterministic");
}

#[test]
fn pipeline_receipt_hash_excludes_sha256_field() {
    let r1 = full_receipt();
    let h1 = abp_core::receipt_hash(&r1).unwrap();

    let mut r2 = full_receipt();
    r2.receipt_sha256 = Some("should_be_ignored".into());
    let h2 = abp_core::receipt_hash(&r2).unwrap();

    assert_eq!(h1, h2, "receipt_sha256 must not affect the hash");
}

#[test]
fn pipeline_execution_mode_mapped_default() {
    let mode = ExecutionMode::default();
    assert_eq!(mode, ExecutionMode::Mapped);
    insta::assert_json_snapshot!(mode);
}

#[test]
fn pipeline_execution_mode_passthrough() {
    insta::assert_json_snapshot!(ExecutionMode::Passthrough);
}

#[test]
fn pipeline_contract_version() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    insta::assert_snapshot!(CONTRACT_VERSION);
}

#[test]
fn pipeline_execution_lane_values() {
    insta::assert_json_snapshot!("lane_patch_first", ExecutionLane::PatchFirst);
    insta::assert_json_snapshot!("lane_workspace_first", ExecutionLane::WorkspaceFirst);
}

#[test]
fn pipeline_decode_stream_roundtrip() {
    let envelopes = vec![
        Envelope::hello(minimal_backend(), BTreeMap::new()),
        Envelope::Fatal {
            ref_id: None,
            error: "test".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();

    let reader = std::io::BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 2);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Fatal { .. }));
}

#[test]
fn pipeline_error_codes_serialize() {
    let codes = vec![
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
        abp_error::ErrorCode::BackendTimeout,
        abp_error::ErrorCode::PolicyDenied,
        abp_error::ErrorCode::ReceiptHashMismatch,
        abp_error::ErrorCode::DialectUnknown,
        abp_error::ErrorCode::Internal,
    ];
    insta::assert_json_snapshot!(codes);
}

#[test]
fn pipeline_work_order_serde_roundtrip() {
    let wo = sample_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, wo.id);
    assert_eq!(back.task, wo.task);
}

#[test]
fn pipeline_receipt_serde_roundtrip() {
    let r = full_receipt();
    let json = serde_json::to_string(&r).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.meta.run_id, r.meta.run_id);
    assert_eq!(back.outcome, r.outcome);
}

#[test]
fn pipeline_envelope_serde_roundtrip_all_variants() {
    let variants: Vec<Envelope> = vec![
        Envelope::hello(minimal_backend(), BTreeMap::new()),
        Envelope::Run {
            id: "r1".into(),
            work_order: minimal_work_order(),
        },
        Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: ts(),
                kind: AgentEventKind::Warning {
                    message: "low budget".into(),
                },
                ext: None,
            },
        },
        Envelope::Final {
            ref_id: "r1".into(),
            receipt: minimal_receipt(),
        },
        Envelope::Fatal {
            ref_id: None,
            error: "crash".into(),
            error_code: None,
        },
    ];
    for env in &variants {
        let json = serde_json::to_string(env).unwrap();
        let _back: Envelope = serde_json::from_str(&json).unwrap();
    }
}
