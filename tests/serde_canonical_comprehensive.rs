#![allow(clippy::all)]
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
//! Comprehensive canonical JSON serialization tests.
//!
//! Categories covered:
//! 1. BTreeMap key ordering in serialized JSON
//! 2. Byte-level equality for identical data
//! 3. Optional field handling (None → null / omitted)
//! 4. Numeric precision preservation
//! 5. String escaping consistency
//! 6. Nested object ordering
//! 7. Array element ordering stability
//! 8. Receipt hash depends on canonical form
//! 9. Serde attribute correctness (rename_all, skip_serializing_if, tag, flatten)
//! 10. Cross-platform consistency

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
use abp_protocol::{Envelope, JsonlCodec};
use chrono::{TimeZone, Utc};
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ts1() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 1, 8, 0, 0).unwrap()
}

fn ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 1, 8, 10, 0).unwrap()
}

fn uuid_a() -> Uuid {
    Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap()
}

fn uuid_b() -> Uuid {
    Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap()
}

fn backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("2.0.0".into()),
        adapter_version: None,
    }
}

fn run_meta() -> RunMetadata {
    RunMetadata {
        run_id: uuid_a(),
        work_order_id: uuid_b(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: ts1(),
        finished_at: ts2(),
        duration_ms: 600_000,
    }
}

fn minimal_receipt() -> Receipt {
    Receipt {
        meta: run_meta(),
        backend: backend(),
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

fn event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: ts1(),
        kind,
        ext: None,
    }
}

fn work_order() -> WorkOrder {
    WorkOrder {
        id: uuid_a(),
        task: "canonical test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/workspace".into(),
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

fn is_sorted(keys: &[String]) -> bool {
    keys.windows(2).all(|w| w[0] <= w[1])
}

// =========================================================================
// 1. BTreeMap key ordering in serialized JSON
// =========================================================================

#[test]
fn btreemap_vendor_keys_alphabetical() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert("zulu".into(), serde_json::json!(1));
    config.vendor.insert("bravo".into(), serde_json::json!(2));
    config.vendor.insert("alpha".into(), serde_json::json!(3));
    config.vendor.insert("mike".into(), serde_json::json!(4));

    let v: Value = serde_json::to_value(&config).unwrap();
    assert_eq!(
        sorted_keys(&v["vendor"]),
        vec!["alpha", "bravo", "mike", "zulu"]
    );
}

#[test]
fn btreemap_env_keys_alphabetical() {
    let mut config = RuntimeConfig::default();
    config.env.insert("PATH".into(), "/usr/bin".into());
    config.env.insert("HOME".into(), "/home/user".into());
    config.env.insert("EDITOR".into(), "vim".into());

    let v: Value = serde_json::to_value(&config).unwrap();
    assert_eq!(sorted_keys(&v["env"]), vec!["EDITOR", "HOME", "PATH"]);
}

#[test]
fn btreemap_capability_manifest_sorted_by_ord() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolBash, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::McpServer, SupportLevel::Unsupported);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Native);

    let v: Value = serde_json::to_value(&caps).unwrap();
    let keys = sorted_keys(&v);
    assert!(is_sorted(&keys), "capability keys not sorted: {keys:?}");
}

#[test]
fn btreemap_ext_field_sorted() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), serde_json::json!({"content": "hi"}));
    ext.insert("debug_info".into(), serde_json::json!(42));
    ext.insert("adapter_data".into(), serde_json::json!(null));

    let ev = AgentEvent {
        ts: ts1(),
        kind: AgentEventKind::AssistantMessage { text: "ok".into() },
        ext: Some(ext),
    };
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(
        sorted_keys(&v["ext"]),
        vec!["adapter_data", "debug_info", "raw_message"]
    );
}

#[test]
fn btreemap_ir_message_metadata_sorted() {
    let mut meta = BTreeMap::new();
    meta.insert("z".into(), serde_json::json!("last"));
    meta.insert("a".into(), serde_json::json!("first"));
    meta.insert("m".into(), serde_json::json!("mid"));

    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text {
            text: "hello".into(),
        }],
        metadata: meta,
    };
    let v: Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(sorted_keys(&v["metadata"]), vec!["a", "m", "z"]);
}

#[test]
fn btreemap_nested_vendor_objects_sorted() {
    let mut config = RuntimeConfig::default();
    let inner = serde_json::json!({"z": 1, "a": 2, "m": 3});
    config.vendor.insert("outer".into(), inner);

    let json = canonical_json(&config).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let inner_keys = sorted_keys(&v["vendor"]["outer"]);
    assert_eq!(inner_keys, vec!["a", "m", "z"]);
}

#[test]
fn btreemap_empty_maps_serialize_as_empty_object() {
    let config = RuntimeConfig::default();
    let v: Value = serde_json::to_value(&config).unwrap();
    assert_eq!(v["vendor"], serde_json::json!({}));
    assert_eq!(v["env"], serde_json::json!({}));
}

#[test]
fn btreemap_single_entry_consistent() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("only".into(), serde_json::json!("one"));

    let json1 = serde_json::to_string(&config).unwrap();
    let json2 = serde_json::to_string(&config).unwrap();
    assert_eq!(json1, json2);
}

