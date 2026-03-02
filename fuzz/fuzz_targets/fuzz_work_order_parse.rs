// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz WorkOrder deserialization with arbitrary byte input.
//!
//! Verifies:
//! 1. `serde_json::from_str::<WorkOrder>` never panics.
//! 2. Successfully parsed WorkOrders round-trip through JSON.
//! 3. All fields are accessible without panics.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Property 1: deserialization never panics
    let wo = match serde_json::from_str::<abp_core::WorkOrder>(s) {
        Ok(w) => w,
        Err(_) => return,
    };

    // Property 2: round-trip through JSON
    if let Ok(json) = serde_json::to_string(&wo) {
        let rt = serde_json::from_str::<abp_core::WorkOrder>(&json);
        assert!(rt.is_ok(), "WorkOrder JSON round-trip must succeed");
    }

    // Property 3: field access never panics
    let _ = wo.id;
    let _ = wo.task.len();
    let _ = wo.workspace.root.len();
    let _ = wo.context.files.len();
    let _ = wo.policy.allowed_tools.len();
    let _ = wo.config.model;
    let _ = format!("{:?}", wo.lane);
    let _ = format!("{:?}", wo.requirements);
});
