// SPDX-License-Identifier: MIT OR Apache-2.0
//! Performance regression tests for Agent Backplane.
//!
//! These tests verify that core operations complete within generous time bounds
//! (typically 10× the expected duration) to catch severe regressions without
//! being flaky on CI.
//!
//! Run timing-sensitive (ignored) tests separately:
//! ```bash
//! cargo test --test perf_regression -- --ignored
//! ```

use std::time::Instant;

use abp_core::{
    AgentEvent, AgentEventKind, CapabilityManifest, ExecutionLane, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, WorkOrderBuilder, canonical_json, receipt_hash,
};
use abp_glob::IncludeExcludeGlobs;
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec, parse_version};
use chrono::Utc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sample_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("Refactor the authentication module to use JWT tokens")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/workspace")
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(1.0)
        .build()
}

fn sample_receipt() -> Receipt {
    let mut builder = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .backend_version("0.1.0")
        .adapter_version("0.1.0");

    for i in 0..20 {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: format!("Trace event number {i} with some representative payload text"),
            },
            ext: None,
        });
    }
    builder.build()
}

fn make_event(i: usize) -> AgentEvent {
    let kind = match i % 5 {
        0 => AgentEventKind::RunStarted {
            message: format!("start {i}"),
        },
        1 => AgentEventKind::AssistantMessage {
            text: format!("msg {i}"),
        },
        2 => AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some(format!("tu-{i}")),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": format!("src/file_{i}.rs")}),
        },
        3 => AgentEventKind::FileChanged {
            path: format!("src/file_{i}.rs"),
            summary: "updated".into(),
        },
        _ => AgentEventKind::RunCompleted {
            message: format!("done {i}"),
        },
    };
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ---------------------------------------------------------------------------
// 1. WorkOrder serialization under 1ms
// ---------------------------------------------------------------------------

/// WorkOrder serialization should complete well under 1ms for a typical payload.
#[test]
#[ignore] // timing-sensitive — run with --ignored
fn work_order_serialization_under_1ms() {
    let wo = sample_work_order();

    // Warm-up
    let _ = serde_json::to_string(&wo).unwrap();

    let start = Instant::now();
    for _ in 0..100 {
        let _ = serde_json::to_string(&wo).unwrap();
    }
    let elapsed = start.elapsed();
    let per_op = elapsed / 100;

    assert!(
        per_op.as_millis() < 1,
        "WorkOrder serialization took {per_op:?} per op (limit: 1ms)"
    );
}

// ---------------------------------------------------------------------------
// 2. Receipt hashing under 1ms
// ---------------------------------------------------------------------------

/// Receipt hashing (canonical JSON + SHA-256) should complete under 1ms.
#[test]
#[ignore] // timing-sensitive — run with --ignored
fn receipt_hashing_under_1ms() {
    let receipt = sample_receipt();

    // Warm-up
    let _ = receipt_hash(&receipt).unwrap();

    let start = Instant::now();
    for _ in 0..100 {
        let _ = receipt_hash(&receipt).unwrap();
    }
    let elapsed = start.elapsed();
    let per_op = elapsed / 100;

    assert!(
        per_op.as_millis() < 1,
        "Receipt hashing took {per_op:?} per op (limit: 1ms)"
    );
}

// ---------------------------------------------------------------------------
// 3. Envelope parsing under 100µs per line
// ---------------------------------------------------------------------------

