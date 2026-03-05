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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive golden-file / snapshot tests for JSON serialization stability.
//!
//! Every test uses `assert_eq!` with expected JSON values to lock down
//! the wire-format contract.  Categories covered:
//!
//! 1. WorkOrder JSON structure
//! 2. Receipt JSON structure
//! 3. AgentEvent kind variants
//! 4. Envelope JSONL (tag = "t")
//! 5. IR type snapshots
//! 6. Error type Display
//! 7. Capability / SupportLevel
//! 8. PolicyProfile
//! 9. Deterministic serialization (BTreeMap ordering)

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, RunMetadata, RuntimeConfig,
    SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};

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

fn mock_backend() -> BackendIdentity {
    BackendIdentity {
        id: "mock".into(),
        backend_version: None,
        adapter_version: None,
    }
}

fn full_backend() -> BackendIdentity {
    BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("3.0.0".into()),
        adapter_version: Some("1.2.0".into()),
    }
}

fn minimal_work_order() -> WorkOrder {
    WorkOrder {
        id: uid1(),
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

fn minimal_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: uid1(),
            work_order_id: uid2(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts(),
            finished_at: ts2(),
            duration_ms: 300_000,
        },
        backend: mock_backend(),
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
// 1. WorkOrder JSON snapshot stability
// ===========================================================================

#[test]
fn wo_execution_lane_patch_first() {
    assert_eq!(
        serde_json::to_string(&ExecutionLane::PatchFirst).unwrap(),
        r#""patch_first""#,
    );
}

#[test]
fn wo_execution_lane_workspace_first() {
    assert_eq!(
        serde_json::to_string(&ExecutionLane::WorkspaceFirst).unwrap(),
        r#""workspace_first""#,
    );
}

#[test]
fn wo_workspace_mode_staged() {
    assert_eq!(
        serde_json::to_string(&WorkspaceMode::Staged).unwrap(),
        r#""staged""#,
    );
}

#[test]
fn wo_workspace_mode_pass_through() {
    assert_eq!(
        serde_json::to_string(&WorkspaceMode::PassThrough).unwrap(),
        r#""pass_through""#,
    );
}

#[test]
fn wo_context_packet_default() {
    assert_eq!(
        serde_json::to_value(ContextPacket::default()).unwrap(),
        json!({"files": [], "snippets": []}),
    );
}

#[test]
fn wo_context_snippet_structure() {
    let s = ContextSnippet {
        name: "hint".into(),
        content: "Look at auth module".into(),
    };
    assert_eq!(
        serde_json::to_value(s).unwrap(),
        json!({"name": "hint", "content": "Look at auth module"}),
    );
}

#[test]
fn wo_runtime_config_default() {
    assert_eq!(
        serde_json::to_value(RuntimeConfig::default()).unwrap(),
        json!({
            "model": null,
            "vendor": {},
            "env": {},
            "max_budget_usd": null,
            "max_turns": null,
        }),
    );
}

#[test]
fn wo_runtime_config_full() {
    let rc = RuntimeConfig {
        model: Some("gpt-4o".into()),
        vendor: {
            let mut m = BTreeMap::new();
            m.insert("temperature".into(), json!(0.7));
            m
        },
        env: {
            let mut m = BTreeMap::new();
            m.insert("RUST_LOG".into(), "debug".into());
            m
        },
        max_budget_usd: Some(1.5),
        max_turns: Some(25),
    };
    assert_eq!(
        serde_json::to_value(rc).unwrap(),
        json!({
            "model": "gpt-4o",
            "vendor": {"temperature": 0.7},
            "env": {"RUST_LOG": "debug"},
            "max_budget_usd": 1.5,
            "max_turns": 25,
        }),
    );
}

#[test]
fn wo_capability_requirements_empty() {
    assert_eq!(
        serde_json::to_value(CapabilityRequirements::default()).unwrap(),
        json!({"required": []}),
    );
}

#[test]
fn wo_capability_requirement_single() {
    let req = CapabilityRequirement {
        capability: Capability::ToolRead,
        min_support: MinSupport::Native,
    };
    assert_eq!(
        serde_json::to_value(req).unwrap(),
        json!({"capability": "tool_read", "min_support": "native"}),
    );
}

#[test]
fn wo_workspace_spec_structure() {
    let spec = WorkspaceSpec {
        root: "/tmp/ws".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["target/**".into()],
    };
    assert_eq!(
        serde_json::to_value(spec).unwrap(),
        json!({
            "root": "/tmp/ws",
            "mode": "staged",
            "include": ["src/**"],
            "exclude": ["target/**"],
        }),
    );
}

#[test]
fn wo_minimal_structure_json() {
    let wo = minimal_work_order();
    let v = serde_json::to_value(wo).unwrap();
    assert_eq!(v["id"], "00000000-0000-4000-8000-000000000001");
    assert_eq!(v["task"], "Hello world");
    assert_eq!(v["lane"], "patch_first");
    assert_eq!(v["workspace"]["mode"], "pass_through");
    assert_eq!(v["context"]["files"], json!([]));
    assert_eq!(v["policy"]["allowed_tools"], json!([]));
    assert_eq!(v["requirements"]["required"], json!([]));
    assert_eq!(v["config"]["model"], json!(null));
}

// ===========================================================================
// 2. Receipt JSON snapshot stability
// ===========================================================================

#[test]
fn receipt_outcome_complete() {
    assert_eq!(
        serde_json::to_string(&Outcome::Complete).unwrap(),
        r#""complete""#,
    );
}

#[test]
fn receipt_outcome_partial() {
    assert_eq!(
        serde_json::to_string(&Outcome::Partial).unwrap(),
        r#""partial""#,
    );
}

#[test]
fn receipt_outcome_failed() {
    assert_eq!(
        serde_json::to_string(&Outcome::Failed).unwrap(),
        r#""failed""#,
    );
}

#[test]
fn receipt_usage_normalized_default() {
    assert_eq!(
        serde_json::to_value(UsageNormalized::default()).unwrap(),
        json!({
            "input_tokens": null,
            "output_tokens": null,
            "cache_read_tokens": null,
            "cache_write_tokens": null,
            "request_units": null,
            "estimated_cost_usd": null,
        }),
    );
}

#[test]
fn receipt_usage_normalized_full() {
    let u = UsageNormalized {
        input_tokens: Some(1200),
        output_tokens: Some(800),
        cache_read_tokens: Some(100),
        cache_write_tokens: Some(50),
        request_units: Some(2),
        estimated_cost_usd: Some(0.015),
    };
    assert_eq!(
        serde_json::to_value(u).unwrap(),
        json!({
            "input_tokens": 1200,
            "output_tokens": 800,
            "cache_read_tokens": 100,
            "cache_write_tokens": 50,
            "request_units": 2,
            "estimated_cost_usd": 0.015,
        }),
    );
}

#[test]
fn receipt_verification_report_default() {
    assert_eq!(
        serde_json::to_value(VerificationReport::default()).unwrap(),
        json!({"git_diff": null, "git_status": null, "harness_ok": false}),
    );
}

#[test]
fn receipt_verification_report_full() {
    let vr = VerificationReport {
        git_diff: Some("diff --git a/f.rs b/f.rs".into()),
        git_status: Some("M f.rs".into()),
        harness_ok: true,
    };
    assert_eq!(
        serde_json::to_value(vr).unwrap(),
        json!({
            "git_diff": "diff --git a/f.rs b/f.rs",
            "git_status": "M f.rs",
            "harness_ok": true,
        }),
    );
}

#[test]
fn receipt_artifact_ref_structure() {
    let a = ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    };
    assert_eq!(
        serde_json::to_value(a).unwrap(),
        json!({"kind": "patch", "path": "output.patch"}),
    );
}

#[test]
fn receipt_run_metadata_structure() {
    let meta = RunMetadata {
        run_id: uid1(),
        work_order_id: uid2(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: ts(),
        finished_at: ts2(),
        duration_ms: 300_000,
    };
    assert_eq!(
        serde_json::to_value(meta).unwrap(),
        json!({
            "run_id": "00000000-0000-4000-8000-000000000001",
            "work_order_id": "00000000-0000-4000-8000-000000000002",
            "contract_version": "abp/v0.1",
            "started_at": "2025-07-01T12:00:00Z",
            "finished_at": "2025-07-01T12:05:00Z",
            "duration_ms": 300000,
        }),
    );
}

#[test]
fn receipt_backend_identity_full() {
    assert_eq!(
        serde_json::to_value(full_backend()).unwrap(),
        json!({
            "id": "sidecar:node",
            "backend_version": "3.0.0",
            "adapter_version": "1.2.0",
        }),
    );
}

#[test]
fn receipt_backend_identity_minimal() {
    assert_eq!(
        serde_json::to_value(mock_backend()).unwrap(),
        json!({"id": "mock", "backend_version": null, "adapter_version": null}),
    );
}

#[test]
fn receipt_minimal_top_level_keys() {
    let r = minimal_receipt();
    let v = serde_json::to_value(r).unwrap();
    let obj = v.as_object().unwrap();
    let keys: Vec<&String> = obj.keys().collect();
    // Verify all expected top-level keys exist
    for k in &[
        "meta",
        "backend",
        "capabilities",
        "mode",
        "usage_raw",
        "usage",
        "trace",
        "artifacts",
        "verification",
        "outcome",
        "receipt_sha256",
    ] {
        assert!(
            keys.iter().any(|key| key.as_str() == *k),
            "missing key: {k}"
        );
    }
}

// ===========================================================================
// 3. AgentEvent kind snapshots (all variants)
// ===========================================================================

#[test]
fn event_run_started() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunStarted {
            message: "Starting".into(),
        },
        ext: None,
    };
    assert_eq!(
        serde_json::to_value(e).unwrap(),
        json!({
            "ts": "2025-07-01T12:00:00Z",
            "type": "run_started",
            "message": "Starting",
        }),
    );
}

