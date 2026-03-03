// SPDX-License-Identifier: MIT OR Apache-2.0
//! Trait-based HTTP route handler signatures for the daemon control-plane.
//!
//! These traits define the contract for each endpoint without coupling to a
//! specific web framework. Implementations can be backed by Axum, Hyper, or
//! plain function pointers.

use crate::handler::{BackendsResponse, HealthResponse, RunRequest, RunResponse, RunStatus};
use std::future::Future;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Route error
// ---------------------------------------------------------------------------

/// Unified error type returned by route handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteError {
    /// HTTP-like status code.
    pub status: u16,
    /// Machine-readable error code.
    pub code: String,
    /// Human-readable error message.
    pub message: String,
}

impl RouteError {
    /// 400 Bad Request.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            code: "bad_request".into(),
            message: message.into(),
        }
    }

    /// 404 Not Found.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: 404,
            code: "not_found".into(),
            message: message.into(),
        }
    }

    /// 409 Conflict.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: 409,
            code: "conflict".into(),
            message: message.into(),
        }
    }

    /// 500 Internal Server Error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: 500,
            code: "internal_error".into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}: {}", self.status, self.code, self.message)
    }
}

impl std::error::Error for RouteError {}

// ---------------------------------------------------------------------------
// Handler traits
// ---------------------------------------------------------------------------

/// `GET /health` — returns server status, contract version, and uptime.
pub trait HealthHandler: Send + Sync {
    /// Handle the health check request.
    fn health(&self) -> impl Future<Output = Result<HealthResponse, RouteError>> + Send;
}

/// `GET /backends` — lists all registered backends with capabilities.
pub trait BackendsHandler: Send + Sync {
    /// Handle the backends list request.
    fn list_backends(&self) -> impl Future<Output = Result<BackendsResponse, RouteError>> + Send;
}

/// `POST /run` — accepts a work order and returns the run result.
pub trait RunHandler: Send + Sync {
    /// Handle a run submission.
    fn submit_run(
        &self,
        request: RunRequest,
    ) -> impl Future<Output = Result<RunResponse, RouteError>> + Send;
}

/// `GET /runs/{id}` — retrieves the current status of a run.
pub trait RunStatusHandler: Send + Sync {
    /// Look up the status of a specific run by ID.
    fn get_run_status(
        &self,
        run_id: Uuid,
    ) -> impl Future<Output = Result<RunStatus, RouteError>> + Send;
}

// ---------------------------------------------------------------------------
// Combined router trait
// ---------------------------------------------------------------------------

/// Aggregates all handler traits into a single daemon router interface.
pub trait DaemonRouter: HealthHandler + BackendsHandler + RunHandler + RunStatusHandler {}

impl<T> DaemonRouter for T where T: HealthHandler + BackendsHandler + RunHandler + RunStatusHandler {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::{BackendInfo, RunState};
    use std::collections::BTreeMap;

    // -- RouteError ---------------------------------------------------------

    #[test]
    fn route_error_bad_request() {
        let err = RouteError::bad_request("missing field");
        assert_eq!(err.status, 400);
        assert_eq!(err.code, "bad_request");
        assert!(err.message.contains("missing field"));
    }

    #[test]
    fn route_error_not_found() {
        let err = RouteError::not_found("run not found");
        assert_eq!(err.status, 404);
        assert_eq!(err.code, "not_found");
    }

    #[test]
    fn route_error_conflict() {
        let err = RouteError::conflict("already completed");
        assert_eq!(err.status, 409);
        assert_eq!(err.code, "conflict");
    }

    #[test]
    fn route_error_internal() {
        let err = RouteError::internal("unexpected");
        assert_eq!(err.status, 500);
        assert_eq!(err.code, "internal_error");
    }

    #[test]
    fn route_error_display() {
        let err = RouteError::not_found("gone");
        let s = err.to_string();
        assert!(s.contains("404"));
        assert!(s.contains("not_found"));
        assert!(s.contains("gone"));
    }

