// SPDX-License-Identifier: MIT OR Apache-2.0
//! Backward-compatible re-exports for workspace disk quotas.
//!
//! Quota logic now lives in the dedicated `abp-workspace-quota` microcrate.

pub use abp_workspace_quota::{CleanupResult, QuotaStatus, WorkspaceQuota};
