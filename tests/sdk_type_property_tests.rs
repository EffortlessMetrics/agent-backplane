//! Property-based tests for all SDK shim types and core contract types.

use std::collections::BTreeMap;

use chrono::Utc;
use proptest::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Core types ──────────────────────────────────────────────────────────────
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityRequirements, ContextPacket,
    ExecutionLane, ExecutionMode, Outcome, PolicyProfile, Receipt, RunMetadata, RuntimeConfig,
    SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkspaceMode, WorkspaceSpec,
};

// ── OpenAI SDK types ────────────────────────────────────────────────────────
use abp_openai_sdk::api::{
    AssistantMessage, ChatCompletionRequest, ChatCompletionResponse, Choice, Delta, FinishReason,
    FunctionCall, FunctionDefinition, Message, StreamChoice, StreamChunk, Tool, ToolCall, Usage,
};

// ── Claude SDK types ────────────────────────────────────────────────────────
use abp_claude_sdk::dialect::{
    ClaudeContentBlock, ClaudeResponse, ClaudeStreamDelta, ClaudeStreamEvent, ClaudeToolDef,
    ClaudeUsage,
};
use abp_claude_sdk::messages::{
    Message as ClaudeMessage, MessageContent, MessagesRequest, MessagesResponse, Role,
};

// ── Gemini SDK types ────────────────────────────────────────────────────────
use abp_gemini_sdk::types::{
    Candidate, Content, FunctionCallingMode, FunctionDeclaration, GeminiTool,
    GenerateContentRequest, GenerateContentResponse, GenerationConfig, HarmCategory,
    HarmProbability, Part, SafetyRating, UsageMetadata,
};

// ── Codex SDK types ─────────────────────────────────────────────────────────
use abp_codex_sdk::types::{
    CodexChoice, CodexChoiceMessage, CodexFileChange, CodexFunctionCall, CodexFunctionDef,
    CodexMessage, CodexRequest, CodexResponse, CodexTool, CodexToolCall, CodexUsage, FileOperation,
};

// ── Kimi SDK types ──────────────────────────────────────────────────────────
use abp_kimi_sdk::types::{
    ChatMessage as KimiChatMessage, Choice as KimiChoice, ChoiceMessage as KimiChoiceMessage,
    FunctionCall as KimiFunctionCall, FunctionDef as KimiFunctionDef, KimiChatRequest,
    KimiChatResponse, KimiUsage, SearchMode, SearchOptions, Tool as KimiTool,
    ToolCall as KimiToolCall,
};

// ── Copilot SDK types ───────────────────────────────────────────────────────
use abp_copilot_sdk::types::{
    CopilotChatChoice, CopilotChatChoiceMessage, CopilotChatMessage, CopilotChatRequest,
    CopilotChatResponse, CopilotFunctionCall, CopilotTool, CopilotToolCall, CopilotToolFunction,
    CopilotUsage, Reference, ReferenceType,
};

// ═══════════════════════════════════════════════════════════════════════════
//  Helper: JSON roundtrip for types that may lack PartialEq
// ═══════════════════════════════════════════════════════════════════════════

fn json_roundtrip<T: Serialize + for<'de> Deserialize<'de>>(val: &T) -> serde_json::Value {
    let json = serde_json::to_string(val).expect("serialize");
    serde_json::from_str::<serde_json::Value>(&json).expect("deserialize to Value")
}

