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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Regression tests for known edge cases and bug fixes.
//!
//! Each test documents a specific regression it prevents with a comment.
//! Organized by category: receipt hashing, BTreeMap determinism, unicode,
//! empty/None handling, defaults, serde skip behavior, contract version,
//! policy conflicts, glob edge cases, workspace staging, event ordering,
//! JSONL protocol edge cases, config precedence, IR types, and dialect names.

use std::collections::{BTreeMap, HashSet};
use std::io::BufReader;
use std::path::Path;

use abp_config::{merge_configs, parse_toml, validate_config, BackendEntry, BackplaneConfig};
use abp_core::filter::EventFilter;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::validate::validate_receipt;
use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, BackendIdentity,
    Capability, CapabilityManifest, ExecutionMode, Outcome, PolicyProfile, Receipt, ReceiptBuilder,
    RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrderBuilder,
    CONTRACT_VERSION,
};
use abp_dialect::{Dialect, DialectDetector};
use abp_glob::{build_globset, IncludeExcludeGlobs, MatchDecision};
use abp_policy::PolicyEngine;
use abp_protocol::version::ProtocolVersion;
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec};
use abp_receipt::{canonicalize, compute_hash, verify_hash};
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
// Receipt hash self-referential prevention
// ===========================================================================

/// Regression: receipt_hash must produce exactly 64 lowercase hex chars (SHA-256).
#[test]
fn receipt_hash_is_sha256_length() {
    let hash = receipt_hash(&make_receipt()).unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex digest must be 64 chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash must be hex-only"
    );
}

/// Regression: the receipt_sha256 field is set to null before hashing so the
/// hash is not self-referential. Changing the stored hash must not change
/// the computed hash.
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

/// Regression: setting receipt_sha256 to a full 64-char hex string must not
/// influence the hash — prevents accidental inclusion of the field.
#[test]
fn receipt_hash_ignores_full_length_stored_hash() {
    let r = make_receipt();
    let h1 = receipt_hash(&r).unwrap();

    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("a".repeat(64));
    let h2 = receipt_hash(&r2).unwrap();

    assert_eq!(h1, h2);
}

/// Regression: with_hash() stores a hash that can be recomputed to the same
/// value — round-trip integrity check.
#[test]
fn receipt_with_hash_is_self_consistent() {
    let r = make_receipt().with_hash().unwrap();
    let recomputed = receipt_hash(&r).unwrap();
    assert_eq!(r.receipt_sha256.as_ref().unwrap(), &recomputed);
}

/// Regression: abp-receipt crate's verify_hash returns true for a correctly
/// hashed receipt and false for a tampered one.
#[test]
fn verify_hash_detects_tampering() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let mut hashed = r.clone();
    hashed.receipt_sha256 = Some(compute_hash(&hashed).unwrap());
    assert!(verify_hash(&hashed));

    hashed.receipt_sha256 = Some("tampered".into());
    assert!(!verify_hash(&hashed));
}

/// Regression: verify_hash returns true when no hash is stored (None).
#[test]
fn verify_hash_accepts_none() {
    let r = make_receipt();
    assert!(r.receipt_sha256.is_none());
    assert!(verify_hash(&r));
}