#[test]
fn event_run_completed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunCompleted {
            message: "Done".into(),
        },
        ext: None,
    };
    assert_eq!(
        serde_json::to_value(e).unwrap(),
        json!({
            "ts": "2025-07-01T12:00:00Z",
            "type": "run_completed",
            "message": "Done",
        }),
    );
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
    assert_eq!(
        serde_json::to_value(e).unwrap(),
        json!({
            "ts": "2025-07-01T12:00:00Z",
            "type": "assistant_delta",
            "text": "Hello ",
        }),
    );
}

#[test]
fn event_assistant_message() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "I will help you.".into(),
        },
        ext: None,
    };
    assert_eq!(
        serde_json::to_value(e).unwrap(),
        json!({
            "ts": "2025-07-01T12:00:00Z",
            "type": "assistant_message",
            "text": "I will help you.",
        }),
    );
}

#[test]
fn event_tool_call_with_ids() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_001".into()),
            parent_tool_use_id: Some("tu_000".into()),
            input: json!({"path": "src/main.rs"}),
        },
        ext: None,
    };
    assert_eq!(
        serde_json::to_value(e).unwrap(),
        json!({
            "ts": "2025-07-01T12:00:00Z",
            "type": "tool_call",
            "tool_name": "read_file",
            "tool_use_id": "tu_001",
            "parent_tool_use_id": "tu_000",
            "input": {"path": "src/main.rs"},
        }),
    );
}

