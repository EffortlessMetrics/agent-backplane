#![allow(dead_code, unused_imports)]
//! Recovery strategies for each error type.
//!
//! Given any typed error ([`mapping_errors::MappingError`], [`protocol_errors::ProtocolError`], [`vendor_errors::VendorApiError`]),
//! this module recommends a `RecoveryStrategy`: Retry, Fallback, Degrade, or Abort.
//!
//! Additionally provides:
//! - [`RetryPolicy`](crate::recovery::RetryPolicy) — configurable retry with exponential backoff and jitter
//! - [`FallbackChain`](crate::recovery::FallbackChain) — ordered list of fallback backends to try on failure
//! - [`CircuitBreakerPolicy`](crate::recovery::CircuitBreakerPolicy) — per-backend circuit breaker tracking
//! - [`RecoveryExecutor`](crate::recovery::RecoveryExecutor) — executes recovery strategies based on error type
//! - [`RecoveryReport`](crate::recovery::RecoveryReport) — report of recovery attempts and outcomes
//! - [`ErrorClassifier`](crate::recovery::ErrorClassifier) — classify errors as transient/permanent/degraded

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::ErrorCode;
use crate::mapping_errors::MappingError;
use crate::protocol_errors::ProtocolError;
use crate::vendor_errors::VendorApiError;

// ---------------------------------------------------------------------------
// RecoveryStrategy
// ---------------------------------------------------------------------------

/// The recommended recovery action after an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RecoveryStrategy {
    /// Retry the same operation after a delay.
    Retry {
        /// Suggested delay before retrying.
        delay_ms: u64,
        /// Maximum number of retries recommended.
        max_retries: u32,
    },
    /// Fall back to an alternative backend or dialect.
    Fallback {
        /// Human-readable description of the fallback option.
        suggestion: String,
    },
    /// Continue with degraded functionality (e.g. lossy mapping).
    Degrade {
        /// What capability or fidelity is being sacrificed.
        degradation: String,
    },
    /// Abort — the error is not recoverable without human intervention.
    Abort {
        /// Why recovery is impossible.
        reason: String,
    },
}

impl RecoveryStrategy {
    /// Stable code for the strategy type.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Retry { .. } => "ABP-REC-RETRY",
            Self::Fallback { .. } => "ABP-REC-FALLBACK",
            Self::Degrade { .. } => "ABP-REC-DEGRADE",
            Self::Abort { .. } => "ABP-REC-ABORT",
        }
    }

    /// Whether this strategy suggests the operation may eventually succeed.
    pub fn is_recoverable(&self) -> bool {
        !matches!(self, Self::Abort { .. })
    }
}

impl fmt::Display for RecoveryStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retry {
                delay_ms,
                max_retries,
            } => write!(
                f,
                "[{}] retry after {}ms (max {} attempts)",
                self.code(),
                delay_ms,
                max_retries
            ),
            Self::Fallback { suggestion } => {
                write!(f, "[{}] fallback: {}", self.code(), suggestion)
            }
            Self::Degrade { degradation } => {
                write!(f, "[{}] degrade: {}", self.code(), degradation)
            }
            Self::Abort { reason } => {
                write!(f, "[{}] abort: {}", self.code(), reason)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ErrorClassification
// ---------------------------------------------------------------------------

/// Classification of an error for recovery purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClassification {
    /// Error is transient and likely to resolve on retry.
    Transient,
    /// Error is permanent and will not resolve without intervention.
    Permanent,
    /// Error indicates degraded operation — partial success possible.
    Degraded,
}

impl fmt::Display for ErrorClassification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transient => f.write_str("transient"),
            Self::Permanent => f.write_str("permanent"),
            Self::Degraded => f.write_str("degraded"),
        }
    }
}

// ---------------------------------------------------------------------------
// ErrorClassifier
// ---------------------------------------------------------------------------

/// Classifies errors as transient, permanent, or degraded.
///
/// Uses [`ErrorCode`] semantics to determine whether an error is likely to
/// self-resolve (transient), requires intervention (permanent), or allows
/// partial progress (degraded).
#[derive(Debug, Clone, Default)]
pub struct ErrorClassifier;

impl ErrorClassifier {
    /// Create a new classifier.
    pub fn new() -> Self {
        Self
    }

    /// Classify an [`ErrorCode`].
    pub fn classify(&self, code: ErrorCode) -> ErrorClassification {
        match code {
            // Transient — retry may succeed
            ErrorCode::BackendUnavailable
            | ErrorCode::BackendTimeout
            | ErrorCode::BackendRateLimited
            | ErrorCode::BackendCrashed
            | ErrorCode::ProtocolHandshakeFailed
            | ErrorCode::WorkspaceInitFailed
            | ErrorCode::WorkspaceStagingFailed
            | ErrorCode::ExecutionToolFailed => ErrorClassification::Transient,

            // Degraded — partial success possible
            ErrorCode::MappingLossyConversion
            | ErrorCode::CapabilityEmulationFailed
            | ErrorCode::MappingUnmappableTool => ErrorClassification::Degraded,

            // Permanent — everything else
            ErrorCode::ProtocolInvalidEnvelope
            | ErrorCode::ProtocolMissingRefId
            | ErrorCode::ProtocolUnexpectedMessage
            | ErrorCode::ProtocolVersionMismatch
            | ErrorCode::MappingUnsupportedCapability
            | ErrorCode::MappingDialectMismatch
            | ErrorCode::BackendNotFound
            | ErrorCode::BackendAuthFailed
            | ErrorCode::BackendModelNotFound
            | ErrorCode::ExecutionWorkspaceError
            | ErrorCode::ExecutionPermissionDenied
            | ErrorCode::ContractVersionMismatch
            | ErrorCode::ContractSchemaViolation
            | ErrorCode::ContractInvalidReceipt
            | ErrorCode::CapabilityUnsupported
            | ErrorCode::PolicyDenied
            | ErrorCode::PolicyInvalid
            | ErrorCode::IrLoweringFailed
            | ErrorCode::IrInvalid
            | ErrorCode::ReceiptHashMismatch
            | ErrorCode::ReceiptChainBroken
            | ErrorCode::DialectUnknown
            | ErrorCode::DialectMappingFailed
            | ErrorCode::ConfigInvalid
            | ErrorCode::Internal => ErrorClassification::Permanent,
        }
    }

    /// Classify a [`MappingError`].
    pub fn classify_mapping(&self, err: &MappingError) -> ErrorClassification {
        match err {
            MappingError::FidelityLoss { .. } => ErrorClassification::Degraded,
            MappingError::EmulationFailed { .. } => ErrorClassification::Degraded,
            MappingError::FeatureUnsupported { .. }
            | MappingError::AmbiguousMapping { .. }
            | MappingError::NegotiationFailed { .. } => ErrorClassification::Permanent,
        }
    }

    /// Classify a [`ProtocolError`].
    pub fn classify_protocol(&self, err: &ProtocolError) -> ErrorClassification {
        match err {
            ProtocolError::HandshakeFailed { .. }
            | ProtocolError::StreamInterrupted { .. }
            | ProtocolError::TimeoutExpired { .. }
            | ProtocolError::SidecarCrashed { .. } => ErrorClassification::Transient,
            ProtocolError::VersionMismatch { .. } | ProtocolError::EnvelopeMalformed { .. } => {
                ErrorClassification::Permanent
            }
        }
    }

