// SPDX-License-Identifier: MIT OR Apache-2.0
//! Change approval workflow for workspace modifications.
//!
//! Provides [`ChangeApproval`] for a submit → approve/reject → apply
//! workflow, and [`ApprovalPolicy`] for determining which changes
//! require human review.

use crate::diff::WorkspaceDiff;
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Status of a change approval request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum ApprovalStatus {
    /// Changes have been submitted and are awaiting review.
    Pending,
    /// Changes have been approved.
    Approved {
        /// Timestamp of approval.
        approved_at: DateTime<Utc>,
    },
    /// Changes have been rejected.
    Rejected {
        /// Reason for rejection.
        reason: String,
        /// Timestamp of rejection.
        rejected_at: DateTime<Utc>,
    },
}

/// Policy for determining which changes require manual review.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovalPolicy {
    /// Maximum number of changed files before review is required.
    pub max_files_without_review: Option<usize>,
    /// Maximum number of added lines before review is required.
    pub max_additions_without_review: Option<usize>,
    /// If true, all changes require review regardless of size.
    pub require_review_for_all: bool,
    /// Glob patterns for paths that always require review.
    pub sensitive_paths: Vec<String>,
}

impl ApprovalPolicy {
    /// Create a permissive policy that never requires review.
    #[must_use]
    pub fn permissive() -> Self {
        Self {
            max_files_without_review: None,
            max_additions_without_review: None,
            require_review_for_all: false,
            sensitive_paths: Vec::new(),
        }
    }

    /// Create a strict policy that always requires review.
    #[must_use]
    pub fn strict() -> Self {
        Self {
            require_review_for_all: true,
            ..Self::default()
        }
    }

    /// Determine whether the given diff requires review.
    #[must_use]
    pub fn needs_review(&self, diff: &WorkspaceDiff) -> bool {
        if self.require_review_for_all {
            return true;
        }

        if let Some(max) = self.max_files_without_review {
            if diff.file_count() > max {
                return true;
            }
        }

        if let Some(max) = self.max_additions_without_review {
            if diff.total_additions > max {
                return true;
            }
        }

        if !self.sensitive_paths.is_empty() {
            if let Ok(globs) = abp_glob::IncludeExcludeGlobs::new(&self.sensitive_paths, &[]) {
                let all_files = diff
                    .files_added
                    .iter()
                    .chain(diff.files_modified.iter())
                    .chain(diff.files_deleted.iter());

                for fc in all_files {
                    if globs.decide_path(&fc.path).is_allowed() {
                        return true;
                    }
                }
            }
        }

        false
    }
}

/// Manages the change approval workflow: submit → approve/reject → apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeApproval {
    /// Current status of the approval.
    status: ApprovalStatus,
    /// The diff under review.
    diff: WorkspaceDiff,
    /// Timestamp when changes were submitted.
    submitted_at: DateTime<Utc>,
}

impl ChangeApproval {
    /// Submit changes for review.
    #[must_use]
    pub fn submit_changes(diff: WorkspaceDiff) -> Self {
        Self {
            status: ApprovalStatus::Pending,
            diff,
            submitted_at: Utc::now(),
        }
    }

    /// Get the current approval status.
    #[must_use]
    pub fn status(&self) -> &ApprovalStatus {
        &self.status
    }

    /// Get the diff under review.
    #[must_use]
    pub fn diff(&self) -> &WorkspaceDiff {
        &self.diff
    }

    /// Timestamp when the changes were submitted.
    #[must_use]
    pub fn submitted_at(&self) -> DateTime<Utc> {
        self.submitted_at
    }

    /// Returns `true` if the approval is still pending.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        matches!(self.status, ApprovalStatus::Pending)
    }

    /// Returns `true` if the changes have been approved.
    #[must_use]
    pub fn is_approved(&self) -> bool {
        matches!(self.status, ApprovalStatus::Approved { .. })
    }

    /// Returns `true` if the changes have been rejected.
    #[must_use]
    pub fn is_rejected(&self) -> bool {
        matches!(self.status, ApprovalStatus::Rejected { .. })
    }

    /// Approve the submitted changes.
    ///
    /// # Errors
    ///
    /// Returns an error if the approval is not in pending state.
    pub fn approve(&mut self) -> Result<()> {
        if !self.is_pending() {
            bail!("cannot approve: current status is not pending");
        }
        self.status = ApprovalStatus::Approved {
            approved_at: Utc::now(),
        };
        Ok(())
    }

    /// Reject the submitted changes with a reason.
    ///
    /// # Errors
    ///
    /// Returns an error if the approval is not in pending state.
    pub fn reject(&mut self, reason: &str) -> Result<()> {
        if !self.is_pending() {
            bail!("cannot reject: current status is not pending");
        }
        self.status = ApprovalStatus::Rejected {
            reason: reason.to_string(),
            rejected_at: Utc::now(),
        };
        Ok(())
    }

    /// Consume the approval and return the diff for application.
    ///
    /// Only succeeds if the changes have been approved.
    ///
    /// # Errors
    ///
    /// Returns an error if the changes have not been approved.
    pub fn apply(self) -> Result<WorkspaceDiff> {
        if !self.is_approved() {
            bail!("cannot apply: changes have not been approved");
        }
        Ok(self.diff)
    }
}