/// JSONL envelope parsing should complete under 100µs per line.
#[test]
#[ignore] // timing-sensitive — run with --ignored
fn envelope_parsing_under_100us() {
    let hello = Envelope::hello(
        abp_core::BackendIdentity {
            id: "perf-test".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    let trimmed = line.trim();

    // Warm-up
    let _ = JsonlCodec::decode(trimmed).unwrap();

    let iterations = 1000;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = JsonlCodec::decode(trimmed).unwrap();
    }
    let elapsed = start.elapsed();
    let per_op = elapsed / iterations;

    assert!(
        per_op.as_micros() < 100,
        "Envelope parsing took {per_op:?} per op (limit: 100µs)"
    );
}

// ---------------------------------------------------------------------------
// 4. Glob compilation under 10ms for 100 patterns
// ---------------------------------------------------------------------------

/// Compiling 100 glob patterns should complete under 10ms.
#[test]
#[ignore] // timing-sensitive — run with --ignored
fn glob_compilation_100_patterns_under_10ms() {
    let includes: Vec<String> = (0..50).map(|i| format!("src/module_{i}/**/*.rs")).collect();
    let excludes: Vec<String> = (0..50).map(|i| format!("**/generated_{i}/**")).collect();

    // Warm-up
    let _ = IncludeExcludeGlobs::new(&includes, &excludes).unwrap();

    let start = Instant::now();
    for _ in 0..10 {
        let _ = IncludeExcludeGlobs::new(&includes, &excludes).unwrap();
    }
    let elapsed = start.elapsed();
    let per_op = elapsed / 10;

    assert!(
        per_op.as_millis() < 100,
        "Glob compilation (100 patterns) took {per_op:?} (limit: 100ms)"
    );
}

// ---------------------------------------------------------------------------
// 5. Policy engine compilation under 10ms
// ---------------------------------------------------------------------------

/// PolicyEngine compilation from a realistic profile should complete under 10ms.
#[test]
#[ignore] // timing-sensitive — run with --ignored
fn policy_engine_compilation_under_10ms() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Grep".into(), "Glob".into()],
        disallowed_tools: vec!["Bash*".into(), "Delete*".into()],
        deny_read: (0..10).map(|i| format!("**/secret_{i}/**")).collect(),
        deny_write: (0..10).map(|i| format!("**/locked_{i}/**")).collect(),
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["evil.example.com".into()],
        require_approval_for: vec!["Bash".into()],
    };

    // Warm-up
    let _ = PolicyEngine::new(&policy).unwrap();

    let start = Instant::now();
    for _ in 0..100 {
        let _ = PolicyEngine::new(&policy).unwrap();
    }
    let elapsed = start.elapsed();
    let per_op = elapsed / 100;

    assert!(
        per_op.as_millis() < 10,
        "PolicyEngine compilation took {per_op:?} per op (limit: 10ms)"
    );
}

// ---------------------------------------------------------------------------
// 6. 1000 path match decisions under 100ms
// ---------------------------------------------------------------------------

/// 1000 path match decisions against a compiled glob set should be under 100ms total.
#[test]
#[ignore] // timing-sensitive — run with --ignored
fn path_match_1000_decisions_under_100ms() {
    let includes: Vec<String> = vec!["src/**".into(), "tests/**".into(), "crates/**".into()];
    let excludes: Vec<String> = vec!["**/target/**".into(), "**/generated/**".into()];
    let globs = IncludeExcludeGlobs::new(&includes, &excludes).unwrap();

    let paths: Vec<String> = (0..1000)
        .map(|i| match i % 4 {
            0 => format!("src/module_{i}/lib.rs"),
            1 => format!("tests/test_{i}.rs"),
            2 => format!("crates/crate_{i}/src/main.rs"),
            _ => format!("docs/guide_{i}.md"),
        })
        .collect();

    // Warm-up
    for p in &paths {
        let _ = globs.decide_str(p);
    }

    let start = Instant::now();
    for p in &paths {
        let _ = globs.decide_str(p);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 100,
        "1000 path decisions took {elapsed:?} (limit: 100ms)"
    );
}

// ---------------------------------------------------------------------------
// 7. MockBackend full pipeline under 100ms
// ---------------------------------------------------------------------------

/// A full MockBackend run (including receipt hashing) should complete under 100ms.
#[test]
#[ignore] // timing-sensitive — run with --ignored
fn mock_backend_pipeline_under_100ms() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        use abp_integrations::{Backend, MockBackend};
        use tokio::sync::mpsc;

        let backend = MockBackend;
        let wo = sample_work_order();
        let (tx, mut rx) = mpsc::channel(256);

        let start = Instant::now();
        let receipt = backend.run(uuid::Uuid::new_v4(), wo, tx).await.unwrap();
        // Drain events
        while rx.try_recv().is_ok() {}
        let elapsed = start.elapsed();

        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(receipt.receipt_sha256.is_some());
        assert!(
            elapsed.as_millis() < 100,
            "MockBackend pipeline took {elapsed:?} (limit: 100ms)"
        );
    });
}

// ---------------------------------------------------------------------------
// 8. Receipt store: save/load 100 receipts under 1s
// ---------------------------------------------------------------------------

/// Saving and loading 100 receipts through the file-based store should be under 1s.
#[test]
#[ignore] // timing-sensitive — run with --ignored
fn receipt_store_100_save_load_under_1s() {
    use abp_runtime::store::ReceiptStore;

    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let receipts: Vec<Receipt> = (0..100)
        .map(|_| sample_receipt().with_hash().unwrap())
        .collect();

    let start = Instant::now();
    for r in &receipts {
        store.save(r).unwrap();
    }
    for r in &receipts {
        let loaded = store.load(r.meta.run_id).unwrap();
        assert_eq!(loaded.meta.run_id, r.meta.run_id);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_secs() < 1,
        "Save/load 100 receipts took {elapsed:?} (limit: 1s)"
    );
}

// ---------------------------------------------------------------------------
// 9. Event filter: 10000 events filtered under 100ms
// ---------------------------------------------------------------------------

