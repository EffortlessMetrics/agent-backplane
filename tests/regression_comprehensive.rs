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
//! Comprehensive regression tests catching known edge cases.

use std::collections::BTreeMap;
use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, ContextPacket, ExecutionLane, ExecutionMode, Outcome, PolicyProfile,
    Receipt, ReceiptBuilder, RuntimeConfig, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    canonical_json, receipt_hash, sha256_hex,
};
use abp_dialect::DialectDetector;
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version, parse_version};
use chrono::Utc;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Receipt hash determinism after serialization roundtrip
// ═══════════════════════════════════════════════════════════════════════════

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .build()
}

#[test]
fn receipt_hash_deterministic_same_receipt() {
    let r = make_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_deterministic_after_json_roundtrip() {
    let r = make_receipt().with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_is_64_hex_chars() {
    let h = receipt_hash(&make_receipt()).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_differs_for_different_outcomes() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .work_order_id(Uuid::nil())
        .build();
    // Different outcomes should (almost certainly) produce different hashes.
    // We allow the possibility timestamps differ too, which guarantees it.
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn receipt_hash_differs_for_different_backends() {
    let r1 = ReceiptBuilder::new("mock-a")
        .work_order_id(Uuid::nil())
        .build();
    let r2 = ReceiptBuilder::new("mock-b")
        .work_order_id(Uuid::nil())
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_roundtrip_preserves_all_fields() {
    let r = make_receipt().with_hash().unwrap();
    let json = serde_json::to_string_pretty(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert_eq!(r.meta.contract_version, r2.meta.contract_version);
}

#[test]
fn receipt_hash_stable_across_multiple_with_hash_calls() {
    let r = make_receipt().with_hash().unwrap();
    let h1 = r.receipt_sha256.clone().unwrap();
    // Calling with_hash again on an already-hashed receipt should yield same hash
    // because with_hash nulls receipt_sha256 before computing.
    let r2 = r.with_hash().unwrap();
    let h2 = r2.receipt_sha256.clone().unwrap();
    assert_eq!(h1, h2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Envelope "t" tag NOT "type" — verify wrong field name fails
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_uses_t_tag_in_json() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(
        json.contains(r#""t":"fatal""#),
        "should use 't' tag: {json}"
    );
    assert!(
        !json.contains(r#""type":"fatal""#),
        "must not use 'type' tag"
    );
}

#[test]
fn envelope_decode_rejects_type_tag() {
    let bad = r#"{"type":"fatal","ref_id":null,"error":"boom"}"#;
    let result = JsonlCodec::decode(bad);
    assert!(
        result.is_err(),
        "envelope with 'type' instead of 't' must fail"
    );
}

#[test]
fn envelope_hello_encodes_t_hello() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn envelope_event_encodes_t_event() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"event""#));
}

#[test]
fn envelope_run_encodes_t_run() {
    let wo = WorkOrderBuilder::new("test task").build();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"run""#));
}

#[test]
fn envelope_final_encodes_t_final() {
    let receipt = make_receipt();
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"final""#));
}

#[test]
fn envelope_roundtrip_all_variants() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let fatal = Envelope::Fatal {
        ref_id: Some("r".into()),
        error: "e".into(),
        error_code: None,
    };
    for env in [hello, fatal] {
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        assert_eq!(encoded, re_encoded);
    }
}

#[test]
fn envelope_decode_empty_string_fails() {
    assert!(JsonlCodec::decode("").is_err());
}

#[test]
fn envelope_decode_random_json_object_fails() {
    assert!(JsonlCodec::decode(r#"{"foo":"bar"}"#).is_err());
}

#[test]
fn envelope_ends_with_newline() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "x".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.ends_with('\n'));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. PolicyProfile field names are tools/read_paths/write_paths
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_profile_has_correct_field_names() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["secret/**".into()],
        deny_write: vec!["locked/**".into()],
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    };
    let json = serde_json::to_value(&p).unwrap();
    assert!(json.get("allowed_tools").is_some());
    assert!(json.get("disallowed_tools").is_some());
    assert!(json.get("deny_read").is_some());
    assert!(json.get("deny_write").is_some());
    assert!(json.get("allow_network").is_some());
    assert!(json.get("deny_network").is_some());
    assert!(json.get("require_approval_for").is_some());
}

#[test]
fn policy_profile_does_not_have_tool_allow_field() {
    let json = serde_json::to_value(PolicyProfile::default()).unwrap();
    assert!(json.get("tool_allow").is_none());
    assert!(json.get("tool_deny").is_none());
    assert!(json.get("read_paths").is_none());
    assert!(json.get("write_paths").is_none());
}

#[test]
fn policy_profile_default_is_empty() {
    let p = PolicyProfile::default();
    assert!(p.allowed_tools.is_empty());
    assert!(p.disallowed_tools.is_empty());
    assert!(p.deny_read.is_empty());
    assert!(p.deny_write.is_empty());
    assert!(p.allow_network.is_empty());
    assert!(p.deny_network.is_empty());
    assert!(p.require_approval_for.is_empty());
}

#[test]
fn policy_profile_roundtrip_json() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["DeleteFile".into()],
    };
    let json = serde_json::to_string(&p).unwrap();
    let p2: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(p.allowed_tools, p2.allowed_tools);
    assert_eq!(p.disallowed_tools, p2.disallowed_tools);
    assert_eq!(p.deny_read, p2.deny_read);
    assert_eq!(p.deny_write, p2.deny_write);
}

#[test]
fn policy_engine_empty_allows_everything() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    assert!(engine.can_use_tool("anything").allowed);
    assert!(engine.can_read_path(Path::new("any/path")).allowed);
    assert!(engine.can_write_path(Path::new("any/path")).allowed);
}

#[test]
fn policy_engine_disallow_beats_allow() {
    let p = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&p).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn policy_engine_deny_read_blocks() {
    let p = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&p).unwrap();
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn policy_engine_deny_write_blocks() {
    let p = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&p).unwrap();
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. BTreeMap ordering preserved in receipts
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn btreemap_ordering_in_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, abp_core::SupportLevel::Native);
    caps.insert(Capability::ToolRead, abp_core::SupportLevel::Native);
    caps.insert(Capability::Streaming, abp_core::SupportLevel::Emulated);
    let json = serde_json::to_string(&caps).unwrap();
    let streaming_pos = json.find("streaming").unwrap();
    let tool_read_pos = json.find("tool_read").unwrap();
    let tool_write_pos = json.find("tool_write").unwrap();
    assert!(
        streaming_pos < tool_read_pos,
        "streaming should come before tool_read"
    );
    assert!(
        tool_read_pos < tool_write_pos,
        "tool_read should come before tool_write"
    );
}

