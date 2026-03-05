// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fluent builder for `GenerateContentRequest` and response convenience helpers.
//!
//! Provides [`GenerateContentRequestBuilder`] for ergonomic request construction
//! and accessor helpers for responses.

use serde_json::Value;

use crate::types::{
    Content, FunctionDeclaration, GenerateContentRequest, GenerateContentResponse,
    GenerationConfig, Part, SafetySetting, ToolConfig, ToolDeclaration,
};

// ── Request builder ─────────────────────────────────────────────────────

/// Fluent builder for [`GenerateContentRequest`].
///
/// # Example
///
/// ```ignore
/// use abp_shim_gemini::generate::GenerateContentRequestBuilder;
///
/// let request = GenerateContentRequestBuilder::new("gemini-2.5-flash")
///     .user_text("Hello, Gemini!")
///     .temperature(0.7)
///     .max_output_tokens(1024)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct GenerateContentRequestBuilder {
    model: String,
    contents: Vec<Content>,
    system_instruction: Option<Content>,
    generation_config: GenerationConfig,
    safety_settings: Vec<SafetySetting>,
    tools: Vec<ToolDeclaration>,
    tool_config: Option<ToolConfig>,
}

impl GenerateContentRequestBuilder {
    /// Create a new builder for the given model.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            contents: Vec::new(),
            system_instruction: None,
            generation_config: GenerationConfig::default(),
            safety_settings: Vec::new(),
            tools: Vec::new(),
            tool_config: None,
        }
    }

    /// Append a user text message.
    #[must_use]
    pub fn user_text(mut self, text: impl Into<String>) -> Self {
        self.contents.push(Content::user(vec![Part::text(text)]));
        self
    }

    /// Append a model text message (for multi-turn conversation history).
    #[must_use]
    pub fn model_text(mut self, text: impl Into<String>) -> Self {
        self.contents.push(Content::model(vec![Part::text(text)]));
        self
    }

    /// Append arbitrary content.
    #[must_use]
    pub fn content(mut self, content: Content) -> Self {
        self.contents.push(content);
        self
    }

    /// Set the system instruction text.
    #[must_use]
    pub fn system_instruction(mut self, text: impl Into<String>) -> Self {
        self.system_instruction = Some(Content::user(vec![Part::text(text)]));
        self
    }

    /// Set the system instruction from a full [`Content`] block.
    #[must_use]
    pub fn system_instruction_content(mut self, content: Content) -> Self {
        self.system_instruction = Some(content);
        self
    }

    /// Set the sampling temperature.
    #[must_use]
    pub fn temperature(mut self, temp: f64) -> Self {
        self.generation_config.temperature = Some(temp);
        self
    }

    /// Set the maximum number of output tokens.
    #[must_use]
    pub fn max_output_tokens(mut self, max: u32) -> Self {
        self.generation_config.max_output_tokens = Some(max);
        self
    }

    /// Set the top-p (nucleus) sampling parameter.
    #[must_use]
    pub fn top_p(mut self, p: f64) -> Self {
        self.generation_config.top_p = Some(p);
        self
    }

    /// Set the top-k sampling parameter.
    #[must_use]
    pub fn top_k(mut self, k: u32) -> Self {
        self.generation_config.top_k = Some(k);
        self
    }

    /// Set the candidate count.
    #[must_use]
    pub fn candidate_count(mut self, count: u32) -> Self {
        self.generation_config.candidate_count = Some(count);
        self
    }

    /// Add stop sequences.
    #[must_use]
    pub fn stop_sequences(mut self, seqs: Vec<String>) -> Self {
        self.generation_config.stop_sequences = Some(seqs);
        self
    }

    /// Set the response MIME type (e.g. `"application/json"`).
    #[must_use]
    pub fn response_mime_type(mut self, mime: impl Into<String>) -> Self {
        self.generation_config.response_mime_type = Some(mime.into());
        self
    }

    /// Set the response JSON schema for structured output.
    #[must_use]
    pub fn response_schema(mut self, schema: Value) -> Self {
        self.generation_config.response_schema = Some(schema);
        self
    }

    /// Set the full generation config, replacing any previously set fields.
    #[must_use]
    pub fn generation_config(mut self, config: GenerationConfig) -> Self {
        self.generation_config = config;
        self
    }

    /// Add a safety setting.
    #[must_use]
    pub fn safety_setting(mut self, setting: SafetySetting) -> Self {
        self.safety_settings.push(setting);
        self
    }

    /// Set all safety settings at once.
    #[must_use]
    pub fn safety_settings(mut self, settings: Vec<SafetySetting>) -> Self {
        self.safety_settings = settings;
        self
    }

    /// Add a function declaration as a tool.
    #[must_use]
    pub fn function(mut self, decl: FunctionDeclaration) -> Self {
        // Find or create a ToolDeclaration to append to.
        if let Some(last) = self.tools.last_mut() {
            last.function_declarations.push(decl);
        } else {
            self.tools.push(ToolDeclaration {
                function_declarations: vec![decl],
            });
        }
        self
    }

    /// Set tool declarations.
    #[must_use]
    pub fn tools(mut self, tools: Vec<ToolDeclaration>) -> Self {
        self.tools = tools;
        self
    }

    /// Set the function-calling configuration.
    #[must_use]
    pub fn tool_config(mut self, config: ToolConfig) -> Self {
        self.tool_config = Some(config);
        self
    }

    /// Build the [`GenerateContentRequest`].
    #[must_use]
    pub fn build(self) -> GenerateContentRequest {
        let has_gen_config = self.generation_config.temperature.is_some()
            || self.generation_config.max_output_tokens.is_some()
            || self.generation_config.top_p.is_some()
            || self.generation_config.top_k.is_some()
            || self.generation_config.candidate_count.is_some()
            || self.generation_config.stop_sequences.is_some()
            || self.generation_config.response_mime_type.is_some()
            || self.generation_config.response_schema.is_some();

        GenerateContentRequest {
            model: self.model,
            contents: self.contents,
            system_instruction: self.system_instruction,
            generation_config: if has_gen_config {
                Some(self.generation_config)
            } else {
                None
            },
            safety_settings: if self.safety_settings.is_empty() {
                None
            } else {
                Some(self.safety_settings)
            },
            tools: if self.tools.is_empty() {
                None
            } else {
                Some(self.tools)
            },
            tool_config: self.tool_config,
        }
    }
}

