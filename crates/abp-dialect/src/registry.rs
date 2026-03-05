// SPDX-License-Identifier: MIT OR Apache-2.0
//! Dialect registry for parser/serializer lookup.
//!
//! The `DialectRegistry` stores `DialectEntry` records — one per
//! registered dialect — each carrying a parser function that lifts raw
//! JSON into `IrRequest` and a serializer function that lowers
//! `IrRequest` back to raw JSON.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
    IrToolDefinition, IrUsage,
};
use crate::Dialect;

// ── Error type ──────────────────────────────────────────────────────────

/// Errors that may occur during dialect parsing or serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectError {
    /// Which dialect produced the error.
    pub dialect: Dialect,
    /// Human-readable description.
    pub message: String,
}

impl std::fmt::Display for DialectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.dialect.label(), self.message)
    }
}

impl std::error::Error for DialectError {}

// ── Function signatures ─────────────────────────────────────────────────

/// Parses a raw JSON request into an [`IrRequest`].
pub type ParseFn = fn(&Value) -> Result<IrRequest, DialectError>;

/// Serializes an [`IrRequest`] into raw JSON.
pub type SerializeFn = fn(&IrRequest) -> Result<Value, DialectError>;

// ── DialectEntry ────────────────────────────────────────────────────────

/// Metadata and codec functions for a single dialect.
#[derive(Clone)]
pub struct DialectEntry {
    /// Dialect tag.
    pub dialect: Dialect,
    /// Canonical name (e.g. `"openai"`).
    pub name: &'static str,
    /// Dialect version string.
    pub version: &'static str,
    /// Parse raw JSON → IR.
    pub parser: ParseFn,
    /// Serialize IR → raw JSON.
    pub serializer: SerializeFn,
}

impl std::fmt::Debug for DialectEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialectEntry")
            .field("dialect", &self.dialect)
            .field("name", &self.name)
            .field("version", &self.version)
            .finish()
    }
}

// ── DialectRegistry ─────────────────────────────────────────────────────

/// Central registry of dialect parsers and serializers.
///
/// Use [`DialectRegistry::with_builtins()`] to get a registry pre-populated
/// with all known dialects.
#[derive(Debug, Clone, Default)]
pub struct DialectRegistry {
    entries: BTreeMap<Dialect, DialectEntry>,
}

impl DialectRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry pre-populated with all built-in dialects.
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut r = Self::new();
        r.register(openai_entry());
        r.register(claude_entry());
        r.register(gemini_entry());
        r.register(codex_entry());
        r.register(kimi_entry());
        r.register(copilot_entry());
        r
    }

    /// Register a dialect entry, replacing any previous entry for that dialect.
    pub fn register(&mut self, entry: DialectEntry) {
        self.entries.insert(entry.dialect, entry);
    }

    /// Look up a dialect entry.
    #[must_use]
    pub fn get(&self, dialect: Dialect) -> Option<&DialectEntry> {
        self.entries.get(&dialect)
    }

    /// List all registered dialects in deterministic order.
    #[must_use]
    pub fn list_dialects(&self) -> Vec<Dialect> {
        self.entries.keys().copied().collect()
    }

    /// Returns `true` if both `from` and `to` dialects are registered.
    #[must_use]
    pub fn supports_pair(&self, from: Dialect, to: Dialect) -> bool {
        self.entries.contains_key(&from) && self.entries.contains_key(&to)
    }

    /// Parse raw JSON using the parser registered for `dialect`.
    pub fn parse(&self, dialect: Dialect, value: &Value) -> Result<IrRequest, DialectError> {
        let entry = self.entries.get(&dialect).ok_or_else(|| DialectError {
            dialect,
            message: format!("dialect {:?} not registered", dialect),
        })?;
        (entry.parser)(value)
    }

    /// Serialize an [`IrRequest`] using the serializer registered for `dialect`.
    pub fn serialize(&self, dialect: Dialect, ir: &IrRequest) -> Result<Value, DialectError> {
        let entry = self.entries.get(&dialect).ok_or_else(|| DialectError {
            dialect,
            message: format!("dialect {:?} not registered", dialect),
        })?;
        (entry.serializer)(ir)
    }

    /// Number of registered dialects.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Auto-detect the dialect of a raw JSON request and parse it into IR.
    ///
    /// Combines [`DialectDetector`](crate::DialectDetector) with the
    /// registry's parser to detect and parse in one step. Returns the
    /// detection result alongside the parsed IR, or an error if detection
    /// fails or the detected dialect is not registered.
    pub fn detect_and_parse(
        &self,
        value: &Value,
    ) -> Result<(crate::DetectionResult, IrRequest), DialectError> {
        let detector = crate::DialectDetector::new();
        let detection = detector.detect(value).ok_or_else(|| DialectError {
            dialect: Dialect::OpenAi,
            message: "could not detect dialect from request JSON".into(),
        })?;
        let ir = self.parse(detection.dialect, value)?;
        Ok((detection, ir))
    }

    /// Validate a raw JSON request against the given dialect using the
    /// [`validate`](crate::validate) module's `RequestValidator`.
    #[must_use]
    pub fn validate_request(
        &self,
        dialect: Dialect,
        value: &Value,
    ) -> crate::validate::ValidationResult {
        crate::validate::RequestValidator::new().validate(dialect, value)
    }

    /// Return the [`DialectFeatures`] for a registered dialect.
    #[must_use]
    pub fn features(&self, dialect: Dialect) -> Option<DialectFeatures> {
        self.entries
            .get(&dialect)
            .map(|_| builtin_features(dialect))
    }

    /// Return the [`DialectVersionInfo`] for a registered dialect.
    #[must_use]
    pub fn version_info(&self, dialect: Dialect) -> Option<DialectVersionInfo> {
        self.entries.get(&dialect).map(|e| DialectVersionInfo {
            dialect,
            api_version: e.version,
            label: dialect.label(),
        })
    }

    /// Compare two dialects and return a [`DialectComparison`] describing
    /// shared and divergent feature support.
    #[must_use]
    pub fn compare(&self, a: Dialect, b: Dialect) -> Option<DialectComparison> {
        let fa = self.features(a)?;
        let fb = self.features(b)?;
        Some(DialectComparison::from_features(a, fa, b, fb))
    }
}

