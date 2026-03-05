//! Enhanced recovery strategies — multi-step recovery plans with backoff.
//!
//! While [`classification::RecoverySuggestion`](crate::classification::RecoverySuggestion)
//! describes a single recovery action, this module provides [`RecoveryPlan`]
//! which chains multiple steps (retry → fallback → contact admin) and
//! [`RetryPolicy`] which encodes exponential-backoff parameters.
//!
//! # Examples
//!
//! ```
//! use abp_error_taxonomy::recovery::{RecoveryPlan, RetryPolicy};
//! use abp_error_taxonomy::classification::{ErrorClassifier, RecoveryAction};
//! use abp_error_taxonomy::ErrorCode;
//!
//! let classifier = ErrorClassifier::new();
//! let classification = classifier.classify(&ErrorCode::BackendRateLimited);
//! let plan = RecoveryPlan::from_classification(&classification);
//!
//! assert!(!plan.steps.is_empty());
//! assert_eq!(plan.steps[0].action, RecoveryAction::Retry);
//! assert!(plan.steps[0].retry_policy.is_some());
//! ```

use crate::classification::{
    ClassificationCategory, ErrorClassification, ErrorSeverity, RecoveryAction,
};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// RetryPolicy
// ---------------------------------------------------------------------------

/// Exponential-backoff parameters for retry actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Initial delay before the first retry (milliseconds).
    pub initial_delay_ms: u64,
    /// Multiplicative backoff factor applied after each attempt.
    pub backoff_factor: u32,
    /// Hard ceiling on the delay between retries (milliseconds).
    pub max_delay_ms: u64,
}

impl RetryPolicy {
    /// Compute the delay for attempt `n` (0-indexed).
    ///
    /// Returns `None` if `n >= max_retries`.
    pub fn delay_for_attempt(&self, n: u32) -> Option<u64> {
        if n >= self.max_retries {
            return None;
        }
        let delay = self
            .initial_delay_ms
            .saturating_mul(self.backoff_factor.saturating_pow(n) as u64);
        Some(delay.min(self.max_delay_ms))
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1000,
            backoff_factor: 2,
            max_delay_ms: 30_000,
        }
    }
}

// ---------------------------------------------------------------------------
// RecoveryStep
// ---------------------------------------------------------------------------

/// A single step in a [`RecoveryPlan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryStep {
    /// The action to take at this step.
    pub action: RecoveryAction,
    /// Human-readable description of the step.
    pub description: String,
    /// If `action` is [`RecoveryAction::Retry`], the backoff policy to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<RetryPolicy>,
}

// ---------------------------------------------------------------------------
// RecoveryPlan
// ---------------------------------------------------------------------------

/// An ordered sequence of recovery steps for an error.
///
/// Steps are attempted in order; if the first step fails the caller should
/// proceed to the next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryPlan {
    /// Ordered recovery steps.
    pub steps: Vec<RecoveryStep>,
}

impl RecoveryPlan {
    /// Derive a recovery plan from an [`ErrorClassification`].
    pub fn from_classification(classification: &ErrorClassification) -> Self {
        Self::build_plan(classification.severity, classification.category)
    }

    /// Check whether any step in the plan involves a retry.
    pub fn has_retry(&self) -> bool {
        self.steps.iter().any(|s| s.action == RecoveryAction::Retry)
    }

    /// Check whether the plan ends with a terminal (non-recoverable) step.
    pub fn is_terminal(&self) -> bool {
        self.steps.last().is_none_or(|s| {
            matches!(
                s.action,
                RecoveryAction::ContactAdmin | RecoveryAction::None
            )
        })
    }

    // -- plan construction -------------------------------------------------

