// SPDX-License-Identifier: MIT OR Apache-2.0
//! High-level streaming emulation for non-streaming backends.
//!
//! [`StreamingEmulator`] converts a complete response into a sequence of
//! [`StreamChunk`]s using configurable splitting strategies.

use crate::strategies::{StreamChunk, StreamingEmulation};
use serde::{Deserialize, Serialize};

/// Strategy for splitting a response into stream-like chunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SplitStrategy {
    /// Split by approximate character count, preferring word boundaries.
    Words {
        /// Target chunk size in characters.
        chunk_size: usize,
    },
    /// Split by exact character count (no word-boundary preference).
    FixedChars {
        /// Exact number of characters per chunk.
        chunk_size: usize,
    },
    /// Split into one chunk per line.
    Lines,
    /// Split on sentence boundaries (`.`, `!`, `?` followed by whitespace).
    Sentences,
}

/// A stream event wrapping a chunk with metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamEvent {
    /// The chunk content.
    pub chunk: StreamChunk,
    /// Event type label.
    pub event_type: StreamEventType,
}

/// Type of stream event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamEventType {
    /// Content delta — partial response text.
    ContentDelta,
    /// Final event — last chunk.
    ContentStop,
}

/// High-level streaming emulator with configurable splitting.
#[derive(Debug, Clone)]
pub struct StreamingEmulator {
    strategy: SplitStrategy,
}

impl StreamingEmulator {
    /// Create with the given split strategy.
    #[must_use]
    pub fn new(strategy: SplitStrategy) -> Self {
        Self { strategy }
    }

    /// Word-boundary splitting with the given chunk size.
    #[must_use]
    pub fn words(chunk_size: usize) -> Self {
        Self::new(SplitStrategy::Words { chunk_size })
    }

    /// Fixed character splitting.
    #[must_use]
    pub fn fixed_chars(chunk_size: usize) -> Self {
        Self::new(SplitStrategy::FixedChars { chunk_size })
    }

    /// Line-based splitting.
    #[must_use]
    pub fn lines() -> Self {
        Self::new(SplitStrategy::Lines)
    }

    /// Sentence-based splitting.
    #[must_use]
    pub fn sentences() -> Self {
        Self::new(SplitStrategy::Sentences)
    }

    /// The configured split strategy.
    #[must_use]
    pub fn strategy(&self) -> &SplitStrategy {
        &self.strategy
    }

    /// Split a complete response into chunks.
    #[must_use]
    pub fn split(&self, text: &str) -> Vec<StreamChunk> {
        match &self.strategy {
            SplitStrategy::Words { chunk_size } => {
                StreamingEmulation::new(*chunk_size).split_into_chunks(text)
            }
            SplitStrategy::FixedChars { chunk_size } => {
                StreamingEmulation::new(*chunk_size).split_fixed(text)
            }
            SplitStrategy::Lines => split_by_lines(text),
            SplitStrategy::Sentences => split_by_sentences(text),
        }
    }

    /// Split and convert to stream events.
    #[must_use]
    pub fn to_events(&self, text: &str) -> Vec<StreamEvent> {
        self.split(text)
            .into_iter()
            .map(|chunk| {
                let event_type = if chunk.is_final {
                    StreamEventType::ContentStop
                } else {
                    StreamEventType::ContentDelta
                };
                StreamEvent { chunk, event_type }
            })
            .collect()
    }

    /// Reassemble chunks back into the original text.
    #[must_use]
    pub fn reassemble(chunks: &[StreamChunk]) -> String {
        StreamingEmulation::reassemble(chunks)
    }

    /// Compute total character count across chunks.
    #[must_use]
    pub fn total_chars(chunks: &[StreamChunk]) -> usize {
        chunks.iter().map(|c| c.content.len()).sum()
    }
}

/// Split text into chunks by line boundaries.
fn split_by_lines(text: &str) -> Vec<StreamChunk> {
    if text.is_empty() {
        return vec![StreamChunk {
            content: String::new(),
            index: 0,
            is_final: true,
        }];
    }

    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let total = lines.len();
    lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| StreamChunk {
            content: line.to_string(),
            index: i,
            is_final: i == total - 1,
        })
        .collect()
}

