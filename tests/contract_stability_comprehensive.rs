// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive contract stability and backward-compatibility tests.
//!
//! These tests pin the ABP wire format so that any accidental change to
//! field names, enum variants, tag formats, or serialization order is
//! caught immediately.

use std::collections::BTreeMap;

use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, ArtifactRef,
    BackendIdentity, Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

// ── helpers ──────────────────────────────────────────────────────────────

fn fixed_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_time2() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 42).unwrap()
}

fn nil_uuid() -> Uuid {
    Uuid::nil()
}

fn minimal_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: nil_uuid(),
            work_order_id: nil_uuid(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_time(),
            finished_at: fixed_time2(),
            duration_ms: 42_000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn minimal_work_order() -> WorkOrder {
    WorkOrder {
        id: nil_uuid(),
        task: "test task".into(),
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

fn sorted_keys(v: &Value) -> Vec<String> {
    v.as_object().unwrap().keys().cloned().collect::<Vec<_>>()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1  CONTRACT_VERSION
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contract_version_exact_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_starts_with_abp_prefix() {
    assert!(CONTRACT_VERSION.starts_with("abp/"));
}

#[test]
fn contract_version_has_semver_minor() {
    let version_part = CONTRACT_VERSION.strip_prefix("abp/v").unwrap();
    assert!(version_part.contains('.'));
}

#[test]
fn contract_version_is_ascii() {
    assert!(CONTRACT_VERSION.is_ascii());
}

#[test]
fn contract_version_no_whitespace() {
    assert!(!CONTRACT_VERSION.contains(' '));
    assert!(!CONTRACT_VERSION.contains('\t'));
    assert!(!CONTRACT_VERSION.contains('\n'));
}

#[test]
fn contract_version_no_null_bytes() {
    assert!(!CONTRACT_VERSION.contains('\0'));
}

#[test]
fn contract_version_static_lifetime() {
    let _: &'static str = CONTRACT_VERSION;
}

// ═══════════════════════════════════════════════════════════════════════════
// 2  WorkOrder field names stability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_json_has_expected_top_level_keys() {
    let wo = minimal_work_order();
    let v: Value = serde_json::to_value(&wo).unwrap();
    let keys = sorted_keys(&v);
    for expected in &[
        "id",
        "task",
        "lane",
        "workspace",
        "context",
        "policy",
        "requirements",
        "config",
    ] {
        assert!(
            keys.contains(&expected.to_string()),
            "missing key: {expected}"
        );
    }
}

#[test]
fn work_order_workspace_has_expected_keys() {
    let wo = minimal_work_order();
    let v: Value = serde_json::to_value(&wo).unwrap();
    let ws = &v["workspace"];
    for expected in &["root", "mode", "include", "exclude"] {
        assert!(
            ws.get(expected).is_some(),
            "missing workspace key: {expected}"
        );
    }
}

#[test]
fn work_order_context_has_expected_keys() {
    let wo = minimal_work_order();
    let v: Value = serde_json::to_value(&wo).unwrap();
    let ctx = &v["context"];
    assert!(ctx.get("files").is_some());
    assert!(ctx.get("snippets").is_some());
}

#[test]
fn work_order_policy_has_expected_keys() {
    let wo = minimal_work_order();
    let v: Value = serde_json::to_value(&wo).unwrap();
    let pol = &v["policy"];
    for expected in &[
        "allowed_tools",
        "disallowed_tools",
        "deny_read",
        "deny_write",
        "allow_network",
        "deny_network",
        "require_approval_for",
    ] {
        assert!(
            pol.get(expected).is_some(),
            "missing policy key: {expected}"
        );
    }
}

#[test]
fn work_order_requirements_has_required_field() {
    let wo = minimal_work_order();
    let v: Value = serde_json::to_value(&wo).unwrap();
    assert!(v["requirements"].get("required").is_some());
}

#[test]
fn work_order_config_has_expected_keys() {
    let wo = minimal_work_order();
    let v: Value = serde_json::to_value(&wo).unwrap();
    let cfg = &v["config"];
    for expected in &["model", "vendor", "env", "max_budget_usd", "max_turns"] {
        assert!(
            cfg.get(expected).is_some(),
            "missing config key: {expected}"
        );
    }
}

#[test]
fn work_order_id_is_uuid_string() {
    let wo = minimal_work_order();
    let v: Value = serde_json::to_value(&wo).unwrap();
    let id_str = v["id"].as_str().unwrap();
    assert!(Uuid::parse_str(id_str).is_ok());
}

#[test]
fn work_order_roundtrip() {
    let wo = minimal_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.task, wo2.task);
    assert_eq!(wo.id, wo2.id);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3  Receipt field names stability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_json_has_expected_top_level_keys() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    for expected in &[
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
        assert!(v.get(expected).is_some(), "missing receipt key: {expected}");
    }
}

#[test]
fn receipt_meta_has_expected_keys() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    let meta = &v["meta"];
    for expected in &[
        "run_id",
        "work_order_id",
        "contract_version",
        "started_at",
        "finished_at",
        "duration_ms",
    ] {
        assert!(meta.get(expected).is_some(), "missing meta key: {expected}");
    }
}

#[test]
fn receipt_backend_has_expected_keys() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    let be = &v["backend"];
    for expected in &["id", "backend_version", "adapter_version"] {
        assert!(
            be.get(expected).is_some(),
            "missing backend key: {expected}"
        );
    }
}

#[test]
fn receipt_usage_has_expected_keys() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    let u = &v["usage"];
    for expected in &[
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "request_units",
        "estimated_cost_usd",
    ] {
        assert!(u.get(expected).is_some(), "missing usage key: {expected}");
    }
}

