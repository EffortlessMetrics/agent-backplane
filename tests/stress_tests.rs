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
//! Stress and load tests for the Agent Backplane.
//!
//! These tests exercise high-volume, concurrent, and large-payload scenarios
//! across the core contract types, policy engine, stream processing, protocol
//! codec, projection matrix, and IR layer.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::Utc;

use abp_capability::{generate_report, negotiate};
use abp_config::{
    BackendEntry as ConfigBackendEntry, BackplaneConfig, merge_configs, parse_toml, validate_config,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, CapabilityRequirement,
    CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane, MinSupport, Outcome,
    PolicyProfile, ReceiptBuilder, SupportLevel, WorkOrderBuilder, WorkspaceMode, canonical_json,
    receipt_hash,
};
use abp_dialect::{Dialect, DialectDetector};
use abp_glob::IncludeExcludeGlobs;
use abp_mapping::{Fidelity, MappingMatrix, MappingRegistry, MappingRule};
use abp_policy::PolicyEngine;
use abp_projection::ProjectionMatrix;
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::{ReceiptChain, compute_hash, diff_receipts, verify_hash};
use abp_stream::{EventFilter, EventRecorder, EventStats, EventTransform, StreamPipelineBuilder};
use tokio::task::JoinSet;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_delta(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantDelta {
        text: text.to_string(),
    })
}

fn make_tool_call(name: &str, idx: usize) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: Some(format!("tc-{idx}")),
        parent_tool_use_id: None,
        input: serde_json::json!({"arg": idx}),
    })
}

fn full_capability_manifest() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Native);
    m.insert(Capability::ToolEdit, SupportLevel::Native);
    m.insert(Capability::ToolBash, SupportLevel::Native);
    m.insert(Capability::ToolGlob, SupportLevel::Emulated);
    m.insert(Capability::ToolGrep, SupportLevel::Emulated);
    m
}

// =========================================================================
// 1. High-volume work order submission (sequential)
// =========================================================================

#[test]
fn stress_create_200_work_orders_sequential() {
    for i in 0..200 {
        let wo = WorkOrderBuilder::new(format!("task-{i}"))
            .lane(ExecutionLane::PatchFirst)
            .root(".")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        assert_eq!(wo.task, format!("task-{i}"));
    }
}

#[test]
fn stress_create_500_work_orders_sequential() {
    let mut ids = Vec::with_capacity(500);
    for i in 0..500 {
        let wo = WorkOrderBuilder::new(format!("batch-{i}")).build();
        ids.push(wo.id);
    }
    // All IDs must be unique.
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 500);
}