    /// Classify a [`VendorApiError`].
    pub fn classify_vendor(&self, err: &VendorApiError) -> ErrorClassification {
        let d = err.detail();
        match d.status_code {
            429 | 500 | 502 | 503 | 504 | 408 => ErrorClassification::Transient,
            401 | 403 | 404 | 422 => ErrorClassification::Permanent,
            _ => ErrorClassification::Permanent,
        }
    }
}

// ---------------------------------------------------------------------------
// RetryPolicy
// ---------------------------------------------------------------------------

/// Configurable retry policy with exponential backoff and optional jitter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
    /// Initial delay before the first retry.
    pub initial_delay: Duration,
    /// Maximum delay cap (backoff will not exceed this).
    pub max_delay: Duration,
    /// Backoff multiplier applied after each attempt.
    pub backoff_multiplier: u32,
    /// Whether to add random jitter to delay.
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 2,
            jitter: true,
        }
    }
}

impl RetryPolicy {
    /// Create a new retry policy with the given max attempts.
    pub fn new(max_attempts: u32) -> Self {
        Self {
            max_attempts,
            ..Default::default()
        }
    }

    /// Set the initial delay.
    pub fn with_initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    /// Set the max delay cap.
    pub fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Set the backoff multiplier.
    pub fn with_backoff_multiplier(mut self, multiplier: u32) -> Self {
        self.backoff_multiplier = multiplier;
        self
    }

    /// Enable or disable jitter.
    pub fn with_jitter(mut self, jitter: bool) -> Self {
        self.jitter = jitter;
        self
    }

    /// Compute the delay for a given attempt number (0-based).
    ///
    /// Uses `jitter_seed` (0..=100) to deterministically add jitter when
    /// enabled, avoiding a dependency on `rand`. Pass `None` for no jitter
    /// regardless of policy setting.
    pub fn delay_for_attempt(&self, attempt: u32, jitter_seed: Option<u32>) -> Duration {
        if attempt >= self.max_attempts {
            return Duration::ZERO;
        }
        let base_ms = self.initial_delay.as_millis() as u64;
        let multiplied =
            base_ms.saturating_mul((self.backoff_multiplier as u64).saturating_pow(attempt));
        let capped = multiplied.min(self.max_delay.as_millis() as u64);

        if self.jitter {
            if let Some(seed) = jitter_seed {
                // Jitter: add 0–50% of capped value based on seed
                let jitter_frac = (seed.min(100) as u64) * capped / 200;
                return Duration::from_millis(capped.saturating_add(jitter_frac));
            }
        }
        Duration::from_millis(capped)
    }

    /// Whether the given attempt number (0-based) is within the allowed range.
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_attempts
    }
}

// ---------------------------------------------------------------------------
// FallbackChain
// ---------------------------------------------------------------------------

/// Ordered list of fallback backends to try on failure.
///
/// When the primary backend fails, the chain is traversed in order until one
/// succeeds or all have been exhausted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackChain {
    /// Ordered backend identifiers to try.
    pub backends: Vec<String>,
}

impl FallbackChain {
    /// Create a new fallback chain.
    pub fn new(backends: Vec<String>) -> Self {
        Self { backends }
    }

    /// Whether the chain is empty (no fallbacks available).
    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }

    /// Number of fallback backends.
    pub fn len(&self) -> usize {
        self.backends.len()
    }

    /// Get the next fallback backend after the given index.
    /// Returns `None` if all backends have been exhausted.
    pub fn next_backend(&self, current_index: usize) -> Option<&str> {
        self.backends.get(current_index).map(|s| s.as_str())
    }

    /// Iterate over all backends in order.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.backends.iter().map(|s| s.as_str())
    }

    /// Execute the fallback chain with the given closure.
    /// Returns the result of the first successful backend, or all errors.
    pub fn execute<F, T, E>(&self, mut f: F) -> Result<(usize, T), Vec<(String, E)>>
    where
        F: FnMut(&str) -> Result<T, E>,
    {
        let mut errors = Vec::new();
        for (idx, backend) in self.backends.iter().enumerate() {
            match f(backend) {
                Ok(val) => return Ok((idx, val)),
                Err(e) => errors.push((backend.clone(), e)),
            }
        }
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// CircuitBreakerState
// ---------------------------------------------------------------------------

/// State of a circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitBreakerState {
    /// Normal operation — requests flow through.
    Closed,
    /// Failures exceeded threshold — requests are rejected immediately.
    Open,
    /// Trial period — a limited number of requests are allowed through.
    HalfOpen,
}

impl fmt::Display for CircuitBreakerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.write_str("closed"),
            Self::Open => f.write_str("open"),
            Self::HalfOpen => f.write_str("half_open"),
        }
    }
}

// ---------------------------------------------------------------------------
// CircuitBreakerPolicy
// ---------------------------------------------------------------------------

/// Per-backend circuit breaker that tracks failure counts and transitions
/// between closed → open → half-open states.
pub struct CircuitBreakerPolicy {
    /// How many consecutive failures before opening the circuit.
    pub failure_threshold: u32,
    /// How many consecutive successes in half-open before closing again.
    pub success_threshold: u32,
    /// Duration the circuit stays open before transitioning to half-open.
    pub open_duration: Duration,
    state: Mutex<CircuitBreakerState>,
    consecutive_failures: AtomicU64,
    consecutive_successes: AtomicU64,
}

impl fmt::Debug for CircuitBreakerPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CircuitBreakerPolicy")
            .field("failure_threshold", &self.failure_threshold)
            .field("success_threshold", &self.success_threshold)
            .field("open_duration", &self.open_duration)
            .field("state", &self.state())
            .field(
                "consecutive_failures",
                &self.consecutive_failures.load(Ordering::Relaxed),
            )
            .field(
                "consecutive_successes",
                &self.consecutive_successes.load(Ordering::Relaxed),
            )
            .finish()
    }
}

impl CircuitBreakerPolicy {
    /// Create a new circuit breaker with default thresholds.
    pub fn new(failure_threshold: u32, success_threshold: u32, open_duration: Duration) -> Self {
        Self {
            failure_threshold,
            success_threshold,
            open_duration,
            state: Mutex::new(CircuitBreakerState::Closed),
            consecutive_failures: AtomicU64::new(0),
            consecutive_successes: AtomicU64::new(0),
        }
    }

    /// Current state of the circuit breaker.
    pub fn state(&self) -> CircuitBreakerState {
        *self.state.lock().unwrap()
    }

    /// Whether requests should be allowed through.
    pub fn is_call_permitted(&self) -> bool {
        let s = self.state.lock().unwrap();
        matches!(
            *s,
            CircuitBreakerState::Closed | CircuitBreakerState::HalfOpen
        )
    }

    /// Record a successful call.
    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        let prev = self.consecutive_successes.fetch_add(1, Ordering::Relaxed);

