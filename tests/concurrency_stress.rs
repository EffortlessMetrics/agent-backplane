// SPDX-License-Identifier: MIT OR Apache-2.0
//! Concurrency stress tests for the Agent Backplane runtime.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use abp_core::{
    AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, WorkOrderBuilder, WorkspaceMode,
    filter::EventFilter, receipt_hash, validate::validate_receipt,
};
use abp_daemon::{AppState, RunRequest, RunResponse, RunTracker};
use abp_runtime::Runtime;
use abp_runtime::pipeline::{AuditStage, Pipeline, ValidationStage};
use abp_runtime::store::ReceiptStore;
use abp_runtime::telemetry::RunMetrics;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio_stream::StreamExt;
use tower::ServiceExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_work_order(task: &str) -> abp_core::WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .root(".")
        .build()
}

fn make_app_state() -> (Arc<AppState>, tempfile::TempDir) {
    let rt = Runtime::with_default_backends();
    let tmp = tempfile::tempdir().unwrap();
    let state = Arc::new(AppState {
        runtime: Arc::new(rt),
        receipts: Arc::new(RwLock::new(HashMap::new())),
        receipts_dir: tmp.path().to_path_buf(),
        run_tracker: RunTracker::new(),
    });
    (state, tmp)
}

// ---------------------------------------------------------------------------
// 1. 50 concurrent MockBackend runs → all produce unique receipts
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_50_concurrent_mock_runs_unique_receipts() {
    let rt = Runtime::with_default_backends();
    let rt = Arc::new(rt);
    let barrier = Arc::new(tokio::sync::Barrier::new(50));

    let mut set = JoinSet::new();
    for i in 0..50 {
        let rt = Arc::clone(&rt);
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let wo = make_work_order(&format!("task-{i}"));
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let mut events = handle.events;
            while events.next().await.is_some() {}
            handle.receipt.await.unwrap().unwrap()
        });
    }

    let mut run_ids = HashSet::new();
    let mut hashes = HashSet::new();
    while let Some(result) = set.join_next().await {
        let receipt = result.unwrap();
        assert!(receipt.receipt_sha256.is_some());
        run_ids.insert(receipt.meta.run_id);
        hashes.insert(receipt.receipt_sha256.clone().unwrap());
    }
    assert_eq!(run_ids.len(), 50, "all run_ids must be unique");
    assert_eq!(hashes.len(), 50, "all receipt hashes must be unique");
}

// ---------------------------------------------------------------------------
// 2. 100 concurrent receipt store operations → no data corruption
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_100_concurrent_receipt_store_operations() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(ReceiptStore::new(tmp.path()));
    let barrier = Arc::new(tokio::sync::Barrier::new(100));

    let mut set = JoinSet::new();
    for _ in 0..100 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let receipt = ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .with_hash()
                .unwrap();
            let run_id = receipt.meta.run_id;
            store.save(&receipt).unwrap();
            let loaded = store.load(run_id).unwrap();
            assert_eq!(loaded.receipt_sha256, receipt.receipt_sha256);
            assert!(store.verify(run_id).unwrap());
            run_id
        });
    }

    let mut ids = HashSet::new();
    while let Some(result) = set.join_next().await {
        ids.insert(result.unwrap());
    }
    assert_eq!(ids.len(), 100);

    let listed = store.list().unwrap();
    assert_eq!(listed.len(), 100);

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, 100);
}

// ---------------------------------------------------------------------------
// 3. 50 concurrent telemetry metric updates → accurate counters
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_50_concurrent_telemetry_metric_updates() {
    let metrics = Arc::new(RunMetrics::new());
    let barrier = Arc::new(tokio::sync::Barrier::new(50));

    let mut set = JoinSet::new();
    for i in 0..50 {
        let metrics = Arc::clone(&metrics);
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let success = i % 2 == 0;
            metrics.record_run(100, success, 10);
        });
    }

    while let Some(result) = set.join_next().await {
        result.unwrap();
    }

    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, 50);
    assert_eq!(snap.successful_runs, 25);
    assert_eq!(snap.failed_runs, 25);
    assert_eq!(snap.total_events, 500);
    assert_eq!(snap.average_run_duration_ms, 100);
}

