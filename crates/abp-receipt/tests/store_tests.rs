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
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Integration tests for `abp_receipt::store`.

use abp_core::ArtifactRef;
use abp_receipt::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore, StoreError};
use abp_receipt::{
    AgentEvent, AgentEventKind, ExecutionMode, Outcome, ReceiptBuilder, VerificationReport,
    compute_hash, verify_hash,
};
use chrono::{TimeZone, Utc};
use std::time::Duration;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn simple_receipt(backend: &str, outcome: Outcome) -> abp_receipt::Receipt {
    ReceiptBuilder::new(backend).outcome(outcome).build()
}

fn hashed_receipt(backend: &str) -> abp_receipt::Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap()
}

// =========================================================================
// 1. Basic CRUD (5 tests)
// =========================================================================

#[test]
fn store_and_retrieve_by_id() {
    let mut store = InMemoryReceiptStore::new();
    let receipt = hashed_receipt("mock");
    let id = receipt.meta.run_id;

    let stored_id = store.store(receipt).unwrap();
    assert_eq!(stored_id, id);

    let fetched = store.get(id).unwrap().expect("receipt should exist");
    assert_eq!(fetched.meta.run_id, id);
    assert_eq!(fetched.backend.id, "mock");
}

#[test]
fn store_multiple_and_list_all() {
    let mut store = InMemoryReceiptStore::new();

    for i in 0..5 {
        let r = simple_receipt(&format!("backend-{i}"), Outcome::Complete);
        store.store(r).unwrap();
    }

    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(all.len(), 5);
    assert_eq!(store.len(), 5);
}

#[test]
fn duplicate_id_rejected() {
    let mut store = InMemoryReceiptStore::new();
    let run_id = Uuid::new_v4();

    let r1 = ReceiptBuilder::new("a").run_id(run_id).build();
    let r2 = ReceiptBuilder::new("b").run_id(run_id).build();

    store.store(r1).unwrap();
    let err = store.store(r2).unwrap_err();
    assert!(matches!(err, StoreError::DuplicateId(id) if id == run_id));
    assert_eq!(store.len(), 1);
}

#[test]
fn store_is_immutable_no_update() {
    // The store has no update method — storing the same ID twice is an error.
    let mut store = InMemoryReceiptStore::new();
    let run_id = Uuid::new_v4();

    let r1 = ReceiptBuilder::new("v1")
        .run_id(run_id)
        .outcome(Outcome::Complete)
        .build();
    store.store(r1).unwrap();

    let r2 = ReceiptBuilder::new("v2")
        .run_id(run_id)
        .outcome(Outcome::Failed)
        .build();
    assert!(store.store(r2).is_err());

    // Original receipt is untouched.
    let fetched = store.get(run_id).unwrap().unwrap();
    assert_eq!(fetched.backend.id, "v1");
    assert_eq!(fetched.outcome, Outcome::Complete);
}

#[test]
fn get_nonexistent_returns_none() {
    let store = InMemoryReceiptStore::new();
    let result = store.get(Uuid::new_v4()).unwrap();
    assert!(result.is_none());
}

// =========================================================================
// 2. Query / Filtering (5 tests)
// =========================================================================

#[test]
fn query_by_run_id_via_get() {
    let mut store = InMemoryReceiptStore::new();
    let r1 = simple_receipt("a", Outcome::Complete);
    let r2 = simple_receipt("b", Outcome::Failed);
    let id1 = r1.meta.run_id;
    let id2 = r2.meta.run_id;

    store.store(r1).unwrap();
    store.store(r2).unwrap();

    let got1 = store.get(id1).unwrap().unwrap();
    assert_eq!(got1.backend.id, "a");

    let got2 = store.get(id2).unwrap().unwrap();
    assert_eq!(got2.backend.id, "b");
}

