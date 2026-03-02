// SPDX-License-Identifier: MIT OR Apache-2.0
//! Golden file / snapshot tests for ABP output formats.
//!
//! Uses `insta` to snapshot-test JSON serialization of all core contract types,
//! protocol envelopes, policy structures, capability manifests, error types,
//! receipt chains, and schema outputs.

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use insta::{assert_debug_snapshot, assert_json_snapshot, assert_snapshot};
use serde_json::json;
use uuid::Uuid;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_protocol::{Envelope, JsonlCodec};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fixed timestamp for deterministic snapshots.
fn ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 5, 0).unwrap()
}

/// Fixed UUIDs for deterministic snapshots.
fn uuid1() -> Uuid {
    Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
}

fn uuid2() -> Uuid {
    Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap()
}

fn uuid3() -> Uuid {
    Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap()
}

/// Build a minimal deterministic work order.
fn minimal_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid1(),
        task: "Fix the login bug".into(),
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
    }
}

/// Build a fully-populated work order.
fn full_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid1(),
        task: "Refactor auth module with tests".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/home/user/project".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**/*.rs".into(), "tests/**/*.rs".into()],
            exclude: vec!["target/**".into(), ".git/**".into()],
        },
        context: ContextPacket {
            files: vec!["src/auth.rs".into(), "README.md".into()],
            snippets: vec![ContextSnippet {
                name: "instructions".into(),
                content: "Follow Rust 2024 edition conventions".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["Read".into(), "Write".into(), "Bash".into()],
            disallowed_tools: vec!["WebSearch".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec!["**/.git/**".into()],
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
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Emulated,
                },
            ],
        },
        config: RuntimeConfig {
            model: Some("claude-sonnet-4-20250514".into()),
            vendor: BTreeMap::from([("temperature".into(), json!(0.7))]),
            env: BTreeMap::from([("RUST_LOG".into(), "debug".into())]),
            max_budget_usd: Some(5.0),
            max_turns: Some(25),
        },
    }
}

/// Build a minimal deterministic receipt.
fn minimal_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: uuid2(),
            work_order_id: uuid1(),
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
    }
}

/// Build a fully-populated deterministic receipt.
fn full_receipt() -> Receipt {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);

    Receipt {
        meta: RunMetadata {
            run_id: uuid2(),
            work_order_id: uuid1(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts(),
            finished_at: ts2(),
            duration_ms: 300_000,
        },
        backend: BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.2.3".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: caps,
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"prompt_tokens": 1500, "completion_tokens": 800}),
        usage: UsageNormalized {
            input_tokens: Some(1500),
            output_tokens: Some(800),
            cache_read_tokens: Some(200),
            cache_write_tokens: Some(50),
            request_units: None,
            estimated_cost_usd: Some(0.0125),
        },
        trace: vec![
            AgentEvent {
                ts: ts(),
                kind: AgentEventKind::RunStarted {
                    message: "Starting refactor".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: ts2(),
                kind: AgentEventKind::RunCompleted {
                    message: "Refactor done".into(),
                },
                ext: None,
            },
        ],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "output/changes.patch".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("+added line\n-removed line".into()),
            git_status: Some("M src/auth.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: ts(),
        kind,
        ext: None,
    }
}

// ===========================================================================
// 1. WorkOrder default JSON snapshot
// ===========================================================================

#[test]
fn golden_work_order_minimal() {
    let wo = minimal_work_order();
    assert_json_snapshot!("golden_work_order_minimal", wo, {
        ".id" => "[uuid]",
    });
}

#[test]
fn golden_work_order_builder_defaults() {
    let wo = WorkOrderBuilder::new("Hello world").build();
    assert_json_snapshot!("golden_work_order_builder_defaults", wo, {
        ".id" => "[uuid]",
    });
}

// ===========================================================================
// 2. WorkOrder with all fields JSON snapshot
// ===========================================================================

#[test]
fn golden_work_order_full() {
    let wo = full_work_order();
    assert_json_snapshot!("golden_work_order_full", wo, {
        ".id" => "[uuid]",
    });
}

#[test]
fn golden_work_order_workspace_first_lane() {
    let wo = WorkOrder {
        lane: ExecutionLane::WorkspaceFirst,
        ..minimal_work_order()
    };
    assert_json_snapshot!("golden_work_order_workspace_first_lane", wo, {
        ".id" => "[uuid]",
    });
}

#[test]
fn golden_work_order_passthrough_workspace() {
    let wo = WorkOrder {
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec!["**/*.py".into()],
            exclude: vec!["__pycache__/**".into()],
        },
        ..minimal_work_order()
    };
    assert_json_snapshot!("golden_work_order_passthrough_workspace", wo, {
        ".id" => "[uuid]",
    });
}

