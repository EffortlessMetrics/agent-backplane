// SPDX-License-Identifier: MIT OR Apache-2.0
//! Serde compatibility and backwards-compatibility tests for abp-core contract types.

use std::collections::BTreeMap;

use abp_core::*;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal valid Receipt for testing.
fn minimal_receipt() -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
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

/// Build a minimal valid WorkOrder for testing.
fn minimal_work_order() -> WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

// ===========================================================================
// 1. Forward compatibility — extra unknown fields are tolerated
// ===========================================================================

#[test]
fn forward_compat_extra_fields_in_runtime_config() {
    let json = json!({
        "model": "gpt-4",
        "vendor": {},
        "env": {},
        "max_budget_usd": null,
        "max_turns": null,
        "future_field": "should be ignored"
    });
    let cfg: RuntimeConfig = serde_json::from_value(json).unwrap();
    assert_eq!(cfg.model.as_deref(), Some("gpt-4"));
}

#[test]
fn forward_compat_extra_fields_in_backend_identity() {
    let json = json!({
        "id": "test",
        "backend_version": null,
        "adapter_version": null,
        "new_field_v2": 42
    });
    let id: BackendIdentity = serde_json::from_value(json).unwrap();
    assert_eq!(id.id, "test");
}

#[test]
fn forward_compat_extra_fields_in_usage_normalized() {
    let json = json!({
        "input_tokens": 100,
        "output_tokens": 200,
        "cache_read_tokens": null,
        "cache_write_tokens": null,
        "request_units": null,
        "estimated_cost_usd": null,
        "total_latency_ms": 999
    });
    let usage: UsageNormalized = serde_json::from_value(json).unwrap();
    assert_eq!(usage.input_tokens, Some(100));
}

#[test]
fn forward_compat_extra_fields_in_verification_report() {
    let json = json!({
        "git_diff": null,
        "git_status": null,
        "harness_ok": true,
        "lint_ok": true
    });
    let vr: VerificationReport = serde_json::from_value(json).unwrap();
    assert!(vr.harness_ok);
}

// ===========================================================================
// 2. Missing optional fields
// ===========================================================================

#[test]
fn missing_optional_fields_runtime_config() {
    // All Option<T> fields absent (not null, truly absent).
    let json = json!({
        "vendor": {},
        "env": {}
    });
    let cfg: RuntimeConfig = serde_json::from_value(json).unwrap();
    assert!(cfg.model.is_none());
    assert!(cfg.max_budget_usd.is_none());
    assert!(cfg.max_turns.is_none());
}

#[test]
fn missing_optional_fields_backend_identity() {
    let json = json!({ "id": "test" });
    let id: BackendIdentity = serde_json::from_value(json).unwrap();
    assert!(id.backend_version.is_none());
    assert!(id.adapter_version.is_none());
}

#[test]
fn missing_optional_fields_usage_normalized() {
    // Completely empty object — all fields are Option<T>.
    let json = json!({});
    let usage: UsageNormalized = serde_json::from_value(json).unwrap();
    assert!(usage.input_tokens.is_none());
    assert!(usage.estimated_cost_usd.is_none());
}

#[test]
fn missing_optional_fields_verification_report() {
    let json = json!({ "harness_ok": false });
    let vr: VerificationReport = serde_json::from_value(json).unwrap();
    assert!(vr.git_diff.is_none());
    assert!(vr.git_status.is_none());
}

// ===========================================================================
// 3. Enum variant strings — snake_case serialization
// ===========================================================================

#[test]
fn enum_execution_lane_variants() {
    assert_eq!(json!(ExecutionLane::PatchFirst), json!("patch_first"));
    assert_eq!(
        json!(ExecutionLane::WorkspaceFirst),
        json!("workspace_first")
    );
}

#[test]
fn enum_workspace_mode_variants() {
    assert_eq!(json!(WorkspaceMode::PassThrough), json!("pass_through"));
    assert_eq!(json!(WorkspaceMode::Staged), json!("staged"));
}

