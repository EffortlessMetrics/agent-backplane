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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for IR translation roundtrips across all bridge crates.
//!
//! Verifies that dialect→IR→dialect roundtrips preserve semantics for:
//! OpenAI, Claude, Gemini, Codex, Copilot, Kimi.
//! Also tests cross-dialect translation, fuzz, invariants, and contract version.

use proptest::prelude::*;
use serde_json::json;
use std::collections::BTreeMap;

// ── Core IR (abp-core) ─────────────────────────────────────────────────
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};

// ── Dialect IR (abp-dialect) ────────────────────────────────────────────
use abp_dialect::ir::{
    IrContentBlock as DialectBlock, IrGenerationConfig, IrMessage as DialectMessage,
    IrRequest as DialectRequest, IrResponse as DialectResponse, IrRole as DialectRole,
    IrStopReason, IrToolDefinition as DialectToolDef, IrUsage as DialectUsage,
};

// ── SDK-types IR (abp-sdk-types) ────────────────────────────────────────
use abp_sdk_types::ir::{
    IrContentPart, IrMessage as SdkMessage, IrRole as SdkRole, IrToolCall, IrToolDefinition,
    IrUsage as SdkUsage,
};
use abp_sdk_types::ir_request::{IrChatRequest, IrSamplingParams, IrStreamConfig};
use abp_sdk_types::ir_response::{IrChatResponse, IrChoice, IrFinishReason};

// ── Bridge crate IR translators ─────────────────────────────────────────
use openai_bridge::ir_translate::{
    ir_to_openai_request, ir_to_openai_response, openai_request_to_ir, openai_response_to_ir,
};
use openai_bridge::openai_types::{
    ChatCompletionChoice, ChatCompletionRequest, ChatCompletionResponse, ChatMessage,
    ChatMessageRole, FunctionCall as OaiFunctionCall, FunctionDefinition as OaiFunctionDef,
    ToolCall as OaiToolCall, ToolDefinition as OaiToolDef, Usage as OaiUsage,
};

use claude_bridge::claude_types::{
    ContentBlock, Message as ClaudeMessage, MessageContent, MessagesRequest, MessagesResponse,
    Role as ClaudeRole, SystemMessage, Usage as ClaudeUsage,
};
use claude_bridge::ir_translate::{
    claude_request_to_ir, claude_response_to_ir, ir_to_claude_request, ir_to_claude_response,
};

use gemini_bridge::gemini_types::{
    Candidate, Content as GeminiContent, GenerateContentRequest, GenerateContentResponse, Part,
    UsageMetadata,
};
use gemini_bridge::ir_translate::{
    gemini_request_to_ir, gemini_response_to_ir, ir_to_gemini_request, ir_to_gemini_response,
};

use abp_codex_sdk::dialect::{
    CodexContentPart, CodexInputItem, CodexRequest, CodexResponse, CodexResponseItem, CodexTool,
    CodexUsage, ReasoningSummary,
};
use codex_bridge::ir_translate::{
    codex_request_to_ir, codex_response_to_ir, ir_to_codex_request, ir_to_codex_response,
};

use copilot_bridge::copilot_types::{
    CopilotChatChoice, CopilotChatRequest, CopilotChatResponse, CopilotMessage, CopilotMessageRole,
    CopilotUsage,
};
use copilot_bridge::ir_translate::{
    copilot_request_to_ir, copilot_response_to_ir, ir_to_copilot_request, ir_to_copilot_response,
};

use kimi_bridge::ir_translate::{
    ir_to_kimi_request, ir_to_kimi_response, kimi_request_to_ir, kimi_response_to_ir,
};
use kimi_bridge::kimi_types::{
    Choice as KimiChoice, KimiRequest, KimiResponse, Message as KimiMessage,
    ResponseMessage as KimiResponseMessage, Role as KimiRole, Usage as KimiUsage,
};

// ═══════════════════════════════════════════════════════════════════════
// Strategies
// ═══════════════════════════════════════════════════════════════════════

fn arb_ir_role() -> BoxedStrategy<IrRole> {
    prop_oneof![
        Just(IrRole::System),
        Just(IrRole::User),
        Just(IrRole::Assistant),
        Just(IrRole::Tool),
    ]
    .boxed()
}

fn arb_non_system_role() -> BoxedStrategy<IrRole> {
    prop_oneof![Just(IrRole::User), Just(IrRole::Assistant),].boxed()
}

fn arb_safe_text() -> BoxedStrategy<String> {
    "[a-zA-Z][a-zA-Z0-9 ]{0,99}".boxed()
}

fn arb_tool_name() -> BoxedStrategy<String> {
    "[a-z][a-z0-9_]{0,19}".boxed()
}

fn arb_tool_id() -> BoxedStrategy<String> {
    "[a-zA-Z][a-zA-Z0-9_]{4,15}".boxed()
}

fn arb_model_name() -> BoxedStrategy<String> {
    prop_oneof![
        Just("gpt-4o".into()),
        Just("claude-sonnet-4-20250514".into()),
        Just("gemini-2.5-flash".into()),
        Just("codex-mini-latest".into()),
        Just("moonshot-v1-8k".into()),
    ]
    .boxed()
}

