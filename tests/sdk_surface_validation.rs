// SPDX-License-Identifier: MIT OR Apache-2.0
//! SDK Surface Area Validation Tests
//!
//! Verifies that each dialect SDK crate provides the required lowering
//! functions (to_ir / from_ir), detection heuristics, native request
//! format handling, standard parameter mapping, tool definition
//! consistency, streaming event mapping, and documents missing features.

// ── Imports ─────────────────────────────────────────────────────────────

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEventKind, WorkOrderBuilder};
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// Module 1: Lowering function existence — every SDK has to_ir + from_ir
// ═══════════════════════════════════════════════════════════════════════

mod lowering_functions_exist {
    use super::*;

    // ── OpenAI ──────────────────────────────────────────────────────────

    #[test]
    fn openai_to_ir_exists_and_works() {
        use abp_openai_sdk::dialect::OpenAIMessage;
        use abp_openai_sdk::lowering::{from_ir, to_ir};

        let msgs = vec![OpenAIMessage {
            role: "user".into(),
            content: Some("hello".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);

        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
    }

    // ── Claude ──────────────────────────────────────────────────────────

    #[test]
    fn claude_to_ir_exists_and_works() {
        use abp_claude_sdk::dialect::ClaudeMessage;
        use abp_claude_sdk::lowering::{from_ir, to_ir};

        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "hello".into(),
        }];
        let conv = to_ir(&msgs, None);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);

        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
    }

    // ── Gemini ──────────────────────────────────────────────────────────

    #[test]
    fn gemini_to_ir_exists_and_works() {
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
        use abp_gemini_sdk::lowering::{from_ir, to_ir};

        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("hello".into())],
        }];
        let conv = to_ir(&contents, None);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);

        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
    }

    // ── Codex ───────────────────────────────────────────────────────────

    #[test]
    fn codex_to_ir_exists_and_works() {
        use abp_codex_sdk::dialect::{CodexContentPart, CodexResponseItem};
        use abp_codex_sdk::lowering::{from_ir, to_ir};

        let items = vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "hello".into(),
            }],
        }];
        let conv = to_ir(&items);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);

        let back = from_ir(&conv);
        assert!(!back.is_empty());
    }

    #[test]
    fn codex_input_to_ir_exists() {
        use abp_codex_sdk::dialect::CodexInputItem;
        use abp_codex_sdk::lowering::input_to_ir;

        let items = vec![CodexInputItem::Message {
            role: "user".into(),
            content: "hello".into(),
        }];
        let conv = input_to_ir(&items);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    // ── Kimi ────────────────────────────────────────────────────────────

    #[test]
    fn kimi_to_ir_exists_and_works() {
        use abp_kimi_sdk::dialect::KimiMessage;
        use abp_kimi_sdk::lowering::{from_ir, to_ir};

        let msgs = vec![KimiMessage {
            role: "user".into(),
            content: Some("hello".into()),
            tool_call_id: None,
            tool_calls: None,
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);

        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
    }

    // ── Copilot ─────────────────────────────────────────────────────────

    #[test]
    fn copilot_to_ir_exists_and_works() {
        use abp_copilot_sdk::dialect::CopilotMessage;
        use abp_copilot_sdk::lowering::{from_ir, to_ir};

        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "hello".into(),
            name: None,
            copilot_references: vec![],
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);

        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 2: Dialect detection — each dialect correctly identifies itself
// ═══════════════════════════════════════════════════════════════════════

mod dialect_detection {
    use super::*;

    #[test]
    fn openai_request_detected() {
        let payload = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "temperature": 0.7
        });
        let detector = DialectDetector::new();
        let result = detector.detect(&payload).expect("should detect");
        assert_eq!(result.dialect, Dialect::OpenAi);
        assert!(result.confidence > 0.3);
    }

    #[test]
    fn openai_response_detected() {
        let payload = json!({
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}}],
            "model": "gpt-4o"
        });
        let detector = DialectDetector::new();
        let result = detector.detect(&payload).expect("should detect");
        assert_eq!(result.dialect, Dialect::OpenAi);
    }

    #[test]
    fn claude_request_detected() {
        let payload = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
            "max_tokens": 4096
        });
        let detector = DialectDetector::new();
        let result = detector.detect(&payload).expect("should detect");
        assert_eq!(result.dialect, Dialect::Claude);
    }

    #[test]
    fn claude_response_detected() {
        let payload = json!({
            "type": "message",
            "model": "claude-sonnet-4-20250514",
            "role": "assistant",
            "content": [{"type": "text", "text": "hello"}],
            "stop_reason": "end_turn"
        });
        let detector = DialectDetector::new();
        let result = detector.detect(&payload).expect("should detect");
        assert_eq!(result.dialect, Dialect::Claude);
    }

    #[test]
    fn gemini_request_detected() {
        let payload = json!({
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "generationConfig": {"temperature": 0.7}
        });
        let detector = DialectDetector::new();
        let result = detector.detect(&payload).expect("should detect");
        assert_eq!(result.dialect, Dialect::Gemini);
    }

    #[test]
    fn gemini_response_detected() {
        let payload = json!({
            "candidates": [{"content": {"role": "model", "parts": [{"text": "hi"}]}}]
        });
        let detector = DialectDetector::new();
        let result = detector.detect(&payload).expect("should detect");
        assert_eq!(result.dialect, Dialect::Gemini);
    }

    #[test]
    fn codex_response_detected() {
        let payload = json!({
            "items": [{"type": "message", "role": "assistant", "content": []}],
            "status": "completed",
            "object": "response"
        });
        let detector = DialectDetector::new();
        let result = detector.detect(&payload).expect("should detect");
        assert_eq!(result.dialect, Dialect::Codex);
    }

    #[test]
    fn kimi_request_detected() {
        let payload = json!({
            "model": "moonshot-v1-8k",
            "messages": [{"role": "user", "content": "hi"}],
            "refs": [{"id": "1", "url": "https://example.com"}],
            "search_plus": true
        });
        let detector = DialectDetector::new();
        let result = detector.detect(&payload).expect("should detect");
        assert_eq!(result.dialect, Dialect::Kimi);
    }

    #[test]
    fn copilot_request_detected() {
        let payload = json!({
            "messages": [{"role": "user", "content": "hi"}],
            "references": [{"type": "file", "id": "f1"}],
            "agent_mode": true
        });
        let detector = DialectDetector::new();
        let result = detector.detect(&payload).expect("should detect");
        assert_eq!(result.dialect, Dialect::Copilot);
    }

    #[test]
    fn all_dialects_have_detection_heuristics() {
        // Ensure detect_all doesn't panic for an ambiguous payload
        let payload = json!({"model": "test", "messages": []});
        let detector = DialectDetector::new();
        let _results = detector.detect_all(&payload);
        // No panic = all scoring functions are wired up
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 3: Native request format validation
// ═══════════════════════════════════════════════════════════════════════

mod native_format_validation {
    use super::*;

    #[test]
    fn openai_valid_request() {
        let payload = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let validator = DialectValidator::new();
        let result = validator.validate(&payload, Dialect::OpenAi);
        assert!(result.valid, "errors: {:?}", result.errors);
    }

    #[test]
    fn openai_missing_model() {
        let payload = json!({
            "messages": [{"role": "user", "content": "hi"}]
        });
        let validator = DialectValidator::new();
        let result = validator.validate(&payload, Dialect::OpenAi);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.path.contains("model")));
    }

    #[test]
    fn claude_valid_request() {
        let payload = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        });
        let validator = DialectValidator::new();
        let result = validator.validate(&payload, Dialect::Claude);
        assert!(result.valid, "errors: {:?}", result.errors);
    }

    #[test]
    fn gemini_valid_request() {
        let payload = json!({
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
        });
        let validator = DialectValidator::new();
        let result = validator.validate(&payload, Dialect::Gemini);
        assert!(result.valid, "errors: {:?}", result.errors);
    }

    #[test]
    fn codex_valid_response() {
        let payload = json!({
            "items": [{"type": "message", "role": "assistant"}],
            "status": "completed"
        });
        let validator = DialectValidator::new();
        let result = validator.validate(&payload, Dialect::Codex);
        assert!(result.valid, "errors: {:?}", result.errors);
    }

    #[test]
    fn kimi_valid_request() {
        let payload = json!({
            "model": "moonshot-v1-8k",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let validator = DialectValidator::new();
        let result = validator.validate(&payload, Dialect::Kimi);
        assert!(result.valid, "errors: {:?}", result.errors);
    }

    #[test]
    fn copilot_valid_request() {
        let payload = json!({
            "messages": [{"role": "user", "content": "hi"}]
        });
        let validator = DialectValidator::new();
        let result = validator.validate(&payload, Dialect::Copilot);
        assert!(result.valid, "errors: {:?}", result.errors);
    }

    #[test]
    fn non_object_fails_all_dialects() {
        let payload = json!([1, 2, 3]);
        let validator = DialectValidator::new();
        for &dialect in Dialect::all() {
            let result = validator.validate(&payload, dialect);
            assert!(!result.valid, "{dialect} should reject array input");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 4: Standard parameter mapping (temperature, max_tokens, stop)
// ═══════════════════════════════════════════════════════════════════════

mod standard_parameters {
    use super::*;

    #[test]
    fn openai_maps_temperature_and_max_tokens() {
        use abp_openai_sdk::dialect::{OpenAIConfig, map_work_order};

        let wo = WorkOrderBuilder::new("test task").build();
        let config = OpenAIConfig {
            temperature: Some(0.5),
            max_tokens: Some(2048),
            ..OpenAIConfig::default()
        };
        let req = map_work_order(&wo, &config);
        assert_eq!(req.temperature, Some(0.5));
        assert_eq!(req.max_tokens, Some(2048));
    }

    #[test]
    fn claude_maps_max_tokens() {
        use abp_claude_sdk::dialect::{ClaudeConfig, map_work_order};

        let wo = WorkOrderBuilder::new("test task").build();
        let config = ClaudeConfig {
            max_tokens: 2048,
            ..ClaudeConfig::default()
        };
        let req = map_work_order(&wo, &config);
        assert_eq!(req.max_tokens, 2048);
    }

    #[test]
    fn gemini_maps_temperature_and_max_tokens() {
        use abp_gemini_sdk::dialect::{GeminiConfig, map_work_order};

        let wo = WorkOrderBuilder::new("test task").build();
        let config = GeminiConfig {
            temperature: Some(0.3),
            max_output_tokens: Some(1024),
            ..GeminiConfig::default()
        };
        let req = map_work_order(&wo, &config);
        let gen_config = req
            .generation_config
            .expect("should have generation_config");
        assert_eq!(gen_config.temperature, Some(0.3));
        assert_eq!(gen_config.max_output_tokens, Some(1024));
    }

    #[test]
    fn codex_maps_temperature_and_max_tokens() {
        use abp_codex_sdk::dialect::{CodexConfig, map_work_order};

        let wo = WorkOrderBuilder::new("test task").build();
        let config = CodexConfig {
            temperature: Some(0.8),
            max_output_tokens: Some(3000),
            ..CodexConfig::default()
        };
        let req = map_work_order(&wo, &config);
        assert_eq!(req.temperature, Some(0.8));
        assert_eq!(req.max_output_tokens, Some(3000));
    }

    #[test]
    fn kimi_maps_temperature_and_max_tokens() {
        use abp_kimi_sdk::dialect::{KimiConfig, map_work_order};

        let wo = WorkOrderBuilder::new("test task").build();
        let config = KimiConfig {
            temperature: Some(0.6),
            max_tokens: Some(2000),
            ..KimiConfig::default()
        };
        let req = map_work_order(&wo, &config);
        assert_eq!(req.temperature, Some(0.6));
        assert_eq!(req.max_tokens, Some(2000));
    }

    #[test]
    fn copilot_has_no_temperature_or_max_tokens_config() {
        // Document: CopilotConfig does not expose temperature or max_tokens;
        // these are server-side concerns in the Copilot API.
        use abp_copilot_sdk::dialect::{CopilotConfig, map_work_order};

        let wo = WorkOrderBuilder::new("test task").build();
        let config = CopilotConfig::default();
        let req = map_work_order(&wo, &config);
        // CopilotRequest carries model + messages but no sampling params
        assert!(!req.model.is_empty());
    }

    #[test]
    fn all_dialects_respect_model_override_from_work_order() {
        let wo = WorkOrderBuilder::new("test").model("custom-model").build();

        {
            use abp_openai_sdk::dialect::{OpenAIConfig, map_work_order};
            let req = map_work_order(&wo, &OpenAIConfig::default());
            assert_eq!(req.model, "custom-model");
        }
        {
            use abp_claude_sdk::dialect::{ClaudeConfig, map_work_order};
            let req = map_work_order(&wo, &ClaudeConfig::default());
            assert_eq!(req.model, "custom-model");
        }
        {
            use abp_gemini_sdk::dialect::{GeminiConfig, map_work_order};
            let req = map_work_order(&wo, &GeminiConfig::default());
            assert_eq!(req.model, "custom-model");
        }
        {
            use abp_codex_sdk::dialect::{CodexConfig, map_work_order};
            let req = map_work_order(&wo, &CodexConfig::default());
            assert_eq!(req.model, "custom-model");
        }
        {
            use abp_kimi_sdk::dialect::{KimiConfig, map_work_order};
            let req = map_work_order(&wo, &KimiConfig::default());
            assert_eq!(req.model, "custom-model");
        }
        {
            use abp_copilot_sdk::dialect::{CopilotConfig, map_work_order};
            let req = map_work_order(&wo, &CopilotConfig::default());
            assert_eq!(req.model, "custom-model");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 5: Tool definition consistency
// ═══════════════════════════════════════════════════════════════════════

mod tool_definitions {
    use super::*;

    #[test]
    fn openai_tool_def_roundtrip() {
        use abp_openai_sdk::dialect::{CanonicalToolDef, tool_def_from_openai, tool_def_to_openai};

        let def = CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        };
        let oai = tool_def_to_openai(&def);
        assert_eq!(oai.tool_type, "function");
        assert_eq!(oai.function.name, "read_file");

        let back = tool_def_from_openai(&oai);
        assert_eq!(back.name, def.name);
        assert_eq!(back.description, def.description);
        assert_eq!(back.parameters_schema, def.parameters_schema);
    }

    #[test]
    fn claude_tool_def_roundtrip() {
        use abp_claude_sdk::dialect::{CanonicalToolDef, tool_def_from_claude, tool_def_to_claude};

        let def = CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: json!({"type": "object"}),
        };
        let claude = tool_def_to_claude(&def);
        assert_eq!(claude.name, "read_file");
        assert_eq!(claude.input_schema, json!({"type": "object"}));

        let back = tool_def_from_claude(&claude);
        assert_eq!(back.name, def.name);
        assert_eq!(back.parameters_schema, def.parameters_schema);
    }

    #[test]
    fn gemini_tool_def_roundtrip() {
        use abp_gemini_sdk::dialect::{CanonicalToolDef, tool_def_from_gemini, tool_def_to_gemini};

        let def = CanonicalToolDef {
            name: "search".into(),
            description: "Search files".into(),
            parameters_schema: json!({"type": "object"}),
        };
        let gemini = tool_def_to_gemini(&def);
        assert_eq!(gemini.name, "search");
        assert_eq!(gemini.parameters, json!({"type": "object"}));

        let back = tool_def_from_gemini(&gemini);
        assert_eq!(back.name, def.name);
    }

    #[test]
    fn codex_tool_def_roundtrip() {
        use abp_codex_sdk::dialect::{CanonicalToolDef, tool_def_from_codex, tool_def_to_codex};

        let def = CanonicalToolDef {
            name: "write_file".into(),
            description: "Write a file".into(),
            parameters_schema: json!({"type": "object"}),
        };
        let codex = tool_def_to_codex(&def);
        assert_eq!(codex.function.name, "write_file");

        let back = tool_def_from_codex(&codex);
        assert_eq!(back.name, def.name);
    }

    #[test]
    fn kimi_tool_def_roundtrip() {
        use abp_kimi_sdk::dialect::{CanonicalToolDef, tool_def_from_kimi, tool_def_to_kimi};

        let def = CanonicalToolDef {
            name: "search".into(),
            description: "Search files".into(),
            parameters_schema: json!({"type": "object"}),
        };
        let kimi = tool_def_to_kimi(&def);
        assert_eq!(kimi.function.name, "search");

        let back = tool_def_from_kimi(&kimi);
        assert_eq!(back.name, def.name);
    }

    #[test]
    fn copilot_tool_def_roundtrip() {
        use abp_copilot_sdk::dialect::{
            CanonicalToolDef, tool_def_from_copilot, tool_def_to_copilot,
        };

        let def = CanonicalToolDef {
            name: "bash".into(),
            description: "Run shell command".into(),
            parameters_schema: json!({"type": "object"}),
        };
        let copilot = tool_def_to_copilot(&def);
        let back = tool_def_from_copilot(&copilot);
        let back = back.expect("should produce a canonical def");
        assert_eq!(back.name, def.name);
    }

    #[test]
    fn all_tool_defs_preserve_name_description_schema() {
        // Verify the three canonical fields survive roundtrip for every SDK
        let schema = json!({"type": "object", "properties": {"x": {"type": "integer"}}});
        let name = "test_tool";
        let desc = "A test tool";

        // OpenAI
        {
            use abp_openai_sdk::dialect::{
                CanonicalToolDef, tool_def_from_openai, tool_def_to_openai,
            };
            let def = CanonicalToolDef {
                name: name.into(),
                description: desc.into(),
                parameters_schema: schema.clone(),
            };
            let back = tool_def_from_openai(&tool_def_to_openai(&def));
            assert_eq!(back.name, name);
            assert_eq!(back.description, desc);
            assert_eq!(back.parameters_schema, schema);
        }
        // Claude
        {
            use abp_claude_sdk::dialect::{
                CanonicalToolDef, tool_def_from_claude, tool_def_to_claude,
            };
            let def = CanonicalToolDef {
                name: name.into(),
                description: desc.into(),
                parameters_schema: schema.clone(),
            };
            let back = tool_def_from_claude(&tool_def_to_claude(&def));
            assert_eq!(back.name, name);
            assert_eq!(back.description, desc);
            assert_eq!(back.parameters_schema, schema);
        }
        // Gemini
        {
            use abp_gemini_sdk::dialect::{
                CanonicalToolDef, tool_def_from_gemini, tool_def_to_gemini,
            };
            let def = CanonicalToolDef {
                name: name.into(),
                description: desc.into(),
                parameters_schema: schema.clone(),
            };
            let back = tool_def_from_gemini(&tool_def_to_gemini(&def));
            assert_eq!(back.name, name);
            assert_eq!(back.description, desc);
            assert_eq!(back.parameters_schema, schema);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 6: Streaming event mapping to AgentEvent
// ═══════════════════════════════════════════════════════════════════════

mod streaming_events {
    use super::*;

    #[test]
    fn openai_text_delta_maps_to_assistant_delta() {
        use abp_openai_sdk::streaming::{ChatCompletionChunk, ChunkChoice, ChunkDelta, map_chunk};

        let chunk = ChatCompletionChunk {
            id: "chatcmpl-1".into(),
            object: "chat.completion.chunk".into(),
            created: 0,
            model: "gpt-4o".into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: None,
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        let events = map_chunk(&chunk);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantDelta { text } if text == "Hello"
        ));
    }

    #[test]
    fn openai_tool_call_accumulator_produces_tool_call_events() {
        use abp_openai_sdk::streaming::{ChunkFunctionCall, ChunkToolCall, ToolCallAccumulator};

        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: Some("call_1".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("read_file".into()),
                arguments: Some(r#"{"path":"#.into()),
            }),
        }]);
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: None,
            call_type: None,
            function: Some(ChunkFunctionCall {
                name: None,
                arguments: Some(r#""main.rs"}"#.into()),
            }),
        }]);

        let events = acc.finish();
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("call_1"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn claude_text_delta_maps_to_assistant_delta() {
        use abp_claude_sdk::dialect::{ClaudeStreamDelta, ClaudeStreamEvent, map_stream_event};

        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "world".into(),
            },
        };
        let events = map_stream_event(&event);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantDelta { text } if text == "world"
        ));
    }

    #[test]
    fn claude_message_start_maps_to_run_started() {
        use abp_claude_sdk::dialect::{ClaudeResponse, ClaudeStreamEvent, map_stream_event};

        let event = ClaudeStreamEvent::MessageStart {
            message: ClaudeResponse {
                id: "msg_1".into(),
                model: "claude-sonnet-4-20250514".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: None,
            },
        };
        let events = map_stream_event(&event);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }))
        );
    }

    #[test]
    fn claude_message_stop_maps_to_run_completed() {
        use abp_claude_sdk::dialect::{ClaudeStreamEvent, map_stream_event};

        let event = ClaudeStreamEvent::MessageStop {};
        let events = map_stream_event(&event);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
        );
    }

    #[test]
    fn gemini_text_delta_maps_to_assistant_delta() {
        use abp_gemini_sdk::dialect::{
            GeminiCandidate, GeminiContent, GeminiPart, GeminiStreamChunk, map_stream_chunk,
        };

        let chunk = GeminiStreamChunk {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("Hi".into())],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        let events = map_stream_chunk(&chunk);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantDelta { text } if text == "Hi"
        ));
    }

    #[test]
    fn codex_text_delta_maps_to_assistant_delta() {
        use abp_codex_sdk::dialect::{CodexStreamDelta, CodexStreamEvent, map_stream_event};

        let event = CodexStreamEvent::OutputItemDelta {
            output_index: 0,
            delta: CodexStreamDelta::OutputTextDelta {
                text: "code".into(),
            },
        };
        let events = map_stream_event(&event);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::AssistantDelta { .. }))
        );
    }

    #[test]
    fn kimi_text_delta_maps_to_assistant_delta() {
        use abp_kimi_sdk::dialect::{KimiChunk, KimiChunkChoice, KimiChunkDelta, map_stream_event};

        let chunk = KimiChunk {
            id: "chunk_1".into(),
            object: "chat.completion.chunk".into(),
            created: 0,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChunkChoice {
                index: 0,
                delta: KimiChunkDelta {
                    role: None,
                    content: Some("response".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        };
        let events = map_stream_event(&chunk);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantDelta { text } if text == "response"
        ));
    }

    #[test]
    fn copilot_text_delta_maps_to_assistant_delta() {
        use abp_copilot_sdk::dialect::{CopilotStreamEvent, map_stream_event};

        let event = CopilotStreamEvent::TextDelta {
            text: "copilot output".into(),
        };
        let events = map_stream_event(&event);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::AssistantDelta { .. }))
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 7: Missing features documented (not silently dropped)
// ═══════════════════════════════════════════════════════════════════════

