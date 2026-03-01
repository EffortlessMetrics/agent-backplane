// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz the JSONL protocol envelope deserializer with arbitrary bytes.
//!
//! Unlike fuzz_envelope which only tests valid UTF-8, this target feeds raw
//! bytes through the protocol layer to ensure no panics on malformed input.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::io::BufReader;

fuzz_target!(|data: &[u8]| {
    // --- Single-line decode with raw bytes ---
    // Try as UTF-8 first.
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = abp_protocol::JsonlCodec::decode(s);

        // Also exercise version parsing with arbitrary strings.
        let _ = abp_protocol::parse_version(s);
        let _ = abp_protocol::is_compatible_version(s, "abp/v0.1");
        let _ = abp_protocol::is_compatible_version("abp/v0.1", s);
    }

    // --- Streaming decode over raw bytes ---
    // BufReader will handle invalid UTF-8 lines as I/O errors.
    let reader = BufReader::new(data);
    for result in abp_protocol::JsonlCodec::decode_stream(reader) {
        match result {
            Ok(envelope) => {
                // Round-trip: encode back and re-decode.
                if let Ok(line) = abp_protocol::JsonlCodec::encode(&envelope) {
                    let _ = abp_protocol::JsonlCodec::decode(line.trim());
                }
            }
            Err(_) => {}
        }
    }

    // --- Encode to writer ---
    // If we managed to decode something, try writing it back out.
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(env) = abp_protocol::JsonlCodec::decode(s) {
            let mut buf = Vec::new();
            let _ = abp_protocol::JsonlCodec::encode_to_writer(&mut buf, &env);
        }
    }
});
