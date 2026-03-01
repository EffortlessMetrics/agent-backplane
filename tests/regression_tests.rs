// SPDX-License-Identifier: MIT OR Apache-2.0
//! Regression tests for key invariants that could break during refactoring.
//!
//! Each test targets a non-obvious contract, wire-format, or behavioural
//! guarantee that must be preserved across versions.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use abp_core::filter::EventFilter;
use abp_core::validate::validate_receipt;
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionMode, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RuntimeConfig,
    WorkOrderBuilder, canonical_json, receipt_hash, sha256_hex,
};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::PolicyEngine;
use abp_protocol::version::ProtocolVersion;
use abp_protocol::{Envelope, JsonlCodec, parse_version};
use chrono::Utc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ===========================================================================
// 1. Receipt hash is SHA-256 (64 hex chars)
// ===========================================================================

#[test]
fn receipt_hash_is_sha256_length() {
    let hash = receipt_hash(&make_receipt()).unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex digest must be 64 chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash must be hex-only"
    );
}

// ===========================================================================
// 2. Receipt hash excludes itself
// ===========================================================================

#[test]
fn receipt_hash_excludes_itself() {
    let r = make_receipt();
    let hash_without = receipt_hash(&r).unwrap();

    let mut r_with = r.clone();
    r_with.receipt_sha256 = Some("deadbeef".to_string());
    let hash_with = receipt_hash(&r_with).unwrap();

    assert_eq!(
        hash_without, hash_with,
        "hash must be identical regardless of receipt_sha256 field value"
    );
}

// ===========================================================================
// 3. WorkOrder IDs are UUIDs
// ===========================================================================

#[test]
fn work_order_ids_are_uuids() {
    let wo = WorkOrderBuilder::new("test task").build();
    // Uuid::new_v4 produces version-4 UUIDs
    assert_eq!(wo.id.get_version_num(), 4);
    // Standard UUID format: 8-4-4-4-12 hex chars
    let s = wo.id.to_string();
    assert_eq!(s.len(), 36);
    let parts: Vec<&str> = s.split('-').collect();
    assert_eq!(parts.len(), 5);
    assert_eq!(
        parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
        vec![8, 4, 4, 4, 12]
    );
}

// ===========================================================================
// 4. CONTRACT_VERSION format
// ===========================================================================

#[test]
fn contract_version_starts_with_abp_v() {
    assert!(
        CONTRACT_VERSION.starts_with("abp/v"),
        "CONTRACT_VERSION must start with 'abp/v', got: {CONTRACT_VERSION}"
    );
}

#[test]
fn contract_version_is_parseable() {
    let parsed = parse_version(CONTRACT_VERSION);
    assert!(
        parsed.is_some(),
        "CONTRACT_VERSION must be parseable as (major, minor)"
    );
}

// ===========================================================================
// 5. Outcome::Complete serde representation is "complete"
// ===========================================================================

#[test]
fn outcome_complete_serializes_to_complete() {
    let json = serde_json::to_value(&Outcome::Complete).unwrap();
    assert_eq!(json.as_str().unwrap(), "complete");
}

// ===========================================================================
// 6. All Outcome variants serialize and deserialize
// ===========================================================================

#[test]
fn outcome_all_variants_roundtrip() {
    let variants = [Outcome::Complete, Outcome::Partial, Outcome::Failed];
    let expected_strings = ["complete", "partial", "failed"];

    for (variant, expected) in variants.iter().zip(expected_strings.iter()) {
        let json = serde_json::to_string(variant).unwrap();
        assert_eq!(json, format!("\"{expected}\""));
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, variant);
    }
}

// ===========================================================================
// 7. Empty events list is valid — receipt with no events → valid hash
// ===========================================================================

#[test]
fn empty_trace_produces_valid_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.trace.is_empty());
    let hash = receipt_hash(&r).unwrap();
    assert_eq!(hash.len(), 64);
}

// ===========================================================================
// 8. BTreeMap ordering in JSON — keys alphabetically sorted
// ===========================================================================

