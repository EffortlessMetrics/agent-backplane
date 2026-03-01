// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive serde roundtrip tests for every public type in abp-core.

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use uuid::Uuid;

use abp_core::*;

// ── helpers ────────────────────────────────────────────────────────────

/// Serialize → Value → Deserialize → re-serialize → compare Values.
/// Works for types that lack `PartialEq`.
fn roundtrip_json<T: Serialize + DeserializeOwned>(val: &T) {
    let v1 = serde_json::to_value(val).expect("serialize");
    let back: T = serde_json::from_value(v1.clone()).expect("deserialize");
    let v2 = serde_json::to_value(&back).expect("re-serialize");
    assert_eq!(v1, v2);
}

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap()
}

fn fixed_uuid() -> Uuid {
    Uuid::nil()
}

// ── struct roundtrips ──────────────────────────────────────────────────

#[test]
fn roundtrip_work_order() {
    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "Fix the login bug".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "look at line 42".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read".into()],
            disallowed_tools: vec!["rm".into()],
            deny_read: vec!["/etc/shadow".into()],
            deny_write: vec!["/usr/**".into()],
            allow_network: vec!["github.com".into()],
            deny_network: vec!["evil.com".into()],
            require_approval_for: vec!["bash".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        },
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor: {
                let mut m = BTreeMap::new();
                m.insert("temperature".into(), json!(0.7));
                m
            },
            env: {
                let mut m = BTreeMap::new();
                m.insert("FOO".into(), "bar".into());
                m
            },
            max_budget_usd: Some(1.5),
            max_turns: Some(10),
        },
    };
    roundtrip_json(&wo);
}

#[test]
fn roundtrip_workspace_spec() {
    let ws = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec!["node_modules".into()],
    };
    roundtrip_json(&ws);
}

#[test]
fn roundtrip_context_packet() {
    let cp = ContextPacket {
        files: vec!["a.rs".into(), "b.rs".into()],
        snippets: vec![ContextSnippet {
            name: "n".into(),
            content: "c".into(),
        }],
    };
    roundtrip_json(&cp);
}

#[test]
fn roundtrip_context_snippet() {
    let cs = ContextSnippet {
        name: "readme".into(),
        content: "# Hello".into(),
    };
    roundtrip_json(&cs);
}

#[test]
fn roundtrip_runtime_config() {
    let rc = RuntimeConfig {
        model: Some("claude-3".into()),
        vendor: BTreeMap::new(),
        env: BTreeMap::new(),
        max_budget_usd: Some(5.0),
        max_turns: Some(20),
    };
    roundtrip_json(&rc);
}

#[test]
fn roundtrip_policy_profile() {
    let pp = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec![],
        deny_read: vec![],
        deny_write: vec!["*.secret".into()],
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    };
    roundtrip_json(&pp);
}

#[test]
fn roundtrip_capability_requirements() {
    let cr = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    roundtrip_json(&cr);
}

#[test]
fn roundtrip_capability_requirement() {
    let cr = CapabilityRequirement {
        capability: Capability::ToolBash,
        min_support: MinSupport::Emulated,
    };
    roundtrip_json(&cr);
}

#[test]
fn roundtrip_backend_identity() {
    let bi = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("1.2.3".into()),
        adapter_version: Some("0.1.0".into()),
    };
    roundtrip_json(&bi);
}

#[test]
fn roundtrip_run_metadata() {
    let rm = RunMetadata {
        run_id: fixed_uuid(),
        work_order_id: fixed_uuid(),
        contract_version: CONTRACT_VERSION.into(),
        started_at: fixed_ts(),
        finished_at: fixed_ts(),
        duration_ms: 123,
    };
    roundtrip_json(&rm);
}

#[test]
fn roundtrip_usage_normalized() {
    let u = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: Some(200),
        cache_write_tokens: Some(100),
        request_units: Some(1),
        estimated_cost_usd: Some(0.03),
    };
    roundtrip_json(&u);
}

#[test]
fn roundtrip_artifact_ref() {
    let ar = ArtifactRef {
        kind: "patch".into(),
        path: "output.diff".into(),
    };
    roundtrip_json(&ar);
}

#[test]
fn roundtrip_verification_report() {
    let vr = VerificationReport {
        git_diff: Some("diff --git a/f b/f\n".into()),
        git_status: Some("M f\n".into()),
        harness_ok: true,
    };
    roundtrip_json(&vr);
}

#[test]
fn roundtrip_agent_event() {
    let ev = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: Some({
            let mut m = BTreeMap::new();
            m.insert("raw_message".into(), json!({"role": "assistant"}));
            m
        }),
    };
    roundtrip_json(&ev);
}