/// Regression: canonicalize() must force receipt_sha256 to null so two
/// receipts differing only in stored hash produce identical canonical JSON.
#[test]
fn canonicalize_nullifies_hash_field() {
    let r = make_receipt();
    let json = canonicalize(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

/// Regression: receipt hash is stable across repeated calls.
#[test]
fn receipt_hash_is_stable() {
    let r = make_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    let h3 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
}

// ===========================================================================
// BTreeMap deterministic serialization
// ===========================================================================

/// Regression: BTreeMap keys must appear alphabetically in JSON output so
/// canonical hashing is deterministic regardless of insertion order.
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

/// Regression: canonical_json with BTreeMap vendor config must produce
/// sorted keys for deterministic hashing.
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

/// Regression: CapabilityManifest (a BTreeMap) must produce deterministic
/// JSON regardless of which capabilities were inserted first.
#[test]
fn capability_manifest_key_order_is_deterministic() {
    let mut m1 = CapabilityManifest::new();
    m1.insert(Capability::ToolBash, SupportLevel::Native);
    m1.insert(Capability::Streaming, SupportLevel::Native);

    let mut m2 = CapabilityManifest::new();
    m2.insert(Capability::Streaming, SupportLevel::Native);
    m2.insert(Capability::ToolBash, SupportLevel::Native);

    let j1 = serde_json::to_string(&m1).unwrap();
    let j2 = serde_json::to_string(&m2).unwrap();
    assert_eq!(j1, j2, "insertion order must not affect serialized output");
}

/// Regression: canonical_json is deterministic across repeated calls.
#[test]
fn canonical_json_is_deterministic() {
    let r = make_receipt();
    let json1 = canonical_json(&r).unwrap();
    let json2 = canonical_json(&r).unwrap();
    assert_eq!(json1, json2);
}

// ===========================================================================
// Unicode handling in all string fields
// ===========================================================================

/// Regression: Unicode in work order task must survive serialization roundtrip.
#[test]
fn unicode_in_work_order_task() {
    let wo = WorkOrderBuilder::new("修复认证模块 🔧").build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, "修复认证模块 🔧");
}

/// Regression: Unicode in agent event messages must roundtrip through JSON.
#[test]
fn unicode_in_agent_event_message() {
    let event = make_event(AgentEventKind::AssistantMessage {
        text: "こんにちは世界 🌍".into(),
    });
    let json = serde_json::to_string(&event).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    match &back.kind {
        AgentEventKind::AssistantMessage { text } => {
            assert_eq!(text, "こんにちは世界 🌍");
        }
        _ => panic!("wrong variant"),
    }
}

/// Regression: Unicode in backend identity id must be preserved in receipt hash.
#[test]
fn unicode_in_backend_identity_hashes_stably() {
    let r = ReceiptBuilder::new("后端-αβγ").build();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(r.backend.id, "后端-αβγ");
}

/// Regression: Unicode in policy tool names must still match correctly.
#[test]
fn unicode_in_policy_tool_name() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["ツール*".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("ツールA").allowed);
}

/// Regression: Unicode in JSONL envelope error message must roundtrip.
#[test]
fn unicode_in_envelope_fatal_error() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "Ошибка: файл не найден 📁".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => {
            assert_eq!(error, "Ошибка: файл не найден 📁");
        }
        _ => panic!("expected Fatal"),
    }
}

/// Regression: Unicode in IR message text must survive roundtrip.
#[test]
fn unicode_in_ir_message_text() {
    let msg = IrMessage::text(IrRole::User, "안녕하세요 🇰🇷");
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.text_content(), "안녕하세요 🇰🇷");
}

// ===========================================================================
// Empty string handling vs None handling
// ===========================================================================

/// Regression: empty string task in work order must not be silently
/// converted to None or cause serialization issues.
#[test]
fn empty_string_task_roundtrips() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
    let json = serde_json::to_string(&wo).unwrap();
    let back: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, "");
}

/// Regression: None model in RuntimeConfig must serialize differently
/// from empty string model (skip_serializing_if = Option::is_none).
#[test]
fn none_vs_empty_string_model() {
    let config_none = RuntimeConfig {
        model: None,
        ..Default::default()
    };
    let json_none = serde_json::to_string(&config_none).unwrap();

    let config_empty = RuntimeConfig {
        model: Some(String::new()),
        ..Default::default()
    };
    let json_empty = serde_json::to_string(&config_empty).unwrap();

    assert_ne!(json_none, json_empty, "None and empty string must differ");
}

/// Regression: empty backend_version (None) in BackendIdentity must not
/// appear in JSON, while Some("") should appear.
#[test]
fn backend_identity_none_vs_empty_version() {
    let id_none = BackendIdentity {
        id: "test".into(),
        backend_version: None,
        adapter_version: None,
    };
    let id_empty = BackendIdentity {
        id: "test".into(),
        backend_version: Some(String::new()),
        adapter_version: None,
    };
    let json_none = serde_json::to_string(&id_none).unwrap();
    let json_empty = serde_json::to_string(&id_empty).unwrap();
    assert_ne!(json_none, json_empty);
}

/// Regression: empty trace vec and empty artifacts vec must roundtrip.
#[test]
fn empty_vec_fields_roundtrip() {
    let r = make_receipt();
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    let json = serde_json::to_string(&r).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert!(back.trace.is_empty());
    assert!(back.artifacts.is_empty());
}

// ===========================================================================
// Default value propagation
// ===========================================================================