#[test]
fn enum_execution_mode_variants() {
    assert_eq!(json!(ExecutionMode::Passthrough), json!("passthrough"));
    assert_eq!(json!(ExecutionMode::Mapped), json!("mapped"));
}

#[test]
fn enum_outcome_variants() {
    assert_eq!(json!(Outcome::Complete), json!("complete"));
    assert_eq!(json!(Outcome::Partial), json!("partial"));
    assert_eq!(json!(Outcome::Failed), json!("failed"));
}

#[test]
fn enum_min_support_variants() {
    assert_eq!(json!(MinSupport::Native), json!("native"));
    assert_eq!(json!(MinSupport::Emulated), json!("emulated"));
}

#[test]
fn enum_support_level_variants() {
    assert_eq!(json!(SupportLevel::Native), json!("native"));
    assert_eq!(json!(SupportLevel::Emulated), json!("emulated"));
    assert_eq!(json!(SupportLevel::Unsupported), json!("unsupported"));

    let restricted = SupportLevel::Restricted {
        reason: "test".into(),
    };
    // Externally tagged: {"restricted": {"reason": "test"}}
    let v = serde_json::to_value(&restricted).unwrap();
    assert_eq!(v["restricted"]["reason"], "test");
}

#[test]
fn enum_capability_all_snake_case() {
    let capabilities = vec![
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
    for (variant, expected) in capabilities {
        assert_eq!(
            json!(variant),
            json!(expected),
            "Capability::{variant:?} should serialize to \"{expected}\""
        );
    }
}

#[test]
fn enum_agent_event_kind_tag_is_type() {
    let kind = AgentEventKind::RunStarted {
        message: "go".into(),
    };
    let v = serde_json::to_value(&kind).unwrap();
    assert_eq!(
        v["type"], "run_started",
        "AgentEventKind tag field must be \"type\""
    );
    assert_eq!(v["message"], "go");
}

#[test]
fn enum_agent_event_kind_all_variants() {
    let variants: Vec<(AgentEventKind, &str)> = vec![
        (
            AgentEventKind::RunStarted { message: "".into() },
            "run_started",
        ),
        (
            AgentEventKind::RunCompleted { message: "".into() },
            "run_completed",
        ),
        (
            AgentEventKind::AssistantDelta { text: "".into() },
            "assistant_delta",
        ),
        (
            AgentEventKind::AssistantMessage { text: "".into() },
            "assistant_message",
        ),
        (
            AgentEventKind::ToolCall {
                tool_name: "t".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({}),
            },
            "tool_call",
        ),
        (
            AgentEventKind::ToolResult {
                tool_name: "t".into(),
                tool_use_id: None,
                output: json!({}),
                is_error: false,
            },
            "tool_result",
        ),
        (
            AgentEventKind::FileChanged {
                path: "f".into(),
                summary: "s".into(),
            },
            "file_changed",
        ),
        (
            AgentEventKind::CommandExecuted {
                command: "c".into(),
                exit_code: None,
                output_preview: None,
            },
            "command_executed",
        ),
        (AgentEventKind::Warning { message: "".into() }, "warning"),
        (AgentEventKind::Error { message: "".into() }, "error"),
    ];
    for (variant, expected_tag) in variants {
        let v = serde_json::to_value(&variant).unwrap();
        assert_eq!(
            v["type"], expected_tag,
            "AgentEventKind variant should have type=\"{expected_tag}\""
        );
    }
}

// ===========================================================================
// 4. BTreeMap ordering — deterministic serialization
// ===========================================================================

#[test]
fn btreemap_config_vendor_keys_sorted() {
    let mut cfg = RuntimeConfig::default();
    cfg.vendor.insert("zebra".into(), json!(1));
    cfg.vendor.insert("alpha".into(), json!(2));
    cfg.vendor.insert("middle".into(), json!(3));

    let s = serde_json::to_string(&cfg).unwrap();
    let alpha_pos = s.find("\"alpha\"").unwrap();
    let middle_pos = s.find("\"middle\"").unwrap();
    let zebra_pos = s.find("\"zebra\"").unwrap();
    assert!(
        alpha_pos < middle_pos && middle_pos < zebra_pos,
        "BTreeMap keys must appear in alphabetical order: {s}"
    );
}

#[test]
fn btreemap_env_keys_sorted() {
    let mut cfg = RuntimeConfig::default();
    cfg.env.insert("Z_VAR".into(), "z".into());
    cfg.env.insert("A_VAR".into(), "a".into());
    cfg.env.insert("M_VAR".into(), "m".into());

    let s = serde_json::to_string(&cfg).unwrap();
    let a_pos = s.find("\"A_VAR\"").unwrap();
    let m_pos = s.find("\"M_VAR\"").unwrap();
    let z_pos = s.find("\"Z_VAR\"").unwrap();
    assert!(
        a_pos < m_pos && m_pos < z_pos,
        "BTreeMap keys must appear in alphabetical order: {s}"
    );
}

#[test]
fn btreemap_capability_manifest_deterministic() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolWrite, SupportLevel::Native);
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::McpClient, SupportLevel::Emulated);

    // BTreeMap<Capability, _> is ordered by Capability's derived Ord
    // (discriminant order), which is deterministic across serializations.
    let s1 = serde_json::to_string(&manifest).unwrap();
    let s2 = serde_json::to_string(&manifest).unwrap();
    assert_eq!(
        s1, s2,
        "CapabilityManifest serialization must be deterministic"
    );

    // Verify the order matches Capability discriminant order:
    // Streaming (0) < ToolWrite (2) < McpClient (16)
    let streaming_pos = s1.find("streaming").unwrap();
    let tool_write_pos = s1.find("tool_write").unwrap();
    let mcp_pos = s1.find("mcp_client").unwrap();
    assert!(
        streaming_pos < tool_write_pos && tool_write_pos < mcp_pos,
        "CapabilityManifest keys must appear in Capability Ord order: {s1}"
    );
}

