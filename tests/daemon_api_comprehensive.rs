#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Comprehensive tests for the daemon crate (`abp-daemon`) and the sdk-types
//! crate (`abp-sdk-types`).

// ═══════════════════════════════════════════════════════════════════════════
// Part 1 — Daemon: DaemonConfig
// ═══════════════════════════════════════════════════════════════════════════

mod daemon_config {
    use abp_daemon::DaemonConfig;

    #[test]
    fn default_bind_address() {
        let cfg = DaemonConfig::default();
        assert_eq!(cfg.bind_address, "127.0.0.1");
    }

    #[test]
    fn default_port() {
        let cfg = DaemonConfig::default();
        assert_eq!(cfg.port, 8088);
    }

    #[test]
    fn default_auth_token_is_none() {
        let cfg = DaemonConfig::default();
        assert!(cfg.auth_token.is_none());
    }

    #[test]
    fn bind_string_default() {
        let cfg = DaemonConfig::default();
        assert_eq!(cfg.bind_string(), "127.0.0.1:8088");
    }

    #[test]
    fn bind_string_custom() {
        let cfg = DaemonConfig {
            bind_address: "0.0.0.0".into(),
            port: 9000,
            auth_token: None,
        };
        assert_eq!(cfg.bind_string(), "0.0.0.0:9000");
    }

    #[test]
    fn serde_roundtrip() {
        let cfg = DaemonConfig {
            bind_address: "10.0.0.1".into(),
            port: 3000,
            auth_token: Some("secret".into()),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: DaemonConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bind_address, "10.0.0.1");
        assert_eq!(back.port, 3000);
        assert_eq!(back.auth_token.as_deref(), Some("secret"));
    }

    #[test]
    fn serde_omits_none_auth_token() {
        let cfg = DaemonConfig::default();
        let val = serde_json::to_value(&cfg).unwrap();
        assert!(val.get("auth_token").is_none());
    }

