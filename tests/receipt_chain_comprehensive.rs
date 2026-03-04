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
//! Comprehensive receipt creation, hashing, and chain tests (80+).

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, ExecutionMode, Outcome, Receipt, RunMetadata, SupportLevel,
    UsageNormalized, VerificationReport, canonical_json, receipt_hash, sha256_hex,
};
use abp_receipt::serde_formats;
use abp_receipt::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore, ReceiptSummary};
use abp_receipt::{
    ChainBuilder, ChainError, ChainSummary, ReceiptAuditor, ReceiptBuilder, ReceiptChain,
    ReceiptValidator, TamperEvidence, TamperKind, ValidationError, canonicalize, compute_hash,
    diff_receipts, verify_hash, verify_receipt,
};
use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

// ── Helpers ────────────────────────────────────────────────────────

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap()
}

fn fixed_ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 1, 12, 5, 0).unwrap()
}

fn fixed_receipt(backend: &str) -> Receipt {
    let ts = fixed_ts();
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build()
}

fn hashed_receipt(backend: &str, ts: chrono::DateTime<Utc>) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap()
}

// ═══════════════════════════════════════════════════════════════════
// 1. Receipt construction with all fields
// ═══════════════════════════════════════════════════════════════════

#[test]
fn construct_receipt_manual_all_fields() {
    let ts = fixed_ts();
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "manual".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("0.1".into()),
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({"tokens": 100}),
        usage: UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(50),
            ..Default::default()
        },
        trace: vec![],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "fix.patch".into(),
        }],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    assert_eq!(receipt.backend.id, "manual");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("1.0"));
    assert_eq!(receipt.artifacts.len(), 1);
    assert_eq!(receipt.usage.input_tokens, Some(50));
}

#[test]
fn construct_receipt_builder_minimal() {
    let r = ReceiptBuilder::new("test").build();
    assert_eq!(r.backend.id, "test");
    assert_eq!(r.outcome, Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
}

#[test]
fn construct_receipt_builder_all_setters() {
    let ts1 = fixed_ts();
    let ts2 = fixed_ts2();
    let wo_id = Uuid::new_v4();
    let run = Uuid::new_v4();
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);

    let r = ReceiptBuilder::new("full")
        .outcome(Outcome::Partial)
        .backend_version("2.0")
        .adapter_version("0.5")
        .started_at(ts1)
        .finished_at(ts2)
        .work_order_id(wo_id)
        .run_id(run)
        .capabilities(caps)
        .mode(ExecutionMode::Passthrough)
        .usage_raw(serde_json::json!({"cost": 0.01}))
        .usage(UsageNormalized {
            input_tokens: Some(1000),
            output_tokens: Some(2000),
            cache_read_tokens: Some(500),
            cache_write_tokens: Some(100),
            request_units: Some(3),
            estimated_cost_usd: Some(0.01),
        })
        .verification(VerificationReport {
            git_diff: Some("diff".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        })
        .add_trace_event(AgentEvent {
            ts: ts1,
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();

    assert_eq!(r.meta.run_id, run);
    assert_eq!(r.meta.work_order_id, wo_id);
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.0"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.5"));
    assert_eq!(r.outcome, Outcome::Partial);
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert_eq!(r.meta.duration_ms, 300_000);
    assert_eq!(r.usage.input_tokens, Some(1000));
    assert_eq!(r.usage.cache_read_tokens, Some(500));
    assert_eq!(r.usage.request_units, Some(3));
    assert!(r.verification.harness_ok);
    assert_eq!(r.trace.len(), 1);
    assert_eq!(r.artifacts.len(), 1);
}

#[test]
fn construct_receipt_duration_computed_correctly() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 1, 30).unwrap();
    let r = ReceiptBuilder::new("x")
        .started_at(t1)
        .finished_at(t2)
        .build();
    assert_eq!(r.meta.duration_ms, 90_000);
}

#[test]
fn construct_receipt_zero_duration_same_timestamps() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("x")
        .started_at(ts)
        .finished_at(ts)
        .build();
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn construct_receipt_negative_duration_clamped_to_zero() {
    let t1 = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 11, 0, 0).unwrap();
    let r = ReceiptBuilder::new("x")
        .started_at(t1)
        .finished_at(t2)
        .build();
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn construct_receipt_contract_version_is_current() {
    let r = ReceiptBuilder::new("x").build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn construct_receipt_multiple_trace_events() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("x")
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .build();
    assert_eq!(r.trace.len(), 3);
}