fn arb_text_block() -> BoxedStrategy<IrContentBlock> {
    arb_safe_text()
        .prop_map(|text| IrContentBlock::Text { text })
        .boxed()
}

fn arb_thinking_block() -> BoxedStrategy<IrContentBlock> {
    arb_safe_text()
        .prop_map(|text| IrContentBlock::Thinking { text })
        .boxed()
}

fn arb_tool_use_block() -> BoxedStrategy<IrContentBlock> {
    (arb_tool_id(), arb_tool_name())
        .prop_map(|(id, name)| IrContentBlock::ToolUse {
            id,
            name,
            input: json!({"key": "value"}),
        })
        .boxed()
}

fn arb_tool_result_block() -> BoxedStrategy<IrContentBlock> {
    (arb_tool_id(), arb_safe_text(), any::<bool>())
        .prop_map(|(tool_use_id, text, is_error)| IrContentBlock::ToolResult {
            tool_use_id,
            content: vec![IrContentBlock::Text { text }],
            is_error,
        })
        .boxed()
}

fn arb_content_block() -> BoxedStrategy<IrContentBlock> {
    prop_oneof![
        arb_text_block(),
        arb_thinking_block(),
        arb_tool_use_block(),
        arb_tool_result_block(),
    ]
    .boxed()
}

fn arb_metadata() -> BoxedStrategy<BTreeMap<String, serde_json::Value>> {
    prop::collection::btree_map("[a-z]{1,10}", arb_safe_text().prop_map(|s| json!(s)), 0..=3)
        .boxed()
}

fn arb_ir_message() -> BoxedStrategy<IrMessage> {
    (
        arb_ir_role(),
        prop::collection::vec(arb_content_block(), 1..=4),
        arb_metadata(),
    )
        .prop_map(|(role, content, metadata)| IrMessage {
            role,
            content,
            metadata,
        })
        .boxed()
}

fn arb_text_message(role: BoxedStrategy<IrRole>) -> BoxedStrategy<IrMessage> {
    (role, arb_text_block())
        .prop_map(|(role, block)| IrMessage::new(role, vec![block]))
        .boxed()
}

fn arb_text_conversation(
    role: BoxedStrategy<IrRole>,
    max_msgs: usize,
) -> BoxedStrategy<IrConversation> {
    prop::collection::vec(arb_text_message(role), 1..=max_msgs)
        .prop_map(IrConversation::from_messages)
        .boxed()
}

fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    }
}

// ── Dialect IR strategies ───────────────────────────────────────────────

fn arb_dialect_role() -> BoxedStrategy<DialectRole> {
    prop_oneof![
        Just(DialectRole::System),
        Just(DialectRole::User),
        Just(DialectRole::Assistant),
        Just(DialectRole::Tool),
    ]
    .boxed()
}

fn arb_dialect_text_block() -> BoxedStrategy<DialectBlock> {
    arb_safe_text()
        .prop_map(|text| DialectBlock::Text { text })
        .boxed()
}

fn arb_dialect_tool_call_block() -> BoxedStrategy<DialectBlock> {
    (arb_tool_id(), arb_tool_name())
        .prop_map(|(id, name)| DialectBlock::ToolCall {
            id,
            name,
            input: json!({"key": "value"}),
        })
        .boxed()
}

fn arb_dialect_tool_result_block() -> BoxedStrategy<DialectBlock> {
    (arb_tool_id(), arb_safe_text())
        .prop_map(|(tool_call_id, text)| DialectBlock::ToolResult {
            tool_call_id,
            content: vec![DialectBlock::Text { text }],
            is_error: false,
        })
        .boxed()
}

fn arb_dialect_message() -> BoxedStrategy<DialectMessage> {
    (arb_dialect_role(), arb_safe_text())
        .prop_map(|(role, text)| DialectMessage::new(role, vec![DialectBlock::Text { text }]))
        .boxed()
}

fn arb_dialect_non_system_message() -> BoxedStrategy<DialectMessage> {
    (
        prop_oneof![Just(DialectRole::User), Just(DialectRole::Assistant)],
        arb_safe_text(),
    )
        .prop_map(|(role, text)| DialectMessage::new(role, vec![DialectBlock::Text { text }]))
        .boxed()
}

fn arb_dialect_tool_def() -> BoxedStrategy<DialectToolDef> {
    (arb_tool_name(), arb_safe_text())
        .prop_map(|(name, desc)| DialectToolDef {
            name,
            description: desc,
            parameters: json!({"type": "object", "properties": {}}),
        })
        .boxed()
}

