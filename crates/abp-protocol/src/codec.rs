// SPDX-License-Identifier: MIT OR Apache-2.0
//! Streaming JSONL batch encoder/decoder and validation utilities.

use crate::{Envelope, JsonlCodec, ProtocolError};

/// Streaming JSONL encoder/decoder for batch operations.
#[derive(Debug, Clone, Copy)]
pub struct StreamingCodec;

impl StreamingCodec {
    /// Encode multiple envelopes into a single JSONL string.
    ///
    /// Each envelope is serialized as one newline-terminated JSON line.
    ///
    /// # Examples
    ///
    /// ```
    /// # use abp_protocol::{Envelope, codec::StreamingCodec};
    /// let envelopes = vec![
    ///     Envelope::Fatal { ref_id: None, error: "err1".into() },
    ///     Envelope::Fatal { ref_id: None, error: "err2".into() },
    /// ];
    /// let batch = StreamingCodec::encode_batch(&envelopes);
    /// assert_eq!(batch.lines().count(), 2);
    /// assert!(batch.contains("err1"));
    /// assert!(batch.contains("err2"));
    /// ```
    #[must_use]
    pub fn encode_batch(envelopes: &[Envelope]) -> String {
        let mut out = String::new();
        for env in envelopes {
            // JsonlCodec::encode already appends '\n'
            if let Ok(line) = JsonlCodec::encode(env) {
                out.push_str(&line);
            }
        }
        out
    }

    /// Decode a JSONL string into a vec of results, one per non-blank line.
    ///
    /// Blank lines are skipped. Each non-blank line produces either a
    /// successfully parsed [`Envelope`] or a [`ProtocolError`].
    pub fn decode_batch(input: &str) -> Vec<Result<Envelope, ProtocolError>> {
        input
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| JsonlCodec::decode(l.trim()))
            .collect()
    }

    /// Count the number of non-blank lines in a JSONL string.
    #[must_use]
    pub fn line_count(input: &str) -> usize {
        input.lines().filter(|l| !l.trim().is_empty()).count()
    }

    /// Validate each non-blank line in a JSONL string.
    ///
    /// Returns a list of `(line_number, error)` pairs for lines that fail to
    /// parse, where `line_number` is 1-based.
    pub fn validate_jsonl(input: &str) -> Vec<(usize, ProtocolError)> {
        input
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty())
            .filter_map(|(idx, l)| JsonlCodec::decode(l.trim()).err().map(|e| (idx + 1, e)))
            .collect()
    }
}