#[test]
fn construct_receipt_multiple_artifacts() {
    let r = ReceiptBuilder::new("x")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "a.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();
    assert_eq!(r.artifacts.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════
// 2. Receipt with_hash() produces consistent hash
// ═══════════════════════════════════════════════════════════════════

#[test]
fn with_hash_produces_some() {
    let r = fixed_receipt("mock").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn with_hash_is_64_hex_chars() {
    let r = fixed_receipt("mock").with_hash().unwrap();
    let h = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn with_hash_deterministic_same_receipt() {
    let r1 = fixed_receipt("mock").with_hash().unwrap();
    let r2 = fixed_receipt("mock").with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn with_hash_changes_with_different_backend() {
    let r1 = fixed_receipt("alpha").with_hash().unwrap();
    let r2 = fixed_receipt("beta").with_hash().unwrap();
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn with_hash_changes_with_different_outcome() {
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("x")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("x")
        .outcome(Outcome::Failed)
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn with_hash_via_builder_matches_manual() {
    let r = fixed_receipt("mock");
    let manual_hash = receipt_hash(&r).unwrap();
    let hashed = r.with_hash().unwrap();
    assert_eq!(hashed.receipt_sha256.as_deref(), Some(manual_hash.as_str()));
}

#[test]
fn builder_with_hash_shortcut_works() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("x")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn with_hash_idempotent_double_hash() {
    let r = fixed_receipt("mock").with_hash().unwrap();
    let h1 = r.receipt_sha256.clone();
    let r2 = r.with_hash().unwrap();
    assert_eq!(h1, r2.receipt_sha256);
}

// ═══════════════════════════════════════════════════════════════════
// 3. receipt_hash() null-before-hashing behavior
// ═══════════════════════════════════════════════════════════════════

#[test]
fn receipt_hash_ignores_existing_hash_field() {
    let r = fixed_receipt("mock");
    let h_no_hash = receipt_hash(&r).unwrap();
    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("anything_here".into());
    let h_with_hash = receipt_hash(&r2).unwrap();
    assert_eq!(h_no_hash, h_with_hash);
}

#[test]
fn canonicalize_nullifies_receipt_sha256() {
    let mut r = fixed_receipt("mock");
    r.receipt_sha256 = Some("deadbeef".into());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
}

#[test]
fn canonicalize_produces_same_output_regardless_of_hash() {
    let r1 = fixed_receipt("mock");
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("something".into());
    let mut r3 = r1.clone();
    r3.receipt_sha256 = Some("other_thing".into());
    let c1 = canonicalize(&r1).unwrap();
    let c2 = canonicalize(&r2).unwrap();
    let c3 = canonicalize(&r3).unwrap();
    assert_eq!(c1, c2);
    assert_eq!(c2, c3);
}

#[test]
fn receipt_hash_output_is_sha256_hex() {
    let r = fixed_receipt("mock");
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_matches_compute_hash() {
    let r = fixed_receipt("mock");
    let h1 = receipt_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn canonical_json_compact_no_newlines() {
    let r = fixed_receipt("mock");
    let json = canonicalize(&r).unwrap();
    assert!(!json.contains('\n'));
    assert!(!json.contains("  "));
}

#[test]
fn sha256_hex_manual_verification() {
    let known = sha256_hex(b"hello");
    assert_eq!(known.len(), 64);
    assert_eq!(
        known,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 4. Receipt serde roundtrip
// ═══════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_minimal() {
    let r = fixed_receipt("mock");
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.meta.run_id, r2.meta.run_id);
}

#[test]
fn serde_roundtrip_with_hash() {
    let r = fixed_receipt("mock").with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn serde_roundtrip_preserves_all_fields() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("full")
        .outcome(Outcome::Partial)
        .backend_version("1.0")
        .adapter_version("0.5")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .mode(ExecutionMode::Passthrough)
        .usage_raw(serde_json::json!({"key": "value"}))
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(200),
            ..Default::default()
        })
        .verification(VerificationReport {
            git_diff: Some("diff".into()),
            git_status: None,
            harness_ok: true,
        })
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "a.patch".into(),
        })
        .build();

    let json = serde_json::to_string_pretty(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.backend.backend_version, r2.backend.backend_version);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.mode, r2.mode);
    assert_eq!(r.usage.input_tokens, r2.usage.input_tokens);
    assert_eq!(r.trace.len(), r2.trace.len());
    assert_eq!(r.artifacts.len(), r2.artifacts.len());
    assert!(r2.verification.harness_ok);
}

#[test]
fn serde_roundtrip_hash_still_valid() {
    let r = fixed_receipt("mock").with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&r2));
}

#[test]
fn serde_value_roundtrip() {
    let r = fixed_receipt("mock");
    let v = serde_json::to_value(&r).unwrap();
    let r2: Receipt = serde_json::from_value(v).unwrap();
    assert_eq!(r.meta.run_id, r2.meta.run_id);
}

#[test]
fn serde_outcome_string_representation() {
    let json_complete = serde_json::to_string(&Outcome::Complete).unwrap();
    let json_partial = serde_json::to_string(&Outcome::Partial).unwrap();
    let json_failed = serde_json::to_string(&Outcome::Failed).unwrap();
    assert_eq!(json_complete, "\"complete\"");
    assert_eq!(json_partial, "\"partial\"");
    assert_eq!(json_failed, "\"failed\"");
}

#[test]
fn serde_execution_mode_string_representation() {
    assert_eq!(
        serde_json::to_string(&ExecutionMode::Passthrough).unwrap(),
        "\"passthrough\""
    );
    assert_eq!(
        serde_json::to_string(&ExecutionMode::Mapped).unwrap(),
        "\"mapped\""
    );
}

#[test]
fn serde_roundtrip_with_ext_field() {
    let ts = fixed_ts();
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), serde_json::json!({"content": "hi"}));
    let r = ReceiptBuilder::new("x")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: Some(ext),
        })
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(r2.trace[0].ext.is_some());
}

// ═══════════════════════════════════════════════════════════════════
// 5. Receipt outcome variants (complete, partial, failed)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn outcome_complete() {
    let r = ReceiptBuilder::new("x").outcome(Outcome::Complete).build();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn outcome_partial() {
    let r = ReceiptBuilder::new("x").outcome(Outcome::Partial).build();
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn outcome_failed() {
    let r = ReceiptBuilder::new("x").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn outcome_equality() {
    assert_eq!(Outcome::Complete, Outcome::Complete);
    assert_eq!(Outcome::Partial, Outcome::Partial);
    assert_eq!(Outcome::Failed, Outcome::Failed);
    assert_ne!(Outcome::Complete, Outcome::Failed);
    assert_ne!(Outcome::Complete, Outcome::Partial);
    assert_ne!(Outcome::Partial, Outcome::Failed);
}

#[test]
fn outcome_serde_roundtrip_all_variants() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&outcome).unwrap();
        let parsed: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(outcome, parsed);
    }
}

#[test]
fn outcome_changes_hash() {
    let ts = fixed_ts();
    let hashes: Vec<String> = [Outcome::Complete, Outcome::Partial, Outcome::Failed]
        .iter()
        .map(|o| {
            let r = ReceiptBuilder::new("x")
                .outcome(o.clone())
                .run_id(Uuid::nil())
                .started_at(ts)
                .finished_at(ts)
                .build();
            receipt_hash(&r).unwrap()
        })
        .collect();
    assert_ne!(hashes[0], hashes[1]);
    assert_ne!(hashes[1], hashes[2]);
    assert_ne!(hashes[0], hashes[2]);
}

// ═══════════════════════════════════════════════════════════════════
// 6. BackendIdentity in receipts
// ═══════════════════════════════════════════════════════════════════

#[test]
fn backend_identity_minimal() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.backend.id, "mock");
    assert!(r.backend.backend_version.is_none());
    assert!(r.backend.adapter_version.is_none());
}