fn arb_dialect_request() -> BoxedStrategy<DialectRequest> {
    (
        arb_model_name().prop_map(Some),
        proptest::option::of(arb_safe_text()),
        prop::collection::vec(arb_dialect_non_system_message(), 1..=5),
        prop::collection::vec(arb_dialect_tool_def(), 0..=2),
        proptest::option::of(0.0..=2.0f64),
        proptest::option::of(100u64..=4096u64),
    )
        .prop_map(
            |(model, system_prompt, messages, tools, temperature, max_tokens)| DialectRequest {
                model,
                system_prompt,
                messages,
                tools,
                config: IrGenerationConfig {
                    max_tokens,
                    temperature,
                    top_p: None,
                    top_k: None,
                    stop_sequences: Vec::new(),
                    extra: BTreeMap::new(),
                },
                metadata: BTreeMap::new(),
            },
        )
        .boxed()
}

fn arb_dialect_response() -> BoxedStrategy<DialectResponse> {
    (
        arb_safe_text(),
        arb_model_name(),
        prop::collection::vec(arb_dialect_text_block(), 1..=3),
        proptest::option::of(prop_oneof![
            Just(IrStopReason::EndTurn),
            Just(IrStopReason::MaxTokens),
            Just(IrStopReason::ToolUse),
        ]),
        (1u64..=1000u64, 1u64..=1000u64),
    )
        .prop_map(
            |(id, model, content, stop_reason, (inp, out))| DialectResponse {
                id: Some(id),
                model: Some(model),
                content,
                stop_reason,
                usage: Some(DialectUsage {
                    input_tokens: inp,
                    output_tokens: out,
                    total_tokens: inp + out,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                }),
                metadata: BTreeMap::new(),
            },
        )
        .boxed()
}

// ── SDK-types IR strategies ─────────────────────────────────────────────

fn arb_sdk_role() -> BoxedStrategy<SdkRole> {
    prop_oneof![
        Just(SdkRole::System),
        Just(SdkRole::User),
        Just(SdkRole::Assistant),
        Just(SdkRole::Tool),
    ]
    .boxed()
}

fn arb_sdk_non_system_role() -> BoxedStrategy<SdkRole> {
    prop_oneof![Just(SdkRole::User), Just(SdkRole::Assistant)].boxed()
}

fn arb_sdk_text_message() -> BoxedStrategy<SdkMessage> {
    (arb_sdk_non_system_role(), arb_safe_text())
        .prop_map(|(role, text)| SdkMessage {
            role,
            content: vec![IrContentPart::Text { text }],
            tool_calls: Vec::new(),
            metadata: BTreeMap::new(),
        })
        .boxed()
}

fn arb_sdk_tool_def() -> BoxedStrategy<IrToolDefinition> {
    (arb_tool_name(), arb_safe_text())
        .prop_map(|(name, desc)| IrToolDefinition {
            name,
            description: desc,
            parameters: json!({"type": "object", "properties": {}}),
        })
        .boxed()
}

fn arb_ir_chat_request() -> BoxedStrategy<IrChatRequest> {
    (
        arb_model_name(),
        prop::collection::vec(arb_sdk_text_message(), 1..=5),
        prop::collection::vec(arb_sdk_tool_def(), 0..=2),
        proptest::option::of(0.0..=2.0f64),
        proptest::option::of(100u64..=4096u64),
    )
        .prop_map(
            |(model, messages, tools, temperature, max_tokens)| IrChatRequest {
                model,
                messages,
                max_tokens,
                tools,
                tool_choice: None,
                sampling: IrSamplingParams {
                    temperature,
                    top_p: None,
                    top_k: None,
                    frequency_penalty: None,
                    presence_penalty: None,
                },
                stop_sequences: Vec::new(),
                stream: IrStreamConfig::default(),
                response_format: None,
                extra: BTreeMap::new(),
            },
        )
        .boxed()
}

