//! Error types for sidecar-kit operations.

use thiserror::Error;

/// Errors from sidecar process I/O and protocol handling.
#[derive(Debug, Error)]
pub enum SidecarError {
    #[error("failed to spawn sidecar: {0}")]
    Spawn(#[source] std::io::Error),

    #[error("failed to read sidecar stdout: {0}")]
    Stdout(#[source] std::io::Error),

    #[error("failed to write sidecar stdin: {0}")]
    Stdin(#[source] std::io::Error),

    #[error("protocol violation: {0}")]
    Protocol(String),

    #[error("serialization error: {0}")]
    Serialize(String),

    #[error("deserialization error: {0}")]
    Deserialize(String),

    #[error("sidecar fatal error: {0}")]
    Fatal(String),

    #[error("sidecar exited unexpectedly (code={0:?})")]
    Exited(Option<i32>),

    #[error("operation timed out")]
    Timeout,
}