#[test]
fn btreemap_keys_sorted_in_json() {
    let mut map = BTreeMap::new();
    map.insert("zebra".to_string(), serde_json::json!(1));
    map.insert("alpha".to_string(), serde_json::json!(2));
    map.insert("middle".to_string(), serde_json::json!(3));

    let json = serde_json::to_string(&map).unwrap();
    let alpha_pos = json.find("alpha").unwrap();
    let middle_pos = json.find("middle").unwrap();
    let zebra_pos = json.find("zebra").unwrap();
    assert!(
        alpha_pos < middle_pos && middle_pos < zebra_pos,
        "BTreeMap keys must appear alphabetically in JSON"
    );
}

// ===========================================================================
// 9. Envelope tag is "t" — not "type"
// ===========================================================================

#[test]
fn envelope_tag_is_t_not_type() {
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    assert!(
        json.contains("\"t\":"),
        "Envelope must use 't' as tag field"
    );
    // "type" should NOT appear as the discriminator key
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert!(v.get("t").is_some(), "Envelope must have 't' key");
}

// ===========================================================================
// 10. Hello envelope has backend — required field present
// ===========================================================================

#[test]
fn hello_envelope_has_backend() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert!(v.get("backend").is_some(), "Hello must contain 'backend'");
    assert_eq!(v["backend"]["id"].as_str().unwrap(), "test");
}

// ===========================================================================
// 11. Run envelope has work_order (with id)
// ===========================================================================

#[test]
fn run_envelope_has_work_order_id() {
    let wo = WorkOrderBuilder::new("task").build();
    let wo_id = wo.id.to_string();
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&run).unwrap();
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert!(v.get("id").is_some(), "Run must contain 'id'");
    assert!(
        v.get("work_order").is_some(),
        "Run must contain 'work_order'"
    );
    assert_eq!(v["work_order"]["id"].as_str().unwrap(), wo_id);
}

// ===========================================================================
// 12. Event envelope has ref_id
// ===========================================================================

#[test]
fn event_envelope_has_ref_id() {
    let event = Envelope::Event {
        ref_id: "run-42".into(),
        event: make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
    };
    let json = JsonlCodec::encode(&event).unwrap();
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(v["ref_id"].as_str().unwrap(), "run-42");
}

// ===========================================================================
// 13. Final envelope has receipt
// ===========================================================================

#[test]
fn final_envelope_has_receipt() {
    let receipt = make_receipt();
    let fin = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let json = JsonlCodec::encode(&fin).unwrap();
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert!(v.get("receipt").is_some(), "Final must contain 'receipt'");
    assert!(
        v["receipt"].get("outcome").is_some(),
        "receipt must have outcome"
    );
}

// ===========================================================================
// 14. Fatal envelope has error
// ===========================================================================

#[test]
fn fatal_envelope_has_error() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "out of memory".into(),
    };
    let json = JsonlCodec::encode(&fatal).unwrap();
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(v["error"].as_str().unwrap(), "out of memory");
}

// ===========================================================================
// 15. Capability name survives roundtrip — all standard capabilities
// ===========================================================================

