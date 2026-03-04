// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]

//! Mapping rules for dialect translation.
//!
//! Provides the [`MappingRule`] trait and concrete implementations for
//! transforming IR conversations between different agent-SDK dialects.
//!
//! ## Rule types
//!
//! - `ToolMappingRule` — maps tool-use/result blocks and tool-role differences.
//! - `ContentMappingRule` — maps content types (text, image, thinking, system).
//! - `MetadataMappingRule` — strips vendor-specific metadata keys.
//! - `StreamMappingRule` — validates streaming compatibility.
//! - `RuleChain` — ordered list of rules applied in sequence.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use serde::{Deserialize, Serialize};

use crate::capabilities::{dialect_capabilities, Support};
use crate::MapError;

// ── MappingRule trait ───────────────────────────────────────────────────

/// Trait for a single mapping rule that transforms an IR conversation.
///
/// Each rule encapsulates one aspect of dialect translation (tools, content,
/// metadata, or streaming). Rules are composed via [`RuleChain`].
pub trait MappingRule: Send + Sync {
    /// Returns `true` if this rule applies to the given dialect pair.
    fn can_map(&self, from_dialect: Dialect, to_dialect: Dialect) -> bool;

    /// Apply this rule to transform the conversation.
    fn apply(
        &self,
        from_dialect: Dialect,
        to_dialect: Dialect,
        request: &IrConversation,
    ) -> Result<IrConversation, MapError>;

    /// Human-readable name for this rule.
    fn name(&self) -> &str;
}

// ── ToolMappingRule ─────────────────────────────────────────────────────

/// Maps tool definitions and tool-related content blocks between dialects.
///
/// Handles differences in tool role representation (dedicated Tool role vs.
/// tool results embedded in User role) and validates tool-use support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMappingRule {
    _priv: (),
}

impl Default for ToolMappingRule {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolMappingRule {
    /// Create a new tool mapping rule.
    #[must_use]
    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl MappingRule for ToolMappingRule {
    fn can_map(&self, from_dialect: Dialect, to_dialect: Dialect) -> bool {
        let from_caps = dialect_capabilities(from_dialect);
        let to_caps = dialect_capabilities(to_dialect);
        to_caps.tool_use.is_native() || !from_caps.tool_use.is_native()
    }

    fn apply(
        &self,
        from_dialect: Dialect,
        to_dialect: Dialect,
        request: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        let to_caps = dialect_capabilities(to_dialect);
        let from_caps = dialect_capabilities(from_dialect);

        let has_tools = request.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolUse { .. } | IrContentBlock::ToolResult { .. }))
        });

        if has_tools && !to_caps.tool_use.is_native() {
            return Err(MapError::IncompatibleCapability {
                capability: "tool_use".into(),
                reason: format!("{} does not support tool use", to_dialect.label()),
            });
        }

        let messages =
            if from_caps.tool_role.is_native() && !to_caps.tool_role.is_native() {
                // Dedicated Tool role → User role
                request
                    .messages
                    .iter()
                    .map(|msg| {
                        if msg.role == IrRole::Tool {
                            IrMessage {
                                role: IrRole::User,
                                content: msg.content.clone(),
                                metadata: msg.metadata.clone(),
                            }
                        } else {
                            msg.clone()
                        }
                    })
                    .collect()
            } else if !from_caps.tool_role.is_native() && to_caps.tool_role.is_native() {
                // Extract tool results from User → dedicated Tool role
                let mut result = Vec::new();
                for msg in &request.messages {
                    if msg.role == IrRole::User {
                        let (tool_results, other): (Vec<_>, Vec<_>) =
                            msg.content.iter().cloned().partition(|b| {
                                matches!(b, IrContentBlock::ToolResult { .. })
                            });

                        if !tool_results.is_empty() {
                            if !other.is_empty() {
                                result.push(IrMessage {
                                    role: IrRole::User,
                                    content: other,
                                    metadata: msg.metadata.clone(),
                                });
                            }
                            for block in tool_results {
                                result.push(IrMessage::new(IrRole::Tool, vec![block]));
                            }
                        } else {
                            result.push(msg.clone());
                        }
                    } else {
                        result.push(msg.clone());
                    }
                }
                result
            } else {
                request.messages.clone()
            };

        Ok(IrConversation::from_messages(messages))
    }

    fn name(&self) -> &str {
        "tool_mapping"
    }
}