#[test]
fn backend_identity_with_versions() {
    let r = ReceiptBuilder::new("sidecar:node")
        .backend_version("18.0.0")
        .adapter_version("0.1.0")
        .build();
    assert_eq!(r.backend.id, "sidecar:node");
    assert_eq!(r.backend.backend_version.as_deref(), Some("18.0.0"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.1.0"));
}

#[test]
fn backend_identity_empty_string_id() {
    let r = ReceiptBuilder::new("").build();
    assert_eq!(r.backend.id, "");
}

#[test]
fn backend_identity_unicode() {
    let r = ReceiptBuilder::new("バックエンド🚀").build();
    assert_eq!(r.backend.id, "バックエンド🚀");
}

#[test]
fn backend_identity_serde_roundtrip() {
    let bi = BackendIdentity {
        id: "test".into(),
        backend_version: Some("1.0".into()),
        adapter_version: Some("0.5".into()),
    };
    let json = serde_json::to_string(&bi).unwrap();
    let bi2: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(bi.id, bi2.id);
    assert_eq!(bi.backend_version, bi2.backend_version);
    assert_eq!(bi.adapter_version, bi2.adapter_version);
}

#[test]
fn backend_id_change_invalidates_hash() {
    let mut r = fixed_receipt("original").with_hash().unwrap();
    assert!(verify_hash(&r));
    r.backend.id = "tampered".into();
    assert!(!verify_hash(&r));
}

// ═══════════════════════════════════════════════════════════════════
// 7. Receipt trace events
// ═══════════════════════════════════════════════════════════════════

#[test]
fn trace_empty_by_default() {
    let r = ReceiptBuilder::new("x").build();
    assert!(r.trace.is_empty());
}

#[test]
fn trace_run_started_event() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("x")
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        })
        .build();
    assert_eq!(r.trace.len(), 1);
    assert!(matches!(r.trace[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn trace_tool_call_event() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("x")
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc_1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "/tmp/file.txt"}),
            },
            ext: None,
        })
        .build();
    assert!(matches!(r.trace[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn trace_tool_result_event() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("x")
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc_1".into()),
                output: serde_json::json!("file contents"),
                is_error: false,
            },
            ext: None,
        })
        .build();
    assert!(matches!(r.trace[0].kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn trace_file_changed_event() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("x")
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "added fn".into(),
            },
            ext: None,
        })
        .build();
    assert!(matches!(
        r.trace[0].kind,
        AgentEventKind::FileChanged { .. }
    ));
}

#[test]
fn trace_affects_hash() {
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("x")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    let r2 = ReceiptBuilder::new("x")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn trace_large_count_hashable() {
    let ts = fixed_ts();
    let mut builder = ReceiptBuilder::new("x")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts);
    for i in 0..200 {
        builder = builder.add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantDelta {
                text: format!("tok-{i}"),
            },
            ext: None,
        });
    }
    let r = builder.with_hash().unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 200);
}

// ═══════════════════════════════════════════════════════════════════
// 8. Receipt contract version
// ═══════════════════════════════════════════════════════════════════

#[test]
fn contract_version_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn receipt_has_contract_version() {
    let r = ReceiptBuilder::new("x").build();
    assert_eq!(r.meta.contract_version, "abp/v0.1");
}

#[test]
fn contract_version_in_serialized_json() {
    let r = fixed_receipt("x");
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("abp/v0.1"));
}

#[test]
fn contract_version_in_canonical_json() {
    let r = fixed_receipt("x");
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("abp/v0.1"));
}

#[test]
fn contract_version_change_affects_hash() {
    let r1 = fixed_receipt("x");
    let h1 = receipt_hash(&r1).unwrap();
    let mut r2 = r1.clone();
    r2.meta.contract_version = "abp/v0.2".into();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

// ═══════════════════════════════════════════════════════════════════
// 9. Receipt deterministic serialization (BTreeMap)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn deterministic_canonical_json() {
    let r = fixed_receipt("x");
    let j1 = canonicalize(&r).unwrap();
    let j2 = canonicalize(&r).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn deterministic_hash_across_calls() {
    let r = fixed_receipt("x");
    let hashes: Vec<String> = (0..10).map(|_| receipt_hash(&r).unwrap()).collect();
    assert!(hashes.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn btreemap_capabilities_sorted() {
    let ts = fixed_ts();
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    caps.insert(Capability::ToolRead, SupportLevel::Native);

    let r = ReceiptBuilder::new("x")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .capabilities(caps)
        .build();
    let json = serde_json::to_string(&r).unwrap();

    // BTreeMap serializes keys in sorted order — streaming < tool_read < tool_write
    let streaming_pos = json.find("streaming").unwrap();
    let tool_read_pos = json.find("tool_read").unwrap();
    let tool_write_pos = json.find("tool_write").unwrap();
    assert!(streaming_pos < tool_read_pos);
    assert!(tool_read_pos < tool_write_pos);
}

#[test]
fn btreemap_vendor_config_sorted_in_usage_raw() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("x")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .usage_raw(serde_json::json!({"z_key": 1, "a_key": 2, "m_key": 3}))
        .build();
    let json = canonicalize(&r).unwrap();
    let a_pos = json.find("a_key").unwrap();
    let m_pos = json.find("m_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

#[test]
fn canonical_json_helper_sorts_keys() {
    let val = serde_json::json!({"z": 1, "a": 2, "m": 3});
    let json = canonical_json(&val).unwrap();
    let a_pos = json.find("\"a\"").unwrap();
    let m_pos = json.find("\"m\"").unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

// ═══════════════════════════════════════════════════════════════════
// 10. Receipt clone and equality
// ═══════════════════════════════════════════════════════════════════

#[test]
fn receipt_clone_produces_equal_hash() {
    let r = fixed_receipt("mock");
    let cloned = r.clone();
    assert_eq!(receipt_hash(&r).unwrap(), receipt_hash(&cloned).unwrap());
}

#[test]
fn receipt_clone_is_independent() {
    let r = fixed_receipt("mock");
    let mut cloned = r.clone();
    cloned.backend.id = "changed".into();
    assert_ne!(receipt_hash(&r).unwrap(), receipt_hash(&cloned).unwrap());
}

#[test]
fn receipt_clone_preserves_hash_field() {
    let r = fixed_receipt("mock").with_hash().unwrap();
    let cloned = r.clone();
    assert_eq!(r.receipt_sha256, cloned.receipt_sha256);
}

#[test]
fn receipt_debug_impl() {
    let r = fixed_receipt("mock");
    let debug = format!("{r:?}");
    assert!(debug.contains("Receipt"));
    assert!(debug.contains("mock"));
}

#[test]
fn outcome_clone() {
    let o = Outcome::Partial;
    let o2 = o.clone();
    assert_eq!(o, o2);
}

// ═══════════════════════════════════════════════════════════════════
// 11. Verification (verify_hash)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn verify_hash_passes_correct() {
    let r = fixed_receipt("mock").with_hash().unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_passes_when_none() {
    let r = fixed_receipt("mock");
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_fails_tampered_outcome() {
    let mut r = fixed_receipt("mock").with_hash().unwrap();
    r.outcome = Outcome::Failed;
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_tampered_backend() {
    let mut r = fixed_receipt("mock").with_hash().unwrap();
    r.backend.id = "evil".into();
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_garbage_hash() {
    let mut r = fixed_receipt("mock");
    r.receipt_sha256 = Some("not_a_real_hash".into());
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_empty_string_hash() {
    let mut r = fixed_receipt("mock");
    r.receipt_sha256 = Some(String::new());
    assert!(!verify_hash(&r));
}

// ═══════════════════════════════════════════════════════════════════
// 12. ReceiptChain operations
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_new_empty() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    assert!(chain.latest().is_none());
}

#[test]
fn chain_push_single() {
    let mut chain = ReceiptChain::new();
    let r = hashed_receipt("a", fixed_ts());
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
    assert!(!chain.is_empty());
}

#[test]
fn chain_push_multiple_ordered() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let ts3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();
    chain.push(hashed_receipt("a", ts1)).unwrap();
    chain.push(hashed_receipt("b", ts2)).unwrap();
    chain.push(hashed_receipt("c", ts3)).unwrap();
    assert_eq!(chain.len(), 3);
}

#[test]
fn chain_verify_empty_is_error() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.verify(), Err(ChainError::EmptyChain));
}

#[test]
fn chain_verify_single_ok() {
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("a", fixed_ts())).unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_verify_multiple_ok() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    chain.push(hashed_receipt("a", ts1)).unwrap();
    chain.push(hashed_receipt("b", ts2)).unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_rejects_duplicate_id() {
    let mut chain = ReceiptChain::new();
    let id = Uuid::new_v4();
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("a")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    chain.push(r1).unwrap();
    assert_eq!(chain.push(r2), Err(ChainError::DuplicateId { id }));
}

#[test]
fn chain_rejects_hash_mismatch() {
    let mut chain = ReceiptChain::new();
    let mut r = hashed_receipt("a", fixed_ts());
    r.outcome = Outcome::Failed; // tamper
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { .. })
    ));
}

#[test]
fn chain_rejects_out_of_order() {
    let mut chain = ReceiptChain::new();
    let ts_later = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();
    let ts_earlier = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    chain.push(hashed_receipt("a", ts_later)).unwrap();
    assert!(matches!(
        chain.push(hashed_receipt("b", ts_earlier)),
        Err(ChainError::BrokenLink { .. })
    ));
}

#[test]
fn chain_allows_same_timestamp() {
    let mut chain = ReceiptChain::new();
    let ts = fixed_ts();
    chain.push(hashed_receipt("a", ts)).unwrap();
    chain.push(hashed_receipt("b", ts)).unwrap();
    assert_eq!(chain.len(), 2);
}

#[test]
fn chain_latest_returns_last() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    chain.push(hashed_receipt("first", ts1)).unwrap();
    chain.push(hashed_receipt("second", ts2)).unwrap();
    assert_eq!(chain.latest().unwrap().backend.id, "second");
}

#[test]
fn chain_iter_order() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    chain.push(hashed_receipt("a", ts1)).unwrap();
    chain.push(hashed_receipt("b", ts2)).unwrap();
    let ids: Vec<_> = chain.iter().map(|r| r.backend.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b"]);
}

#[test]
fn chain_into_iter() {
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("x", fixed_ts())).unwrap();
    let count = (&chain).into_iter().count();
    assert_eq!(count, 1);
}

