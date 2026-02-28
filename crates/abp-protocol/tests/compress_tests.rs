// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the `abp_protocol::compress` module.

use abp_protocol::compress::{
    CompressError, CompressedMessage, CompressionAlgorithm, CompressionStats, MessageCompressor,
};

// ---------------------------------------------------------------------------
// Round-trip tests
// ---------------------------------------------------------------------------

#[test]
fn none_round_trip() {
    let c = MessageCompressor::new(CompressionAlgorithm::None);
    let data = b"hello world";
    let compressed = c.compress(data).unwrap();
    assert_eq!(c.decompress(&compressed).unwrap(), data);
}

#[test]
fn gzip_round_trip() {
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let data = b"some gzip payload";
    let compressed = c.compress(data).unwrap();
    assert_eq!(c.decompress(&compressed).unwrap(), data);
}

#[test]
fn zstd_round_trip() {
    let c = MessageCompressor::new(CompressionAlgorithm::Zstd);
    let data = b"some zstd payload";
    let compressed = c.compress(data).unwrap();
    assert_eq!(c.decompress(&compressed).unwrap(), data);
}

#[test]
fn none_passthrough_unchanged() {
    let c = MessageCompressor::new(CompressionAlgorithm::None);
    let data = b"unchanged";
    // None must return the exact same bytes (no header).
    assert_eq!(c.compress(data).unwrap(), data.to_vec());
}

#[test]
fn gzip_stub_prepends_header() {
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let data = b"abc";
    let compressed = c.compress(data).unwrap();
    assert_eq!(compressed.len(), data.len() + 1);
    assert_eq!(compressed[0], 0x01); // gzip tag
    assert_eq!(&compressed[1..], data);
}

#[test]
fn zstd_stub_prepends_header() {
    let c = MessageCompressor::new(CompressionAlgorithm::Zstd);
    let data = b"xyz";
    let compressed = c.compress(data).unwrap();
    assert_eq!(compressed.len(), data.len() + 1);
    assert_eq!(compressed[0], 0x02); // zstd tag
    assert_eq!(&compressed[1..], data);
}

// ---------------------------------------------------------------------------
// Empty data
// ---------------------------------------------------------------------------

#[test]
fn none_empty_data() {
    let c = MessageCompressor::new(CompressionAlgorithm::None);
    let compressed = c.compress(b"").unwrap();
    assert!(compressed.is_empty());
    assert_eq!(c.decompress(&compressed).unwrap(), b"");
}

#[test]
fn gzip_empty_data_round_trip() {
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let compressed = c.compress(b"").unwrap();
    assert_eq!(compressed.len(), 1); // header only
    assert_eq!(c.decompress(&compressed).unwrap(), b"");
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn gzip_decompress_empty_is_error() {
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let err = c.decompress(b"").unwrap_err();
    assert!(matches!(err, CompressError::TooShort));
}

#[test]
fn zstd_decompress_empty_is_error() {
    let c = MessageCompressor::new(CompressionAlgorithm::Zstd);
    let err = c.decompress(b"").unwrap_err();
    assert!(matches!(err, CompressError::TooShort));
}

#[test]
fn algorithm_mismatch_error() {
    let gzip = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let zstd = MessageCompressor::new(CompressionAlgorithm::Zstd);
    let compressed = gzip.compress(b"data").unwrap();
    let err = zstd.decompress(&compressed).unwrap_err();
    assert!(matches!(
        err,
        CompressError::AlgorithmMismatch {
            expected: CompressionAlgorithm::Zstd,
            found: CompressionAlgorithm::Gzip,
        }
    ));
}

#[test]
fn unknown_algorithm_tag() {
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let err = c.decompress(&[0xFF, 0x00]).unwrap_err();
    assert!(matches!(err, CompressError::UnknownAlgorithm(0xFF)));
}

// ---------------------------------------------------------------------------
// CompressedMessage
// ---------------------------------------------------------------------------

#[test]
fn compressed_message_round_trip() {
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let data = b"message payload";
    let msg = c.compress_message(data).unwrap();
    assert_eq!(msg.algorithm, CompressionAlgorithm::Gzip);
    assert_eq!(msg.original_size, data.len());
    assert_eq!(msg.compressed_size, msg.data.len());
    assert_eq!(c.decompress_message(&msg).unwrap(), data);
}

#[test]
fn compressed_message_serde_round_trip() {
    let c = MessageCompressor::new(CompressionAlgorithm::Zstd);
    let msg = c.compress_message(b"serde test").unwrap();
    let json = serde_json::to_string(&msg).unwrap();
    let restored: CompressedMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.algorithm, msg.algorithm);
    assert_eq!(restored.original_size, msg.original_size);
    assert_eq!(restored.compressed_size, msg.compressed_size);
    assert_eq!(restored.data, msg.data);
}

// ---------------------------------------------------------------------------
// CompressionStats
// ---------------------------------------------------------------------------

#[test]
fn stats_initial_zero() {
    let stats = CompressionStats::new();
    assert_eq!(stats.total_original, 0);
    assert_eq!(stats.total_compressed, 0);
    assert!((stats.compression_ratio() - 0.0).abs() < f64::EPSILON);
    assert_eq!(stats.bytes_saved(), 0);
}

#[test]
fn stats_record_and_ratio() {
    let mut stats = CompressionStats::new();
    stats.record(100, 60);
    stats.record(200, 120);
    assert_eq!(stats.total_original, 300);
    assert_eq!(stats.total_compressed, 180);
    assert!((stats.compression_ratio() - 0.6).abs() < f64::EPSILON);
    assert_eq!(stats.bytes_saved(), 120);
}

#[test]
fn stats_bytes_saved_saturates() {
    let mut stats = CompressionStats::new();
    // Stub compressors increase size by 1 byte — saved should be 0.
    stats.record(10, 11);
    assert_eq!(stats.bytes_saved(), 0);
}

// ---------------------------------------------------------------------------
// Serde for CompressionAlgorithm
// ---------------------------------------------------------------------------

#[test]
fn algorithm_serde_round_trip() {
    for algo in [
        CompressionAlgorithm::None,
        CompressionAlgorithm::Gzip,
        CompressionAlgorithm::Zstd,
    ] {
        let json = serde_json::to_string(&algo).unwrap();
        let restored: CompressionAlgorithm = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, algo);
    }
}

#[test]
fn algorithm_accessor() {
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    assert_eq!(c.algorithm(), CompressionAlgorithm::Gzip);
}
