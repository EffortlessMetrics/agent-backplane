// SPDX-License-Identifier: MIT OR Apache-2.0

//! Per-dialect capability descriptors for capability-aware mapping.
//!
//! `DialectCapabilities` describes the feature surface of each agent-SDK
//! dialect. Mappers use this to decide whether a feature can be mapped
//! directly, needs emulation, or must fail with a clear error.

use abp_dialect::Dialect;

/// Feature support level for a dialect capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Fully supported with native semantics.
    Native,
    /// Not supported — requires emulation or produces an error.
    None,
}

impl Support {
    /// Returns `true` if the feature is natively supported.
    #[must_use]
    pub fn is_native(self) -> bool {
        self == Self::Native
    }
}

/// Describes the feature surface of a single dialect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectCapabilities {
    /// The dialect these capabilities describe.
    pub dialect: Dialect,
    /// System prompt / system instruction support.
    pub system_prompt: Support,
    /// Extended-thinking / chain-of-thought blocks.
    pub thinking: Support,
    /// Image content blocks in user messages.
    pub images: Support,
    /// Image content blocks in system messages specifically.
    pub system_images: Support,
    /// Tool-use / function-calling support.
    pub tool_use: Support,
    /// Dedicated tool-result role (vs user-role with ToolResult blocks).
    pub tool_role: Support,
    /// Streaming support.
    pub streaming: Support,
}

/// Returns the capabilities for a given dialect.
#[must_use]
pub fn dialect_capabilities(dialect: Dialect) -> DialectCapabilities {
    match dialect {
        Dialect::OpenAi => DialectCapabilities {
            dialect,
            system_prompt: Support::Native,
            thinking: Support::None,
            images: Support::Native,
            system_images: Support::None,
            tool_use: Support::Native,
            tool_role: Support::Native,
            streaming: Support::Native,
        },
        Dialect::Claude => DialectCapabilities {
            dialect,
            system_prompt: Support::Native,
            thinking: Support::Native,
            images: Support::Native,
            system_images: Support::None,
            tool_use: Support::Native,
            tool_role: Support::None, // Uses User role + ToolResult blocks
            streaming: Support::Native,
        },
        Dialect::Gemini => DialectCapabilities {
            dialect,
            system_prompt: Support::Native,
            thinking: Support::None,
            images: Support::Native,
            system_images: Support::None, // system_instruction is text-only
            tool_use: Support::Native,
            tool_role: Support::None, // functionResponse in user turns
            streaming: Support::Native,
        },
        Dialect::Codex => DialectCapabilities {
            dialect,
            system_prompt: Support::None,
            thinking: Support::None,
            images: Support::None,
            system_images: Support::None,
            tool_use: Support::None,
            tool_role: Support::None,
            streaming: Support::None,
        },
        Dialect::Kimi => DialectCapabilities {
            dialect,
            system_prompt: Support::Native,
            thinking: Support::None,
            images: Support::None,
            system_images: Support::None,
            tool_use: Support::Native,
            tool_role: Support::Native, // OpenAI-compatible
            streaming: Support::Native,
        },
        Dialect::Copilot => DialectCapabilities {
            dialect,
            system_prompt: Support::Native,
            thinking: Support::None,
            images: Support::None,
            system_images: Support::None,
            tool_use: Support::Native,
            tool_role: Support::Native, // OpenAI-compatible
            streaming: Support::Native,
        },
    }
}

/// Check whether a specific content feature used in the source conversation
/// is supported by the target dialect. Returns a human-readable reason
/// if the feature is not supported.
#[must_use]
pub fn check_feature_support(feature: &str, target: &DialectCapabilities) -> Option<&'static str> {
    match feature {
        "thinking" if !target.thinking.is_native() => {
            Some("target dialect does not support thinking blocks")
        }
        "images" if !target.images.is_native() => {
            Some("target dialect does not support image content")
        }
        "system_images" if !target.system_images.is_native() => {
            Some("target dialect does not support images in system prompts")
        }
        "tool_use" if !target.tool_use.is_native() => {
            Some("target dialect does not support tool use / function calling")
        }
        "system_prompt" if !target.system_prompt.is_native() => {
            Some("target dialect does not support system prompts")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_capabilities() {
        let caps = dialect_capabilities(Dialect::OpenAi);
        assert!(caps.system_prompt.is_native());
        assert!(!caps.thinking.is_native());
        assert!(caps.images.is_native());
        assert!(caps.tool_use.is_native());
        assert!(caps.tool_role.is_native());
    }

    #[test]
    fn claude_capabilities() {
        let caps = dialect_capabilities(Dialect::Claude);
        assert!(caps.thinking.is_native());
        assert!(!caps.tool_role.is_native());
        assert!(caps.images.is_native());
    }

    #[test]
    fn codex_capabilities() {
        let caps = dialect_capabilities(Dialect::Codex);
        assert!(!caps.system_prompt.is_native());
        assert!(!caps.tool_use.is_native());
        assert!(!caps.images.is_native());
        assert!(!caps.thinking.is_native());
    }

    #[test]
    fn gemini_capabilities() {
        let caps = dialect_capabilities(Dialect::Gemini);
        assert!(caps.system_prompt.is_native());
        assert!(!caps.thinking.is_native());
        assert!(!caps.system_images.is_native());
        assert!(caps.tool_use.is_native());
    }

    #[test]
    fn kimi_capabilities() {
        let caps = dialect_capabilities(Dialect::Kimi);
        assert!(caps.system_prompt.is_native());
        assert!(caps.tool_use.is_native());
        assert!(!caps.images.is_native());
    }

    #[test]
    fn copilot_capabilities() {
        let caps = dialect_capabilities(Dialect::Copilot);
        assert!(caps.system_prompt.is_native());
        assert!(caps.tool_use.is_native());
        assert!(!caps.thinking.is_native());
    }

    #[test]
    fn all_dialects_have_capabilities() {
        for &d in Dialect::all() {
            let caps = dialect_capabilities(d);
            assert_eq!(caps.dialect, d);
        }
    }

    #[test]
    fn check_thinking_unsupported() {
        let caps = dialect_capabilities(Dialect::OpenAi);
        assert!(check_feature_support("thinking", &caps).is_some());
    }

    #[test]
    fn check_thinking_supported() {
        let caps = dialect_capabilities(Dialect::Claude);
        assert!(check_feature_support("thinking", &caps).is_none());
    }

    #[test]
    fn check_images_on_codex() {
        let caps = dialect_capabilities(Dialect::Codex);
        assert!(check_feature_support("images", &caps).is_some());
    }

    #[test]
    fn check_tool_use_on_codex() {
        let caps = dialect_capabilities(Dialect::Codex);
        assert!(check_feature_support("tool_use", &caps).is_some());
    }

    #[test]
    fn check_unknown_feature_returns_none() {
        let caps = dialect_capabilities(Dialect::OpenAi);
        assert!(check_feature_support("unknown_feature", &caps).is_none());
    }
}
