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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
//! Deep receipt canonicalization and integrity test suite (60+ tests).
//!
//! Covers canonical JSON determinism, hash integrity, receipt chain validation,
//! and edge cases for the Agent Backplane receipt system.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, ExecutionMode, Outcome, Receipt, ReceiptBuilder, RunMetadata, SupportLevel,
    UsageNormalized, VerificationReport, canonical_json, receipt_hash, sha256_hex,
};
use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_ts() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap()
}

fn fixed_ts2() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 15, 12, 30, 0).unwrap()
}

fn minimal_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_ts(),
            finished_at: fixed_ts(),
            duration_ms: 0,
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

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: fixed_ts(),
        kind,
        ext: None,
    }
}

fn receipt_with_trace(events: Vec<AgentEvent>) -> Receipt {
    Receipt {
        trace: events,
        ..minimal_receipt()
    }
}

fn hash_receipt(r: &Receipt) -> String {
    receipt_hash(r).unwrap()
}

// ===========================================================================
// 1. Canonical JSON determinism (15 tests)
// ===========================================================================

#[test]
fn canonical_same_receipt_identical_json_bytes() {
    let r = minimal_receipt();
    let j1 = canonical_json(&r).unwrap();
    let j2 = canonical_json(&r).unwrap();
    assert_eq!(j1, j2, "same receipt must produce identical JSON bytes");
}

#[test]
fn canonical_repeated_serialization_stable() {
    let r = minimal_receipt();
    let jsons: Vec<String> = (0..100).map(|_| canonical_json(&r).unwrap()).collect();
    assert!(jsons.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn canonical_field_ordering_is_alphabetical() {
    let r = minimal_receipt();
    let json = canonical_json(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = v.as_object().unwrap();
    let keys: Vec<&String> = obj.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(
        keys, sorted,
        "top-level keys must be alphabetically ordered"
    );
}

#[test]
fn canonical_nested_meta_keys_alphabetical() {
    let r = minimal_receipt();
    let json = canonical_json(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let meta = v.get("meta").unwrap().as_object().unwrap();
    let keys: Vec<&String> = meta.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "meta keys must be alphabetically ordered");
}

#[test]
fn canonical_unicode_string_in_backend_id() {
    let mut r = minimal_receipt();
    r.backend.id = "バックエンド".into();
    let j1 = canonical_json(&r).unwrap();
    let j2 = canonical_json(&r).unwrap();
    assert_eq!(j1, j2);
    assert!(j1.contains("バックエンド"));
}

#[test]
fn canonical_unicode_emoji_in_trace() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::AssistantMessage {
        text: "Hello 🌍🚀 world!".into(),
    })]);
    let j1 = canonical_json(&r).unwrap();
    let j2 = canonical_json(&r).unwrap();
    assert_eq!(j1, j2);
    assert!(j1.contains("🌍🚀"));
}

#[test]
fn canonical_unicode_cjk_characters() {
    let mut r = minimal_receipt();
    r.verification.git_diff = Some("变更内容：测试用例".into());
    let json = canonical_json(&r).unwrap();
    assert!(json.contains("变更内容"));
}

#[test]
fn canonical_empty_vs_null_fields_differ() {
    let mut r1 = minimal_receipt();
    r1.backend.backend_version = None;
    let mut r2 = minimal_receipt();
    r2.backend.backend_version = Some(String::new());
    let j1 = canonical_json(&r1).unwrap();
    let j2 = canonical_json(&r2).unwrap();
    assert_ne!(j1, j2, "None vs Some(\"\") must produce different JSON");
}

#[test]
fn canonical_empty_vec_vs_populated_vec() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.artifacts.push(ArtifactRef {
        kind: "patch".into(),
        path: "a.patch".into(),
    });
    let j1 = canonical_json(&r1).unwrap();
    let j2 = canonical_json(&r2).unwrap();
    assert_ne!(j1, j2);
}