#[test]
fn golden_work_order_with_context() {
    let wo = WorkOrder {
        context: ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![
                ContextSnippet {
                    name: "style".into(),
                    content: "Use 4-space indentation".into(),
                },
                ContextSnippet {
                    name: "rules".into(),
                    content: "No unsafe code".into(),
                },
            ],
        },
        ..minimal_work_order()
    };
    assert_json_snapshot!("golden_work_order_with_context", wo, {
        ".id" => "[uuid]",
    });
}

#[test]
fn golden_work_order_with_requirements() {
    let wo = WorkOrder {
        requirements: CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolBash,
                    min_support: MinSupport::Emulated,
                },
                CapabilityRequirement {
                    capability: Capability::ExtendedThinking,
                    min_support: MinSupport::Native,
                },
            ],
        },
        ..minimal_work_order()
    };
    assert_json_snapshot!("golden_work_order_with_requirements", wo, {
        ".id" => "[uuid]",
    });
}

#[test]
fn golden_work_order_with_runtime_config() {
    let wo = WorkOrder {
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor: BTreeMap::from([("top_p".into(), json!(0.95))]),
            env: BTreeMap::new(),
            max_budget_usd: Some(10.0),
            max_turns: Some(50),
        },
        ..minimal_work_order()
    };
    assert_json_snapshot!("golden_work_order_with_runtime_config", wo, {
        ".id" => "[uuid]",
    });
}

// ===========================================================================
// 3. Receipt JSON snapshot
// ===========================================================================

#[test]
fn golden_receipt_minimal() {
    let r = minimal_receipt();
    assert_json_snapshot!("golden_receipt_minimal", r);
}

#[test]
fn golden_receipt_full() {
    let r = full_receipt();
    let json = serde_json::to_string_pretty(&r).unwrap();
    assert_snapshot!("golden_receipt_full", json);
}

#[test]
fn golden_receipt_outcome_partial() {
    let r = Receipt {
        outcome: Outcome::Partial,
        ..minimal_receipt()
    };
    assert_json_snapshot!("golden_receipt_outcome_partial", r);
}

#[test]
fn golden_receipt_outcome_failed() {
    let r = Receipt {
        outcome: Outcome::Failed,
        ..minimal_receipt()
    };
    assert_json_snapshot!("golden_receipt_outcome_failed", r);
}

#[test]
fn golden_receipt_passthrough_mode() {
    let r = Receipt {
        mode: ExecutionMode::Passthrough,
        ..minimal_receipt()
    };
    assert_json_snapshot!("golden_receipt_passthrough_mode", r);
}

#[test]
fn golden_receipt_with_artifacts() {
    let r = Receipt {
        artifacts: vec![
            ArtifactRef {
                kind: "patch".into(),
                path: "output/fix.patch".into(),
            },
            ArtifactRef {
                kind: "log".into(),
                path: "output/run.log".into(),
            },
        ],
        ..minimal_receipt()
    };
    assert_json_snapshot!("golden_receipt_with_artifacts", r);
}

#[test]
fn golden_receipt_with_verification() {
    let r = Receipt {
        verification: VerificationReport {
            git_diff: Some("diff --git a/src/main.rs".into()),
            git_status: Some("M src/main.rs\nA src/new.rs".into()),
            harness_ok: true,
        },
        ..minimal_receipt()
    };
    assert_json_snapshot!("golden_receipt_with_verification", r);
}

#[test]
fn golden_receipt_with_usage() {
    let r = Receipt {
        usage: UsageNormalized {
            input_tokens: Some(2000),
            output_tokens: Some(1000),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: Some(5),
            estimated_cost_usd: Some(0.05),
        },
        usage_raw: json!({"total_tokens": 3000}),
        ..minimal_receipt()
    };
    assert_json_snapshot!("golden_receipt_with_usage", r);
}

// ===========================================================================
// 4. Receipt with hash JSON snapshot
// ===========================================================================

#[test]
fn golden_receipt_with_hash() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
    assert_json_snapshot!("golden_receipt_with_hash", r, {
        ".receipt_sha256" => "[sha256]",
    });
}

#[test]
fn golden_receipt_hash_determinism() {
    let r1 = minimal_receipt().with_hash().unwrap();
    let r2 = minimal_receipt().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    assert_snapshot!("golden_receipt_hash_value", r1.receipt_sha256.unwrap());
}

#[test]
fn golden_receipt_full_with_hash() {
    let r = full_receipt().with_hash().unwrap();
    let json = serde_json::to_string_pretty(&r).unwrap();
    // Redact hash for snapshot stability
    let redacted = json
        .lines()
        .map(|line| {
            if line.contains("receipt_sha256") && line.contains('"') && line.len() > 30 {
                line.split(':')
                    .next()
                    .map(|prefix| format!("{prefix}: \"[sha256]\""))
                    .unwrap_or_else(|| line.to_string())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_snapshot!("golden_receipt_full_with_hash", redacted);
}

// ===========================================================================
// 5. AgentEvent for each variant JSON snapshot
// ===========================================================================

#[test]
fn golden_event_run_started() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "Beginning task execution".into(),
    });
    assert_json_snapshot!("golden_event_run_started", e);
}