// ── Feature matrix ──────────────────────────────────────────────────────

/// Describes which capabilities a dialect supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialectFeatures {
    /// Supports streaming responses.
    pub streaming: bool,
    /// Supports tool / function calling.
    pub tool_use: bool,
    /// Supports vision / image inputs.
    pub vision: bool,
    /// Supports a separate system prompt field.
    pub system_prompt: bool,
    /// Supports multi-turn conversation history.
    pub multi_turn: bool,
    /// Supports structured JSON output mode.
    pub json_mode: bool,
}

impl DialectFeatures {
    /// Returns the list of feature names this dialect supports.
    #[must_use]
    pub fn supported_names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.streaming {
            v.push("streaming");
        }
        if self.tool_use {
            v.push("tool_use");
        }
        if self.vision {
            v.push("vision");
        }
        if self.system_prompt {
            v.push("system_prompt");
        }
        if self.multi_turn {
            v.push("multi_turn");
        }
        if self.json_mode {
            v.push("json_mode");
        }
        v
    }
}

/// Returns the built-in feature set for a dialect.
#[must_use]
pub fn builtin_features(dialect: Dialect) -> DialectFeatures {
    match dialect {
        Dialect::OpenAi => DialectFeatures {
            streaming: true,
            tool_use: true,
            vision: true,
            system_prompt: true,
            multi_turn: true,
            json_mode: true,
        },
        Dialect::Claude => DialectFeatures {
            streaming: true,
            tool_use: true,
            vision: true,
            system_prompt: true,
            multi_turn: true,
            json_mode: false,
        },
        Dialect::Gemini => DialectFeatures {
            streaming: true,
            tool_use: true,
            vision: true,
            system_prompt: true,
            multi_turn: true,
            json_mode: true,
        },
        Dialect::Codex => DialectFeatures {
            streaming: true,
            tool_use: true,
            vision: false,
            system_prompt: true,
            multi_turn: false,
            json_mode: false,
        },
        Dialect::Kimi => DialectFeatures {
            streaming: true,
            tool_use: false,
            vision: false,
            system_prompt: true,
            multi_turn: true,
            json_mode: false,
        },
        Dialect::Copilot => DialectFeatures {
            streaming: true,
            tool_use: true,
            vision: false,
            system_prompt: true,
            multi_turn: true,
            json_mode: false,
        },
    }
}

// ── Version tracking ────────────────────────────────────────────────────

/// Structured version information for a dialect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialectVersionInfo {
    /// The dialect tag.
    pub dialect: Dialect,
    /// API version string (e.g. `"v1"`, `"2023-06-01"`).
    pub api_version: &'static str,
    /// Human-readable label.
    pub label: &'static str,
}

// ── Dialect comparison ──────────────────────────────────────────────────

/// Result of comparing two dialects' feature sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectComparison {
    /// First dialect.
    pub a: Dialect,
    /// Second dialect.
    pub b: Dialect,
    /// Features supported by both.
    pub shared: Vec<&'static str>,
    /// Features supported only by `a`.
    pub only_a: Vec<&'static str>,
    /// Features supported only by `b`.
    pub only_b: Vec<&'static str>,
}

impl DialectComparison {
    fn from_features(a: Dialect, fa: DialectFeatures, b: Dialect, fb: DialectFeatures) -> Self {
        let set_a = fa.supported_names();
        let set_b = fb.supported_names();
        let shared: Vec<&'static str> = set_a
            .iter()
            .filter(|f| set_b.contains(f))
            .copied()
            .collect();
        let only_a: Vec<&'static str> = set_a
            .iter()
            .filter(|f| !set_b.contains(f))
            .copied()
            .collect();
        let only_b: Vec<&'static str> = set_b
            .iter()
            .filter(|f| !set_a.contains(f))
            .copied()
            .collect();
        Self {
            a,
            b,
            shared,
            only_a,
            only_b,
        }
    }

