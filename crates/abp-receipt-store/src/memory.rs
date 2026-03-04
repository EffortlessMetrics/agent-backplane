// SPDX-License-Identifier: MIT OR Apache-2.0

//! In-memory receipt store backed by a `HashMap`.

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

use abp_core::Receipt;

use crate::error::StoreError;
use crate::filter::ReceiptFilter;
use crate::{ReceiptStore, Result};

/// In-memory receipt store using a `HashMap` protected by a `RwLock`.
///
/// Suitable for testing and ephemeral use. All data is lost when dropped.
#[derive(Debug, Default)]
pub struct InMemoryReceiptStore {
    inner: RwLock<HashMap<String, Receipt>>,
}

impl InMemoryReceiptStore {
    /// Create an empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ReceiptStore for InMemoryReceiptStore {
    async fn store(&self, receipt: &Receipt) -> Result<()> {
        let id = receipt.meta.run_id.to_string();
        let mut map = self.inner.write().await;
        if map.contains_key(&id) {
            return Err(StoreError::DuplicateId(id));
        }
        map.insert(id, receipt.clone());
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Receipt>> {
        let map = self.inner.read().await;
        Ok(map.get(id).cloned())
    }

    async fn list(&self, filter: ReceiptFilter) -> Result<Vec<Receipt>> {
        let map = self.inner.read().await;
        let matched: Vec<Receipt> = map
            .values()
            .filter(|r| filter.matches(r))
            .cloned()
            .collect();
        Ok(filter.paginate(matched))
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        let mut map = self.inner.write().await;
        Ok(map.remove(id).is_some())
    }

    async fn count(&self) -> Result<usize> {
        let map = self.inner.read().await;
        Ok(map.len())
    }
}