// ===========================================================================
// 5. Null vs absent — identical behaviour for Option<T>
// ===========================================================================

#[test]
fn null_vs_absent_runtime_config_model() {
    let with_null: RuntimeConfig = serde_json::from_value(json!({
        "model": null,
        "vendor": {},
        "env": {}
    }))
    .unwrap();
    let without: RuntimeConfig = serde_json::from_value(json!({
        "vendor": {},
        "env": {}
    }))
    .unwrap();
    assert_eq!(with_null.model, without.model);
    assert!(with_null.model.is_none());
}

#[test]
fn null_vs_absent_usage_normalized() {
    let with_nulls: UsageNormalized = serde_json::from_value(json!({
        "input_tokens": null,
        "output_tokens": null,
        "cache_read_tokens": null,
        "cache_write_tokens": null,
        "request_units": null,
        "estimated_cost_usd": null
    }))
    .unwrap();
    let without: UsageNormalized = serde_json::from_value(json!({})).unwrap();

    assert_eq!(with_nulls.input_tokens, without.input_tokens);
    assert_eq!(with_nulls.output_tokens, without.output_tokens);
    assert_eq!(with_nulls.estimated_cost_usd, without.estimated_cost_usd);
}

#[test]
fn null_vs_absent_backend_identity() {
    let with_null: BackendIdentity = serde_json::from_value(json!({
        "id": "x",
        "backend_version": null,
        "adapter_version": null
    }))
    .unwrap();
    let without: BackendIdentity = serde_json::from_value(json!({ "id": "x" })).unwrap();
    assert_eq!(with_null.backend_version, without.backend_version);
    assert_eq!(with_null.adapter_version, without.adapter_version);
}

#[test]
fn null_vs_absent_agent_event_ext() {
    let now = Utc::now();
    let with_null: AgentEvent = serde_json::from_value(json!({
        "ts": now.to_rfc3339(),
        "type": "run_started",
        "message": "go",
        "ext": null
    }))
    .unwrap();
    let without: AgentEvent = serde_json::from_value(json!({
        "ts": now.to_rfc3339(),
        "type": "run_started",
        "message": "go"
    }))
    .unwrap();
    assert_eq!(with_null.ext, without.ext);
}

// ===========================================================================
// 6. Default values — Default::default() produces valid JSON
// ===========================================================================