        let mut s = self.state.lock().unwrap();
        if *s == CircuitBreakerState::HalfOpen && (prev + 1) >= self.success_threshold as u64 {
            *s = CircuitBreakerState::Closed;
            self.consecutive_successes.store(0, Ordering::Relaxed);
        }
    }

    /// Record a failed call.
    pub fn record_failure(&self) {
        self.consecutive_successes.store(0, Ordering::Relaxed);
        let prev = self.consecutive_failures.fetch_add(1, Ordering::Relaxed);

        let mut s = self.state.lock().unwrap();
        if *s == CircuitBreakerState::Closed && (prev + 1) >= self.failure_threshold as u64 {
            *s = CircuitBreakerState::Open;
        } else if *s == CircuitBreakerState::HalfOpen {
            // Any failure in half-open goes back to open
            *s = CircuitBreakerState::Open;
            self.consecutive_failures.store(0, Ordering::Relaxed);
        }
    }

    /// Manually transition to half-open (simulates open_duration expiry).
    pub fn transition_to_half_open(&self) {
        let mut s = self.state.lock().unwrap();
        if *s == CircuitBreakerState::Open {
            *s = CircuitBreakerState::HalfOpen;
            self.consecutive_failures.store(0, Ordering::Relaxed);
            self.consecutive_successes.store(0, Ordering::Relaxed);
        }
    }

    /// Reset the circuit breaker to closed state.
    pub fn reset(&self) {
        let mut s = self.state.lock().unwrap();
        *s = CircuitBreakerState::Closed;
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.consecutive_successes.store(0, Ordering::Relaxed);
    }

    /// Current number of consecutive failures.
    pub fn failure_count(&self) -> u64 {
        self.consecutive_failures.load(Ordering::Relaxed)
    }

    /// Current number of consecutive successes.
    pub fn success_count(&self) -> u64 {
        self.consecutive_successes.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// RecoveryAttempt + RecoveryReport
// ---------------------------------------------------------------------------

/// Outcome of a single recovery attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryOutcome {
    /// The recovery attempt succeeded.
    Success,
    /// The recovery attempt failed and more attempts may follow.
    Failed,
    /// All recovery attempts exhausted.
    Exhausted,
    /// The error was classified as non-recoverable.
    Rejected,
}

impl fmt::Display for RecoveryOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success => f.write_str("success"),
            Self::Failed => f.write_str("failed"),
            Self::Exhausted => f.write_str("exhausted"),
            Self::Rejected => f.write_str("rejected"),
        }
    }
}

/// A single recovery attempt record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryAttempt {
    /// Which strategy was tried.
    pub strategy: RecoveryStrategy,
    /// 0-based attempt index within the strategy.
    pub attempt_number: u32,
    /// The backend that was tried (if applicable).
    pub backend: Option<String>,
    /// Outcome of this attempt.
    pub outcome: RecoveryOutcome,
    /// Optional error message if the attempt failed.
    pub error_message: Option<String>,
}

/// Report summarising all recovery attempts for a single operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryReport {
    /// Classification of the original error.
    pub classification: ErrorClassification,
    /// Ordered list of attempts.
    pub attempts: Vec<RecoveryAttempt>,
    /// Final outcome of the entire recovery process.
    pub final_outcome: RecoveryOutcome,
    /// The backend that ultimately handled the request (if any).
    pub successful_backend: Option<String>,
}

impl RecoveryReport {
    /// Create a new empty report for the given classification.
    pub fn new(classification: ErrorClassification) -> Self {
        Self {
            classification,
            attempts: Vec::new(),
            final_outcome: RecoveryOutcome::Failed,
            successful_backend: None,
        }
    }

    /// Record an attempt.
    pub fn record(&mut self, attempt: RecoveryAttempt) {
        if attempt.outcome == RecoveryOutcome::Success {
            self.final_outcome = RecoveryOutcome::Success;
            self.successful_backend = attempt.backend.clone();
        }
        self.attempts.push(attempt);
    }

    /// Mark the report as exhausted (all strategies tried).
    pub fn mark_exhausted(&mut self) {
        if self.final_outcome != RecoveryOutcome::Success {
            self.final_outcome = RecoveryOutcome::Exhausted;
        }
    }

    /// Mark the report as rejected (error not recoverable).
    pub fn mark_rejected(&mut self) {
        self.final_outcome = RecoveryOutcome::Rejected;
    }

    /// Whether the recovery ultimately succeeded.
    pub fn succeeded(&self) -> bool {
        self.final_outcome == RecoveryOutcome::Success
    }

    /// Total number of attempts made.
    pub fn total_attempts(&self) -> usize {
        self.attempts.len()
    }
}

// ---------------------------------------------------------------------------
// RecoveryExecutor
// ---------------------------------------------------------------------------

/// Executes recovery strategies based on error type, combining classification,
/// retry, fallback, and circuit-breaker logic.
pub struct RecoveryExecutor {
    /// Classifier used to determine error type.
    pub classifier: ErrorClassifier,
    /// Default retry policy.
    pub retry_policy: RetryPolicy,
    /// Optional fallback chain.
    pub fallback_chain: Option<FallbackChain>,
}

impl fmt::Debug for RecoveryExecutor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoveryExecutor")
            .field("classifier", &self.classifier)
            .field("retry_policy", &self.retry_policy)
            .field("fallback_chain", &self.fallback_chain)
            .finish()
    }
}

impl RecoveryExecutor {
    /// Create a new executor with the given retry policy.
    pub fn new(retry_policy: RetryPolicy) -> Self {
        Self {
            classifier: ErrorClassifier::new(),
            retry_policy,
            fallback_chain: None,
        }
    }

    /// Attach a fallback chain.
    pub fn with_fallback_chain(mut self, chain: FallbackChain) -> Self {
        self.fallback_chain = Some(chain);
        self
    }

    /// Plan recovery for the given error code, returning a report describing
    /// what *would* be done (no actual execution). The caller drives retries.
    pub fn plan_recovery(&self, code: ErrorCode) -> RecoveryReport {
        let classification = self.classifier.classify(code);
        let mut report = RecoveryReport::new(classification);

        match classification {
            ErrorClassification::Permanent => {
                report.mark_rejected();
            }
            ErrorClassification::Transient => {
                // Record planned retry attempts
                for attempt in 0..self.retry_policy.max_attempts {
                    let delay = self.retry_policy.delay_for_attempt(attempt, None);
                    report.record(RecoveryAttempt {
                        strategy: RecoveryStrategy::Retry {
                            delay_ms: delay.as_millis() as u64,
                            max_retries: self.retry_policy.max_attempts,
                        },
                        attempt_number: attempt,
                        backend: None,
                        outcome: RecoveryOutcome::Failed,
                        error_message: None,
                    });
                }
                // Add fallback attempts if chain exists
                if let Some(chain) = &self.fallback_chain {
                    for (idx, backend) in chain.iter().enumerate() {
                        report.record(RecoveryAttempt {
                            strategy: RecoveryStrategy::Fallback {
                                suggestion: format!("try backend '{}'", backend),
                            },
                            attempt_number: idx as u32,
                            backend: Some(backend.to_string()),
                            outcome: RecoveryOutcome::Failed,
                            error_message: None,
                        });
                    }
                }
                report.mark_exhausted();
            }
            ErrorClassification::Degraded => {
                report.record(RecoveryAttempt {
                    strategy: RecoveryStrategy::Degrade {
                        degradation: "proceed with reduced fidelity".into(),
                    },
                    attempt_number: 0,
                    backend: None,
                    outcome: RecoveryOutcome::Success,
                    error_message: None,
                });
            }
        }

        report
    }

