// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conversion layer between Gemini types and ABP core types.
//!
//! This module provides conversion functions between the shim's public types
//! and the internal dialect types, as well as the ABP intermediate
//! representation (IR) used for the pipeline.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, Outcome, Receipt, ReceiptBuilder, UsageNormalized, WorkOrderBuilder,
};
use abp_gemini_sdk::dialect::{
    self, GeminiContent, GeminiFunctionCallingConfig, GeminiFunctionDeclaration,
    GeminiGenerationConfig, GeminiInlineData, GeminiPart, GeminiRequest, GeminiResponse,
    GeminiSafetySetting, GeminiStreamChunk, GeminiTool, GeminiToolConfig,
};
use abp_gemini_sdk::lowering;
use chrono::Utc;

use crate::GeminiError;
use crate::types::{
    Candidate, Content, GenerateContentRequest, GenerateContentResponse, GenerationConfig, Part,
    SafetySetting, StreamEvent, ToolConfig, ToolDeclaration, UsageMetadata,
};

// ── Shim ↔ Dialect conversions ──────────────────────────────────────────

/// Convert a shim [`Part`] to a dialect [`GeminiPart`].
#[must_use]
pub fn part_to_dialect(part: &Part) -> GeminiPart {
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

/// Convert a dialect [`GeminiPart`] to a shim [`Part`].
#[must_use]
pub fn part_from_dialect(part: &GeminiPart) -> Part {
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

/// Convert a shim [`Content`] to a dialect [`GeminiContent`].
#[must_use]
pub fn content_to_dialect(content: &Content) -> GeminiContent {
    GeminiContent {
        role: content.role.clone(),
        parts: content.parts.iter().map(part_to_dialect).collect(),
    }
}

/// Convert a dialect [`GeminiContent`] to a shim [`Content`].
#[must_use]
pub fn content_from_dialect(content: &GeminiContent) -> Content {
    Content {
        role: content.role.clone(),
        parts: content.parts.iter().map(part_from_dialect).collect(),
    }
}

/// Convert a shim [`SafetySetting`] to a dialect [`GeminiSafetySetting`].
#[must_use]
pub fn safety_to_dialect(s: &SafetySetting) -> GeminiSafetySetting {
    GeminiSafetySetting {
        category: s.category,
        threshold: s.threshold,
    }
}

/// Convert a shim [`GenerationConfig`] to a dialect [`GeminiGenerationConfig`].
#[must_use]
pub fn gen_config_to_dialect(cfg: &GenerationConfig) -> GeminiGenerationConfig {
    GeminiGenerationConfig {
        max_output_tokens: cfg.max_output_tokens,
        temperature: cfg.temperature,
        top_p: cfg.top_p,
        top_k: cfg.top_k,
        candidate_count: None,
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

/// Convert a shim [`ToolDeclaration`] to a dialect [`GeminiTool`].
#[must_use]
pub fn tool_decl_to_dialect(t: &ToolDeclaration) -> GeminiTool {
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

/// Convert a shim [`ToolConfig`] to a dialect [`GeminiToolConfig`].
#[must_use]
pub fn tool_config_to_dialect(tc: &ToolConfig) -> GeminiToolConfig {
    GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode: tc.function_calling_config.mode,
            allowed_function_names: tc.function_calling_config.allowed_function_names.clone(),
        },
    }
}

/// Convert a shim request to a full dialect [`GeminiRequest`].
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

/// Convert a dialect [`GeminiResponse`] to a shim response.
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

/// Convert a dialect [`GeminiStreamChunk`] to a shim [`StreamEvent`].
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
pub struct IrRequest {
    /// The conversation in ABP intermediate representation.
    pub conversation: IrConversation,
}

/// Convert a [`GenerateContentRequest`] to the ABP intermediate representation.
///
/// Returns the IR request, the optional generation config, and safety settings.
///
/// # Errors
///
/// Returns [`GeminiError`] if conversion fails.
pub fn request_to_ir(
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

/// Convert an IR request into an ABP [`WorkOrder`][abp_core::WorkOrder].
#[must_use]
pub fn ir_to_work_order(
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

/// Execute a work order and produce a mock receipt.
pub fn execute_work_order(wo: &abp_core::WorkOrder) -> Receipt {
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

/// Convert a [`Receipt`] back to an IR conversation.
#[must_use]
pub fn receipt_to_ir(receipt: &Receipt) -> IrConversation {
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

/// Convert an IR conversation to a Gemini response.
///
/// # Errors
///
/// Returns [`GeminiError`] if the conversion fails.
pub fn ir_to_response(
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

/// Build [`UsageMetadata`] from normalized usage statistics.
#[must_use]
pub fn make_usage_metadata(usage: &UsageNormalized) -> Option<UsageMetadata> {
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

/// Convert a [`Receipt`] into a sequence of [`StreamEvent`]s.
#[must_use]
pub fn receipt_to_stream_events(receipt: &Receipt) -> Vec<StreamEvent> {
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
