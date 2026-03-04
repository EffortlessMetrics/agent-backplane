// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! Drop-in Gemini SDK shim that routes through the Agent Backplane.
#![deny(unsafe_code)]
#![warn(missing_docs)]

/// HTTP client for the Google Gemini API.
pub mod client;
/// Conversion layer between Gemini types and ABP core types.
pub mod convert;
/// Strongly-typed Gemini API types mirroring the Google Gemini REST API.
pub mod types;

pub use convert::*;
pub use types::*;

// ── Re-exports from dialect for user convenience ────────────────────────

pub use abp_gemini_sdk::dialect::{
    FunctionCallingMode, GeminiFunctionCallingConfig, GeminiFunctionDeclaration,
    HarmBlockThreshold, HarmCategory,
};

use tokio_stream::Stream;

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

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::ir::{IrContentBlock, IrConversation, IrRole, IrUsage};
    use abp_core::{AgentEventKind, Outcome, ReceiptBuilder};
    use abp_gemini_sdk::dialect::{
        self, GeminiCandidate, GeminiContent, GeminiPart, GeminiResponse, GeminiStreamChunk,
        GeminiUsageMetadata,
    };
    use abp_gemini_sdk::lowering;
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
                safety_ratings: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
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
            candidate_count: None,
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
                safety_ratings: None,
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
            prompt_feedback: None,
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

    // ── From<GenerateContentRequest> for WorkOrder ──────────────────────

    #[test]
    fn from_request_extracts_task() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Explain Rust")]));
        let wo: abp_core::WorkOrder = req.into();
        assert_eq!(wo.task, "Explain Rust");
        assert_eq!(wo.config.model.as_deref(), Some("google/gemini-2.5-flash"));
    }

    #[test]
    fn from_request_uses_last_user_text() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("First")]))
            .add_content(Content::model(vec![Part::text("Reply")]))
            .add_content(Content::user(vec![Part::text("Second")]));
        let wo: abp_core::WorkOrder = req.into();
        assert_eq!(wo.task, "Second");
    }

    #[test]
    fn from_request_system_instruction_to_context() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::user(vec![Part::text("Be concise.")]))
            .add_content(Content::user(vec![Part::text("Hello")]));
        let wo: abp_core::WorkOrder = req.into();
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].content, "Be concise.");
    }

    #[test]
    fn from_request_preserves_vendor_fields() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("hi")]))
            .tools(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "f".into(),
                    description: "d".into(),
                    parameters: json!({}),
                }],
            }])
            .generation_config(GenerationConfig {
                temperature: Some(0.5),
                ..Default::default()
            });
        let wo: abp_core::WorkOrder = req.into();
        assert!(wo.config.vendor.contains_key("tools"));
        assert!(wo.config.vendor.contains_key("generation_config"));
        assert_eq!(wo.config.vendor["dialect"], "gemini");
    }

    #[test]
    fn from_request_empty_contents() {
        let req = GenerateContentRequest::new("gemini-2.5-flash");
        let wo: abp_core::WorkOrder = req.into();
        assert!(wo.task.is_empty());
    }

    // ── From<Receipt> for GenerateContentResponse ───────────────────────

    #[test]
    fn from_receipt_text_response() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .add_trace_event(abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Hello!".into(),
                },
                ext: None,
            })
            .build();
        let resp: GenerateContentResponse = receipt.into();
        assert_eq!(resp.text(), Some("Hello!"));
        assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
    }

    #[test]
    fn from_receipt_tool_call() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .add_trace_event(abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "search".into(),
                    tool_use_id: None,
                    parent_tool_use_id: None,
                    input: json!({"q": "rust"}),
                },
                ext: None,
            })
            .build();
        let resp: GenerateContentResponse = receipt.into();
        let calls = resp.function_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "search");
    }

    #[test]
    fn from_receipt_partial_outcome() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Partial)
            .build();
        let resp: GenerateContentResponse = receipt.into();
        assert_eq!(
            resp.candidates[0].finish_reason.as_deref(),
            Some("MAX_TOKENS")
        );
    }

    #[test]
    fn from_receipt_failed_outcome() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Failed)
            .build();
        let resp: GenerateContentResponse = receipt.into();
        assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("OTHER"));
    }

    #[test]
    fn from_receipt_with_usage() {
        let usage = abp_core::UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..Default::default()
        };
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .usage(usage)
            .build();
        let resp: GenerateContentResponse = receipt.into();
        let meta = resp.usage_metadata.unwrap();
        assert_eq!(meta.prompt_token_count, 100);
        assert_eq!(meta.candidates_token_count, 50);
        assert_eq!(meta.total_token_count, 150);
    }

    // ── FinishReason ────────────────────────────────────────────────────

    #[test]
    fn finish_reason_from_str() {
        assert_eq!(FinishReason::from_str_opt("STOP"), Some(FinishReason::Stop));
        assert_eq!(
            FinishReason::from_str_opt("MAX_TOKENS"),
            Some(FinishReason::MaxTokens)
        );
        assert_eq!(
            FinishReason::from_str_opt("SAFETY"),
            Some(FinishReason::Safety)
        );
        assert_eq!(
            FinishReason::from_str_opt("RECITATION"),
            Some(FinishReason::Recitation)
        );
        assert_eq!(
            FinishReason::from_str_opt("OTHER"),
            Some(FinishReason::Other)
        );
        assert_eq!(FinishReason::from_str_opt("UNKNOWN"), None);
    }

    #[test]
    fn finish_reason_serde_roundtrip() {
        let reason = FinishReason::Stop;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, "\"STOP\"");
        let back: FinishReason = serde_json::from_str(&json).unwrap();
        assert_eq!(back, reason);
    }

    #[test]
    fn candidate_finish_reason_typed() {
        let c = Candidate {
            content: Content::model(vec![Part::text("hi")]),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
        };
        assert_eq!(c.finish_reason_typed(), Some(FinishReason::Stop));

        let c2 = Candidate {
            content: Content::model(vec![]),
            finish_reason: None,
            safety_ratings: None,
        };
        assert_eq!(c2.finish_reason_typed(), None);
    }

    // ── HarmProbability / SafetyRating ──────────────────────────────────

    #[test]
    fn harm_probability_serde_roundtrip() {
        let p = HarmProbability::Medium;
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "\"MEDIUM\"");
        let back: HarmProbability = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn safety_rating_serde_roundtrip() {
        let rating = SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::Low,
        };
        let json = serde_json::to_string(&rating).unwrap();
        let back: SafetyRating = serde_json::from_str(&json).unwrap();
        assert_eq!(back, rating);
    }

    #[test]
    fn candidate_with_safety_ratings() {
        let c = Candidate {
            content: Content::model(vec![Part::text("safe")]),
            finish_reason: Some("STOP".into()),
            safety_ratings: Some(vec![SafetyRating {
                category: HarmCategory::HarmCategoryHarassment,
                probability: HarmProbability::Negligible,
            }]),
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: Candidate = serde_json::from_str(&json).unwrap();
        assert_eq!(back.safety_ratings.as_ref().unwrap().len(), 1);
    }

    // ── PromptFeedback ──────────────────────────────────────────────────

    #[test]
    fn prompt_feedback_serde_roundtrip() {
        let pf = PromptFeedback {
            block_reason: Some("SAFETY".into()),
            safety_ratings: Some(vec![SafetyRating {
                category: HarmCategory::HarmCategoryDangerousContent,
                probability: HarmProbability::High,
            }]),
        };
        let json = serde_json::to_string(&pf).unwrap();
        let back: PromptFeedback = serde_json::from_str(&json).unwrap();
        assert_eq!(back, pf);
    }

    #[test]
    fn response_with_prompt_feedback() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![]),
                finish_reason: Some("SAFETY".into()),
                safety_ratings: None,
            }],
            usage_metadata: None,
            prompt_feedback: Some(PromptFeedback {
                block_reason: Some("SAFETY".into()),
                safety_ratings: None,
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("promptFeedback"));
        let back: GenerateContentResponse = serde_json::from_str(&json).unwrap();
        assert!(back.prompt_feedback.is_some());
    }

    // ── candidate_count in GenerationConfig ─────────────────────────────

    #[test]
    fn generation_config_candidate_count() {
        let cfg = GenerationConfig {
            candidate_count: Some(3),
            ..Default::default()
        };
        let dialect = gen_config_to_dialect(&cfg);
        assert_eq!(dialect.candidate_count, Some(3));
        let back = gen_config_from_dialect(&dialect);
        assert_eq!(back.candidate_count, Some(3));
    }

    #[test]
    fn generation_config_candidate_count_serde() {
        let cfg = GenerationConfig {
            candidate_count: Some(5),
            temperature: Some(0.8),
            ..Default::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("candidateCount"));
        let back: GenerationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.candidate_count, Some(5));
    }

    // ── JsonSchema generation ───────────────────────────────────────────

    #[test]
    fn json_schema_for_request() {
        let schema = schemars::schema_for!(GenerateContentRequest);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("GenerateContentRequest"));
    }

    #[test]
    fn json_schema_for_response() {
        let schema = schemars::schema_for!(GenerateContentResponse);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("GenerateContentResponse"));
    }

    #[test]
    fn json_schema_for_part() {
        let schema = schemars::schema_for!(Part);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("Part"));
    }

    #[test]
    fn json_schema_for_stream_event() {
        let schema = schemars::schema_for!(StreamEvent);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("StreamEvent"));
    }

    // ── Roundtrip: Request → WorkOrder → Receipt → Response ─────────────

    #[test]
    fn full_roundtrip_from_traits() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]));

        let wo: abp_core::WorkOrder = req.into();
        assert_eq!(wo.task, "Hello");

        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .work_order_id(wo.id)
            .add_trace_event(abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage { text: "Hi!".into() },
                ext: None,
            })
            .build();

        let resp: GenerateContentResponse = receipt.into();
        assert_eq!(resp.text(), Some("Hi!"));
        assert!(resp.prompt_feedback.is_none());
    }
}