#[test]
fn golden_event_run_completed() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "Task completed successfully".into(),
    });
    assert_json_snapshot!("golden_event_run_completed", e);
}

#[test]
fn golden_event_assistant_delta() {
    let e = make_event(AgentEventKind::AssistantDelta {
        text: "Hello".into(),
    });
    assert_json_snapshot!("golden_event_assistant_delta", e);
}

#[test]
fn golden_event_assistant_message() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "I'll fix the login bug by updating the authentication handler.".into(),
    });
    assert_json_snapshot!("golden_event_assistant_message", e);
}

#[test]
fn golden_event_tool_call() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "Read".into(),
        tool_use_id: Some("tu_001".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/auth.rs"}),
    });
    assert_json_snapshot!("golden_event_tool_call", e);
}

#[test]
fn golden_event_tool_call_nested() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "Bash".into(),
        tool_use_id: Some("tu_002".into()),
        parent_tool_use_id: Some("tu_001".into()),
        input: json!({"command": "cargo test"}),
    });
    assert_json_snapshot!("golden_event_tool_call_nested", e);
}

#[test]
fn golden_event_tool_result_success() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "Read".into(),
        tool_use_id: Some("tu_001".into()),
        output: json!({"content": "fn main() {}"}),
        is_error: false,
    });
    assert_json_snapshot!("golden_event_tool_result_success", e);
}

#[test]
fn golden_event_tool_result_error() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "Read".into(),
        tool_use_id: Some("tu_001".into()),
        output: json!({"error": "File not found"}),
        is_error: true,
    });
    assert_json_snapshot!("golden_event_tool_result_error", e);
}

#[test]
fn golden_event_file_changed() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "src/auth.rs".into(),
        summary: "Added JWT validation function".into(),
    });
    assert_json_snapshot!("golden_event_file_changed", e);
}

#[test]
fn golden_event_command_executed() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test --release".into(),
        exit_code: Some(0),
        output_preview: Some("test result: ok. 42 passed".into()),
    });
    assert_json_snapshot!("golden_event_command_executed", e);
}

#[test]
fn golden_event_command_executed_no_exit_code() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "cargo build".into(),
        exit_code: None,
        output_preview: None,
    });
    assert_json_snapshot!("golden_event_command_executed_no_exit_code", e);
}

#[test]
fn golden_event_warning() {
    let e = make_event(AgentEventKind::Warning {
        message: "Approaching budget limit".into(),
    });
    assert_json_snapshot!("golden_event_warning", e);
}

#[test]
fn golden_event_error_without_code() {
    let e = make_event(AgentEventKind::Error {
        message: "Failed to read file".into(),
        error_code: None,
    });
    assert_json_snapshot!("golden_event_error_without_code", e);
}

#[test]
fn golden_event_error_with_code() {
    let e = make_event(AgentEventKind::Error {
        message: "Backend timed out".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    assert_json_snapshot!("golden_event_error_with_code", e);
}

#[test]
fn golden_event_with_ext() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "Raw passthrough".into(),
        },
        ext: Some(BTreeMap::from([
            (
                "raw_message".into(),
                json!({"role": "assistant", "content": "Raw passthrough"}),
            ),
            ("vendor".into(), json!("openai")),
        ])),
    };
    assert_json_snapshot!("golden_event_with_ext", e);
}

// ===========================================================================
// 6. Envelope for each variant JSON snapshot
// ===========================================================================

#[test]
fn golden_envelope_hello() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        {
            let mut m = CapabilityManifest::new();
            m.insert(Capability::Streaming, SupportLevel::Native);
            m.insert(Capability::ToolRead, SupportLevel::Native);
            m
        },
    );
    let json = serde_json::to_string_pretty(&env).unwrap();
    assert_snapshot!("golden_envelope_hello", json);
}

#[test]
fn golden_envelope_hello_passthrough() {
    let env = Envelope::hello_with_mode(
        BackendIdentity {
            id: "sidecar:claude".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    assert_json_snapshot!("golden_envelope_hello_passthrough", env);
}

#[test]
fn golden_envelope_hello_empty_caps() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert_json_snapshot!("golden_envelope_hello_empty_caps", env);
}

#[test]
fn golden_envelope_run() {
    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: minimal_work_order(),
    };
    assert_json_snapshot!("golden_envelope_run", env, {
        ".work_order.id" => "[uuid]",
    });
}

#[test]
fn golden_envelope_event() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: make_event(AgentEventKind::AssistantMessage {
            text: "Working on it".into(),
        }),
    };
    assert_json_snapshot!("golden_envelope_event", env);
}

#[test]
fn golden_envelope_event_tool_call() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: make_event(AgentEventKind::ToolCall {
            tool_name: "Write".into(),
            tool_use_id: Some("tu_100".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/lib.rs", "content": "fn hello() {}"}),
        }),
    };
    assert_json_snapshot!("golden_envelope_event_tool_call", env);
}

