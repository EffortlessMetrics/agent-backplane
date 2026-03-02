// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! Drop-in Gemini SDK shim that routes through the Agent Backplane.
#![deny(unsafe_code)]
#![warn(missing_docs)]

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, Outcome, Receipt, ReceiptBuilder, UsageNormalized, WorkOrderBuilder,
};
use abp_gemini_sdk::dialect::{
    self, GeminiContent, GeminiGenerationConfig, GeminiInlineData, GeminiPart, GeminiRequest,
    GeminiResponse, GeminiSafetySetting, GeminiStreamChunk, GeminiTool, GeminiToolConfig,
};
use abp_gemini_sdk::lowering;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio_stream::Stream;

// ── Re-exports from dialect for user convenience ────────────────────────

pub use abp_gemini_sdk::dialect::{
    FunctionCallingMode, GeminiFunctionCallingConfig, GeminiFunctionDeclaration,
    HarmBlockThreshold, HarmCategory,
};

// ── Public types mirroring the Gemini SDK ───────────────────────────────

/// A part within a content block, mirroring the Gemini SDK `Part` type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Part {
    /// Plain text content.
    Text(String),
    /// Inline binary data (e.g. images).
    InlineData {
        /// MIME type of the data.
        mime_type: String,
        /// Base64-encoded binary data.
        data: String,
    },
    /// A function call requested by the model.
    FunctionCall {
        /// Name of the function to invoke.
        name: String,
        /// Arguments as a JSON value.
        args: serde_json::Value,
    },
    /// A function response returned to the model.
    FunctionResponse {
        /// Name of the function that was called.
        name: String,
        /// The function's response payload.
        response: serde_json::Value,
    },
}

impl Part {
    /// Create a text part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// Create an inline data part (e.g. image).
    #[must_use]
    pub fn inline_data(mime_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self::InlineData {
            mime_type: mime_type.into(),
            data: data.into(),
        }
    }

    /// Create a function call part.
    #[must_use]
    pub fn function_call(name: impl Into<String>, args: serde_json::Value) -> Self {
        Self::FunctionCall {
            name: name.into(),
            args,
        }
    }

    /// Create a function response part.
    #[must_use]
    pub fn function_response(name: impl Into<String>, response: serde_json::Value) -> Self {
        Self::FunctionResponse {
            name: name.into(),
            response,
        }
    }
}

/// A content block in the Gemini API format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    /// Role of the content author (`user` or `model`).
    pub role: String,
    /// Content parts.
    pub parts: Vec<Part>,
}

impl Content {
    /// Create a user-role content block.
    #[must_use]
    pub fn user(parts: Vec<Part>) -> Self {
        Self {
            role: "user".into(),
            parts,
        }
    }

    /// Create a model-role content block.
    #[must_use]
    pub fn model(parts: Vec<Part>) -> Self {
        Self {
            role: "model".into(),
            parts,
        }
    }
}

/// Safety settings applied to a request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SafetySetting {
    /// The harm category to configure.
    pub category: HarmCategory,
    /// The blocking threshold for this category.
    pub threshold: HarmBlockThreshold,
}

/// Generation configuration parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    /// Maximum number of output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Top-p (nucleus) sampling parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Top-k sampling parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Stop sequences that halt generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// MIME type for the response (e.g. `application/json`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,
    /// JSON Schema for structured output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<serde_json::Value>,
}

/// Token usage metadata returned in a response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    /// Tokens consumed by the prompt.
    pub prompt_token_count: u64,
    /// Tokens generated across all candidates.
    pub candidates_token_count: u64,
    /// Total tokens (prompt + candidates).
    pub total_token_count: u64,
}

/// A request to the Gemini `generateContent` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateContentRequest {
    /// Model identifier (e.g. `gemini-2.5-flash`).
    pub model: String,
    /// Conversation content blocks.
    pub contents: Vec<Content>,
    /// Optional system instruction content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,
    /// Generation configuration parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
    /// Safety settings for content filtering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<SafetySetting>>,
    /// Tool definitions available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDeclaration>>,
    /// Function-calling configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<ToolConfig>,
}