#[test]
fn default_runtime_config_roundtrips() {
    let cfg = RuntimeConfig::default();
    let json = serde_json::to_value(&cfg).unwrap();
    let _back: RuntimeConfig = serde_json::from_value(json).unwrap();
}

#[test]
fn default_policy_profile_roundtrips() {
    let policy = PolicyProfile::default();
    let json = serde_json::to_value(&policy).unwrap();
    let _back: PolicyProfile = serde_json::from_value(json).unwrap();
}

#[test]
fn default_usage_normalized_roundtrips() {
    let usage = UsageNormalized::default();
    let json = serde_json::to_value(&usage).unwrap();
    let _back: UsageNormalized = serde_json::from_value(json).unwrap();
}

#[test]
fn default_context_packet_roundtrips() {
    let ctx = ContextPacket::default();
    let json = serde_json::to_value(&ctx).unwrap();
    let _back: ContextPacket = serde_json::from_value(json).unwrap();
}

#[test]
fn default_verification_report_roundtrips() {
    let vr = VerificationReport::default();
    let json = serde_json::to_value(&vr).unwrap();
    let _back: VerificationReport = serde_json::from_value(json).unwrap();
}

#[test]
fn default_capability_requirements_roundtrips() {
    let reqs = CapabilityRequirements::default();
    let json = serde_json::to_value(&reqs).unwrap();
    let _back: CapabilityRequirements = serde_json::from_value(json).unwrap();
}

#[test]
fn default_execution_mode_is_mapped() {
    let mode = ExecutionMode::default();
    assert_eq!(json!(mode), json!("mapped"));
}

#[test]
fn receipt_roundtrips() {
    let receipt = minimal_receipt();
    let json = serde_json::to_value(&receipt).unwrap();
    let _back: Receipt = serde_json::from_value(json).unwrap();
}

#[test]
fn work_order_roundtrips() {
    let wo = minimal_work_order();
    let json = serde_json::to_value(&wo).unwrap();
    let _back: WorkOrder = serde_json::from_value(json).unwrap();
}

// ===========================================================================
// 7. Cross-version simulation — v0.1-era JSON fixtures
// ===========================================================================

/// A receipt as a v0.1-era consumer might have serialized it.
#[test]
fn cross_version_v01_receipt_fixture() {
    let fixture = json!({
        "meta": {
            "run_id": "00000000-0000-0000-0000-000000000000",
            "work_order_id": "00000000-0000-0000-0000-000000000000",
            "contract_version": "abp/v0.1",
            "started_at": "2025-01-01T00:00:00Z",
            "finished_at": "2025-01-01T00:01:00Z",
            "duration_ms": 60000
        },
        "backend": {
            "id": "sidecar:node",
            "backend_version": "1.0.0",
            "adapter_version": null
        },
        "capabilities": {
            "streaming": "native",
            "tool_read": "native",
            "tool_write": "emulated"
        },
        "mode": "mapped",
        "usage_raw": { "prompt_tokens": 100, "completion_tokens": 50 },
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_read_tokens": null,
            "cache_write_tokens": null,
            "request_units": null,
            "estimated_cost_usd": 0.01
        },
        "trace": [
            {
                "ts": "2025-01-01T00:00:01Z",
                "type": "run_started",
                "message": "Starting task"
            },
            {
                "ts": "2025-01-01T00:00:30Z",
                "type": "tool_call",
                "tool_name": "read_file",
                "tool_use_id": "tu_1",
                "parent_tool_use_id": null,
                "input": { "path": "src/main.rs" }
            },
            {
                "ts": "2025-01-01T00:00:59Z",
                "type": "run_completed",
                "message": "Done"
            }
        ],
        "artifacts": [
            { "kind": "patch", "path": "output.diff" }
        ],
        "verification": {
            "git_diff": "+added line",
            "git_status": "M src/main.rs",
            "harness_ok": true
        },
        "outcome": "complete",
        "receipt_sha256": null
    });

    let receipt: Receipt = serde_json::from_value(fixture).unwrap();
    assert_eq!(receipt.meta.contract_version, "abp/v0.1");
    assert_eq!(receipt.backend.id, "sidecar:node");
    assert_eq!(receipt.trace.len(), 3);
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.usage.input_tokens, Some(100));
}

