#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive canonical JSON serialization, deterministic hashing, and
//! BTreeMap ordering tests across ALL ABP contract types.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec, canonical_json, receipt_hash,
    sha256_hex,
};
use abp_protocol::Envelope;
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ts_a() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 3, 15, 10, 0, 0).unwrap()
}

fn ts_b() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 3, 15, 10, 30, 0).unwrap()
}

fn uid1() -> Uuid {
    Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()
}

fn uid2() -> Uuid {
    Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap()
}

fn mk_backend() -> BackendIdentity {
    BackendIdentity {
        id: "canonical-test".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn mk_meta() -> RunMetadata {
    RunMetadata {
        run_id: uid1(),
        work_order_id: uid2(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: ts_a(),
        finished_at: ts_b(),
        duration_ms: 1_800_000,
    }
}

fn mk_receipt() -> Receipt {
    Receipt {
        meta: mk_meta(),
        backend: mk_backend(),
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

fn mk_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: ts_a(),
        kind,
        ext: None,
    }
}

fn mk_work_order() -> WorkOrder {
    WorkOrder {
        id: uid1(),
        task: "canonical serde test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
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

fn keys_are_sorted(keys: &[String]) -> bool {
    keys.windows(2).all(|w| w[0] <= w[1])
}

// =========================================================================
// 1. Canonical JSON – identical output for identical inputs
// =========================================================================

#[test]
fn canonical_json_receipt_identical_calls() {
    let r = mk_receipt();
    let a = canonical_json(&r).unwrap();
    let b = canonical_json(&r).unwrap();
    assert_eq!(a, b, "canonical_json must be deterministic");
}

#[test]
fn canonical_json_work_order_identical_calls() {
    let wo = mk_work_order();
    let a = canonical_json(&wo).unwrap();
    let b = canonical_json(&wo).unwrap();
    assert_eq!(a, b);
}

#[test]
fn canonical_json_agent_event_identical_calls() {
    let e = mk_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let a = canonical_json(&e).unwrap();
    let b = canonical_json(&e).unwrap();
    assert_eq!(a, b);
}

#[test]
fn canonical_json_capability_manifest_identical_calls() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolBash, SupportLevel::Emulated);
    let a = canonical_json(&caps).unwrap();
    let b = canonical_json(&caps).unwrap();
    assert_eq!(a, b);
}

#[test]
fn canonical_json_policy_profile_identical_calls() {
    let p = PolicyProfile {
        allowed_tools: vec!["bash".into(), "read".into()],
        disallowed_tools: vec!["rm".into()],
        deny_read: vec!["/etc/shadow".into()],
        deny_write: vec!["/etc/**".into()],
        allow_network: vec!["example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["bash".into()],
    };
    let a = canonical_json(&p).unwrap();
    let b = canonical_json(&p).unwrap();
    assert_eq!(a, b);
}

#[test]
fn canonical_json_runtime_config_identical_calls() {
    let mut c = RuntimeConfig::default();
    c.model = Some("gpt-4".into());
    c.max_turns = Some(10);
    let a = canonical_json(&c).unwrap();
    let b = canonical_json(&c).unwrap();
    assert_eq!(a, b);
}

#[test]
fn canonical_json_context_packet_identical_calls() {
    let ctx = ContextPacket {
        files: vec!["main.rs".into(), "lib.rs".into()],
        snippets: vec![ContextSnippet {
            name: "readme".into(),
            content: "# Hello".into(),
        }],
    };
    let a = canonical_json(&ctx).unwrap();
    let b = canonical_json(&ctx).unwrap();
    assert_eq!(a, b);
}

#[test]
fn canonical_json_ir_message_identical_calls() {
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text {
            text: "hello".into(),
        }],
        metadata: BTreeMap::new(),
    };
    let a = canonical_json(&msg).unwrap();
    let b = canonical_json(&msg).unwrap();
    assert_eq!(a, b);
}

#[test]
fn canonical_json_ir_conversation_identical_calls() {
    let conv = IrConversation {
        messages: vec![IrMessage {
            role: IrRole::System,
            content: vec![IrContentBlock::Text {
                text: "You are helpful.".into(),
            }],
            metadata: BTreeMap::new(),
        }],
    };
    let a = canonical_json(&conv).unwrap();
    let b = canonical_json(&conv).unwrap();
    assert_eq!(a, b);
}

#[test]
fn canonical_json_ir_tool_definition_identical_calls() {
    let tool = IrToolDefinition {
        name: "grep".into(),
        description: "search files".into(),
        parameters: serde_json::json!({"type": "object"}),
    };
    let a = canonical_json(&tool).unwrap();
    let b = canonical_json(&tool).unwrap();
    assert_eq!(a, b);
}

#[test]
fn canonical_json_envelope_hello_identical_calls() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: mk_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let a = canonical_json(&env).unwrap();
    let b = canonical_json(&env).unwrap();
    assert_eq!(a, b);
}

// =========================================================================
// 2. BTreeMap key ordering – lexicographic
// =========================================================================

#[test]
fn btreemap_vendor_config_lexicographic() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert("zebra".into(), serde_json::json!(1));
    config.vendor.insert("apple".into(), serde_json::json!(2));
    config.vendor.insert("mango".into(), serde_json::json!(3));
    let v: Value = serde_json::to_value(&config).unwrap();
    assert_eq!(sorted_keys(&v["vendor"]), vec!["apple", "mango", "zebra"]);
}

#[test]
fn btreemap_env_config_lexicographic() {
    let mut config = RuntimeConfig::default();
    config.env.insert("Z_VAR".into(), "z".into());
    config.env.insert("A_VAR".into(), "a".into());
    config.env.insert("M_VAR".into(), "m".into());
    let v: Value = serde_json::to_value(&config).unwrap();
    assert_eq!(sorted_keys(&v["env"]), vec!["A_VAR", "M_VAR", "Z_VAR"]);
}

#[test]
fn btreemap_capability_manifest_lexicographic() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Vision, SupportLevel::Native);
    caps.insert(Capability::Audio, SupportLevel::Native);
    caps.insert(Capability::Logprobs, SupportLevel::Unsupported);
    caps.insert(Capability::Embeddings, SupportLevel::Emulated);
    let v: Value = serde_json::to_value(&caps).unwrap();
    let keys = sorted_keys(&v);
    assert!(keys_are_sorted(&keys), "keys not sorted: {keys:?}");
}

