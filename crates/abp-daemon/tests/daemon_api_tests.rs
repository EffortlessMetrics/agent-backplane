// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the daemon API types, state management, and route
//! matching.

use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, WorkOrderBuilder};
use abp_daemon::api::{
    ApiError as ApiApiError, ApiRequest, ApiResponse, BackendDetail, HealthResponse as ApiHealth,
    RunInfo, RunStatus as ApiRunStatus,
};
use abp_daemon::handler::{
    BackendInfo, BackendsResponse, HealthResponse, RunRequest, RunResponse, RunState, RunStatus,
};
use abp_daemon::routes::{Endpoint, MatchResult, Method, RouteError, RouteTable};
use abp_daemon::state::{BackendList, RegistryError, RunPhase, RunRecord, RunRegistry};
use chrono::Utc;
use std::collections::BTreeMap;
use uuid::Uuid;

// ===========================================================================
// 1. RunPhase unit tests
// ===========================================================================

#[test]
fn run_phase_serde_all_variants() {
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

#[test]
fn run_phase_serializes_snake_case() {
    assert_eq!(
        serde_json::to_string(&RunPhase::Queued).unwrap(),
        "\"queued\""
    );
    assert_eq!(
        serde_json::to_string(&RunPhase::Running).unwrap(),
        "\"running\""
    );
    assert_eq!(
        serde_json::to_string(&RunPhase::Completed).unwrap(),
        "\"completed\""
    );
    assert_eq!(
        serde_json::to_string(&RunPhase::Failed).unwrap(),
        "\"failed\""
    );
    assert_eq!(
        serde_json::to_string(&RunPhase::Cancelled).unwrap(),
        "\"cancelled\""
    );
}

#[test]
fn run_phase_terminal_check() {
    assert!(!RunPhase::Queued.is_terminal());
    assert!(!RunPhase::Running.is_terminal());
    assert!(RunPhase::Completed.is_terminal());
    assert!(RunPhase::Failed.is_terminal());
    assert!(RunPhase::Cancelled.is_terminal());
}

#[test]
fn run_phase_valid_transitions_from_queued() {
    assert!(RunPhase::Queued.can_transition_to(RunPhase::Running));
    assert!(RunPhase::Queued.can_transition_to(RunPhase::Cancelled));
    assert!(!RunPhase::Queued.can_transition_to(RunPhase::Completed));
    assert!(!RunPhase::Queued.can_transition_to(RunPhase::Failed));
}

#[test]
fn run_phase_valid_transitions_from_running() {
    assert!(RunPhase::Running.can_transition_to(RunPhase::Completed));
    assert!(RunPhase::Running.can_transition_to(RunPhase::Failed));
    assert!(RunPhase::Running.can_transition_to(RunPhase::Cancelled));
    assert!(!RunPhase::Running.can_transition_to(RunPhase::Queued));
}

#[test]
fn run_phase_no_transitions_from_terminal() {
    for phase in [RunPhase::Completed, RunPhase::Failed, RunPhase::Cancelled] {
        assert!(!phase.can_transition_to(RunPhase::Queued));
        assert!(!phase.can_transition_to(RunPhase::Running));
        assert!(!phase.can_transition_to(RunPhase::Completed));
        assert!(!phase.can_transition_to(RunPhase::Failed));
        assert!(!phase.can_transition_to(RunPhase::Cancelled));
    }
}

// ===========================================================================
// 2. RunRecord serde tests
// ===========================================================================

#[test]
fn run_record_serde_roundtrip_queued() {
    let record = RunRecord {
        id: Uuid::nil(),
        backend: "mock".into(),
        phase: RunPhase::Queued,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        events: vec![],
        receipt: None,
        error: None,
    };
    let json = serde_json::to_string(&record).unwrap();
    let back: RunRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, record.id);
    assert_eq!(back.backend, "mock");
    assert_eq!(back.phase, RunPhase::Queued);
    assert!(back.receipt.is_none());
    assert!(back.error.is_none());
}

