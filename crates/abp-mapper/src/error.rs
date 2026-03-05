// SPDX-License-Identifier: MIT OR Apache-2.0

//! Typed error variants for dialect mapping failures.

use abp_dialect::Dialect;

/// Errors produced during dialect mapping.
///
/// Each variant captures structured context about *why* the mapping failed,
/// enabling callers to decide whether to retry, fall back, or abort.
///
/// # Examples
///
/// ```
/// use abp_mapper::MappingError;
/// use abp_dialect::Dialect;
///
/// let err = MappingError::UnsupportedCapability {
///     capability: "logprobs".into(),
///     source_dialect: Dialect::Claude,
///     target_dialect: Dialect::OpenAi,
/// };
/// assert!(err.to_string().contains("logprobs"));
/// ```
#[derive(Debug, Clone, thiserror::Error)]
pub enum MappingError {
    /// The source request uses a capability that the target dialect cannot represent.
    #[error("unsupported capability `{capability}`: {source_dialect} -> {target_dialect}")]
    UnsupportedCapability {
        /// Name of the capability that cannot be mapped.
        capability: String,
        /// Dialect the request originated from.
        source_dialect: Dialect,
        /// Dialect the request was being mapped to.
        target_dialect: Dialect,
    },

    /// Source and target use incompatible type representations for the same concept.
    #[error("incompatible types: `{source_type}` -> `{target_type}`: {reason}")]
    IncompatibleTypes {
        /// Type name in the source dialect.
        source_type: String,
        /// Type name in the target dialect.
        target_type: String,
        /// Human-readable explanation of the incompatibility.
        reason: String,
    },

    /// The mapping succeeds but loses information that cannot be recovered.
    #[error("fidelity loss in field `{field}` ({source_dialect} -> {target_dialect}): {detail}")]
    FidelityLoss {
        /// Field or concept that suffers information loss.
        field: String,
        /// Source dialect.
        source_dialect: Dialect,
        /// Target dialect.
        target_dialect: Dialect,
        /// Description of what is lost.
        detail: String,
    },

    /// The request cannot be mapped at all.
    #[error("unmappable request: {reason}")]
    UnmappableRequest {
        /// Human-readable explanation.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_capability_display() {
        let err = MappingError::UnsupportedCapability {
            capability: "logprobs".into(),
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
        };
        let msg = err.to_string();
        assert!(msg.contains("logprobs"));
        assert!(msg.contains("OpenAI"));
        assert!(msg.contains("Claude"));
    }

    #[test]
    fn incompatible_types_display() {
        let err = MappingError::IncompatibleTypes {
            source_type: "function_call".into(),
            target_type: "tool_use".into(),
            reason: "schema mismatch".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("function_call"));
        assert!(msg.contains("tool_use"));
        assert!(msg.contains("schema mismatch"));
    }

    #[test]
    fn fidelity_loss_display() {
        let err = MappingError::FidelityLoss {
            field: "thinking".into(),
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            detail: "no native thinking block".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("thinking"));
        assert!(msg.contains("fidelity loss"));
    }

    #[test]
    fn unmappable_request_display() {
        let err = MappingError::UnmappableRequest {
            reason: "empty messages array".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("empty messages array"));
    }

    #[test]
    fn error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MappingError>();
    }

    #[test]
    fn error_clone() {
        let err = MappingError::UnsupportedCapability {
            capability: "x".into(),
            source_dialect: Dialect::Gemini,
            target_dialect: Dialect::Kimi,
        };
        let cloned = err.clone();
        assert_eq!(err.to_string(), cloned.to_string());
    }
}