    #[test]
    fn route_error_equality() {
        let a = RouteError::bad_request("x");
        let b = RouteError::bad_request("x");
        assert_eq!(a, b);
    }

    #[test]
    fn route_error_inequality() {
        let a = RouteError::bad_request("x");
        let b = RouteError::not_found("x");
        assert_ne!(a, b);
    }

    // -- Mock router implementation for trait testing -----------------------

    struct MockRouter {
        backends: Vec<BackendInfo>,
        run_status: Option<RunStatus>,
    }

    impl MockRouter {
        fn new() -> Self {
            Self {
                backends: vec![BackendInfo {
                    name: "mock".into(),
                    backend_type: "mock".into(),
                    capabilities: BTreeMap::new(),
                }],
                run_status: None,
            }
        }

        fn with_run(mut self, status: RunStatus) -> Self {
            self.run_status = Some(status);
            self
        }
    }

    impl HealthHandler for MockRouter {
        async fn health(&self) -> Result<HealthResponse, RouteError> {
            Ok(HealthResponse {
                status: "ok".into(),
                version: abp_core::CONTRACT_VERSION.into(),
                uptime_secs: 0,
            })
        }
    }

    impl BackendsHandler for MockRouter {
        async fn list_backends(&self) -> Result<BackendsResponse, RouteError> {
            Ok(BackendsResponse {
                backends: self.backends.clone(),
            })
        }
    }

    impl RunHandler for MockRouter {
        async fn submit_run(&self, _request: RunRequest) -> Result<RunResponse, RouteError> {
            let id = Uuid::nil();
            Ok(RunResponse {
                run_id: id,
                status: RunStatus {
                    id,
                    state: RunState::Completed,
                    receipt: None,
                },
            })
        }
    }

    impl RunStatusHandler for MockRouter {
        async fn get_run_status(&self, run_id: Uuid) -> Result<RunStatus, RouteError> {
            self.run_status
                .clone()
                .ok_or_else(|| RouteError::not_found(format!("run {run_id} not found")))
        }
    }

    // -- Trait-based handler tests ------------------------------------------

    #[tokio::test]
    async fn mock_health_returns_ok() {
        let router = MockRouter::new();
        let resp = router.health().await.unwrap();
        assert_eq!(resp.status, "ok");
        assert_eq!(resp.version, abp_core::CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn mock_list_backends_returns_registered() {
        let router = MockRouter::new();
        let resp = router.list_backends().await.unwrap();
        assert_eq!(resp.backends.len(), 1);
        assert_eq!(resp.backends[0].name, "mock");
    }

    #[tokio::test]
    async fn mock_submit_run_returns_completed() {
        use abp_core::WorkOrderBuilder;
        let router = MockRouter::new();
        let req = RunRequest {
            work_order: WorkOrderBuilder::new("test").build(),
            backend_override: None,
            overrides: BTreeMap::new(),
        };
        let resp = router.submit_run(req).await.unwrap();
        assert_eq!(resp.status.state, RunState::Completed);
    }

    #[tokio::test]
    async fn mock_get_run_status_not_found() {
        let router = MockRouter::new();
        let err = router.get_run_status(Uuid::new_v4()).await.unwrap_err();
        assert_eq!(err.status, 404);
    }

    #[tokio::test]
    async fn mock_get_run_status_found() {
        let id = Uuid::new_v4();
        let status = RunStatus {
            id,
            state: RunState::Running,
            receipt: None,
        };
        let router = MockRouter::new().with_run(status.clone());
        let resp = router.get_run_status(id).await.unwrap();
        assert_eq!(resp.state, RunState::Running);
        assert_eq!(resp.id, id);
    }

    #[tokio::test]
    async fn daemon_router_blanket_impl() {
        let router = MockRouter::new();
        // Verify that MockRouter satisfies DaemonRouter via blanket impl.
        fn assert_daemon_router<T: DaemonRouter>(_: &T) {}
        assert_daemon_router(&router);

        let health = HealthHandler::health(&router).await.unwrap();
        assert_eq!(health.status, "ok");
    }
}
