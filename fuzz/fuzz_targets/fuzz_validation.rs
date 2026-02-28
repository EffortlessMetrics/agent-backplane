// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz the receipt validation pipeline with arbitrary receipts.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(receipt) = serde_json::from_str::<abp_core::Receipt>(s) {
            // Exercise validation — must never panic.
            let _ = abp_core::validate::validate_receipt(&receipt);

            // Exercise hashing on the same receipt.
            let _ = abp_core::receipt_hash(&receipt);

            // Exercise with_hash round-trip.
            let _ = receipt.with_hash();
        }
    }
});