#[test]
fn golden_envelope_final() {
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: minimal_receipt(),
    };
    assert_json_snapshot!("golden_envelope_final", env);
}

#[test]
fn golden_envelope_fatal_with_ref() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    assert_json_snapshot!("golden_envelope_fatal_with_ref", env);
}

#[test]
fn golden_envelope_fatal_without_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "sidecar crashed".into(),
        error_code: None,
    };
    assert_json_snapshot!("golden_envelope_fatal_without_ref", env);
}

#[test]
fn golden_envelope_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-001".into()),
        "backend not found",
        abp_error::ErrorCode::BackendNotFound,
    );
    assert_json_snapshot!("golden_envelope_fatal_with_error_code", env);
}

// ===========================================================================
// 7. PolicyProfile JSON snapshot
// ===========================================================================

#[test]
fn golden_policy_profile_empty() {
    let p = PolicyProfile::default();
    assert_json_snapshot!("golden_policy_profile_empty", p);
}

#[test]
fn golden_policy_profile_full() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Edit".into()],
        disallowed_tools: vec!["Bash".into(), "WebSearch".into()],
        deny_read: vec!["**/.env".into(), "**/secrets/**".into()],
        deny_write: vec!["**/.git/**".into(), "**/node_modules/**".into()],
        allow_network: vec!["api.github.com".into(), "crates.io".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["Bash".into(), "Write".into()],
    };
    assert_json_snapshot!("golden_policy_profile_full", p);
}

#[test]
fn golden_policy_profile_tools_only() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    assert_json_snapshot!("golden_policy_profile_tools_only", p);
}

#[test]
fn golden_policy_profile_path_restrictions() {
    let p = PolicyProfile {
        deny_read: vec!["**/.ssh/**".into()],
        deny_write: vec!["**/.git/**".into(), "Cargo.lock".into()],
        ..PolicyProfile::default()
    };
    assert_json_snapshot!("golden_policy_profile_path_restrictions", p);
}

#[test]
fn golden_policy_profile_network_only() {
    let p = PolicyProfile {
        allow_network: vec!["*.github.com".into()],
        deny_network: vec!["*.malware.com".into(), "*.tracking.io".into()],
        ..PolicyProfile::default()
    };
    assert_json_snapshot!("golden_policy_profile_network_only", p);
}

// ===========================================================================
// 8. Capability set JSON snapshot
// ===========================================================================

#[test]
fn golden_capability_manifest_empty() {
    let m = CapabilityManifest::new();
    let json = serde_json::to_string_pretty(&m).unwrap();
    assert_snapshot!("golden_capability_manifest_empty", json);
}

#[test]
fn golden_capability_manifest_full() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Native);
    m.insert(Capability::ToolEdit, SupportLevel::Emulated);
    m.insert(Capability::ToolBash, SupportLevel::Unsupported);
    m.insert(
        Capability::ToolGlob,
        SupportLevel::Restricted {
            reason: "sandbox only".into(),
        },
    );
    let json = serde_json::to_string_pretty(&m).unwrap();
    assert_snapshot!("golden_capability_manifest_full", json);
}

#[test]
fn golden_capability_manifest_streaming_only() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    let json = serde_json::to_string_pretty(&m).unwrap();
    assert_snapshot!("golden_capability_manifest_streaming_only", json);
}

#[test]
fn golden_capability_requirements_snapshot() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolUse,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ExtendedThinking,
                min_support: MinSupport::Native,
            },
        ],
    };
    assert_json_snapshot!("golden_capability_requirements", reqs);
}

#[test]
fn golden_support_level_native() {
    assert_json_snapshot!("golden_support_level_native", SupportLevel::Native);
}

#[test]
fn golden_support_level_emulated() {
    assert_json_snapshot!("golden_support_level_emulated", SupportLevel::Emulated);
}

#[test]
fn golden_support_level_unsupported() {
    assert_json_snapshot!(
        "golden_support_level_unsupported",
        SupportLevel::Unsupported
    );
}

#[test]
fn golden_support_level_restricted() {
    assert_json_snapshot!(
        "golden_support_level_restricted",
        SupportLevel::Restricted {
            reason: "policy sandbox".into(),
        }
    );
}

// ===========================================================================
// 9. Config default JSON snapshot
// ===========================================================================

#[test]
fn golden_runtime_config_default() {
    let c = RuntimeConfig::default();
    assert_json_snapshot!("golden_runtime_config_default", c);
}

#[test]
fn golden_runtime_config_full() {
    let c = RuntimeConfig {
        model: Some("claude-sonnet-4-20250514".into()),
        vendor: BTreeMap::from([
            ("temperature".into(), json!(0.5)),
            ("top_p".into(), json!(0.9)),
        ]),
        env: BTreeMap::from([
            ("RUST_LOG".into(), "debug".into()),
            ("HOME".into(), "/home/user".into()),
        ]),
        max_budget_usd: Some(25.0),
        max_turns: Some(100),
    };
    assert_json_snapshot!("golden_runtime_config_full", c);
}

