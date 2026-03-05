#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

//! Comprehensive tests for the `abp-gemini-sdk` crate covering all public
//! types, functions, constants, serde roundtrips, conversions, and edge cases.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;

// ---------------------------------------------------------------------------
// Re-exports from the crate under test
// ---------------------------------------------------------------------------

use abp_gemini_sdk::{BACKEND_NAME, DEFAULT_NODE_COMMAND, HOST_SCRIPT_RELATIVE, sidecar_script};

// types.rs public API
use abp_gemini_sdk::types::{
    Candidate, Content, FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration,
    GeminiTool, GenerateContentRequest, GenerateContentResponse, GenerationConfig,
    HarmBlockThreshold, HarmCategory, HarmProbability, Part, PromptFeedback, SafetyRating,
    SafetySetting, ToolConfig, UsageMetadata,
};

// dialect.rs public API
use abp_gemini_sdk::dialect::{
    self, CanonicalToolDef, DynamicRetrievalConfig, GeminiCandidate, GeminiCitationMetadata,
    GeminiCitationSource, GeminiConfig, GeminiContent, GeminiFunctionCallingConfig,
    GeminiFunctionDeclaration, GeminiGenerationConfig, GeminiGroundingConfig, GeminiInlineData,
    GeminiPart, GeminiPromptFeedback, GeminiRequest, GeminiResponse, GeminiSafetyRating,
    GeminiSafetySetting, GeminiStreamChunk, GeminiToolConfig, GeminiUsageMetadata,
    GoogleSearchRetrieval,
};

// convert.rs public API
use abp_gemini_sdk::convert::{
    self, canonical_role_to_gemini, from_agent_event, from_receipt, gemini_role_to_canonical,
    generation_config_to_map, outcome_to_finish_reason, part_text, part_to_json, to_work_order,
};

// lowering.rs public API
use abp_gemini_sdk::lowering::{self, extract_system_instruction, from_ir, to_ir};

// abp-core types used in conversions
use abp_core::{
    AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized, WorkOrderBuilder,
    ir::{IrContentBlock, IrConversation, IrMessage, IrRole},
};

// ═══════════════════════════════════════════════════════════════════════════
// Module 1: lib.rs constants and functions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lib_backend_name_value() {
    assert_eq!(BACKEND_NAME, "sidecar:gemini");
}

#[test]
fn lib_host_script_relative_value() {
    assert_eq!(HOST_SCRIPT_RELATIVE, "hosts/gemini/host.js");
}

#[test]
fn lib_default_node_command_value() {
    assert_eq!(DEFAULT_NODE_COMMAND, "node");
}

#[test]
fn lib_sidecar_script_joins_path() {
    let root = Path::new("/workspace");
    let result = sidecar_script(root);
    assert_eq!(result, root.join("hosts/gemini/host.js"));
}

