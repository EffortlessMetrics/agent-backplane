//! abp-runtime
//!
//! Orchestration layer.
//!
//! Responsibilities:
//! - prepare a workspace (pass-through or staged)
//! - enforce/record policy (best-effort in v0.1)
//! - select a backend and stream events
//! - produce a canonical receipt with verification metadata

use abp_core::{AgentEvent, Outcome, Receipt, WorkOrder};
use abp_integrations::Backend;
use abp_policy::PolicyEngine;
use abp_workspace::WorkspaceManager;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::debug;
use uuid::Uuid;

pub struct Runtime {
    backends: HashMap<String, Arc<dyn Backend>>,
}

pub struct RunHandle {
    pub run_id: Uuid,
    pub events: ReceiverStream<AgentEvent>,
    pub receipt: tokio::task::JoinHandle<Result<Receipt>>,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            backends: HashMap::new(),
        }
    }

    pub fn with_default_backends() -> Self {
        let mut rt = Self::new();
        rt.register_backend("mock", abp_integrations::MockBackend);
        rt
    }

    pub fn register_backend<B: Backend + 'static>(&mut self, name: &str, backend: B) {
        self.backends.insert(name.to_string(), Arc::new(backend));
    }

    pub fn backend_names(&self) -> Vec<String> {
        let mut v: Vec<_> = self.backends.keys().cloned().collect();
        v.sort();
        v
    }

    pub fn backend(&self, name: &str) -> Option<Arc<dyn Backend>> {
        self.backends.get(name).cloned()
    }

    pub async fn run_streaming(&self, backend_name: &str, work_order: WorkOrder) -> Result<RunHandle> {
        let backend = self
            .backend(backend_name)
            .with_context(|| format!("unknown backend: {backend_name}"))?;

        let backend_name = backend_name.to_string();
        let run_id = Uuid::new_v4();

        // Two-stage channel: backend -> runtime -> caller
        let (from_backend_tx, mut from_backend_rx) = mpsc::channel::<AgentEvent>(256);
        let (to_caller_tx, to_caller_rx) = mpsc::channel::<AgentEvent>(256);

        let receipt = tokio::spawn(async move {
            // Keep the prepared workspace alive for the duration of the run.
            let prepared = WorkspaceManager::prepare(&work_order.workspace)
                .context("prepare workspace")?;

            // Clone and rewrite the work order to point at prepared workspace.
            let mut wo = work_order.clone();
            wo.workspace.root = prepared.path().to_string_lossy().to_string();

            // Compile policy globs (even if adapters do the heavy lifting).
            let _policy = PolicyEngine::new(&wo.policy).context("compile policy")?;

            debug!(target: "abp.runtime", backend=%backend_name, run_id=%run_id, "starting run");

            // Run backend in a task so we can multiplex events.
            let backend2 = backend.clone();
            let mut backend_handle = tokio::spawn(async move { backend2.run(run_id, wo, from_backend_tx).await });

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
                        let r = res.context("backend task join")??;
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
            receipt = receipt.with_hash().context("hash receipt")?;

            Ok(receipt)
        });

        Ok(RunHandle {
            run_id,
            events: ReceiverStream::new(to_caller_rx),
            receipt,
        })
    }
}