fn assert_roundtrip<T: Serialize + for<'de> Deserialize<'de>>(val: &T) {
    let v1 = json_roundtrip(val);
    let back: T = serde_json::from_value(v1.clone()).expect("deserialize");
    let v2 = json_roundtrip(&back);
    assert_eq!(v1, v2);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Strategies: small building blocks
// ═══════════════════════════════════════════════════════════════════════════

fn safe_string() -> BoxedStrategy<String> {
    "[a-zA-Z0-9_.-]{1,24}".boxed()
}

fn safe_text() -> BoxedStrategy<String> {
    "[a-zA-Z0-9 _.,!?-]{0,64}".boxed()
}

fn safe_f64() -> BoxedStrategy<f64> {
    (0.0f64..=2.0).boxed()
}

fn opt_safe_f64() -> BoxedStrategy<Option<f64>> {
    prop::option::of(safe_f64()).boxed()
}

fn small_u32() -> BoxedStrategy<u32> {
    (0u32..1000).boxed()
}

fn small_u64() -> BoxedStrategy<u64> {
    (0u64..100_000).boxed()
}

fn json_obj() -> BoxedStrategy<serde_json::Value> {
    safe_string()
        .prop_map(|s| serde_json::json!({ "key": s }))
        .boxed()
}

// ═══════════════════════════════════════════════════════════════════════════
//  Strategies: OpenAI
// ═══════════════════════════════════════════════════════════════════════════

fn arb_openai_function_call() -> impl Strategy<Value = FunctionCall> {
    (safe_string(), safe_string()).prop_map(|(name, args)| FunctionCall {
        name,
        arguments: args,
    })
}

fn arb_openai_tool_call() -> impl Strategy<Value = ToolCall> {
    (safe_string(), arb_openai_function_call()).prop_map(|(id, function)| ToolCall {
        id,
        call_type: "function".into(),
        function,
    })
}

fn arb_openai_message() -> BoxedStrategy<Message> {
    prop_oneof![
        safe_text().prop_map(|c| Message::System { content: c }),
        safe_text().prop_map(|c| Message::User { content: c }),
        (
            prop::option::of(safe_text()),
            prop::option::of(prop::collection::vec(arb_openai_tool_call(), 0..2))
        )
            .prop_map(|(c, tc)| Message::Assistant {
                content: c,
                tool_calls: tc
            }),
        (safe_string(), safe_text()).prop_map(|(id, c)| Message::Tool {
            tool_call_id: id,
            content: c
        }),
    ]
    .boxed()
}

fn arb_openai_function_def() -> impl Strategy<Value = FunctionDefinition> {
    (
        safe_string(),
        prop::option::of(safe_text()),
        prop::option::of(json_obj()),
        prop::option::of(prop::bool::ANY),
    )
        .prop_map(|(name, desc, params, strict)| FunctionDefinition {
            name,
            description: desc,
            parameters: params,
            strict,
        })
}

fn arb_openai_tool() -> impl Strategy<Value = Tool> {
    arb_openai_function_def().prop_map(|f| Tool {
        tool_type: "function".into(),
        function: f,
    })
}

fn arb_finish_reason() -> impl Strategy<Value = FinishReason> {
    prop_oneof![
        Just(FinishReason::Stop),
        Just(FinishReason::Length),
        Just(FinishReason::ToolCalls),
        Just(FinishReason::ContentFilter),
    ]
}

fn arb_openai_usage() -> impl Strategy<Value = Usage> {
    (small_u64(), small_u64(), small_u64()).prop_map(|(p, c, t)| Usage {
        prompt_tokens: p,
        completion_tokens: c,
        total_tokens: t,
    })
}

fn arb_chat_completion_request() -> impl Strategy<Value = ChatCompletionRequest> {
    (
        safe_string(),
        prop::collection::vec(arb_openai_message(), 1..4),
        opt_safe_f64(),
        prop::option::of(small_u32()),
        prop::option::of(prop::collection::vec(arb_openai_tool(), 0..2)),
    )
        .prop_map(
            |(model, messages, temp, max_tokens, tools)| ChatCompletionRequest {
                model,
                messages,
                temperature: temp,
                max_tokens,
                tools,
                tool_choice: None,
                stream: Some(false),
                top_p: None,
                frequency_penalty: None,
                presence_penalty: None,
                stop: None,
                n: None,
                seed: None,
                response_format: None,
                user: None,
            },
        )
}

fn arb_assistant_message() -> impl Strategy<Value = AssistantMessage> {
    (
        prop::option::of(safe_text()),
        prop::option::of(prop::collection::vec(arb_openai_tool_call(), 0..2)),
    )
        .prop_map(|(content, tool_calls)| AssistantMessage {
            role: "assistant".into(),
            content,
            tool_calls,
        })
}

fn arb_openai_choice() -> impl Strategy<Value = Choice> {
    (small_u32(), arb_assistant_message(), arb_finish_reason()).prop_map(|(index, message, fr)| {
        Choice {
            index,
            message,
            finish_reason: fr,
        }
    })
}

fn arb_chat_completion_response() -> impl Strategy<Value = ChatCompletionResponse> {
    (
        safe_string(),
        small_u64(),
        safe_string(),
        prop::collection::vec(arb_openai_choice(), 1..3),
        prop::option::of(arb_openai_usage()),
    )
        .prop_map(
            |(id, created, model, choices, usage)| ChatCompletionResponse {
                id,
                object: "chat.completion".into(),
                created,
                model,
                choices,
                usage,
                system_fingerprint: None,
            },
        )
}

fn arb_delta() -> impl Strategy<Value = Delta> {
    (
        prop::option::of(safe_string()),
        prop::option::of(safe_text()),
        prop::option::of(prop::collection::vec(arb_openai_tool_call(), 0..1)),
    )
        .prop_map(|(role, content, tool_calls)| Delta {
            role,
            content,
            tool_calls,
        })
}

fn arb_stream_choice() -> impl Strategy<Value = StreamChoice> {
    (
        small_u32(),
        arb_delta(),
        prop::option::of(arb_finish_reason()),
    )
        .prop_map(|(index, delta, fr)| StreamChoice {
            index,
            delta,
            finish_reason: fr,
        })
}

fn arb_stream_chunk() -> impl Strategy<Value = StreamChunk> {
    (
        safe_string(),
        small_u64(),
        safe_string(),
        prop::collection::vec(arb_stream_choice(), 1..3),
    )
        .prop_map(|(id, created, model, choices)| StreamChunk {
            id,
            object: "chat.completion.chunk".into(),
            created,
            model,
            choices,
            usage: None,
        })
}

// ═══════════════════════════════════════════════════════════════════════════
//  Strategies: Claude
// ═══════════════════════════════════════════════════════════════════════════

fn arb_claude_usage() -> impl Strategy<Value = ClaudeUsage> {
    (small_u64(), small_u64()).prop_map(|(i, o)| ClaudeUsage {
        input_tokens: i,
        output_tokens: o,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    })
}

fn arb_content_block() -> BoxedStrategy<ClaudeContentBlock> {
    prop_oneof![
        safe_text().prop_map(|t| ClaudeContentBlock::Text { text: t }),
        (safe_string(), safe_string(), json_obj())
            .prop_map(|(id, name, input)| ClaudeContentBlock::ToolUse { id, name, input }),
        (
            safe_string(),
            prop::option::of(safe_text()),
            prop::option::of(prop::bool::ANY)
        )
            .prop_map(|(id, content, is_error)| ClaudeContentBlock::ToolResult {
                tool_use_id: id,
                content,
                is_error
            }),
        (safe_text(), prop::option::of(safe_string())).prop_map(|(thinking, sig)| {
            ClaudeContentBlock::Thinking {
                thinking,
                signature: sig,
            }
        }),
    ]
    .boxed()
}

fn arb_role() -> impl Strategy<Value = Role> {
    prop_oneof![Just(Role::User), Just(Role::Assistant)]
}

fn arb_message_content() -> BoxedStrategy<MessageContent> {
    prop_oneof![
        safe_text().prop_map(MessageContent::Text),
        prop::collection::vec(arb_content_block(), 1..3).prop_map(MessageContent::Blocks),
    ]
    .boxed()
}

fn arb_claude_message() -> impl Strategy<Value = ClaudeMessage> {
    (arb_role(), arb_message_content()).prop_map(|(role, content)| ClaudeMessage { role, content })
}

fn arb_claude_tool_def() -> impl Strategy<Value = ClaudeToolDef> {
    (safe_string(), safe_text(), json_obj()).prop_map(|(name, description, schema)| ClaudeToolDef {
        name,
        description,
        input_schema: schema,
    })
}

fn arb_messages_request() -> impl Strategy<Value = MessagesRequest> {
    (
        safe_string(),
        prop::collection::vec(arb_claude_message(), 1..3),
        small_u32(),
        prop::option::of(prop::collection::vec(arb_claude_tool_def(), 0..2)),
        opt_safe_f64(),
    )
        .prop_map(
            |(model, messages, max_tokens, tools, temp)| MessagesRequest {
                model,
                messages,
                max_tokens,
                system: None,
                tools,
                metadata: None,
                stream: Some(false),
                stop_sequences: None,
                temperature: temp,
                top_p: None,
                top_k: None,
            },
        )
}

fn arb_messages_response() -> impl Strategy<Value = MessagesResponse> {
    (
        safe_string(),
        safe_string(),
        prop::collection::vec(arb_content_block(), 1..3),
        arb_claude_usage(),
    )
        .prop_map(|(id, model, content, usage)| MessagesResponse {
            id,
            response_type: "message".into(),
            role: "assistant".into(),
            content,
            model,
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage,
        })
}

fn arb_claude_stream_delta() -> BoxedStrategy<ClaudeStreamDelta> {
    prop_oneof![
        safe_text().prop_map(|t| ClaudeStreamDelta::TextDelta { text: t }),
        safe_text().prop_map(|p| ClaudeStreamDelta::InputJsonDelta { partial_json: p }),
        safe_text().prop_map(|t| ClaudeStreamDelta::ThinkingDelta { thinking: t }),
        safe_text().prop_map(|s| ClaudeStreamDelta::SignatureDelta { signature: s }),
    ]
    .boxed()
}

fn arb_claude_response() -> impl Strategy<Value = ClaudeResponse> {
    (
        safe_string(),
        safe_string(),
        prop::collection::vec(arb_content_block(), 0..3),
        prop::option::of(arb_claude_usage()),
    )
        .prop_map(|(id, model, content, usage)| ClaudeResponse {
            id,
            model,
            role: "assistant".into(),
            content,
            stop_reason: Some("end_turn".into()),
            usage,
        })
}

fn arb_claude_stream_event() -> BoxedStrategy<ClaudeStreamEvent> {
    prop_oneof![
        arb_claude_response().prop_map(|m| ClaudeStreamEvent::MessageStart { message: m }),
        (small_u32(), arb_content_block()).prop_map(|(i, cb)| {
            ClaudeStreamEvent::ContentBlockStart {
                index: i,
                content_block: cb,
            }
        }),
        (small_u32(), arb_claude_stream_delta())
            .prop_map(|(i, d)| ClaudeStreamEvent::ContentBlockDelta { index: i, delta: d }),
        small_u32().prop_map(|i| ClaudeStreamEvent::ContentBlockStop { index: i }),
        Just(ClaudeStreamEvent::MessageStop {}),
        Just(ClaudeStreamEvent::Ping {}),
    ]
    .boxed()
}

// ═══════════════════════════════════════════════════════════════════════════
//  Strategies: Gemini
// ═══════════════════════════════════════════════════════════════════════════

fn arb_part() -> BoxedStrategy<Part> {
    prop_oneof![
        safe_text().prop_map(Part::Text),
        (safe_string(), safe_string()).prop_map(|(name, args)| Part::FunctionCall {
            name,
            args: serde_json::json!({ "arg": args }),
        }),
        (safe_string(), safe_string()).prop_map(|(name, resp)| Part::FunctionResponse {
            name,
            response: serde_json::json!({ "result": resp }),
        }),
    ]
    .boxed()
}

fn arb_content() -> impl Strategy<Value = Content> {
    (
        prop::option::of(safe_string()),
        prop::collection::vec(arb_part(), 1..3),
    )
        .prop_map(|(role, parts)| Content { role, parts })
}

fn arb_harm_category() -> impl Strategy<Value = HarmCategory> {
    prop_oneof![
        Just(HarmCategory::HarmCategoryHarassment),
        Just(HarmCategory::HarmCategoryHateSpeech),
        Just(HarmCategory::HarmCategorySexuallyExplicit),
        Just(HarmCategory::HarmCategoryDangerousContent),
        Just(HarmCategory::HarmCategoryCivicIntegrity),
    ]
}

fn arb_harm_probability() -> impl Strategy<Value = HarmProbability> {
    prop_oneof![
        Just(HarmProbability::Negligible),
        Just(HarmProbability::Low),
        Just(HarmProbability::Medium),
        Just(HarmProbability::High),
    ]
}

fn arb_safety_rating() -> impl Strategy<Value = SafetyRating> {
    (arb_harm_category(), arb_harm_probability()).prop_map(|(category, probability)| SafetyRating {
        category,
        probability,
    })
}

fn arb_usage_metadata() -> impl Strategy<Value = UsageMetadata> {
    (small_u64(), small_u64(), small_u64()).prop_map(|(p, c, t)| UsageMetadata {
        prompt_token_count: p,
        candidates_token_count: c,
        total_token_count: t,
    })
}

fn arb_candidate() -> impl Strategy<Value = Candidate> {
    (
        arb_content(),
        prop::option::of(safe_string()),
        prop::option::of(prop::collection::vec(arb_safety_rating(), 0..2)),
    )
        .prop_map(|(content, fr, ratings)| Candidate {
            content,
            finish_reason: fr,
            safety_ratings: ratings,
        })
}

fn arb_function_declaration() -> impl Strategy<Value = FunctionDeclaration> {
    (safe_string(), safe_text(), json_obj()).prop_map(|(name, desc, params)| FunctionDeclaration {
        name,
        description: desc,
        parameters: params,
    })
}

fn arb_gemini_tool() -> impl Strategy<Value = GeminiTool> {
    prop::collection::vec(arb_function_declaration(), 1..3).prop_map(|fds| GeminiTool {
        function_declarations: fds,
    })
}

fn arb_function_calling_mode() -> impl Strategy<Value = FunctionCallingMode> {
    prop_oneof![
        Just(FunctionCallingMode::Auto),
        Just(FunctionCallingMode::Any),
        Just(FunctionCallingMode::None),
    ]
}

fn arb_generation_config() -> impl Strategy<Value = GenerationConfig> {
    (
        opt_safe_f64(),
        opt_safe_f64(),
        prop::option::of(small_u32()),
        prop::option::of(small_u32()),
    )
        .prop_map(|(temp, top_p, top_k, max_tokens)| GenerationConfig {
            temperature: temp,
            top_p,
            top_k,
            max_output_tokens: max_tokens,
            candidate_count: None,
            stop_sequences: None,
        })
}

fn arb_generate_content_request() -> impl Strategy<Value = GenerateContentRequest> {
    (
        prop::collection::vec(arb_content(), 1..3),
        prop::option::of(arb_generation_config()),
        prop::option::of(prop::collection::vec(arb_gemini_tool(), 0..2)),
    )
        .prop_map(|(contents, gen_config, tools)| GenerateContentRequest {
            contents,
            system_instruction: None,
            tools,
            tool_config: None,
            generation_config: gen_config,
            safety_settings: None,
        })
}

fn arb_generate_content_response() -> impl Strategy<Value = GenerateContentResponse> {
    (
        prop::collection::vec(arb_candidate(), 1..2),
        prop::option::of(arb_usage_metadata()),
    )
        .prop_map(|(candidates, usage)| GenerateContentResponse {
            candidates,
            usage_metadata: usage,
            prompt_feedback: None,
        })
}

// ═══════════════════════════════════════════════════════════════════════════
//  Strategies: Codex
// ═══════════════════════════════════════════════════════════════════════════

fn arb_codex_message() -> BoxedStrategy<CodexMessage> {
    prop_oneof![
        safe_text().prop_map(|c| CodexMessage::System { content: c }),
        safe_text().prop_map(|c| CodexMessage::User { content: c }),
        (
            prop::option::of(safe_text()),
            prop::option::of(prop::collection::vec(arb_codex_tool_call(), 0..2))
        )
            .prop_map(|(c, tc)| CodexMessage::Assistant {
                content: c,
                tool_calls: tc
            }),
        (safe_text(), safe_string()).prop_map(|(c, id)| CodexMessage::Tool {
            content: c,
            tool_call_id: id
        }),
    ]
    .boxed()
}

fn arb_codex_tool_call() -> impl Strategy<Value = CodexToolCall> {
    (safe_string(), safe_string(), safe_string()).prop_map(|(id, name, args)| CodexToolCall {
        id,
        call_type: "function".into(),
        function: CodexFunctionCall {
            name,
            arguments: args,
        },
    })
}

fn arb_codex_tool() -> impl Strategy<Value = CodexTool> {
    (safe_string(), safe_text(), json_obj()).prop_map(|(name, desc, params)| CodexTool {
        tool_type: "function".into(),
        function: CodexFunctionDef {
            name,
            description: desc,
            parameters: params,
        },
    })
}

fn arb_codex_usage() -> impl Strategy<Value = CodexUsage> {
    (small_u64(), small_u64(), small_u64()).prop_map(|(p, c, t)| CodexUsage {
        prompt_tokens: p,
        completion_tokens: c,
        total_tokens: t,
    })
}

fn arb_codex_request() -> impl Strategy<Value = CodexRequest> {
    (
        safe_string(),
        prop::collection::vec(arb_codex_message(), 1..4),
        opt_safe_f64(),
        prop::option::of(small_u32()),
        prop::option::of(prop::collection::vec(arb_codex_tool(), 0..2)),
    )
        .prop_map(|(model, messages, temp, max_tokens, tools)| CodexRequest {
            model,
            messages,
            instructions: None,
            temperature: temp,
            top_p: None,
            max_tokens,
            stream: Some(false),
            tools,
            tool_choice: None,
        })
}

fn arb_codex_choice() -> impl Strategy<Value = CodexChoice> {
    (
        small_u32(),
        prop::option::of(safe_text()),
        prop::option::of(prop::collection::vec(arb_codex_tool_call(), 0..2)),
    )
        .prop_map(|(index, content, tool_calls)| CodexChoice {
            index,
            message: CodexChoiceMessage {
                role: "assistant".into(),
                content,
                tool_calls,
            },
            finish_reason: Some("stop".into()),
        })
}

fn arb_codex_response() -> impl Strategy<Value = CodexResponse> {
    (
        safe_string(),
        small_u64(),
        safe_string(),
        prop::collection::vec(arb_codex_choice(), 1..3),
        prop::option::of(arb_codex_usage()),
    )
        .prop_map(|(id, created, model, choices, usage)| CodexResponse {
            id,
            object: "chat.completion".into(),
            created,
            model,
            choices,
            usage,
        })
}

fn arb_file_operation() -> impl Strategy<Value = FileOperation> {
    prop_oneof![
        Just(FileOperation::Create),
        Just(FileOperation::Update),
        Just(FileOperation::Delete),
        Just(FileOperation::Patch),
    ]
}

fn arb_codex_file_change() -> impl Strategy<Value = CodexFileChange> {
    (
        safe_string(),
        arb_file_operation(),
        prop::option::of(safe_text()),
        prop::option::of(safe_text()),
    )
        .prop_map(|(path, op, content, diff)| CodexFileChange {
            path,
            operation: op,
            content,
            diff,
        })
}

// ═══════════════════════════════════════════════════════════════════════════
//  Strategies: Kimi
// ═══════════════════════════════════════════════════════════════════════════

fn arb_search_mode() -> impl Strategy<Value = SearchMode> {
    prop_oneof![
        Just(SearchMode::Auto),
        Just(SearchMode::Always),
        Just(SearchMode::Never),
    ]
}

fn arb_search_options() -> impl Strategy<Value = SearchOptions> {
    (arb_search_mode(), prop::option::of(small_u32())).prop_map(|(mode, count)| SearchOptions {
        mode,
        result_count: count,
    })
}

fn arb_kimi_tool_call() -> impl Strategy<Value = KimiToolCall> {
    (safe_string(), safe_string(), safe_string()).prop_map(|(id, name, args)| KimiToolCall {
        id,
        call_type: "function".into(),
        function: KimiFunctionCall {
            name,
            arguments: args,
        },
    })
}

fn arb_kimi_message() -> BoxedStrategy<KimiChatMessage> {
    prop_oneof![
        safe_text().prop_map(|c| KimiChatMessage::System { content: c }),
        safe_text().prop_map(|c| KimiChatMessage::User { content: c }),
        (
            prop::option::of(safe_text()),
            prop::option::of(prop::collection::vec(arb_kimi_tool_call(), 0..2))
        )
            .prop_map(|(c, tc)| KimiChatMessage::Assistant {
                content: c,
                tool_calls: tc
            }),
        (safe_text(), safe_string()).prop_map(|(c, id)| KimiChatMessage::Tool {
            content: c,
            tool_call_id: id
        }),
    ]
    .boxed()
}

fn arb_kimi_tool() -> impl Strategy<Value = KimiTool> {
    (safe_string(), safe_text(), json_obj()).prop_map(|(name, desc, params)| KimiTool {
        tool_type: "function".into(),
        function: KimiFunctionDef {
            name,
            description: desc,
            parameters: params,
        },
    })
}

fn arb_kimi_usage() -> impl Strategy<Value = KimiUsage> {
    (small_u64(), small_u64(), small_u64()).prop_map(|(p, c, t)| KimiUsage {
        prompt_tokens: p,
        completion_tokens: c,
        total_tokens: t,
        search_tokens: None,
    })
}

fn arb_kimi_request() -> impl Strategy<Value = KimiChatRequest> {
    (
        safe_string(),
        prop::collection::vec(arb_kimi_message(), 1..4),
        opt_safe_f64(),
        prop::option::of(small_u32()),
        prop::option::of(prop::collection::vec(arb_kimi_tool(), 0..2)),
        prop::option::of(arb_search_options()),
    )
        .prop_map(
            |(model, messages, temp, max_tokens, tools, search)| KimiChatRequest {
                model,
                messages,
                temperature: temp,
                top_p: None,
                max_tokens,
                stream: Some(false),
                tools,
                tool_choice: None,
                use_search: None,
                search_options: search,
            },
        )
}

fn arb_kimi_choice() -> impl Strategy<Value = KimiChoice> {
    (
        small_u32(),
        prop::option::of(safe_text()),
        prop::option::of(prop::collection::vec(arb_kimi_tool_call(), 0..2)),
    )
        .prop_map(|(index, content, tool_calls)| KimiChoice {
            index,
            message: KimiChoiceMessage {
                role: "assistant".into(),
                content,
                tool_calls,
            },
            finish_reason: Some("stop".into()),
        })
}

fn arb_kimi_response() -> impl Strategy<Value = KimiChatResponse> {
    (
        safe_string(),
        small_u64(),
        safe_string(),
        prop::collection::vec(arb_kimi_choice(), 1..3),
        prop::option::of(arb_kimi_usage()),
    )
        .prop_map(|(id, created, model, choices, usage)| KimiChatResponse {
            id,
            object: "chat.completion".into(),
            created,
            model,
            choices,
            usage,
        })
}

// ═══════════════════════════════════════════════════════════════════════════
//  Strategies: Copilot
// ═══════════════════════════════════════════════════════════════════════════

fn arb_reference_type() -> impl Strategy<Value = ReferenceType> {
    prop_oneof![
        Just(ReferenceType::File),
        Just(ReferenceType::Selection),
        Just(ReferenceType::Terminal),
        Just(ReferenceType::WebPage),
        Just(ReferenceType::GitDiff),
    ]
}

fn arb_reference() -> impl Strategy<Value = Reference> {
    (
        arb_reference_type(),
        safe_string(),
        prop::option::of(safe_string()),
        prop::option::of(safe_text()),
    )
        .prop_map(|(ref_type, id, uri, content)| Reference {
            ref_type,
            id,
            uri,
            content,
            metadata: None,
        })
}

fn arb_copilot_tool_call() -> impl Strategy<Value = CopilotToolCall> {
    (safe_string(), safe_string(), safe_string()).prop_map(|(id, name, args)| CopilotToolCall {
        id,
        call_type: "function".into(),
        function: CopilotFunctionCall {
            name,
            arguments: args,
        },
    })
}

fn arb_copilot_message() -> impl Strategy<Value = CopilotChatMessage> {
    (
        safe_string(),
        prop::option::of(safe_text()),
        prop::option::of(safe_string()),
        prop::option::of(prop::collection::vec(arb_copilot_tool_call(), 0..2)),
    )
        .prop_map(|(role, content, name, tool_calls)| CopilotChatMessage {
            role,
            content,
            name,
            tool_calls,
            tool_call_id: None,
        })
}

fn arb_copilot_tool() -> impl Strategy<Value = CopilotTool> {
    (safe_string(), safe_text(), json_obj()).prop_map(|(name, desc, params)| CopilotTool {
        tool_type: "function".into(),
        function: CopilotToolFunction {
            name,
            description: desc,
            parameters: params,
        },
    })
}

fn arb_copilot_usage() -> impl Strategy<Value = CopilotUsage> {
    (small_u64(), small_u64(), small_u64()).prop_map(|(p, c, t)| CopilotUsage {
        prompt_tokens: p,
        completion_tokens: c,
        total_tokens: t,
        copilot_tokens: None,
    })
}

fn arb_copilot_request() -> impl Strategy<Value = CopilotChatRequest> {
    (
        safe_string(),
        prop::collection::vec(arb_copilot_message(), 1..4),
        opt_safe_f64(),
        prop::option::of(small_u32()),
        prop::option::of(prop::collection::vec(arb_copilot_tool(), 0..2)),
        prop::option::of(prop::collection::vec(arb_reference(), 0..3)),
    )
        .prop_map(
            |(model, messages, temp, max_tokens, tools, refs)| CopilotChatRequest {
                model,
                messages,
                temperature: temp,
                top_p: None,
                max_tokens,
                stream: Some(false),
                tools,
                tool_choice: None,
                intent: None,
                references: refs,
            },
        )
}

fn arb_copilot_choice() -> impl Strategy<Value = CopilotChatChoice> {
    (
        small_u32(),
        prop::option::of(safe_text()),
        prop::option::of(prop::collection::vec(arb_copilot_tool_call(), 0..2)),
    )
        .prop_map(|(index, content, tool_calls)| CopilotChatChoice {
            index,
            message: CopilotChatChoiceMessage {
                role: "assistant".into(),
                content,
                tool_calls,
            },
            finish_reason: Some("stop".into()),
        })
}

fn arb_copilot_response() -> impl Strategy<Value = CopilotChatResponse> {
    (
        safe_string(),
        small_u64(),
        safe_string(),
        prop::collection::vec(arb_copilot_choice(), 1..3),
        prop::option::of(arb_copilot_usage()),
    )
        .prop_map(|(id, created, model, choices, usage)| CopilotChatResponse {
            id,
            object: "chat.completion".into(),
            created,
            model,
            choices,
            usage,
        })
}

// ═══════════════════════════════════════════════════════════════════════════
//  Strategies: Core (ABP contract types)
// ═══════════════════════════════════════════════════════════════════════════

fn arb_execution_lane() -> impl Strategy<Value = ExecutionLane> {
    prop_oneof![
        Just(ExecutionLane::PatchFirst),
        Just(ExecutionLane::WorkspaceFirst)
    ]
}

fn arb_workspace_mode() -> impl Strategy<Value = WorkspaceMode> {
    prop_oneof![
        Just(WorkspaceMode::PassThrough),
        Just(WorkspaceMode::Staged)
    ]
}

fn arb_capability() -> impl Strategy<Value = Capability> {
    prop_oneof![
        Just(Capability::Streaming),
        Just(Capability::ToolRead),
        Just(Capability::ToolWrite),
        Just(Capability::ToolEdit),
        Just(Capability::ToolBash),
        Just(Capability::FunctionCalling),
        Just(Capability::Vision),
        Just(Capability::JsonMode),
        Just(Capability::SystemMessage),
        Just(Capability::Temperature),
        Just(Capability::MaxTokens),
        Just(Capability::ExtendedThinking),
        Just(Capability::ImageInput),
    ]
}

fn arb_support_level() -> impl Strategy<Value = SupportLevel> {
    prop_oneof![
        Just(SupportLevel::Native),
        Just(SupportLevel::Emulated),
        Just(SupportLevel::Unsupported),
        safe_string().prop_map(|r| SupportLevel::Restricted { reason: r }),
    ]
}

fn arb_execution_mode() -> impl Strategy<Value = ExecutionMode> {
    prop_oneof![
        Just(ExecutionMode::Passthrough),
        Just(ExecutionMode::Mapped)
    ]
}

fn arb_outcome() -> impl Strategy<Value = Outcome> {
    prop_oneof![
        Just(Outcome::Complete),
        Just(Outcome::Partial),
        Just(Outcome::Failed)
    ]
}

fn arb_work_order() -> impl Strategy<Value = WorkOrder> {
    (
        safe_string(), // task
        arb_execution_lane(),
        safe_string(), // workspace root
        arb_workspace_mode(),
        prop::option::of(safe_string()), // model
    )
        .prop_map(|(task, lane, root, ws_mode, model)| WorkOrder {
            id: Uuid::new_v4(),
            task,
            lane,
            workspace: WorkspaceSpec {
                root,
                mode: ws_mode,
                include: vec![],
                exclude: vec![],
            },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig {
                model,
                vendor: BTreeMap::new(),
                env: BTreeMap::new(),
                max_budget_usd: None,
                max_turns: None,
            },
        })
}

fn arb_agent_event_kind() -> BoxedStrategy<AgentEventKind> {
    prop_oneof![
        safe_text().prop_map(|m| AgentEventKind::RunStarted { message: m }),
        safe_text().prop_map(|m| AgentEventKind::RunCompleted { message: m }),
        safe_text().prop_map(|t| AgentEventKind::AssistantDelta { text: t }),
        safe_text().prop_map(|t| AgentEventKind::AssistantMessage { text: t }),
        (safe_string(), json_obj()).prop_map(|(name, input)| AgentEventKind::ToolCall {
            tool_name: name,
            tool_use_id: None,
            parent_tool_use_id: None,
            input,
        }),
        (safe_string(), json_obj()).prop_map(|(name, output)| AgentEventKind::ToolResult {
            tool_name: name,
            tool_use_id: None,
            output,
            is_error: false,
        }),
        (safe_string(), safe_text()).prop_map(|(p, s)| AgentEventKind::FileChanged {
            path: p,
            summary: s
        }),
        safe_text().prop_map(|m| AgentEventKind::Warning { message: m }),
        safe_text().prop_map(|m| AgentEventKind::Error {
            message: m,
            error_code: None
        }),
    ]
    .boxed()
}

fn arb_agent_event() -> impl Strategy<Value = AgentEvent> {
    arb_agent_event_kind().prop_map(|kind| AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    })
}