// ── ContentMappingRule ──────────────────────────────────────────────────

/// Maps content types between dialects (text, image, thinking, system).
///
/// Degrades unsupported content types to text approximations when the
/// target dialect lacks native support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentMappingRule {
    _priv: (),
}

impl Default for ContentMappingRule {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentMappingRule {
    /// Create a new content mapping rule.
    #[must_use]
    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl MappingRule for ContentMappingRule {
    fn can_map(&self, _from_dialect: Dialect, _to_dialect: Dialect) -> bool {
        true
    }

    fn apply(
        &self,
        _from_dialect: Dialect,
        to_dialect: Dialect,
        request: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        let to_caps = dialect_capabilities(to_dialect);

        let mut messages: Vec<IrMessage> = request
            .messages
            .iter()
            .map(|msg| {
                let content: Vec<IrContentBlock> = msg
                    .content
                    .iter()
                    .map(|block| match block {
                        IrContentBlock::Thinking { text } if !to_caps.thinking.is_native() => {
                            IrContentBlock::Text {
                                text: format!("[Thinking] {text}"),
                            }
                        }
                        IrContentBlock::Image { media_type, .. }
                            if !to_caps.images.is_native() =>
                        {
                            IrContentBlock::Text {
                                text: format!("[Image: {media_type}]"),
                            }
                        }
                        other => other.clone(),
                    })
                    .collect();

                IrMessage {
                    role: msg.role,
                    content,
                    metadata: msg.metadata.clone(),
                }
            })
            .collect();

        if !to_caps.system_prompt.is_native() {
            messages = messages
                .into_iter()
                .map(|msg| {
                    if msg.role == IrRole::System {
                        let text = msg.text_content();
                        IrMessage {
                            role: IrRole::User,
                            content: vec![IrContentBlock::Text {
                                text: format!("[System] {text}"),
                            }],
                            metadata: msg.metadata.clone(),
                        }
                    } else {
                        msg
                    }
                })
                .collect();
        }

        Ok(IrConversation::from_messages(messages))
    }

    fn name(&self) -> &str {
        "content_mapping"
    }
}

// ── MetadataMappingRule ─────────────────────────────────────────────────

/// Maps vendor-specific metadata between dialects.
///
/// Strips metadata keys with vendor-specific prefixes that are not meaningful
/// in the target dialect, preserving only generic metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataMappingRule {
    _priv: (),
}

impl Default for MetadataMappingRule {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataMappingRule {
    /// Create a new metadata mapping rule.
    #[must_use]
    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl MappingRule for MetadataMappingRule {
    fn can_map(&self, _from_dialect: Dialect, _to_dialect: Dialect) -> bool {
        true
    }

    fn apply(
        &self,
        from_dialect: Dialect,
        to_dialect: Dialect,
        request: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        if from_dialect == to_dialect {
            return Ok(request.clone());
        }

        let messages = request
            .messages
            .iter()
            .map(|msg| {
                let mut metadata = BTreeMap::new();
                for (key, value) in &msg.metadata {
                    if !is_vendor_specific_key(key, from_dialect) {
                        metadata.insert(key.clone(), value.clone());
                    }
                }
                IrMessage {
                    role: msg.role,
                    content: msg.content.clone(),
                    metadata,
                }
            })
            .collect();

        Ok(IrConversation::from_messages(messages))
    }

    fn name(&self) -> &str {
        "metadata_mapping"
    }
}

/// Check if a metadata key is vendor-specific for the given dialect.
fn is_vendor_specific_key(key: &str, dialect: Dialect) -> bool {
    match dialect {
        Dialect::OpenAi => key.starts_with("openai_"),
        Dialect::Claude => key.starts_with("claude_") || key.starts_with("anthropic_"),
        Dialect::Gemini => key.starts_with("gemini_") || key.starts_with("google_"),
        Dialect::Codex => key.starts_with("codex_"),
        Dialect::Kimi => key.starts_with("kimi_") || key.starts_with("moonshot_"),
        Dialect::Copilot => key.starts_with("copilot_") || key.starts_with("github_"),
    }
}

// ── StreamMappingRule ───────────────────────────────────────────────────

/// Maps streaming event formats between dialects.
///
/// Validates that the target dialect supports streaming. At the IR level,
/// streaming differences are handled by the protocol layer; this rule
/// gates on compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamMappingRule {
    _priv: (),
}

impl Default for StreamMappingRule {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamMappingRule {
    /// Create a new stream mapping rule.
    #[must_use]
    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl MappingRule for StreamMappingRule {
    fn can_map(&self, _from_dialect: Dialect, to_dialect: Dialect) -> bool {
        dialect_capabilities(to_dialect).streaming.is_native()
    }

    fn apply(
        &self,
        _from_dialect: Dialect,
        to_dialect: Dialect,
        request: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        let to_caps = dialect_capabilities(to_dialect);
        if !to_caps.streaming.is_native() {
            return Err(MapError::IncompatibleCapability {
                capability: "streaming".into(),
                reason: format!("{} does not support streaming", to_dialect.label()),
            });
        }
        Ok(request.clone())
    }

    fn name(&self) -> &str {
        "stream_mapping"
    }
}

// ── MappingRuleKind ─────────────────────────────────────────────────────

/// Serializable enum over known mapping rule types.
///
/// Enables [`RuleChain`] to be fully serializable while delegating to
/// the [`MappingRule`] trait for polymorphic dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "rule_type", rename_all = "snake_case")]
pub enum MappingRuleKind {
    /// Tool mapping rule.
    Tool(ToolMappingRule),
    /// Content mapping rule.
    Content(ContentMappingRule),
    /// Metadata mapping rule.
    Metadata(MetadataMappingRule),
    /// Stream mapping rule.
    Stream(StreamMappingRule),
}

impl MappingRuleKind {
    fn as_rule(&self) -> &dyn MappingRule {
        match self {
            Self::Tool(r) => r,
            Self::Content(r) => r,
            Self::Metadata(r) => r,
            Self::Stream(r) => r,
        }
    }