#[test]
fn event_tool_call_no_ids() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({"command": "ls"}),
        },
        ext: None,
    };
    let v = serde_json::to_value(e).unwrap();
    assert_eq!(v["tool_use_id"], json!(null));
    assert_eq!(v["parent_tool_use_id"], json!(null));
}

#[test]
fn event_tool_result_success() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_001".into()),
            output: json!({"content": "fn main() {}"}),
            is_error: false,
        },
        ext: None,
    };
    assert_eq!(
        serde_json::to_value(e).unwrap(),
        json!({
            "ts": "2025-07-01T12:00:00Z",
            "type": "tool_result",
            "tool_name": "read_file",
            "tool_use_id": "tu_001",
            "output": {"content": "fn main() {}"},
            "is_error": false,
        }),
    );
}

#[test]
fn event_tool_result_error() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_002".into()),
            output: json!({"error": "not found"}),
            is_error: true,
        },
        ext: None,
    };
    let v = serde_json::to_value(e).unwrap();
    assert_eq!(v["is_error"], true);
    assert_eq!(v["type"], "tool_result");
}

#[test]
fn event_file_changed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::FileChanged {
            path: "src/auth.rs".into(),
            summary: "Added JWT validation".into(),
        },
        ext: None,
    };
    assert_eq!(
        serde_json::to_value(e).unwrap(),
        json!({
            "ts": "2025-07-01T12:00:00Z",
            "type": "file_changed",
            "path": "src/auth.rs",
            "summary": "Added JWT validation",
        }),
    );
}