#[test]
fn receipt_verification_has_expected_keys() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    let ver = &v["verification"];
    for expected in &["git_diff", "git_status", "harness_ok"] {
        assert!(
            ver.get(expected).is_some(),
            "missing verification key: {expected}"
        );
    }
}

#[test]
fn receipt_roundtrip() {
    let r = minimal_receipt();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.meta.contract_version, r2.meta.contract_version);
}

#[test]
fn receipt_contract_version_embedded() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["meta"]["contract_version"].as_str().unwrap(), "abp/v0.1");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4  AgentEvent serialization format stability
// ═══════════════════════════════════════════════════════════════════════════

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: fixed_time(),
        kind,
        ext: None,
    }
}

#[test]
fn agent_event_has_ts_field() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "hi".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("ts").is_some());
}

#[test]
fn agent_event_has_type_tag() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "hi".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("type").is_some(), "AgentEventKind tag 'type' missing");
}

#[test]
fn agent_event_kind_flattened_into_event() {
    let e = make_event(AgentEventKind::AssistantDelta { text: "tok".into() });
    let v: Value = serde_json::to_value(&e).unwrap();
    // The 'text' field from AssistantDelta should be at the top level due to #[serde(flatten)]
    assert!(v.get("text").is_some());
    assert_eq!(v["type"].as_str().unwrap(), "assistant_delta");
}

#[test]
fn agent_event_ext_omitted_when_none() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "hi".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("ext").is_none(), "ext should be omitted when None");
}

#[test]
fn agent_event_ext_present_when_some() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"vendor": "test"}));
    let e = AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunStarted {
            message: "hi".into(),
        },
        ext: Some(ext),
    };
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("ext").is_some());
}

#[test]
fn agent_event_roundtrip_run_started() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "starting".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(e2.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn agent_event_roundtrip_run_completed() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(e2.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn agent_event_roundtrip_assistant_delta() {
    let e = make_event(AgentEventKind::AssistantDelta {
        text: "hello".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantDelta { text } = &e2.kind {
        assert_eq!(text, "hello");
    } else {
        panic!("expected AssistantDelta");
    }
}

#[test]
fn agent_event_roundtrip_assistant_message() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "full msg".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantMessage { text } = &e2.kind {
        assert_eq!(text, "full msg");
    } else {
        panic!("expected AssistantMessage");
    }
}

#[test]
fn agent_event_roundtrip_tool_call() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("tu_1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "foo.rs"}),
    });
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolCall {
        tool_name,
        tool_use_id,
        ..
    } = &e2.kind
    {
        assert_eq!(tool_name, "read");
        assert_eq!(tool_use_id.as_deref(), Some("tu_1"));
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn agent_event_roundtrip_tool_result() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: Some("tu_1".into()),
        output: json!("file contents"),
        is_error: false,
    });
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolResult { is_error, .. } = &e2.kind {
        assert!(!is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn agent_event_roundtrip_file_changed() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "added fn main".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(e2.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn agent_event_roundtrip_command_executed() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    });
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(e2.kind, AgentEventKind::CommandExecuted { .. }));
}

#[test]
fn agent_event_roundtrip_warning() {
    let e = make_event(AgentEventKind::Warning {
        message: "be careful".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(e2.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn agent_event_roundtrip_error() {
    let e = make_event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    });
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(e2.kind, AgentEventKind::Error { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 5  AgentEventKind variant names stability (snake_case)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn event_kind_run_started_serializes_as_snake_case() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "x".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "run_started");
}

#[test]
fn event_kind_run_completed_serializes_as_snake_case() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "x".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "run_completed");
}

#[test]
fn event_kind_assistant_delta_serializes_as_snake_case() {
    let e = make_event(AgentEventKind::AssistantDelta { text: "x".into() });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "assistant_delta");
}

#[test]
fn event_kind_assistant_message_serializes_as_snake_case() {
    let e = make_event(AgentEventKind::AssistantMessage { text: "x".into() });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "assistant_message");
}

#[test]
fn event_kind_tool_call_serializes_as_snake_case() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "t".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "tool_call");
}

#[test]
fn event_kind_tool_result_serializes_as_snake_case() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "t".into(),
        tool_use_id: None,
        output: json!(null),
        is_error: false,
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "tool_result");
}

#[test]
fn event_kind_file_changed_serializes_as_snake_case() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "a".into(),
        summary: "b".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "file_changed");
}

#[test]
fn event_kind_command_executed_serializes_as_snake_case() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: None,
        output_preview: None,
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "command_executed");
}