#[test]
fn lib_sidecar_script_empty_root() {
    let root = Path::new("");
    let result = sidecar_script(root);
    assert_eq!(result, Path::new("hosts/gemini/host.js"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 2: types.rs serde roundtrips
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn types_part_text_serde_roundtrip() {
    let p = Part::Text("hello world".into());
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn types_part_inline_data_serde_roundtrip() {
    let p = Part::InlineData {
        mime_type: "image/png".into(),
        data: "base64encoded".into(),
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn types_part_function_call_serde_roundtrip() {
    let p = Part::FunctionCall {
        name: "search".into(),
        args: json!({"query": "rust"}),
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn types_part_function_response_serde_roundtrip() {
    let p = Part::FunctionResponse {
        name: "search".into(),
        response: json!({"results": ["a", "b"]}),
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn types_content_serde_roundtrip() {
    let c = Content {
        role: Some("user".into()),
        parts: vec![Part::Text("hi".into())],
    };
    let json = serde_json::to_string(&c).unwrap();
    let back: Content = serde_json::from_str(&json).unwrap();
    assert_eq!(c, back);
}

#[test]
fn types_content_no_role_serde_roundtrip() {
    let c = Content {
        role: None,
        parts: vec![Part::Text("system text".into())],
    };
    let json = serde_json::to_string(&c).unwrap();
    let back: Content = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, None);
}

#[test]
fn types_generation_config_default_all_none() {
    let cfg = GenerationConfig::default();
    assert!(cfg.temperature.is_none());
    assert!(cfg.top_p.is_none());
    assert!(cfg.top_k.is_none());
    assert!(cfg.max_output_tokens.is_none());
    assert!(cfg.candidate_count.is_none());
    assert!(cfg.stop_sequences.is_none());
}

#[test]
fn types_generation_config_serde_roundtrip() {
    let cfg = GenerationConfig {
        temperature: Some(0.7),
        top_p: Some(0.95),
        top_k: Some(40),
        max_output_tokens: Some(2048),
        candidate_count: Some(1),
        stop_sequences: Some(vec!["END".into()]),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: GenerationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn types_generation_config_camel_case_keys() {
    let cfg = GenerationConfig {
        max_output_tokens: Some(100),
        ..Default::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("maxOutputTokens"));
    assert!(!json.contains("max_output_tokens"));
}

#[test]
fn types_harm_category_all_variants_serde() {
    let variants = vec![
        HarmCategory::HarmCategoryHarassment,
        HarmCategory::HarmCategoryHateSpeech,
        HarmCategory::HarmCategorySexuallyExplicit,
        HarmCategory::HarmCategoryDangerousContent,
        HarmCategory::HarmCategoryCivicIntegrity,
    ];
    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let back: HarmCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(*variant, back);
    }
}

#[test]
fn types_harm_category_screaming_snake_case() {
    let json = serde_json::to_string(&HarmCategory::HarmCategoryHarassment).unwrap();
    assert_eq!(json, "\"HARM_CATEGORY_HARASSMENT\"");
}

#[test]
fn types_harm_block_threshold_all_variants_serde() {
    let variants = vec![
        HarmBlockThreshold::BlockNone,
        HarmBlockThreshold::BlockLowAndAbove,
        HarmBlockThreshold::BlockMediumAndAbove,
        HarmBlockThreshold::BlockOnlyHigh,
    ];
    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let back: HarmBlockThreshold = serde_json::from_str(&json).unwrap();
        assert_eq!(*variant, back);
    }
}

#[test]
fn types_harm_block_threshold_screaming_snake_case() {
    let json = serde_json::to_string(&HarmBlockThreshold::BlockLowAndAbove).unwrap();
    assert_eq!(json, "\"BLOCK_LOW_AND_ABOVE\"");
}

#[test]
fn types_harm_probability_all_variants_serde() {
    let variants = vec![
        HarmProbability::Negligible,
        HarmProbability::Low,
        HarmProbability::Medium,
        HarmProbability::High,
    ];
    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let back: HarmProbability = serde_json::from_str(&json).unwrap();
        assert_eq!(*variant, back);
    }
}

#[test]
fn types_harm_probability_screaming_snake_case() {
    let json = serde_json::to_string(&HarmProbability::Negligible).unwrap();
    assert_eq!(json, "\"NEGLIGIBLE\"");
}

#[test]
fn types_safety_setting_serde_roundtrip() {
    let ss = SafetySetting {
        category: HarmCategory::HarmCategoryHateSpeech,
        threshold: HarmBlockThreshold::BlockMediumAndAbove,
    };
    let json = serde_json::to_string(&ss).unwrap();
    let back: SafetySetting = serde_json::from_str(&json).unwrap();
    assert_eq!(ss, back);
}

#[test]
fn types_safety_rating_serde_roundtrip() {
    let sr = SafetyRating {
        category: HarmCategory::HarmCategoryDangerousContent,
        probability: HarmProbability::Low,
    };
    let json = serde_json::to_string(&sr).unwrap();
    let back: SafetyRating = serde_json::from_str(&json).unwrap();
    assert_eq!(sr, back);
}

#[test]
fn types_usage_metadata_serde_roundtrip() {
    let um = UsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 200,
        total_token_count: 300,
    };
    let json = serde_json::to_string(&um).unwrap();
    let back: UsageMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(um, back);
}

#[test]
fn types_usage_metadata_camel_case_keys() {
    let um = UsageMetadata {
        prompt_token_count: 10,
        candidates_token_count: 20,
        total_token_count: 30,
    };
    let json = serde_json::to_string(&um).unwrap();
    assert!(json.contains("promptTokenCount"));
    assert!(json.contains("candidatesTokenCount"));
    assert!(json.contains("totalTokenCount"));
}

#[test]
fn types_prompt_feedback_serde_roundtrip() {
    let pf = PromptFeedback {
        block_reason: Some("SAFETY".into()),
        safety_ratings: Some(vec![SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::High,
        }]),
    };
    let json = serde_json::to_string(&pf).unwrap();
    let back: PromptFeedback = serde_json::from_str(&json).unwrap();
    assert_eq!(pf, back);
}

#[test]
fn types_prompt_feedback_empty_roundtrip() {
    let pf = PromptFeedback {
        block_reason: None,
        safety_ratings: None,
    };
    let json = serde_json::to_string(&pf).unwrap();
    let back: PromptFeedback = serde_json::from_str(&json).unwrap();
    assert_eq!(back.block_reason, None);
    assert_eq!(back.safety_ratings, None);
}

#[test]
fn types_function_calling_mode_all_variants_serde() {
    let variants = vec![
        FunctionCallingMode::Auto,
        FunctionCallingMode::Any,
        FunctionCallingMode::None,
    ];
    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let back: FunctionCallingMode = serde_json::from_str(&json).unwrap();
        assert_eq!(*variant, back);
    }
}

#[test]
fn types_function_calling_mode_screaming_snake_case() {
    assert_eq!(
        serde_json::to_string(&FunctionCallingMode::Auto).unwrap(),
        "\"AUTO\""
    );
    assert_eq!(
        serde_json::to_string(&FunctionCallingMode::Any).unwrap(),
        "\"ANY\""
    );
    assert_eq!(
        serde_json::to_string(&FunctionCallingMode::None).unwrap(),
        "\"NONE\""
    );
}

#[test]
fn types_function_declaration_serde_roundtrip() {
    let fd = FunctionDeclaration {
        name: "search".into(),
        description: "Search the web".into(),
        parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    let json = serde_json::to_string(&fd).unwrap();
    let back: FunctionDeclaration = serde_json::from_str(&json).unwrap();
    assert_eq!(fd, back);
}

#[test]
fn types_gemini_tool_serde_roundtrip() {
    let tool = GeminiTool {
        function_declarations: vec![FunctionDeclaration {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({"type": "object"}),
        }],
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: GeminiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn types_tool_config_serde_roundtrip() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Auto,
            allowed_function_names: Some(vec!["search".into(), "read".into()]),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn types_tool_config_no_allowed_names_serde() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: None,
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    assert!(!json.contains("allowedFunctionNames"));
    let back: ToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn types_generate_content_request_minimal_serde() {
    let req = GenerateContentRequest {
        contents: vec![Content {
            role: Some("user".into()),
            parts: vec![Part::Text("hi".into())],
        }],
        system_instruction: None,
        tools: None,
        tool_config: None,
        generation_config: None,
        safety_settings: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.contents.len(), 1);
    assert!(back.tools.is_none());
}

#[test]
fn types_generate_content_request_full_serde() {
    let req = GenerateContentRequest {
        contents: vec![Content {
            role: Some("user".into()),
            parts: vec![Part::Text("explain rust".into())],
        }],
        system_instruction: Some(Content {
            role: None,
            parts: vec![Part::Text("Be concise".into())],
        }),
        tools: Some(vec![GeminiTool {
            function_declarations: vec![FunctionDeclaration {
                name: "search".into(),
                description: "Search".into(),
                parameters: json!({"type": "object"}),
            }],
        }]),
        tool_config: Some(ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        }),
        generation_config: Some(GenerationConfig {
            temperature: Some(0.5),
            top_p: Some(0.9),
            top_k: Some(20),
            max_output_tokens: Some(1024),
            candidate_count: Some(1),
            stop_sequences: Some(vec!["DONE".into()]),
        }),
        safety_settings: Some(vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        }]),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json).unwrap();
    assert!(back.tools.is_some());
    assert!(back.tool_config.is_some());
    assert!(back.generation_config.is_some());
    assert!(back.safety_settings.is_some());
    assert!(back.system_instruction.is_some());
}

#[test]
fn types_generate_content_response_serde_roundtrip() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content {
                role: Some("model".into()),
                parts: vec![Part::Text("Hello!".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
        }],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 5,
            total_token_count: 15,
        }),
        prompt_feedback: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: GenerateContentResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.candidates.len(), 1);
    assert!(back.usage_metadata.is_some());
}

#[test]
fn types_candidate_with_safety_ratings() {
    let c = Candidate {
        content: Content {
            role: Some("model".into()),
            parts: vec![Part::Text("answer".into())],
        },
        finish_reason: Some("STOP".into()),
        safety_ratings: Some(vec![SafetyRating {
            category: HarmCategory::HarmCategorySexuallyExplicit,
            probability: HarmProbability::Negligible,
        }]),
    };
    let json = serde_json::to_string(&c).unwrap();
    let back: Candidate = serde_json::from_str(&json).unwrap();
    assert!(back.safety_ratings.is_some());
    assert_eq!(back.safety_ratings.unwrap().len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 3: dialect.rs constants and functions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_version_constant() {
    assert_eq!(dialect::DIALECT_VERSION, "gemini/v0.1");
}

#[test]
fn dialect_default_model_constant() {
    assert_eq!(dialect::DEFAULT_MODEL, "gemini-2.5-flash");
}

#[test]
fn dialect_to_canonical_model_prefixes_google() {
    assert_eq!(
        dialect::to_canonical_model("gemini-2.5-flash"),
        "google/gemini-2.5-flash"
    );
}

#[test]
fn dialect_to_canonical_model_arbitrary_name() {
    assert_eq!(
        dialect::to_canonical_model("custom-model"),
        "google/custom-model"
    );
}

#[test]
fn dialect_from_canonical_model_strips_prefix() {
    assert_eq!(
        dialect::from_canonical_model("google/gemini-2.5-flash"),
        "gemini-2.5-flash"
    );
}

#[test]
fn dialect_from_canonical_model_no_prefix_passthrough() {
    assert_eq!(
        dialect::from_canonical_model("gemini-2.5-flash"),
        "gemini-2.5-flash"
    );
}

#[test]
fn dialect_from_canonical_model_other_prefix_passthrough() {
    assert_eq!(
        dialect::from_canonical_model("openai/gpt-4"),
        "openai/gpt-4"
    );
}

#[test]
fn dialect_is_known_model_returns_true_for_known() {
    assert!(dialect::is_known_model("gemini-2.5-flash"));
    assert!(dialect::is_known_model("gemini-2.5-pro"));
    assert!(dialect::is_known_model("gemini-2.0-flash"));
    assert!(dialect::is_known_model("gemini-2.0-flash-lite"));
    assert!(dialect::is_known_model("gemini-1.5-flash"));
    assert!(dialect::is_known_model("gemini-1.5-pro"));
}

#[test]
fn dialect_is_known_model_returns_false_for_unknown() {
    assert!(!dialect::is_known_model("gpt-4"));
    assert!(!dialect::is_known_model("claude-3"));
    assert!(!dialect::is_known_model("gemini-99"));
}

#[test]
fn dialect_to_from_canonical_model_roundtrip() {
    let original = "gemini-2.5-flash";
    let canonical = dialect::to_canonical_model(original);
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, original);
}

#[test]
fn dialect_capability_manifest_has_streaming() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn dialect_capability_manifest_has_tool_read() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn dialect_capability_manifest_tool_write_emulated() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn dialect_capability_manifest_mcp_unsupported() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpClient),
        Some(SupportLevel::Unsupported)
    ));
    assert!(matches!(
        m.get(&Capability::McpServer),
        Some(SupportLevel::Unsupported)
    ));
}