#[test]
fn canonical_nested_btreemap_ordering() {
    let mut r = minimal_receipt();
    let mut vendor = BTreeMap::new();
    vendor.insert("zebra".to_string(), serde_json::Value::String("z".into()));
    vendor.insert("alpha".to_string(), serde_json::Value::String("a".into()));
    r.usage_raw = serde_json::to_value(&vendor).unwrap();
    let json = canonical_json(&r).unwrap();
    let alpha_pos = json.find("\"alpha\"").unwrap();
    let zebra_pos = json.find("\"zebra\"").unwrap();
    assert!(
        alpha_pos < zebra_pos,
        "alpha must appear before zebra in canonical JSON"
    );
}

#[test]
fn canonical_capability_manifest_btreemap_ordering() {
    let mut r = minimal_receipt();
    r.capabilities
        .insert(Capability::ToolWrite, SupportLevel::Native);
    r.capabilities
        .insert(Capability::Streaming, SupportLevel::Native);
    r.capabilities
        .insert(Capability::ToolRead, SupportLevel::Emulated);
    let json = canonical_json(&r).unwrap();
    let streaming_pos = json.find("\"streaming\"").unwrap();
    let tool_read_pos = json.find("\"tool_read\"").unwrap();
    let tool_write_pos = json.find("\"tool_write\"").unwrap();
    assert!(streaming_pos < tool_read_pos);
    assert!(tool_read_pos < tool_write_pos);
}

#[test]
fn canonical_json_no_trailing_whitespace() {
    let r = minimal_receipt();
    let json = canonical_json(&r).unwrap();
    assert!(!json.ends_with(' '));
    assert!(!json.ends_with('\n'));
    assert!(!json.ends_with('\t'));
}

#[test]
fn canonical_json_is_compact() {
    let r = minimal_receipt();
    let json = canonical_json(&r).unwrap();
    assert!(
        !json.contains("  "),
        "compact JSON should have no indentation"
    );
}

#[test]
fn canonical_json_parseable_roundtrip() {
    let r = minimal_receipt();
    let json = canonical_json(&r).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&parsed).unwrap();
    assert_eq!(json, json2, "canonical JSON must roundtrip through Value");
}

#[test]
fn canonical_two_receipts_different_ids_different_json() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.run_id = Uuid::from_u128(1);
    let j1 = canonical_json(&r1).unwrap();
    let j2 = canonical_json(&r2).unwrap();
    assert_ne!(j1, j2);
}

// ===========================================================================
// 2. Hash integrity (15 tests)
// ===========================================================================

#[test]
fn hash_is_sha256_length() {
    let r = minimal_receipt();
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64, "SHA-256 hex digest must be 64 characters");
}

#[test]
fn hash_is_lowercase_hex() {
    let r = minimal_receipt();
    let h = hash_receipt(&r);
    assert!(
        h.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "hash must be lowercase hex"
    );
}

#[test]
fn hash_is_valid_hex() {
    let r = minimal_receipt();
    let h = hash_receipt(&r);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash_deterministic_multiple_calls() {
    let r = minimal_receipt();
    let hashes: Vec<String> = (0..50).map(|_| hash_receipt(&r)).collect();
    assert!(hashes.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn hash_changes_when_outcome_changes() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.outcome = Outcome::Failed;
    assert_ne!(hash_receipt(&r1), hash_receipt(&r2));
}

#[test]
fn hash_changes_when_backend_id_changes() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.backend.id = "other-backend".into();
    assert_ne!(hash_receipt(&r1), hash_receipt(&r2));
}

#[test]
fn hash_changes_when_trace_event_added() {
    let r1 = minimal_receipt();
    let r2 = receipt_with_trace(vec![make_event(AgentEventKind::RunStarted {
        message: "started".into(),
    })]);
    assert_ne!(hash_receipt(&r1), hash_receipt(&r2));
}

#[test]
fn hash_changes_when_duration_changes() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.duration_ms = 9999;
    assert_ne!(hash_receipt(&r1), hash_receipt(&r2));
}

#[test]
fn hash_changes_when_contract_version_changes() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.contract_version = "abp/v99.0".into();
    assert_ne!(hash_receipt(&r1), hash_receipt(&r2));
}