#[test]
fn btreemap_ordering_in_vendor_config() {
    let mut vendor = BTreeMap::new();
    vendor.insert("zebra".to_string(), serde_json::json!("z"));
    vendor.insert("alpha".to_string(), serde_json::json!("a"));
    vendor.insert("mid".to_string(), serde_json::json!("m"));
    let json = serde_json::to_string(&vendor).unwrap();
    let a_pos = json.find("alpha").unwrap();
    let m_pos = json.find("mid").unwrap();
    let z_pos = json.find("zebra").unwrap();
    assert!(a_pos < m_pos && m_pos < z_pos);
}

#[test]
fn btreemap_ordering_in_env() {
    let mut env = BTreeMap::new();
    env.insert("ZZZ".to_string(), "last".to_string());
    env.insert("AAA".to_string(), "first".to_string());
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.find("AAA").unwrap() < json.find("ZZZ").unwrap());
}

#[test]
fn btreemap_ordering_deterministic_across_serializations() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolBash, abp_core::SupportLevel::Native);
    caps.insert(Capability::ToolRead, abp_core::SupportLevel::Native);
    caps.insert(Capability::Streaming, abp_core::SupportLevel::Emulated);
    let json1 = serde_json::to_string(&caps).unwrap();
    let json2 = serde_json::to_string(&caps).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn canonical_json_sorts_keys() {
    let v = serde_json::json!({"z": 1, "a": 2, "m": 3});
    let s = canonical_json(&v).unwrap();
    assert!(s.find("\"a\"").unwrap() < s.find("\"m\"").unwrap());
    assert!(s.find("\"m\"").unwrap() < s.find("\"z\"").unwrap());
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Empty IncludeExcludeGlobs allows everything
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_globs_allow_everything() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn empty_globs_allow_deep_paths() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("a/b/c/d/e/f/g.txt"), MatchDecision::Allowed);
}

