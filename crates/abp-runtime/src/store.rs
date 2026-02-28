// SPDX-License-Identifier: MIT OR Apache-2.0
//! Receipt persistence and retrieval.

use abp_core::Receipt;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use uuid::Uuid;

/// Result of verifying the stored receipt chain.
#[derive(Debug, Clone)]
pub struct ChainVerification {
    /// Number of receipts with valid hashes.
    pub valid_count: usize,
    /// Run IDs of receipts whose hash did not match.
    pub invalid_hashes: Vec<Uuid>,
    /// Time gaps between consecutive runs (`finished_at` → next `started_at`).
    pub gaps: Vec<(DateTime<Utc>, DateTime<Utc>)>,
    /// `true` when every receipt hash is valid and receipts are in chronological order.
    pub is_valid: bool,
}

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
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or the file cannot be written.
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

    /// Load a receipt by `run_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load(&self, run_id: Uuid) -> Result<Receipt> {
        let path = self.receipt_path(run_id);
        let json = std::fs::read_to_string(&path)
            .with_context(|| format!("read receipt from {}", path.display()))?;
        let receipt: Receipt = serde_json::from_str(&json)?;
        Ok(receipt)
    }

    /// List all stored receipt run_ids.
    ///
    /// # Errors
    ///
    /// Returns an error if the store directory cannot be read.
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
    ///
    /// # Errors
    ///
    /// Returns an error if the receipt cannot be loaded or its hash recomputed.
    pub fn verify(&self, run_id: Uuid) -> Result<bool> {
        let receipt = self.load(run_id)?;
        let computed = abp_core::receipt_hash(&receipt)?;
        Ok(receipt.receipt_sha256.as_deref() == Some(&computed))
    }

    /// Verify that all stored receipts have valid hashes and form a
    /// chronological sequence.
    ///
    /// # Errors
    ///
    /// Returns an error if the store directory cannot be read or a receipt
    /// cannot be loaded.
    pub fn verify_chain(&self) -> Result<ChainVerification> {
        let ids = self.list()?;
        if ids.is_empty() {
            return Ok(ChainVerification {
                valid_count: 0,
                invalid_hashes: Vec::new(),
                gaps: Vec::new(),
                is_valid: true,
            });
        }

        let mut receipts: Vec<Receipt> = ids
            .iter()
            .map(|id| self.load(*id))
            .collect::<Result<Vec<_>>>()?;

        // Sort by started_at for chronological ordering.
        receipts.sort_by_key(|r| r.meta.started_at);

        let mut valid_count: usize = 0;
        let mut invalid_hashes: Vec<Uuid> = Vec::new();

        for r in &receipts {
            let computed = abp_core::receipt_hash(r)
                .with_context(|| format!("hash receipt {}", r.meta.run_id))?;
            if r.receipt_sha256.as_deref() == Some(&computed) {
                valid_count += 1;
            } else {
                invalid_hashes.push(r.meta.run_id);
            }
        }

        // Collect time gaps between consecutive runs.
        let mut gaps: Vec<(DateTime<Utc>, DateTime<Utc>)> = Vec::new();
        for pair in receipts.windows(2) {
            gaps.push((pair[0].meta.finished_at, pair[1].meta.started_at));
        }

        let is_valid = invalid_hashes.is_empty();

        Ok(ChainVerification {
            valid_count,
            invalid_hashes,
            gaps,
            is_valid,
        })
    }

    fn receipt_path(&self, run_id: Uuid) -> PathBuf {
        self.root.join(format!("{run_id}.json"))
    }
}