#[test]
fn filter_by_backend() {
    let mut store = InMemoryReceiptStore::new();
    for _ in 0..3 {
        store
            .store(simple_receipt("alpha", Outcome::Complete))
            .unwrap();
    }
    for _ in 0..2 {
        store
            .store(simple_receipt("beta", Outcome::Complete))
            .unwrap();
    }

    let filter = ReceiptFilter {
        backend_id: Some("alpha".into()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|s| s.backend_id == "alpha"));
}

#[test]
fn filter_by_time_range() {
    let mut store = InMemoryReceiptStore::new();

    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    for (t, label) in [(t1, "jan"), (t2, "jun"), (t3, "dec")] {
        let r = ReceiptBuilder::new(label)
            .started_at(t)
            .finished_at(t)
            .build();
        store.store(r).unwrap();
    }

    // After May
    let filter = ReceiptFilter {
        after: Some(Utc.with_ymd_and_hms(2025, 5, 1, 0, 0, 0).unwrap()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 2);

    // Before July
    let filter = ReceiptFilter {
        before: Some(Utc.with_ymd_and_hms(2025, 7, 1, 0, 0, 0).unwrap()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 2);

    // Between March and September
    let filter = ReceiptFilter {
        after: Some(Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap()),
        before: Some(Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend_id, "jun");
}

#[test]
fn filter_by_outcome() {
    let mut store = InMemoryReceiptStore::new();
    store.store(simple_receipt("a", Outcome::Complete)).unwrap();
    store.store(simple_receipt("b", Outcome::Failed)).unwrap();
    store.store(simple_receipt("c", Outcome::Partial)).unwrap();
    store.store(simple_receipt("d", Outcome::Failed)).unwrap();

    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|s| s.outcome == Outcome::Failed));
}

#[test]
fn empty_store_returns_empty_results() {
    let store = InMemoryReceiptStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);

    let results = store.list(&ReceiptFilter::default()).unwrap();
    assert!(results.is_empty());

    let results = store
        .list(&ReceiptFilter {
            backend_id: Some("anything".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(results.is_empty());
}

// =========================================================================
// 3. Persistence / Behavior (5 tests)
// =========================================================================

#[test]
fn in_memory_store_default_is_empty() {
    let store = InMemoryReceiptStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
}

#[test]
fn receipts_survive_across_operations() {
    let mut store = InMemoryReceiptStore::new();

    let r1 = simple_receipt("first", Outcome::Complete);
    let id1 = r1.meta.run_id;
    store.store(r1).unwrap();

    // Perform other operations in between.
    let _ = store.get(Uuid::new_v4()); // miss
    let _ = store.list(&ReceiptFilter::default()); // list
    let _ = store.store(simple_receipt("second", Outcome::Failed)); // another store

    // Original receipt still there.
    let fetched = store.get(id1).unwrap().unwrap();
    assert_eq!(fetched.backend.id, "first");
    assert_eq!(store.len(), 2);
}

#[test]
fn concurrent_reads_are_safe() {
    // InMemoryReceiptStore uses a plain BTreeMap (no interior mutability),
    // so concurrent reads from a shared reference are safe by construction.
    let mut store = InMemoryReceiptStore::new();
    for i in 0..10 {
        let r = simple_receipt(&format!("b-{i}"), Outcome::Complete);
        store.store(r).unwrap();
    }

    // Multiple immutable borrows coexist — Rust's type system guarantees
    // this, but we exercise it explicitly.
    let list1 = store.list(&ReceiptFilter::default()).unwrap();
    let list2 = store
        .list(&ReceiptFilter {
            backend_id: Some("b-0".into()),
            ..Default::default()
        })
        .unwrap();
    let item = store.get(list1[0].id).unwrap();

    assert_eq!(list1.len(), 10);
    assert_eq!(list2.len(), 1);
    assert!(item.is_some());
}

#[test]
fn large_receipt_payload() {
    let mut store = InMemoryReceiptStore::new();

    // Build a receipt with a large trace and usage_raw payload.
    let big_text = "x".repeat(100_000);
    let events: Vec<AgentEvent> = (0..500)
        .map(|i| AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("chunk-{i}"),
            },
            ext: None,
        })
        .collect();

    let receipt = ReceiptBuilder::new("large-backend")
        .outcome(Outcome::Complete)
        .usage_raw(serde_json::json!({ "big_field": big_text }))
        .events(events)
        .build();
    let id = receipt.meta.run_id;

    store.store(receipt).unwrap();

    let fetched = store.get(id).unwrap().unwrap();
    assert_eq!(fetched.trace.len(), 500);
    assert_eq!(
        fetched.usage_raw["big_field"].as_str().unwrap().len(),
        100_000
    );
}

#[test]
fn receipt_with_all_fields_populated() {
    let mut store = InMemoryReceiptStore::new();

    let run_id = Uuid::new_v4();
    let wo_id = Uuid::new_v4();
    let started = Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap();

    let receipt = ReceiptBuilder::new("full-backend")
        .run_id(run_id)
        .work_order_id(wo_id)
        .backend_version("2.1.0")
        .adapter_version("0.3.0")
        .model("gpt-5")
        .dialect("openai")
        .mode(ExecutionMode::Passthrough)
        .outcome(Outcome::Complete)
        .started_at(started)
        .duration(Duration::from_secs(42))
        .usage_tokens(1000, 2000)
        .usage_raw(serde_json::json!({ "custom": true }))
        .verification(VerificationReport {
            git_diff: Some("diff --git a/f b/f".into()),
            git_status: Some("M f".into()),
            harness_ok: true,
        })
        .add_event(AgentEvent {
            ts: started,
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "file".into(),
            path: "/tmp/out.txt".into(),
        })
        .with_hash()
        .unwrap();

    let stored_id = store.store(receipt).unwrap();
    assert_eq!(stored_id, run_id);

    let r = store.get(run_id).unwrap().unwrap();
    assert_eq!(r.meta.run_id, run_id);
    assert_eq!(r.meta.work_order_id, wo_id);
    assert_eq!(r.backend.id, "full-backend");
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.1.0"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.3.0"));
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(r.trace.len(), 1);
    assert_eq!(r.artifacts.len(), 1);
    assert!(r.verification.harness_ok);
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.usage.input_tokens, Some(1000));
    assert_eq!(r.usage.output_tokens, Some(2000));
    assert_eq!(r.usage_raw["model"], "gpt-5");
    assert_eq!(r.usage_raw["dialect"], "openai");
    assert_eq!(r.usage_raw["custom"], true);
}