#[test]
fn event_kind_warning_serializes_as_snake_case() {
    let e = make_event(AgentEventKind::Warning {
        message: "x".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "warning");
}

#[test]
fn event_kind_error_serializes_as_snake_case() {
    let e = make_event(AgentEventKind::Error {
        message: "x".into(),
        error_code: None,
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "error");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6  Outcome (ReceiptStatus) variant names stability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn outcome_complete_serializes_as_snake_case() {
    let v: Value = serde_json::to_value(&Outcome::Complete).unwrap();
    assert_eq!(v.as_str().unwrap(), "complete");
}

#[test]
fn outcome_partial_serializes_as_snake_case() {
    let v: Value = serde_json::to_value(&Outcome::Partial).unwrap();
    assert_eq!(v.as_str().unwrap(), "partial");
}

#[test]
fn outcome_failed_serializes_as_snake_case() {
    let v: Value = serde_json::to_value(&Outcome::Failed).unwrap();
    assert_eq!(v.as_str().unwrap(), "failed");
}

#[test]
fn outcome_complete_deserializes_from_string() {
    let o: Outcome = serde_json::from_str(r#""complete""#).unwrap();
    assert_eq!(o, Outcome::Complete);
}

#[test]
fn outcome_partial_deserializes_from_string() {
    let o: Outcome = serde_json::from_str(r#""partial""#).unwrap();
    assert_eq!(o, Outcome::Partial);
}

#[test]
fn outcome_failed_deserializes_from_string() {
    let o: Outcome = serde_json::from_str(r#""failed""#).unwrap();
    assert_eq!(o, Outcome::Failed);
}

#[test]
fn outcome_roundtrip_complete() {
    let j = serde_json::to_string(&Outcome::Complete).unwrap();
    let o: Outcome = serde_json::from_str(&j).unwrap();
    assert_eq!(o, Outcome::Complete);
}

#[test]
fn outcome_roundtrip_partial() {
    let j = serde_json::to_string(&Outcome::Partial).unwrap();
    let o: Outcome = serde_json::from_str(&j).unwrap();
    assert_eq!(o, Outcome::Partial);
}

#[test]
fn outcome_roundtrip_failed() {
    let j = serde_json::to_string(&Outcome::Failed).unwrap();
    let o: Outcome = serde_json::from_str(&j).unwrap();
    assert_eq!(o, Outcome::Failed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7  CapabilityProfile field/variant names stability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_streaming_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::Streaming).unwrap();
    assert_eq!(v.as_str().unwrap(), "streaming");
}

#[test]
fn capability_tool_read_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::ToolRead).unwrap();
    assert_eq!(v.as_str().unwrap(), "tool_read");
}

#[test]
fn capability_tool_write_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::ToolWrite).unwrap();
    assert_eq!(v.as_str().unwrap(), "tool_write");
}

#[test]
fn capability_tool_edit_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::ToolEdit).unwrap();
    assert_eq!(v.as_str().unwrap(), "tool_edit");
}

#[test]
fn capability_tool_bash_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::ToolBash).unwrap();
    assert_eq!(v.as_str().unwrap(), "tool_bash");
}

#[test]
fn capability_tool_glob_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::ToolGlob).unwrap();
    assert_eq!(v.as_str().unwrap(), "tool_glob");
}

#[test]
fn capability_tool_grep_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::ToolGrep).unwrap();
    assert_eq!(v.as_str().unwrap(), "tool_grep");
}

#[test]
fn capability_tool_web_search_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::ToolWebSearch).unwrap();
    assert_eq!(v.as_str().unwrap(), "tool_web_search");
}

#[test]
fn capability_tool_web_fetch_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::ToolWebFetch).unwrap();
    assert_eq!(v.as_str().unwrap(), "tool_web_fetch");
}

#[test]
fn capability_tool_ask_user_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::ToolAskUser).unwrap();
    assert_eq!(v.as_str().unwrap(), "tool_ask_user");
}

#[test]
fn capability_hooks_pre_tool_use_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::HooksPreToolUse).unwrap();
    assert_eq!(v.as_str().unwrap(), "hooks_pre_tool_use");
}

#[test]
fn capability_hooks_post_tool_use_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::HooksPostToolUse).unwrap();
    assert_eq!(v.as_str().unwrap(), "hooks_post_tool_use");
}

#[test]
fn capability_session_resume_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::SessionResume).unwrap();
    assert_eq!(v.as_str().unwrap(), "session_resume");
}

#[test]
fn capability_session_fork_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::SessionFork).unwrap();
    assert_eq!(v.as_str().unwrap(), "session_fork");
}

#[test]
fn capability_checkpointing_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::Checkpointing).unwrap();
    assert_eq!(v.as_str().unwrap(), "checkpointing");
}

#[test]
fn capability_structured_output_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::StructuredOutputJsonSchema).unwrap();
    assert_eq!(v.as_str().unwrap(), "structured_output_json_schema");
}

#[test]
fn capability_mcp_client_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::McpClient).unwrap();
    assert_eq!(v.as_str().unwrap(), "mcp_client");
}

#[test]
fn capability_mcp_server_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::McpServer).unwrap();
    assert_eq!(v.as_str().unwrap(), "mcp_server");
}

#[test]
fn capability_tool_use_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::ToolUse).unwrap();
    assert_eq!(v.as_str().unwrap(), "tool_use");
}

#[test]
fn capability_extended_thinking_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::ExtendedThinking).unwrap();
    assert_eq!(v.as_str().unwrap(), "extended_thinking");
}

#[test]
fn capability_image_input_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::ImageInput).unwrap();
    assert_eq!(v.as_str().unwrap(), "image_input");
}

#[test]
fn capability_pdf_input_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::PdfInput).unwrap();
    assert_eq!(v.as_str().unwrap(), "pdf_input");
}

#[test]
fn capability_code_execution_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::CodeExecution).unwrap();
    assert_eq!(v.as_str().unwrap(), "code_execution");
}

#[test]
fn capability_logprobs_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::Logprobs).unwrap();
    assert_eq!(v.as_str().unwrap(), "logprobs");
}

#[test]
fn capability_seed_determinism_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::SeedDeterminism).unwrap();
    assert_eq!(v.as_str().unwrap(), "seed_determinism");
}

#[test]
fn capability_stop_sequences_serializes_correctly() {
    let v: Value = serde_json::to_value(&Capability::StopSequences).unwrap();
    assert_eq!(v.as_str().unwrap(), "stop_sequences");
}

#[test]
fn capability_roundtrip_all_variants() {
    let all = vec![
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
    ];
    for cap in all {
        let json = serde_json::to_string(&cap).unwrap();
        let roundtripped: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, roundtripped);
    }
}