#[test]
fn hash_changes_when_verification_diff_added() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.verification.git_diff = Some("diff --git a/f b/f".into());
    assert_ne!(hash_receipt(&r1), hash_receipt(&r2));
}

#[test]
fn hash_receipt_sha256_excluded_from_input() {
    let r1 = minimal_receipt(); // receipt_sha256 = None
    let mut r2 = minimal_receipt();
    r2.receipt_sha256 = Some("deadbeef".repeat(8));
    // Both must hash the same because receipt_sha256 is set to null before hashing
    assert_eq!(hash_receipt(&r1), hash_receipt(&r2));
}

#[test]
fn hash_with_hash_is_idempotent() {
    let r = minimal_receipt();
    let r1 = r.clone().with_hash().unwrap();
    let h1 = r1.receipt_sha256.clone().unwrap();
    let r2 = r1.with_hash().unwrap();
    let h2 = r2.receipt_sha256.clone().unwrap();
    assert_eq!(h1, h2, "with_hash() must be idempotent");
}

#[test]
fn hash_with_hash_triple_idempotent() {
    let r = minimal_receipt();
    let h1 = r.clone().with_hash().unwrap().receipt_sha256.unwrap();
    let h2 = r
        .clone()
        .with_hash()
        .unwrap()
        .with_hash()
        .unwrap()
        .receipt_sha256
        .unwrap();
    let h3 = r
        .with_hash()
        .unwrap()
        .with_hash()
        .unwrap()
        .with_hash()
        .unwrap()
        .receipt_sha256
        .unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
}

#[test]
fn hash_survives_serialization_roundtrip() {
    let r = minimal_receipt().with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    // Re-hashing the deserialized receipt should yield the same hash
    let r3 = r2.with_hash().unwrap();
    assert_eq!(r.receipt_sha256, r3.receipt_sha256);
}

#[test]
fn hash_matches_manual_sha256_of_canonical_json() {
    let r = minimal_receipt();
    let h = hash_receipt(&r);
    // Manually reproduce the hashing logic
    let mut v = serde_json::to_value(&r).unwrap();
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("receipt_sha256".to_string(), serde_json::Value::Null);
    }
    let json = serde_json::to_string(&v).unwrap();
    let manual_hash = sha256_hex(json.as_bytes());
    assert_eq!(h, manual_hash);
}

// ===========================================================================
// 3. Receipt chain validation (15 tests)
// ===========================================================================

fn build_chain(len: usize) -> Vec<Receipt> {
    let mut chain = Vec::with_capacity(len);
    let mut parent_hash: Option<String> = None;
    for i in 0..len {
        let mut r = minimal_receipt();
        r.meta.run_id = Uuid::from_u128(i as u128 + 1);
        r.meta.duration_ms = i as u64;
        // Store parent hash in usage_raw for chain linkage
        if let Some(ref ph) = parent_hash {
            r.usage_raw = serde_json::json!({ "parent_receipt_hash": ph });
        }
        let r = r.with_hash().unwrap();
        parent_hash = r.receipt_sha256.clone();
        chain.push(r);
    }
    chain
}

fn verify_chain(chain: &[Receipt]) -> bool {
    for (i, receipt) in chain.iter().enumerate() {
        // Verify self-hash is valid
        let expected = hash_receipt(receipt);
        if receipt.receipt_sha256.as_deref() != Some(&expected) {
            return false;
        }
        // Verify parent linkage (except first)
        if i > 0 {
            let parent_hash = chain[i - 1].receipt_sha256.as_deref().unwrap();
            let stored_parent = receipt
                .usage_raw
                .get("parent_receipt_hash")
                .and_then(|v| v.as_str());
            if stored_parent != Some(parent_hash) {
                return false;
            }
        }
    }
    true
}

#[test]
fn chain_single_receipt_valid() {
    let chain = build_chain(1);
    assert!(verify_chain(&chain));
}