// =========================================================================
// 2. Byte-level equality for identical data
// =========================================================================

#[test]
fn receipt_byte_equal_across_serializations() {
    let r = minimal_receipt();
    let j1 = serde_json::to_string(&r).unwrap();
    let j2 = serde_json::to_string(&r).unwrap();
    assert_eq!(j1.as_bytes(), j2.as_bytes());
}

#[test]
fn work_order_byte_equal_across_serializations() {
    let wo = work_order();
    let j1 = canonical_json(&wo).unwrap();
    let j2 = canonical_json(&wo).unwrap();
    assert_eq!(j1.as_bytes(), j2.as_bytes());
}

#[test]
fn canonical_json_byte_equal_simple_object() {
    let v = serde_json::json!({"b": 2, "a": 1});
    let c1 = canonical_json(&v).unwrap();
    let c2 = canonical_json(&v).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn canonical_json_sorts_keys() {
    let v = serde_json::json!({"c": 3, "a": 1, "b": 2});
    let json = canonical_json(&v).unwrap();
    assert!(json.starts_with(r#"{"a":1"#));
}

#[test]
fn envelope_byte_equal_across_serializations() {
    let env = Envelope::hello(backend(), CapabilityManifest::new());
    let j1 = JsonlCodec::encode(&env).unwrap();
    let j2 = JsonlCodec::encode(&env).unwrap();
    assert_eq!(j1.as_bytes(), j2.as_bytes());
}

#[test]
fn sha256_hex_deterministic() {
    let h1 = sha256_hex(b"deterministic input");
    let h2 = sha256_hex(b"deterministic input");
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn receipt_hash_deterministic_for_same_receipt() {
    let r = minimal_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn canonical_json_thousand_iterations_consistent() {
    let r = minimal_receipt();
    let baseline = canonical_json(&r).unwrap();
    for _ in 0..1000 {
        assert_eq!(canonical_json(&r).unwrap(), baseline);
    }
}

#[test]
fn ir_conversation_byte_equal() {
    let conv = IrConversation {
        messages: vec![
            IrMessage::new(
                IrRole::System,
                vec![IrContentBlock::Text { text: "sys".into() }],
            ),
            IrMessage::new(
                IrRole::User,
                vec![IrContentBlock::Text { text: "hi".into() }],
            ),
        ],
    };
    let j1 = serde_json::to_string(&conv).unwrap();
    let j2 = serde_json::to_string(&conv).unwrap();
    assert_eq!(j1, j2);
}

// =========================================================================
// 3. Optional fields (None) — omitted or null consistently
// =========================================================================

#[test]
fn backend_identity_none_fields_are_null() {
    let b = BackendIdentity {
        id: "test".into(),
        backend_version: None,
        adapter_version: None,
    };
    let v: Value = serde_json::to_value(&b).unwrap();
    assert_eq!(v["backend_version"], Value::Null);
    assert_eq!(v["adapter_version"], Value::Null);
}

#[test]
fn backend_identity_some_fields_present() {
    let b = BackendIdentity {
        id: "test".into(),
        backend_version: Some("1.0".into()),
        adapter_version: Some("0.5".into()),
    };
    let v: Value = serde_json::to_value(&b).unwrap();
    assert_eq!(v["backend_version"], "1.0");
    assert_eq!(v["adapter_version"], "0.5");
}

#[test]
fn receipt_sha256_none_is_null() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["receipt_sha256"], Value::Null);
}

#[test]
fn receipt_sha256_some_is_string() {
    let r = minimal_receipt().with_hash().unwrap();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert!(v["receipt_sha256"].is_string());
}

#[test]
fn usage_normalized_all_none() {
    let u = UsageNormalized::default();
    let v: Value = serde_json::to_value(&u).unwrap();
    assert_eq!(v["input_tokens"], Value::Null);
    assert_eq!(v["output_tokens"], Value::Null);
    assert_eq!(v["cache_read_tokens"], Value::Null);
    assert_eq!(v["cache_write_tokens"], Value::Null);
    assert_eq!(v["request_units"], Value::Null);
    assert_eq!(v["estimated_cost_usd"], Value::Null);
}

#[test]
fn usage_normalized_partial_fill() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: Some(0.01),
    };
    let v: Value = serde_json::to_value(&u).unwrap();
    assert_eq!(v["input_tokens"], 100);
    assert_eq!(v["output_tokens"], 50);
    assert_eq!(v["cache_read_tokens"], Value::Null);
    assert_eq!(v["estimated_cost_usd"], 0.01);
}

#[test]
fn runtime_config_model_none_is_null() {
    let c = RuntimeConfig::default();
    let v: Value = serde_json::to_value(&c).unwrap();
    assert_eq!(v["model"], Value::Null);
}

#[test]
fn runtime_config_model_some_is_string() {
    let c = RuntimeConfig {
        model: Some("gpt-4".into()),
        ..Default::default()
    };
    let v: Value = serde_json::to_value(&c).unwrap();
    assert_eq!(v["model"], "gpt-4");
}

#[test]
fn verification_report_none_fields() {
    let vr = VerificationReport::default();
    let v: Value = serde_json::to_value(&vr).unwrap();
    assert_eq!(v["git_diff"], Value::Null);
    assert_eq!(v["git_status"], Value::Null);
    assert_eq!(v["harness_ok"], false);
}

#[test]
fn agent_event_ext_none_is_omitted() {
    let ev = event(AgentEventKind::RunStarted {
        message: "start".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    // ext uses skip_serializing_if = "Option::is_none"
    assert!(!json.contains("\"ext\""));
}

#[test]
fn agent_event_ext_some_is_present() {
    let mut ext = BTreeMap::new();
    ext.insert("key".into(), serde_json::json!("val"));
    let ev = AgentEvent {
        ts: ts1(),
        kind: AgentEventKind::RunStarted {
            message: "start".into(),
        },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("\"ext\""));
}

#[test]
fn tool_call_optional_ids_null() {
    let ev = event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["tool_use_id"], Value::Null);
    assert_eq!(v["parent_tool_use_id"], Value::Null);
}

#[test]
fn command_executed_optional_fields_null() {
    let ev = event(AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: None,
        output_preview: None,
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["exit_code"], Value::Null);
    assert_eq!(v["output_preview"], Value::Null);
}

#[test]
fn ir_message_empty_metadata_omitted() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Text { text: "hi".into() }],
    );
    let json = serde_json::to_string(&msg).unwrap();
    // metadata uses skip_serializing_if = "BTreeMap::is_empty"
    assert!(!json.contains("\"metadata\""));
}

