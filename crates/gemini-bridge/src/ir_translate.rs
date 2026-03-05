// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! High-level translation between Gemini `GenerateContent` API types and
//! the ABP intermediate representation (`IrChatRequest` / `IrChatResponse` /
//! `IrStreamChunk`).
//!
//! This module builds on the lower-level helpers in [`crate::translate`] and
//! is feature-gated behind `normalized` because it depends on `abp-sdk-types`.

#[cfg(feature = "normalized")]
mod inner {
    use std::collections::BTreeMap;

    use abp_sdk_types::ir::{
        IrContentPart, IrMessage, IrRole, IrToolCall, IrToolDefinition, IrUsage,
    };
    use abp_sdk_types::ir_request::{IrChatRequest, IrSamplingParams, IrStreamConfig};
    use abp_sdk_types::ir_response::{IrChatResponse, IrChoice, IrFinishReason, IrStreamChunk};

    use crate::gemini_types::{
        Candidate, Content, FunctionCall, FunctionDeclaration, FunctionResponse, GeminiTool,
        GenerateContentRequest, GenerateContentResponse, GenerationConfig, HarmCategory,
        InlineData, Part, PromptFeedback, SafetyRating, SafetySetting,
        StreamGenerateContentResponse, UsageMetadata,
    };
    use crate::translate::{
        content_to_ir, declaration_to_ir, gemini_role_to_ir, ir_role_to_gemini, ir_to_content,
        ir_to_declaration, ir_to_part, ir_to_tools, ir_to_usage, part_to_ir, tools_to_ir,
        usage_to_ir,
    };

    // ── Finish reason mapping ───────────────────────────────────────────

    /// Map a Gemini `finishReason` string to an [`IrFinishReason`].
    #[must_use]
    pub fn gemini_finish_reason_to_ir(reason: Option<&str>) -> Option<IrFinishReason> {
        reason.map(|r| match r {
            "STOP" => IrFinishReason::Stop,
            "MAX_TOKENS" => IrFinishReason::Length,
            "SAFETY" => IrFinishReason::ContentFilter,
            "RECITATION" => IrFinishReason::ContentFilter,
            _ => IrFinishReason::Error,
        })
    }

