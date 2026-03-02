// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz IR type roundtrips with arbitrary input.
//!
//! Verifies:
//! 1. JSON deserialization into IR types never panics.
//! 2. Successfully parsed IR types survive JSON round-trips.
//! 3. Constructed IrConversations maintain invariants through serde.
#![no_main]
use libfuzzer_sys::fuzz_target;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};

fuzz_target!(|data: &[u8]| {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Path 1: try parsing as IrConversation
    if let Ok(conv) = serde_json::from_str::<IrConversation>(s) {
        // Round-trip
        if let Ok(json) = serde_json::to_string(&conv) {
            let rt = serde_json::from_str::<IrConversation>(&json);
            assert!(rt.is_ok(), "IrConversation round-trip must succeed");
            let rt = rt.unwrap();
            assert_eq!(rt.len(), conv.len(), "round-trip must preserve length");
        }

        // Exercise accessors
        let _ = conv.system_message();
        let _ = conv.last_assistant();
        let _ = conv.last_message();
        let _ = conv.tool_calls();
        let _ = conv.is_empty();
        for msg in &conv.messages {
            let _ = msg.is_text_only();
            let _ = msg.text_content();
            let _ = msg.tool_use_blocks();
        }
    }

    // Path 2: try parsing as IrMessage
    if let Ok(msg) = serde_json::from_str::<IrMessage>(s) {
        if let Ok(json) = serde_json::to_string(&msg) {
            let rt = serde_json::from_str::<IrMessage>(&json);
            assert!(rt.is_ok(), "IrMessage round-trip must succeed");
        }
        let _ = msg.is_text_only();
        let _ = msg.text_content();
        let _ = msg.tool_use_blocks();
    }

    // Path 3: try parsing as IrContentBlock
    if let Ok(block) = serde_json::from_str::<IrContentBlock>(s) {
        if let Ok(json) = serde_json::to_string(&block) {
            let rt = serde_json::from_str::<IrContentBlock>(&json);
            assert!(rt.is_ok(), "IrContentBlock round-trip must succeed");
        }
    }

    // Path 4: try parsing as IrRole
    if let Ok(role) = serde_json::from_str::<IrRole>(s) {
        if let Ok(json) = serde_json::to_string(&role) {
            let rt = serde_json::from_str::<IrRole>(&json);
            assert!(rt.is_ok(), "IrRole round-trip must succeed");
        }
    }
});
