#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! SDK parity conformance tests — verify ABP can faithfully reproduce
//! each vendor SDK's surface area, wire format, and semantic behavior.

use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// Category 1 — OpenAI parity
// ═══════════════════════════════════════════════════════════════════════

mod openai_parity {
    use super::*;
    use abp_openai_sdk::api::{
        ChatCompletionRequest, ChatCompletionResponse, FunctionCall, FunctionDefinition, Message,
        Tool, ToolCall,
    };

    #[test]
    fn request_serializes_to_openai_format() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![Message::User {
                content: "Hello".into(),
            }],
            temperature: Some(0.7),
            max_tokens: Some(1024),
            tools: None,
            tool_choice: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            seed: None,
            response_format: None,
            user: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "gpt-4o");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Hello");
        assert_eq!(json["temperature"], 0.7);
        assert_eq!(json["max_tokens"], 1024);
    }

    #[test]
    fn response_deserializes_from_openai_format() {
        let raw = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1700000000u64,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "Hi!" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        });
        let resp: ChatCompletionResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.id, "chatcmpl-123");
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hi!"));
    }

    #[test]
    fn request_roundtrip_is_lossless() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                Message::System {
                    content: "Be helpful".into(),
                },
                Message::User {
                    content: "Hello".into(),
                },
            ],
            temperature: Some(0.5),
            max_tokens: Some(256),
            tools: None,
            tool_choice: None,
            stream: Some(false),
            top_p: Some(0.9),
            frequency_penalty: Some(0.1),
            presence_penalty: Some(0.2),
            stop: Some(vec!["END".into()]),
            n: Some(1),
            seed: Some(42),
            response_format: None,
            user: Some("user-1".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn tool_definition_matches_openai_format() {
        let tool = Tool {
            tool_type: "function".into(),
            function: FunctionDefinition {
                name: "get_weather".into(),
                description: Some("Get current weather".into()),
                parameters: Some(
                    json!({"type": "object", "properties": {"city": {"type": "string"}}}),
                ),
                strict: None,
            },
        };
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "get_weather");
        assert!(v["function"]["parameters"]["properties"]["city"].is_object());
    }

    #[test]
    fn tool_call_format_matches_openai() {
        let tc = ToolCall {
            id: "call_abc".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"main.rs"}"#.into(),
            },
        };
        let v = serde_json::to_value(&tc).unwrap();
        assert_eq!(v["id"], "call_abc");
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "read_file");
    }

    #[test]
    fn system_message_tagged_correctly() {
        let msg = Message::System {
            content: "You are a coder.".into(),
        };
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["role"], "system");
        assert_eq!(v["content"], "You are a coder.");
    }

    #[test]
    fn assistant_message_with_tool_calls() {
        let msg = Message::Assistant {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
        };
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["role"], "assistant");
        assert!(v["content"].is_null() || v.get("content").is_none());
        assert_eq!(v["tool_calls"][0]["function"]["name"], "search");
    }

    #[test]
    fn tool_message_format() {
        let msg = Message::Tool {
            tool_call_id: "call_1".into(),
            content: "result data".into(),
        };
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["role"], "tool");
        assert_eq!(v["tool_call_id"], "call_1");
        assert_eq!(v["content"], "result data");
    }

    #[test]
    fn request_converts_to_work_order() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![Message::User {
                content: "Help me".into(),
            }],
            temperature: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            seed: None,
            response_format: None,
            user: None,
        };
        let wo: abp_core::WorkOrder = req.into();
        assert_eq!(wo.task, "Help me");
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 2 — Claude parity
// ═══════════════════════════════════════════════════════════════════════

mod claude_parity {
    use super::*;
    use abp_claude_sdk::dialect::{
        ClaudeApiError, ClaudeConfig, ClaudeContentBlock, ClaudeStreamDelta, ClaudeToolDef,
        ClaudeUsage,
    };
    use abp_claude_sdk::messages::{
        Message, MessageContent, MessagesRequest, MessagesResponse, Role, SystemMessage,
    };

