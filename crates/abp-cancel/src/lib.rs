// SPDX-License-Identifier: MIT OR Apache-2.0
//! Shared cooperative cancellation token primitives.

#![deny(unsafe_code)]
#![warn(missing_docs)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

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
    fn debug_format_includes_state() {
        let token = CancellationToken::new();
        let dbg = format!("{token:?}");
        assert!(
            dbg.contains("false"),
            "expected not-cancelled in debug: {dbg}"
        );
        token.cancel();
        let dbg = format!("{token:?}");
        assert!(dbg.contains("true"), "expected cancelled in debug: {dbg}");
    }

    #[test]
    fn token_drop_does_not_panic() {
        let token = CancellationToken::new();
        let clone = token.clone();
        drop(token);
        assert!(!clone.is_cancelled());
        clone.cancel();
        assert!(clone.is_cancelled());
    }
}
