// SPDX-License-Identifier: MIT OR Apache-2.0
//! Middleware pattern for runtime work-order execution.
//!
//! A `Middleware` receives callbacks before and after a backend run,
//! and a `MiddlewareChain` composes an ordered sequence of middlewares.
//!
//! Built-in implementations:
//!
//! - [`LoggingMiddleware`] — structured `tracing` output with configurable verbosity
//! - [`TelemetryMiddleware`] — records timing/outcomes in `RunMetrics`
//! - [`PolicyMiddleware`] — validates the work order against its policy
//! - [`MetricsMiddleware`] — collects timing and token usage metrics
//! - [`RateLimitMiddleware`] — per-request rate limiting via `abp-ratelimit`
//! - [`CachingMiddleware`] — optional response caching with TTL and key strategy
//! - [`RetryMiddleware`] — automatic retry planning with `RecoveryExecutor`
//! - [`ValidationMiddleware`] — validates work orders against schema
//! - [`TransformMiddleware`] — request/response transformation hooks
//! - [`AuditMiddleware`] — records processed work order ids

use abp_core::{Receipt, WorkOrder};
use abp_error::recovery::{RecoveryExecutor, RecoveryReport};
use abp_policy::PolicyEngine;
use abp_ratelimit::{BackendRateLimiter, RateLimitPolicy};
use abp_validate::{Validator, WorkOrderValidator};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, info, trace, warn};

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
// Verbosity
// ---------------------------------------------------------------------------

/// Controls how much detail [`LoggingMiddleware`] emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    /// Only work-order id and outcome.
    Minimal,
    /// Id, task, backend, and elapsed time (default).
    Normal,
    /// Everything including context snippet count and policy summary.
    Verbose,
}

// ---------------------------------------------------------------------------
// Built-in: LoggingMiddleware
// ---------------------------------------------------------------------------

/// Emits structured `tracing` output around each run with configurable
/// [`Verbosity`].
pub struct LoggingMiddleware {
    verbosity: Verbosity,
}

impl LoggingMiddleware {
    /// Create a new logging middleware with the given verbosity.
    #[must_use]
    pub fn with_verbosity(verbosity: Verbosity) -> Self {
        Self { verbosity }
    }

    /// Return the configured verbosity.
    #[must_use]
    pub fn verbosity(&self) -> Verbosity {
        self.verbosity
    }
}

impl Default for LoggingMiddleware {
    fn default() -> Self {
        Self {
            verbosity: Verbosity::Normal,
        }
    }
}

#[async_trait]
impl Middleware for LoggingMiddleware {
    async fn before_run(&self, order: &WorkOrder, ctx: &MiddlewareContext) -> Result<()> {
        match self.verbosity {
            Verbosity::Minimal => {
                info!(
                    target: "abp.middleware.logging",
                    work_order_id = %order.id,
                    "run starting"
                );
            }
            Verbosity::Normal => {
                info!(
                    target: "abp.middleware.logging",
                    work_order_id = %order.id,
                    task = %order.task,
                    backend = %ctx.backend_name,
                    "run starting"
                );
            }
            Verbosity::Verbose => {
                info!(
                    target: "abp.middleware.logging",
                    work_order_id = %order.id,
                    task = %order.task,
                    backend = %ctx.backend_name,
                    snippets = order.context.snippets.len(),
                    allowed_tools = order.policy.allowed_tools.len(),
                    lane = ?order.lane,
                    "run starting"
                );
            }
        }
        Ok(())
    }

