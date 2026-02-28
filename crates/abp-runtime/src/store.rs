// SPDX-License-Identifier: MIT OR Apache-2.0
//! Receipt persistence and retrieval.

use abp_core::Receipt;
use anyhow::{Context, Result};
use std::path::PathBuf;
use uuid::Uuid;

/// File-based receipt store.
#[derive(Debug)]
pub struct ReceiptStore {
    root: PathBuf,
}

impl ReceiptStore {
    /// Create a new store rooted at the given directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Persist a receipt to disk.
    pub fn save(&self, receipt: &Receipt) -> Result<PathBuf> {
        let path = self.receipt_path(receipt.meta.run_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create receipt dir {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(receipt)?;
        std::fs::write(&path, json)
            .with_context(|| format!("write receipt to {}", path.display()))?;
        Ok(path)
    }

    /// Load a receipt by run_id.
    pub fn load(&self, run_id: Uuid) -> Result<Receipt> {
        let path = self.receipt_path(run_id);
        let json = std::fs::read_to_string(&path)
            .with_context(|| format!("read receipt from {}", path.display()))?;
        let receipt: Receipt = serde_json::from_str(&json)?;
        Ok(receipt)
    }

    /// List all stored receipt run_ids.
    pub fn list(&self) -> Result<Vec<Uuid>> {
        let dir = match std::fs::read_dir(&self.root) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(anyhow::Error::new(e)
                    .context(format!("read receipt dir {}", self.root.display())));
            }
        };
        let mut ids = Vec::new();
        for entry in dir {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(id) = Uuid::parse_str(stem) {
                        ids.push(id);
                    }
                }
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// Verify a receipt's hash matches its stored content.
    pub fn verify(&self, run_id: Uuid) -> Result<bool> {
        let receipt = self.load(run_id)?;
        let computed = abp_core::receipt_hash(&receipt)?;
        Ok(receipt.receipt_sha256.as_deref() == Some(&computed))
    }

    fn receipt_path(&self, run_id: Uuid) -> PathBuf {
        self.root.join(format!("{run_id}.json"))
    }
}