impl GenerateContentRequest {
    /// Create a new request for the given model.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            contents: Vec::new(),
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        }
    }

    /// Add a content block and return `self` for chaining.
    #[must_use]
    pub fn add_content(mut self, content: Content) -> Self {
        self.contents.push(content);
        self
    }

    /// Set the system instruction.
    #[must_use]
    pub fn system_instruction(mut self, content: Content) -> Self {
        self.system_instruction = Some(content);
        self
    }

    /// Set generation config.
    #[must_use]
    pub fn generation_config(mut self, config: GenerationConfig) -> Self {
        self.generation_config = Some(config);
        self
    }

    /// Set safety settings.
    #[must_use]
    pub fn safety_settings(mut self, settings: Vec<SafetySetting>) -> Self {
        self.safety_settings = Some(settings);
        self
    }

    /// Set tool declarations.
    #[must_use]
    pub fn tools(mut self, tools: Vec<ToolDeclaration>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set tool config.
    #[must_use]
    pub fn tool_config(mut self, config: ToolConfig) -> Self {
        self.tool_config = Some(config);
        self
    }
}

/// A tool declaration wrapping function declarations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDeclaration {
    /// Function declarations available to the model.
    pub function_declarations: Vec<FunctionDeclaration>,
}

/// A function declaration for tool use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionDeclaration {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// Controls function-calling behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfig {
    /// Function-calling behaviour configuration.
    pub function_calling_config: FunctionCallingConfig,
}

/// Detailed function-calling configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCallingConfig {
    /// The function-calling mode.
    pub mode: FunctionCallingMode,
    /// Restrict calls to these function names, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_function_names: Option<Vec<String>>,
}

/// A single candidate in a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    /// The generated content.
    pub content: Content,
    /// Reason the model stopped generating.
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// The response from a `generateContent` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateContentResponse {
    /// Response candidates from the model.
    pub candidates: Vec<Candidate>,
    /// Token usage metadata.
    #[serde(default)]
    pub usage_metadata: Option<UsageMetadata>,
}

impl GenerateContentResponse {
    /// Extract the text from the first candidate's first text part.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.candidates.first().and_then(|c| {
            c.content.parts.iter().find_map(|p| match p {
                Part::Text(t) => Some(t.as_str()),
                _ => None,
            })
        })
    }

    /// Extract all function calls from the first candidate.
    #[must_use]
    pub fn function_calls(&self) -> Vec<(&str, &serde_json::Value)> {
        self.candidates
            .first()
            .map(|c| {
                c.content
                    .parts
                    .iter()
                    .filter_map(|p| match p {
                        Part::FunctionCall { name, args } => Some((name.as_str(), args)),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A streaming response event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    /// Response candidates in this chunk.
    pub candidates: Vec<Candidate>,
    /// Token usage metadata (usually in the final chunk).
    #[serde(default)]
    pub usage_metadata: Option<UsageMetadata>,
}

impl StreamEvent {
    /// Extract the text delta from the first candidate, if any.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.candidates.first().and_then(|c| {
            c.content.parts.iter().find_map(|p| match p {
                Part::Text(t) => Some(t.as_str()),
                _ => None,
            })
        })
    }
}

/// Errors from the Gemini shim.
#[derive(Debug, thiserror::Error)]
pub enum GeminiError {
    /// Request conversion failed.
    #[error("request conversion error: {0}")]
    RequestConversion(String),
    /// Response conversion failed.
    #[error("response conversion error: {0}")]
    ResponseConversion(String),
    /// The backend returned a failure outcome.
    #[error("backend error: {0}")]
    BackendError(String),
    /// Serialization / deserialization error.
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

// ── Client ──────────────────────────────────────────────────────────────

/// Drop-in replacement for the Google Gemini SDK client.
///
/// Routes requests through the ABP pipeline:
/// request → IR → WorkOrder → (execute) → Receipt → IR → response.
#[derive(Debug, Clone)]
pub struct GeminiClient {
    model: String,
}

impl GeminiClient {
    /// Create a new client targeting the given model.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
        }
    }

    /// Return the model this client targets.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Non-streaming content generation.
    ///
    /// Converts the request through the ABP pipeline and returns the response.
    ///
    /// # Errors
    ///
    /// Returns [`GeminiError`] if conversion or execution fails.
    pub async fn generate(
        &self,
        request: GenerateContentRequest,
    ) -> Result<GenerateContentResponse, GeminiError> {
        let (ir_request, gen_config, safety_settings) = request_to_ir(&request)?;
        let work_order = ir_to_work_order(&ir_request, &request.model, &gen_config);
        let receipt = execute_work_order(&work_order);
        let ir_response = receipt_to_ir(&receipt);
        ir_to_response(&ir_response, &receipt, &gen_config, &safety_settings)
    }

    /// Streaming content generation.
    ///
    /// Returns a stream of [`StreamEvent`]s. Each event may contain
    /// incremental text deltas or tool calls.
    ///
    /// # Errors
    ///
    /// Returns [`GeminiError`] if initial conversion fails.
    pub async fn generate_stream(
        &self,
        request: GenerateContentRequest,
    ) -> Result<impl Stream<Item = StreamEvent>, GeminiError> {
        let (ir_request, gen_config, _safety) = request_to_ir(&request)?;
        let work_order = ir_to_work_order(&ir_request, &request.model, &gen_config);
        let receipt = execute_work_order(&work_order);

        let events = receipt_to_stream_events(&receipt);
        Ok(tokio_stream::iter(events))
    }
}

