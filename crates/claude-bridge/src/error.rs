// SPDX-License-Identifier: MIT OR Apache-2.0
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("node.js not found: {0}")]
    NodeNotFound(String),

    #[error("host script not found: {0}")]
    HostScriptNotFound(String),

    #[error("sidecar error: {0}")]
    Sidecar(#[from] sidecar_kit::SidecarError),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("run error: {0}")]
    Run(String),
}