#[test]
fn chain_two_receipts_linked() {
    let chain = build_chain(2);
    assert!(verify_chain(&chain));
    let parent_hash = chain[0].receipt_sha256.as_deref().unwrap();
    let stored = chain[1]
        .usage_raw
        .get("parent_receipt_hash")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(parent_hash, stored);
}

#[test]
fn chain_ten_receipts_valid() {
    let chain = build_chain(10);
    assert!(verify_chain(&chain));
}

#[test]
fn chain_modification_breaks_integrity() {
    let mut chain = build_chain(5);
    // Tamper with middle receipt
    chain[2].outcome = Outcome::Failed;
    assert!(!verify_chain(&chain));
}

#[test]
fn chain_tampered_hash_detected() {
    let mut chain = build_chain(3);
    chain[1].receipt_sha256 = Some("0".repeat(64));
    assert!(!verify_chain(&chain));
}

#[test]
fn chain_parent_hash_tamper_detected() {
    let mut chain = build_chain(3);
    // Modify parent_receipt_hash in chain[2] to point to wrong parent
    chain[2].usage_raw = serde_json::json!({ "parent_receipt_hash": "0".repeat(64) });
    // Re-hash so self-hash is consistent, but parent link is wrong
    chain[2] = chain[2].clone().with_hash().unwrap();
    assert!(!verify_chain(&chain));
}

#[test]
fn chain_length_can_be_verified() {
    for len in [0, 1, 5, 20] {
        let chain = build_chain(len);
        assert_eq!(chain.len(), len);
        if !chain.is_empty() {
            assert!(verify_chain(&chain));
        }
    }
}

#[test]
fn chain_empty_is_valid() {
    let chain: Vec<Receipt> = vec![];
    assert!(verify_chain(&chain));
}

#[test]
fn chain_each_receipt_has_unique_hash() {
    let chain = build_chain(10);
    let hashes: Vec<&str> = chain
        .iter()
        .map(|r| r.receipt_sha256.as_deref().unwrap())
        .collect();
    let unique: std::collections::HashSet<&&str> = hashes.iter().collect();
    assert_eq!(hashes.len(), unique.len(), "all hashes must be unique");
}

#[test]
fn chain_timestamps_dont_affect_structure() {
    let chain1 = build_chain(3);
    let mut chain2 = build_chain(3);
    // Change timestamps in chain2
    for r in &mut chain2 {
        r.meta.started_at = fixed_ts2();
        r.meta.finished_at = fixed_ts2();
    }
    // Re-hash chain2 (structure broken because hashes changed, but that's expected)
    // The point: timestamps are part of the hash input, as they should be
    let h1: Vec<String> = chain1
        .iter()
        .map(|r| r.receipt_sha256.clone().unwrap())
        .collect();
    // chain2 hashes will differ because content differs
    for r in &mut chain2 {
        *r = r.clone().with_hash().unwrap();
    }
    let h2: Vec<String> = chain2
        .iter()
        .map(|r| r.receipt_sha256.clone().unwrap())
        .collect();
    assert_ne!(h1, h2, "different timestamps must produce different hashes");
}

#[test]
fn chain_first_receipt_has_no_parent() {
    let chain = build_chain(3);
    let first_parent = chain[0].usage_raw.get("parent_receipt_hash");
    assert!(
        first_parent.is_none(),
        "first receipt in chain should have no parent hash"
    );
}

#[test]
fn chain_swap_receipts_breaks_validation() {
    let mut chain = build_chain(4);
    chain.swap(1, 2);
    assert!(!verify_chain(&chain));
}

#[test]
fn chain_duplicate_receipt_breaks_validation() {
    let mut chain = build_chain(3);
    let dup = chain[0].clone();
    chain[1] = dup;
    assert!(!verify_chain(&chain));
}

#[test]
fn chain_remove_middle_breaks_validation() {
    let mut chain = build_chain(4);
    chain.remove(2);
    // chain[2] (formerly [3]) still points to old chain[2]'s hash
    assert!(!verify_chain(&chain));
}

#[test]
fn chain_large_chain_validates() {
    let chain = build_chain(50);
    assert!(verify_chain(&chain));
    assert_eq!(chain.len(), 50);
}