#[test]
fn chain_accepts_unhashed_receipt() {
    let mut chain = ReceiptChain::new();
    let r = ReceiptBuilder::new("x")
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .build();
    // No hash means verify_receipt_hash passes (no stored hash to mismatch)
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════
// 13. ChainError display
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_error_empty_display() {
    assert_eq!(ChainError::EmptyChain.to_string(), "chain is empty");
}

#[test]
fn chain_error_hash_mismatch_display() {
    let err = ChainError::HashMismatch { index: 5 };
    assert_eq!(err.to_string(), "hash mismatch at chain index 5");
}

#[test]
fn chain_error_broken_link_display() {
    let err = ChainError::BrokenLink { index: 2 };
    assert_eq!(err.to_string(), "broken link at chain index 2");
}

#[test]
fn chain_error_duplicate_id_display() {
    let id = Uuid::nil();
    let err = ChainError::DuplicateId { id };
    assert!(err.to_string().contains("duplicate"));
    assert!(err.to_string().contains(&id.to_string()));
}

#[test]
fn chain_error_is_std_error() {
    let err: Box<dyn std::error::Error> = Box::new(ChainError::EmptyChain);
    assert!(!err.to_string().is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// 14. Diff tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn diff_identical_is_empty() {
    let r = fixed_receipt("mock");
    let d = diff_receipts(&r, &r.clone());
    assert!(d.is_empty());
    assert_eq!(d.len(), 0);
}

#[test]
fn diff_detects_outcome() {
    let a = fixed_receipt("mock");
    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "outcome"));
}

#[test]
fn diff_detects_backend_id() {
    let a = fixed_receipt("old");
    let mut b = a.clone();
    b.backend.id = "new".into();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "backend.id"));
}

#[test]
fn diff_detects_multiple_changes() {
    let a = fixed_receipt("a");
    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    b.backend.id = "b".into();
    let d = diff_receipts(&a, &b);
    assert!(d.len() >= 2);
}

// ═══════════════════════════════════════════════════════════════════
// 15. Edge cases & unicode
// ═══════════════════════════════════════════════════════════════════

#[test]
fn empty_backend_id_hashable() {
    let r = fixed_receipt("");
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn unicode_backend_hashable() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("日本語テスト🎉")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn special_characters_in_fields() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("back\"end\\slash")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r2.backend.id, "back\"end\\slash");
}

