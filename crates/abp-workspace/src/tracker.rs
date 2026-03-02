// SPDX-License-Identifier: MIT OR Apache-2.0
//! Backwards-compatible re-exports for change tracking.
//!
//! The implementation lives in `abp-change-tracker` to keep workspace staging
//! concerns separate from generic change accounting primitives.

pub use abp_change_tracker::{ChangeKind, ChangeSummary, ChangeTracker, FileChange};