#[test]
fn golden_runtime_config_model_only() {
    let c = RuntimeConfig {
        model: Some("gpt-4".into()),
        ..RuntimeConfig::default()
    };
    assert_json_snapshot!("golden_runtime_config_model_only", c);
}

#[test]
fn golden_backplane_config_default() {
    let c = abp_config::BackplaneConfig::default();
    assert_json_snapshot!("golden_backplane_config_default", c);
}

#[test]
fn golden_backplane_config_full() {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), abp_config::BackendEntry::Mock {});
    backends.insert(
        "node".into(),
        abp_config::BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["host.js".into()],
            timeout_secs: Some(120),
        },
    );
    let c = abp_config::BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/tmp/workspaces".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        backends,
    };
    assert_json_snapshot!("golden_backplane_config_full", c);
}

#[test]
fn golden_backend_entry_mock() {
    let e = abp_config::BackendEntry::Mock {};
    assert_json_snapshot!("golden_backend_entry_mock", e);
}

#[test]
fn golden_backend_entry_sidecar() {
    let e = abp_config::BackendEntry::Sidecar {
        command: "python".into(),
        args: vec!["-m".into(), "abp_sidecar".into()],
        timeout_secs: Some(60),
    };
    assert_json_snapshot!("golden_backend_entry_sidecar", e);
}

// ===========================================================================
// 10. Error type display snapshots
// ===========================================================================

#[test]
fn golden_error_code_display_all() {
    use abp_error::ErrorCode;
    let codes = vec![
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendCrashed,
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
    ];
    let display: Vec<String> = codes.iter().map(|c| c.to_string()).collect();
    assert_debug_snapshot!("golden_error_code_display_all", display);
}

#[test]
fn golden_error_category_display() {
    use abp_error::ErrorCategory;
    let cats = vec![
        ErrorCategory::Protocol,
        ErrorCategory::Backend,
        ErrorCategory::Capability,
        ErrorCategory::Policy,
        ErrorCategory::Workspace,
        ErrorCategory::Ir,
        ErrorCategory::Receipt,
        ErrorCategory::Dialect,
        ErrorCategory::Config,
        ErrorCategory::Internal,
    ];
    let display: Vec<String> = cats.iter().map(|c| c.to_string()).collect();
    assert_debug_snapshot!("golden_error_category_display", display);
}

#[test]
fn golden_abp_error_display_simple() {
    let err = abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "timed out after 30s");
    assert_snapshot!("golden_abp_error_display_simple", err.to_string());
}

#[test]
fn golden_abp_error_display_with_context() {
    let err = abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);
    assert_snapshot!("golden_abp_error_display_with_context", err.to_string());
}

#[test]
fn golden_abp_error_dto_snapshot() {
    let err = abp_error::AbpError::new(abp_error::ErrorCode::PolicyDenied, "tool not allowed")
        .with_context("tool", "Bash")
        .with_context("policy", "production");
    let dto: abp_error::AbpErrorDto = (&err).into();
    assert_json_snapshot!("golden_abp_error_dto", dto);
}

#[test]
fn golden_abp_error_debug() {
    let err = abp_error::AbpError::new(abp_error::ErrorCode::Internal, "unexpected state");
    assert_debug_snapshot!("golden_abp_error_debug", format!("{err:?}"));
}

#[test]
fn golden_protocol_error_display_violation() {
    let err = abp_protocol::ProtocolError::Violation("hello not received first".into());
    assert_snapshot!("golden_protocol_error_violation", err.to_string());
}

#[test]
fn golden_protocol_error_display_unexpected() {
    let err = abp_protocol::ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_snapshot!("golden_protocol_error_unexpected", err.to_string());
}

#[test]
fn golden_config_error_display_not_found() {
    let err = abp_config::ConfigError::FileNotFound {
        path: "/etc/abp/config.toml".into(),
    };
    assert_snapshot!("golden_config_error_not_found", err.to_string());
}

#[test]
fn golden_config_error_display_parse() {
    let err = abp_config::ConfigError::ParseError {
        reason: "expected value, found newline".into(),
    };
    assert_snapshot!("golden_config_error_parse", err.to_string());
}

#[test]
fn golden_config_error_display_validation() {
    let err = abp_config::ConfigError::ValidationError {
        reasons: vec![
            "invalid log_level 'verbose'".into(),
            "backend 'x': command empty".into(),
        ],
    };
    assert_snapshot!("golden_config_error_validation", err.to_string());
}

#[test]
fn golden_config_error_display_merge() {
    let err = abp_config::ConfigError::MergeConflict {
        reason: "conflicting default_backend values".into(),
    };
    assert_snapshot!("golden_config_error_merge", err.to_string());
}

// ===========================================================================
// 11. Schema output snapshots
// ===========================================================================

#[test]
fn golden_schema_work_order() {
    let schema = schemars::schema_for!(WorkOrder);
    assert_json_snapshot!("golden_schema_work_order", schema);
}