#[test]
fn event_command_executed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok. 42 passed".into()),
        },
        ext: None,
    };
    assert_eq!(
        serde_json::to_value(e).unwrap(),
        json!({
            "ts": "2025-07-01T12:00:00Z",
            "type": "command_executed",
            "command": "cargo test",
            "exit_code": 0,
            "output_preview": "ok. 42 passed",
        }),
    );
}

#[test]
fn event_command_no_exit_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "sleep 60".into(),
            exit_code: None,
            output_preview: None,
        },
        ext: None,
    };
    let v = serde_json::to_value(e).unwrap();
    assert_eq!(v["exit_code"], json!(null));
    assert_eq!(v["output_preview"], json!(null));
}

#[test]
fn event_warning() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Warning {
            message: "Budget low".into(),
        },
        ext: None,
    };
    assert_eq!(
        serde_json::to_value(e).unwrap(),
        json!({
            "ts": "2025-07-01T12:00:00Z",
            "type": "warning",
            "message": "Budget low",
        }),
    );
}

#[test]
fn event_error_no_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Error {
            message: "Unexpected failure".into(),
            error_code: None,
        },
        ext: None,
    };
    let v = serde_json::to_value(e).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["message"], "Unexpected failure");
    // error_code is skip_serializing_if None
    assert!(v.get("error_code").is_none() || v["error_code"].is_null());
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
    let v = serde_json::to_value(e).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["error_code"], "backend_timeout");
}

#[test]
fn event_ext_not_serialized_when_none() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let json_str = serde_json::to_string(&e).unwrap();
    assert!(!json_str.contains("\"ext\""));
}

#[test]
fn event_with_ext_passthrough() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"role": "assistant"}));
    ext.insert("vendor_id".into(), json!("msg_abc"));
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let v = serde_json::to_value(e).unwrap();
    assert_eq!(v["ext"]["raw_message"], json!({"role": "assistant"}));
    assert_eq!(v["ext"]["vendor_id"], "msg_abc");
}

// ===========================================================================
// 4. Envelope JSONL snapshot (tag = "t")
// ===========================================================================

#[test]
fn envelope_hello_mapped() {
    let env = Envelope::hello(mock_backend(), BTreeMap::new());
    assert_eq!(
        serde_json::to_value(env).unwrap(),
        json!({
            "t": "hello",
            "contract_version": "abp/v0.1",
            "backend": {"id": "mock", "backend_version": null, "adapter_version": null},
            "capabilities": {},
            "mode": "mapped",
        }),
    );
}

#[test]
fn envelope_hello_passthrough() {
    let env =
        Envelope::hello_with_mode(mock_backend(), BTreeMap::new(), ExecutionMode::Passthrough);
    let v = serde_json::to_value(env).unwrap();
    assert_eq!(v["mode"], "passthrough");
}

#[test]
fn envelope_hello_with_capabilities() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Emulated);
    let env = Envelope::hello(mock_backend(), caps);
    let v = serde_json::to_value(env).unwrap();
    assert_eq!(v["capabilities"]["streaming"], "native");
    assert_eq!(v["capabilities"]["tool_use"], "emulated");
}

#[test]
fn envelope_run_structure() {
    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: minimal_work_order(),
    };
    let v = serde_json::to_value(env).unwrap();
    assert_eq!(v["t"], "run");
    assert_eq!(v["id"], "run-001");
    assert_eq!(v["work_order"]["task"], "Hello world");
}

#[test]
fn envelope_event_structure() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Working...".into(),
            },
            ext: None,
        },
    };
    let v = serde_json::to_value(env).unwrap();
    assert_eq!(v["t"], "event");
    assert_eq!(v["ref_id"], "run-001");
    assert_eq!(v["event"]["type"], "assistant_message");
    assert_eq!(v["event"]["text"], "Working...");
}

#[test]
fn envelope_final_structure() {
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: minimal_receipt(),
    };
    let v = serde_json::to_value(env).unwrap();
    assert_eq!(v["t"], "final");
    assert_eq!(v["ref_id"], "run-001");
    assert_eq!(v["receipt"]["outcome"], "complete");
}