// ── Tool definition conversions ─────────────────────────────────────────

#[test]
fn dialect_tool_def_to_gemini_maps_fields() {
    let def = CanonicalToolDef {
        name: "search".into(),
        description: "Search the web".into(),
        parameters_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    let gemini = dialect::tool_def_to_gemini(&def);
    assert_eq!(gemini.name, "search");
    assert_eq!(gemini.description, "Search the web");
    assert_eq!(gemini.parameters, def.parameters_schema);
}

#[test]
fn dialect_tool_def_from_gemini_maps_fields() {
    let gemini = GeminiFunctionDeclaration {
        name: "read".into(),
        description: "Read a file".into(),
        parameters: json!({"type": "object"}),
    };
    let canonical = dialect::tool_def_from_gemini(&gemini);
    assert_eq!(canonical.name, "read");
    assert_eq!(canonical.description, "Read a file");
    assert_eq!(canonical.parameters_schema, gemini.parameters);
}

#[test]
fn dialect_tool_def_roundtrip_canonical_to_gemini_and_back() {
    let original = CanonicalToolDef {
        name: "compute".into(),
        description: "Run computation".into(),
        parameters_schema: json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
    };
    let gemini = dialect::tool_def_to_gemini(&original);
    let back = dialect::tool_def_from_gemini(&gemini);
    assert_eq!(original, back);
}

// ── GeminiConfig ────────────────────────────────────────────────────────

#[test]
fn dialect_gemini_config_default_values() {
    let cfg = GeminiConfig::default();
    assert!(cfg.api_key.is_empty());
    assert!(cfg.base_url.contains("googleapis.com"));
    assert_eq!(cfg.model, "gemini-2.5-flash");
    assert_eq!(cfg.max_output_tokens, Some(4096));
    assert!(cfg.temperature.is_none());
}

#[test]
fn dialect_gemini_config_serde_roundtrip() {
    let cfg = GeminiConfig {
        api_key: "test-key".into(),
        base_url: "https://example.com".into(),
        model: "gemini-2.5-pro".into(),
        max_output_tokens: Some(8192),
        temperature: Some(0.8),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: GeminiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.api_key, "test-key");
    assert_eq!(back.model, "gemini-2.5-pro");
    assert_eq!(back.max_output_tokens, Some(8192));
    assert_eq!(back.temperature, Some(0.8));
}

// ── Dialect types serde roundtrips ──────────────────────────────────────

#[test]
fn dialect_gemini_inline_data_serde_roundtrip() {
    let d = GeminiInlineData {
        mime_type: "image/jpeg".into(),
        data: "abc123".into(),
    };
    let json = serde_json::to_string(&d).unwrap();
    let back: GeminiInlineData = serde_json::from_str(&json).unwrap();
    assert_eq!(d, back);
}

#[test]
fn dialect_gemini_part_text_serde_roundtrip() {
    let p = GeminiPart::Text("hello".into());
    let json = serde_json::to_string(&p).unwrap();
    let back: GeminiPart = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, GeminiPart::Text(ref t) if t == "hello"));
}