/// Regression: RuntimeConfig::default() must have all Option fields as None
/// and all collection fields as empty.
#[test]
fn runtime_config_empty_vendor_is_empty() {
    let config = RuntimeConfig::default();
    assert!(config.vendor.is_empty());
    assert!(config.env.is_empty());
    assert!(config.model.is_none());
    assert!(config.max_budget_usd.is_none());
    assert!(config.max_turns.is_none());
}

/// Regression: ExecutionMode default is Mapped, not Passthrough.
#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

/// Regression: PolicyProfile::default() must have all vecs empty (permissive).
#[test]
fn policy_profile_default_is_all_empty() {
    let p = PolicyProfile::default();
    assert!(p.allowed_tools.is_empty());
    assert!(p.disallowed_tools.is_empty());
    assert!(p.deny_read.is_empty());
    assert!(p.deny_write.is_empty());
    assert!(p.allow_network.is_empty());
    assert!(p.deny_network.is_empty());
    assert!(p.require_approval_for.is_empty());
}

/// Regression: UsageNormalized::default() must have all fields as None/defaults.
#[test]
fn usage_normalized_default_is_all_none() {
    let u = UsageNormalized::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.cache_read_tokens.is_none());
    assert!(u.cache_write_tokens.is_none());
    assert!(u.request_units.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

/// Regression: WorkOrderBuilder preserves all config values set via builder.
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

/// Regression: VerificationReport default must have harness_ok = false.
#[test]
fn verification_report_default_harness_ok_false() {
    let v = VerificationReport::default();
    assert!(!v.harness_ok);
    assert!(v.git_diff.is_none());
    assert!(v.git_status.is_none());
}

// ===========================================================================
// Serde skip_serializing_if behavior
// ===========================================================================

/// Regression: AgentEvent ext field (skip_serializing_if = Option::is_none)
/// must be absent in JSON when None.
#[test]
fn agent_event_ext_none_omitted_from_json() {
    let event = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let json = serde_json::to_string(&event).unwrap();
    assert!(
        !json.contains("\"ext\""),
        "ext=None must be omitted from JSON"
    );
}

/// Regression: AgentEvent ext field must appear when Some.
#[test]
fn agent_event_ext_some_present_in_json() {
    let mut ext = BTreeMap::new();
    ext.insert("key".into(), serde_json::json!("value"));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"ext\""));
}

/// Regression: Error event's error_code field must be omitted when None
/// (skip_serializing_if = "Option::is_none").
#[test]
fn error_event_code_none_omitted() {
    let kind = AgentEventKind::Error {
        message: "fail".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(
        !json.contains("error_code"),
        "error_code=None must be omitted"
    );
}

/// Regression: IrMessage metadata field (skip_serializing_if = BTreeMap::is_empty)
/// must be absent when empty.
#[test]
fn ir_message_empty_metadata_omitted() {
    let msg = IrMessage::text(IrRole::User, "hello");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(
        !json.contains("\"metadata\""),
        "empty metadata must be omitted"
    );
}

/// Regression: IrMessage metadata must appear when non-empty.
#[test]
fn ir_message_nonempty_metadata_present() {
    let mut msg = IrMessage::text(IrRole::User, "hello");
    msg.metadata
        .insert("key".into(), serde_json::json!("value"));
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"metadata\""));
}

// ===========================================================================
// Contract version consistency across types
// ===========================================================================

/// Regression: CONTRACT_VERSION must start with "abp/v" prefix.
#[test]
fn contract_version_starts_with_abp_v() {
    assert!(
        CONTRACT_VERSION.starts_with("abp/v"),
        "CONTRACT_VERSION must start with 'abp/v', got: {CONTRACT_VERSION}"
    );
}

/// Regression: CONTRACT_VERSION must be parseable by parse_version.
#[test]
fn contract_version_is_parseable() {
    let parsed = parse_version(CONTRACT_VERSION);
    assert!(
        parsed.is_some(),
        "CONTRACT_VERSION must be parseable as (major, minor)"
    );
}