// =========================================================================
// 4. Serde / Integrity (5 tests)
// =========================================================================

#[test]
fn receipt_roundtrip_through_store() {
    let mut store = InMemoryReceiptStore::new();
    let original = hashed_receipt("roundtrip");
    let id = original.meta.run_id;

    // Serialize to JSON, then deserialize, then store.
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: abp_receipt::Receipt = serde_json::from_str(&json).unwrap();

    store.store(deserialized).unwrap();
    let fetched = store.get(id).unwrap().unwrap();

    // Structural equality through re-serialization.
    let fetched_json = serde_json::to_string(fetched).unwrap();
    assert_eq!(json, fetched_json);
}

#[test]
fn hash_integrity_maintained_after_store() {
    let mut store = InMemoryReceiptStore::new();
    let receipt = hashed_receipt("hashed");
    let id = receipt.meta.run_id;
    let original_hash = receipt.receipt_sha256.clone().unwrap();

    store.store(receipt).unwrap();
    let fetched = store.get(id).unwrap().unwrap();

    assert_eq!(fetched.receipt_sha256.as_ref().unwrap(), &original_hash);
    assert!(verify_hash(fetched));

    // Recomputing the hash from scratch matches.
    let recomputed = compute_hash(fetched).unwrap();
    assert_eq!(recomputed, original_hash);
}

