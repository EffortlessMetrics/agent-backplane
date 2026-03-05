// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code)]

//! Projection matrix — the core cross-dialect mapping engine.
//!
//! Maps `(source_dialect, target_dialect)` pairs to `MappingRuleSet`s and
//! applies dialect-specific transformations to `IrChatRequest` and
//! `IrChatResponse`.
//!
//! The `ProjectionMatrix` registers built-in mappings for all 7 supported
//! dialects (OpenAI, Claude, Gemini, Codex, Kimi, Copilot, Generic) and
//! provides `project_request` / `project_response` free functions for
//! one-shot translation.

use std::collections::BTreeMap;
use std::fmt;

use abp_dialect::Dialect;
use abp_sdk_types::ir::{IrContentPart, IrMessage, IrRole};
use abp_sdk_types::ir_request::IrChatRequest;
use abp_sdk_types::ir_response::{IrChatResponse, IrFinishReason};
use serde::{Deserialize, Serialize};

use crate::error::MappingError;

// ── ProjectionDialect ──────────────────────────────────────────────────

/// Dialect identifier for the projection matrix.
///
/// Extends the core [`Dialect`] with a [`Generic`](Self::Generic) variant
/// that acts as a universal passthrough — mapping to or from `Generic`
/// always produces an identity transformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionDialect {
    /// OpenAI Chat Completions style.
    OpenAi,
    /// Anthropic Claude Messages API.
    Claude,
    /// Google Gemini generateContent style.
    Gemini,
    /// OpenAI Codex / Responses API style.
    Codex,
    /// Moonshot Kimi API style.
    Kimi,
    /// GitHub Copilot Extensions style.
    Copilot,
    /// Generic / passthrough — no dialect-specific transformation.
    Generic,
}

impl ProjectionDialect {
    /// Returns all projection dialect variants.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::OpenAi,
            Self::Claude,
            Self::Gemini,
            Self::Codex,
            Self::Kimi,
            Self::Copilot,
            Self::Generic,
        ]
    }

    /// Human-readable label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Codex => "Codex",
            Self::Kimi => "Kimi",
            Self::Copilot => "Copilot",
            Self::Generic => "Generic",
        }
    }

    /// Convert to the core [`Dialect`], returning `None` for [`Generic`](Self::Generic).
    #[must_use]
    pub fn to_dialect(self) -> Option<Dialect> {
        match self {
            Self::OpenAi => Some(Dialect::OpenAi),
            Self::Claude => Some(Dialect::Claude),
            Self::Gemini => Some(Dialect::Gemini),
            Self::Codex => Some(Dialect::Codex),
            Self::Kimi => Some(Dialect::Kimi),
            Self::Copilot => Some(Dialect::Copilot),
            Self::Generic => None,
        }
    }

    /// Convert to a [`Dialect`] for error reporting, using `OpenAi` as
    /// fallback for Generic.
    fn error_dialect(self) -> Dialect {
        self.to_dialect().unwrap_or(Dialect::OpenAi)
    }
}

impl From<Dialect> for ProjectionDialect {
    fn from(d: Dialect) -> Self {
        match d {
            Dialect::OpenAi => Self::OpenAi,
            Dialect::Claude => Self::Claude,
            Dialect::Gemini => Self::Gemini,
            Dialect::Codex => Self::Codex,
            Dialect::Kimi => Self::Kimi,
            Dialect::Copilot => Self::Copilot,
        }
    }
}

impl fmt::Display for ProjectionDialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

// ── TransformStrategy ──────────────────────────────────────────────────

/// How a feature should be handled when projecting between dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransformStrategy {
    /// Pass the feature through unchanged.
    Passthrough,
    /// Emulate the feature with a best-effort approximation.
    Emulate,
    /// Silently strip the feature from the request/response.
    Strip,
    /// Return an error if the feature is used.
    Error,
}

impl fmt::Display for TransformStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Passthrough => f.write_str("passthrough"),
            Self::Emulate => f.write_str("emulate"),
            Self::Strip => f.write_str("strip"),
            Self::Error => f.write_str("error"),
        }
    }
}

// ── MappingRuleSet ─────────────────────────────────────────────────────

/// Describes how each feature category should be transformed for a
/// dialect pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingRuleSet {
    /// How to handle system-role messages.
    pub system_messages: TransformStrategy,
    /// How to handle tool definitions and tool-call content.
    pub tool_calls: TransformStrategy,
    /// How to handle streaming configuration.
    pub streaming: TransformStrategy,
    /// How to handle image content parts.
    pub images: TransformStrategy,
    /// How to handle audio content parts.
    pub audio: TransformStrategy,
}

impl MappingRuleSet {
    /// All-passthrough rule set (identity mapping).
    #[must_use]
    pub fn identity() -> Self {
        Self {
            system_messages: TransformStrategy::Passthrough,
            tool_calls: TransformStrategy::Passthrough,
            streaming: TransformStrategy::Passthrough,
            images: TransformStrategy::Passthrough,
            audio: TransformStrategy::Passthrough,
        }
    }

    /// Returns `true` if all strategies are [`Passthrough`](TransformStrategy::Passthrough).
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.system_messages == TransformStrategy::Passthrough
            && self.tool_calls == TransformStrategy::Passthrough
            && self.streaming == TransformStrategy::Passthrough
            && self.images == TransformStrategy::Passthrough
            && self.audio == TransformStrategy::Passthrough
    }
}

// ── FeatureSupport ─────────────────────────────────────────────────────

/// How a feature is supported for a given dialect pair in the fidelity matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "level", rename_all = "snake_case")]
pub enum FeatureSupport {
    /// The feature is natively supported by both dialects.
    Native,
    /// The feature is emulated with an approximation strategy.
    Emulated {
        /// Description of the emulation strategy.
        strategy: String,
    },
    /// The feature is not supported and will cause an error or be stripped.
    Unsupported,
}

impl FeatureSupport {
    /// Returns `true` if the feature is natively supported.
    #[must_use]
    pub fn is_native(&self) -> bool {
        matches!(self, Self::Native)
    }