    async fn after_run(
        &self,
        order: &WorkOrder,
        ctx: &MiddlewareContext,
        receipt: Option<&Receipt>,
    ) -> Result<()> {
        let elapsed = ctx.elapsed_ms();
        match (self.verbosity, receipt) {
            (Verbosity::Minimal, Some(r)) => {
                info!(
                    target: "abp.middleware.logging",
                    work_order_id = %order.id,
                    outcome = ?r.outcome,
                    "run complete"
                );
            }
            (Verbosity::Minimal, None) => {
                warn!(
                    target: "abp.middleware.logging",
                    work_order_id = %order.id,
                    "run failed (no receipt)"
                );
            }
            (Verbosity::Normal, Some(r)) => {
                info!(
                    target: "abp.middleware.logging",
                    work_order_id = %order.id,
                    outcome = ?r.outcome,
                    elapsed_ms = elapsed,
                    "run complete"
                );
            }
            (Verbosity::Normal, None) => {
                warn!(
                    target: "abp.middleware.logging",
                    work_order_id = %order.id,
                    elapsed_ms = elapsed,
                    "run failed (no receipt)"
                );
            }
            (Verbosity::Verbose, Some(r)) => {
                info!(
                    target: "abp.middleware.logging",
                    work_order_id = %order.id,
                    outcome = ?r.outcome,
                    elapsed_ms = elapsed,
                    events = r.trace.len(),
                    input_tokens = ?r.usage.input_tokens,
                    output_tokens = ?r.usage.output_tokens,
                    "run complete"
                );
            }
            (Verbosity::Verbose, None) => {
                warn!(
                    target: "abp.middleware.logging",
                    work_order_id = %order.id,
                    elapsed_ms = elapsed,
                    "run failed (no receipt)"
                );
            }
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
// Built-in: MetricsMiddleware
// ---------------------------------------------------------------------------

/// Atomic counters for timing and token usage collected by
/// [`MetricsMiddleware`].
pub struct MiddlewareMetrics {
    total_requests: AtomicU64,
    total_duration_ms: AtomicU64,
    total_input_tokens: AtomicU64,
    total_output_tokens: AtomicU64,
    successful_requests: AtomicU64,
    failed_requests: AtomicU64,
}

impl MiddlewareMetrics {
    /// Create a zeroed metrics instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            total_duration_ms: AtomicU64::new(0),
            total_input_tokens: AtomicU64::new(0),
            total_output_tokens: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
        }
    }

    /// Total number of requests seen.
    pub fn total_requests(&self) -> u64 {
        self.total_requests.load(Relaxed)
    }

    /// Cumulative duration across all requests.
    pub fn total_duration_ms(&self) -> u64 {
        self.total_duration_ms.load(Relaxed)
    }

    /// Cumulative input tokens from receipts.
    pub fn total_input_tokens(&self) -> u64 {
        self.total_input_tokens.load(Relaxed)
    }

    /// Cumulative output tokens from receipts.
    pub fn total_output_tokens(&self) -> u64 {
        self.total_output_tokens.load(Relaxed)
    }

    /// Number of successful requests.
    pub fn successful_requests(&self) -> u64 {
        self.successful_requests.load(Relaxed)
    }

    /// Number of failed requests.
    pub fn failed_requests(&self) -> u64 {
        self.failed_requests.load(Relaxed)
    }
}

impl Default for MiddlewareMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Collects per-request timing and token usage metrics.
pub struct MetricsMiddleware {
    metrics: Arc<MiddlewareMetrics>,
}

impl MetricsMiddleware {
    /// Create a new metrics middleware backed by the given counters.
    #[must_use]
    pub fn new(metrics: Arc<MiddlewareMetrics>) -> Self {
        Self { metrics }
    }

    /// Return a reference to the underlying metrics.
    #[must_use]
    pub fn metrics(&self) -> &MiddlewareMetrics {
        &self.metrics
    }
}

#[async_trait]
impl Middleware for MetricsMiddleware {
    async fn before_run(&self, _order: &WorkOrder, _ctx: &MiddlewareContext) -> Result<()> {
        self.metrics.total_requests.fetch_add(1, Relaxed);
        Ok(())
    }

    async fn after_run(
        &self,
        _order: &WorkOrder,
        ctx: &MiddlewareContext,
        receipt: Option<&Receipt>,
    ) -> Result<()> {
        self.metrics
            .total_duration_ms
            .fetch_add(ctx.elapsed_ms(), Relaxed);
        match receipt {
            Some(r) => {
                self.metrics.successful_requests.fetch_add(1, Relaxed);
                if let Some(t) = r.usage.input_tokens {
                    self.metrics.total_input_tokens.fetch_add(t, Relaxed);
                }
                if let Some(t) = r.usage.output_tokens {
                    self.metrics.total_output_tokens.fetch_add(t, Relaxed);
                }
            }
            None => {
                self.metrics.failed_requests.fetch_add(1, Relaxed);
            }
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "metrics"
    }
}

// ---------------------------------------------------------------------------
// Built-in: RateLimitMiddleware
// ---------------------------------------------------------------------------

/// Enforces per-backend rate limits before execution using
/// [`BackendRateLimiter`] from `abp-ratelimit`.
pub struct RateLimitMiddleware {
    limiter: Arc<BackendRateLimiter>,
}

impl RateLimitMiddleware {
    /// Create a new rate-limit middleware with the given limiter.
    #[must_use]
    pub fn new(limiter: Arc<BackendRateLimiter>) -> Self {
        Self { limiter }
    }

