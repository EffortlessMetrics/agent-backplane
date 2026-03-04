// SPDX-License-Identifier: MIT OR Apache-2.0
//! Middleware pattern for runtime work-order execution.
//!
//! A [`Middleware`] receives callbacks before and after a backend run,
//! and a [`MiddlewareChain`] composes an ordered sequence of middlewares.
//! Three built-in implementations are provided:
//!
//! - [`LoggingMiddleware`] — structured `tracing` output
//! - [`TelemetryMiddleware`] — records timing/outcomes in [`RunMetrics`]
//! - [`PolicyMiddleware`] — validates the work order against its policy

use abp_core::{Receipt, WorkOrder};
use abp_policy::PolicyEngine;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::telemetry::RunMetrics;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Context carried through the middleware chain for a single run.
#[derive(Debug, Clone)]
pub struct MiddlewareContext {
    /// Name of the backend selected for this run.
    pub backend_name: String,
    /// Timestamp when the run was initiated.
    pub started_at: Instant,
}

impl MiddlewareContext {
    /// Create a new context for the given backend.
    #[must_use]
    pub fn new(backend_name: impl Into<String>) -> Self {
        Self {
            backend_name: backend_name.into(),
            started_at: Instant::now(),
        }
    }

    /// Elapsed wall-clock time since the context was created.
    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }
}

/// Extension point called before and after a backend run.
///
/// Implementations must be `Send + Sync` so they can be shared across
/// async tasks.
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Called before the backend receives the work order.
    ///
    /// Returning `Err` short-circuits the chain and prevents the run.
    async fn before_run(&self, order: &WorkOrder, ctx: &MiddlewareContext) -> Result<()>;

    /// Called after the backend returns (or fails).
    ///
    /// `receipt` is `Some` on success, `None` on failure.
    async fn after_run(
        &self,
        order: &WorkOrder,
        ctx: &MiddlewareContext,
        receipt: Option<&Receipt>,
    ) -> Result<()>;

    /// Human-readable name for logging / diagnostics.
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// Chain
// ---------------------------------------------------------------------------

/// An ordered sequence of [`Middleware`] implementations executed in
/// registration order for `before_run` and reverse order for `after_run`.
pub struct MiddlewareChain {
    middlewares: Vec<Box<dyn Middleware>>,
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

impl MiddlewareChain {
    /// Create an empty chain.
    #[must_use]
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    /// Append a middleware (builder pattern).
    #[must_use]
    pub fn with<M: Middleware + 'static>(mut self, m: M) -> Self {
        self.middlewares.push(Box::new(m));
        self
    }

    /// Append a middleware in place.
    pub fn push<M: Middleware + 'static>(&mut self, m: M) {
        self.middlewares.push(Box::new(m));
    }

    /// Run all `before_run` hooks in registration order.
    ///
    /// Short-circuits on the first error.
    pub async fn run_before(&self, order: &WorkOrder, ctx: &MiddlewareContext) -> Result<()> {
        for mw in &self.middlewares {
            debug!(target: "abp.middleware", name=%mw.name(), "before_run");
            mw.before_run(order, ctx).await?;
        }
        Ok(())
    }

    /// Run all `after_run` hooks in **reverse** registration order.
    ///
    /// Errors are collected but do not short-circuit — all hooks are called.
    pub async fn run_after(
        &self,
        order: &WorkOrder,
        ctx: &MiddlewareContext,
        receipt: Option<&Receipt>,
    ) -> Vec<anyhow::Error> {
        let mut errors = Vec::new();
        for mw in self.middlewares.iter().rev() {
            debug!(target: "abp.middleware", name=%mw.name(), "after_run");
            if let Err(e) = mw.after_run(order, ctx, receipt).await {
                errors.push(e);
            }
        }
        errors
    }

    /// Number of middlewares in the chain.
    #[must_use]
    pub fn len(&self) -> usize {
        self.middlewares.len()
    }

    /// `true` if the chain contains no middlewares.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.middlewares.is_empty()
    }

    /// Names of all middlewares in registration order.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.middlewares.iter().map(|m| m.name()).collect()
    }
}

// ---------------------------------------------------------------------------
// Built-in: LoggingMiddleware
// ---------------------------------------------------------------------------

/// Emits structured `tracing` output around each run.
pub struct LoggingMiddleware;