#[test]
fn very_long_backend_id() {
    let long_id = "x".repeat(10_000);
    let ts = fixed_ts();
    let r = ReceiptBuilder::new(&long_id)
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_all_usage_fields() {
    let u = UsageNormalized {
        input_tokens: Some(1),
        output_tokens: Some(2),
        cache_read_tokens: Some(3),
        cache_write_tokens: Some(4),
        request_units: Some(5),
        estimated_cost_usd: Some(0.001),
    };
    let r = ReceiptBuilder::new("x").usage(u).build();
    assert_eq!(r.usage.input_tokens, Some(1));
    assert_eq!(r.usage.output_tokens, Some(2));
    assert_eq!(r.usage.cache_read_tokens, Some(3));
    assert_eq!(r.usage.cache_write_tokens, Some(4));
    assert_eq!(r.usage.request_units, Some(5));
    assert!((r.usage.estimated_cost_usd.unwrap() - 0.001).abs() < f64::EPSILON);
}

#[test]
fn receipt_default_usage_all_none() {
    let r = ReceiptBuilder::new("x").build();
    assert!(r.usage.input_tokens.is_none());
    assert!(r.usage.output_tokens.is_none());
    assert!(r.usage.cache_read_tokens.is_none());
    assert!(r.usage.cache_write_tokens.is_none());
    assert!(r.usage.request_units.is_none());
    assert!(r.usage.estimated_cost_usd.is_none());
}

#[test]
fn receipt_with_capabilities_hashable() {
    let ts = fixed_ts();
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    let r = ReceiptBuilder::new("x")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .capabilities(caps)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_passthrough_mode() {
    let r = ReceiptBuilder::new("x")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn receipt_default_mode_is_mapped() {
    let r = ReceiptBuilder::new("x").build();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

// ═══════════════════════════════════════════════════════════════════
// 16. Receipt store CRUD (InMemoryReceiptStore)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn store_new_is_empty() {
    let store = InMemoryReceiptStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
}

#[test]
fn store_insert_and_get() {
    let mut store = InMemoryReceiptStore::new();
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    let id = r.meta.run_id;
    store.store(r).unwrap();
    assert_eq!(store.len(), 1);
    let fetched = store.get(id).unwrap().unwrap();
    assert_eq!(fetched.backend.id, "mock");
}

#[test]
fn store_get_missing_returns_none() {
    let store = InMemoryReceiptStore::new();
    let result = store.get(Uuid::new_v4()).unwrap();
    assert!(result.is_none());
}

#[test]
fn store_duplicate_id_rejected() {
    let mut store = InMemoryReceiptStore::new();
    let id = Uuid::new_v4();
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("a")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    store.store(r1).unwrap();
    assert!(store.store(r2).is_err());
}

#[test]
fn store_multiple_receipts() {
    let mut store = InMemoryReceiptStore::new();
    for i in 0..5 {
        let r = ReceiptBuilder::new(format!("backend-{i}"))
            .with_hash()
            .unwrap();
        store.store(r).unwrap();
    }
    assert_eq!(store.len(), 5);
}

#[test]
fn store_list_all_with_default_filter() {
    let mut store = InMemoryReceiptStore::new();
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    store.store(r).unwrap();
    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(all.len(), 1);
}

#[test]
fn store_returns_receipt_id_matching_run_id() {
    let mut store = InMemoryReceiptStore::new();
    let r = ReceiptBuilder::new("test").with_hash().unwrap();
    let expected_id = r.meta.run_id;
    let returned_id = store.store(r).unwrap();
    assert_eq!(returned_id, expected_id);
}

// ═══════════════════════════════════════════════════════════════════
// 17. Receipt filtering
// ═══════════════════════════════════════════════════════════════════

#[test]
fn filter_by_backend_id() {
    let mut store = InMemoryReceiptStore::new();
    let ts = fixed_ts();
    for name in &["alpha", "beta", "alpha"] {
        store
            .store(
                ReceiptBuilder::new(*name)
                    .started_at(ts)
                    .finished_at(ts)
                    .with_hash()
                    .unwrap(),
            )
            .unwrap();
    }
    let filter = ReceiptFilter {
        backend_id: Some("alpha".into()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|s| s.backend_id == "alpha"));
}

#[test]
fn filter_by_outcome_complete() {
    let mut store = InMemoryReceiptStore::new();
    let ts = fixed_ts();
    store
        .store(
            ReceiptBuilder::new("a")
                .outcome(Outcome::Complete)
                .started_at(ts)
                .finished_at(ts)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    store
        .store(
            ReceiptBuilder::new("b")
                .outcome(Outcome::Failed)
                .started_at(ts)
                .finished_at(ts)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Complete),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].outcome, Outcome::Complete);
}

#[test]
fn filter_by_outcome_failed() {
    let mut store = InMemoryReceiptStore::new();
    let ts = fixed_ts();
    store
        .store(
            ReceiptBuilder::new("fail")
                .outcome(Outcome::Failed)
                .started_at(ts)
                .finished_at(ts)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    store
        .store(
            ReceiptBuilder::new("ok")
                .outcome(Outcome::Complete)
                .started_at(ts)
                .finished_at(ts)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    assert_eq!(store.list(&filter).unwrap().len(), 1);
}

#[test]
fn filter_by_time_range_after() {
    let mut store = InMemoryReceiptStore::new();
    let early = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let late = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    store
        .store(
            ReceiptBuilder::new("early")
                .started_at(early)
                .finished_at(early)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    store
        .store(
            ReceiptBuilder::new("late")
                .started_at(late)
                .finished_at(late)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let filter = ReceiptFilter {
        after: Some(Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend_id, "late");
}

#[test]
fn filter_by_time_range_before() {
    let mut store = InMemoryReceiptStore::new();
    let early = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let late = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();
    store
        .store(
            ReceiptBuilder::new("early")
                .started_at(early)
                .finished_at(early)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    store
        .store(
            ReceiptBuilder::new("late")
                .started_at(late)
                .finished_at(late)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let filter = ReceiptFilter {
        before: Some(Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend_id, "early");
}

#[test]
fn filter_combined_backend_and_outcome() {
    let mut store = InMemoryReceiptStore::new();
    let ts = fixed_ts();
    store
        .store(
            ReceiptBuilder::new("x")
                .outcome(Outcome::Complete)
                .started_at(ts)
                .finished_at(ts)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    store
        .store(
            ReceiptBuilder::new("x")
                .outcome(Outcome::Failed)
                .started_at(ts)
                .finished_at(ts)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    store
        .store(
            ReceiptBuilder::new("y")
                .outcome(Outcome::Complete)
                .started_at(ts)
                .finished_at(ts)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let filter = ReceiptFilter {
        backend_id: Some("x".into()),
        outcome: Some(Outcome::Complete),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn filter_no_match_returns_empty() {
    let mut store = InMemoryReceiptStore::new();
    let ts = fixed_ts();
    store
        .store(
            ReceiptBuilder::new("a")
                .started_at(ts)
                .finished_at(ts)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let filter = ReceiptFilter {
        backend_id: Some("nonexistent".into()),
        ..Default::default()
    };
    assert!(store.list(&filter).unwrap().is_empty());
}

#[test]
fn receipt_summary_from_receipt() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Partial)
        .started_at(ts)
        .finished_at(ts)
        .build();
    let summary = ReceiptSummary::from(&r);
    assert_eq!(summary.id, r.meta.run_id);
    assert_eq!(summary.backend_id, "test-backend");
    assert_eq!(summary.outcome, Outcome::Partial);
    assert_eq!(summary.started_at, ts);
}

// ═══════════════════════════════════════════════════════════════════
// 18. verify_receipt / VerificationResult
// ═══════════════════════════════════════════════════════════════════

#[test]
fn verify_receipt_valid_hashed() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let result = verify_receipt(&r);
    assert!(result.is_verified());
    assert!(result.hash_valid);
    assert!(result.contract_valid);
    assert!(result.timestamps_valid);
    assert!(result.outcome_consistent);
    assert!(result.issues.is_empty());
}

#[test]
fn verify_receipt_valid_unhashed() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let result = verify_receipt(&r);
    assert!(result.is_verified());
}

#[test]
fn verify_receipt_bad_hash() {
    let mut r = ReceiptBuilder::new("mock").with_hash().unwrap();
    r.receipt_sha256 = Some("badhash".into());
    let result = verify_receipt(&r);
    assert!(!result.is_verified());
    assert!(!result.hash_valid);
}

#[test]
fn verify_receipt_bad_contract_version() {
    let mut r = ReceiptBuilder::new("mock").build();
    r.meta.contract_version = "wrong/v9".into();
    let result = verify_receipt(&r);
    assert!(!result.contract_valid);
    assert!(!result.is_verified());
}

#[test]
fn verify_receipt_bad_timestamps() {
    let t1 = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 11, 0, 0).unwrap();
    let mut r = ReceiptBuilder::new("mock")
        .started_at(t1)
        .finished_at(t1)
        .build();
    r.meta.finished_at = t2; // manually break it
    let result = verify_receipt(&r);
    assert!(!result.timestamps_valid);
}

#[test]
fn verify_receipt_outcome_inconsistency_failed_no_error_event() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .started_at(ts)
        .finished_at(ts)
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .build();
    let result = verify_receipt(&r);
    assert!(!result.outcome_consistent);
}

#[test]
fn verify_receipt_outcome_inconsistency_complete_with_error() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts)
        .finished_at(ts)
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::Error {
                message: "oops".into(),
                error_code: None,
            },
            ext: None,
        })
        .build();
    let result = verify_receipt(&r);
    assert!(!result.outcome_consistent);
}

#[test]
fn verification_result_display_verified() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    let result = verify_receipt(&r);
    let display = format!("{result}");
    assert!(display.contains("verified"));
}

#[test]
fn verification_result_display_failed() {
    let mut r = ReceiptBuilder::new("mock").with_hash().unwrap();
    r.receipt_sha256 = Some("bad".into());
    let result = verify_receipt(&r);
    let display = format!("{result}");
    assert!(display.contains("failed"));
}

// ═══════════════════════════════════════════════════════════════════
// 19. ReceiptAuditor batch auditing
// ═══════════════════════════════════════════════════════════════════

#[test]
fn auditor_clean_batch() {
    let auditor = ReceiptAuditor::new();
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    let report = auditor.audit_batch(&[r]);
    assert!(report.is_clean());
    assert_eq!(report.total, 1);
    assert_eq!(report.valid, 1);
    assert_eq!(report.invalid, 0);
}

#[test]
fn auditor_empty_batch() {
    let auditor = ReceiptAuditor::new();
    let report = auditor.audit_batch(&[]);
    assert!(report.is_clean());
    assert_eq!(report.total, 0);
}

#[test]
fn auditor_detects_invalid_receipt() {
    let auditor = ReceiptAuditor::new();
    let mut r = ReceiptBuilder::new("mock").with_hash().unwrap();
    r.receipt_sha256 = Some("tampered".into());
    let report = auditor.audit_batch(&[r]);
    assert!(!report.is_clean());
    assert_eq!(report.invalid, 1);
}

#[test]
fn auditor_detects_duplicate_run_ids() {
    let auditor = ReceiptAuditor::new();
    let id = Uuid::new_v4();
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("a")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    let report = auditor.audit_batch(&[r1, r2]);
    assert!(!report.is_clean());
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.description.contains("duplicate"))
    );
}

#[test]
fn auditor_report_display() {
    let auditor = ReceiptAuditor::new();
    let report = auditor.audit_batch(&[]);
    let display = format!("{report}");
    assert!(display.contains("AuditReport"));
}

#[test]
fn audit_issue_display_with_index_and_run_id() {
    let issue = abp_receipt::AuditIssue {
        receipt_index: Some(0),
        run_id: Some("abc".into()),
        description: "test issue".into(),
    };
    let display = format!("{issue}");
    assert!(display.contains("#0"));
    assert!(display.contains("abc"));
    assert!(display.contains("test issue"));
}

// ═══════════════════════════════════════════════════════════════════
// 20. ReceiptValidator
// ═══════════════════════════════════════════════════════════════════

#[test]
fn validator_passes_good_receipt() {
    let v = ReceiptValidator::new();
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(v.validate(&r).is_ok());
}

#[test]
fn validator_catches_bad_contract_version() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock").build();
    r.meta.contract_version = "bad/version".into();
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.contract_version"));
}

#[test]
fn validator_catches_empty_backend_id() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock").build();
    r.backend.id = String::new();
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "backend.id"));
}