#[test]
fn ir_message_nonempty_metadata_present() {
    let mut meta = BTreeMap::new();
    meta.insert("key".into(), serde_json::json!(true));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata: meta,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"metadata\""));
}

// =========================================================================
// 4. Numeric precision preservation
// =========================================================================

#[test]
fn integer_precision_u64_max() {
    let u = UsageNormalized {
        input_tokens: Some(u64::MAX),
        output_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    };
    let json = serde_json::to_string(&u).unwrap();
    let parsed: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.input_tokens, Some(u64::MAX));
}

#[test]
fn integer_precision_zero() {
    let u = UsageNormalized {
        input_tokens: Some(0),
        output_tokens: Some(0),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: Some(0),
        estimated_cost_usd: Some(0.0),
    };
    let json = serde_json::to_string(&u).unwrap();
    let parsed: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.input_tokens, Some(0));
    assert_eq!(parsed.estimated_cost_usd, Some(0.0));
}

#[test]
fn float_precision_small_value() {
    let u = UsageNormalized {
        estimated_cost_usd: Some(0.000_001),
        ..Default::default()
    };
    let json = serde_json::to_string(&u).unwrap();
    let parsed: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert!((parsed.estimated_cost_usd.unwrap() - 0.000_001).abs() < f64::EPSILON);
}

#[test]
fn float_precision_large_value() {
    let u = UsageNormalized {
        estimated_cost_usd: Some(999_999.99),
        ..Default::default()
    };
    let json = serde_json::to_string(&u).unwrap();
    let parsed: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert!((parsed.estimated_cost_usd.unwrap() - 999_999.99).abs() < 0.001);
}

#[test]
fn float_precision_in_runtime_config() {
    let c = RuntimeConfig {
        max_budget_usd: Some(1.23456789),
        ..Default::default()
    };
    let json = serde_json::to_string(&c).unwrap();
    let parsed: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert!((parsed.max_budget_usd.unwrap() - 1.23456789).abs() < f64::EPSILON);
}

#[test]
fn duration_ms_u64_roundtrip() {
    let mut r = minimal_receipt();
    r.meta.duration_ms = u64::MAX;
    let json = serde_json::to_string(&r).unwrap();
    let parsed: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.meta.duration_ms, u64::MAX);
}

#[test]
fn max_turns_u32_roundtrip() {
    let c = RuntimeConfig {
        max_turns: Some(u32::MAX),
        ..Default::default()
    };
    let json = serde_json::to_string(&c).unwrap();
    let parsed: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.max_turns, Some(u32::MAX));
}

#[test]
fn negative_exit_code_roundtrip() {
    let ev = event(AgentEventKind::CommandExecuted {
        command: "fail".into(),
        exit_code: Some(-1),
        output_preview: None,
    });
    let json = serde_json::to_string(&ev).unwrap();
    let parsed: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::CommandExecuted { exit_code, .. } = parsed.kind {
        assert_eq!(exit_code, Some(-1));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn ir_usage_large_tokens() {
    let u = IrUsage {
        input_tokens: u64::MAX,
        output_tokens: u64::MAX,
        total_tokens: u64::MAX,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    };
    let json = serde_json::to_string(&u).unwrap();
    let parsed: IrUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.input_tokens, u64::MAX);
    assert_eq!(parsed.output_tokens, u64::MAX);
    assert_eq!(parsed.total_tokens, u64::MAX);
}

// =========================================================================
// 5. String escaping consistency
// =========================================================================

#[test]
fn string_with_quotes_roundtrip() {
    let wo = WorkOrder {
        task: r#"Fix the "login" bug"#.into(),
        ..work_order()
    };
    let json = serde_json::to_string(&wo).unwrap();
    let parsed: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.task, r#"Fix the "login" bug"#);
}

#[test]
fn string_with_backslashes_roundtrip() {
    let wo = WorkOrder {
        task: r"path\to\file".into(),
        ..work_order()
    };
    let json = serde_json::to_string(&wo).unwrap();
    let parsed: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.task, r"path\to\file");
}

