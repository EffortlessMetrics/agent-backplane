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
//! Comprehensive tests for deterministic / canonical serialization across the
//! ABP workspace.  BTreeMap usage, key ordering, tag fields, rename_all, hash
//! stability, null-vs-missing, and numeric precision are all covered.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, ArtifactRef,
    BackendIdentity, Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use abp_protocol::Envelope;
use chrono::{TimeZone, Utc};
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 5, 0).unwrap()
}

fn fixed_uuid() -> Uuid {
    Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
}

fn fixed_uuid2() -> Uuid {
    Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap()
}

fn make_backend() -> BackendIdentity {
    BackendIdentity {
        id: "mock".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn make_run_metadata() -> RunMetadata {
    RunMetadata {
        run_id: fixed_uuid(),
        work_order_id: fixed_uuid2(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: fixed_ts(),
        finished_at: fixed_ts2(),
        duration_ms: 300_000,
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: fixed_ts(),
        kind,
        ext: None,
    }
}

fn make_receipt() -> Receipt {
    Receipt {
        meta: make_run_metadata(),
        backend: make_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!({}),
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
        id: fixed_uuid(),
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
    match v {
        Value::Object(map) => map.keys().cloned().collect(),
        _ => vec![],
    }
}

// =========================================================================
// 1. BTreeMap key ordering
// =========================================================================

#[test]
fn btreemap_runtime_config_vendor_keys_sorted() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert("zebra".into(), serde_json::json!(1));
    config.vendor.insert("alpha".into(), serde_json::json!(2));
    config.vendor.insert("middle".into(), serde_json::json!(3));

    let v: Value = serde_json::to_value(&config).unwrap();
    let vendor_keys = sorted_keys(&v["vendor"]);
    assert_eq!(vendor_keys, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn btreemap_runtime_config_env_keys_sorted() {
    let mut config = RuntimeConfig::default();
    config.env.insert("Z_VAR".into(), "z".into());
    config.env.insert("A_VAR".into(), "a".into());
    config.env.insert("M_VAR".into(), "m".into());

    let v: Value = serde_json::to_value(&config).unwrap();
    let env_keys = sorted_keys(&v["env"]);
    assert_eq!(env_keys, vec!["A_VAR", "M_VAR", "Z_VAR"]);
}

#[test]
fn btreemap_capability_manifest_keys_sorted() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    caps.insert(Capability::McpClient, SupportLevel::Unsupported);

    let v: Value = serde_json::to_value(&caps).unwrap();
    let keys = sorted_keys(&v);
    // BTreeMap<Capability, _> uses Ord on Capability — serialized keys are
    // derived from serde rename_all = "snake_case".
    let is_sorted = keys.windows(2).all(|w| w[0] <= w[1]);
    assert!(is_sorted, "capability manifest keys not sorted: {keys:?}");
}

#[test]
fn btreemap_agent_event_ext_keys_sorted() {
    let mut ext = BTreeMap::new();
    ext.insert("z_field".into(), serde_json::json!("z"));
    ext.insert("a_field".into(), serde_json::json!("a"));
    ext.insert("m_field".into(), serde_json::json!("m"));

    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "hi".into(),
        },
        ext: Some(ext),
    };

    let v: Value = serde_json::to_value(&event).unwrap();
    let ext_keys = sorted_keys(&v["ext"]);
    assert_eq!(ext_keys, vec!["a_field", "m_field", "z_field"]);
}

#[test]
fn btreemap_ir_message_metadata_keys_sorted() {
    let mut meta = BTreeMap::new();
    meta.insert("z_key".into(), serde_json::json!(1));
    meta.insert("a_key".into(), serde_json::json!(2));
    meta.insert("m_key".into(), serde_json::json!(3));

    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text {
            text: "hello".into(),
        }],
        metadata: meta,
    };

    let v: Value = serde_json::to_value(&msg).unwrap();
    let keys = sorted_keys(&v["metadata"]);
    assert_eq!(keys, vec!["a_key", "m_key", "z_key"]);
}

// =========================================================================
// 2. WorkOrder canonical JSON stability
// =========================================================================

#[test]
fn work_order_canonical_json_is_deterministic() {
    let wo = make_work_order();
    let json1 = canonical_json(&wo).unwrap();
    let json2 = canonical_json(&wo).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn work_order_roundtrip_preserves_json() {
    let wo = make_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&wo2).unwrap();
    assert_eq!(json, json2);
}

#[test]
fn work_order_field_ordering_in_json() {
    let wo = make_work_order();
    let v: Value = serde_json::to_value(&wo).unwrap();
    // All top-level keys should exist
    assert!(v.get("id").is_some());
    assert!(v.get("task").is_some());
    assert!(v.get("lane").is_some());
    assert!(v.get("workspace").is_some());
    assert!(v.get("config").is_some());
}

#[test]
fn work_order_with_vendor_config_deterministic() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert("z".into(), serde_json::json!("last"));
    config.vendor.insert("a".into(), serde_json::json!("first"));
    config
        .vendor
        .insert("m".into(), serde_json::json!("middle"));
    config.model = Some("gpt-4".into());

    let mut wo = make_work_order();
    wo.config = config;

    let json1 = canonical_json(&wo).unwrap();
    let json2 = canonical_json(&wo).unwrap();
    assert_eq!(json1, json2);

    // Verify vendor keys are ordered
    let v: Value = serde_json::from_str(&json1).unwrap();
    let vendor_keys = sorted_keys(&v["config"]["vendor"]);
    assert_eq!(vendor_keys, vec!["a", "m", "z"]);
}