#[test]
fn envelope_fatal_with_ref() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "Backend crashed".into(),
        error_code: None,
    };
    let v = serde_json::to_value(env).unwrap();
    assert_eq!(v["t"], "fatal");
    assert_eq!(v["ref_id"], "run-001");
    assert_eq!(v["error"], "Backend crashed");
    // error_code absent when None (skip_serializing_if)
    assert!(v.get("error_code").is_none());
}

#[test]
fn envelope_fatal_no_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "Handshake timeout".into(),
        error_code: None,
    };
    let v = serde_json::to_value(env).unwrap();
    assert_eq!(v["ref_id"], json!(null));
}

#[test]
fn envelope_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-001".into()),
        "Backend not found",
        abp_error::ErrorCode::BackendNotFound,
    );
    let v = serde_json::to_value(env).unwrap();
    assert_eq!(v["error_code"], "backend_not_found");
}

#[test]
fn envelope_tag_is_t_not_type() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
    assert!(!json.contains(r#""type":"fatal""#));
}

#[test]
fn envelope_jsonl_newline_terminated() {
    let env = Envelope::hello(mock_backend(), BTreeMap::new());
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.ends_with('\n'));
    assert!(!line.ends_with("\n\n"));
}

#[test]
fn envelope_jsonl_roundtrip_hello() {
    let env = Envelope::hello(mock_backend(), BTreeMap::new());
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn envelope_jsonl_roundtrip_fatal() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert_eq!(error, "boom");
    } else {
        panic!("Expected Fatal");
    }
}

#[test]
fn envelope_jsonl_decode_stream() {
    let envelopes = vec![
        Envelope::hello(mock_backend(), BTreeMap::new()),
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

// ===========================================================================
// 5. IR type snapshots
// ===========================================================================

#[test]
fn ir_role_system() {
    assert_eq!(
        serde_json::to_string(&IrRole::System).unwrap(),
        r#""system""#
    );
}

#[test]
fn ir_role_user() {
    assert_eq!(serde_json::to_string(&IrRole::User).unwrap(), r#""user""#);
}

#[test]
fn ir_role_assistant() {
    assert_eq!(
        serde_json::to_string(&IrRole::Assistant).unwrap(),
        r#""assistant""#,
    );
}

#[test]
fn ir_role_tool() {
    assert_eq!(serde_json::to_string(&IrRole::Tool).unwrap(), r#""tool""#);
}

#[test]
fn ir_content_text() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    assert_eq!(
        serde_json::to_value(block).unwrap(),
        json!({"type": "text", "text": "hello"}),
    );
}

#[test]
fn ir_content_image() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "aGVsbG8=".into(),
    };
    assert_eq!(
        serde_json::to_value(block).unwrap(),
        json!({"type": "image", "media_type": "image/png", "data": "aGVsbG8="}),
    );
}

#[test]
fn ir_content_tool_use() {
    let block = IrContentBlock::ToolUse {
        id: "tu_001".into(),
        name: "read".into(),
        input: json!({"path": "file.rs"}),
    };
    assert_eq!(
        serde_json::to_value(block).unwrap(),
        json!({
            "type": "tool_use",
            "id": "tu_001",
            "name": "read",
            "input": {"path": "file.rs"},
        }),
    );
}

#[test]
fn ir_content_tool_result() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "tu_001".into(),
        content: vec![IrContentBlock::Text {
            text: "file content".into(),
        }],
        is_error: false,
    };
    assert_eq!(
        serde_json::to_value(block).unwrap(),
        json!({
            "type": "tool_result",
            "tool_use_id": "tu_001",
            "content": [{"type": "text", "text": "file content"}],
            "is_error": false,
        }),
    );
}

#[test]
fn ir_content_thinking() {
    let block = IrContentBlock::Thinking {
        text: "Let me think...".into(),
    };
    assert_eq!(
        serde_json::to_value(block).unwrap(),
        json!({"type": "thinking", "text": "Let me think..."}),
    );
}

#[test]
fn ir_message_text_only() {
    let msg = IrMessage::text(IrRole::User, "hello");
    assert_eq!(
        serde_json::to_value(msg).unwrap(),
        json!({
            "role": "user",
            "content": [{"type": "text", "text": "hello"}],
        }),
    );
}

