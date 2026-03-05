// SPDX-License-Identifier: MIT OR Apache-2.0
//! Extended thinking types and helpers.
//!
//! Supplements the core [`ThinkingConfig`](crate::claude_types::ThinkingConfig)
//! and [`ContentBlock::Thinking`](crate::claude_types::ContentBlock) with
//! builder helpers, streaming delta types, and validation.

use serde::{Deserialize, Serialize};

use crate::claude_types::{ContentBlock, StreamDelta, ThinkingConfig};

// ── Thinking block wrappers ─────────────────────────────────────────────

/// A standalone thinking block extracted from a response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingBlock {
    /// The thinking / chain-of-thought text.
    pub thinking: String,
    /// Optional cryptographic signature for verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl ThinkingBlock {
    /// Create a thinking block without a signature.
    #[must_use]
    pub fn new(thinking: impl Into<String>) -> Self {
        Self {
            thinking: thinking.into(),
            signature: None,
        }
    }

    /// Create a thinking block with a signature.
    #[must_use]
    pub fn with_signature(thinking: impl Into<String>, signature: impl Into<String>) -> Self {
        Self {
            thinking: thinking.into(),
            signature: Some(signature.into()),
        }
    }

    /// Convert to a [`ContentBlock::Thinking`].
    #[must_use]
    pub fn to_content_block(&self) -> ContentBlock {
        ContentBlock::Thinking {
            thinking: self.thinking.clone(),
            signature: self.signature.clone(),
        }
    }

    /// Try to extract a [`ThinkingBlock`] from a [`ContentBlock`].
    #[must_use]
    pub fn from_content_block(block: &ContentBlock) -> Option<Self> {
        match block {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => Some(Self {
                thinking: thinking.clone(),
                signature: signature.clone(),
            }),
            _ => None,
        }
    }

    /// Returns `true` if a cryptographic signature is present.
    #[must_use]
    pub fn has_signature(&self) -> bool {
        self.signature.is_some()
    }
}

// ── Signature block ─────────────────────────────────────────────────────

/// A standalone signature block, separate from thinking text.
///
/// This is useful when accumulating streaming deltas where signature
/// fragments arrive after the thinking text is complete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureBlock {
    /// The accumulated signature string.
    pub signature: String,
}

// ── Streaming delta wrappers ────────────────────────────────────────────

/// A thinking-specific streaming delta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingDelta {
    /// Incremental thinking text.
    pub thinking: String,
}

impl ThinkingDelta {
    /// Convert to a [`StreamDelta::ThinkingDelta`].
    #[must_use]
    pub fn to_stream_delta(&self) -> StreamDelta {
        StreamDelta::ThinkingDelta {
            thinking: self.thinking.clone(),
        }
    }
}

/// A signature-specific streaming delta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureDelta {
    /// Incremental signature data.
    pub signature: String,
}

impl SignatureDelta {
    /// Convert to a [`StreamDelta::SignatureDelta`].
    #[must_use]
    pub fn to_stream_delta(&self) -> StreamDelta {
        StreamDelta::SignatureDelta {
            signature: self.signature.clone(),
        }
    }
}

// ── Config helpers ──────────────────────────────────────────────────────

/// Create a [`ThinkingConfig`] that enables extended thinking.
#[must_use]
pub fn thinking_enabled(budget_tokens: u32) -> ThinkingConfig {
    ThinkingConfig {
        thinking_type: "enabled".into(),
        budget_tokens,
    }
}

/// Create a [`ThinkingConfig`] that disables extended thinking.
#[must_use]
pub fn thinking_disabled() -> ThinkingConfig {
    ThinkingConfig {
        thinking_type: "disabled".into(),
        budget_tokens: 0,
    }
}

/// Validate a [`ThinkingConfig`].
///
/// Returns `Err` if the config is enabled but has a zero or unreasonably
/// low budget.
pub fn validate_thinking_config(config: &ThinkingConfig) -> Result<(), String> {
    if config.thinking_type == "enabled" && config.budget_tokens == 0 {
        return Err("thinking is enabled but budget_tokens is 0".into());
    }
    Ok(())
}

/// Check whether a thinking config represents the "enabled" state.
#[must_use]
pub fn is_thinking_enabled(config: &ThinkingConfig) -> bool {
    config.thinking_type == "enabled" && config.budget_tokens > 0
}