#[test]
fn roundtrip_receipt_full() {
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts(),
            duration_ms: 42,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        capabilities: {
            let mut m = BTreeMap::new();
            m.insert(Capability::ToolRead, SupportLevel::Native);
            m.insert(
                Capability::Streaming,
                SupportLevel::Restricted {
                    reason: "rate-limited".into(),
                },
            );
            m
        },
        mode: ExecutionMode::Passthrough,
        usage_raw: json!({"prompt_tokens": 100}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.01),
        },
        trace: vec![
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunStarted {
                    message: "go".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            },
        ],
        artifacts: vec![ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff".into()),
            git_status: None,
            harness_ok: false,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    roundtrip_json(&receipt);
}

// ── enum variant roundtrips ────────────────────────────────────────────

#[test]
fn roundtrip_execution_lane_variants() {
    roundtrip_json(&ExecutionLane::PatchFirst);
    roundtrip_json(&ExecutionLane::WorkspaceFirst);
}

#[test]
fn roundtrip_workspace_mode_variants() {
    roundtrip_json(&WorkspaceMode::PassThrough);
    roundtrip_json(&WorkspaceMode::Staged);
}

#[test]
fn roundtrip_outcome_variants() {
    roundtrip_json(&Outcome::Complete);
    roundtrip_json(&Outcome::Partial);
    roundtrip_json(&Outcome::Failed);
}

#[test]
fn roundtrip_execution_mode_variants() {
    roundtrip_json(&ExecutionMode::Passthrough);
    roundtrip_json(&ExecutionMode::Mapped);
}

#[test]
fn roundtrip_min_support_variants() {
    roundtrip_json(&MinSupport::Native);
    roundtrip_json(&MinSupport::Emulated);
}

#[test]
fn roundtrip_support_level_variants() {
    roundtrip_json(&SupportLevel::Native);
    roundtrip_json(&SupportLevel::Emulated);
    roundtrip_json(&SupportLevel::Unsupported);
    roundtrip_json(&SupportLevel::Restricted {
        reason: "policy".into(),
    });
}

#[test]
fn roundtrip_capability_all_variants() {
    let caps = [
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
    ];
    for cap in &caps {
        roundtrip_json(cap);
    }
}

#[test]
fn roundtrip_agent_event_kind_run_started() {
    let k = AgentEventKind::RunStarted {
        message: "starting".into(),
    };
    roundtrip_json(&k);
}

#[test]
fn roundtrip_agent_event_kind_run_completed() {
    let k = AgentEventKind::RunCompleted {
        message: "done".into(),
    };
    roundtrip_json(&k);
}

#[test]
fn roundtrip_agent_event_kind_assistant_delta() {
    let k = AgentEventKind::AssistantDelta {
        text: "partial".into(),
    };
    roundtrip_json(&k);
}

#[test]
fn roundtrip_agent_event_kind_assistant_message() {
    let k = AgentEventKind::AssistantMessage {
        text: "full response".into(),
    };
    roundtrip_json(&k);
}

#[test]
fn roundtrip_agent_event_kind_tool_call() {
    let k = AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu_123".into()),
        parent_tool_use_id: Some("tu_parent".into()),
        input: json!({"path": "/tmp/file.txt"}),
    };
    roundtrip_json(&k);
}

#[test]
fn roundtrip_agent_event_kind_tool_result() {
    let k = AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu_123".into()),
        output: json!({"content": "file data"}),
        is_error: false,
    };
    roundtrip_json(&k);
}

#[test]
fn roundtrip_agent_event_kind_file_changed() {
    let k = AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "Added function".into(),
    };
    roundtrip_json(&k);
}

#[test]
fn roundtrip_agent_event_kind_command_executed() {
    let k = AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("test result: ok".into()),
    };
    roundtrip_json(&k);
}

#[test]
fn roundtrip_agent_event_kind_warning() {
    let k = AgentEventKind::Warning {
        message: "deprecated API".into(),
    };
    roundtrip_json(&k);
}

#[test]
fn roundtrip_agent_event_kind_error() {
    let k = AgentEventKind::Error {
        message: "OOM".into(),
    };
    roundtrip_json(&k);
}

// ── capability manifest (BTreeMap alias) ──────────────────────────────

#[test]
fn roundtrip_capability_manifest() {
    let mut manifest: CapabilityManifest = BTreeMap::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::Streaming, SupportLevel::Emulated);
    manifest.insert(Capability::McpServer, SupportLevel::Unsupported);
    manifest.insert(
        Capability::Checkpointing,
        SupportLevel::Restricted {
            reason: "beta".into(),
        },
    );
    roundtrip_json(&manifest);
}

// ── edge cases ─────────────────────────────────────────────────────────