// ===========================================================================
// 4. Edge cases (15+ tests)
// ===========================================================================

#[test]
fn edge_receipt_with_empty_events_list() {
    let r = minimal_receipt();
    assert!(r.trace.is_empty());
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_receipt_with_many_events() {
    let events: Vec<AgentEvent> = (0..500)
        .map(|i| {
            make_event(AgentEventKind::AssistantDelta {
                text: format!("token_{i}"),
            })
        })
        .collect();
    let r = receipt_with_trace(events);
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_receipt_with_very_large_event_count() {
    let events: Vec<AgentEvent> = (0..2000)
        .map(|i| {
            make_event(AgentEventKind::AssistantDelta {
                text: format!("t{i}"),
            })
        })
        .collect();
    let r = receipt_with_trace(events);
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
    // Must be deterministic
    assert_eq!(h, hash_receipt(&r));
}

#[test]
fn edge_unicode_in_all_string_fields() {
    let mut r = minimal_receipt();
    r.backend.id = "бэкенд".into();
    r.backend.backend_version = Some("版本".into());
    r.backend.adapter_version = Some("アダプター".into());
    r.meta.contract_version = "عقد/v0.1".into();
    r.verification.git_diff = Some("변경사항".into());
    r.verification.git_status = Some("상태".into());
    r.trace.push(make_event(AgentEventKind::AssistantMessage {
        text: "مرحبا بالعالم".into(),
    }));
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
    assert_eq!(h, hash_receipt(&r));
}

#[test]
fn edge_special_json_characters_newlines() {
    let mut r = minimal_receipt();
    r.verification.git_diff = Some("line1\nline2\nline3".into());
    let json = canonical_json(&r).unwrap();
    // Newlines must be escaped in JSON
    assert!(json.contains("\\n"));
    assert_eq!(hash_receipt(&r).len(), 64);
}

#[test]
fn edge_special_json_characters_quotes() {
    let mut r = minimal_receipt();
    r.verification.git_diff = Some(r#"she said "hello""#.into());
    let json = canonical_json(&r).unwrap();
    assert!(json.contains("\\\"hello\\\""));
    assert_eq!(hash_receipt(&r).len(), 64);
}

#[test]
fn edge_special_json_characters_backslashes() {
    let mut r = minimal_receipt();
    r.verification.git_diff = Some(r"path\to\file".into());
    let json = canonical_json(&r).unwrap();
    assert!(json.contains("\\\\"));
    assert_eq!(hash_receipt(&r).len(), 64);
}

#[test]
fn edge_special_json_characters_tabs() {
    let mut r = minimal_receipt();
    r.verification.git_diff = Some("col1\tcol2\tcol3".into());
    let json = canonical_json(&r).unwrap();
    assert!(json.contains("\\t"));
}

#[test]
fn edge_all_null_optional_fields() {
    let mut r = minimal_receipt();
    r.backend.backend_version = None;
    r.backend.adapter_version = None;
    r.verification.git_diff = None;
    r.verification.git_status = None;
    r.usage.input_tokens = None;
    r.usage.output_tokens = None;
    r.usage.cache_read_tokens = None;
    r.usage.cache_write_tokens = None;
    r.usage.request_units = None;
    r.usage.estimated_cost_usd = None;
    r.receipt_sha256 = None;
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_all_optional_fields_populated() {
    let mut r = minimal_receipt();
    r.backend.backend_version = Some("1.0.0".into());
    r.backend.adapter_version = Some("2.0.0".into());
    r.verification.git_diff = Some("diff".into());
    r.verification.git_status = Some("M file.rs".into());
    r.usage.input_tokens = Some(100);
    r.usage.output_tokens = Some(200);
    r.usage.cache_read_tokens = Some(50);
    r.usage.cache_write_tokens = Some(25);
    r.usage.request_units = Some(1);
    r.usage.estimated_cost_usd = Some(0.01);
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
    // Must differ from all-null
    assert_ne!(h, hash_receipt(&minimal_receipt()));
}

#[test]
fn edge_maximum_size_receipt() {
    let large_text = "x".repeat(100_000);
    let mut r = minimal_receipt();
    r.verification.git_diff = Some(large_text.clone());
    r.trace.push(make_event(AgentEventKind::AssistantMessage {
        text: large_text,
    }));
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_receipt_with_complex_usage_raw() {
    let mut r = minimal_receipt();
    r.usage_raw = serde_json::json!({
        "model": "gpt-4",
        "nested": {
            "deep": {
                "value": [1, 2, 3],
                "flag": true
            }
        },
        "array": [{"a": 1}, {"b": 2}],
        "null_val": null
    });
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
    assert_eq!(h, hash_receipt(&r));
}

#[test]
fn edge_receipt_with_tool_call_events() {
    let events = vec![
        make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/main.rs"}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            output: serde_json::json!({"content": "fn main() {}"}),
            is_error: false,
        }),
    ];
    let r = receipt_with_trace(events);
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_receipt_with_all_event_kinds() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "tok".into() }),
        make_event(AgentEventKind::AssistantMessage {
            text: "full message".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!("ls"),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: serde_json::json!("file.txt"),
            is_error: false,
        }),
        make_event(AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "updated".into(),
        }),
        make_event(AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(0),
            output_preview: Some("Compiling...".into()),
        }),
        make_event(AgentEventKind::Warning {
            message: "deprecated".into(),
        }),
        make_event(AgentEventKind::Error {
            message: "oops".into(),
            error_code: None,
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let r = receipt_with_trace(events);
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
    assert_eq!(h, hash_receipt(&r));
}

#[test]
fn edge_receipt_with_ext_fields() {
    let mut event = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"role": "assistant", "content": "hello"}),
    );
    event.ext = Some(ext);
    let r = receipt_with_trace(vec![event]);
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_receipt_empty_string_fields() {
    let mut r = minimal_receipt();
    r.backend.id = String::new();
    r.meta.contract_version = String::new();
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
    assert_ne!(h, hash_receipt(&minimal_receipt()));
}

