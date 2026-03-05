// SPDX-License-Identifier: MIT OR Apache-2.0

//! Integration tests for the enhanced mapper: capabilities, emulation,
//! validation, and capability-aware error handling.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapper::capabilities::{Support, check_feature_support, dialect_capabilities};
use abp_mapper::emulation::{
    emulate_images_as_placeholder, emulate_system_as_user, emulate_thinking_as_text,
    strip_thinking, tool_results_to_user_role, user_tool_results_to_tool_role,
};
use abp_mapper::validate_ir::{
    validate_for_target, validate_ir_for_mapping, validate_ir_structure,
};
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, CodexClaudeIrMapper, GeminiKimiIrMapper, IrMapper,
    MapError, OpenAiClaudeIrMapper, OpenAiCodexIrMapper, OpenAiCopilotIrMapper,
    OpenAiGeminiIrMapper, OpenAiKimiIrMapper,
};
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn system_user_assistant() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
    ])
}

fn with_thinking() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Solve it"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Step by step...".into(),
                },
                IrContentBlock::Text {
                    text: "The answer is 42.".into(),
                },
            ],
        ),
    ])
}

fn with_image() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is this?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64==".into(),
            },
        ],
    )])
}

fn with_tool_call() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Weather?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "get_weather".into(),
                input: json!({"city": "NYC"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "72°F".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

fn system_with_image() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::System,
        vec![
            IrContentBlock::Text {
                text: "You are a visual assistant.".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64==".into(),
            },
        ],
    )])
}

// ═══════════════════════════════════════════════════════════════════════
// Capability-aware mapping tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn capabilities_all_dialects_defined() {
    for &d in Dialect::all() {
        let caps = dialect_capabilities(d);
        assert_eq!(caps.dialect, d);
    }
}

#[test]
fn capabilities_support_enum_coverage() {
    assert!(Support::Native.is_native());
    assert!(!Support::None.is_native());
}

#[test]
fn check_feature_thinking_on_openai() {
    let caps = dialect_capabilities(Dialect::OpenAi);
    let reason = check_feature_support("thinking", &caps);
    assert!(reason.is_some());
}

#[test]
fn check_feature_images_on_claude() {
    let caps = dialect_capabilities(Dialect::Claude);
    assert!(check_feature_support("images", &caps).is_none());
}

#[test]
fn check_feature_images_on_codex() {
    let caps = dialect_capabilities(Dialect::Codex);
    assert!(check_feature_support("images", &caps).is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// Image-rejection for dialects that don't support images
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_kimi_rejects_images() {
    let mapper = OpenAiKimiIrMapper;
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Kimi, &with_image());
    assert!(matches!(result, Err(MapError::UnmappableContent { .. })));
}

#[test]
fn claude_to_kimi_rejects_images() {
    let mapper = ClaudeKimiIrMapper;
    let result = mapper.map_request(Dialect::Claude, Dialect::Kimi, &with_image());
    assert!(matches!(result, Err(MapError::UnmappableContent { .. })));
}

#[test]
fn gemini_to_kimi_rejects_images() {
    let mapper = GeminiKimiIrMapper;
    let result = mapper.map_request(Dialect::Gemini, Dialect::Kimi, &with_image());
    assert!(matches!(result, Err(MapError::UnmappableContent { .. })));
}

#[test]
fn openai_to_copilot_rejects_images() {
    let mapper = OpenAiCopilotIrMapper;
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Copilot, &with_image());
    assert!(matches!(result, Err(MapError::UnmappableContent { .. })));
}

#[test]
fn openai_to_claude_rejects_system_images() {
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &system_with_image());
    assert!(matches!(result, Err(MapError::UnmappableContent { .. })));
}

#[test]
fn claude_to_gemini_rejects_system_images() {
    let mapper = ClaudeGeminiIrMapper;
    let result = mapper.map_request(Dialect::Claude, Dialect::Gemini, &system_with_image());
    assert!(matches!(result, Err(MapError::UnmappableContent { .. })));
}