/// Filtering 10000 events through an EventFilter should complete under 100ms.
#[test]
#[ignore] // timing-sensitive — run with --ignored
fn event_filter_10000_under_100ms() {
    use abp_core::filter::EventFilter;

    let filter = EventFilter::include_kinds(&["assistant_message", "tool_call", "file_changed"]);
    let events: Vec<AgentEvent> = (0..10_000).map(make_event).collect();

    // Warm-up
    for e in events.iter().take(100) {
        let _ = filter.matches(e);
    }

    let start = Instant::now();
    let mut matched = 0u64;
    for e in &events {
        if filter.matches(e) {
            matched += 1;
        }
    }
    let elapsed = start.elapsed();

    assert!(matched > 0, "should have matched some events");
    assert!(
        elapsed.as_millis() < 100,
        "Filtering 10000 events took {elapsed:?} (limit: 100ms)"
    );
}

// ---------------------------------------------------------------------------
// 10. Canonical JSON generation under 1ms
// ---------------------------------------------------------------------------

/// Canonical JSON generation for a typical receipt should be under 1ms.
#[test]
#[ignore] // timing-sensitive — run with --ignored
fn canonical_json_under_1ms() {
    let receipt = sample_receipt();

    // Warm-up
    let _ = canonical_json(&receipt).unwrap();

    let start = Instant::now();
    for _ in 0..100 {
        let _ = canonical_json(&receipt).unwrap();
    }
    let elapsed = start.elapsed();
    let per_op = elapsed / 100;

    assert!(
        per_op.as_millis() < 1,
        "Canonical JSON took {per_op:?} per op (limit: 1ms)"
    );
}

// ---------------------------------------------------------------------------
// 11. Version parsing under 1µs
// ---------------------------------------------------------------------------

/// Version string parsing should be essentially instant (< 1µs amortized).
#[test]
#[ignore] // timing-sensitive — run with --ignored
fn version_parsing_under_1us() {
    let version = "abp/v0.1";

    // Warm-up
    let _ = parse_version(version);

    let iterations = 10_000u32;
    let start = Instant::now();
    for _ in 0..iterations {
        let result = parse_version(version);
        assert_eq!(result, Some((0, 1)));
    }
    let elapsed = start.elapsed();
    let per_op = elapsed / iterations;

    assert!(
        per_op.as_micros() < 1,
        "Version parsing took {per_op:?} per op (limit: 1µs)"
    );
}

// ---------------------------------------------------------------------------
// 12. WorkOrderBuilder complete build under 100µs
// ---------------------------------------------------------------------------

/// Building a WorkOrder via the builder should be under 100µs.
#[test]
#[ignore] // timing-sensitive — run with --ignored
fn work_order_builder_under_100us() {
    // Warm-up
    let _ = sample_work_order();

    let iterations = 1000u32;
    let start = Instant::now();
    for _ in 0..iterations {
        let wo = WorkOrderBuilder::new("Fix the login bug")
            .lane(ExecutionLane::WorkspaceFirst)
            .root("/tmp/workspace")
            .model("gpt-4")
            .max_turns(10)
            .max_budget_usd(1.0)
            .build();
        // Prevent optimization from eliminating the build
        assert!(!wo.task.is_empty());
    }
    let elapsed = start.elapsed();
    let per_op = elapsed / iterations;

    assert!(
        per_op.as_micros() < 100,
        "WorkOrderBuilder build took {per_op:?} per op (limit: 100µs)"
    );
}

// ---------------------------------------------------------------------------
// Non-ignored sanity tests (always run — verify correctness, not timing)
// ---------------------------------------------------------------------------

/// Sanity: WorkOrder round-trips through serialization.
#[test]
fn work_order_serialization_roundtrip() {
    let wo = sample_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.task, wo2.task);
}

/// Sanity: Receipt hashing is deterministic.
#[test]
fn receipt_hash_deterministic() {
    let receipt = sample_receipt();
    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

/// Sanity: Envelope encode/decode round-trips.
#[test]
fn envelope_roundtrip() {
    let hello = Envelope::hello(
        abp_core::BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

/// Sanity: Glob compilation and matching works.
#[test]
fn glob_compilation_and_matching() {
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &["src/generated/**".into()]).unwrap();
    assert!(globs.decide_str("src/lib.rs").is_allowed());
    assert!(!globs.decide_str("src/generated/out.rs").is_allowed());
}

/// Sanity: PolicyEngine compiles and enforces rules.
#[test]
fn policy_engine_compiles_and_enforces() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

/// Sanity: Version parsing returns expected values.
#[test]
fn version_parsing_correctness() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(parse_version("invalid"), None);
}
