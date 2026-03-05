// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz AgentEvent JSON deserialization with arbitrary bytes.
//!
//! Verifies:
//! 1. `serde_json::from_slice::<AgentEvent>` never panics.
//! 2. Successfully parsed events round-trip through JSON.
//! 3. Inner `AgentEventKind` deserialization never panics.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Try deserializing as a full AgentEvent (has ts + kind + ext).
    if let Ok(event) = serde_json::from_slice::<abp_core::AgentEvent>(data) {
        // Round-trip through JSON.
        if let Ok(json) = serde_json::to_vec(&event) {
            let rt = serde_json::from_slice::<abp_core::AgentEvent>(&json);
            assert!(rt.is_ok(), "AgentEvent JSON round-trip must succeed");
        }
        // Field access must never panic.
        let _ = event.ts;
        let _ = format!("{:?}", event.kind);
        let _ = event.ext;
    }

    // Also try just the kind discriminator on its own.
    let _ = serde_json::from_slice::<abp_core::AgentEventKind>(data);
});
