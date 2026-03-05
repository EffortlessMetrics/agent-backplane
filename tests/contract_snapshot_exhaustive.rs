#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive snapshot tests for all core contract types and protocol envelopes.
//!
//! Each test constructs a value with deterministic data (fixed UUIDs, timestamps)
//! and asserts the serialized JSON output via `insta::assert_json_snapshot!`.

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_config::{BackendEntry, BackplaneConfig};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata,
    RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_protocol::Envelope;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 7, 1, 12, 0, 0).unwrap()
}

fn ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 7, 1, 12, 10, 0).unwrap()
}

fn uid1() -> Uuid {
    Uuid::parse_str("01010101-0202-4303-8404-050505050505").unwrap()
}

fn uid2() -> Uuid {
    Uuid::parse_str("a1a1a1a1-b2b2-4c3c-8d4d-e5e5e5e5e5e5").unwrap()
}

fn backend_id() -> BackendIdentity {
    BackendIdentity {
        id: "sidecar:claude".into(),
        backend_version: Some("3.5.0".into()),
        adapter_version: Some("0.2.0".into()),
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
    m.insert(Capability::ExtendedThinking, SupportLevel::Native);
    m.insert(Capability::ImageInput, SupportLevel::Emulated);
    m.insert(
        Capability::McpClient,
        SupportLevel::Restricted {
            reason: "beta feature".into(),
        },
    );
    m.insert(Capability::McpServer, SupportLevel::Unsupported);
    m
}

/// Serialize to pretty JSON string for types where `assert_json_snapshot!`
/// struggles (e.g. non-string map keys in `CapabilityManifest`).
fn snap_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap()
}

fn make_receipt(outcome: Outcome) -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: uid1(),
            work_order_id: uid2(),
            contract_version: "abp/v0.1".into(),
            started_at: ts(),
            finished_at: ts2(),
            duration_ms: 600_000,
        },
        backend: backend_id(),
        capabilities: small_caps(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome,
        receipt_sha256: None,
    }
}

// =========================================================================
// 1. WorkOrder snapshots (10 tests)
// =========================================================================

#[test]
fn exhaustive_work_order_minimal() {
    let wo = WorkOrder {
        id: uid1(),
        task: "ping".into(),
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
    insta::assert_json_snapshot!(wo);
}

#[test]
fn exhaustive_work_order_full() {
    let wo = WorkOrder {
        id: uid1(),
        task: "Refactor auth module and add tests".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/home/dev/project".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**/*.rs".into(), "tests/**".into()],
            exclude: vec!["target/**".into(), "node_modules/**".into()],
        },
        context: ContextPacket {
            files: vec!["README.md".into(), "src/auth.rs".into()],
            snippets: vec![
                ContextSnippet {
                    name: "error_log".into(),
                    content: "thread 'main' panicked at auth.rs:42".into(),
                },
                ContextSnippet {
                    name: "spec".into(),
                    content: "Auth tokens must expire after 1 hour".into(),
                },
            ],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read".into(), "write".into(), "edit".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec![".env".into(), "secrets/**".into()],
            deny_write: vec!["Cargo.lock".into()],
            allow_network: vec!["api.github.com".into()],
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
                CapabilityRequirement {
                    capability: Capability::ExtendedThinking,
                    min_support: MinSupport::Any,
                },
            ],
        },
        config: RuntimeConfig {
            model: Some("claude-3.5-sonnet".into()),
            vendor: {
                let mut v = BTreeMap::new();
                v.insert("temperature".into(), json!(0.7));
                v.insert("top_p".into(), json!(0.95));
                v
            },
            env: {
                let mut e = BTreeMap::new();
                e.insert("LANG".into(), "en_US.UTF-8".into());
                e.insert("CI".into(), "true".into());
                e
            },
            max_budget_usd: Some(5.0),
            max_turns: Some(50),
        },
    };
    insta::assert_json_snapshot!(wo);
}

#[test]
fn exhaustive_work_order_with_capabilities() {
    let wo = WorkOrder {
        id: uid1(),
        task: "needs many capabilities".into(),
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
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolWrite,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ExtendedThinking,
                    min_support: MinSupport::Emulated,
                },
                CapabilityRequirement {
                    capability: Capability::ImageInput,
                    min_support: MinSupport::Any,
                },
                CapabilityRequirement {
                    capability: Capability::McpClient,
                    min_support: MinSupport::Emulated,
                },
            ],
        },
        config: RuntimeConfig::default(),
    };
    insta::assert_json_snapshot!(wo);
}

#[test]
fn exhaustive_work_order_with_policy() {
    let wo = WorkOrder {
        id: uid1(),
        task: "locked-down task".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/secure/ws".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile {
            allowed_tools: vec!["grep".into(), "glob".into()],
            disallowed_tools: vec!["bash".into(), "write".into(), "edit".into()],
            deny_read: vec!["secrets/**".into(), ".env*".into(), "*.pem".into()],
            deny_write: vec!["**/*".into()],
            allow_network: vec![],
            deny_network: vec!["*".into()],
            require_approval_for: vec!["read".into()],
        },
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    insta::assert_json_snapshot!(wo);
}