#[async_trait]
impl Middleware for LoggingMiddleware {
    async fn before_run(&self, order: &WorkOrder, ctx: &MiddlewareContext) -> Result<()> {
        info!(
            target: "abp.middleware.logging",
            work_order_id = %order.id,
            task = %order.task,
            backend = %ctx.backend_name,
            "run starting"
        );
        Ok(())
    }

    async fn after_run(
        &self,
        order: &WorkOrder,
        ctx: &MiddlewareContext,
        receipt: Option<&Receipt>,
    ) -> Result<()> {
        let elapsed = ctx.elapsed_ms();
        match receipt {
            Some(r) => info!(
                target: "abp.middleware.logging",
                work_order_id = %order.id,
                outcome = ?r.outcome,
                elapsed_ms = elapsed,
                "run complete"
            ),
            None => warn!(
                target: "abp.middleware.logging",
                work_order_id = %order.id,
                elapsed_ms = elapsed,
                "run failed (no receipt)"
            ),
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "logging"
    }
}

// ---------------------------------------------------------------------------
// Built-in: TelemetryMiddleware
// ---------------------------------------------------------------------------

/// Records run timing and outcome into a shared [`RunMetrics`] collector.
pub struct TelemetryMiddleware {
    metrics: Arc<RunMetrics>,
}

impl TelemetryMiddleware {
    /// Create a new telemetry middleware backed by the given metrics.
    #[must_use]
    pub fn new(metrics: Arc<RunMetrics>) -> Self {
        Self { metrics }
    }

    /// Return a reference to the underlying metrics.
    #[must_use]
    pub fn metrics(&self) -> &RunMetrics {
        &self.metrics
    }
}

#[async_trait]
impl Middleware for TelemetryMiddleware {
    async fn before_run(&self, _order: &WorkOrder, _ctx: &MiddlewareContext) -> Result<()> {
        Ok(())
    }

    async fn after_run(
        &self,
        _order: &WorkOrder,
        ctx: &MiddlewareContext,
        receipt: Option<&Receipt>,
    ) -> Result<()> {
        let duration_ms = ctx.elapsed_ms();
        let (success, event_count) = match receipt {
            Some(r) => {
                let ok = matches!(
                    r.outcome,
                    abp_core::Outcome::Complete | abp_core::Outcome::Partial
                );
                (ok, r.trace.len() as u64)
            }
            None => (false, 0),
        };
        self.metrics.record_run(duration_ms, success, event_count);
        Ok(())
    }

    fn name(&self) -> &str {
        "telemetry"
    }
}

// ---------------------------------------------------------------------------
// Built-in: PolicyMiddleware
// ---------------------------------------------------------------------------

/// Validates the work order's policy before the run.
///
/// Compiles the [`PolicyEngine`] and checks that every tool in the
/// allow-list is not simultaneously denied.
pub struct PolicyMiddleware;

#[async_trait]
impl Middleware for PolicyMiddleware {
    async fn before_run(&self, order: &WorkOrder, _ctx: &MiddlewareContext) -> Result<()> {
        let engine = PolicyEngine::new(&order.policy)?;
        for tool in &order.policy.allowed_tools {
            let decision = engine.can_use_tool(tool);
            anyhow::ensure!(
                decision.allowed,
                "policy blocks tool `{}`: {}",
                tool,
                decision.reason.as_deref().unwrap_or("denied by policy")
            );
        }
        Ok(())
    }

    async fn after_run(
        &self,
        _order: &WorkOrder,
        _ctx: &MiddlewareContext,
        _receipt: Option<&Receipt>,
    ) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "policy"
    }
}

// ---------------------------------------------------------------------------
// Built-in: audit middleware (records order ids)
// ---------------------------------------------------------------------------

/// Records processed work order ids for audit / testing.
pub struct AuditMiddleware {
    log: Arc<Mutex<Vec<uuid::Uuid>>>,
}

impl AuditMiddleware {
    /// Create a new audit middleware with an empty log.
    #[must_use]
    pub fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Return a snapshot of recorded work order ids.
    pub async fn ids(&self) -> Vec<uuid::Uuid> {
        self.log.lock().await.clone()
    }
}

impl Default for AuditMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for AuditMiddleware {
    async fn before_run(&self, order: &WorkOrder, _ctx: &MiddlewareContext) -> Result<()> {
        self.log.lock().await.push(order.id);
        Ok(())
    }

