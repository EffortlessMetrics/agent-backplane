// SPDX-License-Identifier: MIT OR Apache-2.0
//! Message compression support for the ABP protocol.
//!
//! Provides [`MessageCompressor`] for compressing and decompressing raw byte
//! payloads, [`CompressedMessage`] as a self-describing compressed envelope,
//! and [`CompressionStats`] for tracking cumulative compression metrics.

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};

/// Identifies which compression algorithm to apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionAlgorithm {
    /// No compression — data is passed through unchanged.
    None,
    /// Gzip compression via the `flate2` crate.
    Gzip,
    /// Zstandard compression via the `zstd` crate.
    Zstd,
}

impl CompressionAlgorithm {
    /// Header byte prepended to compressed output so `decompress` can
    /// identify the algorithm that was used.
    fn tag(self) -> u8 {
        match self {
            Self::None => 0x00,
            Self::Gzip => 0x01,
            Self::Zstd => 0x02,
        }
    }

    /// Resolve a header tag back to an algorithm.
    fn from_tag(tag: u8) -> Result<Self> {
        match tag {
            0x00 => Ok(Self::None),
            0x01 => Ok(Self::Gzip),
            0x02 => Ok(Self::Zstd),
            other => Err(CompressError::UnknownAlgorithm(other)),
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during compression or decompression.
#[derive(Debug, thiserror::Error)]
pub enum CompressError {
    /// The compressed payload is empty or too short to contain a valid header.
    #[error("compressed data is too short")]
    TooShort,
    /// The header byte does not correspond to any known algorithm.
    #[error("unknown compression algorithm tag: 0x{0:02x}")]
    UnknownAlgorithm(u8),
    /// The header algorithm does not match the expected algorithm.
    #[error("algorithm mismatch: expected {expected:?}, found {found:?}")]
    AlgorithmMismatch {
        /// The algorithm the caller expected.
        expected: CompressionAlgorithm,
        /// The algorithm indicated by the header byte.
        found: CompressionAlgorithm,
    },
    /// An I/O error occurred during compression or decompression.
    #[error("compression I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience alias used throughout this module.
pub type Result<T> = std::result::Result<T, CompressError>;

// ---------------------------------------------------------------------------
// MessageCompressor
// ---------------------------------------------------------------------------

/// Compresses and decompresses raw byte buffers using a chosen algorithm.
#[derive(Clone, Copy, Debug)]
pub struct MessageCompressor {
    algorithm: CompressionAlgorithm,
}

impl MessageCompressor {
    /// Create a new compressor for the given algorithm.
    #[must_use]
    pub fn new(algorithm: CompressionAlgorithm) -> Self {
        Self { algorithm }
    }

    /// Return the algorithm this compressor uses.
    #[must_use]
    pub fn algorithm(&self) -> CompressionAlgorithm {
        self.algorithm
    }

    /// Compress `data`, returning the compressed byte vector.
    ///
    /// For [`CompressionAlgorithm::None`] the input is returned unchanged.
    /// For `Gzip` and `Zstd` a one-byte algorithm tag is prepended, followed
    /// by the real compressed payload.
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self.algorithm {
            CompressionAlgorithm::None => Ok(data.to_vec()),
            CompressionAlgorithm::Gzip => {
                let mut out = Vec::with_capacity(1 + data.len());
                out.push(self.algorithm.tag());
                let mut encoder = GzEncoder::new(&mut out, Compression::default());
                encoder.write_all(data)?;
                encoder.finish()?;
                Ok(out)
            }
            CompressionAlgorithm::Zstd => {
                let mut out = vec![self.algorithm.tag()];
                let compressed = zstd::encode_all(data, 3)?;
                out.extend_from_slice(&compressed);
                Ok(out)
            }
        }
    }

    /// Decompress `data` previously produced by [`compress`](Self::compress).
    ///
    /// For [`CompressionAlgorithm::None`] the input is returned unchanged.
    /// For `Gzip` and `Zstd` the leading header byte is validated and
    /// stripped, then the remaining bytes are decompressed.
    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self.algorithm {
            CompressionAlgorithm::None => Ok(data.to_vec()),
            CompressionAlgorithm::Gzip | CompressionAlgorithm::Zstd => {
                if data.is_empty() {
                    return Err(CompressError::TooShort);
                }
                let found = CompressionAlgorithm::from_tag(data[0])?;
                if found != self.algorithm {
                    return Err(CompressError::AlgorithmMismatch {
                        expected: self.algorithm,
                        found,
                    });
                }
                let payload = &data[1..];
                match self.algorithm {
                    CompressionAlgorithm::Gzip => {
                        let mut decoder = GzDecoder::new(payload);
                        let mut out = Vec::new();
                        decoder.read_to_end(&mut out)?;
                        Ok(out)
                    }
                    CompressionAlgorithm::Zstd => {
                        let out = zstd::decode_all(payload)?;
                        Ok(out)
                    }
                    CompressionAlgorithm::None => unreachable!(),
                }
            }
        }
    }

    /// Compress `data` and wrap the result in a [`CompressedMessage`].
    pub fn compress_message(&self, data: &[u8]) -> Result<CompressedMessage> {
        let compressed = self.compress(data)?;
        Ok(CompressedMessage {
            algorithm: self.algorithm,
            original_size: data.len(),
            compressed_size: compressed.len(),
            data: compressed,
        })
    }

    /// Decompress a [`CompressedMessage`] back into raw bytes.
    pub fn decompress_message(&self, msg: &CompressedMessage) -> Result<Vec<u8>> {
        self.decompress(&msg.data)
    }
}

// ---------------------------------------------------------------------------
// CompressedMessage
// ---------------------------------------------------------------------------

/// A self-describing compressed payload that records the algorithm used,
/// original size, and compressed size alongside the data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompressedMessage {
    /// Algorithm that was used to produce `data`.
    pub algorithm: CompressionAlgorithm,
    /// Size in bytes of the original uncompressed payload.
    pub original_size: usize,
    /// Size in bytes of the `data` field.
    pub compressed_size: usize,
    /// The (possibly compressed) payload bytes.
    pub data: Vec<u8>,
}

// ---------------------------------------------------------------------------
// CompressionStats
// ---------------------------------------------------------------------------

/// Cumulative statistics for compression operations.
#[derive(Clone, Debug, Default)]
pub struct CompressionStats {
    /// Total bytes after compression.
    pub total_compressed: u64,
    /// Total bytes before compression.
    pub total_original: u64,
}

impl CompressionStats {
    /// Create a new, zeroed stats tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a single compression operation.
    pub fn record(&mut self, original: usize, compressed: usize) {
        self.total_original += original as u64;
        self.total_compressed += compressed as u64;
    }