#[test]
fn capability_manifest_is_btreemap() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::Streaming, SupportLevel::Emulated);
    let v: Value = serde_json::to_value(&manifest).unwrap();
    let keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    // BTreeMap should produce sorted keys
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8  JSON schema generation consistency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_schema_generates_without_panic() {
    let schema = schemars::generate::SchemaSettings::openapi3()
        .into_generator()
        .into_root_schema_for::<WorkOrder>();
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(!json.is_empty());
}

#[test]
fn receipt_schema_generates_without_panic() {
    let schema = schemars::generate::SchemaSettings::openapi3()
        .into_generator()
        .into_root_schema_for::<Receipt>();
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(!json.is_empty());
}

#[test]
fn agent_event_schema_generates_without_panic() {
    let schema = schemars::generate::SchemaSettings::openapi3()
        .into_generator()
        .into_root_schema_for::<AgentEvent>();
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(!json.is_empty());
}

#[test]
fn capability_schema_generates_without_panic() {
    let schema = schemars::generate::SchemaSettings::openapi3()
        .into_generator()
        .into_root_schema_for::<Capability>();
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(!json.is_empty());
}

#[test]
fn work_order_schema_is_deterministic() {
    let s1 = serde_json::to_string(
        &schemars::generate::SchemaSettings::openapi3()
            .into_generator()
            .into_root_schema_for::<WorkOrder>(),
    )
    .unwrap();
    let s2 = serde_json::to_string(
        &schemars::generate::SchemaSettings::openapi3()
            .into_generator()
            .into_root_schema_for::<WorkOrder>(),
    )
    .unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn receipt_schema_is_deterministic() {
    let s1 = serde_json::to_string(
        &schemars::generate::SchemaSettings::openapi3()
            .into_generator()
            .into_root_schema_for::<Receipt>(),
    )
    .unwrap();
    let s2 = serde_json::to_string(
        &schemars::generate::SchemaSettings::openapi3()
            .into_generator()
            .into_root_schema_for::<Receipt>(),
    )
    .unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn outcome_schema_references_three_variants() {
    let schema = schemars::generate::SchemaSettings::openapi3()
        .into_generator()
        .into_root_schema_for::<Outcome>();
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("complete"));
    assert!(json.contains("partial"));
    assert!(json.contains("failed"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9  Backward-compatible deserialization (unknown fields)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn outcome_rejects_unknown_variant() {
    let result = serde_json::from_str::<Outcome>(r#""unknown_status""#);
    assert!(result.is_err());
}

#[test]
fn execution_lane_rejects_unknown_variant() {
    let result = serde_json::from_str::<ExecutionLane>(r#""sideways""#);
    assert!(result.is_err());
}

#[test]
fn execution_mode_rejects_unknown_variant() {
    let result = serde_json::from_str::<ExecutionMode>(r#""hybrid""#);
    assert!(result.is_err());
}

#[test]
fn workspace_mode_rejects_unknown_variant() {
    let result = serde_json::from_str::<WorkspaceMode>(r#""cloud""#);
    assert!(result.is_err());
}

#[test]
fn min_support_rejects_unknown_variant() {
    let result = serde_json::from_str::<MinSupport>(r#""partial""#);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 10  Forward-compatible serialization (extra data)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn runtime_config_vendor_preserves_arbitrary_data() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "custom_backend".into(),
        json!({"nested": true, "count": 42}),
    );
    let json = serde_json::to_string(&config).unwrap();
    let config2: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config2.vendor["custom_backend"]["nested"], json!(true));
    assert_eq!(config2.vendor["custom_backend"]["count"], json!(42));
}

#[test]
fn agent_event_ext_preserves_arbitrary_data() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        json!({"sdk": "claude", "version": "3.5"}),
    );
    ext.insert("extra_field".into(), json!(123));
    let e = AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    let ext2 = e2.ext.unwrap();
    assert_eq!(ext2["raw_message"]["sdk"], json!("claude"));
    assert_eq!(ext2["extra_field"], json!(123));
}

#[test]
fn receipt_usage_raw_preserves_arbitrary_vendor_data() {
    let mut r = minimal_receipt();
    r.usage_raw = json!({"anthropic": {"cache_creation_input_tokens": 100}});
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(
        r2.usage_raw["anthropic"]["cache_creation_input_tokens"],
        json!(100)
    );
}

#[test]
fn runtime_config_env_preserves_entries() {
    let mut config = RuntimeConfig::default();
    config.env.insert("API_KEY".into(), "secret".into());
    config.env.insert("REGION".into(), "us-west-2".into());
    let json = serde_json::to_string(&config).unwrap();
    let config2: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config2.env["API_KEY"], "secret");
    assert_eq!(config2.env["REGION"], "us-west-2");
}

// ═══════════════════════════════════════════════════════════════════════════
// 11  Canonical JSON ordering (BTreeMap ensures alphabetical keys)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn canonical_json_sorts_keys_alphabetically() {
    let v = json!({"z": 1, "a": 2, "m": 3});
    let cj = canonical_json(&v).unwrap();
    assert!(cj.starts_with(r#"{"a":2"#));
}

#[test]
fn canonical_json_sorts_nested_keys() {
    let v = json!({"outer": {"z": 1, "a": 2}});
    let cj = canonical_json(&v).unwrap();
    assert!(cj.contains(r#""a":2,"z":1"#));
}

#[test]
fn runtime_config_vendor_keys_sorted() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert("zulu".into(), json!(1));
    config.vendor.insert("alpha".into(), json!(2));
    config.vendor.insert("bravo".into(), json!(3));
    let v: Value = serde_json::to_value(&config).unwrap();
    let keys: Vec<String> = v["vendor"].as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, vec!["alpha", "bravo", "zulu"]);
}

#[test]
fn runtime_config_env_keys_sorted() {
    let mut config = RuntimeConfig::default();
    config.env.insert("ZEBRA".into(), "z".into());
    config.env.insert("APPLE".into(), "a".into());
    let v: Value = serde_json::to_value(&config).unwrap();
    let keys: Vec<String> = v["env"].as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, vec!["APPLE", "ZEBRA"]);
}

#[test]
fn capability_manifest_keys_sorted() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolWrite, SupportLevel::Native);
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolBash, SupportLevel::Native);
    let v: Value = serde_json::to_value(&manifest).unwrap();
    let keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    let mut expected = keys.clone();
    expected.sort();
    assert_eq!(keys, expected);
}

#[test]
fn agent_event_ext_keys_sorted() {
    let mut ext = BTreeMap::new();
    ext.insert("z_field".into(), json!(1));
    ext.insert("a_field".into(), json!(2));
    let e = AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunStarted {
            message: "x".into(),
        },
        ext: Some(ext),
    };
    let v: Value = serde_json::to_value(&e).unwrap();
    let ext_keys: Vec<String> = v["ext"].as_object().unwrap().keys().cloned().collect();
    assert_eq!(ext_keys, vec!["a_field", "z_field"]);
}