    async fn after_run(
        &self,
        _order: &WorkOrder,
        _ctx: &MiddlewareContext,
        _receipt: Option<&Receipt>,
    ) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "audit"
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{
        CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, WorkOrder,
        WorkspaceMode, WorkspaceSpec,
    };

    fn sample_work_order() -> WorkOrder {
        WorkOrder {
            id: uuid::Uuid::new_v4(),
            task: "test task".into(),
            lane: ExecutionLane::PatchFirst,
            workspace: WorkspaceSpec {
                root: "/tmp/test".into(),
                mode: WorkspaceMode::PassThrough,
                include: vec![],
                exclude: vec![],
            },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: abp_core::RuntimeConfig::default(),
        }
    }

    fn sample_receipt() -> Receipt {
        abp_receipt::ReceiptBuilder::new("mock")
            .outcome(abp_core::Outcome::Complete)
            .build()
    }

    // -- MiddlewareContext --

    #[test]
    fn context_stores_backend_name() {
        let ctx = MiddlewareContext::new("test-backend");
        assert_eq!(ctx.backend_name, "test-backend");
    }

    #[test]
    fn context_elapsed_is_non_negative() {
        let ctx = MiddlewareContext::new("b");
        assert!(ctx.elapsed_ms() < 1000);
    }

    // -- MiddlewareChain --

    #[test]
    fn empty_chain() {
        let chain = MiddlewareChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
        assert!(chain.names().is_empty());
    }

    #[test]
    fn chain_default_is_empty() {
        let chain = MiddlewareChain::default();
        assert!(chain.is_empty());
    }

    #[test]
    fn chain_with_appends() {
        let chain = MiddlewareChain::new()
            .with(LoggingMiddleware)
            .with(PolicyMiddleware);
        assert_eq!(chain.len(), 2);
        assert_eq!(chain.names(), vec!["logging", "policy"]);
    }

    #[test]
    fn chain_push_appends() {
        let mut chain = MiddlewareChain::new();
        chain.push(LoggingMiddleware);
        assert_eq!(chain.len(), 1);
    }

    #[tokio::test]
    async fn chain_before_run_calls_all() {
        let audit = Arc::new(AuditMiddleware::new());
        let chain = MiddlewareChain::new()
            .with(LoggingMiddleware)
            .with(AuditMiddleware {
                log: Arc::clone(&audit.log),
            });
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        chain.run_before(&wo, &ctx).await.unwrap();
        assert_eq!(audit.ids().await.len(), 1);
    }

    #[tokio::test]
    async fn chain_after_run_calls_reverse_order() {
        struct Marker {
            label: &'static str,
            log: Arc<Mutex<Vec<&'static str>>>,
        }

        #[async_trait]
        impl Middleware for Marker {
            async fn before_run(&self, _order: &WorkOrder, _ctx: &MiddlewareContext) -> Result<()> {
                Ok(())
            }
            async fn after_run(
                &self,
                _order: &WorkOrder,
                _ctx: &MiddlewareContext,
                _receipt: Option<&Receipt>,
            ) -> Result<()> {
                self.log.lock().await.push(self.label);
                Ok(())
            }
            fn name(&self) -> &str {
                self.label
            }
        }

        let shared_log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        let chain = MiddlewareChain::new()
            .with(Marker {
                label: "first",
                log: Arc::clone(&shared_log),
            })
            .with(Marker {
                label: "second",
                log: Arc::clone(&shared_log),
            });

        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        let receipt = sample_receipt();

        let errors = chain.run_after(&wo, &ctx, Some(&receipt)).await;
        assert!(errors.is_empty());

        let order = shared_log.lock().await;
        assert_eq!(*order, vec!["second", "first"]); // reverse
    }

    #[tokio::test]
    async fn chain_after_run_collects_errors() {
        struct FailAfter;

        #[async_trait]
        impl Middleware for FailAfter {
            async fn before_run(&self, _order: &WorkOrder, _ctx: &MiddlewareContext) -> Result<()> {
                Ok(())
            }
            async fn after_run(
                &self,
                _order: &WorkOrder,
                _ctx: &MiddlewareContext,
                _receipt: Option<&Receipt>,
            ) -> Result<()> {
                anyhow::bail!("intentional failure");
            }
            fn name(&self) -> &str {
                "fail"
            }
        }

        let chain = MiddlewareChain::new().with(FailAfter).with(FailAfter);

        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        let errors = chain.run_after(&wo, &ctx, None).await;
        assert_eq!(errors.len(), 2);
    }

