// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//!
//! # Runtime orchestration
//!
//! `abp-runtime` is the top-level orchestrator for the Agent Backplane.  It
//! owns the full lifecycle of a work-order run:
//!
//! 1. **Workspace preparation** — copies or stages the workspace via
//!    `abp-workspace`, initialises git for verification diffs.
//! 2. **Policy compilation** — compiles the work order's `PolicyProfile`
//!    into a `PolicyEngine` (from `abp-policy`).
//! 3. **Capability negotiation** — checks that the selected backend
//!    satisfies the work order's capability requirements (via
//!    `abp-capability`), applying emulation where configured.
//! 4. **Backend execution** — dispatches the work order to a registered
//!    `Backend` implementation and streams `AgentEvent`s back to the caller.
//! 5. **Receipt production** — builds a canonical `Receipt` with
//!    verification metadata, traces, and a SHA-256 hash.
//!
//! ## Key types
//!
//! * [`Runtime`] — the central orchestrator; register backends, then call
//!   `run_streaming` or `run_projected`.
//! * [`RunHandle`] — handle returned from a run: provides the event stream
//!   and a receipt future.
//! * [`RuntimeError`] — error enum covering all runtime failure modes.
//! * [`BackendRegistry`] — named backend lookup table.
//!
//! ## Middleware & pipeline
//!
//! The [`middleware`] module provides pre/post-run hooks, and [`pipeline`]
//! offers a processing pipeline for work-order pre-processing.  The
//! [`stream`] module integrates with `abp-stream` for event filtering,
//! transformation, and recording.
//!
//! ## Projection-based routing
//!
//! When a [`ProjectionMatrix`] is attached (via [`Runtime::with_projection`]),
//! the runtime can automatically select the best-fit backend for a work
//! order using [`Runtime::run_projected`].

#![deny(unsafe_code)]
#![warn(missing_docs)]

/// Budget enforcement for runtime runs.
pub mod budget;
/// Broadcast-based event bus for decoupled event distribution.
pub mod bus;
/// Cancellation primitives for runtime runs.
pub mod cancel;
/// Runtime configuration integration (backend selection, telemetry, workspace).
pub mod config_integration;
/// Retry-and-fallback execution pipeline (parallel path to [`Runtime::run_streaming`]).
pub mod execution;
/// Lifecycle hooks for runtime extensibility.
pub mod hooks;
/// Middleware pattern for pre/post run hooks.
pub mod middleware;
/// Event multiplexing and routing for broadcasting agent events.
pub mod multiplex;
/// Combined capability negotiation result for the runtime pipeline.
pub mod negotiate;
/// Observability primitives: tracing spans and runtime observer.
pub mod observe;
/// Processing pipeline for work order pre-processing.
pub mod pipeline;
/// Backend registry for named backend lookup.
pub mod registry;
/// Retry policies and timeout configuration for resilient backend execution.
pub mod retry;
/// Additional built-in pipeline stages, builder, and execution helpers.
pub mod stages;
/// Receipt persistence and retrieval.
pub mod store;
/// Stream pipeline integration for event filtering, transformation, and recording.
pub mod stream;
/// Telemetry and metrics collection.
pub mod telemetry;