#[test]
fn btreemap_event_ext_lexicographic() {
    let mut ext = BTreeMap::new();
    ext.insert("z_raw".into(), serde_json::json!("z"));
    ext.insert("a_raw".into(), serde_json::json!("a"));
    ext.insert("m_raw".into(), serde_json::json!("m"));
    let ev = AgentEvent {
        ts: ts_a(),
        kind: AgentEventKind::RunStarted {
            message: "x".into(),
        },
        ext: Some(ext),
    };
    let v: Value = serde_json::to_value(&ev).unwrap();
    let keys = sorted_keys(&v["ext"]);
    assert_eq!(keys, vec!["a_raw", "m_raw", "z_raw"]);
}

#[test]
fn btreemap_ir_message_metadata_lexicographic() {
    let mut meta = BTreeMap::new();
    meta.insert("z_key".into(), serde_json::json!("z"));
    meta.insert("a_key".into(), serde_json::json!("a"));
    let msg = IrMessage {
        role: IrRole::Assistant,
        content: vec![],
        metadata: meta,
    };
    let v: Value = serde_json::to_value(&msg).unwrap();
    let keys = sorted_keys(&v["metadata"]);
    assert_eq!(keys, vec!["a_key", "z_key"]);
}

#[test]
fn btreemap_nested_vendor_keys_sorted() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "openai".into(),
        serde_json::json!({"z_param": 1, "a_param": 2}),
    );
    let v: Value = serde_json::to_value(&config).unwrap();
    // serde_json::Value::Object uses insertion order but serde_json::json!
    // macro preserves source order. The outer BTreeMap is sorted.
    let vendor_keys = sorted_keys(&v["vendor"]);
    assert_eq!(vendor_keys, vec!["openai"]);
}

#[test]
fn btreemap_single_entry_still_valid() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let v: Value = serde_json::to_value(&caps).unwrap();
    assert_eq!(sorted_keys(&v).len(), 1);
}

#[test]
fn btreemap_many_entries_stay_sorted() {
    let mut config = RuntimeConfig::default();
    for c in ('a'..='z').rev() {
        config.env.insert(format!("VAR_{c}"), format!("val_{c}"));
    }
    let v: Value = serde_json::to_value(&config).unwrap();
    let keys = sorted_keys(&v["env"]);
    assert!(keys_are_sorted(&keys));
    assert_eq!(keys.len(), 26);
}

// =========================================================================
// 3. Clone/serialize/deserialize cycle stability
// =========================================================================