#[test]
fn work_order_builder_produces_valid_json() {
    let wo = WorkOrderBuilder::new("test task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/test")
        .model("claude-3")
        .max_budget_usd(10.0)
        .max_turns(5)
        .build();

    let json = canonical_json(&wo).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["task"], "test task");
    assert_eq!(v["lane"], "workspace_first");
}

#[test]
fn work_order_with_all_policy_fields() {
    let mut wo = make_work_order();
    wo.policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/etc/*".into()],
        allow_network: vec!["example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["delete".into()],
    };

    let json1 = canonical_json(&wo).unwrap();
    let json2 = canonical_json(&wo).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn work_order_with_context_snippets() {
    let mut wo = make_work_order();
    wo.context = ContextPacket {
        files: vec!["src/main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "readme".into(),
            content: "# Hello".into(),
        }],
    };

    let json = canonical_json(&wo).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["context"]["files"][0], "src/main.rs");
    assert_eq!(v["context"]["snippets"][0]["name"], "readme");
}

// =========================================================================
// 3. Receipt canonical JSON stability
// =========================================================================

#[test]
fn receipt_canonical_json_is_deterministic() {
    let r = make_receipt();
    let json1 = canonical_json(&r).unwrap();
    let json2 = canonical_json(&r).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn receipt_roundtrip_preserves_json() {
    let r = make_receipt();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&r2).unwrap();
    assert_eq!(json, json2);
}

#[test]
fn receipt_hash_is_deterministic() {
    let r = make_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn receipt_with_hash_idempotent() {
    let r = make_receipt().with_hash().unwrap();
    let h1 = r.receipt_sha256.clone().unwrap();
    let r2 = r.with_hash().unwrap();
    let h2 = r2.receipt_sha256.unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_excludes_receipt_sha256() {
    let mut r1 = make_receipt();
    r1.receipt_sha256 = None;
    let mut r2 = make_receipt();
    r2.receipt_sha256 = Some("deadbeef".repeat(8));
    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_on_outcome_change() {
    let r1 = make_receipt();
    let mut r2 = make_receipt();
    r2.outcome = Outcome::Failed;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_on_backend_change() {
    let r1 = make_receipt();
    let mut r2 = make_receipt();
    r2.backend.id = "other".into();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_builder_canonical_stability() {
    let ts = fixed_ts();
    let ts2 = fixed_ts2();
    let build = || {
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(ts)
            .finished_at(ts2)
            .work_order_id(fixed_uuid())
            .build()
    };

    let r1 = build();
    let r2 = build();
    // run_id is random, so compare everything except meta.run_id
    let mut v1: Value = serde_json::to_value(&r1).unwrap();
    let mut v2: Value = serde_json::to_value(&r2).unwrap();
    v1["meta"]["run_id"] = Value::Null;
    v2["meta"]["run_id"] = Value::Null;
    assert_eq!(v1, v2);
}

#[test]
fn receipt_with_capabilities_ordered() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    caps.insert(Capability::ToolRead, SupportLevel::Native);

    let mut r = make_receipt();
    r.capabilities = caps;

    let v: Value = serde_json::to_value(&r).unwrap();
    let keys = sorted_keys(&v["capabilities"]);
    let is_sorted = keys.windows(2).all(|w| w[0] <= w[1]);
    assert!(is_sorted, "capabilities not sorted: {keys:?}");
}

#[test]
fn receipt_with_trace_events_deterministic() {
    let mut r = make_receipt();
    r.trace = vec![
        make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];

    let json1 = canonical_json(&r).unwrap();
    let json2 = canonical_json(&r).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn receipt_with_artifacts_deterministic() {
    let mut r = make_receipt();
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

    let json1 = canonical_json(&r).unwrap();
    let json2 = canonical_json(&r).unwrap();
    assert_eq!(json1, json2);
}

// =========================================================================
// 4. AgentEvent canonical JSON stability
// =========================================================================

#[test]
fn agent_event_run_started_canonical() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "starting".into(),
    });
    let json1 = canonical_json(&e).unwrap();
    let json2 = canonical_json(&e).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn agent_event_tool_call_canonical() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu_1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "/etc/passwd"}),
    });
    let json1 = canonical_json(&e).unwrap();
    let json2 = canonical_json(&e).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn agent_event_tool_result_canonical() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu_1".into()),
        output: serde_json::json!({"content": "root:x:0:0"}),
        is_error: false,
    });
    let json = canonical_json(&e).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "tool_result");
    assert_eq!(v["is_error"], false);
}

#[test]
fn agent_event_file_changed_canonical() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "Added main function".into(),
    });
    let json = canonical_json(&e).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "file_changed");
}

#[test]
fn agent_event_command_executed_canonical() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("All tests passed".into()),
    });
    let json1 = canonical_json(&e).unwrap();
    let json2 = canonical_json(&e).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn agent_event_warning_canonical() {
    let e = make_event(AgentEventKind::Warning {
        message: "Low budget".into(),
    });
    let json = canonical_json(&e).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "warning");
}

#[test]
fn agent_event_error_canonical() {
    let e = make_event(AgentEventKind::Error {
        message: "crash".into(),
        error_code: None,
    });
    let json = canonical_json(&e).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "error");
}

#[test]
fn agent_event_assistant_delta_canonical() {
    let e = make_event(AgentEventKind::AssistantDelta {
        text: "streaming chunk".into(),
    });
    let json1 = canonical_json(&e).unwrap();
    let json2 = canonical_json(&e).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn agent_event_all_kinds_roundtrip() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "a".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "b".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "c".into() }),
        make_event(AgentEventKind::AssistantMessage { text: "d".into() }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: None,
            output: serde_json::json!(null),
            is_error: false,
        }),
        make_event(AgentEventKind::FileChanged {
            path: "f".into(),
            summary: "s".into(),
        }),
        make_event(AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: None,
            output_preview: None,
        }),
        make_event(AgentEventKind::Warning {
            message: "w".into(),
        }),
        make_event(AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        }),
    ];

    for event in &events {
        let json = serde_json::to_string(event).unwrap();
        let back: AgentEvent = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2, "roundtrip failed for event");
    }
}

// =========================================================================
// 5. Envelope canonical JSON stability
// =========================================================================

#[test]
fn envelope_hello_uses_tag_t() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: make_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "hello");
    assert!(
        v.get("type").is_none(),
        "Envelope should use 't', not 'type'"
    );
}

#[test]
fn envelope_run_uses_tag_t() {
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: make_work_order(),
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "run");
}

#[test]
fn envelope_event_uses_tag_t() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "event");
}

#[test]
fn envelope_final_uses_tag_t() {
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: make_receipt(),
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "final");
}

#[test]
fn envelope_fatal_uses_tag_t() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "fatal");
}

#[test]
fn envelope_hello_canonical_deterministic() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: make_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let json1 = canonical_json(&env).unwrap();
    let json2 = canonical_json(&env).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn envelope_run_canonical_deterministic() {
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: make_work_order(),
    };
    let json1 = canonical_json(&env).unwrap();
    let json2 = canonical_json(&env).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn envelope_all_variants_roundtrip() {
    let envs: Vec<Envelope> = vec![
        Envelope::Hello {
            contract_version: CONTRACT_VERSION.into(),
            backend: make_backend(),
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
        },
        Envelope::Run {
            id: "r".into(),
            work_order: make_work_order(),
        },
        Envelope::Event {
            ref_id: "r".into(),
            event: make_event(AgentEventKind::RunStarted {
                message: "go".into(),
            }),
        },
        Envelope::Final {
            ref_id: "r".into(),
            receipt: make_receipt(),
        },
        Envelope::Fatal {
            ref_id: None,
            error: "err".into(),
            error_code: None,
        },
    ];

    for env in &envs {
        let json = serde_json::to_string(env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2, "Envelope roundtrip failed");
    }
}

// =========================================================================
// 6. IR types canonical serialization
// =========================================================================

#[test]
fn ir_role_snake_case_serialization() {
    assert_eq!(
        serde_json::to_string(&IrRole::System).unwrap(),
        "\"system\""
    );
    assert_eq!(serde_json::to_string(&IrRole::User).unwrap(), "\"user\"");
    assert_eq!(
        serde_json::to_string(&IrRole::Assistant).unwrap(),
        "\"assistant\""
    );
    assert_eq!(serde_json::to_string(&IrRole::Tool).unwrap(), "\"tool\"");
}

#[test]
fn ir_content_block_text_tag_type() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    let v: Value = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "text");
}

#[test]
fn ir_content_block_image_tag_type() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "base64data".into(),
    };
    let v: Value = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "image");
}

#[test]
fn ir_content_block_tool_use_tag_type() {
    let block = IrContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "read".into(),
        input: serde_json::json!({}),
    };
    let v: Value = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "tool_use");
}

#[test]
fn ir_content_block_tool_result_tag_type() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: vec![IrContentBlock::Text { text: "ok".into() }],
        is_error: false,
    };
    let v: Value = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "tool_result");
}

#[test]
fn ir_content_block_thinking_tag_type() {
    let block = IrContentBlock::Thinking {
        text: "let me think".into(),
    };
    let v: Value = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "thinking");
}

#[test]
fn ir_message_canonical_deterministic() {
    let msg = IrMessage {
        role: IrRole::Assistant,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata: BTreeMap::new(),
    };
    let json1 = canonical_json(&msg).unwrap();
    let json2 = canonical_json(&msg).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn ir_message_metadata_skipped_when_empty() {
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata: BTreeMap::new(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(
        !json.contains("metadata"),
        "empty metadata should be skipped via skip_serializing_if"
    );
}

#[test]
fn ir_message_metadata_present_when_nonempty() {
    let mut meta = BTreeMap::new();
    meta.insert("key".into(), serde_json::json!("value"));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![],
        metadata: meta,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("metadata"));
}

#[test]
fn ir_conversation_roundtrip() {
    let conv = IrConversation {
        messages: vec![
            IrMessage {
                role: IrRole::User,
                content: vec![IrContentBlock::Text {
                    text: "hello".into(),
                }],
                metadata: BTreeMap::new(),
            },
            IrMessage {
                role: IrRole::Assistant,
                content: vec![IrContentBlock::Text {
                    text: "hi there".into(),
                }],
                metadata: BTreeMap::new(),
            },
        ],
    };
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(conv, back);
}

#[test]
fn ir_tool_definition_canonical() {
    let tool = IrToolDefinition {
        name: "read_file".into(),
        description: "Reads a file".into(),
        parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let json1 = canonical_json(&tool).unwrap();
    let json2 = canonical_json(&tool).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn ir_usage_default_all_zeros() {
    let usage = IrUsage::default();
    let v: Value = serde_json::to_value(usage).unwrap();
    assert_eq!(v["input_tokens"], 0);
    assert_eq!(v["output_tokens"], 0);
    assert_eq!(v["total_tokens"], 0);
    assert_eq!(v["cache_read_tokens"], 0);
    assert_eq!(v["cache_write_tokens"], 0);
}

// =========================================================================
// 7. Insertion order independence
// =========================================================================

#[test]
fn vendor_map_insertion_order_independent() {
    let mut m1 = BTreeMap::new();
    m1.insert("z".to_string(), serde_json::json!(1));
    m1.insert("a".to_string(), serde_json::json!(2));
    m1.insert("m".to_string(), serde_json::json!(3));

    let mut m2 = BTreeMap::new();
    m2.insert("a".to_string(), serde_json::json!(2));
    m2.insert("m".to_string(), serde_json::json!(3));
    m2.insert("z".to_string(), serde_json::json!(1));

    let mut m3 = BTreeMap::new();
    m3.insert("m".to_string(), serde_json::json!(3));
    m3.insert("z".to_string(), serde_json::json!(1));
    m3.insert("a".to_string(), serde_json::json!(2));

    let j1 = serde_json::to_string(&m1).unwrap();
    let j2 = serde_json::to_string(&m2).unwrap();
    let j3 = serde_json::to_string(&m3).unwrap();
    assert_eq!(j1, j2);
    assert_eq!(j2, j3);
}

#[test]
fn capability_manifest_insertion_order_independent() {
    let mut caps1 = CapabilityManifest::new();
    caps1.insert(Capability::Streaming, SupportLevel::Native);
    caps1.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps1.insert(Capability::ToolWrite, SupportLevel::Native);

    let mut caps2 = CapabilityManifest::new();
    caps2.insert(Capability::ToolWrite, SupportLevel::Native);
    caps2.insert(Capability::Streaming, SupportLevel::Native);
    caps2.insert(Capability::ToolRead, SupportLevel::Emulated);

    let j1 = serde_json::to_string(&caps1).unwrap();
    let j2 = serde_json::to_string(&caps2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn runtime_config_env_insertion_order_independent() {
    let mut c1 = RuntimeConfig::default();
    c1.env.insert("Z".into(), "1".into());
    c1.env.insert("A".into(), "2".into());

    let mut c2 = RuntimeConfig::default();
    c2.env.insert("A".into(), "2".into());
    c2.env.insert("Z".into(), "1".into());

    let j1 = canonical_json(&c1).unwrap();
    let j2 = canonical_json(&c2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn receipt_caps_insertion_order_independent_hash() {
    let mut caps1 = CapabilityManifest::new();
    caps1.insert(Capability::Streaming, SupportLevel::Native);
    caps1.insert(Capability::ToolRead, SupportLevel::Native);

    let mut caps2 = CapabilityManifest::new();
    caps2.insert(Capability::ToolRead, SupportLevel::Native);
    caps2.insert(Capability::Streaming, SupportLevel::Native);

    let mut r1 = make_receipt();
    r1.capabilities = caps1;
    let mut r2 = make_receipt();
    r2.capabilities = caps2;

    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// =========================================================================
// 8. Canonical JSON → hash → same hash regardless of construction order
// =========================================================================

#[test]
fn hash_stability_across_construction_orders() {
    let build_receipt = |order: u8| -> Receipt {
        let mut caps = CapabilityManifest::new();
        let items = vec![
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::McpClient, SupportLevel::Unsupported),
        ];

        match order {
            0 => {
                for (k, v) in items {
                    caps.insert(k, v);
                }
            }
            1 => {
                for (k, v) in items.into_iter().rev() {
                    caps.insert(k, v);
                }
            }
            _ => {
                // Middle-first
                caps.insert(Capability::Streaming, SupportLevel::Emulated);
                caps.insert(Capability::ToolRead, SupportLevel::Native);
                caps.insert(Capability::McpClient, SupportLevel::Unsupported);
            }
        }

        let mut r = make_receipt();
        r.capabilities = caps;
        r
    };

    let h0 = receipt_hash(&build_receipt(0)).unwrap();
    let h1 = receipt_hash(&build_receipt(1)).unwrap();
    let h2 = receipt_hash(&build_receipt(2)).unwrap();
    assert_eq!(h0, h1);
    assert_eq!(h1, h2);
}

#[test]
fn sha256_hex_deterministic() {
    let h1 = sha256_hex(b"canonical json test");
    let h2 = sha256_hex(b"canonical json test");
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn canonical_json_produces_identical_bytes() {
    let r = make_receipt();
    let j1 = canonical_json(&r).unwrap();
    let j2 = canonical_json(&r).unwrap();
    assert_eq!(j1.as_bytes(), j2.as_bytes());
}

#[test]
fn hash_differs_for_different_env_values() {
    let mut r1 = make_receipt();
    let mut config1 = RuntimeConfig::default();
    config1.env.insert("KEY".into(), "value1".into());
    let mut wo1 = make_work_order();
    wo1.config = config1;

    let mut config2 = RuntimeConfig::default();
    config2.env.insert("KEY".into(), "value2".into());
    let mut wo2 = make_work_order();
    wo2.config = config2;

    let j1 = canonical_json(&wo1).unwrap();
    let j2 = canonical_json(&wo2).unwrap();
    assert_ne!(j1, j2);

    // Different env → different hash bytes
    let h1 = sha256_hex(j1.as_bytes());
    let h2 = sha256_hex(j2.as_bytes());
    assert_ne!(h1, h2);

    drop(r1); // suppress unused
    r1 = make_receipt();
    drop(r1);
}

// =========================================================================
// 9. Serde rename_all = "snake_case" consistency across all enums
// =========================================================================

#[test]
fn execution_lane_rename() {
    let j1 = serde_json::to_string(&ExecutionLane::PatchFirst).unwrap();
    let j2 = serde_json::to_string(&ExecutionLane::WorkspaceFirst).unwrap();
    assert_eq!(j1, "\"patch_first\"");
    assert_eq!(j2, "\"workspace_first\"");
}

#[test]
fn workspace_mode_rename() {
    let j1 = serde_json::to_string(&WorkspaceMode::PassThrough).unwrap();
    let j2 = serde_json::to_string(&WorkspaceMode::Staged).unwrap();
    assert_eq!(j1, "\"pass_through\"");
    assert_eq!(j2, "\"staged\"");
}

#[test]
fn execution_mode_rename() {
    let j1 = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    let j2 = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
    assert_eq!(j1, "\"passthrough\"");
    assert_eq!(j2, "\"mapped\"");
}

#[test]
fn outcome_rename() {
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

#[test]
fn capability_rename_snake_case() {
    assert_eq!(
        serde_json::to_string(&Capability::ToolRead).unwrap(),
        "\"tool_read\""
    );
    assert_eq!(
        serde_json::to_string(&Capability::McpClient).unwrap(),
        "\"mcp_client\""
    );
    assert_eq!(
        serde_json::to_string(&Capability::HooksPreToolUse).unwrap(),
        "\"hooks_pre_tool_use\""
    );
    assert_eq!(
        serde_json::to_string(&Capability::StructuredOutputJsonSchema).unwrap(),
        "\"structured_output_json_schema\""
    );
}

#[test]
fn support_level_rename_snake_case() {
    assert_eq!(
        serde_json::to_string(&SupportLevel::Native).unwrap(),
        "\"native\""
    );
    assert_eq!(
        serde_json::to_string(&SupportLevel::Emulated).unwrap(),
        "\"emulated\""
    );
    assert_eq!(
        serde_json::to_string(&SupportLevel::Unsupported).unwrap(),
        "\"unsupported\""
    );
}

#[test]
fn min_support_rename_snake_case() {
    assert_eq!(
        serde_json::to_string(&MinSupport::Native).unwrap(),
        "\"native\""
    );
    assert_eq!(
        serde_json::to_string(&MinSupport::Emulated).unwrap(),
        "\"emulated\""
    );
}

#[test]
fn ir_role_rename_snake_case() {
    assert_eq!(
        serde_json::to_string(&IrRole::System).unwrap(),
        "\"system\""
    );
    assert_eq!(serde_json::to_string(&IrRole::User).unwrap(), "\"user\"");
    assert_eq!(
        serde_json::to_string(&IrRole::Assistant).unwrap(),
        "\"assistant\""
    );
    assert_eq!(serde_json::to_string(&IrRole::Tool).unwrap(), "\"tool\"");
}

#[test]
fn agent_event_kind_type_tag_snake_case() {
    let cases: Vec<(AgentEventKind, &str)> = vec![
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
                tool_name: "".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!(null),
            },
            "tool_call",
        ),
        (
            AgentEventKind::ToolResult {
                tool_name: "".into(),
                tool_use_id: None,
                output: serde_json::json!(null),
                is_error: false,
            },
            "tool_result",
        ),
        (
            AgentEventKind::FileChanged {
                path: "".into(),
                summary: "".into(),
            },
            "file_changed",
        ),
        (
            AgentEventKind::CommandExecuted {
                command: "".into(),
                exit_code: None,
                output_preview: None,
            },
            "command_executed",
        ),
        (AgentEventKind::Warning { message: "".into() }, "warning"),
        (
            AgentEventKind::Error {
                message: "".into(),
                error_code: None,
            },
            "error",
        ),
    ];

    for (kind, expected_tag) in cases {
        let event = make_event(kind);
        let v: Value = serde_json::to_value(&event).unwrap();
        assert_eq!(
            v["type"].as_str().unwrap(),
            expected_tag,
            "wrong tag for event"
        );
    }
}

// =========================================================================
// 10. Tag field consistency
// =========================================================================

#[test]
fn agent_event_uses_tag_type() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "x".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("type").is_some(), "AgentEvent should use 'type' tag");
    assert!(v.get("t").is_none(), "AgentEvent should NOT use 't' tag");
}

#[test]
fn envelope_uses_tag_t_not_type() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: make_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert!(v.get("t").is_some(), "Envelope should use 't' tag");
    assert!(
        v.get("type").is_none(),
        "Envelope should NOT use 'type' tag"
    );
}

#[test]
fn ir_content_block_uses_tag_type() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    let v: Value = serde_json::to_value(&block).unwrap();
    assert!(
        v.get("type").is_some(),
        "IrContentBlock should use 'type' tag"
    );
    assert!(
        v.get("t").is_none(),
        "IrContentBlock should NOT use 't' tag"
    );
}

#[test]
fn nested_event_in_envelope_preserves_both_tags() {
    let event = make_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("tu_1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"cmd": "ls"}),
    });
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event,
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    // Outer envelope uses "t"
    assert_eq!(v["t"], "event");
    // Inner event uses "type"
    assert_eq!(v["event"]["type"], "tool_call");
}

// =========================================================================
// 11. Nested BTreeMap determinism
// =========================================================================

#[test]
fn nested_btreemap_in_vendor_config() {
    let mut inner = serde_json::Map::new();
    inner.insert("z_inner".into(), serde_json::json!(1));
    inner.insert("a_inner".into(), serde_json::json!(2));

    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("z_vendor".into(), Value::Object(inner.clone()));
    config
        .vendor
        .insert("a_vendor".into(), Value::Object(inner));

    let v: Value = serde_json::to_value(&config).unwrap();
    let vendor_keys = sorted_keys(&v["vendor"]);
    assert_eq!(vendor_keys, vec!["a_vendor", "z_vendor"]);

    // Inner keys should also be sorted (serde_json::Map uses BTreeMap)
    let inner_keys = sorted_keys(&v["vendor"]["a_vendor"]);
    assert_eq!(inner_keys, vec!["a_inner", "z_inner"]);
}

#[test]
fn deeply_nested_btreemap_three_levels() {
    let mut level3 = serde_json::Map::new();
    level3.insert("z3".into(), serde_json::json!(1));
    level3.insert("a3".into(), serde_json::json!(2));

    let mut level2 = serde_json::Map::new();
    level2.insert("z2".into(), Value::Object(level3));
    level2.insert("a2".into(), serde_json::json!("leaf"));

    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("z1".into(), Value::Object(level2.clone()));
    config.vendor.insert("a1".into(), Value::Object(level2));

    let json1 = canonical_json(&config).unwrap();
    let json2 = canonical_json(&config).unwrap();
    assert_eq!(json1, json2);

    let v: Value = serde_json::from_str(&json1).unwrap();
    assert_eq!(sorted_keys(&v["vendor"]), vec!["a1", "z1"]);
    assert_eq!(sorted_keys(&v["vendor"]["a1"]), vec!["a2", "z2"]);
    assert_eq!(sorted_keys(&v["vendor"]["a1"]["z2"]), vec!["a3", "z3"]);
}

#[test]
fn nested_btreemap_in_agent_event_ext() {
    let mut inner = BTreeMap::new();
    inner.insert("nested".to_string(), serde_json::json!({"z": 1, "a": 2}));
    inner.insert("top".to_string(), serde_json::json!("value"));

    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: Some(inner),
    };

    let v: Value = serde_json::to_value(&event).unwrap();
    let ext_keys = sorted_keys(&v["ext"]);
    assert_eq!(ext_keys, vec!["nested", "top"]);
    let nested_keys = sorted_keys(&v["ext"]["nested"]);
    assert_eq!(nested_keys, vec!["a", "z"]);
}

#[test]
fn nested_btreemap_in_ir_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("z_meta".into(), serde_json::json!({"b": 2, "a": 1}));
    meta.insert("a_meta".into(), serde_json::json!("simple"));

    let msg = IrMessage {
        role: IrRole::User,
        content: vec![],
        metadata: meta,
    };

    let v: Value = serde_json::to_value(&msg).unwrap();
    let meta_keys = sorted_keys(&v["metadata"]);
    assert_eq!(meta_keys, vec!["a_meta", "z_meta"]);
    let inner_keys = sorted_keys(&v["metadata"]["z_meta"]);
    assert_eq!(inner_keys, vec!["a", "b"]);
}

// =========================================================================
// 12. Large maps with many keys
// =========================================================================

#[test]
fn large_btreemap_100_keys_sorted() {
    let mut map: BTreeMap<String, Value> = BTreeMap::new();
    for i in (0..100).rev() {
        map.insert(format!("key_{i:03}"), serde_json::json!(i));
    }

    let json = serde_json::to_string(&map).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let keys = sorted_keys(&v);
    let expected: Vec<String> = (0..100).map(|i| format!("key_{i:03}")).collect();
    assert_eq!(keys, expected);
}

#[test]
fn large_capability_manifest_all_capabilities() {
    let all_caps = vec![
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

    let mut caps = CapabilityManifest::new();
    for cap in all_caps {
        caps.insert(cap, SupportLevel::Native);
    }

    let json1 = canonical_json(&caps).unwrap();
    let json2 = canonical_json(&caps).unwrap();
    assert_eq!(json1, json2);

    let v: Value = serde_json::from_str(&json1).unwrap();
    let keys = sorted_keys(&v);
    let is_sorted = keys.windows(2).all(|w| w[0] <= w[1]);
    assert!(is_sorted, "full capability manifest not sorted: {keys:?}");
}

#[test]
fn large_env_map_50_entries() {
    let mut config = RuntimeConfig::default();
    for i in (0..50).rev() {
        config.env.insert(format!("VAR_{i:03}"), format!("val_{i}"));
    }

    let json1 = canonical_json(&config).unwrap();
    let json2 = canonical_json(&config).unwrap();
    assert_eq!(json1, json2);

    let v: Value = serde_json::from_str(&json1).unwrap();
    let keys = sorted_keys(&v["env"]);
    let expected: Vec<String> = (0..50).map(|i| format!("VAR_{i:03}")).collect();
    assert_eq!(keys, expected);
}

#[test]
fn receipt_with_many_trace_events() {
    let mut r = make_receipt();
    for i in 0..100 {
        r.trace.push(make_event(AgentEventKind::AssistantDelta {
            text: format!("chunk {i}"),
        }));
    }
    let json1 = canonical_json(&r).unwrap();
    let json2 = canonical_json(&r).unwrap();
    assert_eq!(json1, json2);

    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

// =========================================================================
// 13. Unicode key ordering
// =========================================================================

#[test]
fn unicode_keys_sorted_by_codepoint() {
    let mut map: BTreeMap<String, Value> = BTreeMap::new();
    map.insert("ñ".into(), serde_json::json!(1));
    map.insert("a".into(), serde_json::json!(2));
    map.insert("z".into(), serde_json::json!(3));
    map.insert("á".into(), serde_json::json!(4));

    let v: Value = serde_json::to_value(&map).unwrap();
    let keys = sorted_keys(&v);
    // BTreeMap sorts by Rust's Ord on String = byte/codepoint order
    let is_sorted = keys.windows(2).all(|w| w[0] <= w[1]);
    assert!(is_sorted, "unicode keys not sorted: {keys:?}");
}

#[test]
fn unicode_keys_in_vendor_config() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("日本語".into(), serde_json::json!("ja"));
    config
        .vendor
        .insert("english".into(), serde_json::json!("en"));
    config.vendor.insert("中文".into(), serde_json::json!("zh"));

    let json1 = canonical_json(&config).unwrap();
    let json2 = canonical_json(&config).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn unicode_values_in_agent_event() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "Hello 你好 مرحبا 🎉".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&back).unwrap();
    assert_eq!(json, json2);
}

#[test]
fn emoji_in_btreemap_keys() {
    let mut map: BTreeMap<String, Value> = BTreeMap::new();
    map.insert("🍎".into(), serde_json::json!("apple"));
    map.insert("🍌".into(), serde_json::json!("banana"));
    map.insert("🍇".into(), serde_json::json!("grape"));

    let json1 = serde_json::to_string(&map).unwrap();
    let json2 = serde_json::to_string(&map).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn unicode_in_ir_content() {
    let msg = IrMessage {
        role: IrRole::Assistant,
        content: vec![IrContentBlock::Text {
            text: "日本語テスト 🚀".into(),
        }],
        metadata: BTreeMap::new(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

// =========================================================================
// 14. Numeric precision in JSON
// =========================================================================

#[test]
fn integer_precision_in_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(u64::MAX),
        output_tokens: Some(0),
        cache_read_tokens: Some(1_000_000_000),
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(back.input_tokens, Some(u64::MAX));
    assert_eq!(back.cache_read_tokens, Some(1_000_000_000));
}

#[test]
fn float_precision_in_usage_cost() {
    let usage = UsageNormalized {
        input_tokens: None,
        output_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: Some(0.001_234_567_89),
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(back.estimated_cost_usd, Some(0.001_234_567_89));
}

#[test]
fn float_precision_in_budget() {
    let config = RuntimeConfig {
        max_budget_usd: Some(99.999_999_999),
        ..RuntimeConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_budget_usd, Some(99.999_999_999));
}

#[test]
fn duration_ms_large_value() {
    let meta = RunMetadata {
        run_id: fixed_uuid(),
        work_order_id: fixed_uuid2(),
        contract_version: CONTRACT_VERSION.into(),
        started_at: fixed_ts(),
        finished_at: fixed_ts2(),
        duration_ms: u64::MAX,
    };
    let json = serde_json::to_string(&meta).unwrap();
    let back: RunMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back.duration_ms, u64::MAX);
}

#[test]
fn numeric_zero_values() {
    let usage = UsageNormalized {
        input_tokens: Some(0),
        output_tokens: Some(0),
        cache_read_tokens: Some(0),
        cache_write_tokens: Some(0),
        request_units: Some(0),
        estimated_cost_usd: Some(0.0),
    };
    let v: Value = serde_json::to_value(&usage).unwrap();
    assert_eq!(v["input_tokens"], 0);
    assert_eq!(v["estimated_cost_usd"], 0.0);

    let json = serde_json::to_string(&usage).unwrap();
    let back: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(back.estimated_cost_usd, Some(0.0));
}

#[test]
fn ir_usage_large_token_counts() {
    let usage = IrUsage {
        input_tokens: u64::MAX,
        output_tokens: u64::MAX,
        total_tokens: u64::MAX,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: IrUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.input_tokens, u64::MAX);
    assert_eq!(back.output_tokens, u64::MAX);
}

// =========================================================================
// 15. Null vs missing field handling in serde
// =========================================================================

#[test]
fn receipt_sha256_none_serializes_as_null() {
    let r = make_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

#[test]
fn receipt_sha256_some_serializes_as_string() {
    let r = make_receipt().with_hash().unwrap();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert!(v["receipt_sha256"].is_string());
}

#[test]
fn optional_fields_none_vs_missing() {
    let config = RuntimeConfig::default();
    let v: Value = serde_json::to_value(&config).unwrap();
    // model is None → should be null in JSON
    assert!(v["model"].is_null());
    // max_budget_usd is None
    assert!(v["max_budget_usd"].is_null());
}

#[test]
fn agent_event_ext_none_is_absent() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    // ext has skip_serializing_if = "Option::is_none"
    assert!(
        !json.contains("\"ext\""),
        "ext:None should be omitted from JSON"
    );
}

#[test]
fn agent_event_ext_some_is_present() {
    let mut ext = BTreeMap::new();
    ext.insert("key".into(), serde_json::json!("value"));
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("\"ext\""));
}

#[test]
fn agent_event_error_code_none_omitted() {
    let e = make_event(AgentEventKind::Error {
        message: "oops".into(),
        error_code: None,
    });
    let json = serde_json::to_string(&e).unwrap();
    assert!(
        !json.contains("error_code"),
        "error_code:None should be omitted"
    );
}

#[test]
fn backend_identity_versions_none_vs_some() {
    let b_none = BackendIdentity {
        id: "mock".into(),
        backend_version: None,
        adapter_version: None,
    };
    let b_some = BackendIdentity {
        id: "mock".into(),
        backend_version: Some("1.0".into()),
        adapter_version: Some("0.1".into()),
    };

    let v_none: Value = serde_json::to_value(&b_none).unwrap();
    let v_some: Value = serde_json::to_value(&b_some).unwrap();
    assert!(v_none["backend_version"].is_null());
    assert!(v_some["backend_version"].is_string());
}

#[test]
fn usage_all_none_fields() {
    let usage = UsageNormalized::default();
    let v: Value = serde_json::to_value(&usage).unwrap();
    assert!(v["input_tokens"].is_null());
    assert!(v["output_tokens"].is_null());
    assert!(v["cache_read_tokens"].is_null());
    assert!(v["cache_write_tokens"].is_null());
    assert!(v["request_units"].is_null());
    assert!(v["estimated_cost_usd"].is_null());
}

#[test]
fn command_executed_optional_fields() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: None,
        output_preview: None,
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v["exit_code"].is_null());
    assert!(v["output_preview"].is_null());
}

#[test]
fn tool_call_optional_ids() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v["tool_use_id"].is_null());
    assert!(v["parent_tool_use_id"].is_null());
}

#[test]
fn verification_report_defaults() {
    let vr = VerificationReport::default();
    let v: Value = serde_json::to_value(&vr).unwrap();
    assert!(v["git_diff"].is_null());
    assert!(v["git_status"].is_null());
    assert_eq!(v["harness_ok"], false);
}

#[test]
fn envelope_fatal_optional_ref_id() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert!(v["ref_id"].is_null());
}

#[test]
fn envelope_fatal_with_ref_id() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["ref_id"], "run-1");
}

// =========================================================================
// Extra: Cross-type canonical consistency
// =========================================================================

#[test]
fn canonical_json_matches_to_string_for_simple_types() {
    let outcome = Outcome::Complete;
    let canon = canonical_json(&outcome).unwrap();
    let direct = serde_json::to_string(&outcome).unwrap();
    assert_eq!(canon, direct);
}

#[test]
fn repeated_serialization_50_iterations() {
    let r = make_receipt();
    let reference = canonical_json(&r).unwrap();
    for _ in 0..50 {
        assert_eq!(canonical_json(&r).unwrap(), reference);
    }
}

#[test]
fn repeated_hashing_50_iterations() {
    let r = make_receipt();
    let reference = receipt_hash(&r).unwrap();
    for _ in 0..50 {
        assert_eq!(receipt_hash(&r).unwrap(), reference);
    }
}

#[test]
fn support_level_restricted_with_reason() {
    let level = SupportLevel::Restricted {
        reason: "feature flag disabled".into(),
    };
    let v: Value = serde_json::to_value(&level).unwrap();
    assert_eq!(v["restricted"]["reason"], "feature flag disabled");
}

#[test]
fn capability_requirement_roundtrip() {
    let req = CapabilityRequirement {
        capability: Capability::ToolRead,
        min_support: MinSupport::Emulated,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CapabilityRequirement = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&back).unwrap();
    assert_eq!(json, json2);
}

#[test]
fn empty_collections_serialize_as_arrays() {
    let wo = make_work_order();
    let v: Value = serde_json::to_value(&wo).unwrap();
    assert!(v["workspace"]["include"].as_array().unwrap().is_empty());
    assert!(v["workspace"]["exclude"].as_array().unwrap().is_empty());
    assert!(v["context"]["files"].as_array().unwrap().is_empty());
    assert!(v["context"]["snippets"].as_array().unwrap().is_empty());
}

#[test]
fn receipt_full_lifecycle_hash_stability() {
    // Build a receipt with all fields populated, hash it, roundtrip, re-hash
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);

    let mut r = make_receipt();
    r.capabilities = caps;
    r.usage = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: Some(100),
        cache_write_tokens: Some(50),
        request_units: Some(10),
        estimated_cost_usd: Some(0.05),
    };
    r.verification = VerificationReport {
        git_diff: Some("diff content".into()),
        git_status: Some("M src/main.rs".into()),
        harness_ok: true,
    };
    r.trace = vec![
        make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];

    let h1 = receipt_hash(&r).unwrap();

    // Roundtrip through JSON
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn work_order_full_config_canonical() {
    let mut config = RuntimeConfig {
        model: Some("claude-3".into()),
        vendor: BTreeMap::new(),
        env: BTreeMap::new(),
        max_budget_usd: Some(100.0),
        max_turns: Some(10),
    };
    config.vendor.insert(
        "anthropic".into(),
        serde_json::json!({"api_key_env": "ANTHROPIC_API_KEY"}),
    );
    config.env.insert("PATH".into(), "/usr/bin".into());
    config.env.insert("HOME".into(), "/home/user".into());

    let mut wo = make_work_order();
    wo.config = config;

    let json1 = canonical_json(&wo).unwrap();
    let json2 = canonical_json(&wo).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn special_characters_in_strings() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "line1\nline2\ttab\"quote\\backslash".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&back).unwrap();
    assert_eq!(json, json2);
}

#[test]
fn empty_string_fields() {
    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    let json1 = canonical_json(&wo).unwrap();
    let json2 = canonical_json(&wo).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn envelope_hello_with_full_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);
    caps.insert(
        Capability::McpClient,
        SupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    );

    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: make_backend(),
        capabilities: caps,
        mode: ExecutionMode::Passthrough,
    };
    let json1 = canonical_json(&env).unwrap();
    let json2 = canonical_json(&env).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn contract_version_present_in_receipt() {
    let r = make_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["meta"]["contract_version"], CONTRACT_VERSION);
}