// ── Conversion: Shim types ↔ Dialect types ──────────────────────────────

fn part_to_dialect(part: &Part) -> GeminiPart {
    match part {
        Part::Text(t) => GeminiPart::Text(t.clone()),
        Part::InlineData { mime_type, data } => GeminiPart::InlineData(GeminiInlineData {
            mime_type: mime_type.clone(),
            data: data.clone(),
        }),
        Part::FunctionCall { name, args } => GeminiPart::FunctionCall {
            name: name.clone(),
            args: args.clone(),
        },
        Part::FunctionResponse { name, response } => GeminiPart::FunctionResponse {
            name: name.clone(),
            response: response.clone(),
        },
    }
}

fn part_from_dialect(part: &GeminiPart) -> Part {
    match part {
        GeminiPart::Text(t) => Part::Text(t.clone()),
        GeminiPart::InlineData(d) => Part::InlineData {
            mime_type: d.mime_type.clone(),
            data: d.data.clone(),
        },
        GeminiPart::FunctionCall { name, args } => Part::FunctionCall {
            name: name.clone(),
            args: args.clone(),
        },
        GeminiPart::FunctionResponse { name, response } => Part::FunctionResponse {
            name: name.clone(),
            response: response.clone(),
        },
    }
}

fn content_to_dialect(content: &Content) -> GeminiContent {
    GeminiContent {
        role: content.role.clone(),
        parts: content.parts.iter().map(part_to_dialect).collect(),
    }
}

fn content_from_dialect(content: &GeminiContent) -> Content {
    Content {
        role: content.role.clone(),
        parts: content.parts.iter().map(part_from_dialect).collect(),
    }
}

fn safety_to_dialect(s: &SafetySetting) -> GeminiSafetySetting {
    GeminiSafetySetting {
        category: s.category,
        threshold: s.threshold,
    }
}

fn gen_config_to_dialect(cfg: &GenerationConfig) -> GeminiGenerationConfig {
    GeminiGenerationConfig {
        max_output_tokens: cfg.max_output_tokens,
        temperature: cfg.temperature,
        top_p: cfg.top_p,
        top_k: cfg.top_k,
        stop_sequences: cfg.stop_sequences.clone(),
        response_mime_type: cfg.response_mime_type.clone(),
        response_schema: cfg.response_schema.clone(),
    }
}

/// Convert a dialect generation config back to a shim generation config.
#[must_use]
pub fn gen_config_from_dialect(cfg: &GeminiGenerationConfig) -> GenerationConfig {
    GenerationConfig {
        max_output_tokens: cfg.max_output_tokens,
        temperature: cfg.temperature,
        top_p: cfg.top_p,
        top_k: cfg.top_k,
        stop_sequences: cfg.stop_sequences.clone(),
        response_mime_type: cfg.response_mime_type.clone(),
        response_schema: cfg.response_schema.clone(),
    }
}

fn tool_decl_to_dialect(t: &ToolDeclaration) -> GeminiTool {
    GeminiTool {
        function_declarations: t
            .function_declarations
            .iter()
            .map(|f| GeminiFunctionDeclaration {
                name: f.name.clone(),
                description: f.description.clone(),
                parameters: f.parameters.clone(),
            })
            .collect(),
    }
}

fn tool_config_to_dialect(tc: &ToolConfig) -> GeminiToolConfig {
    GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode: tc.function_calling_config.mode,
            allowed_function_names: tc.function_calling_config.allowed_function_names.clone(),
        },
    }
}

/// Convert a shim request to a full dialect `GeminiRequest`.
#[must_use]
pub fn to_dialect_request(req: &GenerateContentRequest) -> GeminiRequest {
    GeminiRequest {
        model: req.model.clone(),
        contents: req.contents.iter().map(content_to_dialect).collect(),
        system_instruction: req.system_instruction.as_ref().map(content_to_dialect),
        generation_config: req.generation_config.as_ref().map(gen_config_to_dialect),
        safety_settings: req
            .safety_settings
            .as_ref()
            .map(|ss| ss.iter().map(safety_to_dialect).collect()),
        tools: req
            .tools
            .as_ref()
            .map(|ts| ts.iter().map(tool_decl_to_dialect).collect()),
        tool_config: req.tool_config.as_ref().map(tool_config_to_dialect),
    }
}