#[test]
fn golden_schema_receipt() {
    let schema = schemars::schema_for!(Receipt);
    assert_json_snapshot!("golden_schema_receipt", schema);
}

#[test]
fn golden_schema_agent_event() {
    let schema = schemars::schema_for!(AgentEvent);
    assert_json_snapshot!("golden_schema_agent_event", schema);
}

#[test]
fn golden_schema_policy_profile() {
    let schema = schemars::schema_for!(PolicyProfile);
    assert_json_snapshot!("golden_schema_policy_profile", schema);
}

#[test]
fn golden_schema_capability() {
    let schema = schemars::schema_for!(Capability);
    assert_json_snapshot!("golden_schema_capability", schema);
}

#[test]
fn golden_schema_outcome() {
    let schema = schemars::schema_for!(Outcome);
    assert_json_snapshot!("golden_schema_outcome", schema);
}

#[test]
fn golden_schema_runtime_config() {
    let schema = schemars::schema_for!(RuntimeConfig);
    assert_json_snapshot!("golden_schema_runtime_config", schema);
}

#[test]
fn golden_schema_execution_mode() {
    let schema = schemars::schema_for!(ExecutionMode);
    assert_json_snapshot!("golden_schema_execution_mode", schema);
}

#[test]
fn golden_schema_backend_identity() {
    let schema = schemars::schema_for!(BackendIdentity);
    assert_json_snapshot!("golden_schema_backend_identity", schema);
}

// ===========================================================================
// 12. Protocol handshake sequence snapshot
// ===========================================================================

#[test]
fn golden_handshake_hello_jsonl() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        {
            let mut m = CapabilityManifest::new();
            m.insert(Capability::Streaming, SupportLevel::Native);
            m
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains("\"t\":\"hello\""));
    assert_snapshot!("golden_handshake_hello_jsonl", line.trim());
}

#[test]
fn golden_handshake_run_jsonl() {
    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: minimal_work_order(),
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("\"t\":\"run\""));
    // Redact the uuid for snapshot stability
    let redacted = line.trim().replace(&uuid1().to_string(), "[uuid]");
    assert_snapshot!("golden_handshake_run_jsonl", redacted);
}

#[test]
fn golden_handshake_event_jsonl() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: make_event(AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        }),
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("\"t\":\"event\""));
    assert_snapshot!("golden_handshake_event_jsonl", line.trim());
}

#[test]
fn golden_handshake_final_jsonl() {
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: minimal_receipt(),
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("\"t\":\"final\""));
    assert_snapshot!("golden_handshake_final_jsonl", line.trim());
}

#[test]
fn golden_handshake_fatal_jsonl() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "process killed".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("\"t\":\"fatal\""));
    assert_snapshot!("golden_handshake_fatal_jsonl", line.trim());
}

#[test]
fn golden_handshake_full_sequence() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let run = Envelope::Run {
        id: "run-seq".into(),
        work_order: minimal_work_order(),
    };
    let event = Envelope::Event {
        ref_id: "run-seq".into(),
        event: make_event(AgentEventKind::RunStarted {
            message: "Go".into(),
        }),
    };
    let fin = Envelope::Final {
        ref_id: "run-seq".into(),
        receipt: minimal_receipt(),
    };

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &[hello, run, event, fin]).unwrap();
    let output = String::from_utf8(buf).unwrap();
    let line_count = output.lines().count();
    assert_eq!(line_count, 4);
    assert_snapshot!(
        "golden_handshake_sequence_line_count",
        line_count.to_string()
    );
}

#[test]
fn golden_handshake_decode_roundtrip() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "roundtrip".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
    let re_encoded = JsonlCodec::encode(&decoded).unwrap();
    assert_eq!(line, re_encoded);
    assert_snapshot!("golden_handshake_decode_roundtrip", line.trim());
}

// ===========================================================================
// 13. Receipt chain JSON snapshot
// ===========================================================================

#[test]
fn golden_receipt_chain_single() {
    let r = abp_receipt::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(uuid1())
        .work_order_id(uuid2())
        .started_at(ts())
        .finished_at(ts2())
        .with_hash()
        .unwrap();

    let mut chain = abp_receipt::ReceiptChain::new();
    chain.push(r).unwrap();
    assert!(chain.verify().is_ok());
    assert_debug_snapshot!("golden_receipt_chain_single_len", chain.len());

    let latest = chain.latest().unwrap();
    assert_json_snapshot!("golden_receipt_chain_single_latest", latest, {
        ".receipt_sha256" => "[sha256]",
    });
}

