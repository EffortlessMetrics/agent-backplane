// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deterministic serialization tests for all ABP contract types.
//!
//! ABP uses `BTreeMap` throughout to guarantee sorted keys, making JSON output
//! canonical and reproducible. These tests verify that property holds across
//! all serializable types, round-trips, and hash computations.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

// ───────────────────── helpers ─────────────────────

/// Fixed UUID for deterministic tests.
fn fixed_uuid() -> Uuid {
    Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
}

/// Another fixed UUID.
fn fixed_uuid2() -> Uuid {
    Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap()
}

/// Fixed timestamp for deterministic tests.
fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

/// Fixed later timestamp.
fn fixed_ts_end() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 42).unwrap()
}

/// Serialize, deserialize, re-serialize, and assert byte-identical output.
fn assert_roundtrip_deterministic<T>(value: &T)
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    let json1 = serde_json::to_string(value).expect("first serialize");
    let deserialized: T = serde_json::from_str(&json1).expect("deserialize");
    let json2 = serde_json::to_string(&deserialized).expect("second serialize");
    assert_eq!(json1, json2, "round-trip produced different JSON");
}

/// Serialize twice independently and assert identical output.
fn assert_serialize_stable<T: Serialize>(value: &T) {
    let json1 = serde_json::to_string(value).expect("serialize 1");
    let json2 = serde_json::to_string(value).expect("serialize 2");
    assert_eq!(
        json1, json2,
        "repeated serialization produced different JSON"
    );
}

/// SHA-256 hex digest of a byte slice.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Build a minimal receipt with deterministic fields.
fn make_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_ts(),
            finished_at: fixed_ts_end(),
            duration_ms: 42000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
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

/// Build a minimal work order with deterministic fields.
fn make_work_order() -> WorkOrder {
    WorkOrder {
        id: fixed_uuid(),
        task: "Test task".into(),
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

/// Build an AgentEvent with a fixed timestamp.
fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: fixed_ts(),
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════
// 1. BTreeMap key ordering
// ═══════════════════════════════════════════════════

#[test]
fn btreemap_keys_serialize_in_sorted_order() {
    let mut map = BTreeMap::new();
    map.insert("zebra".to_string(), serde_json::json!(1));
    map.insert("apple".to_string(), serde_json::json!(2));
    map.insert("mango".to_string(), serde_json::json!(3));
    let json = serde_json::to_string(&map).unwrap();
    assert!(json.starts_with(r#"{"apple":2"#));
}

#[test]
fn runtime_config_vendor_keys_sorted() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("zoo".into(), serde_json::json!("last"));
    config
        .vendor
        .insert("alpha".into(), serde_json::json!("first"));
    config
        .vendor
        .insert("middle".into(), serde_json::json!("mid"));
    let json = serde_json::to_string(&config).unwrap();
    let alpha_pos = json.find("\"alpha\"").unwrap();
    let middle_pos = json.find("\"middle\"").unwrap();
    let zoo_pos = json.find("\"zoo\"").unwrap();
    assert!(alpha_pos < middle_pos);
    assert!(middle_pos < zoo_pos);
}

#[test]
fn runtime_config_env_keys_sorted() {
    let mut config = RuntimeConfig::default();
    config.env.insert("ZZVAR".into(), "last".into());
    config.env.insert("AAVAR".into(), "first".into());
    let json = serde_json::to_string(&config).unwrap();
    let a_pos = json.find("\"AAVAR\"").unwrap();
    let z_pos = json.find("\"ZZVAR\"").unwrap();
    assert!(a_pos < z_pos);
}

#[test]
fn capability_manifest_keys_sorted() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolBash, SupportLevel::Emulated);
    let json = serde_json::to_string(&caps).unwrap();
    // BTreeMap uses Ord on Capability (derived = declaration order).
    // Verify keys appear in a consistent order across serializations.
    let json2 = serde_json::to_string(&caps).unwrap();
    assert_eq!(
        json, json2,
        "capability manifest serialization must be stable"
    );
}

#[test]
fn nested_btreemap_in_vendor_config_sorted() {
    let mut inner = serde_json::Map::new();
    inner.insert("z_key".into(), serde_json::json!(1));
    inner.insert("a_key".into(), serde_json::json!(2));
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("nested".into(), serde_json::Value::Object(inner));
    let json = serde_json::to_string(&config).unwrap();
    let a_pos = json.find("\"a_key\"").unwrap();
    let z_pos = json.find("\"z_key\"").unwrap();
    assert!(a_pos < z_pos);
}

#[test]
fn agent_event_ext_keys_sorted() {
    let mut ext = BTreeMap::new();
    ext.insert("z_field".into(), serde_json::json!("last"));
    ext.insert("a_field".into(), serde_json::json!("first"));
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&event).unwrap();
    let a_pos = json.find("\"a_field\"").unwrap();
    let z_pos = json.find("\"z_field\"").unwrap();
    assert!(a_pos < z_pos);
}