fn arb_ir_chat_response() -> BoxedStrategy<IrChatResponse> {
    (
        arb_safe_text(),
        arb_model_name(),
        arb_safe_text(),
        proptest::option::of(prop_oneof![
            Just(IrFinishReason::Stop),
            Just(IrFinishReason::Length),
            Just(IrFinishReason::ToolUse),
        ]),
        (1u64..=1000u64, 1u64..=1000u64),
    )
        .prop_map(
            |(id, model, text, finish_reason, (prompt, compl))| IrChatResponse {
                id: Some(id),
                model: Some(model),
                choices: vec![IrChoice {
                    index: 0,
                    message: SdkMessage {
                        role: SdkRole::Assistant,
                        content: vec![IrContentPart::Text { text }],
                        tool_calls: Vec::new(),
                        metadata: BTreeMap::new(),
                    },
                    finish_reason,
                }],
                usage: Some(SdkUsage::from_counts(prompt, compl)),
                metadata: BTreeMap::new(),
            },
        )
        .boxed()
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn serde_roundtrip<T: serde::Serialize + serde::de::DeserializeOwned>(val: &T) -> T {
    let json = serde_json::to_string(val).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

fn text_contents(conv: &IrConversation) -> Vec<String> {
    conv.messages.iter().map(|m| m.text_content()).collect()
}

fn roles(conv: &IrConversation) -> Vec<IrRole> {
    conv.messages.iter().map(|m| m.role).collect()
}

fn dialect_text_contents(msgs: &[DialectMessage]) -> Vec<String> {
    msgs.iter().map(|m| m.text_content()).collect()
}

fn sdk_text_contents(msgs: &[SdkMessage]) -> Vec<String> {
    msgs.iter().map(|m| m.text_content()).collect()
}

// ═══════════════════════════════════════════════════════════════════════
// §1  Core IR serde roundtrip (4 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t01_ir_message_serde_roundtrip(msg in arb_ir_message()) {
        let rt = serde_roundtrip(&msg);
        prop_assert_eq!(msg, rt);
    }

    #[test]
    fn t02_ir_role_preserved(role in arb_ir_role()) {
        let rt: IrRole = serde_roundtrip(&role);
        prop_assert_eq!(role, rt);
    }

    #[test]
    fn t03_ir_content_blocks_preserved(blocks in prop::collection::vec(arb_content_block(), 1..=6)) {
        let rt: Vec<IrContentBlock> = serde_roundtrip(&blocks);
        prop_assert_eq!(blocks, rt);
    }

    #[test]
    fn t04_ir_metadata_preserved(
        role in arb_ir_role(),
        text in arb_safe_text(),
        metadata in arb_metadata(),
    ) {
        let msg = IrMessage {
            role,
            content: vec![IrContentBlock::Text { text }],
            metadata: metadata.clone(),
        };
        let rt = serde_roundtrip(&msg);
        prop_assert_eq!(msg.metadata, rt.metadata);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2  IrConversation roundtrip (3 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t05_ir_conversation_roundtrip(
        msgs in prop::collection::vec(arb_text_message(arb_ir_role()), 1..=20)
    ) {
        let conv = IrConversation::from_messages(msgs);
        let rt = serde_roundtrip(&conv);
        prop_assert_eq!(conv, rt);
    }

    #[test]
    fn t06_ir_system_message_position_preserved(
        pre in prop::collection::vec(arb_text_message(arb_non_system_role()), 0..=3),
        sys_text in arb_safe_text(),
        post in prop::collection::vec(arb_text_message(arb_non_system_role()), 1..=5),
    ) {
        let mut msgs = pre;
        msgs.push(IrMessage::text(IrRole::System, &sys_text));
        msgs.extend(post);
        let conv = IrConversation::from_messages(msgs);
        let rt = serde_roundtrip(&conv);
        let orig_pos = conv.messages.iter().position(|m| m.role == IrRole::System);
        let rt_pos = rt.messages.iter().position(|m| m.role == IrRole::System);
        prop_assert_eq!(orig_pos, rt_pos);
    }

    #[test]
    fn t07_ir_tool_call_result_pairing(
        tool_id in arb_tool_id(),
        tool_name in arb_tool_name(),
        result_text in arb_safe_text(),
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(IrRole::Assistant, vec![IrContentBlock::ToolUse {
                id: tool_id.clone(),
                name: tool_name.clone(),
                input: json!({"arg": 1}),
            }]),
            IrMessage::new(IrRole::Tool, vec![IrContentBlock::ToolResult {
                tool_use_id: tool_id.clone(),
                content: vec![IrContentBlock::Text { text: result_text }],
                is_error: false,
            }]),
        ]);
        let rt = serde_roundtrip(&conv);
        let use_id = match &rt.messages[0].content[0] {
            IrContentBlock::ToolUse { id, .. } => id.clone(),
            other => panic!("expected ToolUse, got {other:?}"),
        };
        let result_ref = match &rt.messages[1].content[0] {
            IrContentBlock::ToolResult { tool_use_id, .. } => tool_use_id.clone(),
            other => panic!("expected ToolResult, got {other:?}"),
        };
        prop_assert_eq!(&use_id, &tool_id);
        prop_assert_eq!(use_id, result_ref);
    }
}

