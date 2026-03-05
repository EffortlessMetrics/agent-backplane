#![allow(clippy::all)]
#![allow(dead_code)]
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
//! Comprehensive thread safety and concurrency correctness tests for ABP.

use std::sync::Arc;

use abp_core::{
    filter::EventFilter, receipt_hash, AgentEvent, AgentEventKind, Outcome, Receipt, WorkOrder,
    WorkOrderBuilder, WorkspaceMode,
};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_integrations::{Backend, MockBackend};
use abp_policy::PolicyEngine;
use abp_projection::ProjectionMatrix;
use abp_receipt::{ReceiptBuilder, ReceiptChain};
use abp_runtime::{BackendRegistry, Runtime};
use abp_stream::{
    EventFilter as StreamEventFilter, EventRecorder, EventStats, EventTransform, StreamPipeline,
    StreamPipelineBuilder,
};
use chrono::Utc;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .root(".")
        .build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_receipt(backend_id: &str) -> Receipt {
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .build()
}

fn make_hashed_receipt(backend_id: &str) -> Receipt {
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap()
}

// ===========================================================================
// 1. Send + Sync static assertions
// ===========================================================================

fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}
fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn work_order_is_send() {
    assert_send::<WorkOrder>();
}

#[test]
fn work_order_is_sync() {
    assert_sync::<WorkOrder>();
}

#[test]
fn receipt_is_send() {
    assert_send::<Receipt>();
}

#[test]
fn receipt_is_sync() {
    assert_sync::<Receipt>();
}

#[test]
fn agent_event_is_send() {
    assert_send::<AgentEvent>();
}

#[test]
fn agent_event_is_sync() {
    assert_sync::<AgentEvent>();
}

#[test]
fn mock_backend_is_send_sync() {
    assert_send_sync::<MockBackend>();
}

#[test]
fn policy_engine_is_send_sync() {
    assert_send_sync::<PolicyEngine>();
}

#[test]
fn include_exclude_globs_is_send_sync() {
    assert_send_sync::<IncludeExcludeGlobs>();
}

#[test]
fn event_filter_is_send_sync() {
    assert_send_sync::<EventFilter>();
}

#[test]
fn projection_matrix_is_send_sync() {
    assert_send_sync::<ProjectionMatrix>();
}

#[test]
fn receipt_chain_is_send_sync() {
    assert_send_sync::<ReceiptChain>();
}

#[test]
fn backend_registry_is_send() {
    assert_send::<BackendRegistry>();
}

#[test]
fn backend_registry_is_sync() {
    assert_sync::<BackendRegistry>();
}

#[test]
fn runtime_is_send() {
    assert_send::<Runtime>();
}

#[test]
fn stream_pipeline_is_send_sync() {
    assert_send_sync::<StreamPipeline>();
}

#[test]
fn event_recorder_is_send_sync() {
    assert_send_sync::<EventRecorder>();
}

#[test]
fn event_stats_is_send_sync() {
    assert_send_sync::<EventStats>();
}

#[test]
fn stream_event_filter_is_send_sync() {
    assert_send_sync::<StreamEventFilter>();
}

#[test]
fn event_transform_is_send_sync() {
    assert_send_sync::<EventTransform>();
}

#[test]
fn outcome_is_send_sync() {
    assert_send_sync::<Outcome>();
}

#[test]
fn dyn_backend_is_send_sync() {
    // Backend trait requires Send + Sync
    fn accepts_dyn(_b: &(dyn Backend + Send + Sync)) {}
    let mock = MockBackend;
    accepts_dyn(&mock);
}