#[test]
fn all_capabilities_roundtrip_through_json() {
    let all_caps = [
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

    for cap in &all_caps {
        let json = serde_json::to_string(cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, cap, "Capability {json} did not roundtrip");
    }
}

// ===========================================================================
// 16. PolicyProfile empty is permissive — no rules → everything allowed
// ===========================================================================

#[test]
fn empty_policy_is_permissive() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    assert!(engine.can_use_tool("AnyTool").allowed);
    assert!(engine.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(engine.can_write_path(Path::new("any/file.txt")).allowed);
}

// ===========================================================================
// 17. Glob empty set allows all — no include/exclude → all allowed
// ===========================================================================

#[test]
fn empty_glob_set_allows_all() {
    let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(globs.decide_str("anything"), MatchDecision::Allowed);
    assert_eq!(
        globs.decide_str("deeply/nested/path.rs"),
        MatchDecision::Allowed
    );
}

// ===========================================================================
// 18. WorkOrderBuilder produces unique IDs — build 100 → all unique
// ===========================================================================

#[test]
fn work_order_builder_produces_unique_ids() {
    let ids: HashSet<Uuid> = (0..100)
        .map(|_| WorkOrderBuilder::new("task").build().id)
        .collect();
    assert_eq!(ids.len(), 100, "100 WorkOrders must have 100 unique IDs");
}

// ===========================================================================
// 19. ReceiptBuilder always hashes via with_hash
// ===========================================================================

#[test]
fn receipt_builder_build_has_no_hash() {
    let r = ReceiptBuilder::new("mock").build();
    assert!(
        r.receipt_sha256.is_none(),
        "build() must NOT set receipt_sha256"
    );
}

#[test]
fn receipt_builder_with_hash_sets_hash() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(
        r.receipt_sha256.is_some(),
        "with_hash() must set receipt_sha256"
    );
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

// ===========================================================================
// 20. Config merge is additive — merging empty config → no change
// ===========================================================================

#[test]
fn runtime_config_empty_vendor_is_empty() {
    let config = RuntimeConfig::default();
    assert!(config.vendor.is_empty());
    assert!(config.env.is_empty());
    assert!(config.model.is_none());
    assert!(config.max_budget_usd.is_none());
    assert!(config.max_turns.is_none());
}

#[test]
fn work_order_builder_config_preserves_values() {
    let wo = WorkOrderBuilder::new("task")
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(10));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

// ===========================================================================
// 21. EventFilter by kind — each AgentEventKind variant
// ===========================================================================

#[test]
fn event_filter_matches_each_kind() {
    let kinds_and_events: Vec<(&str, AgentEventKind)> = vec![
        (
            "run_started",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        (
            "run_completed",
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
        (
            "assistant_delta",
            AgentEventKind::AssistantDelta { text: "tok".into() },
        ),
        (
            "assistant_message",
            AgentEventKind::AssistantMessage { text: "hi".into() },
        ),
        (
            "tool_call",
            AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
        ),
        (
            "tool_result",
            AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: None,
                output: serde_json::json!({}),
                is_error: false,
            },
        ),
        (
            "file_changed",
            AgentEventKind::FileChanged {
                path: "a.rs".into(),
                summary: "changed".into(),
            },
        ),
        (
            "command_executed",
            AgentEventKind::CommandExecuted {
                command: "ls".into(),
                exit_code: Some(0),
                output_preview: None,
            },
        ),
        (
            "warning",
            AgentEventKind::Warning {
                message: "warn".into(),
            },
        ),
        (
            "error",
            AgentEventKind::Error {
                message: "err".into(),
            },
        ),
    ];

    for (kind_name, kind) in &kinds_and_events {
        let filter = EventFilter::include_kinds(&[kind_name]);
        let event = make_event(kind.clone());
        assert!(
            filter.matches(&event),
            "EventFilter include_kinds([{kind_name}]) must match its own event"
        );
    }
}

// ===========================================================================
// 22. ValidateReceipt on freshly built receipt — always passes
// ===========================================================================

#[test]
fn validate_receipt_on_fresh_receipt_passes() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(
        validate_receipt(&r).is_ok(),
        "freshly built receipt with hash must pass validation"
    );
}

#[test]
fn validate_receipt_detects_tampered_hash() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.receipt_sha256 = Some("0".repeat(64));
    assert!(
        validate_receipt(&r).is_err(),
        "tampered hash must fail validation"
    );
}

// ===========================================================================
// 23. Version parse roundtrip — parse → display → parse → same
// ===========================================================================

#[test]
fn protocol_version_parse_roundtrip() {
    let original = "abp/v0.1";
    let v = ProtocolVersion::parse(original).unwrap();
    let displayed = v.to_string();
    assert_eq!(displayed, original);
    let reparsed = ProtocolVersion::parse(&displayed).unwrap();
    assert_eq!(v, reparsed);
}

