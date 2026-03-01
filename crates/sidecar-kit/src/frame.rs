// SPDX-License-Identifier: MIT OR Apache-2.0
//! Value-based JSONL frame definitions for the ABP sidecar protocol.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use super::SidecarError;

/// Value-based JSONL frame matching the ABP sidecar protocol.
///
/// All payload fields use [`serde_json::Value`] so this crate stays independent
/// of `abp-core` types. The discriminator tag is `"t"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Frame {
    /// Handshake frame sent by the sidecar on startup.
    Hello {
        /// ABP contract version.
        contract_version: String,
        /// Backend identity descriptor.
        backend: Value,
        /// Capability set advertised by the sidecar.
        capabilities: Value,
        /// Execution mode.
        #[serde(default)]
        mode: Value,
    },
    /// Instructs the sidecar to begin a run.
    Run {
        /// Unique identifier for this run.
        id: String,
        /// The work order payload.
        work_order: Value,
    },
    /// Streaming event emitted during a run.
    Event {
        /// Identifier of the run this event belongs to.
        ref_id: String,
        /// The event payload.
        event: Value,
    },
    /// Terminal frame carrying the run receipt.
    Final {
        /// Identifier of the completed run.
        ref_id: String,
        /// The receipt payload.
        receipt: Value,
    },
    /// Unrecoverable error frame from the sidecar.
    Fatal {
        /// Identifier of the run, if known.
        ref_id: Option<String>,
        /// Human-readable error description.
        error: String,
    },
    /// Request to cancel an in-progress run.
    Cancel {
        /// Identifier of the run to cancel.
        ref_id: String,
        /// Optional reason for cancellation.
        reason: Option<String>,
    },
    /// Keep-alive ping frame.
    Ping {
        /// Monotonic sequence number.
        seq: u64,
    },
    /// Keep-alive pong response.
    Pong {
        /// Echoed sequence number from the ping.
        seq: u64,
    },
}

impl Frame {
    /// Try to extract a typed event from an Event frame.
    pub fn try_event<T: DeserializeOwned>(&self) -> Result<(String, T), SidecarError> {
        match self {
            Frame::Event { ref_id, event } => {
                let typed =
                    serde_json::from_value(event.clone()).map_err(SidecarError::Deserialize)?;
                Ok((ref_id.clone(), typed))
            }
            _ => Err(SidecarError::Protocol("expected Event frame".into())),
        }
    }

    /// Try to extract a typed receipt from a Final frame.
    pub fn try_final<T: DeserializeOwned>(&self) -> Result<(String, T), SidecarError> {
        match self {
            Frame::Final { ref_id, receipt } => {
                let typed =
                    serde_json::from_value(receipt.clone()).map_err(SidecarError::Deserialize)?;
                Ok((ref_id.clone(), typed))
            }
            _ => Err(SidecarError::Protocol("expected Final frame".into())),
        }
    }
}
