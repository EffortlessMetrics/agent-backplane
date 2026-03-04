#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for `StreamParser` incremental JSONL parsing.

use abp_protocol::stream::StreamParser;
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};

/// Helper: encode a fatal envelope to a newline-terminated JSON string.
fn fatal_line(msg: &str) -> String {
    JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: msg.into(),
        error_code: None,
    })
    .unwrap()
}

// -----------------------------------------------------------------------
// 1. Single complete line → one envelope
// -----------------------------------------------------------------------
#[test]
fn single_complete_line() {
    let mut parser = StreamParser::new();
    let line = fatal_line("boom");
    let results = parser.feed(line.as_bytes());
    assert_eq!(results.len(), 1);
    let env = results.into_iter().next().unwrap().unwrap();
    assert!(matches!(env, Envelope::Fatal { error, .. } if error == "boom"));
}

// -----------------------------------------------------------------------
// 2. Multiple lines in one feed → multiple envelopes
// -----------------------------------------------------------------------
#[test]
fn multiple_lines_in_one_feed() {
    let mut parser = StreamParser::new();
    let input = format!("{}{}{}", fatal_line("a"), fatal_line("b"), fatal_line("c"));
    let results = parser.feed(input.as_bytes());
    assert_eq!(results.len(), 3);
    for r in &results {
        assert!(r.is_ok());
    }
}

// -----------------------------------------------------------------------
// 3. Partial line across two feeds → correctly assembled
// -----------------------------------------------------------------------
#[test]
fn partial_line_across_two_feeds() {
    let mut parser = StreamParser::new();
    let line = fatal_line("split");
    let bytes = line.as_bytes();
    let mid = bytes.len() / 2;

    let r1 = parser.feed(&bytes[..mid]);
    assert!(r1.is_empty());
    assert!(!parser.is_empty());

    let r2 = parser.feed(&bytes[mid..]);
    assert_eq!(r2.len(), 1);
    let env = r2.into_iter().next().unwrap().unwrap();
    assert!(matches!(env, Envelope::Fatal { error, .. } if error == "split"));
}

// -----------------------------------------------------------------------
// 4. Empty lines ignored
// -----------------------------------------------------------------------
#[test]
fn empty_lines_ignored() {
    let mut parser = StreamParser::new();
    let input = format!(
        "\n\n{}\n\n{}\n\n",
        fatal_line("x").trim(),
        fatal_line("y").trim()
    );
    let results = parser.feed(input.as_bytes());
    assert_eq!(results.len(), 2);
}

// -----------------------------------------------------------------------
// 5. Max line length exceeded → error
// -----------------------------------------------------------------------
#[test]
fn max_line_length_exceeded() {
    let mut parser = StreamParser::with_max_line_len(20);
    let line = fatal_line("this message is definitely longer than twenty bytes");
    let results = parser.feed(line.as_bytes());
    assert_eq!(results.len(), 1);
    let err = results.into_iter().next().unwrap().unwrap_err();
    assert!(matches!(err, ProtocolError::Violation(ref msg) if msg.contains("exceeds maximum")));
}

// -----------------------------------------------------------------------
// 6. Invalid JSON → error but parsing continues
// -----------------------------------------------------------------------
#[test]
fn invalid_json_continues_parsing() {
    let mut parser = StreamParser::new();
    let good = fatal_line("ok");
    let input = format!("not valid json\n{good}");
    let results = parser.feed(input.as_bytes());
    assert_eq!(results.len(), 2);
    assert!(results[0].is_err()); // bad JSON
    assert!(results[1].is_ok()); // good line after
}

// -----------------------------------------------------------------------
// 7. Invalid UTF-8 → error
// -----------------------------------------------------------------------
#[test]
fn invalid_utf8_error() {
    let mut parser = StreamParser::new();
    // 0xFF 0xFE are not valid UTF-8 start bytes
    let mut data: Vec<u8> = vec![0xFF, 0xFE, b'{', b'}'];
    data.push(b'\n');
    let results = parser.feed(&data);
    assert_eq!(results.len(), 1);
    let err = results.into_iter().next().unwrap().unwrap_err();
    assert!(matches!(err, ProtocolError::Violation(ref msg) if msg.contains("UTF-8")));
}

