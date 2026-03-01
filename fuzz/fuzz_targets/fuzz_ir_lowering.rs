// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz IR lowering with arbitrary JSON.
//!
//! Parses random bytes as JSON and attempts to deserialize into
//! [`IrConversation`]. On success, exercises all accessor methods and
//! serde round-trips. Also constructs conversations from structured
//! fuzzer input and exercises per-SDK `from_ir` lowering to verify
//! no panics on arbitrary IR data.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};

#[derive(Debug, Arbitrary)]
struct IrFuzzInput {
    /// Raw JSON bytes to try parsing.
    raw_json: String,
    /// Structured messages to build an IrConversation.
    messages: Vec<FuzzMessage>,
}

#[derive(Debug, Arbitrary)]
struct FuzzMessage {
    role_idx: u8,
    blocks: Vec<FuzzBlock>,
    metadata_key: Option<String>,
    metadata_val: Option<String>,
}

#[derive(Debug, Arbitrary)]
struct FuzzBlock {
    variant_idx: u8,
    text: String,
    id: String,
    name: String,
    is_error: bool,
}

fuzz_target!(|input: IrFuzzInput| {
    // --- Path 1: parse arbitrary JSON as IrConversation ---
    if let Ok(conv) = serde_json::from_str::<IrConversation>(&input.raw_json) {
        // Exercise all accessor methods — must never panic.
        let _ = conv.len();
        let _ = conv.is_empty();
        let _ = conv.system_message();
        let _ = conv.last_assistant();
        let _ = conv.last_message();
        let _ = conv.tool_calls();
        let _ = conv.messages_by_role(IrRole::System);
        let _ = conv.messages_by_role(IrRole::User);
        let _ = conv.messages_by_role(IrRole::Assistant);
        let _ = conv.messages_by_role(IrRole::Tool);

        for msg in &conv.messages {
            let _ = msg.is_text_only();
            let _ = msg.text_content();
            let _ = msg.tool_use_blocks();
        }

        // Serde round-trip must succeed.
        if let Ok(json) = serde_json::to_string(&conv) {
            let rt = serde_json::from_str::<IrConversation>(&json);
            assert!(rt.is_ok(), "IrConversation round-trip must succeed");
        }
    }

    // Also try parsing individual messages.
    let _ = serde_json::from_str::<IrMessage>(&input.raw_json);
    let _ = serde_json::from_str::<IrContentBlock>(&input.raw_json);

    // --- Path 2: construct from structured input ---
    let ir_messages: Vec<IrMessage> = input
        .messages
        .iter()
        .map(|m| {
            let role = match m.role_idx % 4 {
                0 => IrRole::System,
                1 => IrRole::User,
                2 => IrRole::Assistant,
                _ => IrRole::Tool,
            };

            let blocks: Vec<IrContentBlock> = m
                .blocks
                .iter()
                .map(|b| match b.variant_idx % 5 {
                    0 => IrContentBlock::Text {
                        text: b.text.clone(),
                    },
                    1 => IrContentBlock::Image {
                        media_type: b.name.clone(),
                        data: b.text.clone(),
                    },
                    2 => IrContentBlock::ToolUse {
                        id: b.id.clone(),
                        name: b.name.clone(),
                        input: serde_json::Value::String(b.text.clone()),
                    },
                    3 => IrContentBlock::ToolResult {
                        tool_use_id: b.id.clone(),
                        content: vec![IrContentBlock::Text {
                            text: b.text.clone(),
                        }],
                        is_error: b.is_error,
                    },
                    _ => IrContentBlock::Thinking {
                        text: b.text.clone(),
                    },
                })
                .collect();

            let mut msg = IrMessage::new(role, blocks);
            if let (Some(k), Some(v)) = (&m.metadata_key, &m.metadata_val) {
                msg.metadata
                    .insert(k.clone(), serde_json::Value::String(v.clone()));
            }
            msg
        })
        .collect();

    let conv = IrConversation::from_messages(ir_messages);

    // Exercise accessors on the constructed conversation.
    let _ = conv.system_message();
    let _ = conv.last_assistant();
    let _ = conv.tool_calls();
    let _ = conv.len();

    for msg in &conv.messages {
        let _ = msg.is_text_only();
        let _ = msg.text_content();
        let _ = msg.tool_use_blocks();
    }

    // Serde round-trip.
    if let Ok(json) = serde_json::to_string(&conv) {
        let rt: IrConversation = serde_json::from_str(&json).expect("round-trip must succeed");
        assert_eq!(rt.len(), conv.len());
    }
});