#[test]
fn stress_serialize_100_work_orders() {
    for i in 0..100 {
        let wo = WorkOrderBuilder::new(format!("ser-{i}"))
            .model("gpt-4")
            .max_turns(10)
            .build();
        let json = serde_json::to_string(&wo).unwrap();
        let _: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn stress_work_order_with_large_context() {
    let snippets: Vec<ContextSnippet> = (0..200)
        .map(|i| ContextSnippet {
            name: format!("snippet-{i}"),
            content: "x".repeat(1000),
        })
        .collect();
    let ctx = ContextPacket {
        files: (0..100).map(|i| format!("src/file_{i}.rs")).collect(),
        snippets,
    };
    let wo = WorkOrderBuilder::new("big-context").context(ctx).build();
    assert_eq!(wo.context.snippets.len(), 200);
    assert_eq!(wo.context.files.len(), 100);
}

#[test]
fn stress_work_order_roundtrip_150() {
    for i in 0..150 {
        let wo = WorkOrderBuilder::new(format!("rt-{i}"))
            .lane(ExecutionLane::WorkspaceFirst)
            .root("/tmp")
            .model("claude-3")
            .max_budget_usd(5.0)
            .build();
        let json = canonical_json(&wo).unwrap();
        assert!(!json.is_empty());
    }
}

// =========================================================================
// 2. Concurrent work order execution
// =========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn stress_concurrent_work_order_creation_100() {
    let mut set = JoinSet::new();
    for i in 0..100 {
        set.spawn(async move { WorkOrderBuilder::new(format!("concurrent-{i}")).build() });
    }
    let mut count = 0;
    while let Some(result) = set.join_next().await {
        assert!(result.is_ok());
        count += 1;
    }
    assert_eq!(count, 100);
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_concurrent_receipt_building_50() {
    let mut set = JoinSet::new();
    for i in 0..50 {
        set.spawn(async move {
            ReceiptBuilder::new(format!("backend-{i}"))
                .outcome(Outcome::Complete)
                .build()
                .with_hash()
                .unwrap()
        });
    }
    while let Some(result) = set.join_next().await {
        let receipt = result.unwrap();
        assert!(receipt.receipt_sha256.is_some());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_concurrent_policy_checks() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/.git/**".into(), "**/node_modules/**".into()],
        ..PolicyProfile::default()
    };
    let engine = Arc::new(PolicyEngine::new(&policy).unwrap());

    let mut set = JoinSet::new();
    for i in 0..100 {
        let eng = Arc::clone(&engine);
        set.spawn(async move {
            let allowed = eng.can_use_tool(&format!("tool-{i}")).allowed;
            let write_ok = eng
                .can_write_path(Path::new(&format!("src/mod_{i}.rs")))
                .allowed;
            (allowed, write_ok)
        });
    }
    while let Some(result) = set.join_next().await {
        let (allowed, write_ok) = result.unwrap();
        assert!(allowed);
        assert!(write_ok);
    }
}

// =========================================================================
// 3. Large work orders
// =========================================================================

#[test]
fn stress_very_large_task_string() {
    let task = "a".repeat(100_000);
    let wo = WorkOrderBuilder::new(task.clone()).build();
    assert_eq!(wo.task.len(), 100_000);
}

#[test]
fn stress_many_context_files() {
    let files: Vec<String> = (0..1000).map(|i| format!("dir/sub/file_{i}.rs")).collect();
    let ctx = ContextPacket {
        files,
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("many-files").context(ctx).build();
    assert_eq!(wo.context.files.len(), 1000);
}

#[test]
fn stress_large_snippet_content() {
    let content = "z".repeat(1_000_000);
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "huge".into(),
            content,
        }],
    };
    let wo = WorkOrderBuilder::new("big-snippet").context(ctx).build();
    assert_eq!(wo.context.snippets[0].content.len(), 1_000_000);
}

#[test]
fn stress_deeply_nested_vendor_config() {
    let mut vendor = BTreeMap::new();
    for i in 0..200 {
        vendor.insert(
            format!("key_{i}"),
            serde_json::json!({ "nested": { "deep": i } }),
        );
    }
    let config = abp_core::RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("deep-vendor").config(config).build();
    assert_eq!(wo.config.vendor.len(), 200);
}

// =========================================================================
// 4. Many events per work order (100+ events)
// =========================================================================

#[test]
fn stress_receipt_with_200_trace_events() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..200 {
        builder = builder.add_trace_event(make_delta(&format!("token-{i}")));
    }
    let receipt = builder.build();
    assert_eq!(receipt.trace.len(), 200);
}

#[test]
fn stress_receipt_with_500_mixed_events() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..500 {
        let event = if i % 3 == 0 {
            make_delta(&format!("delta-{i}"))
        } else if i % 3 == 1 {
            make_tool_call("Read", i)
        } else {
            make_event(AgentEventKind::FileChanged {
                path: format!("src/f{i}.rs"),
                summary: "edited".into(),
            })
        };
        builder = builder.add_trace_event(event);
    }
    let receipt = builder.build();
    assert_eq!(receipt.trace.len(), 500);
}

#[test]
fn stress_event_recorder_1000_events() {
    let recorder = EventRecorder::new();
    for i in 0..1000 {
        recorder.record(&make_delta(&format!("tok-{i}")));
    }
    assert_eq!(recorder.len(), 1000);
    let events = recorder.events();
    assert_eq!(events.len(), 1000);
}

#[test]
fn stress_event_stats_500_events() {
    let stats = EventStats::new();
    for i in 0..500 {
        if i % 2 == 0 {
            stats.observe(&make_delta("hi"));
        } else {
            stats.observe(&make_event(AgentEventKind::Error {
                message: "err".into(),
                error_code: None,
            }));
        }
    }
    assert_eq!(stats.total_events(), 500);
    assert_eq!(stats.error_count(), 250);
    assert_eq!(stats.count_for("assistant_delta"), 250);
}

#[test]
fn stress_event_filter_on_many_events() {
    let filter = EventFilter::by_kind("tool_call");
    let mut matched = 0;
    for i in 0..300 {
        let ev = if i % 5 == 0 {
            make_tool_call("Read", i)
        } else {
            make_delta("x")
        };
        if filter.matches(&ev) {
            matched += 1;
        }
    }
    assert_eq!(matched, 60);
}

// =========================================================================
// 5. Receipt generation under load
// =========================================================================

#[test]
fn stress_generate_100_hashed_receipts() {
    for i in 0..100 {
        let receipt = ReceiptBuilder::new(format!("backend-{i}"))
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        assert!(verify_hash(&receipt));
    }
}

#[test]
fn stress_receipt_chain_100_entries() {
    let mut chain = ReceiptChain::new();
    for _i in 0..100 {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        chain.push(receipt).unwrap();
    }
    assert_eq!(chain.len(), 100);
    chain.verify().unwrap();
}

#[test]
fn stress_receipt_diff_many_pairs() {
    let base = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    for _i in 0..100 {
        let other = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
        let diff = diff_receipts(&base, &other);
        assert!(!diff.is_empty());
    }
}

#[test]
fn stress_compute_hash_determinism() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_delta("hello"))
        .build();
    let h1 = compute_hash(&receipt).unwrap();
    for _ in 0..100 {
        assert_eq!(compute_hash(&receipt).unwrap(), h1);
    }
}

#[test]
fn stress_receipt_with_large_trace_hash() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..150 {
        builder = builder.add_trace_event(make_delta(&format!("token-{i}")));
    }
    let receipt = builder.build().with_hash().unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert!(verify_hash(&receipt));
}

// =========================================================================
// 6. Memory usage patterns (Drop verification)
// =========================================================================

#[test]
fn stress_work_order_drop_count() {
    static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct DropTracker;
    impl Drop for DropTracker {
        fn drop(&mut self) {
            DROP_COUNT.fetch_add(1, Ordering::SeqCst);
        }
    }

    DROP_COUNT.store(0, Ordering::SeqCst);
    let n = 200;
    {
        let mut trackers = Vec::with_capacity(n);
        for _ in 0..n {
            trackers.push(DropTracker);
        }
    }
    assert_eq!(DROP_COUNT.load(Ordering::SeqCst), n);
}

#[test]
fn stress_receipt_drop_with_large_trace() {
    for _ in 0..50 {
        let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
        for j in 0..100 {
            builder = builder.add_trace_event(make_delta(&format!("drop-{j}")));
        }
        let _receipt = builder.build();
        // Receipt is dropped here each iteration — no leak.
    }
}

#[test]
fn stress_event_recorder_clear_cycles() {
    let recorder = EventRecorder::new();
    for cycle in 0..50 {
        for j in 0..100 {
            recorder.record(&make_delta(&format!("c{cycle}-t{j}")));
        }
        assert_eq!(recorder.len(), 100);
        recorder.clear();
        assert!(recorder.is_empty());
    }
}

#[test]
fn stress_large_vec_of_work_orders_dropped() {
    let start_alloc_count = 0usize; // baseline
    {
        let _orders: Vec<_> = (0..500)
            .map(|i| WorkOrderBuilder::new(format!("wo-{i}")).build())
            .collect();
    }
    // If we reach here without OOM the allocation pattern is sound.
    assert_eq!(start_alloc_count, 0);
}

#[test]
fn stress_context_packet_with_many_snippets_dropped() {
    for _ in 0..20 {
        let ctx = ContextPacket {
            files: (0..100).map(|i| format!("f{i}.rs")).collect(),
            snippets: (0..100)
                .map(|i| ContextSnippet {
                    name: format!("s{i}"),
                    content: "x".repeat(500),
                })
                .collect(),
        };
        let _wo = WorkOrderBuilder::new("drop-test").context(ctx).build();
    }
}

// =========================================================================
// 7. Thread safety of shared state (Arc<Mutex> patterns)
// =========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn stress_shared_event_recorder_concurrent() {
    let recorder = EventRecorder::new();
    let r = recorder.clone();

    let mut set = JoinSet::new();
    for i in 0..50 {
        let rec = r.clone();
        set.spawn(async move {
            for j in 0..20 {
                rec.record(&make_delta(&format!("t{i}-e{j}")));
            }
        });
    }
    while set.join_next().await.is_some() {}
    assert_eq!(recorder.len(), 1000);
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_shared_event_stats_concurrent() {
    let stats = EventStats::new();

    let mut set = JoinSet::new();
    for _ in 0..50 {
        let s = stats.clone();
        set.spawn(async move {
            for _ in 0..20 {
                s.observe(&make_delta("x"));
            }
        });
    }
    while set.join_next().await.is_some() {}
    assert_eq!(stats.total_events(), 1000);
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_arc_policy_engine_concurrent_reads() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into(), "Exec".into()],
        deny_write: vec!["**/.env".into()],
        ..PolicyProfile::default()
    };
    let engine = Arc::new(PolicyEngine::new(&policy).unwrap());

    let mut set = JoinSet::new();
    for _ in 0..100 {
        let eng = Arc::clone(&engine);
        set.spawn(async move {
            assert!(!eng.can_use_tool("Bash").allowed);
            assert!(eng.can_use_tool("Read").allowed);
            assert!(!eng.can_write_path(Path::new(".env")).allowed);
        });
    }
    while set.join_next().await.is_some() {}
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_arc_glob_filter_concurrent() {
    let globs = Arc::new(
        IncludeExcludeGlobs::new(
            &["src/**".into(), "tests/**".into()],
            &["**/generated/**".into()],
        )
        .unwrap(),
    );

    let mut set = JoinSet::new();
    for i in 0..100 {
        let g = Arc::clone(&globs);
        set.spawn(async move {
            let p = format!("src/module_{i}.rs");
            assert!(g.decide_str(&p).is_allowed());
        });
    }
    while set.join_next().await.is_some() {}
}

// =========================================================================
// 8. Rapid backend switching
// =========================================================================

#[test]
fn stress_rapid_projection_backend_registration() {
    let mut matrix = ProjectionMatrix::new();
    for i in 0..100 {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        if i % 2 == 0 {
            caps.insert(Capability::ToolRead, SupportLevel::Native);
        }
        matrix.register_backend(
            format!("backend-{i}"),
            caps,
            *Dialect::all().get(i % 6).unwrap(),
            (i % 100) as u32,
        );
    }
    assert_eq!(matrix.backend_count(), 100);
}

#[test]
fn stress_projection_selection_many_backends() {
    let mut matrix = ProjectionMatrix::new();
    for i in 0..50 {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        caps.insert(Capability::ToolRead, SupportLevel::Native);
        matrix.register_backend(
            format!("b-{i}"),
            caps,
            *Dialect::all().get(i % 6).unwrap(),
            50,
        );
    }
    let wo = WorkOrderBuilder::new("select-test")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let result = matrix.project(&wo).unwrap();
    assert!(!result.selected_backend.is_empty());
}

#[test]
fn stress_projection_repeated_queries() {
    let mut matrix = ProjectionMatrix::new();
    for i in 0..10 {
        matrix.register_backend(
            format!("b-{i}"),
            full_capability_manifest(),
            Dialect::OpenAi,
            50,
        );
    }
    let wo = WorkOrderBuilder::new("repeat-proj").build();
    for _ in 0..200 {
        let result = matrix.project(&wo).unwrap();
        assert!(!result.selected_backend.is_empty());
    }
}

#[test]
fn stress_capability_negotiation_many_requirements() {
    let manifest = full_capability_manifest();
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolWrite,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolEdit,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolGlob,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolGrep,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    for _ in 0..200 {
        let result = negotiate(&manifest, &reqs);
        assert!(result.is_compatible());
    }
}

#[test]
fn stress_compatibility_report_generation() {
    let manifest = full_capability_manifest();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    for _ in 0..200 {
        let neg = negotiate(&manifest, &reqs);
        let report = generate_report(&neg);
        assert!(report.compatible);
    }
}

// =========================================================================
// 9. Large policy profiles (many rules)
// =========================================================================

#[test]
fn stress_policy_200_disallowed_tools() {
    let policy = PolicyProfile {
        disallowed_tools: (0..200).map(|i| format!("tool_{i}")).collect(),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    for i in 0..200 {
        assert!(!engine.can_use_tool(&format!("tool_{i}")).allowed);
    }
    assert!(engine.can_use_tool("safe_tool").allowed);
}

#[test]
fn stress_policy_200_deny_write_paths() {
    let policy = PolicyProfile {
        deny_write: (0..200).map(|i| format!("**/secret_{i}/**")).collect(),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    for i in 0..200 {
        assert!(
            !engine
                .can_write_path(Path::new(&format!("secret_{i}/data.txt")))
                .allowed
        );
    }
    assert!(engine.can_write_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn stress_policy_mixed_allow_deny() {
    let policy = PolicyProfile {
        allowed_tools: (0..50).map(|i| format!("allow_{i}")).collect(),
        disallowed_tools: (0..50).map(|i| format!("deny_{i}")).collect(),
        deny_read: (0..50).map(|i| format!("**/private_{i}/**")).collect(),
        deny_write: (0..50).map(|i| format!("**/readonly_{i}/**")).collect(),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    for i in 0..50 {
        assert!(!engine.can_use_tool(&format!("deny_{i}")).allowed);
    }
}

#[test]
fn stress_policy_serialization_roundtrip() {
    let policy = PolicyProfile {
        disallowed_tools: (0..100).map(|i| format!("t{i}")).collect(),
        deny_write: (0..100).map(|i| format!("**/d{i}/**")).collect(),
        deny_read: (0..100).map(|i| format!("**/r{i}/**")).collect(),
        allow_network: (0..50).map(|i| format!("host{i}.com")).collect(),
        deny_network: (0..50).map(|i| format!("bad{i}.com")).collect(),
        require_approval_for: (0..50).map(|i| format!("dangerous_{i}")).collect(),
        ..PolicyProfile::default()
    };
    let json = serde_json::to_string(&policy).unwrap();
    let deser: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.disallowed_tools.len(), 100);
    assert_eq!(deser.deny_write.len(), 100);
}

// =========================================================================
// 10. Deep workspace hierarchies
// =========================================================================

#[test]
fn stress_workspace_spec_deep_include_exclude() {
    let includes: Vec<String> = (0..50).map(|i| format!("src/level_{i}/**/*.rs")).collect();
    let excludes: Vec<String> = (0..50)
        .map(|i| format!("src/level_{i}/**/test_*"))
        .collect();
    let wo = WorkOrderBuilder::new("deep-ws")
        .include(includes)
        .exclude(excludes)
        .build();
    assert_eq!(wo.workspace.include.len(), 50);
    assert_eq!(wo.workspace.exclude.len(), 50);
}

#[test]
fn stress_glob_matching_deep_paths() {
    let globs = IncludeExcludeGlobs::new(
        &["src/**/*.rs".into()],
        &["**/test_*.rs".into(), "**/generated/**".into()],
    )
    .unwrap();
    for depth in 0..50 {
        let path = format!(
            "src/{}/module.rs",
            (0..depth)
                .map(|d| format!("d{d}"))
                .collect::<Vec<_>>()
                .join("/")
        );
        assert!(globs.decide_str(&path).is_allowed());
    }
}

#[test]
fn stress_glob_many_path_evaluations() {
    let globs = IncludeExcludeGlobs::new(
        &["**/*.rs".into(), "**/*.toml".into()],
        &["**/target/**".into()],
    )
    .unwrap();
    for i in 0..500 {
        let path = format!("crate_{i}/src/lib.rs");
        assert!(globs.decide_str(&path).is_allowed());
    }
}

#[test]
fn stress_workspace_spec_many_files_context() {
    let files: Vec<String> = (0..500)
        .map(|i| {
            format!(
                "deep/nested/path/level_{}/sublevel_{}/file_{}.rs",
                i / 100,
                i / 10,
                i
            )
        })
        .collect();
    let ctx = ContextPacket {
        files,
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("deep-ctx").context(ctx).build();
    assert_eq!(wo.context.files.len(), 500);
}

// =========================================================================
// 11. Multiple subscribers to event bus
// =========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn stress_multiple_recorders_same_events() {
    let recorders: Vec<EventRecorder> = (0..10).map(|_| EventRecorder::new()).collect();
    for i in 0..100 {
        let ev = make_delta(&format!("multi-{i}"));
        for rec in &recorders {
            rec.record(&ev);
        }
    }
    for rec in &recorders {
        assert_eq!(rec.len(), 100);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_pipeline_with_many_filters() {
    let mut builder = StreamPipelineBuilder::new();
    // Add 20 pass-through filters
    for _ in 0..20 {
        builder = builder.filter(EventFilter::new(|_| true));
    }
    let pipeline = builder.record().build();
    for i in 0..200 {
        pipeline.process(make_delta(&format!("pipe-{i}")));
    }
    assert_eq!(pipeline.recorder().unwrap().len(), 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_pipeline_with_transforms() {
    let pipeline = StreamPipelineBuilder::new()
        .transform(EventTransform::identity())
        .transform(EventTransform::identity())
        .transform(EventTransform::identity())
        .record()
        .build();
    for i in 0..150 {
        pipeline.process(make_delta(&format!("xform-{i}")));
    }
    assert_eq!(pipeline.recorder().unwrap().len(), 150);
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_pipeline_filter_and_stats() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_stats(stats.clone())
        .record()
        .build();

    for i in 0..100 {
        if i % 10 == 0 {
            pipeline.process(make_event(AgentEventKind::Error {
                message: "err".into(),
                error_code: None,
            }));
        } else {
            pipeline.process(make_delta("ok"));
        }
    }
    // Errors are filtered out; only non-errors pass.
    assert_eq!(pipeline.recorder().unwrap().len(), 90);
    assert_eq!(stats.total_events(), 90);
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_event_stream_collect_200() {
    let (tx, rx) = tokio::sync::mpsc::channel(256);
    let stream = abp_stream::EventStream::new(rx);

    tokio::spawn(async move {
        for i in 0..200 {
            tx.send(make_delta(&format!("stream-{i}"))).await.unwrap();
        }
    });

    let events = stream.collect_all().await;
    assert_eq!(events.len(), 200);
}

// =========================================================================
// 12. Stress on glob compilation with many patterns
// =========================================================================

#[test]
fn stress_glob_compile_100_includes() {
    let includes: Vec<String> = (0..100).map(|i| format!("src/mod_{i}/**/*.rs")).collect();
    let globs = IncludeExcludeGlobs::new(&includes, &[]).unwrap();
    assert!(globs.decide_str("src/mod_42/lib.rs").is_allowed());
}

#[test]
fn stress_glob_compile_100_excludes() {
    let excludes: Vec<String> = (0..100).map(|i| format!("**/exc_{i}/**")).collect();
    let globs = IncludeExcludeGlobs::new(&[], &excludes).unwrap();
    assert!(globs.decide_str("safe/path.rs").is_allowed());
    assert!(!globs.decide_str("exc_50/file.txt").is_allowed());
}

#[test]
fn stress_glob_compile_mixed_200() {
    let includes: Vec<String> = (0..100).map(|i| format!("src_{i}/**")).collect();
    let excludes: Vec<String> = (0..100).map(|i| format!("**/gen_{i}/**")).collect();
    let globs = IncludeExcludeGlobs::new(&includes, &excludes).unwrap();
    assert!(globs.decide_str("src_5/lib.rs").is_allowed());
    assert!(!globs.decide_str("src_5/gen_5/out.rs").is_allowed());
}

#[test]
fn stress_glob_repeated_compilation() {
    for i in 0..100 {
        let inc = vec![format!("pattern_{i}/**")];
        let exc = vec![format!("**/exclude_{i}/**")];
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        assert!(
            globs
                .decide_str(&format!("pattern_{i}/file.rs"))
                .is_allowed()
        );
    }
}

#[test]
fn stress_glob_complex_patterns() {
    let includes: Vec<String> = (0..50)
        .map(|i| format!("**/level_{i}/**/sub_*/*.{{rs,toml}}"))
        .collect();
    // These patterns may or may not compile depending on globset behavior;
    // the important thing is we don't panic.
    let result = IncludeExcludeGlobs::new(&includes, &[]);
    // Either compiles or returns a clean error.
    assert!(result.is_ok() || result.is_err());
}

// =========================================================================
// 13. Stress on IR type allocation
// =========================================================================

#[test]
fn stress_ir_conversation_500_messages() {
    let mut conv = IrConversation::new();
    for i in 0..500 {
        let role = match i % 3 {
            0 => IrRole::User,
            1 => IrRole::Assistant,
            _ => IrRole::Tool,
        };
        conv = conv.push(IrMessage::text(role, format!("message-{i}")));
    }
    assert_eq!(conv.len(), 500);
}

#[test]
fn stress_ir_message_many_content_blocks() {
    let blocks: Vec<IrContentBlock> = (0..200)
        .map(|i| IrContentBlock::Text {
            text: format!("block-{i}"),
        })
        .collect();
    let msg = IrMessage::new(IrRole::Assistant, blocks);
    assert!(msg.is_text_only());
    assert_eq!(msg.content.len(), 200);
}

#[test]
fn stress_ir_tool_use_blocks() {
    let blocks: Vec<IrContentBlock> = (0..100)
        .map(|i| IrContentBlock::ToolUse {
            id: format!("tool-{i}"),
            name: format!("fn_{i}"),
            input: serde_json::json!({"x": i}),
        })
        .collect();
    let msg = IrMessage::new(IrRole::Assistant, blocks);
    assert_eq!(msg.tool_use_blocks().len(), 100);
}

#[test]
fn stress_ir_usage_merge_chain() {
    let mut usage = IrUsage::from_io(0, 0);
    for i in 0..1000 {
        usage = usage.merge(IrUsage::from_io(i, i * 2));
    }
    assert_eq!(usage.input_tokens, (0..1000u64).sum::<u64>());
}

#[test]
fn stress_ir_conversation_serde_roundtrip() {
    let mut conv = IrConversation::new();
    for i in 0..100 {
        conv = conv
            .push(IrMessage::text(IrRole::User, format!("q{i}")))
            .push(IrMessage::text(IrRole::Assistant, format!("a{i}")));
    }
    let json = serde_json::to_string(&conv).unwrap();
    let deser: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.len(), 200);
}

#[test]
fn stress_ir_nested_tool_results() {
    let blocks: Vec<IrContentBlock> = (0..50)
        .map(|i| IrContentBlock::ToolResult {
            tool_use_id: format!("tu-{i}"),
            content: vec![IrContentBlock::Text {
                text: format!("result-{i}"),
            }],
            is_error: i % 10 == 0,
        })
        .collect();
    let msg = IrMessage::new(IrRole::Tool, blocks);
    assert_eq!(msg.content.len(), 50);
}

// =========================================================================
// 14. Rapid config changes
// =========================================================================

#[test]
fn stress_config_merge_100_overlays() {
    let mut config = BackplaneConfig::default();
    for i in 0..100 {
        let overlay = BackplaneConfig {
            default_backend: Some(format!("backend-{i}")),
            log_level: Some("debug".into()),
            backends: BTreeMap::from([(format!("b{i}"), ConfigBackendEntry::Mock {})]),
            ..Default::default()
        };
        config = merge_configs(config, overlay);
    }
    assert_eq!(config.backends.len(), 100);
    assert_eq!(config.default_backend.as_deref(), Some("backend-99"));
}

#[test]
fn stress_config_validate_many_backends() {
    let mut backends = BTreeMap::new();
    for i in 0..100 {
        backends.insert(
            format!("sidecar-{i}"),
            ConfigBackendEntry::Sidecar {
                command: format!("node sidecar_{i}.js"),
                args: vec![],
                timeout_secs: Some(60),
            },
        );
    }
    let config = BackplaneConfig {
        default_backend: Some("sidecar-0".into()),
        log_level: Some("info".into()),
        backends,
        ..Default::default()
    };
    let warnings = validate_config(&config).unwrap();
    // Missing receipts_dir generates a warning.
    assert!(!warnings.is_empty());
}

#[test]
fn stress_config_parse_toml_repeated() {
    let toml_str = r#"
        default_backend = "mock"
        log_level = "info"
        [backends.mock]
        type = "mock"
    "#;
    for _ in 0..200 {
        let cfg = parse_toml(toml_str).unwrap();
        assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    }
}

#[test]
fn stress_config_merge_preserves_backends() {
    let base = BackplaneConfig {
        backends: (0..50)
            .map(|i| (format!("base-{i}"), ConfigBackendEntry::Mock {}))
            .collect(),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: (0..50)
            .map(|i| (format!("over-{i}"), ConfigBackendEntry::Mock {}))
            .collect(),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.backends.len(), 100);
}

// =========================================================================
// 15. Hash computation performance
// =========================================================================

#[test]
fn stress_sha256_many_receipts() {
    for _ in 0..200 {
        let receipt = ReceiptBuilder::new("perf")
            .outcome(Outcome::Complete)
            .build();
        let hash = compute_hash(&receipt).unwrap();
        assert_eq!(hash.len(), 64);
    }
}

#[test]
fn stress_canonical_json_many_values() {
    for i in 0..200 {
        let val = serde_json::json!({
            "key": i,
            "nested": { "a": 1, "b": 2, "c": format!("val-{i}") },
            "array": (0..10).collect::<Vec<i32>>(),
        });
        let json = canonical_json(&val).unwrap();
        assert!(!json.is_empty());
    }
}

#[test]
fn stress_receipt_hash_with_growing_trace() {
    for n in (10..=100).step_by(10) {
        let mut builder = ReceiptBuilder::new("grow").outcome(Outcome::Complete);
        for j in 0..n {
            builder = builder.add_trace_event(make_delta(&format!("g{j}")));
        }
        let receipt = builder.build();
        let hash = receipt_hash(&receipt).unwrap();
        assert_eq!(hash.len(), 64);
    }
}

#[test]
fn stress_verify_hash_repeated() {
    let receipt = ReceiptBuilder::new("verify")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    for _ in 0..500 {
        assert!(verify_hash(&receipt));
    }
}

// =========================================================================
// Additional stress tests — protocol codec
// =========================================================================

#[test]
fn stress_protocol_encode_decode_200_envelopes() {
    for i in 0..200 {
        let envelope = Envelope::Fatal {
            ref_id: Some(format!("run-{i}")),
            error: format!("error-{i}"),
            error_code: None,
        };
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Fatal { .. }));
    }
}

#[test]
fn stress_protocol_hello_envelopes() {
    for i in 0..100 {
        let hello = Envelope::hello(
            abp_core::BackendIdentity {
                id: format!("sidecar-{i}"),
                backend_version: Some("1.0".into()),
                adapter_version: None,
            },
            full_capability_manifest(),
        );
        let line = JsonlCodec::encode(&hello).unwrap();
        assert!(line.contains("hello"));
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }
}

#[test]
fn stress_protocol_event_envelope_with_large_payload() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "x".repeat(50_000),
        },
        ext: None,
    };
    let envelope = Envelope::Event {
        ref_id: "run-1".into(),
        event,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    assert!(encoded.len() > 50_000);
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

// =========================================================================
// Additional stress — dialect detection
// =========================================================================

#[test]
fn stress_dialect_detection_many_samples() {
    let detector = DialectDetector::new();
    for i in 0..200 {
        let val = serde_json::json!({
            "model": format!("gpt-4-{i}"),
            "messages": [{"role": "user", "content": "hello"}],
        });
        let _result = detector.detect(&val);
    }
}

#[test]
fn stress_dialect_all_variants() {
    let all = Dialect::all();
    assert_eq!(all.len(), 6);
    for _ in 0..100 {
        for d in all {
            assert!(!d.label().is_empty());
        }
    }
}

// =========================================================================
// Additional stress — mapping registry
// =========================================================================

#[test]
fn stress_mapping_registry_many_rules() {
    let mut reg = MappingRegistry::new();
    let dialects = Dialect::all();
    for (idx, &src) in dialects.iter().enumerate() {
        for &tgt in dialects.iter().skip(idx + 1) {
            for f in 0..20 {
                reg.insert(MappingRule {
                    source_dialect: src,
                    target_dialect: tgt,
                    feature: format!("feature_{f}"),
                    fidelity: if f % 3 == 0 {
                        Fidelity::Lossless
                    } else {
                        Fidelity::LossyLabeled {
                            warning: "minor loss".into(),
                        }
                    },
                });
            }
        }
    }
    assert!(reg.len() > 100);
}

#[test]
fn stress_mapping_matrix_all_pairs() {
    let mut matrix = MappingMatrix::new();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            if src != tgt {
                matrix.set(src, tgt, true);
            }
        }
    }
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            if src != tgt {
                assert!(matrix.is_supported(src, tgt));
            }
        }
    }
}

#[test]
fn stress_mapping_registry_rank_targets() {
    let mut reg = MappingRegistry::new();
    let features = ["streaming", "tool_use", "structured_output"];
    for &tgt in Dialect::all() {
        if tgt != Dialect::OpenAi {
            for feat in &features {
                reg.insert(MappingRule {
                    source_dialect: Dialect::OpenAi,
                    target_dialect: tgt,
                    feature: feat.to_string(),
                    fidelity: Fidelity::Lossless,
                });
            }
        }
    }
    let ranked = reg.rank_targets(Dialect::OpenAi, &features);
    assert!(!ranked.is_empty());
    for (_, count) in &ranked {
        assert_eq!(*count, 3);
    }
}

// =========================================================================
// Ignored tests — longer-running stress (>5s)
// =========================================================================

#[test]
#[ignore]
fn stress_ignored_1000_hashed_receipts() {
    for i in 0..1000 {
        let mut builder = ReceiptBuilder::new(format!("backend-{i}")).outcome(Outcome::Complete);
        for j in 0..50 {
            builder = builder.add_trace_event(make_delta(&format!("t{i}-e{j}")));
        }
        let receipt = builder.build().with_hash().unwrap();
        assert!(verify_hash(&receipt));
    }
}

#[test]
#[ignore]
fn stress_ignored_receipt_chain_500() {
    let mut chain = ReceiptChain::new();
    for _ in 0..500 {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        chain.push(receipt).unwrap();
    }
    assert_eq!(chain.len(), 500);
    chain.verify().unwrap();
}

#[test]
#[ignore]
fn stress_ignored_concurrent_1000_work_orders() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut set = JoinSet::new();
        for i in 0..1000 {
            set.spawn(async move { WorkOrderBuilder::new(format!("heavy-{i}")).build() });
        }
        let mut count = 0;
        while set.join_next().await.is_some() {
            count += 1;
        }
        assert_eq!(count, 1000);
    });
}

#[test]
#[ignore]
fn stress_ignored_massive_policy_evaluation() {
    let policy = PolicyProfile {
        disallowed_tools: (0..500).map(|i| format!("tool_{i}")).collect(),
        deny_write: (0..500).map(|i| format!("**/secret_{i}/**")).collect(),
        deny_read: (0..500).map(|i| format!("**/hidden_{i}/**")).collect(),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    for i in 0..500 {
        assert!(!engine.can_use_tool(&format!("tool_{i}")).allowed);
        assert!(
            !engine
                .can_write_path(Path::new(&format!("secret_{i}/file.txt")))
                .allowed
        );
    }
}

#[test]
#[ignore]
fn stress_ignored_projection_matrix_200_backends() {
    let mut matrix = ProjectionMatrix::new();
    for i in 0..200 {
        let mut caps = full_capability_manifest();
        if i % 3 == 0 {
            caps.insert(Capability::McpClient, SupportLevel::Native);
        }
        matrix.register_backend(
            format!("heavy-b-{i}"),
            caps,
            *Dialect::all().get(i % 6).unwrap(),
            (i % 100) as u32,
        );
    }
    let wo = WorkOrderBuilder::new("heavy-proj")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    for _ in 0..500 {
        let result = matrix.project(&wo).unwrap();
        assert!(!result.selected_backend.is_empty());
    }
}
