// SPDX-License-Identifier: MIT OR Apache-2.0
//! JSONL codec for [`Frame`] serialization.

use super::{Frame, SidecarError};

/// Stateless JSONL codec for [`Frame`] values.
pub struct JsonlCodec;

impl JsonlCodec {
    /// Serialize a [`Frame`] to a newline-terminated JSON string.
    pub fn encode(frame: &Frame) -> Result<String, SidecarError> {
        let mut s =
            serde_json::to_string(frame).map_err(|e| SidecarError::Serialize(e.to_string()))?;
        s.push('\n');
        Ok(s)
    }

    /// Deserialize a single JSON line into a [`Frame`].
    pub fn decode(line: &str) -> Result<Frame, SidecarError> {
        serde_json::from_str(line).map_err(|e| SidecarError::Deserialize(e.to_string()))
    }
}