    /// Returns `true` when both dialects support exactly the same features.
    #[must_use]
    pub fn is_fully_compatible(&self) -> bool {
        self.only_a.is_empty() && self.only_b.is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Built-in dialect entries
// ═══════════════════════════════════════════════════════════════════════

// ── OpenAI ──────────────────────────────────────────────────────────────

fn openai_entry() -> DialectEntry {
    DialectEntry {
        dialect: Dialect::OpenAi,
        name: "openai",
        version: "v1",
        parser: parse_openai,
        serializer: serialize_openai,
    }
}

fn parse_openai(value: &Value) -> Result<IrRequest, DialectError> {
    let obj = value.as_object().ok_or_else(|| DialectError {
        dialect: Dialect::OpenAi,
        message: "expected JSON object".into(),
    })?;

    let model = obj.get("model").and_then(Value::as_str).map(String::from);
    let mut system_prompt = None;
    let mut messages = Vec::new();

    if let Some(Value::Array(msgs)) = obj.get("messages") {
        for m in msgs {
            let role_str = m.get("role").and_then(Value::as_str).unwrap_or("user");
            let role = match role_str {
                "system" => IrRole::System,
                "assistant" => IrRole::Assistant,
                "tool" => IrRole::Tool,
                _ => IrRole::User,
            };

            let mut blocks = Vec::new();

            if let Some(text) = m.get("content").and_then(Value::as_str) {
                if role == IrRole::System && system_prompt.is_none() {
                    system_prompt = Some(text.to_string());
                }
                if !text.is_empty() {
                    blocks.push(IrContentBlock::Text {
                        text: text.to_string(),
                    });
                }
            }

            if let Some(Value::Array(tcs)) = m.get("tool_calls") {
                for tc in tcs {
                    let id = tc
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let func = tc.get("function").cloned().unwrap_or(Value::Null);
                    let name = func
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let args_str = func
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    let input: Value =
                        serde_json::from_str(args_str).unwrap_or(Value::String(args_str.into()));
                    blocks.push(IrContentBlock::ToolCall { id, name, input });
                }
            }

            if role == IrRole::Tool {
                if let Some(tcid) = m.get("tool_call_id").and_then(Value::as_str) {
                    let inner = blocks.clone();
                    blocks = vec![IrContentBlock::ToolResult {
                        tool_call_id: tcid.to_string(),
                        content: inner,
                        is_error: false,
                    }];
                }
            }

            messages.push(IrMessage::new(role, blocks));
        }
    }

    let tools = parse_openai_tools(obj);
    let config = parse_openai_config(obj);

    Ok(IrRequest {
        model,
        system_prompt,
        messages,
        tools,
        config,
        metadata: BTreeMap::new(),
    })
}

fn parse_openai_tools(obj: &serde_json::Map<String, Value>) -> Vec<IrToolDefinition> {
    let Some(Value::Array(tools)) = obj.get("tools") else {
        return Vec::new();
    };
    tools
        .iter()
        .filter_map(|t| {
            let func = t.get("function")?;
            Some(IrToolDefinition {
                name: func.get("name")?.as_str()?.to_string(),
                description: func
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                parameters: func
                    .get("parameters")
                    .cloned()
                    .unwrap_or(Value::Object(Default::default())),
            })
        })
        .collect()
}

fn parse_openai_config(obj: &serde_json::Map<String, Value>) -> IrGenerationConfig {
    IrGenerationConfig {
        max_tokens: obj
            .get("max_tokens")
            .and_then(Value::as_u64)
            .or_else(|| obj.get("max_completion_tokens").and_then(Value::as_u64)),
        temperature: obj.get("temperature").and_then(Value::as_f64),
        top_p: obj.get("top_p").and_then(Value::as_f64),
        top_k: None,
        stop_sequences: match obj.get("stop") {
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
            Some(Value::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        },
        extra: BTreeMap::new(),
    }
}

fn serialize_openai(ir: &IrRequest) -> Result<Value, DialectError> {
    let mut obj = serde_json::Map::new();

    if let Some(model) = &ir.model {
        obj.insert("model".into(), Value::String(model.clone()));
    }

    let mut messages = Vec::new();
    if let Some(sp) = &ir.system_prompt {
        let already_has_system = ir.messages.iter().any(|m| m.role == IrRole::System);
        if !already_has_system {
            messages.push(serde_json::json!({
                "role": "system",
                "content": sp
            }));
        }
    }

    for msg in &ir.messages {
        messages.push(serialize_openai_message(msg));
    }
    obj.insert("messages".into(), Value::Array(messages));

    if !ir.tools.is_empty() {
        let tools: Vec<Value> = ir
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect();
        obj.insert("tools".into(), Value::Array(tools));
    }

    if let Some(mt) = ir.config.max_tokens {
        obj.insert("max_tokens".into(), Value::Number(mt.into()));
    }
    if let Some(t) = ir.config.temperature {
        obj.insert(
            "temperature".into(),
            serde_json::Number::from_f64(t)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
    }
    if let Some(tp) = ir.config.top_p {
        obj.insert(
            "top_p".into(),
            serde_json::Number::from_f64(tp)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
    }
    if !ir.config.stop_sequences.is_empty() {
        let stops: Vec<Value> = ir
            .config
            .stop_sequences
            .iter()
            .map(|s| Value::String(s.clone()))
            .collect();
        obj.insert("stop".into(), Value::Array(stops));
    }

    Ok(Value::Object(obj))
}

fn serialize_openai_message(msg: &IrMessage) -> Value {
    let role = match msg.role {
        IrRole::System => "system",
        IrRole::User => "user",
        IrRole::Assistant => "assistant",
        IrRole::Tool => "tool",
    };

    let mut m = serde_json::Map::new();
    m.insert("role".into(), Value::String(role.into()));

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut tool_call_id = None;

    for block in &msg.content {
        match block {
            IrContentBlock::Text { text } => text_parts.push(text.as_str()),
            IrContentBlock::ToolCall { id, name, input } => {
                tool_calls.push(serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": serde_json::to_string(input).unwrap_or_default()
                    }
                }));
            }
            IrContentBlock::ToolResult {
                tool_call_id: tcid,
                content,
                ..
            } => {
                tool_call_id = Some(tcid.clone());
                for inner in content {
                    if let IrContentBlock::Text { text } = inner {
                        text_parts.push(text.as_str());
                    }
                }
            }
            IrContentBlock::Thinking { text } => text_parts.push(text.as_str()),
            _ => {}
        }
    }

    if !text_parts.is_empty() {
        m.insert("content".into(), Value::String(text_parts.join("")));
    } else {
        m.insert("content".into(), Value::Null);
    }

    if !tool_calls.is_empty() {
        m.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    if let Some(tcid) = tool_call_id {
        m.insert("tool_call_id".into(), Value::String(tcid));
    }

    Value::Object(m)
}

// ── Claude ──────────────────────────────────────────────────────────────

fn claude_entry() -> DialectEntry {
    DialectEntry {
        dialect: Dialect::Claude,
        name: "claude",
        version: "v1",
        parser: parse_claude,
        serializer: serialize_claude,
    }
}

fn parse_claude(value: &Value) -> Result<IrRequest, DialectError> {
    let obj = value.as_object().ok_or_else(|| DialectError {
        dialect: Dialect::Claude,
        message: "expected JSON object".into(),
    })?;

    let model = obj.get("model").and_then(Value::as_str).map(String::from);
    let system_prompt = obj.get("system").and_then(Value::as_str).map(String::from);

    let mut messages = Vec::new();
    if let Some(Value::Array(msgs)) = obj.get("messages") {
        for m in msgs {
            let role_str = m.get("role").and_then(Value::as_str).unwrap_or("user");
            let role = match role_str {
                "assistant" => IrRole::Assistant,
                _ => IrRole::User,
            };

            let blocks = match m.get("content") {
                Some(Value::String(s)) => vec![IrContentBlock::Text { text: s.clone() }],
                Some(Value::Array(arr)) => parse_claude_content_blocks(arr),
                _ => Vec::new(),
            };
            messages.push(IrMessage::new(role, blocks));
        }
    }

    let tools = parse_claude_tools(obj);
    let config = IrGenerationConfig {
        max_tokens: obj.get("max_tokens").and_then(Value::as_u64),
        temperature: obj.get("temperature").and_then(Value::as_f64),
        top_p: obj.get("top_p").and_then(Value::as_f64),
        top_k: obj.get("top_k").and_then(Value::as_u64).map(|v| v as u32),
        stop_sequences: match obj.get("stop_sequences") {
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
            _ => Vec::new(),
        },
        extra: BTreeMap::new(),
    };

    Ok(IrRequest {
        model,
        system_prompt,
        messages,
        tools,
        config,
        metadata: BTreeMap::new(),
    })
}

fn parse_claude_content_blocks(arr: &[Value]) -> Vec<IrContentBlock> {
    arr.iter()
        .filter_map(|b| {
            let t = b.get("type")?.as_str()?;
            match t {
                "text" => Some(IrContentBlock::Text {
                    text: b.get("text")?.as_str()?.to_string(),
                }),
                "tool_use" => Some(IrContentBlock::ToolCall {
                    id: b.get("id")?.as_str()?.to_string(),
                    name: b.get("name")?.as_str()?.to_string(),
                    input: b.get("input").cloned().unwrap_or(Value::Null),
                }),
                "tool_result" => {
                    let tcid = b.get("tool_use_id")?.as_str()?.to_string();
                    let is_error = b.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                    let inner = match b.get("content") {
                        Some(Value::String(s)) => vec![IrContentBlock::Text { text: s.clone() }],
                        Some(Value::Array(inner_arr)) => parse_claude_content_blocks(inner_arr),
                        _ => Vec::new(),
                    };
                    Some(IrContentBlock::ToolResult {
                        tool_call_id: tcid,
                        content: inner,
                        is_error,
                    })
                }
                "thinking" => Some(IrContentBlock::Thinking {
                    text: b
                        .get("thinking")
                        .or_else(|| b.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                }),
                "image" => {
                    let src = b.get("source")?;
                    Some(IrContentBlock::Image {
                        media_type: src
                            .get("media_type")
                            .and_then(Value::as_str)
                            .unwrap_or("image/png")
                            .to_string(),
                        data: src
                            .get("data")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    })
                }
                _ => None,
            }
        })
        .collect()
}

fn parse_claude_tools(obj: &serde_json::Map<String, Value>) -> Vec<IrToolDefinition> {
    let Some(Value::Array(tools)) = obj.get("tools") else {
        return Vec::new();
    };
    tools
        .iter()
        .filter_map(|t| {
            Some(IrToolDefinition {
                name: t.get("name")?.as_str()?.to_string(),
                description: t
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                parameters: t
                    .get("input_schema")
                    .cloned()
                    .unwrap_or(Value::Object(Default::default())),
            })
        })
        .collect()
}

fn serialize_claude(ir: &IrRequest) -> Result<Value, DialectError> {
    let mut obj = serde_json::Map::new();

    if let Some(model) = &ir.model {
        obj.insert("model".into(), Value::String(model.clone()));
    }
    if let Some(sp) = &ir.system_prompt {
        obj.insert("system".into(), Value::String(sp.clone()));
    }

    let mut messages = Vec::new();
    for msg in &ir.messages {
        if msg.role == IrRole::System {
            continue; // Claude uses a top-level "system" field
        }
        let role = match msg.role {
            IrRole::Assistant => "assistant",
            _ => "user",
        };

        let content = serialize_claude_content(&msg.content);
        messages.push(serde_json::json!({
            "role": role,
            "content": content
        }));
    }
    obj.insert("messages".into(), Value::Array(messages));

    if let Some(mt) = ir.config.max_tokens {
        obj.insert("max_tokens".into(), Value::Number(mt.into()));
    }
    if let Some(t) = ir.config.temperature {
        obj.insert(
            "temperature".into(),
            serde_json::Number::from_f64(t)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
    }

    if !ir.tools.is_empty() {
        let tools: Vec<Value> = ir
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters
                })
            })
            .collect();
        obj.insert("tools".into(), Value::Array(tools));
    }

    Ok(Value::Object(obj))
}

fn serialize_claude_content(blocks: &[IrContentBlock]) -> Value {
    if blocks.len() == 1 {
        if let IrContentBlock::Text { text } = &blocks[0] {
            return Value::String(text.clone());
        }
    }

    let arr: Vec<Value> = blocks
        .iter()
        .map(|b| match b {
            IrContentBlock::Text { text } => serde_json::json!({"type": "text", "text": text}),
            IrContentBlock::ToolCall { id, name, input } => {
                serde_json::json!({"type": "tool_use", "id": id, "name": name, "input": input})
            }
            IrContentBlock::ToolResult {
                tool_call_id,
                content,
                is_error,
            } => {
                let inner = serialize_claude_content(content);
                serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": tool_call_id,
                    "content": inner,
                    "is_error": is_error
                })
            }
            IrContentBlock::Thinking { text } => {
                serde_json::json!({"type": "thinking", "thinking": text})
            }
            IrContentBlock::Image { media_type, data } => serde_json::json!({
                "type": "image",
                "source": {"type": "base64", "media_type": media_type, "data": data}
            }),
            IrContentBlock::Audio { media_type, data } => serde_json::json!({
                "type": "audio",
                "media_type": media_type,
                "data": data
            }),
            IrContentBlock::Custom { custom_type, data } => serde_json::json!({
                "type": custom_type,
                "data": data
            }),
        })
        .collect();

    Value::Array(arr)
}