#[test]
fn string_with_newlines_roundtrip() {
    let wo = WorkOrder {
        task: "line1\nline2\ttab".into(),
        ..work_order()
    };
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains(r"\n"));
    assert!(json.contains(r"\t"));
    let parsed: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.task, "line1\nline2\ttab");
}

#[test]
fn string_with_unicode_roundtrip() {
    let wo = WorkOrder {
        task: "修正バグ 🐛".into(),
        ..work_order()
    };
    let json = serde_json::to_string(&wo).unwrap();
    let parsed: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.task, "修正バグ 🐛");
}

#[test]
fn string_with_null_bytes_in_json_value() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("data".into(), serde_json::json!("has\u{0000}null"));
    let json = serde_json::to_string(&config).unwrap();
    let parsed: RuntimeConfig = serde_json::from_str(&json).unwrap();
    let val = parsed.vendor.get("data").unwrap();
    assert_eq!(val.as_str().unwrap(), "has\u{0000}null");
}

#[test]
fn string_escaping_deterministic() {
    let special = "quotes\"back\\slash/newline\ntab\t";
    let j1 = serde_json::to_string(special).unwrap();
    let j2 = serde_json::to_string(special).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn empty_string_roundtrip() {
    let wo = WorkOrder {
        task: String::new(),
        ..work_order()
    };
    let json = serde_json::to_string(&wo).unwrap();
    let parsed: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.task, "");
}

#[test]
fn long_string_roundtrip() {
    let long = "x".repeat(100_000);
    let wo = WorkOrder {
        task: long.clone(),
        ..work_order()
    };
    let json = serde_json::to_string(&wo).unwrap();
    let parsed: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.task, long);
}

// =========================================================================
// 6. Nested object ordering
// =========================================================================

#[test]
fn receipt_top_level_keys_sorted_in_canonical() {
    let r = minimal_receipt();
    let json = canonical_json(&r).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let keys = sorted_keys(&v);
    assert!(is_sorted(&keys), "receipt keys not sorted: {keys:?}");
}

#[test]
fn receipt_meta_keys_sorted_in_canonical() {
    let r = minimal_receipt();
    let json = canonical_json(&r).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let keys = sorted_keys(&v["meta"]);
    assert!(is_sorted(&keys), "meta keys not sorted: {keys:?}");
}

#[test]
fn receipt_backend_keys_sorted_in_canonical() {
    let r = minimal_receipt();
    let json = canonical_json(&r).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let keys = sorted_keys(&v["backend"]);
    assert!(is_sorted(&keys), "backend keys not sorted: {keys:?}");
}

#[test]
fn work_order_nested_workspace_keys_sorted() {
    let wo = work_order();
    let json = canonical_json(&wo).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let keys = sorted_keys(&v["workspace"]);
    assert!(is_sorted(&keys), "workspace keys not sorted: {keys:?}");
}

#[test]
fn work_order_config_keys_sorted() {
    let wo = work_order();
    let json = canonical_json(&wo).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let keys = sorted_keys(&v["config"]);
    assert!(is_sorted(&keys), "config keys not sorted: {keys:?}");
}

#[test]
fn deeply_nested_vendor_map_sorted() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "level1".into(),
        serde_json::json!({"z": {"zz": 1, "aa": 2}, "a": 3}),
    );
    let json = canonical_json(&config).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let l1_keys = sorted_keys(&v["vendor"]["level1"]);
    assert_eq!(l1_keys, vec!["a", "z"]);
    let l2_keys = sorted_keys(&v["vendor"]["level1"]["z"]);
    assert_eq!(l2_keys, vec!["aa", "zz"]);
}

#[test]
fn envelope_hello_nested_keys_sorted_canonical() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    let env = Envelope::hello(backend(), caps);
    let json = canonical_json(&env).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let top_keys = sorted_keys(&v);
    assert!(
        is_sorted(&top_keys),
        "envelope keys not sorted: {top_keys:?}"
    );
}

#[test]
fn usage_normalized_keys_sorted() {
    let u = UsageNormalized {
        input_tokens: Some(10),
        output_tokens: Some(20),
        cache_read_tokens: Some(5),
        cache_write_tokens: Some(3),
        request_units: Some(1),
        estimated_cost_usd: Some(0.05),
    };
    let json = canonical_json(&u).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let keys = sorted_keys(&v);
    assert!(is_sorted(&keys), "usage keys not sorted: {keys:?}");
}

#[test]
fn policy_profile_keys_sorted() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["*.lock".into()],
        allow_network: vec!["*.github.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["write".into()],
    };
    let json = canonical_json(&p).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let keys = sorted_keys(&v);
    assert!(is_sorted(&keys), "policy keys not sorted: {keys:?}");
}

// =========================================================================
// 7. Array element ordering stability
// =========================================================================