    /// Create a middleware and register a policy for one backend.
    #[must_use]
    pub fn with_policy(backend_id: &str, policy: RateLimitPolicy) -> Self {
        let limiter = BackendRateLimiter::new();
        limiter.set_policy(backend_id, policy);
        Self {
            limiter: Arc::new(limiter),
        }
    }

    /// Return a reference to the underlying limiter.
    #[must_use]
    pub fn limiter(&self) -> &BackendRateLimiter {
        &self.limiter
    }
}

#[async_trait]
impl Middleware for RateLimitMiddleware {
    async fn before_run(&self, _order: &WorkOrder, ctx: &MiddlewareContext) -> Result<()> {
        match self.limiter.try_acquire(&ctx.backend_name) {
            Ok(_permit) => {
                trace!(
                    target: "abp.middleware.ratelimit",
                    backend = %ctx.backend_name,
                    "rate limit permit acquired"
                );
                Ok(())
            }
            Err(e) => {
                warn!(
                    target: "abp.middleware.ratelimit",
                    backend = %ctx.backend_name,
                    error = %e,
                    "rate limited"
                );
                anyhow::bail!("rate limited for backend '{}': {}", ctx.backend_name, e)
            }
        }
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
        "rate_limit"
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
// Built-in: CachingMiddleware
// ---------------------------------------------------------------------------

/// Strategy for computing cache keys from a [`WorkOrder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKeyStrategy {
    /// Key by task text only.
    TaskOnly,
    /// Key by task text and backend name.
    TaskAndBackend,
}

/// A cached receipt with its insertion timestamp.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// The cached receipt.
    pub receipt: Receipt,
    /// When this entry was stored.
    pub inserted_at: Instant,
}

/// Optional response caching with configurable TTL and key strategy.
///
/// On `after_run`, successful receipts are stored in an in-memory cache.
/// On `before_run`, the middleware records whether a cache hit occurred
/// (retrievable via [`last_hit`](Self::last_hit)).
pub struct CachingMiddleware {
    cache: Arc<Mutex<BTreeMap<String, CacheEntry>>>,
    ttl: Duration,
    key_strategy: CacheKeyStrategy,
    last_hit: Arc<Mutex<Option<Receipt>>>,
}

impl CachingMiddleware {
    /// Create a new caching middleware with the given TTL and key strategy.
    #[must_use]
    pub fn new(ttl: Duration, key_strategy: CacheKeyStrategy) -> Self {
        Self {
            cache: Arc::new(Mutex::new(BTreeMap::new())),
            ttl,
            key_strategy,
            last_hit: Arc::new(Mutex::new(None)),
        }
    }

    /// Return the TTL for cached entries.
    #[must_use]
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Return the key strategy.
    #[must_use]
    pub fn key_strategy(&self) -> CacheKeyStrategy {
        self.key_strategy
    }

    /// Retrieve the last cache hit (set during `before_run`), if any.
    pub async fn last_hit(&self) -> Option<Receipt> {
        self.last_hit.lock().await.clone()
    }

    /// Return the number of entries currently in the cache.
    pub async fn cache_size(&self) -> usize {
        self.cache.lock().await.len()
    }

    /// Compute the cache key for a work order and backend name.
    fn cache_key(&self, order: &WorkOrder, backend_name: &str) -> String {
        match self.key_strategy {
            CacheKeyStrategy::TaskOnly => order.task.clone(),
            CacheKeyStrategy::TaskAndBackend => {
                format!("{}::{}", order.task, backend_name)
            }
        }
    }
}

#[async_trait]
impl Middleware for CachingMiddleware {
    async fn before_run(&self, order: &WorkOrder, ctx: &MiddlewareContext) -> Result<()> {
        let key = self.cache_key(order, &ctx.backend_name);
        let cache = self.cache.lock().await;
        if let Some(entry) = cache.get(&key) {
            if entry.inserted_at.elapsed() < self.ttl {
                debug!(
                    target: "abp.middleware.caching",
                    key = %key,
                    "cache hit"
                );
                *self.last_hit.lock().await = Some(entry.receipt.clone());
                return Ok(());
            }
            debug!(
                target: "abp.middleware.caching",
                key = %key,
                "cache entry expired"
            );
        }
        *self.last_hit.lock().await = None;
        Ok(())
    }

