// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cancellation primitives for runtime work-order execution.

pub use abp_cancel::CancellationToken;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ── CancellationReason ──────────────────────────────────────────────

/// Why a run was cancelled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancellationReason {
    /// The user explicitly requested cancellation.
    UserRequested,
    /// A deadline or timeout expired.
    Timeout,
    /// Token / cost budget was exhausted.
    BudgetExhausted,
    /// The run violated its policy profile.
    PolicyViolation,
    /// The host system is shutting down.
    SystemShutdown,
}

impl CancellationReason {
    /// Human-readable description of the reason.
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::UserRequested => "cancelled by user request",
            Self::Timeout => "cancelled due to timeout",
            Self::BudgetExhausted => "cancelled because budget was exhausted",
            Self::PolicyViolation => "cancelled due to policy violation",
            Self::SystemShutdown => "cancelled because the system is shutting down",
        }
    }
}

// ── CancellableRun ──────────────────────────────────────────────────

/// Wraps a [`CancellationToken`] together with an optional reason.
///
/// Intended to be attached to a single run so callers can both cancel
/// it and later inspect *why* it was cancelled.
#[derive(Debug, Clone)]
pub struct CancellableRun {
    token: CancellationToken,
    reason: Arc<std::sync::Mutex<Option<CancellationReason>>>,
}

impl CancellableRun {
    /// Create a new cancellable run wrapper.
    #[must_use]
    pub fn new(token: CancellationToken) -> Self {
        Self {
            token,
            reason: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Cancel the run with the given reason.
    ///
    /// Only the **first** reason is recorded; subsequent calls still
    /// signal the token but do not overwrite the reason.
    pub fn cancel(&self, reason: CancellationReason) {
        {
            let mut guard = self.reason.lock().expect("reason lock poisoned");
            if guard.is_none() {
                *guard = Some(reason);
            }
        }
        self.token.cancel();
    }

    /// Returns `true` if the run has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Returns the cancellation reason, if one has been recorded.
    #[must_use]
    pub fn reason(&self) -> Option<CancellationReason> {
        self.reason.lock().expect("reason lock poisoned").clone()
    }

    /// Returns a reference to the underlying [`CancellationToken`].
    #[must_use]
    pub fn token(&self) -> &CancellationToken {
        &self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_reasons_have_descriptions() {
        let reasons = [
            CancellationReason::UserRequested,
            CancellationReason::Timeout,
            CancellationReason::BudgetExhausted,
            CancellationReason::PolicyViolation,
            CancellationReason::SystemShutdown,
        ];
        for r in &reasons {
            assert!(!r.description().is_empty(), "{r:?} has empty description");
        }
    }

    #[test]
    fn cancellable_run_tracks_reason() {
        let run = CancellableRun::new(CancellationToken::new());
        assert!(run.reason().is_none());
        run.cancel(CancellationReason::Timeout);
        assert!(run.is_cancelled());
        assert_eq!(run.reason(), Some(CancellationReason::Timeout));
    }

    #[test]
    fn cancellable_run_keeps_first_reason() {
        let run = CancellableRun::new(CancellationToken::new());
        run.cancel(CancellationReason::Timeout);
        run.cancel(CancellationReason::UserRequested);
        assert_eq!(run.reason(), Some(CancellationReason::Timeout));
    }

    #[test]
    fn serde_roundtrip_for_cancellation_reason() {
        let reasons = [
            CancellationReason::UserRequested,
            CancellationReason::Timeout,
            CancellationReason::BudgetExhausted,
            CancellationReason::PolicyViolation,
            CancellationReason::SystemShutdown,
        ];
        for reason in &reasons {
            let json = serde_json::to_string(reason).unwrap();
            let back: CancellationReason = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, reason);
        }
    }
}