#[test]
fn ir_message_with_metadata() {
    let mut msg = IrMessage::text(IrRole::Assistant, "hi");
    msg.metadata.insert("source".into(), json!("test"));
    assert_eq!(
        serde_json::to_value(msg).unwrap(),
        json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "hi"}],
            "metadata": {"source": "test"},
        }),
    );
}

#[test]
fn ir_message_metadata_skipped_when_empty() {
    let msg = IrMessage::text(IrRole::User, "hello");
    let json_str = serde_json::to_string(&msg).unwrap();
    assert!(!json_str.contains("\"metadata\""));
}

#[test]
fn ir_conversation_empty() {
    assert_eq!(
        serde_json::to_value(IrConversation::new()).unwrap(),
        json!({"messages": []}),
    );
}

#[test]
fn ir_conversation_with_messages() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hi"));
    let v = serde_json::to_value(conv).unwrap();
    assert_eq!(v["messages"].as_array().unwrap().len(), 2);
    assert_eq!(v["messages"][0]["role"], "system");
    assert_eq!(v["messages"][1]["role"], "user");
}

#[test]
fn ir_tool_definition() {
    let td = IrToolDefinition {
        name: "read_file".into(),
        description: "Read file contents".into(),
        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    assert_eq!(
        serde_json::to_value(td).unwrap(),
        json!({
            "name": "read_file",
            "description": "Read file contents",
            "parameters": {
                "type": "object",
                "properties": {"path": {"type": "string"}},
            },
        }),
    );
}

#[test]
fn ir_usage_default() {
    assert_eq!(
        serde_json::to_value(IrUsage::default()).unwrap(),
        json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0,
            "cache_read_tokens": 0,
            "cache_write_tokens": 0,
        }),
    );
}

#[test]
fn ir_usage_from_io() {
    let u = IrUsage::from_io(100, 50);
    assert_eq!(
        serde_json::to_value(u).unwrap(),
        json!({
            "input_tokens": 100,
            "output_tokens": 50,
            "total_tokens": 150,
            "cache_read_tokens": 0,
            "cache_write_tokens": 0,
        }),
    );
}

#[test]
fn ir_usage_with_cache() {
    let u = IrUsage::with_cache(500, 200, 100, 50);
    assert_eq!(
        serde_json::to_value(u).unwrap(),
        json!({
            "input_tokens": 500,
            "output_tokens": 200,
            "total_tokens": 700,
            "cache_read_tokens": 100,
            "cache_write_tokens": 50,
        }),
    );
}

// ===========================================================================
// 6. Error type Display snapshot
// ===========================================================================

#[test]
fn error_code_display_backend_timeout() {
    assert_eq!(
        abp_error::ErrorCode::BackendTimeout.as_str(),
        "backend_timeout",
    );
}

#[test]
fn error_code_display_policy_denied() {
    assert_eq!(abp_error::ErrorCode::PolicyDenied.as_str(), "policy_denied",);
}

#[test]
fn error_code_display_internal() {
    assert_eq!(abp_error::ErrorCode::Internal.as_str(), "internal");
}

#[test]
fn error_category_display() {
    assert_eq!(abp_error::ErrorCategory::Protocol.to_string(), "protocol");
    assert_eq!(abp_error::ErrorCategory::Backend.to_string(), "backend");
    assert_eq!(abp_error::ErrorCategory::Policy.to_string(), "policy");
    assert_eq!(abp_error::ErrorCategory::Ir.to_string(), "ir");
}

#[test]
fn error_code_json_serialization() {
    assert_eq!(
        serde_json::to_string(&abp_error::ErrorCode::ProtocolInvalidEnvelope).unwrap(),
        r#""protocol_invalid_envelope""#,
    );
    assert_eq!(
        serde_json::to_string(&abp_error::ErrorCode::ReceiptHashMismatch).unwrap(),
        r#""receipt_hash_mismatch""#,
    );
}

#[test]
fn abp_error_display_simple() {
    let err =
        abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "timed out after 30 s");
    assert_eq!(err.to_string(), "[backend_timeout] timed out after 30 s");
}

#[test]
fn abp_error_display_with_context() {
    let err = abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "openai");
    let display = err.to_string();
    assert!(display.starts_with("[backend_timeout] timed out"));
    assert!(display.contains("openai"));
}