    /// Returns `true` if the feature is unsupported.
    #[must_use]
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Self::Unsupported)
    }
}

// ── FeatureFidelity ────────────────────────────────────────────────────

/// A single entry in the [`FidelityMatrix`] describing fidelity for one
/// feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFidelity {
    /// Name of the feature (e.g. `"system_messages"`, `"tool_calls"`).
    pub feature: String,
    /// Support level for this feature.
    pub support: FeatureSupport,
    /// Human-readable notes about the mapping behaviour.
    pub notes: String,
}

// ── FidelityMatrix ─────────────────────────────────────────────────────

/// Tracks what features are natively supported, emulated, or unsupported
/// for each dialect pair.
#[derive(Debug, Clone, Default)]
pub struct FidelityMatrix {
    entries: BTreeMap<(ProjectionDialect, ProjectionDialect), Vec<FeatureFidelity>>,
}

impl FidelityMatrix {
    /// Create an empty fidelity matrix.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a fidelity matrix pre-populated with entries for all
    /// built-in dialect pairs.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut m = Self::new();
        for &src in ProjectionDialect::all() {
            for &tgt in ProjectionDialect::all() {
                let entries = build_fidelity_entries(src, tgt);
                m.entries.insert((src, tgt), entries);
            }
        }
        m
    }

    /// Look up fidelity entries for a dialect pair.
    #[must_use]
    pub fn lookup(&self, from: ProjectionDialect, to: ProjectionDialect) -> &[FeatureFidelity] {
        self.entries
            .get(&(from, to))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Returns `true` if any feature is unsupported for the given pair.
    #[must_use]
    pub fn has_unsupported(&self, from: ProjectionDialect, to: ProjectionDialect) -> bool {
        self.lookup(from, to)
            .iter()
            .any(|f| f.support.is_unsupported())
    }

    /// Returns `true` if all features are natively supported for the
    /// given pair.
    #[must_use]
    pub fn is_fully_native(&self, from: ProjectionDialect, to: ProjectionDialect) -> bool {
        let entries = self.lookup(from, to);
        !entries.is_empty() && entries.iter().all(|f| f.support.is_native())
    }

    /// Register fidelity entries for a dialect pair.
    pub fn register(
        &mut self,
        from: ProjectionDialect,
        to: ProjectionDialect,
        entries: Vec<FeatureFidelity>,
    ) {
        self.entries.insert((from, to), entries);
    }
}

// ── ProjectionMatrix ───────────────────────────────────────────────────

/// The core projection matrix mapping `(source, target)` dialect pairs to
/// [`MappingRuleSet`]s.
///
/// Use [`with_defaults()`](Self::with_defaults) to get a pre-configured
/// matrix with built-in rules for all 7 dialects (including Generic).
#[derive(Debug, Clone)]
pub struct ProjectionMatrix {
    rules: BTreeMap<(ProjectionDialect, ProjectionDialect), MappingRuleSet>,
    fidelity: FidelityMatrix,
}

impl ProjectionMatrix {
    /// Create an empty projection matrix.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rules: BTreeMap::new(),
            fidelity: FidelityMatrix::new(),
        }
    }

    /// Create a projection matrix pre-populated with all built-in
    /// dialect pair rules.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut matrix = Self::new();
        matrix.fidelity = FidelityMatrix::with_defaults();
        register_builtin_rules(&mut matrix);
        matrix
    }

    /// Register a mapping rule for a dialect pair.
    pub fn register_rule(
        &mut self,
        from: ProjectionDialect,
        to: ProjectionDialect,
        rule: MappingRuleSet,
    ) {
        self.rules.insert((from, to), rule);
    }

    /// Look up the rule for a dialect pair.
    #[must_use]
    pub fn get_rule(
        &self,
        from: ProjectionDialect,
        to: ProjectionDialect,
    ) -> Option<&MappingRuleSet> {
        self.rules.get(&(from, to))
    }

    /// Returns a reference to the fidelity matrix.
    #[must_use]
    pub fn fidelity(&self) -> &FidelityMatrix {
        &self.fidelity
    }

    /// Returns all registered dialect pairs.
    #[must_use]
    pub fn registered_pairs(&self) -> Vec<(ProjectionDialect, ProjectionDialect)> {
        self.rules.keys().copied().collect()
    }

    /// Apply a request projection using this matrix's rules.
    pub fn project_request(
        &self,
        source: ProjectionDialect,
        target: ProjectionDialect,
        request: &IrChatRequest,
    ) -> Result<IrChatRequest, MappingError> {
        let rule =
            self.get_rule(source, target)
                .ok_or_else(|| MappingError::UnmappableRequest {
                    reason: format!("no projection rule for {source} -> {target}"),
                })?;
        apply_request_rule(rule, source, target, request)
    }

    /// Apply a response projection using this matrix's rules.
    pub fn project_response(
        &self,
        source: ProjectionDialect,
        target: ProjectionDialect,
        response: &IrChatResponse,
    ) -> Result<IrChatResponse, MappingError> {
        let rule =
            self.get_rule(source, target)
                .ok_or_else(|| MappingError::UnmappableRequest {
                    reason: format!("no projection rule for {source} -> {target}"),
                })?;
        apply_response_rule(rule, source, target, response)
    }
}

impl Default for ProjectionMatrix {
    fn default() -> Self {
        Self::new()
    }
}

// ── Free functions ─────────────────────────────────────────────────────

/// Project an IR chat request from one dialect to another using the
/// default built-in projection matrix.
pub fn project_request(
    source: ProjectionDialect,
    target: ProjectionDialect,
    request: &IrChatRequest,
) -> Result<IrChatRequest, MappingError> {
    ProjectionMatrix::with_defaults().project_request(source, target, request)
}

/// Project an IR chat response from one dialect to another using the
/// default built-in projection matrix.
pub fn project_response(
    source: ProjectionDialect,
    target: ProjectionDialect,
    response: &IrChatResponse,
) -> Result<IrChatResponse, MappingError> {
    ProjectionMatrix::with_defaults().project_response(source, target, response)
}

