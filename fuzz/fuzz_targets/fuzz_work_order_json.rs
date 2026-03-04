// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz WorkOrder JSON deserialization with arbitrary bytes.
//!
//! Verifies that `serde_json::from_slice::<WorkOrder>` never panics on any
//! input, and that successfully parsed work orders survive a JSON round-trip.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Feed raw bytes directly to serde_json (handles UTF-8 internally).
    let wo: abp_core::WorkOrder = match serde_json::from_slice(data) {
        Ok(w) => w,
        Err(_) => return,
    };

    // Round-trip: serialize back then deserialize again.
    if let Ok(json) = serde_json::to_vec(&wo) {
        let rt = serde_json::from_slice::<abp_core::WorkOrder>(&json);
        assert!(rt.is_ok(), "WorkOrder JSON round-trip must succeed");
    }
});