    /// Execute recovery with retries using the given closure.
    /// Returns a report and the successful result, or just the report on failure.
    pub fn execute_with_retries<F, T>(
        &self,
        code: ErrorCode,
        mut operation: F,
    ) -> (RecoveryReport, Option<T>)
    where
        F: FnMut(u32) -> Result<T, String>,
    {
        let classification = self.classifier.classify(code);
        let mut report = RecoveryReport::new(classification);

        if classification == ErrorClassification::Permanent {
            report.mark_rejected();
            return (report, None);
        }

        if classification == ErrorClassification::Degraded {
            // Try once
            match operation(0) {
                Ok(val) => {
                    report.record(RecoveryAttempt {
                        strategy: RecoveryStrategy::Degrade {
                            degradation: "proceed with reduced fidelity".into(),
                        },
                        attempt_number: 0,
                        backend: None,
                        outcome: RecoveryOutcome::Success,
                        error_message: None,
                    });
                    return (report, Some(val));
                }
                Err(msg) => {
                    report.record(RecoveryAttempt {
                        strategy: RecoveryStrategy::Degrade {
                            degradation: "proceed with reduced fidelity".into(),
                        },
                        attempt_number: 0,
                        backend: None,
                        outcome: RecoveryOutcome::Failed,
                        error_message: Some(msg),
                    });
                    report.mark_exhausted();
                    return (report, None);
                }
            }
        }

        // Transient — try retries
        for attempt in 0..self.retry_policy.max_attempts {
            let delay = self.retry_policy.delay_for_attempt(attempt, None);
            match operation(attempt) {
                Ok(val) => {
                    report.record(RecoveryAttempt {
                        strategy: RecoveryStrategy::Retry {
                            delay_ms: delay.as_millis() as u64,
                            max_retries: self.retry_policy.max_attempts,
                        },
                        attempt_number: attempt,
                        backend: None,
                        outcome: RecoveryOutcome::Success,
                        error_message: None,
                    });
                    return (report, Some(val));
                }
                Err(msg) => {
                    report.record(RecoveryAttempt {
                        strategy: RecoveryStrategy::Retry {
                            delay_ms: delay.as_millis() as u64,
                            max_retries: self.retry_policy.max_attempts,
                        },
                        attempt_number: attempt,
                        backend: None,
                        outcome: RecoveryOutcome::Failed,
                        error_message: Some(msg),
                    });
                }
            }
        }

        report.mark_exhausted();
        (report, None)
    }
}

// ---------------------------------------------------------------------------
// Recommend recovery for each error type
// ---------------------------------------------------------------------------

/// Recommend a recovery strategy for a [`MappingError`].
pub fn recover_mapping(err: &MappingError) -> RecoveryStrategy {
    match err {
        MappingError::FeatureUnsupported { .. } => RecoveryStrategy::Fallback {
            suggestion: "use a backend that supports the required feature".into(),
        },
        MappingError::EmulationFailed { .. } => RecoveryStrategy::Fallback {
            suggestion: "use a backend with native support for the feature".into(),
        },
        MappingError::FidelityLoss { .. } => RecoveryStrategy::Degrade {
            degradation: "proceed with approximated value".into(),
        },
        MappingError::AmbiguousMapping { .. } => RecoveryStrategy::Abort {
            reason: "ambiguous mapping requires explicit configuration".into(),
        },
        MappingError::NegotiationFailed { .. } => RecoveryStrategy::Abort {
            reason: "no compatible capability set found".into(),
        },
    }
}

/// Recommend a recovery strategy for a [`ProtocolError`].
pub fn recover_protocol(err: &ProtocolError) -> RecoveryStrategy {
    match err {
        ProtocolError::HandshakeFailed { .. } => RecoveryStrategy::Retry {
            delay_ms: 1000,
            max_retries: 3,
        },
        ProtocolError::VersionMismatch { .. } => RecoveryStrategy::Abort {
            reason: "contract version mismatch requires sidecar update".into(),
        },
        ProtocolError::EnvelopeMalformed { .. } => RecoveryStrategy::Abort {
            reason: "malformed envelope indicates a sidecar bug".into(),
        },
        ProtocolError::StreamInterrupted { .. } => RecoveryStrategy::Retry {
            delay_ms: 2000,
            max_retries: 2,
        },
        ProtocolError::TimeoutExpired { .. } => RecoveryStrategy::Retry {
            delay_ms: 5000,
            max_retries: 2,
        },
        ProtocolError::SidecarCrashed { .. } => RecoveryStrategy::Retry {
            delay_ms: 3000,
            max_retries: 1,
        },
    }
}