fn arb_receipt() -> impl Strategy<Value = Receipt> {
    (
        arb_execution_mode(),
        arb_outcome(),
        prop::collection::vec(arb_agent_event(), 0..3),
    )
        .prop_map(|(mode, outcome, trace)| {
            let now = Utc::now();
            Receipt {
                meta: RunMetadata {
                    run_id: Uuid::new_v4(),
                    work_order_id: Uuid::new_v4(),
                    contract_version: "abp/v0.1".into(),
                    started_at: now,
                    finished_at: now,
                    duration_ms: 42,
                },
                backend: BackendIdentity {
                    id: "test-backend".into(),
                    backend_version: Some("1.0".into()),
                    adapter_version: Some("0.1".into()),
                },
                capabilities: BTreeMap::new(),
                mode,
                usage_raw: serde_json::json!({}),
                usage: UsageNormalized::default(),
                trace,
                artifacts: vec![],
                verification: VerificationReport::default(),
                outcome,
                receipt_sha256: None,
            }
        })
}

// ═══════════════════════════════════════════════════════════════════════════
//  TESTS — OpenAI
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    // ── OpenAI ──────────────────────────────────────────────────────────

    #[test]
    fn openai_request_roundtrip(req in arb_chat_completion_request()) {
        assert_roundtrip(&req);
    }

    #[test]
    fn openai_response_roundtrip(resp in arb_chat_completion_response()) {
        assert_roundtrip(&resp);
    }

    #[test]
    fn openai_stream_chunk_roundtrip(chunk in arb_stream_chunk()) {
        assert_roundtrip(&chunk);
    }

    #[test]
    fn openai_message_roundtrip(msg in arb_openai_message()) {
        assert_roundtrip(&msg);
    }

    #[test]
    fn openai_finish_reason_roundtrip(fr in arb_finish_reason()) {
        assert_roundtrip(&fr);
    }

    // ── Claude ──────────────────────────────────────────────────────────

    #[test]
    fn claude_request_roundtrip(req in arb_messages_request()) {
        assert_roundtrip(&req);
    }

    #[test]
    fn claude_response_roundtrip(resp in arb_messages_response()) {
        assert_roundtrip(&resp);
    }

    #[test]
    fn claude_content_block_roundtrip(block in arb_content_block()) {
        assert_roundtrip(&block);
    }

    #[test]
    fn claude_stream_event_roundtrip(event in arb_claude_stream_event()) {
        assert_roundtrip(&event);
    }

    #[test]
    fn claude_role_roundtrip(role in arb_role()) {
        assert_roundtrip(&role);
    }

    // ── Gemini ──────────────────────────────────────────────────────────

    #[test]
    fn gemini_request_roundtrip(req in arb_generate_content_request()) {
        assert_roundtrip(&req);
    }

    #[test]
    fn gemini_response_roundtrip(resp in arb_generate_content_response()) {
        assert_roundtrip(&resp);
    }

    #[test]
    fn gemini_part_roundtrip(part in arb_part()) {
        assert_roundtrip(&part);
    }

    #[test]
    fn gemini_content_roundtrip(content in arb_content()) {
        assert_roundtrip(&content);
    }

    #[test]
    fn gemini_function_calling_mode_roundtrip(mode in arb_function_calling_mode()) {
        assert_roundtrip(&mode);
    }

    // ── Codex ───────────────────────────────────────────────────────────

    #[test]
    fn codex_request_roundtrip(req in arb_codex_request()) {
        assert_roundtrip(&req);
    }

    #[test]
    fn codex_response_roundtrip(resp in arb_codex_response()) {
        assert_roundtrip(&resp);
    }

    #[test]
    fn codex_file_change_roundtrip(fc in arb_codex_file_change()) {
        assert_roundtrip(&fc);
    }

    #[test]
    fn codex_file_operation_roundtrip(op in arb_file_operation()) {
        assert_roundtrip(&op);
    }

    #[test]
    fn codex_message_roundtrip(msg in arb_codex_message()) {
        assert_roundtrip(&msg);
    }

    // ── Kimi ────────────────────────────────────────────────────────────

    #[test]
    fn kimi_request_roundtrip(req in arb_kimi_request()) {
        assert_roundtrip(&req);
    }

    #[test]
    fn kimi_response_roundtrip(resp in arb_kimi_response()) {
        assert_roundtrip(&resp);
    }

    #[test]
    fn kimi_search_options_roundtrip(opts in arb_search_options()) {
        assert_roundtrip(&opts);
    }

    #[test]
    fn kimi_search_mode_roundtrip(mode in arb_search_mode()) {
        assert_roundtrip(&mode);
    }

    #[test]
    fn kimi_message_roundtrip(msg in arb_kimi_message()) {
        assert_roundtrip(&msg);
    }

    // ── Copilot ─────────────────────────────────────────────────────────

    #[test]
    fn copilot_request_roundtrip(req in arb_copilot_request()) {
        assert_roundtrip(&req);
    }

    #[test]
    fn copilot_response_roundtrip(resp in arb_copilot_response()) {
        assert_roundtrip(&resp);
    }

    #[test]
    fn copilot_reference_roundtrip(r in arb_reference()) {
        assert_roundtrip(&r);
    }

    #[test]
    fn copilot_reference_type_roundtrip(rt in arb_reference_type()) {
        assert_roundtrip(&rt);
    }

    #[test]
    fn copilot_message_roundtrip(msg in arb_copilot_message()) {
        assert_roundtrip(&msg);
    }

    // ── Core contract types ─────────────────────────────────────────────

    #[test]
    fn core_work_order_roundtrip(wo in arb_work_order()) {
        assert_roundtrip(&wo);
    }

    #[test]
    fn core_receipt_roundtrip(receipt in arb_receipt()) {
        assert_roundtrip(&receipt);
    }

    #[test]
    fn core_agent_event_roundtrip(event in arb_agent_event()) {
        assert_roundtrip(&event);
    }

    #[test]
    fn core_capability_roundtrip(cap in arb_capability()) {
        assert_roundtrip(&cap);
    }

    #[test]
    fn core_support_level_roundtrip(sl in arb_support_level()) {
        assert_roundtrip(&sl);
    }

    #[test]
    fn core_outcome_roundtrip(o in arb_outcome()) {
        assert_roundtrip(&o);
    }

    #[test]
    fn core_execution_mode_roundtrip(m in arb_execution_mode()) {
        assert_roundtrip(&m);
    }

    // ── Hash determinism ────────────────────────────────────────────────

    #[test]
    fn receipt_hash_determinism(receipt in arb_receipt()) {
        let h1 = abp_core::receipt_hash(&receipt).expect("hash 1");
        let h2 = abp_core::receipt_hash(&receipt).expect("hash 2");
        prop_assert_eq!(h1, h2);
    }

    #[test]
    fn receipt_with_hash_is_stable(receipt in arb_receipt()) {
        let hashed = receipt.clone().with_hash().expect("with_hash");
        prop_assert!(hashed.receipt_sha256.is_some());
        let expected = abp_core::receipt_hash(&hashed).expect("recompute");
        prop_assert_eq!(hashed.receipt_sha256.as_deref().unwrap(), expected.as_str());
    }

    #[test]
    fn receipt_hash_excludes_sha256_field(receipt in arb_receipt()) {
        let mut r1 = receipt.clone();
        r1.receipt_sha256 = None;
        let mut r2 = receipt;
        r2.receipt_sha256 = Some("fake_hash_value".into());
        let h1 = abp_core::receipt_hash(&r1).expect("hash without");
        let h2 = abp_core::receipt_hash(&r2).expect("hash with fake");
        prop_assert_eq!(h1, h2);
    }

    // ── Canonical JSON / BTreeMap ordering ──────────────────────────────

    #[test]
    fn btreemap_keys_always_sorted(
        keys in prop::collection::vec(safe_string(), 1..10),
        vals in prop::collection::vec(safe_string(), 1..10),
    ) {
        let map: BTreeMap<String, String> = keys.into_iter()
            .zip(vals.into_iter())
            .collect();
        let json = serde_json::to_string(&map).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        if let serde_json::Value::Object(obj) = parsed {
            let keys_in_json: Vec<&String> = obj.keys().collect();
            let mut sorted = keys_in_json.clone();
            sorted.sort();
            prop_assert_eq!(keys_in_json, sorted);
        }
    }

    #[test]
    fn runtime_config_vendor_keys_sorted(
        k1 in safe_string(),
        k2 in safe_string(),
        k3 in safe_string(),
    ) {
        let mut vendor = BTreeMap::new();
        vendor.insert(k1, serde_json::json!("v1"));
        vendor.insert(k2, serde_json::json!("v2"));
        vendor.insert(k3, serde_json::json!("v3"));
        let config = RuntimeConfig {
            model: None,
            vendor,
            env: BTreeMap::new(),
            max_budget_usd: None,
            max_turns: None,
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");
        if let Some(obj) = v.get("vendor").and_then(|v| v.as_object()) {
            let keys: Vec<&String> = obj.keys().collect();
            let mut sorted = keys.clone();
            sorted.sort();
            prop_assert_eq!(keys, sorted);
        }
    }

    // ── Cross-cutting properties ────────────────────────────────────────

    #[test]
    fn openai_request_always_has_messages(req in arb_chat_completion_request()) {
        prop_assert!(!req.messages.is_empty());
    }

    #[test]
    fn claude_request_always_has_messages(req in arb_messages_request()) {
        prop_assert!(!req.messages.is_empty());
    }

    #[test]
    fn gemini_request_always_has_contents(req in arb_generate_content_request()) {
        prop_assert!(!req.contents.is_empty());
    }

    #[test]
    fn receipt_json_is_valid_json(receipt in arb_receipt()) {
        let json = serde_json::to_string(&receipt).expect("serialize");
        let parsed: serde_json::Result<serde_json::Value> = serde_json::from_str(&json);
        prop_assert!(parsed.is_ok());
    }

    #[test]
    fn work_order_json_is_valid_json(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).expect("serialize");
        let parsed: serde_json::Result<serde_json::Value> = serde_json::from_str(&json);
        prop_assert!(parsed.is_ok());
    }
}