    #[test]
    fn serde_includes_auth_token_when_set() {
        let cfg = DaemonConfig {
            auth_token: Some("tok".into()),
            ..DaemonConfig::default()
        };
        let val = serde_json::to_value(&cfg).unwrap();
        assert_eq!(val["auth_token"], "tok");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 2 — Daemon: DaemonError
// ═══════════════════════════════════════════════════════════════════════════

mod daemon_error {
    use abp_daemon::DaemonError;
    use axum::http::StatusCode;

    #[test]
    fn not_found_status() {
        let e = DaemonError::NotFound("gone".into());
        assert_eq!(e.status_code(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn bad_request_status() {
        let e = DaemonError::BadRequest("bad".into());
        assert_eq!(e.status_code(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn conflict_status() {
        let e = DaemonError::Conflict("dup".into());
        assert_eq!(e.status_code(), StatusCode::CONFLICT);
    }

    #[test]
    fn internal_status() {
        let e = DaemonError::Internal(anyhow::anyhow!("boom"));
        assert_eq!(e.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn not_found_display() {
        let e = DaemonError::NotFound("run xyz".into());
        assert!(e.to_string().contains("run xyz"));
    }

    #[test]
    fn bad_request_display() {
        let e = DaemonError::BadRequest("missing field".into());
        assert!(e.to_string().contains("missing field"));
    }

    #[test]
    fn conflict_display() {
        let e = DaemonError::Conflict("already done".into());
        assert!(e.to_string().contains("already done"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 3 — Daemon: RunStatus (lib.rs enum)
// ═══════════════════════════════════════════════════════════════════════════

mod run_status_lib {
    use abp_daemon::RunStatus;

    #[test]
    fn pending_serde_roundtrip() {
        let s = RunStatus::Pending;
        let json = serde_json::to_string(&s).unwrap();
        let back: RunStatus = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, RunStatus::Pending));
    }

    #[test]
    fn running_serde_roundtrip() {
        let s = RunStatus::Running;
        let json = serde_json::to_string(&s).unwrap();
        let back: RunStatus = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, RunStatus::Running));
    }

    #[test]
    fn cancelled_serde_roundtrip() {
        let s = RunStatus::Cancelled;
        let json = serde_json::to_string(&s).unwrap();
        let back: RunStatus = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, RunStatus::Cancelled));
    }

    #[test]
    fn failed_serde_roundtrip() {
        let s = RunStatus::Failed {
            error: "boom".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RunStatus = serde_json::from_str(&json).unwrap();
        match back {
            RunStatus::Failed { error } => assert_eq!(error, "boom"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn completed_serde_roundtrip() {
        use abp_core::{Outcome, ReceiptBuilder};
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let s = RunStatus::Completed {
            receipt: Box::new(receipt),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RunStatus = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, RunStatus::Completed { .. }));
    }

    #[test]
    fn pending_serializes_as_tagged() {
        let s = RunStatus::Pending;
        let val = serde_json::to_value(&s).unwrap();
        assert_eq!(val["status"], "pending");
    }

    #[test]
    fn running_serializes_as_tagged() {
        let s = RunStatus::Running;
        let val = serde_json::to_value(&s).unwrap();
        assert_eq!(val["status"], "running");
    }

    #[test]
    fn cancelled_serializes_as_tagged() {
        let s = RunStatus::Cancelled;
        let val = serde_json::to_value(&s).unwrap();
        assert_eq!(val["status"], "cancelled");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 4 — Daemon: RunTracker
// ═══════════════════════════════════════════════════════════════════════════

mod run_tracker {
    use abp_core::{Outcome, ReceiptBuilder};
    use abp_daemon::RunTracker;
    use uuid::Uuid;

    #[tokio::test]
    async fn start_and_list() {
        let tracker = RunTracker::new();
        let id = Uuid::new_v4();
        tracker.start_run(id).await.unwrap();
        let runs = tracker.list_runs().await;
        assert_eq!(runs.len(), 1);
    }

    #[tokio::test]
    async fn duplicate_start_errors() {
        let tracker = RunTracker::new();
        let id = Uuid::new_v4();
        tracker.start_run(id).await.unwrap();
        assert!(tracker.start_run(id).await.is_err());
    }

    #[tokio::test]
    async fn complete_run_works() {
        let tracker = RunTracker::new();
        let id = Uuid::new_v4();
        tracker.start_run(id).await.unwrap();
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        tracker.complete_run(id, receipt).await.unwrap();
        let status = tracker.get_run_status(id).await.unwrap();
        assert!(matches!(status, abp_daemon::RunStatus::Completed { .. }));
    }

    #[tokio::test]
    async fn fail_run_works() {
        let tracker = RunTracker::new();
        let id = Uuid::new_v4();
        tracker.start_run(id).await.unwrap();
        tracker.fail_run(id, "oops".into()).await.unwrap();
        let status = tracker.get_run_status(id).await.unwrap();
        match status {
            abp_daemon::RunStatus::Failed { error } => assert_eq!(error, "oops"),
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn cancel_running() {
        let tracker = RunTracker::new();
        let id = Uuid::new_v4();
        tracker.start_run(id).await.unwrap();
        tracker.cancel_run(id).await.unwrap();
        let status = tracker.get_run_status(id).await.unwrap();
        assert!(matches!(status, abp_daemon::RunStatus::Cancelled));
    }

    #[tokio::test]
    async fn cancel_completed_errors() {
        let tracker = RunTracker::new();
        let id = Uuid::new_v4();
        tracker.start_run(id).await.unwrap();
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        tracker.complete_run(id, receipt).await.unwrap();
        assert!(tracker.cancel_run(id).await.is_err());
    }

    #[tokio::test]
    async fn cancel_unknown_errors() {
        let tracker = RunTracker::new();
        assert!(tracker.cancel_run(Uuid::new_v4()).await.is_err());
    }

    #[tokio::test]
    async fn remove_completed_run() {
        let tracker = RunTracker::new();
        let id = Uuid::new_v4();
        tracker.start_run(id).await.unwrap();
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        tracker.complete_run(id, receipt).await.unwrap();
        let removed = tracker.remove_run(id).await.unwrap();
        assert!(matches!(removed, abp_daemon::RunStatus::Completed { .. }));
        assert!(tracker.get_run_status(id).await.is_none());
    }

    #[tokio::test]
    async fn remove_running_errors() {
        let tracker = RunTracker::new();
        let id = Uuid::new_v4();
        tracker.start_run(id).await.unwrap();
        assert!(tracker.remove_run(id).await.is_err());
    }

    #[tokio::test]
    async fn remove_not_found_errors() {
        let tracker = RunTracker::new();
        assert!(tracker.remove_run(Uuid::new_v4()).await.is_err());
    }

    #[tokio::test]
    async fn get_unknown_returns_none() {
        let tracker = RunTracker::new();
        assert!(tracker.get_run_status(Uuid::new_v4()).await.is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 5 — Daemon: DaemonState
// ═══════════════════════════════════════════════════════════════════════════

mod daemon_state {
    use abp_daemon::DaemonState;
    use uuid::Uuid;

    #[tokio::test]
    async fn register_backend() {
        let state = DaemonState::new();
        state.register_backend("mock".into()).await;
        let names = state.backend_names().await;
        assert_eq!(names, vec!["mock"]);
    }

    #[tokio::test]
    async fn register_duplicate_backend_is_idempotent() {
        let state = DaemonState::new();
        state.register_backend("mock".into()).await;
        state.register_backend("mock".into()).await;
        let names = state.backend_names().await;
        assert_eq!(names.len(), 1);
    }

    #[tokio::test]
    async fn set_and_get_run_status() {
        let state = DaemonState::new();
        let id = Uuid::new_v4();
        let status = abp_daemon::handler::RunStatus {
            id,
            state: abp_daemon::handler::RunState::Running,
            receipt: None,
        };
        state.set_run_status(id, status).await;
        let s = state.get_run_status(id).await;
        assert!(s.is_some());
    }

    #[tokio::test]
    async fn run_ids_returns_all() {
        let state = DaemonState::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let s1 = abp_daemon::handler::RunStatus {
            id: id1,
            state: abp_daemon::handler::RunState::Running,
            receipt: None,
        };
        let s2 = abp_daemon::handler::RunStatus {
            id: id2,
            state: abp_daemon::handler::RunState::Pending,
            receipt: None,
        };
        state.set_run_status(id1, s1).await;
        state.set_run_status(id2, s2).await;
        let ids = state.run_ids().await;
        assert_eq!(ids.len(), 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 6 — Daemon: api.rs types
// ═══════════════════════════════════════════════════════════════════════════

mod daemon_api {
    use abp_daemon::api;

    #[test]
    fn api_run_status_serde_all_variants() {
        for status in [
            api::RunStatus::Queued,
            api::RunStatus::Running,
            api::RunStatus::Completed,
            api::RunStatus::Failed,
            api::RunStatus::Cancelled,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: api::RunStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back);
        }
    }

    #[test]
    fn api_run_status_is_terminal() {
        assert!(!api::RunStatus::Queued.is_terminal());
        assert!(!api::RunStatus::Running.is_terminal());
        assert!(api::RunStatus::Completed.is_terminal());
        assert!(api::RunStatus::Failed.is_terminal());
        assert!(api::RunStatus::Cancelled.is_terminal());
    }

    #[test]
    fn api_run_status_transitions_queued() {
        assert!(api::RunStatus::Queued.can_transition_to(api::RunStatus::Running));
        assert!(api::RunStatus::Queued.can_transition_to(api::RunStatus::Cancelled));
        assert!(!api::RunStatus::Queued.can_transition_to(api::RunStatus::Completed));
        assert!(!api::RunStatus::Queued.can_transition_to(api::RunStatus::Failed));
    }

    #[test]
    fn api_run_status_transitions_running() {
        assert!(api::RunStatus::Running.can_transition_to(api::RunStatus::Completed));
        assert!(api::RunStatus::Running.can_transition_to(api::RunStatus::Failed));
        assert!(api::RunStatus::Running.can_transition_to(api::RunStatus::Cancelled));
        assert!(!api::RunStatus::Running.can_transition_to(api::RunStatus::Queued));
    }

    #[test]
    fn api_run_status_terminal_no_transitions() {
        for status in [
            api::RunStatus::Completed,
            api::RunStatus::Failed,
            api::RunStatus::Cancelled,
        ] {
            assert!(status.valid_transitions().is_empty());
        }
    }

    #[test]
    fn api_error_not_found() {
        let err = api::ApiError::not_found("run 123 not found");
        assert_eq!(err.code, "not_found");
        assert_eq!(err.message, "run 123 not found");
        assert!(err.details.is_none());
    }

    #[test]
    fn api_error_invalid_request() {
        let err = api::ApiError::invalid_request("bad");
        assert_eq!(err.code, "invalid_request");
    }

    #[test]
    fn api_error_conflict() {
        let err = api::ApiError::conflict("dup");
        assert_eq!(err.code, "conflict");
    }

    #[test]
    fn api_error_internal() {
        let err = api::ApiError::internal("boom");
        assert_eq!(err.code, "internal_error");
    }

    #[test]
    fn api_error_with_details() {
        let err =
            api::ApiError::not_found("x").with_details(serde_json::json!({"field": "run_id"}));
        assert!(err.details.is_some());
        assert_eq!(err.details.unwrap()["field"], "run_id");
    }

    #[test]
    fn api_error_display() {
        let err = api::ApiError::not_found("gone");
        let s = err.to_string();
        assert!(s.contains("not_found"));
        assert!(s.contains("gone"));
    }

    #[test]
    fn api_error_serde_roundtrip() {
        let err = api::ApiError::new("custom_code", "custom msg");
        let json = serde_json::to_string(&err).unwrap();
        let back: api::ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, "custom_code");
        assert_eq!(back.message, "custom msg");
    }

    #[test]
    fn api_error_omits_null_details() {
        let err = api::ApiError::not_found("x");
        let val = serde_json::to_value(&err).unwrap();
        assert!(val.get("details").is_none());
    }

    #[test]
    fn health_response_serde_roundtrip() {
        let resp = api::HealthResponse {
            status: "ok".into(),
            version: "abp/v0.1".into(),
            uptime_secs: 42,
            backends: vec!["mock".into()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: api::HealthResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, "ok");
        assert_eq!(back.uptime_secs, 42);
    }

    #[test]
    fn error_response_serde_roundtrip() {
        let resp = api::ErrorResponse {
            error: "bad".into(),
            code: Some("invalid_request".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: api::ErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.error, "bad");
        assert_eq!(back.code.as_deref(), Some("invalid_request"));
    }

    #[test]
    fn error_response_omits_none_code() {
        let resp = api::ErrorResponse {
            error: "oops".into(),
            code: None,
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert!(val.get("code").is_none());
    }

    #[test]
    fn v1_run_request_serde() {
        let req = api::RunRequest {
            task: "hello world".into(),
            backend: Some("mock".into()),
            config: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: api::RunRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task, "hello world");
        assert_eq!(back.backend.as_deref(), Some("mock"));
    }

    #[test]
    fn v1_run_response_serde() {
        let resp = api::RunResponse {
            run_id: "abc-123".into(),
            status: api::RunStatus::Queued,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: api::RunResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run_id, "abc-123");
        assert_eq!(back.status, api::RunStatus::Queued);
    }

    #[test]
    fn backend_info_serde() {
        let info = api::BackendInfo {
            name: "mock".into(),
            dialect: "openai".into(),
            status: "available".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: api::BackendInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "mock");
        assert_eq!(back.dialect, "openai");
    }

    #[test]
    fn list_backends_response_serde() {
        let resp = api::ListBackendsResponse {
            backends: vec![api::BackendInfo {
                name: "test".into(),
                dialect: "claude".into(),
                status: "available".into(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: api::ListBackendsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.backends.len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 7 — Daemon: queue.rs
// ═══════════════════════════════════════════════════════════════════════════

mod daemon_queue {
    use abp_daemon::queue::*;
    use std::collections::BTreeMap;

    fn make_run(id: &str, pri: QueuePriority) -> QueuedRun {
        QueuedRun {
            id: id.into(),
            work_order_id: "wo-1".into(),
            priority: pri,
            queued_at: "2025-01-01T00:00:00Z".into(),
            backend: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn enqueue_and_dequeue() {
        let mut q = RunQueue::new(10);
        q.enqueue(make_run("r1", QueuePriority::Normal)).unwrap();
        assert_eq!(q.len(), 1);
        let run = q.dequeue().unwrap();
        assert_eq!(run.id, "r1");
        assert!(q.is_empty());
    }

    #[test]
    fn dequeue_empty_returns_none() {
        let mut q = RunQueue::new(10);
        assert!(q.dequeue().is_none());
    }

    #[test]
    fn priority_ordering() {
        let mut q = RunQueue::new(10);
        q.enqueue(make_run("low", QueuePriority::Low)).unwrap();
        q.enqueue(make_run("crit", QueuePriority::Critical))
            .unwrap();
        q.enqueue(make_run("high", QueuePriority::High)).unwrap();
        assert_eq!(q.dequeue().unwrap().id, "crit");
        assert_eq!(q.dequeue().unwrap().id, "high");
        assert_eq!(q.dequeue().unwrap().id, "low");
    }

    #[test]
    fn fifo_within_same_priority() {
        let mut q = RunQueue::new(10);
        q.enqueue(make_run("a", QueuePriority::Normal)).unwrap();
        q.enqueue(make_run("b", QueuePriority::Normal)).unwrap();
        q.enqueue(make_run("c", QueuePriority::Normal)).unwrap();
        assert_eq!(q.dequeue().unwrap().id, "a");
        assert_eq!(q.dequeue().unwrap().id, "b");
        assert_eq!(q.dequeue().unwrap().id, "c");
    }

    #[test]
    fn queue_full_error() {
        let mut q = RunQueue::new(1);
        q.enqueue(make_run("r1", QueuePriority::Normal)).unwrap();
        let err = q
            .enqueue(make_run("r2", QueuePriority::Normal))
            .unwrap_err();
        assert!(matches!(err, QueueError::Full { max: 1 }));
    }

    #[test]
    fn duplicate_id_error() {
        let mut q = RunQueue::new(10);
        q.enqueue(make_run("r1", QueuePriority::Normal)).unwrap();
        let err = q.enqueue(make_run("r1", QueuePriority::High)).unwrap_err();
        assert!(matches!(err, QueueError::DuplicateId(_)));
    }

    #[test]
    fn peek_without_removing() {
        let mut q = RunQueue::new(10);
        q.enqueue(make_run("r1", QueuePriority::Normal)).unwrap();
        assert_eq!(q.peek().unwrap().id, "r1");
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn remove_by_id() {
        let mut q = RunQueue::new(10);
        q.enqueue(make_run("r1", QueuePriority::Normal)).unwrap();
        q.enqueue(make_run("r2", QueuePriority::Normal)).unwrap();
        let removed = q.remove("r1").unwrap();
        assert_eq!(removed.id, "r1");
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut q = RunQueue::new(10);
        assert!(q.remove("nope").is_none());
    }

    #[test]
    fn clear_empties_queue() {
        let mut q = RunQueue::new(10);
        q.enqueue(make_run("r1", QueuePriority::Normal)).unwrap();
        q.enqueue(make_run("r2", QueuePriority::Normal)).unwrap();
        q.clear();
        assert!(q.is_empty());
    }

    #[test]
    fn is_full() {
        let mut q = RunQueue::new(2);
        q.enqueue(make_run("r1", QueuePriority::Normal)).unwrap();
        assert!(!q.is_full());
        q.enqueue(make_run("r2", QueuePriority::Normal)).unwrap();
        assert!(q.is_full());
    }

    #[test]
    fn by_priority_filters() {
        let mut q = RunQueue::new(10);
        q.enqueue(make_run("l1", QueuePriority::Low)).unwrap();
        q.enqueue(make_run("l2", QueuePriority::Low)).unwrap();
        q.enqueue(make_run("h1", QueuePriority::High)).unwrap();
        assert_eq!(q.by_priority(QueuePriority::Low).len(), 2);
        assert_eq!(q.by_priority(QueuePriority::High).len(), 1);
        assert_eq!(q.by_priority(QueuePriority::Critical).len(), 0);
    }

    #[test]
    fn stats_snapshot() {
        let mut q = RunQueue::new(100);
        q.enqueue(make_run("l1", QueuePriority::Low)).unwrap();
        q.enqueue(make_run("n1", QueuePriority::Normal)).unwrap();
        q.enqueue(make_run("h1", QueuePriority::High)).unwrap();
        let stats = q.stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.max, 100);
        assert_eq!(stats.by_priority["low"], 1);
        assert_eq!(stats.by_priority["normal"], 1);
        assert_eq!(stats.by_priority["high"], 1);
    }

    #[test]
    fn priority_serde_roundtrip() {
        for p in [
            QueuePriority::Low,
            QueuePriority::Normal,
            QueuePriority::High,
            QueuePriority::Critical,
        ] {
            let json = serde_json::to_string(&p).unwrap();
            let back: QueuePriority = serde_json::from_str(&json).unwrap();
            assert_eq!(p, back);
        }
    }

    #[test]
    fn priority_ordering_enum() {
        assert!(QueuePriority::Low < QueuePriority::Normal);
        assert!(QueuePriority::Normal < QueuePriority::High);
        assert!(QueuePriority::High < QueuePriority::Critical);
    }

    #[test]
    fn queue_error_display() {
        let e = QueueError::Full { max: 5 };
        assert!(e.to_string().contains("5"));
        let e2 = QueueError::DuplicateId("abc".into());
        assert!(e2.to_string().contains("abc"));
    }

    #[test]
    fn queued_run_serde_roundtrip() {
        let run = make_run("r1", QueuePriority::High);
        let json = serde_json::to_string(&run).unwrap();
        let back: QueuedRun = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "r1");
        assert_eq!(back.priority, QueuePriority::High);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 8 — Daemon: validation.rs
// ═══════════════════════════════════════════════════════════════════════════

mod daemon_validation {
    use abp_daemon::validation::RequestValidator;
    use uuid::Uuid;

    #[test]
    fn valid_uuid() {
        assert!(RequestValidator::validate_run_id(&Uuid::new_v4().to_string()).is_ok());
    }

    #[test]
    fn nil_uuid_accepted() {
        assert!(RequestValidator::validate_run_id(&Uuid::nil().to_string()).is_ok());
    }

    #[test]
    fn invalid_uuid_rejected() {
        assert!(RequestValidator::validate_run_id("not-a-uuid").is_err());
    }

    #[test]
    fn empty_uuid_rejected() {
        assert!(RequestValidator::validate_run_id("").is_err());
    }

    #[test]
    fn valid_backend_name() {
        let backends = vec!["mock".into(), "sidecar:node".into()];
        assert!(RequestValidator::validate_backend_name("mock", &backends).is_ok());
    }

    #[test]
    fn unknown_backend_rejected() {
        let backends = vec!["mock".into()];
        let err = RequestValidator::validate_backend_name("nope", &backends).unwrap_err();
        assert!(err.contains("unknown backend"));
    }

    #[test]
    fn empty_backend_rejected() {
        let backends = vec!["mock".into()];
        assert!(RequestValidator::validate_backend_name("", &backends).is_err());
    }

    #[test]
    fn config_must_be_object() {
        let config = serde_json::json!("string");
        assert!(RequestValidator::validate_config(&config).is_err());
    }

    #[test]
    fn array_config_rejected() {
        let config = serde_json::json!([1, 2]);
        assert!(RequestValidator::validate_config(&config).is_err());
    }

    #[test]
    fn valid_object_config() {
        let config = serde_json::json!({"key": "val"});
        assert!(RequestValidator::validate_config(&config).is_ok());
    }

    #[test]
    fn work_order_empty_task_rejected() {
        use abp_core::WorkOrderBuilder;
        let mut wo = WorkOrderBuilder::new("x").build();
        wo.task = "".into();
        assert!(RequestValidator::validate_work_order(&wo).is_err());
    }

    #[test]
    fn work_order_whitespace_task_rejected() {
        use abp_core::WorkOrderBuilder;
        let mut wo = WorkOrderBuilder::new("x").build();
        wo.task = "   ".into();
        assert!(RequestValidator::validate_work_order(&wo).is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 9 — Daemon: versioning.rs
// ═══════════════════════════════════════════════════════════════════════════

mod daemon_versioning {
    use abp_daemon::versioning::*;

    #[test]
    fn parse_v1() {
        let v = ApiVersion::parse("v1").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 0);
    }

    #[test]
    fn parse_v1_2() {
        let v = ApiVersion::parse("v1.2").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
    }

    #[test]
    fn parse_no_prefix() {
        let v = ApiVersion::parse("2.3").unwrap();
        assert_eq!(v.major, 2);
        assert_eq!(v.minor, 3);
    }

    #[test]
    fn parse_empty_errors() {
        assert!(ApiVersion::parse("").is_err());
        assert!(ApiVersion::parse("v").is_err());
    }

    #[test]
    fn parse_invalid_errors() {
        assert!(ApiVersion::parse("vx.1").is_err());
        assert!(ApiVersion::parse("v1.x").is_err());
    }

    #[test]
    fn display() {
        let v = ApiVersion { major: 1, minor: 3 };
        assert_eq!(v.to_string(), "v1.3");
    }

    #[test]
    fn compatible_same_major() {
        let a = ApiVersion { major: 1, minor: 0 };
        let b = ApiVersion { major: 1, minor: 5 };
        assert!(a.is_compatible(&b));
    }

    #[test]
    fn incompatible_different_major() {
        let a = ApiVersion { major: 1, minor: 0 };
        let b = ApiVersion { major: 2, minor: 0 };
        assert!(!a.is_compatible(&b));
    }

    #[test]
    fn ordering() {
        let a = ApiVersion { major: 1, minor: 0 };
        let b = ApiVersion { major: 1, minor: 5 };
        let c = ApiVersion { major: 2, minor: 0 };
        assert!(a < b);
        assert!(b < c);
    }

    #[test]
    fn serde_roundtrip() {
        let v = ApiVersion { major: 1, minor: 2 };
        let json = serde_json::to_string(&v).unwrap();
        let back: ApiVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn version_error_display() {
        let e = ApiVersionError::InvalidFormat("bad".into());
        assert!(e.to_string().contains("bad"));
        let e2 = ApiVersionError::UnsupportedVersion(ApiVersion { major: 9, minor: 0 });
        assert!(e2.to_string().contains("v9.0"));
    }

    #[test]
    fn negotiate_picks_highest_compatible() {
        let requested = ApiVersion { major: 1, minor: 5 };
        let supported = vec![
            ApiVersion { major: 1, minor: 0 },
            ApiVersion { major: 1, minor: 3 },
            ApiVersion { major: 2, minor: 0 },
        ];
        let result = VersionNegotiator::negotiate(&requested, &supported);
        assert_eq!(result, Some(ApiVersion { major: 1, minor: 3 }));
    }

    #[test]
    fn negotiate_no_compatible() {
        let requested = ApiVersion { major: 3, minor: 0 };
        let supported = vec![
            ApiVersion { major: 1, minor: 0 },
            ApiVersion { major: 2, minor: 0 },
        ];
        assert!(VersionNegotiator::negotiate(&requested, &supported).is_none());
    }

    #[test]
    fn negotiate_exact_match() {
        let v = ApiVersion { major: 1, minor: 2 };
        let result = VersionNegotiator::negotiate(&v, &[v]);
        assert_eq!(result, Some(v));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 10 — Daemon: routes.rs
// ═══════════════════════════════════════════════════════════════════════════

mod daemon_routes {
    use abp_daemon::routes::*;

    #[test]
    fn api_routes_returns_six() {
        let routes = api_routes();
        assert_eq!(routes.len(), 6);
    }

    #[test]
    fn api_routes_contain_health() {
        let routes = api_routes();
        assert!(routes.iter().any(|r| r.path.contains("health")));
    }

    #[test]
    fn route_serde_roundtrip() {
        let route = Route {
            method: "GET".into(),
            path: "/api/v1/health".into(),
            description: "Health check".into(),
        };
        let json = serde_json::to_string(&route).unwrap();
        let back: Route = serde_json::from_str(&json).unwrap();
        assert_eq!(back.method, "GET");
        assert_eq!(back.path, "/api/v1/health");
    }

    #[test]
    fn route_error_codes() {
        assert_eq!(RouteError::bad_request("x").code, "bad_request");
        assert_eq!(RouteError::not_found("x").code, "not_found");
        assert_eq!(RouteError::conflict("x").code, "conflict");
        assert_eq!(RouteError::internal("x").code, "internal_error");
    }

    #[test]
    fn route_error_statuses() {
        assert_eq!(RouteError::bad_request("x").status, 400);
        assert_eq!(RouteError::not_found("x").status, 404);
        assert_eq!(RouteError::conflict("x").status, 409);
        assert_eq!(RouteError::internal("x").status, 500);
    }

    #[test]
    fn method_display() {
        assert_eq!(Method::Get.to_string(), "GET");
        assert_eq!(Method::Post.to_string(), "POST");
        assert_eq!(Method::Delete.to_string(), "DELETE");
    }

    #[test]
    fn route_table_health() {
        let table = RouteTable::new("/api/v1");
        let m = table.match_route(Method::Get, "/api/v1/health");
        assert_eq!(m, MatchResult::Matched(Endpoint::Health));
    }

    #[test]
    fn route_table_backends() {
        let table = RouteTable::new("/api/v1");
        let m = table.match_route(Method::Get, "/api/v1/backends");
        assert_eq!(m, MatchResult::Matched(Endpoint::ListBackends));
    }

    #[test]
    fn route_table_submit_run() {
        let table = RouteTable::new("/api/v1");
        let m = table.match_route(Method::Post, "/api/v1/runs");
        assert_eq!(m, MatchResult::Matched(Endpoint::SubmitRun));
    }

    #[test]
    fn route_table_get_run() {
        let table = RouteTable::new("/api/v1");
        let m = table.match_route(Method::Get, "/api/v1/runs/abc-123");
        assert_eq!(
            m,
            MatchResult::Matched(Endpoint::GetRun {
                run_id: "abc-123".into()
            })
        );
    }

    #[test]
    fn route_table_delete_run() {
        let table = RouteTable::new("/api/v1");
        let m = table.match_route(Method::Delete, "/api/v1/runs/abc");
        assert_eq!(
            m,
            MatchResult::Matched(Endpoint::DeleteRun {
                run_id: "abc".into()
            })
        );
    }

    #[test]
    fn route_table_cancel_run() {
        let table = RouteTable::new("/api/v1");
        let m = table.match_route(Method::Post, "/api/v1/runs/abc/cancel");
        assert_eq!(
            m,
            MatchResult::Matched(Endpoint::CancelRun {
                run_id: "abc".into()
            })
        );
    }

    #[test]
    fn route_table_get_events() {
        let table = RouteTable::new("/api/v1");
        let m = table.match_route(Method::Get, "/api/v1/runs/abc/events");
        assert_eq!(
            m,
            MatchResult::Matched(Endpoint::GetRunEvents {
                run_id: "abc".into()
            })
        );
    }

    #[test]
    fn route_table_not_found() {
        let table = RouteTable::new("/api/v1");
        let m = table.match_route(Method::Get, "/api/v1/nonexistent");
        assert_eq!(m, MatchResult::NotFound);
    }

    #[test]
    fn route_table_method_not_allowed() {
        let table = RouteTable::new("/api/v1");
        let m = table.match_route(Method::Post, "/api/v1/health");
        assert_eq!(m, MatchResult::MethodNotAllowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 11 — Daemon: state.rs
// ═══════════════════════════════════════════════════════════════════════════

mod daemon_state_module {
    use abp_daemon::state::*;
    use uuid::Uuid;

    #[test]
    fn run_phase_is_terminal() {
        assert!(!RunPhase::Queued.is_terminal());
        assert!(!RunPhase::Running.is_terminal());
        assert!(RunPhase::Completed.is_terminal());
        assert!(RunPhase::Failed.is_terminal());
        assert!(RunPhase::Cancelled.is_terminal());
    }

    #[test]
    fn run_phase_transitions() {
        assert!(RunPhase::Queued.can_transition_to(RunPhase::Running));
        assert!(RunPhase::Queued.can_transition_to(RunPhase::Cancelled));
        assert!(!RunPhase::Queued.can_transition_to(RunPhase::Completed));
        assert!(RunPhase::Running.can_transition_to(RunPhase::Completed));
        assert!(RunPhase::Running.can_transition_to(RunPhase::Failed));
        assert!(!RunPhase::Completed.can_transition_to(RunPhase::Running));
    }

    #[test]
    fn run_phase_serde_roundtrip() {
        for phase in [
            RunPhase::Queued,
            RunPhase::Running,
            RunPhase::Completed,
            RunPhase::Failed,
            RunPhase::Cancelled,
        ] {
            let json = serde_json::to_string(&phase).unwrap();
            let back: RunPhase = serde_json::from_str(&json).unwrap();
            assert_eq!(phase, back);
        }
    }

    #[tokio::test]
    async fn registry_create_and_get() {
        let reg = RunRegistry::new();
        let id = Uuid::new_v4();
        reg.create_run(id, "mock".into()).await.unwrap();
        let record = reg.get(id).await.unwrap();
        assert_eq!(record.phase, RunPhase::Queued);
        assert_eq!(record.backend, "mock");
    }

    #[tokio::test]
    async fn registry_duplicate_id_errors() {
        let reg = RunRegistry::new();
        let id = Uuid::new_v4();
        reg.create_run(id, "mock".into()).await.unwrap();
        let err = reg.create_run(id, "mock".into()).await.unwrap_err();
        assert!(matches!(err, RegistryError::DuplicateId(_)));
    }

    #[tokio::test]
    async fn registry_transition() {
        let reg = RunRegistry::new();
        let id = Uuid::new_v4();
        reg.create_run(id, "mock".into()).await.unwrap();
        reg.transition(id, RunPhase::Running).await.unwrap();
        assert_eq!(reg.get(id).await.unwrap().phase, RunPhase::Running);
    }

    #[tokio::test]
    async fn registry_invalid_transition_errors() {
        let reg = RunRegistry::new();
        let id = Uuid::new_v4();
        reg.create_run(id, "mock".into()).await.unwrap();
        let err = reg.transition(id, RunPhase::Completed).await.unwrap_err();
        assert!(matches!(err, RegistryError::InvalidTransition { .. }));
    }

    #[tokio::test]
    async fn registry_complete() {
        use abp_core::{Outcome, ReceiptBuilder};
        let reg = RunRegistry::new();
        let id = Uuid::new_v4();
        reg.create_run(id, "mock".into()).await.unwrap();
        reg.transition(id, RunPhase::Running).await.unwrap();
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        reg.complete(id, receipt).await.unwrap();
        let rec = reg.get(id).await.unwrap();
        assert_eq!(rec.phase, RunPhase::Completed);
        assert!(rec.receipt.is_some());
    }

    #[tokio::test]
    async fn registry_fail() {
        let reg = RunRegistry::new();
        let id = Uuid::new_v4();
        reg.create_run(id, "mock".into()).await.unwrap();
        reg.transition(id, RunPhase::Running).await.unwrap();
        reg.fail(id, "oops".into()).await.unwrap();
        let rec = reg.get(id).await.unwrap();
        assert_eq!(rec.phase, RunPhase::Failed);
        assert_eq!(rec.error.as_deref(), Some("oops"));
    }

    #[tokio::test]
    async fn registry_cancel() {
        let reg = RunRegistry::new();
        let id = Uuid::new_v4();
        reg.create_run(id, "mock".into()).await.unwrap();
        reg.cancel(id).await.unwrap();
        assert_eq!(reg.get(id).await.unwrap().phase, RunPhase::Cancelled);
    }

    #[tokio::test]
    async fn registry_push_event() {
        use abp_core::{AgentEvent, AgentEventKind};
        let reg = RunRegistry::new();
        let id = Uuid::new_v4();
        reg.create_run(id, "mock".into()).await.unwrap();
        let event = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: None,
        };
        let count = reg.push_event(id, event).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn registry_list_and_count() {
        let reg = RunRegistry::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        reg.create_run(id1, "mock".into()).await.unwrap();
        reg.create_run(id2, "mock".into()).await.unwrap();
        assert_eq!(reg.len().await, 2);
        assert!(!reg.is_empty().await);
        assert_eq!(reg.count_by_phase(RunPhase::Queued).await, 2);
    }

    #[tokio::test]
    async fn registry_remove_terminal() {
        let reg = RunRegistry::new();
        let id = Uuid::new_v4();
        reg.create_run(id, "mock".into()).await.unwrap();
        reg.cancel(id).await.unwrap();
        let removed = reg.remove(id).await.unwrap();
        assert_eq!(removed.phase, RunPhase::Cancelled);
        assert!(reg.get(id).await.is_none());
    }

    #[tokio::test]
    async fn registry_remove_active_errors() {
        let reg = RunRegistry::new();
        let id = Uuid::new_v4();
        reg.create_run(id, "mock".into()).await.unwrap();
        assert!(reg.remove(id).await.is_err());
    }

    #[tokio::test]
    async fn backend_list_operations() {
        let bl = BackendList::new();
        assert!(bl.is_empty().await);
        bl.register("mock".into()).await;
        bl.register("mock".into()).await; // duplicate
        assert_eq!(bl.len().await, 1);
        assert!(bl.contains("mock").await);
        assert!(!bl.contains("nope").await);
        let names = bl.list().await;
        assert_eq!(names, vec!["mock"]);
    }

    #[tokio::test]
    async fn backend_list_from_names() {
        let bl = BackendList::from_names(vec!["a".into(), "b".into()]);
        assert_eq!(bl.len().await, 2);
    }

    #[test]
    fn server_state_uptime() {
        let state = ServerState::new(vec!["mock".into()]);
        // Uptime should be zero or very small immediately after creation.
        assert!(state.uptime_secs() < 2);
    }

    #[test]
    fn registry_error_display() {
        let id = Uuid::nil();
        let e = RegistryError::NotFound(id);
        assert!(e.to_string().contains(&id.to_string()));
        let e2 = RegistryError::DuplicateId(id);
        assert!(e2.to_string().contains("already exists"));
        let e3 = RegistryError::InvalidTransition {
            run_id: id,
            from: RunPhase::Queued,
            to: RunPhase::Completed,
        };
        assert!(e3.to_string().contains("invalid transition"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 12 — Daemon: server.rs
// ═══════════════════════════════════════════════════════════════════════════

mod daemon_server {
    use abp_daemon::server::VersionResponse;

    #[test]
    fn version_response_serde_roundtrip() {
        let resp = VersionResponse {
            version: "0.1.0".into(),
            contract_version: "abp/v0.1".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: VersionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, "0.1.0");
        assert_eq!(back.contract_version, "abp/v0.1");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 13 — Daemon: lib.rs top-level types
// ═══════════════════════════════════════════════════════════════════════════

mod daemon_lib_types {
    use abp_daemon::*;

    #[test]
    fn run_metrics_serde_roundtrip() {
        let m = RunMetrics {
            total_runs: 10,
            running: 2,
            completed: 6,
            failed: 2,
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: RunMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total_runs, 10);
        assert_eq!(back.running, 2);
    }

    #[test]
    fn status_response_serde_roundtrip() {
        let s = StatusResponse {
            status: "ok".into(),
            contract_version: abp_core::CONTRACT_VERSION.into(),
            backends: vec!["mock".into()],
            active_runs: vec![],
            total_runs: 5,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: StatusResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, "ok");
        assert_eq!(back.total_runs, 5);
    }

    #[test]
    fn validation_response_valid() {
        let v = ValidationResponse {
            valid: true,
            errors: vec![],
        };
        let val = serde_json::to_value(&v).unwrap();
        assert_eq!(val["valid"], true);
        // errors should be omitted when empty
        assert!(val.get("errors").is_none());
    }

    #[test]
    fn validation_response_invalid() {
        let v = ValidationResponse {
            valid: false,
            errors: vec!["task empty".into()],
        };
        let val = serde_json::to_value(&v).unwrap();
        assert_eq!(val["valid"], false);
        assert_eq!(val["errors"][0], "task empty");
    }

    #[test]
    fn backend_info_serde() {
        let b = BackendInfo {
            id: "mock".into(),
            capabilities: std::collections::BTreeMap::new(),
        };
        let json = serde_json::to_string(&b).unwrap();
        let back: BackendInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "mock");
    }

    #[test]
    fn receipt_list_query_with_limit() {
        let json = r#"{"limit": 10}"#;
        let q: ReceiptListQuery = serde_json::from_str(json).unwrap();
        assert_eq!(q.limit, Some(10));
    }

    #[test]
    fn receipt_list_query_without_limit() {
        let json = r#"{}"#;
        let q: ReceiptListQuery = serde_json::from_str(json).unwrap();
        assert!(q.limit.is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 14 — SDK Types: Dialect
// ═══════════════════════════════════════════════════════════════════════════

mod sdk_types_dialect {
    use abp_sdk_types::Dialect;

    #[test]
    fn all_returns_six() {
        assert_eq!(Dialect::all().len(), 6);
    }

    #[test]
    fn labels() {
        assert_eq!(Dialect::OpenAi.label(), "OpenAI");
        assert_eq!(Dialect::Claude.label(), "Claude");
        assert_eq!(Dialect::Gemini.label(), "Gemini");
        assert_eq!(Dialect::Kimi.label(), "Kimi");
        assert_eq!(Dialect::Codex.label(), "Codex");
        assert_eq!(Dialect::Copilot.label(), "Copilot");
    }

    #[test]
    fn display_matches_label() {
        for d in Dialect::all() {
            assert_eq!(d.to_string(), d.label());
        }
    }

    #[test]
    fn serde_roundtrip_all() {
        for d in Dialect::all() {
            let json = serde_json::to_string(d).unwrap();
            let back: Dialect = serde_json::from_str(&json).unwrap();
            assert_eq!(*d, back);
        }
    }

    #[test]
    fn serde_snake_case() {
        assert_eq!(
            serde_json::to_string(&Dialect::OpenAi).unwrap(),
            "\"open_ai\""
        );
        assert_eq!(
            serde_json::to_string(&Dialect::Claude).unwrap(),
            "\"claude\""
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 15 — SDK Types: ModelConfig
// ═══════════════════════════════════════════════════════════════════════════

mod sdk_types_model_config {
    use abp_sdk_types::ModelConfig;
    use std::collections::BTreeMap;

    #[test]
    fn default_is_empty() {
        let cfg = ModelConfig::default();
        assert!(cfg.model.is_empty());
        assert!(cfg.max_tokens.is_none());
        assert!(cfg.temperature.is_none());
        assert!(cfg.top_p.is_none());
        assert!(cfg.stop_sequences.is_none());
        assert!(cfg.extra.is_empty());
    }

    #[test]
    fn serde_roundtrip() {
        let cfg = ModelConfig {
            model: "gpt-4o".into(),
            max_tokens: Some(4096),
            temperature: Some(0.7),
            top_p: Some(0.9),
            stop_sequences: Some(vec!["STOP".into()]),
            extra: BTreeMap::new(),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: ModelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn omits_none_fields() {
        let cfg = ModelConfig {
            model: "test".into(),
            ..ModelConfig::default()
        };
        let val = serde_json::to_value(&cfg).unwrap();
        assert!(val.get("max_tokens").is_none());
        assert!(val.get("temperature").is_none());
        assert!(val.get("extra").is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 16 — SDK Types: CanonicalToolDef
// ═══════════════════════════════════════════════════════════════════════════

mod sdk_types_canonical_tool {
    use abp_sdk_types::CanonicalToolDef;

    #[test]
    fn serde_roundtrip() {
        let def = CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_string(&def).unwrap();
        let back: CanonicalToolDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 17 — SDK Types: DialectRequest / Response / StreamChunk
// ═══════════════════════════════════════════════════════════════════════════

mod sdk_types_dialect_enums {
    use abp_sdk_types::*;

    #[test]
    fn dialect_request_openai_dialect() {
        let req = DialectRequest::OpenAi(openai::OpenAiRequest {
            model: "gpt-4o".into(),
            messages: vec![],
            tools: None,
            tool_choice: None,
            temperature: None,
            max_tokens: None,
            response_format: None,
            stream: None,
        });
        assert_eq!(req.dialect(), Dialect::OpenAi);
        assert_eq!(req.model(), "gpt-4o");
    }

    #[test]
    fn dialect_request_claude_dialect() {
        let req = DialectRequest::Claude(claude::ClaudeRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            system: None,
            messages: vec![],
            tools: None,
            thinking: None,
            stream: None,
        });
        assert_eq!(req.dialect(), Dialect::Claude);
        assert_eq!(req.model(), "claude-sonnet-4-20250514");
    }

    #[test]
    fn dialect_request_gemini_dialect() {
        let req = DialectRequest::Gemini(gemini::GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        });
        assert_eq!(req.dialect(), Dialect::Gemini);
    }

    #[test]
    fn dialect_response_openai_dialect() {
        let resp = DialectResponse::OpenAi(openai::OpenAiResponse {
            id: "x".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![],
            usage: None,
        });
        assert_eq!(resp.dialect(), Dialect::OpenAi);
    }

    #[test]
    fn dialect_stream_chunk_copilot() {
        let chunk = DialectStreamChunk::Copilot(copilot::CopilotStreamEvent::Done {});
        assert_eq!(chunk.dialect(), Dialect::Copilot);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 18 — SDK Types: common.rs
// ═══════════════════════════════════════════════════════════════════════════

mod sdk_types_common {
    use abp_sdk_types::common::*;

    #[test]
    fn role_serde_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant, Role::Tool] {
            let json = serde_json::to_string(&role).unwrap();
            let back: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(role, back);
        }
    }

    #[test]
    fn role_display() {
        assert_eq!(Role::System.to_string(), "system");
        assert_eq!(Role::User.to_string(), "user");
        assert_eq!(Role::Assistant.to_string(), "assistant");
        assert_eq!(Role::Tool.to_string(), "tool");
    }

    #[test]
    fn token_usage_default() {
        let u = TokenUsage::default();
        assert!(u.input_tokens.is_none());
        assert!(u.output_tokens.is_none());
        assert!(u.total_tokens.is_none());
    }

    #[test]
    fn token_usage_serde() {
        let u = TokenUsage {
            input_tokens: Some(100),
            output_tokens: Some(50),
            total_tokens: Some(150),
        };
        let json = serde_json::to_string(&u).unwrap();
        let back: TokenUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(u, back);
    }

    #[test]
    fn token_usage_omits_none() {
        let u = TokenUsage::default();
        let val = serde_json::to_value(&u).unwrap();
        assert!(val.get("input_tokens").is_none());
    }

    #[test]
    fn finish_reason_serde_all() {
        for reason in [
            FinishReason::Stop,
            FinishReason::ToolUse,
            FinishReason::MaxTokens,
            FinishReason::StopSequence,
            FinishReason::ContentFilter,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            let back: FinishReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, back);
        }
    }

    #[test]
    fn finish_reason_snake_case() {
        assert_eq!(
            serde_json::to_string(&FinishReason::ToolUse).unwrap(),
            "\"tool_use\""
        );
        assert_eq!(
            serde_json::to_string(&FinishReason::MaxTokens).unwrap(),
            "\"max_tokens\""
        );
        assert_eq!(
            serde_json::to_string(&FinishReason::ContentFilter).unwrap(),
            "\"content_filter\""
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 19 — SDK Types: convert.rs
// ═══════════════════════════════════════════════════════════════════════════

mod sdk_types_convert {
    use abp_sdk_types::convert::*;
    use abp_sdk_types::Dialect;

    #[test]
    fn message_serde_roundtrip() {
        let msg = Message {
            role: "user".into(),
            content: Some("hello".into()),
            tool_call_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn message_omits_none_fields() {
        let msg = Message {
            role: "user".into(),
            content: None,
            tool_call_id: None,
        };
        let val = serde_json::to_value(&msg).unwrap();
        assert!(val.get("content").is_none());
        assert!(val.get("tool_call_id").is_none());
    }

    #[test]
    fn tool_definition_serde_roundtrip() {
        let tool = ToolDefinition {
            name: "search".into(),
            description: "Search".into(),
            parameters: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let back: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, back);
    }

    #[test]
    fn conversion_error_unsupported_serde() {
        let err = ConversionError::UnsupportedField {
            field: "system".into(),
            dialect: Dialect::Claude,
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ConversionError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn conversion_error_incompatible_serde() {
        let err = ConversionError::IncompatibleType {
            source_type: "a".into(),
            target_type: "b".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ConversionError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn conversion_error_missing_serde() {
        let err = ConversionError::MissingRequiredField {
            field: "role".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ConversionError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn conversion_error_too_long_serde() {
        let err = ConversionError::ContentTooLong {
            max: 100,
            actual: 200,
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ConversionError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn conversion_report_new() {
        let r = ConversionReport::new(Dialect::OpenAi, Dialect::Claude);
        assert!(r.is_ok());
        assert!(r.is_lossless);
        assert_eq!(r.conversions, 0);
    }

    #[test]
    fn conversion_report_with_errors_not_ok() {
        let mut r = ConversionReport::new(Dialect::OpenAi, Dialect::Gemini);
        r.errors
            .push(ConversionError::MissingRequiredField { field: "x".into() });
        assert!(!r.is_ok());
    }

    #[test]
    fn conversion_report_serde_roundtrip() {
        let r = ConversionReport::new(Dialect::Claude, Dialect::Kimi);
        let json = serde_json::to_string(&r).unwrap();
        let back: ConversionReport = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn role_mapper_openai_to_gemini() {
        assert_eq!(
            RoleMapper::map_role("assistant", Dialect::OpenAi, Dialect::Gemini).unwrap(),
            "model"
        );
        assert_eq!(
            RoleMapper::map_role("user", Dialect::OpenAi, Dialect::Gemini).unwrap(),
            "user"
        );
    }

    #[test]
    fn role_mapper_gemini_to_openai() {
        assert_eq!(
            RoleMapper::map_role("model", Dialect::Gemini, Dialect::OpenAi).unwrap(),
            "assistant"
        );
    }

    #[test]
    fn role_mapper_system_to_claude_fails() {
        let err = RoleMapper::map_role("system", Dialect::OpenAi, Dialect::Claude).unwrap_err();
        assert!(matches!(err, ConversionError::UnsupportedField { .. }));
    }

    #[test]
    fn role_mapper_tool_to_gemini_fails() {
        let err = RoleMapper::map_role("tool", Dialect::OpenAi, Dialect::Gemini).unwrap_err();
        assert!(matches!(err, ConversionError::UnsupportedField { .. }));
    }

    #[test]
    fn role_mapper_unknown_role_fails() {
        let err = RoleMapper::map_role("narrator", Dialect::OpenAi, Dialect::OpenAi).unwrap_err();
        assert!(matches!(err, ConversionError::IncompatibleType { .. }));
    }

    #[test]
    fn role_mapper_kimi_same_as_openai() {
        assert_eq!(
            RoleMapper::map_role("system", Dialect::Kimi, Dialect::Kimi).unwrap(),
            "system"
        );
        assert_eq!(
            RoleMapper::map_role("tool", Dialect::Kimi, Dialect::Codex).unwrap(),
            "tool"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 20 — SDK Types: vendor configs default
// ═══════════════════════════════════════════════════════════════════════════

mod sdk_types_configs {
    use abp_sdk_types::{claude, codex, copilot, gemini, kimi, openai};

    #[test]
    fn openai_config_default() {
        let cfg = openai::OpenAiConfig::default();
        assert!(cfg.base_url.contains("openai.com"));
        assert_eq!(cfg.model, "gpt-4o");
    }

    #[test]
    fn claude_config_default() {
        let cfg = claude::ClaudeConfig::default();
        assert!(cfg.base_url.contains("anthropic.com"));
        assert!(cfg.model.contains("claude"));
        assert_eq!(cfg.max_tokens, 4096);
    }

    #[test]
    fn gemini_config_default() {
        let cfg = gemini::GeminiConfig::default();
        assert!(cfg.base_url.contains("googleapis.com"));
        assert!(cfg.model.contains("gemini"));
    }

    #[test]
    fn kimi_config_default() {
        let cfg = kimi::KimiConfig::default();
        assert!(cfg.base_url.contains("moonshot.cn"));
        assert!(cfg.model.contains("moonshot"));
    }

    #[test]
    fn codex_config_default() {
        let cfg = codex::CodexConfig::default();
        assert!(cfg.model.contains("codex"));
    }

    #[test]
    fn copilot_config_default() {
        let cfg = copilot::CopilotConfig::default();
        assert!(cfg.base_url.contains("githubcopilot"));
        assert_eq!(cfg.model, "gpt-4o");
    }

    #[test]
    fn codex_sandbox_config_default() {
        let cfg = codex::SandboxConfig::default();
        assert_eq!(cfg.networking, codex::NetworkAccess::None);
        assert_eq!(cfg.file_access, codex::FileAccess::WorkspaceOnly);
        assert!(cfg.timeout_seconds.is_some());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 21 — SDK Types: vendor-specific serde roundtrips
// ═══════════════════════════════════════════════════════════════════════════

mod sdk_types_vendor_serde {
    use abp_sdk_types::*;

    #[test]
    fn claude_content_block_text_serde() {
        let block = claude::ClaudeContentBlock::Text { text: "hi".into() };
        let json = serde_json::to_string(&block).unwrap();
        let back: claude::ClaudeContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn claude_content_block_tool_use_serde() {
        let block = claude::ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read_file".into(),
            input: serde_json::json!({"path": "x"}),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: claude::ClaudeContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn claude_content_block_thinking_serde() {
        let block = claude::ClaudeContentBlock::Thinking {
            thinking: "hmm".into(),
            signature: Some("sig".into()),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: claude::ClaudeContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn gemini_function_calling_mode_serde() {
        for mode in [
            gemini::FunctionCallingMode::Auto,
            gemini::FunctionCallingMode::Any,
            gemini::FunctionCallingMode::None,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: gemini::FunctionCallingMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }

    #[test]
    fn gemini_function_calling_mode_screaming_snake() {
        assert_eq!(
            serde_json::to_string(&gemini::FunctionCallingMode::Auto).unwrap(),
            "\"AUTO\""
        );
    }

    #[test]
    fn gemini_harm_category_serde() {
        let cat = gemini::HarmCategory::HarmCategoryHarassment;
        let json = serde_json::to_string(&cat).unwrap();
        let back: gemini::HarmCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(cat, back);
    }

    #[test]
    fn copilot_reference_type_serde() {
        for rt in [
            copilot::CopilotReferenceType::File,
            copilot::CopilotReferenceType::Snippet,
            copilot::CopilotReferenceType::Repository,
            copilot::CopilotReferenceType::WebSearchResult,
        ] {
            let json = serde_json::to_string(&rt).unwrap();
            let back: copilot::CopilotReferenceType = serde_json::from_str(&json).unwrap();
            assert_eq!(rt, back);
        }
    }

    #[test]
    fn copilot_tool_type_serde() {
        for tt in [
            copilot::CopilotToolType::Function,
            copilot::CopilotToolType::Confirmation,
        ] {
            let json = serde_json::to_string(&tt).unwrap();
            let back: copilot::CopilotToolType = serde_json::from_str(&json).unwrap();
            assert_eq!(tt, back);
        }
    }

    #[test]
    fn copilot_confirmation_serde() {
        let c = copilot::CopilotConfirmation {
            id: "c1".into(),
            title: "Confirm".into(),
            message: "Do it?".into(),
            accepted: Some(true),
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: copilot::CopilotConfirmation = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn copilot_error_serde() {
        let e = copilot::CopilotError {
            error_type: "rate_limit".into(),
            message: "slow down".into(),
            code: Some("429".into()),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: copilot::CopilotError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn codex_input_item_message_serde() {
        let item = codex::CodexInputItem::Message {
            role: "user".into(),
            content: "hi".into(),
        };
        let json = serde_json::to_string(&item).unwrap();
        let back: codex::CodexInputItem = serde_json::from_str(&json).unwrap();
        assert_eq!(item, back);
    }

    #[test]
    fn codex_tool_code_interpreter_serde() {
        let tool = codex::CodexTool::CodeInterpreter {};
        let json = serde_json::to_string(&tool).unwrap();
        let back: codex::CodexTool = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, back);
    }

    #[test]
    fn codex_tool_file_search_serde() {
        let tool = codex::CodexTool::FileSearch {
            max_num_results: Some(10),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let back: codex::CodexTool = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, back);
    }

    #[test]
    fn codex_network_access_default() {
        assert_eq!(codex::NetworkAccess::default(), codex::NetworkAccess::None);
    }

    #[test]
    fn codex_file_access_default() {
        assert_eq!(
            codex::FileAccess::default(),
            codex::FileAccess::WorkspaceOnly
        );
    }

    #[test]
    fn codex_text_format_json_schema_serde() {
        let fmt = codex::CodexTextFormat::JsonSchema {
            name: "output".into(),
            schema: serde_json::json!({"type": "object"}),
            strict: true,
        };
        let json = serde_json::to_string(&fmt).unwrap();
        let back: codex::CodexTextFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(fmt, back);
    }

    #[test]
    fn kimi_builtin_tool_serde() {
        let tool = kimi::KimiTool::BuiltinFunction {
            function: kimi::KimiBuiltinFunction {
                name: "$web_search".into(),
            },
        };
        let json = serde_json::to_string(&tool).unwrap();
        let back: kimi::KimiTool = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, back);
    }

    #[test]
    fn kimi_ref_serde() {
        let r = kimi::KimiRef {
            index: 1,
            url: "https://example.com".into(),
            title: Some("Example".into()),
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: kimi::KimiRef = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn openai_response_format_json_schema() {
        let fmt = openai::ResponseFormat::JsonSchema {
            json_schema: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_string(&fmt).unwrap();
        let back: openai::ResponseFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(fmt, back);
    }

    #[test]
    fn openai_tool_choice_mode_serde() {
        for mode in [
            openai::ToolChoiceMode::None,
            openai::ToolChoiceMode::Auto,
            openai::ToolChoiceMode::Required,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: openai::ToolChoiceMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }

    #[test]
    fn copilot_stream_event_references_serde() {
        let event = copilot::CopilotStreamEvent::CopilotReferences {
            references: vec![copilot::CopilotReference {
                ref_type: copilot::CopilotReferenceType::File,
                id: "f1".into(),
                data: serde_json::json!({}),
                metadata: None,
            }],
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: copilot::CopilotStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn codex_stream_event_error_serde() {
        let event = codex::CodexStreamEvent::Error {
            message: "boom".into(),
            code: Some("500".into()),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: codex::CodexStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn codex_response_item_reasoning_serde() {
        let item = codex::CodexResponseItem::Reasoning {
            summary: vec![codex::ReasoningSummary {
                text: "I think...".into(),
            }],
        };
        let json = serde_json::to_string(&item).unwrap();
        let back: codex::CodexResponseItem = serde_json::from_str(&json).unwrap();
        assert_eq!(item, back);
    }

    #[test]
    fn claude_stream_delta_all_variants() {
        let variants: Vec<claude::ClaudeStreamDelta> = vec![
            claude::ClaudeStreamDelta::TextDelta { text: "hi".into() },
            claude::ClaudeStreamDelta::InputJsonDelta {
                partial_json: "{}".into(),
            },
            claude::ClaudeStreamDelta::ThinkingDelta {
                thinking: "hmm".into(),
            },
            claude::ClaudeStreamDelta::SignatureDelta {
                signature: "sig".into(),
            },
        ];
        for v in variants {
            let json = serde_json::to_string(&v).unwrap();
            let back: claude::ClaudeStreamDelta = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Part 22 — Daemon: persist and hydrate receipts
// ═══════════════════════════════════════════════════════════════════════════

mod daemon_receipt_persistence {
    use abp_core::{Outcome, ReceiptBuilder};
    use abp_daemon::{hydrate_receipts_from_disk, persist_receipt};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn persist_and_hydrate_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        persist_receipt(dir.path(), &receipt).await.unwrap();

        let cache = Arc::new(RwLock::new(HashMap::new()));
        hydrate_receipts_from_disk(&cache, dir.path())
            .await
            .unwrap();
        let guard = cache.read().await;
        assert_eq!(guard.len(), 1);
        assert!(guard.contains_key(&receipt.meta.run_id));
    }
}