#[test]
fn exhaustive_work_order_with_vendor_config() {
    let wo = WorkOrder {
        id: uid1(),
        task: "vendor-heavy config".into(),
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
            model: Some("gpt-4o".into()),
            vendor: {
                let mut v = BTreeMap::new();
                v.insert("temperature".into(), json!(0.3));
                v.insert("max_tokens".into(), json!(8192));
                v.insert("top_p".into(), json!(0.9));
                v.insert("abp".into(), json!({"mode": "passthrough", "debug": true}));
                v
            },
            env: BTreeMap::new(),
            max_budget_usd: Some(100.0),
            max_turns: Some(200),
        },
    };
    insta::assert_json_snapshot!(wo);
}

#[test]
fn exhaustive_work_order_with_env() {
    let wo = WorkOrder {
        id: uid1(),
        task: "env-heavy task".into(),
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
            model: None,
            vendor: BTreeMap::new(),
            env: {
                let mut e = BTreeMap::new();
                e.insert("HOME".into(), "/home/agent".into());
                e.insert("PATH".into(), "/usr/bin:/usr/local/bin".into());
                e.insert("RUST_LOG".into(), "abp=debug".into());
                e.insert("NODE_ENV".into(), "production".into());
                e
            },
            max_budget_usd: None,
            max_turns: None,
        },
    };
    insta::assert_json_snapshot!(wo);
}