#[test]
fn canonical_json_empty_object() {
    let cj = canonical_json(&json!({})).unwrap();
    assert_eq!(cj, "{}");
}

#[test]
fn canonical_json_preserves_array_order() {
    let cj = canonical_json(&json!([3, 1, 2])).unwrap();
    assert_eq!(cj, "[3,1,2]");
}

// ═══════════════════════════════════════════════════════════════════════════
// 12  Receipt hash determinism
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_hash_deterministic() {
    let r = minimal_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_is_64_hex_chars() {
    let r = minimal_receipt();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_ignores_existing_hash_field() {
    let mut r1 = minimal_receipt();
    r1.receipt_sha256 = None;
    let mut r2 = minimal_receipt();
    r2.receipt_sha256 = Some("deadbeef".repeat(8));

    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_with_hash_populates_sha256() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_with_hash_matches_receipt_hash() {
    let r = minimal_receipt();
    let expected = receipt_hash(&r).unwrap();
    let r_hashed = r.with_hash().unwrap();
    assert_eq!(r_hashed.receipt_sha256.as_ref().unwrap(), &expected);
}

#[test]
fn receipt_hash_changes_with_different_outcome() {
    let mut r1 = minimal_receipt();
    r1.outcome = Outcome::Complete;
    let mut r2 = minimal_receipt();
    r2.outcome = Outcome::Failed;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_different_backend() {
    let mut r1 = minimal_receipt();
    r1.backend.id = "mock".into();
    let mut r2 = minimal_receipt();
    r2.backend.id = "sidecar:node".into();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_trace_event() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunStarted {
            message: "started".into(),
        },
        ext: None,
    });
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn sha256_hex_deterministic() {
    let h1 = sha256_hex(b"test data");
    let h2 = sha256_hex(b"test data");
    assert_eq!(h1, h2);
}

#[test]
fn sha256_hex_different_input_different_hash() {
    let h1 = sha256_hex(b"input a");
    let h2 = sha256_hex(b"input b");
    assert_ne!(h1, h2);
}

#[test]
fn sha256_hex_empty_input() {
    let h = sha256_hex(b"");
    assert_eq!(h.len(), 64);
}

// ═══════════════════════════════════════════════════════════════════════════
// 13  Empty/null field handling consistency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_sha256_null_when_none() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

#[test]
fn backend_version_null_when_none() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert!(v["backend"]["backend_version"].is_null());
}

#[test]
fn adapter_version_null_when_none() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert!(v["backend"]["adapter_version"].is_null());
}

#[test]
fn usage_normalized_all_none_fields() {
    let u = UsageNormalized::default();
    let v: Value = serde_json::to_value(&u).unwrap();
    assert!(v["input_tokens"].is_null());
    assert!(v["output_tokens"].is_null());
    assert!(v["cache_read_tokens"].is_null());
    assert!(v["cache_write_tokens"].is_null());
    assert!(v["request_units"].is_null());
    assert!(v["estimated_cost_usd"].is_null());
}

#[test]
fn empty_trace_serializes_as_empty_array() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["trace"], json!([]));
}

#[test]
fn empty_artifacts_serializes_as_empty_array() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["artifacts"], json!([]));
}

#[test]
fn empty_capabilities_serializes_as_empty_object() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["capabilities"], json!({}));
}

#[test]
fn empty_policy_lists_serialize_as_empty_arrays() {
    let p = PolicyProfile::default();
    let v: Value = serde_json::to_value(&p).unwrap();
    assert_eq!(v["allowed_tools"], json!([]));
    assert_eq!(v["disallowed_tools"], json!([]));
    assert_eq!(v["deny_read"], json!([]));
    assert_eq!(v["deny_write"], json!([]));
    assert_eq!(v["allow_network"], json!([]));
    assert_eq!(v["deny_network"], json!([]));
    assert_eq!(v["require_approval_for"], json!([]));
}

#[test]
fn empty_context_serializes_as_empty_collections() {
    let ctx = ContextPacket::default();
    let v: Value = serde_json::to_value(&ctx).unwrap();
    assert_eq!(v["files"], json!([]));
    assert_eq!(v["snippets"], json!([]));
}

#[test]
fn empty_runtime_config_vendor_is_empty_object() {
    let rc = RuntimeConfig::default();
    let v: Value = serde_json::to_value(&rc).unwrap();
    assert_eq!(v["vendor"], json!({}));
}

#[test]
fn empty_runtime_config_env_is_empty_object() {
    let rc = RuntimeConfig::default();
    let v: Value = serde_json::to_value(&rc).unwrap();
    assert_eq!(v["env"], json!({}));
}

