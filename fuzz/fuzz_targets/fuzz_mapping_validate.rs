// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz mapping validation with random dialect pairs from raw bytes.
//!
//! Parses raw bytes as JSON, then validates against randomly selected
//! dialect pairs. Verifies no panics and checks consistency invariants.
#![no_main]
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }

    let all_dialects = Dialect::all();
    let source = all_dialects[data[0] as usize % all_dialects.len()];
    let target = all_dialects[data[1] as usize % all_dialects.len()];

    let json_bytes = &data[2..];
    let s = match std::str::from_utf8(json_bytes) {
        Ok(s) => s,
        Err(_) => return,
    };

    let value: serde_json::Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => return,
    };

    let detector = DialectDetector::new();
    let validator = DialectValidator::new();

    // --- Validate source dialect ---
    let src_result = validator.validate(&value, source);
    assert_eq!(src_result.valid, src_result.errors.is_empty());

    // --- Validate target dialect ---
    let tgt_result = validator.validate(&value, target);
    assert_eq!(tgt_result.valid, tgt_result.errors.is_empty());

    // --- Detection ---
    let detected = detector.detect(&value);
    if let Some(ref det) = detected {
        assert!(det.confidence > 0.0 && det.confidence <= 1.0);
    }

    // detect_all sorted by confidence.
    let all_detected = detector.detect_all(&value);
    for w in all_detected.windows(2) {
        assert!(w[0].confidence >= w[1].confidence);
    }

    // --- Dialect Display never panics ---
    let _ = format!("{source}");
    let _ = format!("{target}");
    assert!(!source.label().is_empty());
    assert!(!target.label().is_empty());
});