// -----------------------------------------------------------------------
// 8. finish() flushes remaining data
// -----------------------------------------------------------------------
#[test]
fn finish_flushes_remaining() {
    let mut parser = StreamParser::new();
    let line = fatal_line("leftover");
    let trimmed = line.trim(); // no trailing newline
    parser.feed(trimmed.as_bytes());
    assert!(!parser.is_empty());

    let results = parser.finish();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
    assert!(parser.is_empty());
}

// -----------------------------------------------------------------------
// 9. finish() on empty parser returns nothing
// -----------------------------------------------------------------------
#[test]
fn finish_empty_parser() {
    let mut parser = StreamParser::new();
    let results = parser.finish();
    assert!(results.is_empty());
}

// -----------------------------------------------------------------------
// 10. Byte-at-a-time feeding
// -----------------------------------------------------------------------
#[test]
fn byte_at_a_time_feeding() {
    let mut parser = StreamParser::new();
    let line = fatal_line("byte-by-byte");
    let mut collected = Vec::new();
    for &b in line.as_bytes() {
        collected.extend(parser.feed(&[b]));
    }
    assert_eq!(collected.len(), 1);
    assert!(collected[0].is_ok());
}

// -----------------------------------------------------------------------
// 11. Large payload
// -----------------------------------------------------------------------
#[test]
fn large_payload() {
    let mut parser = StreamParser::new();
    let big_msg = "x".repeat(100_000);
    let line = fatal_line(&big_msg);
    let results = parser.feed(line.as_bytes());
    assert_eq!(results.len(), 1);
    let env = results.into_iter().next().unwrap().unwrap();
    match env {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 100_000),
        _ => panic!("expected Fatal"),
    }
}

// -----------------------------------------------------------------------
// 12. Unicode content preservation
// -----------------------------------------------------------------------
#[test]
fn unicode_content_preservation() {
    let mut parser = StreamParser::new();
    let msg = "こんにちは世界 🌍 émojis ñ";
    let line = fatal_line(msg);
    let results = parser.feed(line.as_bytes());
    assert_eq!(results.len(), 1);
    match results.into_iter().next().unwrap().unwrap() {
        Envelope::Fatal { error, .. } => assert_eq!(error, msg),
        _ => panic!("expected Fatal"),
    }
}

// -----------------------------------------------------------------------
// 13. feed is alias for push
// -----------------------------------------------------------------------
#[test]
fn feed_is_alias_for_push() {
    let line = fatal_line("alias");
    let bytes = line.as_bytes();

    let mut p1 = StreamParser::new();
    let r1 = p1.push(bytes);

    let mut p2 = StreamParser::new();
    let r2 = p2.feed(bytes);

    assert_eq!(r1.len(), r2.len());
    // Both should succeed
    assert!(r1[0].is_ok());
    assert!(r2[0].is_ok());
}