/// Regression: Receipt's meta.contract_version must match CONTRACT_VERSION.
#[test]
fn receipt_contract_version_matches_constant() {
    let r = make_receipt();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

/// Regression: Hello envelope's contract_version must match CONTRACT_VERSION.
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

/// Regression: parse_version and is_compatible_version must agree on
/// CONTRACT_VERSION being compatible with itself.
#[test]
fn contract_version_compatible_with_itself() {
    assert!(is_compatible_version(CONTRACT_VERSION, CONTRACT_VERSION));
}

// ===========================================================================
// Policy profile with conflicting rules (deny overrides allow)
// ===========================================================================

/// Regression: disallowed_tools must override allowed_tools wildcard.
#[test]
fn policy_deny_overrides_wildcard_allow() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

/// Regression: empty policy must be fully permissive (no rules = allow all).
#[test]
fn empty_policy_is_permissive() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    assert!(engine.can_use_tool("AnyTool").allowed);
    assert!(engine.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(engine.can_write_path(Path::new("any/file.txt")).allowed);
}

/// Regression: deny_read glob must block reading even without explicit allow.
#[test]
fn policy_deny_read_blocks_path() {
    let policy = PolicyProfile {
        deny_read: vec!["*.secret".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("config.secret")).allowed);
    assert!(engine.can_read_path(Path::new("config.toml")).allowed);
}

/// Regression: deny_write glob must block writing even without explicit allow.
#[test]
fn policy_deny_write_blocks_path() {
    let policy = PolicyProfile {
        deny_write: vec![".env*".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new(".env")).allowed);
    assert!(!engine.can_write_path(Path::new(".env.local")).allowed);
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
}

/// Regression: specific allowed_tools list must reject unlisted tools.
#[test]
fn policy_allowlist_rejects_unlisted_tool() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
}

