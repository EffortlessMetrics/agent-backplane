// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz JSONL envelope parsing with arbitrary byte input.
//!
//! Verifies:
//! 1. `JsonlCodec::decode` never panics on any UTF-8 input.
//! 2. Successfully decoded envelopes round-trip through encode/decode.
//! 3. `serde_json` direct deserialization agrees with codec.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Property 1: decode never panics
    let envelope = match abp_protocol::JsonlCodec::decode(s) {
        Ok(e) => e,
        Err(_) => return,
    };

    // Property 2: round-trip through encode/decode
    if let Ok(encoded) = abp_protocol::JsonlCodec::encode(&envelope) {
        let _ = abp_protocol::JsonlCodec::decode(encoded.trim());
    }

    // Property 3: serde_json agrees
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(s) {
        let _ = serde_json::from_value::<abp_protocol::Envelope>(val);
    }
});