#[test]
fn dialect_gemini_part_inline_data_serde_roundtrip() {
    let p = GeminiPart::InlineData(GeminiInlineData {
        mime_type: "image/png".into(),
        data: "data".into(),
    });
    let json = serde_json::to_string(&p).unwrap();
    let back: GeminiPart = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, GeminiPart::InlineData(_)));
}

#[test]
fn dialect_gemini_part_function_call_serde_roundtrip() {
    let p = GeminiPart::FunctionCall {
        name: "f".into(),
        args: json!({"x": 1}),
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: GeminiPart = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, GeminiPart::FunctionCall { .. }));
}

#[test]
fn dialect_gemini_part_function_response_serde_roundtrip() {
    let p = GeminiPart::FunctionResponse {
        name: "f".into(),
        response: json!("ok"),
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: GeminiPart = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, GeminiPart::FunctionResponse { .. }));
}

#[test]
fn dialect_gemini_safety_setting_serde_roundtrip() {
    let ss = GeminiSafetySetting {
        category: dialect::HarmCategory::HarmCategoryHarassment,
        threshold: dialect::HarmBlockThreshold::BlockOnlyHigh,
    };
    let json = serde_json::to_string(&ss).unwrap();
    let back: GeminiSafetySetting = serde_json::from_str(&json).unwrap();
    assert_eq!(ss, back);
}

#[test]
fn dialect_gemini_safety_rating_serde_roundtrip() {
    let sr = GeminiSafetyRating {
        category: dialect::HarmCategory::HarmCategoryHateSpeech,
        probability: dialect::HarmProbability::Medium,
    };
    let json = serde_json::to_string(&sr).unwrap();
    let back: GeminiSafetyRating = serde_json::from_str(&json).unwrap();
    assert_eq!(sr, back);
}

#[test]
fn dialect_gemini_generation_config_default_all_none() {
    let cfg = GeminiGenerationConfig::default();
    assert!(cfg.temperature.is_none());
    assert!(cfg.top_p.is_none());
    assert!(cfg.top_k.is_none());
    assert!(cfg.max_output_tokens.is_none());
    assert!(cfg.candidate_count.is_none());
    assert!(cfg.stop_sequences.is_none());
    assert!(cfg.response_mime_type.is_none());
    assert!(cfg.response_schema.is_none());
}

#[test]
fn dialect_gemini_generation_config_serde_roundtrip() {
    let cfg = GeminiGenerationConfig {
        temperature: Some(1.0),
        top_p: Some(0.8),
        top_k: Some(50),
        max_output_tokens: Some(4096),
        candidate_count: Some(2),
        stop_sequences: Some(vec!["END".into()]),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(json!({"type": "object"})),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: GeminiGenerationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.temperature, Some(1.0));
    assert_eq!(back.response_mime_type.as_deref(), Some("application/json"));
}

#[test]
fn dialect_gemini_prompt_feedback_serde_roundtrip() {
    let pf = GeminiPromptFeedback {
        block_reason: Some("SAFETY".into()),
        safety_ratings: Some(vec![GeminiSafetyRating {
            category: dialect::HarmCategory::HarmCategoryDangerousContent,
            probability: dialect::HarmProbability::High,
        }]),
    };
    let json = serde_json::to_string(&pf).unwrap();
    let back: GeminiPromptFeedback = serde_json::from_str(&json).unwrap();
    assert_eq!(pf, back);
}

#[test]
fn dialect_gemini_usage_metadata_serde_roundtrip() {
    let um = GeminiUsageMetadata {
        prompt_token_count: 50,
        candidates_token_count: 100,
        total_token_count: 150,
    };
    let json = serde_json::to_string(&um).unwrap();
    let back: GeminiUsageMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back.prompt_token_count, 50);
    assert_eq!(back.total_token_count, 150);
}

#[test]
fn dialect_gemini_citation_source_serde_roundtrip() {
    let cs = GeminiCitationSource {
        start_index: Some(0),
        end_index: Some(10),
        uri: Some("https://example.com".into()),
        license: Some("MIT".into()),
    };
    let json = serde_json::to_string(&cs).unwrap();
    let back: GeminiCitationSource = serde_json::from_str(&json).unwrap();
    assert_eq!(cs, back);
}

#[test]
fn dialect_gemini_citation_source_all_optional_none() {
    let cs = GeminiCitationSource {
        start_index: None,
        end_index: None,
        uri: None,
        license: None,
    };
    let json = serde_json::to_string(&cs).unwrap();
    let back: GeminiCitationSource = serde_json::from_str(&json).unwrap();
    assert_eq!(cs, back);
}

#[test]
fn dialect_gemini_citation_metadata_serde_roundtrip() {
    let cm = GeminiCitationMetadata {
        citation_sources: vec![GeminiCitationSource {
            start_index: Some(5),
            end_index: Some(15),
            uri: Some("https://rust-lang.org".into()),
            license: None,
        }],
    };
    let json = serde_json::to_string(&cm).unwrap();
    let back: GeminiCitationMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(cm, back);
}

#[test]
fn dialect_grounding_config_serde_roundtrip() {
    let gc = GeminiGroundingConfig {
        google_search_retrieval: Some(GoogleSearchRetrieval {
            dynamic_retrieval_config: Some(DynamicRetrievalConfig {
                mode: "MODE_DYNAMIC".into(),
                dynamic_threshold: Some(0.5),
            }),
        }),
    };
    let json = serde_json::to_string(&gc).unwrap();
    let back: GeminiGroundingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(gc, back);
}

#[test]
fn dialect_grounding_config_empty_serde() {
    let gc = GeminiGroundingConfig {
        google_search_retrieval: None,
    };
    let json = serde_json::to_string(&gc).unwrap();
    let back: GeminiGroundingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(gc, back);
}

#[test]
fn dialect_dynamic_retrieval_config_no_threshold() {
    let drc = DynamicRetrievalConfig {
        mode: "MODE_STATIC".into(),
        dynamic_threshold: None,
    };
    let json = serde_json::to_string(&drc).unwrap();
    let back: DynamicRetrievalConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(drc, back);
}

