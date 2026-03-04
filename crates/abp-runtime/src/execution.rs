// SPDX-License-Identifier: MIT OR Apache-2.0
//! Retry-and-fallback execution pipeline.
//!
//! [`ExecutionPipeline`] wraps the [`Runtime`](crate::Runtime) to add
//! automatic retry on transient errors and fallback to alternate backends
//! on permanent failures. It is a parallel execution path — existing callers
//! that use [`Runtime::run_streaming`](crate::Runtime::run_streaming) are
//! unaffected.

use crate::retry::{FallbackChain, RetryPolicy};
use crate::{Runtime, RuntimeError};
use abp_core::Receipt;
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the execution pipeline's retry and fallback behaviour.
///
/// All fields are optional so that an empty/default config disables both
/// retry and fallback, preserving the original single-attempt semantics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionConfig {
    /// Retry policy applied to each backend attempt.
    ///
    /// When `None`, a failed attempt is not retried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<RetryPolicy>,

    /// Ordered list of fallback backend names.
    ///
    /// On a permanent (non-retryable) error the pipeline advances to the next
    /// backend in this chain. When `None` or empty, no fallback occurs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_chain: Option<FallbackChain>,
}

// ---------------------------------------------------------------------------
// Pipeline events
// ---------------------------------------------------------------------------

/// Events emitted by the execution pipeline to describe retry/fallback
/// decisions. These are metadata — not agent events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PipelineEvent {
    /// A retryable error triggered another attempt on the same backend.
    Retry {
        /// Zero-based attempt number that just failed.
        attempt: u32,
        /// Backend that will be retried.
        backend: String,
        /// Backoff delay in milliseconds before the next attempt.
        delay_ms: u64,
        /// Human-readable reason for the retry.
        reason: String,
    },
    /// A permanent error caused the pipeline to switch backends.
    Fallback {
        /// Backend that failed.
        from_backend: String,
        /// Backend that will be tried next.
        to_backend: String,
        /// Human-readable reason for the fallback.
        reason: String,
    },
    /// A backend completed successfully.
    Success {
        /// Backend that produced the receipt.
        backend: String,
        /// Number of attempts (1 = first try).
        attempts: u32,
    },
}

/// Result of an [`ExecutionPipeline::execute`] call.
pub type PipelineResult = Result<PipelineOutput, RuntimeError>;

/// Successful output from the execution pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineOutput {
    /// The receipt from the backend that succeeded.
    pub receipt: Receipt,
    /// The backend that ultimately produced the receipt.
    pub backend: String,
    /// Pipeline-level events (retries, fallbacks, success).
    pub events: Vec<PipelineEvent>,
}

// ---------------------------------------------------------------------------
// ExecutionPipeline
// ---------------------------------------------------------------------------

/// Wraps a [`Runtime`] to add retry and fallback execution semantics.
///
/// Construct with an [`ExecutionConfig`] and call [`execute`](Self::execute)
/// to run a work order with resilience logic.
///
/// ```no_run
/// # use abp_runtime::execution::{ExecutionPipeline, ExecutionConfig};
/// # use abp_runtime::retry::RetryPolicy;
/// let config = ExecutionConfig {
///     retry_policy: Some(RetryPolicy::default()),
///     fallback_chain: None,
/// };
/// let pipeline = ExecutionPipeline::new(config);
/// ```
pub struct ExecutionPipeline {
    config: ExecutionConfig,
}

impl ExecutionPipeline {
    /// Create a new pipeline with the given configuration.
    #[must_use]
    pub fn new(config: ExecutionConfig) -> Self {
        Self { config }
    }

    /// Return a reference to the pipeline's configuration.
    #[must_use]
    pub fn config(&self) -> &ExecutionConfig {
        &self.config
    }