#[test]
fn run_record_omits_none_fields() {
    let record = RunRecord {
        id: Uuid::nil(),
        backend: "mock".into(),
        phase: RunPhase::Running,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        events: vec![],
        receipt: None,
        error: None,
    };
    let val = serde_json::to_value(&record).unwrap();
    assert!(val.get("receipt").is_none());
    assert!(val.get("error").is_none());
}

#[test]
fn run_record_with_receipt() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let record = RunRecord {
        id: receipt.meta.run_id,
        backend: "mock".into(),
        phase: RunPhase::Completed,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        events: vec![],
        receipt: Some(receipt),
        error: None,
    };
    let val = serde_json::to_value(&record).unwrap();
    assert!(val.get("receipt").is_some());
}

#[test]
fn run_record_with_error() {
    let record = RunRecord {
        id: Uuid::new_v4(),
        backend: "mock".into(),
        phase: RunPhase::Failed,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        events: vec![],
        receipt: None,
        error: Some("backend timeout".into()),
    };
    let val = serde_json::to_value(&record).unwrap();
    assert_eq!(val["error"], "backend timeout");
}

#[test]
fn run_record_with_events() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let record = RunRecord {
        id: Uuid::new_v4(),
        backend: "mock".into(),
        phase: RunPhase::Running,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        events: vec![event],
        receipt: None,
        error: None,
    };
    let json = serde_json::to_string(&record).unwrap();
    let back: RunRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back.events.len(), 1);
}

// ===========================================================================
// 3. RunRegistry async tests
// ===========================================================================

#[tokio::test]
async fn registry_create_run() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    let result = reg.create_run(id, "mock".into()).await;
    assert_eq!(result.unwrap(), id);
    assert_eq!(reg.len().await, 1);
}

#[tokio::test]
async fn registry_duplicate_id_rejected() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    let err = reg.create_run(id, "mock".into()).await.unwrap_err();
    assert_eq!(err, RegistryError::DuplicateId(id));
}

#[tokio::test]
async fn registry_get_existing_run() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Queued);
    assert_eq!(record.backend, "mock");
}

#[tokio::test]
async fn registry_get_missing_run_returns_none() {
    let reg = RunRegistry::new();
    assert!(reg.get(Uuid::new_v4()).await.is_none());
}

#[tokio::test]
async fn registry_transition_queued_to_running() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    let phase = reg.transition(id, RunPhase::Running).await.unwrap();
    assert_eq!(phase, RunPhase::Running);
    assert_eq!(reg.get(id).await.unwrap().phase, RunPhase::Running);
}

#[tokio::test]
async fn registry_transition_running_to_completed() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    let phase = reg.transition(id, RunPhase::Completed).await.unwrap();
    assert_eq!(phase, RunPhase::Completed);
}

#[tokio::test]
async fn registry_invalid_transition_rejected() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    // Queued -> Completed is invalid
    let err = reg.transition(id, RunPhase::Completed).await.unwrap_err();
    match err {
        RegistryError::InvalidTransition { from, to, .. } => {
            assert_eq!(from, RunPhase::Queued);
            assert_eq!(to, RunPhase::Completed);
        }
        other => panic!("expected InvalidTransition, got {other:?}"),
    }
}

#[tokio::test]
async fn registry_transition_not_found() {
    let reg = RunRegistry::new();
    let err = reg
        .transition(Uuid::new_v4(), RunPhase::Running)
        .await
        .unwrap_err();
    matches!(err, RegistryError::NotFound(_));
}

#[tokio::test]
async fn registry_push_event() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let count = reg.push_event(id, event).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn registry_push_multiple_events() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    for i in 0..5 {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: format!("msg {i}"),
            },
            ext: None,
        };
        reg.push_event(id, event).await.unwrap();
    }
    let events = reg.events(id).await.unwrap();
    assert_eq!(events.len(), 5);
}