    /// Returns `true` if this rule applies to the given dialect pair.
    pub fn can_map(&self, from_dialect: Dialect, to_dialect: Dialect) -> bool {
        self.as_rule().can_map(from_dialect, to_dialect)
    }

    /// Apply this rule to transform the conversation.
    pub fn apply(
        &self,
        from_dialect: Dialect,
        to_dialect: Dialect,
        request: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        self.as_rule().apply(from_dialect, to_dialect, request)
    }

    /// Human-readable name for this rule.
    pub fn name(&self) -> &str {
        self.as_rule().name()
    }
}

// ── RuleChain ───────────────────────────────────────────────────────────

/// Ordered list of mapping rules applied in sequence.
///
/// Each rule in the chain transforms the conversation, passing the result
/// to the next rule. Rules that report `can_map() == false` are skipped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleChain {
    /// The ordered list of rules.
    rules: Vec<MappingRuleKind>,
}

impl Default for RuleChain {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleChain {
    /// Create an empty rule chain.
    #[must_use]
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Append a rule to the chain, returning `self` for chaining.
    #[must_use]
    pub fn push(mut self, rule: MappingRuleKind) -> Self {
        self.rules.push(rule);
        self
    }

    /// Returns a slice of the rules in order.
    #[must_use]
    pub fn rules(&self) -> &[MappingRuleKind] {
        &self.rules
    }

    /// Number of rules in the chain.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Returns `true` if the chain has no rules.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Apply all applicable rules in order, threading the conversation through.
    pub fn apply_all(
        &self,
        from_dialect: Dialect,
        to_dialect: Dialect,
        request: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        let mut current = request.clone();
        for rule in &self.rules {
            if rule.can_map(from_dialect, to_dialect) {
                current = rule.apply(from_dialect, to_dialect, &current)?;
            }
        }
        Ok(current)
    }