#[test]
fn protocol_error_display_violation() {
    let err = ProtocolError::Violation("missing ref_id".into());
    assert_eq!(err.to_string(), "protocol violation: missing ref_id");
}

#[test]
fn protocol_error_display_unexpected_message() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(
        err.to_string(),
        "unexpected message: expected hello, got run",
    );
}

// ===========================================================================
// 7. Capability / SupportLevel snapshots
// ===========================================================================

#[test]
fn capability_streaming_json() {
    assert_eq!(
        serde_json::to_string(&Capability::Streaming).unwrap(),
        r#""streaming""#,
    );
}

#[test]
fn capability_tool_read_json() {
    assert_eq!(
        serde_json::to_string(&Capability::ToolRead).unwrap(),
        r#""tool_read""#,
    );
}

#[test]
fn capability_tool_write_json() {
    assert_eq!(
        serde_json::to_string(&Capability::ToolWrite).unwrap(),
        r#""tool_write""#,
    );
}

#[test]
fn capability_tool_edit_json() {
    assert_eq!(
        serde_json::to_string(&Capability::ToolEdit).unwrap(),
        r#""tool_edit""#,
    );
}

#[test]
fn capability_tool_bash_json() {
    assert_eq!(
        serde_json::to_string(&Capability::ToolBash).unwrap(),
        r#""tool_bash""#,
    );
}

#[test]
fn capability_tool_use_json() {
    assert_eq!(
        serde_json::to_string(&Capability::ToolUse).unwrap(),
        r#""tool_use""#,
    );
}

#[test]
fn capability_extended_thinking_json() {
    assert_eq!(
        serde_json::to_string(&Capability::ExtendedThinking).unwrap(),
        r#""extended_thinking""#,
    );
}

#[test]
fn capability_mcp_client_json() {
    assert_eq!(
        serde_json::to_string(&Capability::McpClient).unwrap(),
        r#""mcp_client""#,
    );
}

#[test]
fn support_level_native_json() {
    assert_eq!(
        serde_json::to_string(&SupportLevel::Native).unwrap(),
        r#""native""#,
    );
}

#[test]
fn support_level_emulated_json() {
    assert_eq!(
        serde_json::to_string(&SupportLevel::Emulated).unwrap(),
        r#""emulated""#,
    );
}

#[test]
fn support_level_unsupported_json() {
    assert_eq!(
        serde_json::to_string(&SupportLevel::Unsupported).unwrap(),
        r#""unsupported""#,
    );
}

#[test]
fn support_level_restricted_json() {
    let sl = SupportLevel::Restricted {
        reason: "experimental".into(),
    };
    assert_eq!(
        serde_json::to_value(sl).unwrap(),
        json!({"restricted": {"reason": "experimental"}}),
    );
}

#[test]
fn min_support_native_json() {
    assert_eq!(
        serde_json::to_string(&MinSupport::Native).unwrap(),
        r#""native""#,
    );
}

#[test]
fn min_support_emulated_json() {
    assert_eq!(
        serde_json::to_string(&MinSupport::Emulated).unwrap(),
        r#""emulated""#,
    );
}

#[test]
fn capability_manifest_with_restricted() {
    let mut m: CapabilityManifest = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(
        Capability::McpClient,
        SupportLevel::Restricted {
            reason: "beta".into(),
        },
    );
    let v = serde_json::to_value(m).unwrap();
    assert_eq!(v["streaming"], "native");
    assert_eq!(v["mcp_client"], json!({"restricted": {"reason": "beta"}}),);
}

#[test]
fn execution_mode_default_is_mapped() {
    let mode = ExecutionMode::default();
    assert_eq!(mode, ExecutionMode::Mapped);
    assert_eq!(serde_json::to_string(&mode).unwrap(), r#""mapped""#);
}

#[test]
fn execution_mode_passthrough_json() {
    assert_eq!(
        serde_json::to_string(&ExecutionMode::Passthrough).unwrap(),
        r#""passthrough""#,
    );
}

#[test]
fn contract_version_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

// ===========================================================================
// 8. PolicyProfile snapshots
// ===========================================================================

#[test]
fn policy_profile_default() {
    assert_eq!(
        serde_json::to_value(PolicyProfile::default()).unwrap(),
        json!({
            "allowed_tools": [],
            "disallowed_tools": [],
            "deny_read": [],
            "deny_write": [],
            "allow_network": [],
            "deny_network": [],
            "require_approval_for": [],
        }),
    );
}

#[test]
fn policy_profile_full() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec![".env".into()],
        deny_write: vec!["Cargo.lock".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["execute".into()],
    };
    assert_eq!(
        serde_json::to_value(p).unwrap(),
        json!({
            "allowed_tools": ["read", "write"],
            "disallowed_tools": ["bash"],
            "deny_read": [".env"],
            "deny_write": ["Cargo.lock"],
            "allow_network": ["api.example.com"],
            "deny_network": ["*.evil.com"],
            "require_approval_for": ["execute"],
        }),
    );
}

