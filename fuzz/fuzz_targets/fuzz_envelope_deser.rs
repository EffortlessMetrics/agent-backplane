// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz JSONL envelope deserialization with arbitrary bytes.
//!
//! Tests that `Envelope` deserialization (via both `JsonlCodec::decode` and
//! direct `serde_json::from_str`) never panics on any input, including
//! invalid UTF-8, truncated JSON, and adversarial payloads. Also verifies
//! that successfully decoded envelopes survive encode→decode round-trips
//! and that the `EnvelopeValidator` never panics.
#![no_main]
use abp_protocol::validate::EnvelopeValidator;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // --- Raw serde_json from bytes ---
    // serde_json::from_slice handles invalid UTF-8 gracefully.
    let direct: Result<abp_protocol::Envelope, _> = serde_json::from_slice(data);

    // --- Codec-based decode (UTF-8 path) ---
    if let Ok(s) = std::str::from_utf8(data) {
        // Process each line independently to exercise multi-line JSONL.
        for line in s.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(envelope) = abp_protocol::JsonlCodec::decode(trimmed) {
                // Round-trip: encode back then re-decode — must be lossless.
                if let Ok(encoded) = abp_protocol::JsonlCodec::encode(&envelope) {
                    let rt = abp_protocol::JsonlCodec::decode(encoded.trim());
                    assert!(rt.is_ok(), "round-trip decode must succeed");
                }

                // Validate the parsed envelope — validator must not panic.
                let validator = EnvelopeValidator::new();
                let _ = validator.validate(&envelope);
            }
        }
    }

    // --- Streaming decode from raw bytes ---
    let reader = std::io::BufReader::new(data);
    for result in abp_protocol::JsonlCodec::decode_stream(reader) {
        if let Ok(env) = result {
            // Successful stream decode should also survive validation.
            let validator = EnvelopeValidator::new();
            let _ = validator.validate(&env);
        }
    }

    // If direct deserialization succeeded, verify it round-trips through JSON.
    if let Ok(envelope) = direct {
        if let Ok(json) = serde_json::to_string(&envelope) {
            let _ = serde_json::from_str::<abp_protocol::Envelope>(&json);
        }
    }
});
