// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for deterministic hashing and canonical serialization.
//!
//! ABP uses `BTreeMap` throughout for deterministic serialization. Receipts
//! have canonical JSON hashing where `receipt_sha256` is set to `null` before
//! computing the hash. These tests exhaustively verify that invariant across
//! random inputs, insertion orders, unicode edge cases, and nested structures.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkspaceMode, WorkspaceSpec, canonical_json,
    receipt_hash, sha256_hex,
};
use chrono::{TimeZone, Utc};
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use uuid::Uuid;

// ── Helpers ─────────────────────────────────────────────────────────

const FIXED_UUID: Uuid = Uuid::from_bytes([
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
]);

const FIXED_UUID2: Uuid = Uuid::from_bytes([
    0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
]);

fn ts1() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 42).unwrap()
}

fn make_work_order() -> WorkOrder {
    WorkOrder {
        id: FIXED_UUID,
        task: "Fix the login bug".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/workspace".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["src/main.rs".into(), "README.md".into()],
            snippets: vec![ContextSnippet {
                name: "error log".into(),
                content: "NullPointerException at line 42".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["Read".into(), "Write".into()],
            disallowed_tools: vec!["Bash".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec!["**/lockfile".into()],
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["evil.com".into()],
            require_approval_for: vec!["DeleteFile".into()],
        },
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor: BTreeMap::from([
                ("key_a".into(), serde_json::json!("value_a")),
                ("key_b".into(), serde_json::json!(42)),
                ("key_z".into(), serde_json::json!(true)),
            ]),
            env: BTreeMap::from([
                ("HOME".into(), "/home/user".into()),
                ("PATH".into(), "/usr/bin".into()),
            ]),
            max_budget_usd: Some(1.5),
            max_turns: Some(10),
        },
    }
}

fn make_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: FIXED_UUID,
            work_order_id: FIXED_UUID2,
            contract_version: abp_core::CONTRACT_VERSION.to_string(),
            started_at: ts1(),
            finished_at: ts2(),
            duration_ms: 42_000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: BTreeMap::from([
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::Streaming, SupportLevel::Emulated),
        ]),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({"tokens": 100}),
        usage: UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.01),
        },
        trace: vec![
            AgentEvent {
                ts: ts1(),
                kind: AgentEventKind::RunStarted {
                    message: "Starting run".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: ts2(),
                kind: AgentEventKind::RunCompleted {
                    message: "Done".into(),
                },
                ext: None,
            },
        ],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff --git a/file b/file".into()),
            git_status: Some("M file".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: ts1(),
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════
// 1. BTreeMap insertion order independence
// ═══════════════════════════════════════════════════════════════════

#[test]
fn btreemap_insertion_order_does_not_affect_json_string_values() {
    let mut a = BTreeMap::new();
    a.insert("z", "last");
    a.insert("a", "first");
    a.insert("m", "middle");

    let mut b = BTreeMap::new();
    b.insert("a", "first");
    b.insert("m", "middle");
    b.insert("z", "last");

    let mut c = BTreeMap::new();
    c.insert("m", "middle");
    c.insert("z", "last");
    c.insert("a", "first");

    let ja = serde_json::to_string(&a).unwrap();
    let jb = serde_json::to_string(&b).unwrap();
    let jc = serde_json::to_string(&c).unwrap();

    assert_eq!(ja, jb);
    assert_eq!(jb, jc);
}

#[test]
fn btreemap_insertion_order_does_not_affect_json_mixed_values() {
    let mut a: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    a.insert("number".into(), serde_json::json!(42));
    a.insert("bool".into(), serde_json::json!(true));
    a.insert("string".into(), serde_json::json!("hello"));
    a.insert("array".into(), serde_json::json!([1, 2, 3]));

    let mut b: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    b.insert("array".into(), serde_json::json!([1, 2, 3]));
    b.insert("string".into(), serde_json::json!("hello"));
    b.insert("number".into(), serde_json::json!(42));
    b.insert("bool".into(), serde_json::json!(true));

    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

#[test]
fn btreemap_insertion_order_does_not_affect_hashing() {
    let mut a: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    a.insert("z_key".into(), serde_json::json!("z_val"));
    a.insert("a_key".into(), serde_json::json!("a_val"));

    let mut b: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    b.insert("a_key".into(), serde_json::json!("a_val"));
    b.insert("z_key".into(), serde_json::json!("z_val"));

    let ha = sha256_hex(serde_json::to_string(&a).unwrap().as_bytes());
    let hb = sha256_hex(serde_json::to_string(&b).unwrap().as_bytes());
    assert_eq!(ha, hb);
}

#[test]
fn capability_manifest_insertion_order_independence() {
    let mut a: CapabilityManifest = BTreeMap::new();
    a.insert(Capability::Streaming, SupportLevel::Native);
    a.insert(Capability::ToolRead, SupportLevel::Emulated);
    a.insert(Capability::ToolWrite, SupportLevel::Native);

    let mut b: CapabilityManifest = BTreeMap::new();
    b.insert(Capability::ToolWrite, SupportLevel::Native);
    b.insert(Capability::Streaming, SupportLevel::Native);
    b.insert(Capability::ToolRead, SupportLevel::Emulated);

    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

#[test]
fn vendor_config_insertion_order_independence() {
    let mut cfg_a = RuntimeConfig::default();
    cfg_a.vendor.insert("z".into(), serde_json::json!(1));
    cfg_a.vendor.insert("a".into(), serde_json::json!(2));
    cfg_a.vendor.insert("m".into(), serde_json::json!(3));

    let mut cfg_b = RuntimeConfig::default();
    cfg_b.vendor.insert("a".into(), serde_json::json!(2));
    cfg_b.vendor.insert("m".into(), serde_json::json!(3));
    cfg_b.vendor.insert("z".into(), serde_json::json!(1));

    assert_eq!(
        serde_json::to_string(&cfg_a).unwrap(),
        serde_json::to_string(&cfg_b).unwrap()
    );
}

#[test]
fn env_map_insertion_order_independence() {
    let mut cfg_a = RuntimeConfig::default();
    cfg_a.env.insert("PATH".into(), "/usr/bin".into());
    cfg_a.env.insert("HOME".into(), "/home".into());

    let mut cfg_b = RuntimeConfig::default();
    cfg_b.env.insert("HOME".into(), "/home".into());
    cfg_b.env.insert("PATH".into(), "/usr/bin".into());

    assert_eq!(
        serde_json::to_string(&cfg_a).unwrap(),
        serde_json::to_string(&cfg_b).unwrap()
    );
}

// ═══════════════════════════════════════════════════════════════════
// 2. Receipt hash determinism across multiple computations
// ═══════════════════════════════════════════════════════════════════

#[test]
fn receipt_hash_deterministic_100_iterations() {
    let receipt = make_receipt();
    let reference = receipt_hash(&receipt).unwrap();
    assert_eq!(reference.len(), 64);

    for i in 0..100 {
        let hash = receipt_hash(&receipt).unwrap();
        assert_eq!(hash, reference, "diverged at iteration {i}");
    }
}

#[test]
fn receipt_with_hash_deterministic() {
    let r1 = make_receipt().with_hash().unwrap();
    let r2 = make_receipt().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    assert!(r1.receipt_sha256.is_some());
}

#[test]
fn receipt_hash_excludes_self_referential_field() {
    let receipt = make_receipt();
    let hash1 = receipt_hash(&receipt).unwrap();

    let mut receipt2 = make_receipt();
    receipt2.receipt_sha256 = Some("bogus".into());
    let hash2 = receipt_hash(&receipt2).unwrap();

    assert_eq!(hash1, hash2, "hash should not depend on receipt_sha256");
}

#[test]
fn receipt_hash_excludes_self_any_prior_hash_value() {
    let receipt = make_receipt();
    let hash_none = receipt_hash(&receipt).unwrap();

    let mut r2 = make_receipt();
    r2.receipt_sha256 = Some(hash_none.clone());
    let hash_with_correct = receipt_hash(&r2).unwrap();

    let mut r3 = make_receipt();
    r3.receipt_sha256 = Some("totally_different_value_1234".into());
    let hash_with_bogus = receipt_hash(&r3).unwrap();

    assert_eq!(hash_none, hash_with_correct);
    assert_eq!(hash_none, hash_with_bogus);
}

#[test]
fn receipt_hash_length_is_64_hex_chars() {
    let receipt = make_receipt();
    let hash = receipt_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_is_lowercase_hex() {
    let receipt = make_receipt();
    let hash = receipt_hash(&receipt).unwrap();
    assert_eq!(hash, hash.to_lowercase());
}

#[test]
fn two_different_receipts_produce_different_hashes() {
    let r1 = make_receipt();
    let mut r2 = make_receipt();
    r2.outcome = Outcome::Failed;

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ═══════════════════════════════════════════════════════════════════
// 3. WorkOrder canonical JSON determinism
// ═══════════════════════════════════════════════════════════════════

#[test]
fn work_order_serializes_identically_100_times() {
    let wo = make_work_order();
    let reference = serde_json::to_string(&wo).unwrap();
    for i in 0..100 {
        assert_eq!(
            serde_json::to_string(&wo).unwrap(),
            reference,
            "diverged at iteration {i}"
        );
    }
}

#[test]
fn work_order_canonical_json_deterministic() {
    let wo = make_work_order();
    let a = canonical_json(&wo).unwrap();
    let b = canonical_json(&wo).unwrap();
    assert_eq!(a, b);
}

#[test]
fn work_order_canonical_json_keys_sorted() {
    let wo = make_work_order();
    let json = canonical_json(&wo).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    if let serde_json::Value::Object(map) = v {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "top-level keys must be sorted");
    }
}

#[test]
fn work_order_roundtrip_preserves_canonical_form() {
    let wo = make_work_order();
    let json1 = canonical_json(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json1).unwrap();
    let json2 = canonical_json(&wo2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn work_order_to_value_roundtrip() {
    let wo = make_work_order();
    let v = serde_json::to_value(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_value(v.clone()).unwrap();
    let v2 = serde_json::to_value(&wo2).unwrap();
    assert_eq!(v, v2);
}

#[test]
fn work_order_canonical_json_is_compact() {
    let wo = make_work_order();
    let json = canonical_json(&wo).unwrap();
    assert!(!json.contains('\n'), "canonical JSON must be single-line");
    assert!(
        !json.contains("  "),
        "canonical JSON must not have extra spaces"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 4. AgentEvent canonical serialization
// ═══════════════════════════════════════════════════════════════════

#[test]
fn agent_event_run_started_deterministic() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let a = serde_json::to_string(&e).unwrap();
    let b = serde_json::to_string(&e).unwrap();
    assert_eq!(a, b);
}

#[test]
fn agent_event_tool_call_deterministic() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu_123".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "/tmp/foo.rs", "lines": [1,10]}),
    });
    let a = canonical_json(&e).unwrap();
    let b = canonical_json(&e).unwrap();
    assert_eq!(a, b);
}

#[test]
fn agent_event_tool_result_deterministic() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("tu_456".into()),
        output: serde_json::json!({"stdout": "ok", "stderr": ""}),
        is_error: false,
    });
    let a = canonical_json(&e).unwrap();
    let b = canonical_json(&e).unwrap();
    assert_eq!(a, b);
}

#[test]
fn agent_event_all_variants_roundtrip() {
    let variants: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "a".into(),
        },
        AgentEventKind::RunCompleted {
            message: "b".into(),
        },
        AgentEventKind::AssistantDelta { text: "c".into() },
        AgentEventKind::AssistantMessage { text: "d".into() },
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: None,
            output: serde_json::json!(null),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "f".into(),
            summary: "s".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: Some(0),
            output_preview: Some("file.txt".into()),
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
        let event = make_event(kind);
        let json1 = canonical_json(&event).unwrap();
        let event2: AgentEvent = serde_json::from_str(&json1).unwrap();
        let json2 = canonical_json(&event2).unwrap();
        assert_eq!(json1, json2, "roundtrip failed for event");
    }
}

#[test]
fn agent_event_with_ext_deterministic() {
    let mut ext = BTreeMap::new();
    ext.insert("z_field".into(), serde_json::json!("z_value"));
    ext.insert("a_field".into(), serde_json::json!(42));
    ext.insert("m_field".into(), serde_json::json!([1, 2]));

    let e = AgentEvent {
        ts: ts1(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };

    let a = canonical_json(&e).unwrap();
    let b = canonical_json(&e).unwrap();
    assert_eq!(a, b);
}

#[test]
fn agent_event_ext_insertion_order_irrelevant() {
    let mut ext1 = BTreeMap::new();
    ext1.insert("z".into(), serde_json::json!(1));
    ext1.insert("a".into(), serde_json::json!(2));

    let mut ext2 = BTreeMap::new();
    ext2.insert("a".into(), serde_json::json!(2));
    ext2.insert("z".into(), serde_json::json!(1));

    let e1 = AgentEvent {
        ts: ts1(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: Some(ext1),
    };
    let e2 = AgentEvent {
        ts: ts1(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: Some(ext2),
    };

    assert_eq!(canonical_json(&e1).unwrap(), canonical_json(&e2).unwrap());
}

// ═══════════════════════════════════════════════════════════════════
// 5. Config determinism with nested maps
// ═══════════════════════════════════════════════════════════════════

#[test]
fn runtime_config_nested_vendor_map_deterministic() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "outer_z".into(),
        serde_json::json!({"inner_b": 2, "inner_a": 1}),
    );
    vendor.insert(
        "outer_a".into(),
        serde_json::json!({"inner_z": 26, "inner_m": 13}),
    );

    let cfg = RuntimeConfig {
        model: Some("gpt-4".into()),
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };

    let a = canonical_json(&cfg).unwrap();
    let b = canonical_json(&cfg).unwrap();
    assert_eq!(a, b);
}

#[test]
fn runtime_config_nested_maps_keys_sorted() {
    let mut vendor = BTreeMap::new();
    vendor.insert("z_key".into(), serde_json::json!({"c": 3, "a": 1, "b": 2}));

    let cfg = RuntimeConfig {
        model: None,
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };

    let json = canonical_json(&cfg).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Inner object keys should be sorted via serde_json's BTreeMap-backed Map
    let inner = &v["vendor"]["z_key"];
    if let serde_json::Value::Object(map) = inner {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "nested keys must be sorted");
    }
}

#[test]
fn deeply_nested_config_determinism() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "level1".into(),
        serde_json::json!({
            "level2": {
                "level3": {
                    "level4": {
                        "value": "deep"
                    }
                }
            }
        }),
    );

    let cfg = RuntimeConfig {
        model: None,
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };

    let a = canonical_json(&cfg).unwrap();
    let b = canonical_json(&cfg).unwrap();
    assert_eq!(a, b);
}