#[test]
fn validator_catches_bad_hash() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock").with_hash().unwrap();
    r.receipt_sha256 = Some("wrong".into());
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "receipt_sha256"));
}

#[test]
fn validator_catches_inconsistent_duration() {
    let v = ReceiptValidator::new();
    let ts = fixed_ts();
    let mut r = ReceiptBuilder::new("mock")
        .started_at(ts)
        .finished_at(ts)
        .build();
    r.meta.duration_ms = 99999;
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.duration_ms"));
}

#[test]
fn validation_error_display() {
    let err = ValidationError {
        field: "meta.contract_version".into(),
        message: "wrong version".into(),
    };
    let display = format!("{err}");
    assert!(display.contains("meta.contract_version"));
    assert!(display.contains("wrong version"));
}

// ═══════════════════════════════════════════════════════════════════
// 21. ChainBuilder
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_builder_empty() {
    let chain = ChainBuilder::new().build();
    assert!(chain.is_empty());
}

#[test]
fn chain_builder_single_append() {
    let r = hashed_receipt("a", fixed_ts());
    let chain = ChainBuilder::new().append(r).unwrap().build();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_builder_multiple_append() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let chain = ChainBuilder::new()
        .append(hashed_receipt("a", ts1))
        .unwrap()
        .append(hashed_receipt("b", ts2))
        .unwrap()
        .build();
    assert_eq!(chain.len(), 2);
}