#[test]
fn dialect_function_calling_mode_all_variants() {
    let variants = vec![
        dialect::FunctionCallingMode::Auto,
        dialect::FunctionCallingMode::Any,
        dialect::FunctionCallingMode::None,
    ];
    for v in variants {
        let json = serde_json::to_string(&v).unwrap();
        let back: dialect::FunctionCallingMode = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

#[test]
fn dialect_gemini_tool_config_serde_roundtrip() {
    let tc = GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode: dialect::FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["search".into()]),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: GeminiToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn dialect_gemini_stream_chunk_serde_roundtrip() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("partial".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: GeminiStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(back.candidates.len(), 1);
}

// ── map_work_order / map_response / map_stream_chunk ────────────────────

#[test]
fn dialect_map_work_order_basic() {
    let wo = WorkOrderBuilder::new("Test task").build();
    let cfg = GeminiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.contents.len(), 1);
    assert_eq!(req.contents[0].role, "user");
    match &req.contents[0].parts[0] {
        GeminiPart::Text(t) => assert!(t.contains("Test task")),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn dialect_map_work_order_uses_default_model_when_none() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = GeminiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gemini-2.5-flash");
}

#[test]
fn dialect_map_work_order_respects_model_override() {
    let wo = WorkOrderBuilder::new("task")
        .model("gemini-2.5-pro")
        .build();
    let cfg = GeminiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gemini-2.5-pro");
}

#[test]
fn dialect_map_work_order_includes_generation_config_when_set() {
    let cfg = GeminiConfig {
        max_output_tokens: Some(2048),
        temperature: Some(0.5),
        ..GeminiConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.generation_config.is_some());
    let gen_cfg = req.generation_config.unwrap();
    assert_eq!(gen_cfg.max_output_tokens, Some(2048));
    assert_eq!(gen_cfg.temperature, Some(0.5));
}

#[test]
fn dialect_map_work_order_no_generation_config_when_none() {
    let cfg = GeminiConfig {
        max_output_tokens: None,
        temperature: None,
        ..GeminiConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.generation_config.is_none());
}

#[test]
fn dialect_map_response_text() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("output".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0].kind, AgentEventKind::AssistantMessage { text } if text == "output")
    );
}

#[test]
fn dialect_map_response_function_call() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "search".into(),
                    args: json!({"q": "test"}),
                }],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0].kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "search")
    );
}

#[test]
fn dialect_map_response_function_response() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionResponse {
                    name: "search".into(),
                    response: json!("results"),
                }],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn dialect_map_response_inline_data_skipped() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::InlineData(GeminiInlineData {
                    mime_type: "image/png".into(),
                    data: "base64".into(),
                })],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn dialect_map_response_empty_candidates() {
    let resp = GeminiResponse {
        candidates: vec![],
        prompt_feedback: None,
        usage_metadata: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn dialect_map_stream_chunk_produces_delta() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("partial".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let events = dialect::map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0].kind, AgentEventKind::AssistantDelta { text } if text == "partial")
    );
}

#[test]
fn dialect_map_stream_event_is_alias_for_map_stream_chunk() {
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
        usage_metadata: None,
    };
    let a = dialect::map_stream_chunk(&chunk);
    let b = dialect::map_stream_event(&chunk);
    assert_eq!(a.len(), b.len());
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 4: convert.rs functions
// ═══════════════════════════════════════════════════════════════════════════

// ── Role mapping ────────────────────────────────────────────────────────

#[test]
fn convert_gemini_role_model_to_assistant() {
    assert_eq!(gemini_role_to_canonical("model"), "assistant");
}

#[test]
fn convert_gemini_role_user_to_user() {
    assert_eq!(gemini_role_to_canonical("user"), "user");
}

#[test]
fn convert_gemini_role_unknown_defaults_user() {
    assert_eq!(gemini_role_to_canonical("narrator"), "user");
    assert_eq!(gemini_role_to_canonical(""), "user");
}

#[test]
fn convert_canonical_assistant_to_model() {
    assert_eq!(canonical_role_to_gemini("assistant"), "model");
}

#[test]
fn convert_canonical_model_to_model() {
    assert_eq!(canonical_role_to_gemini("model"), "model");
}

#[test]
fn convert_canonical_system_to_user() {
    assert_eq!(canonical_role_to_gemini("system"), "user");
}

#[test]
fn convert_canonical_user_to_user() {
    assert_eq!(canonical_role_to_gemini("user"), "user");
}

#[test]
fn convert_canonical_unknown_to_user() {
    assert_eq!(canonical_role_to_gemini("narrator"), "user");
}

// ── Part helpers ────────────────────────────────────────────────────────

#[test]
fn convert_part_text_returns_some_for_text() {
    let p = Part::Text("hello".into());
    assert_eq!(part_text(&p), Some("hello"));
}

#[test]
fn convert_part_text_returns_none_for_inline_data() {
    let p = Part::InlineData {
        mime_type: "image/png".into(),
        data: "abc".into(),
    };
    assert!(part_text(&p).is_none());
}

#[test]
fn convert_part_text_returns_none_for_function_call() {
    let p = Part::FunctionCall {
        name: "f".into(),
        args: json!({}),
    };
    assert!(part_text(&p).is_none());
}

#[test]
fn convert_part_text_returns_none_for_function_response() {
    let p = Part::FunctionResponse {
        name: "f".into(),
        response: json!("ok"),
    };
    assert!(part_text(&p).is_none());
}

#[test]
fn convert_text_part_creates_text() {
    let p = convert::text_part("hello");
    assert_eq!(p, Part::Text("hello".into()));
}

#[test]
fn convert_text_part_from_string() {
    let s = String::from("world");
    let p = convert::text_part(s);
    assert_eq!(p, Part::Text("world".into()));
}

