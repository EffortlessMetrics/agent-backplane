// SPDX-License-Identifier: MIT OR Apache-2.0
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

/// Backend registry for named backend lookup.
pub mod registry;
/// Receipt persistence and retrieval.
pub mod store;

use abp_core::{AgentEvent, CapabilityRequirements, ExecutionMode, Outcome, Receipt, WorkOrder};
use abp_integrations::{Backend, ensure_capability_requirements};
use abp_policy::PolicyEngine;
use abp_workspace::WorkspaceManager;
use anyhow::Context;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::debug;
use uuid::Uuid;

pub use registry::BackendRegistry;

/// Errors from the ABP runtime orchestrator.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("unknown backend: {name}")]
    UnknownBackend { name: String },

    #[error("workspace preparation failed")]
    WorkspaceFailed(#[source] anyhow::Error),

    #[error("policy compilation failed")]
    PolicyFailed(#[source] anyhow::Error),

    #[error("backend execution failed")]
    BackendFailed(#[source] anyhow::Error),

    #[error("capability check failed: {0}")]
    CapabilityCheckFailed(String),
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
    pub fn new() -> Self {
        Self {
            backends: BackendRegistry::default(),
        }
    }

    /// Create a runtime pre-loaded with the [`MockBackend`](abp_integrations::MockBackend).
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
    pub fn backend_names(&self) -> Vec<String> {
        self.backends.list().into_iter().map(String::from).collect()
    }

    /// Look up a backend by name.
    pub fn backend(&self, name: &str) -> Option<Arc<dyn Backend>> {
        self.backends.get_arc(name)
    }

    /// Return a reference to the underlying [`BackendRegistry`].
    pub fn registry(&self) -> &BackendRegistry {
        &self.backends
    }

    /// Return a mutable reference to the underlying [`BackendRegistry`].
    pub fn registry_mut(&mut self) -> &mut BackendRegistry {
        &mut self.backends
    }

    /// Check whether a backend's capabilities satisfy the given requirements.
    ///
    /// For sidecar backends whose capabilities come from handshake, this will
    /// check against the (empty) default manifest — use the in-backend check
    /// for authoritative validation.
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
        ensure_capability_requirements(requirements, &backend.capabilities())
            .map_err(|e| RuntimeError::CapabilityCheckFailed(e.to_string()))?;
        Ok(())
    }

    /// Execute a work order against the named backend, returning a [`RunHandle`].
    ///
    /// The handle provides a streaming event channel and a receipt future.
    /// The runtime prepares the workspace, compiles the policy, and attaches
    /// verification metadata and receipt hash after the backend finishes.
    pub async fn run_streaming(
        &self,
        backend_name: &str,
        work_order: WorkOrder,
    ) -> Result<RunHandle, RuntimeError> {
        let backend = self
            .backend(backend_name)
            .ok_or_else(|| RuntimeError::UnknownBackend {
                name: backend_name.to_string(),
            })?;

        // Pre-flight capability check: skip for sidecar backends whose
        // capabilities are only known after handshake (empty default manifest).
        let caps = backend.capabilities();
        if !caps.is_empty() {
            ensure_capability_requirements(&work_order.requirements, &caps)
                .map_err(|e| RuntimeError::CapabilityCheckFailed(e.to_string()))?;
        }

        let backend_name = backend_name.to_string();
        let run_id = Uuid::new_v4();

        // Two-stage channel: backend -> runtime -> caller
        let (from_backend_tx, mut from_backend_rx) = mpsc::channel::<AgentEvent>(256);
        let (to_caller_tx, to_caller_rx) = mpsc::channel::<AgentEvent>(256);

        let receipt = tokio::spawn(async move {
            // Keep the prepared workspace alive for the duration of the run.
            let prepared = WorkspaceManager::prepare(&work_order.workspace)
                .context("prepare workspace")
                .map_err(RuntimeError::WorkspaceFailed)?;

            // Clone and rewrite the work order to point at prepared workspace.
            let mut wo = work_order.clone();
            wo.workspace.root = prepared.path().to_string_lossy().to_string();

            // Compile policy globs (even if adapters do the heavy lifting).
            let _policy = PolicyEngine::new(&wo.policy)
                .context("compile policy")
                .map_err(RuntimeError::PolicyFailed)?;

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
                            Ok(Err(e)) => return Err(RuntimeError::BackendFailed(e)),
                            Err(e) => return Err(RuntimeError::BackendFailed(
                                anyhow::Error::new(e).context("backend task panicked"),
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
                    Ok(Err(e)) => return Err(RuntimeError::BackendFailed(e)),
                    Err(e) => {
                        return Err(RuntimeError::BackendFailed(
                            anyhow::Error::new(e).context("backend task panicked"),
                        ));
                    }
                }
            }

            drop(to_caller_tx);

            let mut receipt = receipt_opt.unwrap_or_else(|| {
                // Backend crashed before returning a receipt.
                Receipt {
                    meta: abp_core::RunMetadata {
                        run_id,
                        work_order_id: work_order.id,
                        contract_version: abp_core::CONTRACT_VERSION.to_string(),
                        started_at: chrono::Utc::now(),
                        finished_at: chrono::Utc::now(),
                        duration_ms: 0,
                    },
                    backend: backend.identity(),
                    capabilities: backend.capabilities(),
                    mode: ExecutionMode::default(),
                    usage_raw: serde_json::json!({"error": "no receipt"}),
                    usage: Default::default(),
                    trace: vec![],
                    artifacts: vec![],
                    verification: Default::default(),
                    outcome: Outcome::Failed,
                    receipt_sha256: None,
                }
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

            // Ensure receipt hash is present and consistent.
            receipt = receipt
                .with_hash()
                .context("hash receipt")
                .map_err(RuntimeError::BackendFailed)?;

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
}