    /// Map an [`IrFinishReason`] to a Gemini `finishReason` string.
    #[must_use]
    pub fn ir_finish_reason_to_gemini(reason: IrFinishReason) -> &'static str {
        match reason {
            IrFinishReason::Stop => "STOP",
            IrFinishReason::Length => "MAX_TOKENS",
            IrFinishReason::ToolUse => "STOP",
            IrFinishReason::ContentFilter => "SAFETY",
            IrFinishReason::Error => "OTHER",
        }
    }

    // ── Request translation ─────────────────────────────────────────────

    /// Convert a Gemini [`GenerateContentRequest`] into an [`IrChatRequest`].
    ///
    /// # Mapping
    ///
    /// | Gemini field           | IR field                          |
    /// |------------------------|-----------------------------------|
    /// | `model`                | `model`                           |
    /// | `contents`             | `messages`                        |
    /// | `system_instruction`   | prepended `System` message        |
    /// | `generation_config`    | `sampling` / `max_tokens` / etc.  |
    /// | `tools`                | `tools`                           |
    /// | `safety_settings`      | `extra["safety_settings"]`        |
    #[must_use]
    pub fn gemini_request_to_ir(req: &GenerateContentRequest) -> IrChatRequest {
        let mut messages: Vec<IrMessage> = Vec::new();

        // System instruction becomes a System-role message.
        if let Some(sys) = &req.system_instruction {
            let parts: Vec<IrContentPart> = sys.parts.iter().map(part_to_ir).collect();
            if !parts.is_empty() {
                messages.push(IrMessage {
                    role: IrRole::System,
                    content: parts,
                    tool_calls: Vec::new(),
                    metadata: BTreeMap::new(),
                });
            }
        }

        // Conversation turns.
        for content in &req.contents {
            messages.push(content_to_ir(content));
        }

        // Tool definitions.
        let tools = req.tools.as_deref().map(tools_to_ir).unwrap_or_default();

        // Sampling / generation config.
        let (sampling, max_tokens, stop_sequences, response_format) =
            if let Some(gc) = &req.generation_config {
                let sampling = IrSamplingParams {
                    temperature: gc.temperature,
                    top_p: gc.top_p,
                    top_k: gc.top_k,
                    frequency_penalty: None,
                    presence_penalty: None,
                };
                let max = gc.max_output_tokens.map(u64::from);
                let stops = gc.stop_sequences.clone().unwrap_or_default();
                let fmt = gc
                    .response_mime_type
                    .as_ref()
                    .map(|mt| serde_json::json!({ "mime_type": mt }))
                    .or_else(|| gc.response_schema.clone());
                (sampling, max, stops, fmt)
            } else {
                (IrSamplingParams::default(), None, Vec::new(), None)
            };

        // Vendor-specific extras (safety settings).
        let mut extra = BTreeMap::new();
        if let Some(ss) = &req.safety_settings {
            if let Ok(v) = serde_json::to_value(ss) {
                extra.insert("safety_settings".into(), v);
            }
        }

        IrChatRequest {
            model: req.model.clone(),
            messages,
            max_tokens,
            tools,
            tool_choice: None,
            sampling,
            stop_sequences,
            stream: IrStreamConfig::default(),
            response_format,
            extra,
        }
    }

    /// Convert an [`IrChatRequest`] into a Gemini [`GenerateContentRequest`].
    #[must_use]
    pub fn ir_to_gemini_request(ir: &IrChatRequest) -> GenerateContentRequest {
        let mut system_instruction: Option<Content> = None;
        let mut contents: Vec<Content> = Vec::new();

        for msg in &ir.messages {
            if msg.role == IrRole::System {
                // Merge all system messages into a single system_instruction.
                let parts: Vec<Part> = msg.content.iter().map(ir_to_part).collect();
                match &mut system_instruction {
                    Some(existing) => existing.parts.extend(parts),
                    None => {
                        system_instruction = Some(Content { role: None, parts });
                    }
                }
            } else {
                contents.push(ir_to_content(msg));
            }
        }

        // Generation config.
        let has_config = ir.sampling.temperature.is_some()
            || ir.sampling.top_p.is_some()
            || ir.sampling.top_k.is_some()
            || ir.max_tokens.is_some()
            || !ir.stop_sequences.is_empty()
            || ir.response_format.is_some();

        let generation_config = if has_config {
            let response_mime_type = ir
                .response_format
                .as_ref()
                .and_then(|v| v.get("mime_type"))
                .and_then(|v| v.as_str())
                .map(String::from);
            let response_schema = ir
                .response_format
                .as_ref()
                .filter(|v| v.get("mime_type").is_none())
                .cloned();

            Some(GenerationConfig {
                temperature: ir.sampling.temperature,
                top_p: ir.sampling.top_p,
                top_k: ir.sampling.top_k,
                max_output_tokens: ir.max_tokens.map(|v| v as u32),
                candidate_count: None,
                stop_sequences: if ir.stop_sequences.is_empty() {
                    None
                } else {
                    Some(ir.stop_sequences.clone())
                },
                response_mime_type,
                response_schema,
            })
        } else {
            None
        };

        // Tools.
        let tools = if ir.tools.is_empty() {
            None
        } else {
            Some(vec![ir_to_tools(&ir.tools)])
        };

        // Safety settings from extras.
        let safety_settings = ir
            .extra
            .get("safety_settings")
            .and_then(|v| serde_json::from_value::<Vec<SafetySetting>>(v.clone()).ok());

        GenerateContentRequest {
            model: ir.model.clone(),
            contents,
            tools,
            generation_config,
            safety_settings,
            system_instruction,
        }
    }

    // ── Response translation ────────────────────────────────────────────

    /// Convert a Gemini [`GenerateContentResponse`] into an [`IrChatResponse`].
    #[must_use]
    pub fn gemini_response_to_ir(resp: &GenerateContentResponse) -> IrChatResponse {
        let choices: Vec<IrChoice> = resp
            .candidates
            .iter()
            .enumerate()
            .map(|(i, cand)| {
                let message = content_to_ir(&cand.content);
                let finish_reason = gemini_finish_reason_to_ir(cand.finish_reason.as_deref());
                IrChoice {
                    index: i as u32,
                    message,
                    finish_reason,
                }
            })
            .collect();

        let usage = resp.usage_metadata.as_ref().map(usage_to_ir);

        let mut metadata = BTreeMap::new();
        if let Some(pf) = &resp.prompt_feedback {
            if let Ok(v) = serde_json::to_value(pf) {
                metadata.insert("prompt_feedback".into(), v);
            }
        }
        // Preserve safety ratings from first candidate.
        if let Some(cand) = resp.candidates.first() {
            if let Some(ratings) = &cand.safety_ratings {
                if let Ok(v) = serde_json::to_value(ratings) {
                    metadata.insert("safety_ratings".into(), v);
                }
            }
        }

        IrChatResponse {
            id: None,
            model: None,
            choices,
            usage,
            metadata,
        }
    }

    /// Convert an [`IrChatResponse`] into a Gemini [`GenerateContentResponse`].
    #[must_use]
    pub fn ir_to_gemini_response(ir: &IrChatResponse) -> GenerateContentResponse {
        let candidates: Vec<Candidate> = ir
            .choices
            .iter()
            .map(|choice| {
                let content = ir_to_content(&choice.message);
                let finish_reason = choice
                    .finish_reason
                    .map(ir_finish_reason_to_gemini)
                    .map(String::from);

                // Restore safety ratings from metadata if present.
                let safety_ratings = ir
                    .metadata
                    .get("safety_ratings")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());

                Candidate {
                    content,
                    finish_reason,
                    safety_ratings,
                    citation_metadata: None,
                }
            })
            .collect();

        let usage_metadata = ir.usage.as_ref().map(ir_to_usage);

        let prompt_feedback = ir
            .metadata
            .get("prompt_feedback")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        GenerateContentResponse {
            candidates,
            usage_metadata,
            prompt_feedback,
        }
    }

    // ── Streaming translation ───────────────────────────────────────────

    /// Convert a Gemini streaming [`GenerateContentResponse`] into IR stream
    /// chunks.
    ///
    /// A single Gemini SSE frame can carry text deltas, function calls, finish
    /// reasons, and usage — all of which are unpacked into one or more
    /// [`IrStreamChunk`]s.
    #[must_use]
    pub fn gemini_stream_to_ir(chunk: &GenerateContentResponse) -> Vec<IrStreamChunk> {
        let mut chunks = Vec::new();

        for (i, cand) in chunk.candidates.iter().enumerate() {
            let idx = i as u32;
            let mut delta_content = Vec::new();
            let mut delta_tool_calls = Vec::new();

            for part in &cand.content.parts {
                match part {
                    Part::Text(text) => {
                        delta_content.push(IrContentPart::text(text.clone()));
                    }
                    Part::InlineData(data) => {
                        delta_content.push(IrContentPart::Image {
                            url: None,
                            base64: Some(data.data.clone()),
                            media_type: Some(data.mime_type.clone()),
                        });
                    }
                    Part::FunctionCall(fc) => {
                        delta_tool_calls.push(IrToolCall {
                            id: format!("fc_{}", fc.name),
                            name: fc.name.clone(),
                            arguments: fc.args.clone(),
                        });
                    }
                    Part::FunctionResponse(fr) => {
                        delta_content.push(IrContentPart::ToolResult {
                            call_id: format!("fc_{}", fr.name),
                            content: fr.response.to_string(),
                            is_error: false,
                        });
                    }
                }
            }

            let finish_reason = gemini_finish_reason_to_ir(cand.finish_reason.as_deref());

            let role = cand
                .content
                .role
                .as_deref()
                .map(|r| gemini_role_to_ir(Some(r)));

            // Include usage from the chunk-level metadata (typically final chunk).
            let usage = chunk.usage_metadata.as_ref().map(usage_to_ir);

            let mut metadata = BTreeMap::new();
            if let Some(ratings) = &cand.safety_ratings {
                if let Ok(v) = serde_json::to_value(ratings) {
                    metadata.insert("safety_ratings".into(), v);
                }
            }

            chunks.push(IrStreamChunk {
                id: None,
                model: None,
                index: idx,
                delta_content,
                delta_tool_calls,
                role,
                finish_reason,
                usage,
                metadata,
            });
        }

        chunks
    }
}

