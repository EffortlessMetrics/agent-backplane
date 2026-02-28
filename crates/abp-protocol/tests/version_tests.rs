// SPDX-License-Identifier: MIT OR Apache-2.0

use abp_protocol::{is_compatible_version, parse_version};

// --- parse_version -----------------------------------------------------------

#[test]
fn parse_valid_version() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(parse_version("abp/v12.34"), Some((12, 34)));
}

#[test]
fn parse_invalid_empty() {
    assert_eq!(parse_version(""), None);
}

#[test]
fn parse_invalid_no_prefix() {
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version("0.1"), None);
}

#[test]
fn parse_invalid_no_numbers() {
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version("abp/vx.y"), None);
    assert_eq!(parse_version("abp/v1."), None);
    assert_eq!(parse_version("abp/v.1"), None);
}

#[test]
fn parse_invalid_extra_dots() {
    // "abp/v1.2.3" — split_once('.') gives ("1", "2.3") and "2.3" fails u32 parse
    assert_eq!(parse_version("abp/v1.2.3"), None);
}

// --- is_compatible_version ---------------------------------------------------

#[test]
fn compatible_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(is_compatible_version("abp/v1.0", "abp/v1.99"));
}

#[test]
fn incompatible_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "abp/v2.0"));
}

#[test]
fn incompatible_invalid_input() {
    assert!(!is_compatible_version("", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", ""));
    assert!(!is_compatible_version("garbage", "trash"));
}