// ── Built-in rule registration ─────────────────────────────────────────

fn register_builtin_rules(matrix: &mut ProjectionMatrix) {
    for &src in ProjectionDialect::all() {
        for &tgt in ProjectionDialect::all() {
            matrix.register_rule(src, tgt, build_rule_set(src, tgt));
        }
    }
}

/// Build the rule set for a specific source → target pair.
fn build_rule_set(src: ProjectionDialect, tgt: ProjectionDialect) -> MappingRuleSet {
    use ProjectionDialect::*;

    // Identity / Generic passthrough
    if src == tgt || src == Generic || tgt == Generic {
        return MappingRuleSet::identity();
    }

    // Target is Codex — very limited feature surface
    if tgt == Codex {
        return MappingRuleSet {
            system_messages: TransformStrategy::Emulate,
            tool_calls: TransformStrategy::Error,
            streaming: TransformStrategy::Strip,
            images: TransformStrategy::Strip,
            audio: TransformStrategy::Strip,
        };
    }

    // Target is Kimi or Copilot — no images / audio
    if tgt == Kimi || tgt == Copilot {
        return MappingRuleSet {
            system_messages: TransformStrategy::Passthrough,
            tool_calls: TransformStrategy::Passthrough,
            streaming: TransformStrategy::Passthrough,
            images: TransformStrategy::Strip,
            audio: TransformStrategy::Strip,
        };
    }

    // All other pairs (OpenAI ↔ Claude ↔ Gemini, Codex → *) — full support
    MappingRuleSet::identity()
}

// ── Request transformation ─────────────────────────────────────────────

fn apply_request_rule(
    rule: &MappingRuleSet,
    source: ProjectionDialect,
    target: ProjectionDialect,
    request: &IrChatRequest,
) -> Result<IrChatRequest, MappingError> {
    let mut req = request.clone();

    // System messages
    match rule.system_messages {
        TransformStrategy::Passthrough => {}
        TransformStrategy::Emulate => {
            req.messages = emulate_system_messages(&req.messages);
        }
        TransformStrategy::Strip => {
            req.messages.retain(|m| m.role != IrRole::System);
        }
        TransformStrategy::Error => {
            if req.messages.iter().any(|m| m.role == IrRole::System) {
                return Err(MappingError::UnsupportedCapability {
                    capability: "system_messages".into(),
                    source_dialect: source.error_dialect(),
                    target_dialect: target.error_dialect(),
                });
            }
        }
    }

    // Tool calls
    match rule.tool_calls {
        TransformStrategy::Passthrough => {}
        TransformStrategy::Emulate => { /* stub — no emulation strategy yet */ }
        TransformStrategy::Strip => {
            req.tools.clear();
            req.tool_choice = None;
            strip_tool_content(&mut req.messages);
        }
        TransformStrategy::Error => {
            if !req.tools.is_empty() || has_tool_content(&req.messages) {
                return Err(MappingError::UnsupportedCapability {
                    capability: "tool_calls".into(),
                    source_dialect: source.error_dialect(),
                    target_dialect: target.error_dialect(),
                });
            }
        }
    }

    // Streaming
    match rule.streaming {
        TransformStrategy::Passthrough => {}
        TransformStrategy::Strip => {
            req.stream = abp_sdk_types::ir_request::IrStreamConfig::default();
        }
        TransformStrategy::Error => {
            if req.stream.enabled {
                return Err(MappingError::UnsupportedCapability {
                    capability: "streaming".into(),
                    source_dialect: source.error_dialect(),
                    target_dialect: target.error_dialect(),
                });
            }
        }
        TransformStrategy::Emulate => {}
    }

    // Images
    match rule.images {
        TransformStrategy::Passthrough => {}
        TransformStrategy::Strip | TransformStrategy::Emulate => {
            strip_image_content(&mut req.messages);
        }
        TransformStrategy::Error => {
            if has_image_content(&req.messages) {
                return Err(MappingError::UnsupportedCapability {
                    capability: "images".into(),
                    source_dialect: source.error_dialect(),
                    target_dialect: target.error_dialect(),
                });
            }
        }
    }

    // Audio
    match rule.audio {
        TransformStrategy::Passthrough => {}
        TransformStrategy::Strip | TransformStrategy::Emulate => {
            strip_audio_content(&mut req.messages);
        }
        TransformStrategy::Error => {
            if has_audio_content(&req.messages) {
                return Err(MappingError::UnsupportedCapability {
                    capability: "audio".into(),
                    source_dialect: source.error_dialect(),
                    target_dialect: target.error_dialect(),
                });
            }
        }
    }

    Ok(req)
}

// ── Response transformation ────────────────────────────────────────────