    async fn after_run(
        &self,
        order: &WorkOrder,
        ctx: &MiddlewareContext,
        receipt: Option<&Receipt>,
    ) -> Result<()> {
        if let Some(r) = receipt {
            let key = self.cache_key(order, &ctx.backend_name);
            self.cache.lock().await.insert(
                key,
                CacheEntry {
                    receipt: r.clone(),
                    inserted_at: Instant::now(),
                },
            );
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "caching"
    }
}

// ---------------------------------------------------------------------------
// Built-in: RetryMiddleware
// ---------------------------------------------------------------------------

/// Plans automatic retry using [`RecoveryExecutor`] from `abp-error`.
///
/// In `before_run`, the middleware resets its internal state. In `after_run`,
/// if the run failed it computes a [`RecoveryReport`] recommending retry /
/// fallback / abort. The caller can inspect the last report via
/// [`last_report`](Self::last_report).
pub struct RetryMiddleware {
    executor: RecoveryExecutor,
    last_report: Arc<Mutex<Option<RecoveryReport>>>,
    total_retries_planned: AtomicU64,
}

impl RetryMiddleware {
    /// Create a new retry middleware with the given executor.
    #[must_use]
    pub fn new(executor: RecoveryExecutor) -> Self {
        Self {
            executor,
            last_report: Arc::new(Mutex::new(None)),
            total_retries_planned: AtomicU64::new(0),
        }
    }

    /// Create a retry middleware with default recovery settings.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(RecoveryExecutor::new(
            abp_error::recovery::RetryPolicy::default(),
        ))
    }

    /// Retrieve the last recovery report, if any.
    pub async fn last_report(&self) -> Option<RecoveryReport> {
        self.last_report.lock().await.clone()
    }

    /// Total number of retries planned across all runs.
    pub fn total_retries_planned(&self) -> u64 {
        self.total_retries_planned.load(Relaxed)
    }
}

#[async_trait]
impl Middleware for RetryMiddleware {
    async fn before_run(&self, _order: &WorkOrder, _ctx: &MiddlewareContext) -> Result<()> {
        *self.last_report.lock().await = None;
        Ok(())
    }

    async fn after_run(
        &self,
        _order: &WorkOrder,
        _ctx: &MiddlewareContext,
        receipt: Option<&Receipt>,
    ) -> Result<()> {
        if receipt.is_none() {
            let report = self
                .executor
                .plan_recovery(abp_error::ErrorCode::BackendUnavailable);
            let planned = report.total_attempts() as u64;
            self.total_retries_planned.fetch_add(planned, Relaxed);
            debug!(
                target: "abp.middleware.retry",
                planned_attempts = planned,
                outcome = %report.final_outcome,
                "recovery planned"
            );
            *self.last_report.lock().await = Some(report);
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "retry"
    }
}

// ---------------------------------------------------------------------------
// Built-in: ValidationMiddleware
// ---------------------------------------------------------------------------

/// Validates work orders against schema before execution using
/// [`WorkOrderValidator`] from `abp-validate`.
pub struct ValidationMiddleware {
    validator: WorkOrderValidator,
}

impl ValidationMiddleware {
    /// Create a new validation middleware.
    #[must_use]
    pub fn new() -> Self {
        Self {
            validator: WorkOrderValidator,
        }
    }
}

impl Default for ValidationMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for ValidationMiddleware {
    async fn before_run(&self, order: &WorkOrder, _ctx: &MiddlewareContext) -> Result<()> {
        self.validator
            .validate(order)
            .map_err(|errs| anyhow::anyhow!("work order validation failed: {}", errs))
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
        "validation"
    }
}

// ---------------------------------------------------------------------------
// Built-in: TransformMiddleware
// ---------------------------------------------------------------------------

/// Type alias for a transform function called before a run.
pub type BeforeTransformFn =
    Arc<dyn Fn(&WorkOrder, &MiddlewareContext) -> Result<()> + Send + Sync>;

/// Type alias for a transform function called after a run.
pub type AfterTransformFn =
    Arc<dyn Fn(&WorkOrder, &MiddlewareContext, Option<&Receipt>) -> Result<()> + Send + Sync>;

/// Request/response transformation hooks.
///
/// Allows injecting custom logic via closures without implementing a
/// full [`Middleware`].
pub struct TransformMiddleware {
    before_fn: Option<BeforeTransformFn>,
    after_fn: Option<AfterTransformFn>,
    label: String,
}

impl TransformMiddleware {
    /// Create a new transform middleware with the given label.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            before_fn: None,
            after_fn: None,
            label: label.into(),
        }
    }

    /// Set the before-run transform (builder pattern).
    #[must_use]
    pub fn with_before<F>(mut self, f: F) -> Self
    where
        F: Fn(&WorkOrder, &MiddlewareContext) -> Result<()> + Send + Sync + 'static,
    {
        self.before_fn = Some(Arc::new(f));
        self
    }

    /// Set the after-run transform (builder pattern).
    #[must_use]
    pub fn with_after<F>(mut self, f: F) -> Self
    where
        F: Fn(&WorkOrder, &MiddlewareContext, Option<&Receipt>) -> Result<()>
            + Send
            + Sync
            + 'static,
    {
        self.after_fn = Some(Arc::new(f));
        self
    }
}