#[test]
fn convert_function_call_part_creates_function_call() {
    let p = convert::function_call_part("search", json!({"q": "rust"}));
    match p {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args["q"], "rust");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn convert_function_response_part_creates_function_response() {
    let p = convert::function_response_part("search", json!({"results": []}));
    match p {
        Part::FunctionResponse { name, response } => {
            assert_eq!(name, "search");
            assert_eq!(response["results"], json!([]));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn convert_part_to_json_text() {
    let p = Part::Text("hi".into());
    let v = part_to_json(&p);
    assert!(v.is_object() || v.is_string());
}

#[test]
fn convert_part_to_json_function_call() {
    let p = Part::FunctionCall {
        name: "f".into(),
        args: json!({"x": 1}),
    };
    let v = part_to_json(&p);
    assert!(!v.is_null());
}

// ── outcome_to_finish_reason ────────────────────────────────────────────

#[test]
fn convert_outcome_complete_to_stop() {
    assert_eq!(outcome_to_finish_reason(&Outcome::Complete), "STOP");
}

#[test]
fn convert_outcome_partial_to_max_tokens() {
    assert_eq!(outcome_to_finish_reason(&Outcome::Partial), "MAX_TOKENS");
}

#[test]
fn convert_outcome_failed_to_other() {
    assert_eq!(outcome_to_finish_reason(&Outcome::Failed), "OTHER");
}

// ── to_work_order ───────────────────────────────────────────────────────

#[test]
fn convert_to_work_order_extracts_task() {
    let req = GenerateContentRequest {
        contents: vec![Content {
            role: Some("user".into()),
            parts: vec![Part::Text("explain monads".into())],
        }],
        system_instruction: None,
        tools: None,
        tool_config: None,
        generation_config: None,
        safety_settings: None,
    };
    let wo = to_work_order(&req);
    assert_eq!(wo.task, "explain monads");
}

#[test]
fn convert_to_work_order_empty_contents_yields_empty_task() {
    let req = GenerateContentRequest {
        contents: vec![],
        system_instruction: None,
        tools: None,
        tool_config: None,
        generation_config: None,
        safety_settings: None,
    };
    let wo = to_work_order(&req);
    assert!(wo.task.is_empty());
}

#[test]
fn convert_to_work_order_stores_dialect_tag() {
    let req = GenerateContentRequest {
        contents: vec![Content {
            role: Some("user".into()),
            parts: vec![Part::Text("hi".into())],
        }],
        system_instruction: None,
        tools: None,
        tool_config: None,
        generation_config: None,
        safety_settings: None,
    };
    let wo = to_work_order(&req);
    assert_eq!(wo.config.vendor["dialect"], "gemini");
}

#[test]
fn convert_to_work_order_model_is_none() {
    let req = GenerateContentRequest {
        contents: vec![Content {
            role: Some("user".into()),
            parts: vec![Part::Text("hi".into())],
        }],
        system_instruction: None,
        tools: None,
        tool_config: None,
        generation_config: None,
        safety_settings: None,
    };
    let wo = to_work_order(&req);
    assert!(wo.config.model.is_none());
}

#[test]
fn convert_to_work_order_with_system_instruction() {
    let req = GenerateContentRequest {
        contents: vec![Content {
            role: Some("user".into()),
            parts: vec![Part::Text("hello".into())],
        }],
        system_instruction: Some(Content {
            role: None,
            parts: vec![Part::Text("Be helpful.".into())],
        }),
        tools: None,
        tool_config: None,
        generation_config: None,
        safety_settings: None,
    };
    let wo = to_work_order(&req);
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].name, "system_instruction");
    assert_eq!(wo.context.snippets[0].content, "Be helpful.");
}

#[test]
fn convert_to_work_order_preserves_contents_in_vendor() {
    let req = GenerateContentRequest {
        contents: vec![Content {
            role: Some("user".into()),
            parts: vec![Part::Text("hi".into())],
        }],
        system_instruction: None,
        tools: None,
        tool_config: None,
        generation_config: None,
        safety_settings: None,
    };
    let wo = to_work_order(&req);
    assert!(wo.config.vendor.contains_key("contents"));
}

// ── from_receipt ────────────────────────────────────────────────────────

#[test]
fn convert_from_receipt_assistant_message() {
    let wo = WorkOrderBuilder::new("test").build();
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "response".into(),
            },
            ext: None,
        })
        .build();
    let resp = from_receipt(&receipt, &wo);
    assert_eq!(resp.candidates.len(), 1);
    assert_eq!(resp.candidates[0].content.role.as_deref(), Some("model"));
    match &resp.candidates[0].content.parts[0] {
        Part::Text(t) => assert_eq!(t, "response"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn convert_from_receipt_finish_reason_stop() {
    let wo = WorkOrderBuilder::new("test").build();
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .build();
    let resp = from_receipt(&receipt, &wo);
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
}

#[test]
fn convert_from_receipt_usage_metadata() {
    let wo = WorkOrderBuilder::new("test").build();
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(200),
            output_tokens: Some(100),
            ..Default::default()
        })
        .build();
    let resp = from_receipt(&receipt, &wo);
    let meta = resp.usage_metadata.unwrap();
    assert_eq!(meta.prompt_token_count, 200);
    assert_eq!(meta.candidates_token_count, 100);
    assert_eq!(meta.total_token_count, 300);
}

#[test]
fn convert_from_receipt_no_usage_when_empty() {
    let wo = WorkOrderBuilder::new("test").build();
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .build();
    let resp = from_receipt(&receipt, &wo);
    assert!(resp.usage_metadata.is_none());
}

// ── from_agent_event ────────────────────────────────────────────────────

#[test]
fn convert_from_agent_event_assistant_delta() {
    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "streaming".into(),
        },
        ext: None,
    };
    let val = from_agent_event(&event).unwrap();
    assert_eq!(
        val["candidates"][0]["content"]["parts"][0]["text"],
        "streaming"
    );
}

#[test]
fn convert_from_agent_event_tool_call() {
    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({"path": "a.rs"}),
        },
        ext: None,
    };
    let val = from_agent_event(&event).unwrap();
    let fc = &val["candidates"][0]["content"]["parts"][0]["functionCall"];
    assert_eq!(fc["name"], "read");
    assert_eq!(fc["args"]["path"], "a.rs");
}

#[test]
fn convert_from_agent_event_run_completed() {
    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    };
    let val = from_agent_event(&event).unwrap();
    assert_eq!(val["candidates"][0]["finishReason"], "STOP");
}

#[test]
fn convert_from_agent_event_warning_returns_none() {
    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::Warning {
            message: "warn".into(),
        },
        ext: None,
    };
    assert!(from_agent_event(&event).is_none());
}

#[test]
fn convert_from_agent_event_run_started_returns_none() {
    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "start".into(),
        },
        ext: None,
    };
    assert!(from_agent_event(&event).is_none());
}

#[test]
fn convert_from_agent_event_error_returns_none() {
    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::Error {
            message: "err".into(),
            error_code: None,
        },
        ext: None,
    };
    assert!(from_agent_event(&event).is_none());
}

// ── generation_config_to_map ────────────────────────────────────────────

#[test]
fn convert_generation_config_to_map_full() {
    let cfg = GenerationConfig {
        temperature: Some(0.5),
        top_p: Some(0.9),
        top_k: Some(40),
        max_output_tokens: Some(2048),
        candidate_count: Some(2),
        stop_sequences: None,
    };
    let map = generation_config_to_map(&cfg);
    assert_eq!(map.len(), 5);
    assert_eq!(map["temperature"], json!(0.5));
    assert_eq!(map["top_k"], json!(40));
}

