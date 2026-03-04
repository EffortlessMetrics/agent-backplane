// SPDX-License-Identifier: MIT OR Apache-2.0
//! SSE-compatible streaming adapter for OpenAI chat completions.
//!
//! Provides utilities for parsing Server-Sent Events (SSE) streams that
//! conform to the OpenAI streaming format, and for formatting stream
//! chunks back into SSE text.

use crate::chat::ChatCompletionChunk;
use crate::types::StreamChunk;

// Re-export the SseLineStream from client.rs for direct usage.
pub use crate::client::SseLineStream;

// ── SSE formatting ──────────────────────────────────────────────────────

/// Format a single [`ChatCompletionChunk`] as an SSE `data:` line.
///
/// Returns a string like `"data: {json}\n\n"` suitable for writing to
/// an SSE response stream.
pub fn format_sse_chunk(chunk: &ChatCompletionChunk) -> Result<String, serde_json::Error> {
    let json = serde_json::to_string(chunk)?;
    Ok(format!("data: {json}\n\n"))
}

/// Format the SSE `[DONE]` sentinel that terminates a stream.
#[must_use]
pub fn format_sse_done() -> String {
    "data: [DONE]\n\n".to_string()
}

/// Format an entire sequence of chunks as a complete SSE text block.
///
/// Includes the `[DONE]` sentinel at the end.
pub fn format_sse_stream(chunks: &[ChatCompletionChunk]) -> Result<String, serde_json::Error> {
    let mut out = String::new();
    for chunk in chunks {
        out.push_str(&format_sse_chunk(chunk)?);
    }
    out.push_str(&format_sse_done());
    Ok(out)
}

// ── SSE parsing (delegates to SseLineStream) ────────────────────────────

/// Parse a complete SSE text block into a vector of [`StreamChunk`]s.
///
/// This is a convenience wrapper around [`SseLineStream`] for cases where
/// the entire SSE payload is available as a string.
pub fn parse_sse_text(text: &str) -> Vec<Result<StreamChunk, String>> {
    crate::client::parse_sse_text(text)
}

/// Parse a single SSE `data:` line into a [`StreamChunk`].
///
/// Returns `None` for the `[DONE]` sentinel, comments, and non-data lines.
pub fn parse_sse_line(line: &str) -> Option<Result<StreamChunk, String>> {
    crate::client::parse_sse_line(line)
}

// ── Stream collector ────────────────────────────────────────────────────

/// Collect streaming chunks into a single assembled response text.
///
/// Concatenates all `delta.content` fields across all chunks.
#[must_use]
pub fn collect_text(chunks: &[ChatCompletionChunk]) -> String {
    let mut text = String::new();
    for chunk in chunks {
        for choice in &chunk.choices {
            if let Some(content) = &choice.delta.content {
                text.push_str(content);
            }
        }
    }
    text
}

/// Extract the finish reason from the last chunk.
#[must_use]
pub fn finish_reason(chunks: &[ChatCompletionChunk]) -> Option<String> {
    chunks
        .last()
        .and_then(|c| c.choices.first())
        .and_then(|ch| ch.finish_reason.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{StreamChoice, StreamDelta};

    fn make_chunk(id: &str, content: Option<&str>, finish: Option<&str>) -> ChatCompletionChunk {
        StreamChunk {
            id: id.into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: None,
                    content: content.map(|s| s.to_string()),
                    tool_calls: None,
                },
                finish_reason: finish.map(|s| s.to_string()),
            }],
        }
    }

    #[test]
    fn format_single_sse_chunk() {
        let chunk = make_chunk("c1", Some("Hi"), None);
        let sse = format_sse_chunk(&chunk).unwrap();
        assert!(sse.starts_with("data: "));
        assert!(sse.ends_with("\n\n"));
        assert!(sse.contains("\"content\":\"Hi\""));
    }

    #[test]
    fn format_sse_done_sentinel() {
        assert_eq!(format_sse_done(), "data: [DONE]\n\n");
    }

    #[test]
    fn format_complete_sse_stream() {
        let chunks = vec![
            make_chunk("c1", Some("Hel"), None),
            make_chunk("c1", Some("lo"), None),
            make_chunk("c1", None, Some("stop")),
        ];
        let sse = format_sse_stream(&chunks).unwrap();
        assert!(sse.contains("data: [DONE]"));
        // Should have 3 data lines + DONE
        let data_count = sse.matches("data: ").count();
        assert_eq!(data_count, 4); // 3 chunks + [DONE]
    }

    #[test]
    fn parse_sse_text_roundtrip() {
        let chunk = make_chunk("c1", Some("test"), None);
        let sse = format_sse_chunk(&chunk).unwrap();
        let full = format!("{sse}data: [DONE]\n\n");
        let results = parse_sse_text(&full);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
        assert_eq!(
            results[0].as_ref().unwrap().choices[0]
                .delta
                .content
                .as_deref(),
            Some("test")
        );
    }

    #[test]
    fn parse_sse_line_data() {
        let chunk = make_chunk("c1", Some("data"), None);
        let json = serde_json::to_string(&chunk).unwrap();
        let line = format!("data: {json}");
        let result = parse_sse_line(&line).unwrap().unwrap();
        assert_eq!(result.id, "c1");
    }

    #[test]
    fn parse_sse_line_done_returns_none() {
        assert!(parse_sse_line("data: [DONE]").is_none());
    }

    #[test]
    fn parse_sse_line_comment_returns_none() {
        assert!(parse_sse_line(": keepalive").is_none());
    }

    #[test]
    fn collect_text_from_chunks() {
        let chunks = vec![
            make_chunk("c1", Some("Hello"), None),
            make_chunk("c1", Some(" "), None),
            make_chunk("c1", Some("world"), None),
        ];
        assert_eq!(collect_text(&chunks), "Hello world");
    }

    #[test]
    fn collect_text_skips_none_content() {
        let chunks = vec![
            make_chunk("c1", Some("Hi"), None),
            make_chunk("c1", None, Some("stop")),
        ];
        assert_eq!(collect_text(&chunks), "Hi");
    }

    #[test]
    fn finish_reason_from_last_chunk() {
        let chunks = vec![
            make_chunk("c1", Some("Hi"), None),
            make_chunk("c1", None, Some("stop")),
        ];
        assert_eq!(finish_reason(&chunks), Some("stop".into()));
    }

    #[test]
    fn finish_reason_none_when_empty() {
        let chunks: Vec<ChatCompletionChunk> = vec![];
        assert_eq!(finish_reason(&chunks), None);
    }

    #[test]
    fn sse_line_stream_incremental_parse() {
        let chunk = make_chunk("c1", Some("Hi"), None);
        let json = serde_json::to_string(&chunk).unwrap();

        let mut parser = SseLineStream::new();
        parser.feed_str(&format!("data: {json}\n\n"));
        let results: Vec<_> = parser.drain().collect();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }
}