/// A work order as a v0.1-era consumer might have serialized it.
#[test]
fn cross_version_v01_work_order_fixture() {
    let fixture = json!({
        "id": "00000000-0000-0000-0000-000000000001",
        "task": "Fix the login bug",
        "lane": "workspace_first",
        "workspace": {
            "root": "/tmp/ws",
            "mode": "staged",
            "include": ["src/**"],
            "exclude": ["target/**"]
        },
        "context": {
            "files": ["README.md"],
            "snippets": [
                { "name": "error_log", "content": "panic at line 42" }
            ]
        },
        "policy": {
            "allowed_tools": ["read_file", "write_file"],
            "disallowed_tools": [],
            "deny_read": ["**/.env"],
            "deny_write": ["Cargo.lock"],
            "allow_network": [],
            "deny_network": [],
            "require_approval_for": ["bash"]
        },
        "requirements": {
            "required": [
                { "capability": "tool_read", "min_support": "native" },
                { "capability": "tool_write", "min_support": "emulated" }
            ]
        },
        "config": {
            "model": "gpt-4",
            "vendor": { "openai": { "temperature": 0.7 } },
            "env": { "RUST_LOG": "debug" },
            "max_budget_usd": 1.50,
            "max_turns": 20
        }
    });

    let wo: WorkOrder = serde_json::from_value(fixture).unwrap();
    assert_eq!(wo.task, "Fix the login bug");
    assert_eq!(json!(wo.lane), json!("workspace_first"));
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(20));
    assert_eq!(wo.requirements.required.len(), 2);
}

/// A v0.1 receipt without the `mode` field (added later with `#[serde(default)]`).
#[test]
fn cross_version_receipt_missing_mode_defaults() {
    let fixture = json!({
        "meta": {
            "run_id": "00000000-0000-0000-0000-000000000000",
            "work_order_id": "00000000-0000-0000-0000-000000000000",
            "contract_version": "abp/v0.1",
            "started_at": "2025-01-01T00:00:00Z",
            "finished_at": "2025-01-01T00:00:01Z",
            "duration_ms": 1000
        },
        "backend": { "id": "mock" },
        "capabilities": {},
        "usage_raw": {},
        "usage": {},
        "trace": [],
        "artifacts": [],
        "verification": { "harness_ok": false },
        "outcome": "failed",
        "receipt_sha256": null
    });

    let receipt: Receipt = serde_json::from_value(fixture).unwrap();
    // mode should default to Mapped when absent.
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

/// AgentEvent with extension data (passthrough mode).
#[test]
fn cross_version_agent_event_with_ext() {
    let fixture = json!({
        "ts": "2025-01-01T00:00:00Z",
        "type": "assistant_message",
        "text": "Hello!",
        "ext": {
            "raw_message": { "role": "assistant", "content": "Hello!" }
        }
    });

    let event: AgentEvent = serde_json::from_value(fixture).unwrap();
    assert!(event.ext.is_some());
    let ext = event.ext.unwrap();
    assert!(ext.contains_key("raw_message"));
}

// ===========================================================================
// Additional: ext field skip_serializing_if behaviour
// ===========================================================================

#[test]
fn agent_event_ext_none_omitted_in_serialization() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "test".into(),
        },
        ext: None,
    };
    let v = serde_json::to_value(&event).unwrap();
    // ext: None should be omitted entirely (skip_serializing_if).
    assert!(
        !v.as_object().unwrap().contains_key("ext"),
        "ext=None should be omitted from JSON"
    );
}

#[test]
fn agent_event_ext_some_included_in_serialization() {
    let mut ext = BTreeMap::new();
    ext.insert("key".into(), json!("value"));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "test".into(),
        },
        ext: Some(ext),
    };
    let v = serde_json::to_value(&event).unwrap();
    assert!(v.as_object().unwrap().contains_key("ext"));
}
