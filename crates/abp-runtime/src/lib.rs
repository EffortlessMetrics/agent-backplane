// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! abp-runtime
//!
//! Orchestration layer.
//!
//! Responsibilities:
//! - prepare a workspace (pass-through or staged)
//! - enforce/record policy (best-effort in v0.1)
//! - select a backend and stream events
//! - produce a canonical receipt with verification metadata

#![deny(unsafe_code)]
#![warn(missing_docs)]

/// Budget enforcement for runtime runs.
pub mod budget;
/// Broadcast-based event bus for decoupled event distribution.
pub mod bus;
/// Cancellation primitives for runtime runs.
pub mod cancel;
/// Lifecycle hooks for runtime extensibility.
pub mod hooks;
/// Event multiplexing and routing for broadcasting agent events.
pub mod multiplex;
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
/// Telemetry and metrics collection.
pub mod telemetry;

use abp_core::{AgentEvent, CapabilityRequirements, Outcome, Receipt, WorkOrder};
use abp_emulation::{EmulationConfig, EmulationEngine, EmulationReport};
use abp_integrations::{Backend, ensure_capability_requirements};
use abp_policy::PolicyEngine;
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

        // Two-stage channel: backend -> runtime -> caller
        let (from_backend_tx, mut from_backend_rx) = mpsc::channel::<AgentEvent>(256);
        let (to_caller_tx, to_caller_rx) = mpsc::channel::<AgentEvent>(256);

        let receipt_chain = Arc::clone(&self.receipt_chain);

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
                                    .unsupported
                                    .iter()
                                    .filter(|c| !emulated_caps.contains(c))
                                    .cloned()
                                    .collect()
                            }
                            None => result.unsupported.clone(),
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
                    if !result.emulatable.is_empty() {
                        warn!(
                            target: "abp.runtime",
                            backend=%backend_name,
                            emulated=?result.emulatable,
                            "capabilities require emulation"
                        );
                    }
                    Some(result)
                } else {
                    None
                }
            };

            debug!(target: "abp.runtime", backend=%backend_name, run_id=%run_id, "starting run");

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
                                trace.push(ev.clone());
                                let _ = to_caller_tx.send(ev).await;
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
                trace.push(ev.clone());
                let _ = to_caller_tx.send(ev).await;
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
            if let Some(emu_report) = emulation_report
                && let (false, Ok(report_value)) =
                    (emu_report.is_empty(), serde_json::to_value(&emu_report))
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
}
