// SPDX-License-Identifier: MIT OR Apache-2.0

//! Contract evolution tests — ensure the v0.1 wire format never accidentally breaks.
//!
//! Every fixture and assertion here represents a "blessed" serialisation shape.
//! If a refactor causes any of these tests to fail the change is a **breaking**
//! wire-format change and must be handled deliberately.

use abp_core::*;
use serde_json::json;
use std::collections::BTreeMap;

// ── Fixtures ────────────────────────────────────────────────────────────────

const WORK_ORDER_JSON: &str = r#"{
  "id": "00000000-0000-0000-0000-000000000001",
  "task": "Fix the login bug",
  "lane": "patch_first",
  "workspace": {
    "root": "/tmp/ws",
    "mode": "staged",
    "include": ["src/**"],
    "exclude": [".git"]
  },
  "context": {
    "files": ["src/main.rs"],
    "snippets": []
  },
  "policy": {
    "allowed_tools": ["read", "write"],
    "disallowed_tools": [],
    "deny_read": [],
    "deny_write": [],
    "allow_network": [],
    "deny_network": [],
    "require_approval_for": []
  },
  "requirements": {
    "required": []
  },
  "config": {
    "model": "gpt-4",
    "vendor": {},
    "env": {},
    "max_budget_usd": 1.5,
    "max_turns": 10
  }
}"#;

const RECEIPT_JSON: &str = r#"{
  "meta": {
    "run_id": "00000000-0000-0000-0000-000000000002",
    "work_order_id": "00000000-0000-0000-0000-000000000001",
    "contract_version": "abp/v0.1",
    "started_at": "2025-01-01T00:00:00Z",
    "finished_at": "2025-01-01T00:01:00Z",
    "duration_ms": 60000
  },
  "backend": {
    "id": "mock",
    "backend_version": "1.0",
    "adapter_version": null
  },
  "capabilities": {
    "streaming": "native",
    "tool_read": "emulated"
  },
  "mode": "mapped",
  "usage_raw": {"tokens": 100},
  "usage": {
    "input_tokens": 50,
    "output_tokens": 50,
    "cache_read_tokens": null,
    "cache_write_tokens": null,
    "request_units": null,
    "estimated_cost_usd": null
  },
  "trace": [],
  "artifacts": [],
  "verification": {
    "git_diff": null,
    "git_status": null,
    "harness_ok": true
  },
  "outcome": "complete",
  "receipt_sha256": null
}"#;

const EVENT_RUN_STARTED: &str = r#"{
  "ts": "2025-01-01T00:00:00Z",
  "type": "run_started",
  "message": "Starting run"
}"#;

const EVENT_RUN_COMPLETED: &str = r#"{
  "ts": "2025-01-01T00:00:01Z",
  "type": "run_completed",
  "message": "Done"
}"#;

const EVENT_ASSISTANT_DELTA: &str = r#"{
  "ts": "2025-01-01T00:00:00Z",
  "type": "assistant_delta",
  "text": "Hello"
}"#;

const EVENT_ASSISTANT_MESSAGE: &str = r#"{
  "ts": "2025-01-01T00:00:00Z",
  "type": "assistant_message",
  "text": "Hello, world!"
}"#;

const EVENT_TOOL_CALL: &str = r#"{
  "ts": "2025-01-01T00:00:00Z",
  "type": "tool_call",
  "tool_name": "read",
  "tool_use_id": "t1",
  "parent_tool_use_id": null,
  "input": {"path": "file.rs"}
}"#;

const EVENT_TOOL_RESULT: &str = r#"{
  "ts": "2025-01-01T00:00:00Z",
  "type": "tool_result",
  "tool_name": "read",
  "tool_use_id": "t1",
  "output": "contents",
  "is_error": false
}"#;

const EVENT_FILE_CHANGED: &str = r#"{
  "ts": "2025-01-01T00:00:00Z",
  "type": "file_changed",
  "path": "src/main.rs",
  "summary": "Added error handling"
}"#;

const EVENT_COMMAND_EXECUTED: &str = r#"{
  "ts": "2025-01-01T00:00:00Z",
  "type": "command_executed",
  "command": "cargo test",
  "exit_code": 0,
  "output_preview": "ok"
}"#;

const EVENT_WARNING: &str = r#"{
  "ts": "2025-01-01T00:00:00Z",
  "type": "warning",
  "message": "Rate limit approaching"
}"#;

const EVENT_ERROR: &str = r#"{
  "ts": "2025-01-01T00:00:00Z",
  "type": "error",
  "message": "Connection lost"
}"#;

