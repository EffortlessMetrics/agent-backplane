// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cancellation primitives for runtime work-order execution.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Notify;
use std::sync::atomic::{AtomicBool, Ordering};

/// A cloneable, cheaply-shareable token used to signal cancellation.
///
/// All clones share the same underlying state; cancelling one
/// immediately makes every clone observe `is_cancelled() == true`.
#[derive(Clone)]
pub struct CancellationToken {
    inner: Arc<TokenInner>,
}

struct TokenInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancellationToken {
    /// Create a new token that is **not** cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TokenInner {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    /// Signal cancellation. Idempotent — calling more than once is harmless.
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    /// Returns `true` if [`cancel`](Self::cancel) has been called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Returns a future that completes when the token is cancelled.
    ///
    /// If the token is already cancelled the future resolves immediately.
    pub async fn cancelled(&self) {
        // Fast-path: already cancelled.
        if self.is_cancelled() {
            return;
        }
        loop {
            self.inner.notify.notified().await;
            if self.is_cancelled() {
                return;
            }
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CancellationToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

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
    fn token_starts_not_cancelled() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancel_flips_state() {
        let token = CancellationToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn clone_shares_state() {
        let a = CancellationToken::new();
        let b = a.clone();
        a.cancel();
        assert!(b.is_cancelled());
    }

    #[test]
    fn multiple_cancels_are_idempotent() {
        let token = CancellationToken::new();
        token.cancel();
        token.cancel();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn default_impl_is_not_cancelled() {
        let token = CancellationToken::default();
        assert!(!token.is_cancelled());
    }

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

    #[test]
    fn debug_format_includes_state() {
        let token = CancellationToken::new();
        let dbg = format!("{token:?}");
        assert!(dbg.contains("false"), "expected not-cancelled in debug: {dbg}");
        token.cancel();
        let dbg = format!("{token:?}");
        assert!(dbg.contains("true"), "expected cancelled in debug: {dbg}");
    }

    #[test]
    fn token_drop_does_not_panic() {
        let token = CancellationToken::new();
        let clone = token.clone();
        drop(token);
        // Clone should still work after original is dropped.
        assert!(!clone.is_cancelled());
        clone.cancel();
        assert!(clone.is_cancelled());
    }
}