// ---------------------------------------------------------------------------
// 4. 20 concurrent daemon API requests → correct responses
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_20_concurrent_daemon_api_requests() {
    let (state, _tmp) = make_app_state();
    let app = abp_daemon::build_app(Arc::clone(&state));
    let barrier = Arc::new(tokio::sync::Barrier::new(20));

    let mut set = JoinSet::new();
    for i in 0..20 {
        let app = app.clone();
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let wo = make_work_order(&format!("daemon-task-{i}"));
            let body = serde_json::to_string(&RunRequest {
                backend: "mock".into(),
                work_order: wo,
            })
            .unwrap();
            let req = Request::builder()
                .method("POST")
                .uri("/run")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap();
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let bytes = http_body_util::BodyExt::collect(resp.into_body())
                .await
                .unwrap()
                .to_bytes();
            let run_resp: RunResponse = serde_json::from_slice(&bytes).unwrap();
            assert!(run_resp.receipt.receipt_sha256.is_some());
            run_resp.run_id
        });
    }

    let mut ids = HashSet::new();
    while let Some(result) = set.join_next().await {
        ids.insert(result.unwrap());
    }
    assert_eq!(ids.len(), 20);
}

// ---------------------------------------------------------------------------
// 5. 10 concurrent workspace staging operations → all isolated
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_10_concurrent_workspace_staging() {
    use abp_workspace::WorkspaceStager;

    let src = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("hello.txt"), "world").unwrap();
    let barrier = Arc::new(tokio::sync::Barrier::new(10));

    let mut set = JoinSet::new();
    for _ in 0..10 {
        let src_path = src.path().to_path_buf();
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let ws = WorkspaceStager::new()
                .source_root(src_path)
                .with_git_init(false)
                .stage()
                .unwrap();
            let content = std::fs::read_to_string(ws.path().join("hello.txt")).unwrap();
            assert_eq!(content, "world");
            // Verify isolation: writing in one staged workspace doesn't affect others.
            std::fs::write(ws.path().join("unique.txt"), "marker").unwrap();
            ws.path().to_path_buf()
        });
    }

    let mut paths = HashSet::new();
    while let Some(result) = set.join_next().await {
        paths.insert(result.unwrap());
    }
    assert_eq!(
        paths.len(),
        10,
        "all staged workspaces must have unique paths"
    );
}

// ---------------------------------------------------------------------------
// 6. 100 concurrent event filter applications → correct results
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_100_concurrent_event_filter_applications() {
    let include_filter = Arc::new(EventFilter::include_kinds(&["assistant_message", "error"]));
    let exclude_filter = Arc::new(EventFilter::exclude_kinds(&["assistant_delta"]));

    let events: Arc<Vec<AgentEvent>> = Arc::new(vec![
        AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        },
        AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "chunk".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::Error {
                message: "oops".into(),
                error_code: None,
            },
            ext: None,
        },
        AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        },
    ]);

    let barrier = Arc::new(tokio::sync::Barrier::new(100));
    let mut set = JoinSet::new();

    for _ in 0..100 {
        let inc = Arc::clone(&include_filter);
        let exc = Arc::clone(&exclude_filter);
        let evts = Arc::clone(&events);
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let inc_results: Vec<bool> = evts.iter().map(|e| inc.matches(e)).collect();
            let exc_results: Vec<bool> = evts.iter().map(|e| exc.matches(e)).collect();
            // Include filter: assistant_message=true, assistant_delta=false, error=true, run_started=false
            assert_eq!(inc_results, vec![true, false, true, false]);
            // Exclude filter: assistant_message=true, assistant_delta=false, error=true, run_started=true
            assert_eq!(exc_results, vec![true, false, true, true]);
        });
    }

    while let Some(result) = set.join_next().await {
        result.unwrap();
    }
}