/// Regression: deny_read and deny_write can coexist independently.
#[test]
fn policy_independent_read_write_deny() {
    let policy = PolicyProfile {
        deny_read: vec!["secrets/**".into()],
        deny_write: vec!["logs/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("secrets/key.pem")).allowed);
    assert!(engine.can_write_path(Path::new("secrets/key.pem")).allowed);
    assert!(engine.can_read_path(Path::new("logs/app.log")).allowed);
    assert!(!engine.can_write_path(Path::new("logs/app.log")).allowed);
}

// ===========================================================================
// Glob edge cases
// ===========================================================================

/// Regression: empty pattern list must return None from build_globset.
#[test]
fn glob_empty_patterns_returns_none() {
    let result = build_globset(&[]).unwrap();
    assert!(result.is_none());
}

/// Regression: empty include/exclude both empty must allow everything.
#[test]
fn empty_glob_set_allows_all() {
    let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(globs.decide_str("anything"), MatchDecision::Allowed);
    assert_eq!(
        globs.decide_str("deeply/nested/path.rs"),
        MatchDecision::Allowed
    );
}

/// Regression: star-only pattern must match all single-segment paths.
#[test]
fn glob_star_only_matches_all() {
    let globs = IncludeExcludeGlobs::new(&["*".into()], &[]).unwrap();
    assert_eq!(globs.decide_str("file.rs"), MatchDecision::Allowed);
}

/// Regression: double-star pattern must match nested paths.
#[test]
fn glob_double_star_matches_nested() {
    let globs = IncludeExcludeGlobs::new(&["**".into()], &[]).unwrap();
    assert_eq!(globs.decide_str("a/b/c/d.txt"), MatchDecision::Allowed);
}

/// Regression: exclude must take precedence over include for the same path.
#[test]
fn glob_exclude_takes_precedence_over_include() {
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &["src/secret/**".into()]).unwrap();
    assert_eq!(globs.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        globs.decide_str("src/secret/key.pem"),
        MatchDecision::DeniedByExclude
    );
}

/// Regression: include set with no matching path must return DeniedByMissingInclude.
#[test]
fn glob_include_denies_non_matching() {
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(
        globs.decide_str("tests/test.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

/// Regression: dot-prefixed files must be matchable by globs.
#[test]
fn glob_matches_dotfiles() {
    let globs = IncludeExcludeGlobs::new(&[], &[".*".into()]).unwrap();
    assert_eq!(
        globs.decide_str(".gitignore"),
        MatchDecision::DeniedByExclude
    );
}

// ===========================================================================
// Event ordering invariants
// ===========================================================================

/// Regression: EventFilter include_kinds must match each AgentEventKind variant
/// by its serde tag name.
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
                error_code: None,
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

/// Regression: EventFilter exclude_kinds must reject events of that kind.
#[test]
fn event_filter_exclude_rejects() {
    let filter = EventFilter::exclude_kinds(&["warning"]);
    let warning = make_event(AgentEventKind::Warning {
        message: "oops".into(),
    });
    assert!(!filter.matches(&warning));
    let started = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(filter.matches(&started));
}

/// Regression: receipt trace preserves insertion order of events.
#[test]
fn receipt_trace_preserves_insertion_order() {
    let e1 = make_event(AgentEventKind::RunStarted {
        message: "first".into(),
    });
    let e2 = make_event(AgentEventKind::AssistantMessage {
        text: "second".into(),
    });
    let e3 = make_event(AgentEventKind::RunCompleted {
        message: "third".into(),
    });
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(e1)
        .add_trace_event(e2)
        .add_trace_event(e3)
        .build();
    assert_eq!(r.trace.len(), 3);
    assert!(matches!(
        &r.trace[0].kind,
        AgentEventKind::RunStarted { .. }
    ));
    assert!(matches!(
        &r.trace[1].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(
        &r.trace[2].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

// ===========================================================================
// JSONL protocol edge cases
// ===========================================================================

/// Regression: Envelope tag is "t" not "type" — prevent accidental rename.
#[test]
fn envelope_tag_is_t_not_type() {
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    assert!(
        json.contains("\"t\":"),
        "Envelope must use 't' as tag field"
    );
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert!(v.get("t").is_some(), "Envelope must have 't' key");
}

/// Regression: AgentEventKind uses "type" tag, not "t" — these differ.
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

/// Regression: JSONL encode must end with newline.
#[test]
fn jsonl_encode_ends_with_newline() {
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: "x".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    assert!(encoded.ends_with('\n'), "JSONL line must end with \\n");
}

/// Regression: JSONL decode must handle leading/trailing whitespace.
#[test]
fn jsonl_decode_handles_trailing_whitespace() {
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: "x".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let with_spaces = format!("  {}  ", encoded.trim());
    // decode_stream trims, but decode itself expects valid JSON
    let decoded = JsonlCodec::decode(with_spaces.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "x"),
        _ => panic!("expected Fatal"),
    }
}

/// Regression: JSONL decode_stream must skip empty lines gracefully.
#[test]
fn jsonl_decode_stream_skips_empty_lines() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&fatal).unwrap();
    let input = format!("\n\n{line}\n\n{line}\n");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

/// Regression: JSONL decode of empty string must return an error, not panic.
#[test]
fn jsonl_decode_empty_string_is_error() {
    assert!(JsonlCodec::decode("").is_err());
}

/// Regression: JSONL decode of whitespace-only string must return an error.
#[test]
fn jsonl_decode_whitespace_only_is_error() {
    assert!(JsonlCodec::decode("   \t  ").is_err());
}

/// Regression: Envelope roundtrips through encode/decode.
#[test]
fn envelope_jsonl_roundtrip() {
    let original = Envelope::Fatal {
        ref_id: Some("ref-1".into()),
        error: "test error".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("ref-1"));
            assert_eq!(error, "test error");
        }
        _ => panic!("expected Fatal variant"),
    }
}

/// Regression: parse_version must reject various malformed inputs.
#[test]
fn parse_version_rejects_invalid() {
    assert!(parse_version("invalid").is_none());
    assert!(parse_version("v0.1").is_none());
    assert!(parse_version("abp/0.1").is_none());
    assert!(parse_version("abp/v").is_none());
    assert!(parse_version("abp/v1").is_none());
    assert!(parse_version("").is_none());
}

/// Regression: is_compatible_version requires same major version.
#[test]
fn version_compatibility_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
}

/// Regression: ProtocolVersion parse → display → parse must be idempotent.
#[test]
fn protocol_version_parse_roundtrip() {
    let original = "abp/v0.1";
    let v = ProtocolVersion::parse(original).unwrap();
    let displayed = v.to_string();
    assert_eq!(displayed, original);
    let reparsed = ProtocolVersion::parse(&displayed).unwrap();
    assert_eq!(v, reparsed);
}

/// Regression: ProtocolVersion ordering must be v0.1 < v0.2 < v1.0.
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
// Config precedence (env > file > default)
// ===========================================================================

/// Regression: parse_toml of empty string must produce sensible defaults.
#[test]
fn config_empty_toml_parses_to_defaults() {
    let cfg = parse_toml("").unwrap();
    assert_eq!(cfg.default_backend, None);
    assert!(cfg.backends.is_empty());
}

/// Regression: merge_configs overlay values must take precedence over base.
#[test]
fn config_merge_overlay_wins() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("info".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("openai".into()),
        log_level: None,
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("openai"));
    // overlay.log_level is None so base wins
    assert_eq!(merged.log_level.as_deref(), Some("info"));
}

/// Regression: merge_configs must combine backend maps.
#[test]
fn config_merge_combines_backends() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("a".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("b".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert!(merged.backends.contains_key("a"));
    assert!(merged.backends.contains_key("b"));
}

/// Regression: merge_configs overlay backend must win on name collision.
#[test]
fn config_merge_overlay_backend_wins_collision() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "python".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    match &merged.backends["sc"] {
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

/// Regression: validate_config must reject invalid log_level.
#[test]
fn config_validates_log_level() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

/// Regression: BackplaneConfig default must have log_level = "info".
#[test]
fn config_default_log_level_is_info() {
    let cfg = BackplaneConfig::default();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

// ===========================================================================
// IR type coercions and edge cases
// ===========================================================================

/// Regression: IrMessage::text must create a single Text content block.
#[test]
fn ir_message_text_creates_single_block() {
    let msg = IrMessage::text(IrRole::User, "hello");
    assert_eq!(msg.content.len(), 1);
    assert!(matches!(&msg.content[0], IrContentBlock::Text { text } if text == "hello"));
}

/// Regression: IrMessage::is_text_only must return true for text-only messages.
#[test]
fn ir_message_is_text_only_for_text() {
    let msg = IrMessage::text(IrRole::User, "hello");
    assert!(msg.is_text_only());
}

/// Regression: IrMessage::is_text_only must return false when tool_use present.
#[test]
fn ir_message_not_text_only_with_tool_use() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "read".into(),
            input: serde_json::json!({}),
        }],
    );
    assert!(!msg.is_text_only());
}

/// Regression: IrMessage::text_content must concatenate all Text blocks.
#[test]
fn ir_message_text_content_concatenates() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "hello ".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: serde_json::json!({}),
            },
            IrContentBlock::Text {
                text: "world".into(),
            },
        ],
    );
    assert_eq!(msg.text_content(), "hello world");
}