/// Convert a dialect `GeminiResponse` to a shim response.
#[must_use]
pub fn from_dialect_response(resp: &GeminiResponse) -> GenerateContentResponse {
    GenerateContentResponse {
        candidates: resp
            .candidates
            .iter()
            .map(|c| Candidate {
                content: content_from_dialect(&c.content),
                finish_reason: c.finish_reason.clone(),
            })
            .collect(),
        usage_metadata: resp.usage_metadata.as_ref().map(|u| UsageMetadata {
            prompt_token_count: u.prompt_token_count,
            candidates_token_count: u.candidates_token_count,
            total_token_count: u.total_token_count,
        }),
    }
}

/// Convert a dialect `GeminiStreamChunk` to a shim `StreamEvent`.
#[must_use]
pub fn from_dialect_stream_chunk(chunk: &GeminiStreamChunk) -> StreamEvent {
    StreamEvent {
        candidates: chunk
            .candidates
            .iter()
            .map(|c| Candidate {
                content: content_from_dialect(&c.content),
                finish_reason: c.finish_reason.clone(),
            })
            .collect(),
        usage_metadata: chunk.usage_metadata.as_ref().map(|u| UsageMetadata {
            prompt_token_count: u.prompt_token_count,
            candidates_token_count: u.candidates_token_count,
            total_token_count: u.total_token_count,
        }),
    }
}

// ── Internal pipeline helpers ───────────────────────────────────────────

/// Intermediate result after request-to-IR conversion.
struct IrRequest {
    conversation: IrConversation,
}

fn request_to_ir(
    req: &GenerateContentRequest,
) -> Result<(IrRequest, Option<GenerationConfig>, Vec<SafetySetting>), GeminiError> {
    let dialect_contents: Vec<GeminiContent> =
        req.contents.iter().map(content_to_dialect).collect();
    let dialect_sys = req.system_instruction.as_ref().map(content_to_dialect);

    let conversation = lowering::to_ir(&dialect_contents, dialect_sys.as_ref());

    let gen_config = req.generation_config.clone();
    let safety = req.safety_settings.clone().unwrap_or_default();

    Ok((IrRequest { conversation }, gen_config, safety))
}

fn ir_to_work_order(
    ir: &IrRequest,
    model: &str,
    gen_config: &Option<GenerationConfig>,
) -> abp_core::WorkOrder {
    let task = ir
        .conversation
        .messages
        .iter()
        .filter(|m| m.role == IrRole::User)
        .map(|m| m.text_content())
        .collect::<Vec<_>>()
        .join("\n");

    let mut builder = WorkOrderBuilder::new(if task.is_empty() {
        "Gemini generate content".to_string()
    } else {
        task
    })
    .model(dialect::to_canonical_model(model));

    if let Some(cfg) = gen_config
        && let Some(max_tokens) = cfg.max_output_tokens
    {
        builder = builder.max_turns(max_tokens);
    }

    builder.build()
}

fn execute_work_order(wo: &abp_core::WorkOrder) -> Receipt {
    let task_text = wo.task.clone();
    let usage = UsageNormalized {
        input_tokens: Some(10),
        output_tokens: Some(20),
        ..Default::default()
    };

    ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .work_order_id(wo.id)
        .usage(usage)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "Gemini shim run started".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: format!("Response to: {task_text}"),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "Gemini shim run completed".into(),
            },
            ext: None,
        })
        .build()
}

fn receipt_to_ir(receipt: &Receipt) -> IrConversation {
    let mut messages = Vec::new();

    for event in &receipt.trace {
        match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                messages.push(IrMessage::text(IrRole::Assistant, text.clone()));
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                let id = tool_use_id
                    .clone()
                    .unwrap_or_else(|| format!("gemini_{tool_name}"));
                messages.push(IrMessage::new(
                    IrRole::Assistant,
                    vec![IrContentBlock::ToolUse {
                        id,
                        name: tool_name.clone(),
                        input: input.clone(),
                    }],
                ));
            }
            AgentEventKind::ToolResult {
                tool_name,
                tool_use_id,
                output,
                is_error,
            } => {
                let id = tool_use_id
                    .clone()
                    .unwrap_or_else(|| format!("gemini_{tool_name}"));
                let content_text = match output {
                    serde_json::Value::String(s) => s.clone(),
                    other => serde_json::to_string(other).unwrap_or_default(),
                };
                messages.push(IrMessage::new(
                    IrRole::Tool,
                    vec![IrContentBlock::ToolResult {
                        tool_use_id: id,
                        content: vec![IrContentBlock::Text { text: content_text }],
                        is_error: *is_error,
                    }],
                ));
            }
            _ => {}
        }
    }

    IrConversation::from_messages(messages)
}

