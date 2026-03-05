// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz Receipt deserialization and hashing from arbitrary bytes.
//!
//! Verifies:
//! 1. `serde_json::from_slice::<Receipt>` never panics on any input.
//! 2. `serde_json::from_str::<Receipt>` never panics on valid UTF-8.
//! 3. `receipt_hash` never panics on any successfully parsed Receipt.
//! 4. Successful parses survive JSON round-trips.
//! 5. `canonical_json` never panics on valid Receipts.
//! 6. Hash is deterministic: same receipt always produces same hash.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // --- Property 1: from_slice never panics (handles malformed UTF-8) ---
    let from_bytes: Result<abp_core::Receipt, _> = serde_json::from_slice(data);

    if let Ok(receipt) = &from_bytes {
        let _ = abp_core::receipt_hash(receipt);
        let _ = abp_core::canonical_json(receipt);
    }

    // --- Property 2: from_str path ---
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    let from_str: Result<abp_core::Receipt, _> = serde_json::from_str(s);

    if let Ok(receipt) = from_str {
        // --- Property 3: receipt_hash never panics ---
        let hash1 = abp_core::receipt_hash(&receipt);

        // --- Property 4: JSON round-trip ---
        if let Ok(json) = serde_json::to_string(&receipt) {
            let rt = serde_json::from_str::<abp_core::Receipt>(&json);
            assert!(rt.is_ok(), "JSON round-trip must succeed for valid Receipt");

            // --- Property 6: hash determinism ---
            if let Ok(rt_receipt) = rt {
                let hash2 = abp_core::receipt_hash(&rt_receipt);
                if let (Ok(h1), Ok(h2)) = (hash1, hash2) {
                    assert_eq!(
                        h1, h2,
                        "receipt_hash must be deterministic across round-trips"
                    );
                }
            }
        }

        // --- Property 5: canonical_json never panics ---
        let _ = abp_core::canonical_json(&receipt);
    }

    // Also try related types.
    let _ = serde_json::from_str::<abp_core::Outcome>(s);
    let _ = serde_json::from_str::<abp_core::AgentEvent>(s);

    // Edge cases.
    let _ = serde_json::from_str::<abp_core::Receipt>("");
    let _ = serde_json::from_str::<abp_core::Receipt>("null");
    let _ = serde_json::from_str::<abp_core::Receipt>("[]");
});