    /// Execute a work order against `primary_backend` with retry and
    /// fallback semantics.
    ///
    /// 1. Try the primary backend up to `retry_policy.max_retries + 1` times
    ///    for retryable errors.
    /// 2. On a permanent error (or retries exhausted), advance through the
    ///    fallback chain, applying the same retry policy to each backend.
    /// 3. Return the receipt from whichever backend succeeds, or the last
    ///    error if all backends are exhausted.
    pub async fn execute(
        &self,
        runtime: &Runtime,
        primary_backend: &str,
        work_order: abp_core::WorkOrder,
    ) -> PipelineResult {
        let mut pipeline_events = Vec::new();

        // Build the ordered list: primary first, then fallbacks.
        let mut backends = vec![primary_backend.to_string()];
        if let Some(ref chain) = self.config.fallback_chain {
            let mut chain_clone = chain.clone();
            chain_clone.reset();
            while let Some(name) = chain_clone.next_backend() {
                // Avoid duplicating the primary backend in the chain.
                if name != primary_backend {
                    backends.push(name.to_string());
                }
            }
        }

        let retry_policy = self
            .config
            .retry_policy
            .clone()
            .unwrap_or_else(RetryPolicy::no_retry);

        let mut last_error: Option<RuntimeError> = None;

        for (backend_idx, backend_name) in backends.iter().enumerate() {
            // Record fallback event when moving past the primary.
            if backend_idx > 0 {
                if let Some(ref err) = last_error {
                    let prev = &backends[backend_idx - 1];
                    let event = PipelineEvent::Fallback {
                        from_backend: prev.clone(),
                        to_backend: backend_name.clone(),
                        reason: err.to_string(),
                    };
                    info!(
                        target: "abp.runtime.pipeline",
                        from=%prev, to=%backend_name, "falling back"
                    );
                    pipeline_events.push(event);
                }
            }

            // Attempt this backend with retries.
            let max_attempts = retry_policy.max_retries + 1;
            for attempt in 0..max_attempts {
                debug!(
                    target: "abp.runtime.pipeline",
                    backend=%backend_name, attempt, max_attempts, "trying"
                );

                match Self::try_backend(runtime, backend_name, &work_order).await {
                    Ok(receipt) => {
                        pipeline_events.push(PipelineEvent::Success {
                            backend: backend_name.clone(),
                            attempts: attempt + 1,
                        });
                        return Ok(PipelineOutput {
                            receipt,
                            backend: backend_name.clone(),
                            events: pipeline_events,
                        });
                    }
                    Err(err) => {
                        let is_retryable = err.is_retryable();
                        let can_retry = is_retryable && retry_policy.should_retry(attempt);

                        if can_retry {
                            let delay = retry_policy.delay_for(attempt);
                            let event = PipelineEvent::Retry {
                                attempt,
                                backend: backend_name.clone(),
                                delay_ms: delay.as_millis() as u64,
                                reason: err.to_string(),
                            };
                            warn!(
                                target: "abp.runtime.pipeline",
                                backend=%backend_name, attempt,
                                delay_ms=%delay.as_millis(),
                                "retrying after transient error"
                            );
                            pipeline_events.push(event);
                            tokio::time::sleep(delay).await;
                            last_error = Some(err);
                        } else {
                            // Not retryable or retries exhausted — move to next backend.
                            last_error = Some(err);
                            break;
                        }
                    }
                }
            }
        }

        // All backends exhausted.
        Err(last_error.unwrap_or_else(|| RuntimeError::UnknownBackend {
            name: primary_backend.to_string(),
        }))
    }

    /// Run a single attempt against a backend, consuming the event stream
    /// and returning the receipt.
    async fn try_backend(
        runtime: &Runtime,
        backend_name: &str,
        work_order: &abp_core::WorkOrder,
    ) -> Result<Receipt, RuntimeError> {
        let handle = runtime
            .run_streaming(backend_name, work_order.clone())
            .await?;

        // Drain the event stream so the backend task can complete.
        let _events: Vec<_> = handle.events.collect().await;

        // Await the receipt.
        handle
            .receipt
            .await
            .map_err(|e| RuntimeError::BackendFailed(anyhow::Error::new(e)))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_execution_config() {
        let config = ExecutionConfig::default();
        assert!(config.retry_policy.is_none());
        assert!(config.fallback_chain.is_none());
    }

    #[test]
    fn execution_config_serde_roundtrip() {
        let config = ExecutionConfig {
            retry_policy: Some(RetryPolicy::default()),
            fallback_chain: Some(FallbackChain::new(vec!["alpha".into(), "beta".into()])),
        };
        let json = serde_json::to_string(&config).unwrap();
        let decoded: ExecutionConfig = serde_json::from_str(&json).unwrap();
        assert!(decoded.retry_policy.is_some());
        assert!(decoded.fallback_chain.is_some());
    }

    #[test]
    fn pipeline_event_serde_roundtrip() {
        let events = vec![
            PipelineEvent::Retry {
                attempt: 0,
                backend: "mock".into(),
                delay_ms: 100,
                reason: "timeout".into(),
            },
            PipelineEvent::Fallback {
                from_backend: "a".into(),
                to_backend: "b".into(),
                reason: "permanent".into(),
            },
            PipelineEvent::Success {
                backend: "b".into(),
                attempts: 1,
            },
        ];
        for ev in &events {
            let json = serde_json::to_string(ev).unwrap();
            let decoded: PipelineEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(&decoded, ev);
        }
    }
}