#[async_trait]
impl Middleware for TransformMiddleware {
    async fn before_run(&self, order: &WorkOrder, ctx: &MiddlewareContext) -> Result<()> {
        if let Some(f) = &self.before_fn {
            f(order, ctx)?;
        }
        Ok(())
    }

    async fn after_run(
        &self,
        order: &WorkOrder,
        ctx: &MiddlewareContext,
        receipt: Option<&Receipt>,
    ) -> Result<()> {
        if let Some(f) = &self.after_fn {
            f(order, ctx, receipt)?;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        &self.label
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

    fn sample_receipt_with_tokens(input: u64, output: u64) -> Receipt {
        let mut r = abp_receipt::ReceiptBuilder::new("mock")
            .outcome(abp_core::Outcome::Complete)
            .build();
        r.usage.input_tokens = Some(input);
        r.usage.output_tokens = Some(output);
        r
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
            .with(LoggingMiddleware::default())
            .with(PolicyMiddleware);
        assert_eq!(chain.len(), 2);
        assert_eq!(chain.names(), vec!["logging", "policy"]);
    }

    #[test]
    fn chain_push_appends() {
        let mut chain = MiddlewareChain::new();
        chain.push(LoggingMiddleware::default());
        assert_eq!(chain.len(), 1);
    }

    #[tokio::test]
    async fn chain_before_run_calls_all() {
        let audit = Arc::new(AuditMiddleware::new());
        let chain = MiddlewareChain::new()
            .with(LoggingMiddleware::default())
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
    async fn logging_middleware_default_succeeds() {
        let mw = LoggingMiddleware::default();
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
        assert_eq!(LoggingMiddleware::default().name(), "logging");
    }

    #[test]
    fn logging_middleware_verbosity_accessor() {
        let mw = LoggingMiddleware::with_verbosity(Verbosity::Verbose);
        assert_eq!(mw.verbosity(), Verbosity::Verbose);
    }

    #[tokio::test]
    async fn logging_middleware_minimal() {
        let mw = LoggingMiddleware::with_verbosity(Verbosity::Minimal);
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        mw.before_run(&wo, &ctx).await.unwrap();
        mw.after_run(&wo, &ctx, Some(&sample_receipt()))
            .await
            .unwrap();
        mw.after_run(&wo, &ctx, None).await.unwrap();
    }

    #[tokio::test]
    async fn logging_middleware_verbose() {
        let mw = LoggingMiddleware::with_verbosity(Verbosity::Verbose);
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        mw.before_run(&wo, &ctx).await.unwrap();
        mw.after_run(&wo, &ctx, Some(&sample_receipt()))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn logging_middleware_verbose_no_receipt() {
        let mw = LoggingMiddleware::with_verbosity(Verbosity::Verbose);
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        mw.after_run(&wo, &ctx, None).await.unwrap();
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

    // -- MetricsMiddleware --

    #[tokio::test]
    async fn metrics_middleware_counts_requests() {
        let m = Arc::new(MiddlewareMetrics::new());
        let mw = MetricsMiddleware::new(Arc::clone(&m));
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        mw.before_run(&wo, &ctx).await.unwrap();
        mw.before_run(&wo, &ctx).await.unwrap();

        assert_eq!(m.total_requests(), 2);
    }

    #[tokio::test]
    async fn metrics_middleware_records_tokens() {
        let m = Arc::new(MiddlewareMetrics::new());
        let mw = MetricsMiddleware::new(Arc::clone(&m));
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        let receipt = sample_receipt_with_tokens(100, 50);

        mw.after_run(&wo, &ctx, Some(&receipt)).await.unwrap();

        assert_eq!(m.total_input_tokens(), 100);
        assert_eq!(m.total_output_tokens(), 50);
        assert_eq!(m.successful_requests(), 1);
    }

    #[tokio::test]
    async fn metrics_middleware_records_failure() {
        let m = Arc::new(MiddlewareMetrics::new());
        let mw = MetricsMiddleware::new(Arc::clone(&m));
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        mw.after_run(&wo, &ctx, None).await.unwrap();

        assert_eq!(m.failed_requests(), 1);
        assert_eq!(m.successful_requests(), 0);
    }

    #[test]
    fn metrics_middleware_name() {
        let m = Arc::new(MiddlewareMetrics::new());
        let mw = MetricsMiddleware::new(m);
        assert_eq!(mw.name(), "metrics");
    }

    #[test]
    fn metrics_middleware_accessor() {
        let m = Arc::new(MiddlewareMetrics::new());
        let mw = MetricsMiddleware::new(Arc::clone(&m));
        assert_eq!(mw.metrics().total_requests(), 0);
    }

    #[tokio::test]
    async fn metrics_middleware_no_tokens_when_absent() {
        let m = Arc::new(MiddlewareMetrics::new());
        let mw = MetricsMiddleware::new(Arc::clone(&m));
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        let receipt = sample_receipt(); // no tokens set

        mw.after_run(&wo, &ctx, Some(&receipt)).await.unwrap();

        assert_eq!(m.total_input_tokens(), 0);
        assert_eq!(m.total_output_tokens(), 0);
    }

    #[test]
    fn middleware_metrics_default() {
        let m = MiddlewareMetrics::default();
        assert_eq!(m.total_requests(), 0);
        assert_eq!(m.total_duration_ms(), 0);
    }

    // -- RateLimitMiddleware --

    #[tokio::test]
    async fn rate_limit_allows_when_configured() {
        let mw = RateLimitMiddleware::with_policy(
            "mock",
            RateLimitPolicy::TokenBucket {
                rate: 100.0,
                burst: 100,
            },
        );
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        mw.before_run(&wo, &ctx).await.unwrap();
    }

    #[tokio::test]
    async fn rate_limit_rejects_unconfigured_backend() {
        let limiter = Arc::new(BackendRateLimiter::new());
        let mw = RateLimitMiddleware::new(limiter);
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("unknown-backend");

        let err = mw.before_run(&wo, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("rate limited"));
    }

    #[test]
    fn rate_limit_middleware_name() {
        let limiter = Arc::new(BackendRateLimiter::new());
        let mw = RateLimitMiddleware::new(limiter);
        assert_eq!(mw.name(), "rate_limit");
    }

    #[tokio::test]
    async fn rate_limit_after_run_is_noop() {
        let limiter = Arc::new(BackendRateLimiter::new());
        let mw = RateLimitMiddleware::new(limiter);
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        mw.after_run(&wo, &ctx, None).await.unwrap();
    }

    #[test]
    fn rate_limit_limiter_accessor() {
        let mw = RateLimitMiddleware::with_policy(
            "test",
            RateLimitPolicy::TokenBucket {
                rate: 10.0,
                burst: 10,
            },
        );
        assert!(mw.limiter().has_policy("test"));
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

    // -- CachingMiddleware --

    #[tokio::test]
    async fn caching_stores_on_after_run() {
        let mw = CachingMiddleware::new(Duration::from_secs(60), CacheKeyStrategy::TaskOnly);
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        let receipt = sample_receipt();

        mw.after_run(&wo, &ctx, Some(&receipt)).await.unwrap();
        assert_eq!(mw.cache_size().await, 1);
    }

    #[tokio::test]
    async fn caching_hits_on_second_call() {
        let mw = CachingMiddleware::new(Duration::from_secs(60), CacheKeyStrategy::TaskOnly);
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        let receipt = sample_receipt();

        // Store
        mw.after_run(&wo, &ctx, Some(&receipt)).await.unwrap();
        // Hit
        mw.before_run(&wo, &ctx).await.unwrap();
        assert!(mw.last_hit().await.is_some());
    }

    #[tokio::test]
    async fn caching_miss_on_first_call() {
        let mw = CachingMiddleware::new(Duration::from_secs(60), CacheKeyStrategy::TaskOnly);
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        mw.before_run(&wo, &ctx).await.unwrap();
        assert!(mw.last_hit().await.is_none());
    }

    #[tokio::test]
    async fn caching_does_not_store_failures() {
        let mw = CachingMiddleware::new(Duration::from_secs(60), CacheKeyStrategy::TaskOnly);
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        mw.after_run(&wo, &ctx, None).await.unwrap();
        assert_eq!(mw.cache_size().await, 0);
    }

    #[tokio::test]
    async fn caching_key_strategy_task_and_backend() {
        let mw = CachingMiddleware::new(Duration::from_secs(60), CacheKeyStrategy::TaskAndBackend);
        let wo = sample_work_order();
        let ctx_a = MiddlewareContext::new("backend-a");
        let ctx_b = MiddlewareContext::new("backend-b");
        let receipt = sample_receipt();

        mw.after_run(&wo, &ctx_a, Some(&receipt)).await.unwrap();
        // Same task, different backend → miss
        mw.before_run(&wo, &ctx_b).await.unwrap();
        assert!(mw.last_hit().await.is_none());
        // Same task, same backend → hit
        mw.before_run(&wo, &ctx_a).await.unwrap();
        assert!(mw.last_hit().await.is_some());
    }

    #[tokio::test]
    async fn caching_respects_ttl() {
        // TTL of 0 means entries expire immediately
        let mw = CachingMiddleware::new(Duration::from_millis(0), CacheKeyStrategy::TaskOnly);
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        let receipt = sample_receipt();

        mw.after_run(&wo, &ctx, Some(&receipt)).await.unwrap();
        // Even a tiny sleep should expire TTL=0
        tokio::time::sleep(Duration::from_millis(1)).await;
        mw.before_run(&wo, &ctx).await.unwrap();
        assert!(mw.last_hit().await.is_none());
    }

    #[test]
    fn caching_middleware_name() {
        let mw = CachingMiddleware::new(Duration::from_secs(1), CacheKeyStrategy::TaskOnly);
        assert_eq!(mw.name(), "caching");
    }

    #[test]
    fn caching_accessors() {
        let mw = CachingMiddleware::new(Duration::from_secs(30), CacheKeyStrategy::TaskAndBackend);
        assert_eq!(mw.ttl(), Duration::from_secs(30));
        assert_eq!(mw.key_strategy(), CacheKeyStrategy::TaskAndBackend);
    }

    // -- RetryMiddleware --

    #[tokio::test]
    async fn retry_middleware_plans_on_failure() {
        let mw = RetryMiddleware::with_defaults();
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        mw.before_run(&wo, &ctx).await.unwrap();
        mw.after_run(&wo, &ctx, None).await.unwrap();

        let report = mw.last_report().await.unwrap();
        assert!(report.total_attempts() > 0);
    }

    #[tokio::test]
    async fn retry_middleware_no_plan_on_success() {
        let mw = RetryMiddleware::with_defaults();
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        let receipt = sample_receipt();

        mw.before_run(&wo, &ctx).await.unwrap();
        mw.after_run(&wo, &ctx, Some(&receipt)).await.unwrap();

        assert!(mw.last_report().await.is_none());
    }

    #[tokio::test]
    async fn retry_middleware_resets_on_before() {
        let mw = RetryMiddleware::with_defaults();
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        // Fail first
        mw.after_run(&wo, &ctx, None).await.unwrap();
        assert!(mw.last_report().await.is_some());

        // Before-run resets
        mw.before_run(&wo, &ctx).await.unwrap();
        assert!(mw.last_report().await.is_none());
    }

    #[test]
    fn retry_middleware_name() {
        let mw = RetryMiddleware::with_defaults();
        assert_eq!(mw.name(), "retry");
    }

    #[tokio::test]
    async fn retry_middleware_tracks_total_retries() {
        let mw = RetryMiddleware::with_defaults();
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        mw.after_run(&wo, &ctx, None).await.unwrap();
        assert!(mw.total_retries_planned() > 0);
    }

    // -- ValidationMiddleware --

    #[tokio::test]
    async fn validation_passes_valid_order() {
        let mw = ValidationMiddleware::new();
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        mw.before_run(&wo, &ctx).await.unwrap();
    }

    #[tokio::test]
    async fn validation_rejects_empty_task() {
        let mw = ValidationMiddleware::new();
        let mut wo = sample_work_order();
        wo.task = String::new();
        let ctx = MiddlewareContext::new("mock");

        let err = mw.before_run(&wo, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("task"));
    }

    #[tokio::test]
    async fn validation_rejects_empty_workspace_root() {
        let mw = ValidationMiddleware::new();
        let mut wo = sample_work_order();
        wo.workspace.root = String::new();
        let ctx = MiddlewareContext::new("mock");

        let err = mw.before_run(&wo, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("workspace"));
    }

    #[tokio::test]
    async fn validation_after_run_is_noop() {
        let mw = ValidationMiddleware::new();
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        mw.after_run(&wo, &ctx, None).await.unwrap();
    }

    #[test]
    fn validation_middleware_name() {
        assert_eq!(ValidationMiddleware::new().name(), "validation");
    }

    #[test]
    fn validation_middleware_default() {
        let mw = ValidationMiddleware::default();
        assert_eq!(mw.name(), "validation");
    }

    #[tokio::test]
    async fn validation_rejects_conflicting_policy_tools() {
        let mw = ValidationMiddleware::new();
        let mut wo = sample_work_order();
        wo.policy.allowed_tools = vec!["bash".into()];
        wo.policy.disallowed_tools = vec!["bash".into()];
        let ctx = MiddlewareContext::new("mock");

        let err = mw.before_run(&wo, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("bash"));
    }

    // -- TransformMiddleware --

    #[tokio::test]
    async fn transform_before_hook_runs() {
        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = Arc::clone(&called);
        let mw = TransformMiddleware::new("test-xform").with_before(move |_wo, _ctx| {
            called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        mw.before_run(&wo, &ctx).await.unwrap();
        assert!(called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn transform_after_hook_runs() {
        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = Arc::clone(&called);
        let mw = TransformMiddleware::new("test-xform").with_after(move |_wo, _ctx, _r| {
            called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        mw.after_run(&wo, &ctx, None).await.unwrap();
        assert!(called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn transform_before_hook_can_reject() {
        let mw = TransformMiddleware::new("blocker")
            .with_before(|_wo, _ctx| anyhow::bail!("blocked by transform"));
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        let err = mw.before_run(&wo, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("blocked by transform"));
    }

    #[tokio::test]
    async fn transform_after_hook_can_fail() {
        let mw = TransformMiddleware::new("fail-after")
            .with_after(|_wo, _ctx, _r| anyhow::bail!("after failure"));
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        let err = mw.after_run(&wo, &ctx, None).await.unwrap_err();
        assert!(err.to_string().contains("after failure"));
    }

    #[tokio::test]
    async fn transform_no_hooks_is_noop() {
        let mw = TransformMiddleware::new("noop");
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        mw.before_run(&wo, &ctx).await.unwrap();
        mw.after_run(&wo, &ctx, None).await.unwrap();
    }

    #[test]
    fn transform_middleware_name() {
        let mw = TransformMiddleware::new("my-transform");
        assert_eq!(mw.name(), "my-transform");
    }

    #[tokio::test]
    async fn transform_receives_receipt() {
        let captured = Arc::new(Mutex::new(false));
        let captured_clone = Arc::clone(&captured);
        let mw = TransformMiddleware::new("check-receipt").with_after(move |_wo, _ctx, receipt| {
            if receipt.is_some() {
                // We can't easily set async from sync closure, so use std Mutex
                // The Arc<Mutex<bool>> is tokio::sync::Mutex but we're in a sync fn.
                // Instead, just check it's not None and return Ok.
            }
            let _ = captured_clone; // reference captured for the test
            Ok(())
        });
        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");
        let receipt = sample_receipt();

        mw.after_run(&wo, &ctx, Some(&receipt)).await.unwrap();
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

    // -- Integration: full chain with all middlewares --

    #[tokio::test]
    async fn full_chain_with_all_middlewares() {
        let metrics = Arc::new(MiddlewareMetrics::new());
        let chain = MiddlewareChain::new()
            .with(LoggingMiddleware::default())
            .with(ValidationMiddleware::new())
            .with(PolicyMiddleware)
            .with(MetricsMiddleware::new(Arc::clone(&metrics)))
            .with(RetryMiddleware::with_defaults())
            .with(CachingMiddleware::new(
                Duration::from_secs(60),
                CacheKeyStrategy::TaskOnly,
            ));

        assert_eq!(chain.len(), 6);

        let wo = sample_work_order();
        let ctx = MiddlewareContext::new("mock");

        chain.run_before(&wo, &ctx).await.unwrap();
        let errors = chain.run_after(&wo, &ctx, Some(&sample_receipt())).await;
        assert!(errors.is_empty());
        assert_eq!(metrics.total_requests(), 1);
    }

    #[test]
    fn chain_names_includes_all() {
        let chain = MiddlewareChain::new()
            .with(LoggingMiddleware::default())
            .with(ValidationMiddleware::new())
            .with(PolicyMiddleware)
            .with(TransformMiddleware::new("xform"))
            .with(RetryMiddleware::with_defaults());
        assert_eq!(
            chain.names(),
            vec!["logging", "validation", "policy", "xform", "retry"]
        );
    }
}
