// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz WorkOrder deserialization from arbitrary bytes.
//!
//! Verifies:
//! 1. `serde_json::from_slice::<WorkOrder>` never panics on any input.
//! 2. `serde_json::from_str::<WorkOrder>` never panics on valid UTF-8.
//! 3. Successful parses survive JSON round-trips.
//! 4. `canonical_json` never panics on any valid WorkOrder.
//! 5. Related types (RuntimeConfig, WorkspaceSpec, ContextPacket) never panic.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // --- Property 1: from_slice never panics (handles malformed UTF-8) ---
    let from_bytes: Result<abp_core::WorkOrder, _> = serde_json::from_slice(data);

    if let Ok(wo) = &from_bytes {
        // canonical_json must not panic.
        let _ = abp_core::canonical_json(wo);
    }

    // --- Property 2: from_str path for valid UTF-8 ---
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    let from_str: Result<abp_core::WorkOrder, _> = serde_json::from_str(s);

    // --- Property 3: JSON round-trip ---
    if let Ok(wo) = from_str {
        if let Ok(json) = serde_json::to_string(&wo) {
            let rt = serde_json::from_str::<abp_core::WorkOrder>(&json);
            assert!(
                rt.is_ok(),
                "JSON round-trip must succeed for valid WorkOrder"
            );
        }

        // --- Property 4: canonical_json must not panic ---
        let _ = abp_core::canonical_json(&wo);

        // Access fields to check they're consistent.
        let _ = wo.id;
        let _ = wo.task.len();
    }

    // --- Property 5: related types never panic ---
    let _ = serde_json::from_str::<abp_core::RuntimeConfig>(s);
    let _ = serde_json::from_str::<abp_core::WorkspaceSpec>(s);
    let _ = serde_json::from_str::<abp_core::ContextPacket>(s);

    // Also try empty and edge-case strings explicitly.
    let _ = serde_json::from_str::<abp_core::WorkOrder>("");
    let _ = serde_json::from_str::<abp_core::WorkOrder>("null");
    let _ = serde_json::from_str::<abp_core::WorkOrder>("[]");
});