#[test]
fn convert_generation_config_to_map_empty_config() {
    let cfg = GenerationConfig::default();
    let map = generation_config_to_map(&cfg);
    assert!(map.is_empty());
}

#[test]
fn convert_generation_config_to_map_partial() {
    let cfg = GenerationConfig {
        temperature: Some(1.0),
        ..Default::default()
    };
    let map = generation_config_to_map(&cfg);
    assert_eq!(map.len(), 1);
    assert!(map.contains_key("temperature"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 5: conversions.rs (From impls)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn conversions_gemini_request_to_work_order() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hello".into())],
        }],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    };
    let wo: abp_core::WorkOrder = req.into();
    assert_eq!(wo.task, "Hello");
    assert_eq!(wo.config.model.as_deref(), Some("gemini-2.5-flash"));
}

#[test]
fn conversions_receipt_to_gemini_response_complete() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "done".into(),
            },
            ext: None,
        })
        .build();
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates.len(), 1);
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
}

#[test]
fn conversions_receipt_to_gemini_response_partial() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Partial)
        .build();
    let resp: GeminiResponse = receipt.into();
    assert_eq!(
        resp.candidates[0].finish_reason.as_deref(),
        Some("MAX_TOKENS")
    );
}

#[test]
fn conversions_receipt_to_gemini_response_failed() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Failed)
        .build();
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("OTHER"));
}

#[test]
fn conversions_receipt_with_usage() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(25),
            ..Default::default()
        })
        .build();
    let resp: GeminiResponse = receipt.into();
    let meta = resp.usage_metadata.unwrap();
    assert_eq!(meta.prompt_token_count, 50);
    assert_eq!(meta.candidates_token_count, 25);
    assert_eq!(meta.total_token_count, 75);
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 6: lowering.rs (IR conversions)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lowering_user_text_to_ir_and_back() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hello".into())],
    }];
    let conv = to_ir(&contents, None);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello");

    let back = from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

#[test]
fn lowering_model_text_to_ir_becomes_assistant() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text("Reply".into())],
    }];
    let conv = to_ir(&contents, None);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);

    let back = from_ir(&conv);
    assert_eq!(back[0].role, "model");
}

#[test]
fn lowering_system_instruction_prepended() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Be concise".into())],
    };
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hi".into())],
    }];
    let conv = to_ir(&contents, Some(&sys));
    assert_eq!(conv.messages.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "Be concise");
}

#[test]
fn lowering_system_messages_skipped_in_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "instructions"),
        IrMessage::text(IrRole::User, "hello"),
    ]);
    let back = from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

#[test]
fn lowering_extract_system_instruction_present() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be helpful"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let sys = extract_system_instruction(&conv).unwrap();
    match &sys.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Be helpful"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn lowering_extract_system_instruction_absent() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
    assert!(extract_system_instruction(&conv).is_none());
}

#[test]
fn lowering_empty_contents_roundtrip() {
    let conv = to_ir(&[], None);
    assert!(conv.messages.is_empty());
    let back = from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn lowering_function_call_to_ir() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "search".into(),
            args: json!({"query": "rust"}),
        }],
    }];
    let conv = to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { name, input, id } => {
            assert_eq!(name, "search");
            assert_eq!(input, &json!({"query": "rust"}));
            assert_eq!(id, "gemini_search");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn lowering_function_response_to_ir() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "search".into(),
            response: json!("results"),
        }],
    }];
    let conv = to_ir(&contents, None);
    match &conv.messages[0].content[0] {
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
fn lowering_inline_data_to_ir() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/jpeg".into(),
            data: "base64data".into(),
        })],
    }];
    let conv = to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/jpeg");
            assert_eq!(data, "base64data");
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

#[test]
fn lowering_inline_data_roundtrip() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "xyz".into(),
        })],
    }];
    let conv = to_ir(&contents, None);
    let back = from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/png");
            assert_eq!(d.data, "xyz");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn lowering_multi_turn_conversation() {
    let contents = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hi".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("Hello!".into())],
        },
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Bye".into())],
        },
    ];
    let conv = to_ir(&contents, None);
    assert_eq!(conv.messages.len(), 3);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[1].role, IrRole::Assistant);
    assert_eq!(conv.messages[2].role, IrRole::User);

    let back = from_ir(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[1].role, "model");
}

#[test]
fn lowering_empty_system_instruction_not_added() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![],
    };
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("hi".into())],
    }];
    let conv = to_ir(&contents, Some(&sys));
    assert_eq!(conv.messages.len(), 1);
}