#[test]
fn empty_globs_allow_unicode_paths() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("données/日本語.txt"), MatchDecision::Allowed);
}

#[test]
fn exclude_only_denies_matches() {
    let g = IncludeExcludeGlobs::new(&[], &["*.log".into()]).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn include_only_gates_matches() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_overrides_include() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &["src/generated/**".into()]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/generated/out.rs"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn match_decision_is_allowed_helper() {
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

#[test]
fn invalid_glob_returns_error() {
    let result = IncludeExcludeGlobs::new(&["[".into()], &[]);
    assert!(result.is_err());
}

#[test]
fn decide_path_and_decide_str_agree() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &["src/secret/**".into()]).unwrap();
    for path in ["src/lib.rs", "src/secret/key.pem", "README.md"] {
        assert_eq!(g.decide_str(path), g.decide_path(Path::new(path)));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. with_hash() nulls receipt_sha256 before hashing (self-referential prevention)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn with_hash_produces_some() {
    let r = make_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn with_hash_nulls_field_before_hashing() {
    // Manually set receipt_sha256 to a bogus value
    let mut r = make_receipt();
    r.receipt_sha256 = Some("bogus_previous_hash".into());
    let _hashed = r.with_hash().unwrap();
    // receipt_hash always nulls receipt_sha256 before hashing.
    // Test with cloned receipts to guarantee identical timestamps.
    let base = make_receipt();
    let mut with_bogus = base.clone();
    with_bogus.receipt_sha256 = Some("bogus".into());
    assert_eq!(
        receipt_hash(&base).unwrap(),
        receipt_hash(&with_bogus).unwrap()
    );
}

#[test]
fn with_hash_idempotent() {
    let r = make_receipt().with_hash().unwrap();
    let h1 = r.receipt_sha256.clone().unwrap();
    let r2 = r.with_hash().unwrap();
    let h2 = r2.receipt_sha256.clone().unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_ignores_existing_sha256_field() {
    let base = make_receipt();
    let h1 = receipt_hash(&base).unwrap();
    let mut modified = base.clone();
    modified.receipt_sha256 = Some("completely_different_value".into());
    let h2 = receipt_hash(&modified).unwrap();
    assert_eq!(h1, h2, "receipt_hash must ignore receipt_sha256 field");
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. CONTRACT_VERSION is "abp/v0.1"
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contract_version_is_correct() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_not_v1() {
    assert_ne!(CONTRACT_VERSION, "abp/v1.0");
}

#[test]
fn contract_version_not_plain_version() {
    assert_ne!(CONTRACT_VERSION, "0.1");
    assert_ne!(CONTRACT_VERSION, "v0.1");
}

#[test]
fn contract_version_in_receipt_metadata() {
    let r = make_receipt();
    assert_eq!(r.meta.contract_version, "abp/v0.1");
}

#[test]
fn contract_version_in_hello_envelope() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    if let Envelope::Hello {
        contract_version, ..
    } = env
    {
        assert_eq!(contract_version, "abp/v0.1");
    } else {
        panic!("expected Hello variant");
    }
}

#[test]
fn parse_version_parses_contract_version() {
    assert_eq!(parse_version(CONTRACT_VERSION), Some((0, 1)));
}

#[test]
fn parse_version_rejects_garbage() {
    assert_eq!(parse_version("garbage"), None);
    assert_eq!(parse_version(""), None);
    assert_eq!(parse_version("v0.1"), None);
}

#[test]
fn compatible_version_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
}

#[test]
fn incompatible_version_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Dialect detection doesn't panic on empty/malformed JSON
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_detect_on_null_returns_none() {
    let d = DialectDetector::new();
    assert!(d.detect(&serde_json::json!(null)).is_none());
}

#[test]
fn dialect_detect_on_empty_object_returns_none() {
    let d = DialectDetector::new();
    assert!(d.detect(&serde_json::json!({})).is_none());
}

#[test]
fn dialect_detect_on_array_returns_none() {
    let d = DialectDetector::new();
    assert!(d.detect(&serde_json::json!([1, 2, 3])).is_none());
}

#[test]
fn dialect_detect_on_string_returns_none() {
    let d = DialectDetector::new();
    assert!(d.detect(&serde_json::json!("hello")).is_none());
}

#[test]
fn dialect_detect_on_number_returns_none() {
    let d = DialectDetector::new();
    assert!(d.detect(&serde_json::json!(42)).is_none());
}

#[test]
fn dialect_detect_on_boolean_returns_none() {
    let d = DialectDetector::new();
    assert!(d.detect(&serde_json::json!(true)).is_none());
}

#[test]
fn dialect_detect_on_malformed_keys_does_not_panic() {
    let d = DialectDetector::new();
    let v = serde_json::json!({"": "", "null": null, "nested": {"a": []}});
    // Should not panic — may return None or Some
    let _ = d.detect(&v);
}

#[test]
fn dialect_detect_all_on_empty_returns_empty() {
    let d = DialectDetector::new();
    assert!(d.detect_all(&serde_json::json!({})).is_empty());
}

#[test]
fn dialect_detect_all_on_non_object_returns_empty() {
    let d = DialectDetector::new();
    assert!(d.detect_all(&serde_json::json!("string")).is_empty());
    assert!(d.detect_all(&serde_json::json!(null)).is_empty());
}

#[test]
fn dialect_detect_recognizes_openai_style() {
    let d = DialectDetector::new();
    let v = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.7
    });
    let result = d.detect(&v);
    assert!(result.is_some());
}