#[tokio::test]
async fn registry_events_not_found() {
    let reg = RunRegistry::new();
    let err = reg.events(Uuid::new_v4()).await.unwrap_err();
    matches!(err, RegistryError::NotFound(_));
}

#[tokio::test]
async fn registry_complete_run() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    reg.complete(id, receipt).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Completed);
    assert!(record.receipt.is_some());
}

#[tokio::test]
async fn registry_fail_run() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    reg.fail(id, "timeout".into()).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Failed);
    assert_eq!(record.error.as_deref(), Some("timeout"));
}

#[tokio::test]
async fn registry_cancel_queued_run() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.cancel(id).await.unwrap();
    assert_eq!(reg.get(id).await.unwrap().phase, RunPhase::Cancelled);
}

#[tokio::test]
async fn registry_cancel_running_run() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    reg.cancel(id).await.unwrap();
    assert_eq!(reg.get(id).await.unwrap().phase, RunPhase::Cancelled);
}

#[tokio::test]
async fn registry_cancel_completed_fails() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    reg.complete(id, receipt).await.unwrap();
    let err = reg.cancel(id).await.unwrap_err();
    matches!(err, RegistryError::InvalidTransition { .. });
}

#[tokio::test]
async fn registry_list_ids() {
    let reg = RunRegistry::new();
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    reg.create_run(id1, "a".into()).await.unwrap();
    reg.create_run(id2, "b".into()).await.unwrap();
    let ids = reg.list_ids().await;
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));
}

#[tokio::test]
async fn registry_list_all() {
    let reg = RunRegistry::new();
    reg.create_run(Uuid::new_v4(), "a".into()).await.unwrap();
    reg.create_run(Uuid::new_v4(), "b".into()).await.unwrap();
    let all = reg.list_all().await;
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn registry_count_by_phase() {
    let reg = RunRegistry::new();
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();
    reg.create_run(id1, "a".into()).await.unwrap();
    reg.create_run(id2, "b".into()).await.unwrap();
    reg.create_run(id3, "c".into()).await.unwrap();
    reg.transition(id1, RunPhase::Running).await.unwrap();
    assert_eq!(reg.count_by_phase(RunPhase::Queued).await, 2);
    assert_eq!(reg.count_by_phase(RunPhase::Running).await, 1);
    assert_eq!(reg.count_by_phase(RunPhase::Completed).await, 0);
}

#[tokio::test]
async fn registry_is_empty() {
    let reg = RunRegistry::new();
    assert!(reg.is_empty().await);
    reg.create_run(Uuid::new_v4(), "a".into()).await.unwrap();
    assert!(!reg.is_empty().await);
}

#[tokio::test]
async fn registry_remove_terminal_run() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    reg.fail(id, "err".into()).await.unwrap();
    let removed = reg.remove(id).await.unwrap();
    assert_eq!(removed.id, id);
    assert!(reg.get(id).await.is_none());
}

#[tokio::test]
async fn registry_remove_active_run_fails() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    let err = reg.remove(id).await.unwrap_err();
    matches!(err, RegistryError::InvalidTransition { .. });
}

#[tokio::test]
async fn registry_remove_missing_run_fails() {
    let reg = RunRegistry::new();
    let err = reg.remove(Uuid::new_v4()).await.unwrap_err();
    matches!(err, RegistryError::NotFound(_));
}

// ===========================================================================
// 4. BackendList tests
// ===========================================================================

#[tokio::test]
async fn backend_list_register_and_list() {
    let bl = BackendList::new();
    bl.register("mock".into()).await;
    bl.register("sidecar:node".into()).await;
    let names = bl.list().await;
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"mock".to_string()));
}

#[tokio::test]
async fn backend_list_dedup() {
    let bl = BackendList::new();
    bl.register("mock".into()).await;
    bl.register("mock".into()).await;
    assert_eq!(bl.len().await, 1);
}

#[tokio::test]
async fn backend_list_contains() {
    let bl = BackendList::new();
    bl.register("mock".into()).await;
    assert!(bl.contains("mock").await);
    assert!(!bl.contains("other").await);
}