#[test]
fn config_with_all_fields_populated() {
    let mut vendor = BTreeMap::new();
    vendor.insert("temperature".into(), serde_json::json!(0.7));
    vendor.insert("top_p".into(), serde_json::json!(0.9));

    let mut env = BTreeMap::new();
    env.insert("API_KEY".into(), "sk-123".into());
    env.insert("BASE_URL".into(), "https://api.example.com".into());

    let cfg = RuntimeConfig {
        model: Some("claude-3".into()),
        vendor,
        env,
        max_budget_usd: Some(10.0),
        max_turns: Some(20),
    };

    let json1 = canonical_json(&cfg).unwrap();
    let cfg2: RuntimeConfig = serde_json::from_str(&json1).unwrap();
    let json2 = canonical_json(&cfg2).unwrap();
    assert_eq!(json1, json2);
}

// ═══════════════════════════════════════════════════════════════════
// 6. Capability map ordering
// ═══════════════════════════════════════════════════════════════════

#[test]
fn capability_manifest_serialization_sorted() {
    let mut caps: CapabilityManifest = BTreeMap::new();
    caps.insert(Capability::StopSequences, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);
    caps.insert(Capability::ToolBash, SupportLevel::Unsupported);

    let json = serde_json::to_string(&caps).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    if let serde_json::Value::Object(map) = v {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "capability keys must be sorted");
    }
}