    /// Build a default rule chain with all standard rules in recommended order.
    #[must_use]
    pub fn default_chain() -> Self {
        Self::new()
            .push(MappingRuleKind::Content(ContentMappingRule::new()))
            .push(MappingRuleKind::Tool(ToolMappingRule::new()))
            .push(MappingRuleKind::Metadata(MetadataMappingRule::new()))
            .push(MappingRuleKind::Stream(StreamMappingRule::new()))
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- ToolMappingRule --

    #[test]
    fn tool_rule_can_map_openai_to_claude() {
        let rule = ToolMappingRule::new();
        assert!(rule.can_map(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn tool_rule_cannot_map_tools_to_codex() {
        let rule = ToolMappingRule::new();
        // OpenAI has tool_use=Native, Codex has tool_use=None → can_map false
        assert!(!rule.can_map(Dialect::OpenAi, Dialect::Codex));
    }

    #[test]
    fn tool_rule_can_map_codex_to_codex() {
        let rule = ToolMappingRule::new();
        // Codex has no tools → from_caps.tool_use is None → can_map true
        assert!(rule.can_map(Dialect::Codex, Dialect::Codex));
    }

    #[test]
    fn tool_rule_error_on_tools_to_no_tool_dialect() {
        let rule = ToolMappingRule::new();
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read_file".into(),
                input: json!({"path": "foo.txt"}),
            }],
        )]);
        let result = rule.apply(Dialect::OpenAi, Dialect::Codex, &conv);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MapError::IncompatibleCapability { .. }
        ));
    }

