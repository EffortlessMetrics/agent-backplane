// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz AgentEvent deserialization with arbitrary byte input.
//!
//! Verifies:
//! 1. `serde_json::from_str::<AgentEvent>` never panics.
//! 2. Successfully parsed events round-trip through JSON.
//! 3. AgentEventKind variants are all accessible without panics.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Property 1: deserialization never panics
    let event = match serde_json::from_str::<abp_core::AgentEvent>(s) {
        Ok(e) => e,
        Err(_) => return,
    };

    // Property 2: round-trip through JSON
    if let Ok(json) = serde_json::to_string(&event) {
        let rt = serde_json::from_str::<abp_core::AgentEvent>(&json);
        assert!(rt.is_ok(), "AgentEvent JSON round-trip must succeed");
    }

    // Property 3: field access never panics
    let _ = event.ts;
    let _ = format!("{:?}", event.kind);
    let _ = event.ext;

    // Also try deserializing as just AgentEventKind
    let _ = serde_json::from_str::<abp_core::AgentEventKind>(s);
});