/// Regression: IrMessage::tool_use_blocks must return only ToolUse blocks.
#[test]
fn ir_message_tool_use_blocks_filters_correctly() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "thinking".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: serde_json::json!({}),
            },
            IrContentBlock::ToolUse {
                id: "t2".into(),
                name: "write".into(),
                input: serde_json::json!({}),
            },
        ],
    );
    assert_eq!(msg.tool_use_blocks().len(), 2);
}

/// Regression: IrConversation::new must be empty.
#[test]
fn ir_conversation_new_is_empty() {
    let conv = IrConversation::new();
    assert!(conv.is_empty());
    assert_eq!(conv.len(), 0);
    assert!(conv.system_message().is_none());
    assert!(conv.last_assistant().is_none());
    assert!(conv.last_message().is_none());
}

/// Regression: IrConversation::push must be chainable and preserve order.
#[test]
fn ir_conversation_push_is_chainable() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "you are helpful"))
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello!"));
    assert_eq!(conv.len(), 3);
    assert_eq!(
        conv.system_message().unwrap().text_content(),
        "you are helpful"
    );
    assert_eq!(conv.last_assistant().unwrap().text_content(), "hello!");
    assert_eq!(conv.last_message().unwrap().text_content(), "hello!");
}

/// Regression: IrConversation::messages_by_role must filter correctly.
#[test]
fn ir_conversation_messages_by_role() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "q1"))
        .push(IrMessage::text(IrRole::Assistant, "a1"))
        .push(IrMessage::text(IrRole::User, "q2"));
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 1);
    assert_eq!(conv.messages_by_role(IrRole::System).len(), 0);
}

/// Regression: IrConversation::tool_calls must aggregate across messages.
#[test]
fn ir_conversation_tool_calls_aggregates() {
    let conv = IrConversation::new()
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: serde_json::json!({}),
            }],
        ))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t2".into(),
                name: "write".into(),
                input: serde_json::json!({}),
            }],
        ));
    assert_eq!(conv.tool_calls().len(), 2);
}

