//! abp-protocol
//!
//! Wire format for talking to sidecars and daemons.
//! Current transport: JSONL over stdio.

use abp_core::{
    AgentEvent, BackendIdentity, CapabilityManifest, ExecutionMode, Receipt, WorkOrder,
    CONTRACT_VERSION,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// JSONL message envelope.
///
/// The protocol is intentionally simple:
/// - sidecar announces itself via `hello`
/// - control plane sends `run`
/// - sidecar streams `event`
/// - sidecar concludes with `final` (receipt)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Envelope {
    Hello {
        contract_version: String,
        backend: BackendIdentity,
        capabilities: CapabilityManifest,
        /// Execution mode this sidecar will use. Defaults to "mapped" if absent.
        #[serde(default)]
        mode: ExecutionMode,
    },

    Run {
        id: String,
        work_order: WorkOrder,
    },

    Event {
        ref_id: String,
        event: AgentEvent,
    },

    Final {
        ref_id: String,
        receipt: Receipt,
    },

    Fatal {
        ref_id: Option<String>,
        error: String,
    },
}

impl Envelope {
    pub fn hello(backend: BackendIdentity, capabilities: CapabilityManifest) -> Self {
        Self::hello_with_mode(backend, capabilities, ExecutionMode::default())
    }

    pub fn hello_with_mode(
        backend: BackendIdentity,
        capabilities: CapabilityManifest,
        mode: ExecutionMode,
    ) -> Self {
        Self::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend,
            capabilities,
            mode,
        }
    }
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("protocol violation: {0}")]
    Violation(String),
}

pub struct JsonlCodec;

impl JsonlCodec {
    pub fn encode(msg: &Envelope) -> Result<String, ProtocolError> {
        let mut s = serde_json::to_string(msg)?;
        s.push('\n');
        Ok(s)
    }

    pub fn decode(line: &str) -> Result<Envelope, ProtocolError> {
        Ok(serde_json::from_str::<Envelope>(line)?)
    }
}

// Raw (Value-based) protocol types from sidecar-kit.
pub use sidecar_kit::Frame as RawFrame;
pub use sidecar_kit::JsonlCodec as RawCodec;