    fn build_plan(severity: ErrorSeverity, category: ClassificationCategory) -> Self {
        let steps = match (severity, category) {
            // Retriable: retry with backoff, then fall back
            (ErrorSeverity::Retriable, ClassificationCategory::RateLimit) => vec![
                RecoveryStep {
                    action: RecoveryAction::Retry,
                    description: "Retry after exponential backoff".into(),
                    retry_policy: Some(RetryPolicy {
                        max_retries: 5,
                        initial_delay_ms: 2000,
                        backoff_factor: 2,
                        max_delay_ms: 60_000,
                    }),
                },
                RecoveryStep {
                    action: RecoveryAction::Fallback,
                    description: "Switch to an alternative backend".into(),
                    retry_policy: None,
                },
            ],
            (ErrorSeverity::Retriable, ClassificationCategory::TimeoutError) => vec![
                RecoveryStep {
                    action: RecoveryAction::Retry,
                    description: "Retry with the same parameters".into(),
                    retry_policy: Some(RetryPolicy {
                        max_retries: 3,
                        initial_delay_ms: 1000,
                        backoff_factor: 2,
                        max_delay_ms: 15_000,
                    }),
                },
                RecoveryStep {
                    action: RecoveryAction::Fallback,
                    description: "Try an alternative backend".into(),
                    retry_policy: None,
                },
            ],
            (ErrorSeverity::Retriable, ClassificationCategory::ServerError) => vec![
                RecoveryStep {
                    action: RecoveryAction::Retry,
                    description: "Retry after a short delay".into(),
                    retry_policy: Some(RetryPolicy::default()),
                },
                RecoveryStep {
                    action: RecoveryAction::Fallback,
                    description: "Switch to a different backend".into(),
                    retry_policy: None,
                },
            ],
            (ErrorSeverity::Retriable, _) => vec![RecoveryStep {
                action: RecoveryAction::Retry,
                description: "Retry after a short delay".into(),
                retry_policy: Some(RetryPolicy::default()),
            }],

            // Fatal with specific recovery
            (_, ClassificationCategory::Authentication) => vec![RecoveryStep {
                action: RecoveryAction::ContactAdmin,
                description: "Verify credentials or API keys".into(),
                retry_policy: None,
            }],
            (_, ClassificationCategory::ModelNotFound) => vec![
                RecoveryStep {
                    action: RecoveryAction::ChangeModel,
                    description: "Switch to an available model".into(),
                    retry_policy: None,
                },
                RecoveryStep {
                    action: RecoveryAction::Fallback,
                    description: "Try an alternative backend".into(),
                    retry_policy: None,
                },
            ],
            (_, ClassificationCategory::ContextLength) => vec![
                RecoveryStep {
                    action: RecoveryAction::ReduceContext,
                    description: "Reduce input size or summarise".into(),
                    retry_policy: None,
                },
                RecoveryStep {
                    action: RecoveryAction::ChangeModel,
                    description: "Switch to a model with a larger context window".into(),
                    retry_policy: None,
                },
            ],
            (_, ClassificationCategory::CapabilityUnsupported) => vec![
                RecoveryStep {
                    action: RecoveryAction::Fallback,
                    description: "Try an alternative backend with the required capability".into(),
                    retry_policy: None,
                },
                RecoveryStep {
                    action: RecoveryAction::ChangeModel,
                    description: "Switch to a model that supports the capability".into(),
                    retry_policy: None,
                },
            ],
            (_, ClassificationCategory::MappingFailure) => vec![RecoveryStep {
                action: RecoveryAction::Fallback,
                description: "Try a compatible dialect or backend".into(),
                retry_policy: None,
            }],
            (_, ClassificationCategory::ContentFilter) => vec![RecoveryStep {
                action: RecoveryAction::ContactAdmin,
                description: "Review the request content against policy".into(),
                retry_policy: None,
            }],

            // Degraded / informational
            (ErrorSeverity::Degraded, _) => vec![RecoveryStep {
                action: RecoveryAction::None,
                description: "Operation completed with reduced fidelity".into(),
                retry_policy: None,
            }],
            (ErrorSeverity::Informational, _) => vec![RecoveryStep {
                action: RecoveryAction::None,
                description: "Informational — no action required".into(),
                retry_policy: None,
            }],

            // Remaining fatal
            _ => vec![RecoveryStep {
                action: RecoveryAction::ContactAdmin,
                description: "Investigate logs and contact support".into(),
                retry_policy: None,
            }],
        };
        Self { steps }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classification::ErrorClassifier;
    use crate::ErrorCode;

    #[test]
    fn rate_limited_plan_starts_with_retry() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendRateLimited);
        let plan = RecoveryPlan::from_classification(&cl);
        assert_eq!(plan.steps[0].action, RecoveryAction::Retry);
        assert!(plan.steps[0].retry_policy.is_some());
    }

    #[test]
    fn rate_limited_plan_has_fallback_step() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendRateLimited);
        let plan = RecoveryPlan::from_classification(&cl);
        assert!(plan.steps.len() >= 2);
        assert_eq!(plan.steps[1].action, RecoveryAction::Fallback);
    }

    #[test]
    fn auth_failed_plan_is_terminal() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendAuthFailed);
        let plan = RecoveryPlan::from_classification(&cl);
        assert!(plan.is_terminal());
    }

    #[test]
    fn retry_policy_delay_calculation() {
        let policy = RetryPolicy {
            max_retries: 3,
            initial_delay_ms: 1000,
            backoff_factor: 2,
            max_delay_ms: 10_000,
        };
        assert_eq!(policy.delay_for_attempt(0), Some(1000));
        assert_eq!(policy.delay_for_attempt(1), Some(2000));
        assert_eq!(policy.delay_for_attempt(2), Some(4000));
        assert_eq!(policy.delay_for_attempt(3), None); // exceeds max_retries
    }

    #[test]
    fn retry_policy_respects_max_delay() {
        let policy = RetryPolicy {
            max_retries: 10,
            initial_delay_ms: 1000,
            backoff_factor: 10,
            max_delay_ms: 5000,
        };
        assert_eq!(policy.delay_for_attempt(3), Some(5000));
    }

    #[test]
    fn has_retry_is_correct() {
        let c = ErrorClassifier::new();

        let cl = c.classify(&ErrorCode::BackendRateLimited);
        let plan = RecoveryPlan::from_classification(&cl);
        assert!(plan.has_retry());

        let cl = c.classify(&ErrorCode::BackendAuthFailed);
        let plan = RecoveryPlan::from_classification(&cl);
        assert!(!plan.has_retry());
    }

    #[test]
    fn every_code_produces_a_non_empty_plan() {
        let c = ErrorClassifier::new();
        let codes = [
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::BackendRateLimited,
            ErrorCode::BackendTimeout,
            ErrorCode::BackendAuthFailed,
            ErrorCode::BackendModelNotFound,
            ErrorCode::MappingLossyConversion,
            ErrorCode::CapabilityUnsupported,
            ErrorCode::PolicyDenied,
            ErrorCode::Internal,
        ];
        for code in &codes {
            let cl = c.classify(code);
            let plan = RecoveryPlan::from_classification(&cl);
            assert!(!plan.steps.is_empty(), "{:?} has empty plan", code);
        }
    }
}
