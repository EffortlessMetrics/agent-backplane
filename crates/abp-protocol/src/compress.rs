// SPDX-License-Identifier: MIT OR Apache-2.0
//! Message compression support for the ABP protocol.
//!
//! Provides [`MessageCompressor`] for compressing and decompressing raw byte
//! payloads, [`CompressedMessage`] as a self-describing compressed envelope,
//! and [`CompressionStats`] for tracking cumulative compression metrics.
//!
//! # Stub implementations
//!
//! The `Gzip` and `Zstd` variants are currently **stubs** that prepend a
//! one-byte algorithm tag but do **not** perform real compression.  The
//! round-trip contract (compress then decompress yields the original bytes)
//! is always upheld.

use serde::{Deserialize, Serialize};

/// Identifies which compression algorithm to apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionAlgorithm {
    /// No compression — data is passed through unchanged.
    None,
    /// Gzip compression (stub: tags data but does not actually compress).
    // TODO: Replace stub with real gzip via the `flate2` crate.
    Gzip,
    /// Zstandard compression (stub: tags data but does not actually compress).
    // TODO: Replace stub with real zstd via the `zstd` crate.
    Zstd,
}

impl CompressionAlgorithm {
    /// Header byte written by the stub compressors so `decompress` can
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
}

/// Convenience alias used throughout this module.
pub type Result<T> = std::result::Result<T, CompressError>;

// ---------------------------------------------------------------------------
// MessageCompressor
// ---------------------------------------------------------------------------

/// Compresses and decompresses raw byte buffers using a chosen algorithm.
///
/// For `Gzip` and `Zstd` the current implementation is a **stub**: a single
/// header byte is prepended during compression and stripped during
/// decompression, but the payload is stored verbatim.
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
    /// For the stub `Gzip`/`Zstd` implementations a one-byte header is
    /// prepended.
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self.algorithm {
            CompressionAlgorithm::None => Ok(data.to_vec()),
            CompressionAlgorithm::Gzip | CompressionAlgorithm::Zstd => {
                // TODO: Replace with real compression (flate2 / zstd).
                let mut out = Vec::with_capacity(1 + data.len());
                out.push(self.algorithm.tag());
                out.extend_from_slice(data);
                Ok(out)
            }
        }
    }

    /// Decompress `data` previously produced by [`compress`](Self::compress).
    ///
    /// For [`CompressionAlgorithm::None`] the input is returned unchanged.
    /// For stub `Gzip`/`Zstd` the leading header byte is validated and
    /// stripped.
    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self.algorithm {
            CompressionAlgorithm::None => Ok(data.to_vec()),
            CompressionAlgorithm::Gzip | CompressionAlgorithm::Zstd => {
                // TODO: Replace with real decompression (flate2 / zstd).
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
                Ok(data[1..].to_vec())
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
    /// Returns `0` when compressed size exceeds original (as with stubs).
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
}
