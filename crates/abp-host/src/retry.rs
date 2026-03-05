// SPDX-License-Identifier: MIT OR Apache-2.0
//! Retry and recovery layer for sidecar connection (spawn + hello handshake).

use crate::{HostError, SidecarClient, SidecarSpec};
use std::future::Future;

pub use abp_retry::{RetryAttempt, RetryConfig, RetryMetadata, RetryOutcome, compute_delay};

/// Returns `true` if the error is eligible for retry.
///
/// Protocol violations and unrecognised-message errors are generally
/// non-transient and should *not* be retried.
pub fn is_retryable(err: &HostError) -> bool {
    matches!(
        err,
        HostError::Spawn(_)
            | HostError::Stdout(_)
            | HostError::Exited { .. }
            | HostError::SidecarCrashed { .. }
            | HostError::Timeout { .. }
    )
}

/// Host-specialized retry loop preserving the historical `abp_host::retry` API.
pub async fn retry_async<T, F, Fut>(
    config: &RetryConfig,
    op: F,
    retryable: fn(&HostError) -> bool,
) -> Result<RetryOutcome<T>, HostError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, HostError>>,
{
    abp_retry::retry_async(config, op, retryable, |duration| HostError::Timeout {
        duration,
    })
    .await
}

/// Spawn a sidecar with automatic retry on transient failures.
///
/// Wraps [`SidecarClient::spawn`] with exponential backoff and captures
/// retry metadata that can later be embedded in a receipt.
pub async fn spawn_with_retry(
    spec: SidecarSpec,
    config: &RetryConfig,
) -> Result<RetryOutcome<SidecarClient>, HostError> {
    retry_async(config, || SidecarClient::spawn(spec.clone()), is_retryable).await
}