#[test]
fn protocol_version_roundtrip_higher_numbers() {
    for (major, minor) in [(1, 0), (2, 3), (10, 99)] {
        let s = format!("abp/v{major}.{minor}");
        let v = ProtocolVersion::parse(&s).unwrap();
        assert_eq!(v.major, major);
        assert_eq!(v.minor, minor);
        assert_eq!(v.to_string(), s);
    }
}

// ===========================================================================
// 24. ProtocolVersion comparison — v0.1 < v0.2 < v1.0
// ===========================================================================

#[test]
fn protocol_version_ordering() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let v10 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(v01 < v02);
    assert!(v02 < v10);
    assert!(v01 < v10);
}

// ===========================================================================
// 25. Canonical JSON is deterministic — multiple serializations identical
// ===========================================================================

#[test]
fn canonical_json_is_deterministic() {
    let r = make_receipt();
    let json1 = canonical_json(&r).unwrap();
    let json2 = canonical_json(&r).unwrap();
    let json3 = canonical_json(&r).unwrap();
    assert_eq!(json1, json2);
    assert_eq!(json2, json3);
}

#[test]
fn canonical_json_with_btreemap_is_key_sorted() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert("z_key".into(), serde_json::json!("z"));
    config.vendor.insert("a_key".into(), serde_json::json!("a"));
    let json = canonical_json(&config).unwrap();
    let a_pos = json.find("a_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(a_pos < z_pos, "canonical JSON must have sorted keys");
}

// ===========================================================================
// Additional regression guards
// ===========================================================================

/// sha256_hex produces 64 lowercase hex chars for any input
#[test]
fn sha256_hex_format() {
    let h = sha256_hex(b"hello world");
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    // Must be lowercase
    assert_eq!(h, h.to_ascii_lowercase());
}

/// Envelope roundtrips through encode/decode
#[test]
fn envelope_jsonl_roundtrip() {
    let original = Envelope::Fatal {
        ref_id: Some("ref-1".into()),
        error: "test error".into(),
    };
    let encoded = JsonlCodec::encode(&original).unwrap();
    assert!(encoded.ends_with('\n'));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("ref-1"));
            assert_eq!(error, "test error");
        }
        _ => panic!("expected Fatal variant"),
    }
}

/// Receipt hash is stable across repeated calls
#[test]
fn receipt_hash_is_stable() {
    let r = make_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    let h3 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
}

/// ReceiptBuilder run_id is also UUIDv4
#[test]
fn receipt_run_id_is_uuid_v4() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.meta.run_id.get_version_num(), 4);
}

/// AgentEventKind uses "type" tag, not "t"
#[test]
fn agent_event_kind_tag_is_type() {
    let kind = AgentEventKind::RunStarted {
        message: "go".into(),
    };
    let v = serde_json::to_value(&kind).unwrap();
    assert!(
        v.get("type").is_some(),
        "AgentEventKind must use 'type' as tag field"
    );
    assert!(
        v.get("t").is_none(),
        "AgentEventKind must NOT use 't' as tag field"
    );
}

/// ExecutionMode default is Mapped
#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

/// parse_version rejects invalid input gracefully
#[test]
fn parse_version_rejects_invalid() {
    assert!(parse_version("invalid").is_none());
    assert!(parse_version("v0.1").is_none());
    assert!(parse_version("abp/0.1").is_none());
    assert!(parse_version("abp/v").is_none());
    assert!(parse_version("abp/v1").is_none());
    assert!(parse_version("").is_none());
}

/// Glob deny takes precedence over include
#[test]
fn glob_exclude_takes_precedence_over_include() {
    let globs =
        IncludeExcludeGlobs::new(&["src/**".to_string()], &["src/secret/**".to_string()]).unwrap();
    assert_eq!(globs.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        globs.decide_str("src/secret/key.pem"),
        MatchDecision::DeniedByExclude
    );
}

/// Hello envelope contains contract_version matching CONTRACT_VERSION
#[test]
fn hello_envelope_contract_version_matches() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(v["contract_version"].as_str().unwrap(), CONTRACT_VERSION);
}