#[test]
fn golden_receipt_chain_multiple() {
    let r1 = abp_receipt::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(uuid1())
        .started_at(ts())
        .finished_at(ts2())
        .with_hash()
        .unwrap();

    let later_start = Utc.with_ymd_and_hms(2025, 1, 15, 13, 0, 0).unwrap();
    let later_end = Utc.with_ymd_and_hms(2025, 1, 15, 13, 5, 0).unwrap();
    let r2 = abp_receipt::ReceiptBuilder::new("sidecar:node")
        .outcome(Outcome::Partial)
        .run_id(uuid2())
        .started_at(later_start)
        .finished_at(later_end)
        .with_hash()
        .unwrap();

    let even_later = Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();
    let even_later_end = Utc.with_ymd_and_hms(2025, 1, 15, 14, 10, 0).unwrap();
    let r3 = abp_receipt::ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .run_id(uuid3())
        .started_at(even_later)
        .finished_at(even_later_end)
        .with_hash()
        .unwrap();

    let mut chain = abp_receipt::ReceiptChain::new();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    chain.push(r3).unwrap();

    assert!(chain.verify().is_ok());
    assert_eq!(chain.len(), 3);

    let outcomes: Vec<&Outcome> = chain.iter().map(|r| &r.outcome).collect();
    assert_debug_snapshot!("golden_receipt_chain_multiple_outcomes", outcomes);
}

#[test]
fn golden_receipt_chain_error_empty() {
    let chain = abp_receipt::ReceiptChain::new();
    let err = chain.verify().unwrap_err();
    assert_snapshot!("golden_receipt_chain_error_empty", err.to_string());
}

#[test]
fn golden_receipt_chain_error_duplicate() {
    let r1 = abp_receipt::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(uuid1())
        .started_at(ts())
        .finished_at(ts2())
        .with_hash()
        .unwrap();
    let r2 = r1.clone();
    let mut chain = abp_receipt::ReceiptChain::new();
    chain.push(r1).unwrap();
    let err = chain.push(r2).unwrap_err();
    assert_snapshot!("golden_receipt_chain_error_duplicate", err.to_string());
}

// ===========================================================================
// Additional structural snapshots
// ===========================================================================

#[test]
fn golden_execution_lane_patch_first() {
    assert_json_snapshot!(
        "golden_execution_lane_patch_first",
        ExecutionLane::PatchFirst
    );
}

#[test]
fn golden_execution_lane_workspace_first() {
    assert_json_snapshot!(
        "golden_execution_lane_workspace_first",
        ExecutionLane::WorkspaceFirst
    );
}

#[test]
fn golden_execution_mode_mapped() {
    assert_json_snapshot!("golden_execution_mode_mapped", ExecutionMode::Mapped);
}

#[test]
fn golden_execution_mode_passthrough() {
    assert_json_snapshot!(
        "golden_execution_mode_passthrough",
        ExecutionMode::Passthrough
    );
}

#[test]
fn golden_workspace_mode_staged() {
    assert_json_snapshot!("golden_workspace_mode_staged", WorkspaceMode::Staged);
}

#[test]
fn golden_workspace_mode_passthrough() {
    assert_json_snapshot!(
        "golden_workspace_mode_passthrough",
        WorkspaceMode::PassThrough
    );
}

#[test]
fn golden_outcome_complete() {
    assert_json_snapshot!("golden_outcome_complete", Outcome::Complete);
}

#[test]
fn golden_outcome_partial() {
    assert_json_snapshot!("golden_outcome_partial", Outcome::Partial);
}

#[test]
fn golden_outcome_failed() {
    assert_json_snapshot!("golden_outcome_failed", Outcome::Failed);
}

#[test]
fn golden_contract_version() {
    assert_snapshot!("golden_contract_version", CONTRACT_VERSION);
}

#[test]
fn golden_run_metadata() {
    let m = RunMetadata {
        run_id: uuid2(),
        work_order_id: uuid1(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: ts(),
        finished_at: ts2(),
        duration_ms: 300_000,
    };
    assert_json_snapshot!("golden_run_metadata", m);
}

#[test]
fn golden_usage_normalized_default() {
    let u = UsageNormalized::default();
    assert_json_snapshot!("golden_usage_normalized_default", u);
}

#[test]
fn golden_usage_normalized_full() {
    let u = UsageNormalized {
        input_tokens: Some(5000),
        output_tokens: Some(2500),
        cache_read_tokens: Some(1000),
        cache_write_tokens: Some(500),
        request_units: Some(10),
        estimated_cost_usd: Some(0.15),
    };
    assert_json_snapshot!("golden_usage_normalized_full", u);
}

#[test]
fn golden_verification_report_default() {
    let v = VerificationReport::default();
    assert_json_snapshot!("golden_verification_report_default", v);
}

#[test]
fn golden_verification_report_full() {
    let v = VerificationReport {
        git_diff: Some("diff content here".into()),
        git_status: Some("M file.rs".into()),
        harness_ok: true,
    };
    assert_json_snapshot!("golden_verification_report_full", v);
}

#[test]
fn golden_backend_identity_minimal() {
    let b = BackendIdentity {
        id: "mock".into(),
        backend_version: None,
        adapter_version: None,
    };
    assert_json_snapshot!("golden_backend_identity_minimal", b);
}

#[test]
fn golden_backend_identity_full() {
    let b = BackendIdentity {
        id: "sidecar:claude".into(),
        backend_version: Some("3.5".into()),
        adapter_version: Some("0.2.0".into()),
    };
    assert_json_snapshot!("golden_backend_identity_full", b);
}

#[test]
fn golden_artifact_ref() {
    let a = ArtifactRef {
        kind: "patch".into(),
        path: "output/changes.patch".into(),
    };
    assert_json_snapshot!("golden_artifact_ref", a);
}

#[test]
fn golden_context_packet_empty() {
    let c = ContextPacket::default();
    assert_json_snapshot!("golden_context_packet_empty", c);
}

#[test]
fn golden_context_packet_full() {
    let c = ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![
            ContextSnippet {
                name: "arch".into(),
                content: "This is a Rust workspace".into(),
            },
            ContextSnippet {
                name: "style".into(),
                content: "Use snake_case".into(),
            },
        ],
    };
    assert_json_snapshot!("golden_context_packet_full", c);
}