fn apply_response_rule(
    rule: &MappingRuleSet,
    source: ProjectionDialect,
    target: ProjectionDialect,
    response: &IrChatResponse,
) -> Result<IrChatResponse, MappingError> {
    let mut resp = response.clone();

    // Tool calls in response
    match rule.tool_calls {
        TransformStrategy::Passthrough => {}
        TransformStrategy::Strip | TransformStrategy::Emulate => {
            for choice in &mut resp.choices {
                choice.message.tool_calls.clear();
                strip_tool_content_parts(&mut choice.message.content);
                if choice.finish_reason == Some(IrFinishReason::ToolUse) {
                    choice.finish_reason = Some(IrFinishReason::Stop);
                }
            }
        }
        TransformStrategy::Error => {
            if resp.has_tool_calls() {
                return Err(MappingError::UnsupportedCapability {
                    capability: "tool_calls".into(),
                    source_dialect: source.error_dialect(),
                    target_dialect: target.error_dialect(),
                });
            }
        }
    }

    // Images in response
    match rule.images {
        TransformStrategy::Strip | TransformStrategy::Emulate => {
            for choice in &mut resp.choices {
                strip_image_parts(&mut choice.message.content);
            }
        }
        TransformStrategy::Error => {
            let has = resp.choices.iter().any(|c| {
                c.message
                    .content
                    .iter()
                    .any(|p| matches!(p, IrContentPart::Image { .. }))
            });
            if has {
                return Err(MappingError::UnsupportedCapability {
                    capability: "images".into(),
                    source_dialect: source.error_dialect(),
                    target_dialect: target.error_dialect(),
                });
            }
        }
        _ => {}
    }

    // Audio in response
    match rule.audio {
        TransformStrategy::Strip | TransformStrategy::Emulate => {
            for choice in &mut resp.choices {
                strip_audio_parts(&mut choice.message.content);
            }
        }
        TransformStrategy::Error => {
            let has = resp.choices.iter().any(|c| {
                c.message
                    .content
                    .iter()
                    .any(|p| matches!(p, IrContentPart::Audio { .. }))
            });
            if has {
                return Err(MappingError::UnsupportedCapability {
                    capability: "audio".into(),
                    source_dialect: source.error_dialect(),
                    target_dialect: target.error_dialect(),
                });
            }
        }
        _ => {}
    }

    Ok(resp)
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Fold system messages into user messages prefixed with `[System] `.
fn emulate_system_messages(messages: &[IrMessage]) -> Vec<IrMessage> {
    messages
        .iter()
        .map(|msg| {
            if msg.role == IrRole::System {
                let text = msg.text_content();
                IrMessage {
                    role: IrRole::User,
                    content: vec![IrContentPart::text(format!("[System] {text}"))],
                    tool_calls: Vec::new(),
                    metadata: msg.metadata.clone(),
                }
            } else {
                msg.clone()
            }
        })
        .collect()
}

fn has_tool_content(messages: &[IrMessage]) -> bool {
    messages.iter().any(|m| {
        !m.tool_calls.is_empty()
            || m.content
                .iter()
                .any(|p| p.is_tool_use() || p.is_tool_result())
    })
}

fn strip_tool_content(messages: &mut Vec<IrMessage>) {
    for msg in messages.iter_mut() {
        msg.tool_calls.clear();
        strip_tool_content_parts(&mut msg.content);
    }
    messages.retain(|m| m.role != IrRole::Tool);
}

fn strip_tool_content_parts(parts: &mut Vec<IrContentPart>) {
    parts.retain(|p| !p.is_tool_use() && !p.is_tool_result());
}

fn has_image_content(messages: &[IrMessage]) -> bool {
    messages.iter().any(|m| {
        m.content
            .iter()
            .any(|p| matches!(p, IrContentPart::Image { .. }))
    })
}

fn strip_image_content(messages: &mut [IrMessage]) {
    for msg in messages.iter_mut() {
        strip_image_parts(&mut msg.content);
    }
}

fn strip_image_parts(parts: &mut Vec<IrContentPart>) {
    parts.retain(|p| !matches!(p, IrContentPart::Image { .. }));
}

fn has_audio_content(messages: &[IrMessage]) -> bool {
    messages.iter().any(|m| {
        m.content
            .iter()
            .any(|p| matches!(p, IrContentPart::Audio { .. }))
    })
}

fn strip_audio_content(messages: &mut [IrMessage]) {
    for msg in messages.iter_mut() {
        strip_audio_parts(&mut msg.content);
    }
}

fn strip_audio_parts(parts: &mut Vec<IrContentPart>) {
    parts.retain(|p| !matches!(p, IrContentPart::Audio { .. }));
}

// ── Fidelity entry builder ─────────────────────────────────────────────

fn build_fidelity_entries(src: ProjectionDialect, tgt: ProjectionDialect) -> Vec<FeatureFidelity> {
    let rule = build_rule_set(src, tgt);
    vec![
        fidelity_from_strategy("system_messages", rule.system_messages),
        fidelity_from_strategy("tool_calls", rule.tool_calls),
        fidelity_from_strategy("streaming", rule.streaming),
        fidelity_from_strategy("images", rule.images),
        fidelity_from_strategy("audio", rule.audio),
    ]
}

fn fidelity_from_strategy(feature: &str, strategy: TransformStrategy) -> FeatureFidelity {
    let (support, notes) = match strategy {
        TransformStrategy::Passthrough => (
            FeatureSupport::Native,
            format!("{feature} passed through natively"),
        ),
        TransformStrategy::Emulate => (
            FeatureSupport::Emulated {
                strategy: format!("{feature} emulated via approximation"),
            },
            format!("{feature} emulated with best-effort approximation"),
        ),
        TransformStrategy::Strip => (
            FeatureSupport::Unsupported,
            format!("{feature} silently stripped from request"),
        ),
        TransformStrategy::Error => (
            FeatureSupport::Unsupported,
            format!("{feature} will cause an error if used"),
        ),
    };
    FeatureFidelity {
        feature: feature.into(),
        support,
        notes,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use abp_sdk_types::ir::{IrToolCall, IrToolDefinition};
    use abp_sdk_types::ir_request::IrStreamConfig;
    use abp_sdk_types::ir_response::IrChoice;
    use std::collections::BTreeMap;

    // ── Helpers ─────────────────────────────────────────────────────

    fn simple_request() -> IrChatRequest {
        IrChatRequest::new("gpt-4o", vec![IrMessage::text(IrRole::User, "Hello")])
    }

    fn request_with_system() -> IrChatRequest {
        IrChatRequest::new(
            "gpt-4o",
            vec![
                IrMessage::text(IrRole::System, "Be helpful"),
                IrMessage::text(IrRole::User, "Hello"),
            ],
        )
    }

    fn request_with_tools() -> IrChatRequest {
        IrChatRequest::new("gpt-4o", vec![IrMessage::text(IrRole::User, "Search")]).with_tool(
            IrToolDefinition {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        )
    }

    fn request_with_images() -> IrChatRequest {
        IrChatRequest::new(
            "gpt-4o",
            vec![IrMessage::new(
                IrRole::User,
                vec![
                    IrContentPart::text("What is this?"),
                    IrContentPart::image_url("https://example.com/img.png"),
                ],
            )],
        )
    }

    fn request_with_audio() -> IrChatRequest {
        IrChatRequest::new(
            "gpt-4o",
            vec![IrMessage::new(
                IrRole::User,
                vec![IrContentPart::Audio {
                    media_type: "audio/wav".into(),
                    data: "RIFF...".into(),
                }],
            )],
        )
    }

    fn streaming_request() -> IrChatRequest {
        IrChatRequest::new("gpt-4o", vec![IrMessage::text(IrRole::User, "Hello")]).with_stream(
            IrStreamConfig {
                enabled: true,
                include_usage: Some(true),
                extra: BTreeMap::new(),
            },
        )
    }

    fn simple_response() -> IrChatResponse {
        IrChatResponse::text("Hello!")
    }

    fn response_with_tool_calls() -> IrChatResponse {
        IrChatResponse {
            id: None,
            model: None,
            choices: vec![IrChoice {
                index: 0,
                message: IrMessage {
                    role: IrRole::Assistant,
                    content: vec![],
                    tool_calls: vec![IrToolCall {
                        id: "call_1".into(),
                        name: "search".into(),
                        arguments: serde_json::json!({"q": "rust"}),
                    }],
                    metadata: BTreeMap::new(),
                },
                finish_reason: Some(IrFinishReason::ToolUse),
            }],
            usage: None,
            metadata: BTreeMap::new(),
        }
    }

    // ── ProjectionDialect ──────────────────────────────────────────

    #[test]
    fn dialect_all_has_seven_variants() {
        assert_eq!(ProjectionDialect::all().len(), 7);
    }

    #[test]
    fn dialect_display() {
        assert_eq!(ProjectionDialect::OpenAi.to_string(), "OpenAI");
        assert_eq!(ProjectionDialect::Claude.to_string(), "Claude");
        assert_eq!(ProjectionDialect::Generic.to_string(), "Generic");
    }

    #[test]
    fn dialect_serde_roundtrip() {
        for &d in ProjectionDialect::all() {
            let json = serde_json::to_string(&d).unwrap();
            let back: ProjectionDialect = serde_json::from_str(&json).unwrap();
            assert_eq!(d, back);
        }
    }

    #[test]
    fn dialect_from_core_dialect() {
        assert_eq!(
            ProjectionDialect::from(Dialect::OpenAi),
            ProjectionDialect::OpenAi
        );
        assert_eq!(
            ProjectionDialect::from(Dialect::Claude),
            ProjectionDialect::Claude
        );
        assert_eq!(
            ProjectionDialect::from(Dialect::Codex),
            ProjectionDialect::Codex
        );
    }

    #[test]
    fn dialect_to_core_dialect() {
        assert_eq!(
            ProjectionDialect::OpenAi.to_dialect(),
            Some(Dialect::OpenAi)
        );
        assert_eq!(ProjectionDialect::Generic.to_dialect(), None);
    }

    // ── TransformStrategy ──────────────────────────────────────────

    #[test]
    fn transform_strategy_serde_roundtrip() {
        for s in [
            TransformStrategy::Passthrough,
            TransformStrategy::Emulate,
            TransformStrategy::Strip,
            TransformStrategy::Error,
        ] {
            let json = serde_json::to_string(&s).unwrap();
            let back: TransformStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(s, back);
        }
    }

    #[test]
    fn transform_strategy_display() {
        assert_eq!(TransformStrategy::Passthrough.to_string(), "passthrough");
        assert_eq!(TransformStrategy::Emulate.to_string(), "emulate");
        assert_eq!(TransformStrategy::Strip.to_string(), "strip");
        assert_eq!(TransformStrategy::Error.to_string(), "error");
    }

    // ── MappingRuleSet ─────────────────────────────────────────────

    #[test]
    fn rule_set_identity() {
        let rule = MappingRuleSet::identity();
        assert!(rule.is_identity());
        assert_eq!(rule.system_messages, TransformStrategy::Passthrough);
    }

    #[test]
    fn rule_set_serde_roundtrip() {
        let rule = MappingRuleSet {
            system_messages: TransformStrategy::Emulate,
            tool_calls: TransformStrategy::Error,
            streaming: TransformStrategy::Strip,
            images: TransformStrategy::Strip,
            audio: TransformStrategy::Strip,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: MappingRuleSet = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, back);
        assert!(!rule.is_identity());
    }

    #[test]
    fn rule_set_non_identity() {
        let rule = MappingRuleSet {
            system_messages: TransformStrategy::Emulate,
            tool_calls: TransformStrategy::Passthrough,
            streaming: TransformStrategy::Passthrough,
            images: TransformStrategy::Passthrough,
            audio: TransformStrategy::Passthrough,
        };
        assert!(!rule.is_identity());
    }

    // ── FeatureSupport ─────────────────────────────────────────────

    #[test]
    fn feature_support_native() {
        let s = FeatureSupport::Native;
        assert!(s.is_native());
        assert!(!s.is_unsupported());
    }

    #[test]
    fn feature_support_emulated() {
        let s = FeatureSupport::Emulated {
            strategy: "fold into user".into(),
        };
        assert!(!s.is_native());
        assert!(!s.is_unsupported());
    }

    #[test]
    fn feature_support_unsupported() {
        let s = FeatureSupport::Unsupported;
        assert!(s.is_unsupported());
        assert!(!s.is_native());
    }

    #[test]
    fn feature_support_serde_roundtrip() {
        let variants = vec![
            FeatureSupport::Native,
            FeatureSupport::Emulated {
                strategy: "approx".into(),
            },
            FeatureSupport::Unsupported,
        ];
        for v in variants {
            let json = serde_json::to_string(&v).unwrap();
            let back: FeatureSupport = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back);
        }
    }

    // ── FeatureFidelity ────────────────────────────────────────────

    #[test]
    fn feature_fidelity_serde_roundtrip() {
        let ff = FeatureFidelity {
            feature: "tool_calls".into(),
            support: FeatureSupport::Native,
            notes: "natively supported".into(),
        };
        let json = serde_json::to_string(&ff).unwrap();
        let back: FeatureFidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(back.feature, "tool_calls");
    }

    #[test]
    fn feature_fidelity_emulated_roundtrip() {
        let ff = FeatureFidelity {
            feature: "images".into(),
            support: FeatureSupport::Emulated {
                strategy: "placeholder".into(),
            },
            notes: "replaced with placeholders".into(),
        };
        let json = serde_json::to_string(&ff).unwrap();
        let back: FeatureFidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(back.feature, "images");
    }

    // ── ProjectionMatrix ───────────────────────────────────────────

    #[test]
    fn matrix_new_is_empty() {
        let m = ProjectionMatrix::new();
        assert!(m.registered_pairs().is_empty());
    }

    #[test]
    fn matrix_with_defaults_has_all_pairs() {
        let m = ProjectionMatrix::with_defaults();
        let pairs = m.registered_pairs();
        // 7 dialects × 7 = 49 pairs
        assert_eq!(pairs.len(), 49);
    }

    #[test]
    fn matrix_register_and_get() {
        let mut m = ProjectionMatrix::new();
        m.register_rule(
            ProjectionDialect::OpenAi,
            ProjectionDialect::Claude,
            MappingRuleSet::identity(),
        );
        assert!(m
            .get_rule(ProjectionDialect::OpenAi, ProjectionDialect::Claude)
            .is_some());
        assert!(m
            .get_rule(ProjectionDialect::Claude, ProjectionDialect::OpenAi)
            .is_none());
    }

    #[test]
    fn matrix_default_identity_pairs() {
        let m = ProjectionMatrix::with_defaults();
        for &d in ProjectionDialect::all() {
            let rule = m.get_rule(d, d).unwrap();
            assert!(rule.is_identity(), "identity rule expected for {d} -> {d}");
        }
    }

    #[test]
    fn matrix_generic_pairs_are_identity() {
        let m = ProjectionMatrix::with_defaults();
        for &d in ProjectionDialect::all() {
            let rule = m.get_rule(ProjectionDialect::Generic, d).unwrap();
            assert!(rule.is_identity());
            let rule = m.get_rule(d, ProjectionDialect::Generic).unwrap();
            assert!(rule.is_identity());
        }
    }

    // ── Identity projections ───────────────────────────────────────

    #[test]
    fn identity_projection_openai() {
        let req = simple_request();
        let result =
            project_request(ProjectionDialect::OpenAi, ProjectionDialect::OpenAi, &req).unwrap();
        assert_eq!(result, req);
    }

    #[test]
    fn identity_projection_claude() {
        let req = simple_request();
        let result =
            project_request(ProjectionDialect::Claude, ProjectionDialect::Claude, &req).unwrap();
        assert_eq!(result, req);
    }

    #[test]
    fn identity_projection_gemini() {
        let req = simple_request();
        let result =
            project_request(ProjectionDialect::Gemini, ProjectionDialect::Gemini, &req).unwrap();
        assert_eq!(result, req);
    }

    #[test]
    fn identity_projection_codex() {
        let req = simple_request();
        let result =
            project_request(ProjectionDialect::Codex, ProjectionDialect::Codex, &req).unwrap();
        assert_eq!(result, req);
    }

    #[test]
    fn identity_projection_kimi() {
        let req = simple_request();
        let result =
            project_request(ProjectionDialect::Kimi, ProjectionDialect::Kimi, &req).unwrap();
        assert_eq!(result, req);
    }

    #[test]
    fn identity_projection_copilot() {
        let req = simple_request();
        let result =
            project_request(ProjectionDialect::Copilot, ProjectionDialect::Copilot, &req).unwrap();
        assert_eq!(result, req);
    }

    #[test]
    fn identity_projection_generic() {
        let req = simple_request();
        let result =
            project_request(ProjectionDialect::Generic, ProjectionDialect::Generic, &req).unwrap();
        assert_eq!(result, req);
    }

    // ── Cross-dialect request projections ──────────────────────────

    #[test]
    fn openai_to_claude_passthrough() {
        let req = request_with_system();
        let result =
            project_request(ProjectionDialect::OpenAi, ProjectionDialect::Claude, &req).unwrap();
        assert_eq!(result, req);
    }

    #[test]
    fn openai_to_gemini_passthrough() {
        let req = request_with_system();
        let result =
            project_request(ProjectionDialect::OpenAi, ProjectionDialect::Gemini, &req).unwrap();
        assert_eq!(result, req);
    }

    #[test]
    fn openai_to_codex_emulates_system() {
        let req = request_with_system();
        let result =
            project_request(ProjectionDialect::OpenAi, ProjectionDialect::Codex, &req).unwrap();
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[0].role, IrRole::User);
        assert!(result.messages[0].text_content().starts_with("[System]"));
    }

    #[test]
    fn openai_to_codex_strips_streaming() {
        let req = streaming_request();
        let result =
            project_request(ProjectionDialect::OpenAi, ProjectionDialect::Codex, &req).unwrap();
        assert!(!result.stream.enabled);
    }

    #[test]
    fn openai_to_codex_errors_on_tools() {
        let req = request_with_tools();
        let result = project_request(ProjectionDialect::OpenAi, ProjectionDialect::Codex, &req);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("tool_calls"));
    }

    #[test]
    fn openai_to_kimi_strips_images() {
        let req = request_with_images();
        let result =
            project_request(ProjectionDialect::OpenAi, ProjectionDialect::Kimi, &req).unwrap();
        assert_eq!(result.messages[0].content.len(), 1);
    }

    #[test]
    fn openai_to_copilot_strips_images() {
        let req = request_with_images();
        let result =
            project_request(ProjectionDialect::OpenAi, ProjectionDialect::Copilot, &req).unwrap();
        assert_eq!(result.messages[0].content.len(), 1);
    }

    #[test]
    fn claude_to_codex_errors_on_tools() {
        let req = request_with_tools();
        let result = project_request(ProjectionDialect::Claude, ProjectionDialect::Codex, &req);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("tool_calls"));
    }

    // ── Cross-dialect response projections ─────────────────────────

    #[test]
    fn response_identity_projection() {
        let resp = simple_response();
        let result =
            project_response(ProjectionDialect::OpenAi, ProjectionDialect::OpenAi, &resp).unwrap();
        assert_eq!(result, resp);
    }

    #[test]
    fn response_openai_to_claude_passthrough() {
        let resp = simple_response();
        let result =
            project_response(ProjectionDialect::OpenAi, ProjectionDialect::Claude, &resp).unwrap();
        assert_eq!(result, resp);
    }

    #[test]
    fn response_to_codex_errors_on_tool_calls() {
        let resp = response_with_tool_calls();
        let result = project_response(ProjectionDialect::OpenAi, ProjectionDialect::Codex, &resp);
        assert!(result.is_err());
    }

    #[test]
    fn response_generic_passthrough() {
        let resp = response_with_tool_calls();
        let result =
            project_response(ProjectionDialect::OpenAi, ProjectionDialect::Generic, &resp).unwrap();
        assert_eq!(result, resp);
    }

    #[test]
    fn response_codex_to_openai_passthrough() {
        let resp = simple_response();
        let result =
            project_response(ProjectionDialect::Codex, ProjectionDialect::OpenAi, &resp).unwrap();
        assert_eq!(result, resp);
    }

    // ── Error on unsupported features ──────────────────────────────

    #[test]
    fn error_tools_to_codex() {
        let req = request_with_tools();
        let err =
            project_request(ProjectionDialect::Claude, ProjectionDialect::Codex, &req).unwrap_err();
        assert!(err.to_string().contains("tool_calls"));
    }

    #[test]
    fn no_error_simple_request_to_codex() {
        let req = simple_request();
        let result = project_request(ProjectionDialect::OpenAi, ProjectionDialect::Codex, &req);
        assert!(result.is_ok());
    }

    #[test]
    fn error_on_missing_rule() {
        let m = ProjectionMatrix::new();
        let req = simple_request();
        let result = m.project_request(ProjectionDialect::OpenAi, ProjectionDialect::Claude, &req);
        assert!(result.is_err());
    }

    #[test]
    fn error_streaming_to_codex_not_if_disabled() {
        let req = simple_request(); // streaming disabled by default
        let result = project_request(ProjectionDialect::OpenAi, ProjectionDialect::Codex, &req);
        assert!(result.is_ok());
    }

    #[test]
    fn error_tool_content_parts_to_codex() {
        // Request with ToolUse content part (not via tools vec)
        let req = IrChatRequest::new(
            "gpt-4o",
            vec![IrMessage::new(
                IrRole::Assistant,
                vec![IrContentPart::ToolUse {
                    id: "call_1".into(),
                    name: "search".into(),
                    arguments: serde_json::json!({}),
                }],
            )],
        );
        let result = project_request(ProjectionDialect::OpenAi, ProjectionDialect::Codex, &req);
        assert!(result.is_err());
    }

    // ── Fidelity reporting ─────────────────────────────────────────

    #[test]
    fn fidelity_same_dialect_all_native() {
        let fm = FidelityMatrix::with_defaults();
        for &d in ProjectionDialect::all() {
            assert!(
                fm.is_fully_native(d, d),
                "expected all native for {d} -> {d}"
            );
        }
    }

    #[test]
    fn fidelity_generic_all_native() {
        let fm = FidelityMatrix::with_defaults();
        for &d in ProjectionDialect::all() {
            assert!(fm.is_fully_native(ProjectionDialect::Generic, d));
            assert!(fm.is_fully_native(d, ProjectionDialect::Generic));
        }
    }

    #[test]
    fn fidelity_openai_to_codex_has_unsupported() {
        let fm = FidelityMatrix::with_defaults();
        assert!(fm.has_unsupported(ProjectionDialect::OpenAi, ProjectionDialect::Codex));
    }

    #[test]
    fn fidelity_openai_to_claude_fully_native() {
        let fm = FidelityMatrix::with_defaults();
        assert!(fm.is_fully_native(ProjectionDialect::OpenAi, ProjectionDialect::Claude));
    }

    #[test]
    fn fidelity_lookup_returns_five_entries() {
        let fm = FidelityMatrix::with_defaults();
        let entries = fm.lookup(ProjectionDialect::OpenAi, ProjectionDialect::Codex);
        assert_eq!(entries.len(), 5);
    }

    #[test]
    fn fidelity_openai_to_kimi_has_unsupported() {
        let fm = FidelityMatrix::with_defaults();
        assert!(fm.has_unsupported(ProjectionDialect::OpenAi, ProjectionDialect::Kimi));
    }

    #[test]
    fn fidelity_empty_matrix_lookup() {
        let fm = FidelityMatrix::new();
        let entries = fm.lookup(ProjectionDialect::OpenAi, ProjectionDialect::Claude);
        assert!(entries.is_empty());
    }

    // ── Serde roundtrips for all new types ─────────────────────────

    #[test]
    fn projection_dialect_all_serde_roundtrip() {
        for &d in ProjectionDialect::all() {
            let json = serde_json::to_string(&d).unwrap();
            let back: ProjectionDialect = serde_json::from_str(&json).unwrap();
            assert_eq!(d, back);
        }
    }

    #[test]
    fn mapping_rule_set_full_roundtrip() {
        let rule = MappingRuleSet {
            system_messages: TransformStrategy::Emulate,
            tool_calls: TransformStrategy::Error,
            streaming: TransformStrategy::Strip,
            images: TransformStrategy::Passthrough,
            audio: TransformStrategy::Strip,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: MappingRuleSet = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, back);
    }

    #[test]
    fn feature_fidelity_full_roundtrip() {
        let ff = FeatureFidelity {
            feature: "images".into(),
            support: FeatureSupport::Emulated {
                strategy: "placeholder".into(),
            },
            notes: "replaced with placeholders".into(),
        };
        let json = serde_json::to_string(&ff).unwrap();
        let back: FeatureFidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(back.feature, "images");
    }

    #[test]
    fn feature_support_all_variants_roundtrip() {
        let native = FeatureSupport::Native;
        let emulated = FeatureSupport::Emulated {
            strategy: "text prefix".into(),
        };
        let unsupported = FeatureSupport::Unsupported;
        for v in [native, emulated, unsupported] {
            let json = serde_json::to_string(&v).unwrap();
            let back: FeatureSupport = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back);
        }
    }

    #[test]
    fn transform_strategy_all_variants_roundtrip() {
        for s in [
            TransformStrategy::Passthrough,
            TransformStrategy::Emulate,
            TransformStrategy::Strip,
            TransformStrategy::Error,
        ] {
            let json = serde_json::to_string(&s).unwrap();
            let back: TransformStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(s, back);
        }
    }

    // ── Edge cases ─────────────────────────────────────────────────

    #[test]
    fn empty_messages_passthrough() {
        let req = IrChatRequest::new("gpt-4o", vec![]);
        let result =
            project_request(ProjectionDialect::OpenAi, ProjectionDialect::Codex, &req).unwrap();
        assert!(result.messages.is_empty());
    }

    #[test]
    fn codex_to_openai_passthrough() {
        let req = simple_request();
        let result =
            project_request(ProjectionDialect::Codex, ProjectionDialect::OpenAi, &req).unwrap();
        assert_eq!(result, req);
    }

    #[test]
    fn generic_to_any_is_identity() {
        let req = request_with_system();
        for &d in ProjectionDialect::all() {
            let result = project_request(ProjectionDialect::Generic, d, &req).unwrap();
            assert_eq!(result, req);
        }
    }

    #[test]
    fn any_to_generic_is_identity() {
        let req = request_with_tools();
        for &d in ProjectionDialect::all() {
            let result = project_request(d, ProjectionDialect::Generic, &req).unwrap();
            assert_eq!(result, req);
        }
    }

    #[test]
    fn strip_audio_content_to_codex() {
        let req = request_with_audio();
        let result =
            project_request(ProjectionDialect::OpenAi, ProjectionDialect::Codex, &req).unwrap();
        assert!(result.messages[0].content.is_empty());
    }

    #[test]
    fn strip_images_preserves_text() {
        let req = request_with_images();
        let result =
            project_request(ProjectionDialect::OpenAi, ProjectionDialect::Kimi, &req).unwrap();
        assert_eq!(result.messages[0].content.len(), 1);
        assert!(result.messages[0].content[0].is_text());
        assert_eq!(
            result.messages[0].content[0].as_text(),
            Some("What is this?")
        );
    }

    #[test]
    fn matrix_codex_target_rules() {
        let m = ProjectionMatrix::with_defaults();
        let rule = m
            .get_rule(ProjectionDialect::OpenAi, ProjectionDialect::Codex)
            .unwrap();
        assert_eq!(rule.system_messages, TransformStrategy::Emulate);
        assert_eq!(rule.tool_calls, TransformStrategy::Error);
        assert_eq!(rule.streaming, TransformStrategy::Strip);
        assert_eq!(rule.images, TransformStrategy::Strip);
        assert_eq!(rule.audio, TransformStrategy::Strip);
    }

    #[test]
    fn matrix_kimi_target_rules() {
        let m = ProjectionMatrix::with_defaults();
        let rule = m
            .get_rule(ProjectionDialect::OpenAi, ProjectionDialect::Kimi)
            .unwrap();
        assert_eq!(rule.system_messages, TransformStrategy::Passthrough);
        assert_eq!(rule.tool_calls, TransformStrategy::Passthrough);
        assert_eq!(rule.images, TransformStrategy::Strip);
    }

    #[test]
    fn matrix_copilot_target_rules() {
        let m = ProjectionMatrix::with_defaults();
        let rule = m
            .get_rule(ProjectionDialect::Gemini, ProjectionDialect::Copilot)
            .unwrap();
        assert_eq!(rule.system_messages, TransformStrategy::Passthrough);
        assert_eq!(rule.tool_calls, TransformStrategy::Passthrough);
        assert_eq!(rule.streaming, TransformStrategy::Passthrough);
        assert_eq!(rule.images, TransformStrategy::Strip);
        assert_eq!(rule.audio, TransformStrategy::Strip);
    }

    #[test]
    fn response_strip_tool_calls_for_kimi() {
        // Kimi supports tools, so this should passthrough
        let resp = response_with_tool_calls();
        let result =
            project_response(ProjectionDialect::OpenAi, ProjectionDialect::Kimi, &resp).unwrap();
        assert!(result.has_tool_calls());
    }

    #[test]
    fn matrix_default_trait() {
        let m = ProjectionMatrix::default();
        assert!(m.registered_pairs().is_empty());
    }

    #[test]
    fn fidelity_register_custom() {
        let mut fm = FidelityMatrix::new();
        fm.register(
            ProjectionDialect::OpenAi,
            ProjectionDialect::Claude,
            vec![FeatureFidelity {
                feature: "custom".into(),
                support: FeatureSupport::Native,
                notes: "custom entry".into(),
            }],
        );
        assert_eq!(
            fm.lookup(ProjectionDialect::OpenAi, ProjectionDialect::Claude)
                .len(),
            1
        );
    }

    #[test]
    fn emulate_system_preserves_user_messages() {
        let req = request_with_system();
        let result =
            project_request(ProjectionDialect::OpenAi, ProjectionDialect::Codex, &req).unwrap();
        assert_eq!(result.messages.len(), 2);
        // Second message (original user) is unchanged
        assert_eq!(result.messages[1].role, IrRole::User);
        assert_eq!(result.messages[1].text_content(), "Hello");
    }

    #[test]
    fn strip_audio_to_kimi() {
        let req = request_with_audio();
        let result =
            project_request(ProjectionDialect::OpenAi, ProjectionDialect::Kimi, &req).unwrap();
        assert!(result.messages[0].content.is_empty());
    }
}