// ── Response convenience helpers ────────────────────────────────────────

/// Extract all text parts from a response, concatenated.
#[must_use]
pub fn response_full_text(resp: &GenerateContentResponse) -> String {
    resp.candidates
        .iter()
        .flat_map(|c| c.content.parts.iter())
        .filter_map(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Quick constructor: single-turn text request.
#[must_use]
pub fn text_request(model: &str, prompt: &str) -> GenerateContentRequest {
    GenerateContentRequestBuilder::new(model)
        .user_text(prompt)
        .build()
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Candidate, Content, FunctionCallingConfig, Part};
    use abp_gemini_sdk::dialect::FunctionCallingMode;
    use serde_json::json;

    #[test]
    fn builder_minimal() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("Hello")
            .build();
        assert_eq!(req.model, "gemini-2.5-flash");
        assert_eq!(req.contents.len(), 1);
        assert!(req.generation_config.is_none());
        assert!(req.safety_settings.is_none());
        assert!(req.tools.is_none());
    }

    #[test]
    fn builder_with_temperature() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("test")
            .temperature(0.5)
            .build();
        assert_eq!(
            req.generation_config.as_ref().unwrap().temperature,
            Some(0.5)
        );
    }

    #[test]
    fn builder_with_max_tokens() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("test")
            .max_output_tokens(2048)
            .build();
        assert_eq!(
            req.generation_config.as_ref().unwrap().max_output_tokens,
            Some(2048)
        );
    }

    #[test]
    fn builder_with_top_p_and_top_k() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("test")
            .top_p(0.9)
            .top_k(40)
            .build();
        let cfg = req.generation_config.as_ref().unwrap();
        assert_eq!(cfg.top_p, Some(0.9));
        assert_eq!(cfg.top_k, Some(40));
    }

    #[test]
    fn builder_with_system_instruction() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .system_instruction("Be helpful")
            .user_text("Hello")
            .build();
        let sys = req.system_instruction.as_ref().unwrap();
        assert_eq!(sys.role, "user");
        match &sys.parts[0] {
            Part::Text(t) => assert_eq!(t, "Be helpful"),
            _ => panic!("expected text part"),
        }
    }

    #[test]
    fn builder_multi_turn() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("Hi")
            .model_text("Hello!")
            .user_text("How are you?")
            .build();
        assert_eq!(req.contents.len(), 3);
        assert_eq!(req.contents[0].role, "user");
        assert_eq!(req.contents[1].role, "model");
        assert_eq!(req.contents[2].role, "user");
    }

    #[test]
    fn builder_with_function() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("weather?")
            .function(FunctionDeclaration {
                name: "get_weather".into(),
                description: "Get weather".into(),
                parameters: json!({"type": "object"}),
            })
            .build();
        let tools = req.tools.as_ref().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function_declarations[0].name, "get_weather");
    }

    #[test]
    fn builder_multiple_functions() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("help")
            .function(FunctionDeclaration {
                name: "fn_a".into(),
                description: "A".into(),
                parameters: json!({}),
            })
            .function(FunctionDeclaration {
                name: "fn_b".into(),
                description: "B".into(),
                parameters: json!({}),
            })
            .build();
        let tools = req.tools.as_ref().unwrap();
        assert_eq!(tools[0].function_declarations.len(), 2);
    }

    #[test]
    fn builder_with_tool_config() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("test")
            .tool_config(ToolConfig {
                function_calling_config: FunctionCallingConfig {
                    mode: FunctionCallingMode::Any,
                    allowed_function_names: Some(vec!["fn_a".into()]),
                },
            })
            .build();
        let tc = req.tool_config.as_ref().unwrap();
        assert_eq!(tc.function_calling_config.mode, FunctionCallingMode::Any);
    }

    #[test]
    fn builder_with_safety_settings() {
        use abp_gemini_sdk::dialect::{HarmBlockThreshold, HarmCategory};
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("test")
            .safety_setting(SafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockNone,
            })
            .build();
        assert_eq!(req.safety_settings.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn builder_stop_sequences() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("test")
            .stop_sequences(vec!["END".into(), "STOP".into()])
            .build();
        let cfg = req.generation_config.as_ref().unwrap();
        assert_eq!(cfg.stop_sequences.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn builder_response_mime_and_schema() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("test")
            .response_mime_type("application/json")
            .response_schema(json!({"type": "object"}))
            .build();
        let cfg = req.generation_config.as_ref().unwrap();
        assert_eq!(cfg.response_mime_type.as_deref(), Some("application/json"));
        assert!(cfg.response_schema.is_some());
    }

    #[test]
    fn builder_candidate_count() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("test")
            .candidate_count(3)
            .build();
        assert_eq!(
            req.generation_config.as_ref().unwrap().candidate_count,
            Some(3)
        );
    }

    #[test]
    fn builder_no_generation_config_when_empty() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .user_text("test")
            .build();
        assert!(req.generation_config.is_none());
    }

    #[test]
    fn builder_custom_content() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .content(Content::user(vec![
                Part::text("Look at this:"),
                Part::inline_data("image/png", "base64data"),
            ]))
            .build();
        assert_eq!(req.contents[0].parts.len(), 2);
    }

    #[test]
    fn text_request_helper() {
        let req = text_request("gemini-2.5-flash", "Hello");
        assert_eq!(req.model, "gemini-2.5-flash");
        assert_eq!(req.contents.len(), 1);
        assert_eq!(req.contents[0].role, "user");
    }

    #[test]
    fn response_full_text_single_candidate() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("Hello "), Part::text("world")]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        assert_eq!(response_full_text(&resp), "Hello world");
    }

    #[test]
    fn response_full_text_mixed_parts() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![
                    Part::text("text"),
                    Part::function_call("fn", json!({})),
                    Part::text(" more"),
                ]),
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
            prompt_feedback: None,
        };
        assert_eq!(response_full_text(&resp), "text more");
    }

    #[test]
    fn response_full_text_empty() {
        let resp = GenerateContentResponse {
            candidates: vec![],
            usage_metadata: None,
            prompt_feedback: None,
        };
        assert_eq!(response_full_text(&resp), "");
    }

    #[test]
    fn builder_system_instruction_content() {
        let req = GenerateContentRequestBuilder::new("gemini-2.5-flash")
            .system_instruction_content(Content::model(vec![Part::text("System text")]))
            .user_text("Hello")
            .build();
        let sys = req.system_instruction.as_ref().unwrap();
        assert_eq!(sys.role, "model");
    }
}