/// Regression: IrUsage::from_io must set total_tokens = input + output.
#[test]
fn ir_usage_from_io_computes_total() {
    let u = IrUsage::from_io(100, 50);
    assert_eq!(u.total_tokens, 150);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
}

/// Regression: IrUsage::merge must sum all fields correctly.
#[test]
fn ir_usage_merge_sums_all_fields() {
    let u1 = IrUsage::with_cache(100, 50, 10, 5);
    let u2 = IrUsage::with_cache(200, 100, 20, 10);
    let merged = u1.merge(u2);
    assert_eq!(merged.input_tokens, 300);
    assert_eq!(merged.output_tokens, 150);
    // merge sums the stored total_tokens fields: (100+50) + (200+100) = 450
    assert_eq!(merged.total_tokens, 450);
    assert_eq!(merged.cache_read_tokens, 30);
    assert_eq!(merged.cache_write_tokens, 15);
}

/// Regression: IrUsage::default must be all zeros.
#[test]
fn ir_usage_default_is_zero() {
    let u = IrUsage::default();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
}

/// Regression: IrContentBlock serde tag is "type" with snake_case rename.
#[test]
fn ir_content_block_tag_is_type_snake_case() {
    let block = IrContentBlock::Text { text: "hi".into() };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "text");

    let tool = IrContentBlock::ToolUse {
        id: "t1".into(),
        name: "read".into(),
        input: serde_json::json!({}),
    };
    let v = serde_json::to_value(&tool).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "tool_use");
}

/// Regression: IrRole serde must use snake_case.
#[test]
fn ir_role_serde_snake_case() {
    let roles = [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ];
    let expected = ["\"system\"", "\"user\"", "\"assistant\"", "\"tool\""];
    for (role, exp) in roles.iter().zip(expected.iter()) {
        let json = serde_json::to_string(role).unwrap();
        assert_eq!(&json, exp);
    }
}

/// Regression: IrToolDefinition must roundtrip through JSON.
#[test]
fn ir_tool_definition_roundtrip() {
    let tool = IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "read_file");
    assert_eq!(back.description, "Read a file");
}

// ===========================================================================
// SDK dialect name normalization
// ===========================================================================

/// Regression: Dialect enum serde must use snake_case (open_ai, not OpenAI).
#[test]
fn dialect_serde_snake_case() {
    let d = Dialect::OpenAi;
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(json, "\"open_ai\"");
    let back: Dialect = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Dialect::OpenAi);
}

/// Regression: Dialect::label must produce human-readable names.
#[test]
fn dialect_label_human_readable() {
    assert_eq!(Dialect::OpenAi.label(), "OpenAI");
    assert_eq!(Dialect::Claude.label(), "Claude");
    assert_eq!(Dialect::Gemini.label(), "Gemini");
    assert_eq!(Dialect::Codex.label(), "Codex");
    assert_eq!(Dialect::Kimi.label(), "Kimi");
    assert_eq!(Dialect::Copilot.label(), "Copilot");
}

/// Regression: Dialect::all must return all 6 known dialects.
#[test]
fn dialect_all_returns_all_known() {
    let all = Dialect::all();
    assert_eq!(all.len(), 6);
    let set: HashSet<Dialect> = all.iter().copied().collect();
    assert!(set.contains(&Dialect::OpenAi));
    assert!(set.contains(&Dialect::Claude));
    assert!(set.contains(&Dialect::Gemini));
    assert!(set.contains(&Dialect::Codex));
    assert!(set.contains(&Dialect::Kimi));
    assert!(set.contains(&Dialect::Copilot));
}

/// Regression: all Dialect variants must roundtrip through serde.
#[test]
fn dialect_all_variants_roundtrip() {
    for &d in Dialect::all() {
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d, "Dialect {json} did not roundtrip");
    }
}

/// Regression: DialectDetector must return None for non-object JSON.
#[test]
fn dialect_detector_none_for_non_object() {
    let detector = DialectDetector::new();
    assert!(detector.detect(&serde_json::json!("hello")).is_none());
    assert!(detector.detect(&serde_json::json!(42)).is_none());
    assert!(detector.detect(&serde_json::json!([])).is_none());
    assert!(detector.detect(&serde_json::json!(null)).is_none());
}