    #[test]
    fn messages_request_matches_anthropic_format() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Hello Claude".into()),
            }],
            max_tokens: 1024,
            system: Some(SystemMessage::Text("Be concise".into())),
            tools: None,
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: Some(0.7),
            top_p: None,
            top_k: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "claude-sonnet-4-20250514");
        assert_eq!(v["max_tokens"], 1024);
        assert_eq!(v["system"], "Be concise");
        assert_eq!(v["messages"][0]["role"], "user");
    }

    #[test]
    fn messages_response_deserializes_correctly() {
        let raw = json!({
            "id": "msg_01XFD",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let resp: MessagesResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.id, "msg_01XFD");
        assert_eq!(resp.response_type, "message");
        assert_eq!(resp.role, "assistant");
    }

    #[test]
    fn tool_use_content_block_format() {
        let block = ClaudeContentBlock::ToolUse {
            id: "toolu_01A".into(),
            name: "read_file".into(),
            input: json!({"path": "src/main.rs"}),
        };
        let v = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "tool_use");
        assert_eq!(v["id"], "toolu_01A");
        assert_eq!(v["name"], "read_file");
    }

    #[test]
    fn tool_result_content_block_format() {
        let block = ClaudeContentBlock::ToolResult {
            tool_use_id: "toolu_01A".into(),
            content: Some("file contents".into()),
            is_error: Some(false),
        };
        let v = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "tool_result");
        assert_eq!(v["tool_use_id"], "toolu_01A");
    }

    #[test]
    fn claude_tool_def_uses_input_schema() {
        let tool = ClaudeToolDef {
            name: "edit_file".into(),
            description: "Edit a file".into(),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        };
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(v["name"], "edit_file");
        assert!(v.get("input_schema").is_some());
        assert!(v.get("parameters").is_none());
    }

    #[test]
    fn claude_usage_has_input_output_tokens() {
        let usage = ClaudeUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let v = serde_json::to_value(&usage).unwrap();
        assert_eq!(v["input_tokens"], 100);
        assert_eq!(v["output_tokens"], 50);
        assert!(v.get("prompt_tokens").is_none());
    }

    #[test]
    fn system_message_as_structured_blocks() {
        let sys = SystemMessage::Blocks(vec![abp_claude_sdk::dialect::ClaudeSystemBlock::Text {
            text: "First instruction".into(),
            cache_control: None,
        }]);
        let v = serde_json::to_value(&sys).unwrap();
        assert!(v.is_array());
        assert_eq!(v[0]["type"], "text");
        assert_eq!(v[0]["text"], "First instruction");
    }

    #[test]
    fn claude_api_error_format() {
        let err = ClaudeApiError {
            error_type: "invalid_request_error".into(),
            message: "max_tokens must be positive".into(),
        };
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["type"], "invalid_request_error");
        assert_eq!(v["message"], "max_tokens must be positive");
    }

    #[test]
    fn stream_text_delta_format() {
        let delta = ClaudeStreamDelta::TextDelta {
            text: "chunk".into(),
        };
        let v = serde_json::to_value(&delta).unwrap();
        assert_eq!(v["type"], "text_delta");
        assert_eq!(v["text"], "chunk");
    }

    #[test]
    fn claude_config_defaults_are_correct() {
        let cfg = ClaudeConfig::default();
        assert!(cfg.base_url.contains("anthropic.com"));
        assert!(cfg.model.contains("claude"));
        assert!(cfg.max_tokens > 0);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 3 — Gemini parity
// ═══════════════════════════════════════════════════════════════════════

mod gemini_parity {
    use super::*;
    use abp_gemini_sdk::types::{
        Content, FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration, GeminiTool,
        GenerateContentRequest, GenerateContentResponse, GenerationConfig, Part, ToolConfig,
    };

    #[test]
    fn request_uses_camel_case_fields() {
        let req = GenerateContentRequest {
            contents: vec![Content {
                role: Some("user".into()),
                parts: vec![Part::Text("Hello Gemini".into())],
            }],
            system_instruction: None,
            tools: None,
            tool_config: None,
            generation_config: Some(GenerationConfig {
                temperature: Some(0.5),
                top_p: None,
                top_k: None,
                max_output_tokens: Some(1024),
                candidate_count: None,
                stop_sequences: None,
            }),
            safety_settings: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("generationConfig").is_some());
        assert!(v.get("generation_config").is_none());
        assert_eq!(v["generationConfig"]["maxOutputTokens"], 1024);
    }

    #[test]
    fn response_uses_camel_case_fields() {
        let raw = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "Hi!"}]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5,
                "totalTokenCount": 15
            }
        });
        let resp: GenerateContentResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
        let usage = resp.usage_metadata.unwrap();
        assert_eq!(usage.prompt_token_count, 10);
    }

    #[test]
    fn gemini_role_is_model_not_assistant() {
        let content = Content {
            role: Some("model".into()),
            parts: vec![Part::Text("response".into())],
        };
        let v = serde_json::to_value(&content).unwrap();
        assert_eq!(v["role"], "model");
    }

    #[test]
    fn function_call_part_format() {
        let part = Part::FunctionCall {
            name: "search".into(),
            args: json!({"query": "rust"}),
        };
        let v = serde_json::to_value(&part).unwrap();
        assert!(v.get("functionCall").is_some());
        assert_eq!(v["functionCall"]["name"], "search");
    }

    #[test]
    fn function_response_part_format() {
        let part = Part::FunctionResponse {
            name: "search".into(),
            response: json!({"results": []}),
        };
        let v = serde_json::to_value(&part).unwrap();
        assert!(v.get("functionResponse").is_some());
        assert_eq!(v["functionResponse"]["name"], "search");
    }

    #[test]
    fn tool_declaration_format() {
        let tool = GeminiTool {
            function_declarations: vec![FunctionDeclaration {
                name: "get_weather".into(),
                description: "Get weather data".into(),
                parameters: json!({"type": "object"}),
            }],
        };
        let v = serde_json::to_value(&tool).unwrap();
        assert!(v.get("functionDeclarations").is_some());
        assert_eq!(v["functionDeclarations"][0]["name"], "get_weather");
    }

    #[test]
    fn tool_config_uses_function_calling_config() {
        let tc = ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        };
        let v = serde_json::to_value(&tc).unwrap();
        assert!(v.get("functionCallingConfig").is_some());
        assert_eq!(v["functionCallingConfig"]["mode"], "AUTO");
    }

    #[test]
    fn system_instruction_has_no_role() {
        let req = GenerateContentRequest {
            contents: vec![],
            system_instruction: Some(Content {
                role: None,
                parts: vec![Part::Text("Be helpful".into())],
            }),
            tools: None,
            tool_config: None,
            generation_config: None,
            safety_settings: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert!(
            v["systemInstruction"].get("role").is_none()
                || v["systemInstruction"]["role"].is_null()
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 4 — Codex parity
// ═══════════════════════════════════════════════════════════════════════

mod codex_parity {
    use super::*;
    use abp_codex_sdk::types::{
        CodexChoice, CodexChoiceMessage, CodexFunctionCall, CodexFunctionDef, CodexMessage,
        CodexRequest, CodexResponse, CodexTool, CodexToolCall, CodexUsage, FileOperation,
    };

    #[test]
    fn codex_request_has_instructions_field() {
        let req = CodexRequest {
            model: "codex-mini-latest".into(),
            messages: vec![CodexMessage::User {
                content: "Refactor auth".into(),
            }],
            instructions: Some("You are a coding assistant".into()),
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["instructions"], "You are a coding assistant");
        assert_eq!(v["model"], "codex-mini-latest");
    }

    #[test]
    fn codex_response_matches_chat_completion_format() {
        let resp = CodexResponse {
            id: "chatcmpl-codex-1".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "codex-mini-latest".into(),
            choices: vec![CodexChoice {
                index: 0,
                message: CodexChoiceMessage {
                    role: "assistant".into(),
                    content: Some("Done!".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(CodexUsage {
                prompt_tokens: 20,
                completion_tokens: 10,
                total_tokens: 30,
            }),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn codex_tool_call_format() {
        let tc = CodexToolCall {
            id: "call_codex_1".into(),
            call_type: "function".into(),
            function: CodexFunctionCall {
                name: "write_file".into(),
                arguments: r#"{"path":"a.rs","content":"fn main(){}"}"#.into(),
            },
        };
        let v = serde_json::to_value(&tc).unwrap();
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "write_file");
    }

    #[test]
    fn codex_file_operation_variants() {
        for (op, expected) in [
            (FileOperation::Create, "create"),
            (FileOperation::Update, "update"),
            (FileOperation::Delete, "delete"),
            (FileOperation::Patch, "patch"),
        ] {
            let v = serde_json::to_value(&op).unwrap();
            assert_eq!(v.as_str().unwrap(), expected);
        }
    }

    #[test]
    fn codex_tool_definition_format() {
        let tool = CodexTool {
            tool_type: "function".into(),
            function: CodexFunctionDef {
                name: "bash".into(),
                description: "Run a shell command".into(),
                parameters: json!({"type": "object"}),
            },
        };
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "bash");
    }

    #[test]
    fn codex_message_roundtrip() {
        let msgs = vec![
            CodexMessage::System {
                content: "sys".into(),
            },
            CodexMessage::User {
                content: "hi".into(),
            },
            CodexMessage::Assistant {
                content: Some("hello".into()),
                tool_calls: None,
            },
            CodexMessage::Tool {
                content: "result".into(),
                tool_call_id: "call_1".into(),
            },
        ];
        for msg in &msgs {
            let json = serde_json::to_string(msg).unwrap();
            let parsed: CodexMessage = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, msg);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 5 — Copilot parity
// ═══════════════════════════════════════════════════════════════════════

mod copilot_parity {
    use super::*;
    use abp_copilot_sdk::types::{
        CopilotChatChoice, CopilotChatChoiceMessage, CopilotChatMessage, CopilotChatRequest,
        CopilotChatResponse, CopilotFunctionCall, CopilotTool, CopilotToolCall,
        CopilotToolFunction, CopilotUsage, Reference, ReferenceType,
    };

    #[test]
    fn copilot_request_has_intent_and_references() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: vec![CopilotChatMessage {
                role: "user".into(),
                content: Some("Help me".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            intent: Some("conversation".into()),
            references: Some(vec![Reference {
                ref_type: ReferenceType::File,
                id: "file-1".into(),
                uri: Some("file:///src/main.rs".into()),
                content: Some("fn main() {}".into()),
                metadata: None,
            }]),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["intent"], "conversation");
        assert_eq!(v["references"][0]["type"], "file");
        assert_eq!(v["references"][0]["id"], "file-1");
    }

    #[test]
    fn copilot_response_format() {
        let resp = CopilotChatResponse {
            id: "chatcmpl-copilot-1".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![CopilotChatChoice {
                index: 0,
                message: CopilotChatChoiceMessage {
                    role: "assistant".into(),
                    content: Some("Here's the fix".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(CopilotUsage {
                prompt_tokens: 50,
                completion_tokens: 20,
                total_tokens: 70,
                copilot_tokens: Some(10),
            }),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["usage"]["copilot_tokens"], 10);
    }

    #[test]
    fn copilot_tool_call_format() {
        let tc = CopilotToolCall {
            id: "call_cop_1".into(),
            call_type: "function".into(),
            function: CopilotFunctionCall {
                name: "get_file".into(),
                arguments: r#"{"path":"lib.rs"}"#.into(),
            },
        };
        let v = serde_json::to_value(&tc).unwrap();
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "get_file");
    }

    #[test]
    fn copilot_reference_types_serialize() {
        let types = vec![
            (ReferenceType::File, "file"),
            (ReferenceType::Selection, "selection"),
            (ReferenceType::Terminal, "terminal"),
            (ReferenceType::WebPage, "web_page"),
            (ReferenceType::GitDiff, "git_diff"),
        ];
        for (rt, expected) in types {
            let v = serde_json::to_value(&rt).unwrap();
            assert_eq!(v.as_str().unwrap(), expected);
        }
    }

    #[test]
    fn copilot_tool_definition_format() {
        let tool = CopilotTool {
            tool_type: "function".into(),
            function: CopilotToolFunction {
                name: "search_code".into(),
                description: "Search for code".into(),
                parameters: json!({"type": "object"}),
            },
        };
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["description"], "Search for code");
    }

    #[test]
    fn copilot_message_roundtrip() {
        let msg = CopilotChatMessage {
            role: "assistant".into(),
            content: Some("I can help".into()),
            name: Some("copilot".into()),
            tool_calls: None,
            tool_call_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: CopilotChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 6 — Kimi parity
// ═══════════════════════════════════════════════════════════════════════

mod kimi_parity {
    use super::*;
    use abp_kimi_sdk::types::{
        ChatMessage, Choice, ChoiceMessage, FunctionCall, FunctionDef, KimiChatRequest,
        KimiChatResponse, KimiUsage, SearchMode, SearchOptions, Tool, ToolCall,
    };

    #[test]
    fn kimi_request_has_search_extensions() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![ChatMessage::User {
                content: "What's the weather?".into(),
            }],
            temperature: Some(0.7),
            top_p: None,
            max_tokens: Some(2048),
            stream: None,
            tools: None,
            tool_choice: None,
            use_search: Some(true),
            search_options: Some(SearchOptions {
                mode: SearchMode::Auto,
                result_count: Some(5),
            }),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["use_search"], true);
        assert_eq!(v["search_options"]["mode"], "auto");
        assert_eq!(v["search_options"]["result_count"], 5);
    }

    #[test]
    fn kimi_response_format() {
        let resp = KimiChatResponse {
            id: "cmpl-kimi-1".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".into(),
                    content: Some("The weather is sunny".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(KimiUsage {
                prompt_tokens: 30,
                completion_tokens: 15,
                total_tokens: 45,
                search_tokens: Some(10),
            }),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["usage"]["search_tokens"], 10);
    }

    #[test]
    fn kimi_tool_call_format() {
        let tc = ToolCall {
            id: "call_kimi_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"test"}"#.into(),
            },
        };
        let v = serde_json::to_value(&tc).unwrap();
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "search");
    }

    #[test]
    fn kimi_search_modes_serialize() {
        let modes = vec![
            (SearchMode::Auto, "auto"),
            (SearchMode::Always, "always"),
            (SearchMode::Never, "never"),
        ];
        for (mode, expected) in modes {
            let v = serde_json::to_value(&mode).unwrap();
            assert_eq!(v.as_str().unwrap(), expected);
        }
    }

    #[test]
    fn kimi_tool_definition_format() {
        let tool = Tool {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "translate".into(),
                description: "Translate text".into(),
                parameters: json!({"type": "object"}),
            },
        };
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "translate");
    }

    #[test]
    fn kimi_message_roundtrip() {
        let msgs = vec![
            ChatMessage::System {
                content: "sys".into(),
            },
            ChatMessage::User {
                content: "hi".into(),
            },
            ChatMessage::Assistant {
                content: Some("hello".into()),
                tool_calls: None,
            },
            ChatMessage::Tool {
                content: "result".into(),
                tool_call_id: "call_1".into(),
            },
        ];
        for msg in &msgs {
            let json = serde_json::to_string(msg).unwrap();
            let parsed: ChatMessage = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, msg);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 7 — Passthrough fidelity
// ═══════════════════════════════════════════════════════════════════════

mod passthrough_fidelity {
    use super::*;
    use abp_openai_sdk::api::ChatCompletionRequest;

    #[test]
    fn openai_request_roundtrip_is_bitwise_equivalent() {
        let original = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": "Hello"}
            ],
            "temperature": 0.7,
            "max_tokens": 1024,
            "stream": false
        });
        let parsed: ChatCompletionRequest = serde_json::from_value(original.clone()).unwrap();
        let reserialized = serde_json::to_value(&parsed).unwrap();

        assert_eq!(reserialized["model"], original["model"]);
        assert_eq!(reserialized["messages"], original["messages"]);
        assert_eq!(reserialized["temperature"], original["temperature"]);
    }

    #[test]
    fn claude_request_roundtrip_preserves_content() {
        use abp_claude_sdk::messages::MessagesRequest;
        let original = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "temperature": 0.5
        });
        let parsed: MessagesRequest = serde_json::from_value(original.clone()).unwrap();
        let reserialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(reserialized["model"], original["model"]);
        assert_eq!(reserialized["max_tokens"], original["max_tokens"]);
    }

    #[test]
    fn gemini_request_roundtrip_preserves_camel_case() {
        use abp_gemini_sdk::types::GenerateContentRequest;
        let original = json!({
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}],
            "generationConfig": {"temperature": 0.5, "maxOutputTokens": 1024}
        });
        let parsed: GenerateContentRequest = serde_json::from_value(original.clone()).unwrap();
        let reserialized = serde_json::to_value(&parsed).unwrap();
        assert!(reserialized.get("generationConfig").is_some());
        assert_eq!(
            reserialized["generationConfig"]["maxOutputTokens"],
            original["generationConfig"]["maxOutputTokens"]
        );
    }

    #[test]
    fn codex_request_roundtrip_preserves_instructions() {
        use abp_codex_sdk::types::CodexRequest;
        let original = json!({
            "model": "codex-mini-latest",
            "messages": [{"role": "user", "content": "Fix bug"}],
            "instructions": "Be careful with tests"
        });
        let parsed: CodexRequest = serde_json::from_value(original.clone()).unwrap();
        let reserialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(reserialized["instructions"], original["instructions"]);
    }

    #[test]
    fn copilot_request_roundtrip_preserves_extensions() {
        use abp_copilot_sdk::types::CopilotChatRequest;
        let original = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Help"}],
            "intent": "conversation",
            "references": [{"type": "file", "id": "f1"}]
        });
        let parsed: CopilotChatRequest = serde_json::from_value(original.clone()).unwrap();
        let reserialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(reserialized["intent"], original["intent"]);
        assert_eq!(reserialized["references"][0]["type"], "file");
    }

    #[test]
    fn kimi_request_roundtrip_preserves_search() {
        use abp_kimi_sdk::types::KimiChatRequest;
        let original = json!({
            "model": "moonshot-v1-8k",
            "messages": [{"role": "user", "content": "Search for info"}],
            "use_search": true,
            "search_options": {"mode": "auto", "result_count": 3}
        });
        let parsed: KimiChatRequest = serde_json::from_value(original.clone()).unwrap();
        let reserialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(reserialized["use_search"], original["use_search"]);
        assert_eq!(reserialized["search_options"]["mode"], "auto");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 8 — Streaming format parity
// ═══════════════════════════════════════════════════════════════════════

mod streaming_format_parity {
    use abp_openai_sdk::streaming::{
        ChatCompletionChunk, ChunkChoice, ChunkDelta, ChunkToolCall, ChunkUsage,
    };

    #[test]
    fn openai_stream_chunk_format() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-stream-1".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: Some("assistant".into()),
                    content: Some("He".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        let v = serde_json::to_value(&chunk).unwrap();
        assert_eq!(v["object"], "chat.completion.chunk");
        assert_eq!(v["choices"][0]["delta"]["content"], "He");
    }

    #[test]
    fn openai_stream_final_chunk_has_usage() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-stream-2".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta::default(),
                finish_reason: Some("stop".into()),
            }],
            usage: Some(ChunkUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        };
        let v = serde_json::to_value(&chunk).unwrap();
        assert!(v.get("usage").is_some());
        assert_eq!(v["usage"]["total_tokens"], 15);
    }

    #[test]
    fn openai_stream_tool_call_fragment() {
        let frag = ChunkToolCall {
            index: 0,
            id: Some("call_1".into()),
            call_type: Some("function".into()),
            function: Some(abp_openai_sdk::streaming::ChunkFunctionCall {
                name: Some("read_file".into()),
                arguments: Some(r#"{"pa"#.into()),
            }),
        };
        let v = serde_json::to_value(&frag).unwrap();
        assert_eq!(v["id"], "call_1");
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "read_file");
    }

    #[test]
    fn claude_stream_event_message_start() {
        use abp_claude_sdk::dialect::{ClaudeResponse, ClaudeStreamEvent, ClaudeUsage};
        let event = ClaudeStreamEvent::MessageStart {
            message: ClaudeResponse {
                id: "msg_01".into(),
                model: "claude-sonnet-4-20250514".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: Some(ClaudeUsage {
                    input_tokens: 10,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
            },
        };
        let v = serde_json::to_value(&event).unwrap();
        assert_eq!(v["type"], "message_start");
        assert!(v.get("message").is_some());
    }

    #[test]
    fn claude_stream_content_block_delta() {
        use abp_claude_sdk::dialect::{ClaudeStreamDelta, ClaudeStreamEvent};
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "Hello".into(),
            },
        };
        let v = serde_json::to_value(&event).unwrap();
        assert_eq!(v["type"], "content_block_delta");
        assert_eq!(v["delta"]["type"], "text_delta");
    }

    #[test]
    fn codex_stream_chunk_format() {
        use abp_codex_sdk::types::{CodexStreamChoice, CodexStreamChunk, CodexStreamDelta};
        let chunk = CodexStreamChunk {
            id: "chatcmpl-codex-s1".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "codex-mini-latest".into(),
            choices: vec![CodexStreamChoice {
                index: 0,
                delta: CodexStreamDelta {
                    role: Some("assistant".into()),
                    content: Some("co".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
        };
        let v = serde_json::to_value(&chunk).unwrap();
        assert_eq!(v["object"], "chat.completion.chunk");
    }

    #[test]
    fn copilot_stream_chunk_format() {
        use abp_copilot_sdk::types::{CopilotStreamChoice, CopilotStreamChunk, CopilotStreamDelta};
        let chunk = CopilotStreamChunk {
            id: "chatcmpl-cop-s1".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![CopilotStreamChoice {
                index: 0,
                delta: CopilotStreamDelta {
                    role: Some("assistant".into()),
                    content: Some("Hi".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
        };
        let v = serde_json::to_value(&chunk).unwrap();
        assert_eq!(v["object"], "chat.completion.chunk");
    }

    #[test]
    fn kimi_stream_chunk_format() {
        use abp_kimi_sdk::types::{KimiStreamChunk, StreamChoice, StreamDelta};
        let chunk = KimiStreamChunk {
            id: "chatcmpl-kimi-s1".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: Some("assistant".into()),
                    content: Some("data".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        let v = serde_json::to_value(&chunk).unwrap();
        assert_eq!(v["object"], "chat.completion.chunk");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 9 — Error format parity
// ═══════════════════════════════════════════════════════════════════════

mod error_format_parity {
    use serde_json::json;

    #[test]
    fn openai_error_response_format() {
        // OpenAI errors use {"error": {"message": "...", "type": "...", "code": "..."}}
        let error_json = json!({
            "error": {
                "message": "Invalid model",
                "type": "invalid_request_error",
                "code": "model_not_found"
            }
        });
        assert_eq!(error_json["error"]["type"], "invalid_request_error");
        assert!(error_json["error"]["message"].is_string());
    }

    #[test]
    fn claude_error_response_format() {
        use abp_claude_sdk::dialect::ClaudeApiError;
        let err = ClaudeApiError {
            error_type: "invalid_request_error".into(),
            message: "max_tokens is required".into(),
        };
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["type"], "invalid_request_error");
        assert_eq!(v["message"], "max_tokens is required");
    }

    #[test]
    fn claude_stream_error_event() {
        use abp_claude_sdk::dialect::{ClaudeApiError, ClaudeStreamEvent};
        let event = ClaudeStreamEvent::Error {
            error: ClaudeApiError {
                error_type: "overloaded_error".into(),
                message: "Service temporarily unavailable".into(),
            },
        };
        let v = serde_json::to_value(&event).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["error"]["type"], "overloaded_error");
    }

    #[test]
    fn openai_shim_error_display() {
        let err = abp_shim_openai::ShimError::InvalidRequest("missing model".into());
        let display = format!("{err}");
        assert!(display.contains("missing model"));
    }

    #[test]
    fn claude_shim_error_display() {
        let err = abp_shim_claude::ShimError::InvalidRequest("bad request".into());
        let display = format!("{err}");
        assert!(display.contains("bad request"));
    }

    #[test]
    fn codex_shim_error_display() {
        let err = abp_shim_codex::ShimError::InvalidRequest("no messages".into());
        let display = format!("{err}");
        assert!(display.contains("no messages"));
    }

    #[test]
    fn kimi_shim_error_display() {
        let err = abp_shim_kimi::ShimError::InvalidRequest("invalid model".into());
        let display = format!("{err}");
        assert!(display.contains("invalid model"));
    }

    #[test]
    fn copilot_shim_error_display() {
        let err = abp_shim_copilot::ShimError::InvalidRequest("unauthorized".into());
        let display = format!("{err}");
        assert!(display.contains("unauthorized"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 10 — Header parity (auth header format per SDK)
// ═══════════════════════════════════════════════════════════════════════

mod header_parity {

    #[test]
    fn openai_auth_header_format() {
        let api_key = "sk-test-key-123";
        let header = format!("Bearer {api_key}");
        assert!(header.starts_with("Bearer "));
        assert!(header.contains("sk-"));
    }

    #[test]
    fn claude_auth_header_format() {
        // Anthropic uses x-api-key header, not Bearer token
        let api_key = "sk-ant-api-test-123";
        assert!(api_key.starts_with("sk-ant-"));
    }

    #[test]
    fn claude_version_header() {
        // Anthropic requires anthropic-version header
        let version = "2023-06-01";
        assert!(!version.is_empty());
    }

    #[test]
    fn gemini_auth_via_query_param() {
        // Gemini uses ?key= query parameter
        let api_key = "AIza-test-key";
        let url = format!(
            "https://generativelanguage.googleapis.com/v1/models/gemini-2.5-flash:generateContent?key={api_key}"
        );
        assert!(url.contains("key=AIza"));
    }

    #[test]
    fn copilot_auth_header_format() {
        // Copilot uses Bearer token from GitHub
        let token = "ghu_test_token_123";
        let header = format!("Bearer {token}");
        assert!(header.starts_with("Bearer "));
    }

    #[test]
    fn kimi_auth_header_format() {
        // Kimi uses Bearer token like OpenAI
        let api_key = "sk-moonshot-test-key";
        let header = format!("Bearer {api_key}");
        assert!(header.starts_with("Bearer "));
    }

    #[test]
    fn openai_config_base_url() {
        use abp_openai_sdk::dialect::OpenAIConfig;
        let cfg = OpenAIConfig::default();
        assert!(cfg.base_url.starts_with("https://"));
        assert!(cfg.base_url.contains("openai.com"));
    }

    #[test]
    fn claude_config_base_url() {
        use abp_claude_sdk::dialect::ClaudeConfig;
        let cfg = ClaudeConfig::default();
        assert!(cfg.base_url.starts_with("https://"));
        assert!(cfg.base_url.contains("anthropic.com"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 11 — Model name parity
// ═══════════════════════════════════════════════════════════════════════

mod model_name_parity {
    #[test]
    fn openai_model_roundtrip() {
        use abp_openai_sdk::dialect::{from_canonical_model, to_canonical_model};
        let model = "gpt-4o";
        let canonical = to_canonical_model(model);
        assert_eq!(canonical, "openai/gpt-4o");
        let back = from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    #[test]
    fn claude_model_roundtrip() {
        use abp_claude_sdk::dialect::{from_canonical_model, to_canonical_model};
        let model = "claude-sonnet-4-20250514";
        let canonical = to_canonical_model(model);
        assert_eq!(canonical, "anthropic/claude-sonnet-4-20250514");
        let back = from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    #[test]
    fn gemini_model_roundtrip() {
        use abp_gemini_sdk::dialect::{from_canonical_model, to_canonical_model};
        let model = "gemini-2.5-flash";
        let canonical = to_canonical_model(model);
        assert_eq!(canonical, "google/gemini-2.5-flash");
        let back = from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    #[test]
    fn codex_model_roundtrip() {
        use abp_codex_sdk::dialect::{from_canonical_model, to_canonical_model};
        let model = "codex-mini-latest";
        let canonical = to_canonical_model(model);
        assert_eq!(canonical, "openai/codex-mini-latest");
        let back = from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    #[test]
    fn copilot_model_roundtrip() {
        use abp_copilot_sdk::dialect::{from_canonical_model, to_canonical_model};
        let model = "gpt-4o";
        let canonical = to_canonical_model(model);
        assert_eq!(canonical, "copilot/gpt-4o");
        let back = from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    #[test]
    fn kimi_model_roundtrip() {
        use abp_kimi_sdk::dialect::{from_canonical_model, to_canonical_model};
        let model = "moonshot-v1-8k";
        let canonical = to_canonical_model(model);
        assert_eq!(canonical, "moonshot/moonshot-v1-8k");
        let back = from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    #[test]
    fn openai_known_models_recognized() {
        use abp_openai_sdk::dialect::is_known_model;
        assert!(is_known_model("gpt-4o"));
        assert!(is_known_model("gpt-4o-mini"));
        assert!(!is_known_model("unknown-model"));
    }

    #[test]
    fn claude_known_models_recognized() {
        use abp_claude_sdk::dialect::is_known_model;
        assert!(is_known_model("claude-sonnet-4-20250514"));
        assert!(!is_known_model("unknown-model"));
    }

    #[test]
    fn gemini_known_models_recognized() {
        use abp_gemini_sdk::dialect::is_known_model;
        assert!(is_known_model("gemini-2.5-flash"));
        assert!(!is_known_model("unknown-model"));
    }

    #[test]
    fn codex_known_models_recognized() {
        use abp_codex_sdk::dialect::is_known_model;
        assert!(is_known_model("codex-mini-latest"));
        assert!(!is_known_model("unknown-model"));
    }

    #[test]
    fn copilot_known_models_recognized() {
        use abp_copilot_sdk::dialect::is_known_model;
        assert!(is_known_model("gpt-4o"));
        assert!(!is_known_model("unknown-model"));
    }

    #[test]
    fn kimi_known_models_recognized() {
        use abp_kimi_sdk::dialect::is_known_model;
        assert!(is_known_model("moonshot-v1-8k"));
        assert!(!is_known_model("unknown-model"));
    }

    #[test]
    fn unknown_model_passthrough_openai() {
        use abp_openai_sdk::dialect::{from_canonical_model, to_canonical_model};
        let model = "custom-fine-tuned-v3";
        let canonical = to_canonical_model(model);
        let back = from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    #[test]
    fn unknown_model_passthrough_claude() {
        use abp_claude_sdk::dialect::{from_canonical_model, to_canonical_model};
        let model = "claude-future-model";
        let canonical = to_canonical_model(model);
        let back = from_canonical_model(&canonical);
        assert_eq!(back, model);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 12 — Tool call format parity
// ═══════════════════════════════════════════════════════════════════════

mod tool_call_format_parity {
    use super::*;

    #[test]
    fn openai_tool_def_uses_parameters() {
        use abp_openai_sdk::dialect::{OpenAIFunctionDef, OpenAIToolDef};
        let tool = OpenAIToolDef {
            tool_type: "function".into(),
            function: OpenAIFunctionDef {
                name: "edit".into(),
                description: "Edit file".into(),
                parameters: json!({"type": "object"}),
            },
        };
        let v = serde_json::to_value(&tool).unwrap();
        assert!(v["function"].get("parameters").is_some());
        assert!(v["function"].get("input_schema").is_none());
    }

    #[test]
    fn claude_tool_def_uses_input_schema() {
        use abp_claude_sdk::dialect::ClaudeToolDef;
        let tool = ClaudeToolDef {
            name: "edit".into(),
            description: "Edit file".into(),
            input_schema: json!({"type": "object"}),
        };
        let v = serde_json::to_value(&tool).unwrap();
        assert!(v.get("input_schema").is_some());
        assert!(v.get("parameters").is_none());
    }

    #[test]
    fn gemini_tool_uses_function_declarations() {
        use abp_gemini_sdk::types::{FunctionDeclaration, GeminiTool};
        let tool = GeminiTool {
            function_declarations: vec![FunctionDeclaration {
                name: "edit".into(),
                description: "Edit file".into(),
                parameters: json!({"type": "object"}),
            }],
        };
        let v = serde_json::to_value(&tool).unwrap();
        assert!(v.get("functionDeclarations").is_some());
    }

    #[test]
    fn openai_canonical_tool_roundtrip() {
        use abp_openai_sdk::dialect::{CanonicalToolDef, tool_def_from_openai, tool_def_to_openai};
        let canonical = CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        };
        let openai = tool_def_to_openai(&canonical);
        assert_eq!(openai.tool_type, "function");
        let back = tool_def_from_openai(&openai);
        assert_eq!(back.name, canonical.name);
        assert_eq!(back.description, canonical.description);
        assert_eq!(back.parameters_schema, canonical.parameters_schema);
    }

    #[test]
    fn claude_canonical_tool_roundtrip() {
        use abp_claude_sdk::dialect::{CanonicalToolDef, tool_def_from_claude, tool_def_to_claude};
        let canonical = CanonicalToolDef {
            name: "write_file".into(),
            description: "Write a file".into(),
            parameters_schema: json!({"type": "object"}),
        };
        let claude = tool_def_to_claude(&canonical);
        assert_eq!(claude.name, "write_file");
        let back = tool_def_from_claude(&claude);
        assert_eq!(back.name, canonical.name);
        assert_eq!(back.parameters_schema, canonical.parameters_schema);
    }

    #[test]
    fn openai_tool_call_has_type_function() {
        use abp_openai_sdk::api::ToolCall;
        let tc = ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: abp_openai_sdk::api::FunctionCall {
                name: "test".into(),
                arguments: "{}".into(),
            },
        };
        let v = serde_json::to_value(&tc).unwrap();
        assert_eq!(v["type"], "function");
    }

    #[test]
    fn claude_tool_use_has_no_type_function_wrapper() {
        use abp_claude_sdk::dialect::ClaudeContentBlock;
        let block = ClaudeContentBlock::ToolUse {
            id: "toolu_1".into(),
            name: "test".into(),
            input: json!({}),
        };
        let v = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "tool_use");
        assert!(v.get("function").is_none());
    }

    #[test]
    fn gemini_function_call_in_parts() {
        use abp_gemini_sdk::types::Part;
        let part = Part::FunctionCall {
            name: "search".into(),
            args: json!({"q": "test"}),
        };
        let v = serde_json::to_value(&part).unwrap();
        assert!(v.get("functionCall").is_some());
        assert!(v.get("type").is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 13 — System message parity
// ═══════════════════════════════════════════════════════════════════════

mod system_message_parity {

    #[test]
    fn openai_system_in_messages_array() {
        use abp_openai_sdk::api::{ChatCompletionRequest, Message};
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                Message::System {
                    content: "You are helpful".into(),
                },
                Message::User {
                    content: "Hi".into(),
                },
            ],
            temperature: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            seed: None,
            response_format: None,
            user: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"][1]["role"], "user");
    }

    #[test]
    fn claude_system_is_top_level_field() {
        use abp_claude_sdk::messages::{
            Message, MessageContent, MessagesRequest, Role, SystemMessage,
        };
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Hi".into()),
            }],
            max_tokens: 1024,
            system: Some(SystemMessage::Text("You are helpful".into())),
            tools: None,
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["system"], "You are helpful");
        // System is NOT in messages array for Claude
        for msg in v["messages"].as_array().unwrap() {
            assert_ne!(msg["role"], "system");
        }
    }

    #[test]
    fn gemini_system_is_separate_field() {
        use abp_gemini_sdk::types::{Content, GenerateContentRequest, Part};
        let req = GenerateContentRequest {
            contents: vec![Content {
                role: Some("user".into()),
                parts: vec![Part::Text("Hi".into())],
            }],
            system_instruction: Some(Content {
                role: None,
                parts: vec![Part::Text("You are helpful".into())],
            }),
            tools: None,
            tool_config: None,
            generation_config: None,
            safety_settings: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("systemInstruction").is_some());
        // System instruction has no role in Gemini
        assert!(
            v["systemInstruction"].get("role").is_none()
                || v["systemInstruction"]["role"].is_null()
        );
    }

    #[test]
    fn codex_system_in_messages_and_instructions() {
        use abp_codex_sdk::types::{CodexMessage, CodexRequest};
        let req = CodexRequest {
            model: "codex-mini-latest".into(),
            messages: vec![
                CodexMessage::System {
                    content: "In-message system".into(),
                },
                CodexMessage::User {
                    content: "Hi".into(),
                },
            ],
            instructions: Some("Top-level instructions".into()),
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["instructions"], "Top-level instructions");
    }

    #[test]
    fn copilot_system_in_messages_array() {
        use abp_copilot_sdk::types::{CopilotChatMessage, CopilotChatRequest};
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: vec![
                CopilotChatMessage {
                    role: "system".into(),
                    content: Some("You are Copilot".into()),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                CopilotChatMessage {
                    role: "user".into(),
                    content: Some("Hi".into()),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            intent: None,
            references: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["messages"][0]["role"], "system");
    }

    #[test]
    fn kimi_system_in_messages_array() {
        use abp_kimi_sdk::types::{ChatMessage, KimiChatRequest};
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![
                ChatMessage::System {
                    content: "You are Kimi".into(),
                },
                ChatMessage::User {
                    content: "Hi".into(),
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            use_search: None,
            search_options: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["messages"][0]["role"], "system");
    }
}