    /// Overall compression ratio (`compressed / original`).
    ///
    /// Returns `0.0` when no data has been recorded.
    #[must_use]
    pub fn compression_ratio(&self) -> f64 {
        if self.total_original == 0 {
            return 0.0;
        }
        self.total_compressed as f64 / self.total_original as f64
    }

    /// Total bytes saved by compression (`original - compressed`).
    ///
    /// Returns `0` when compressed size exceeds original.
    #[must_use]
    pub fn bytes_saved(&self) -> u64 {
        self.total_original.saturating_sub(self.total_compressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_round_trip() {
        let c = MessageCompressor::new(CompressionAlgorithm::None);
        let data = b"hello world";
        assert_eq!(c.decompress(&c.compress(data).unwrap()).unwrap(), data);
    }

    #[test]
    fn gzip_round_trip() {
        let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
        let data = b"hello gzip";
        assert_eq!(c.decompress(&c.compress(data).unwrap()).unwrap(), data);
    }

    #[test]
    fn zstd_round_trip() {
        let c = MessageCompressor::new(CompressionAlgorithm::Zstd);
        let data = b"hello zstd";
        assert_eq!(c.decompress(&c.compress(data).unwrap()).unwrap(), data);
    }

    #[test]
    fn gzip_empty_input() {
        let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
        let data = b"";
        assert_eq!(
            c.decompress(&c.compress(data).unwrap()).unwrap(),
            data.as_slice()
        );
    }

    #[test]
    fn zstd_empty_input() {
        let c = MessageCompressor::new(CompressionAlgorithm::Zstd);
        let data = b"";
        assert_eq!(
            c.decompress(&c.compress(data).unwrap()).unwrap(),
            data.as_slice()
        );
    }

    #[test]
    fn gzip_actually_compresses_repetitive_data() {
        let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
        let data = "abcdef".repeat(1000);
        let compressed = c.compress(data.as_bytes()).unwrap();
        assert!(
            compressed.len() < data.len(),
            "gzip should compress repetitive data"
        );
        assert_eq!(c.decompress(&compressed).unwrap(), data.as_bytes());
    }

    #[test]
    fn zstd_actually_compresses_repetitive_data() {
        let c = MessageCompressor::new(CompressionAlgorithm::Zstd);
        let data = "abcdef".repeat(1000);
        let compressed = c.compress(data.as_bytes()).unwrap();
        assert!(
            compressed.len() < data.len(),
            "zstd should compress repetitive data"
        );
        assert_eq!(c.decompress(&compressed).unwrap(), data.as_bytes());
    }

    #[test]
    fn decompress_too_short() {
        let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
        assert!(matches!(c.decompress(b""), Err(CompressError::TooShort)));
    }

    #[test]
    fn decompress_algorithm_mismatch() {
        let gzip = MessageCompressor::new(CompressionAlgorithm::Gzip);
        let zstd = MessageCompressor::new(CompressionAlgorithm::Zstd);
        let compressed = gzip.compress(b"test data").unwrap();
        let err = zstd.decompress(&compressed).unwrap_err();
        assert!(matches!(err, CompressError::AlgorithmMismatch { .. }));
    }

    #[test]
    fn compressed_message_round_trip() {
        let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
        let data = b"hello compressed message";
        let msg = c.compress_message(data).unwrap();
        assert_eq!(msg.algorithm, CompressionAlgorithm::Gzip);
        assert_eq!(msg.original_size, data.len());
        assert_eq!(msg.compressed_size, msg.data.len());
        assert_eq!(c.decompress_message(&msg).unwrap(), data);
    }

    #[test]
    fn compression_stats_tracking() {
        let mut stats = CompressionStats::new();
        stats.record(1000, 200);
        stats.record(2000, 400);
        assert_eq!(stats.total_original, 3000);
        assert_eq!(stats.total_compressed, 600);
        assert_eq!(stats.bytes_saved(), 2400);
        assert!((stats.compression_ratio() - 0.2).abs() < f64::EPSILON);
    }
}