// ---------------------------------------------------------------------------
// 7. 50 concurrent backend registry lookups → consistent results
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_50_concurrent_backend_registry_lookups() {
    let rt = Runtime::with_default_backends();
    let rt = Arc::new(rt);
    let barrier = Arc::new(tokio::sync::Barrier::new(50));

    let mut set = JoinSet::new();
    for _ in 0..50 {
        let rt = Arc::clone(&rt);
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let names = rt.backend_names();
            assert!(names.contains(&"mock".to_string()));

            let backend = rt.backend("mock");
            assert!(backend.is_some());

            let identity = backend.unwrap().identity();
            assert_eq!(identity.id, "mock");

            let caps = rt.registry().get("mock").unwrap().capabilities();
            assert!(!caps.is_empty());
        });
    }

    while let Some(result) = set.join_next().await {
        result.unwrap();
    }
}

// ---------------------------------------------------------------------------
// 8. 20 concurrent pipeline executions → no interference
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_20_concurrent_pipeline_executions() {
    let audit = Arc::new(AuditStage::new());
    let barrier = Arc::new(tokio::sync::Barrier::new(20));

    let mut set = JoinSet::new();
    for i in 0..20 {
        let audit = Arc::clone(&audit);
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let pipeline = Pipeline::new().stage(ValidationStage);
            let mut wo = make_work_order(&format!("pipeline-task-{i}"));
            pipeline.execute(&mut wo).await.unwrap();

            // Also run through audit stage directly to verify no cross-talk.
            audit.process_order(&mut wo).await;
            wo.id
        });
    }

    let mut ids = HashSet::new();
    while let Some(result) = set.join_next().await {
        ids.insert(result.unwrap());
    }
    assert_eq!(ids.len(), 20);
}

/// Extension trait so we can call audit stage process in the test.
trait AuditStageExt {
    async fn process_order(&self, order: &mut abp_core::WorkOrder);
}

impl AuditStageExt for AuditStage {
    async fn process_order(&self, order: &mut abp_core::WorkOrder) {
        use abp_runtime::pipeline::PipelineStage;
        self.process(order).await.unwrap();
    }
}

// ---------------------------------------------------------------------------
// 9. Rapid create/read/delete cycles on receipt store
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_rapid_create_read_delete_receipt_store() {
    let tmp = tempfile::tempdir().unwrap();
    let store_root = tmp.path().to_path_buf();
    let store = Arc::new(ReceiptStore::new(&store_root));
    let barrier = Arc::new(tokio::sync::Barrier::new(50));

    let mut set = JoinSet::new();
    for _ in 0..50 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        let root = store_root.clone();
        set.spawn(async move {
            barrier.wait().await;

            // Create
            let receipt = ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .with_hash()
                .unwrap();
            let run_id = receipt.meta.run_id;
            store.save(&receipt).unwrap();

            // Read
            let loaded = store.load(run_id).unwrap();
            assert_eq!(loaded.meta.run_id, run_id);
            assert!(store.verify(run_id).unwrap());

            // Delete (remove the file)
            let path = root.join(format!("{run_id}.json"));
            std::fs::remove_file(&path).unwrap();

            // Verify deleted
            assert!(store.load(run_id).is_err());
        });
    }

    while let Some(result) = set.join_next().await {
        result.unwrap();
    }

    // Store should be empty or near-empty after deletions.
    let remaining = store.list().unwrap();
    assert_eq!(remaining.len(), 0);
}

// ---------------------------------------------------------------------------
// 10. Mixed read/write load on daemon run tracker
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_mixed_read_write_daemon_run_tracker() {
    let tracker = RunTracker::new();
    let tracker = Arc::new(tracker);
    let barrier = Arc::new(tokio::sync::Barrier::new(60));

    let mut set = JoinSet::new();

    // 20 writers: start + complete runs
    for _ in 0..20 {
        let tracker = Arc::clone(&tracker);
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let run_id = Uuid::new_v4();
            tracker.start_run(run_id).await.unwrap();

            let receipt = ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .with_hash()
                .unwrap();
            tracker
                .complete_run(run_id, *Box::new(receipt))
                .await
                .unwrap();
            run_id
        });
    }

    // 20 writers: start + fail runs
    for _ in 0..20 {
        let tracker = Arc::clone(&tracker);
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let run_id = Uuid::new_v4();
            tracker.start_run(run_id).await.unwrap();
            tracker.fail_run(run_id, "test error".into()).await.unwrap();
            run_id
        });
    }

    // 20 readers: list runs concurrently
    for _ in 0..20 {
        let tracker = Arc::clone(&tracker);
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            // Multiple reads shouldn't panic or deadlock.
            let _ = tracker.list_runs().await;
            Uuid::nil() // placeholder
        });
    }

    let mut completed_ids = HashSet::new();
    while let Some(result) = set.join_next().await {
        let id = result.unwrap();
        if id != Uuid::nil() {
            completed_ids.insert(id);
        }
    }
    assert_eq!(completed_ids.len(), 40);

    let all_runs = tracker.list_runs().await;
    assert_eq!(all_runs.len(), 40);
}