    #[test]
    fn tool_rule_converts_tool_role_to_user() {
        // OpenAI (tool_role=Native) → Claude (tool_role=None)
        let rule = ToolMappingRule::new();
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        )]);
        let result = rule.apply(Dialect::OpenAi, Dialect::Claude, &conv).unwrap();
        assert_eq!(result.messages[0].role, IrRole::User);
    }

    #[test]
    fn tool_rule_extracts_tool_results_to_tool_role() {
        // Claude (tool_role=None) → OpenAI (tool_role=Native)
        let rule = ToolMappingRule::new();
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "Here's the result".into(),
                },
                IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "done".into(),
                    }],
                    is_error: false,
                },
            ],
        )]);
        let result = rule.apply(Dialect::Claude, Dialect::OpenAi, &conv).unwrap();
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[0].role, IrRole::User);
        assert_eq!(result.messages[1].role, IrRole::Tool);
    }

    #[test]
    fn tool_rule_passthrough_same_tool_role() {
        // OpenAI → Kimi (both have tool_role=Native)
        let rule = ToolMappingRule::new();
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "hello"),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "ok".into(),
                    }],
                    is_error: false,
                }],
            ),
        ]);
        let result = rule.apply(Dialect::OpenAi, Dialect::Kimi, &conv).unwrap();
        assert_eq!(result.messages[1].role, IrRole::Tool);
    }

    #[test]
    fn tool_rule_no_tools_passthrough() {
        let rule = ToolMappingRule::new();
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hello")]);
        let result = rule
            .apply(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    // -- ContentMappingRule --

    #[test]
    fn content_rule_always_can_map() {
        let rule = ContentMappingRule::new();
        for &from in Dialect::all() {
            for &to in Dialect::all() {
                assert!(rule.can_map(from, to));
            }
        }
    }

    #[test]
    fn content_rule_degrades_thinking() {
        let rule = ContentMappingRule::new();
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think".into(),
                },
                IrContentBlock::Text {
                    text: "Answer".into(),
                },
            ],
        )]);
        let result = rule.apply(Dialect::Claude, Dialect::OpenAi, &conv).unwrap();
        let asst = &result.messages[0];
        assert!(matches!(&asst.content[0], IrContentBlock::Text { text } if text.starts_with("[Thinking]")));
        assert!(matches!(&asst.content[1], IrContentBlock::Text { text } if text == "Answer"));
    }

    #[test]
    fn content_rule_preserves_thinking_for_claude() {
        let rule = ContentMappingRule::new();
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "hmm".into(),
            }],
        )]);
        let result = rule
            .apply(Dialect::Claude, Dialect::Claude, &conv)
            .unwrap();
        assert!(matches!(
            &result.messages[0].content[0],
            IrContentBlock::Thinking { .. }
        ));
    }

    #[test]
    fn content_rule_degrades_images() {
        let rule = ContentMappingRule::new();
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            }],
        )]);
        let result = rule.apply(Dialect::OpenAi, Dialect::Codex, &conv).unwrap();
        assert!(
            matches!(&result.messages[0].content[0], IrContentBlock::Text { text } if text == "[Image: image/png]")
        );
    }

    #[test]
    fn content_rule_converts_system_for_codex() {
        let rule = ContentMappingRule::new();
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful"),
            IrMessage::text(IrRole::User, "Hi"),
        ]);
        let result = rule.apply(Dialect::OpenAi, Dialect::Codex, &conv).unwrap();
        assert_eq!(result.messages[0].role, IrRole::User);
        assert!(result.messages[0].text_content().starts_with("[System]"));
        assert_eq!(result.messages[1].role, IrRole::User);
    }

    #[test]
    fn content_rule_preserves_text() {
        let rule = ContentMappingRule::new();
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hello")]);
        let result = rule
            .apply(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages[0].text_content(), "hello");
    }

    // -- MetadataMappingRule --

    #[test]
    fn metadata_rule_strips_vendor_keys() {
        let rule = MetadataMappingRule::new();
        let mut metadata = BTreeMap::new();
        metadata.insert("openai_logprobs".into(), json!(true));
        metadata.insert("generic_key".into(), json!("value"));
        let conv = IrConversation::from_messages(vec![IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "hi".into(),
            }],
            metadata,
        }]);
        let result = rule
            .apply(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert!(!result.messages[0].metadata.contains_key("openai_logprobs"));
        assert!(result.messages[0].metadata.contains_key("generic_key"));
    }

    #[test]
    fn metadata_rule_passthrough_same_dialect() {
        let rule = MetadataMappingRule::new();
        let mut metadata = BTreeMap::new();
        metadata.insert("openai_logprobs".into(), json!(true));
        let conv = IrConversation::from_messages(vec![IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "hi".into(),
            }],
            metadata,
        }]);
        let result = rule
            .apply(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert!(result.messages[0].metadata.contains_key("openai_logprobs"));
    }

    #[test]
    fn metadata_rule_strips_claude_keys() {
        let rule = MetadataMappingRule::new();
        let mut metadata = BTreeMap::new();
        metadata.insert("claude_cache".into(), json!(true));
        metadata.insert("anthropic_beta".into(), json!("v2"));
        metadata.insert("shared".into(), json!(1));
        let conv = IrConversation::from_messages(vec![IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "hi".into(),
            }],
            metadata,
        }]);
        let result = rule
            .apply(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        assert!(!result.messages[0].metadata.contains_key("claude_cache"));
        assert!(!result.messages[0].metadata.contains_key("anthropic_beta"));
        assert!(result.messages[0].metadata.contains_key("shared"));
    }

    // -- StreamMappingRule --

    #[test]
    fn stream_rule_cannot_map_to_codex() {
        let rule = StreamMappingRule::new();
        assert!(!rule.can_map(Dialect::OpenAi, Dialect::Codex));
    }

    #[test]
    fn stream_rule_can_map_to_openai() {
        let rule = StreamMappingRule::new();
        assert!(rule.can_map(Dialect::Claude, Dialect::OpenAi));
    }

    #[test]
    fn stream_rule_error_to_non_streaming() {
        let rule = StreamMappingRule::new();
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        let result = rule.apply(Dialect::OpenAi, Dialect::Codex, &conv);
        assert!(result.is_err());
    }

    #[test]
    fn stream_rule_passthrough_to_streaming() {
        let rule = StreamMappingRule::new();
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        let result = rule
            .apply(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    // -- MappingRuleKind --

    #[test]
    fn rule_kind_serialize_roundtrip() {
        let kind = MappingRuleKind::Tool(ToolMappingRule::new());
        let json = serde_json::to_string(&kind).unwrap();
        let back: MappingRuleKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name(), "tool_mapping");
    }

    #[test]
    fn rule_kind_delegates_name() {
        assert_eq!(
            MappingRuleKind::Content(ContentMappingRule::new()).name(),
            "content_mapping"
        );
        assert_eq!(
            MappingRuleKind::Metadata(MetadataMappingRule::new()).name(),
            "metadata_mapping"
        );
        assert_eq!(
            MappingRuleKind::Stream(StreamMappingRule::new()).name(),
            "stream_mapping"
        );
    }

    // -- RuleChain --

    #[test]
    fn empty_chain_passthrough() {
        let chain = RuleChain::new();
        assert!(chain.is_empty());
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        let result = chain
            .apply_all(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    #[test]
    fn default_chain_has_all_rules() {
        let chain = RuleChain::default_chain();
        assert_eq!(chain.len(), 4);
        let names: Vec<&str> = chain.rules().iter().map(|r| r.name()).collect();
        assert_eq!(
            names,
            vec![
                "content_mapping",
                "tool_mapping",
                "metadata_mapping",
                "stream_mapping"
            ]
        );
    }

    #[test]
    fn chain_applies_rules_in_order() {
        // Content rule runs first (degrades thinking), then tool rule
        let chain = RuleChain::default_chain();
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Solve this"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "Let me think".into(),
                    },
                    IrContentBlock::Text {
                        text: "42".into(),
                    },
                ],
            ),
        ]);
        let result = chain
            .apply_all(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        // Thinking should be degraded to text by the content rule
        assert!(matches!(
            &result.messages[1].content[0],
            IrContentBlock::Text { text } if text.starts_with("[Thinking]")
        ));
    }

    #[test]
    fn chain_skips_inapplicable_rules() {
        // Stream rule can_map is false for Codex target, so it's skipped
        let chain = RuleChain::new()
            .push(MappingRuleKind::Content(ContentMappingRule::new()))
            .push(MappingRuleKind::Stream(StreamMappingRule::new()));
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        // Should not error because stream rule is skipped (can_map=false for Codex)
        let result = chain
            .apply_all(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn chain_serialize_roundtrip() {
        let chain = RuleChain::default_chain();
        let json = serde_json::to_string(&chain).unwrap();
        let back: RuleChain = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), chain.len());
    }

    #[test]
    fn rule_types_are_debug_clone() {
        let tool = ToolMappingRule::new();
        let _ = format!("{:?}", tool);
        let _ = tool.clone();

        let content = ContentMappingRule::new();
        let _ = format!("{:?}", content);
        let _ = content.clone();

        let meta = MetadataMappingRule::new();
        let _ = format!("{:?}", meta);
        let _ = meta.clone();

        let stream = StreamMappingRule::new();
        let _ = format!("{:?}", stream);
        let _ = stream.clone();

        let chain = RuleChain::default_chain();
        let _ = format!("{:?}", chain);
        let _ = chain.clone();
    }

    #[test]
    fn chain_complex_cross_dialect() {
        // Claude → Codex: system + thinking + images all need content mapping
        let chain = RuleChain::new().push(MappingRuleKind::Content(ContentMappingRule::new()));
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful"),
            IrMessage::new(
                IrRole::User,
                vec![
                    IrContentBlock::Text {
                        text: "Look".into(),
                    },
                    IrContentBlock::Image {
                        media_type: "image/jpeg".into(),
                        data: "data".into(),
                    },
                ],
            ),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "hmm".into(),
                    },
                    IrContentBlock::Text {
                        text: "I see".into(),
                    },
                ],
            ),
        ]);
        let result = chain
            .apply_all(Dialect::Claude, Dialect::Codex, &conv)
            .unwrap();
        // System → User with prefix
        assert_eq!(result.messages[0].role, IrRole::User);
        assert!(result.messages[0].text_content().contains("[System]"));
        // Image → placeholder
        assert!(result.messages[1].text_content().contains("[Image:"));
        // Thinking → text
        assert!(result.messages[2].text_content().contains("[Thinking]"));
    }

    #[test]
    fn empty_request_passthrough() {
        let chain = RuleChain::default_chain();
        let conv = IrConversation::new();
        let result = chain
            .apply_all(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert!(result.is_empty());
    }
}