// Images are fine for dialects that support them
#[test]
fn openai_to_claude_preserves_images() {
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &with_image())
        .unwrap();
    assert!(
        result.messages[0]
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Image { .. }))
    );
}

#[test]
fn openai_to_gemini_preserves_images() {
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &with_image())
        .unwrap();
    assert!(
        result.messages[0]
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Image { .. }))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Codex emulation tests (system + images)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_codex_emulates_system_as_user() {
    let mapper = OpenAiCodexIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &system_user_assistant())
        .unwrap();
    // System is emulated as [System]-prefixed user message
    assert_eq!(result.messages[0].role, IrRole::User);
    assert!(result.messages[0].text_content().starts_with("[System]"));
    assert!(
        result.messages[0]
            .text_content()
            .contains("You are helpful.")
    );
}

#[test]
fn openai_to_codex_emulates_images_as_placeholders() {
    let mapper = OpenAiCodexIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &with_image())
        .unwrap();
    assert_eq!(result.len(), 1);
    let text = result.messages[0].text_content();
    assert!(text.contains("[Image: image/png]"));
    assert!(text.contains("What is this?") || result.messages[0].content.len() == 2);
}

#[test]
fn claude_to_codex_emulates_system() {
    let mapper = CodexClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Codex, &system_user_assistant())
        .unwrap();
    assert_eq!(result.messages[0].role, IrRole::User);
    assert!(result.messages[0].text_content().starts_with("[System]"));
}

#[test]
fn codex_to_openai_lossless() {
    let mapper = OpenAiCodexIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::text(IrRole::Assistant, "hi"),
    ]);
    let result = mapper
        .map_request(Dialect::Codex, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result, conv);
}

// ═══════════════════════════════════════════════════════════════════════
// Emulation strategy unit tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn emulate_thinking_preserves_content() {
    let result = emulate_thinking_as_text(&with_thinking());
    assert_eq!(result.conversation.messages[1].content.len(), 2);
    assert!(
        result.conversation.messages[1]
            .text_content()
            .contains("[Thinking]")
    );
    assert!(
        result.conversation.messages[1]
            .text_content()
            .contains("The answer is 42.")
    );
}

#[test]
fn strip_thinking_removes_block() {
    let result = strip_thinking(&with_thinking());
    let asst = &result.conversation.messages[1];
    assert_eq!(asst.content.len(), 1);
    assert_eq!(asst.text_content(), "The answer is 42.");
    assert_eq!(result.notes.len(), 1);
}

#[test]
fn emulate_system_as_user_converts_role() {
    let result = emulate_system_as_user(&system_user_assistant());
    assert_eq!(result.conversation.messages[0].role, IrRole::User);
    assert!(
        result.conversation.messages[0]
            .text_content()
            .contains("[System]")
    );
    assert_eq!(result.notes.len(), 1);
}

#[test]
fn emulate_images_replaces_with_placeholder() {
    let result = emulate_images_as_placeholder(&with_image());
    let msg = &result.conversation.messages[0];
    assert!(msg.text_content().contains("[Image: image/png]"));
    assert!(
        !msg.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Image { .. }))
    );
}

#[test]
fn tool_results_to_user_converts_role() {
    let result = tool_results_to_user_role(&with_tool_call());
    let tool_msgs: Vec<_> = result
        .conversation
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert!(tool_msgs.is_empty());
    // The original Tool message should now be User
    let user_with_result = result
        .conversation
        .messages
        .iter()
        .filter(|m| {
            m.role == IrRole::User
                && m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        })
        .count();
    assert_eq!(user_with_result, 1);
}

#[test]
fn user_tool_results_split_to_tool_role() {
    // Create conversation with User message containing ToolResult blocks
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "done".into(),
                }],
                is_error: false,
            },
            IrContentBlock::ToolResult {
                tool_use_id: "t2".into(),
                content: vec![IrContentBlock::Text {
                    text: "also done".into(),
                }],
                is_error: false,
            },
        ],
    )]);
    let result = user_tool_results_to_tool_role(&conv);
    assert_eq!(result.conversation.messages.len(), 2);
    assert!(
        result
            .conversation
            .messages
            .iter()
            .all(|m| m.role == IrRole::Tool)
    );
}