/// Split text into chunks by sentence boundaries.
fn split_by_sentences(text: &str) -> Vec<StreamChunk> {
    if text.is_empty() {
        return vec![StreamChunk {
            content: String::new(),
            index: 0,
            is_final: true,
        }];
    }

    let mut sentences = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    for (i, &ch) in chars.iter().enumerate() {
        current.push(ch);
        let is_end_punct = ch == '.' || ch == '!' || ch == '?';
        let followed_by_space = i + 1 < len && chars[i + 1].is_whitespace();
        let is_last = i == len - 1;

        if is_end_punct && (followed_by_space || is_last) && !current.trim().is_empty() {
            sentences.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        sentences.push(current);
    }

    let total = sentences.len();
    sentences
        .into_iter()
        .enumerate()
        .map(|(i, s)| StreamChunk {
            content: s,
            index: i,
            is_final: i == total - 1,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_split_reassembles() {
        let emu = StreamingEmulator::words(10);
        let text = "Hello world, this is a test of streaming emulation.";
        let chunks = emu.split(text);
        let reassembled = StreamingEmulator::reassemble(&chunks);
        assert_eq!(reassembled, text);
    }

    #[test]
    fn fixed_split_reassembles() {
        let emu = StreamingEmulator::fixed_chars(5);
        let text = "abcdefghijklmnop";
        let chunks = emu.split(text);
        assert_eq!(StreamingEmulator::reassemble(&chunks), text);
    }

    #[test]
    fn line_split() {
        let emu = StreamingEmulator::lines();
        let text = "line1\nline2\nline3";
        let chunks = emu.split(text);
        assert_eq!(chunks.len(), 3);
        assert!(chunks[2].is_final);
    }

    #[test]
    fn sentence_split() {
        let emu = StreamingEmulator::sentences();
        let text = "First sentence. Second sentence! Third?";
        let chunks = emu.split(text);
        assert_eq!(chunks.len(), 3);
        assert!(chunks[0].content.contains("First"));
        assert!(chunks[2].is_final);
    }

    #[test]
    fn empty_text_produces_single_final_chunk() {
        for emu in [
            StreamingEmulator::words(10),
            StreamingEmulator::fixed_chars(5),
            StreamingEmulator::lines(),
            StreamingEmulator::sentences(),
        ] {
            let chunks = emu.split("");
            assert_eq!(chunks.len(), 1);
            assert!(chunks[0].is_final);
            assert!(chunks[0].content.is_empty());
        }
    }

    #[test]
    fn to_events_last_is_content_stop() {
        let emu = StreamingEmulator::words(5);
        let events = emu.to_events("Hello world");
        assert!(!events.is_empty());
        assert_eq!(
            events.last().unwrap().event_type,
            StreamEventType::ContentStop
        );
        // All non-final events are ContentDelta
        for e in &events[..events.len() - 1] {
            assert_eq!(e.event_type, StreamEventType::ContentDelta);
        }
    }

    #[test]
    fn total_chars_counts_correctly() {
        let emu = StreamingEmulator::fixed_chars(3);
        let chunks = emu.split("abcdef");
        assert_eq!(StreamingEmulator::total_chars(&chunks), 6);
    }

    #[test]
    fn sentence_split_no_punct() {
        let emu = StreamingEmulator::sentences();
        let text = "no punctuation here";
        let chunks = emu.split(text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, text);
    }

    #[test]
    fn line_split_trailing_newline() {
        let emu = StreamingEmulator::lines();
        let text = "line1\nline2\n";
        let chunks = emu.split(text);
        let reassembled = StreamingEmulator::reassemble(&chunks);
        assert_eq!(reassembled, text);
    }

    #[test]
    fn serde_roundtrip_split_strategy() {
        let strategies = vec![
            SplitStrategy::Words { chunk_size: 10 },
            SplitStrategy::FixedChars { chunk_size: 5 },
            SplitStrategy::Lines,
            SplitStrategy::Sentences,
        ];
        for s in &strategies {
            let json = serde_json::to_string(s).unwrap();
            let decoded: SplitStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(*s, decoded);
        }
    }

    #[test]
    fn serde_roundtrip_stream_event() {
        let event = StreamEvent {
            chunk: StreamChunk {
                content: "hello".into(),
                index: 0,
                is_final: true,
            },
            event_type: StreamEventType::ContentStop,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, decoded);
    }
}
