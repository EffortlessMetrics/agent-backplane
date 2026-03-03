// SPDX-License-Identifier: MIT OR Apache-2.0
//! Receipt persistence and retrieval.
//!
//! This module provides both a concrete [`ReceiptStore`] (file-system based,
//! keyed by `run_id`) and a trait-based [`ReceiptStorage`] abstraction keyed
//! by receipt SHA-256 hash.  The trait allows swapping backends (filesystem,
//! SQLite, etc.) while keeping the same interface.

use abp_core::Receipt;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors specific to receipt storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// An I/O operation on a storage path failed.
    #[error("I/O error at {path}: {source}")]
    Io {
        /// Filesystem path involved in the failure.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// JSON serialization or deserialization failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// The receipt does not carry a `receipt_sha256` hash.
    #[error("receipt has no hash; call Receipt::with_hash() first")]
    MissingHash,

    /// Re-computed hash does not match the stored value.
    #[error("integrity check failed: stored {stored}, computed {computed}")]
    IntegrityMismatch {
        /// Hash value found inside the receipt.
        stored: String,
        /// Freshly computed hash.
        computed: String,
    },

    /// Hashing the receipt failed (upstream contract error).
    #[error("hash computation failed: {0}")]
    Hash(#[from] abp_core::ContractError),

    /// The requested receipt was not found.
    #[error("receipt not found: {0}")]
    NotFound(String),
}

/// Helper to wrap an [`std::io::Error`] with a path for context.
fn io_err(path: &Path, source: std::io::Error) -> StoreError {
    StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Backend-agnostic receipt persistence keyed by SHA-256 hash.
///
/// Implementations may store receipts on the local filesystem, in a database,
/// or in any other durable medium.  The hash used as key is the value of
/// [`Receipt::receipt_sha256`], which must be populated (via
/// [`Receipt::with_hash`]) before saving.
pub trait ReceiptStorage {
    /// Persist a receipt keyed by its `receipt_sha256` hash.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::MissingHash`] if the receipt has no hash, or a
    /// backend-specific error on failure.
    fn save_by_hash(&self, receipt: &Receipt) -> Result<PathBuf, StoreError>;

    /// Load a receipt by its SHA-256 hash string.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if no receipt with this hash exists.
    fn load_by_hash(&self, hash: &str) -> Result<Receipt, StoreError>;

    /// List the SHA-256 hashes of all stored receipts.
    ///
    /// # Errors
    ///
    /// Returns an error if the backing store cannot be enumerated.
    fn list_hashes(&self) -> Result<Vec<String>, StoreError>;

    /// Verify a stored receipt's integrity by recomputing its hash and
    /// comparing it with the stored value.
    ///
    /// Returns `Ok(true)` when the hashes match.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] when the hash is unknown, or
    /// [`StoreError::Hash`] if the hash cannot be recomputed.
    fn verify_integrity(&self, hash: &str) -> Result<bool, StoreError>;
}

// ---------------------------------------------------------------------------
// Chain verification (unchanged)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// File-system implementation
// ---------------------------------------------------------------------------

/// File-based receipt store.
///
/// Receipts are stored as pretty-printed JSON files under a configurable root
/// directory.  Run-id keyed files use `{run_id}.json`; hash-keyed files
/// (via [`ReceiptStorage`]) use `by_hash/{hash}.json`.
#[derive(Debug)]
pub struct ReceiptStore {
    root: PathBuf,
}

impl ReceiptStore {
    /// Create a new store rooted at the given directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Return the root directory of this store.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Persist a receipt to disk (keyed by `run_id`).
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
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && let Ok(id) = Uuid::parse_str(stem)
            {
                ids.push(id);
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

    /// Directory used for hash-keyed receipt files.
    fn hash_dir(&self) -> PathBuf {
        self.root.join("by_hash")
    }

    /// Full path for a hash-keyed receipt file.
    fn hash_path(&self, hash: &str) -> PathBuf {
        self.hash_dir().join(format!("{hash}.json"))
    }
}

// ---------------------------------------------------------------------------
// ReceiptStorage trait implementation for ReceiptStore
// ---------------------------------------------------------------------------

impl ReceiptStorage for ReceiptStore {
    fn save_by_hash(&self, receipt: &Receipt) -> Result<PathBuf, StoreError> {
        let hash = receipt
            .receipt_sha256
            .as_deref()
            .ok_or(StoreError::MissingHash)?;

        let dir = self.hash_dir();
        std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;

        let path = self.hash_path(hash);
        let json = serde_json::to_string_pretty(receipt)?;
        std::fs::write(&path, json).map_err(|e| io_err(&path, e))?;
        Ok(path)
    }

    fn load_by_hash(&self, hash: &str) -> Result<Receipt, StoreError> {
        let path = self.hash_path(hash);
        let json = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StoreError::NotFound(hash.to_string())
            } else {
                io_err(&path, e)
            }
        })?;
        let receipt: Receipt = serde_json::from_str(&json)?;
        Ok(receipt)
    }

    fn list_hashes(&self) -> Result<Vec<String>, StoreError> {
        let dir = self.hash_dir();
        let rd = match std::fs::read_dir(&dir) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_err(&dir, e)),
        };

        let mut hashes = Vec::new();
        for entry in rd {
            let entry = entry.map_err(|e| io_err(&dir, e))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                // SHA-256 hex digests are exactly 64 hex characters.
                if stem.len() == 64 && stem.chars().all(|c| c.is_ascii_hexdigit()) {
                    hashes.push(stem.to_string());
                }
            }
        }
        hashes.sort();
        Ok(hashes)
    }

    fn verify_integrity(&self, hash: &str) -> Result<bool, StoreError> {
        let receipt = self.load_by_hash(hash)?;
        let computed = abp_core::receipt_hash(&receipt)?;
        Ok(receipt.receipt_sha256.as_deref() == Some(computed.as_str()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{Outcome, ReceiptBuilder};

    /// Build a minimal receipt with a hash attached.
    fn make_receipt() -> Receipt {
        ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .expect("hashing should succeed")
    }

    // -- ReceiptStorage trait (hash-keyed) tests --

    #[test]
    fn save_and_load_by_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let receipt = make_receipt();
        let hash = receipt.receipt_sha256.clone().unwrap();

        let path = store.save_by_hash(&receipt).unwrap();
        assert!(path.exists());

        let loaded = store.load_by_hash(&hash).unwrap();
        assert_eq!(loaded.receipt_sha256, receipt.receipt_sha256);
        assert_eq!(loaded.meta.run_id, receipt.meta.run_id);
    }

    #[test]
    fn save_by_hash_requires_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let receipt = ReceiptBuilder::new("no-hash")
            .outcome(Outcome::Complete)
            .build();

        let err = store.save_by_hash(&receipt).unwrap_err();
        assert!(matches!(err, StoreError::MissingHash));
    }

    #[test]
    fn load_by_hash_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        let err = store
            .load_by_hash("0000000000000000000000000000000000000000000000000000000000000000")
            .unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
    }

    #[test]
    fn list_hashes_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        assert!(store.list_hashes().unwrap().is_empty());
    }

    #[test]
    fn list_hashes_returns_stored() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        let r1 = make_receipt();
        let r2 = make_receipt();
        store.save_by_hash(&r1).unwrap();
        store.save_by_hash(&r2).unwrap();

        let mut hashes = store.list_hashes().unwrap();
        hashes.sort();
        let mut expected = vec![
            r1.receipt_sha256.clone().unwrap(),
            r2.receipt_sha256.clone().unwrap(),
        ];
        expected.sort();
        assert_eq!(hashes, expected);
    }

    #[test]
    fn verify_integrity_valid() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let receipt = make_receipt();
        let hash = receipt.receipt_sha256.clone().unwrap();

        store.save_by_hash(&receipt).unwrap();
        assert!(store.verify_integrity(&hash).unwrap());
    }

    #[test]
    fn verify_integrity_tampered() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let receipt = make_receipt();
        let hash = receipt.receipt_sha256.clone().unwrap();

        store.save_by_hash(&receipt).unwrap();

        // Tamper with the stored file by injecting extra data into usage_raw.
        let path = store.hash_path(&hash);
        let mut val: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        val["usage_raw"] = serde_json::json!({"tampered": true});
        std::fs::write(&path, serde_json::to_string_pretty(&val).unwrap()).unwrap();

        assert!(!store.verify_integrity(&hash).unwrap());
    }

    #[test]
    fn verify_integrity_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        let err = store
            .verify_integrity("0000000000000000000000000000000000000000000000000000000000000000")
            .unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
    }

    #[test]
    fn hash_and_runid_stores_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let receipt = make_receipt();

        // Save via both mechanisms.
        store.save(&receipt).unwrap();
        store.save_by_hash(&receipt).unwrap();

        // run_id listing should not include hash files and vice versa.
        let ids = store.list().unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], receipt.meta.run_id);

        let hashes = store.list_hashes().unwrap();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0], receipt.receipt_sha256.clone().unwrap());
    }

    #[test]
    fn root_accessor() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        assert_eq!(store.root(), dir.path());
    }
}