#[test]
fn chained_emulations_for_codex() {
    // Simulate what a full Codex-bound pipeline would do
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be concise"),
        IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "Look at this".into(),
                },
                IrContentBlock::Image {
                    media_type: "image/jpeg".into(),
                    data: "data".into(),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "reasoning...".into(),
                },
                IrContentBlock::Text {
                    text: "I see a photo".into(),
                },
            ],
        ),
    ]);

    let r1 = emulate_system_as_user(&conv);
    let r2 = emulate_images_as_placeholder(&r1.conversation);
    let r3 = strip_thinking(&r2.conversation);

    // System → user
    assert_eq!(r3.conversation.messages[0].role, IrRole::User);
    assert!(
        r3.conversation.messages[0]
            .text_content()
            .contains("[System]")
    );

    // Image → placeholder
    assert!(
        r3.conversation.messages[1]
            .text_content()
            .contains("[Image:")
    );

    // Thinking → stripped
    assert!(
        !r3.conversation.messages[2]
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    );

    // Total emulation notes
    let total_notes = r1.notes.len() + r2.notes.len() + r3.notes.len();
    assert_eq!(total_notes, 3);
}

// ═══════════════════════════════════════════════════════════════════════
// IR validation tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn valid_conversation_passes_structure_check() {
    let result = validate_ir_structure(&system_user_assistant());
    assert!(result.is_valid());
}

#[test]
fn empty_message_fails_structure_check() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "hi"),
        IrMessage::new(IrRole::Assistant, vec![]),
    ]);
    let result = validate_ir_structure(&conv);
    assert!(!result.is_valid());
    assert_eq!(result.issues[0].code, "empty_message");
}

#[test]
fn orphaned_tool_result_fails() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "ghost".into(),
            content: vec![IrContentBlock::Text {
                text: "orphan".into(),
            }],
            is_error: false,
        }],
    )]);
    let result = validate_ir_structure(&conv);
    assert!(!result.is_valid());
    assert_eq!(result.issues[0].code, "orphaned_tool_result");
}

#[test]
fn matched_tool_pair_is_valid() {
    let result = validate_ir_structure(&with_tool_call());
    assert!(result.is_valid());
}

#[test]
fn validate_thinking_for_openai_target() {
    let caps = dialect_capabilities(Dialect::OpenAi);
    let result = validate_for_target(&with_thinking(), &caps);
    assert!(!result.is_valid());
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.code == "unsupported_thinking")
    );
}

#[test]
fn validate_thinking_for_claude_target() {
    let caps = dialect_capabilities(Dialect::Claude);
    let result = validate_for_target(&with_thinking(), &caps);
    assert!(result.is_valid());
}

#[test]
fn validate_images_for_codex_target() {
    let caps = dialect_capabilities(Dialect::Codex);
    let result = validate_for_target(&with_image(), &caps);
    assert!(!result.is_valid());
    assert!(result.issues.iter().any(|i| i.code == "unsupported_image"));
}

#[test]
fn validate_system_for_codex_target() {
    let caps = dialect_capabilities(Dialect::Codex);
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::System, "Be helpful")]);
    let result = validate_for_target(&conv, &caps);
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.code == "unsupported_system_prompt")
    );
}

#[test]
fn validate_tool_use_for_codex_target() {
    let caps = dialect_capabilities(Dialect::Codex);
    let result = validate_for_target(&with_tool_call(), &caps);
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.code == "unsupported_tool_use")
    );
}

#[test]
fn combined_validation_for_codex() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::new(IrRole::Assistant, vec![]),
    ]);
    let result = validate_ir_for_mapping(&conv, Dialect::Codex);
    assert!(!result.is_valid());
    let codes: Vec<_> = result.issues.iter().map(|i| i.code).collect();
    assert!(codes.contains(&"empty_message"));
    assert!(codes.contains(&"unsupported_system_prompt"));
}

