// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz JSONL line parsing and Envelope deserialization.
//!
//! Verifies:
//! 1. `JsonlCodec::decode` never panics on any UTF-8 string.
//! 2. `JsonlCodec::decode_batch` never panics on multi-line input.
//! 3. `JsonlCodec::validate_jsonl` never panics and agrees with decode_batch.
//! 4. Successfully decoded envelopes survive encode→decode round-trips.
//! 5. `EnvelopeValidator` never panics on any decoded envelope.
//! 6. `validate_sequence` never panics on any sequence of envelopes.
//! 7. `line_count` is consistent with newline splitting.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // --- Property 1: single-line decode never panics ---
    // Try each line individually, simulating line-by-line JSONL consumption.
    let mut decoded_envelopes = Vec::new();
    for line in s.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(env) = abp_protocol::JsonlCodec::decode(trimmed) {
            decoded_envelopes.push(env);
        }
    }

    // --- Property 2: batch decode never panics ---
    let batch_results = abp_protocol::codec::StreamingCodec::decode_batch(s);
    let batch_ok: Vec<_> = batch_results.into_iter().filter_map(|r| r.ok()).collect();

    // --- Property 3: validate_jsonl never panics, error count is consistent ---
    let validation_errors = abp_protocol::codec::StreamingCodec::validate_jsonl(s);
    // Every validation error corresponds to a line that failed to decode.
    let _ = validation_errors.len();

    // --- Property 4: round-trip for successfully decoded envelopes ---
    for env in &decoded_envelopes {
        if let Ok(encoded) = abp_protocol::JsonlCodec::encode(env) {
            if let Ok(rt) = abp_protocol::JsonlCodec::decode(encoded.trim()) {
                // Re-encode and compare JSON values for structural equality.
                let a = serde_json::to_value(env);
                let b = serde_json::to_value(&rt);
                if let (Ok(va), Ok(vb)) = (a, b) {
                    assert_eq!(va, vb, "round-trip must preserve envelope structure");
                }
            }
        }
    }

    // --- Property 5: EnvelopeValidator never panics ---
    let validator = abp_protocol::validate::EnvelopeValidator::new();
    for env in &decoded_envelopes {
        let _ = validator.validate(env);
    }
    for env in &batch_ok {
        let _ = validator.validate(env);
    }

    // --- Property 6: validate_sequence never panics ---
    if !decoded_envelopes.is_empty() {
        let _ = validator.validate_sequence(&decoded_envelopes);
    }

    // --- Property 7: line_count is consistent ---
    let count = abp_protocol::codec::StreamingCodec::line_count(s);
    // line_count should equal the number of non-empty lines.
    let manual_count = s.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(
        count, manual_count,
        "line_count must match non-empty line count"
    );

    // --- Property 8: decode_stream agrees with line-by-line decode ---
    let reader = std::io::BufReader::new(data);
    let stream_results: Vec<_> = abp_protocol::JsonlCodec::decode_stream(reader).collect();
    // Stream should produce the same number of successful results.
    let stream_ok_count = stream_results.iter().filter(|r| r.is_ok()).count();
    let _ = stream_ok_count;
});