#[test]
fn exhaustive_work_order_workspace_passthrough() {
    let wo = WorkOrder {
        id: uid1(),
        task: "passthrough workspace".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/mnt/repo".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec!["**/*.py".into()],
            exclude: vec!["__pycache__/**".into(), ".venv/**".into()],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    insta::assert_json_snapshot!(wo);
}

#[test]
fn exhaustive_work_order_with_context() {
    let wo = WorkOrder {
        id: uid1(),
        task: "context-rich task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket {
            files: vec![
                "src/main.rs".into(),
                "src/lib.rs".into(),
                "Cargo.toml".into(),
                "tests/integration.rs".into(),
            ],
            snippets: vec![
                ContextSnippet {
                    name: "compile_error".into(),
                    content: "error[E0308]: mismatched types".into(),
                },
                ContextSnippet {
                    name: "user_intent".into(),
                    content: "Fix the type error in parse_config".into(),
                },
                ContextSnippet {
                    name: "git_diff".into(),
                    content: "diff --git a/src/config.rs b/src/config.rs".into(),
                },
            ],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    insta::assert_json_snapshot!(wo);
}

#[test]
fn exhaustive_work_order_builder_all_options() {
    let wo = WorkOrderBuilder::new("builder test")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/projects/app")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .model("claude-3.5-sonnet")
        .max_turns(30)
        .max_budget_usd(25.0)
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into()],
            ..PolicyProfile::default()
        })
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    insta::assert_json_snapshot!(wo, {
        ".id" => "[uuid]"
    });
}

#[test]
fn exhaustive_work_order_builder_defaults_only() {
    let wo = WorkOrderBuilder::new("defaults").build();
    insta::assert_json_snapshot!(wo, {
        ".id" => "[uuid]"
    });
}

// =========================================================================
// 2. Receipt snapshots (10 tests)
// =========================================================================

#[test]
fn exhaustive_receipt_success() {
    let r = make_receipt(Outcome::Complete);
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn exhaustive_receipt_partial() {
    let r = make_receipt(Outcome::Partial);
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn exhaustive_receipt_failed() {
    let r = make_receipt(Outcome::Failed);
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn exhaustive_receipt_with_hash() {
    let r = make_receipt(Outcome::Complete).with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn exhaustive_receipt_with_usage() {
    let mut r = make_receipt(Outcome::Complete);
    r.usage = UsageNormalized {
        input_tokens: Some(2500),
        output_tokens: Some(1200),
        cache_read_tokens: Some(500),
        cache_write_tokens: Some(200),
        request_units: Some(4),
        estimated_cost_usd: Some(0.035),
    };
    r.usage_raw = json!({
        "prompt_tokens": 2500,
        "completion_tokens": 1200,
        "total_tokens": 3700,
        "cached_tokens": 500,
    });
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn exhaustive_receipt_with_extensions() {
    let mut r = make_receipt(Outcome::Complete);
    r.capabilities = full_caps();
    r.mode = ExecutionMode::Passthrough;
    r.verification = VerificationReport {
        git_diff: Some("diff --git a/src/auth.rs b/src/auth.rs\n+pub fn validate() {}".into()),
        git_status: Some("M src/auth.rs\nA src/auth_test.rs".into()),
        harness_ok: true,
    };
    r.artifacts = vec![
        ArtifactRef {
            kind: "patch".into(),
            path: "auth.patch".into(),
        },
        ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        },
        ArtifactRef {
            kind: "screenshot".into(),
            path: "output.png".into(),
        },
    ];
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn exhaustive_receipt_full_trace() {
    let mut r = make_receipt(Outcome::Complete);
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
                text: "I will fix the bug.".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
            ext: None,
        },
        AgentEvent {
            ts: ts(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_1".into()),
                output: json!({"content": "fn main() {}"}),
                is_error: false,
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
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn exhaustive_receipt_builder_full() {
    let mut r = ReceiptBuilder::new("sidecar:node")
        .outcome(Outcome::Complete)
        .started_at(ts())
        .finished_at(ts2())
        .work_order_id(uid2())
        .backend_version("2.0.0")
        .adapter_version("1.0.0")
        .capabilities(small_caps())
        .mode(ExecutionMode::Mapped)
        .usage_raw(json!({"prompt_tokens": 100}))
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..UsageNormalized::default()
        })
        .verification(VerificationReport {
            git_diff: Some("diff".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        })
        .add_trace_event(AgentEvent {
            ts: ts(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "out.patch".into(),
        })
        .build();
    r.meta.run_id = uid1();
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn exhaustive_receipt_passthrough_mode() {
    let mut r = make_receipt(Outcome::Complete);
    r.mode = ExecutionMode::Passthrough;
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn exhaustive_receipt_no_usage() {
    let r = make_receipt(Outcome::Complete);
    // Default usage has all None fields
    assert!(r.usage.input_tokens.is_none());
    assert!(r.usage.output_tokens.is_none());
    insta::assert_snapshot!(snap_json(&r));
}

// =========================================================================
// 3. AgentEvent snapshots (15 tests)
// =========================================================================

#[test]
fn exhaustive_event_run_started() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunStarted {
            message: "initializing workspace and loading context".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_run_completed() {
    let e = AgentEvent {
        ts: ts2(),
        kind: AgentEventKind::RunCompleted {
            message: "task finished successfully with 3 files modified".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_assistant_message() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "I've analyzed the code and found the root cause of the bug.".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_assistant_delta() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantDelta {
            text: "Let me ".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_tool_call_simple() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_abc".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/lib.rs"}),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_tool_call_nested() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("tu_def".into()),
            parent_tool_use_id: Some("tu_abc".into()),
            input: json!({"path": "out.txt", "content": "hello"}),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_tool_call_no_ids() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({"command": "ls -la"}),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_tool_result_success() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_abc".into()),
            output: json!({"content": "fn main() {\n    println!(\"hello\");\n}"}),
            is_error: false,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_tool_result_error() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_xyz".into()),
            output: json!({"error": "ENOENT: no such file or directory"}),
            is_error: true,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_file_changed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::FileChanged {
            path: "src/auth/mod.rs".into(),
            summary: "Added token validation function".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_command_executed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test --lib".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok. 47 passed; 0 failed".into()),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_command_no_exit() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "sleep 3600".into(),
            exit_code: None,
            output_preview: None,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_warning() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Warning {
            message: "budget 80% consumed, 2 turns remaining".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_error_without_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Error {
            message: "unexpected backend response".into(),
            error_code: None,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_error_with_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Error {
            message: "backend crashed during tool execution".into(),
            error_code: Some(abp_error::ErrorCode::BackendCrashed),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn exhaustive_event_with_ext_passthrough() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        json!({"role": "assistant", "content": "raw sdk data"}),
    );
    ext.insert("vendor_trace_id".into(), json!("trace-9876"));
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "passthrough content".into(),
        },
        ext: Some(ext),
    };
    insta::assert_json_snapshot!(e);
}

// =========================================================================
// 4. Envelope snapshots (10 tests)
// =========================================================================

#[test]
fn exhaustive_envelope_hello_mapped() {
    let env = Envelope::hello(backend_id(), small_caps());
    insta::assert_snapshot!(snap_json(&env));
}

#[test]
fn exhaustive_envelope_hello_passthrough() {
    let env = Envelope::hello_with_mode(backend_id(), full_caps(), ExecutionMode::Passthrough);
    insta::assert_snapshot!(snap_json(&env));
}

#[test]
fn exhaustive_envelope_hello_empty_caps() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        BTreeMap::new(),
    );
    insta::assert_snapshot!(snap_json(&env));
}

#[test]
fn exhaustive_envelope_run() {
    let wo = WorkOrder {
        id: uid1(),
        task: "Fix the login page".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/app".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec![],
        },
        context: ContextPacket {
            files: vec!["README.md".into()],
            snippets: vec![],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: Some("gpt-4o".into()),
            ..RuntimeConfig::default()
        },
    };
    let env = Envelope::Run {
        id: "run-exhaustive-001".into(),
        work_order: wo,
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn exhaustive_envelope_event_delta() {
    let env = Envelope::Event {
        ref_id: "run-exhaustive-001".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "Here is ".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn exhaustive_envelope_event_tool_call() {
    let env = Envelope::Event {
        ref_id: "run-exhaustive-001".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "edit_file".into(),
                tool_use_id: Some("tu_env_1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs", "old": "bug", "new": "fix"}),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn exhaustive_envelope_event_file_changed() {
    let env = Envelope::Event {
        ref_id: "run-exhaustive-001".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "Applied bug fix".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn exhaustive_envelope_final() {
    let env = Envelope::Final {
        ref_id: "run-exhaustive-001".into(),
        receipt: make_receipt(Outcome::Complete),
    };
    insta::assert_snapshot!(snap_json(&env));
}

#[test]
fn exhaustive_envelope_fatal_with_ref() {
    let env = Envelope::Fatal {
        ref_id: Some("run-exhaustive-001".into()),
        error: "sidecar process exited with code 137 (OOM killed)".into(),
        error_code: Some(abp_error::ErrorCode::BackendCrashed),
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn exhaustive_envelope_fatal_no_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "handshake timed out after 30s".into(),
        error_code: Some(abp_error::ErrorCode::ProtocolHandshakeFailed),
    };
    insta::assert_json_snapshot!(env);
}

// =========================================================================
// 5. Config snapshots (10 tests)
// =========================================================================

#[test]
fn exhaustive_config_default() {
    insta::assert_json_snapshot!(BackplaneConfig::default());
}

#[test]
fn exhaustive_config_full() {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    backends.insert(
        "node".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["hosts/node/index.js".into()],
            timeout_secs: Some(120),
        },
    );
    backends.insert(
        "python".into(),
        BackendEntry::Sidecar {
            command: "python3".into(),
            args: vec!["hosts/python/main.py".into()],
            timeout_secs: Some(60),
        },
    );
    backends.insert(
        "claude".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["hosts/claude/index.js".into()],
            timeout_secs: Some(300),
        },
    );
    let cfg = BackplaneConfig {
        default_backend: Some("claude".into()),
        workspace_dir: Some("/tmp/abp-workspaces".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/var/log/abp/receipts".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        policy_profiles: vec![
            "profiles/strict.json".into(),
            "profiles/default.json".into(),
        ],
        backends,
    };
    insta::assert_json_snapshot!(cfg);
}

#[test]
fn exhaustive_config_sidecar_backend() {
    let mut backends = BTreeMap::new();
    backends.insert(
        "node-sidecar".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["hosts/node/index.js".into(), "--verbose".into()],
            timeout_secs: Some(90),
        },
    );
    let cfg = BackplaneConfig {
        default_backend: Some("node-sidecar".into()),
        backends,
        ..BackplaneConfig::default()
    };
    insta::assert_json_snapshot!(cfg);
}

#[test]
fn exhaustive_config_mock_backend() {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        backends,
        ..BackplaneConfig::default()
    };
    insta::assert_json_snapshot!(cfg);
}

#[test]
fn exhaustive_config_multi_backend() {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    backends.insert(
        "node".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["index.js".into()],
            timeout_secs: None,
        },
    );
    backends.insert(
        "python".into(),
        BackendEntry::Sidecar {
            command: "python3".into(),
            args: vec!["main.py".into()],
            timeout_secs: Some(30),
        },
    );
    let cfg = BackplaneConfig {
        backends,
        ..BackplaneConfig::default()
    };
    insta::assert_json_snapshot!(cfg);
}

#[test]
fn exhaustive_config_network_binding() {
    let cfg = BackplaneConfig {
        bind_address: Some("0.0.0.0".into()),
        port: Some(9090),
        ..BackplaneConfig::default()
    };
    insta::assert_json_snapshot!(cfg);
}

#[test]
fn exhaustive_config_with_policy_profiles() {
    let cfg = BackplaneConfig {
        policy_profiles: vec![
            "policies/strict.json".into(),
            "policies/read-only.json".into(),
            "policies/ci.json".into(),
        ],
        ..BackplaneConfig::default()
    };
    insta::assert_json_snapshot!(cfg);
}

#[test]
fn exhaustive_config_backend_entry_mock() {
    insta::assert_json_snapshot!(BackendEntry::Mock {});
}

#[test]
fn exhaustive_config_backend_entry_sidecar_full() {
    insta::assert_json_snapshot!(BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![
            "hosts/claude/index.js".into(),
            "--debug".into(),
            "--model".into(),
            "claude-3.5-sonnet".into()
        ],
        timeout_secs: Some(600),
    });
}

#[test]
fn exhaustive_config_backend_entry_sidecar_no_timeout() {
    insta::assert_json_snapshot!(BackendEntry::Sidecar {
        command: "python3".into(),
        args: vec!["sidecar.py".into()],
        timeout_secs: None,
    });
}

// =========================================================================
// 6. Policy snapshots (10 tests)
// =========================================================================

#[test]
fn exhaustive_policy_empty() {
    insta::assert_json_snapshot!(PolicyProfile::default());
}

#[test]
fn exhaustive_policy_tool_deny() {
    let p = PolicyProfile {
        disallowed_tools: vec!["bash".into(), "exec".into(), "write_file".into()],
        ..PolicyProfile::default()
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn exhaustive_policy_tool_allow_only() {
    let p = PolicyProfile {
        allowed_tools: vec!["read_file".into(), "glob".into(), "grep".into()],
        ..PolicyProfile::default()
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn exhaustive_policy_read_deny() {
    let p = PolicyProfile {
        deny_read: vec![
            ".env".into(),
            ".env.*".into(),
            "secrets/**".into(),
            "**/*.pem".into(),
            "**/*.key".into(),
        ],
        ..PolicyProfile::default()
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn exhaustive_policy_write_deny() {
    let p = PolicyProfile {
        deny_write: vec![
            "Cargo.lock".into(),
            "package-lock.json".into(),
            "yarn.lock".into(),
            "**/*.lock".into(),
            ".github/**".into(),
        ],
        ..PolicyProfile::default()
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn exhaustive_policy_network_rules() {
    let p = PolicyProfile {
        allow_network: vec![
            "api.github.com".into(),
            "registry.npmjs.org".into(),
            "crates.io".into(),
        ],
        deny_network: vec!["*.evil.com".into(), "*.malware.net".into()],
        ..PolicyProfile::default()
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn exhaustive_policy_require_approval() {
    let p = PolicyProfile {
        require_approval_for: vec!["delete_file".into(), "bash".into(), "write_file".into()],
        ..PolicyProfile::default()
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn exhaustive_policy_complex_mixed() {
    let p = PolicyProfile {
        allowed_tools: vec![
            "read_file".into(),
            "write_file".into(),
            "edit_file".into(),
            "glob".into(),
            "grep".into(),
        ],
        disallowed_tools: vec!["bash".into(), "exec".into()],
        deny_read: vec![".env".into(), "secrets/**".into(), "**/*.pem".into()],
        deny_write: vec!["Cargo.lock".into(), ".github/**".into(), "LICENSE*".into()],
        allow_network: vec!["api.github.com".into(), "crates.io".into()],
        deny_network: vec!["*".into()],
        require_approval_for: vec!["delete_file".into()],
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn exhaustive_policy_deny_all_network() {
    let p = PolicyProfile {
        allow_network: vec![],
        deny_network: vec!["*".into()],
        ..PolicyProfile::default()
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn exhaustive_policy_read_write_deny_combined() {
    let p = PolicyProfile {
        deny_read: vec![
            "/etc/passwd".into(),
            "/etc/shadow".into(),
            "~/.ssh/**".into(),
        ],
        deny_write: vec!["**/*".into()],
        ..PolicyProfile::default()
    };
    insta::assert_json_snapshot!(p);
}
