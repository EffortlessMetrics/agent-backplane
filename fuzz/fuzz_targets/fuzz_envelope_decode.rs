// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz Envelope deserialization from arbitrary JSONL bytes.
//!
//! Tests that `Envelope` deserialization via `JsonlCodec`, `StreamParser`,
//! and direct `serde_json` paths never panics on any input. Exercises:
//! 1. Single-line decode from raw bytes and UTF-8 strings.
//! 2. Multi-line JSONL streams with interleaved garbage.
//! 3. `StreamParser` incremental chunked decoding.
//! 4. Round-trip: successful decode → encode → re-decode.
//! 5. `EnvelopeValidator` and `validate_sequence` on decoded envelopes.
//! 6. Edge cases: empty input, single byte, huge lines, invalid UTF-8.
#![no_main]
use libfuzzer_sys::fuzz_target;

use abp_protocol::stream::StreamParser;
use abp_protocol::validate::EnvelopeValidator;
use abp_protocol::{Envelope, JsonlCodec};

fuzz_target!(|data: &[u8]| {
    // --- Path 1: direct serde_json::from_slice (handles non-UTF-8) ---
    let direct: Result<Envelope, _> = serde_json::from_slice(data);
    if let Ok(ref env) = direct {
        // Round-trip through JSON must succeed.
        if let Ok(json) = serde_json::to_string(env) {
            let rt: Result<Envelope, _> = serde_json::from_str(&json);
            assert!(rt.is_ok(), "direct round-trip must succeed");
        }
    }

    // --- Path 2: JsonlCodec line-by-line decode (UTF-8 path) ---
    let mut decoded_envelopes = Vec::new();
    if let Ok(s) = std::str::from_utf8(data) {
        for line in s.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(envelope) = JsonlCodec::decode(trimmed) {
                // Encode back then re-decode — must be lossless.
                if let Ok(encoded) = JsonlCodec::encode(&envelope) {
                    let rt = JsonlCodec::decode(encoded.trim());
                    assert!(rt.is_ok(), "codec round-trip must succeed");
                }
                decoded_envelopes.push(envelope);
            }
        }
    }

    // --- Path 3: streaming decode via BufRead ---
    let reader = std::io::BufReader::new(data);
    let mut stream_envelopes = Vec::new();
    for result in JsonlCodec::decode_stream(reader) {
        if let Ok(env) = result {
            stream_envelopes.push(env);
        }
    }

    // --- Path 4: StreamParser incremental chunked decode ---
    // Split data into random-sized chunks to exercise buffering.
    let mut parser = StreamParser::new();
    let mut chunk_envelopes = Vec::new();
    let chunk_size = if data.is_empty() {
        1
    } else {
        (data[0] as usize % 64) + 1
    };
    for chunk in data.chunks(chunk_size) {
        for result in parser.push(chunk) {
            if let Ok(env) = result {
                chunk_envelopes.push(env);
            }
        }
    }
    // Flush remaining buffer.
    for result in parser.finish() {
        if let Ok(env) = result {
            chunk_envelopes.push(env);
        }
    }

    // --- Path 5: EnvelopeValidator on all decoded envelopes ---
    let validator = EnvelopeValidator::new();
    for env in &decoded_envelopes {
        let result = validator.validate(env);
        // valid flag must match errors.
        assert_eq!(result.valid, result.errors.is_empty());
    }
    for env in &stream_envelopes {
        let result = validator.validate(env);
        assert_eq!(result.valid, result.errors.is_empty());
    }
    for env in &chunk_envelopes {
        let result = validator.validate(env);
        assert_eq!(result.valid, result.errors.is_empty());
    }

    // --- Path 6: validate_sequence on collected envelopes ---
    if !decoded_envelopes.is_empty() {
        let seq_errors = validator.validate_sequence(&decoded_envelopes);
        // Sequence errors must be displayable without panic.
        for e in &seq_errors {
            let _ = format!("{e:?}");
        }
    }

    // --- Path 7: edge case — empty StreamParser ---
    let mut empty_parser = StreamParser::new();
    let results = empty_parser.finish();
    assert!(results.is_empty(), "empty parser finish must yield nothing");
    assert!(empty_parser.is_empty());
    assert_eq!(empty_parser.buffered_len(), 0);
});
