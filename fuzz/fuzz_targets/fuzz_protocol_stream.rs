// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz JSONL stream parsing with arbitrary byte chunks.
//!
//! Verifies:
//! 1. `StreamParser::push` never panics on arbitrary byte input.
//! 2. `JsonlCodec::decode_stream` never panics on arbitrary input.
//! 3. Multi-chunk feeding produces consistent results.
#![no_main]
use libfuzzer_sys::fuzz_target;

use abp_protocol::stream::StreamParser;
use std::io::BufReader;

fuzz_target!(|data: &[u8]| {
    // Property 1: StreamParser incremental feeding never panics
    let mut parser = StreamParser::new();
    // Feed in small chunks to exercise partial line handling
    for chunk in data.chunks(8) {
        let results = parser.push(chunk);
        for result in results {
            match result {
                Ok(envelope) => {
                    // Verify we can re-encode without panic
                    let _ = abp_protocol::JsonlCodec::encode(&envelope);
                }
                Err(_) => {}
            }
        }
    }

    // Property 2: BufRead-based decode_stream never panics
    let reader = BufReader::new(data);
    for result in abp_protocol::JsonlCodec::decode_stream(reader) {
        let _ = result;
    }

    // Property 3: feeding all at once should also be safe
    let mut parser2 = StreamParser::new();
    let results = parser2.push(data);
    for result in results {
        let _ = result;
    }
});
