// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz dialect detection with arbitrary JSON.
//!
//! Feeds arbitrary bytes/JSON to [`DialectDetector`] and [`DialectValidator`].
//! Verifies no panics on any input, confidence is in [0.0, 1.0], evidence is
//! non-empty on match, detect_all is sorted by confidence, and validation
//! always returns a valid result.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let detector = abp_dialect::DialectDetector::new();
    let validator = abp_dialect::DialectValidator::new();

    // --- Parse bytes as JSON ---
    let value: serde_json::Value = match serde_json::from_slice(data) {
        Ok(v) => v,
        Err(_) => {
            // Also try UTF-8 string path.
            if let Ok(s) = std::str::from_utf8(data) {
                match serde_json::from_str(s) {
                    Ok(v) => v,
                    Err(_) => return,
                }
            } else {
                return;
            }
        }
    };

    // --- DialectDetector.detect() ---
    if let Some(result) = detector.detect(&value) {
        // Confidence must be in [0.0, 1.0].
        assert!(
            result.confidence >= 0.0 && result.confidence <= 1.0,
            "confidence out of range: {}",
            result.confidence
        );
        // Evidence must be non-empty when there's a match.
        assert!(
            !result.evidence.is_empty(),
            "evidence must be non-empty on match"
        );
        // Dialect label must not be empty.
        assert!(!result.dialect.label().is_empty());
        // Display must not panic.
        let _ = format!("{}", result.dialect);
    }

    // --- DialectDetector.detect_all() ---
    let all_results = detector.detect_all(&value);
    // Results must be sorted by descending confidence.
    for window in all_results.windows(2) {
        assert!(
            window[0].confidence >= window[1].confidence,
            "detect_all must be sorted by confidence"
        );
    }
    for r in &all_results {
        assert!(r.confidence > 0.0, "detect_all entries must have score > 0");
        assert!(r.confidence <= 1.0);
    }

    // --- DialectValidator.validate() for every dialect ---
    for &dialect in abp_dialect::Dialect::all() {
        let result = validator.validate(&value, dialect);
        // valid == errors.is_empty() invariant.
        assert_eq!(
            result.valid,
            result.errors.is_empty(),
            "valid must match errors.is_empty()"
        );
        // Display on errors must not panic.
        for e in &result.errors {
            let _ = format!("{e}");
        }
    }

    // --- Dialect serde round-trip ---
    for &dialect in abp_dialect::Dialect::all() {
        if let Ok(json) = serde_json::to_string(&dialect) {
            let rt: Result<abp_dialect::Dialect, _> = serde_json::from_str(&json);
            assert!(rt.is_ok(), "Dialect serde round-trip must succeed");
            assert_eq!(rt.unwrap(), dialect);
        }
    }
});