#[test]
fn empty_conversation_validates_for_any_target() {
    let conv = IrConversation::new();
    for &d in Dialect::all() {
        let result = validate_ir_for_mapping(&conv, d);
        assert!(result.is_valid(), "empty conv should be valid for {d}");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Roundtrip fidelity with enhanced mappers
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_openai_claude_with_tools() {
    let mapper = OpenAiClaudeIrMapper;
    let orig = with_tool_call();
    let claude = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &claude)
        .unwrap();

    assert_eq!(orig.tool_calls().len(), back.tool_calls().len());
    // Text content preserved
    assert_eq!(
        orig.messages[0].text_content(),
        back.messages[0].text_content()
    );
}

#[test]
fn roundtrip_openai_gemini_text() {
    let mapper = OpenAiGeminiIrMapper;
    let orig = system_user_assistant();
    let gemini = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.text_content(), b.text_content());
    }
}

#[test]
fn roundtrip_thinking_lossy_to_openai() {
    let mapper = OpenAiClaudeIrMapper;
    let orig = with_thinking();
    let openai = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &openai)
        .unwrap();
    // Text survives
    assert_eq!(back.messages[1].text_content(), "The answer is 42.");
    // Thinking is gone
    assert!(
        !back.messages[1]
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Codex unmappable tools
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codex_to_claude_rejects_apply_patch() {
    let mapper = CodexClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "apply_patch".into(),
            input: json!({"patch": "..."}),
        }],
    )]);
    let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &conv);
    assert!(matches!(result, Err(MapError::UnmappableTool { .. })));
}

#[test]
fn codex_to_claude_allows_normal_tools() {
    let mapper = CodexClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        }],
    )]);
    let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &conv);
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// Metadata preservation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn metadata_survives_all_mappers() {
    let mut msg = IrMessage::text(IrRole::User, "hello");
    msg.metadata.insert("key".into(), json!("value"));
    let conv = IrConversation::from_messages(vec![msg]);

    // Test a few mappers
    let mappers: Vec<(&dyn IrMapper, Dialect, Dialect)> = vec![
        (&OpenAiClaudeIrMapper, Dialect::OpenAi, Dialect::Claude),
        (&OpenAiGeminiIrMapper, Dialect::OpenAi, Dialect::Gemini),
        (&OpenAiKimiIrMapper, Dialect::OpenAi, Dialect::Kimi),
        (&OpenAiCopilotIrMapper, Dialect::OpenAi, Dialect::Copilot),
    ];

    for (mapper, from, to) in mappers {
        let result = mapper.map_request(from, to, &conv).unwrap();
        assert_eq!(
            result.messages[0].metadata.get("key"),
            Some(&json!("value")),
            "metadata lost in {from} -> {to}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_conversation_maps_cleanly() {
    let empty = IrConversation::new();
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &empty)
        .unwrap();
    assert!(result.is_empty());
}

#[test]
fn codex_emulation_preserves_message_order() {
    let mapper = OpenAiCodexIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "msg1"),
        IrMessage::text(IrRole::Assistant, "resp1"),
        IrMessage::text(IrRole::User, "msg2"),
        IrMessage::text(IrRole::Assistant, "resp2"),
    ]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
        .unwrap();
    assert_eq!(result.len(), 5);
    // System emulated as first user message
    assert!(result.messages[0].text_content().contains("[System]"));
    // Alternating roles preserved
    assert_eq!(result.messages[1].text_content(), "msg1");
    assert_eq!(result.messages[2].text_content(), "resp1");
    assert_eq!(result.messages[3].text_content(), "msg2");
    assert_eq!(result.messages[4].text_content(), "resp2");
}

#[test]
fn error_tool_result_preserved_through_mapping() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "run"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "exec".into(),
                input: json!({"cmd": "fail"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "error: denied".into(),
                }],
                is_error: true,
            }],
        ),
    ]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let tool_result = &result.messages[2].content[0];
    if let IrContentBlock::ToolResult { is_error, .. } = tool_result {
        assert!(is_error, "error flag should be preserved");
    } else {
        panic!("expected ToolResult block");
    }
}