#[test]
fn metadata_preserved_in_store() {
    let mut store = InMemoryReceiptStore::new();

    let run_id = Uuid::new_v4();
    let wo_id = Uuid::new_v4();
    let started = Utc.with_ymd_and_hms(2025, 3, 1, 10, 30, 0).unwrap();
    let finished = Utc.with_ymd_and_hms(2025, 3, 1, 10, 31, 0).unwrap();

    let receipt = ReceiptBuilder::new("meta-test")
        .run_id(run_id)
        .work_order_id(wo_id)
        .started_at(started)
        .finished_at(finished)
        .backend_version("1.2.3")
        .adapter_version("4.5.6")
        .build();

    store.store(receipt).unwrap();
    let r = store.get(run_id).unwrap().unwrap();

    assert_eq!(r.meta.run_id, run_id);
    assert_eq!(r.meta.work_order_id, wo_id);
    assert_eq!(r.meta.started_at, started);
    assert_eq!(r.meta.finished_at, finished);
    assert_eq!(r.backend.id, "meta-test");
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.2.3"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("4.5.6"));
}

#[test]
fn events_preserved_in_store() {
    let mut store = InMemoryReceiptStore::new();

    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({ "path": "/etc/hosts" }),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-1".into()),
                output: serde_json::json!("127.0.0.1 localhost"),
                is_error: false,
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ];

    let receipt = ReceiptBuilder::new("evt-test")
        .events(events.clone())
        .build();
    let id = receipt.meta.run_id;

    store.store(receipt).unwrap();
    let fetched = store.get(id).unwrap().unwrap();

    assert_eq!(fetched.trace.len(), 4);

    // Verify event kinds round-tripped.
    assert!(matches!(
        fetched.trace[0].kind,
        AgentEventKind::RunStarted { .. }
    ));
    assert!(matches!(
        fetched.trace[1].kind,
        AgentEventKind::ToolCall { .. }
    ));
    assert!(matches!(
        fetched.trace[2].kind,
        AgentEventKind::ToolResult { .. }
    ));
    assert!(matches!(
        fetched.trace[3].kind,
        AgentEventKind::RunCompleted { .. }
    ));

    // Verify tool call details.
    if let AgentEventKind::ToolCall {
        ref tool_name,
        ref tool_use_id,
        ref input,
        ..
    } = fetched.trace[1].kind
    {
        assert_eq!(tool_name, "read_file");
        assert_eq!(tool_use_id.as_deref(), Some("tc-1"));
        assert_eq!(input["path"], "/etc/hosts");
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn timestamps_preserved_in_store() {
    let mut store = InMemoryReceiptStore::new();

    let started = Utc.with_ymd_and_hms(2024, 12, 25, 0, 0, 0).unwrap();
    let finished = Utc.with_ymd_and_hms(2024, 12, 25, 0, 5, 0).unwrap();

    let receipt = ReceiptBuilder::new("ts-test")
        .started_at(started)
        .finished_at(finished)
        .build();
    let id = receipt.meta.run_id;

    store.store(receipt).unwrap();
    let r = store.get(id).unwrap().unwrap();

    assert_eq!(r.meta.started_at, started);
    assert_eq!(r.meta.finished_at, finished);
    assert_eq!(r.meta.duration_ms, 300_000); // 5 minutes
}

// =========================================================================
// 5. Extra coverage (5+ additional tests)
// =========================================================================

#[test]
fn list_summary_fields_match_receipt() {
    let mut store = InMemoryReceiptStore::new();
    let started = Utc.with_ymd_and_hms(2025, 1, 15, 8, 0, 0).unwrap();
    let finished = Utc.with_ymd_and_hms(2025, 1, 15, 8, 1, 0).unwrap();

    let receipt = ReceiptBuilder::new("summary-check")
        .outcome(Outcome::Partial)
        .started_at(started)
        .finished_at(finished)
        .build();
    let id = receipt.meta.run_id;
    store.store(receipt).unwrap();

    let summaries = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(summaries.len(), 1);
    let s = &summaries[0];
    assert_eq!(s.id, id);
    assert_eq!(s.backend_id, "summary-check");
    assert_eq!(s.outcome, Outcome::Partial);
    assert_eq!(s.started_at, started);
    assert_eq!(s.finished_at, finished);
}

#[test]
fn combined_filters_intersect() {
    let mut store = InMemoryReceiptStore::new();

    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();

    // alpha + complete + jan
    store
        .store(
            ReceiptBuilder::new("alpha")
                .outcome(Outcome::Complete)
                .started_at(t1)
                .finished_at(t1)
                .build(),
        )
        .unwrap();
    // alpha + failed + jun
    store
        .store(
            ReceiptBuilder::new("alpha")
                .outcome(Outcome::Failed)
                .started_at(t2)
                .finished_at(t2)
                .build(),
        )
        .unwrap();
    // beta + complete + jun
    store
        .store(
            ReceiptBuilder::new("beta")
                .outcome(Outcome::Complete)
                .started_at(t2)
                .finished_at(t2)
                .build(),
        )
        .unwrap();

    // backend=alpha AND outcome=failed
    let filter = ReceiptFilter {
        backend_id: Some("alpha".into()),
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend_id, "alpha");
    assert_eq!(results[0].outcome, Outcome::Failed);

    // backend=alpha AND after=March
    let filter = ReceiptFilter {
        backend_id: Some("alpha".into()),
        after: Some(Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn filter_no_match_returns_empty() {
    let mut store = InMemoryReceiptStore::new();
    store
        .store(simple_receipt("real", Outcome::Complete))
        .unwrap();

    let filter = ReceiptFilter {
        backend_id: Some("nonexistent".into()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert!(results.is_empty());
}

#[test]
fn error_builder_sets_failed_and_trace() {
    let mut store = InMemoryReceiptStore::new();

    let receipt = ReceiptBuilder::new("err-backend")
        .error("something went wrong")
        .build();
    let id = receipt.meta.run_id;
    store.store(receipt).unwrap();

    let r = store.get(id).unwrap().unwrap();
    assert_eq!(r.outcome, Outcome::Failed);
    assert_eq!(r.trace.len(), 1);
    assert!(matches!(
        r.trace[0].kind,
        AgentEventKind::Error { ref message, .. } if message == "something went wrong"
    ));
}

#[test]
fn store_clone_is_independent() {
    let mut store = InMemoryReceiptStore::new();
    store.store(simple_receipt("a", Outcome::Complete)).unwrap();

    let store2 = store.clone();

    // Mutating the original does not affect the clone.
    store.store(simple_receipt("b", Outcome::Failed)).unwrap();

    assert_eq!(store.len(), 2);
    assert_eq!(store2.len(), 1);
}

#[test]
fn many_receipts_stress() {
    let mut store = InMemoryReceiptStore::new();
    let n = 1_000;

    for i in 0..n {
        let r = ReceiptBuilder::new(format!("b-{}", i % 10))
            .outcome(if i % 3 == 0 {
                Outcome::Failed
            } else {
                Outcome::Complete
            })
            .build();
        store.store(r).unwrap();
    }

    assert_eq!(store.len(), n);

    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(all.len(), n);

    let failed = store
        .list(&ReceiptFilter {
            outcome: Some(Outcome::Failed),
            ..Default::default()
        })
        .unwrap();
    // Every 3rd receipt is failed: 0, 3, 6, … → ceil(1000/3) = 334
    assert_eq!(failed.len(), 334);

    let backend_0 = store
        .list(&ReceiptFilter {
            backend_id: Some("b-0".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(backend_0.len(), 100);
}