#[test]
fn chain_builder_skip_validation() {
    let mut r = hashed_receipt("a", fixed_ts());
    r.receipt_sha256 = Some("fake_hash".into()); // would fail normal validation
    let chain = ChainBuilder::new()
        .skip_validation()
        .append(r)
        .unwrap()
        .build();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_builder_append_with_sequence() {
    let chain = ChainBuilder::new()
        .append_with_sequence(hashed_receipt("a", fixed_ts()), 10)
        .unwrap()
        .build();
    assert_eq!(chain.sequence_at(0), Some(10));
}

#[test]
fn chain_builder_rejects_duplicate_id() {
    let id = Uuid::new_v4();
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("a")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    let result = ChainBuilder::new().append(r1).unwrap().append(r2);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════
// 22. Chain advanced: verify_chain, detect_tampering, find_gaps, summary
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_verify_chain_single() {
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("a", fixed_ts())).unwrap();
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn chain_verify_chain_empty_is_error() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.verify_chain(), Err(ChainError::EmptyChain));
}

#[test]
fn chain_verify_chain_multiple_ok() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let ts3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("a", ts1)).unwrap();
    chain.push(hashed_receipt("b", ts2)).unwrap();
    chain.push(hashed_receipt("c", ts3)).unwrap();
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn chain_detect_tampering_clean() {
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("a", fixed_ts())).unwrap();
    let evidence = chain.detect_tampering();
    assert!(evidence.is_empty());
}

#[test]
fn chain_detect_tampering_hash_mismatch() {
    let mut r = hashed_receipt("a", fixed_ts());
    let chain = ChainBuilder::new()
        .skip_validation()
        .append({
            r.receipt_sha256 = Some("fake".into());
            r
        })
        .unwrap()
        .build();
    let evidence = chain.detect_tampering();
    assert!(!evidence.is_empty());
    assert!(matches!(evidence[0].kind, TamperKind::HashMismatch { .. }));
}

#[test]
fn chain_find_gaps_none() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    chain.push(hashed_receipt("a", ts1)).unwrap();
    chain.push(hashed_receipt("b", ts2)).unwrap();
    assert!(chain.find_gaps().is_empty());
}

#[test]
fn chain_find_gaps_with_gap() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let chain = ChainBuilder::new()
        .append_with_sequence(hashed_receipt("a", ts1), 0)
        .unwrap()
        .append_with_sequence(hashed_receipt("b", ts2), 5)
        .unwrap()
        .build();
    let gaps = chain.find_gaps();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].expected, 1);
    assert_eq!(gaps[0].actual, 5);
}

#[test]
fn chain_summary_empty() {
    let chain = ReceiptChain::new();
    let s = chain.chain_summary();
    assert_eq!(s.total_receipts, 0);
    assert!(s.first_started_at.is_none());
    assert!(s.last_finished_at.is_none());
}

#[test]
fn chain_summary_single() {
    let mut chain = ReceiptChain::new();
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts)
        .finished_at(ts)
        .usage_tokens(100, 200)
        .with_hash()
        .unwrap();
    chain.push(r).unwrap();
    let s = chain.chain_summary();
    assert_eq!(s.total_receipts, 1);
    assert_eq!(s.complete_count, 1);
    assert_eq!(s.failed_count, 0);
    assert_eq!(s.total_input_tokens, 100);
    assert_eq!(s.total_output_tokens, 200);
    assert_eq!(s.backends, vec!["mock"]);
    assert!(s.all_hashes_valid);
}

#[test]
fn chain_summary_mixed_outcomes() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let ts3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();
    chain
        .push(
            ReceiptBuilder::new("a")
                .outcome(Outcome::Complete)
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .outcome(Outcome::Failed)
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("a")
                .outcome(Outcome::Partial)
                .started_at(ts3)
                .finished_at(ts3)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let s = chain.chain_summary();
    assert_eq!(s.complete_count, 1);
    assert_eq!(s.failed_count, 1);
    assert_eq!(s.partial_count, 1);
    assert_eq!(s.backends.len(), 2); // "a" and "b"
}

#[test]
fn chain_sequence_numbers_auto_increment() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    chain.push(hashed_receipt("a", ts1)).unwrap();
    chain.push(hashed_receipt("b", ts2)).unwrap();
    assert_eq!(chain.sequence_at(0), Some(0));
    assert_eq!(chain.sequence_at(1), Some(1));
}

#[test]
fn chain_parent_hash_first_is_none() {
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("a", fixed_ts())).unwrap();
    assert!(chain.parent_hash_at(0).is_none());
}

#[test]
fn chain_parent_hash_second_links_to_first() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let r1 = hashed_receipt("a", ts1);
    let first_hash = r1.receipt_sha256.clone().unwrap();
    chain.push(r1).unwrap();
    chain.push(hashed_receipt("b", ts2)).unwrap();
    assert_eq!(chain.parent_hash_at(1), Some(first_hash.as_str()));
}

#[test]
fn chain_get_returns_correct_receipt() {
    let mut chain = ReceiptChain::new();
    let r = hashed_receipt("target", fixed_ts());
    chain.push(r).unwrap();
    assert_eq!(chain.get(0).unwrap().backend.id, "target");
    assert!(chain.get(1).is_none());
}

#[test]
fn chain_as_slice() {
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("a", fixed_ts())).unwrap();
    let slice = chain.as_slice();
    assert_eq!(slice.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════
// 23. serde_formats module
// ═══════════════════════════════════════════════════════════════════

#[test]
fn serde_formats_to_json_roundtrip() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    let json = serde_formats::to_json(&r).unwrap();
    let r2 = serde_formats::from_json(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert_eq!(r.backend.id, r2.backend.id);
}

#[test]
fn serde_formats_to_bytes_roundtrip() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    let bytes = serde_formats::to_bytes(&r).unwrap();
    let r2 = serde_formats::from_bytes(&bytes).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert_eq!(r.backend.id, r2.backend.id);
}

#[test]
fn serde_formats_to_json_is_pretty() {
    let r = ReceiptBuilder::new("mock").build();
    let json = serde_formats::to_json(&r).unwrap();
    assert!(json.contains('\n')); // pretty printed has newlines
}

#[test]
fn serde_formats_to_bytes_is_compact() {
    let r = ReceiptBuilder::new("mock").build();
    let bytes = serde_formats::to_bytes(&r).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains('\n'));
}

