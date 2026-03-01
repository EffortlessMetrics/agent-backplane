// SPDX-License-Identifier: MIT OR Apache-2.0
use thiserror::Error;

/// Errors that can occur during bridge operations.
#[derive(Debug, Error)]
pub enum BridgeError {
    /// Node.js binary could not be found on the system.
    #[error("node.js not found: {0}")]
    NodeNotFound(String),

    /// The host sidecar script could not be located.
    #[error("host script not found: {0}")]
    HostScriptNotFound(String),

    /// An error propagated from the underlying sidecar transport.
    #[error("sidecar error: {0}")]
    Sidecar(#[from] sidecar_kit::SidecarError),

    /// Invalid or incomplete bridge configuration.
    #[error("configuration error: {0}")]
    Config(String),

    /// A runtime error during a bridge run.
    #[error("run error: {0}")]
    Run(String),
}