#[test]
fn dialect_detect_recognizes_claude_style() {
    let d = DialectDetector::new();
    let v = serde_json::json!({
        "model": "claude-3",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        "max_tokens": 1024
    });
    let result = d.detect(&v);
    assert!(result.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Config env vars don't race (no exact-value assertions)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn runtime_config_default_has_empty_env() {
    let cfg = RuntimeConfig::default();
    assert!(cfg.env.is_empty());
}

#[test]
fn runtime_config_env_is_btreemap() {
    let mut cfg = RuntimeConfig::default();
    cfg.env.insert("B".into(), "2".into());
    cfg.env.insert("A".into(), "1".into());
    let keys: Vec<_> = cfg.env.keys().collect();
    assert_eq!(keys, vec!["A", "B"], "BTreeMap should sort keys");
}

#[test]
fn runtime_config_env_roundtrip() {
    let mut cfg = RuntimeConfig::default();
    cfg.env.insert("FOO".into(), "bar".into());
    cfg.env.insert("BAZ".into(), "qux".into());
    let json = serde_json::to_string(&cfg).unwrap();
    let cfg2: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg.env, cfg2.env);
}

#[test]
fn runtime_config_vendor_is_btreemap() {
    let mut cfg = RuntimeConfig::default();
    cfg.vendor.insert("z_vendor".into(), serde_json::json!("z"));
    cfg.vendor.insert("a_vendor".into(), serde_json::json!("a"));
    let keys: Vec<_> = cfg.vendor.keys().collect();
    assert_eq!(keys, vec!["a_vendor", "z_vendor"]);
}

