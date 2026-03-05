// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! Async receipt storage, indexing, and chain validation for the Agent Backplane.

mod chain;
mod diff;
mod error;
mod export;
mod file;
mod filter;
mod index;
mod memory;
mod stats;

pub use chain::{
    validate_chain, validate_chain_with_parents, ChainValidation, ChainValidationError,
};
pub use diff::{diff_receipts, FieldDiff, ReceiptDiff};
pub use error::StoreError;
pub use export::{export_json, export_jsonl, import_json, import_jsonl};
pub use file::FileReceiptStore;
pub use filter::ReceiptFilter;
pub use index::ReceiptIndex;
pub use memory::InMemoryReceiptStore;
pub use stats::ReceiptStats;

// Re-export core types for convenience.
pub use abp_core::{Outcome, Receipt};

use async_trait::async_trait;

/// Result alias for receipt store operations.
pub type Result<T> = std::result::Result<T, StoreError>;

/// Async trait for pluggable receipt storage backends.
#[async_trait]
pub trait ReceiptStore: Send + Sync {
    /// Persist a receipt.
    async fn store(&self, receipt: &Receipt) -> Result<()>;

    /// Retrieve a receipt by its run ID (UUID string).
    async fn get(&self, id: &str) -> Result<Option<Receipt>>;

    /// List receipts matching the given filter.
    async fn list(&self, filter: ReceiptFilter) -> Result<Vec<Receipt>>;

    /// Delete a receipt by its run ID. Returns `true` if it existed.
    async fn delete(&self, id: &str) -> Result<bool>;

    /// Count total receipts in the store.
    async fn count(&self) -> Result<usize>;
}

#[cfg(test)]
mod tests;