#[test]
fn lowering_function_call_roundtrip() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "read".into(),
            args: json!({"file": "a.rs"}),
        }],
    }];
    let conv = to_ir(&contents, None);
    let back = from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "read");
            assert_eq!(args, &json!({"file": "a.rs"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn lowering_function_response_with_object_payload() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "api".into(),
            response: json!({"status": 200}),
        }],
    }];
    let conv = to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => {
            assert_eq!(content.len(), 1);
            match &content[0] {
                IrContentBlock::Text { text } => assert!(text.contains("200")),
                _ => panic!("expected text block"),
            }
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 7: Edge cases and cross-module roundtrips
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_case_empty_text_part() {
    let p = Part::Text("".into());
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn edge_case_unicode_text_part() {
    let p = Part::Text("日本語テスト 🦀".into());
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn edge_case_large_function_args() {
    let big_obj: serde_json::Value = (0..100)
        .map(|i| (format!("key_{i}"), json!(i)))
        .collect::<serde_json::Map<String, serde_json::Value>>()
        .into();
    let p = Part::FunctionCall {
        name: "big".into(),
        args: big_obj,
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn edge_case_null_function_args() {
    let p = Part::FunctionCall {
        name: "f".into(),
        args: json!(null),
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn edge_case_nested_function_response() {
    let p = Part::FunctionResponse {
        name: "f".into(),
        response: json!({"a": {"b": {"c": [1, 2, 3]}}}),
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn edge_case_generation_config_skip_serializing_none_fields() {
    let cfg = GenerationConfig {
        temperature: Some(0.5),
        ..Default::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    // None fields should be skipped due to skip_serializing_if
    assert!(!json.contains("topP"));
    assert!(!json.contains("topK"));
    assert!(!json.contains("maxOutputTokens"));
    assert!(json.contains("temperature"));
}

#[test]
fn edge_case_multiple_candidates_in_response() {
    let resp = GenerateContentResponse {
        candidates: vec![
            Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::Text("option A".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            },
            Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::Text("option B".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            },
        ],
        usage_metadata: None,
        prompt_feedback: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: GenerateContentResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.candidates.len(), 2);
}

#[test]
fn edge_case_multiple_parts_in_content() {
    let c = Content {
        role: Some("user".into()),
        parts: vec![
            Part::Text("Look at this image:".into()),
            Part::InlineData {
                mime_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    };
    let json = serde_json::to_string(&c).unwrap();
    let back: Content = serde_json::from_str(&json).unwrap();
    assert_eq!(back.parts.len(), 2);
}

#[test]
fn edge_case_multiple_safety_settings() {
    let settings = vec![
        SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        },
        SafetySetting {
            category: HarmCategory::HarmCategoryHateSpeech,
            threshold: HarmBlockThreshold::BlockLowAndAbove,
        },
        SafetySetting {
            category: HarmCategory::HarmCategorySexuallyExplicit,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        },
        SafetySetting {
            category: HarmCategory::HarmCategoryDangerousContent,
            threshold: HarmBlockThreshold::BlockOnlyHigh,
        },
        SafetySetting {
            category: HarmCategory::HarmCategoryCivicIntegrity,
            threshold: HarmBlockThreshold::BlockNone,
        },
    ];
    let json = serde_json::to_string(&settings).unwrap();
    let back: Vec<SafetySetting> = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), 5);
    for (orig, deserialized) in settings.iter().zip(back.iter()) {
        assert_eq!(orig, deserialized);
    }
}

#[test]
fn edge_case_gemini_tool_multiple_declarations() {
    let tool = GeminiTool {
        function_declarations: vec![
            FunctionDeclaration {
                name: "search".into(),
                description: "Search".into(),
                parameters: json!({"type": "object"}),
            },
            FunctionDeclaration {
                name: "read".into(),
                description: "Read a file".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            },
        ],
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: GeminiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back.function_declarations.len(), 2);
}

#[test]
fn crossmodule_roundtrip_request_through_work_order_to_response() {
    // Build a Gemini request using types.rs
    let req = GenerateContentRequest {
        contents: vec![Content {
            role: Some("user".into()),
            parts: vec![Part::Text("Hello Gemini".into())],
        }],
        system_instruction: None,
        tools: None,
        tool_config: None,
        generation_config: None,
        safety_settings: None,
    };

    // Convert to work order using convert.rs
    let wo = to_work_order(&req);
    assert_eq!(wo.task, "Hello Gemini");

    // Build a receipt as if the backend processed it
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .work_order_id(wo.id)
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hi there!".into(),
            },
            ext: None,
        })
        .build();

    // Convert back to response
    let resp = from_receipt(&receipt, &wo);
    assert_eq!(resp.candidates.len(), 1);
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
    match &resp.candidates[0].content.parts[0] {
        Part::Text(t) => assert_eq!(t, "Hi there!"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn crossmodule_dialect_request_through_conversions() {
    // Build a dialect GeminiRequest
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Test".into())],
        }],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    };

    // Use the From impl to convert to WorkOrder
    let wo: abp_core::WorkOrder = req.into();
    assert_eq!(wo.task, "Test");

    // Build receipt and convert via From
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Result".into(),
            },
            ext: None,
        })
        .build();
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
    assert_eq!(resp.candidates[0].content.role, "model");
}

#[test]
fn dialect_harm_category_all_variants_serde() {
    let variants = vec![
        dialect::HarmCategory::HarmCategoryHarassment,
        dialect::HarmCategory::HarmCategoryHateSpeech,
        dialect::HarmCategory::HarmCategorySexuallyExplicit,
        dialect::HarmCategory::HarmCategoryDangerousContent,
        dialect::HarmCategory::HarmCategoryCivicIntegrity,
    ];
    for v in &variants {
        let json = serde_json::to_string(v).unwrap();
        let back: dialect::HarmCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(*v, back);
    }
}

#[test]
fn dialect_harm_block_threshold_all_variants_serde() {
    let variants = vec![
        dialect::HarmBlockThreshold::BlockNone,
        dialect::HarmBlockThreshold::BlockLowAndAbove,
        dialect::HarmBlockThreshold::BlockMediumAndAbove,
        dialect::HarmBlockThreshold::BlockOnlyHigh,
    ];
    for v in &variants {
        let json = serde_json::to_string(v).unwrap();
        let back: dialect::HarmBlockThreshold = serde_json::from_str(&json).unwrap();
        assert_eq!(*v, back);
    }
}

#[test]
fn dialect_harm_probability_all_variants_serde() {
    let variants = vec![
        dialect::HarmProbability::Negligible,
        dialect::HarmProbability::Low,
        dialect::HarmProbability::Medium,
        dialect::HarmProbability::High,
    ];
    for v in &variants {
        let json = serde_json::to_string(v).unwrap();
        let back: dialect::HarmProbability = serde_json::from_str(&json).unwrap();
        assert_eq!(*v, back);
    }
}

#[test]
fn dialect_stream_chunk_with_usage_metadata() {
    let chunk = GeminiStreamChunk {
        candidates: vec![],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 5,
            total_token_count: 15,
        }),
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: GeminiStreamChunk = serde_json::from_str(&json).unwrap();
    assert!(back.usage_metadata.is_some());
    assert_eq!(back.usage_metadata.unwrap().total_token_count, 15);
}

#[test]
fn dialect_gemini_candidate_with_citation_metadata() {
    let candidate = GeminiCandidate {
        content: GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("cited text".into())],
        },
        finish_reason: Some("STOP".into()),
        safety_ratings: None,
        citation_metadata: Some(GeminiCitationMetadata {
            citation_sources: vec![GeminiCitationSource {
                start_index: Some(0),
                end_index: Some(5),
                uri: Some("https://example.com".into()),
                license: None,
            }],
        }),
    };
    let json = serde_json::to_string(&candidate).unwrap();
    let back: GeminiCandidate = serde_json::from_str(&json).unwrap();
    assert!(back.citation_metadata.is_some());
}

#[test]
fn types_content_empty_parts() {
    let c = Content {
        role: Some("user".into()),
        parts: vec![],
    };
    let json = serde_json::to_string(&c).unwrap();
    let back: Content = serde_json::from_str(&json).unwrap();
    assert!(back.parts.is_empty());
}