#[cfg(feature = "normalized")]
pub use inner::*;

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "normalized"))]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use abp_sdk_types::ir::{
        IrContentPart, IrMessage, IrRole, IrToolCall, IrToolDefinition, IrUsage,
    };
    use abp_sdk_types::ir_request::{IrChatRequest, IrSamplingParams, IrStreamConfig};
    use abp_sdk_types::ir_response::{IrChatResponse, IrChoice, IrFinishReason, IrStreamChunk};

    use crate::gemini_types::*;

    // ── Finish reason mapping ───────────────────────────────────────

    #[test]
    fn finish_reason_stop() {
        assert_eq!(
            gemini_finish_reason_to_ir(Some("STOP")),
            Some(IrFinishReason::Stop)
        );
    }

    #[test]
    fn finish_reason_max_tokens() {
        assert_eq!(
            gemini_finish_reason_to_ir(Some("MAX_TOKENS")),
            Some(IrFinishReason::Length)
        );
    }

    #[test]
    fn finish_reason_safety() {
        assert_eq!(
            gemini_finish_reason_to_ir(Some("SAFETY")),
            Some(IrFinishReason::ContentFilter)
        );
    }

    #[test]
    fn finish_reason_recitation() {
        assert_eq!(
            gemini_finish_reason_to_ir(Some("RECITATION")),
            Some(IrFinishReason::ContentFilter)
        );
    }

    #[test]
    fn finish_reason_none() {
        assert_eq!(gemini_finish_reason_to_ir(None), None);
    }

    #[test]
    fn finish_reason_unknown_maps_to_error() {
        assert_eq!(
            gemini_finish_reason_to_ir(Some("SOMETHING_ELSE")),
            Some(IrFinishReason::Error)
        );
    }

    #[test]
    fn ir_finish_stop_to_gemini() {
        assert_eq!(ir_finish_reason_to_gemini(IrFinishReason::Stop), "STOP");
    }

    #[test]
    fn ir_finish_length_to_gemini() {
        assert_eq!(
            ir_finish_reason_to_gemini(IrFinishReason::Length),
            "MAX_TOKENS"
        );
    }

    #[test]
    fn ir_finish_content_filter_to_gemini() {
        assert_eq!(
            ir_finish_reason_to_gemini(IrFinishReason::ContentFilter),
            "SAFETY"
        );
    }

    #[test]
    fn ir_finish_tool_use_to_gemini() {
        assert_eq!(ir_finish_reason_to_gemini(IrFinishReason::ToolUse), "STOP");
    }

    // ── Request: Gemini → IR ────────────────────────────────────────

    #[test]
    fn request_minimal_text() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]));
        let ir = gemini_request_to_ir(&req);
        assert_eq!(ir.model, "gemini-2.5-flash");
        assert_eq!(ir.messages.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Hello");
    }

    #[test]
    fn request_with_system_instruction() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::system(vec![Part::text("Be concise")]))
            .add_content(Content::user(vec![Part::text("Hi")]));
        let ir = gemini_request_to_ir(&req);
        assert_eq!(ir.messages.len(), 2);
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[0].text_content(), "Be concise");
        assert_eq!(ir.messages[1].role, IrRole::User);
    }

    #[test]
    fn request_with_generation_config() {
        let gc = GenerationConfig {
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
            max_output_tokens: Some(1024),
            stop_sequences: Some(vec!["END".into()]),
            ..Default::default()
        };
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .generation_config(gc)
            .add_content(Content::user(vec![Part::text("Go")]));
        let ir = gemini_request_to_ir(&req);
        assert_eq!(ir.sampling.temperature, Some(0.7));
        assert_eq!(ir.sampling.top_p, Some(0.9));
        assert_eq!(ir.sampling.top_k, Some(40));
        assert_eq!(ir.max_tokens, Some(1024));
        assert_eq!(ir.stop_sequences, vec!["END"]);
    }

    #[test]
    fn request_with_tools() {
        let tool = GeminiTool {
            function_declarations: vec![FunctionDeclaration {
                name: "search".into(),
                description: "Web search".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
        };
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .tools(vec![tool])
            .add_content(Content::user(vec![Part::text("Find info")]));
        let ir = gemini_request_to_ir(&req);
        assert_eq!(ir.tools.len(), 1);
        assert_eq!(ir.tools[0].name, "search");
        assert_eq!(ir.tools[0].description, "Web search");
    }

    #[test]
    fn request_with_safety_settings() {
        let settings = vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        }];
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .safety_settings(settings)
            .add_content(Content::user(vec![Part::text("Hi")]));
        let ir = gemini_request_to_ir(&req);
        assert!(ir.extra.contains_key("safety_settings"));
    }

    #[test]
    fn request_multimodal_inline_data() {
        let req = GenerateContentRequest::new("gemini-2.5-flash").add_content(Content::user(vec![
            Part::text("Describe this:"),
            Part::inline_data("image/png", "iVBOR..."),
        ]));
        let ir = gemini_request_to_ir(&req);
        assert_eq!(ir.messages[0].content.len(), 2);
        match &ir.messages[0].content[1] {
            IrContentPart::Image {
                base64, media_type, ..
            } => {
                assert_eq!(base64.as_deref(), Some("iVBOR..."));
                assert_eq!(media_type.as_deref(), Some("image/png"));
            }
            other => panic!("expected Image, got {:?}", other),
        }
    }

    #[test]
    fn request_with_function_call_in_history() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Search for rust")]))
            .add_content(Content::model(vec![Part::function_call(
                "search",
                serde_json::json!({"q": "rust"}),
            )]))
            .add_content(Content::user(vec![Part::function_response(
                "search",
                serde_json::json!({"results": []}),
            )]));
        let ir = gemini_request_to_ir(&req);
        assert_eq!(ir.messages.len(), 3);
        // Model message should have tool_calls populated
        assert_eq!(ir.messages[1].role, IrRole::Assistant);
        assert_eq!(ir.messages[1].tool_calls.len(), 1);
        assert_eq!(ir.messages[1].tool_calls[0].name, "search");
    }

    #[test]
    fn request_response_mime_type() {
        let gc = GenerationConfig {
            response_mime_type: Some("application/json".into()),
            ..Default::default()
        };
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .generation_config(gc)
            .add_content(Content::user(vec![Part::text("JSON please")]));
        let ir = gemini_request_to_ir(&req);
        assert!(ir.response_format.is_some());
        let fmt = ir.response_format.as_ref().unwrap();
        assert_eq!(fmt["mime_type"], "application/json");
    }

    // ── Request: IR → Gemini ────────────────────────────────────────

    #[test]
    fn ir_to_gemini_minimal() {
        let ir = IrChatRequest::new(
            "gemini-2.5-flash",
            vec![IrMessage::text(IrRole::User, "Hello")],
        );
        let req = ir_to_gemini_request(&ir);
        assert_eq!(req.model, "gemini-2.5-flash");
        assert_eq!(req.contents.len(), 1);
        assert_eq!(req.contents[0].role.as_deref(), Some("user"));
        assert!(req.system_instruction.is_none());
        assert!(req.tools.is_none());
        assert!(req.generation_config.is_none());
    }

    #[test]
    fn ir_to_gemini_with_system() {
        let ir = IrChatRequest::new(
            "gemini-2.5-flash",
            vec![
                IrMessage::text(IrRole::System, "Be helpful"),
                IrMessage::text(IrRole::User, "Hi"),
            ],
        );
        let req = ir_to_gemini_request(&ir);
        assert!(req.system_instruction.is_some());
        let sys = req.system_instruction.as_ref().unwrap();
        assert_eq!(sys.role, None); // system_instruction has no role
        assert_eq!(req.contents.len(), 1); // only the user message
    }

    #[test]
    fn ir_to_gemini_with_sampling() {
        let ir = IrChatRequest::new(
            "gemini-2.5-flash",
            vec![IrMessage::text(IrRole::User, "Hi")],
        )
        .with_max_tokens(2048)
        .with_sampling(IrSamplingParams {
            temperature: Some(0.5),
            top_p: Some(0.95),
            top_k: Some(32),
            frequency_penalty: None,
            presence_penalty: None,
        });
        let req = ir_to_gemini_request(&ir);
        let gc = req.generation_config.as_ref().unwrap();
        assert_eq!(gc.temperature, Some(0.5));
        assert_eq!(gc.top_p, Some(0.95));
        assert_eq!(gc.top_k, Some(32));
        assert_eq!(gc.max_output_tokens, Some(2048));
    }

    #[test]
    fn ir_to_gemini_with_tools() {
        let ir = IrChatRequest::new(
            "gemini-2.5-flash",
            vec![IrMessage::text(IrRole::User, "Find info")],
        )
        .with_tool(IrToolDefinition {
            name: "search".into(),
            description: "Web search".into(),
            parameters: serde_json::json!({"type": "object"}),
        });
        let req = ir_to_gemini_request(&ir);
        assert!(req.tools.is_some());
        let tools = req.tools.as_ref().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function_declarations[0].name, "search");
    }

    #[test]
    fn ir_to_gemini_safety_roundtrip() {
        let settings = vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        }];
        let orig = GenerateContentRequest::new("gemini-2.5-flash")
            .safety_settings(settings)
            .add_content(Content::user(vec![Part::text("Hi")]));
        let ir = gemini_request_to_ir(&orig);
        let back = ir_to_gemini_request(&ir);
        assert!(back.safety_settings.is_some());
        let ss = back.safety_settings.as_ref().unwrap();
        assert_eq!(ss[0].category, HarmCategory::HarmCategoryHarassment);
        assert_eq!(ss[0].threshold, HarmBlockThreshold::BlockMediumAndAbove);
    }

    // ── Request roundtrip ───────────────────────────────────────────

    #[test]
    fn request_roundtrip_preserves_model() {
        let orig = GenerateContentRequest::new("gemini-2.5-pro")
            .add_content(Content::user(vec![Part::text("Yo")]));
        let ir = gemini_request_to_ir(&orig);
        let back = ir_to_gemini_request(&ir);
        assert_eq!(back.model, "gemini-2.5-pro");
    }

    #[test]
    fn request_roundtrip_preserves_tools() {
        let tool = GeminiTool {
            function_declarations: vec![
                FunctionDeclaration {
                    name: "read".into(),
                    description: "Read a file".into(),
                    parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
                },
                FunctionDeclaration {
                    name: "write".into(),
                    description: "Write a file".into(),
                    parameters: serde_json::json!({"type": "object"}),
                },
            ],
        };
        let orig = GenerateContentRequest::new("gemini-2.5-flash")
            .tools(vec![tool])
            .add_content(Content::user(vec![Part::text("Go")]));
        let ir = gemini_request_to_ir(&orig);
        assert_eq!(ir.tools.len(), 2);
        let back = ir_to_gemini_request(&ir);
        let decls = &back.tools.as_ref().unwrap()[0].function_declarations;
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].name, "read");
        assert_eq!(decls[1].name, "write");
    }

    // ── Response: Gemini → IR ───────────────────────────────────────

    #[test]
    fn response_simple_text() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("Hello!")]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 5,
                total_token_count: 15,
            }),
            prompt_feedback: None,
        };
        let ir = gemini_response_to_ir(&resp);
        assert_eq!(ir.choices.len(), 1);
        assert_eq!(ir.choices[0].index, 0);
        assert_eq!(ir.choices[0].finish_reason, Some(IrFinishReason::Stop));
        assert_eq!(ir.choices[0].message.text_content(), "Hello!");
        let usage = ir.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn response_with_function_call() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::function_call(
                    "search",
                    serde_json::json!({"q": "rust"}),
                )]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        let ir = gemini_response_to_ir(&resp);
        assert_eq!(ir.choices[0].message.tool_calls.len(), 1);
        assert_eq!(ir.choices[0].message.tool_calls[0].name, "search");
    }

    #[test]
    fn response_with_safety_ratings() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("OK")]),
                finish_reason: Some("STOP".into()),
                safety_ratings: Some(vec![SafetyRating {
                    category: HarmCategory::HarmCategoryHarassment,
                    probability: HarmProbability::Negligible,
                }]),
                citation_metadata: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        let ir = gemini_response_to_ir(&resp);
        assert!(ir.metadata.contains_key("safety_ratings"));
    }

    #[test]
    fn response_with_prompt_feedback() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![]),
                finish_reason: Some("SAFETY".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
            prompt_feedback: Some(PromptFeedback {
                block_reason: Some("SAFETY".into()),
                safety_ratings: None,
            }),
        };
        let ir = gemini_response_to_ir(&resp);
        assert!(ir.metadata.contains_key("prompt_feedback"));
        assert_eq!(
            ir.choices[0].finish_reason,
            Some(IrFinishReason::ContentFilter)
        );
    }

    #[test]
    fn response_no_usage() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("Hi")]),
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        let ir = gemini_response_to_ir(&resp);
        assert!(ir.usage.is_none());
        assert_eq!(ir.choices[0].finish_reason, None);
    }

    // ── Response: IR → Gemini ───────────────────────────────────────

    #[test]
    fn ir_to_gemini_response_text() {
        let ir = IrChatResponse::text("Hello from IR");
        let resp = ir_to_gemini_response(&ir);
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(resp.text().unwrap(), "Hello from IR");
        assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
    }

    #[test]
    fn ir_to_gemini_response_with_usage() {
        let ir = IrChatResponse::text("OK").with_usage(IrUsage::from_counts(50, 20));
        let resp = ir_to_gemini_response(&ir);
        let usage = resp.usage_metadata.as_ref().unwrap();
        assert_eq!(usage.prompt_token_count, 50);
        assert_eq!(usage.candidates_token_count, 20);
        assert_eq!(usage.total_token_count, 70);
    }

    #[test]
    fn ir_to_gemini_response_length_finish() {
        let ir = IrChatResponse {
            id: None,
            model: None,
            choices: vec![IrChoice {
                index: 0,
                message: IrMessage::text(IrRole::Assistant, "truncated"),
                finish_reason: Some(IrFinishReason::Length),
            }],
            usage: None,
            metadata: BTreeMap::new(),
        };
        let resp = ir_to_gemini_response(&ir);
        assert_eq!(
            resp.candidates[0].finish_reason.as_deref(),
            Some("MAX_TOKENS")
        );
    }

    // ── Response roundtrip ──────────────────────────────────────────

    #[test]
    fn response_roundtrip_text() {
        let orig = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("Roundtrip!")]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 100,
                candidates_token_count: 50,
                total_token_count: 150,
            }),
            prompt_feedback: None,
        };
        let ir = gemini_response_to_ir(&orig);
        let back = ir_to_gemini_response(&ir);
        assert_eq!(back.text().unwrap(), "Roundtrip!");
        let usage = back.usage_metadata.as_ref().unwrap();
        assert_eq!(usage.prompt_token_count, 100);
        assert_eq!(usage.candidates_token_count, 50);
    }

    // ── Streaming: Gemini → IR ──────────────────────────────────────

    #[test]
    fn stream_text_delta() {
        let chunk = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("Hello")]),
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        let ir_chunks = gemini_stream_to_ir(&chunk);
        assert_eq!(ir_chunks.len(), 1);
        assert_eq!(ir_chunks[0].index, 0);
        assert_eq!(ir_chunks[0].delta_content.len(), 1);
        assert_eq!(ir_chunks[0].delta_content[0].as_text(), Some("Hello"));
        assert!(!ir_chunks[0].is_final());
    }

    #[test]
    fn stream_final_chunk() {
        let chunk = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 20,
                total_token_count: 30,
            }),
            prompt_feedback: None,
        };
        let ir_chunks = gemini_stream_to_ir(&chunk);
        assert_eq!(ir_chunks.len(), 1);
        assert!(ir_chunks[0].is_final());
        assert_eq!(ir_chunks[0].finish_reason, Some(IrFinishReason::Stop));
        let usage = ir_chunks[0].usage.as_ref().unwrap();
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn stream_function_call() {
        let chunk = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::function_call(
                    "search",
                    serde_json::json!({"q": "rust"}),
                )]),
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        let ir_chunks = gemini_stream_to_ir(&chunk);
        assert_eq!(ir_chunks[0].delta_tool_calls.len(), 1);
        assert_eq!(ir_chunks[0].delta_tool_calls[0].name, "search");
    }

    #[test]
    fn stream_empty_candidates() {
        let chunk = GenerateContentResponse {
            candidates: vec![],
            usage_metadata: None,
            prompt_feedback: None,
        };
        let ir_chunks = gemini_stream_to_ir(&chunk);
        assert!(ir_chunks.is_empty());
    }

    #[test]
    fn stream_with_role() {
        let chunk = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("Hi")]),
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        let ir_chunks = gemini_stream_to_ir(&chunk);
        assert_eq!(ir_chunks[0].role, Some(IrRole::Assistant));
    }

    #[test]
    fn stream_inline_data() {
        let chunk = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::inline_data("image/png", "abc123")]),
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        let ir_chunks = gemini_stream_to_ir(&chunk);
        match &ir_chunks[0].delta_content[0] {
            IrContentPart::Image {
                base64, media_type, ..
            } => {
                assert_eq!(base64.as_deref(), Some("abc123"));
                assert_eq!(media_type.as_deref(), Some("image/png"));
            }
            other => panic!("expected Image, got {:?}", other),
        }
    }

    #[test]
    fn stream_safety_ratings_in_metadata() {
        let chunk = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("delta")]),
                finish_reason: None,
                safety_ratings: Some(vec![SafetyRating {
                    category: HarmCategory::HarmCategoryHarassment,
                    probability: HarmProbability::Low,
                }]),
                citation_metadata: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        let ir_chunks = gemini_stream_to_ir(&chunk);
        assert!(ir_chunks[0].metadata.contains_key("safety_ratings"));
    }

    // ── Multiple system messages merge ──────────────────────────────

    #[test]
    fn multiple_system_messages_merge_into_one() {
        let ir = IrChatRequest::new(
            "gemini-2.5-flash",
            vec![
                IrMessage::text(IrRole::System, "Rule 1"),
                IrMessage::text(IrRole::System, "Rule 2"),
                IrMessage::text(IrRole::User, "Go"),
            ],
        );
        let req = ir_to_gemini_request(&ir);
        assert!(req.system_instruction.is_some());
        let sys = req.system_instruction.as_ref().unwrap();
        // Both system messages merged into one system_instruction
        assert_eq!(sys.parts.len(), 2);
        assert_eq!(req.contents.len(), 1);
    }

    // ── Tool role maps to user in Gemini ────────────────────────────

    #[test]
    fn tool_role_message_maps_to_user() {
        let ir = IrChatRequest::new(
            "gemini-2.5-flash",
            vec![
                IrMessage::text(IrRole::User, "Search for rust"),
                IrMessage {
                    role: IrRole::Tool,
                    content: vec![IrContentPart::ToolResult {
                        call_id: "fc_search".into(),
                        content: "42 results".into(),
                        is_error: false,
                    }],
                    tool_calls: Vec::new(),
                    metadata: BTreeMap::new(),
                },
            ],
        );
        let req = ir_to_gemini_request(&ir);
        // Tool messages become user-role in Gemini
        assert_eq!(req.contents[1].role.as_deref(), Some("user"));
    }

    #[test]
    fn ir_to_gemini_stop_sequences() {
        let mut ir = IrChatRequest::new(
            "gemini-2.5-flash",
            vec![IrMessage::text(IrRole::User, "Hi")],
        );
        ir.stop_sequences = vec!["STOP".into(), "END".into()];
        let req = ir_to_gemini_request(&ir);
        let gc = req.generation_config.as_ref().unwrap();
        assert_eq!(gc.stop_sequences, Some(vec!["STOP".into(), "END".into()]));
    }
}