mod missing_features {
    use super::*;
    use abp_core::Capability;

    /// Helper: verify that a capability manifest has an entry for a given capability.
    fn manifest_has(manifest: &abp_core::CapabilityManifest, cap: &Capability) -> bool {
        manifest.contains_key(cap)
    }

    #[test]
    fn openai_documents_mcp_unsupported() {
        let manifest = abp_openai_sdk::dialect::capability_manifest();
        assert!(manifest_has(&manifest, &Capability::McpClient));
        assert!(matches!(
            manifest.get(&Capability::McpClient),
            Some(abp_core::SupportLevel::Unsupported)
        ));
        assert!(matches!(
            manifest.get(&Capability::McpServer),
            Some(abp_core::SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn claude_documents_mcp_server_unsupported() {
        let manifest = abp_claude_sdk::dialect::capability_manifest();
        assert!(matches!(
            manifest.get(&Capability::McpClient),
            Some(abp_core::SupportLevel::Native)
        ));
        assert!(matches!(
            manifest.get(&Capability::McpServer),
            Some(abp_core::SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn gemini_documents_glob_grep_unsupported() {
        let manifest = abp_gemini_sdk::dialect::capability_manifest();
        assert!(matches!(
            manifest.get(&Capability::ToolGlob),
            Some(abp_core::SupportLevel::Unsupported)
        ));
        assert!(matches!(
            manifest.get(&Capability::ToolGrep),
            Some(abp_core::SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn kimi_documents_edit_bash_unsupported() {
        let manifest = abp_kimi_sdk::dialect::capability_manifest();
        assert!(matches!(
            manifest.get(&Capability::ToolEdit),
            Some(abp_core::SupportLevel::Unsupported)
        ));
        assert!(matches!(
            manifest.get(&Capability::ToolBash),
            Some(abp_core::SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn copilot_documents_glob_grep_unsupported() {
        let manifest = abp_copilot_sdk::dialect::capability_manifest();
        assert!(matches!(
            manifest.get(&Capability::ToolGlob),
            Some(abp_core::SupportLevel::Unsupported)
        ));
        assert!(matches!(
            manifest.get(&Capability::ToolGrep),
            Some(abp_core::SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn all_dialects_have_streaming_capability() {
        let manifests: Vec<(&str, abp_core::CapabilityManifest)> = vec![
            ("OpenAI", abp_openai_sdk::dialect::capability_manifest()),
            ("Claude", abp_claude_sdk::dialect::capability_manifest()),
            ("Gemini", abp_gemini_sdk::dialect::capability_manifest()),
            ("Codex", abp_codex_sdk::dialect::capability_manifest()),
            ("Kimi", abp_kimi_sdk::dialect::capability_manifest()),
            ("Copilot", abp_copilot_sdk::dialect::capability_manifest()),
        ];
        for (name, manifest) in &manifests {
            assert!(
                manifest_has(manifest, &Capability::Streaming),
                "{name} should declare Streaming capability"
            );
            assert!(
                matches!(
                    manifest.get(&Capability::Streaming),
                    Some(abp_core::SupportLevel::Native)
                ),
                "{name} should have native streaming"
            );
        }
    }

    #[test]
    fn openai_image_content_not_in_message_type() {
        // Document: OpenAIMessage has flat string content, no image support
        use abp_openai_sdk::dialect::OpenAIMessage;
        let msg = OpenAIMessage {
            role: "user".into(),
            content: Some("text only".into()),
            tool_calls: None,
            tool_call_id: None,
        };
        // There is no image field — this documents the limitation
        let serialized = serde_json::to_value(&msg).unwrap();
        assert!(serialized.get("image").is_none());
    }

    #[test]
    fn gemini_tool_calls_lack_per_call_ids() {
        // Document: Gemini FunctionCall has no unique ID field
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
        use abp_gemini_sdk::lowering::to_ir;

        let contents = vec![GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({}),
            }],
        }];
        let conv = to_ir(&contents, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, .. } => {
                // ID is synthesized, not from the API
                assert!(id.starts_with("gemini_"), "ID should be synthesized: {id}");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn claude_thinking_signature_lost_in_roundtrip() {
        // Document: Thinking block signature is lost in IR roundtrip
        use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
        use abp_claude_sdk::lowering::{from_ir, to_ir};

        let blocks = vec![ClaudeContentBlock::Thinking {
            thinking: "reasoning...".into(),
            signature: Some("sig_abc".into()),
        }];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = to_ir(&msgs, None);
        let back = from_ir(&conv);
        let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
        match &parsed[0] {
            ClaudeContentBlock::Thinking { signature, .. } => {
                assert!(
                    signature.is_none(),
                    "signature should be lost in IR roundtrip"
                );
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn copilot_references_preserved_in_ir_metadata() {
        // Document: Copilot references survive via metadata, not via IR content blocks
        use abp_copilot_sdk::dialect::{CopilotMessage, CopilotReference, CopilotReferenceType};
        use abp_copilot_sdk::lowering::{from_ir, to_ir};

        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "check this file".into(),
            name: None,
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "file_1".into(),
                data: json!(null),
                metadata: None,
            }],
        }];
        let conv = to_ir(&msgs);
        // References are in metadata, not content blocks
        let meta = &conv.messages[0].metadata;
        assert!(
            meta.contains_key("copilot_references"),
            "references should be in metadata"
        );

        let back = from_ir(&conv);
        assert_eq!(back[0].copilot_references.len(), 1);
        assert_eq!(back[0].copilot_references[0].id, "file_1");
    }

    #[test]
    fn gemini_safety_settings_not_in_ir() {
        // Document: Gemini safety settings/ratings have no IR representation
        use abp_gemini_sdk::dialect::{
            GeminiCandidate, GeminiContent, GeminiPart, GeminiResponse, GeminiSafetyRating,
            HarmCategory, HarmProbability, map_response,
        };

        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("ok".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: Some(vec![GeminiSafetyRating {
                    category: HarmCategory::HarmCategoryHarassment,
                    probability: HarmProbability::Negligible,
                }]),
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        let events = map_response(&resp);
        // Safety ratings are not in the AgentEvent
        assert_eq!(events.len(), 1);
        assert!(events[0].ext.is_none(), "safety data should not be in ext");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 8: IR roundtrip fidelity for tool calls across all dialects
// ═══════════════════════════════════════════════════════════════════════

mod ir_tool_call_roundtrip {
    use super::*;

    fn make_ir_with_tool_call() -> IrConversation {
        IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "read the file"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "tu_1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "src/main.rs"}),
                }],
            ),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "tu_1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "fn main() {}".into(),
                    }],
                    is_error: false,
                }],
            ),
        ])
    }

    #[test]
    fn openai_tool_call_ir_roundtrip() {
        use abp_openai_sdk::lowering::{from_ir, to_ir};

        let conv = make_ir_with_tool_call();
        let native = from_ir(&conv);
        let back = to_ir(&native);

        assert_eq!(back.len(), 3);
        assert!(matches!(
            &back.messages[1].content[0],
            IrContentBlock::ToolUse { name, .. } if name == "read_file"
        ));
        assert!(matches!(
            &back.messages[2].content[0],
            IrContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "tu_1"
        ));
    }

    #[test]
    fn claude_tool_call_ir_roundtrip() {
        use abp_claude_sdk::lowering::{extract_system_prompt, from_ir, to_ir};

        let conv = make_ir_with_tool_call();
        let system = extract_system_prompt(&conv);
        let native = from_ir(&conv);
        let back = to_ir(&native, system.as_deref());

        // Claude skips system, so 2 messages (user, assistant).
        // Tool result is a user message with structured content.
        assert!(back.len() >= 2);
        let assistant_msg = back
            .messages
            .iter()
            .find(|m| m.role == IrRole::Assistant)
            .expect("should have assistant");
        assert!(
            assistant_msg
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolUse { .. }))
        );
    }

    #[test]
    fn gemini_tool_call_ir_roundtrip() {
        use abp_gemini_sdk::lowering::{from_ir, to_ir};

        let conv = make_ir_with_tool_call();
        let native = from_ir(&conv);
        let back = to_ir(&native, None);

        assert!(back.len() >= 2);
        let has_tool_use = back.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolUse { .. }))
        });
        assert!(has_tool_use, "should have ToolUse in roundtripped IR");
    }

    #[test]
    fn kimi_tool_call_ir_roundtrip() {
        use abp_kimi_sdk::lowering::{from_ir, to_ir};

        let conv = make_ir_with_tool_call();
        let native = from_ir(&conv);
        let back = to_ir(&native);

        assert_eq!(back.len(), 3);
        assert!(matches!(
            &back.messages[1].content[0],
            IrContentBlock::ToolUse { name, .. } if name == "read_file"
        ));
    }

    #[test]
    fn copilot_preserves_user_text_through_ir() {
        use abp_copilot_sdk::dialect::CopilotMessage;
        use abp_copilot_sdk::lowering::{from_ir, to_ir};

        let msgs = vec![
            CopilotMessage {
                role: "user".into(),
                content: "hello".into(),
                name: None,
                copilot_references: vec![],
            },
            CopilotMessage {
                role: "assistant".into(),
                content: "hi there".into(),
                name: None,
                copilot_references: vec![],
            },
        ];
        let conv = to_ir(&msgs);
        let back = from_ir(&conv);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].content, "hello");
        assert_eq!(back[1].content, "hi there");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 9: Capability manifest completeness
// ═══════════════════════════════════════════════════════════════════════

mod capability_manifests {
    use super::*;
    use abp_core::Capability;

    #[test]
    fn all_sdk_manifests_are_non_empty() {
        let manifests: Vec<(&str, abp_core::CapabilityManifest)> = vec![
            ("OpenAI", abp_openai_sdk::dialect::capability_manifest()),
            ("Claude", abp_claude_sdk::dialect::capability_manifest()),
            ("Gemini", abp_gemini_sdk::dialect::capability_manifest()),
            ("Codex", abp_codex_sdk::dialect::capability_manifest()),
            ("Kimi", abp_kimi_sdk::dialect::capability_manifest()),
            ("Copilot", abp_copilot_sdk::dialect::capability_manifest()),
        ];
        for (name, manifest) in &manifests {
            assert!(
                !manifest.is_empty(),
                "{name} capability manifest should not be empty"
            );
            assert!(
                manifest.len() >= 3,
                "{name} should declare at least 3 capabilities, got {}",
                manifest.len()
            );
        }
    }

    #[test]
    fn all_sdk_manifests_declare_streaming() {
        let manifests: Vec<(&str, abp_core::CapabilityManifest)> = vec![
            ("OpenAI", abp_openai_sdk::dialect::capability_manifest()),
            ("Claude", abp_claude_sdk::dialect::capability_manifest()),
            ("Gemini", abp_gemini_sdk::dialect::capability_manifest()),
            ("Codex", abp_codex_sdk::dialect::capability_manifest()),
            ("Kimi", abp_kimi_sdk::dialect::capability_manifest()),
            ("Copilot", abp_copilot_sdk::dialect::capability_manifest()),
        ];
        for (name, manifest) in &manifests {
            assert!(
                manifest.contains_key(&Capability::Streaming),
                "{name} should declare Streaming"
            );
        }
    }

    #[test]
    fn all_sdk_manifests_declare_mcp_status() {
        let manifests: Vec<(&str, abp_core::CapabilityManifest)> = vec![
            ("OpenAI", abp_openai_sdk::dialect::capability_manifest()),
            ("Claude", abp_claude_sdk::dialect::capability_manifest()),
            ("Gemini", abp_gemini_sdk::dialect::capability_manifest()),
            ("Codex", abp_codex_sdk::dialect::capability_manifest()),
            ("Kimi", abp_kimi_sdk::dialect::capability_manifest()),
            ("Copilot", abp_copilot_sdk::dialect::capability_manifest()),
        ];
        for (name, manifest) in &manifests {
            assert!(
                manifest.contains_key(&Capability::McpClient),
                "{name} should declare McpClient status"
            );
            assert!(
                manifest.contains_key(&Capability::McpServer),
                "{name} should declare McpServer status"
            );
        }
    }

    #[test]
    fn dialect_enum_covers_all_sdks() {
        let all = Dialect::all();
        assert!(all.contains(&Dialect::OpenAi));
        assert!(all.contains(&Dialect::Claude));
        assert!(all.contains(&Dialect::Gemini));
        assert!(all.contains(&Dialect::Codex));
        assert!(all.contains(&Dialect::Kimi));
        assert!(all.contains(&Dialect::Copilot));
        assert_eq!(all.len(), 6, "should have exactly 6 dialects");
    }

    #[test]
    fn dialect_labels_are_non_empty() {
        for &dialect in Dialect::all() {
            let label = dialect.label();
            assert!(
                !label.is_empty(),
                "{dialect:?} should have a non-empty label"
            );
        }
    }

    #[test]
    fn dialect_display_matches_label() {
        for &dialect in Dialect::all() {
            assert_eq!(
                format!("{dialect}"),
                dialect.label(),
                "{dialect:?} Display should match label()"
            );
        }
    }
}