#[test]
fn policy_profile_tools_only() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into(), "glob".into()],
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    let v = serde_json::to_value(p).unwrap();
    assert_eq!(v["allowed_tools"], json!(["read", "glob"]));
    assert_eq!(v["disallowed_tools"], json!(["bash"]));
    assert_eq!(v["deny_read"], json!([]));
}

#[test]
fn policy_profile_paths_only() {
    let p = PolicyProfile {
        deny_read: vec![".env".into(), "secrets/**".into()],
        deny_write: vec!["Cargo.lock".into()],
        ..Default::default()
    };
    let v = serde_json::to_value(p).unwrap();
    assert_eq!(v["deny_read"], json!([".env", "secrets/**"]));
    assert_eq!(v["deny_write"], json!(["Cargo.lock"]));
    assert_eq!(v["allowed_tools"], json!([]));
}

// ===========================================================================
// 9. Deterministic serialization (BTreeMap ordering)
// ===========================================================================

#[test]
fn deterministic_vendor_btreemap_ordering() {
    let mut vendor: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    vendor.insert("zebra".into(), json!(1));
    vendor.insert("alpha".into(), json!(2));
    vendor.insert("mid".into(), json!(3));
    let json = serde_json::to_string(&vendor).unwrap();
    // BTreeMap keys are sorted alphabetically
    let alpha_pos = json.find("\"alpha\"").unwrap();
    let mid_pos = json.find("\"mid\"").unwrap();
    let zebra_pos = json.find("\"zebra\"").unwrap();
    assert!(alpha_pos < mid_pos);
    assert!(mid_pos < zebra_pos);
}

#[test]
fn deterministic_env_btreemap_ordering() {
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    env.insert("RUST_LOG".into(), "debug".into());
    env.insert("API_KEY".into(), "secret".into());
    env.insert("HOME".into(), "/home/user".into());
    let json = serde_json::to_string(&env).unwrap();
    let api_pos = json.find("\"API_KEY\"").unwrap();
    let home_pos = json.find("\"HOME\"").unwrap();
    let rust_pos = json.find("\"RUST_LOG\"").unwrap();
    assert!(api_pos < home_pos);
    assert!(home_pos < rust_pos);
}

#[test]
fn deterministic_capability_manifest_ordering() {
    let mut m: CapabilityManifest = BTreeMap::new();
    m.insert(Capability::ToolWrite, SupportLevel::Native);
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    let json = serde_json::to_string(&m).unwrap();
    let streaming_pos = json.find("\"streaming\"").unwrap();
    let read_pos = json.find("\"tool_read\"").unwrap();
    let write_pos = json.find("\"tool_write\"").unwrap();
    assert!(streaming_pos < read_pos);
    assert!(read_pos < write_pos);
}

#[test]
fn deterministic_canonical_json() {
    let r = minimal_receipt();
    let json1 = abp_core::canonical_json(&r).unwrap();
    let json2 = abp_core::canonical_json(&r).unwrap();
    assert_eq!(json1, json2, "canonical_json must be deterministic");
}

#[test]
fn deterministic_receipt_hash_stable() {
    let r1 = minimal_receipt().with_hash().unwrap();
    let r2 = minimal_receipt().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    let hash = r1.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn deterministic_receipt_hash_ignores_sha256() {
    let r1 = minimal_receipt();
    let h1 = abp_core::receipt_hash(&r1).unwrap();

    let mut r2 = minimal_receipt();
    r2.receipt_sha256 = Some("should_be_ignored".into());
    let h2 = abp_core::receipt_hash(&r2).unwrap();

    assert_eq!(h1, h2, "receipt_sha256 must not affect the hash");
}
