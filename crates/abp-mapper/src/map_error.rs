// SPDX-License-Identifier: MIT OR Apache-2.0

//! Typed error variants for IR-level dialect mapping failures.

use abp_dialect::Dialect;
use serde::{Deserialize, Serialize};

/// Errors produced during IR-level dialect mapping.
///
/// Each variant captures structured context about *why* the mapping failed,
/// enabling callers to decide whether to retry, fall back, or abort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MapError {
    /// The (from, to) dialect pair is not supported by this mapper.
    #[error("unsupported dialect pair: {from} -> {to}")]
    UnsupportedPair {
        /// Source dialect.
        from: Dialect,
        /// Target dialect.
        to: Dialect,
    },

    /// The mapping succeeds but loses information that cannot be recovered.
    #[error("lossy conversion in field `{field}`: {reason}")]
    LossyConversion {
        /// Field or concept that suffers information loss.
        field: String,
        /// Human-readable explanation of what is lost.
        reason: String,
    },

    /// A tool cannot be mapped to the target dialect.
    #[error("unmappable tool `{name}`: {reason}")]
    UnmappableTool {
        /// Name of the tool that cannot be mapped.
        name: String,
        /// Human-readable explanation.
        reason: String,
    },

    /// A capability required by the source is incompatible with the target.
    #[error("incompatible capability `{capability}`: {reason}")]
    IncompatibleCapability {
        /// Name of the capability.
        capability: String,
        /// Human-readable explanation.
        reason: String,
    },

    /// A content block or content combination cannot be represented in the target dialect.
    #[error("unmappable content in `{field}`: {reason}")]
    UnmappableContent {
        /// Field or location of the unmappable content.
        field: String,
        /// Human-readable explanation of why the content cannot be mapped.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_pair_display() {
        let err = MapError::UnsupportedPair {
            from: Dialect::Kimi,
            to: Dialect::Copilot,
        };
        let msg = err.to_string();
        assert!(msg.contains("Kimi"));
        assert!(msg.contains("Copilot"));
    }

    #[test]
    fn lossy_conversion_display() {
        let err = MapError::LossyConversion {
            field: "thinking".into(),
            reason: "target has no thinking block".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("thinking"));
        assert!(msg.contains("lossy"));
    }

    #[test]
    fn unmappable_tool_display() {
        let err = MapError::UnmappableTool {
            name: "computer_use".into(),
            reason: "not supported in target".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("computer_use"));
    }

    #[test]
    fn incompatible_capability_display() {
        let err = MapError::IncompatibleCapability {
            capability: "logprobs".into(),
            reason: "target dialect does not support logprobs".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("logprobs"));
    }

    #[test]
    fn error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MapError>();
    }

    #[test]
    fn error_serialize_roundtrip() {
        let err = MapError::UnsupportedPair {
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn lossy_serialize_roundtrip() {
        let err = MapError::LossyConversion {
            field: "system_instruction".into(),
            reason: "flattened".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn unmappable_tool_serialize_roundtrip() {
        let err = MapError::UnmappableTool {
            name: "bash".into(),
            reason: "restricted".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn incompatible_capability_serialize_roundtrip() {
        let err = MapError::IncompatibleCapability {
            capability: "vision".into(),
            reason: "no image support".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn unmappable_content_display() {
        let err = MapError::UnmappableContent {
            field: "system".into(),
            reason: "image blocks in system prompt".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("system"));
        assert!(msg.contains("image blocks"));
    }

    #[test]
    fn unmappable_content_serialize_roundtrip() {
        let err = MapError::UnmappableContent {
            field: "system".into(),
            reason: "image blocks in system prompt".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn error_clone() {
        let err = MapError::UnsupportedPair {
            from: Dialect::Gemini,
            to: Dialect::Kimi,
        };
        let cloned = err.clone();
        assert_eq!(err, cloned);
    }
}
