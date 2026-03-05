// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz Receipt JSON deserialization with arbitrary bytes.
//!
//! Verifies:
//! 1. `serde_json::from_slice::<Receipt>` never panics.
//! 2. Successfully parsed receipts round-trip through JSON.
//! 3. `receipt_hash()` and `with_hash()` never panic on parsed receipts.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Feed raw bytes directly — serde_json handles UTF-8 validation.
    let receipt: abp_core::Receipt = match serde_json::from_slice(data) {
        Ok(r) => r,
        Err(_) => return,
    };

    // Round-trip through JSON.
    if let Ok(json) = serde_json::to_vec(&receipt) {
        let rt = serde_json::from_slice::<abp_core::Receipt>(&json);
        assert!(rt.is_ok(), "Receipt JSON round-trip must succeed");
    }

    // Hashing must never panic, even on arbitrary valid receipts.
    let _ = abp_core::receipt_hash(&receipt);
    let _ = receipt.with_hash();
});
