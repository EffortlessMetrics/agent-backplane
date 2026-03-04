// SPDX-License-Identifier: MIT OR Apache-2.0
//! Telemetry middleware hooks for request lifecycle logging.
//!
//! The hooks record structured tracing events at key points in a request's
//! lifecycle (start, completion, error) so operators can observe ABP behaviour
//! without coupling application code to a specific metrics backend.

use std::time::Instant;

use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// RequestOutcome
// ---------------------------------------------------------------------------

/// Describes how a request completed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestOutcome {
    /// The request succeeded.
    Success,
    /// The request failed with a classified error.
    Error {
        /// Machine-readable error code (e.g. `"timeout"`, `"rate_limit"`).
        code: String,
        /// Human-readable description.
        message: String,
    },
}

// ---------------------------------------------------------------------------
// ErrorClassification
// ---------------------------------------------------------------------------

/// Broad classification buckets for errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClassification {
    /// Transient / retriable (e.g. network blip, 503).
    Transient,
    /// Permanent / non-retriable (e.g. bad request, auth failure).
    Permanent,
    /// Unknown / unclassified.
    Unknown,
}

impl std::fmt::Display for ErrorClassification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transient => f.write_str("transient"),
            Self::Permanent => f.write_str("permanent"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

// ---------------------------------------------------------------------------
// Pre-request hook
// ---------------------------------------------------------------------------

/// Log the start of a request and return an [`Instant`] for later timing.
///
/// Emits an `info`-level tracing event with the work-order ID and backend.
pub fn on_request_start(work_order_id: &str, backend: &str) -> Instant {
    let start = Instant::now();
    info!(
        work_order_id = %work_order_id,
        backend = %backend,
        "request_started",
    );
    start
}

// ---------------------------------------------------------------------------
// Post-request hook
// ---------------------------------------------------------------------------

/// Log the completion of a request with timing information.
///
/// `start` should be the value returned by [`on_request_start`].
pub fn on_request_complete(
    work_order_id: &str,
    backend: &str,
    outcome: &RequestOutcome,
    start: Instant,
) -> u64 {
    let elapsed_ms = start.elapsed().as_millis() as u64;
    match outcome {
        RequestOutcome::Success => {
            info!(
                work_order_id = %work_order_id,
                backend = %backend,
                duration_ms = elapsed_ms,
                outcome = "success",
                "request_completed",
            );
        }
        RequestOutcome::Error { code, message } => {
            warn!(
                work_order_id = %work_order_id,
                backend = %backend,
                duration_ms = elapsed_ms,
                outcome = "error",
                error_code = %code,
                error_message = %message,
                "request_completed",
            );
        }
    }
    elapsed_ms
}

// ---------------------------------------------------------------------------
// Error hook
// ---------------------------------------------------------------------------

/// Log an error event with classification metadata.
///
/// This is intended for errors detected mid-stream (e.g. a sidecar crash),
/// not only for terminal request failures.
pub fn on_error(
    work_order_id: &str,
    error_code: &str,
    message: &str,
    classification: ErrorClassification,
) {
    error!(
        work_order_id = %work_order_id,
        error_code = %error_code,
        error_message = %message,
        classification = %classification,
        "request_error",
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_request_start_returns_instant() {
        let before = Instant::now();
        let start = on_request_start("wo-1", "mock");
        assert!(start >= before);
    }

    #[test]
    fn on_request_complete_success() {
        let start = Instant::now();
        let elapsed = on_request_complete("wo-1", "mock", &RequestOutcome::Success, start);
        // Elapsed should be tiny but non-negative.
        assert!(elapsed < 1000);
    }

    #[test]
    fn on_request_complete_error() {
        let start = Instant::now();
        let outcome = RequestOutcome::Error {
            code: "timeout".into(),
            message: "deadline exceeded".into(),
        };
        let elapsed = on_request_complete("wo-1", "mock", &outcome, start);
        assert!(elapsed < 1000);
    }

    #[test]
    fn on_error_does_not_panic() {
        on_error("wo-1", "E001", "something broke", ErrorClassification::Transient);
        on_error("wo-2", "E002", "bad input", ErrorClassification::Permanent);
        on_error("wo-3", "E999", "mystery", ErrorClassification::Unknown);
    }

    #[test]
    fn error_classification_display() {
        assert_eq!(ErrorClassification::Transient.to_string(), "transient");
        assert_eq!(ErrorClassification::Permanent.to_string(), "permanent");
        assert_eq!(ErrorClassification::Unknown.to_string(), "unknown");
    }

    #[test]
    fn request_outcome_equality() {
        assert_eq!(RequestOutcome::Success, RequestOutcome::Success);
        let e1 = RequestOutcome::Error {
            code: "x".into(),
            message: "y".into(),
        };
        let e2 = RequestOutcome::Error {
            code: "x".into(),
            message: "y".into(),
        };
        assert_eq!(e1, e2);
        assert_ne!(RequestOutcome::Success, e1);
    }
}