#[tokio::test]
async fn backend_list_empty() {
    let bl = BackendList::new();
    assert!(bl.is_empty().await);
    bl.register("mock".into()).await;
    assert!(!bl.is_empty().await);
}

// ===========================================================================
// 5. RegistryError display
// ===========================================================================

#[test]
fn registry_error_not_found_display() {
    let id = Uuid::nil();
    let err = RegistryError::NotFound(id);
    assert!(err.to_string().contains("not found"));
}

#[test]
fn registry_error_duplicate_display() {
    let id = Uuid::nil();
    let err = RegistryError::DuplicateId(id);
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn registry_error_transition_display() {
    let err = RegistryError::InvalidTransition {
        run_id: Uuid::nil(),
        from: RunPhase::Queued,
        to: RunPhase::Completed,
    };
    let s = err.to_string();
    assert!(s.contains("invalid transition"));
    assert!(s.contains("Queued"));
    assert!(s.contains("Completed"));
}

// ===========================================================================
// 6. Route matching tests
// ===========================================================================

#[test]
fn route_health_get() {
    let rt = RouteTable::new("/api/v1");
    assert_eq!(
        rt.match_route(Method::Get, "/api/v1/health"),
        MatchResult::Matched(Endpoint::Health)
    );
}

#[test]
fn route_health_post_not_allowed() {
    let rt = RouteTable::new("/api/v1");
    assert_eq!(
        rt.match_route(Method::Post, "/api/v1/health"),
        MatchResult::MethodNotAllowed
    );
}

#[test]
fn route_backends_get() {
    let rt = RouteTable::new("/api/v1");
    assert_eq!(
        rt.match_route(Method::Get, "/api/v1/backends"),
        MatchResult::Matched(Endpoint::ListBackends)
    );
}

#[test]
fn route_runs_post() {
    let rt = RouteTable::new("/api/v1");
    assert_eq!(
        rt.match_route(Method::Post, "/api/v1/runs"),
        MatchResult::Matched(Endpoint::SubmitRun)
    );
}

#[test]
fn route_runs_get_not_allowed() {
    let rt = RouteTable::new("/api/v1");
    assert_eq!(
        rt.match_route(Method::Get, "/api/v1/runs"),
        MatchResult::MethodNotAllowed
    );
}

#[test]
fn route_get_run_by_id() {
    let rt = RouteTable::new("/api/v1");
    let id = Uuid::new_v4().to_string();
    let result = rt.match_route(Method::Get, &format!("/api/v1/runs/{id}"));
    assert_eq!(
        result,
        MatchResult::Matched(Endpoint::GetRun { run_id: id })
    );
}

#[test]
fn route_delete_run_by_id() {
    let rt = RouteTable::new("/api/v1");
    let id = Uuid::new_v4().to_string();
    let result = rt.match_route(Method::Delete, &format!("/api/v1/runs/{id}"));
    assert_eq!(
        result,
        MatchResult::Matched(Endpoint::DeleteRun { run_id: id })
    );
}

#[test]
fn route_get_run_events() {
    let rt = RouteTable::new("/api/v1");
    let id = "abc123";
    let result = rt.match_route(Method::Get, &format!("/api/v1/runs/{id}/events"));
    assert_eq!(
        result,
        MatchResult::Matched(Endpoint::GetRunEvents {
            run_id: id.to_string()
        })
    );
}

#[test]
fn route_cancel_run() {
    let rt = RouteTable::new("/api/v1");
    let id = "run-1";
    let result = rt.match_route(Method::Post, &format!("/api/v1/runs/{id}/cancel"));
    assert_eq!(
        result,
        MatchResult::Matched(Endpoint::CancelRun {
            run_id: id.to_string()
        })
    );
}

#[test]
fn route_unknown_path_returns_not_found() {
    let rt = RouteTable::new("/api/v1");
    assert_eq!(
        rt.match_route(Method::Get, "/api/v1/unknown"),
        MatchResult::NotFound
    );
}

#[test]
fn route_trailing_slash_handled() {
    let rt = RouteTable::new("/api/v1");
    assert_eq!(
        rt.match_route(Method::Get, "/api/v1/health/"),
        MatchResult::Matched(Endpoint::Health)
    );
}

#[test]
fn route_method_display() {
    assert_eq!(Method::Get.to_string(), "GET");
    assert_eq!(Method::Post.to_string(), "POST");
    assert_eq!(Method::Delete.to_string(), "DELETE");
}

// ===========================================================================
// 7. RouteError tests
// ===========================================================================

#[test]
fn route_error_bad_request_fields() {
    let err = RouteError::bad_request("missing field");
    assert_eq!(err.status, 400);
    assert_eq!(err.code, "bad_request");
    assert!(err.message.contains("missing field"));
}

#[test]
fn route_error_not_found_fields() {
    let err = RouteError::not_found("resource gone");
    assert_eq!(err.status, 404);
    assert_eq!(err.code, "not_found");
}

#[test]
fn route_error_conflict_fields() {
    let err = RouteError::conflict("already done");
    assert_eq!(err.status, 409);
}

#[test]
fn route_error_internal_fields() {
    let err = RouteError::internal("oops");
    assert_eq!(err.status, 500);
    assert_eq!(err.code, "internal_error");
}

// ===========================================================================
// 8. Handler type serde tests (cross-module)
// ===========================================================================

#[test]
fn health_response_roundtrip() {
    let resp = HealthResponse {
        status: "ok".into(),
        version: abp_core::CONTRACT_VERSION.into(),
        uptime_secs: 42,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: HealthResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn run_request_roundtrip() {
    let wo = WorkOrderBuilder::new("test task").build();
    let req = RunRequest {
        work_order: wo,
        backend_override: None,
        overrides: BTreeMap::new(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: RunRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.work_order.task, "test task");
}

#[test]
fn run_response_roundtrip() {
    let id = Uuid::nil();
    let resp = RunResponse {
        run_id: id,
        status: RunStatus {
            id,
            state: RunState::Completed,
            receipt: None,
        },
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: RunResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.run_id, id);
    assert_eq!(back.status.state, RunState::Completed);
}

#[test]
fn backends_response_roundtrip() {
    let resp = BackendsResponse {
        backends: vec![BackendInfo {
            name: "mock".into(),
            backend_type: "mock".into(),
            capabilities: BTreeMap::new(),
        }],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: BackendsResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.backends.len(), 1);
}

// ===========================================================================
// 9. API module type tests
// ===========================================================================

#[test]
fn api_run_status_serde_roundtrip() {
    for status in [
        ApiRunStatus::Queued,
        ApiRunStatus::Running,
        ApiRunStatus::Completed,
        ApiRunStatus::Failed,
        ApiRunStatus::Cancelled,
    ] {
        let json = serde_json::to_string(&status).unwrap();
        let back: ApiRunStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }
}

#[test]
fn api_run_status_transitions() {
    assert!(ApiRunStatus::Queued.can_transition_to(ApiRunStatus::Running));
    assert!(!ApiRunStatus::Completed.can_transition_to(ApiRunStatus::Running));
}

#[test]
fn api_run_info_roundtrip() {
    let info = RunInfo {
        id: Uuid::nil(),
        status: ApiRunStatus::Running,
        backend: "mock".into(),
        created_at: Utc::now(),
        events_count: 10,
    };
    let json = serde_json::to_string(&info).unwrap();
    let back: RunInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(back.events_count, 10);
}

#[test]
fn api_request_submit_run_roundtrip() {
    let wo = WorkOrderBuilder::new("test").build();
    let req = ApiRequest::SubmitRun {
        backend: "mock".into(),
        work_order: Box::new(wo),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ApiRequest = serde_json::from_str(&json).unwrap();
    match back {
        ApiRequest::SubmitRun { backend, .. } => assert_eq!(backend, "mock"),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn api_response_run_created_roundtrip() {
    let resp = ApiResponse::RunCreated {
        run_id: Uuid::nil(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: ApiResponse = serde_json::from_str(&json).unwrap();
    match back {
        ApiResponse::RunCreated { run_id } => assert_eq!(run_id, Uuid::nil()),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn api_response_health_roundtrip() {
    let resp = ApiResponse::Health(ApiHealth {
        status: "ok".into(),
        version: abp_core::CONTRACT_VERSION.into(),
        uptime_secs: 99,
        backends: vec!["mock".into(), "sidecar".into()],
    });
    let json = serde_json::to_string(&resp).unwrap();
    let back: ApiResponse = serde_json::from_str(&json).unwrap();
    match back {
        ApiResponse::Health(h) => {
            assert_eq!(h.status, "ok");
            assert_eq!(h.uptime_secs, 99);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn api_error_roundtrip() {
    let err = ApiApiError::not_found("run xyz not found");
    let json = serde_json::to_string(&err).unwrap();
    let back: ApiApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(back.code, "not_found");
    assert_eq!(back.message, "run xyz not found");
}

#[test]
fn api_error_stable_codes() {
    assert_eq!(ApiApiError::not_found("x").code, "not_found");
    assert_eq!(ApiApiError::invalid_request("x").code, "invalid_request");
    assert_eq!(ApiApiError::conflict("x").code, "conflict");
    assert_eq!(ApiApiError::internal("x").code, "internal_error");
}

#[test]
fn api_error_with_details_present() {
    let err =
        ApiApiError::invalid_request("bad").with_details(serde_json::json!({"field": "task"}));
    let val = serde_json::to_value(&err).unwrap();
    assert_eq!(val["details"]["field"], "task");
}

#[test]
fn api_error_omits_null_details() {
    let err = ApiApiError::not_found("gone");
    let val = serde_json::to_value(&err).unwrap();
    assert!(val.get("details").is_none());
}

#[test]
fn backend_detail_roundtrip() {
    let detail = BackendDetail {
        id: "mock".into(),
        capabilities: BTreeMap::new(),
    };
    let json = serde_json::to_string(&detail).unwrap();
    let back: BackendDetail = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "mock");
}

// ===========================================================================
// 10. Concurrent registry access
// ===========================================================================

#[tokio::test]
async fn registry_concurrent_creates() {
    let reg = RunRegistry::new();
    let mut handles = vec![];
    for _ in 0..20 {
        let r = reg.clone();
        handles.push(tokio::spawn(async move {
            r.create_run(Uuid::new_v4(), "mock".into()).await
        }));
    }
    for h in handles {
        h.await.unwrap().unwrap();
    }
    assert_eq!(reg.len().await, 20);
}

#[tokio::test]
async fn registry_concurrent_transitions() {
    let reg = RunRegistry::new();
    let mut ids = vec![];
    for _ in 0..10 {
        let id = Uuid::new_v4();
        reg.create_run(id, "mock".into()).await.unwrap();
        ids.push(id);
    }
    let mut handles = vec![];
    for id in ids {
        let r = reg.clone();
        handles.push(tokio::spawn(async move {
            r.transition(id, RunPhase::Running).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(reg.count_by_phase(RunPhase::Running).await, 10);
}

// ===========================================================================
// 11. RunState (handler module) tests
// ===========================================================================

#[test]
fn run_state_terminal_check() {
    assert!(!RunState::Pending.is_terminal());
    assert!(!RunState::Running.is_terminal());
    assert!(RunState::Completed.is_terminal());
    assert!(RunState::Failed.is_terminal());
}

#[test]
fn run_state_all_variants_serde() {
    for state in [
        RunState::Pending,
        RunState::Running,
        RunState::Completed,
        RunState::Failed,
    ] {
        let json = serde_json::to_string(&state).unwrap();
        let back: RunState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, back);
    }
}