#[test]
fn runtime_config_model_null_when_none() {
    let rc = RuntimeConfig::default();
    let v: Value = serde_json::to_value(&rc).unwrap();
    assert!(v["model"].is_null());
}

#[test]
fn runtime_config_max_budget_null_when_none() {
    let rc = RuntimeConfig::default();
    let v: Value = serde_json::to_value(&rc).unwrap();
    assert!(v["max_budget_usd"].is_null());
}

#[test]
fn runtime_config_max_turns_null_when_none() {
    let rc = RuntimeConfig::default();
    let v: Value = serde_json::to_value(&rc).unwrap();
    assert!(v["max_turns"].is_null());
}

#[test]
fn verification_report_default_harness_ok_is_false() {
    let vr = VerificationReport::default();
    let v: Value = serde_json::to_value(&vr).unwrap();
    assert_eq!(v["harness_ok"], json!(false));
}

#[test]
fn verification_report_git_diff_null_when_none() {
    let vr = VerificationReport::default();
    let v: Value = serde_json::to_value(&vr).unwrap();
    assert!(v["git_diff"].is_null());
}

#[test]
fn verification_report_git_status_null_when_none() {
    let vr = VerificationReport::default();
    let v: Value = serde_json::to_value(&vr).unwrap();
    assert!(v["git_status"].is_null());
}

// ═══════════════════════════════════════════════════════════════════════════
// 14  Nested type stability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn run_metadata_has_all_expected_keys() {
    let meta = RunMetadata {
        run_id: nil_uuid(),
        work_order_id: nil_uuid(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: fixed_time(),
        finished_at: fixed_time2(),
        duration_ms: 100,
    };
    let v: Value = serde_json::to_value(&meta).unwrap();
    for expected in &[
        "run_id",
        "work_order_id",
        "contract_version",
        "started_at",
        "finished_at",
        "duration_ms",
    ] {
        assert!(v.get(expected).is_some(), "missing: {expected}");
    }
}

#[test]
fn run_metadata_roundtrip() {
    let meta = RunMetadata {
        run_id: nil_uuid(),
        work_order_id: nil_uuid(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: fixed_time(),
        finished_at: fixed_time2(),
        duration_ms: 100,
    };
    let json = serde_json::to_string(&meta).unwrap();
    let meta2: RunMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(meta.run_id, meta2.run_id);
    assert_eq!(meta.duration_ms, meta2.duration_ms);
}

#[test]
fn workspace_spec_roundtrip() {
    let ws = WorkspaceSpec {
        root: "/tmp/ws".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into()],
        exclude: vec!["target/".into()],
    };
    let json = serde_json::to_string(&ws).unwrap();
    let ws2: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(ws.root, ws2.root);
    assert_eq!(ws.include, ws2.include);
    assert_eq!(ws.exclude, ws2.exclude);
}

#[test]
fn context_snippet_has_name_and_content() {
    let s = ContextSnippet {
        name: "readme".into(),
        content: "# Hello".into(),
    };
    let v: Value = serde_json::to_value(&s).unwrap();
    assert_eq!(v["name"].as_str().unwrap(), "readme");
    assert_eq!(v["content"].as_str().unwrap(), "# Hello");
}

#[test]
fn context_snippet_roundtrip() {
    let s = ContextSnippet {
        name: "test".into(),
        content: "data".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let s2: ContextSnippet = serde_json::from_str(&json).unwrap();
    assert_eq!(s.name, s2.name);
    assert_eq!(s.content, s2.content);
}

#[test]
fn artifact_ref_has_kind_and_path() {
    let a = ArtifactRef {
        kind: "patch".into(),
        path: "output.diff".into(),
    };
    let v: Value = serde_json::to_value(&a).unwrap();
    assert_eq!(v["kind"].as_str().unwrap(), "patch");
    assert_eq!(v["path"].as_str().unwrap(), "output.diff");
}

#[test]
fn artifact_ref_roundtrip() {
    let a = ArtifactRef {
        kind: "log".into(),
        path: "run.log".into(),
    };
    let json = serde_json::to_string(&a).unwrap();
    let a2: ArtifactRef = serde_json::from_str(&json).unwrap();
    assert_eq!(a.kind, a2.kind);
    assert_eq!(a.path, a2.path);
}

#[test]
fn usage_normalized_roundtrip_with_values() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(50),
        cache_write_tokens: Some(25),
        request_units: Some(1),
        estimated_cost_usd: Some(0.05),
    };
    let json = serde_json::to_string(&u).unwrap();
    let u2: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(u.input_tokens, u2.input_tokens);
    assert_eq!(u.output_tokens, u2.output_tokens);
    assert_eq!(u.cache_read_tokens, u2.cache_read_tokens);
    assert_eq!(u.cache_write_tokens, u2.cache_write_tokens);
    assert_eq!(u.request_units, u2.request_units);
    assert_eq!(u.estimated_cost_usd, u2.estimated_cost_usd);
}

#[test]
fn capability_requirement_roundtrip() {
    let req = CapabilityRequirement {
        capability: Capability::ToolRead,
        min_support: MinSupport::Native,
    };
    let json = serde_json::to_string(&req).unwrap();
    let req2: CapabilityRequirement = serde_json::from_str(&json).unwrap();
    assert_eq!(req.capability, req2.capability);
}

#[test]
fn capability_requirements_roundtrip() {
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
        ],
    };
    let json = serde_json::to_string(&reqs).unwrap();
    let reqs2: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    assert_eq!(reqs.required.len(), reqs2.required.len());
}