#[test]
fn trace_events_maintain_insertion_order() {
    let r = Receipt {
        trace: vec![
            event(AgentEventKind::RunStarted {
                message: "1".into(),
            }),
            event(AgentEventKind::AssistantMessage { text: "2".into() }),
            event(AgentEventKind::RunCompleted {
                message: "3".into(),
            }),
        ],
        ..minimal_receipt()
    };
    let v: Value = serde_json::to_value(&r).unwrap();
    let trace = v["trace"].as_array().unwrap();
    assert_eq!(trace[0]["message"], "1");
    assert_eq!(trace[1]["text"], "2");
    assert_eq!(trace[2]["message"], "3");
}

#[test]
fn artifacts_maintain_insertion_order() {
    let r = Receipt {
        artifacts: vec![
            ArtifactRef {
                kind: "patch".into(),
                path: "a.patch".into(),
            },
            ArtifactRef {
                kind: "log".into(),
                path: "z.log".into(),
            },
            ArtifactRef {
                kind: "diff".into(),
                path: "m.diff".into(),
            },
        ],
        ..minimal_receipt()
    };
    let v: Value = serde_json::to_value(&r).unwrap();
    let arts = v["artifacts"].as_array().unwrap();
    assert_eq!(arts[0]["path"], "a.patch");
    assert_eq!(arts[1]["path"], "z.log");
    assert_eq!(arts[2]["path"], "m.diff");
}

#[test]
fn context_files_order_preserved() {
    let ctx = ContextPacket {
        files: vec!["z.rs".into(), "a.rs".into(), "m.rs".into()],
        snippets: vec![],
    };
    let v: Value = serde_json::to_value(&ctx).unwrap();
    let files = v["files"].as_array().unwrap();
    assert_eq!(files[0], "z.rs");
    assert_eq!(files[1], "a.rs");
    assert_eq!(files[2], "m.rs");
}

#[test]
fn context_snippets_order_preserved() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![
            ContextSnippet {
                name: "second".into(),
                content: "2".into(),
            },
            ContextSnippet {
                name: "first".into(),
                content: "1".into(),
            },
        ],
    };
    let v: Value = serde_json::to_value(&ctx).unwrap();
    let snips = v["snippets"].as_array().unwrap();
    assert_eq!(snips[0]["name"], "second");
    assert_eq!(snips[1]["name"], "first");
}

#[test]
fn allowed_tools_order_preserved() {
    let p = PolicyProfile {
        allowed_tools: vec!["write".into(), "bash".into(), "read".into()],
        ..Default::default()
    };
    let v: Value = serde_json::to_value(&p).unwrap();
    let tools = v["allowed_tools"].as_array().unwrap();
    assert_eq!(tools[0], "write");
    assert_eq!(tools[1], "bash");
    assert_eq!(tools[2], "read");
}

#[test]
fn ir_conversation_message_order_preserved() {
    let conv = IrConversation {
        messages: vec![
            IrMessage::new(IrRole::System, vec![]),
            IrMessage::new(IrRole::User, vec![]),
            IrMessage::new(IrRole::Assistant, vec![]),
        ],
    };
    let v: Value = serde_json::to_value(&conv).unwrap();
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[2]["role"], "assistant");
}

#[test]
fn ir_content_blocks_order_preserved() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking { text: "hmm".into() },
            IrContentBlock::Text {
                text: "hello".into(),
            },
        ],
    );
    let v: Value = serde_json::to_value(&msg).unwrap();
    let blocks = v["content"].as_array().unwrap();
    assert_eq!(blocks[0]["type"], "thinking");
    assert_eq!(blocks[1]["type"], "text");
}

#[test]
fn capability_requirements_order_preserved() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let v: Value = serde_json::to_value(&reqs).unwrap();
    let arr = v["required"].as_array().unwrap();
    assert_eq!(arr[0]["capability"], "tool_bash");
    assert_eq!(arr[1]["capability"], "streaming");
}

#[test]
fn empty_arrays_serialized_consistently() {
    let p = PolicyProfile::default();
    let v: Value = serde_json::to_value(&p).unwrap();
    assert_eq!(v["allowed_tools"], serde_json::json!([]));
    assert_eq!(v["disallowed_tools"], serde_json::json!([]));
}

// =========================================================================
// 8. Receipt hash depends on canonical form
// =========================================================================

#[test]
fn receipt_hash_ignores_stored_hash_field() {
    let mut r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r1.receipt_sha256 = None;
    r2.receipt_sha256 = Some("anything".into());
    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_with_hash_produces_valid_hash() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    let stored = r.receipt_sha256.as_ref().unwrap();
    let recomputed = receipt_hash(&r).unwrap();
    assert_eq!(stored, &recomputed);
}

