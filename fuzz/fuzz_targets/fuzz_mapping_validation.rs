// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz mapping validation with random dialect pairs and JSON payloads.
//!
//! Generates arbitrary JSON, detects the dialect, then validates against
//! every known dialect pair. Verifies no panics regardless of input and
//! checks cross-dialect detection + validation consistency.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use abp_dialect::{Dialect, DialectDetector, DialectValidator};

#[derive(Debug, Arbitrary)]
struct MappingFuzzInput {
    /// Raw JSON string to test.
    json_str: String,
    /// Source dialect index.
    source_idx: u8,
    /// Target dialect index.
    target_idx: u8,
    /// Additional feature flag strings to exercise.
    features: Vec<String>,
}

fuzz_target!(|input: MappingFuzzInput| {
    let all_dialects = Dialect::all();
    let detector = DialectDetector::new();
    let validator = DialectValidator::new();

    let source = all_dialects[input.source_idx as usize % all_dialects.len()];
    let target = all_dialects[input.target_idx as usize % all_dialects.len()];

    // --- Parse as JSON ---
    let value: serde_json::Value = match serde_json::from_str(&input.json_str) {
        Ok(v) => v,
        Err(_) => return,
    };

    // --- Validate against source dialect (never panics) ---
    let source_result = validator.validate(&value, source);
    assert_eq!(
        source_result.valid,
        source_result.errors.is_empty(),
        "valid flag must match errors"
    );

    // --- Validate against target dialect (never panics) ---
    let target_result = validator.validate(&value, target);
    assert_eq!(
        target_result.valid,
        target_result.errors.is_empty(),
        "valid flag must match errors"
    );

    // --- Detect the actual dialect ---
    let detected = detector.detect(&value);
    let all_detected = detector.detect_all(&value);

    // If detection succeeded, validate against the detected dialect too.
    if let Some(ref det) = detected {
        assert!(det.confidence > 0.0 && det.confidence <= 1.0);
        let det_result = validator.validate(&value, det.dialect);
        let _ = det_result.valid;
    }

    // detect_all must be sorted by confidence.
    for w in all_detected.windows(2) {
        assert!(w[0].confidence >= w[1].confidence);
    }

    // --- Cross-validate: try all dialect pairs ---
    for &d in all_dialects {
        let r = validator.validate(&value, d);
        assert_eq!(r.valid, r.errors.is_empty());
        // Display errors must not panic.
        for e in &r.errors {
            let _ = format!("{e}");
        }
    }

    // --- Exercise Dialect enum methods with feature strings ---
    for feat in &input.features {
        // Try to deserialize arbitrary strings as Dialect.
        let _ = serde_json::from_str::<Dialect>(&format!("\"{feat}\""));
    }

    // --- Dialect label/display must not panic ---
    assert!(!source.label().is_empty());
    assert!(!target.label().is_empty());
    let _ = format!("{source}");
    let _ = format!("{target}");
});
