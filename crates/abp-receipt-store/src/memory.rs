// SPDX-License-Identifier: MIT OR Apache-2.0

//! In-memory receipt store backed by a `HashMap` with secondary indexes.

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

use abp_core::Receipt;

use crate::error::StoreError;
use crate::filter::ReceiptFilter;
use crate::index::ReceiptIndex;
use crate::{ReceiptStore, Result};

/// Inner state protected by the `RwLock`.
#[derive(Debug, Default)]
struct Inner {
    map: HashMap<String, Receipt>,
    index: ReceiptIndex,
}

/// In-memory receipt store using a `HashMap` protected by a `RwLock`.
///
/// Maintains secondary indexes (backend, outcome, time, work order) for
/// fast queries. Suitable for testing and ephemeral use—all data is lost
/// when dropped.
#[derive(Debug, Default)]
pub struct InMemoryReceiptStore {
    inner: RwLock<Inner>,
}

impl InMemoryReceiptStore {
    /// Create an empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return a snapshot of the current index (useful for direct index queries).
    pub async fn index(&self) -> ReceiptIndex {
        self.inner.read().await.index.clone()
    }

    /// Retrieve all receipts matching a given work order ID.
    pub async fn get_by_work_order_id(&self, work_order_id: &str) -> Result<Vec<Receipt>> {
        let guard = self.inner.read().await;
        let ids = guard.index.by_work_order_id(work_order_id);
        let mut out = Vec::with_capacity(ids.len());
        for id in &ids {
            if let Some(r) = guard.map.get(id) {
                out.push(r.clone());
            }
        }
        Ok(out)
    }
}

#[async_trait]
impl ReceiptStore for InMemoryReceiptStore {
    async fn store(&self, receipt: &Receipt) -> Result<()> {
        let id = receipt.meta.run_id.to_string();
        let mut guard = self.inner.write().await;
        if guard.map.contains_key(&id) {
            return Err(StoreError::DuplicateId(id));
        }
        guard.index.insert(receipt);
        guard.map.insert(id, receipt.clone());
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Receipt>> {
        let guard = self.inner.read().await;
        Ok(guard.map.get(id).cloned())
    }

    async fn list(&self, filter: ReceiptFilter) -> Result<Vec<Receipt>> {
        let guard = self.inner.read().await;
        let matched: Vec<Receipt> = guard
            .map
            .values()
            .filter(|r| filter.matches(r))
            .cloned()
            .collect();
        Ok(filter.paginate(matched))
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        let mut guard = self.inner.write().await;
        if let Some(receipt) = guard.map.remove(id) {
            guard.index.remove(&receipt);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn count(&self) -> Result<usize> {
        let guard = self.inner.read().await;
        Ok(guard.map.len())
    }
}
