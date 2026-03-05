// SPDX-License-Identifier: MIT OR Apache-2.0

//! JSON-lines file-based receipt store.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::debug;

use abp_core::Receipt;

use crate::error::StoreError;
use crate::filter::ReceiptFilter;
use crate::{ReceiptStore, Result};

/// Receipt store backed by a JSON-lines file (one receipt per line).
///
/// All mutations are serialized through a `Mutex` to prevent corruption.
/// Reads parse the entire file each time—suitable for moderate-size stores.
#[derive(Debug)]
pub struct FileReceiptStore {
    path: PathBuf,
    mu: Mutex<()>,
}

impl FileReceiptStore {
    /// Create (or open) a store at the given file path.
    ///
    /// The file is created on the first write if it does not exist.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            mu: Mutex::new(()),
        }
    }

    /// Read all receipts from the file.
    async fn read_all(&self) -> Result<Vec<Receipt>> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || read_all_sync(&path))
            .await
            .map_err(|e| StoreError::Other(e.to_string()))?
    }

    /// Write all receipts back to the file (full rewrite).
    async fn write_all(&self, receipts: &[Receipt]) -> Result<()> {
        let path = self.path.clone();
        let data = serialize_all(receipts)?;
        tokio::fs::write(&path, data).await?;
        Ok(())
    }
}

fn read_all_sync(path: &Path) -> Result<Vec<Receipt>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    let mut receipts = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let receipt: Receipt =
            serde_json::from_str(line).map_err(|e| StoreError::Other(format!("line {i}: {e}")))?;
        receipts.push(receipt);
    }
    Ok(receipts)
}

fn serialize_all(receipts: &[Receipt]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    for r in receipts {
        let line = serde_json::to_string(r)?;
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
    }
    Ok(buf)
}

#[async_trait]
impl ReceiptStore for FileReceiptStore {
    async fn store(&self, receipt: &Receipt) -> Result<()> {
        let _lock = self.mu.lock().await;
        let id = receipt.meta.run_id.to_string();
        let mut all = self.read_all().await?;
        if all.iter().any(|r| r.meta.run_id.to_string() == id) {
            return Err(StoreError::DuplicateId(id));
        }
        debug!(id = %id, path = %self.path.display(), "storing receipt");
        all.push(receipt.clone());
        self.write_all(&all).await
    }

    async fn get(&self, id: &str) -> Result<Option<Receipt>> {
        let all = self.read_all().await?;
        Ok(all.into_iter().find(|r| r.meta.run_id.to_string() == id))
    }

    async fn list(&self, filter: ReceiptFilter) -> Result<Vec<Receipt>> {
        let all = self.read_all().await?;
        let matched: Vec<Receipt> = all.into_iter().filter(|r| filter.matches(r)).collect();
        Ok(filter.paginate(matched))
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        let _lock = self.mu.lock().await;
        let mut all = self.read_all().await?;
        let before = all.len();
        all.retain(|r| r.meta.run_id.to_string() != id);
        let removed = all.len() < before;
        if removed {
            debug!(id = %id, path = %self.path.display(), "deleted receipt");
            self.write_all(&all).await?;
        }
        Ok(removed)
    }

    async fn count(&self) -> Result<usize> {
        let all = self.read_all().await?;
        Ok(all.len())
    }
}