// ── Gemini ──────────────────────────────────────────────────────────────

fn gemini_entry() -> DialectEntry {
    DialectEntry {
        dialect: Dialect::Gemini,
        name: "gemini",
        version: "v1",
        parser: parse_gemini,
        serializer: serialize_gemini,
    }
}

fn parse_gemini(value: &Value) -> Result<IrRequest, DialectError> {
    let obj = value.as_object().ok_or_else(|| DialectError {
        dialect: Dialect::Gemini,
        message: "expected JSON object".into(),
    })?;

    let model = obj.get("model").and_then(Value::as_str).map(String::from);

    let system_prompt = obj
        .get("system_instruction")
        .and_then(|si| {
            si.get("parts")
                .and_then(Value::as_array)
                .and_then(|parts| parts.first())
                .and_then(|p| p.get("text"))
                .and_then(Value::as_str)
        })
        .map(String::from);

    let mut messages = Vec::new();
    if let Some(Value::Array(contents)) = obj.get("contents") {
        for c in contents {
            let role_str = c.get("role").and_then(Value::as_str).unwrap_or("user");
            let role = match role_str {
                "model" => IrRole::Assistant,
                _ => IrRole::User,
            };

            let blocks = if let Some(Value::Array(parts)) = c.get("parts") {
                parse_gemini_parts(parts)
            } else {
                Vec::new()
            };

            messages.push(IrMessage::new(role, blocks));
        }
    }

    let tools = parse_gemini_tools(obj);

    let config = obj
        .get("generationConfig")
        .or_else(|| obj.get("generation_config"))
        .map(|gc| IrGenerationConfig {
            max_tokens: gc
                .get("maxOutputTokens")
                .and_then(Value::as_u64)
                .or_else(|| gc.get("max_output_tokens").and_then(Value::as_u64)),
            temperature: gc.get("temperature").and_then(Value::as_f64),
            top_p: gc
                .get("topP")
                .and_then(Value::as_f64)
                .or_else(|| gc.get("top_p").and_then(Value::as_f64)),
            top_k: gc
                .get("topK")
                .and_then(Value::as_u64)
                .map(|v| v as u32)
                .or_else(|| gc.get("top_k").and_then(Value::as_u64).map(|v| v as u32)),
            stop_sequences: match gc.get("stopSequences").or_else(|| gc.get("stop_sequences")) {
                Some(Value::Array(arr)) => arr
                    .iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect(),
                _ => Vec::new(),
            },
            extra: BTreeMap::new(),
        })
        .unwrap_or_default();

    Ok(IrRequest {
        model,
        system_prompt,
        messages,
        tools,
        config,
        metadata: BTreeMap::new(),
    })
}