    #[tokio::test]
    async fn chain_before_run_short_circuits() {
        struct FailBefore;

        #[async_trait]
        impl Middleware for FailBefore {
            async fn before_run(&self, _order: &WorkOrder, _ctx: &MiddlewareContext) -> Result<()> {
                anyhow::bail!("blocked");
            }
            async fn after_run(
                &self,
                _order: &WorkOrder,
                _ctx: &MiddlewareContext,
                _receipt: Option<&Receipt>,
            ) -> Result<()> {
                Ok(())
            }
            fn name(&self) -> &str {
                "fail"
            }
        }

        let audit = AuditMiddleware::new();
        let chain = MiddlewareChain::new()
            .with(FailBefore)
            .with(AuditMiddleware {
                log: Arc::clone(&audit.log),
            });

        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        let err = chain.run_before(&wo, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("blocked"));
        // Audit should NOT have been called because FailBefore short-circuited.
        assert!(audit.ids().await.is_empty());
    }

    // -- LoggingMiddleware --

    #[tokio::test]
    async fn logging_middleware_succeeds() {
        let mw = LoggingMiddleware;
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        mw.before_run(&wo, &ctx).await.unwrap();
        mw.after_run(&wo, &ctx, Some(&sample_receipt()))
            .await
            .unwrap();
        mw.after_run(&wo, &ctx, None).await.unwrap();
    }

    #[test]
    fn logging_middleware_name() {
        assert_eq!(LoggingMiddleware.name(), "logging");
    }

    // -- TelemetryMiddleware --

    #[tokio::test]
    async fn telemetry_records_success() {
        let metrics = Arc::new(RunMetrics::new());
        let mw = TelemetryMiddleware::new(Arc::clone(&metrics));
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        let receipt = sample_receipt();

        mw.after_run(&wo, &ctx, Some(&receipt)).await.unwrap();

        let snap = metrics.snapshot();
        assert_eq!(snap.total_runs, 1);
        assert_eq!(snap.successful_runs, 1);
        assert_eq!(snap.failed_runs, 0);
    }

    #[tokio::test]
    async fn telemetry_records_failure() {
        let metrics = Arc::new(RunMetrics::new());
        let mw = TelemetryMiddleware::new(Arc::clone(&metrics));
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        mw.after_run(&wo, &ctx, None).await.unwrap();

        let snap = metrics.snapshot();
        assert_eq!(snap.total_runs, 1);
        assert_eq!(snap.failed_runs, 1);
    }

    #[test]
    fn telemetry_middleware_name() {
        let metrics = Arc::new(RunMetrics::new());
        let mw = TelemetryMiddleware::new(metrics);
        assert_eq!(mw.name(), "telemetry");
    }

    #[test]
    fn telemetry_metrics_accessor() {
        let metrics = Arc::new(RunMetrics::new());
        let mw = TelemetryMiddleware::new(Arc::clone(&metrics));
        let snap = mw.metrics().snapshot();
        assert_eq!(snap.total_runs, 0);
    }

    // -- PolicyMiddleware --

    #[tokio::test]
    async fn policy_middleware_passes_clean_order() {
        let mw = PolicyMiddleware;
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        mw.before_run(&wo, &ctx).await.unwrap();
    }

    #[tokio::test]
    async fn policy_middleware_rejects_conflicting_tools() {
        let mw = PolicyMiddleware;
        let mut wo = sample_work_order();
        wo.policy.allowed_tools = vec!["bash".into()];
        wo.policy.disallowed_tools = vec!["bash".into()];
        let ctx = MiddlewareContext::new("mock");

        let err = mw.before_run(&wo, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("bash"));
    }

    #[tokio::test]
    async fn policy_middleware_after_run_is_noop() {
        let mw = PolicyMiddleware;
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        mw.after_run(&wo, &ctx, None).await.unwrap();
    }

    #[test]
    fn policy_middleware_name() {
        assert_eq!(PolicyMiddleware.name(), "policy");
    }

    // -- AuditMiddleware --

    #[tokio::test]
    async fn audit_middleware_records_ids() {
        let mw = AuditMiddleware::new();
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        mw.before_run(&wo, &ctx).await.unwrap();
        mw.before_run(&wo, &ctx).await.unwrap();

        let ids = mw.ids().await;
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], wo.id);
    }

    #[test]
    fn audit_middleware_default() {
        let mw = AuditMiddleware::default();
        assert_eq!(mw.name(), "audit");
    }
}
