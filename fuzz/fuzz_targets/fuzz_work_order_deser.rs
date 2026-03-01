// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz WorkOrder deserialization from arbitrary bytes.
//!
//! Feeds raw bytes through `serde_json::from_slice` and string-based
//! `serde_json::from_str` to ensure `WorkOrder` deserialization never panics.
//! On successful parse, exercises JSON round-trip, canonical JSON, and
//! receipt validation to check for downstream panics from adversarial data.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // --- Direct from bytes (handles non-UTF-8 gracefully) ---
    let from_bytes: Result<abp_core::WorkOrder, _> = serde_json::from_slice(data);

    // --- From string (UTF-8 path) ---
    if let Ok(s) = std::str::from_utf8(data) {
        let from_str: Result<abp_core::WorkOrder, _> = serde_json::from_str(s);

        if let Ok(wo) = from_str {
            // JSON round-trip must not panic.
            if let Ok(json) = serde_json::to_string(&wo) {
                let rt = serde_json::from_str::<abp_core::WorkOrder>(&json);
                assert!(rt.is_ok(), "JSON round-trip must succeed for valid WorkOrder");
            }

            // canonical_json must not panic on any valid WorkOrder.
            let _ = abp_core::canonical_json(&wo);

            // Verify ID is present (Uuid is always valid).
            let _ = wo.id;
        }

        // Also try parsing as RuntimeConfig — must never panic.
        let _ = serde_json::from_str::<abp_core::RuntimeConfig>(s);

        // And as WorkspaceSpec — must never panic.
        let _ = serde_json::from_str::<abp_core::WorkspaceSpec>(s);

        // And as ContextPacket — must never panic.
        let _ = serde_json::from_str::<abp_core::ContextPacket>(s);
    }

    // Byte-level parse check.
    if let Ok(wo) = from_bytes {
        let _ = abp_core::canonical_json(&wo);
    }
});