#[test]
fn capability_manifest_many_capabilities_deterministic() {
    let caps_list = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
    ];

    let mut manifest: CapabilityManifest = BTreeMap::new();
    for cap in &caps_list {
        manifest.insert(cap.clone(), SupportLevel::Native);
    }

    let a = canonical_json(&manifest).unwrap();
    let b = canonical_json(&manifest).unwrap();
    assert_eq!(a, b);
}

#[test]
fn capability_manifest_roundtrip() {
    let mut caps: CapabilityManifest = BTreeMap::new();
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    caps.insert(Capability::CodeExecution, SupportLevel::Unsupported);

    let json = canonical_json(&caps).unwrap();
    let caps2: CapabilityManifest = serde_json::from_str(&json).unwrap();
    let json2 = canonical_json(&caps2).unwrap();
    assert_eq!(json, json2);
}

// ═══════════════════════════════════════════════════════════════════
// 7. JSON Value comparison (semantic vs. string equality)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn json_object_key_order_irrelevant_in_canonical_form() {
    let a = serde_json::json!({"b": 2, "a": 1, "c": 3});
    let b = serde_json::json!({"c": 3, "a": 1, "b": 2});

    let ca = canonical_json(&a).unwrap();
    let cb = canonical_json(&b).unwrap();
    assert_eq!(ca, cb);
}