#[test]
fn t08_ir_empty_conversation_roundtrip() {
    let conv = IrConversation::new();
    let rt = serde_roundtrip(&conv);
    assert_eq!(conv, rt);
    assert!(rt.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// §3  OpenAI→IR→OpenAI roundtrip (3 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t09_openai_request_roundtrip(ir_req in arb_ir_chat_request()) {
        let oai = ir_to_openai_request(&ir_req);
        let rt = openai_request_to_ir(&oai);
        prop_assert_eq!(ir_req.model, rt.model);
        prop_assert_eq!(ir_req.messages.len(), rt.messages.len());
        prop_assert_eq!(sdk_text_contents(&ir_req.messages), sdk_text_contents(&rt.messages));
        prop_assert_eq!(ir_req.tools.len(), rt.tools.len());
        prop_assert_eq!(ir_req.sampling.temperature, rt.sampling.temperature);
    }

    #[test]
    fn t10_openai_response_roundtrip(ir_resp in arb_ir_chat_response()) {
        let oai = ir_to_openai_response(&ir_resp);
        let rt = openai_response_to_ir(&oai);
        prop_assert_eq!(ir_resp.choices.len(), rt.choices.len());
        for (orig, round) in ir_resp.choices.iter().zip(rt.choices.iter()) {
            prop_assert_eq!(orig.message.text_content(), round.message.text_content());
        }
    }

    #[test]
    fn t11_openai_tool_defs_preserved(
        tools in prop::collection::vec(arb_sdk_tool_def(), 1..=4),
    ) {
        let ir_req = IrChatRequest {
            model: "gpt-4o".into(),
            messages: vec![SdkMessage::text(SdkRole::User, "hi")],
            max_tokens: None,
            tools: tools.clone(),
            tool_choice: None,
            sampling: IrSamplingParams::default(),
            stop_sequences: Vec::new(),
            stream: IrStreamConfig::default(),
            response_format: None,
            extra: BTreeMap::new(),
        };
        let oai = ir_to_openai_request(&ir_req);
        let rt = openai_request_to_ir(&oai);
        prop_assert_eq!(tools.len(), rt.tools.len());
        for (orig, round) in tools.iter().zip(rt.tools.iter()) {
            prop_assert_eq!(&orig.name, &round.name);
            prop_assert_eq!(&orig.description, &round.description);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4  Claude→IR→Claude roundtrip (3 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t12_claude_request_roundtrip(ir_req in arb_dialect_request()) {
        let claude = ir_to_claude_request(&ir_req);
        let rt = claude_request_to_ir(&claude);
        prop_assert_eq!(ir_req.model, rt.model);
        prop_assert_eq!(ir_req.system_prompt, rt.system_prompt);
        prop_assert_eq!(ir_req.messages.len(), rt.messages.len());
        prop_assert_eq!(
            dialect_text_contents(&ir_req.messages),
            dialect_text_contents(&rt.messages)
        );
    }

    #[test]
    fn t13_claude_response_roundtrip(ir_resp in arb_dialect_response()) {
        let claude = ir_to_claude_response(&ir_resp);
        let rt = claude_response_to_ir(&claude);
        let orig_text: String = ir_resp.content.iter().filter_map(|b| b.as_text()).collect::<Vec<_>>().join("");
        let rt_text: String = rt.content.iter().filter_map(|b| b.as_text()).collect::<Vec<_>>().join("");
        prop_assert_eq!(orig_text, rt_text);
        prop_assert_eq!(ir_resp.content.len(), rt.content.len());
    }

    #[test]
    fn t14_claude_tools_preserved(
        tools in prop::collection::vec(arb_dialect_tool_def(), 1..=4),
    ) {
        let ir_req = DialectRequest {
            model: Some("claude-sonnet-4-20250514".into()),
            system_prompt: None,
            messages: vec![DialectMessage::text(DialectRole::User, "hi")],
            tools: tools.clone(),
            config: IrGenerationConfig { max_tokens: Some(1024), ..Default::default() },
            metadata: BTreeMap::new(),
        };
        let claude = ir_to_claude_request(&ir_req);
        let rt = claude_request_to_ir(&claude);
        prop_assert_eq!(tools.len(), rt.tools.len());
        for (orig, round) in tools.iter().zip(rt.tools.iter()) {
            prop_assert_eq!(&orig.name, &round.name);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §5  Gemini→IR→Gemini roundtrip (3 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t15_gemini_request_roundtrip(ir_req in arb_ir_chat_request()) {
        let gemini = ir_to_gemini_request(&ir_req);
        let rt = gemini_request_to_ir(&gemini);
        // Gemini prepends system instruction as a System message so count may differ
        prop_assert_eq!(sdk_text_contents(&ir_req.messages), sdk_text_contents(&rt.messages));
    }

    #[test]
    fn t16_gemini_response_roundtrip(ir_resp in arb_ir_chat_response()) {
        let gemini = ir_to_gemini_response(&ir_resp);
        let rt = gemini_response_to_ir(&gemini);
        prop_assert_eq!(ir_resp.choices.len(), rt.choices.len());
        for (orig, round) in ir_resp.choices.iter().zip(rt.choices.iter()) {
            prop_assert_eq!(orig.message.text_content(), round.message.text_content());
        }
    }

    #[test]
    fn t17_gemini_model_preserved(model in arb_model_name()) {
        let ir_req = IrChatRequest {
            model: model.clone(),
            messages: vec![SdkMessage::text(SdkRole::User, "test")],
            max_tokens: None,
            tools: Vec::new(),
            tool_choice: None,
            sampling: IrSamplingParams::default(),
            stop_sequences: Vec::new(),
            stream: IrStreamConfig::default(),
            response_format: None,
            extra: BTreeMap::new(),
        };
        let gemini = ir_to_gemini_request(&ir_req);
        let rt = gemini_request_to_ir(&gemini);
        prop_assert_eq!(model, rt.model);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §6  Codex→IR→Codex roundtrip (3 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t18_codex_request_roundtrip(ir_req in arb_dialect_request()) {
        let codex = ir_to_codex_request(&ir_req);
        let rt = codex_request_to_ir(&codex);
        prop_assert_eq!(ir_req.model, rt.model);
        // Codex flattens system prompt into input items, count may differ
        // but user messages should survive
        let orig_user: Vec<_> = ir_req.messages.iter()
            .filter(|m| m.role == DialectRole::User)
            .map(|m| m.text_content())
            .collect();
        let rt_user: Vec<_> = rt.messages.iter()
            .filter(|m| m.role == DialectRole::User)
            .map(|m| m.text_content())
            .collect();
        prop_assert_eq!(orig_user, rt_user);
    }

    #[test]
    fn t19_codex_response_roundtrip(ir_resp in arb_dialect_response()) {
        let codex = ir_to_codex_response(&ir_resp);
        let rt = codex_response_to_ir(&codex);
        let orig_text: String = ir_resp.content.iter().filter_map(|b| b.as_text()).collect::<Vec<_>>().join("");
        let rt_text: String = rt.content.iter().filter_map(|b| b.as_text()).collect::<Vec<_>>().join("");
        prop_assert_eq!(orig_text, rt_text);
    }

    #[test]
    fn t20_codex_system_prompt_preserved(
        sys in arb_safe_text(),
        user in arb_safe_text(),
    ) {
        let ir_req = DialectRequest {
            model: Some("codex-mini-latest".into()),
            system_prompt: Some(sys.clone()),
            messages: vec![DialectMessage::text(DialectRole::User, &user)],
            tools: Vec::new(),
            config: IrGenerationConfig::default(),
            metadata: BTreeMap::new(),
        };
        let codex = ir_to_codex_request(&ir_req);
        let rt = codex_request_to_ir(&codex);
        prop_assert_eq!(Some(sys), rt.system_prompt);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7  Copilot→IR→Copilot roundtrip (3 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t21_copilot_request_roundtrip(ir_req in arb_ir_chat_request()) {
        let copilot = ir_to_copilot_request(&ir_req);
        let rt = copilot_request_to_ir(&copilot);
        prop_assert_eq!(ir_req.model, rt.model);
        prop_assert_eq!(ir_req.messages.len(), rt.messages.len());
        prop_assert_eq!(sdk_text_contents(&ir_req.messages), sdk_text_contents(&rt.messages));
    }

    #[test]
    fn t22_copilot_response_roundtrip(ir_resp in arb_ir_chat_response()) {
        let copilot = ir_to_copilot_response(&ir_resp);
        let rt = copilot_response_to_ir(&copilot);
        prop_assert_eq!(ir_resp.choices.len(), rt.choices.len());
        for (orig, round) in ir_resp.choices.iter().zip(rt.choices.iter()) {
            prop_assert_eq!(orig.message.text_content(), round.message.text_content());
        }
    }

    #[test]
    fn t23_copilot_tools_preserved(
        tools in prop::collection::vec(arb_sdk_tool_def(), 1..=4),
    ) {
        let ir_req = IrChatRequest {
            model: "gpt-4o".into(),
            messages: vec![SdkMessage::text(SdkRole::User, "hi")],
            max_tokens: None,
            tools: tools.clone(),
            tool_choice: None,
            sampling: IrSamplingParams::default(),
            stop_sequences: Vec::new(),
            stream: IrStreamConfig::default(),
            response_format: None,
            extra: BTreeMap::new(),
        };
        let copilot = ir_to_copilot_request(&ir_req);
        let rt = copilot_request_to_ir(&copilot);
        prop_assert_eq!(tools.len(), rt.tools.len());
        for (orig, round) in tools.iter().zip(rt.tools.iter()) {
            prop_assert_eq!(&orig.name, &round.name);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §8  Kimi→IR→Kimi roundtrip (3 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t24_kimi_request_roundtrip(ir_req in arb_dialect_request()) {
        let kimi = ir_to_kimi_request(&ir_req);
        let rt = kimi_request_to_ir(&kimi);
        prop_assert_eq!(ir_req.model, rt.model);
        // Kimi embeds system prompt as a system message
        let orig_non_sys: Vec<_> = ir_req.messages.iter()
            .map(|m| m.text_content())
            .collect();
        let rt_non_sys: Vec<_> = rt.messages.iter()
            .filter(|m| m.role != DialectRole::System)
            .map(|m| m.text_content())
            .collect();
        prop_assert_eq!(orig_non_sys, rt_non_sys);
    }

    #[test]
    fn t25_kimi_response_roundtrip(ir_resp in arb_dialect_response()) {
        let kimi = ir_to_kimi_response(&ir_resp);
        let rt = kimi_response_to_ir(&kimi);
        let orig_text: String = ir_resp.content.iter().filter_map(|b| b.as_text()).collect::<Vec<_>>().join("");
        let rt_text: String = rt.content.iter().filter_map(|b| b.as_text()).collect::<Vec<_>>().join("");
        prop_assert_eq!(orig_text, rt_text);
    }

    #[test]
    fn t26_kimi_system_prompt_preserved(
        sys in arb_safe_text(),
        user in arb_safe_text(),
    ) {
        let ir_req = DialectRequest {
            model: Some("moonshot-v1-8k".into()),
            system_prompt: Some(sys.clone()),
            messages: vec![DialectMessage::text(DialectRole::User, &user)],
            tools: Vec::new(),
            config: IrGenerationConfig::default(),
            metadata: BTreeMap::new(),
        };
        let kimi = ir_to_kimi_request(&ir_req);
        // System prompt should appear as the first message with system role
        prop_assert!(kimi.messages.len() >= 2);
        prop_assert_eq!(kimi.messages[0].role, KimiRole::System);
        prop_assert_eq!(kimi.messages[0].content.as_deref(), Some(sys.as_str()));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §9  Cross-dialect: OpenAI→IR→Claude→IR→OpenAI (2 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t27_cross_dialect_openai_claude_openai_text_preserved(
        text in arb_safe_text(),
    ) {
        // Build an OpenAI request with a single user message
        let oai_req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage {
                role: ChatMessageRole::User,
                content: Some(text.clone()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: None,
            temperature: None,
            max_tokens: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            tool_choice: None,
        };
        // OpenAI → SDK IR
        let sdk_ir = openai_request_to_ir(&oai_req);
        // Extract text from SDK IR and build dialect IR for Claude
        let user_text = sdk_ir.messages[0].text_content();
        let dialect_ir = DialectRequest {
            model: Some("claude-sonnet-4-20250514".into()),
            system_prompt: None,
            messages: vec![DialectMessage::text(DialectRole::User, &user_text)],
            tools: Vec::new(),
            config: IrGenerationConfig { max_tokens: Some(1024), ..Default::default() },
            metadata: BTreeMap::new(),
        };
        // Dialect IR → Claude → Dialect IR
        let claude_req = ir_to_claude_request(&dialect_ir);
        let rt_dialect = claude_request_to_ir(&claude_req);
        // Back to SDK IR → OpenAI
        let rt_text = rt_dialect.messages[0].text_content();
        let sdk_ir2 = IrChatRequest {
            model: "gpt-4o".into(),
            messages: vec![SdkMessage::text(SdkRole::User, &rt_text)],
            max_tokens: None,
            tools: Vec::new(),
            tool_choice: None,
            sampling: IrSamplingParams::default(),
            stop_sequences: Vec::new(),
            stream: IrStreamConfig::default(),
            response_format: None,
            extra: BTreeMap::new(),
        };
        let oai_rt = ir_to_openai_request(&sdk_ir2);
        prop_assert_eq!(oai_req.messages[0].content.as_deref(), oai_rt.messages[0].content.as_deref());
    }

    #[test]
    fn t28_cross_dialect_gemini_codex_gemini_text_preserved(
        text in arb_safe_text(),
    ) {
        // Gemini request
        let gemini_req = GenerateContentRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent::user(vec![Part::text(&text)])],
            tools: None,
            generation_config: None,
            safety_settings: None,
            system_instruction: None,
        };
        let sdk_ir = gemini_request_to_ir(&gemini_req);
        let user_text = sdk_ir.messages[0].text_content();
        // Through Codex dialect IR
        let dialect_ir = DialectRequest {
            model: Some("codex-mini-latest".into()),
            system_prompt: None,
            messages: vec![DialectMessage::text(DialectRole::User, &user_text)],
            tools: Vec::new(),
            config: IrGenerationConfig::default(),
            metadata: BTreeMap::new(),
        };
        let codex_req = ir_to_codex_request(&dialect_ir);
        let rt_dialect = codex_request_to_ir(&codex_req);
        let rt_text = rt_dialect.messages[0].text_content();
        prop_assert_eq!(text, rt_text);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §10  Cross-SDK legacy roundtrip (4 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t29_legacy_openai_user_messages_preserved(
        conv in arb_text_conversation(Just(IrRole::User).boxed(), 8)
    ) {
        let sdk = abp_openai_sdk::lowering::from_ir(&conv);
        let rt = abp_openai_sdk::lowering::to_ir(&sdk);
        prop_assert_eq!(text_contents(&conv), text_contents(&rt));
        prop_assert_eq!(roles(&conv), roles(&rt));
    }

    #[test]
    fn t30_legacy_claude_assistant_messages_preserved(
        conv in arb_text_conversation(Just(IrRole::Assistant).boxed(), 8)
    ) {
        let sdk = abp_claude_sdk::lowering::from_ir(&conv);
        let rt = abp_claude_sdk::lowering::to_ir(&sdk, None);
        prop_assert_eq!(text_contents(&conv), text_contents(&rt));
    }

    #[test]
    fn t31_legacy_gemini_multi_turn_preserved(
        conv in arb_text_conversation(arb_non_system_role(), 10)
    ) {
        let sdk = abp_gemini_sdk::lowering::from_ir(&conv);
        let rt = abp_gemini_sdk::lowering::to_ir(&sdk, None);
        prop_assert_eq!(text_contents(&conv), text_contents(&rt));
        prop_assert_eq!(conv.len(), rt.len());
    }

    #[test]
    fn t32_legacy_system_message_handled(
        sys_text in arb_safe_text(),
        user_text in arb_safe_text(),
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, &sys_text),
            IrMessage::text(IrRole::User, &user_text),
        ]);
        let oai = abp_openai_sdk::lowering::from_ir(&conv);
        let oai_rt = abp_openai_sdk::lowering::to_ir(&oai);
        prop_assert_eq!(oai_rt.system_message().map(|m| m.text_content()), Some(sys_text.clone()));

        let sys_prompt = abp_claude_sdk::lowering::extract_system_prompt(&conv);
        prop_assert_eq!(sys_prompt.as_deref(), Some(sys_text.as_str()));

        let kimi = abp_kimi_sdk::lowering::from_ir(&conv);
        let kimi_rt = abp_kimi_sdk::lowering::to_ir(&kimi);
        prop_assert_eq!(kimi_rt.system_message().map(|m| m.text_content()), Some(sys_text));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §11  Fuzz: random IR always produces valid output (6 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t33_fuzz_ir_to_openai_always_valid(ir_req in arb_ir_chat_request()) {
        let oai = ir_to_openai_request(&ir_req);
        let json = serde_json::to_string(&oai).unwrap();
        let _: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn t34_fuzz_ir_to_claude_always_valid(ir_req in arb_dialect_request()) {
        let claude = ir_to_claude_request(&ir_req);
        let json = serde_json::to_string(&claude).unwrap();
        let _: MessagesRequest = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn t35_fuzz_ir_to_gemini_always_valid(ir_req in arb_ir_chat_request()) {
        let gemini = ir_to_gemini_request(&ir_req);
        let json = serde_json::to_string(&gemini).unwrap();
        let _: GenerateContentRequest = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn t36_fuzz_ir_to_codex_always_valid(ir_req in arb_dialect_request()) {
        let codex = ir_to_codex_request(&ir_req);
        let json = serde_json::to_string(&codex).unwrap();
        let _: CodexRequest = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn t37_fuzz_ir_to_copilot_always_valid(ir_req in arb_ir_chat_request()) {
        let copilot = ir_to_copilot_request(&ir_req);
        let json = serde_json::to_string(&copilot).unwrap();
        let _: CopilotChatRequest = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn t38_fuzz_ir_to_kimi_always_valid(ir_req in arb_dialect_request()) {
        let kimi = ir_to_kimi_request(&ir_req);
        let json = serde_json::to_string(&kimi).unwrap();
        let _: KimiRequest = serde_json::from_str(&json).unwrap();
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §12  Invariants (5 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t39_invariant_message_count_preserved(
        conv in arb_text_conversation(arb_ir_role(), 15)
    ) {
        let rt = serde_roundtrip(&conv);
        prop_assert_eq!(conv.len(), rt.len());
        let oai = abp_openai_sdk::lowering::from_ir(&conv);
        let oai_rt = abp_openai_sdk::lowering::to_ir(&oai);
        prop_assert_eq!(conv.len(), oai_rt.len());
    }

    #[test]
    fn t40_invariant_role_ordering_preserved(
        conv in arb_text_conversation(arb_ir_role(), 12)
    ) {
        let rt = serde_roundtrip(&conv);
        prop_assert_eq!(roles(&conv), roles(&rt));
        let oai = abp_openai_sdk::lowering::from_ir(&conv);
        let oai_rt = abp_openai_sdk::lowering::to_ir(&oai);
        prop_assert_eq!(roles(&conv), roles(&oai_rt));
    }

    #[test]
    fn t41_invariant_content_type_preserved(
        blocks in prop::collection::vec(arb_content_block(), 1..=6)
    ) {
        let msg = IrMessage::new(IrRole::Assistant, blocks);
        let rt = serde_roundtrip(&msg);
        prop_assert_eq!(msg.content.len(), rt.content.len());
        for (orig, round) in msg.content.iter().zip(rt.content.iter()) {
            prop_assert_eq!(std::mem::discriminant(orig), std::mem::discriminant(round));
        }
    }

    #[test]
    fn t42_invariant_canonical_json_stable(ir_req in arb_dialect_request()) {
        let json1 = serde_json::to_string(&ir_req).unwrap();
        let json2 = serde_json::to_string(&ir_req).unwrap();
        prop_assert_eq!(json1, json2);
    }

    #[test]
    fn t43_invariant_dialect_ir_serde_roundtrip(ir_req in arb_dialect_request()) {
        let rt: DialectRequest = serde_roundtrip(&ir_req);
        prop_assert_eq!(ir_req.model, rt.model);
        prop_assert_eq!(ir_req.system_prompt, rt.system_prompt);
        prop_assert_eq!(ir_req.messages.len(), rt.messages.len());
        prop_assert_eq!(ir_req.tools.len(), rt.tools.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §13  Contract version & receipt hashing (2 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t44_contract_version_preserved() {
    let version = abp_core::CONTRACT_VERSION;
    assert_eq!(version, "abp/v0.1");
}

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn t45_receipt_hash_deterministic(
        input in (1u64..=1000u64, 1u64..=1000u64),
    ) {
        use abp_core::ir::IrUsage;
        let usage = IrUsage::from_io(input.0, input.1);
        // Hashing the same usage twice produces the same JSON
        let json1 = serde_json::to_string(&usage).unwrap();
        let json2 = serde_json::to_string(&usage).unwrap();
        prop_assert_eq!(json1, json2);
    }
}
