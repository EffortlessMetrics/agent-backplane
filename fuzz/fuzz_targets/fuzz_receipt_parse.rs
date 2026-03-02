// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz Receipt deserialization with arbitrary byte input.
//!
//! Verifies:
//! 1. `serde_json::from_str::<Receipt>` never panics.
//! 2. Successfully parsed Receipts round-trip through JSON.
//! 3. `receipt_hash` never panics on parsed receipts.
//! 4. `validate_receipt` never panics.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Property 1: deserialization never panics
    let receipt = match serde_json::from_str::<abp_core::Receipt>(s) {
        Ok(r) => r,
        Err(_) => return,
    };

    // Property 2: round-trip through JSON
    if let Ok(json) = serde_json::to_string(&receipt) {
        let rt = serde_json::from_str::<abp_core::Receipt>(&json);
        assert!(rt.is_ok(), "Receipt JSON round-trip must succeed");
    }

    // Property 3: receipt_hash never panics
    let _ = abp_core::receipt_hash(&receipt);

    // Property 4: validate never panics
    let _ = abp_core::validate::validate_receipt(&receipt);

    // Property 5: with_hash never panics
    let _ = receipt.with_hash();
});
