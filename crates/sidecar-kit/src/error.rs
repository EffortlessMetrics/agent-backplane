// SPDX-License-Identifier: MIT OR Apache-2.0
//! Error types for sidecar-kit operations.

use thiserror::Error;

/// Errors from sidecar process I/O and protocol handling.
#[derive(Debug, Error)]
pub enum SidecarError {
    /// Failed to spawn the sidecar process.
    #[error("failed to spawn sidecar: {0}")]
    Spawn(#[source] std::io::Error),

    /// Failed to read from sidecar stdout.
    #[error("failed to read sidecar stdout: {0}")]
    Stdout(#[source] std::io::Error),

    /// Failed to write to sidecar stdin.
    #[error("failed to write sidecar stdin: {0}")]
    Stdin(#[source] std::io::Error),

    /// JSONL protocol violation.
    #[error("protocol violation: {0}")]
    Protocol(String),

    /// JSON serialization failure.
    #[error("serialization error: {0}")]
    Serialize(#[source] serde_json::Error),

    /// JSON deserialization failure.
    #[error("deserialization error: {0}")]
    Deserialize(#[source] serde_json::Error),

    /// The sidecar sent a fatal error frame.
    #[error("sidecar fatal error: {0}")]
    Fatal(String),

    /// The sidecar process exited unexpectedly.
    #[error("sidecar exited unexpectedly (code={0:?})")]
    Exited(Option<i32>),

    /// An operation exceeded its timeout.
    #[error("operation timed out")]
    Timeout,
}