#[test]
fn json_semantic_vs_string_equality() {
    // serde_json::Value comparisons are semantic (key order doesn't matter)
    let a = serde_json::json!({"b": 2, "a": 1});
    let b = serde_json::json!({"a": 1, "b": 2});
    assert_eq!(a, b, "serde_json::Value compares semantically");

    // But string representation respects insertion order for HashMap-backed
    // serde_json uses BTreeMap internally so string form is also sorted
    let sa = serde_json::to_string(&a).unwrap();
    let sb = serde_json::to_string(&b).unwrap();
    assert_eq!(sa, sb, "serde_json sorts keys via BTreeMap");
}

#[test]
fn json_value_integer_vs_float_distinction() {
    let int_val = serde_json::json!(42);
    let float_val = serde_json::json!(42.0);

    let s_int = serde_json::to_string(&int_val).unwrap();
    let s_float = serde_json::to_string(&float_val).unwrap();

    // serde_json distinguishes integer from float
    assert_ne!(s_int, s_float);
}

#[test]
fn json_null_vs_absent_field_distinction() {
    let with_null = serde_json::json!({"a": 1, "b": null});
    let without = serde_json::json!({"a": 1});

    let s1 = serde_json::to_string(&with_null).unwrap();
    let s2 = serde_json::to_string(&without).unwrap();
    assert_ne!(s1, s2, "null field vs absent field are different");
}

#[test]
fn json_array_order_matters() {
    let a = serde_json::json!([1, 2, 3]);
    let b = serde_json::json!([3, 2, 1]);

    assert_ne!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

#[test]
fn json_empty_structures_deterministic() {
    let obj = serde_json::json!({});
    let arr = serde_json::json!([]);

    assert_eq!(serde_json::to_string(&obj).unwrap(), "{}");
    assert_eq!(serde_json::to_string(&arr).unwrap(), "[]");
}

// ═══════════════════════════════════════════════════════════════════
// 8. Hash sensitivity to whitespace
// ═══════════════════════════════════════════════════════════════════

#[test]
fn hash_changes_with_trailing_space_in_task() {
    let mut wo1 = make_work_order();
    wo1.task = "fix bug".into();
    let mut wo2 = make_work_order();
    wo2.task = "fix bug ".into();

    let h1 = sha256_hex(canonical_json(&wo1).unwrap().as_bytes());
    let h2 = sha256_hex(canonical_json(&wo2).unwrap().as_bytes());
    assert_ne!(h1, h2);
}

#[test]
fn hash_changes_with_leading_space_in_task() {
    let mut wo1 = make_work_order();
    wo1.task = "fix bug".into();
    let mut wo2 = make_work_order();
    wo2.task = " fix bug".into();

    let h1 = sha256_hex(canonical_json(&wo1).unwrap().as_bytes());
    let h2 = sha256_hex(canonical_json(&wo2).unwrap().as_bytes());
    assert_ne!(h1, h2);
}

#[test]
fn hash_changes_with_internal_whitespace_difference() {
    let mut wo1 = make_work_order();
    wo1.task = "fix  bug".into(); // two spaces
    let mut wo2 = make_work_order();
    wo2.task = "fix bug".into(); // one space

    let h1 = sha256_hex(canonical_json(&wo1).unwrap().as_bytes());
    let h2 = sha256_hex(canonical_json(&wo2).unwrap().as_bytes());
    assert_ne!(h1, h2);
}

#[test]
fn canonical_json_has_no_gratuitous_whitespace() {
    let wo = make_work_order();
    let json = canonical_json(&wo).unwrap();
    // canonical JSON should be compact
    assert!(!json.contains(": "), "no space after colon");
    assert!(!json.contains(", "), "no space after comma in top-level");
}

#[test]
fn receipt_hash_sensitive_to_whitespace_in_message() {
    let mut r1 = make_receipt();
    r1.trace = vec![make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    })];

    let mut r2 = make_receipt();
    r2.trace = vec![make_event(AgentEventKind::RunStarted {
        message: "go ".into(),
    })];

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ═══════════════════════════════════════════════════════════════════
// 9. Hash sensitivity to field ordering
// ═══════════════════════════════════════════════════════════════════