#[test]
fn receipt_roundtrip_canonical_stable() {
    let r = mk_receipt();
    let json1 = canonical_json(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json1).unwrap();
    let json2 = canonical_json(&r2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn work_order_roundtrip_canonical_stable() {
    let wo = mk_work_order();
    let json1 = canonical_json(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json1).unwrap();
    let json2 = canonical_json(&wo2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn agent_event_run_started_roundtrip_stable() {
    let e = mk_event(AgentEventKind::RunStarted {
        message: "begin".into(),
    });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn agent_event_run_completed_roundtrip_stable() {
    let e = mk_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn agent_event_assistant_delta_roundtrip_stable() {
    let e = mk_event(AgentEventKind::AssistantDelta { text: "tok".into() });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn agent_event_assistant_message_roundtrip_stable() {
    let e = mk_event(AgentEventKind::AssistantMessage {
        text: "full msg".into(),
    });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn agent_event_tool_call_roundtrip_stable() {
    let e = mk_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("tu_1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"cmd": "ls"}),
    });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn agent_event_tool_result_roundtrip_stable() {
    let e = mk_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("tu_1".into()),
        output: serde_json::json!("output text"),
        is_error: false,
    });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn agent_event_file_changed_roundtrip_stable() {
    let e = mk_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "added function".into(),
    });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn agent_event_command_executed_roundtrip_stable() {
    let e = mk_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn agent_event_warning_roundtrip_stable() {
    let e = mk_event(AgentEventKind::Warning {
        message: "watch out".into(),
    });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn agent_event_error_roundtrip_stable() {
    let e = mk_event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn agent_event_error_no_code_roundtrip_stable() {
    let e = mk_event(AgentEventKind::Error {
        message: "unknown".into(),
        error_code: None,
    });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn policy_profile_roundtrip_stable() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["write".into()],
        deny_read: vec!["/secret".into()],
        deny_write: vec!["/etc".into()],
        allow_network: vec!["api.github.com".into()],
        deny_network: vec![],
        require_approval_for: vec!["bash".into()],
    };
    let j1 = canonical_json(&p).unwrap();
    let p2: PolicyProfile = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&p2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn capability_manifest_roundtrip_stable() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(
        Capability::McpClient,
        SupportLevel::Restricted {
            reason: "policy".into(),
        },
    );
    let j1 = canonical_json(&caps).unwrap();
    let caps2: CapabilityManifest = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&caps2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn runtime_config_roundtrip_stable() {
    let mut c = RuntimeConfig::default();
    c.model = Some("claude-3".into());
    c.max_budget_usd = Some(5.0);
    c.max_turns = Some(20);
    c.vendor
        .insert("anthropic".into(), serde_json::json!({"key": "val"}));
    c.env.insert("TOKEN".into(), "abc".into());
    let j1 = canonical_json(&c).unwrap();
    let c2: RuntimeConfig = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&c2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn ir_message_roundtrip_stable() {
    let mut meta = BTreeMap::new();
    meta.insert("source".into(), serde_json::json!("test"));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text {
            text: "hello world".into(),
        }],
        metadata: meta,
    };
    let j1 = canonical_json(&msg).unwrap();
    let msg2: IrMessage = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&msg2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn ir_conversation_roundtrip_stable() {
    let conv = IrConversation {
        messages: vec![
            IrMessage {
                role: IrRole::System,
                content: vec![IrContentBlock::Text {
                    text: "system".into(),
                }],
                metadata: BTreeMap::new(),
            },
            IrMessage {
                role: IrRole::User,
                content: vec![IrContentBlock::Text {
                    text: "user".into(),
                }],
                metadata: BTreeMap::new(),
            },
        ],
    };
    let j1 = canonical_json(&conv).unwrap();
    let conv2: IrConversation = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&conv2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn ir_tool_definition_roundtrip_stable() {
    let tool = IrToolDefinition {
        name: "search".into(),
        description: "full-text search".into(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    };
    let j1 = canonical_json(&tool).unwrap();
    let tool2: IrToolDefinition = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&tool2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn envelope_hello_roundtrip_stable() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: mk_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Passthrough,
    };
    let j1 = canonical_json(&env).unwrap();
    let env2: Envelope = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&env2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn envelope_run_roundtrip_stable() {
    let env = Envelope::Run {
        id: uid1().to_string(),
        work_order: mk_work_order(),
    };
    let j1 = canonical_json(&env).unwrap();
    let env2: Envelope = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&env2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn envelope_event_roundtrip_stable() {
    let env = Envelope::Event {
        ref_id: uid1().to_string(),
        event: mk_event(AgentEventKind::AssistantDelta { text: "hi".into() }),
    };
    let j1 = canonical_json(&env).unwrap();
    let env2: Envelope = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&env2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn envelope_final_roundtrip_stable() {
    let env = Envelope::Final {
        ref_id: uid1().to_string(),
        receipt: mk_receipt(),
    };
    let j1 = canonical_json(&env).unwrap();
    let env2: Envelope = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&env2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn envelope_fatal_roundtrip_stable() {
    let env = Envelope::Fatal {
        ref_id: Some(uid1().to_string()),
        error: "something broke".into(),
        error_code: Some(abp_error::ErrorCode::Internal),
    };
    let j1 = canonical_json(&env).unwrap();
    let env2: Envelope = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&env2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn clone_then_canonical_receipt() {
    let r = mk_receipt();
    let cloned = r.clone();
    assert_eq!(
        canonical_json(&r).unwrap(),
        canonical_json(&cloned).unwrap()
    );
}

#[test]
fn clone_then_canonical_work_order() {
    let wo = mk_work_order();
    let cloned = wo.clone();
    assert_eq!(
        canonical_json(&wo).unwrap(),
        canonical_json(&cloned).unwrap()
    );
}

// =========================================================================
// 4. All contract type serialization format tests
// =========================================================================

#[test]
fn execution_lane_serde_format() {
    assert_eq!(
        serde_json::to_value(ExecutionLane::PatchFirst).unwrap(),
        Value::String("patch_first".into())
    );
    assert_eq!(
        serde_json::to_value(ExecutionLane::WorkspaceFirst).unwrap(),
        Value::String("workspace_first".into())
    );
}

#[test]
fn workspace_mode_serde_format() {
    assert_eq!(
        serde_json::to_value(WorkspaceMode::PassThrough).unwrap(),
        Value::String("pass_through".into())
    );
    assert_eq!(
        serde_json::to_value(WorkspaceMode::Staged).unwrap(),
        Value::String("staged".into())
    );
}

#[test]
fn outcome_serde_format() {
    assert_eq!(
        serde_json::to_value(Outcome::Complete).unwrap(),
        Value::String("complete".into())
    );
    assert_eq!(
        serde_json::to_value(Outcome::Partial).unwrap(),
        Value::String("partial".into())
    );
    assert_eq!(
        serde_json::to_value(Outcome::Failed).unwrap(),
        Value::String("failed".into())
    );
}

#[test]
fn execution_mode_serde_format() {
    assert_eq!(
        serde_json::to_value(ExecutionMode::Passthrough).unwrap(),
        Value::String("passthrough".into())
    );
    assert_eq!(
        serde_json::to_value(ExecutionMode::Mapped).unwrap(),
        Value::String("mapped".into())
    );
}

#[test]
fn min_support_serde_format() {
    assert_eq!(
        serde_json::to_value(MinSupport::Native).unwrap(),
        Value::String("native".into())
    );
    assert_eq!(
        serde_json::to_value(MinSupport::Emulated).unwrap(),
        Value::String("emulated".into())
    );
}

#[test]
fn support_level_native_format() {
    assert_eq!(
        serde_json::to_value(SupportLevel::Native).unwrap(),
        Value::String("native".into())
    );
}

#[test]
fn support_level_emulated_format() {
    assert_eq!(
        serde_json::to_value(SupportLevel::Emulated).unwrap(),
        Value::String("emulated".into())
    );
}

#[test]
fn support_level_unsupported_format() {
    assert_eq!(
        serde_json::to_value(SupportLevel::Unsupported).unwrap(),
        Value::String("unsupported".into())
    );
}

#[test]
fn support_level_restricted_format() {
    let v = serde_json::to_value(SupportLevel::Restricted {
        reason: "policy block".into(),
    })
    .unwrap();
    assert_eq!(v["restricted"]["reason"], "policy block");
}

#[test]
fn ir_role_serde_format() {
    assert_eq!(
        serde_json::to_value(IrRole::System).unwrap(),
        Value::String("system".into())
    );
    assert_eq!(
        serde_json::to_value(IrRole::User).unwrap(),
        Value::String("user".into())
    );
    assert_eq!(
        serde_json::to_value(IrRole::Assistant).unwrap(),
        Value::String("assistant".into())
    );
    assert_eq!(
        serde_json::to_value(IrRole::Tool).unwrap(),
        Value::String("tool".into())
    );
}

#[test]
fn capability_serde_snake_case() {
    assert_eq!(
        serde_json::to_value(Capability::ToolBash).unwrap(),
        Value::String("tool_bash".into())
    );
    assert_eq!(
        serde_json::to_value(Capability::ExtendedThinking).unwrap(),
        Value::String("extended_thinking".into())
    );
    assert_eq!(
        serde_json::to_value(Capability::McpClient).unwrap(),
        Value::String("mcp_client".into())
    );
    assert_eq!(
        serde_json::to_value(Capability::ImageGeneration).unwrap(),
        Value::String("image_generation".into())
    );
}

#[test]
fn agent_event_kind_tag_field_is_type() {
    let e = mk_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "run_started");
}

#[test]
fn agent_event_kind_all_variants_tag() {
    let variants: Vec<(&str, AgentEventKind)> = vec![
        (
            "run_started",
            AgentEventKind::RunStarted { message: "".into() },
        ),
        (
            "run_completed",
            AgentEventKind::RunCompleted { message: "".into() },
        ),
        (
            "assistant_delta",
            AgentEventKind::AssistantDelta { text: "".into() },
        ),
        (
            "assistant_message",
            AgentEventKind::AssistantMessage { text: "".into() },
        ),
        (
            "tool_call",
            AgentEventKind::ToolCall {
                tool_name: "x".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!(null),
            },
        ),
        (
            "tool_result",
            AgentEventKind::ToolResult {
                tool_name: "x".into(),
                tool_use_id: None,
                output: serde_json::json!(null),
                is_error: false,
            },
        ),
        (
            "file_changed",
            AgentEventKind::FileChanged {
                path: "a".into(),
                summary: "b".into(),
            },
        ),
        (
            "command_executed",
            AgentEventKind::CommandExecuted {
                command: "ls".into(),
                exit_code: None,
                output_preview: None,
            },
        ),
        (
            "warning",
            AgentEventKind::Warning {
                message: "w".into(),
            },
        ),
        (
            "error",
            AgentEventKind::Error {
                message: "e".into(),
                error_code: None,
            },
        ),
    ];
    for (expected_tag, kind) in variants {
        let ev = mk_event(kind);
        let v: Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(
            v["type"].as_str().unwrap(),
            expected_tag,
            "tag mismatch for {expected_tag}"
        );
    }
}

#[test]
fn envelope_discriminator_is_t_field() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: mk_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "hello");
}

#[test]
fn envelope_all_variants_t_field() {
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: mk_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: mk_work_order(),
    };
    let event = Envelope::Event {
        ref_id: "r1".into(),
        event: mk_event(AgentEventKind::RunStarted {
            message: "x".into(),
        }),
    };
    let fin = Envelope::Final {
        ref_id: "r1".into(),
        receipt: mk_receipt(),
    };
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };

    for (env, expected) in [
        (&hello, "hello"),
        (&run, "run"),
        (&event, "event"),
        (&fin, "final"),
        (&fatal, "fatal"),
    ] {
        let v: Value = serde_json::to_value(env).unwrap();
        assert_eq!(v["t"].as_str().unwrap(), expected);
    }
}

#[test]
fn error_code_serde_snake_case() {
    let code = abp_error::ErrorCode::BackendTimeout;
    let v = serde_json::to_value(code).unwrap();
    assert_eq!(v, Value::String("backend_timeout".into()));
}

#[test]
fn error_code_as_str_matches_serde() {
    let codes = vec![
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
        abp_error::ErrorCode::BackendTimeout,
        abp_error::ErrorCode::MappingLossyConversion,
        abp_error::ErrorCode::ExecutionPermissionDenied,
        abp_error::ErrorCode::Internal,
        abp_error::ErrorCode::PolicyDenied,
        abp_error::ErrorCode::IrLoweringFailed,
        abp_error::ErrorCode::ReceiptHashMismatch,
    ];
    for code in codes {
        let serialized = serde_json::to_value(code).unwrap();
        assert_eq!(
            serialized.as_str().unwrap(),
            code.as_str(),
            "mismatch for {:?}",
            code
        );
    }
}

#[test]
fn error_code_in_agent_event_serialized_snake_case() {
    let e = mk_event(AgentEventKind::Error {
        message: "timeout".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["error_code"], "backend_timeout");
}

#[test]
fn error_code_none_skipped_in_event() {
    let e = mk_event(AgentEventKind::Error {
        message: "unknown".into(),
        error_code: None,
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(
        v.get("error_code").is_none(),
        "error_code should be skipped when None"
    );
}

// =========================================================================
// 5. Hash stability – same data → same hash
// =========================================================================

#[test]
fn receipt_hash_stable_across_calls() {
    let r = mk_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_stable_after_clone() {
    let r = mk_receipt();
    let cloned = r.clone();
    assert_eq!(receipt_hash(&r).unwrap(), receipt_hash(&cloned).unwrap());
}

#[test]
fn receipt_with_hash_sets_sha256() {
    let r = mk_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(!r.receipt_sha256.as_ref().unwrap().is_empty());
}

#[test]
fn receipt_with_hash_deterministic() {
    let h1 = mk_receipt().with_hash().unwrap().receipt_sha256.unwrap();
    let h2 = mk_receipt().with_hash().unwrap().receipt_sha256.unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_nullifies_sha256_before_hashing() {
    let mut r = mk_receipt();
    r.receipt_sha256 = Some("fake_hash".into());
    let h1 = receipt_hash(&r).unwrap();

    r.receipt_sha256 = None;
    let h2 = receipt_hash(&r).unwrap();

    assert_eq!(h1, h2, "hash must be independent of receipt_sha256 field");
}

#[test]
fn receipt_hash_changes_with_different_outcome() {
    let r1 = mk_receipt();
    let mut r2 = mk_receipt();
    r2.outcome = Outcome::Failed;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_different_backend_id() {
    let r1 = mk_receipt();
    let mut r2 = mk_receipt();
    r2.backend.id = "other-backend".into();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_trace_event() {
    let r1 = mk_receipt();
    let mut r2 = mk_receipt();
    r2.trace.push(mk_event(AgentEventKind::RunStarted {
        message: "trace event".into(),
    }));
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_artifact() {
    let r1 = mk_receipt();
    let mut r2 = mk_receipt();
    r2.artifacts.push(ArtifactRef {
        kind: "patch".into(),
        path: "out.patch".into(),
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
fn sha256_hex_lowercase() {
    let h = sha256_hex(b"hello");
    assert_eq!(h, h.to_lowercase(), "hash must be lowercase hex");
}

#[test]
fn sha256_hex_length_64() {
    let h = sha256_hex(b"anything");
    assert_eq!(h.len(), 64, "SHA-256 hex must be 64 chars");
}

#[test]
fn sha256_hex_differs_for_different_input() {
    let h1 = sha256_hex(b"alpha");
    let h2 = sha256_hex(b"beta");
    assert_ne!(h1, h2);
}

#[test]
fn receipt_hash_hex_is_lowercase() {
    let h = receipt_hash(&mk_receipt()).unwrap();
    assert_eq!(h, h.to_lowercase());
}

#[test]
fn receipt_hash_hex_is_64_chars() {
    let h = receipt_hash(&mk_receipt()).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_hash_stable_after_roundtrip() {
    let r = mk_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let json = canonical_json(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_builder_hash_matches_manual() {
    let r = ReceiptBuilder::new("hash-test")
        .work_order_id(uid2())
        .outcome(Outcome::Complete)
        .started_at(ts_a())
        .finished_at(ts_b())
        .build();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

// =========================================================================
// 6. Edge cases
// =========================================================================

#[test]
fn empty_capability_manifest_serializes() {
    let caps = CapabilityManifest::new();
    let j = canonical_json(&caps).unwrap();
    assert_eq!(j, "{}");
}

#[test]
fn empty_runtime_config_maps_serialize_as_empty() {
    let c = RuntimeConfig::default();
    let v: Value = serde_json::to_value(&c).unwrap();
    assert_eq!(v["vendor"], serde_json::json!({}));
    assert_eq!(v["env"], serde_json::json!({}));
}

#[test]
fn empty_policy_profile_all_vecs_empty() {
    let p = PolicyProfile::default();
    let v: Value = serde_json::to_value(&p).unwrap();
    assert_eq!(v["allowed_tools"], serde_json::json!([]));
    assert_eq!(v["disallowed_tools"], serde_json::json!([]));
    assert_eq!(v["deny_read"], serde_json::json!([]));
    assert_eq!(v["deny_write"], serde_json::json!([]));
}

#[test]
fn null_optional_fields_in_receipt() {
    let r = mk_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

#[test]
fn null_model_in_runtime_config() {
    let c = RuntimeConfig::default();
    let v: Value = serde_json::to_value(&c).unwrap();
    assert!(v["model"].is_null());
}

#[test]
fn null_max_budget_in_runtime_config() {
    let c = RuntimeConfig::default();
    let v: Value = serde_json::to_value(&c).unwrap();
    assert!(v["max_budget_usd"].is_null());
}

#[test]
fn null_max_turns_in_runtime_config() {
    let c = RuntimeConfig::default();
    let v: Value = serde_json::to_value(&c).unwrap();
    assert!(v["max_turns"].is_null());
}

#[test]
fn empty_trace_serializes_as_array() {
    let r = mk_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["trace"], serde_json::json!([]));
}

#[test]
fn empty_artifacts_serializes_as_array() {
    let r = mk_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["artifacts"], serde_json::json!([]));
}

#[test]
fn unicode_keys_in_vendor_config() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("日本語".into(), serde_json::json!("value"));
    config
        .vendor
        .insert("émoji".into(), serde_json::json!("🎉"));
    config
        .vendor
        .insert("ascii".into(), serde_json::json!("plain"));
    let j1 = canonical_json(&config).unwrap();
    let j2 = canonical_json(&config).unwrap();
    assert_eq!(j1, j2);
    // BTreeMap sorts by UTF-8 byte order
    let v: Value = serde_json::to_value(&config).unwrap();
    let keys = sorted_keys(&v["vendor"]);
    assert!(keys_are_sorted(&keys));
}

#[test]
fn unicode_keys_in_env_config() {
    let mut config = RuntimeConfig::default();
    config.env.insert("ÜBER".into(), "uber".into());
    config.env.insert("ALPHA".into(), "a".into());
    let v: Value = serde_json::to_value(&config).unwrap();
    let keys = sorted_keys(&v["env"]);
    assert!(keys_are_sorted(&keys));
}

#[test]
fn unicode_in_task_field() {
    let mut wo = mk_work_order();
    wo.task = "修正バグ 🐛".into();
    let j1 = canonical_json(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&j1).unwrap();
    assert_eq!(wo2.task, "修正バグ 🐛");
}

#[test]
fn unicode_in_agent_event_message() {
    let e = mk_event(AgentEventKind::Warning {
        message: "⚠️ 注意".into(),
    });
    let j = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn nested_json_in_tool_call_input() {
    let e = mk_event(AgentEventKind::ToolCall {
        tool_name: "edit".into(),
        tool_use_id: Some("tc_1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({
            "file": "main.rs",
            "changes": [{"line": 10, "text": "new line"}],
            "nested": {"deep": {"deeper": true}}
        }),
    });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn empty_context_packet_roundtrips() {
    let ctx = ContextPacket::default();
    let j1 = canonical_json(&ctx).unwrap();
    let ctx2: ContextPacket = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&ctx2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn empty_requirements_roundtrips() {
    let reqs = CapabilityRequirements::default();
    let j1 = canonical_json(&reqs).unwrap();
    let reqs2: CapabilityRequirements = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&reqs2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn verification_report_default_roundtrips() {
    let vr = VerificationReport::default();
    let j1 = canonical_json(&vr).unwrap();
    let vr2: VerificationReport = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&vr2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn verification_report_with_git_diff() {
    let vr = VerificationReport {
        git_diff: Some("diff --git a/f.rs\n+new line".into()),
        git_status: Some("M f.rs".into()),
        harness_ok: true,
    };
    let j1 = canonical_json(&vr).unwrap();
    let vr2: VerificationReport = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&vr2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn usage_normalized_default_all_none() {
    let u = UsageNormalized::default();
    let v: Value = serde_json::to_value(&u).unwrap();
    assert!(v["input_tokens"].is_null());
    assert!(v["output_tokens"].is_null());
    assert!(v["cache_read_tokens"].is_null());
    assert!(v["cache_write_tokens"].is_null());
    assert!(v["estimated_cost_usd"].is_null());
}

#[test]
fn usage_normalized_roundtrips() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(50),
        cache_write_tokens: Some(25),
        request_units: Some(1),
        estimated_cost_usd: Some(0.005),
    };
    let j1 = canonical_json(&u).unwrap();
    let u2: UsageNormalized = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&u2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn artifact_ref_roundtrips() {
    let a = ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    };
    let j1 = canonical_json(&a).unwrap();
    let a2: ArtifactRef = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&a2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn backend_identity_optional_versions() {
    let b1 = BackendIdentity {
        id: "test".into(),
        backend_version: None,
        adapter_version: None,
    };
    let j1 = canonical_json(&b1).unwrap();
    let b2: BackendIdentity = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&b2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn context_snippet_roundtrips() {
    let s = ContextSnippet {
        name: "example".into(),
        content: "fn main() {}".into(),
    };
    let j1 = canonical_json(&s).unwrap();
    let s2: ContextSnippet = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&s2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn capability_requirement_roundtrips() {
    let r = CapabilityRequirement {
        capability: Capability::ToolRead,
        min_support: MinSupport::Emulated,
    };
    let j1 = canonical_json(&r).unwrap();
    let r2: CapabilityRequirement = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&r2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn ir_content_block_text_roundtrips() {
    let b = IrContentBlock::Text {
        text: "hello".into(),
    };
    let j1 = canonical_json(&b).unwrap();
    let b2: IrContentBlock = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&b2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn ir_content_block_tool_use_roundtrips() {
    let b = IrContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "bash".into(),
        input: serde_json::json!({"cmd": "ls"}),
    };
    let j1 = canonical_json(&b).unwrap();
    let b2: IrContentBlock = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&b2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn ir_content_block_tool_result_roundtrips() {
    let b = IrContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: vec![IrContentBlock::Text {
            text: "output".into(),
        }],
        is_error: false,
    };
    let j1 = canonical_json(&b).unwrap();
    let b2: IrContentBlock = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&b2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn ir_content_block_thinking_roundtrips() {
    let b = IrContentBlock::Thinking {
        text: "reasoning...".into(),
    };
    let j1 = canonical_json(&b).unwrap();
    let b2: IrContentBlock = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&b2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn ir_content_block_image_roundtrips() {
    let b = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "base64data==".into(),
    };
    let j1 = canonical_json(&b).unwrap();
    let b2: IrContentBlock = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&b2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn ir_usage_roundtrips() {
    let u = IrUsage {
        input_tokens: 100,
        output_tokens: 200,
        total_tokens: 300,
        cache_read_tokens: 50,
        cache_write_tokens: 25,
    };
    let j1 = canonical_json(&u).unwrap();
    let u2: IrUsage = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&u2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn work_order_builder_canonical_stable() {
    let wo = WorkOrderBuilder::new("builder test")
        .root("/tmp")
        .lane(ExecutionLane::WorkspaceFirst)
        .model("gpt-4")
        .max_turns(5)
        .build();
    let j1 = canonical_json(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&wo2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn receipt_builder_canonical_stable() {
    let r = ReceiptBuilder::new("builder-test")
        .outcome(Outcome::Partial)
        .work_order_id(uid2())
        .started_at(ts_a())
        .finished_at(ts_b())
        .mode(ExecutionMode::Passthrough)
        .build();
    let j1 = canonical_json(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&r2).unwrap();
    assert_eq!(j1, j2);
}

// =========================================================================
// 7. UUID and datetime format
// =========================================================================

#[test]
fn uuid_serialized_as_lowercase_hyphenated() {
    let wo = mk_work_order();
    let v: Value = serde_json::to_value(&wo).unwrap();
    let id_str = v["id"].as_str().unwrap();
    assert_eq!(id_str, id_str.to_lowercase());
    assert!(id_str.contains('-'));
    assert_eq!(id_str.len(), 36);
}

#[test]
fn datetime_serialized_as_rfc3339() {
    let r = mk_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    let started = v["meta"]["started_at"].as_str().unwrap();
    // Must be parseable as RFC 3339
    chrono::DateTime::parse_from_rfc3339(started).expect("started_at should be RFC 3339");
}

#[test]
fn contract_version_in_receipt_meta() {
    let r = mk_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["meta"]["contract_version"], CONTRACT_VERSION);
}

// =========================================================================
// 8. Numeric edge cases
// =========================================================================

#[test]
fn zero_duration_ms_serializes() {
    let mut r = mk_receipt();
    r.meta.duration_ms = 0;
    let v: Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["meta"]["duration_ms"], 0);
}

#[test]
fn large_duration_ms_serializes() {
    let mut r = mk_receipt();
    r.meta.duration_ms = u64::MAX;
    let j1 = canonical_json(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&j1).unwrap();
    assert_eq!(r2.meta.duration_ms, u64::MAX);
}

#[test]
fn float_budget_serializes_precisely() {
    let mut c = RuntimeConfig::default();
    c.max_budget_usd = Some(0.001);
    let j = canonical_json(&c).unwrap();
    let c2: RuntimeConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(c2.max_budget_usd, Some(0.001));
}

#[test]
fn zero_budget_serializes() {
    let mut c = RuntimeConfig::default();
    c.max_budget_usd = Some(0.0);
    let j = canonical_json(&c).unwrap();
    let c2: RuntimeConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(c2.max_budget_usd, Some(0.0));
}

// =========================================================================
// 9. String escaping
// =========================================================================

#[test]
fn special_chars_in_task_preserved() {
    let mut wo = mk_work_order();
    wo.task = r#"fix "bug" in <html> & "quotes""#.into();
    let j = canonical_json(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&j).unwrap();
    assert_eq!(wo2.task, wo.task);
}

#[test]
fn newlines_in_context_snippet() {
    let s = ContextSnippet {
        name: "code".into(),
        content: "line1\nline2\nline3".into(),
    };
    let j = canonical_json(&s).unwrap();
    let s2: ContextSnippet = serde_json::from_str(&j).unwrap();
    assert_eq!(s2.content, s.content);
}

#[test]
fn tab_and_backslash_in_strings() {
    let e = mk_event(AgentEventKind::Warning {
        message: "path\\to\\file\ttab".into(),
    });
    let j = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j).unwrap();
    if let AgentEventKind::Warning { message } = &e2.kind {
        assert_eq!(message, "path\\to\\file\ttab");
    } else {
        panic!("expected Warning");
    }
}

#[test]
fn empty_string_fields_preserved() {
    let mut wo = mk_work_order();
    wo.task = String::new();
    let j = canonical_json(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&j).unwrap();
    assert_eq!(wo2.task, "");
}

// =========================================================================
// 10. Complex receipt with all fields populated
// =========================================================================

#[test]
fn full_receipt_roundtrip_and_hash() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);

    let r = Receipt {
        meta: mk_meta(),
        backend: BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("18.0.0".into()),
            adapter_version: Some("0.2.0".into()),
        },
        capabilities: caps,
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({"prompt_tokens": 100, "completion_tokens": 50}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.002),
        },
        trace: vec![
            mk_event(AgentEventKind::RunStarted {
                message: "starting".into(),
            }),
            mk_event(AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"command": "ls -la"}),
            }),
            mk_event(AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("tu_1".into()),
                output: serde_json::json!("file1.rs\nfile2.rs"),
                is_error: false,
            }),
            mk_event(AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added main function".into(),
            }),
            mk_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ],
        artifacts: vec![
            ArtifactRef {
                kind: "patch".into(),
                path: "output.patch".into(),
            },
            ArtifactRef {
                kind: "log".into(),
                path: "run.log".into(),
            },
        ],
        verification: VerificationReport {
            git_diff: Some("diff content".into()),
            git_status: Some("M src/main.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };

    // Roundtrip stability
    let j1 = canonical_json(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&r2).unwrap();
    assert_eq!(j1, j2);

    // Hash stability
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn full_work_order_roundtrip() {
    let wo = WorkOrder {
        id: uid1(),
        task: "full work order test with all fields".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/home/user/project".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec!["src/**".into(), "Cargo.toml".into()],
            exclude: vec!["target/**".into(), ".git/**".into()],
        },
        context: ContextPacket {
            files: vec!["README.md".into(), "CONTRIBUTING.md".into()],
            snippets: vec![
                ContextSnippet {
                    name: "instructions".into(),
                    content: "Follow the style guide.".into(),
                },
                ContextSnippet {
                    name: "constraints".into(),
                    content: "Do not modify tests.".into(),
                },
            ],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["bash".into(), "read".into(), "write".into()],
            disallowed_tools: vec!["rm".into()],
            deny_read: vec!["/etc/shadow".into()],
            deny_write: vec!["/etc/**".into()],
            allow_network: vec!["github.com".into()],
            deny_network: vec!["*.evil.com".into()],
            require_approval_for: vec!["bash".into()],
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
            ],
        },
        config: {
            let mut c = RuntimeConfig::default();
            c.model = Some("claude-3-opus".into());
            c.max_budget_usd = Some(10.0);
            c.max_turns = Some(50);
            c.vendor
                .insert("anthropic".into(), serde_json::json!({"thinking": true}));
            c.env.insert("API_KEY".into(), "secret".into());
            c
        },
    };

    let j1 = canonical_json(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&wo2).unwrap();
    assert_eq!(j1, j2);
}

// =========================================================================
// 11. Ext field flattening and skip behavior
// =========================================================================

#[test]
fn agent_event_ext_none_is_absent() {
    let e = AgentEvent {
        ts: ts_a(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("ext").is_none(), "ext should be absent when None");
}

#[test]
fn agent_event_ext_some_is_present() {
    let mut ext = BTreeMap::new();
    ext.insert("raw".into(), serde_json::json!("data"));
    let e = AgentEvent {
        ts: ts_a(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: Some(ext),
    };
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("ext").is_some());
}

#[test]
fn agent_event_kind_fields_are_flattened() {
    let e = mk_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    // Because AgentEventKind uses #[serde(flatten)], fields appear at top level
    assert_eq!(v["text"], "hello");
    assert_eq!(v["type"], "assistant_message");
}

// =========================================================================
// 12. Error code serialization across all categories
// =========================================================================

#[test]
fn error_code_protocol_category_snake_case() {
    let codes = vec![
        (
            abp_error::ErrorCode::ProtocolInvalidEnvelope,
            "protocol_invalid_envelope",
        ),
        (
            abp_error::ErrorCode::ProtocolHandshakeFailed,
            "protocol_handshake_failed",
        ),
        (
            abp_error::ErrorCode::ProtocolMissingRefId,
            "protocol_missing_ref_id",
        ),
        (
            abp_error::ErrorCode::ProtocolUnexpectedMessage,
            "protocol_unexpected_message",
        ),
        (
            abp_error::ErrorCode::ProtocolVersionMismatch,
            "protocol_version_mismatch",
        ),
    ];
    for (code, expected) in codes {
        assert_eq!(code.as_str(), expected);
        assert_eq!(
            serde_json::to_value(code).unwrap(),
            Value::String(expected.into())
        );
    }
}

#[test]
fn error_code_backend_category_snake_case() {
    let codes = vec![
        (abp_error::ErrorCode::BackendNotFound, "backend_not_found"),
        (
            abp_error::ErrorCode::BackendUnavailable,
            "backend_unavailable",
        ),
        (abp_error::ErrorCode::BackendTimeout, "backend_timeout"),
        (
            abp_error::ErrorCode::BackendRateLimited,
            "backend_rate_limited",
        ),
        (
            abp_error::ErrorCode::BackendAuthFailed,
            "backend_auth_failed",
        ),
        (
            abp_error::ErrorCode::BackendModelNotFound,
            "backend_model_not_found",
        ),
        (abp_error::ErrorCode::BackendCrashed, "backend_crashed"),
    ];
    for (code, expected) in codes {
        assert_eq!(code.as_str(), expected);
        assert_eq!(
            serde_json::to_value(code).unwrap(),
            Value::String(expected.into())
        );
    }
}

#[test]
fn error_code_mapping_category_snake_case() {
    let codes = vec![
        (
            abp_error::ErrorCode::MappingUnsupportedCapability,
            "mapping_unsupported_capability",
        ),
        (
            abp_error::ErrorCode::MappingDialectMismatch,
            "mapping_dialect_mismatch",
        ),
        (
            abp_error::ErrorCode::MappingLossyConversion,
            "mapping_lossy_conversion",
        ),
        (
            abp_error::ErrorCode::MappingUnmappableTool,
            "mapping_unmappable_tool",
        ),
    ];
    for (code, expected) in codes {
        assert_eq!(code.as_str(), expected);
    }
}

#[test]
fn error_code_execution_category_snake_case() {
    let codes = vec![
        (
            abp_error::ErrorCode::ExecutionToolFailed,
            "execution_tool_failed",
        ),
        (
            abp_error::ErrorCode::ExecutionWorkspaceError,
            "execution_workspace_error",
        ),
        (
            abp_error::ErrorCode::ExecutionPermissionDenied,
            "execution_permission_denied",
        ),
    ];
    for (code, expected) in codes {
        assert_eq!(code.as_str(), expected);
    }
}

#[test]
fn error_code_remaining_categories_snake_case() {
    let codes = vec![
        (
            abp_error::ErrorCode::ContractVersionMismatch,
            "contract_version_mismatch",
        ),
        (
            abp_error::ErrorCode::CapabilityUnsupported,
            "capability_unsupported",
        ),
        (abp_error::ErrorCode::PolicyDenied, "policy_denied"),
        (
            abp_error::ErrorCode::WorkspaceInitFailed,
            "workspace_init_failed",
        ),
        (abp_error::ErrorCode::IrLoweringFailed, "ir_lowering_failed"),
        (
            abp_error::ErrorCode::ReceiptHashMismatch,
            "receipt_hash_mismatch",
        ),
        (abp_error::ErrorCode::DialectUnknown, "dialect_unknown"),
        (abp_error::ErrorCode::ConfigInvalid, "config_invalid"),
        (abp_error::ErrorCode::Internal, "internal"),
    ];
    for (code, expected) in codes {
        assert_eq!(code.as_str(), expected);
    }
}

#[test]
fn error_code_in_fatal_envelope_snake_case() {
    let env = Envelope::Fatal {
        ref_id: Some("ref_1".into()),
        error: "timed out".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["error_code"], "backend_timeout");
}

#[test]
fn error_code_none_in_fatal_envelope_absent() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "unknown".into(),
        error_code: None,
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert!(v.get("error_code").is_none());
}

// =========================================================================
// 13. Multiple hash calls with complex data
// =========================================================================

#[test]
fn receipt_hash_100_calls_consistent() {
    let r = mk_receipt();
    let baseline = receipt_hash(&r).unwrap();
    for i in 0..100 {
        assert_eq!(
            receipt_hash(&r).unwrap(),
            baseline,
            "hash diverged on call {i}"
        );
    }
}

#[test]
fn sha256_hex_empty_input_stable() {
    let h1 = sha256_hex(b"");
    let h2 = sha256_hex(b"");
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

// =========================================================================
// 14. WorkspaceSpec edge cases
// =========================================================================

#[test]
fn workspace_spec_empty_globs_roundtrip() {
    let ws = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let j1 = canonical_json(&ws).unwrap();
    let ws2: WorkspaceSpec = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&ws2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn workspace_spec_many_globs_roundtrip() {
    let ws = WorkspaceSpec {
        root: "/project".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec!["**/*.rs".into(), "**/*.toml".into(), "**/*.md".into()],
        exclude: vec![
            "target/**".into(),
            ".git/**".into(),
            "node_modules/**".into(),
        ],
    };
    let j1 = canonical_json(&ws).unwrap();
    let ws2: WorkspaceSpec = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&ws2).unwrap();
    assert_eq!(j1, j2);
}

// =========================================================================
// 15. Execution mode default
// =========================================================================

#[test]
fn execution_mode_default_is_mapped() {
    let mode = ExecutionMode::default();
    assert_eq!(
        serde_json::to_value(&mode).unwrap(),
        Value::String("mapped".into())
    );
}

#[test]
fn execution_mode_in_receipt_default() {
    let r = mk_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["mode"], "mapped");
}

// =========================================================================
// 16. Canonical JSON byte-level equality
// =========================================================================

#[test]
fn canonical_json_bytes_equal_for_receipt() {
    let r = mk_receipt();
    let b1 = canonical_json(&r).unwrap().into_bytes();
    let b2 = canonical_json(&r).unwrap().into_bytes();
    assert_eq!(b1, b2);
}

#[test]
fn canonical_json_bytes_equal_for_work_order() {
    let wo = mk_work_order();
    let b1 = canonical_json(&wo).unwrap().into_bytes();
    let b2 = canonical_json(&wo).unwrap().into_bytes();
    assert_eq!(b1, b2);
}

#[test]
fn canonical_json_bytes_equal_for_event() {
    let e = mk_event(AgentEventKind::ToolCall {
        tool_name: "test".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({"nested": {"key": "value"}}),
    });
    let b1 = canonical_json(&e).unwrap().into_bytes();
    let b2 = canonical_json(&e).unwrap().into_bytes();
    assert_eq!(b1, b2);
}

// =========================================================================
// 17. Deeply nested structures
// =========================================================================

#[test]
fn deeply_nested_tool_input_canonical_stable() {
    let deep = serde_json::json!({
        "level1": {
            "level2": {
                "level3": {
                    "level4": {
                        "value": "deep"
                    }
                }
            }
        }
    });
    let e = mk_event(AgentEventKind::ToolCall {
        tool_name: "deep".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: deep,
    });
    let j1 = canonical_json(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&j1).unwrap();
    let j2 = canonical_json(&e2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn large_trace_receipt_hash_stable() {
    let mut r = mk_receipt();
    for i in 0..50 {
        r.trace.push(mk_event(AgentEventKind::AssistantDelta {
            text: format!("token_{i}"),
        }));
    }
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_with_all_event_types_hash_stable() {
    let mut r = mk_receipt();
    r.trace = vec![
        mk_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        mk_event(AgentEventKind::AssistantDelta { text: "tok".into() }),
        mk_event(AgentEventKind::AssistantMessage {
            text: "full".into(),
        }),
        mk_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tc1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        }),
        mk_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tc1".into()),
            output: serde_json::json!("ok"),
            is_error: false,
        }),
        mk_event(AgentEventKind::FileChanged {
            path: "f.rs".into(),
            summary: "edit".into(),
        }),
        mk_event(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: None,
        }),
        mk_event(AgentEventKind::Warning {
            message: "warn".into(),
        }),
        mk_event(AgentEventKind::Error {
            message: "err".into(),
            error_code: Some(abp_error::ErrorCode::Internal),
        }),
        mk_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}