// ── CONTRACT_VERSION ────────────────────────────────────────────────────────

#[test]
fn contract_version_is_v0_1() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

// ── Fixture round-trip tests ────────────────────────────────────────────────

#[test]
fn work_order_fixture_deserializes() {
    let wo: WorkOrder =
        serde_json::from_str(WORK_ORDER_JSON).expect("WorkOrder fixture must deserialize");
    assert_eq!(wo.task, "Fix the login bug");
    assert_eq!(wo.id.to_string(), "00000000-0000-0000-0000-000000000001");

    // Round-trip: serialize then deserialize again.
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.id, wo2.id);
    assert_eq!(wo.task, wo2.task);
}

#[test]
fn receipt_fixture_deserializes() {
    let r: Receipt = serde_json::from_str(RECEIPT_JSON).expect("Receipt fixture must deserialize");
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(r.meta.contract_version, "abp/v0.1");

    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.outcome, r2.outcome);
}

#[test]
fn agent_event_run_started_deserializes() {
    let e: AgentEvent = serde_json::from_str(EVENT_RUN_STARTED).unwrap();
    assert!(matches!(e.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn agent_event_run_completed_deserializes() {
    let e: AgentEvent = serde_json::from_str(EVENT_RUN_COMPLETED).unwrap();
    assert!(matches!(e.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn agent_event_assistant_delta_deserializes() {
    let e: AgentEvent = serde_json::from_str(EVENT_ASSISTANT_DELTA).unwrap();
    assert!(matches!(e.kind, AgentEventKind::AssistantDelta { .. }));
}

#[test]
fn agent_event_assistant_message_deserializes() {
    let e: AgentEvent = serde_json::from_str(EVENT_ASSISTANT_MESSAGE).unwrap();
    assert!(matches!(e.kind, AgentEventKind::AssistantMessage { .. }));
}

#[test]
fn agent_event_tool_call_deserializes() {
    let e: AgentEvent = serde_json::from_str(EVENT_TOOL_CALL).unwrap();
    assert!(matches!(e.kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn agent_event_tool_result_deserializes() {
    let e: AgentEvent = serde_json::from_str(EVENT_TOOL_RESULT).unwrap();
    assert!(matches!(e.kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn agent_event_file_changed_deserializes() {
    let e: AgentEvent = serde_json::from_str(EVENT_FILE_CHANGED).unwrap();
    assert!(matches!(e.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn agent_event_command_executed_deserializes() {
    let e: AgentEvent = serde_json::from_str(EVENT_COMMAND_EXECUTED).unwrap();
    assert!(matches!(e.kind, AgentEventKind::CommandExecuted { .. }));
}

#[test]
fn agent_event_warning_deserializes() {
    let e: AgentEvent = serde_json::from_str(EVENT_WARNING).unwrap();
    assert!(matches!(e.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn agent_event_error_deserializes() {
    let e: AgentEvent = serde_json::from_str(EVENT_ERROR).unwrap();
    assert!(matches!(e.kind, AgentEventKind::Error { .. }));
}

// ── Field presence tests ────────────────────────────────────────────────────

#[test]
fn work_order_has_required_fields() {
    let v: serde_json::Value = serde_json::from_str(WORK_ORDER_JSON).unwrap();
    let obj = v.as_object().expect("WorkOrder must be an object");

    for field in [
        "id",
        "task",
        "lane",
        "workspace",
        "context",
        "policy",
        "requirements",
        "config",
    ] {
        assert!(obj.contains_key(field), "WorkOrder missing field `{field}`");
    }
}

#[test]
fn receipt_has_required_fields() {
    let v: serde_json::Value = serde_json::from_str(RECEIPT_JSON).unwrap();
    let obj = v.as_object().expect("Receipt must be an object");

    for field in [
        "meta",
        "backend",
        "capabilities",
        "trace",
        "artifacts",
        "verification",
        "outcome",
    ] {
        assert!(obj.contains_key(field), "Receipt missing field `{field}`");
    }
}

// ── Enum value stability ────────────────────────────────────────────────────

fn enum_to_string<T: serde::Serialize>(value: &T) -> String {
    let v = serde_json::to_value(value).unwrap();
    match v {
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    }
}

#[test]
fn execution_lane_serializes_stably() {
    assert_eq!(enum_to_string(&ExecutionLane::PatchFirst), "patch_first");
    assert_eq!(
        enum_to_string(&ExecutionLane::WorkspaceFirst),
        "workspace_first"
    );
}

#[test]
fn outcome_serializes_stably() {
    assert_eq!(enum_to_string(&Outcome::Complete), "complete");
    assert_eq!(enum_to_string(&Outcome::Partial), "partial");
    assert_eq!(enum_to_string(&Outcome::Failed), "failed");
}

#[test]
fn execution_mode_serializes_stably() {
    assert_eq!(enum_to_string(&ExecutionMode::Passthrough), "passthrough");
    assert_eq!(enum_to_string(&ExecutionMode::Mapped), "mapped");
}

#[test]
fn workspace_mode_serializes_stably() {
    assert_eq!(enum_to_string(&WorkspaceMode::PassThrough), "pass_through");
    assert_eq!(enum_to_string(&WorkspaceMode::Staged), "staged");
}

#[test]
fn min_support_serializes_stably() {
    assert_eq!(enum_to_string(&MinSupport::Native), "native");
    assert_eq!(enum_to_string(&MinSupport::Emulated), "emulated");
}

#[test]
fn support_level_serializes_stably() {
    assert_eq!(enum_to_string(&SupportLevel::Native), "native");
    assert_eq!(enum_to_string(&SupportLevel::Emulated), "emulated");
    assert_eq!(enum_to_string(&SupportLevel::Unsupported), "unsupported");

    // Restricted is externally tagged with data.
    let restricted = SupportLevel::Restricted {
        reason: "beta".into(),
    };
    let v = serde_json::to_value(&restricted).unwrap();
    assert_eq!(v, json!({"restricted": {"reason": "beta"}}));
}

#[test]
fn capability_serializes_stably() {
    let cases: Vec<(Capability, &str)> = vec![
        (Capability::Streaming, "streaming"),
        (Capability::ToolRead, "tool_read"),
        (Capability::ToolWrite, "tool_write"),
        (Capability::ToolEdit, "tool_edit"),
        (Capability::ToolBash, "tool_bash"),
        (Capability::ToolGlob, "tool_glob"),
        (Capability::ToolGrep, "tool_grep"),
        (Capability::ToolWebSearch, "tool_web_search"),
        (Capability::ToolWebFetch, "tool_web_fetch"),
        (Capability::ToolAskUser, "tool_ask_user"),
        (Capability::HooksPreToolUse, "hooks_pre_tool_use"),
        (Capability::HooksPostToolUse, "hooks_post_tool_use"),
        (Capability::SessionResume, "session_resume"),
        (Capability::SessionFork, "session_fork"),
        (Capability::Checkpointing, "checkpointing"),
        (
            Capability::StructuredOutputJsonSchema,
            "structured_output_json_schema",
        ),
        (Capability::McpClient, "mcp_client"),
        (Capability::McpServer, "mcp_server"),
    ];
    for (cap, expected) in cases {
        assert_eq!(
            enum_to_string(&cap),
            expected,
            "Capability::{cap:?} serialized incorrectly"
        );
    }
}

#[test]
fn agent_event_kind_discriminator_is_type() {
    // The internally-tagged enum must use "type" as the discriminator key.
    let event = AgentEvent {
        ts: chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
            .unwrap()
            .into(),
        kind: AgentEventKind::RunStarted {
            message: "hi".into(),
        },
        ext: None,
    };
    let v = serde_json::to_value(&event).unwrap();
    let obj = v.as_object().unwrap();
    assert!(
        obj.contains_key("type"),
        "AgentEvent must have a `type` discriminator field"
    );
    assert_eq!(obj["type"], "run_started");
}

// ── Capability map key ordering ─────────────────────────────────────────────

#[test]
fn capability_map_round_trips_with_btreemap() {
    let mut caps: BTreeMap<Capability, SupportLevel> = BTreeMap::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    let json = serde_json::to_string(&caps).unwrap();
    let reparsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    // Verify round-trip produces identical JSON.
    let original: serde_json::Value = serde_json::to_value(&caps).unwrap();
    assert_eq!(original, reparsed);

    // BTreeMap ordering: keys must be sorted.
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "Capability map keys must be sorted");
}

// ── Receipt mode defaults ───────────────────────────────────────────────────

#[test]
fn receipt_without_mode_defaults_to_mapped() {
    // Older v0.1 payloads may omit `mode`; it should default to `mapped`.
    let mut v: serde_json::Value = serde_json::from_str(RECEIPT_JSON).unwrap();
    v.as_object_mut().unwrap().remove("mode");
    let r: Receipt = serde_json::from_value(v).expect("Receipt must accept missing `mode`");
    assert_eq!(enum_to_string(&r.mode), "mapped");
}