#[test]
fn backend_identity_roundtrip() {
    let bi = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.2.0".into()),
    };
    let json = serde_json::to_string(&bi).unwrap();
    let bi2: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(bi.id, bi2.id);
    assert_eq!(bi.backend_version, bi2.backend_version);
    assert_eq!(bi.adapter_version, bi2.adapter_version);
}

#[test]
fn policy_profile_roundtrip() {
    let pp = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/node_modules/**".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["*.internal".into()],
        require_approval_for: vec!["bash".into()],
    };
    let json = serde_json::to_string(&pp).unwrap();
    let pp2: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(pp.allowed_tools, pp2.allowed_tools);
    assert_eq!(pp.disallowed_tools, pp2.disallowed_tools);
    assert_eq!(pp.deny_read, pp2.deny_read);
    assert_eq!(pp.deny_write, pp2.deny_write);
    assert_eq!(pp.allow_network, pp2.allow_network);
    assert_eq!(pp.deny_network, pp2.deny_network);
    assert_eq!(pp.require_approval_for, pp2.require_approval_for);
}

// ═══════════════════════════════════════════════════════════════════════════
// 15  Enum tag format consistency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn agent_event_kind_uses_type_tag() {
    let e = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(
        v.get("type").is_some(),
        "AgentEventKind must use 'type' tag"
    );
    assert!(v.get("t").is_none(), "AgentEventKind must NOT use 't' tag");
}

#[test]
fn execution_lane_patch_first_snake_case() {
    let v: Value = serde_json::to_value(&ExecutionLane::PatchFirst).unwrap();
    assert_eq!(v.as_str().unwrap(), "patch_first");
}

#[test]
fn execution_lane_workspace_first_snake_case() {
    let v: Value = serde_json::to_value(&ExecutionLane::WorkspaceFirst).unwrap();
    assert_eq!(v.as_str().unwrap(), "workspace_first");
}

#[test]
fn execution_mode_passthrough_snake_case() {
    let v: Value = serde_json::to_value(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(v.as_str().unwrap(), "passthrough");
}

#[test]
fn execution_mode_mapped_snake_case() {
    let v: Value = serde_json::to_value(&ExecutionMode::Mapped).unwrap();
    assert_eq!(v.as_str().unwrap(), "mapped");
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn workspace_mode_pass_through_snake_case() {
    let v: Value = serde_json::to_value(&WorkspaceMode::PassThrough).unwrap();
    assert_eq!(v.as_str().unwrap(), "pass_through");
}

#[test]
fn workspace_mode_staged_snake_case() {
    let v: Value = serde_json::to_value(&WorkspaceMode::Staged).unwrap();
    assert_eq!(v.as_str().unwrap(), "staged");
}

#[test]
fn support_level_native_snake_case() {
    let v: Value = serde_json::to_value(&SupportLevel::Native).unwrap();
    assert_eq!(v.as_str().unwrap(), "native");
}

#[test]
fn support_level_emulated_snake_case() {
    let v: Value = serde_json::to_value(&SupportLevel::Emulated).unwrap();
    assert_eq!(v.as_str().unwrap(), "emulated");
}

#[test]
fn support_level_unsupported_snake_case() {
    let v: Value = serde_json::to_value(&SupportLevel::Unsupported).unwrap();
    assert_eq!(v.as_str().unwrap(), "unsupported");
}

#[test]
fn support_level_restricted_has_reason() {
    let sl = SupportLevel::Restricted {
        reason: "policy".into(),
    };
    let v: Value = serde_json::to_value(&sl).unwrap();
    assert_eq!(v["restricted"]["reason"].as_str().unwrap(), "policy");
}

#[test]
fn min_support_native_snake_case() {
    let v: Value = serde_json::to_value(&MinSupport::Native).unwrap();
    assert_eq!(v.as_str().unwrap(), "native");
}

#[test]
fn min_support_emulated_snake_case() {
    let v: Value = serde_json::to_value(&MinSupport::Emulated).unwrap();
    assert_eq!(v.as_str().unwrap(), "emulated");
}

// ═══════════════════════════════════════════════════════════════════════════
// Builder tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_builder_sets_task() {
    let wo = WorkOrderBuilder::new("my task").build();
    assert_eq!(wo.task, "my task");
}

#[test]
fn work_order_builder_defaults_patch_first() {
    let wo = WorkOrderBuilder::new("task").build();
    let v: Value = serde_json::to_value(&wo.lane).unwrap();
    assert_eq!(v.as_str().unwrap(), "patch_first");
}

#[test]
fn work_order_builder_defaults_staged_workspace() {
    let wo = WorkOrderBuilder::new("task").build();
    let v: Value = serde_json::to_value(&wo.workspace.mode).unwrap();
    assert_eq!(v.as_str().unwrap(), "staged");
}

#[test]
fn work_order_builder_sets_model() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn work_order_builder_sets_max_turns() {
    let wo = WorkOrderBuilder::new("task").max_turns(10).build();
    assert_eq!(wo.config.max_turns, Some(10));
}

#[test]
fn work_order_builder_sets_max_budget() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(5.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

#[test]
fn receipt_builder_sets_backend_id() {
    let r = ReceiptBuilder::new("test-backend").build();
    assert_eq!(r.backend.id, "test-backend");
}

#[test]
fn receipt_builder_defaults_complete() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn receipt_builder_embeds_contract_version() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_builder_sha256_none_before_hash() {
    let r = ReceiptBuilder::new("mock").build();
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn receipt_builder_with_hash_fills_sha256() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// SupportLevel::satisfies tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn native_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn native_satisfies_emulated() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn emulated_does_not_satisfy_native() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn emulated_satisfies_emulated() {
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn unsupported_does_not_satisfy_native() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
}

#[test]
fn unsupported_does_not_satisfy_emulated() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn restricted_does_not_satisfy_native() {
    let r = SupportLevel::Restricted {
        reason: "policy".into(),
    };
    assert!(!r.satisfies(&MinSupport::Native));
}

#[test]
fn restricted_satisfies_emulated() {
    let r = SupportLevel::Restricted {
        reason: "policy".into(),
    };
    assert!(r.satisfies(&MinSupport::Emulated));
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-cutting: byte-level serialization stability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_byte_equal_across_serializations() {
    let r = minimal_receipt();
    let j1 = serde_json::to_string(&r).unwrap();
    let j2 = serde_json::to_string(&r).unwrap();
    assert_eq!(j1.as_bytes(), j2.as_bytes());
}

#[test]
fn work_order_byte_equal_across_serializations() {
    let wo = minimal_work_order();
    let j1 = serde_json::to_string(&wo).unwrap();
    let j2 = serde_json::to_string(&wo).unwrap();
    assert_eq!(j1.as_bytes(), j2.as_bytes());
}

#[test]
fn agent_event_byte_equal_across_serializations() {
    let e = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    let j1 = serde_json::to_string(&e).unwrap();
    let j2 = serde_json::to_string(&e).unwrap();
    assert_eq!(j1.as_bytes(), j2.as_bytes());
}

// ═══════════════════════════════════════════════════════════════════════════
// ToolCall/ToolResult field name stability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_call_has_expected_field_names() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("tu_1".into()),
        parent_tool_use_id: Some("tu_0".into()),
        input: json!({"file": "a.rs"}),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("tool_name").is_some());
    assert!(v.get("tool_use_id").is_some());
    assert!(v.get("parent_tool_use_id").is_some());
    assert!(v.get("input").is_some());
}

#[test]
fn tool_result_has_expected_field_names() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: Some("tu_1".into()),
        output: json!("contents"),
        is_error: false,
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("tool_name").is_some());
    assert!(v.get("tool_use_id").is_some());
    assert!(v.get("output").is_some());
    assert!(v.get("is_error").is_some());
}

#[test]
fn file_changed_has_expected_field_names() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "a.rs".into(),
        summary: "created".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("path").is_some());
    assert!(v.get("summary").is_some());
}

#[test]
fn command_executed_has_expected_field_names() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("command").is_some());
    assert!(v.get("exit_code").is_some());
    assert!(v.get("output_preview").is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// Error variant field names
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_event_has_message_field() {
    let e = make_event(AgentEventKind::Error {
        message: "err".into(),
        error_code: None,
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("message").is_some());
}

#[test]
fn error_event_omits_error_code_when_none() {
    let e = make_event(AgentEventKind::Error {
        message: "err".into(),
        error_code: None,
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("error_code").is_none());
}

#[test]
fn warning_event_has_message_field() {
    let e = make_event(AgentEventKind::Warning {
        message: "warn".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("message").is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// Receipt with populated trace
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_with_trace_events_roundtrips() {
    let mut r = minimal_receipt();
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    });
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    });
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    });
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r2.trace.len(), 3);
}

#[test]
fn receipt_with_artifacts_roundtrips() {
    let mut r = minimal_receipt();
    r.artifacts.push(ArtifactRef {
        kind: "patch".into(),
        path: "a.diff".into(),
    });
    r.artifacts.push(ArtifactRef {
        kind: "log".into(),
        path: "run.log".into(),
    });
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r2.artifacts.len(), 2);
    assert_eq!(r2.artifacts[0].kind, "patch");
}

#[test]
fn receipt_with_capabilities_roundtrips() {
    let mut r = minimal_receipt();
    r.capabilities
        .insert(Capability::ToolRead, SupportLevel::Native);
    r.capabilities
        .insert(Capability::Streaming, SupportLevel::Emulated);
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r2.capabilities.len(), 2);
}

#[test]
fn receipt_with_usage_roundtrips() {
    let mut r = minimal_receipt();
    r.usage = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: Some(0.01),
    };
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r2.usage.input_tokens, Some(1000));
    assert_eq!(r2.usage.output_tokens, Some(500));
}

#[test]
fn receipt_with_verification_roundtrips() {
    let mut r = minimal_receipt();
    r.verification = VerificationReport {
        git_diff: Some("diff --git ...".into()),
        git_status: Some("M src/main.rs".into()),
        harness_ok: true,
    };
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(r2.verification.harness_ok);
    assert!(r2.verification.git_diff.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// WorkOrder with populated fields
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_with_context_roundtrips() {
    let wo = WorkOrderBuilder::new("task")
        .context(ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "readme".into(),
                content: "# Test".into(),
            }],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.context.files.len(), 1);
    assert_eq!(wo2.context.snippets.len(), 1);
}

#[test]
fn work_order_with_policy_roundtrips() {
    let wo = WorkOrderBuilder::new("task")
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec![],
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.policy.allowed_tools, vec!["read"]);
    assert_eq!(wo2.policy.disallowed_tools, vec!["bash"]);
}

#[test]
fn work_order_with_requirements_roundtrips() {
    let wo = WorkOrderBuilder::new("task")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.requirements.required.len(), 1);
}

#[test]
fn work_order_with_runtime_config_roundtrips() {
    let mut config = RuntimeConfig::default();
    config.model = Some("claude-3".into());
    config.max_turns = Some(20);
    config.max_budget_usd = Some(10.0);
    config.vendor.insert("key".into(), json!("val"));
    config.env.insert("TOKEN".into(), "abc".into());
    let wo = WorkOrderBuilder::new("task").config(config).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.model.as_deref(), Some("claude-3"));
    assert_eq!(wo2.config.max_turns, Some(20));
    assert_eq!(wo2.config.vendor["key"], json!("val"));
}