// ---------------------------------------------------------------------------
// 11. Concurrent work order validation → deterministic results
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_concurrent_work_order_validation() {
    let barrier = Arc::new(tokio::sync::Barrier::new(50));

    let mut set = JoinSet::new();
    for i in 0..50 {
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;

            // Valid work orders
            let mut valid_wo = make_work_order(&format!("valid-task-{i}"));
            let pipeline = Pipeline::new().stage(ValidationStage);
            assert!(pipeline.execute(&mut valid_wo).await.is_ok());

            // Also test receipt validation concurrently
            let receipt = ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .with_hash()
                .unwrap();
            assert!(validate_receipt(&receipt).is_ok());

            // Deterministic hashing
            let hash1 = receipt_hash(&receipt).unwrap();
            let hash2 = receipt_hash(&receipt).unwrap();
            assert_eq!(hash1, hash2);
        });
    }

    while let Some(result) = set.join_next().await {
        result.unwrap();
    }
}

// ---------------------------------------------------------------------------
// 12. Stress test: 200 rapid-fire work orders via runtime
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_200_rapid_fire_work_orders_via_runtime() {
    let rt = Runtime::with_default_backends();
    let rt = Arc::new(rt);

    let mut set = JoinSet::new();
    for i in 0..200 {
        let rt = Arc::clone(&rt);
        set.spawn(async move {
            let wo = make_work_order(&format!("rapid-{i}"));
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let mut events = handle.events;
            let mut event_count = 0usize;
            while events.next().await.is_some() {
                event_count += 1;
            }
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert!(receipt.receipt_sha256.is_some());
            assert!(event_count > 0);
            assert!(matches!(receipt.outcome, Outcome::Complete));
            receipt.meta.run_id
        });
    }

    let mut ids = HashSet::new();
    while let Some(result) = set.join_next().await {
        ids.insert(result.unwrap());
    }
    assert_eq!(ids.len(), 200, "all 200 runs must produce unique IDs");
}

// ---------------------------------------------------------------------------
// 13. Concurrent receipt hash computation is deterministic
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_concurrent_receipt_hashing_deterministic() {
    let receipt = Arc::new(
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap(),
    );

    let expected_hash = receipt.receipt_sha256.clone().unwrap();
    let barrier = Arc::new(tokio::sync::Barrier::new(100));

    let mut set = JoinSet::new();
    for _ in 0..100 {
        let receipt = Arc::clone(&receipt);
        let expected = expected_hash.clone();
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let hash = receipt_hash(&receipt).unwrap();
            assert_eq!(hash, expected);
        });
    }

    while let Some(result) = set.join_next().await {
        result.unwrap();
    }
}

// ---------------------------------------------------------------------------
// 14. Concurrent daemon health + metrics endpoints
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn stress_concurrent_daemon_health_and_metrics() {
    let (state, _tmp) = make_app_state();
    let app = abp_daemon::build_app(Arc::clone(&state));
    let barrier = Arc::new(tokio::sync::Barrier::new(40));

    let mut set = JoinSet::new();
    for i in 0..40 {
        let app = app.clone();
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            let uri = if i % 2 == 0 { "/health" } else { "/metrics" };
            let req = Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap();
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        });
    }

    while let Some(result) = set.join_next().await {
        result.unwrap();
    }
}