// ===========================================================================
// 2. Runtime shared across threads
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn runtime_arc_shared_backend_names() {
    let rt = Arc::new(Runtime::with_default_backends());
    let mut set = JoinSet::new();
    for _ in 0..10 {
        let rt = Arc::clone(&rt);
        set.spawn(async move {
            let names = rt.backend_names();
            assert!(names.contains(&"mock".to_string()));
        });
    }
    while let Some(res) = set.join_next().await {
        res.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_arc_shared_backend_lookup() {
    let rt = Arc::new(Runtime::with_default_backends());
    let mut set = JoinSet::new();
    for _ in 0..20 {
        let rt = Arc::clone(&rt);
        set.spawn(async move {
            let backend = rt.backend("mock");
            assert!(backend.is_some());
        });
    }
    while let Some(res) = set.join_next().await {
        res.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_arc_shared_metrics_access() {
    let rt = Arc::new(Runtime::with_default_backends());
    let mut set = JoinSet::new();
    for _ in 0..10 {
        let rt = Arc::clone(&rt);
        set.spawn(async move {
            let _metrics = rt.metrics();
        });
    }
    while let Some(res) = set.join_next().await {
        res.unwrap();
    }
}

// ===========================================================================
// 3. BackendRegistry concurrent access
// ===========================================================================

#[test]
fn backend_registry_concurrent_reads() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    let reg = Arc::new(reg);

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let reg = Arc::clone(&reg);
            std::thread::spawn(move || {
                assert!(reg.get("mock").is_some());
                assert!(reg.contains("mock"));
                let names = reg.list();
                assert!(names.contains(&"mock"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn backend_registry_get_arc_concurrent() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    let reg = Arc::new(reg);

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let reg = Arc::clone(&reg);
            std::thread::spawn(move || {
                let arc = reg.get_arc("mock").unwrap();
                let _id = arc.identity();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 4. MockBackend concurrent runs
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn mock_backend_concurrent_runs() {
    let backend = Arc::new(MockBackend);
    let mut set = JoinSet::new();

    for i in 0..20 {
        let backend = Arc::clone(&backend);
        set.spawn(async move {
            let (tx, mut rx) = mpsc::channel(256);
            let wo = make_work_order(&format!("task-{i}"));
            let run_id = Uuid::new_v4();
            let receipt = backend.run(run_id, wo, tx).await.unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
            assert!(receipt.receipt_sha256.is_some());

            // Drain events
            let mut count = 0;
            while rx.try_recv().is_ok() {
                count += 1;
            }
            assert!(count > 0);
        });
    }

    while let Some(res) = set.join_next().await {
        res.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mock_backend_concurrent_identity_calls() {
    let backend = Arc::new(MockBackend);
    let mut set = JoinSet::new();

    for _ in 0..50 {
        let backend = Arc::clone(&backend);
        set.spawn(async move {
            let id = backend.identity();
            assert_eq!(id.id, "mock");
        });
    }

    while let Some(res) = set.join_next().await {
        res.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mock_backend_concurrent_capabilities_calls() {
    let backend = Arc::new(MockBackend);
    let mut set = JoinSet::new();

    for _ in 0..50 {
        let backend = Arc::clone(&backend);
        set.spawn(async move {
            let caps = backend.capabilities();
            assert!(!caps.is_empty());
        });
    }

    while let Some(res) = set.join_next().await {
        res.unwrap();
    }
}

// ===========================================================================
// 5. Receipt hash determinism across threads
// ===========================================================================

#[test]
fn receipt_hash_deterministic_across_threads() {
    let receipt = make_receipt("mock");
    let receipt = Arc::new(receipt);

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let receipt = Arc::clone(&receipt);
            std::thread::spawn(move || receipt_hash(&receipt).unwrap())
        })
        .collect();

    let hashes: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let first = &hashes[0];
    for h in &hashes[1..] {
        assert_eq!(
            first, h,
            "receipt hash must be deterministic across threads"
        );
    }
}

#[test]
fn receipt_with_hash_deterministic_across_threads() {
    // Build a shared receipt (deterministic), then hash it from multiple threads.
    let started = Utc::now();
    let receipt = Arc::new(
        abp_receipt::ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(started)
            .finished_at(started)
            .work_order_id(Uuid::nil())
            .run_id(Uuid::nil())
            .build(),
    );

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let receipt = Arc::clone(&receipt);
            std::thread::spawn(move || abp_receipt::compute_hash(&receipt).unwrap())
        })
        .collect();

    let hashes: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let first = &hashes[0];
    for h in &hashes[1..] {
        assert_eq!(first, h);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn receipt_hash_deterministic_tokio_tasks() {
    let receipt = Arc::new(make_receipt("mock"));
    let mut set = JoinSet::new();

    for _ in 0..20 {
        let receipt = Arc::clone(&receipt);
        set.spawn(async move { receipt_hash(&receipt).unwrap() });
    }

    let mut hashes = Vec::new();
    while let Some(res) = set.join_next().await {
        hashes.push(res.unwrap());
    }

    let first = &hashes[0];
    for h in &hashes[1..] {
        assert_eq!(first, h);
    }
}

// ===========================================================================
// 6. Event stream from multiple producers
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn multiple_producers_single_consumer() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(256);
    let mut set = JoinSet::new();
    let num_producers = 10;
    let events_per_producer = 5;

    for i in 0..num_producers {
        let tx = tx.clone();
        set.spawn(async move {
            for j in 0..events_per_producer {
                let ev = make_event(AgentEventKind::AssistantDelta {
                    text: format!("producer-{i}-msg-{j}"),
                });
                tx.send(ev).await.unwrap();
            }
        });
    }
    drop(tx); // close sender so receiver terminates

    // Wait for all producers
    while let Some(res) = set.join_next().await {
        res.unwrap();
    }

    let mut collected = Vec::new();
    while let Some(ev) = rx.recv().await {
        collected.push(ev);
    }
    assert_eq!(collected.len(), num_producers * events_per_producer);
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_producers_events_no_data_loss() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(512);
    let barrier = Arc::new(tokio::sync::Barrier::new(20));

    let mut set = JoinSet::new();
    for i in 0..20 {
        let tx = tx.clone();
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            for j in 0..10 {
                let ev = make_event(AgentEventKind::AssistantMessage {
                    text: format!("{i}-{j}"),
                });
                tx.send(ev).await.unwrap();
            }
        });
    }
    drop(tx);

    while let Some(res) = set.join_next().await {
        res.unwrap();
    }

    let mut count = 0;
    while rx.recv().await.is_some() {
        count += 1;
    }
    assert_eq!(count, 200);
}

// ===========================================================================
// 7. Workspace staging is thread-safe
// ===========================================================================

#[test]
fn workspace_stager_concurrent_staging() {
    // Create a shared source directory
    let src = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("file.txt"), "hello").unwrap();
    let src_path = src.path().to_path_buf();

    let handles: Vec<_> = (0..5)
        .map(|_| {
            let src = src_path.clone();
            std::thread::spawn(move || {
                let ws = abp_workspace::WorkspaceStager::new()
                    .source_root(src)
                    .with_git_init(false)
                    .stage()
                    .unwrap();
                let content = std::fs::read_to_string(ws.path().join("file.txt")).unwrap();
                assert_eq!(content, "hello");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn workspace_manager_prepare_concurrent() {
    let src = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("a.txt"), "data").unwrap();
    let root_str = src.path().to_string_lossy().to_string();

    let handles: Vec<_> = (0..5)
        .map(|_| {
            let root = root_str.clone();
            std::thread::spawn(move || {
                let spec = abp_core::WorkspaceSpec {
                    root,
                    mode: WorkspaceMode::Staged,
                    include: vec![],
                    exclude: vec![],
                };
                let prepared = abp_workspace::WorkspaceManager::prepare(&spec).unwrap();
                assert!(prepared.path().join("a.txt").exists());
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 8. Policy engine concurrent checks
// ===========================================================================

#[test]
fn policy_engine_concurrent_tool_checks() {
    let policy = abp_core::PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = Arc::new(PolicyEngine::new(&policy).unwrap());

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                assert!(engine.can_use_tool("Read").allowed);
                assert!(!engine.can_use_tool("Bash").allowed);
                assert!(!engine.can_use_tool("Grep").allowed);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn policy_engine_concurrent_path_checks() {
    let policy = abp_core::PolicyProfile {
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        ..Default::default()
    };
    let engine = Arc::new(PolicyEngine::new(&policy).unwrap());

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                assert!(!engine.can_read_path(std::path::Path::new(".env")).allowed);
                assert!(
                    engine
                        .can_read_path(std::path::Path::new("src/lib.rs"))
                        .allowed
                );
                assert!(
                    !engine
                        .can_write_path(std::path::Path::new(".git/config"))
                        .allowed
                );
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn policy_engine_concurrent_mixed_checks() {
    let policy = abp_core::PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["secret*".into()],
        deny_write: vec!["locked*".into()],
        ..Default::default()
    };
    let engine = Arc::new(PolicyEngine::new(&policy).unwrap());

    let handles: Vec<_> = (0..30)
        .map(|i| {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                let tool = if i % 2 == 0 { "Read" } else { "Bash" };
                let decision = engine.can_use_tool(tool);
                if tool == "Bash" {
                    assert!(!decision.allowed);
                } else {
                    assert!(decision.allowed);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 9. Glob compilation is thread-safe
// ===========================================================================

#[test]
fn glob_compilation_concurrent() {
    let handles: Vec<_> = (0..20)
        .map(|i| {
            std::thread::spawn(move || {
                let include = vec![format!("src{i}/**")];
                let exclude = vec![format!("src{i}/generated/**")];
                let globs = IncludeExcludeGlobs::new(&include, &exclude).unwrap();
                assert_eq!(
                    globs.decide_str(&format!("src{i}/lib.rs")),
                    MatchDecision::Allowed
                );
                assert_eq!(
                    globs.decide_str(&format!("src{i}/generated/out.rs")),
                    MatchDecision::DeniedByExclude
                );
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn glob_matching_concurrent_reads() {
    let globs = Arc::new(
        IncludeExcludeGlobs::new(
            &["src/**".into(), "tests/**".into()],
            &["src/generated/**".into()],
        )
        .unwrap(),
    );

    let handles: Vec<_> = (0..30)
        .map(|_| {
            let globs = Arc::clone(&globs);
            std::thread::spawn(move || {
                assert_eq!(globs.decide_str("src/lib.rs"), MatchDecision::Allowed);
                assert_eq!(
                    globs.decide_str("src/generated/out.rs"),
                    MatchDecision::DeniedByExclude
                );
                assert_eq!(
                    globs.decide_str("README.md"),
                    MatchDecision::DeniedByMissingInclude
                );
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 10. Config loading is thread-safe
// ===========================================================================

#[test]
fn config_parse_toml_concurrent() {
    let toml_str = r#"
        default_backend = "mock"
        log_level = "debug"
        [backends.mock]
        type = "mock"
    "#;

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let s = toml_str.to_string();
            std::thread::spawn(move || {
                let cfg = abp_config::parse_toml(&s).unwrap();
                assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn config_validate_concurrent() {
    let cfg = Arc::new(abp_config::BackplaneConfig::default());

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let cfg = Arc::clone(&cfg);
            std::thread::spawn(move || {
                let warnings = abp_config::validate_config(&cfg).unwrap();
                assert!(!warnings.is_empty());
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn config_load_none_concurrent() {
    let handles: Vec<_> = (0..10)
        .map(|_| {
            std::thread::spawn(|| {
                let cfg = abp_config::load_config(None).unwrap();
                assert_eq!(cfg.log_level.as_deref(), Some("info"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 11. Multiple runtimes in parallel
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn multiple_runtimes_parallel_runs() {
    let mut set = JoinSet::new();

    for i in 0..5 {
        set.spawn(async move {
            let rt = Runtime::with_default_backends();
            let wo = make_work_order(&format!("parallel-runtime-{i}"));
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
        });
    }

    while let Some(res) = set.join_next().await {
        res.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_runtimes_independent_registries() {
    let mut set = JoinSet::new();

    for _ in 0..10 {
        set.spawn(async move {
            let rt = Runtime::with_default_backends();
            let names = rt.backend_names();
            assert!(names.contains(&"mock".to_string()));
            assert_eq!(names.len(), 1);
        });
    }

    while let Some(res) = set.join_next().await {
        res.unwrap();
    }
}

// ===========================================================================
// 12. Atomic receipt chain updates
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn receipt_chain_sequential_push_via_mutex() {
    let chain = Arc::new(tokio::sync::Mutex::new(ReceiptChain::new()));
    let mut set = JoinSet::new();

    for i in 0..10 {
        let chain = Arc::clone(&chain);
        set.spawn(async move {
            // Each task creates a hashed receipt with increasing timestamps
            let started = Utc::now();
            tokio::time::sleep(std::time::Duration::from_millis(i * 5)).await;
            let finished = Utc::now();
            let r = abp_receipt::ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .started_at(started)
                .finished_at(finished)
                .with_hash()
                .unwrap();
            let mut guard = chain.lock().await;
            // May fail if out of order — that's OK, we just track successes
            let _ = guard.push(r);
        });
    }

    while let Some(res) = set.join_next().await {
        res.unwrap();
    }

    let guard = chain.lock().await;
    // At least some receipts should have been pushed
    assert!(!guard.is_empty());
}

#[test]
fn receipt_chain_thread_safe_verify() {
    let mut chain = ReceiptChain::new();
    for _ in 0..5 {
        let r = make_hashed_receipt("mock");
        let _ = chain.push(r);
    }
    let chain = Arc::new(chain);

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let chain = Arc::clone(&chain);
            std::thread::spawn(move || {
                // verify is read-only
                let result = chain.verify();
                assert!(result.is_ok());
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn receipt_chain_concurrent_iteration() {
    let mut chain = ReceiptChain::new();
    for _ in 0..5 {
        let _ = chain.push(make_hashed_receipt("mock"));
    }
    let chain = Arc::new(chain);

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let chain = Arc::clone(&chain);
            std::thread::spawn(move || {
                let count = chain.iter().count();
                assert_eq!(count, 5);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 13. Concurrent event filtering
// ===========================================================================

#[test]
fn event_filter_include_concurrent() {
    let filter = Arc::new(EventFilter::include_kinds(&["assistant_message", "error"]));

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let filter = Arc::clone(&filter);
            std::thread::spawn(move || {
                let msg = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
                assert!(filter.matches(&msg));

                let started = make_event(AgentEventKind::RunStarted {
                    message: "go".into(),
                });
                assert!(!filter.matches(&started));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn event_filter_exclude_concurrent() {
    let filter = Arc::new(EventFilter::exclude_kinds(&["assistant_delta"]));

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let filter = Arc::clone(&filter);
            std::thread::spawn(move || {
                let delta = make_event(AgentEventKind::AssistantDelta { text: "...".into() });
                assert!(!filter.matches(&delta));

                let msg = make_event(AgentEventKind::AssistantMessage {
                    text: "done".into(),
                });
                assert!(filter.matches(&msg));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn stream_event_filter_concurrent() {
    let filter = Arc::new(StreamEventFilter::by_kind("error"));

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let filter = Arc::clone(&filter);
            std::thread::spawn(move || {
                let err_ev = make_event(AgentEventKind::Error {
                    message: "oops".into(),
                    error_code: None,
                });
                assert!(filter.matches(&err_ev));

                let msg_ev = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
                assert!(!filter.matches(&msg_ev));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 14. No data races in projection matrix
// ===========================================================================

#[test]
fn projection_matrix_concurrent_reads() {
    use abp_core::{Capability, SupportLevel};
    use abp_dialect::Dialect;

    let mut matrix = ProjectionMatrix::new();
    let mut caps = abp_core::CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    matrix.register_backend("mock", caps.clone(), Dialect::Claude, 50);
    matrix.register_backend("openai", caps, Dialect::OpenAi, 80);

    let matrix = Arc::new(matrix);

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let matrix = Arc::clone(&matrix);
            std::thread::spawn(move || {
                let wo = make_work_order("test-projection");
                let result = matrix.project(&wo);
                assert!(result.is_ok());
                let r = result.unwrap();
                assert!(!r.selected_backend.is_empty());
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn projection_matrix_concurrent_with_requirements() {
    use abp_core::{
        Capability, CapabilityRequirement, CapabilityRequirements, MinSupport, SupportLevel,
    };
    use abp_dialect::Dialect;

    let mut matrix = ProjectionMatrix::new();
    let mut caps = abp_core::CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    matrix.register_backend("backend-a", caps, Dialect::Claude, 50);

    let matrix = Arc::new(matrix);

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let matrix = Arc::clone(&matrix);
            std::thread::spawn(move || {
                let wo = WorkOrderBuilder::new("test")
                    .workspace_mode(WorkspaceMode::PassThrough)
                    .root(".")
                    .requirements(CapabilityRequirements {
                        required: vec![CapabilityRequirement {
                            capability: Capability::Streaming,
                            min_support: MinSupport::Native,
                        }],
                    })
                    .build();
                let result = matrix.project(&wo).unwrap();
                assert_eq!(result.selected_backend, "backend-a");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 15. Tokio task spawning with ABP types
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn spawn_tasks_with_work_orders() {
    let mut set = JoinSet::new();

    for i in 0..20 {
        set.spawn(async move {
            let wo = make_work_order(&format!("spawned-task-{i}"));
            assert_eq!(wo.task, format!("spawned-task-{i}"));
            wo
        });
    }

    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        results.push(res.unwrap());
    }
    assert_eq!(results.len(), 20);
}

#[tokio::test(flavor = "multi_thread")]
async fn spawn_tasks_with_receipts() {
    let mut set = JoinSet::new();

    for i in 0..20 {
        set.spawn(async move {
            let r = make_hashed_receipt(&format!("backend-{i}"));
            assert!(r.receipt_sha256.is_some());
            r
        });
    }

    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        results.push(res.unwrap());
    }
    assert_eq!(results.len(), 20);
}

#[tokio::test(flavor = "multi_thread")]
async fn spawn_tasks_with_events() {
    let mut set = JoinSet::new();

    for i in 0..50 {
        set.spawn(async move {
            make_event(AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            })
        });
    }

    let mut events = Vec::new();
    while let Some(res) = set.join_next().await {
        events.push(res.unwrap());
    }
    assert_eq!(events.len(), 50);
}

// ===========================================================================
// 16. Event recorder concurrent access
// ===========================================================================

#[test]
fn event_recorder_concurrent_recording() {
    let recorder = Arc::new(EventRecorder::new());

    let handles: Vec<_> = (0..20)
        .map(|i| {
            let recorder = Arc::clone(&recorder);
            std::thread::spawn(move || {
                let ev = make_event(AgentEventKind::AssistantMessage {
                    text: format!("msg-{i}"),
                });
                recorder.record(&ev);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(recorder.len(), 20);
    assert_eq!(recorder.events().len(), 20);
}

#[test]
fn event_recorder_concurrent_read_write() {
    let recorder = Arc::new(EventRecorder::new());

    // Writers
    let write_handles: Vec<_> = (0..10)
        .map(|i| {
            let recorder = Arc::clone(&recorder);
            std::thread::spawn(move || {
                let ev = make_event(AgentEventKind::AssistantDelta {
                    text: format!("delta-{i}"),
                });
                recorder.record(&ev);
            })
        })
        .collect();

    // Readers
    let read_handles: Vec<_> = (0..10)
        .map(|_| {
            let recorder = Arc::clone(&recorder);
            std::thread::spawn(move || {
                let _events = recorder.events();
                let _len = recorder.len();
            })
        })
        .collect();

    for h in write_handles.into_iter().chain(read_handles) {
        h.join().unwrap();
    }

    assert!(recorder.len() <= 10);
}

// ===========================================================================
// 17. Event stats concurrent observation
// ===========================================================================

#[test]
fn event_stats_concurrent_observe() {
    let stats = Arc::new(EventStats::new());

    let handles: Vec<_> = (0..20)
        .map(|i| {
            let stats = Arc::clone(&stats);
            std::thread::spawn(move || {
                let ev = make_event(AgentEventKind::AssistantDelta {
                    text: format!("tok-{i}"),
                });
                stats.observe(&ev);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(stats.total_events(), 20);
    assert_eq!(stats.count_for("assistant_delta"), 20);
    assert!(stats.total_delta_bytes() > 0);
}

#[test]
fn event_stats_concurrent_mixed_kinds() {
    let stats = Arc::new(EventStats::new());

    let handles: Vec<_> = (0..30)
        .map(|i| {
            let stats = Arc::clone(&stats);
            std::thread::spawn(move || {
                let kind = match i % 3 {
                    0 => AgentEventKind::AssistantMessage { text: "msg".into() },
                    1 => AgentEventKind::Error {
                        message: "err".into(),
                        error_code: None,
                    },
                    _ => AgentEventKind::AssistantDelta {
                        text: "delta".into(),
                    },
                };
                stats.observe(&make_event(kind));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(stats.total_events(), 30);
    assert_eq!(stats.error_count(), 10);
}

// ===========================================================================
// 18. Stream pipeline concurrent processing
// ===========================================================================

#[test]
fn stream_pipeline_concurrent_process() {
    let pipeline = Arc::new(
        StreamPipelineBuilder::new()
            .filter(StreamEventFilter::exclude_errors())
            .transform(EventTransform::identity())
            .build(),
    );

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let pipeline = Arc::clone(&pipeline);
            std::thread::spawn(move || {
                let msg = make_event(AgentEventKind::AssistantMessage {
                    text: "hello".into(),
                });
                let result = pipeline.process(msg);
                assert!(result.is_some());

                let err = make_event(AgentEventKind::Error {
                    message: "bad".into(),
                    error_code: None,
                });
                let result = pipeline.process(err);
                assert!(result.is_none());
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn stream_pipeline_with_recorder_concurrent() {
    let recorder = EventRecorder::new();
    let pipeline = Arc::new(
        StreamPipelineBuilder::new()
            .with_recorder(recorder.clone())
            .build(),
    );

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let pipeline = Arc::clone(&pipeline);
            std::thread::spawn(move || {
                let ev = make_event(AgentEventKind::AssistantMessage {
                    text: format!("msg-{i}"),
                });
                pipeline.process(ev);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(recorder.len(), 10);
}

#[test]
fn stream_pipeline_with_stats_concurrent() {
    let stats = EventStats::new();
    let pipeline = Arc::new(
        StreamPipelineBuilder::new()
            .with_stats(stats.clone())
            .build(),
    );

    let handles: Vec<_> = (0..15)
        .map(|_| {
            let pipeline = Arc::clone(&pipeline);
            std::thread::spawn(move || {
                let ev = make_event(AgentEventKind::RunStarted {
                    message: "go".into(),
                });
                pipeline.process(ev);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(stats.total_events(), 15);
}

// ===========================================================================
// 19. Work order builder from multiple threads
// ===========================================================================

#[test]
fn work_order_builder_concurrent() {
    let handles: Vec<_> = (0..20)
        .map(|i| {
            std::thread::spawn(move || {
                let wo = WorkOrderBuilder::new(format!("task-{i}"))
                    .workspace_mode(WorkspaceMode::PassThrough)
                    .root(".")
                    .model(format!("model-{i}"))
                    .max_turns(10)
                    .build();
                assert_eq!(wo.task, format!("task-{i}"));
                assert_eq!(
                    wo.config.model.as_deref(),
                    Some(format!("model-{i}").as_str())
                );
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 20. Receipt builder from multiple threads
// ===========================================================================

#[test]
fn receipt_builder_concurrent() {
    let handles: Vec<_> = (0..20)
        .map(|i| {
            std::thread::spawn(move || {
                let r = ReceiptBuilder::new(format!("backend-{i}"))
                    .outcome(Outcome::Complete)
                    .with_hash()
                    .unwrap();
                assert!(r.receipt_sha256.is_some());
                assert_eq!(r.backend.id, format!("backend-{i}"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 21. Canonical JSON hashing is thread-safe
// ===========================================================================

#[test]
fn canonical_json_concurrent() {
    let value = Arc::new(serde_json::json!({"b": 2, "a": 1, "c": [3, 2, 1]}));

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let value = Arc::clone(&value);
            std::thread::spawn(move || abp_core::canonical_json(&*value).unwrap())
        })
        .collect();

    let results: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let first = &results[0];
    for r in &results[1..] {
        assert_eq!(first, r);
    }
}

#[test]
fn sha256_hex_concurrent() {
    let data = Arc::new(b"hello world".to_vec());

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let data = Arc::clone(&data);
            std::thread::spawn(move || abp_core::sha256_hex(&data))
        })
        .collect();

    let results: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let first = &results[0];
    for r in &results[1..] {
        assert_eq!(first, r);
    }
}

// ===========================================================================
// 22. Event multiplexer from concurrent sources
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn event_multiplexer_concurrent_sources() {
    use abp_stream::EventMultiplexer;

    let mut receivers = Vec::new();
    let events_per_source = 5;
    let num_sources = 4;

    for i in 0..num_sources {
        let (tx, rx) = mpsc::channel::<AgentEvent>(32);
        receivers.push(rx);
        tokio::spawn(async move {
            for j in 0..events_per_source {
                let ev = make_event(AgentEventKind::AssistantDelta {
                    text: format!("src{i}-{j}"),
                });
                let _ = tx.send(ev).await;
            }
        });
    }

    let mux = EventMultiplexer::new(receivers);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), num_sources * events_per_source);
}

// ===========================================================================
// 23. Config merge is thread-safe
// ===========================================================================

#[test]
fn config_merge_concurrent() {
    let handles: Vec<_> = (0..10)
        .map(|i| {
            std::thread::spawn(move || {
                let base = abp_config::BackplaneConfig {
                    default_backend: Some("mock".into()),
                    ..Default::default()
                };
                let overlay = abp_config::BackplaneConfig {
                    default_backend: Some(format!("backend-{i}")),
                    ..Default::default()
                };
                let merged = abp_config::merge_configs(base, overlay);
                assert_eq!(
                    merged.default_backend.as_deref(),
                    Some(format!("backend-{i}").as_str())
                );
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 24. Receipt verify_hash concurrent
// ===========================================================================

#[test]
fn receipt_verify_hash_concurrent() {
    let receipt = Arc::new(make_hashed_receipt("mock"));

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let receipt = Arc::clone(&receipt);
            std::thread::spawn(move || {
                assert!(abp_receipt::verify_hash(&receipt));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// 25. Receipt canonicalize concurrent
// ===========================================================================

#[test]
fn receipt_canonicalize_concurrent() {
    let receipt = Arc::new(make_receipt("mock"));

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let receipt = Arc::clone(&receipt);
            std::thread::spawn(move || abp_receipt::canonicalize(&receipt).unwrap())
        })
        .collect();

    let results: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let first = &results[0];
    for r in &results[1..] {
        assert_eq!(first, r);
    }
}

// ===========================================================================
// 26. MockBackend unique run IDs across concurrent runs
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn mock_backend_unique_run_ids_concurrent() {
    let backend = Arc::new(MockBackend);
    let mut set = JoinSet::new();

    for i in 0..30 {
        let backend = Arc::clone(&backend);
        set.spawn(async move {
            let (tx, _rx) = mpsc::channel(256);
            let wo = make_work_order(&format!("unique-id-{i}"));
            let run_id = Uuid::new_v4();
            let receipt = backend.run(run_id, wo, tx).await.unwrap();
            (receipt.meta.run_id, receipt.receipt_sha256)
        });
    }

    let mut run_ids = std::collections::HashSet::new();
    while let Some(res) = set.join_next().await {
        let (run_id, hash) = res.unwrap();
        assert!(hash.is_some());
        run_ids.insert(run_id);
    }
    // All 30 run IDs should be unique
    assert_eq!(run_ids.len(), 30);
}

// ===========================================================================
// 27. Runtime run_streaming concurrent
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn runtime_concurrent_run_streaming() {
    let rt = Arc::new(Runtime::with_default_backends());
    let barrier = Arc::new(tokio::sync::Barrier::new(10));
    let mut set = JoinSet::new();

    for i in 0..10 {
        let rt = Arc::clone(&rt);
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let wo = make_work_order(&format!("concurrent-stream-{i}"));
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
            receipt.meta.run_id
        });
    }

    let mut ids = std::collections::HashSet::new();
    while let Some(res) = set.join_next().await {
        ids.insert(res.unwrap());
    }
    assert_eq!(ids.len(), 10);
}

// ===========================================================================
// 28. Event stream piping with pipeline
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn event_stream_pipe_concurrent() {
    use abp_stream::EventStream;

    let pipeline = StreamPipelineBuilder::new()
        .filter(StreamEventFilter::exclude_errors())
        .build();

    let (in_tx, in_rx) = mpsc::channel::<AgentEvent>(64);
    let (out_tx, mut out_rx) = mpsc::channel::<AgentEvent>(64);

    let stream = EventStream::new(in_rx);
    let pipe_task = tokio::spawn(async move {
        stream.pipe(&pipeline, out_tx).await;
    });

    // Send events from multiple tasks
    let mut producers = JoinSet::new();
    for i in 0..5 {
        let tx = in_tx.clone();
        producers.spawn(async move {
            let ev = make_event(AgentEventKind::AssistantMessage {
                text: format!("msg-{i}"),
            });
            tx.send(ev).await.unwrap();
        });
    }
    // Also send an error that should be filtered out
    in_tx
        .send(make_event(AgentEventKind::Error {
            message: "filtered".into(),
            error_code: None,
        }))
        .await
        .unwrap();
    drop(in_tx);

    while let Some(res) = producers.join_next().await {
        res.unwrap();
    }

    let mut received = Vec::new();
    while let Some(ev) = out_rx.recv().await {
        received.push(ev);
    }

    pipe_task.await.unwrap();

    // Error should be filtered out: only 5 assistant messages should pass
    assert_eq!(received.len(), 5);
}

// ===========================================================================
// 29. Cloning types across threads
// ===========================================================================

#[test]
fn receipt_clone_across_threads() {
    let receipt = make_hashed_receipt("mock");
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let r = receipt.clone();
            std::thread::spawn(move || {
                assert!(r.receipt_sha256.is_some());
                r.backend.id.clone()
            })
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), "mock");
    }
}

#[test]
fn work_order_clone_across_threads() {
    let wo = make_work_order("clone-test");
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let w = wo.clone();
            std::thread::spawn(move || w.task.clone())
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), "clone-test");
    }
}

#[test]
fn agent_event_clone_across_threads() {
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "thread-safe".into(),
    });
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let e = ev.clone();
            std::thread::spawn(move || matches!(e.kind, AgentEventKind::AssistantMessage { .. }))
        })
        .collect();

    for h in handles {
        assert!(h.join().unwrap());
    }
}

// ===========================================================================
// 30. Serialization across threads
// ===========================================================================

#[test]
fn receipt_serde_concurrent() {
    let receipt = Arc::new(make_hashed_receipt("mock"));

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let receipt = Arc::clone(&receipt);
            std::thread::spawn(move || {
                let json = serde_json::to_string(&*receipt).unwrap();
                let deserialized: Receipt = serde_json::from_str(&json).unwrap();
                assert_eq!(deserialized.backend.id, "mock");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn work_order_serde_concurrent() {
    let wo = Arc::new(make_work_order("serde-test"));

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let wo = Arc::clone(&wo);
            std::thread::spawn(move || {
                let json = serde_json::to_string(&*wo).unwrap();
                let deserialized: WorkOrder = serde_json::from_str(&json).unwrap();
                assert_eq!(deserialized.task, "serde-test");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn agent_event_serde_concurrent() {
    let ev = Arc::new(make_event(AgentEventKind::ToolCall {
        tool_name: "Read".into(),
        tool_use_id: Some("id-1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "src/lib.rs"}),
    }));

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let ev = Arc::clone(&ev);
            std::thread::spawn(move || {
                let json = serde_json::to_string(&*ev).unwrap();
                let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
                assert!(matches!(deserialized.kind, AgentEventKind::ToolCall { .. }));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}