/// Regression: DialectDetector must detect Claude-style messages.
#[test]
fn dialect_detector_detects_claude() {
    let detector = DialectDetector::new();
    let msg = serde_json::json!({
        "type": "message",
        "model": "claude-3",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn"
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Claude);
    assert!(result.confidence > 0.0);
}

/// Regression: DialectDetector must detect OpenAI-style messages.
#[test]
fn dialect_detector_detects_openai() {
    let detector = DialectDetector::new();
    let msg = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": "hi"}}],
        "model": "gpt-4",
        "temperature": 0.7
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::OpenAi);
}

/// Regression: DialectDetector must return None for empty object.
#[test]
fn dialect_detector_none_for_empty_object() {
    let detector = DialectDetector::new();
    assert!(detector.detect(&serde_json::json!({})).is_none());
}

// ===========================================================================
// Additional structural regression guards
// ===========================================================================

/// Regression: WorkOrder IDs must be UUIDv4.
#[test]
fn work_order_ids_are_uuids() {
    let wo = WorkOrderBuilder::new("test task").build();
    assert_eq!(wo.id.get_version_num(), 4);
    let s = wo.id.to_string();
    assert_eq!(s.len(), 36);
    let parts: Vec<&str> = s.split('-').collect();
    assert_eq!(parts.len(), 5);
    assert_eq!(
        parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
        vec![8, 4, 4, 4, 12]
    );
}

/// Regression: 100 WorkOrders must all have unique IDs.
#[test]
fn work_order_builder_produces_unique_ids() {
    let ids: HashSet<Uuid> = (0..100)
        .map(|_| WorkOrderBuilder::new("task").build().id)
        .collect();
    assert_eq!(ids.len(), 100, "100 WorkOrders must have 100 unique IDs");
}

/// Regression: ReceiptBuilder::build() must NOT set receipt_sha256.
#[test]
fn receipt_builder_build_has_no_hash() {
    let r = ReceiptBuilder::new("mock").build();
    assert!(
        r.receipt_sha256.is_none(),
        "build() must NOT set receipt_sha256"
    );
}

/// Regression: ReceiptBuilder::with_hash() must set a valid 64-char hash.
#[test]
fn receipt_builder_with_hash_sets_hash() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

/// Regression: ReceiptBuilder run_id must also be UUIDv4.
#[test]
fn receipt_run_id_is_uuid_v4() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.meta.run_id.get_version_num(), 4);
}

/// Regression: validate_receipt on freshly built receipt must pass.
#[test]
fn validate_receipt_on_fresh_receipt_passes() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(validate_receipt(&r).is_ok());
}

/// Regression: validate_receipt must detect a tampered hash.
#[test]
fn validate_receipt_detects_tampered_hash() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.receipt_sha256 = Some("0".repeat(64));
    assert!(validate_receipt(&r).is_err());
}

/// Regression: all Outcome variants must roundtrip through JSON serde.
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

/// Regression: all standard Capabilities must roundtrip through JSON.
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

/// Regression: sha256_hex must produce 64 lowercase hex chars for any input.
#[test]
fn sha256_hex_format() {
    let h = sha256_hex(b"hello world");
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(h, h.to_ascii_lowercase());
}

/// Regression: Hello envelope must contain backend field.
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
    assert!(v.get("backend").is_some());
    assert_eq!(v["backend"]["id"].as_str().unwrap(), "test");
}

/// Regression: Run envelope must contain work_order with its id.
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
    assert_eq!(v["work_order"]["id"].as_str().unwrap(), wo_id);
}

/// Regression: Event envelope must contain ref_id.
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

/// Regression: Final envelope must contain receipt with outcome.
#[test]
fn final_envelope_has_receipt() {
    let receipt = make_receipt();
    let fin = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let json = JsonlCodec::encode(&fin).unwrap();
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert!(v.get("receipt").is_some());
    assert!(v["receipt"].get("outcome").is_some());
}

/// Regression: Fatal envelope must contain error field.
#[test]
fn fatal_envelope_has_error() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&fatal).unwrap();
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(v["error"].as_str().unwrap(), "out of memory");
}

/// Regression: ProtocolVersion roundtrip for higher version numbers.
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

/// Regression: empty trace must produce a valid hash.
#[test]
fn empty_trace_produces_valid_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.trace.is_empty());
    let hash = receipt_hash(&r).unwrap();
    assert_eq!(hash.len(), 64);
}