fn ir_to_response(
    ir: &IrConversation,
    receipt: &Receipt,
    _gen_config: &Option<GenerationConfig>,
    _safety_settings: &[SafetySetting],
) -> Result<GenerateContentResponse, GeminiError> {
    let dialect_contents = lowering::from_ir(ir);

    let candidates: Vec<Candidate> = if dialect_contents.is_empty() {
        vec![Candidate {
            content: Content::model(vec![Part::text("")]),
            finish_reason: Some("STOP".into()),
        }]
    } else {
        dialect_contents
            .iter()
            .filter(|c| c.role == "model")
            .map(|c| Candidate {
                content: content_from_dialect(c),
                finish_reason: Some("STOP".into()),
            })
            .collect()
    };

    let candidates = if candidates.is_empty() {
        // If no model messages, produce one from all content
        dialect_contents
            .iter()
            .map(|c| Candidate {
                content: content_from_dialect(c),
                finish_reason: Some("STOP".into()),
            })
            .collect()
    } else {
        candidates
    };

    let usage_metadata = make_usage_metadata(&receipt.usage);

    Ok(GenerateContentResponse {
        candidates,
        usage_metadata,
    })
}

fn make_usage_metadata(usage: &UsageNormalized) -> Option<UsageMetadata> {
    let input = usage.input_tokens.unwrap_or(0);
    let output = usage.output_tokens.unwrap_or(0);
    if input > 0 || output > 0 {
        Some(UsageMetadata {
            prompt_token_count: input,
            candidates_token_count: output,
            total_token_count: input + output,
        })
    } else {
        None
    }
}

fn receipt_to_stream_events(receipt: &Receipt) -> Vec<StreamEvent> {
    let mut events = Vec::new();

    for agent_event in &receipt.trace {
        match &agent_event.kind {
            AgentEventKind::AssistantMessage { text } | AgentEventKind::AssistantDelta { text } => {
                events.push(StreamEvent {
                    candidates: vec![Candidate {
                        content: Content::model(vec![Part::text(text.clone())]),
                        finish_reason: None,
                    }],
                    usage_metadata: None,
                });
            }
            AgentEventKind::ToolCall {
                tool_name, input, ..
            } => {
                events.push(StreamEvent {
                    candidates: vec![Candidate {
                        content: Content::model(vec![Part::function_call(
                            tool_name.clone(),
                            input.clone(),
                        )]),
                        finish_reason: None,
                    }],
                    usage_metadata: None,
                });
            }
            _ => {}
        }
    }

    // Final chunk with usage
    let usage = make_usage_metadata(&receipt.usage);
    if let Some(usage) = usage {
        events.push(StreamEvent {
            candidates: vec![],
            usage_metadata: Some(usage),
        });
    }

    events
}

// ── IR Usage conversion ─────────────────────────────────────────────────

/// Convert Gemini usage metadata to IR usage.
#[must_use]
pub fn usage_to_ir(usage: &UsageMetadata) -> IrUsage {
    IrUsage::from_io(usage.prompt_token_count, usage.candidates_token_count)
}

