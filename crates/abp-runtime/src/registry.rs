// SPDX-License-Identifier: MIT OR Apache-2.0
//! Typed wrapper around the backend map used by the runtime.

use abp_core::{AgentEvent, BackendIdentity, CapabilityManifest, Receipt, WorkOrder};
use abp_integrations::Backend;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

/// A typed registry of named [`Backend`] implementations.
#[derive(Default)]
pub struct BackendRegistry {
    backends: HashMap<String, Arc<dyn Backend>>,
}

impl BackendRegistry {
    /// Register a backend under the given name, replacing any previous entry.
    pub fn register(&mut self, name: impl Into<String>, backend: impl Backend + 'static) {
        self.backends.insert(name.into(), Arc::new(backend));
    }

    /// Look up a backend by name.
    pub fn get(&self, name: &str) -> Option<&dyn Backend> {
        self.backends.get(name).map(|b| &**b)
    }

    /// Return an `Arc` handle to the named backend.
    pub fn get_arc(&self, name: &str) -> Option<Arc<dyn Backend>> {
        self.backends.get(name).cloned()
    }

    /// Return a sorted list of registered backend names.
    pub fn list(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.backends.keys().map(|s| s.as_str()).collect();
        v.sort();
        v
    }

    /// Check whether a backend with the given name is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.backends.contains_key(name)
    }

    /// Remove a backend by name, returning it if it existed.
    pub fn remove(&mut self, name: &str) -> Option<Box<dyn Backend>> {
        self.backends
            .remove(name)
            .map(|arc| Box::new(SharedBackend(arc)) as Box<dyn Backend>)
    }
}

/// Thin forwarding wrapper so we can return `Box<dyn Backend>` from [`BackendRegistry::remove`].
struct SharedBackend(Arc<dyn Backend>);

#[async_trait]
impl Backend for SharedBackend {
    fn identity(&self) -> BackendIdentity {
        self.0.identity()
    }
    fn capabilities(&self) -> CapabilityManifest {
        self.0.capabilities()
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        self.0.run(run_id, work_order, events_tx).await
    }
}