#[test]
fn serde_formats_from_json_invalid_fails() {
    let result = serde_formats::from_json("not json at all");
    assert!(result.is_err());
}

#[test]
fn serde_formats_from_bytes_invalid_fails() {
    let result = serde_formats::from_bytes(b"garbage");
    assert!(result.is_err());
}

#[test]
fn serde_formats_hash_survives_bytes_roundtrip() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    let bytes = serde_formats::to_bytes(&r).unwrap();
    let r2 = serde_formats::from_bytes(&bytes).unwrap();
    assert!(verify_hash(&r2));
}

// ═══════════════════════════════════════════════════════════════════
// 24. Chain serialization roundtrip
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_serde_roundtrip_single() {
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("a", fixed_ts())).unwrap();
    let json = serde_json::to_string(&chain).unwrap();
    let chain2: ReceiptChain = serde_json::from_str(&json).unwrap();
    assert_eq!(chain2.len(), 1);
    assert_eq!(chain2.get(0).unwrap().backend.id, "a");
}

#[test]
fn chain_serde_roundtrip_multiple() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("a", ts1)).unwrap();
    chain.push(hashed_receipt("b", ts2)).unwrap();
    let json = serde_json::to_string(&chain).unwrap();
    let chain2: ReceiptChain = serde_json::from_str(&json).unwrap();
    assert_eq!(chain2.len(), 2);
}

#[test]
fn chain_serde_roundtrip_preserves_verify() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("a", ts1)).unwrap();
    chain.push(hashed_receipt("b", ts2)).unwrap();
    let json = serde_json::to_string(&chain).unwrap();
    let chain2: ReceiptChain = serde_json::from_str(&json).unwrap();
    assert!(chain2.verify().is_ok());
}

// ═══════════════════════════════════════════════════════════════════
// 25. TamperEvidence / TamperKind display and serde
// ═══════════════════════════════════════════════════════════════════

#[test]
fn tamper_kind_hash_mismatch_display() {
    let tk = TamperKind::HashMismatch {
        stored: "abc".into(),
        computed: "def".into(),
    };
    let display = format!("{tk}");
    assert!(display.contains("abc"));
    assert!(display.contains("def"));
}

#[test]
fn tamper_kind_parent_link_broken_display() {
    let tk = TamperKind::ParentLinkBroken {
        expected: Some("hash1".into()),
        actual: None,
    };
    let display = format!("{tk}");
    assert!(display.contains("hash1"));
}

#[test]
fn tamper_evidence_display() {
    let te = TamperEvidence {
        index: 3,
        sequence: 3,
        kind: TamperKind::HashMismatch {
            stored: "a".into(),
            computed: "b".into(),
        },
    };
    let display = format!("{te}");
    assert!(display.contains("index=3"));
    assert!(display.contains("seq=3"));
}

#[test]
fn tamper_evidence_serde_roundtrip() {
    let te = TamperEvidence {
        index: 1,
        sequence: 1,
        kind: TamperKind::HashMismatch {
            stored: "aa".into(),
            computed: "bb".into(),
        },
    };
    let json = serde_json::to_string(&te).unwrap();
    let te2: TamperEvidence = serde_json::from_str(&json).unwrap();
    assert_eq!(te, te2);
}

#[test]
fn chain_gap_display() {
    let gap = abp_receipt::ChainGap {
        expected: 5,
        actual: 10,
        after_index: 4,
    };
    let display = format!("{gap}");
    assert!(display.contains("5"));
    assert!(display.contains("10"));
}

#[test]
fn chain_summary_serde_roundtrip() {
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("a", fixed_ts())).unwrap();
    let summary = chain.chain_summary();
    let json = serde_json::to_string(&summary).unwrap();
    let s2: ChainSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s2.total_receipts, 1);
}

// ═══════════════════════════════════════════════════════════════════
// 26. Diff additional tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn diff_detects_backend_id_change() {
    let a = fixed_receipt("alpha");
    let mut b = a.clone();
    b.backend.id = "beta".into();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "backend.id"));
}

#[test]
fn diff_detects_mode_change() {
    let ts = fixed_ts();
    let a = ReceiptBuilder::new("x")
        .mode(ExecutionMode::Mapped)
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    let mut b = a.clone();
    b.mode = ExecutionMode::Passthrough;
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "mode"));
}

#[test]
fn diff_detects_trace_length_change() {
    let ts = fixed_ts();
    let a = ReceiptBuilder::new("x")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    let b = ReceiptBuilder::new("x")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .build();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "trace.len"));
}

// ═══════════════════════════════════════════════════════════════════
// 27. Builder error() shortcut
// ═══════════════════════════════════════════════════════════════════

#[test]
fn builder_error_sets_outcome_and_trace() {
    let r = ReceiptBuilder::new("mock").error("something broke").build();
    assert_eq!(r.outcome, Outcome::Failed);
    assert_eq!(r.trace.len(), 1);
    assert!(matches!(r.trace[0].kind, AgentEventKind::Error { .. }));
}

#[test]
fn builder_usage_tokens_shortcut() {
    let r = ReceiptBuilder::new("mock").usage_tokens(500, 300).build();
    assert_eq!(r.usage.input_tokens, Some(500));
    assert_eq!(r.usage.output_tokens, Some(300));
}

#[test]
fn builder_model_and_dialect_in_usage_raw() {
    let r = ReceiptBuilder::new("mock")
        .model("gpt-4")
        .dialect("openai")
        .build();
    assert_eq!(r.usage_raw["model"], "gpt-4");
    assert_eq!(r.usage_raw["dialect"], "openai");
}

// ═══════════════════════════════════════════════════════════════════
// 28. ChainError equality and variants
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_error_parent_mismatch_display() {
    let err = ChainError::ParentMismatch { index: 3 };
    assert_eq!(err.to_string(), "parent hash mismatch at chain index 3");
}

#[test]
fn chain_error_sequence_gap_display() {
    let err = ChainError::SequenceGap {
        expected: 5,
        actual: 10,
    };
    let display = err.to_string();
    assert!(display.contains("5"));
    assert!(display.contains("10"));
}

#[test]
fn chain_error_equality() {
    assert_eq!(ChainError::EmptyChain, ChainError::EmptyChain);
    assert_eq!(
        ChainError::HashMismatch { index: 1 },
        ChainError::HashMismatch { index: 1 }
    );
    assert_ne!(
        ChainError::HashMismatch { index: 1 },
        ChainError::HashMismatch { index: 2 }
    );
}