#[test]
fn runtime_config_model_optional() {
    let cfg = RuntimeConfig::default();
    assert!(cfg.model.is_none());
    let json = serde_json::to_string(&cfg).unwrap();
    let cfg2: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert!(cfg2.model.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. WorkOrderBuilder produces valid work orders with UUID IDs
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_builder_produces_uuid_id() {
    let wo = WorkOrderBuilder::new("test task").build();
    assert_ne!(wo.id, Uuid::nil(), "should be a random UUID, not nil");
}

#[test]
fn work_order_builder_unique_ids() {
    let wo1 = WorkOrderBuilder::new("task 1").build();
    let wo2 = WorkOrderBuilder::new("task 2").build();
    assert_ne!(wo1.id, wo2.id, "each build should produce a unique UUID");
}

#[test]
fn work_order_builder_sets_task() {
    let wo = WorkOrderBuilder::new("Fix the bug").build();
    assert_eq!(wo.task, "Fix the bug");
}

#[test]
fn work_order_builder_default_lane() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn work_order_builder_custom_lane() {
    let wo = WorkOrderBuilder::new("test")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn work_order_builder_sets_model() {
    let wo = WorkOrderBuilder::new("test").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn work_order_builder_sets_max_turns() {
    let wo = WorkOrderBuilder::new("test").max_turns(10).build();
    assert_eq!(wo.config.max_turns, Some(10));
}

#[test]
fn work_order_builder_sets_root() {
    let wo = WorkOrderBuilder::new("test").root("/tmp/ws").build();
    assert_eq!(wo.workspace.root, "/tmp/ws");
}

#[test]
fn work_order_builder_default_workspace_mode_staged() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn work_order_builder_sets_policy() {
    let p = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let wo = WorkOrderBuilder::new("test").policy(p).build();
    assert_eq!(wo.policy.disallowed_tools, vec!["Bash"]);
}

#[test]
fn work_order_builder_sets_max_budget() {
    let wo = WorkOrderBuilder::new("test").max_budget_usd(5.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

#[test]
fn work_order_roundtrip_json() {
    let wo = WorkOrderBuilder::new("roundtrip test")
        .model("claude-3")
        .max_turns(5)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.id, wo2.id);
    assert_eq!(wo.task, wo2.task);
    assert_eq!(wo.config.model, wo2.config.model);
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional edge case tests to reach 80+ total
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sha256_hex_produces_64_chars() {
    let h = sha256_hex(b"test");
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn sha256_hex_deterministic() {
    assert_eq!(sha256_hex(b"hello"), sha256_hex(b"hello"));
}

#[test]
fn sha256_hex_differs_for_different_input() {
    assert_ne!(sha256_hex(b"a"), sha256_hex(b"b"));
}

#[test]
fn agent_event_kind_uses_type_tag() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"assistant_message""#));
}

#[test]
fn agent_event_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/lib.rs"}),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let event2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(
        matches!(event2.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "Read")
    );
}

#[test]
fn outcome_serde_roundtrip() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&outcome).unwrap();
        let o2: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(outcome, o2);
    }
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serde_roundtrip() {
    for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let json = serde_json::to_string(&mode).unwrap();
        let m2: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, m2);
    }
}

#[test]
fn receipt_builder_defaults() {
    let r = ReceiptBuilder::new("test-backend").build();
    assert_eq!(r.backend.id, "test-backend");
    assert_eq!(r.outcome, Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_builder_with_trace() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("mock").add_trace_event(event).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn receipt_builder_with_artifact() {
    let r = ReceiptBuilder::new("mock")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "out.patch".into(),
        })
        .build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
}

#[test]
fn receipt_builder_with_hash_shortcut() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn envelope_fatal_with_code() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "timeout",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    assert_eq!(
        env.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn envelope_error_code_none_for_non_fatal() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert!(env.error_code().is_none());
}

#[test]
fn work_order_builder_include_exclude() {
    let wo = WorkOrderBuilder::new("test")
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn work_order_builder_context_packet() {
    let ctx = ContextPacket {
        files: vec!["README.md".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("test").context(ctx).build();
    assert_eq!(wo.context.files, vec!["README.md"]);
}

#[test]
fn canonical_json_empty_object() {
    let s = canonical_json(&serde_json::json!({})).unwrap();
    assert_eq!(s, "{}");
}

#[test]
fn canonical_json_nested_sorting() {
    let v = serde_json::json!({"b": {"d": 1, "c": 2}, "a": 3});
    let s = canonical_json(&v).unwrap();
    assert!(s.find("\"a\"").unwrap() < s.find("\"b\"").unwrap());
    assert!(s.find("\"c\"").unwrap() < s.find("\"d\"").unwrap());
}

#[test]
fn decode_stream_handles_blank_lines() {
    let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"x\"}\n\n";
    let reader = std::io::BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn decode_stream_multiple_messages() {
    let input = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"a\"}\n\
                 {\"t\":\"fatal\",\"ref_id\":null,\"error\":\"b\"}\n";
    let reader = std::io::BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 2);
}