/// Convert IR usage to Gemini usage metadata.
#[must_use]
pub fn usage_from_ir(usage: &IrUsage) -> UsageMetadata {
    UsageMetadata {
        prompt_token_count: usage.input_tokens,
        candidates_token_count: usage.output_tokens,
        total_token_count: usage.total_tokens,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use abp_gemini_sdk::dialect::{GeminiCandidate, GeminiUsageMetadata};
    use serde_json::json;

    // ── 1. Simple content generation roundtrip ──────────────────────────

    #[tokio::test]
    async fn simple_text_generation() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let request = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]));
        let response = client.generate(request).await.unwrap();
        assert!(!response.candidates.is_empty());
        assert!(response.text().is_some());
    }

    #[tokio::test]
    async fn generate_returns_usage_metadata() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let request = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Count to 5")]));
        let response = client.generate(request).await.unwrap();
        let usage = response.usage_metadata.as_ref().unwrap();
        assert!(usage.total_token_count > 0);
        assert_eq!(
            usage.total_token_count,
            usage.prompt_token_count + usage.candidates_token_count
        );
    }

    #[test]
    fn generate_content_request_builder() {
        let req = GenerateContentRequest::new("gemini-2.5-pro")
            .add_content(Content::user(vec![Part::text("hi")]))
            .generation_config(GenerationConfig {
                temperature: Some(0.5),
                ..Default::default()
            });
        assert_eq!(req.model, "gemini-2.5-pro");
        assert_eq!(req.contents.len(), 1);
        assert_eq!(
            req.generation_config.as_ref().unwrap().temperature,
            Some(0.5)
        );
    }

    // ── 2. Multi-turn conversation ──────────────────────────────────────

    #[tokio::test]
    async fn multi_turn_conversation() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let request = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hi")]))
            .add_content(Content::model(vec![Part::text("Hello!")]))
            .add_content(Content::user(vec![Part::text("How are you?")]));
        let response = client.generate(request).await.unwrap();
        assert!(!response.candidates.is_empty());
    }

    #[test]
    fn multi_turn_ir_roundtrip() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Turn 1")]))
            .add_content(Content::model(vec![Part::text("Reply 1")]))
            .add_content(Content::user(vec![Part::text("Turn 2")]));

        let dialect_req = to_dialect_request(&req);
        let ir = lowering::to_ir(
            &dialect_req.contents,
            dialect_req.system_instruction.as_ref(),
        );
        assert_eq!(ir.len(), 3);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[1].role, IrRole::Assistant);
        assert_eq!(ir.messages[2].role, IrRole::User);
    }

    // ── 3. Function calling / tool use ──────────────────────────────────

    #[tokio::test]
    async fn function_calling_request() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let request = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("What's the weather?")]))
            .tools(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "get_weather".into(),
                    description: "Get weather for a location".into(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "location": {"type": "string"}
                        }
                    }),
                }],
            }]);
        let response = client.generate(request).await.unwrap();
        assert!(!response.candidates.is_empty());
    }

    #[test]
    fn function_call_part_roundtrip_to_ir() {
        let content = Content::model(vec![Part::function_call(
            "search",
            json!({"query": "rust"}),
        )]);
        let dialect = content_to_dialect(&content);
        let ir_conv = lowering::to_ir(&[dialect], None);
        assert_eq!(ir_conv.len(), 1);
        match &ir_conv.messages[0].content[0] {
            IrContentBlock::ToolUse { name, input, .. } => {
                assert_eq!(name, "search");
                assert_eq!(input, &json!({"query": "rust"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn function_response_part_roundtrip_to_ir() {
        let content = Content::user(vec![Part::function_response(
            "search",
            json!("results here"),
        )]);
        let dialect = content_to_dialect(&content);
        let ir_conv = lowering::to_ir(&[dialect], None);
        match &ir_conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id, "gemini_search");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn function_calls_accessor() {
        let response = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![
                    Part::function_call("fn_a", json!({"x": 1})),
                    Part::function_call("fn_b", json!({"y": 2})),
                ]),
                finish_reason: None,
            }],
            usage_metadata: None,
        };
        let calls = response.function_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "fn_a");
        assert_eq!(calls[1].0, "fn_b");
    }

    // ── 4. Image input (inline data) ────────────────────────────────────

    #[test]
    fn inline_data_part_construction() {
        let part = Part::inline_data("image/png", "base64data");
        match &part {
            Part::InlineData { mime_type, data } => {
                assert_eq!(mime_type, "image/png");
                assert_eq!(data, "base64data");
            }
            _ => panic!("expected InlineData"),
        }
    }

    #[test]
    fn inline_data_ir_roundtrip() {
        let content = Content::user(vec![Part::inline_data("image/jpeg", "abc123")]);
        let dialect = content_to_dialect(&content);
        let ir = lowering::to_ir(&[dialect], None);
        match &ir.messages[0].content[0] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/jpeg");
                assert_eq!(data, "abc123");
            }
            other => panic!("expected Image, got {other:?}"),
        }

        let back = lowering::from_ir(&ir);
        let shim_content = content_from_dialect(&back[0]);
        match &shim_content.parts[0] {
            Part::InlineData { mime_type, data } => {
                assert_eq!(mime_type, "image/jpeg");
                assert_eq!(data, "abc123");
            }
            other => panic!("expected InlineData, got {other:?}"),
        }
    }

    // ── 5. Safety settings handling ─────────────────────────────────────

    #[test]
    fn safety_settings_preserved_in_request() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .safety_settings(vec![
                SafetySetting {
                    category: HarmCategory::HarmCategoryHarassment,
                    threshold: HarmBlockThreshold::BlockNone,
                },
                SafetySetting {
                    category: HarmCategory::HarmCategoryDangerousContent,
                    threshold: HarmBlockThreshold::BlockOnlyHigh,
                },
            ]);
        assert_eq!(req.safety_settings.as_ref().unwrap().len(), 2);

        let dialect_req = to_dialect_request(&req);
        let dialect_safety = dialect_req.safety_settings.unwrap();
        assert_eq!(dialect_safety.len(), 2);
        assert_eq!(
            dialect_safety[0].category,
            HarmCategory::HarmCategoryHarassment
        );
    }

    #[test]
    fn safety_setting_serde_roundtrip() {
        let setting = SafetySetting {
            category: HarmCategory::HarmCategorySexuallyExplicit,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        };
        let json = serde_json::to_string(&setting).unwrap();
        let back: SafetySetting = serde_json::from_str(&json).unwrap();
        assert_eq!(back, setting);
    }

    // ── 6. Generation config ────────────────────────────────────────────

    #[test]
    fn generation_config_all_fields() {
        let cfg = GenerationConfig {
            max_output_tokens: Some(1024),
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
            stop_sequences: Some(vec!["END".into()]),
            response_mime_type: Some("application/json".into()),
            response_schema: Some(json!({"type": "object"})),
        };
        let dialect = gen_config_to_dialect(&cfg);
        assert_eq!(dialect.max_output_tokens, Some(1024));
        assert_eq!(dialect.temperature, Some(0.7));
        assert_eq!(dialect.top_p, Some(0.9));
        assert_eq!(dialect.top_k, Some(40));
        assert_eq!(dialect.stop_sequences, Some(vec!["END".into()]));

        let back = gen_config_from_dialect(&dialect);
        assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
        assert_eq!(back.temperature, cfg.temperature);
        assert_eq!(back.top_p, cfg.top_p);
        assert_eq!(back.top_k, cfg.top_k);
    }

    #[test]
    fn generation_config_default_is_all_none() {
        let cfg = GenerationConfig::default();
        assert!(cfg.max_output_tokens.is_none());
        assert!(cfg.temperature.is_none());
        assert!(cfg.top_p.is_none());
        assert!(cfg.top_k.is_none());
        assert!(cfg.stop_sequences.is_none());
    }

    // ── 7. Streaming response chunks ────────────────────────────────────

    #[tokio::test]
    async fn streaming_produces_events() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let request = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Stream test")]));
        let stream = client.generate_stream(request).await.unwrap();

        use tokio_stream::StreamExt;
        let events: Vec<StreamEvent> = stream.collect().await;
        // Should have at least one text event and one usage event
        assert!(events.len() >= 2);
    }

    #[test]
    fn stream_event_text_accessor() {
        let event = StreamEvent {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("hello")]),
                finish_reason: None,
            }],
            usage_metadata: None,
        };
        assert_eq!(event.text(), Some("hello"));
    }

    #[test]
    fn from_dialect_stream_chunk_conversion() {
        let chunk = GeminiStreamChunk {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("delta".into())],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: 5,
                candidates_token_count: 10,
                total_token_count: 15,
            }),
        };
        let event = from_dialect_stream_chunk(&chunk);
        assert_eq!(event.text(), Some("delta"));
        let usage = event.usage_metadata.unwrap();
        assert_eq!(usage.prompt_token_count, 5);
        assert_eq!(usage.total_token_count, 15);
    }

    // ── 8. Usage metadata ───────────────────────────────────────────────

    #[test]
    fn usage_to_ir_conversion() {
        let usage = UsageMetadata {
            prompt_token_count: 100,
            candidates_token_count: 50,
            total_token_count: 150,
        };
        let ir = usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 100);
        assert_eq!(ir.output_tokens, 50);
        assert_eq!(ir.total_tokens, 150);
    }

    #[test]
    fn usage_from_ir_conversion() {
        let ir = IrUsage::from_io(200, 100);
        let usage = usage_from_ir(&ir);
        assert_eq!(usage.prompt_token_count, 200);
        assert_eq!(usage.candidates_token_count, 100);
        assert_eq!(usage.total_token_count, 300);
    }

    #[test]
    fn usage_metadata_serde_roundtrip() {
        let usage = UsageMetadata {
            prompt_token_count: 42,
            candidates_token_count: 18,
            total_token_count: 60,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: UsageMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back, usage);
    }

    // ── 9. Model name handling ──────────────────────────────────────────

    #[test]
    fn client_model_accessor() {
        let client = GeminiClient::new("gemini-2.5-pro");
        assert_eq!(client.model(), "gemini-2.5-pro");
    }

    #[test]
    fn model_canonical_roundtrip() {
        let canonical = dialect::to_canonical_model("gemini-2.5-flash");
        assert_eq!(canonical, "google/gemini-2.5-flash");
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, "gemini-2.5-flash");
    }

    #[test]
    fn work_order_uses_canonical_model() {
        let req = GenerateContentRequest::new("gemini-2.5-pro")
            .add_content(Content::user(vec![Part::text("test")]));
        let (ir, gen_config, _) = request_to_ir(&req).unwrap();
        let wo = ir_to_work_order(&ir, &req.model, &gen_config);
        assert_eq!(wo.config.model.as_deref(), Some("google/gemini-2.5-pro"));
    }

    // ── 10. Error responses ─────────────────────────────────────────────

    #[test]
    fn empty_candidates_still_returns_response() {
        let ir = IrConversation::new();
        let receipt = ReceiptBuilder::new("shim:gemini")
            .outcome(Outcome::Complete)
            .build();
        let response = ir_to_response(&ir, &receipt, &None, &[]).unwrap();
        assert!(!response.candidates.is_empty());
    }

    #[test]
    fn gemini_error_display() {
        let err = GeminiError::BackendError("something broke".into());
        assert!(err.to_string().contains("something broke"));
    }

    // ── Additional coverage ─────────────────────────────────────────────

    #[test]
    fn content_user_helper() {
        let c = Content::user(vec![Part::text("hi")]);
        assert_eq!(c.role, "user");
        assert_eq!(c.parts.len(), 1);
    }

    #[test]
    fn content_model_helper() {
        let c = Content::model(vec![Part::text("hello")]);
        assert_eq!(c.role, "model");
    }

    #[test]
    fn part_constructors() {
        let t = Part::text("hello");
        assert!(matches!(t, Part::Text(ref s) if s == "hello"));

        let fc = Part::function_call("fn", json!({}));
        assert!(matches!(fc, Part::FunctionCall { ref name, .. } if name == "fn"));

        let fr = Part::function_response("fn", json!("ok"));
        assert!(matches!(fr, Part::FunctionResponse { ref name, .. } if name == "fn"));
    }

    #[test]
    fn to_dialect_request_preserves_all_fields() {
        let req = GenerateContentRequest::new("model-x")
            .add_content(Content::user(vec![Part::text("hi")]))
            .system_instruction(Content::user(vec![Part::text("Be helpful")]))
            .generation_config(GenerationConfig {
                temperature: Some(1.0),
                ..Default::default()
            })
            .tools(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "f".into(),
                    description: "d".into(),
                    parameters: json!({}),
                }],
            }])
            .tool_config(ToolConfig {
                function_calling_config: FunctionCallingConfig {
                    mode: FunctionCallingMode::Auto,
                    allowed_function_names: None,
                },
            });

        let dialect = to_dialect_request(&req);
        assert_eq!(dialect.model, "model-x");
        assert!(dialect.system_instruction.is_some());
        assert!(dialect.generation_config.is_some());
        assert!(dialect.tools.is_some());
        assert!(dialect.tool_config.is_some());
    }

    #[test]
    fn from_dialect_response_conversion() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("Hi there".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: 5,
                candidates_token_count: 3,
                total_token_count: 8,
            }),
        };
        let shim_resp = from_dialect_response(&resp);
        assert_eq!(shim_resp.text(), Some("Hi there"));
        assert_eq!(
            shim_resp.usage_metadata.as_ref().unwrap().total_token_count,
            8
        );
    }

    #[test]
    fn system_instruction_in_request() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::user(vec![Part::text("Be concise")]))
            .add_content(Content::user(vec![Part::text("Hello")]));
        let dialect = to_dialect_request(&req);
        let ir = lowering::to_ir(&dialect.contents, dialect.system_instruction.as_ref());
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[0].text_content(), "Be concise");
    }

    #[test]
    fn tool_declaration_to_dialect_roundtrip() {
        let tool = ToolDeclaration {
            function_declarations: vec![FunctionDeclaration {
                name: "get_time".into(),
                description: "Gets current time".into(),
                parameters: json!({"type": "object", "properties": {}}),
            }],
        };
        let dialect = tool_decl_to_dialect(&tool);
        assert_eq!(dialect.function_declarations.len(), 1);
        assert_eq!(dialect.function_declarations[0].name, "get_time");
    }
}