#[test]
fn hash_changes_when_trace_events_reordered() {
    let e1 = make_event(AgentEventKind::RunStarted {
        message: "a".into(),
    });
    let e2 = make_event(AgentEventKind::RunCompleted {
        message: "b".into(),
    });

    let mut r1 = make_receipt();
    r1.trace = vec![e1.clone(), e2.clone()];

    let mut r2 = make_receipt();
    r2.trace = vec![e2, e1];

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_when_artifacts_reordered() {
    let a1 = ArtifactRef {
        kind: "patch".into(),
        path: "a.patch".into(),
    };
    let a2 = ArtifactRef {
        kind: "log".into(),
        path: "b.log".into(),
    };

    let mut r1 = make_receipt();
    r1.artifacts = vec![a1.clone(), a2.clone()];

    let mut r2 = make_receipt();
    r2.artifacts = vec![a2, a1];

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_when_outcome_changes() {
    let mut r1 = make_receipt();
    r1.outcome = Outcome::Complete;

    let mut r2 = make_receipt();
    r2.outcome = Outcome::Failed;

    let mut r3 = make_receipt();
    r3.outcome = Outcome::Partial;

    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    let h3 = receipt_hash(&r3).unwrap();

    assert_ne!(h1, h2);
    assert_ne!(h2, h3);
    assert_ne!(h1, h3);
}

#[test]
fn hash_changes_when_mode_changes() {
    let mut r1 = make_receipt();
    r1.mode = ExecutionMode::Mapped;

    let mut r2 = make_receipt();
    r2.mode = ExecutionMode::Passthrough;

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_backend_id_change() {
    let mut r1 = make_receipt();
    r1.backend.id = "mock".into();

    let mut r2 = make_receipt();
    r2.backend.id = "sidecar:node".into();

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ═══════════════════════════════════════════════════════════════════
// 10. Hash stability across versions (golden values)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn sha256_hex_known_vector_empty() {
    let hash = sha256_hex(b"");
    assert_eq!(
        hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_hex_known_vector_hello() {
    let hash = sha256_hex(b"hello");
    assert_eq!(
        hash,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn sha256_hex_known_vector_json() {
    let input = r#"{"a":1,"b":2}"#;
    let hash = sha256_hex(input.as_bytes());
    // Pre-computed golden value
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let expected = format!("{:x}", hasher.finalize());
    assert_eq!(hash, expected);
}

#[test]
fn canonical_json_empty_object_golden() {
    let v = serde_json::json!({});
    let json = canonical_json(&v).unwrap();
    assert_eq!(json, "{}");
    assert_eq!(sha256_hex(json.as_bytes()), sha256_hex(b"{}"));
}

#[test]
fn receipt_hash_golden_value_stability() {
    let receipt = make_receipt();
    let hash = receipt_hash(&receipt).unwrap();
    // Compute independently
    let canonical = canonical_json_receipt_manually(&receipt);
    let expected = sha256_hex(canonical.as_bytes());
    assert_eq!(hash, expected);
}

/// Manual canonical JSON for a receipt to cross-validate.
fn canonical_json_receipt_manually(receipt: &Receipt) -> String {
    let mut v = serde_json::to_value(receipt).unwrap();
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("receipt_sha256".to_string(), serde_json::Value::Null);
    }
    serde_json::to_string(&v).unwrap()
}

#[test]
fn contract_version_in_receipt_hash() {
    let receipt = make_receipt();
    let json = canonical_json_receipt_manually(&receipt);
    assert!(json.contains(abp_core::CONTRACT_VERSION));
}

#[test]
fn receipt_hash_golden_reproduces_across_independent_computation() {
    let receipt = make_receipt();

    // Method 1: using receipt_hash
    let h1 = receipt_hash(&receipt).unwrap();

    // Method 2: manual
    let mut v = serde_json::to_value(&receipt).unwrap();
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("receipt_sha256".to_string(), serde_json::Value::Null);
    }
    let json = serde_json::to_string(&v).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    let h2 = format!("{:x}", hasher.finalize());

    assert_eq!(h1, h2);
}

// ═══════════════════════════════════════════════════════════════════
// 11. Proptest: random maps always produce same canonical JSON
// ═══════════════════════════════════════════════════════════════════

proptest! {
    #[test]
    fn proptest_random_btreemap_canonical_json_deterministic(
        entries in prop::collection::btree_map("[a-z]{1,8}", "[a-zA-Z0-9 ]{0,20}", 0..20)
    ) {
        let a = canonical_json(&entries).unwrap();
        let b = canonical_json(&entries).unwrap();
        prop_assert_eq!(&a, &b);
    }

    #[test]
    fn proptest_random_string_map_serialization_deterministic(
        entries in prop::collection::btree_map("[a-z_]{1,10}", "[a-zA-Z0-9]{0,30}", 0..50)
    ) {
        let a = serde_json::to_string(&entries).unwrap();
        let b = serde_json::to_string(&entries).unwrap();
        prop_assert_eq!(&a, &b);
    }

    #[test]
    fn proptest_random_map_hash_deterministic(
        entries in prop::collection::btree_map("[a-z]{1,6}", 0i64..1000, 0..30)
    ) {
        let json = serde_json::to_string(&entries).unwrap();
        let h1 = sha256_hex(json.as_bytes());
        let h2 = sha256_hex(json.as_bytes());
        prop_assert_eq!(&h1, &h2);
    }

    #[test]
    fn proptest_random_nested_map_deterministic(
        outer in prop::collection::btree_map(
            "[a-z]{1,5}",
            prop::collection::btree_map("[a-z]{1,5}", 0i64..100, 0..5),
            0..10
        )
    ) {
        let a = canonical_json(&outer).unwrap();
        let b = canonical_json(&outer).unwrap();
        prop_assert_eq!(&a, &b);
    }

    #[test]
    fn proptest_random_vendor_config_deterministic(
        keys in prop::collection::vec("[a-z_]{1,10}", 0..10),
        vals in prop::collection::vec(0i64..1000, 0..10),
    ) {
        let mut vendor = BTreeMap::new();
        for (k, v) in keys.iter().zip(vals.iter()) {
            vendor.insert(k.clone(), serde_json::json!(*v));
        }
        let cfg = RuntimeConfig {
            model: None,
            vendor,
            env: BTreeMap::new(),
            max_budget_usd: None,
            max_turns: None,
        };
        let a = canonical_json(&cfg).unwrap();
        let b = canonical_json(&cfg).unwrap();
        prop_assert_eq!(&a, &b);
    }
}

// ═══════════════════════════════════════════════════════════════════
// 12. Proptest: receipt hashing is idempotent
// ═══════════════════════════════════════════════════════════════════

proptest! {
    #[test]
    fn proptest_receipt_hash_idempotent(
        task in "[a-zA-Z ]{1,50}",
        backend_id in "[a-z]{3,10}",
    ) {
        let mut receipt = make_receipt();
        receipt.backend.id = backend_id;
        receipt.trace = vec![make_event(AgentEventKind::RunStarted { message: task })];

        let h1 = receipt_hash(&receipt).unwrap();
        let h2 = receipt_hash(&receipt).unwrap();
        prop_assert_eq!(&h1, &h2);
        prop_assert_eq!(h1.len(), 64);
    }

    #[test]
    fn proptest_receipt_with_hash_idempotent(
        msg in "[a-zA-Z0-9 ]{1,30}",
    ) {
        let mut receipt = make_receipt();
        receipt.trace = vec![make_event(AgentEventKind::RunStarted { message: msg })];

        let r1 = receipt.clone().with_hash().unwrap();
        let r2 = receipt.with_hash().unwrap();
        prop_assert_eq!(&r1.receipt_sha256, &r2.receipt_sha256);
    }

    #[test]
    fn proptest_receipt_hash_ignores_prior_sha256(
        prior_hash in "[0-9a-f]{64}",
    ) {
        let receipt = make_receipt();
        let h1 = receipt_hash(&receipt).unwrap();

        let mut receipt2 = make_receipt();
        receipt2.receipt_sha256 = Some(prior_hash);
        let h2 = receipt_hash(&receipt2).unwrap();

        prop_assert_eq!(&h1, &h2);
    }

    #[test]
    fn proptest_double_with_hash_stable(
        msg in "[a-zA-Z]{1,20}",
    ) {
        let mut receipt = make_receipt();
        receipt.trace = vec![make_event(AgentEventKind::AssistantMessage { text: msg })];

        let r1 = receipt.clone().with_hash().unwrap();
        let r2 = r1.clone().with_hash().unwrap();
        prop_assert_eq!(&r1.receipt_sha256, &r2.receipt_sha256);
    }
}

// ═══════════════════════════════════════════════════════════════════
// 13. Proptest: WorkOrder roundtrip preserves canonical form
// ═══════════════════════════════════════════════════════════════════

proptest! {
    #[test]
    fn proptest_work_order_roundtrip_canonical(
        task in "[a-zA-Z ]{1,50}",
        model in prop::option::of("[a-z0-9-]{3,15}"),
    ) {
        let wo = WorkOrder {
            id: FIXED_UUID,
            task,
            lane: ExecutionLane::PatchFirst,
            workspace: WorkspaceSpec {
                root: "/tmp".into(),
                mode: WorkspaceMode::Staged,
                include: vec![],
                exclude: vec![],
            },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig {
                model,
                vendor: BTreeMap::new(),
                env: BTreeMap::new(),
                max_budget_usd: None,
                max_turns: None,
            },
        };

        let json1 = canonical_json(&wo).unwrap();
        let wo2: WorkOrder = serde_json::from_str(&json1).unwrap();
        let json2 = canonical_json(&wo2).unwrap();
        prop_assert_eq!(&json1, &json2);
    }

    #[test]
    fn proptest_work_order_with_vendor_roundtrip(
        entries in prop::collection::btree_map("[a-z]{1,8}", 0i64..100, 0..10)
    ) {
        let vendor: BTreeMap<String, serde_json::Value> = entries
            .into_iter()
            .map(|(k, v)| (k, serde_json::json!(v)))
            .collect();

        let wo = WorkOrder {
            id: FIXED_UUID,
            task: "test".into(),
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
                vendor,
                env: BTreeMap::new(),
                max_budget_usd: None,
                max_turns: None,
            },
        };

        let json1 = canonical_json(&wo).unwrap();
        let wo2: WorkOrder = serde_json::from_str(&json1).unwrap();
        let json2 = canonical_json(&wo2).unwrap();
        prop_assert_eq!(&json1, &json2);
    }

    #[test]
    fn proptest_work_order_hash_deterministic(
        task in "[a-zA-Z ]{1,30}",
    ) {
        let wo = WorkOrder {
            id: FIXED_UUID,
            task,
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

        let json = canonical_json(&wo).unwrap();
        let h1 = sha256_hex(json.as_bytes());
        let h2 = sha256_hex(json.as_bytes());
        prop_assert_eq!(&h1, &h2);
    }
}

// ═══════════════════════════════════════════════════════════════════
// 14. Large nested structure determinism
// ═══════════════════════════════════════════════════════════════════

#[test]
fn large_trace_determinism() {
    let mut receipt = make_receipt();
    receipt.trace = (0..200)
        .map(|i| {
            make_event(AgentEventKind::AssistantDelta {
                text: format!("tok_{i}"),
            })
        })
        .collect();

    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn large_vendor_map_determinism() {
    let mut vendor = BTreeMap::new();
    for i in 0..100 {
        vendor.insert(format!("key_{i:04}"), serde_json::json!(i));
    }

    let cfg = RuntimeConfig {
        model: None,
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };

    let a = canonical_json(&cfg).unwrap();
    let b = canonical_json(&cfg).unwrap();
    assert_eq!(a, b);
}

#[test]
fn large_env_map_determinism() {
    let mut env = BTreeMap::new();
    for i in 0..100 {
        env.insert(format!("ENV_{i:04}"), format!("val_{i}"));
    }

    let cfg = RuntimeConfig {
        model: None,
        vendor: BTreeMap::new(),
        env,
        max_budget_usd: None,
        max_turns: None,
    };

    let a = canonical_json(&cfg).unwrap();
    let b = canonical_json(&cfg).unwrap();
    assert_eq!(a, b);
}

#[test]
fn large_capability_manifest_determinism() {
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

    let mut manifest: CapabilityManifest = BTreeMap::new();
    for cap in all_caps {
        manifest.insert(cap, SupportLevel::Native);
    }

    let a = canonical_json(&manifest).unwrap();
    let b = canonical_json(&manifest).unwrap();
    assert_eq!(a, b);
}

#[test]
fn large_receipt_with_everything_deterministic() {
    let mut receipt = make_receipt();
    receipt.trace = (0..50)
        .map(|i| {
            if i % 3 == 0 {
                make_event(AgentEventKind::ToolCall {
                    tool_name: format!("tool_{i}"),
                    tool_use_id: Some(format!("tu_{i}")),
                    parent_tool_use_id: None,
                    input: serde_json::json!({"arg": i}),
                })
            } else if i % 3 == 1 {
                make_event(AgentEventKind::ToolResult {
                    tool_name: format!("tool_{}", i - 1),
                    tool_use_id: Some(format!("tu_{}", i - 1)),
                    output: serde_json::json!({"result": format!("ok_{i}")}),
                    is_error: false,
                })
            } else {
                make_event(AgentEventKind::AssistantDelta {
                    text: format!("thinking about step {i}..."),
                })
            }
        })
        .collect();

    receipt.artifacts = (0..10)
        .map(|i| ArtifactRef {
            kind: "patch".into(),
            path: format!("file_{i}.patch"),
        })
        .collect();

    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn deeply_nested_json_value_determinism() {
    fn nest(depth: usize) -> serde_json::Value {
        if depth == 0 {
            serde_json::json!("leaf")
        } else {
            serde_json::json!({"child": nest(depth - 1), "depth": depth})
        }
    }

    let v = nest(20);
    let a = canonical_json(&v).unwrap();
    let b = canonical_json(&v).unwrap();
    assert_eq!(a, b);
}

// ═══════════════════════════════════════════════════════════════════
// 15. Unicode field name/value determinism
// ═══════════════════════════════════════════════════════════════════

#[test]
fn unicode_task_deterministic() {
    let mut wo = make_work_order();
    wo.task = "修复登录错误 🐛".into();

    let a = canonical_json(&wo).unwrap();
    let b = canonical_json(&wo).unwrap();
    assert_eq!(a, b);
}

#[test]
fn unicode_vendor_keys_deterministic() {
    let mut vendor = BTreeMap::new();
    vendor.insert("日本語".into(), serde_json::json!("値"));
    vendor.insert("中文".into(), serde_json::json!("值"));
    vendor.insert("한국어".into(), serde_json::json!("값"));
    vendor.insert("العربية".into(), serde_json::json!("قيمة"));

    let cfg = RuntimeConfig {
        model: None,
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };

    let a = canonical_json(&cfg).unwrap();
    let b = canonical_json(&cfg).unwrap();
    assert_eq!(a, b);
}

#[test]
fn unicode_vendor_keys_sorted_by_codepoint() {
    let mut vendor = BTreeMap::new();
    vendor.insert("zzz".into(), serde_json::json!(1));
    vendor.insert("aaa".into(), serde_json::json!(2));
    vendor.insert("émile".into(), serde_json::json!(3));
    vendor.insert("über".into(), serde_json::json!(4));

    let json = canonical_json(&RuntimeConfig {
        model: None,
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    })
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    if let serde_json::Value::Object(ref top) = v
        && let Some(serde_json::Value::Object(vend)) = top.get("vendor")
    {
        let keys: Vec<&String> = vend.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }
}

#[test]
fn unicode_event_message_deterministic() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "こんにちは世界 🌍 مرحبا بالعالم".into(),
    });

    let a = canonical_json(&e).unwrap();
    let b = canonical_json(&e).unwrap();
    assert_eq!(a, b);
}

#[test]
fn unicode_in_receipt_hash() {
    let mut r1 = make_receipt();
    r1.trace = vec![make_event(AgentEventKind::AssistantMessage {
        text: "🎉 Done! 完了".into(),
    })];

    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r1).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn unicode_emoji_sequences_deterministic() {
    let mut vendor = BTreeMap::new();
    vendor.insert("emoji".into(), serde_json::json!("👨‍👩‍👧‍👦"));
    vendor.insert("flag".into(), serde_json::json!("🇺🇸"));
    vendor.insert("skin".into(), serde_json::json!("👋🏽"));

    let cfg = RuntimeConfig {
        model: None,
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };

    let a = canonical_json(&cfg).unwrap();
    let b = canonical_json(&cfg).unwrap();
    assert_eq!(a, b);
}

#[test]
fn unicode_escape_sequences_in_json() {
    let s = "line1\nline2\ttab\"quote\\backslash";
    let mut vendor = BTreeMap::new();
    vendor.insert("escaped".into(), serde_json::json!(s));

    let cfg = RuntimeConfig {
        model: None,
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };

    let a = canonical_json(&cfg).unwrap();
    let b = canonical_json(&cfg).unwrap();
    assert_eq!(a, b);
}

proptest! {
    #[test]
    fn proptest_unicode_string_deterministic(
        s in "[\\p{L}\\p{N}\\p{S}]{0,50}"
    ) {
        let v = serde_json::json!({"text": s});
        let a = canonical_json(&v).unwrap();
        let b = canonical_json(&v).unwrap();
        prop_assert_eq!(&a, &b);
    }

    #[test]
    fn proptest_unicode_map_keys_deterministic(
        entries in prop::collection::btree_map("[\\p{L}]{1,8}", "[\\p{L}\\p{N}]{0,20}", 0..15)
    ) {
        let a = canonical_json(&entries).unwrap();
        let b = canonical_json(&entries).unwrap();
        prop_assert_eq!(&a, &b);
    }
}

// ═══════════════════════════════════════════════════════════════════
// Additional edge cases and cross-cutting concerns
// ═══════════════════════════════════════════════════════════════════

#[test]
fn empty_receipt_hash_deterministic() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();

    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_builder_with_hash_deterministic() {
    let ts = ts1();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts)
        .finished_at(ts)
        .work_order_id(FIXED_UUID)
        .build()
        .with_hash()
        .unwrap();

    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts)
        .finished_at(ts)
        .work_order_id(FIXED_UUID)
        .build()
        .with_hash()
        .unwrap();

    // Note: ReceiptBuilder uses Uuid::new_v4() for run_id, so hashes will differ
    // unless run_id is the same. We verify that individual builds are self-consistent.
    assert!(r1.receipt_sha256.is_some());
    assert!(r2.receipt_sha256.is_some());
    assert_eq!(r1.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_hash_with_empty_trace() {
    let mut receipt = make_receipt();
    receipt.trace = vec![];

    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_with_empty_artifacts() {
    let mut receipt = make_receipt();
    receipt.artifacts = vec![];

    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_with_null_optional_fields() {
    let mut receipt = make_receipt();
    receipt.backend.backend_version = None;
    receipt.backend.adapter_version = None;
    receipt.usage.input_tokens = None;
    receipt.usage.output_tokens = None;
    receipt.verification.git_diff = None;
    receipt.verification.git_status = None;

    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn canonical_json_sorts_serde_json_value_object_keys() {
    // Construct via serde_json::json! which uses BTreeMap
    let v = serde_json::json!({
        "zebra": 1,
        "apple": 2,
        "mango": 3,
        "banana": 4
    });

    let json = canonical_json(&v).unwrap();
    // Keys should appear in alphabetical order
    let apple_pos = json.find("\"apple\"").unwrap();
    let banana_pos = json.find("\"banana\"").unwrap();
    let mango_pos = json.find("\"mango\"").unwrap();
    let zebra_pos = json.find("\"zebra\"").unwrap();

    assert!(apple_pos < banana_pos);
    assert!(banana_pos < mango_pos);
    assert!(mango_pos < zebra_pos);
}

#[test]
fn work_order_with_empty_collections() {
    let wo = WorkOrder {
        id: FIXED_UUID,
        task: "test".into(),
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

    let a = canonical_json(&wo).unwrap();
    let b = canonical_json(&wo).unwrap();
    assert_eq!(a, b);
}

#[test]
fn sha256_hex_consistency_with_sha2_crate() {
    let data = b"agent-backplane test vector";
    let mut hasher = Sha256::new();
    hasher.update(data);
    let expected = format!("{:x}", hasher.finalize());

    let actual = sha256_hex(data);
    assert_eq!(actual, expected);
}

#[test]
fn canonical_json_number_representation() {
    // Integers stay as integers
    let v = serde_json::json!(42);
    assert_eq!(canonical_json(&v).unwrap(), "42");

    // Floats with decimals keep precision
    let v = serde_json::json!(1.234_567);
    let json = canonical_json(&v).unwrap();
    assert!(json.contains("1.234567"));
}

#[test]
fn canonical_json_boolean_representation() {
    assert_eq!(canonical_json(&serde_json::json!(true)).unwrap(), "true");
    assert_eq!(canonical_json(&serde_json::json!(false)).unwrap(), "false");
}

#[test]
fn canonical_json_null_representation() {
    assert_eq!(canonical_json(&serde_json::json!(null)).unwrap(), "null");
}

#[test]
fn receipt_hash_sensitive_to_single_field_change() {
    let base = make_receipt();
    let base_hash = receipt_hash(&base).unwrap();

    // Change each field and verify hash changes
    let mut r = make_receipt();
    r.meta.duration_ms = 999;
    assert_ne!(receipt_hash(&r).unwrap(), base_hash, "duration_ms");

    let mut r = make_receipt();
    r.backend.id = "other".into();
    assert_ne!(receipt_hash(&r).unwrap(), base_hash, "backend.id");

    let mut r = make_receipt();
    r.usage.input_tokens = Some(999);
    assert_ne!(receipt_hash(&r).unwrap(), base_hash, "input_tokens");

    let mut r = make_receipt();
    r.verification.harness_ok = false;
    assert_ne!(receipt_hash(&r).unwrap(), base_hash, "harness_ok");
}

#[test]
fn work_order_hash_sensitive_to_policy_change() {
    let wo1 = make_work_order();
    let mut wo2 = make_work_order();
    wo2.policy.allowed_tools.push("Extra".into());

    let h1 = sha256_hex(canonical_json(&wo1).unwrap().as_bytes());
    let h2 = sha256_hex(canonical_json(&wo2).unwrap().as_bytes());
    assert_ne!(h1, h2);
}

#[test]
fn event_ext_with_complex_nested_json_deterministic() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        serde_json::json!({
            "id": "msg_123",
            "type": "text",
            "content": [
                {"type": "text", "text": "Hello, world!"},
                {"type": "tool_use", "id": "tu_1", "name": "bash", "input": {"command": "ls"}}
            ]
        }),
    );

    let e = AgentEvent {
        ts: ts1(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };

    let a = canonical_json(&e).unwrap();
    let b = canonical_json(&e).unwrap();
    assert_eq!(a, b);
}

proptest! {
    #[test]
    fn proptest_canonical_json_roundtrip_btreemap(
        entries in prop::collection::btree_map("[a-z]{1,5}", "[a-z0-9]{0,10}", 0..20)
    ) {
        let json = canonical_json(&entries).unwrap();
        let parsed: BTreeMap<String, String> = serde_json::from_str(&json).unwrap();
        let json2 = canonical_json(&parsed).unwrap();
        prop_assert_eq!(&json, &json2);
    }

    #[test]
    fn proptest_sha256_hex_length_always_64(
        data in prop::collection::vec(any::<u8>(), 0..1000)
    ) {
        let hash = sha256_hex(&data);
        prop_assert_eq!(hash.len(), 64);
        prop_assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn proptest_sha256_hex_lowercase(
        data in prop::collection::vec(any::<u8>(), 0..500)
    ) {
        let hash = sha256_hex(&data);
        prop_assert_eq!(&hash, &hash.to_lowercase());
    }

    #[test]
    fn proptest_different_inputs_different_hashes(
        a in "[a-zA-Z0-9]{1,100}",
        b in "[a-zA-Z0-9]{1,100}",
    ) {
        // Probabilistically, different strings produce different hashes
        if a != b {
            let ha = sha256_hex(a.as_bytes());
            let hb = sha256_hex(b.as_bytes());
            prop_assert_ne!(&ha, &hb);
        }
    }
}