#[test]
fn empty_collections_roundtrip() {
    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "empty".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket {
            files: vec![],
            snippets: vec![],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    roundtrip_json(&wo);
}

#[test]
fn optional_fields_absent() {
    let rc = RuntimeConfig {
        model: None,
        vendor: BTreeMap::new(),
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };
    roundtrip_json(&rc);

    let bi = BackendIdentity {
        id: "x".into(),
        backend_version: None,
        adapter_version: None,
    };
    roundtrip_json(&bi);

    let u = UsageNormalized::default();
    roundtrip_json(&u);

    let vr = VerificationReport::default();
    roundtrip_json(&vr);
}

#[test]
fn optional_fields_present() {
    let rc = RuntimeConfig {
        model: Some("model".into()),
        vendor: {
            let mut m = BTreeMap::new();
            m.insert("k".into(), json!("v"));
            m
        },
        env: {
            let mut m = BTreeMap::new();
            m.insert("A".into(), "B".into());
            m
        },
        max_budget_usd: Some(10.0),
        max_turns: Some(100),
    };
    roundtrip_json(&rc);
}

#[test]
fn agent_event_ext_none_vs_some() {
    let ev_none = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Warning {
            message: "w".into(),
        },
        ext: None,
    };
    roundtrip_json(&ev_none);

    let ev_some = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Warning {
            message: "w".into(),
        },
        ext: Some({
            let mut m = BTreeMap::new();
            m.insert("raw_message".into(), json!({"x": 1}));
            m
        }),
    };
    roundtrip_json(&ev_some);
}

#[test]
fn tool_call_optional_ids_absent() {
    let k = AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    };
    roundtrip_json(&k);
}

#[test]
fn command_executed_optional_fields_absent() {
    let k = AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: None,
        output_preview: None,
    };
    roundtrip_json(&k);
}

#[test]
fn default_values_roundtrip() {
    roundtrip_json(&ContextPacket::default());
    roundtrip_json(&RuntimeConfig::default());
    roundtrip_json(&PolicyProfile::default());
    roundtrip_json(&CapabilityRequirements::default());
    roundtrip_json(&UsageNormalized::default());
    roundtrip_json(&VerificationReport::default());
    roundtrip_json(&ExecutionMode::default());
}

#[test]
fn nested_events_in_receipt_preserve() {
    let tool_call_event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("id1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "f.rs", "content": "fn main(){}"}),
        },
        ext: None,
    };
    let tool_result_event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "write_file".into(),
            tool_use_id: Some("id1".into()),
            output: json!({"ok": true}),
            is_error: false,
        },
        ext: None,
    };
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts(),
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!(null),
        usage: UsageNormalized::default(),
        trace: vec![tool_call_event, tool_result_event],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Partial,
        receipt_sha256: None,
    };
    roundtrip_json(&receipt);
}

#[test]
fn unknown_fields_ignored_agent_event_kind() {
    // AgentEventKind uses internally tagged enum; extra fields should deserialize fine
    // if deny_unknown_fields is not set.
    let json_str = r#"{
        "type": "warning",
        "message": "something",
        "extra_unknown_field": 42
    }"#;
    let kind: AgentEventKind = serde_json::from_str(json_str).expect("should ignore unknown field");
    match kind {
        AgentEventKind::Warning { ref message } => assert_eq!(message, "something"),
        _ => panic!("expected Warning variant"),
    }
}

#[test]
fn unknown_fields_ignored_work_order() {
    // Build a valid WorkOrder JSON, then inject an unknown field.
    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "t".into(),
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
    let mut v = serde_json::to_value(&wo).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("totally_unknown".into(), json!("surprise"));
    let back: WorkOrder = serde_json::from_value(v).expect("should ignore unknown field");
    assert_eq!(back.task, "t");
}

#[test]
fn unknown_fields_ignored_receipt() {
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts(),
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "m".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::default(),
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Failed,
        receipt_sha256: None,
    };
    let mut v = serde_json::to_value(&receipt).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("new_field_v2".into(), json!(true));
    let _back: Receipt = serde_json::from_value(v).expect("should ignore unknown field");
}

#[test]
fn tool_result_is_error_true() {
    let k = AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: json!("command not found"),
        is_error: true,
    };
    roundtrip_json(&k);
}

#[test]
fn receipt_with_hash_roundtrips() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    roundtrip_json(&receipt);
}

#[test]
fn execution_mode_default_is_mapped() {
    let mode: ExecutionMode = serde_json::from_str(r#""mapped""#).unwrap();
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn capability_manifest_empty() {
    let manifest: CapabilityManifest = BTreeMap::new();
    roundtrip_json(&manifest);
}