// -----------------------------------------------------------------------
// 14. Multiple partial feeds then finish
// -----------------------------------------------------------------------
#[test]
fn multiple_partial_feeds_then_finish() {
    let mut parser = StreamParser::new();
    let line = fatal_line("multi-part");
    let bytes = line.trim().as_bytes(); // no trailing newline
    let chunk_size = 5;

    for chunk in bytes.chunks(chunk_size) {
        let r = parser.feed(chunk);
        assert!(r.is_empty(), "should not yield until newline");
    }

    let results = parser.finish();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

// -----------------------------------------------------------------------
// 15. Whitespace-only lines are skipped
// -----------------------------------------------------------------------
#[test]
fn whitespace_only_lines_skipped() {
    let mut parser = StreamParser::new();
    let input = format!("   \n\t\n{}", fatal_line("ws"));
    let results = parser.feed(input.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

// -----------------------------------------------------------------------
// 16. CRLF line endings
// -----------------------------------------------------------------------
#[test]
fn crlf_line_endings() {
    let mut parser = StreamParser::new();
    let json = fatal_line("cr").trim().to_string();
    let input = format!("{json}\r\n");
    let results = parser.feed(input.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

// -----------------------------------------------------------------------
// 17. Reset clears buffer
// -----------------------------------------------------------------------
#[test]
fn reset_clears_buffer() {
    let mut parser = StreamParser::new();
    parser.feed(b"partial data without newline");
    assert!(!parser.is_empty());
    parser.reset();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

// -----------------------------------------------------------------------
// 18. buffered_len tracks correctly
// -----------------------------------------------------------------------
#[test]
fn buffered_len_tracks() {
    let mut parser = StreamParser::new();
    assert_eq!(parser.buffered_len(), 0);

    parser.feed(b"hello");
    assert_eq!(parser.buffered_len(), 5);

    // Feed a newline to drain
    parser.feed(b"\n");
    // "hello\n" is consumed (invalid JSON, but drained)
    assert_eq!(parser.buffered_len(), 0);
}

// -----------------------------------------------------------------------
// 19. Interleaved valid and invalid lines
// -----------------------------------------------------------------------
#[test]
fn interleaved_valid_invalid() {
    let mut parser = StreamParser::new();
    let good1 = fatal_line("first");
    let good2 = fatal_line("second");
    let input = format!("{good1}{{bad json}}\n{good2}");
    let results = parser.feed(input.as_bytes());
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

// -----------------------------------------------------------------------
// 20. Default trait implementation
// -----------------------------------------------------------------------
#[test]
fn default_impl() {
    let parser = StreamParser::default();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

// -----------------------------------------------------------------------
// 21. Max line length boundary — exactly at limit
// -----------------------------------------------------------------------
#[test]
fn max_line_length_boundary() {
    // A line of exactly max_line_len bytes should be accepted
    let limit = 100;
    let mut parser = StreamParser::with_max_line_len(limit);

    // Build a JSON line of exactly `limit` bytes (excluding newline)
    // We use padding in the error field to hit the target size
    let prefix = r#"{"t":"fatal","ref_id":null,"error":""#;
    let suffix = r#""}"#;
    let overhead = prefix.len() + suffix.len();
    let padding = "a".repeat(limit - overhead);
    let line = format!("{prefix}{padding}{suffix}\n");
    // Verify it's exactly at the limit (line without newline)
    assert_eq!(line.trim().len(), limit);

    let results = parser.feed(line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

// -----------------------------------------------------------------------
// 22. Max line length boundary — one over limit
// -----------------------------------------------------------------------
#[test]
fn max_line_length_one_over() {
    let limit = 100;
    let mut parser = StreamParser::with_max_line_len(limit);

    let prefix = r#"{"t":"fatal","ref_id":null,"error":""#;
    let suffix = r#""}"#;
    let overhead = prefix.len() + suffix.len();
    let padding = "a".repeat(limit - overhead + 1);
    let line = format!("{prefix}{padding}{suffix}\n");
    assert_eq!(line.trim().len(), limit + 1);

    let results = parser.feed(line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

// -----------------------------------------------------------------------
// 23. finish() after complete line (nothing pending) returns empty
// -----------------------------------------------------------------------
#[test]
fn finish_after_complete_line() {
    let mut parser = StreamParser::new();
    let line = fatal_line("done");
    parser.feed(line.as_bytes());
    let results = parser.finish();
    assert!(results.is_empty());
}

// -----------------------------------------------------------------------
// 24. Chunk splitting mid-multibyte UTF-8
// -----------------------------------------------------------------------
#[test]
fn split_mid_multibyte_utf8() {
    let mut parser = StreamParser::new();
    let line = fatal_line("café");
    let bytes = line.as_bytes();
    // 'é' is 2 bytes in UTF-8 — split inside it
    // Find the é in the serialized JSON
    let pos = bytes.windows(2).position(|w| w == "é".as_bytes()).unwrap();
    let (first, second) = bytes.split_at(pos + 1); // split inside the é

    let r1 = parser.feed(first);
    assert!(r1.is_empty());
    let r2 = parser.feed(second);
    assert_eq!(r2.len(), 1);
    assert!(r2[0].is_ok());
}

// -----------------------------------------------------------------------
// 25. Many envelopes streamed in small chunks
// -----------------------------------------------------------------------
#[test]
fn many_envelopes_small_chunks() {
    let mut parser = StreamParser::new();
    let mut input = String::new();
    for i in 0..50 {
        input.push_str(&fatal_line(&format!("msg-{i}")));
    }

    let mut total = Vec::new();
    for chunk in input.as_bytes().chunks(17) {
        total.extend(parser.feed(chunk));
    }
    total.extend(parser.finish());

    assert_eq!(total.len(), 50);
    for r in &total {
        assert!(r.is_ok());
    }
}
