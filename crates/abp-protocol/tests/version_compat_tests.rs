// SPDX-License-Identifier: MIT OR Apache-2.0

//! Extended version parsing and compatibility tests.
//!
//! These complement `version_tests.rs` with edge cases, future-version
//! scenarios, and exhaustive invalid-input coverage.

use abp_core::CONTRACT_VERSION;
use abp_protocol::{is_compatible_version, parse_version};

// ── Parse valid versions ────────────────────────────────────────────────────

#[test]
fn parse_current_contract_version() {
    let parsed = parse_version(CONTRACT_VERSION);
    assert_eq!(parsed, Some((0, 1)), "CONTRACT_VERSION must parse as (0,1)");
}

#[test]
fn parse_future_minor_version() {
    assert_eq!(parse_version("abp/v0.2"), Some((0, 2)));
    assert_eq!(parse_version("abp/v0.99"), Some((0, 99)));
}

#[test]
fn parse_future_major_version() {
    assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(parse_version("abp/v2.5"), Some((2, 5)));
}

#[test]
fn parse_large_version_numbers() {
    assert_eq!(parse_version("abp/v100.200"), Some((100, 200)));
    assert_eq!(parse_version("abp/v999.999"), Some((999, 999)));
}

// ── Parse invalid versions ──────────────────────────────────────────────────

#[test]
fn parse_empty_string() {
    assert_eq!(parse_version(""), None);
}

#[test]
fn parse_missing_prefix() {
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version("0.1"), None);
    assert_eq!(parse_version("abp/0.1"), None, "missing 'v' after prefix");
}

#[test]
fn parse_wrong_prefix() {
    assert_eq!(parse_version("xyz/v0.1"), None);
    assert_eq!(parse_version("ABP/v0.1"), None, "prefix is case-sensitive");
}

#[test]
fn parse_non_numeric_components() {
    assert_eq!(parse_version("abp/vx.y"), None);
    assert_eq!(parse_version("abp/v1.y"), None);
    assert_eq!(parse_version("abp/vx.1"), None);
}

#[test]
fn parse_trailing_dot() {
    assert_eq!(parse_version("abp/v1."), None);
    assert_eq!(parse_version("abp/v.1"), None);
}

#[test]
fn parse_extra_segments_rejected() {
    // "abp/v1.2.3" splits as ("1", "2.3") and "2.3" is not a valid u32.
    assert_eq!(parse_version("abp/v1.2.3"), None);
    assert_eq!(parse_version("abp/v0.1.0"), None);
}

#[test]
fn parse_negative_numbers() {
    assert_eq!(parse_version("abp/v-1.0"), None);
    assert_eq!(parse_version("abp/v0.-1"), None);
}

#[test]
fn parse_numeric_overflow() {
    // u32::MAX is 4294967295 — one more should fail.
    assert_eq!(parse_version("abp/v4294967296.0"), None);
    assert_eq!(parse_version("abp/v0.4294967296"), None);
}

#[test]
fn parse_whitespace_not_tolerated() {
    assert_eq!(parse_version(" abp/v0.1"), None);
    assert_eq!(parse_version("abp/v0.1 "), None);
    assert_eq!(parse_version("abp/v 0.1"), None);
}

// ── Compatible version pairs ────────────────────────────────────────────────

#[test]
fn compatible_exact_match() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(is_compatible_version("abp/v1.0", "abp/v1.0"));
}

#[test]
fn compatible_same_major_different_minor() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.99", "abp/v0.1"));
    assert!(is_compatible_version("abp/v1.0", "abp/v1.42"));
}

#[test]
fn compatible_is_symmetric() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.9"));
    assert!(is_compatible_version("abp/v0.9", "abp/v0.1"));
}

#[test]
fn compatible_with_current_contract() {
    // Any v0.x sidecar is compatible with the current control plane.
    assert!(is_compatible_version("abp/v0.5", CONTRACT_VERSION));
    assert!(is_compatible_version(CONTRACT_VERSION, "abp/v0.99"));
}

// ── Incompatible version pairs ──────────────────────────────────────────────

#[test]
fn incompatible_different_major() {
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v1.0", "abp/v2.0"));
}

#[test]
fn incompatible_when_either_invalid() {
    assert!(!is_compatible_version("", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", ""));
    assert!(!is_compatible_version("garbage", "trash"));
    assert!(!is_compatible_version("abp/v0.1", "not-a-version"));
}

#[test]
fn incompatible_both_invalid() {
    assert!(!is_compatible_version("", ""));
    assert!(!is_compatible_version("xyz", "abc"));
}

// ── Future version compatibility ────────────────────────────────────────────

#[test]
fn future_minor_compatible_with_current() {
    // A future v0.x sidecar should be compatible with v0.1.
    assert!(is_compatible_version("abp/v0.50", CONTRACT_VERSION));
}

#[test]
fn future_major_incompatible_with_current() {
    // A v1.0 sidecar is not compatible with v0.1.
    assert!(!is_compatible_version("abp/v1.0", CONTRACT_VERSION));
    assert!(!is_compatible_version("abp/v2.0", CONTRACT_VERSION));
}

#[test]
fn future_major_versions_compatible_among_themselves() {
    assert!(is_compatible_version("abp/v3.0", "abp/v3.7"));
    assert!(!is_compatible_version("abp/v3.0", "abp/v4.0"));
}

// ── Boundary: major version 0 ───────────────────────────────────────────────

#[test]
fn major_zero_is_a_real_major_version() {
    // v0.x versions are mutually compatible (0 is a valid major version).
    assert!(is_compatible_version("abp/v0.0", "abp/v0.1"));
    assert!(is_compatible_version("abp/v0.0", "abp/v0.99"));
}

#[test]
fn major_zero_not_compatible_with_major_one() {
    assert!(!is_compatible_version("abp/v0.99", "abp/v1.0"));
}