use abp_core::{AgentEvent, CapabilityRequirements, Outcome, Receipt, WorkOrder};
use abp_dialect::Dialect;
use abp_emulation::{EmulationConfig, EmulationEngine, EmulationReport};
use abp_integrations::{Backend, ensure_capability_requirements};
use abp_policy::PolicyEngine;
use abp_projection::translate::{TranslationEngine, TranslationMode, TranslationResult};
use abp_receipt::{ReceiptBuilder, ReceiptChain};
use abp_workspace::WorkspaceManager;
use anyhow::Context;
use std::sync::Arc;
use telemetry::RunMetrics;
use thiserror::Error;
use tokio::sync::{Mutex, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Re-export of [`registry::BackendRegistry`] for convenience.
pub use registry::BackendRegistry;

/// Re-export combined negotiation result type.
pub use negotiate::NegotiationResult;

/// Re-export projection types for callers that use projection-based routing.
pub use abp_projection::{self, ProjectionMatrix, ProjectionResult, ProjectionScore};

/// Re-export the translation engine for callers that need direct access.
pub use abp_projection::translate::TranslationEngine as ProjectionTranslationEngine;

/// Re-export receipt chain and builder from `abp-receipt`.
pub use abp_receipt::{self, ReceiptChain as ReceiptChainType};

/// Errors from the ABP runtime orchestrator.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// The requested backend name is not registered in the [`BackendRegistry`].
    #[error("unknown backend: {name}")]
    UnknownBackend {
        /// Name that was looked up.
        name: String,
    },

    /// Workspace staging (temp-dir copy, git init) failed.
    #[error("workspace preparation failed")]
    WorkspaceFailed(#[source] anyhow::Error),

    /// The [`PolicyEngine`] could not compile the work order's policy globs.
    #[error("policy compilation failed")]
    PolicyFailed(#[source] anyhow::Error),

    /// The backend returned an error during execution.
    #[error("backend execution failed")]
    BackendFailed(#[source] anyhow::Error),

    /// Pre-flight capability requirements were not satisfied.
    #[error("capability check failed: {0}")]
    CapabilityCheckFailed(String),

    /// An error from the unified ABP error taxonomy.
    #[error("{0}")]
    Classified(#[from] abp_error::AbpError),

    /// The projection matrix could not find a suitable backend.
    #[error("projection failed: {reason}")]
    NoProjectionMatch {
        /// Human-readable explanation.
        reason: String,
    },
}

impl RuntimeError {
    /// Return the [`ErrorCode`](abp_error::ErrorCode) for this error, if one applies.
    ///
    /// Classified errors always carry a code; other variants are mapped to
    /// the most appropriate code from the taxonomy.
    pub fn error_code(&self) -> abp_error::ErrorCode {
        match self {
            Self::UnknownBackend { .. } => abp_error::ErrorCode::BackendNotFound,
            Self::WorkspaceFailed(_) => abp_error::ErrorCode::WorkspaceInitFailed,
            Self::PolicyFailed(_) => abp_error::ErrorCode::PolicyInvalid,
            Self::BackendFailed(_) => abp_error::ErrorCode::BackendCrashed,
            Self::CapabilityCheckFailed(_) => abp_error::ErrorCode::CapabilityUnsupported,
            Self::Classified(e) => e.code,
            Self::NoProjectionMatch { .. } => abp_error::ErrorCode::BackendNotFound,
        }
    }

    /// Convert this runtime error into an [`AbpError`](abp_error::AbpError).
    pub fn into_abp_error(self) -> abp_error::AbpError {
        match self {
            Self::Classified(e) => e,
            other => {
                let code = other.error_code();
                let message = other.to_string();
                abp_error::AbpError::new(code, message)
            }
        }
    }

    /// Returns `true` when the error is transient and the operation could
    /// reasonably succeed on a subsequent attempt.
    ///
    /// Errors that are inherently permanent (unknown backend, policy
    /// compilation, capability mismatch) always return `false`.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            // Transient — backend may recover on retry.
            Self::BackendFailed(_) | Self::WorkspaceFailed(_) => true,
            // Classified errors delegate to the error taxonomy.
            Self::Classified(e) => e.is_retryable(),
            // Everything else is a permanent configuration/logic error.
            Self::UnknownBackend { .. }
            | Self::PolicyFailed(_)
            | Self::CapabilityCheckFailed(_)
            | Self::NoProjectionMatch { .. } => false,
        }
    }
}

/// Central orchestrator that holds registered backends and executes work orders.
///
/// ```no_run
/// # use abp_runtime::Runtime;
/// let mut rt = Runtime::with_default_backends();
/// // rt.register_backend("sidecar:node", my_sidecar);
/// ```
pub struct Runtime {
    backends: BackendRegistry,
    metrics: Arc<RunMetrics>,
    emulation: Option<EmulationConfig>,
    receipt_chain: Arc<Mutex<ReceiptChain>>,
    projection: Option<ProjectionMatrix>,
    stream_pipeline: Option<abp_stream::StreamPipeline>,
    translation_engine: Arc<TranslationEngine>,
}

/// Handle to a running work order: provides a run id, event stream, and receipt future.
pub struct RunHandle {
    /// Unique identifier for this run.
    pub run_id: Uuid,
    /// Stream of [`AgentEvent`]s emitted during execution.
    pub events: ReceiverStream<AgentEvent>,
    /// Future that resolves to the final [`Receipt`] or an error.
    pub receipt: tokio::task::JoinHandle<Result<Receipt, RuntimeError>>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    /// Create an empty runtime with no backends registered.
    #[must_use]
    pub fn new() -> Self {
        Self {
            backends: BackendRegistry::default(),
            metrics: Arc::new(RunMetrics::new()),
            emulation: None,
            receipt_chain: Arc::new(Mutex::new(ReceiptChain::new())),
            projection: None,
            stream_pipeline: None,
            translation_engine: Arc::new(TranslationEngine::with_defaults()),
        }
    }

    /// Create a runtime pre-loaded with the [`MockBackend`](abp_integrations::MockBackend).
    #[must_use]
    pub fn with_default_backends() -> Self {
        let mut rt = Self::new();
        rt.register_backend("mock", abp_integrations::MockBackend);
        rt
    }

    /// Register a backend under the given name, replacing any previous registration.
    pub fn register_backend<B: Backend + 'static>(&mut self, name: &str, backend: B) {
        self.backends.register(name, backend);
    }

    /// Return a sorted list of all registered backend names.
    #[must_use]
    pub fn backend_names(&self) -> Vec<String> {
        self.backends.list().into_iter().map(String::from).collect()
    }

    /// Look up a backend by name.
    #[must_use]
    pub fn backend(&self, name: &str) -> Option<Arc<dyn Backend>> {
        self.backends.get_arc(name)
    }

    /// Return a reference to the underlying [`BackendRegistry`].
    #[must_use]
    pub fn registry(&self) -> &BackendRegistry {
        &self.backends
    }

    /// Return a mutable reference to the underlying [`BackendRegistry`].
    pub fn registry_mut(&mut self) -> &mut BackendRegistry {
        &mut self.backends
    }

    /// Return a reference to the shared [`RunMetrics`] collector.
    #[must_use]
    pub fn metrics(&self) -> &RunMetrics {
        &self.metrics
    }

    /// Enable capability emulation with the given configuration.
    ///
    /// When emulation is enabled and a backend is missing required capabilities,
    /// the runtime will check if the missing capabilities can be emulated. If so,
    /// the run proceeds and the emulation report is recorded in the receipt.
    #[must_use]
    pub fn with_emulation(mut self, config: EmulationConfig) -> Self {
        self.emulation = Some(config);
        self
    }

    /// Return the current emulation configuration, if any.
    #[must_use]
    pub fn emulation_config(&self) -> Option<&EmulationConfig> {
        self.emulation.as_ref()
    }

    /// Set a [`ProjectionMatrix`] for capability-aware backend selection.
    ///
    /// When a projection matrix is configured, [`select_backend`](Self::select_backend)
    /// can rank registered backends and [`run_projected`](Self::run_projected) can
    /// execute a work order against the best-fit backend automatically.
    #[must_use]
    pub fn with_projection(mut self, matrix: ProjectionMatrix) -> Self {
        self.projection = Some(matrix);
        self
    }

    /// Return the current projection matrix, if any.
    #[must_use]
    pub fn projection(&self) -> Option<&ProjectionMatrix> {
        self.projection.as_ref()
    }

    /// Return a mutable reference to the projection matrix, if any.
    pub fn projection_mut(&mut self) -> Option<&mut ProjectionMatrix> {
        self.projection.as_mut()
    }

    /// Attach a [`StreamPipeline`](abp_stream::StreamPipeline) that every event
    /// passes through before being forwarded to the caller.
    ///
    /// The pipeline can filter, transform, record, and collect statistics on
    /// events in-flight.
    #[must_use]
    pub fn with_stream_pipeline(mut self, pipeline: abp_stream::StreamPipeline) -> Self {
        self.stream_pipeline = Some(pipeline);
        self
    }

    /// Return the current stream pipeline, if any.
    #[must_use]
    pub fn stream_pipeline(&self) -> Option<&abp_stream::StreamPipeline> {
        self.stream_pipeline.as_ref()
    }

    /// Return a reference to the runtime's [`TranslationEngine`].
    #[must_use]
    pub fn translation_engine(&self) -> &TranslationEngine {
        &self.translation_engine
    }

    /// Replace the translation engine (builder pattern).
    #[must_use]
    pub fn with_translation_engine(mut self, engine: TranslationEngine) -> Self {
        self.translation_engine = Arc::new(engine);
        self
    }

    /// Use the projection matrix to select the best backend for a work order.
    ///
    /// Returns a [`ProjectionResult`] containing the selected backend name,
    /// its composite score, any required emulations, and a fallback chain.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NoProjectionMatch`] if the projection matrix
    /// is not configured or no backend satisfies the work order.
    /// Returns [`RuntimeError::UnknownBackend`] if the selected backend is
    /// not registered in the runtime's [`BackendRegistry`].
    pub fn select_backend(&self, work_order: &WorkOrder) -> Result<ProjectionResult, RuntimeError> {
        let matrix = self
            .projection
            .as_ref()
            .ok_or_else(|| RuntimeError::NoProjectionMatch {
                reason: "no projection matrix configured".into(),
            })?;

        let result = matrix
            .project(work_order)
            .map_err(|e| RuntimeError::NoProjectionMatch {
                reason: e.to_string(),
            })?;

        // Verify the selected backend is actually registered in the runtime.
        if !self.backends.contains(&result.selected_backend) {
            return Err(RuntimeError::UnknownBackend {
                name: result.selected_backend.clone(),
            });
        }

        Ok(result)
    }

    /// Execute a work order using the projection matrix to select the best backend.
    ///
    /// This is a convenience wrapper around [`select_backend`](Self::select_backend)
    /// followed by [`run_streaming`](Self::run_streaming). The projection result
    /// metadata is recorded in the receipt's `usage_raw`.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NoProjectionMatch`] if no suitable backend is found,
    /// or any error that [`run_streaming`](Self::run_streaming) can return.
    pub async fn run_projected(&self, work_order: WorkOrder) -> Result<RunHandle, RuntimeError> {
        let projection_result = self.select_backend(&work_order)?;

        info!(
            target: "abp.runtime",
            selected = %projection_result.selected_backend,
            score = %projection_result.fidelity_score.total,
            fallbacks = projection_result.fallback_chain.len(),
            "projection selected backend"
        );

        self.run_streaming(&projection_result.selected_backend, work_order)
            .await
    }

    /// Return a reference to the shared receipt chain.
    ///
    /// The chain accumulates receipts from successive [`run_streaming`](Self::run_streaming)
    /// calls, enabling multi-step receipt verification and diffing.
    #[must_use]
    pub fn receipt_chain(&self) -> Arc<Mutex<ReceiptChain>> {
        Arc::clone(&self.receipt_chain)
    }

    /// Check whether a backend's capabilities satisfy the given requirements.
    ///
    /// For sidecar backends whose capabilities come from handshake, this will
    /// check against the (empty) default manifest — use the in-backend check
    /// for authoritative validation.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnknownBackend`] if the backend is not registered,
    /// or [`RuntimeError::CapabilityCheckFailed`] if requirements are not met.
    pub fn check_capabilities(
        &self,
        backend_name: &str,
        requirements: &CapabilityRequirements,
    ) -> Result<(), RuntimeError> {
        let backend = self
            .backend(backend_name)
            .ok_or_else(|| RuntimeError::UnknownBackend {
                name: backend_name.to_string(),
            })?;
        ensure_capability_requirements(requirements, &backend.capabilities()).map_err(|e| {
            RuntimeError::CapabilityCheckFailed(format!("backend '{backend_name}': {e}"))
        })?;
        Ok(())
    }

    /// Execute a work order against the named backend, returning a [`RunHandle`].
    ///
    /// The handle provides a streaming event channel and a receipt future.
    /// The runtime prepares the workspace, compiles the policy, and attaches
    /// verification metadata and receipt hash after the backend finishes.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnknownBackend`] if the named backend is not
    /// registered, or [`RuntimeError::CapabilityCheckFailed`] if pre-flight
    /// capability checks fail.
    pub async fn run_streaming(
        &self,
        backend_name: &str,
        work_order: WorkOrder,
    ) -> Result<RunHandle, RuntimeError> {
        let backend = self.backend(backend_name).ok_or_else(|| {
            warn!(target: "abp.runtime", name = %backend_name, "unknown backend");
            RuntimeError::UnknownBackend {
                name: backend_name.to_string(),
            }
        })?;

        // Pre-flight capability check: skip for sidecar backends whose
        // capabilities are only known after handshake (empty default manifest).
        let caps = backend.capabilities();
        let emulation_report: Option<EmulationReport> = if !caps.is_empty() {
            match ensure_capability_requirements(&work_order.requirements, &caps) {
                Ok(()) => None,
                Err(e) => {
                    // Capabilities unsatisfied — try emulation if configured.
                    if let Some(emu_config) = &self.emulation {
                        let missing: Vec<_> = work_order
                            .requirements
                            .required
                            .iter()
                            .filter(|req| !caps.contains_key(&req.capability))
                            .map(|req| req.capability.clone())
                            .collect();

                        let engine = EmulationEngine::new(emu_config.clone());
                        let report = engine.check_missing(&missing);

                        if report.has_unemulatable() {
                            return Err(RuntimeError::CapabilityCheckFailed(format!(
                                "backend '{backend_name}': {e} (emulation unavailable for some capabilities)"
                            )));
                        }

                        info!(
                            target: "abp.runtime",
                            backend=%backend_name,
                            emulated=?report.applied.iter().map(|e| &e.capability).collect::<Vec<_>>(),
                            "emulating missing capabilities"
                        );
                        Some(report)
                    } else {
                        return Err(RuntimeError::CapabilityCheckFailed(format!(
                            "backend '{backend_name}': {e}"
                        )));
                    }
                }
            }
        } else {
            None
        };

        let backend_name = backend_name.to_string();
        let run_id = Uuid::new_v4();
        let metrics = Arc::clone(&self.metrics);

        // Resolve source and target dialects for translation.
        let source_dialect = extract_dialect(&work_order);
        let target_dialect = resolve_backend_dialect(self.projection.as_ref(), &backend_name);
        let translation_engine = Arc::clone(&self.translation_engine);

        // Two-stage channel: backend -> runtime -> caller
        let (from_backend_tx, mut from_backend_rx) = mpsc::channel::<AgentEvent>(256);
        let (to_caller_tx, to_caller_rx) = mpsc::channel::<AgentEvent>(256);

        let receipt_chain = Arc::clone(&self.receipt_chain);
        let pipeline = self.stream_pipeline.clone();

        let receipt = tokio::spawn(async move {
            let run_start = std::time::Instant::now();

            // Keep the prepared workspace alive for the duration of the run.
            let prepared = WorkspaceManager::prepare(&work_order.workspace)
                .context("prepare workspace")
                .map_err(RuntimeError::WorkspaceFailed)?;

            // Clone and rewrite the work order to point at prepared workspace.
            let mut wo = work_order.clone();
            wo.workspace.root = prepared.path().to_string_lossy().to_string();

            // Strip emulated capability requirements so the backend's own check
            // does not reject capabilities the runtime is emulating.
            if let Some(ref report) = emulation_report {
                let emulated_caps: std::collections::BTreeSet<_> =
                    report.applied.iter().map(|e| &e.capability).collect();
                wo.requirements
                    .required
                    .retain(|r| !emulated_caps.contains(&r.capability));
            }

            // Compile policy globs (even if adapters do the heavy lifting).
            let _policy = PolicyEngine::new(&wo.policy)
                .context("compile policy")
                .map_err(RuntimeError::PolicyFailed)?;

            // Capability negotiation via abp-capability crate.
            let negotiation_result = {
                let manifest = backend.capabilities();
                if !manifest.is_empty() {
                    let result = abp_capability::negotiate(&manifest, &work_order.requirements);
                    if !result.is_compatible() {
                        // Check if unsupported capabilities are covered by runtime emulation.
                        let truly_unsupported: Vec<_> = match emulation_report {
                            Some(ref emu) => {
                                let emulated_caps: std::collections::BTreeSet<_> =
                                    emu.applied.iter().map(|e| &e.capability).collect();
                                result
                                    .unsupported_caps()
                                    .into_iter()
                                    .filter(|c| !emulated_caps.contains(c))
                                    .collect()
                            }
                            None => result.unsupported_caps(),
                        };
                        if !truly_unsupported.is_empty() {
                            let names: Vec<String> =
                                truly_unsupported.iter().map(|c| format!("{c:?}")).collect();
                            return Err(RuntimeError::CapabilityCheckFailed(format!(
                                "backend '{backend_name}': unsupported capabilities: {}",
                                names.join(", ")
                            )));
                        }
                    }
                    if !result.emulated.is_empty() {
                        warn!(
                            target: "abp.runtime",
                            backend=%backend_name,
                            emulated=?result.emulated_caps(),
                            "capabilities require emulation"
                        );
                    }
                    Some(result)
                } else {
                    None
                }
            };

            debug!(target: "abp.runtime", backend=%backend_name, run_id=%run_id, "starting run");

            // ── Dialect translation layer ────────────────────────────────
            // Detect whether cross-dialect translation is needed and classify
            // the translation mode.  When source ≠ target, the translation
            // engine is used to validate the pair and collect capability gaps.
            let translation_meta: Option<TranslationResult> = match (source_dialect, target_dialect)
            {
                (Some(src), Some(tgt)) if src != tgt => {
                    // Build a probe conversation from the work order task to
                    // detect capability gaps and validate the mapping pair.
                    let probe = abp_core::ir::IrConversation::from_messages(vec![
                        abp_core::ir::IrMessage::text(abp_core::ir::IrRole::User, &wo.task),
                    ]);
                    match translation_engine.translate(src, tgt, &probe) {
                        Ok(result) => {
                            info!(
                                target: "abp.runtime",
                                from = %src,
                                to = %tgt,
                                mode = %result.mode,
                                gaps = result.gaps.len(),
                                "dialect translation active"
                            );
                            Some(result)
                        }
                        Err(e) => {
                            warn!(
                                target: "abp.runtime",
                                from = %src,
                                to = %tgt,
                                error = %e,
                                "dialect translation not available, proceeding without"
                            );
                            None
                        }
                    }
                }
                (Some(d), Some(t)) if d == t => {
                    debug!(
                        target: "abp.runtime",
                        dialect = %d,
                        "passthrough — source and target dialects match"
                    );
                    Some(TranslationResult {
                        conversation: abp_core::ir::IrConversation::new(),
                        from: d,
                        to: t,
                        mode: TranslationMode::Passthrough,
                        gaps: Vec::new(),
                    })
                }
                _ => {
                    debug!(target: "abp.runtime", "no dialect translation — dialect(s) not specified");
                    None
                }
            };

            // Run backend in a task so we can multiplex events.
            let backend2 = backend.clone();
            let mut backend_handle =
                tokio::spawn(async move { backend2.run(run_id, wo, from_backend_tx).await });

            let mut trace: Vec<AgentEvent> = Vec::new();
            let mut receipt_opt: Option<Receipt> = None;

            loop {
                tokio::select! {
                    ev = from_backend_rx.recv() => {
                        match ev {
                            Some(ev) => {
                                if let Some(ev) = stream::apply_pipeline(pipeline.as_ref(), ev) {
                                    trace.push(ev.clone());
                                    let _ = to_caller_tx.send(ev).await;
                                }
                            }
                            None => break,
                        }
                    }
                    res = &mut backend_handle => {
                        let r = match res {
                            Ok(Ok(receipt)) => receipt,
                            Ok(Err(e)) => return Err(RuntimeError::BackendFailed(
                                e.context(format!("backend '{backend_name}'")),
                            )),
                            Err(e) => return Err(RuntimeError::BackendFailed(
                                anyhow::Error::new(e).context(format!("backend '{backend_name}' task panicked")),
                            )),
                        };
                        receipt_opt = Some(r);
                        break;
                    }
                }
            }

            // Drain any remaining events (best-effort).
            while let Some(ev) = from_backend_rx.recv().await {
                if let Some(ev) = stream::apply_pipeline(pipeline.as_ref(), ev) {
                    trace.push(ev.clone());
                    let _ = to_caller_tx.send(ev).await;
                }
            }

            // If the channel closed before the select polled the backend handle,
            // await it now so we don't lose the real receipt or error.
            if receipt_opt.is_none() {
                match backend_handle.await {
                    Ok(Ok(r)) => receipt_opt = Some(r),
                    Ok(Err(e)) => {
                        return Err(RuntimeError::BackendFailed(
                            e.context(format!("backend '{backend_name}'")),
                        ));
                    }
                    Err(e) => {
                        return Err(RuntimeError::BackendFailed(
                            anyhow::Error::new(e)
                                .context(format!("backend '{backend_name}' task panicked")),
                        ));
                    }
                }
            }

            drop(to_caller_tx);

            let mut receipt = receipt_opt.unwrap_or_else(|| {
                // Backend crashed before returning a receipt — build via ReceiptBuilder.
                let identity = backend.identity();
                ReceiptBuilder::new(&identity.id)
                    .backend_version(identity.backend_version.unwrap_or_default())
                    .adapter_version(identity.adapter_version.unwrap_or_default())
                    .capabilities(backend.capabilities())
                    .run_id(run_id)
                    .work_order_id(work_order.id)
                    .outcome(Outcome::Failed)
                    .usage_raw(serde_json::json!({"error": "no receipt"}))
                    .build()
            });

            // If backend didn't include a trace, attach what we observed.
            if receipt.trace.is_empty() {
                receipt.trace = trace;
            }

            // Fill verification if missing.
            if receipt.verification.git_diff.is_none() {
                receipt.verification.git_diff = WorkspaceManager::git_diff(prepared.path());
            }
            if receipt.verification.git_status.is_none() {
                receipt.verification.git_status = WorkspaceManager::git_status(prepared.path());
            }

            // Record emulation report in receipt metadata if emulation was applied.
            if let Some(ref emu_report) = emulation_report
                && let (false, Ok(report_value)) =
                    (emu_report.is_empty(), serde_json::to_value(emu_report))
            {
                if let Some(obj) = receipt.usage_raw.as_object_mut() {
                    obj.insert("emulation".to_string(), report_value);
                } else {
                    receipt.usage_raw = serde_json::json!({
                        "original": receipt.usage_raw,
                        "emulation": report_value,
                    });
                }
            }

            // Record capability negotiation result in receipt metadata.
            if let Some(ref neg_result) = negotiation_result
                && let Ok(neg_value) = serde_json::to_value(neg_result)
                && let Some(obj) = receipt.usage_raw.as_object_mut()
            {
                obj.insert("capability_negotiation".to_string(), neg_value);
            }

            // Record dialect translation metadata in receipt.
            if let Some(ref tr) = translation_meta {
                let translation_value = serde_json::json!({
                    "source_dialect": tr.from.label(),
                    "target_dialect": tr.to.label(),
                    "translation_mode": tr.mode.to_string(),
                    "capability_gaps": tr.gaps.iter().map(|g| serde_json::json!({
                        "feature": format!("{:?}", g.feature),
                        "source": g.source.label(),
                        "target": g.target.label(),
                        "description": g.description,
                    })).collect::<Vec<_>>(),
                });
                if let Some(obj) = receipt.usage_raw.as_object_mut() {
                    obj.insert("dialect_translation".to_string(), translation_value);
                } else {
                    receipt.usage_raw = serde_json::json!({
                        "original": receipt.usage_raw,
                        "dialect_translation": translation_value,
                    });
                }
            }

            // Build and record combined negotiation result.
            {
                let combined = match &negotiation_result {
                    Some(neg) => negotiate::NegotiationResult::from_negotiation(
                        neg,
                        emulation_report.as_ref(),
                    ),
                    None => negotiate::NegotiationResult::all_native(vec![]),
                };
                if let Ok(val) = serde_json::to_value(&combined) {
                    if let Some(obj) = receipt.usage_raw.as_object_mut() {
                        obj.insert("negotiation_result".to_string(), val);
                    }
                }
            }

            // Ensure receipt hash is present and consistent via abp-receipt.
            receipt.receipt_sha256 = Some(
                abp_receipt::compute_hash(&receipt)
                    .context("hash receipt")
                    .map_err(RuntimeError::BackendFailed)?,
            );

            // Append to the runtime's receipt chain for multi-step tracking.
            {
                let mut chain = receipt_chain.lock().await;
                // Best-effort: log but don't fail the run if chain push fails.
                if let Err(e) = chain.push(receipt.clone()) {
                    warn!(target: "abp.runtime", error=%e, "failed to append receipt to chain");
                }
            }

            // Record telemetry.
            let duration_ms = run_start.elapsed().as_millis() as u64;
            let success = matches!(receipt.outcome, Outcome::Complete | Outcome::Partial);
            let event_count = receipt.trace.len() as u64;
            metrics.record_run(duration_ms, success, event_count);

            Ok(receipt)
        });

        Ok(RunHandle {
            run_id,
            events: ReceiverStream::new(to_caller_rx),
            receipt,
        })
    }
}

// ---------------------------------------------------------------------------
// Dialect extraction helpers
// ---------------------------------------------------------------------------

/// Extract the source dialect from a work order's vendor config.
///
/// Checks `config.vendor["abp"]["dialect"]` first, then falls back to
/// `config.vendor["abp.dialect"]`. Returns `None` if not specified.
#[must_use]
pub fn extract_dialect(work_order: &WorkOrder) -> Option<Dialect> {
    // Nested: config.vendor.abp.dialect
    if let Some(abp_val) = work_order.config.vendor.get("abp")
        && let Some(dialect_str) = abp_val.get("dialect").and_then(|v| v.as_str())
    {
        return parse_dialect_str(dialect_str);
    }

    // Flat: config.vendor["abp.dialect"]
    if let Some(val) = work_order.config.vendor.get("abp.dialect")
        && let Some(dialect_str) = val.as_str()
    {
        return parse_dialect_str(dialect_str);
    }

    None
}

/// Infer a backend's native dialect from its name.
///
/// Common patterns: `"sidecar:claude"` → Claude, `"openai"` → OpenAI, etc.
#[must_use]
pub fn infer_dialect_from_backend(backend_name: &str) -> Option<Dialect> {
    let lower = backend_name.to_lowercase();
    if lower.contains("claude") {
        Some(Dialect::Claude)
    } else if lower.contains("gemini") {
        Some(Dialect::Gemini)
    } else if lower.contains("codex") {
        Some(Dialect::Codex)
    } else if lower.contains("kimi") || lower.contains("moonshot") {
        Some(Dialect::Kimi)
    } else if lower.contains("copilot") {
        Some(Dialect::Copilot)
    } else if lower.contains("openai") || lower.contains("gpt") {
        Some(Dialect::OpenAi)
    } else {
        None
    }
}

/// Resolve the target dialect for a backend: first check the projection matrix
/// for a registered dialect, then fall back to name inference.
fn resolve_backend_dialect(
    projection: Option<&ProjectionMatrix>,
    backend_name: &str,
) -> Option<Dialect> {
    if let Some(matrix) = projection {
        if let Some(entry) = matrix.backend_entry(backend_name) {
            return Some(entry.dialect);
        }
    }
    infer_dialect_from_backend(backend_name)
}

/// Parse a dialect name string into a [`Dialect`].
fn parse_dialect_str(s: &str) -> Option<Dialect> {
    match s.to_lowercase().as_str() {
        "openai" | "open_ai" => Some(Dialect::OpenAi),
        "claude" => Some(Dialect::Claude),
        "gemini" => Some(Dialect::Gemini),
        "codex" => Some(Dialect::Codex),
        "kimi" => Some(Dialect::Kimi),
        "copilot" => Some(Dialect::Copilot),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{Capability, CapabilityRequirement, MinSupport};

    #[test]
    fn check_capabilities_passes_for_satisfiable_requirements() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        rt.check_capabilities("mock", &reqs).unwrap();
    }

    #[test]
    fn check_capabilities_fails_for_unsatisfiable_requirements() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        };
        let err = rt.check_capabilities("mock", &reqs).unwrap_err();
        assert!(
            matches!(err, RuntimeError::CapabilityCheckFailed(_)),
            "expected CapabilityCheckFailed, got {err:?}"
        );
    }

    #[test]
    fn check_capabilities_empty_requirements_always_passes() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements::default();
        rt.check_capabilities("mock", &reqs).unwrap();
    }

    // -- abp-error integration tests --

    #[test]
    fn runtime_error_variants_have_error_codes() {
        let unknown = RuntimeError::UnknownBackend { name: "foo".into() };
        assert_eq!(unknown.error_code(), abp_error::ErrorCode::BackendNotFound);

        let ws = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
        assert_eq!(ws.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);

        let pol = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
        assert_eq!(pol.error_code(), abp_error::ErrorCode::PolicyInvalid);

        let be = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
        assert_eq!(be.error_code(), abp_error::ErrorCode::BackendCrashed);

        let cap = RuntimeError::CapabilityCheckFailed("missing mcp".into());
        assert_eq!(
            cap.error_code(),
            abp_error::ErrorCode::CapabilityUnsupported
        );
    }

    #[test]
    fn abp_error_converts_to_runtime_error() {
        let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "timed out");
        let rt_err: RuntimeError = abp_err.into();
        assert_eq!(rt_err.error_code(), abp_error::ErrorCode::BackendTimeout);
        assert!(rt_err.to_string().contains("timed out"));
    }

    #[test]
    fn runtime_error_into_abp_error_roundtrip() {
        let rt_err = RuntimeError::UnknownBackend {
            name: "missing".into(),
        };
        let code = rt_err.error_code();
        let abp_err = rt_err.into_abp_error();
        assert_eq!(abp_err.code, code);
        assert_eq!(abp_err.code, abp_error::ErrorCode::BackendNotFound);
        assert!(abp_err.message.contains("missing"));
    }

    #[test]
    fn classified_error_preserves_context_through_runtime() {
        let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::ConfigInvalid, "bad config")
            .with_context("file", "backplane.toml");
        let rt_err: RuntimeError = abp_err.into();
        // Converting back preserves the original AbpError (including context).
        let back = rt_err.into_abp_error();
        assert_eq!(back.code, abp_error::ErrorCode::ConfigInvalid);
        assert_eq!(
            back.context.get("file"),
            Some(&serde_json::json!("backplane.toml"))
        );
    }

    // -- projection integration tests --

    mod projection {
        use super::*;
        use abp_core::{
            Capability, CapabilityRequirement, MinSupport, SupportLevel, WorkOrderBuilder,
        };
        use abp_dialect::Dialect;

        fn mock_manifest() -> abp_core::CapabilityManifest {
            let mut m = abp_core::CapabilityManifest::default();
            m.insert(Capability::Streaming, SupportLevel::Native);
            m.insert(Capability::ToolRead, SupportLevel::Emulated);
            m.insert(Capability::ToolWrite, SupportLevel::Emulated);
            m.insert(Capability::ToolEdit, SupportLevel::Emulated);
            m.insert(Capability::ToolBash, SupportLevel::Emulated);
            m.insert(
                Capability::StructuredOutputJsonSchema,
                SupportLevel::Emulated,
            );
            m
        }

        #[test]
        fn select_backend_without_projection_returns_error() {
            let rt = Runtime::with_default_backends();
            let wo = WorkOrderBuilder::new("test").build();
            let err = rt.select_backend(&wo).unwrap_err();
            assert!(
                matches!(err, RuntimeError::NoProjectionMatch { .. }),
                "expected NoProjectionMatch, got {err:?}"
            );
        }

        #[test]
        fn select_backend_picks_registered_backend() {
            let mut matrix = ProjectionMatrix::new();
            matrix.register_backend("mock", mock_manifest(), Dialect::OpenAi, 50);

            let rt = Runtime::with_default_backends().with_projection(matrix);
            // Ensure the mock backend is in the registry (it already is from with_default_backends).
            let wo = WorkOrderBuilder::new("test")
                .requirements(CapabilityRequirements {
                    required: vec![CapabilityRequirement {
                        capability: Capability::Streaming,
                        min_support: MinSupport::Native,
                    }],
                })
                .build();
            let result = rt.select_backend(&wo).unwrap();
            assert_eq!(result.selected_backend, "mock");
            assert!(result.fidelity_score.total > 0.0);
        }

        #[test]
        fn select_backend_fails_when_projected_backend_not_in_registry() {
            let mut matrix = ProjectionMatrix::new();
            // Register a backend in projection that is NOT in the runtime registry.
            matrix.register_backend("nonexistent", mock_manifest(), Dialect::OpenAi, 50);

            let rt = Runtime::new().with_projection(matrix);
            let wo = WorkOrderBuilder::new("test").build();
            let err = rt.select_backend(&wo).unwrap_err();
            assert!(
                matches!(err, RuntimeError::UnknownBackend { .. }),
                "expected UnknownBackend, got {err:?}"
            );
        }

        #[test]
        fn select_backend_prefers_higher_capability_coverage() {
            let mut strong_manifest = abp_core::CapabilityManifest::default();
            strong_manifest.insert(Capability::Streaming, SupportLevel::Native);
            strong_manifest.insert(Capability::ToolRead, SupportLevel::Native);
            strong_manifest.insert(Capability::ToolWrite, SupportLevel::Native);

            let mut weak_manifest = abp_core::CapabilityManifest::default();
            weak_manifest.insert(Capability::Streaming, SupportLevel::Native);

            let mut matrix = ProjectionMatrix::new();
            matrix.register_backend("strong", strong_manifest, Dialect::OpenAi, 50);
            matrix.register_backend("weak", weak_manifest, Dialect::Claude, 50);

            let mut rt = Runtime::new().with_projection(matrix);
            rt.register_backend("strong", abp_integrations::MockBackend);
            rt.register_backend("weak", abp_integrations::MockBackend);

            let wo = WorkOrderBuilder::new("test")
                .requirements(CapabilityRequirements {
                    required: vec![
                        CapabilityRequirement {
                            capability: Capability::Streaming,
                            min_support: MinSupport::Native,
                        },
                        CapabilityRequirement {
                            capability: Capability::ToolRead,
                            min_support: MinSupport::Emulated,
                        },
                        CapabilityRequirement {
                            capability: Capability::ToolWrite,
                            min_support: MinSupport::Emulated,
                        },
                    ],
                })
                .build();

            let result = rt.select_backend(&wo).unwrap();
            assert_eq!(result.selected_backend, "strong");
            assert!(!result.fallback_chain.is_empty());
        }

        #[test]
        fn select_backend_prefers_higher_priority() {
            let manifest = mock_manifest();

            let mut matrix = ProjectionMatrix::new();
            matrix.register_backend("low-prio", manifest.clone(), Dialect::OpenAi, 10);
            matrix.register_backend("high-prio", manifest, Dialect::OpenAi, 90);

            let mut rt = Runtime::new().with_projection(matrix);
            rt.register_backend("low-prio", abp_integrations::MockBackend);
            rt.register_backend("high-prio", abp_integrations::MockBackend);

            let wo = WorkOrderBuilder::new("test").build();
            let result = rt.select_backend(&wo).unwrap();
            assert_eq!(result.selected_backend, "high-prio");
        }

        #[test]
        fn with_projection_builder_sets_and_retrieves() {
            let matrix = ProjectionMatrix::new();
            let rt = Runtime::new().with_projection(matrix);
            assert!(rt.projection().is_some());
        }

        #[test]
        fn projection_mut_allows_modification() {
            let mut rt = Runtime::new().with_projection(ProjectionMatrix::new());
            let pm = rt.projection_mut().unwrap();
            pm.register_backend("added", mock_manifest(), Dialect::OpenAi, 50);
            assert_eq!(pm.backend_count(), 1);
        }

        #[test]
        fn no_projection_match_error_has_correct_code() {
            let err = RuntimeError::NoProjectionMatch {
                reason: "empty".into(),
            };
            assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
        }

        #[tokio::test]
        async fn run_projected_selects_and_executes() {
            let mut matrix = ProjectionMatrix::new();
            matrix.register_backend("mock", mock_manifest(), Dialect::OpenAi, 50);

            let rt = Runtime::with_default_backends().with_projection(matrix);
            let wo = WorkOrderBuilder::new("hello projection").build();
            let handle = rt.run_projected(wo).await.unwrap();
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(receipt.backend.id, "mock");
        }

        #[tokio::test]
        async fn run_projected_fails_without_matrix() {
            let rt = Runtime::with_default_backends();
            let wo = WorkOrderBuilder::new("no matrix").build();
            let result = rt.run_projected(wo).await;
            assert!(
                matches!(result, Err(RuntimeError::NoProjectionMatch { .. })),
                "expected NoProjectionMatch error"
            );
        }

        #[test]
        fn select_backend_returns_fallback_chain() {
            let manifest = mock_manifest();

            let mut matrix = ProjectionMatrix::new();
            matrix.register_backend("alpha", manifest.clone(), Dialect::OpenAi, 80);
            matrix.register_backend("beta", manifest.clone(), Dialect::Claude, 60);
            matrix.register_backend("gamma", manifest, Dialect::Gemini, 40);

            let mut rt = Runtime::new().with_projection(matrix);
            rt.register_backend("alpha", abp_integrations::MockBackend);
            rt.register_backend("beta", abp_integrations::MockBackend);
            rt.register_backend("gamma", abp_integrations::MockBackend);

            let wo = WorkOrderBuilder::new("test").build();
            let result = rt.select_backend(&wo).unwrap();
            // The selected backend should be the highest priority.
            assert_eq!(result.selected_backend, "alpha");
            // Fallback chain should contain the other two.
            assert_eq!(result.fallback_chain.len(), 2);
            let fb_ids: Vec<_> = result
                .fallback_chain
                .iter()
                .map(|f| f.backend_id.as_str())
                .collect();
            assert!(fb_ids.contains(&"beta"));
            assert!(fb_ids.contains(&"gamma"));
        }
    }
}