// Policy decision types
#[test]
fn golden_policy_decision_allow() {
    let d = abp_policy::Decision::allow();
    assert_json_snapshot!("golden_policy_decision_allow", d);
}

#[test]
fn golden_policy_decision_deny() {
    let d = abp_policy::Decision::deny("tool Bash is disallowed");
    assert_json_snapshot!("golden_policy_decision_deny", d);
}

// Policy compose types
#[test]
fn golden_policy_composed_allow() {
    let d = abp_policy::compose::PolicyDecision::Allow {
        reason: "matched allowlist".into(),
    };
    assert_json_snapshot!("golden_policy_composed_allow", d);
}

#[test]
fn golden_policy_composed_deny() {
    let d = abp_policy::compose::PolicyDecision::Deny {
        reason: "matched denylist".into(),
    };
    assert_json_snapshot!("golden_policy_composed_deny", d);
}

#[test]
fn golden_policy_composed_abstain() {
    let d = abp_policy::compose::PolicyDecision::Abstain;
    assert_json_snapshot!("golden_policy_composed_abstain", d);
}

// Policy rule types
#[test]
fn golden_policy_rule() {
    let r = abp_policy::rules::Rule {
        id: "deny-bash".into(),
        description: "Deny all bash commands".into(),
        condition: abp_policy::rules::RuleCondition::Pattern("Bash*".into()),
        effect: abp_policy::rules::RuleEffect::Deny,
        priority: 100,
    };
    assert_json_snapshot!("golden_policy_rule", r);
}

#[test]
fn golden_policy_rule_effect_allow() {
    assert_json_snapshot!(
        "golden_policy_rule_effect_allow",
        abp_policy::rules::RuleEffect::Allow
    );
}

#[test]
fn golden_policy_rule_effect_deny() {
    assert_json_snapshot!(
        "golden_policy_rule_effect_deny",
        abp_policy::rules::RuleEffect::Deny
    );
}

#[test]
fn golden_policy_rule_effect_log() {
    assert_json_snapshot!(
        "golden_policy_rule_effect_log",
        abp_policy::rules::RuleEffect::Log
    );
}

#[test]
fn golden_policy_rule_effect_throttle() {
    assert_json_snapshot!(
        "golden_policy_rule_effect_throttle",
        abp_policy::rules::RuleEffect::Throttle { max: 10 }
    );
}

#[test]
fn golden_policy_rule_condition_always() {
    assert_json_snapshot!(
        "golden_policy_rule_condition_always",
        abp_policy::rules::RuleCondition::Always
    );
}

#[test]
fn golden_policy_rule_condition_compound() {
    let cond = abp_policy::rules::RuleCondition::And(vec![
        abp_policy::rules::RuleCondition::Pattern("Read*".into()),
        abp_policy::rules::RuleCondition::Not(Box::new(abp_policy::rules::RuleCondition::Pattern(
            "*.secret".into(),
        ))),
    ]);
    assert_json_snapshot!("golden_policy_rule_condition_compound", cond);
}

// Error code serde roundtrip
#[test]
fn golden_error_code_serde_roundtrip() {
    let code = abp_error::ErrorCode::ProtocolInvalidEnvelope;
    let json = serde_json::to_string(&code).unwrap();
    assert_snapshot!("golden_error_code_serde", json);
}

#[test]
fn golden_error_category_serde() {
    let cat = abp_error::ErrorCategory::Backend;
    let json = serde_json::to_string(&cat).unwrap();
    assert_snapshot!("golden_error_category_serde", json);
}

// Version parsing
#[test]
fn golden_version_parse() {
    assert_debug_snapshot!(
        "golden_version_parse_valid",
        abp_protocol::parse_version("abp/v0.1")
    );
    assert_debug_snapshot!(
        "golden_version_parse_invalid",
        abp_protocol::parse_version("invalid")
    );
}

#[test]
fn golden_version_compatibility() {
    let compat = abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2");
    let incompat = abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1");
    assert_debug_snapshot!("golden_version_compatible", compat);
    assert_debug_snapshot!("golden_version_incompatible", incompat);
}