fn parse_gemini_parts(parts: &[Value]) -> Vec<IrContentBlock> {
    parts
        .iter()
        .filter_map(|p| {
            if let Some(text) = p.get("text").and_then(Value::as_str) {
                return Some(IrContentBlock::Text {
                    text: text.to_string(),
                });
            }
            if let Some(fc) = p.get("functionCall") {
                return Some(IrContentBlock::ToolCall {
                    id: String::new(),
                    name: fc
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    input: fc.get("args").cloned().unwrap_or(Value::Null),
                });
            }
            if let Some(fr) = p.get("functionResponse") {
                return Some(IrContentBlock::ToolResult {
                    tool_call_id: fr
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    content: vec![IrContentBlock::Text {
                        text: serde_json::to_string(
                            &fr.get("response").cloned().unwrap_or(Value::Null),
                        )
                        .unwrap_or_default(),
                    }],
                    is_error: false,
                });
            }
            if let Some(inline) = p.get("inlineData") {
                return Some(IrContentBlock::Image {
                    media_type: inline
                        .get("mimeType")
                        .and_then(Value::as_str)
                        .unwrap_or("image/png")
                        .to_string(),
                    data: inline
                        .get("data")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                });
            }
            None
        })
        .collect()
}

fn parse_gemini_tools(obj: &serde_json::Map<String, Value>) -> Vec<IrToolDefinition> {
    let Some(Value::Array(tools)) = obj.get("tools") else {
        return Vec::new();
    };
    tools
        .iter()
        .flat_map(|t| {
            let Some(Value::Array(decls)) = t.get("functionDeclarations") else {
                return Vec::new();
            };
            decls
                .iter()
                .filter_map(|d| {
                    Some(IrToolDefinition {
                        name: d.get("name")?.as_str()?.to_string(),
                        description: d
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        parameters: d
                            .get("parameters")
                            .cloned()
                            .unwrap_or(Value::Object(Default::default())),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn serialize_gemini(ir: &IrRequest) -> Result<Value, DialectError> {
    let mut obj = serde_json::Map::new();

    if let Some(model) = &ir.model {
        obj.insert("model".into(), Value::String(model.clone()));
    }

    if let Some(sp) = &ir.system_prompt {
        obj.insert(
            "system_instruction".into(),
            serde_json::json!({"parts": [{"text": sp}]}),
        );
    }

    let mut contents = Vec::new();
    for msg in &ir.messages {
        if msg.role == IrRole::System {
            continue;
        }
        let role = match msg.role {
            IrRole::Assistant => "model",
            _ => "user",
        };
        let parts = serialize_gemini_parts(&msg.content);
        contents.push(serde_json::json!({"role": role, "parts": parts}));
    }
    obj.insert("contents".into(), Value::Array(contents));

    if !ir.tools.is_empty() {
        let decls: Vec<Value> = ir
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters
                })
            })
            .collect();
        obj.insert(
            "tools".into(),
            serde_json::json!([{"functionDeclarations": decls}]),
        );
    }

    let mut gc = serde_json::Map::new();
    if let Some(mt) = ir.config.max_tokens {
        gc.insert("maxOutputTokens".into(), Value::Number(mt.into()));
    }
    if let Some(t) = ir.config.temperature {
        if let Some(n) = serde_json::Number::from_f64(t) {
            gc.insert("temperature".into(), Value::Number(n));
        }
    }
    if let Some(tp) = ir.config.top_p {
        if let Some(n) = serde_json::Number::from_f64(tp) {
            gc.insert("topP".into(), Value::Number(n));
        }
    }
    if let Some(tk) = ir.config.top_k {
        gc.insert("topK".into(), Value::Number(tk.into()));
    }
    if !gc.is_empty() {
        obj.insert("generationConfig".into(), Value::Object(gc));
    }

    Ok(Value::Object(obj))
}

fn serialize_gemini_parts(blocks: &[IrContentBlock]) -> Vec<Value> {
    blocks
        .iter()
        .map(|b| match b {
            IrContentBlock::Text { text } => serde_json::json!({"text": text}),
            IrContentBlock::ToolCall { name, input, .. } => {
                serde_json::json!({"functionCall": {"name": name, "args": input}})
            }
            IrContentBlock::ToolResult {
                tool_call_id,
                content,
                ..
            } => {
                let text = content
                    .iter()
                    .filter_map(|c| c.as_text())
                    .collect::<Vec<_>>()
                    .join("");
                let resp: Value = serde_json::from_str(&text).unwrap_or(Value::String(text));
                serde_json::json!({"functionResponse": {"name": tool_call_id, "response": resp}})
            }
            IrContentBlock::Image { media_type, data } => {
                serde_json::json!({"inlineData": {"mimeType": media_type, "data": data}})
            }
            IrContentBlock::Thinking { text } => serde_json::json!({"text": text}),
            IrContentBlock::Audio { .. } | IrContentBlock::Custom { .. } => {
                serde_json::json!({"text": "[unsupported content]"})
            }
        })
        .collect()
}

// ── Kimi ────────────────────────────────────────────────────────────────

fn kimi_entry() -> DialectEntry {
    DialectEntry {
        dialect: Dialect::Kimi,
        name: "kimi",
        version: "v1",
        parser: parse_kimi,
        serializer: serialize_kimi,
    }
}

fn parse_kimi(value: &Value) -> Result<IrRequest, DialectError> {
    // Kimi uses OpenAI-compatible format with extra fields
    let mut ir = parse_openai(value).map_err(|e| DialectError {
        dialect: Dialect::Kimi,
        message: e.message,
    })?;

    let empty_map = serde_json::Map::new();
    let obj = value.as_object().unwrap_or(&empty_map);

    // Preserve Kimi-specific fields in metadata
    if let Some(refs) = obj.get("refs") {
        ir.metadata.insert("kimi_refs".into(), refs.clone());
    }
    if let Some(sp) = obj.get("search_plus") {
        ir.metadata.insert("kimi_search_plus".into(), sp.clone());
    }

    Ok(ir)
}

fn serialize_kimi(ir: &IrRequest) -> Result<Value, DialectError> {
    let mut value = serialize_openai(ir).map_err(|e| DialectError {
        dialect: Dialect::Kimi,
        message: e.message,
    })?;

    if let Value::Object(ref mut obj) = value {
        if let Some(refs) = ir.metadata.get("kimi_refs") {
            obj.insert("refs".into(), refs.clone());
        }
        if let Some(sp) = ir.metadata.get("kimi_search_plus") {
            obj.insert("search_plus".into(), sp.clone());
        }
    }

    Ok(value)
}

// ── Codex ───────────────────────────────────────────────────────────────

fn codex_entry() -> DialectEntry {
    DialectEntry {
        dialect: Dialect::Codex,
        name: "codex",
        version: "v1",
        parser: parse_codex,
        serializer: serialize_codex,
    }
}

fn parse_codex(value: &Value) -> Result<IrRequest, DialectError> {
    let obj = value.as_object().ok_or_else(|| DialectError {
        dialect: Dialect::Codex,
        message: "expected JSON object".into(),
    })?;

    let model = obj.get("model").and_then(Value::as_str).map(String::from);

    let mut messages = Vec::new();

    // Codex uses "instructions" as system prompt
    let system_prompt = obj
        .get("instructions")
        .and_then(Value::as_str)
        .map(String::from);

    // Codex "input" field as user message
    if let Some(input) = obj.get("input").and_then(Value::as_str) {
        messages.push(IrMessage::text(IrRole::User, input));
    }

    // Codex "items" in responses
    if let Some(Value::Array(items)) = obj.get("items") {
        for item in items {
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
            match item_type {
                "message" => {
                    let role_str = item
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or("assistant");
                    let role = match role_str {
                        "user" => IrRole::User,
                        _ => IrRole::Assistant,
                    };
                    if let Some(text) = item.get("content").and_then(Value::as_str) {
                        messages.push(IrMessage::text(role, text));
                    }
                }
                "function_call" => {
                    let id = item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let input_val = item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or(Value::Null);
                    messages.push(IrMessage::new(
                        IrRole::Assistant,
                        vec![IrContentBlock::ToolCall {
                            id,
                            name,
                            input: input_val,
                        }],
                    ));
                }
                _ => {}
            }
        }
    }

    let tools = parse_openai_tools(obj);

    Ok(IrRequest {
        model,
        system_prompt,
        messages,
        tools,
        config: IrGenerationConfig::default(),
        metadata: BTreeMap::new(),
    })
}

fn serialize_codex(ir: &IrRequest) -> Result<Value, DialectError> {
    let mut obj = serde_json::Map::new();

    if let Some(model) = &ir.model {
        obj.insert("model".into(), Value::String(model.clone()));
    }
    if let Some(sp) = &ir.system_prompt {
        obj.insert("instructions".into(), Value::String(sp.clone()));
    }

    // First user message becomes "input"
    if let Some(first_user) = ir.messages.iter().find(|m| m.role == IrRole::User) {
        let text = first_user.text_content();
        if !text.is_empty() {
            obj.insert("input".into(), Value::String(text));
        }
    }

    if !ir.tools.is_empty() {
        let tools: Vec<Value> = ir
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect();
        obj.insert("tools".into(), Value::Array(tools));
    }

    Ok(Value::Object(obj))
}

// ── Copilot ─────────────────────────────────────────────────────────────

fn copilot_entry() -> DialectEntry {
    DialectEntry {
        dialect: Dialect::Copilot,
        name: "copilot",
        version: "v1",
        parser: parse_copilot,
        serializer: serialize_copilot,
    }
}

fn parse_copilot(value: &Value) -> Result<IrRequest, DialectError> {
    // Copilot uses OpenAI-compatible format with extra fields
    let mut ir = parse_openai(value).map_err(|e| DialectError {
        dialect: Dialect::Copilot,
        message: e.message,
    })?;

    let empty_map = serde_json::Map::new();
    let obj = value.as_object().unwrap_or(&empty_map);

    if let Some(refs) = obj.get("references") {
        ir.metadata
            .insert("copilot_references".into(), refs.clone());
    }
    if let Some(confirmations) = obj.get("confirmations") {
        ir.metadata
            .insert("copilot_confirmations".into(), confirmations.clone());
    }
    if let Some(am) = obj.get("agent_mode") {
        ir.metadata.insert("copilot_agent_mode".into(), am.clone());
    }

    Ok(ir)
}

fn serialize_copilot(ir: &IrRequest) -> Result<Value, DialectError> {
    let mut value = serialize_openai(ir).map_err(|e| DialectError {
        dialect: Dialect::Copilot,
        message: e.message,
    })?;

    if let Value::Object(ref mut obj) = value {
        if let Some(refs) = ir.metadata.get("copilot_references") {
            obj.insert("references".into(), refs.clone());
        }
        if let Some(c) = ir.metadata.get("copilot_confirmations") {
            obj.insert("confirmations".into(), c.clone());
        }
        if let Some(am) = ir.metadata.get("copilot_agent_mode") {
            obj.insert("agent_mode".into(), am.clone());
        }
    }

    Ok(value)
}

// ── Response parsing helpers (used by tests, not registry) ──────────────

/// Parse a raw JSON response into an [`IrResponse`] for the given dialect.
///
/// This is a convenience function; the registry focuses on request
/// parsing/serialization.
#[must_use]
pub fn parse_response(dialect: Dialect, value: &Value) -> Option<IrResponse> {
    match dialect {
        Dialect::OpenAi => parse_openai_response(value),
        Dialect::Claude => parse_claude_response(value),
        Dialect::Gemini => parse_gemini_response(value),
        _ => parse_openai_response(value), // fallback for OpenAI-compatible
    }
}

fn parse_openai_response(value: &Value) -> Option<IrResponse> {
    let obj = value.as_object()?;
    let id = obj.get("id").and_then(Value::as_str).map(String::from);
    let model = obj.get("model").and_then(Value::as_str).map(String::from);

    let mut content = Vec::new();
    if let Some(Value::Array(choices)) = obj.get("choices") {
        if let Some(choice) = choices.first() {
            let msg = choice.get("message")?;
            if let Some(text) = msg.get("content").and_then(Value::as_str) {
                content.push(IrContentBlock::Text {
                    text: text.to_string(),
                });
            }
            if let Some(Value::Array(tcs)) = msg.get("tool_calls") {
                for tc in tcs {
                    let tc_id = tc
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let func = tc.get("function").cloned().unwrap_or(Value::Null);
                    let name = func
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let args_str = func
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    let input: Value =
                        serde_json::from_str(args_str).unwrap_or(Value::String(args_str.into()));
                    content.push(IrContentBlock::ToolCall {
                        id: tc_id,
                        name,
                        input,
                    });
                }
            }

            let stop_reason =
                choice
                    .get("finish_reason")
                    .and_then(Value::as_str)
                    .map(|r| match r {
                        "stop" => IrStopReason::EndTurn,
                        "length" => IrStopReason::MaxTokens,
                        "tool_calls" => IrStopReason::ToolUse,
                        "content_filter" => IrStopReason::ContentFilter,
                        other => IrStopReason::Other(other.to_string()),
                    });

            let usage = obj.get("usage").map(|u| IrUsage {
                input_tokens: u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
                output_tokens: u
                    .get("completion_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                total_tokens: u.get("total_tokens").and_then(Value::as_u64).unwrap_or(0),
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            });

            return Some(IrResponse {
                id,
                model,
                content,
                stop_reason,
                usage,
                metadata: BTreeMap::new(),
            });
        }
    }

    Some(IrResponse::new(content).with_id(id.unwrap_or_default()))
}

fn parse_claude_response(value: &Value) -> Option<IrResponse> {
    let obj = value.as_object()?;
    let id = obj.get("id").and_then(Value::as_str).map(String::from);
    let model = obj.get("model").and_then(Value::as_str).map(String::from);

    let content = match obj.get("content") {
        Some(Value::Array(arr)) => parse_claude_content_blocks(arr),
        _ => Vec::new(),
    };

    let stop_reason = obj
        .get("stop_reason")
        .and_then(Value::as_str)
        .map(|r| match r {
            "end_turn" => IrStopReason::EndTurn,
            "stop_sequence" => IrStopReason::StopSequence,
            "max_tokens" => IrStopReason::MaxTokens,
            "tool_use" => IrStopReason::ToolUse,
            other => IrStopReason::Other(other.to_string()),
        });

    let usage = obj.get("usage").map(|u| IrUsage {
        input_tokens: u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
        output_tokens: u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0),
        total_tokens: u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0)
            + u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0),
        cache_read_tokens: u
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_write_tokens: u
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    });

    Some(IrResponse {
        id,
        model,
        content,
        stop_reason,
        usage,
        metadata: BTreeMap::new(),
    })
}

fn parse_gemini_response(value: &Value) -> Option<IrResponse> {
    let obj = value.as_object()?;

    let mut content = Vec::new();
    if let Some(Value::Array(candidates)) = obj.get("candidates") {
        if let Some(candidate) = candidates.first() {
            if let Some(Value::Array(parts)) = candidate.get("content").and_then(|c| c.get("parts"))
            {
                content = parse_gemini_parts(parts);
            }
        }
    }

    let usage = obj.get("usageMetadata").map(|u| IrUsage {
        input_tokens: u
            .get("promptTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: u
            .get("candidatesTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        total_tokens: u
            .get("totalTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read_tokens: u
            .get("cachedContentTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_write_tokens: 0,
    });

    Some(IrResponse {
        id: None,
        model: None,
        content,
        stop_reason: None,
        usage,
        metadata: BTreeMap::new(),
    })
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reg() -> DialectRegistry {
        DialectRegistry::with_builtins()
    }

    // ── Auto-detection from request JSON ────────────────────────────

    #[test]
    fn detect_and_parse_openai() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hello"}]
        });
        let (det, ir) = reg().detect_and_parse(&req).unwrap();
        assert_eq!(det.dialect, Dialect::OpenAi);
        assert_eq!(ir.model.as_deref(), Some("gpt-4"));
        assert_eq!(ir.messages.len(), 1);
    }

    #[test]
    fn detect_and_parse_claude() {
        let req = json!({
            "model": "claude-3-opus",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        });
        let (det, ir) = reg().detect_and_parse(&req).unwrap();
        assert_eq!(det.dialect, Dialect::Claude);
        assert_eq!(ir.model.as_deref(), Some("claude-3-opus"));
    }

    #[test]
    fn detect_and_parse_gemini() {
        let req = json!({
            "contents": [{"parts": [{"text": "hello"}]}]
        });
        let (det, ir) = reg().detect_and_parse(&req).unwrap();
        assert_eq!(det.dialect, Dialect::Gemini);
        assert!(!ir.messages.is_empty());
    }

    #[test]
    fn detect_and_parse_fails_for_empty() {
        let err = reg().detect_and_parse(&json!({}));
        assert!(err.is_err());
    }

    #[test]
    fn detect_and_parse_fails_for_non_object() {
        assert!(reg().detect_and_parse(&json!(42)).is_err());
    }

    // ── Feature matrix ──────────────────────────────────────────────

    #[test]
    fn features_all_builtins_present() {
        let r = reg();
        for &d in Dialect::all() {
            assert!(r.features(d).is_some(), "missing features for {d:?}");
        }
    }

    #[test]
    fn features_openai_supports_all() {
        let f = builtin_features(Dialect::OpenAi);
        assert!(f.streaming);
        assert!(f.tool_use);
        assert!(f.vision);
        assert!(f.system_prompt);
        assert!(f.multi_turn);
        assert!(f.json_mode);
    }

    #[test]
    fn features_kimi_no_tool_use() {
        let f = builtin_features(Dialect::Kimi);
        assert!(!f.tool_use);
        assert!(f.streaming);
    }

    #[test]
    fn features_codex_no_multi_turn() {
        let f = builtin_features(Dialect::Codex);
        assert!(!f.multi_turn);
        assert!(f.tool_use);
    }

    #[test]
    fn supported_names_reflects_fields() {
        let f = DialectFeatures {
            streaming: true,
            tool_use: false,
            vision: true,
            system_prompt: false,
            multi_turn: false,
            json_mode: false,
        };
        let names = f.supported_names();
        assert!(names.contains(&"streaming"));
        assert!(names.contains(&"vision"));
        assert!(!names.contains(&"tool_use"));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn features_not_found_for_unregistered() {
        let r = DialectRegistry::new(); // empty
        assert!(r.features(Dialect::OpenAi).is_none());
    }

    // ── Version tracking ────────────────────────────────────────────

    #[test]
    fn version_info_all_builtins() {
        let r = reg();
        for &d in Dialect::all() {
            let vi = r.version_info(d).unwrap();
            assert_eq!(vi.dialect, d);
            assert!(!vi.api_version.is_empty());
            assert_eq!(vi.label, d.label());
        }
    }

    #[test]
    fn version_info_openai() {
        let vi = reg().version_info(Dialect::OpenAi).unwrap();
        assert_eq!(vi.api_version, "v1");
        assert_eq!(vi.label, "OpenAI");
    }

    #[test]
    fn version_info_none_for_unregistered() {
        assert!(DialectRegistry::new()
            .version_info(Dialect::OpenAi)
            .is_none());
    }

    // ── Comparison utilities ────────────────────────────────────────

    #[test]
    fn compare_openai_and_claude() {
        let cmp = reg().compare(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(cmp.a, Dialect::OpenAi);
        assert_eq!(cmp.b, Dialect::Claude);
        assert!(cmp.shared.contains(&"streaming"));
        assert!(cmp.shared.contains(&"tool_use"));
        // json_mode is OpenAI-only vs Claude
        assert!(cmp.only_a.contains(&"json_mode"));
        assert!(!cmp.is_fully_compatible());
    }

    #[test]
    fn compare_identical_is_fully_compatible() {
        let cmp = reg().compare(Dialect::OpenAi, Dialect::OpenAi).unwrap();
        assert!(cmp.is_fully_compatible());
        assert!(cmp.shared.len() >= 6);
        assert!(cmp.only_a.is_empty());
        assert!(cmp.only_b.is_empty());
    }

    #[test]
    fn compare_none_for_unregistered() {
        let r = DialectRegistry::new();
        assert!(r.compare(Dialect::OpenAi, Dialect::Claude).is_none());
    }

    #[test]
    fn compare_kimi_vs_codex() {
        let cmp = reg().compare(Dialect::Kimi, Dialect::Codex).unwrap();
        // Kimi has multi_turn but Codex does not
        assert!(cmp.only_a.contains(&"multi_turn"));
        // Codex has tool_use but Kimi does not
        assert!(cmp.only_b.contains(&"tool_use"));
    }

    // ── Validate through registry ───────────────────────────────────

    #[test]
    fn validate_request_openai_valid() {
        let r = reg().validate_request(
            Dialect::OpenAi,
            &json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}]
            }),
        );
        assert!(r.is_valid());
    }

    #[test]
    fn validate_request_openai_missing_model() {
        let r = reg().validate_request(
            Dialect::OpenAi,
            &json!({"messages": [{"role": "user", "content": "hi"}]}),
        );
        assert!(!r.is_valid());
    }

    #[test]
    fn validate_request_gemini_valid() {
        let r = reg().validate_request(
            Dialect::Gemini,
            &json!({"model": "gemini-pro", "contents": [{"parts": [{"text": "hi"}]}]}),
        );
        assert!(r.is_valid());
    }

    // ── Existing registry behaviour ─────────────────────────────────

    #[test]
    fn builtins_has_six_entries() {
        assert_eq!(reg().len(), 6);
    }

    #[test]
    fn list_dialects_returns_all() {
        let dialects = reg().list_dialects();
        for &d in Dialect::all() {
            assert!(dialects.contains(&d));
        }
    }

    #[test]
    fn supports_pair_true_for_builtins() {
        assert!(reg().supports_pair(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn supports_pair_false_for_empty() {
        assert!(!DialectRegistry::new().supports_pair(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn dialect_features_serde_roundtrip() {
        let f = builtin_features(Dialect::OpenAi);
        let json = serde_json::to_string(&f).unwrap();
        let back: DialectFeatures = serde_json::from_str(&json).unwrap();
        assert_eq!(f, back);
    }

    #[test]
    fn dialect_version_info_serde_roundtrip() {
        let vi = reg().version_info(Dialect::Claude).unwrap();
        let json = serde_json::to_value(&vi).unwrap();
        assert_eq!(json["api_version"], "v1");
        assert_eq!(json["label"], "Claude");
        assert_eq!(json["dialect"], "claude");
    }
}
