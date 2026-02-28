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
    Hello {
        contract_version: String,
        backend: Value,
        capabilities: Value,
        #[serde(default)]
        mode: Value,
    },
    Run {
        id: String,
        work_order: Value,
    },
    Event {
        ref_id: String,
        event: Value,
    },
    Final {
        ref_id: String,
        receipt: Value,
    },
    Fatal {
        ref_id: Option<String>,
        error: String,
    },
    Cancel {
        ref_id: String,
        reason: Option<String>,
    },
    Ping {
        seq: u64,
    },
    Pong {
        seq: u64,
    },
}

impl Frame {
    /// Try to extract a typed event from an Event frame.
    pub fn try_event<T: DeserializeOwned>(&self) -> Result<(String, T), SidecarError> {
        match self {
            Frame::Event { ref_id, event } => {
                let typed = serde_json::from_value(event.clone())
                    .map_err(|e| SidecarError::Deserialize(e.to_string()))?;
                Ok((ref_id.clone(), typed))
            }
            _ => Err(SidecarError::Protocol("expected Event frame".into())),
        }
    }

    /// Try to extract a typed receipt from a Final frame.
    pub fn try_final<T: DeserializeOwned>(&self) -> Result<(String, T), SidecarError> {
        match self {
            Frame::Final { ref_id, receipt } => {
                let typed = serde_json::from_value(receipt.clone())
                    .map_err(|e| SidecarError::Deserialize(e.to_string()))?;
                Ok((ref_id.clone(), typed))
            }
            _ => Err(SidecarError::Protocol("expected Final frame".into())),
        }
    }
}