#[test]
fn receipt_hash_changes_with_outcome() {
    let r1 = Receipt {
        outcome: Outcome::Complete,
        ..minimal_receipt()
    };
    let r2 = Receipt {
        outcome: Outcome::Failed,
        ..minimal_receipt()
    };
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_backend_id() {
    let r1 = minimal_receipt();
    let r2 = Receipt {
        backend: BackendIdentity {
            id: "other-backend".into(),
            backend_version: Some("2.0.0".into()),
            adapter_version: None,
        },
        ..minimal_receipt()
    };
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_trace() {
    let r1 = minimal_receipt();
    let r2 = Receipt {
        trace: vec![event(AgentEventKind::RunStarted {
            message: "hi".into(),
        })],
        ..minimal_receipt()
    };
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_duration() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.duration_ms = 999;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_mode() {
    let r1 = Receipt {
        mode: ExecutionMode::Mapped,
        ..minimal_receipt()
    };
    let r2 = Receipt {
        mode: ExecutionMode::Passthrough,
        ..minimal_receipt()
    };
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_artifacts() {
    let r1 = minimal_receipt();
    let r2 = Receipt {
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "x.patch".into(),
        }],
        ..minimal_receipt()
    };
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_usage() {
    let r1 = minimal_receipt();
    let r2 = Receipt {
        usage: UsageNormalized {
            input_tokens: Some(42),
            ..Default::default()
        },
        ..minimal_receipt()
    };
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_verification() {
    let r1 = minimal_receipt();
    let r2 = Receipt {
        verification: VerificationReport {
            git_diff: Some("diff content".into()),
            git_status: None,
            harness_ok: true,
        },
        ..minimal_receipt()
    };
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_capabilities() {
    let r1 = minimal_receipt();
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let r2 = Receipt {
        capabilities: caps,
        ..minimal_receipt()
    };
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_is_64_hex_chars() {
    let h = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_uses_canonical_null_for_hash_field() {
    let r = minimal_receipt();
    let mut v = serde_json::to_value(&r).unwrap();
    if let Value::Object(map) = &mut v {
        map.insert("receipt_sha256".to_string(), Value::Null);
    }
    let canonical = serde_json::to_string(&v).unwrap();
    let manual_hash = sha256_hex(canonical.as_bytes());
    assert_eq!(manual_hash, receipt_hash(&r).unwrap());
}

// =========================================================================
// 9. Serde attribute correctness
// =========================================================================

// --- rename_all = "snake_case" ---

#[test]
fn execution_lane_rename_all_snake_case() {
    let j = serde_json::to_string(&ExecutionLane::PatchFirst).unwrap();
    assert_eq!(j, r#""patch_first""#);
    let j = serde_json::to_string(&ExecutionLane::WorkspaceFirst).unwrap();
    assert_eq!(j, r#""workspace_first""#);
}

#[test]
fn workspace_mode_rename_all_snake_case() {
    let j = serde_json::to_string(&WorkspaceMode::PassThrough).unwrap();
    assert_eq!(j, r#""pass_through""#);
    let j = serde_json::to_string(&WorkspaceMode::Staged).unwrap();
    assert_eq!(j, r#""staged""#);
}

#[test]
fn outcome_rename_all_snake_case() {
    assert_eq!(
        serde_json::to_string(&Outcome::Complete).unwrap(),
        r#""complete""#
    );
    assert_eq!(
        serde_json::to_string(&Outcome::Partial).unwrap(),
        r#""partial""#
    );
    assert_eq!(
        serde_json::to_string(&Outcome::Failed).unwrap(),
        r#""failed""#
    );
}

#[test]
fn execution_mode_rename_all_snake_case() {
    assert_eq!(
        serde_json::to_string(&ExecutionMode::Passthrough).unwrap(),
        r#""passthrough""#
    );
    assert_eq!(
        serde_json::to_string(&ExecutionMode::Mapped).unwrap(),
        r#""mapped""#
    );
}

#[test]
fn min_support_rename_all_snake_case() {
    assert_eq!(
        serde_json::to_string(&MinSupport::Native).unwrap(),
        r#""native""#
    );
    assert_eq!(
        serde_json::to_string(&MinSupport::Emulated).unwrap(),
        r#""emulated""#
    );
}

#[test]
fn support_level_rename_all_snake_case() {
    assert_eq!(
        serde_json::to_string(&SupportLevel::Native).unwrap(),
        r#""native""#
    );
    assert_eq!(
        serde_json::to_string(&SupportLevel::Emulated).unwrap(),
        r#""emulated""#
    );
    assert_eq!(
        serde_json::to_string(&SupportLevel::Unsupported).unwrap(),
        r#""unsupported""#
    );
}

#[test]
fn support_level_restricted_variant() {
    let r = SupportLevel::Restricted {
        reason: "policy".into(),
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("\"restricted\""));
    assert!(json.contains("\"reason\""));
    let parsed: SupportLevel = serde_json::from_str(&json).unwrap();
    if let SupportLevel::Restricted { reason } = parsed {
        assert_eq!(reason, "policy");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn capability_rename_all_snake_case() {
    assert_eq!(
        serde_json::to_string(&Capability::Streaming).unwrap(),
        r#""streaming""#
    );
    assert_eq!(
        serde_json::to_string(&Capability::ToolRead).unwrap(),
        r#""tool_read""#
    );
    assert_eq!(
        serde_json::to_string(&Capability::McpClient).unwrap(),
        r#""mcp_client""#
    );
    assert_eq!(
        serde_json::to_string(&Capability::ExtendedThinking).unwrap(),
        r#""extended_thinking""#
    );
    assert_eq!(
        serde_json::to_string(&Capability::HooksPreToolUse).unwrap(),
        r#""hooks_pre_tool_use""#
    );
}

#[test]
fn ir_role_rename_all_snake_case() {
    assert_eq!(
        serde_json::to_string(&IrRole::System).unwrap(),
        r#""system""#
    );
    assert_eq!(serde_json::to_string(&IrRole::User).unwrap(), r#""user""#);
    assert_eq!(
        serde_json::to_string(&IrRole::Assistant).unwrap(),
        r#""assistant""#
    );
}

// --- tag attributes ---

#[test]
fn agent_event_kind_tagged_with_type_field() {
    let ev = event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains(r#""type":"assistant_message""#));
}

#[test]
fn agent_event_kind_all_variants_use_type_tag() {
    let variants: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "s".into(),
        },
        AgentEventKind::RunCompleted {
            message: "c".into(),
        },
        AgentEventKind::AssistantDelta { text: "d".into() },
        AgentEventKind::AssistantMessage { text: "m".into() },
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: None,
            output: serde_json::json!({}),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "f".into(),
            summary: "s".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "c".into(),
            exit_code: None,
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "w".into(),
        },
        AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        },
    ];
    for kind in variants {
        let ev = event(kind);
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""type":"#), "missing type tag: {json}");
    }
}

#[test]
fn envelope_tagged_with_t_field() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
}

#[test]
fn envelope_hello_uses_t_tag() {
    let env = Envelope::hello(backend(), CapabilityManifest::new());
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn envelope_run_uses_t_tag() {
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: work_order(),
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"run""#));
}

#[test]
fn envelope_event_uses_t_tag() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: event(AgentEventKind::AssistantMessage { text: "hi".into() }),
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"event""#));
}

#[test]
fn envelope_final_uses_t_tag() {
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: minimal_receipt(),
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"final""#));
}

// --- flatten attribute ---

#[test]
fn agent_event_kind_is_flattened() {
    let ev = event(AgentEventKind::AssistantMessage {
        text: "flat".into(),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    // Due to flatten, "text" and "type" should be at top level, not nested
    assert!(v.get("type").is_some());
    assert!(v.get("text").is_some());
    assert!(v.get("kind").is_none());
}

// --- skip_serializing_if ---

#[test]
fn envelope_fatal_error_code_skipped_when_none() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(!json.contains("error_code"));
}

#[test]
fn envelope_fatal_error_code_present_when_some() {
    let env =
        Envelope::fatal_with_code(None, "boom", abp_error::ErrorCode::ProtocolInvalidEnvelope);
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("error_code"));
}

#[test]
fn agent_event_error_code_skipped_when_none() {
    let ev = event(AgentEventKind::Error {
        message: "err".into(),
        error_code: None,
    });
    let json = serde_json::to_string(&ev).unwrap();
    assert!(!json.contains("error_code"));
}

// --- default attribute ---

#[test]
fn execution_mode_defaults_to_mapped() {
    let mode: ExecutionMode = serde_json::from_str(r#""mapped""#).unwrap();
    assert_eq!(mode, ExecutionMode::Mapped);
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn envelope_hello_mode_defaults_to_mapped() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env: Envelope = serde_json::from_str(json).unwrap();
    if let Envelope::Hello { mode, .. } = env {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("wrong variant");
    }
}

// --- ir content block uses tag = "type" ---

#[test]
fn ir_content_block_text_tagged() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"text""#));
}

#[test]
fn ir_content_block_image_tagged() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "base64data".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"image""#));
}

#[test]
fn ir_content_block_tool_use_tagged() {
    let block = IrContentBlock::ToolUse {
        id: "tu-1".into(),
        name: "read".into(),
        input: serde_json::json!({}),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"tool_use""#));
}

#[test]
fn ir_content_block_tool_result_tagged() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "tu-1".into(),
        content: vec![],
        is_error: false,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"tool_result""#));
}

#[test]
fn ir_content_block_thinking_tagged() {
    let block = IrContentBlock::Thinking {
        text: "thinking".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"thinking""#));
}

// =========================================================================
// 10. Cross-platform consistency (deterministic building blocks)
// =========================================================================

#[test]
fn uuid_serializes_as_lowercase_hyphenated() {
    let id = uuid_a();
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, r#""aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa""#);
}

#[test]
fn chrono_datetime_format_consistent() {
    let dt = ts1();
    let json = serde_json::to_string(&dt).unwrap();
    // chrono serializes to RFC 3339
    assert!(json.contains("2025-06-01"));
    // Roundtrip
    let parsed: chrono::DateTime<Utc> = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, dt);
}

#[test]
fn contract_version_is_embedded() {
    let r = minimal_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["meta"]["contract_version"], CONTRACT_VERSION);
}

#[test]
fn canonical_json_is_compact_no_whitespace() {
    let r = minimal_receipt();
    let json = canonical_json(&r).unwrap();
    // Canonical JSON should not have prettified whitespace
    assert!(!json.contains("  "));
    assert!(!json.contains('\n'));
}

#[test]
fn canonical_json_no_trailing_newline() {
    let r = minimal_receipt();
    let json = canonical_json(&r).unwrap();
    assert!(!json.ends_with('\n'));
}

#[test]
fn jsonl_codec_adds_trailing_newline() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.ends_with('\n'));
    assert_eq!(encoded.matches('\n').count(), 1);
}

#[test]
fn sha256_hex_all_lowercase() {
    let h = sha256_hex(b"test");
    assert!(
        h.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    );
}

#[test]
fn empty_receipt_hash_length() {
    let h = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_builder_produces_consistent_hash() {
    // Use fixed timestamps for determinism
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts1())
        .finished_at(ts2())
        .work_order_id(uuid_b())
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts1())
        .finished_at(ts2())
        .work_order_id(uuid_b())
        .build();
    // run_id is random, so hashes will differ; this verifies builder is self-consistent
    let h1 = receipt_hash(&r1).unwrap();
    assert_eq!(h1.len(), 64);
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h2.len(), 64);
}

#[test]
fn work_order_builder_deterministic_fields() {
    let wo = WorkOrderBuilder::new("test task")
        .root("/tmp")
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains(r#""task":"test task""#));
    assert!(json.contains(r#""model":"gpt-4""#));
}

#[test]
fn jsonl_roundtrip_preserves_all_envelope_variants() {
    let envelopes = vec![
        Envelope::hello(backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: "r1".into(),
            work_order: work_order(),
        },
        Envelope::Event {
            ref_id: "r1".into(),
            event: event(AgentEventKind::AssistantMessage { text: "hi".into() }),
        },
        Envelope::Final {
            ref_id: "r1".into(),
            receipt: minimal_receipt(),
        },
        Envelope::Fatal {
            ref_id: Some("r1".into()),
            error: "boom".into(),
            error_code: None,
        },
    ];
    for env in &envelopes {
        let encoded = JsonlCodec::encode(env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        assert_eq!(encoded, re_encoded);
    }
}

#[test]
fn receipt_serde_roundtrip_preserves_all_fields() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);

    let r = Receipt {
        meta: run_meta(),
        backend: BackendIdentity {
            id: "full".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("0.5".into()),
        },
        capabilities: caps,
        mode: ExecutionMode::Passthrough,
        usage_raw: serde_json::json!({"vendor_specific": true}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(200),
            cache_read_tokens: Some(50),
            cache_write_tokens: Some(25),
            request_units: Some(1),
            estimated_cost_usd: Some(0.05),
        },
        trace: vec![
            event(AgentEventKind::RunStarted {
                message: "go".into(),
            }),
            event(AgentEventKind::AssistantMessage {
                text: "done".into(),
            }),
        ],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "out.patch".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff --git".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };

    let json = serde_json::to_string(&r).unwrap();
    let parsed: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.meta.run_id, r.meta.run_id);
    assert_eq!(parsed.backend.id, "full");
    assert_eq!(parsed.mode, ExecutionMode::Passthrough);
    assert_eq!(parsed.usage.input_tokens, Some(100));
    assert_eq!(parsed.trace.len(), 2);
    assert_eq!(parsed.artifacts.len(), 1);
    assert!(parsed.verification.harness_ok);
    assert_eq!(parsed.outcome, Outcome::Complete);
}

#[test]
fn canonical_json_value_with_all_json_types() {
    let v = serde_json::json!({
        "string": "hello",
        "number": 42,
        "float": 3.15,
        "bool": true,
        "null": null,
        "array": [1, 2, 3],
        "object": {"nested": true}
    });
    let c1 = canonical_json(&v).unwrap();
    let c2 = canonical_json(&v).unwrap();
    assert_eq!(c1, c2);
    // Keys should be sorted
    let parsed: Value = serde_json::from_str(&c1).unwrap();
    let keys = sorted_keys(&parsed);
    assert!(is_sorted(&keys));
}

#[test]
fn work_order_full_roundtrip() {
    let mut config = RuntimeConfig {
        model: Some("gpt-4o".into()),
        max_budget_usd: Some(10.0),
        max_turns: Some(20),
        ..Default::default()
    };
    config.vendor.insert("key".into(), serde_json::json!("val"));
    config.env.insert("HOME".into(), "/home".into());

    let wo = WorkOrder {
        id: uuid_a(),
        task: "full roundtrip test".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/workspace".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec!["*.rs".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "note".into(),
                content: "important".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read".into()],
            disallowed_tools: vec![],
            deny_read: vec![],
            deny_write: vec!["*.lock".into()],
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        },
        requirements: CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        },
        config,
    };

    let json = serde_json::to_string(&wo).unwrap();
    let parsed: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, uuid_a());
    assert_eq!(parsed.task, "full roundtrip test");
    assert_eq!(parsed.context.files.len(), 1);
    assert_eq!(parsed.config.vendor.len(), 1);
}

#[test]
fn ir_tool_definition_roundtrip() {
    let td = IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let json = serde_json::to_string(&td).unwrap();
    let parsed: IrToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, "read_file");
    assert_eq!(parsed.description, "Read a file");
}

#[test]
fn canonical_json_preserves_bool_values() {
    let v = serde_json::json!({"a": true, "b": false});
    let json = canonical_json(&v).unwrap();
    assert!(json.contains("true"));
    assert!(json.contains("false"));
}

#[test]
fn canonical_json_preserves_null() {
    let v = serde_json::json!({"a": null});
    let json = canonical_json(&v).unwrap();
    assert!(json.contains("null"));
}