#[test]
fn edge_receipt_builder_produces_hashable_receipt() {
    let r = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Partial)
        .build()
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn edge_receipt_with_multiple_artifacts() {
    let mut r = minimal_receipt();
    for i in 0..100 {
        r.artifacts.push(ArtifactRef {
            kind: format!("kind_{i}"),
            path: format!("path/to/artifact_{i}.txt"),
        });
    }
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
    assert_eq!(h, hash_receipt(&r));
}

#[test]
fn edge_receipt_with_capabilities_all_support_levels() {
    let mut r = minimal_receipt();
    r.capabilities
        .insert(Capability::Streaming, SupportLevel::Native);
    r.capabilities
        .insert(Capability::ToolRead, SupportLevel::Emulated);
    r.capabilities
        .insert(Capability::ToolWrite, SupportLevel::Unsupported);
    r.capabilities.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    let h = hash_receipt(&r);
    assert_eq!(h.len(), 64);
    assert_eq!(h, hash_receipt(&r));
}

#[test]
fn edge_receipt_mode_passthrough_vs_mapped() {
    let mut r1 = minimal_receipt();
    r1.mode = ExecutionMode::Passthrough;
    let mut r2 = minimal_receipt();
    r2.mode = ExecutionMode::Mapped;
    assert_ne!(
        hash_receipt(&r1),
        hash_receipt(&r2),
        "different modes must produce different hashes"
    );
}

#[test]
fn edge_receipt_zero_vs_nonzero_usage() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.usage.input_tokens = Some(0);
    // None vs Some(0) are serialized differently
    assert_ne!(hash_receipt(&r1), hash_receipt(&r2));
}

#[test]
fn edge_sha256_hex_empty_input() {
    let h = sha256_hex(b"");
    assert_eq!(h.len(), 64);
    assert_eq!(
        h,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn edge_sha256_hex_known_vector() {
    let h = sha256_hex(b"hello");
    assert_eq!(
        h,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}