// ═══════════════════════════════════════════════════
// 2. Canonical JSON — same data → same bytes
// ═══════════════════════════════════════════════════

#[test]
fn canonical_json_produces_identical_output() {
    let val = serde_json::json!({"b": 2, "a": 1, "c": [3, 2, 1]});
    let json1 = abp_core::canonical_json(&val).unwrap();
    let json2 = abp_core::canonical_json(&val).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn canonical_json_sorts_keys() {
    let val = serde_json::json!({"z": 1, "a": 2, "m": 3});
    let json = abp_core::canonical_json(&val).unwrap();
    assert!(json.starts_with(r#"{"a":2"#));
}

#[test]
fn canonical_json_of_work_order_is_stable() {
    let wo = make_work_order();
    let json1 = abp_core::canonical_json(&wo).unwrap();
    let json2 = abp_core::canonical_json(&wo).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn canonical_json_of_receipt_is_stable() {
    let r = make_receipt();
    let json1 = abp_core::canonical_json(&r).unwrap();
    let json2 = abp_core::canonical_json(&r).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn canonical_json_of_agent_event_is_stable() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let json1 = abp_core::canonical_json(&e).unwrap();
    let json2 = abp_core::canonical_json(&e).unwrap();
    assert_eq!(json1, json2);
}

// ═══════════════════════════════════════════════════
// 3. Receipt hashing stability
// ═══════════════════════════════════════════════════

#[test]
fn receipt_hash_is_deterministic() {
    let r = make_receipt();
    let h1 = abp_core::receipt_hash(&r).unwrap();
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_is_64_hex_chars() {
    let r = make_receipt();
    let h = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_with_hash_produces_stable_hash() {
    let r1 = make_receipt().with_hash().unwrap();
    let r2 = make_receipt().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn receipt_hash_ignores_stored_hash_field() {
    let r1 = make_receipt();
    let mut r2 = make_receipt();
    r2.receipt_sha256 = Some("garbage".into());
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2, "receipt_sha256 field must not affect hash");
}

#[test]
fn receipt_hash_changes_with_different_outcome() {
    let mut r1 = make_receipt();
    r1.outcome = Outcome::Complete;
    let mut r2 = make_receipt();
    r2.outcome = Outcome::Failed;
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn receipt_hash_changes_with_different_backend() {
    let r1 = make_receipt();
    let mut r2 = make_receipt();
    r2.backend.id = "other".into();
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn receipt_hash_changes_with_different_trace() {
    let r1 = make_receipt();
    let mut r2 = make_receipt();
    r2.trace.push(make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn receipt_hash_changes_with_different_duration() {
    let r1 = make_receipt();
    let mut r2 = make_receipt();
    r2.meta.duration_ms = 99999;
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

// ═══════════════════════════════════════════════════
// 4. WorkOrder canonical form
// ═══════════════════════════════════════════════════

#[test]
fn work_order_roundtrip_deterministic() {
    let wo = make_work_order();
    assert_roundtrip_deterministic(&wo);
}

#[test]
fn work_order_serialize_stable() {
    let wo = make_work_order();
    assert_serialize_stable(&wo);
}

#[test]
fn work_order_with_policy_roundtrip() {
    let mut wo = make_work_order();
    wo.policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/etc/**".into()],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["deploy".into()],
    };
    assert_roundtrip_deterministic(&wo);
}

#[test]
fn work_order_with_context_roundtrip() {
    let mut wo = make_work_order();
    wo.context = ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![ContextSnippet {
            name: "test".into(),
            content: "fn main() {}".into(),
        }],
    };
    assert_roundtrip_deterministic(&wo);
}

#[test]
fn work_order_with_requirements_roundtrip() {
    let mut wo = make_work_order();
    wo.requirements = CapabilityRequirements {
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
    };
    assert_roundtrip_deterministic(&wo);
}

#[test]
fn work_order_canonical_json_equals_direct_serialize() {
    let wo = make_work_order();
    let direct = serde_json::to_string(&wo).unwrap();
    let canonical = abp_core::canonical_json(&wo).unwrap();
    // Both should produce compact JSON; serde_json::to_string is already compact.
    // canonical_json goes through Value, so key order should be identical for BTreeMap types.
    let direct_val: serde_json::Value = serde_json::from_str(&direct).unwrap();
    let canonical_val: serde_json::Value = serde_json::from_str(&canonical).unwrap();
    assert_eq!(direct_val, canonical_val);
}

// ═══════════════════════════════════════════════════
// 5. Config canonical form
// ═══════════════════════════════════════════════════

#[test]
fn runtime_config_default_roundtrip() {
    let cfg = RuntimeConfig::default();
    assert_roundtrip_deterministic(&cfg);
}

#[test]
fn runtime_config_with_all_fields_roundtrip() {
    let mut cfg = RuntimeConfig::default();
    cfg.model = Some("gpt-4".into());
    cfg.max_budget_usd = Some(10.5);
    cfg.max_turns = Some(25);
    cfg.vendor
        .insert("temperature".into(), serde_json::json!(0.7));
    cfg.env.insert("API_KEY".into(), "test".into());
    assert_roundtrip_deterministic(&cfg);
}

#[test]
fn runtime_config_vendor_order_irrelevant() {
    let mut cfg1 = RuntimeConfig::default();
    cfg1.vendor.insert("a".into(), serde_json::json!(1));
    cfg1.vendor.insert("b".into(), serde_json::json!(2));

    let mut cfg2 = RuntimeConfig::default();
    cfg2.vendor.insert("b".into(), serde_json::json!(2));
    cfg2.vendor.insert("a".into(), serde_json::json!(1));

    let json1 = serde_json::to_string(&cfg1).unwrap();
    let json2 = serde_json::to_string(&cfg2).unwrap();
    assert_eq!(
        json1, json2,
        "insertion order must not affect serialization"
    );
}

#[test]
fn runtime_config_env_order_irrelevant() {
    let mut cfg1 = RuntimeConfig::default();
    cfg1.env.insert("B".into(), "2".into());
    cfg1.env.insert("A".into(), "1".into());

    let mut cfg2 = RuntimeConfig::default();
    cfg2.env.insert("A".into(), "1".into());
    cfg2.env.insert("B".into(), "2".into());

    let json1 = serde_json::to_string(&cfg1).unwrap();
    let json2 = serde_json::to_string(&cfg2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn runtime_config_canonical_json_stable() {
    let mut cfg = RuntimeConfig::default();
    cfg.vendor.insert("key".into(), serde_json::json!("val"));
    let json1 = abp_core::canonical_json(&cfg).unwrap();
    let json2 = abp_core::canonical_json(&cfg).unwrap();
    assert_eq!(json1, json2);
}

// ═══════════════════════════════════════════════════
// 6. Event canonical form
// ═══════════════════════════════════════════════════

#[test]
fn event_run_started_roundtrip() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "starting".into(),
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn event_run_completed_roundtrip() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn event_assistant_delta_roundtrip() {
    let e = make_event(AgentEventKind::AssistantDelta { text: "tok".into() });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn event_assistant_message_roundtrip() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "full msg".into(),
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn event_tool_call_roundtrip() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("tu-1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "/tmp/test.rs"}),
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn event_tool_result_roundtrip() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: Some("tu-1".into()),
        output: serde_json::json!({"content": "hello"}),
        is_error: false,
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn event_file_changed_roundtrip() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "added function".into(),
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn event_command_executed_roundtrip() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn event_warning_roundtrip() {
    let e = make_event(AgentEventKind::Warning {
        message: "heads up".into(),
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn event_error_roundtrip() {
    let e = make_event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn event_error_with_code_roundtrip() {
    let e = make_event(AgentEventKind::Error {
        message: "timeout".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn event_serialize_stable() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({"command": "ls -la"}),
    });
    assert_serialize_stable(&e);
}

#[test]
fn event_canonical_json_stable() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let j1 = abp_core::canonical_json(&e).unwrap();
    let j2 = abp_core::canonical_json(&e).unwrap();
    assert_eq!(j1, j2);
}

// ═══════════════════════════════════════════════════
// 7. Cross-platform consistency (JSON output identical)
// ═══════════════════════════════════════════════════

#[test]
fn cross_platform_receipt_json_is_compact() {
    let r = make_receipt();
    let json = serde_json::to_string(&r).unwrap();
    // Compact JSON has no unnecessary whitespace
    assert!(!json.contains("  "));
    assert!(!json.contains('\n'));
}

#[test]
fn cross_platform_work_order_json_is_compact() {
    let wo = make_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(!json.contains('\n'));
}

#[test]
fn cross_platform_canonical_json_matches_hash_input() {
    let r = make_receipt();
    // The hash should be based on canonical (compact, sorted) JSON.
    let mut v = serde_json::to_value(&r).unwrap();
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("receipt_sha256".into(), serde_json::Value::Null);
    }
    let canonical = serde_json::to_string(&v).unwrap();
    let expected_hash = sha256_hex(canonical.as_bytes());
    let actual_hash = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(expected_hash, actual_hash);
}

#[test]
fn cross_platform_enum_serialization_uses_snake_case() {
    let json = serde_json::to_string(&ExecutionLane::PatchFirst).unwrap();
    assert_eq!(json, "\"patch_first\"");

    let json = serde_json::to_string(&WorkspaceMode::PassThrough).unwrap();
    assert_eq!(json, "\"pass_through\"");

    let json = serde_json::to_string(&Outcome::Complete).unwrap();
    assert_eq!(json, "\"complete\"");

    let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(json, "\"passthrough\"");

    let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
    assert_eq!(json, "\"mapped\"");
}

#[test]
fn cross_platform_capability_serialization() {
    let json = serde_json::to_string(&Capability::Streaming).unwrap();
    assert_eq!(json, "\"streaming\"");

    let json = serde_json::to_string(&Capability::ToolRead).unwrap();
    assert_eq!(json, "\"tool_read\"");

    let json = serde_json::to_string(&Capability::ExtendedThinking).unwrap();
    assert_eq!(json, "\"extended_thinking\"");
}

#[test]
fn cross_platform_min_support_serialization() {
    let json = serde_json::to_string(&MinSupport::Native).unwrap();
    assert_eq!(json, "\"native\"");

    let json = serde_json::to_string(&MinSupport::Emulated).unwrap();
    assert_eq!(json, "\"emulated\"");
}

// ═══════════════════════════════════════════════════
// 8. Floating point handling
// ═══════════════════════════════════════════════════

#[test]
fn float_serializes_consistently() {
    let cfg = RuntimeConfig {
        max_budget_usd: Some(1.5),
        ..Default::default()
    };
    let json1 = serde_json::to_string(&cfg).unwrap();
    let json2 = serde_json::to_string(&cfg).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn float_roundtrip_preserves_value() {
    let cfg = RuntimeConfig {
        max_budget_usd: Some(0.1),
        ..Default::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let cfg2: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg.max_budget_usd, cfg2.max_budget_usd);
}

#[test]
fn float_zero_serializes_deterministically() {
    let cfg = RuntimeConfig {
        max_budget_usd: Some(0.0),
        ..Default::default()
    };
    assert_roundtrip_deterministic(&cfg);
}

#[test]
fn float_large_value_deterministic() {
    let cfg = RuntimeConfig {
        max_budget_usd: Some(999999.99),
        ..Default::default()
    };
    assert_roundtrip_deterministic(&cfg);
}

#[test]
fn usage_float_roundtrip() {
    let usage = UsageNormalized {
        estimated_cost_usd: Some(0.0023),
        input_tokens: Some(1500),
        output_tokens: Some(500),
        ..Default::default()
    };
    assert_roundtrip_deterministic(&usage);
}

#[test]
fn float_in_vendor_config_roundtrip() {
    let mut cfg = RuntimeConfig::default();
    cfg.vendor
        .insert("temperature".into(), serde_json::json!(0.7));
    cfg.vendor.insert("top_p".into(), serde_json::json!(0.95));
    assert_roundtrip_deterministic(&cfg);
}

// ═══════════════════════════════════════════════════
// 9. Unicode normalization
// ═══════════════════════════════════════════════════

#[test]
fn unicode_ascii_roundtrip() {
    let wo = WorkOrder {
        task: "Hello world".into(),
        ..make_work_order()
    };
    assert_roundtrip_deterministic(&wo);
}

#[test]
fn unicode_emoji_roundtrip() {
    let wo = WorkOrder {
        task: "Fix the 🐛 bug".into(),
        ..make_work_order()
    };
    assert_roundtrip_deterministic(&wo);
}

#[test]
fn unicode_cjk_roundtrip() {
    let wo = WorkOrder {
        task: "修復認證模組".into(),
        ..make_work_order()
    };
    assert_roundtrip_deterministic(&wo);
}

#[test]
fn unicode_combining_chars_roundtrip() {
    // e followed by combining acute accent
    let wo = WorkOrder {
        task: "caf\u{0065}\u{0301}".into(),
        ..make_work_order()
    };
    assert_roundtrip_deterministic(&wo);
}

#[test]
fn unicode_escape_sequences_stable() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "line1\nline2\ttab".into(),
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn unicode_null_byte_in_string() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "before\0after".into(),
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn unicode_bidi_markers_roundtrip() {
    let wo = WorkOrder {
        task: "Hello \u{200F}world\u{200E}".into(),
        ..make_work_order()
    };
    assert_roundtrip_deterministic(&wo);
}

// ═══════════════════════════════════════════════════
// 10. Null vs missing — Option<T> with skip_serializing_if
// ═══════════════════════════════════════════════════

#[test]
fn optional_none_model_serializes_as_null() {
    let cfg = RuntimeConfig {
        model: None,
        ..Default::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    // RuntimeConfig does NOT use skip_serializing_if, so None → null
    assert!(json.contains("\"model\":null"));
}

#[test]
fn optional_some_model_serializes_as_value() {
    let cfg = RuntimeConfig {
        model: Some("gpt-4".into()),
        ..Default::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("\"model\":\"gpt-4\""));
}

#[test]
fn receipt_sha256_none_serializes_as_null() {
    let r = make_receipt();
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
}

#[test]
fn receipt_sha256_some_serializes_as_string() {
    let r = make_receipt().with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":\""));
    assert!(!json.contains("\"receipt_sha256\":null"));
}

#[test]
fn agent_event_ext_none_is_omitted() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    // ext uses skip_serializing_if = "Option::is_none", so field should be absent
    assert!(!json.contains("\"ext\""));
}

#[test]
fn agent_event_ext_some_is_present() {
    let mut ext = BTreeMap::new();
    ext.insert("key".into(), serde_json::json!("val"));
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("\"key\":\"val\""));
}

#[test]
fn error_code_none_is_omitted() {
    let e = make_event(AgentEventKind::Error {
        message: "fail".into(),
        error_code: None,
    });
    let json = serde_json::to_string(&e).unwrap();
    assert!(!json.contains("\"error_code\""));
}

#[test]
fn error_code_some_is_present() {
    let e = make_event(AgentEventKind::Error {
        message: "fail".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("\"error_code\":\"backend_timeout\""));
}

#[test]
fn verification_report_optional_none_git_diff() {
    let v = VerificationReport {
        git_diff: None,
        git_status: None,
        harness_ok: false,
    };
    let json = serde_json::to_string(&v).unwrap();
    assert!(json.contains("\"git_diff\":null"));
}

#[test]
fn usage_optional_none_fields_are_null() {
    let u = UsageNormalized::default();
    let json = serde_json::to_string(&u).unwrap();
    assert!(json.contains("\"input_tokens\":null"));
    assert!(json.contains("\"output_tokens\":null"));
}

// ═══════════════════════════════════════════════════
// 11. Empty collections
// ═══════════════════════════════════════════════════

#[test]
fn empty_vec_serializes_consistently() {
    let policy = PolicyProfile::default();
    let json1 = serde_json::to_string(&policy).unwrap();
    let json2 = serde_json::to_string(&policy).unwrap();
    assert_eq!(json1, json2);
    assert!(json1.contains("\"allowed_tools\":[]"));
}

#[test]
fn empty_btreemap_serializes_consistently() {
    let cfg = RuntimeConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("\"vendor\":{}"));
    assert!(json.contains("\"env\":{}"));
}

#[test]
fn empty_capability_manifest_stable() {
    let caps = CapabilityManifest::new();
    let json1 = serde_json::to_string(&caps).unwrap();
    let json2 = serde_json::to_string(&caps).unwrap();
    assert_eq!(json1, json2);
    assert_eq!(json1, "{}");
}

#[test]
fn empty_trace_in_receipt_stable() {
    let r = make_receipt();
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("\"trace\":[]"));
}

#[test]
fn empty_artifacts_in_receipt_stable() {
    let r = make_receipt();
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("\"artifacts\":[]"));
}

#[test]
fn empty_context_packet_stable() {
    let ctx = ContextPacket::default();
    assert_roundtrip_deterministic(&ctx);
    let json = serde_json::to_string(&ctx).unwrap();
    assert!(json.contains("\"files\":[]"));
    assert!(json.contains("\"snippets\":[]"));
}

#[test]
fn empty_requirements_stable() {
    let reqs = CapabilityRequirements::default();
    assert_roundtrip_deterministic(&reqs);
    let json = serde_json::to_string(&reqs).unwrap();
    assert!(json.contains("\"required\":[]"));
}

// ═══════════════════════════════════════════════════
// 12. Nested determinism
// ═══════════════════════════════════════════════════

#[test]
fn deeply_nested_work_order_deterministic() {
    let mut wo = make_work_order();
    wo.config.model = Some("gpt-4".into());
    wo.config.max_turns = Some(10);
    wo.config.max_budget_usd = Some(5.0);
    wo.config.vendor.insert(
        "abp".into(),
        serde_json::json!({"mode": "passthrough", "debug": true}),
    );
    wo.config
        .env
        .insert("OPENAI_API_KEY".into(), "sk-test".into());
    wo.policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.key".into()],
        deny_write: vec!["/etc/**".into()],
        allow_network: vec!["api.openai.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["deploy".into()],
    };
    wo.context = ContextPacket {
        files: vec!["src/main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "use BTreeMap".into(),
        }],
    };
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    assert_roundtrip_deterministic(&wo);
}

#[test]
fn deeply_nested_receipt_deterministic() {
    let mut r = make_receipt();
    r.trace = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/lib.rs", "opts": {"encoding": "utf-8"}}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!({"content": "fn main() {}", "lines": 1}),
            is_error: false,
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    r.artifacts = vec![ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    }];
    r.usage = UsageNormalized {
        input_tokens: Some(1500),
        output_tokens: Some(500),
        cache_read_tokens: Some(100),
        cache_write_tokens: Some(50),
        request_units: None,
        estimated_cost_usd: Some(0.003),
    };
    r.verification = VerificationReport {
        git_diff: Some("+fn new_func() {}".into()),
        git_status: Some("M src/lib.rs".into()),
        harness_ok: true,
    };
    assert_roundtrip_deterministic(&r);
}

#[test]
fn nested_json_value_in_tool_call_deterministic() {
    let deep_json = serde_json::json!({
        "level1": {
            "level2": {
                "level3": {
                    "key": "value",
                    "array": [1, 2, 3]
                }
            }
        }
    });
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "complex".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: deep_json,
    });
    assert_roundtrip_deterministic(&e);
}

#[test]
fn nested_vendor_config_with_arrays_deterministic() {
    let mut cfg = RuntimeConfig::default();
    cfg.vendor.insert(
        "config".into(),
        serde_json::json!({
            "stop_sequences": ["END", "DONE"],
            "params": {"temp": 0.5, "top_k": 40}
        }),
    );
    assert_roundtrip_deterministic(&cfg);
}

#[test]
fn receipt_with_all_event_kinds_deterministic() {
    let mut r = make_receipt();
    r.trace = vec![
        make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_event(AgentEventKind::AssistantDelta {
            text: "tok1".into(),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "full".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            output: serde_json::json!("ok"),
            is_error: false,
        }),
        make_event(AgentEventKind::FileChanged {
            path: "f.rs".into(),
            summary: "changed".into(),
        }),
        make_event(AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: Some(0),
            output_preview: None,
        }),
        make_event(AgentEventKind::Warning {
            message: "warn".into(),
        }),
        make_event(AgentEventKind::Error {
            message: "err".into(),
            error_code: None,
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    assert_roundtrip_deterministic(&r);
}

// ═══════════════════════════════════════════════════
// 13. Hash stability — SHA-256 of canonical JSON
// ═══════════════════════════════════════════════════

#[test]
fn sha256_hex_deterministic() {
    let h1 = abp_core::sha256_hex(b"hello world");
    let h2 = abp_core::sha256_hex(b"hello world");
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn sha256_hex_different_input_different_hash() {
    let h1 = abp_core::sha256_hex(b"hello");
    let h2 = abp_core::sha256_hex(b"world");
    assert_ne!(h1, h2);
}

#[test]
fn receipt_json_hash_stable_across_serializations() {
    let r = make_receipt();
    let json1 = abp_core::canonical_json(&r).unwrap();
    let json2 = abp_core::canonical_json(&r).unwrap();
    let h1 = sha256_hex(json1.as_bytes());
    let h2 = sha256_hex(json2.as_bytes());
    assert_eq!(h1, h2);
}

#[test]
fn work_order_json_hash_stable() {
    let wo = make_work_order();
    let json1 = abp_core::canonical_json(&wo).unwrap();
    let json2 = abp_core::canonical_json(&wo).unwrap();
    assert_eq!(sha256_hex(json1.as_bytes()), sha256_hex(json2.as_bytes()));
}

#[test]
fn event_json_hash_stable() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "test".into(),
    });
    let json1 = abp_core::canonical_json(&e).unwrap();
    let json2 = abp_core::canonical_json(&e).unwrap();
    assert_eq!(sha256_hex(json1.as_bytes()), sha256_hex(json2.as_bytes()));
}

#[test]
fn config_json_hash_stable() {
    let mut cfg = RuntimeConfig::default();
    cfg.vendor.insert("key".into(), serde_json::json!("val"));
    let json1 = abp_core::canonical_json(&cfg).unwrap();
    let json2 = abp_core::canonical_json(&cfg).unwrap();
    assert_eq!(sha256_hex(json1.as_bytes()), sha256_hex(json2.as_bytes()));
}

#[test]
fn receipt_hash_via_builder_stable() {
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts)
        .finished_at(ts)
        .work_order_id(fixed_uuid())
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts)
        .finished_at(ts)
        .work_order_id(fixed_uuid())
        .build();
    // Note: run_id is random from builder, so we fix it.
    let mut r1_fixed = r1;
    r1_fixed.meta.run_id = fixed_uuid();
    let mut r2_fixed = r2;
    r2_fixed.meta.run_id = fixed_uuid();
    let h1 = abp_core::receipt_hash(&r1_fixed).unwrap();
    let h2 = abp_core::receipt_hash(&r2_fixed).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_sensitive_to_all_fields() {
    let base = make_receipt();
    let base_hash = abp_core::receipt_hash(&base).unwrap();

    // Changing contract_version
    let mut r = make_receipt();
    r.meta.contract_version = "abp/v999".into();
    assert_ne!(abp_core::receipt_hash(&r).unwrap(), base_hash);

    // Changing mode
    let mut r = make_receipt();
    r.mode = ExecutionMode::Passthrough;
    assert_ne!(abp_core::receipt_hash(&r).unwrap(), base_hash);

    // Changing usage_raw
    let mut r = make_receipt();
    r.usage_raw = serde_json::json!({"tokens": 100});
    assert_ne!(abp_core::receipt_hash(&r).unwrap(), base_hash);

    // Changing verification
    let mut r = make_receipt();
    r.verification.harness_ok = true;
    assert_ne!(abp_core::receipt_hash(&r).unwrap(), base_hash);
}

// ═══════════════════════════════════════════════════
// Additional: Protocol envelope determinism
// ═══════════════════════════════════════════════════

#[test]
fn protocol_envelope_hello_roundtrip() {
    let env = abp_protocol::Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        {
            let mut caps = CapabilityManifest::new();
            caps.insert(Capability::Streaming, SupportLevel::Native);
            caps
        },
    );
    assert_roundtrip_deterministic(&env);
}

#[test]
fn protocol_envelope_event_roundtrip() {
    let env = abp_protocol::Envelope::Event {
        ref_id: "run-1".into(),
        event: make_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
    };
    assert_roundtrip_deterministic(&env);
}

#[test]
fn protocol_envelope_fatal_roundtrip() {
    let env = abp_protocol::Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "something broke".into(),
        error_code: Some(abp_error::ErrorCode::BackendCrashed),
    };
    assert_roundtrip_deterministic(&env);
}

#[test]
fn protocol_envelope_run_roundtrip() {
    let env = abp_protocol::Envelope::Run {
        id: "run-1".into(),
        work_order: make_work_order(),
    };
    assert_roundtrip_deterministic(&env);
}

#[test]
fn protocol_envelope_final_roundtrip() {
    let env = abp_protocol::Envelope::Final {
        ref_id: "run-1".into(),
        receipt: make_receipt(),
    };
    assert_roundtrip_deterministic(&env);
}

// ═══════════════════════════════════════════════════
// Additional: Compound stability
// ═══════════════════════════════════════════════════

#[test]
fn capability_manifest_insertion_order_irrelevant() {
    let mut m1 = CapabilityManifest::new();
    m1.insert(Capability::ToolRead, SupportLevel::Native);
    m1.insert(Capability::Streaming, SupportLevel::Native);

    let mut m2 = CapabilityManifest::new();
    m2.insert(Capability::Streaming, SupportLevel::Native);
    m2.insert(Capability::ToolRead, SupportLevel::Native);

    let j1 = serde_json::to_string(&m1).unwrap();
    let j2 = serde_json::to_string(&m2).unwrap();
    assert_eq!(
        j1, j2,
        "insertion order must not affect manifest serialization"
    );
}

#[test]
fn receipt_with_capabilities_hash_stable() {
    let mut r = make_receipt();
    r.capabilities
        .insert(Capability::ToolWrite, SupportLevel::Native);
    r.capabilities
        .insert(Capability::Streaming, SupportLevel::Emulated);
    let h1 = abp_core::receipt_hash(&r).unwrap();
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_with_usage_roundtrip() {
    let mut r = make_receipt();
    r.usage = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(200),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: Some(5),
        estimated_cost_usd: Some(0.01),
    };
    assert_roundtrip_deterministic(&r);
}

#[test]
fn artifact_ref_roundtrip() {
    let a = ArtifactRef {
        kind: "patch".into(),
        path: "output.diff".into(),
    };
    assert_roundtrip_deterministic(&a);
}

#[test]
fn verification_report_roundtrip() {
    let v = VerificationReport {
        git_diff: Some("+new line".into()),
        git_status: Some("M file.rs".into()),
        harness_ok: true,
    };
    assert_roundtrip_deterministic(&v);
}

#[test]
fn backend_identity_roundtrip() {
    let b = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("2.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    };
    assert_roundtrip_deterministic(&b);
}

#[test]
fn context_snippet_roundtrip() {
    let s = ContextSnippet {
        name: "test snippet".into(),
        content: "some code here".into(),
    };
    assert_roundtrip_deterministic(&s);
}

#[test]
fn workspace_spec_roundtrip() {
    let ws = WorkspaceSpec {
        root: "/home/user/project".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["target/**".into(), "*.lock".into()],
    };
    assert_roundtrip_deterministic(&ws);
}

#[test]
fn policy_profile_roundtrip() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["*.config".into()],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec![],
        require_approval_for: vec!["deploy".into()],
    };
    assert_roundtrip_deterministic(&p);
}

#[test]
fn run_metadata_roundtrip() {
    let m = RunMetadata {
        run_id: fixed_uuid(),
        work_order_id: fixed_uuid2(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: fixed_ts(),
        finished_at: fixed_ts_end(),
        duration_ms: 42000,
    };
    assert_roundtrip_deterministic(&m);
}

#[test]
fn support_level_restricted_roundtrip() {
    let level = SupportLevel::Restricted {
        reason: "disabled by policy".into(),
    };
    assert_roundtrip_deterministic(&level);
}

#[test]
fn multiple_serializations_hash_identical() {
    let wo = make_work_order();
    let hashes: Vec<String> = (0..10)
        .map(|_| {
            let json = abp_core::canonical_json(&wo).unwrap();
            sha256_hex(json.as_bytes())
        })
        .collect();
    assert!(hashes.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn receipt_hash_10_times_same() {
    let r = make_receipt();
    let hashes: Vec<String> = (0..10)
        .map(|_| abp_core::receipt_hash(&r).unwrap())
        .collect();
    assert!(hashes.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn tool_call_input_with_sorted_keys() {
    let input = serde_json::json!({"z": 1, "a": 2, "m": 3});
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "test".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input,
    });
    // serde_json::Value preserves insertion order for objects created from json!()
    // But after roundtrip through Value → string → Value, keys get sorted (serde_json
    // uses BTreeMap for its Map type by default).
    let json = serde_json::to_string(&e).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&val).unwrap();
    let val2: serde_json::Value = serde_json::from_str(&json2).unwrap();
    assert_eq!(val, val2);
}

#[test]
fn canonical_json_matches_sha256_hex() {
    let data = serde_json::json!({"key": "value"});
    let canonical = abp_core::canonical_json(&data).unwrap();
    let expected = abp_core::sha256_hex(canonical.as_bytes());
    let actual = sha256_hex(canonical.as_bytes());
    assert_eq!(
        expected, actual,
        "sha256_hex from abp_core must match local impl"
    );
}