/// Recommend a recovery strategy for a [`VendorApiError`].
pub fn recover_vendor(err: &VendorApiError) -> RecoveryStrategy {
    let d = err.detail();
    match d.status_code {
        429 => {
            let delay = d.retry_after_secs.unwrap_or(30) * 1000;
            RecoveryStrategy::Retry {
                delay_ms: delay,
                max_retries: 3,
            }
        }
        500 | 502 | 503 => RecoveryStrategy::Retry {
            delay_ms: d.retry_after_secs.map(|s| s * 1000).unwrap_or(5000),
            max_retries: 3,
        },
        504 | 408 => RecoveryStrategy::Retry {
            delay_ms: 10000,
            max_retries: 2,
        },
        401 => RecoveryStrategy::Abort {
            reason: "authentication failed — check API key".into(),
        },
        403 => RecoveryStrategy::Abort {
            reason: "permission denied by vendor".into(),
        },
        404 => RecoveryStrategy::Fallback {
            suggestion: "model not found — try an alternative model".into(),
        },
        _ => RecoveryStrategy::Abort {
            reason: format!("unexpected HTTP status {}", d.status_code),
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vendor_errors::VendorErrorDetail;

    // == RecoveryStrategy basics ==========================================

    #[test]
    fn retry_code() {
        let s = RecoveryStrategy::Retry {
            delay_ms: 1000,
            max_retries: 3,
        };
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    #[test]
    fn fallback_code() {
        let s = RecoveryStrategy::Fallback {
            suggestion: "x".into(),
        };
        assert_eq!(s.code(), "ABP-REC-FALLBACK");
    }

    #[test]
    fn degrade_code() {
        let s = RecoveryStrategy::Degrade {
            degradation: "x".into(),
        };
        assert_eq!(s.code(), "ABP-REC-DEGRADE");
    }

    #[test]
    fn abort_code() {
        let s = RecoveryStrategy::Abort { reason: "x".into() };
        assert_eq!(s.code(), "ABP-REC-ABORT");
    }

    #[test]
    fn retry_is_recoverable() {
        let s = RecoveryStrategy::Retry {
            delay_ms: 100,
            max_retries: 1,
        };
        assert!(s.is_recoverable());
    }

    #[test]
    fn fallback_is_recoverable() {
        let s = RecoveryStrategy::Fallback {
            suggestion: "x".into(),
        };
        assert!(s.is_recoverable());
    }

    #[test]
    fn degrade_is_recoverable() {
        let s = RecoveryStrategy::Degrade {
            degradation: "x".into(),
        };
        assert!(s.is_recoverable());
    }

    #[test]
    fn abort_is_not_recoverable() {
        let s = RecoveryStrategy::Abort { reason: "x".into() };
        assert!(!s.is_recoverable());
    }

    #[test]
    fn display_retry() {
        let s = RecoveryStrategy::Retry {
            delay_ms: 5000,
            max_retries: 3,
        };
        let d = s.to_string();
        assert!(d.contains("ABP-REC-RETRY"));
        assert!(d.contains("5000ms"));
        assert!(d.contains("3 attempts"));
    }

    #[test]
    fn display_fallback() {
        let s = RecoveryStrategy::Fallback {
            suggestion: "use openai".into(),
        };
        assert!(s.to_string().contains("use openai"));
    }

    #[test]
    fn display_degrade() {
        let s = RecoveryStrategy::Degrade {
            degradation: "lose precision".into(),
        };
        assert!(s.to_string().contains("lose precision"));
    }

    #[test]
    fn display_abort() {
        let s = RecoveryStrategy::Abort {
            reason: "fatal".into(),
        };
        assert!(s.to_string().contains("fatal"));
    }

    #[test]
    fn serde_roundtrip_retry() {
        let s = RecoveryStrategy::Retry {
            delay_ms: 1000,
            max_retries: 2,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RecoveryStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn serde_roundtrip_fallback() {
        let s = RecoveryStrategy::Fallback {
            suggestion: "alt".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RecoveryStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn serde_roundtrip_degrade() {
        let s = RecoveryStrategy::Degrade {
            degradation: "lossy".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RecoveryStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn serde_roundtrip_abort() {
        let s = RecoveryStrategy::Abort {
            reason: "done".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RecoveryStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    // == recover_mapping() ================================================

    #[test]
    fn mapping_feature_unsupported_fallback() {
        let e = MappingError::FeatureUnsupported {
            feature: "vision".into(),
            source_dialect: "openai".into(),
            target_dialect: "gemini".into(),
        };
        let s = recover_mapping(&e);
        assert_eq!(s.code(), "ABP-REC-FALLBACK");
    }

    #[test]
    fn mapping_emulation_failed_fallback() {
        let e = MappingError::EmulationFailed {
            feature: "tool_use".into(),
            reason: "no adapter".into(),
        };
        let s = recover_mapping(&e);
        assert_eq!(s.code(), "ABP-REC-FALLBACK");
    }

    #[test]
    fn mapping_fidelity_loss_degrade() {
        let e = MappingError::FidelityLoss {
            field: "temp".into(),
            original: "0.7".into(),
            approximation: "0.5".into(),
        };
        let s = recover_mapping(&e);
        assert_eq!(s.code(), "ABP-REC-DEGRADE");
    }

    #[test]
    fn mapping_ambiguous_abort() {
        let e = MappingError::AmbiguousMapping {
            field: "role".into(),
            candidates: vec!["a".into(), "b".into()],
        };
        let s = recover_mapping(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
        assert!(!s.is_recoverable());
    }

    #[test]
    fn mapping_negotiation_abort() {
        let e = MappingError::NegotiationFailed {
            reason: "incompatible".into(),
        };
        let s = recover_mapping(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
    }

    // == recover_protocol() ===============================================

    #[test]
    fn protocol_handshake_retry() {
        let e = ProtocolError::HandshakeFailed {
            reason: "timeout".into(),
        };
        let s = recover_protocol(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    #[test]
    fn protocol_version_abort() {
        let e = ProtocolError::VersionMismatch {
            expected: "v0.1".into(),
            actual: "v0.2".into(),
        };
        let s = recover_protocol(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
    }

    #[test]
    fn protocol_envelope_abort() {
        let e = ProtocolError::EnvelopeMalformed {
            raw_line: "bad".into(),
            parse_error: "err".into(),
        };
        let s = recover_protocol(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
    }

    #[test]
    fn protocol_stream_retry() {
        let e = ProtocolError::StreamInterrupted {
            events_received: 5,
            reason: "eof".into(),
        };
        let s = recover_protocol(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    #[test]
    fn protocol_timeout_retry() {
        let e = ProtocolError::TimeoutExpired {
            operation: "hello".into(),
            timeout_ms: 5000,
        };
        let s = recover_protocol(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    #[test]
    fn protocol_crash_retry() {
        let e = ProtocolError::SidecarCrashed {
            exit_code: Some(1),
            stderr_tail: "err".into(),
        };
        let s = recover_protocol(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    // == recover_vendor() =================================================

    #[test]
    fn vendor_429_retry() {
        let e = VendorApiError::OpenAi(VendorErrorDetail::new(429, "rate limited"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
        if let RecoveryStrategy::Retry { delay_ms, .. } = &s {
            assert_eq!(*delay_ms, 30000); // default 30s
        } else {
            panic!("expected Retry");
        }
    }

    #[test]
    fn vendor_429_with_retry_after() {
        let e = VendorApiError::OpenAi(VendorErrorDetail::new(429, "limited").with_retry_after(10));
        let s = recover_vendor(&e);
        if let RecoveryStrategy::Retry { delay_ms, .. } = &s {
            assert_eq!(*delay_ms, 10000);
        } else {
            panic!("expected Retry");
        }
    }

    #[test]
    fn vendor_503_retry() {
        let e = VendorApiError::Gemini(VendorErrorDetail::new(503, "overloaded"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    #[test]
    fn vendor_504_retry() {
        let e = VendorApiError::Claude(VendorErrorDetail::new(504, "gateway timeout"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    #[test]
    fn vendor_401_abort() {
        let e = VendorApiError::Claude(VendorErrorDetail::new(401, "bad key"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
        assert!(!s.is_recoverable());
    }

    #[test]
    fn vendor_403_abort() {
        let e = VendorApiError::Copilot(VendorErrorDetail::new(403, "forbidden"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
    }

    #[test]
    fn vendor_404_fallback() {
        let e = VendorApiError::Codex(VendorErrorDetail::new(404, "not found"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-FALLBACK");
    }

    #[test]
    fn vendor_unknown_status_abort() {
        let e = VendorApiError::Kimi(VendorErrorDetail::new(418, "teapot"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
    }

    // == ErrorClassifier ==================================================

    #[test]
    fn classify_backend_unavailable_transient() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::BackendUnavailable),
            ErrorClassification::Transient
        );
    }

    #[test]
    fn classify_backend_timeout_transient() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::BackendTimeout),
            ErrorClassification::Transient
        );
    }

    #[test]
    fn classify_backend_rate_limited_transient() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::BackendRateLimited),
            ErrorClassification::Transient
        );
    }

    #[test]
    fn classify_backend_crashed_transient() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::BackendCrashed),
            ErrorClassification::Transient
        );
    }

    #[test]
    fn classify_handshake_failed_transient() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::ProtocolHandshakeFailed),
            ErrorClassification::Transient
        );
    }

    #[test]
    fn classify_workspace_init_transient() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::WorkspaceInitFailed),
            ErrorClassification::Transient
        );
    }

    #[test]
    fn classify_execution_tool_transient() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::ExecutionToolFailed),
            ErrorClassification::Transient
        );
    }

    #[test]
    fn classify_lossy_conversion_degraded() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::MappingLossyConversion),
            ErrorClassification::Degraded
        );
    }

    #[test]
    fn classify_emulation_failed_degraded() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::CapabilityEmulationFailed),
            ErrorClassification::Degraded
        );
    }

    #[test]
    fn classify_unmappable_tool_degraded() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::MappingUnmappableTool),
            ErrorClassification::Degraded
        );
    }

    #[test]
    fn classify_auth_failed_permanent() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::BackendAuthFailed),
            ErrorClassification::Permanent
        );
    }

    #[test]
    fn classify_policy_denied_permanent() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::PolicyDenied),
            ErrorClassification::Permanent
        );
    }

    #[test]
    fn classify_config_invalid_permanent() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::ConfigInvalid),
            ErrorClassification::Permanent
        );
    }

    #[test]
    fn classify_internal_permanent() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::Internal),
            ErrorClassification::Permanent
        );
    }

    #[test]
    fn classify_version_mismatch_permanent() {
        let c = ErrorClassifier::new();
        assert_eq!(
            c.classify(ErrorCode::ProtocolVersionMismatch),
            ErrorClassification::Permanent
        );
    }

    #[test]
    fn classify_mapping_error_fidelity_loss_degraded() {
        let c = ErrorClassifier::new();
        let e = MappingError::FidelityLoss {
            field: "temp".into(),
            original: "0.7".into(),
            approximation: "0.5".into(),
        };
        assert_eq!(c.classify_mapping(&e), ErrorClassification::Degraded);
    }

    #[test]
    fn classify_mapping_error_negotiation_permanent() {
        let c = ErrorClassifier::new();
        let e = MappingError::NegotiationFailed {
            reason: "nope".into(),
        };
        assert_eq!(c.classify_mapping(&e), ErrorClassification::Permanent);
    }

    #[test]
    fn classify_mapping_error_emulation_degraded() {
        let c = ErrorClassifier::new();
        let e = MappingError::EmulationFailed {
            feature: "tool".into(),
            reason: "fail".into(),
        };
        assert_eq!(c.classify_mapping(&e), ErrorClassification::Degraded);
    }

    #[test]
    fn classify_protocol_handshake_transient() {
        let c = ErrorClassifier::new();
        let e = ProtocolError::HandshakeFailed {
            reason: "slow".into(),
        };
        assert_eq!(c.classify_protocol(&e), ErrorClassification::Transient);
    }

    #[test]
    fn classify_protocol_version_mismatch_permanent() {
        let c = ErrorClassifier::new();
        let e = ProtocolError::VersionMismatch {
            expected: "v0.1".into(),
            actual: "v0.2".into(),
        };
        assert_eq!(c.classify_protocol(&e), ErrorClassification::Permanent);
    }

    #[test]
    fn classify_protocol_stream_interrupted_transient() {
        let c = ErrorClassifier::new();
        let e = ProtocolError::StreamInterrupted {
            events_received: 5,
            reason: "eof".into(),
        };
        assert_eq!(c.classify_protocol(&e), ErrorClassification::Transient);
    }

    #[test]
    fn classify_protocol_timeout_transient() {
        let c = ErrorClassifier::new();
        let e = ProtocolError::TimeoutExpired {
            operation: "run".into(),
            timeout_ms: 5000,
        };
        assert_eq!(c.classify_protocol(&e), ErrorClassification::Transient);
    }

    #[test]
    fn classify_protocol_crash_transient() {
        let c = ErrorClassifier::new();
        let e = ProtocolError::SidecarCrashed {
            exit_code: Some(1),
            stderr_tail: "err".into(),
        };
        assert_eq!(c.classify_protocol(&e), ErrorClassification::Transient);
    }

    #[test]
    fn classify_protocol_malformed_permanent() {
        let c = ErrorClassifier::new();
        let e = ProtocolError::EnvelopeMalformed {
            raw_line: "x".into(),
            parse_error: "y".into(),
        };
        assert_eq!(c.classify_protocol(&e), ErrorClassification::Permanent);
    }

    #[test]
    fn classify_vendor_429_transient() {
        let c = ErrorClassifier::new();
        let e = VendorApiError::OpenAi(VendorErrorDetail::new(429, "rate limited"));
        assert_eq!(c.classify_vendor(&e), ErrorClassification::Transient);
    }

    #[test]
    fn classify_vendor_500_transient() {
        let c = ErrorClassifier::new();
        let e = VendorApiError::Claude(VendorErrorDetail::new(500, "internal"));
        assert_eq!(c.classify_vendor(&e), ErrorClassification::Transient);
    }

    #[test]
    fn classify_vendor_401_permanent() {
        let c = ErrorClassifier::new();
        let e = VendorApiError::Claude(VendorErrorDetail::new(401, "bad key"));
        assert_eq!(c.classify_vendor(&e), ErrorClassification::Permanent);
    }

    #[test]
    fn classify_vendor_404_permanent() {
        let c = ErrorClassifier::new();
        let e = VendorApiError::Copilot(VendorErrorDetail::new(404, "not found"));
        assert_eq!(c.classify_vendor(&e), ErrorClassification::Permanent);
    }

    #[test]
    fn classify_vendor_503_transient() {
        let c = ErrorClassifier::new();
        let e = VendorApiError::Gemini(VendorErrorDetail::new(503, "overloaded"));
        assert_eq!(c.classify_vendor(&e), ErrorClassification::Transient);
    }

    #[test]
    fn error_classification_display() {
        assert_eq!(ErrorClassification::Transient.to_string(), "transient");
        assert_eq!(ErrorClassification::Permanent.to_string(), "permanent");
        assert_eq!(ErrorClassification::Degraded.to_string(), "degraded");
    }

    #[test]
    fn error_classification_serde_roundtrip() {
        for cls in [
            ErrorClassification::Transient,
            ErrorClassification::Permanent,
            ErrorClassification::Degraded,
        ] {
            let json = serde_json::to_string(&cls).unwrap();
            let back: ErrorClassification = serde_json::from_str(&json).unwrap();
            assert_eq!(cls, back);
        }
    }

    // == RetryPolicy ======================================================

    #[test]
    fn retry_policy_default() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_attempts, 3);
        assert_eq!(p.initial_delay, Duration::from_secs(1));
        assert_eq!(p.max_delay, Duration::from_secs(60));
        assert_eq!(p.backoff_multiplier, 2);
        assert!(p.jitter);
    }

    #[test]
    fn retry_policy_builder() {
        let p = RetryPolicy::new(5)
            .with_initial_delay(Duration::from_millis(500))
            .with_max_delay(Duration::from_secs(30))
            .with_backoff_multiplier(3)
            .with_jitter(false);
        assert_eq!(p.max_attempts, 5);
        assert_eq!(p.initial_delay, Duration::from_millis(500));
        assert_eq!(p.max_delay, Duration::from_secs(30));
        assert_eq!(p.backoff_multiplier, 3);
        assert!(!p.jitter);
    }

    #[test]
    fn retry_should_retry_within_range() {
        let p = RetryPolicy::new(3);
        assert!(p.should_retry(0));
        assert!(p.should_retry(1));
        assert!(p.should_retry(2));
        assert!(!p.should_retry(3));
        assert!(!p.should_retry(100));
    }

    #[test]
    fn retry_exponential_backoff_no_jitter() {
        let p = RetryPolicy::new(4)
            .with_initial_delay(Duration::from_secs(1))
            .with_backoff_multiplier(2)
            .with_max_delay(Duration::from_secs(60))
            .with_jitter(false);
        // attempt 0: 1s * 2^0 = 1s
        assert_eq!(p.delay_for_attempt(0, None), Duration::from_secs(1));
        // attempt 1: 1s * 2^1 = 2s
        assert_eq!(p.delay_for_attempt(1, None), Duration::from_secs(2));
        // attempt 2: 1s * 2^2 = 4s
        assert_eq!(p.delay_for_attempt(2, None), Duration::from_secs(4));
        // attempt 3: 1s * 2^3 = 8s
        assert_eq!(p.delay_for_attempt(3, None), Duration::from_secs(8));
    }

    #[test]
    fn retry_backoff_capped_at_max_delay() {
        let p = RetryPolicy::new(10)
            .with_initial_delay(Duration::from_secs(10))
            .with_backoff_multiplier(3)
            .with_max_delay(Duration::from_secs(30))
            .with_jitter(false);
        // attempt 0: 10s * 3^0 = 10s
        assert_eq!(p.delay_for_attempt(0, None), Duration::from_secs(10));
        // attempt 1: 10s * 3^1 = 30s (at cap)
        assert_eq!(p.delay_for_attempt(1, None), Duration::from_secs(30));
        // attempt 2: 10s * 3^2 = 90s → capped to 30s
        assert_eq!(p.delay_for_attempt(2, None), Duration::from_secs(30));
    }

    #[test]
    fn retry_backoff_with_jitter_seed_zero() {
        let p = RetryPolicy::new(3)
            .with_initial_delay(Duration::from_secs(1))
            .with_backoff_multiplier(2);
        // jitter seed 0 => 0% extra
        let d = p.delay_for_attempt(0, Some(0));
        assert_eq!(d, Duration::from_secs(1));
    }

    #[test]
    fn retry_backoff_with_jitter_seed_100() {
        let p = RetryPolicy::new(3)
            .with_initial_delay(Duration::from_secs(1))
            .with_backoff_multiplier(2);
        // jitter seed 100 => +50% of 1000ms = 500ms => 1500ms
        let d = p.delay_for_attempt(0, Some(100));
        assert_eq!(d, Duration::from_millis(1500));
    }

    #[test]
    fn retry_backoff_with_jitter_seed_50() {
        let p = RetryPolicy::new(3)
            .with_initial_delay(Duration::from_secs(1))
            .with_backoff_multiplier(2);
        // jitter seed 50 => +25% of 1000ms = 250ms => 1250ms
        let d = p.delay_for_attempt(0, Some(50));
        assert_eq!(d, Duration::from_millis(1250));
    }

    #[test]
    fn retry_delay_for_attempt_beyond_max_is_zero() {
        let p = RetryPolicy::new(2);
        assert_eq!(p.delay_for_attempt(2, None), Duration::ZERO);
        assert_eq!(p.delay_for_attempt(5, None), Duration::ZERO);
    }

    #[test]
    fn retry_policy_serde_roundtrip() {
        let p = RetryPolicy::new(5)
            .with_initial_delay(Duration::from_millis(200))
            .with_max_delay(Duration::from_secs(10))
            .with_backoff_multiplier(3)
            .with_jitter(false);
        let json = serde_json::to_string(&p).unwrap();
        let back: RetryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    // == FallbackChain ====================================================

    #[test]
    fn fallback_chain_empty() {
        let chain = FallbackChain::new(vec![]);
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
        assert!(chain.next_backend(0).is_none());
    }

    #[test]
    fn fallback_chain_basics() {
        let chain = FallbackChain::new(vec!["openai".into(), "claude".into(), "gemini".into()]);
        assert!(!chain.is_empty());
        assert_eq!(chain.len(), 3);
        assert_eq!(chain.next_backend(0), Some("openai"));
        assert_eq!(chain.next_backend(1), Some("claude"));
        assert_eq!(chain.next_backend(2), Some("gemini"));
        assert_eq!(chain.next_backend(3), None);
    }

    #[test]
    fn fallback_chain_iter() {
        let chain = FallbackChain::new(vec!["a".into(), "b".into()]);
        let backends: Vec<&str> = chain.iter().collect();
        assert_eq!(backends, vec!["a", "b"]);
    }

    #[test]
    fn fallback_chain_execute_first_succeeds() {
        let chain = FallbackChain::new(vec!["openai".into(), "claude".into()]);
        let result = chain.execute(|backend| {
            if backend == "openai" {
                Ok("done")
            } else {
                Err("fail")
            }
        });
        assert_eq!(result, Ok((0, "done")));
    }

    #[test]
    fn fallback_chain_execute_second_succeeds() {
        let chain = FallbackChain::new(vec!["openai".into(), "claude".into()]);
        let result = chain.execute(|backend| {
            if backend == "claude" {
                Ok("ok")
            } else {
                Err("fail")
            }
        });
        assert_eq!(result, Ok((1, "ok")));
    }

    #[test]
    fn fallback_chain_execute_all_fail() {
        let chain = FallbackChain::new(vec!["openai".into(), "claude".into()]);
        let result: Result<(usize, &str), Vec<(String, &str)>> = chain.execute(|_| Err("fail"));
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].0, "openai");
        assert_eq!(errors[1].0, "claude");
    }

    #[test]
    fn fallback_chain_execute_empty() {
        let chain = FallbackChain::new(vec![]);
        let result: Result<(usize, &str), Vec<(String, &str)>> = chain.execute(|_| Ok("never"));
        let errors = result.unwrap_err();
        assert!(errors.is_empty());
    }

    #[test]
    fn fallback_chain_serde_roundtrip() {
        let chain = FallbackChain::new(vec!["openai".into(), "claude".into()]);
        let json = serde_json::to_string(&chain).unwrap();
        let back: FallbackChain = serde_json::from_str(&json).unwrap();
        assert_eq!(chain, back);
    }

    // == CircuitBreakerPolicy =============================================

    #[test]
    fn circuit_breaker_starts_closed() {
        let cb = CircuitBreakerPolicy::new(3, 2, Duration::from_secs(30));
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
        assert!(cb.is_call_permitted());
    }

    #[test]
    fn circuit_breaker_opens_after_threshold() {
        let cb = CircuitBreakerPolicy::new(3, 2, Duration::from_secs(30));
        cb.record_failure();
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitBreakerState::Open);
        assert!(!cb.is_call_permitted());
    }

    #[test]
    fn circuit_breaker_success_resets_failure_count() {
        let cb = CircuitBreakerPolicy::new(3, 2, Duration::from_secs(30));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.failure_count(), 2);
        cb.record_success();
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
    }

    #[test]
    fn circuit_breaker_transition_half_open() {
        let cb = CircuitBreakerPolicy::new(2, 2, Duration::from_secs(30));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitBreakerState::Open);
        cb.transition_to_half_open();
        assert_eq!(cb.state(), CircuitBreakerState::HalfOpen);
        assert!(cb.is_call_permitted());
    }

    #[test]
    fn circuit_breaker_half_open_to_closed() {
        let cb = CircuitBreakerPolicy::new(2, 2, Duration::from_secs(30));
        cb.record_failure();
        cb.record_failure();
        cb.transition_to_half_open();
        cb.record_success();
        assert_eq!(cb.state(), CircuitBreakerState::HalfOpen);
        cb.record_success();
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
    }

    #[test]
    fn circuit_breaker_half_open_failure_reopens() {
        let cb = CircuitBreakerPolicy::new(2, 2, Duration::from_secs(30));
        cb.record_failure();
        cb.record_failure();
        cb.transition_to_half_open();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitBreakerState::Open);
    }

    #[test]
    fn circuit_breaker_reset() {
        let cb = CircuitBreakerPolicy::new(2, 2, Duration::from_secs(30));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitBreakerState::Open);
        cb.reset();
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.success_count(), 0);
    }

    #[test]
    fn circuit_breaker_open_does_not_transition_to_half_open_from_closed() {
        let cb = CircuitBreakerPolicy::new(2, 2, Duration::from_secs(30));
        // Should be no-op when already closed
        cb.transition_to_half_open();
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
    }

    #[test]
    fn circuit_breaker_state_display() {
        assert_eq!(CircuitBreakerState::Closed.to_string(), "closed");
        assert_eq!(CircuitBreakerState::Open.to_string(), "open");
        assert_eq!(CircuitBreakerState::HalfOpen.to_string(), "half_open");
    }

    #[test]
    fn circuit_breaker_state_serde_roundtrip() {
        for state in [
            CircuitBreakerState::Closed,
            CircuitBreakerState::Open,
            CircuitBreakerState::HalfOpen,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let back: CircuitBreakerState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, back);
        }
    }

    #[test]
    fn circuit_breaker_failure_count_tracks() {
        let cb = CircuitBreakerPolicy::new(5, 2, Duration::from_secs(30));
        assert_eq!(cb.failure_count(), 0);
        cb.record_failure();
        assert_eq!(cb.failure_count(), 1);
        cb.record_failure();
        assert_eq!(cb.failure_count(), 2);
    }

    #[test]
    fn circuit_breaker_success_count_tracks() {
        let cb = CircuitBreakerPolicy::new(5, 3, Duration::from_secs(30));
        assert_eq!(cb.success_count(), 0);
        cb.record_success();
        assert_eq!(cb.success_count(), 1);
        cb.record_success();
        assert_eq!(cb.success_count(), 2);
    }

    // == RecoveryReport ===================================================

    #[test]
    fn recovery_report_new() {
        let r = RecoveryReport::new(ErrorClassification::Transient);
        assert_eq!(r.classification, ErrorClassification::Transient);
        assert!(r.attempts.is_empty());
        assert_eq!(r.final_outcome, RecoveryOutcome::Failed);
        assert!(r.successful_backend.is_none());
    }

    #[test]
    fn recovery_report_record_success() {
        let mut r = RecoveryReport::new(ErrorClassification::Transient);
        r.record(RecoveryAttempt {
            strategy: RecoveryStrategy::Retry {
                delay_ms: 1000,
                max_retries: 3,
            },
            attempt_number: 0,
            backend: Some("openai".into()),
            outcome: RecoveryOutcome::Success,
            error_message: None,
        });
        assert!(r.succeeded());
        assert_eq!(r.successful_backend.as_deref(), Some("openai"));
        assert_eq!(r.total_attempts(), 1);
    }

    #[test]
    fn recovery_report_record_failures_then_success() {
        let mut r = RecoveryReport::new(ErrorClassification::Transient);
        r.record(RecoveryAttempt {
            strategy: RecoveryStrategy::Retry {
                delay_ms: 1000,
                max_retries: 3,
            },
            attempt_number: 0,
            backend: None,
            outcome: RecoveryOutcome::Failed,
            error_message: Some("timeout".into()),
        });
        r.record(RecoveryAttempt {
            strategy: RecoveryStrategy::Retry {
                delay_ms: 2000,
                max_retries: 3,
            },
            attempt_number: 1,
            backend: None,
            outcome: RecoveryOutcome::Success,
            error_message: None,
        });
        assert!(r.succeeded());
        assert_eq!(r.total_attempts(), 2);
    }

    #[test]
    fn recovery_report_exhausted() {
        let mut r = RecoveryReport::new(ErrorClassification::Transient);
        r.record(RecoveryAttempt {
            strategy: RecoveryStrategy::Retry {
                delay_ms: 1000,
                max_retries: 1,
            },
            attempt_number: 0,
            backend: None,
            outcome: RecoveryOutcome::Failed,
            error_message: Some("fail".into()),
        });
        r.mark_exhausted();
        assert!(!r.succeeded());
        assert_eq!(r.final_outcome, RecoveryOutcome::Exhausted);
    }

    #[test]
    fn recovery_report_rejected() {
        let mut r = RecoveryReport::new(ErrorClassification::Permanent);
        r.mark_rejected();
        assert!(!r.succeeded());
        assert_eq!(r.final_outcome, RecoveryOutcome::Rejected);
    }

    #[test]
    fn recovery_report_serde_roundtrip() {
        let mut r = RecoveryReport::new(ErrorClassification::Transient);
        r.record(RecoveryAttempt {
            strategy: RecoveryStrategy::Retry {
                delay_ms: 1000,
                max_retries: 3,
            },
            attempt_number: 0,
            backend: None,
            outcome: RecoveryOutcome::Failed,
            error_message: Some("err".into()),
        });
        r.mark_exhausted();
        let json = serde_json::to_string(&r).unwrap();
        let back: RecoveryReport = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn recovery_outcome_display() {
        assert_eq!(RecoveryOutcome::Success.to_string(), "success");
        assert_eq!(RecoveryOutcome::Failed.to_string(), "failed");
        assert_eq!(RecoveryOutcome::Exhausted.to_string(), "exhausted");
        assert_eq!(RecoveryOutcome::Rejected.to_string(), "rejected");
    }

    // == RecoveryExecutor =================================================

    #[test]
    fn executor_plan_permanent_error_rejected() {
        let exec = RecoveryExecutor::new(RetryPolicy::new(3));
        let report = exec.plan_recovery(ErrorCode::BackendAuthFailed);
        assert_eq!(report.final_outcome, RecoveryOutcome::Rejected);
        assert_eq!(report.classification, ErrorClassification::Permanent);
    }

    #[test]
    fn executor_plan_transient_error_has_retry_attempts() {
        let exec = RecoveryExecutor::new(RetryPolicy::new(3));
        let report = exec.plan_recovery(ErrorCode::BackendTimeout);
        assert_eq!(report.classification, ErrorClassification::Transient);
        // Should have at least 3 retry attempts planned
        assert!(report.total_attempts() >= 3);
    }

    #[test]
    fn executor_plan_transient_with_fallback() {
        let exec = RecoveryExecutor::new(RetryPolicy::new(2))
            .with_fallback_chain(FallbackChain::new(vec!["openai".into(), "claude".into()]));
        let report = exec.plan_recovery(ErrorCode::BackendUnavailable);
        // 2 retries + 2 fallbacks = 4
        assert_eq!(report.total_attempts(), 4);
    }

    #[test]
    fn executor_plan_degraded_error_succeeds() {
        let exec = RecoveryExecutor::new(RetryPolicy::new(3));
        let report = exec.plan_recovery(ErrorCode::MappingLossyConversion);
        assert_eq!(report.classification, ErrorClassification::Degraded);
        assert!(report.succeeded());
    }

    #[test]
    fn executor_execute_retries_succeeds_on_second() {
        let exec = RecoveryExecutor::new(
            RetryPolicy::new(3)
                .with_initial_delay(Duration::from_millis(10))
                .with_jitter(false),
        );
        let mut call_count = 0u32;
        let (report, result) = exec.execute_with_retries(ErrorCode::BackendTimeout, |_attempt| {
            call_count += 1;
            if call_count >= 2 {
                Ok("success")
            } else {
                Err("fail".to_string())
            }
        });
        assert!(report.succeeded());
        assert_eq!(result, Some("success"));
        assert_eq!(report.total_attempts(), 2);
    }

    #[test]
    fn executor_execute_retries_all_fail() {
        let exec = RecoveryExecutor::new(
            RetryPolicy::new(2)
                .with_initial_delay(Duration::from_millis(10))
                .with_jitter(false),
        );
        let (report, result) =
            exec.execute_with_retries::<_, &str>(ErrorCode::BackendTimeout, |_| {
                Err("fail".to_string())
            });
        assert!(!report.succeeded());
        assert_eq!(report.final_outcome, RecoveryOutcome::Exhausted);
        assert!(result.is_none());
        assert_eq!(report.total_attempts(), 2);
    }

    #[test]
    fn executor_execute_permanent_no_retries() {
        let exec = RecoveryExecutor::new(RetryPolicy::new(3));
        let (report, result) = exec
            .execute_with_retries::<_, &str>(ErrorCode::BackendAuthFailed, |_| {
                Ok("should not run")
            });
        assert!(!report.succeeded());
        assert_eq!(report.final_outcome, RecoveryOutcome::Rejected);
        assert!(result.is_none());
        assert_eq!(report.total_attempts(), 0);
    }

    #[test]
    fn executor_execute_degraded_tries_once() {
        let exec = RecoveryExecutor::new(RetryPolicy::new(3));
        let (report, result) =
            exec.execute_with_retries(ErrorCode::MappingLossyConversion, |_| Ok("degraded_ok"));
        assert!(report.succeeded());
        assert_eq!(result, Some("degraded_ok"));
    }

    #[test]
    fn executor_debug_format() {
        let exec = RecoveryExecutor::new(RetryPolicy::default());
        let dbg = format!("{:?}", exec);
        assert!(dbg.contains("RecoveryExecutor"));
    }
}