/// Extract all thinking blocks from a list of content blocks.
#[must_use]
pub fn extract_thinking(blocks: &[ContentBlock]) -> Vec<ThinkingBlock> {
    blocks
        .iter()
        .filter_map(ThinkingBlock::from_content_block)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn thinking_block_new() {
        let tb = ThinkingBlock::new("Let me think...");
        assert_eq!(tb.thinking, "Let me think...");
        assert!(!tb.has_signature());
    }

    #[test]
    fn thinking_block_with_signature() {
        let tb = ThinkingBlock::with_signature("analysis", "sig_abc");
        assert!(tb.has_signature());
        assert_eq!(tb.signature, Some("sig_abc".into()));
    }

    #[test]
    fn thinking_block_roundtrip() {
        let tb = ThinkingBlock::with_signature("deep thought", "sig_xyz");
        let json = serde_json::to_string(&tb).unwrap();
        let rt: ThinkingBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, tb);
    }

    #[test]
    fn thinking_block_no_sig_omitted() {
        let tb = ThinkingBlock::new("hmm");
        let v = serde_json::to_value(&tb).unwrap();
        assert!(v.get("signature").is_none());
    }

    #[test]
    fn thinking_block_to_content_block() {
        let tb = ThinkingBlock::with_signature("think", "sig");
        let cb = tb.to_content_block();
        assert_eq!(
            cb,
            ContentBlock::Thinking {
                thinking: "think".into(),
                signature: Some("sig".into()),
            }
        );
    }

    #[test]
    fn thinking_block_from_content_block() {
        let cb = ContentBlock::Thinking {
            thinking: "analysis".into(),
            signature: Some("abc".into()),
        };
        let tb = ThinkingBlock::from_content_block(&cb).unwrap();
        assert_eq!(tb.thinking, "analysis");
        assert_eq!(tb.signature, Some("abc".into()));
    }

    #[test]
    fn thinking_block_from_non_thinking_returns_none() {
        let cb = ContentBlock::Text {
            text: "hello".into(),
        };
        assert!(ThinkingBlock::from_content_block(&cb).is_none());
    }

    #[test]
    fn signature_block_roundtrip() {
        let sb = SignatureBlock {
            signature: "sig123".into(),
        };
        let json = serde_json::to_string(&sb).unwrap();
        let rt: SignatureBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, sb);
    }

    #[test]
    fn thinking_delta_roundtrip() {
        let td = ThinkingDelta {
            thinking: "step 1".into(),
        };
        let json = serde_json::to_string(&td).unwrap();
        let rt: ThinkingDelta = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, td);
    }

    #[test]
    fn thinking_delta_to_stream_delta() {
        let td = ThinkingDelta {
            thinking: "chunk".into(),
        };
        let sd = td.to_stream_delta();
        assert_eq!(
            sd,
            StreamDelta::ThinkingDelta {
                thinking: "chunk".into()
            }
        );
    }

    #[test]
    fn signature_delta_to_stream_delta() {
        let sd = SignatureDelta {
            signature: "part".into(),
        };
        let delta = sd.to_stream_delta();
        assert_eq!(
            delta,
            StreamDelta::SignatureDelta {
                signature: "part".into()
            }
        );
    }

    #[test]
    fn thinking_enabled_config() {
        let cfg = thinking_enabled(10000);
        assert_eq!(cfg.thinking_type, "enabled");
        assert_eq!(cfg.budget_tokens, 10000);
    }

    #[test]
    fn thinking_disabled_config() {
        let cfg = thinking_disabled();
        assert_eq!(cfg.thinking_type, "disabled");
        assert_eq!(cfg.budget_tokens, 0);
    }

    #[test]
    fn validate_thinking_config_ok() {
        let cfg = thinking_enabled(5000);
        assert!(validate_thinking_config(&cfg).is_ok());
    }

    #[test]
    fn validate_thinking_config_enabled_zero_budget() {
        let cfg = ThinkingConfig {
            thinking_type: "enabled".into(),
            budget_tokens: 0,
        };
        assert!(validate_thinking_config(&cfg).is_err());
    }

    #[test]
    fn validate_thinking_config_disabled_zero_ok() {
        let cfg = thinking_disabled();
        assert!(validate_thinking_config(&cfg).is_ok());
    }

    #[test]
    fn is_thinking_enabled_true() {
        assert!(is_thinking_enabled(&thinking_enabled(8000)));
    }

    #[test]
    fn is_thinking_enabled_false_disabled() {
        assert!(!is_thinking_enabled(&thinking_disabled()));
    }

    #[test]
    fn is_thinking_enabled_false_zero_budget() {
        let cfg = ThinkingConfig {
            thinking_type: "enabled".into(),
            budget_tokens: 0,
        };
        assert!(!is_thinking_enabled(&cfg));
    }

    #[test]
    fn extract_thinking_blocks() {
        let blocks = vec![
            ContentBlock::Text {
                text: "before".into(),
            },
            ContentBlock::Thinking {
                thinking: "step 1".into(),
                signature: Some("sig1".into()),
            },
            ContentBlock::Text {
                text: "between".into(),
            },
            ContentBlock::Thinking {
                thinking: "step 2".into(),
                signature: None,
            },
        ];
        let thinking = extract_thinking(&blocks);
        assert_eq!(thinking.len(), 2);
        assert_eq!(thinking[0].thinking, "step 1");
        assert!(thinking[0].has_signature());
        assert_eq!(thinking[1].thinking, "step 2");
        assert!(!thinking[1].has_signature());
    }

    #[test]
    fn extract_thinking_empty() {
        let blocks = vec![ContentBlock::Text {
            text: "no thinking".into(),
        }];
        assert!(extract_thinking(&blocks).is_empty());
    }
}
